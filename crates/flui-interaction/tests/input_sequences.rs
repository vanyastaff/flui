//! Input sequence identity through the public binding (ADR-0207 §4): the
//! binding mints one id per contact, latch and contactless source, and never
//! reissues one.

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::geometry::Offset;
use flui_interaction::events::{
    PointerEvent, PointerKind, make_down_event_for_id, make_move_event_for_id, make_scroll_event,
    make_up_event_for_id,
};
use flui_interaction::haptics::HapticsHost;
use flui_interaction::{
    ContactlessSource, EventPropagation, GestureBinding, HitTestEntry, HitTestResult, IdExhausted,
    InputSequenceId, InputSurface, InteractionLane, PointerId, RenderId, ScrollLatchEnd,
};
use flui_platform_api::haptics::{FakeHaptics, HapticRequest, HapticTiming};
use flui_platform_api::pointer::{DeviceId, ScrollPhase};
use flui_platform_api::{HapticFeedback, PlatformHaptics};
use static_assertions::assert_impl_all;

assert_impl_all!(InputSequenceId: Copy, Send, Sync, Ord, std::hash::Hash);

fn pointer(id: u64) -> PointerId {
    PointerId::try_from(id).expect("nonzero pointer id")
}

fn down(id: u64) -> PointerEvent {
    make_down_event_for_id(pointer(id), Offset::ZERO, PointerKind::Touch).expect("down")
}

fn moved(id: u64) -> PointerEvent {
    make_move_event_for_id(pointer(id), Offset::new(3.0, 0.0), PointerKind::Touch).expect("move")
}

fn up(id: u64) -> PointerEvent {
    make_up_event_for_id(pointer(id), Offset::new(3.0, 0.0), PointerKind::Touch).expect("up")
}

fn wheel(phase: ScrollPhase) -> PointerEvent {
    let PointerEvent::Scroll(mut scroll) =
        make_scroll_event(Offset::ZERO, Offset::new(0.0, 10.0)).expect("finite wheel")
    else {
        unreachable!("a scroll helper builds a scroll")
    };
    scroll.phase = Some(phase);
    PointerEvent::Scroll(scroll)
}

fn send(binding: &GestureBinding, event: &PointerEvent) -> Option<InputSequenceId> {
    binding
        .dispatch_pointer_on(InputSurface::Window, event, |_| HitTestResult::new())
        .sequence()
}

#[test]
fn a_contact_carries_one_sequence_from_down_to_up() {
    let binding = GestureBinding::new();
    let first = send(&binding, &down(1)).expect("a Down starts a sequence");
    assert_eq!(send(&binding, &moved(1)), Some(first));
    assert_eq!(send(&binding, &up(1)), Some(first));

    // The platform reuses the pointer id; the binding does not reuse the sequence.
    let second = send(&binding, &down(1)).expect("a reused pointer starts a new sequence");
    assert_ne!(first, second);
    let other = send(&binding, &down(2)).expect("a second finger starts its own");
    assert_ne!(other, second);
    assert_eq!(send(&binding, &up(1)), Some(second));
    assert_eq!(send(&binding, &up(2)), Some(other));

    // A hover belongs to no sequence.
    let hover =
        make_move_event_for_id(pointer(9), Offset::ZERO, PointerKind::Mouse).expect("hover");
    assert_eq!(send(&binding, &hover), None);
}

#[test]
fn a_wheel_latch_carries_one_sequence_until_it_ends() {
    let binding = GestureBinding::new();
    let latch = send(&binding, &wheel(ScrollPhase::Began)).expect("a wheel burst is a sequence");
    assert_eq!(send(&binding, &wheel(ScrollPhase::Changed)), Some(latch));
    assert_eq!(send(&binding, &wheel(ScrollPhase::Ended)), Some(latch));
    let next = send(&binding, &wheel(ScrollPhase::Began)).expect("the next burst");
    assert_ne!(next, latch);
}

