//! Button edges preserve the chronological contact route around queued movement.

use std::{cell::RefCell, rc::Rc};

use flui_interaction::{GestureBinding, HitTestEntry, HitTestResult, InteractionLane, RenderId};
use flui_platform_api::{
    EventTime,
    pointer::{
        ButtonChange, DeviceId, PointerButton, PointerButtons, PointerEvent, PointerId,
        PointerInfo, PointerKind, PointerMove, PointerPosition, PointerPress, PointerRelease,
        PointerRole, PointerSample,
    },
};

#[test]
fn queued_move_precedes_button_edges_without_restarting_the_contact() {
    assert_button_contacts(false, false, false);
}

#[test]
fn resampled_move_precedes_button_edges_without_restarting_the_contact() {
    assert_button_contacts(false, false, true);
}

#[test]
fn queued_move_failure_still_delivers_button_edge_and_preserves_first_failure() {
    for edge_panics in [false, true] {
        assert_button_contacts(true, edge_panics, false);
    }
}

fn assert_button_contacts(move_panics: bool, edge_panics: bool, resampling: bool) {
    let lane = InteractionLane::try_new().expect("owner lane");
    let clock = std::sync::Arc::new(flui_foundation::ManualClock::new());
    let binding = GestureBinding::with_clock(clock.clone());
    binding
        .set_resampling_enabled(resampling)
        .expect("no active contact");
    binding.set_sampling_clock(flui_interaction::processing::SamplingClock::Manual {
        period: std::time::Duration::from_millis(16),
    });
    let observed = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&observed);
    lane.enter(|| {
        let target = lane
            .dispatch_handle()
            .register_pointer(move |dispatch| {
                let record = match dispatch.local {
                    PointerEvent::Down(event) => (
                        "down",
                        event.sample.time.as_nanos(),
                        event.buttons(),
                        Vec::new(),
                    ),
                    PointerEvent::Move(event) => (
                        "move",
                        event.current().time.as_nanos(),
                        event.buttons,
                        event
                            .coalesced()
                            .iter()
                            .map(|sample| sample.time.as_nanos())
                            .collect(),
                    ),
                    PointerEvent::ButtonChange(ButtonChange::Pressed(event)) => (
                        "press",
                        event.sample.time.as_nanos(),
                        event.buttons(),
                        Vec::new(),
                    ),
                    PointerEvent::ButtonChange(ButtonChange::Released(event)) => (
                        "release",
                        event.sample.time.as_nanos(),
                        event.buttons(),
                        Vec::new(),
                    ),
                    PointerEvent::Up(event) => (
                        "up",
                        event.sample.time.as_nanos(),
                        event.buttons(),
                        Vec::new(),
                    ),
                    _ => return,
                };
                let failing_move = move_panics && record.0 == "move" && record.1 == 10_000_000;
                let failing_edge = edge_panics && record.0 == "press" && record.1 == 20_000_000;
                log.borrow_mut().push(record);
                if failing_move {
                    panic!("older movement failure");
                }
                if failing_edge {
                    panic!("later button failure");
                }
            })
            .expect("register listener route");
        let route = |_| {
            let mut path = HitTestResult::new();
            path.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
            path
        };
        let primary = PointerButtons::only(PointerButton::PRIMARY);
        let both = primary.with(PointerButton::SECONDARY);
        for (identity, base) in [(1, 0), (2, 40)] {
            observed.borrow_mut().clear();
            let pointer = PointerInfo::new(
                PointerId::try_from(identity).expect("nonzero contact"),
                PointerKind::Mouse,
            )
            .with_device(DeviceId::try_from(1).expect("nonzero hardware"))
            .with_role(PointerRole::Primary);
            let sample = |millis| {
                PointerSample::new(
                    EventTime::from_nanos((base + millis) * 1_000_000),
                    PointerPosition::try_new(flui_foundation::geometry::Point::new(
                        millis as f64,
                        0.0,
                    ))
                    .expect("finite coordinate"),
                )
            };
            binding.handle_pointer_event(
                &PointerEvent::Down(PointerPress::new(
                    pointer,
                    PointerButton::PRIMARY,
                    primary,
                    sample(0),
                )),
                route,
            );
            binding.handle_pointer_event(
                &PointerEvent::Move(
                    PointerMove::new(pointer, primary, sample(10)).with_coalesced(vec![sample(5)]),
                ),
                route,
            );
            assert_eq!(
                observed.borrow().len(),
                1,
                "the movement awaits frame delivery"
            );
            let edge = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                binding.handle_pointer_event(
                    &PointerEvent::ButtonChange(ButtonChange::Pressed(PointerPress::new(
                        pointer,
                        PointerButton::SECONDARY,
                        both,
                        sample(20),
                    ))),
                    route,
                )
            }));
            if move_panics && base == 0 {
                let payload = edge
                    .expect_err("the earlier movement failure leaves dispatch after edge delivery");
                assert_eq!(
                    payload.downcast_ref::<&str>(),
                    Some(&"older movement failure")
                );
            } else {
                edge.expect("healthy button edge");
            }
            assert_eq!(
                binding.active_pointer_count(),
                1,
                "another button does not create or finish a contact"
            );
            assert_eq!(
                &*observed.borrow(),
                &[
                    ("down", base * 1_000_000, primary, vec![]),
                    (
                        "move",
                        (base + 10) * 1_000_000,
                        primary,
                        vec![(base + 5) * 1_000_000]
                    ),
                    ("press", (base + 20) * 1_000_000, both, vec![]),
                ],
                "an edge cannot overtake the earlier movement or rewrite its samples/buttons"
            );
            binding.handle_pointer_event(
                &PointerEvent::ButtonChange(ButtonChange::Released(PointerRelease::new(
                    pointer,
                    PointerButton::SECONDARY,
                    primary,
                    sample(25),
                ))),
                route,
            );
            assert_eq!(binding.active_pointer_count(), 1);
            binding.handle_pointer_event(
                &PointerEvent::Move(
                    PointerMove::new(pointer, primary, sample(28)).with_coalesced(vec![sample(26)]),
                ),
                route,
            );
            binding.handle_pointer_event(
                &PointerEvent::Up(PointerRelease::new(
                    pointer,
                    PointerButton::PRIMARY,
                    PointerButtons::NONE,
                    sample(30),
                )),
                route,
            );
            use flui_foundation::MonotonicClock as _;
            clock.advance(std::time::Duration::from_millis(40));
            let now = clock.now();
            binding
                .flush_pending_moves_at(now, now + std::time::Duration::from_millis(16))
                .expect("advancing manual sample window");
            assert_eq!(binding.active_pointer_count(), 0);
            assert_eq!(
                &observed.borrow()[3..],
                &[
                    ("release", (base + 25) * 1_000_000, primary, vec![]),
                    (
                        "move",
                        (base + 28) * 1_000_000,
                        primary,
                        vec![(base + 26) * 1_000_000]
                    ),
                    ("up", (base + 30) * 1_000_000, PointerButtons::NONE, vec![]),
                ],
                "the terminal event leaves no queued movement debt"
            );
        }
    });
}