#[test]
fn contactless_sequences_get_binding_minted_devices() {
    let binding = GestureBinding::new();
    let contact = send(&binding, &down(1)).expect("contact");
    let mut seen = HashSet::from([contact]);
    for (source, device) in [
        (ContactlessSource::Assistive, DeviceId::ASSISTIVE),
        (ContactlessSource::Programmatic, DeviceId::PROGRAMMATIC),
        (ContactlessSource::AutoScroll, DeviceId::PROGRAMMATIC),
        (ContactlessSource::Keyboard, DeviceId::KEYBOARD),
    ] {
        let sequence = binding
            .begin_contactless(source)
            .expect("a fresh binding has ids to issue");
        assert_eq!(
            sequence.device, device,
            "{source:?} runs on its reserved device"
        );
        assert!(sequence.device.is_reserved());
        assert!(
            seen.insert(sequence.id),
            "{source:?} reused {:?}",
            sequence.id
        );
    }
    // Contacts and contactless sources draw from the binding's one mint.
    let later = send(&binding, &down(2)).expect("contact");
    assert!(seen.insert(later));
}

#[test]
fn an_exhausted_binding_refuses_new_sequences_permanently() {
    let binding = GestureBinding::new().with_sequences_after(u64::MAX - 1);
    let last = send(&binding, &down(1)).expect("one id remains");
    assert_eq!(last.get(), u64::MAX);
    assert_eq!(send(&binding, &up(1)), Some(last));
    for id in [2, 3] {
        assert_eq!(
            send(&binding, &down(id)),
            None,
            "an exhausted binding refused the contact"
        );
        assert_eq!(binding.active_pointer_count(), 0);
    }
    assert_eq!(send(&binding, &wheel(ScrollPhase::Began)), None);
    assert_eq!(
        binding.begin_contactless(ContactlessSource::Assistive),
        Err(IdExhausted)
    );
}

#[test]
fn a_wheel_under_a_held_button_belongs_to_its_latch() {
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let binding = GestureBinding::new();
    let arena = binding.arena().clone();
    let seen = Rc::new(Cell::new(None));
    lane.enter(|| {
        let record = Rc::clone(&seen);
        let target = handle
            .register_scroll(move |_| {
                record.set(Some((
                    arena.dispatched_sequence(),
                    arena.dispatched_latch_end(),
                )));
                EventPropagation::Stop
            })
            .expect("scroll consumer");
        let mut path = HitTestResult::new();
        path.add(HitTestEntry::new(RenderId::new(1)).scroll_target(target));

        let press = make_down_event_for_id(pointer(1), Offset::ZERO, PointerKind::Mouse)
            .expect("mouse down");
        let contact = send(&binding, &press).expect("the press is a contact");
        let PointerEvent::Scroll(scroll) =
            make_scroll_event(Offset::ZERO, Offset::new(0.0, 10.0)).expect("finite wheel")
        else {
            unreachable!("a scroll helper builds a scroll")
        };
        assert_eq!(
            scroll.pointer.id,
            pointer(1),
            "the wheel shares the held pointer"
        );
        let latch = binding
            .dispatch_pointer_on(InputSurface::Window, &PointerEvent::Scroll(scroll), |_| {
                path.clone()
            })
            .sequence()
            .expect("a wheel burst is a sequence");
        assert_ne!(latch, contact, "the wheel opened its own latch");
        let (consumed, end) = seen.get().expect("the consumer took the packet");
        assert_eq!(
            consumed,
            Some(latch),
            "the consumer names the latch, not the contact"
        );
        assert!(
            matches!(end, Some(ScrollLatchEnd::IdleAt(_))),
            "the consumer learns when the latch lapses, got {end:?}"
        );
        let release =
            make_up_event_for_id(pointer(1), Offset::ZERO, PointerKind::Mouse).expect("mouse up");
        assert_eq!(send(&binding, &release), Some(contact));
    });
}

#[test]
fn haptics_requested_in_a_contactless_dispatch_carry_its_sequence() {
    let binding = GestureBinding::new();
    let fake = Arc::new(FakeHaptics::new());
    let actuator: Arc<dyn PlatformHaptics> = fake.clone();
    let host = HapticsHost::builder(&binding, move || Some(Arc::clone(&actuator))).build();
    let haptics = host.handle();
    let action = binding
        .begin_contactless(ContactlessSource::Assistive)
        .expect("an id to issue");
    let request = HapticRequest::new(HapticTiming::Immediate);
    binding.dispatch_contactless(action.id, || {
        let _ = haptics.perform(HapticFeedback::SelectionClick, request);
    });
    let _ = haptics.perform(HapticFeedback::SelectionClick, request);
    let tagged: Vec<_> = fake
        .calls()
        .iter()
        .map(|call| call.request.sequence)
        .collect();
    assert_eq!(tagged, [Some(action.id), None]);
}
