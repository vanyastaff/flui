//! Shared pointer assembly: the rules every native backend decodes through.

use std::num::{NonZeroU8, NonZeroU64};
use std::time::Duration;

use flui_platform::shared::pointer_assembly::{
    ClickSettings, ContactRegistry, CounterAnchor, DecodeFault, GestureComponent, GestureDelta,
    GesturePhase, GestureReading, MoveBatch, NativeCancel, NativeClock, NativeContact,
    NativeDevice, NativeReading, NativeSurface, NativeTime, QpcTicks, ReadingAction, ReadingTime,
    ScrollPhases, ScrollReading, decode,
};
use flui_platform::traits::PlatformInput;
use flui_platform_api::EventTime;
use flui_platform_api::pointer::{
    CancelReason, DeviceId, PanZoomPhase, PointerButton, PointerEvent, PointerKind, PointerRole,
};

fn at_ms(ms: u64) -> ReadingTime {
    ReadingTime::epoch(EventTime::from_nanos(ms * 1_000_000))
}

fn reading(contact: u64, kind: PointerKind, action: ReadingAction, ms: u64) -> NativeReading {
    NativeReading::new(NativeContact(contact), kind, action, at_ms(ms)).at(10.0, 20.0, 1.0)
}

fn touch(contact: u64, action: ReadingAction, ms: u64) -> NativeReading {
    reading(contact, PointerKind::Touch, action, ms)
}

fn mouse(action: ReadingAction, ms: u64) -> NativeReading {
    reading(0, PointerKind::Mouse, action, ms)
}

fn pointer_events(inputs: &[PlatformInput]) -> Vec<&PointerEvent> {
    inputs
        .iter()
        .map(|input| input.as_pointer().expect("pointer input"))
        .collect()
}

fn single(inputs: &[PlatformInput]) -> PointerEvent {
    match pointer_events(inputs).as_slice() {
        [event] => (*event).clone(),
        events => panic!("expected one event, got {events:?}"),
    }
}

const PRESS: ReadingAction = ReadingAction::Press(PointerButton::PRIMARY);
const RELEASE: ReadingAction = ReadingAction::Release(PointerButton::PRIMARY);

fn registry() -> ContactRegistry {
    ContactRegistry::new(NativeClock::epoch_only())
}

fn a_released_label_starts_a_new_contact() {
    let mut contacts = registry();
    let PointerEvent::Down(first) =
        single(&decode(&mut contacts, &touch(7, PRESS, 1)).expect("valid"))
    else {
        panic!("first down")
    };
    let first = first.pointer;
    assert!(matches!(
        single(&decode(&mut contacts, &touch(7, ReadingAction::Move, 2)).expect("valid")),
        PointerEvent::Move(movement) if movement.pointer == first && movement.buttons.contains(PointerButton::PRIMARY)
    ));
    assert!(matches!(
        single(&decode(&mut contacts, &touch(7, RELEASE, 3)).expect("valid")),
        PointerEvent::Up(up) if up.pointer == first && up.buttons().is_empty()
    ));
    let PointerEvent::Down(second) =
        single(&decode(&mut contacts, &touch(7, PRESS, 4)).expect("valid"))
    else {
        panic!("second down")
    };
    assert_ne!(
        second.pointer.id, first.id,
        "a reused label is a new contact"
    );
    assert_eq!(second.pointer.role, PointerRole::Primary);
}

fn a_second_finger_is_additional_until_the_first_lifts() {
    let mut contacts = registry();
    decode(&mut contacts, &touch(1, PRESS, 1)).expect("valid");
    let PointerEvent::Down(second) =
        single(&decode(&mut contacts, &touch(2, PRESS, 2)).expect("valid"))
    else {
        panic!("second down")
    };
    assert_eq!(second.pointer.role, PointerRole::Additional);
    decode(&mut contacts, &touch(1, RELEASE, 3)).expect("valid");
    decode(&mut contacts, &touch(2, RELEASE, 4)).expect("valid");
    let PointerEvent::Down(third) =
        single(&decode(&mut contacts, &touch(3, PRESS, 5)).expect("valid"))
    else {
        panic!("third down")
    };
    assert_eq!(third.pointer.role, PointerRole::Primary);
}

fn a_mouse_keeps_its_identity_across_presses() {
    let mut contacts = registry();
    decode(&mut contacts, &mouse(ReadingAction::Move, 1)).expect("valid");
    let PointerEvent::Down(down) = single(&decode(&mut contacts, &mouse(PRESS, 2)).expect("valid"))
    else {
        panic!("down")
    };
    let chord = decode(
        &mut contacts,
        &mouse(ReadingAction::Press(PointerButton::SECONDARY), 3),
    )
    .expect("valid");
    assert!(matches!(single(&chord), PointerEvent::ButtonChange(_)));
    decode(
        &mut contacts,
        &mouse(ReadingAction::Release(PointerButton::SECONDARY), 4),
    )
    .expect("valid");
    assert!(matches!(
        single(&decode(&mut contacts, &mouse(RELEASE, 5)).expect("valid")),
        PointerEvent::Up(_)
    ));
    let PointerEvent::Down(again) =
        single(&decode(&mut contacts, &mouse(PRESS, 6)).expect("valid"))
    else {
        panic!("down again")
    };
    assert_eq!(again.pointer, down.pointer);
    let cancel = ReadingAction::Cancel(NativeCancel::Platform);
    assert!(matches!(
        single(&decode(&mut contacts, &mouse(cancel, 7)).expect("valid")),
        PointerEvent::Cancel(ended) if ended.pointer == down.pointer
    ));
    assert!(
        decode(&mut contacts, &mouse(cancel, 8))
            .expect("the cursor stays open")
            .is_empty(),
        "a hovering mouse has no sequence to cancel"
    );
    let PointerEvent::Down(after) =
        single(&decode(&mut contacts, &mouse(PRESS, 9)).expect("valid"))
    else {
        panic!("down after cancel")
    };
    assert_eq!(after.pointer, down.pointer);
}

fn a_pen_sequence_is_its_contact_not_its_barrel_button() {
    let mut contacts = registry();
    let pen = PointerKind::Pen {
        tool: flui_platform_api::pointer::PenTool::Tip,
    };
    let barrel = PointerButton::SECONDARY;
    decode(&mut contacts, &reading(3, pen, ReadingAction::Move, 1)).expect("valid");
    assert!(
        matches!(
            single(
                &decode(
                    &mut contacts,
                    &reading(3, pen, ReadingAction::Press(barrel), 2)
                )
                .expect("valid")
            ),
            PointerEvent::ButtonChange(_)
        ),
        "a hovering pen's barrel press does not start a sequence"
    );
    let PointerEvent::Down(down) =
        single(&decode(&mut contacts, &reading(3, pen, PRESS, 3)).expect("valid"))
    else {
        panic!("the tip touching starts it")
    };
    assert!(down.buttons().contains(barrel));
    let lifted = reading(3, pen, RELEASE, 4)
        .with_buttons(flui_platform_api::pointer::PointerButtons::only(barrel));
    assert!(
        matches!(
            single(&decode(&mut contacts, &lifted).expect("valid")),
            PointerEvent::Up(_)
        ),
        "the tip lifting ends it while the barrel stays held"
    );
}

/// A pen that lifts goes back to hovering on the same identity, and the
/// platform's leave ends it, so its `Enter` and `Leave` pair up. A touch has
/// no hover: it ends at `Up`. A hovering pen whose label comes back as
/// another kind leaves before the new contact starts.
fn a_lifted_pen_hovers_on_its_identity_until_it_leaves() {
    let mut contacts = registry();
    let pen = PointerKind::Pen {
        tool: flui_platform_api::pointer::PenTool::Tip,
    };
    let kinds = |events: &[PlatformInput]| -> Vec<(&'static str, u64)> {
        pointer_events(events)
            .into_iter()
            .map(|event| match event {
                PointerEvent::Enter(signal) => ("enter", signal.pointer.id.get().get()),
                PointerEvent::Leave(signal) => ("leave", signal.pointer.id.get().get()),
                PointerEvent::Down(press) => ("down", press.pointer.id.get().get()),
                PointerEvent::Up(release) => ("up", release.pointer.id.get().get()),
                PointerEvent::Move(movement) => ("move", movement.pointer.id.get().get()),
                other => panic!("unexpected {other:?}"),
            })
            .collect()
    };
    let mut events = Vec::new();
    for (action, ms) in [
        (ReadingAction::Enter, 1),
        (PRESS, 2),
        (RELEASE, 3),
        (ReadingAction::Move, 4),
        (ReadingAction::Leave, 5),
    ] {
        events.extend(decode(&mut contacts, &reading(3, pen, action, ms)).expect("valid"));
    }
    assert_eq!(
        kinds(&events),
        [
            ("enter", 1),
            ("down", 1),
            ("up", 1),
            ("move", 1),
            ("leave", 1)
        ],
        "one identity from enter to leave"
    );

    for (action, ms) in [(PRESS, 6), (RELEASE, 7)] {
        decode(&mut contacts, &touch(4, action, ms)).expect("valid");
    }
    assert_eq!(
        decode(&mut contacts, &touch(4, ReadingAction::Move, 8))
            .expect_err("no touch contact is left after Up")
            .fault(),
        DecodeFault::Orphan
    );

    let mut events = Vec::new();
    for (action, ms) in [(ReadingAction::Enter, 9), (PRESS, 10), (RELEASE, 11)] {
        events.extend(decode(&mut contacts, &reading(5, pen, action, ms)).expect("valid"));
    }
    events.extend(decode(&mut contacts, &touch(5, PRESS, 12)).expect("valid"));
    assert_eq!(
        kinds(&events),
        [
            ("enter", 3),
            ("down", 3),
            ("up", 3),
            ("leave", 3),
            ("down", 4)
        ],
        "the label's next owner starts after the hover leaves"
    );
}

fn the_originating_reading_lives_for_its_sequence() {
    let mut contacts = registry();
    let press = touch(4, PRESS, 1);
    let PointerEvent::Down(down) = single(&decode(&mut contacts, &press).expect("valid")) else {
        panic!("down")
    };
    decode(&mut contacts, &touch(4, ReadingAction::Move, 2)).expect("valid");
    assert_eq!(contacts.origin(down.pointer.id), Some(&press));
    decode(&mut contacts, &touch(4, RELEASE, 3)).expect("valid");
    assert_eq!(contacts.origin(down.pointer.id), None);
}

fn a_hot_plugged_device_is_announced_and_retired() {
    let mut contacts = registry();
    let pen = PointerKind::Pen {
        tool: flui_platform_api::pointer::PenTool::Tip,
    };
    let first = reading(1, pen, PRESS, 1).from_device(NativeDevice(40));
    let events = decode(&mut contacts, &first).expect("valid");
    let [PointerEvent::DeviceAdded(added), PointerEvent::Down(down)] =
        pointer_events(&events).as_slice()
    else {
        panic!("an unseen device is announced before its first contact: {events:?}")
    };
    assert_eq!(down.pointer.device, Some(added.device));
    assert_eq!(added.kind, pen);
    let (ended, removed) =
        contacts.remove_device(NativeDevice(40), EventTime::from_nanos(9_000_000));
    assert!(matches!(
        ended.as_slice(),
        [(NativeSurface(0), PlatformInput::Pointer(PointerEvent::Cancel(cancel)))]
            if cancel.pointer == down.pointer && cancel.reason == CancelReason::DeviceRemoved
    ));
    let removed = removed.expect("a present device is removed");
    assert_eq!((removed.device, removed.kind), (added.device, pen));
    let replugged = decode(
        &mut contacts,
        &reading(1, pen, PRESS, 10).from_device(NativeDevice(40)),
    )
    .expect("valid");
    let [PointerEvent::DeviceAdded(again), PointerEvent::Down(_)] =
        pointer_events(&replugged).as_slice()
    else {
        panic!("a replugged label is announced again")
    };
    assert_ne!(
        again.device, added.device,
        "a retired identity is never reissued"
    );
    assert!(
        contacts
            .add_device(NativeDevice(40), pen, EventTime::from_nanos(0))
            .is_none(),
        "a present device is not announced twice"
    );
}

fn exhausted_identities_refuse_instead_of_wrapping() {
    let mut contacts = registry().with_first_pointer(NonZeroU64::MAX);
    let PointerEvent::Down(last) =
        single(&decode(&mut contacts, &touch(1, PRESS, 1)).expect("valid"))
    else {
        panic!("the last identity is issued")
    };
    assert_eq!(last.pointer.id.get(), NonZeroU64::MAX);
    decode(&mut contacts, &touch(1, RELEASE, 2)).expect("valid");
    let refused = decode(&mut contacts, &touch(2, PRESS, 3)).expect_err("refused");
    assert_eq!(refused.fault(), DecodeFault::Exhausted);
    assert!(refused.into_inputs().is_empty());

    // Hardware never takes a reserved contactless device id: the last one it
    // takes is just below them.
    let last_hardware = DeviceId::KEYBOARD.get().get() - 1;
    let mut devices =
        registry().with_first_device(NonZeroU64::new(last_hardware).expect("nonzero"));
    let time = EventTime::from_nanos(0);
    let last = devices
        .add_device(NativeDevice(1), PointerKind::Touch, time)
        .expect("the last device identity is issued");
    assert_eq!(last.device.get().get(), last_hardware);
    assert!(!last.device.is_reserved());
    devices.remove_device(NativeDevice(1), time);
    assert!(
        devices
            .add_device(NativeDevice(1), PointerKind::Touch, time)
            .is_none(),
        "a retired device label gets no reissued identity"
    );
    let unannounced = decode(
        &mut devices,
        &touch(1, PRESS, 1).from_device(NativeDevice(2)),
    )
    .expect_err("refused");
    assert_eq!(unannounced.fault(), DecodeFault::Exhausted);
}

fn click_settings() -> ClickSettings {
    ClickSettings {
        interval: Duration::from_millis(500),
        area: (4.0, 4.0),
    }
}

fn click(contacts: &mut ContactRegistry, ms: u64, x: f64) -> Option<u8> {
    let press = mouse(PRESS, ms).at(x, 20.0, 1.0);
    let PointerEvent::Down(down) = single(&decode(contacts, &press).expect("valid")) else {
        panic!("press")
    };
    decode(contacts, &mouse(RELEASE, ms + 1).at(x, 20.0, 1.0)).expect("valid");
    down.click_count.map(NonZeroU8::get)
}

fn clicks_count_within_the_os_double_click_interval() {
    let mut contacts = registry();
    contacts.set_click_settings(Some(click_settings()));
    assert_eq!(click(&mut contacts, 0, 10.0), Some(1));
    assert_eq!(
        click(&mut contacts, 400, 11.0),
        Some(2),
        "inside interval and area"
    );
    assert_eq!(click(&mut contacts, 800, 11.0), Some(3));
    assert_eq!(
        click(&mut contacts, 1_400, 11.0),
        Some(1),
        "past the interval"
    );
    assert_eq!(
        click(&mut contacts, 1_500, 20.0),
        Some(1),
        "outside the area"
    );
    let mut uncounted = registry();
    assert_eq!(
        click(&mut uncounted, 0, 10.0),
        None,
        "no OS settings, no count"
    );
    let reported = mouse(PRESS, 0).with_click_count(NonZeroU8::new(3).expect("valid"));
    let PointerEvent::Down(down) = single(&decode(&mut uncounted, &reported).expect("valid"))
    else {
        panic!("press")
    };
    assert_eq!(
        down.click_count,
        NonZeroU8::new(3),
        "the platform's own count wins"
    );
}

fn gesture(component: GestureComponent, phase: GesturePhase, delta: GestureDelta) -> ReadingAction {
    ReadingAction::Gesture(GestureReading {
        component,
        phase,
        delta,
    })
}

fn pan_zoom_phases(contacts: &mut ContactRegistry, action: ReadingAction) -> Vec<PanZoomPhase> {
    decode(contacts, &reading(9, PointerKind::Trackpad, action, 1))
        .expect("valid")
        .iter()
        .map(|input| match input.as_pointer() {
            Some(PointerEvent::PanZoom(event)) => event.phase,
            other => panic!("pan-zoom expected, got {other:?}"),
        })
        .collect()
}

fn pinch_and_rotation_accumulate_into_one_gesture() {
    use GestureComponent::{Rotation, Scale, Session};
    use GesturePhase::{Began, Cancelled, Changed, Ended};
    let mut contacts = registry();
    let phases = pan_zoom_phases(
        &mut contacts,
        gesture(Scale, Began, GestureDelta::Scale(1.5)),
    );
    assert!(matches!(
        phases.as_slice(),
        [PanZoomPhase::Start, PanZoomPhase::Update(t)] if t.scale() == 1.5
    ));
    let phases = pan_zoom_phases(
        &mut contacts,
        gesture(Rotation, Began, GestureDelta::Rotation(0.25)),
    );
    assert!(matches!(
        phases.as_slice(),
        [PanZoomPhase::Update(t)] if t.scale() == 1.5 && t.rotation() == 0.25
    ));
    let phases = pan_zoom_phases(
        &mut contacts,
        gesture(Scale, Changed, GestureDelta::Scale(2.0)),
    );
    assert!(matches!(
        phases.as_slice(),
        [PanZoomPhase::Update(t)] if t.scale() == 3.0 && t.rotation() == 0.25
    ));
    assert_eq!(
        pan_zoom_phases(&mut contacts, gesture(Scale, Ended, GestureDelta::None)),
        [] as [PanZoomPhase; 0]
    );
    assert_eq!(
        pan_zoom_phases(&mut contacts, gesture(Rotation, Ended, GestureDelta::None)),
        [PanZoomPhase::End]
    );
    let late = reading(
        9,
        PointerKind::Trackpad,
        gesture(Scale, Changed, GestureDelta::Scale(2.0)),
        2,
    );
    assert_eq!(
        decode(&mut contacts, &late).expect_err("refused").fault(),
        DecodeFault::Orphan
    );

    pan_zoom_phases(&mut contacts, gesture(Scale, Began, GestureDelta::None));
    pan_zoom_phases(&mut contacts, gesture(Rotation, Began, GestureDelta::None));
    assert_eq!(
        pan_zoom_phases(&mut contacts, gesture(Session, Ended, GestureDelta::None)),
        [PanZoomPhase::End],
        "the session's end retires components whose own end never came"
    );
    let phases = pan_zoom_phases(
        &mut contacts,
        gesture(Scale, Began, GestureDelta::Scale(2.0)),
    );
    assert!(
        matches!(
            phases.as_slice(),
            [PanZoomPhase::Start, PanZoomPhase::Update(t)] if t.scale() == 2.0
        ),
        "a new gesture starts from identity"
    );
    assert_eq!(
        pan_zoom_phases(&mut contacts, gesture(Scale, Cancelled, GestureDelta::None)),
        [PanZoomPhase::Cancelled]
    );
}

/// A pinch carries only scale and rotation; translation is its focal point,
/// refreshed on every update, so moving fingers publish an update even while
/// scale and rotation stand still.
fn pan_zoom_carries_only_scale_and_rotation() {
    use GestureComponent::Scale;
    use GesturePhase::{Began, Changed, Ended};
    let mut contacts = registry();
    let mut at = |action, x: f64, ms| {
        decode(
            &mut contacts,
            &reading(9, PointerKind::Trackpad, action, ms).at(x, 20.0, 2.0),
        )
        .expect("valid")
        .into_iter()
        .map(|input| match input {
            PlatformInput::Pointer(PointerEvent::PanZoom(event)) => {
                (event.phase, event.position.get().x)
            }
            other => panic!("pan-zoom expected, got {other:?}"),
        })
        .collect::<Vec<_>>()
    };
    let identity = flui_platform_api::pointer::PanZoomTransform::IDENTITY;
    assert_eq!(
        at(gesture(Scale, Began, GestureDelta::None), 10.0, 1),
        [(PanZoomPhase::Start, 5.0)]
    );
    assert_eq!(
        at(gesture(Scale, Changed, GestureDelta::Scale(1.0)), 30.0, 2),
        [(PanZoomPhase::Update(identity), 15.0)],
        "the focal point moved: an update with an unchanged transform"
    );
    assert_eq!(
        at(gesture(Scale, Changed, GestureDelta::Scale(1.0)), 30.0, 3),
        [],
        "nothing changed: no update"
    );
    assert_eq!(
        at(gesture(Scale, Ended, GestureDelta::None), 50.0, 4),
        [
            (PanZoomPhase::Update(identity), 25.0),
            (PanZoomPhase::End, 25.0)
        ]
    );
}

/// Every backend's native scroll phases, through the shared grammar: a
/// two-finger pan begins, changes and ends as a phased scroll, and platform
/// momentum follows as `Momentum*`. Rows are the native encodings: winit's
/// `PanGesture`/pixel `MouseWheel` touch phases, AppKit's `phase` and
/// `momentumPhase`.
fn two_finger_pan_is_phased_scroll_on_every_backend() {
    use GesturePhase::{Began, Changed, Ended};
    use flui_platform_api::pointer::ScrollPhase;
    let fingers = |phase| ScrollReading::fingers(phase, true);
    type Row = (&'static str, Vec<ScrollReading>, Vec<Option<ScrollPhase>>);
    let rows: [Row; 3] = [
        (
            "winit",
            vec![fingers(Began), fingers(Changed), fingers(Ended)],
            vec![
                Some(ScrollPhase::Began),
                Some(ScrollPhase::Changed),
                Some(ScrollPhase::Ended),
            ],
        ),
        (
            "appkit",
            vec![
                fingers(Began),
                fingers(Changed),
                fingers(Ended),
                ScrollReading::momentum(Began),
                ScrollReading::momentum(Changed),
                ScrollReading {
                    fingers: Some(Ended),
                    momentum: Some(Ended),
                    precise: true,
                },
            ],
            vec![
                Some(ScrollPhase::Began),
                Some(ScrollPhase::Changed),
                Some(ScrollPhase::Ended),
                Some(ScrollPhase::MomentumBegan),
                Some(ScrollPhase::MomentumChanged),
                Some(ScrollPhase::MomentumEnded),
            ],
        ),
        (
            "notched wheel",
            vec![
                ScrollReading::fingers(Changed, false),
                ScrollReading::default(),
            ],
            vec![None, None],
        ),
    ];
    for (backend, readings, expected) in rows {
        let mut phases = ScrollPhases::default();
        let got: Vec<_> = readings
            .into_iter()
            .map(|reading| phases.phase(NativeSurface(1), Some(NativeDevice(1)), reading))
            .collect();
        assert_eq!(got, expected, "{backend}");
    }
}

fn a_batch_attaches_its_readings_in_order() {
    let mut contacts = registry();
    decode(&mut contacts, &touch(1, PRESS, 10)).expect("valid");
    let mut batch = MoveBatch::new();
    for (ms, x) in [(11, 11.0), (12, 12.0)] {
        batch.push_coalesced(touch(1, ReadingAction::Move, ms).at(x, 20.0, 1.0));
    }
    batch.push_predicted(touch(1, ReadingAction::Move, 15).at(15.0, 20.0, 1.0));
    batch.push_predicted(touch(1, ReadingAction::Move, 16).at(f64::NAN, 20.0, 1.0));
    let assembled = batch
        .decode(
            &mut contacts,
            &touch(1, ReadingAction::Move, 13).at(13.0, 20.0, 1.0),
        )
        .expect("valid");
    let PointerEvent::Move(movement) = single(&assembled) else {
        panic!("move")
    };
    let xs = |samples: &[flui_platform_api::pointer::PointerSample]| {
        samples
            .iter()
            .map(|sample| sample.position.get().x)
            .collect::<Vec<_>>()
    };
    assert_eq!(xs(movement.coalesced()), [11.0, 12.0]);
    assert_eq!(movement.current().position.get().x, 13.0);
    assert_eq!(
        xs(movement.predicted()),
        [15.0],
        "a bad prediction is only dropped"
    );

    let plain = decode(&mut contacts, &touch(1, ReadingAction::Move, 14)).expect("valid");
    let PointerEvent::Move(plain) = single(&plain) else {
        panic!("move")
    };
    assert!(plain.coalesced().is_empty(), "the batch was emptied");

    batch.push_coalesced(touch(1, ReadingAction::Move, 15).at(f64::INFINITY, 0.0, 1.0));
    let failed = batch
        .decode(&mut contacts, &touch(1, ReadingAction::Move, 16))
        .expect_err("refused");
    assert!(
        matches!(
            failed.cancel().as_ref().and_then(PlatformInput::as_pointer),
            Some(PointerEvent::Cancel(cancel)) if cancel.reason == CancelReason::InvalidInput
        ),
        "one undecodable coalesced reading fails the batch"
    );
}

fn an_undecodable_reading_cancels_its_contact_as_invalid_input() {
    let mut contacts = registry();
    let PointerEvent::Down(down) =
        single(&decode(&mut contacts, &touch(1, PRESS, 1)).expect("valid"))
    else {
        panic!("down")
    };
    for (name, bad) in [
        (
            "non-finite position",
            touch(1, ReadingAction::Move, 2).at(f64::NAN, 0.0, 1.0),
        ),
        (
            "zero scale",
            touch(1, ReadingAction::Move, 2).at(1.0, 1.0, 0.0),
        ),
        (
            "pressure past one",
            touch(1, ReadingAction::Move, 2).with_sensors(
                flui_platform::shared::pointer_assembly::RawSensors {
                    pressure: Some(1.5),
                    ..Default::default()
                },
            ),
        ),
    ] {
        let mut contacts = registry();
        decode(&mut contacts, &touch(1, PRESS, 1)).expect("valid");
        let error = decode(&mut contacts, &bad).expect_err("refused");
        let inputs = error.clone().into_inputs();
        assert!(
            matches!(
                single(&inputs),
                PointerEvent::Cancel(cancel)
                    if cancel.pointer == down.pointer && cancel.reason == CancelReason::InvalidInput
            ),
            "{name}: one cancel ends the open contact"
        );
        let after = decode(&mut contacts, &touch(1, RELEASE, 3)).expect_err("refused");
        assert_eq!(
            after.fault(),
            DecodeFault::Orphan,
            "{name}: the contact is closed"
        );
        assert!(after.into_inputs().is_empty());
    }
    let release_without_position =
        NativeReading::new(NativeContact(1), PointerKind::Touch, RELEASE, at_ms(2));
    assert!(
        matches!(
            single(&decode(&mut contacts, &release_without_position).expect("valid")),
            PointerEvent::Up(up) if up.sample.position == down.sample.position
        ),
        "an event without a position is read at the contact's last one"
    );
}

fn a_press_without_any_position_is_suppressed_with_its_release() {
    let mut contacts = registry();
    let unplaced =
        |action| NativeReading::new(NativeContact(0), PointerKind::Mouse, action, at_ms(1));
    assert!(
        decode(&mut contacts, &unplaced(PRESS))
            .expect("valid")
            .is_empty()
    );
    decode(&mut contacts, &mouse(ReadingAction::Move, 2)).expect("valid");
    assert!(
        decode(&mut contacts, &mouse(RELEASE, 3))
            .expect("valid")
            .is_empty(),
        "the release of a suppressed press is dropped too"
    );
    assert!(matches!(
        single(&decode(&mut contacts, &mouse(PRESS, 4)).expect("valid")),
        PointerEvent::Down(_)
    ));
}

fn an_orphan_reading_emits_nothing() {
    let mut contacts = registry();
    for action in [
        RELEASE,
        ReadingAction::Leave,
        ReadingAction::Cancel(NativeCancel::Platform),
        ReadingAction::Move,
    ] {
        let error = decode(&mut contacts, &touch(5, action, 1)).expect_err("refused");
        assert_eq!(error.fault(), DecodeFault::Orphan, "{action:?}");
        assert!(error.into_inputs().is_empty(), "{action:?}");
    }
    let undecodable = touch(6, ReadingAction::Move, 1).at(f64::NAN, 0.0, 1.0);
    assert!(
        decode(&mut contacts, &undecodable)
            .expect_err("refused")
            .into_inputs()
            .is_empty()
    );
}

fn counter_clock() -> NativeClock {
    NativeClock::new(
        EventTime::from_nanos(1_000_000_000),
        Duration::from_millis(5_000),
        Some(CounterAnchor {
            count: QpcTicks(1_000_000),
            frequency: NonZeroU64::new(10_000_000).expect("valid"),
        }),
    )
}

fn stamped(contacts: &mut ContactRegistry, reading: &NativeReading) -> u64 {
    let events = decode(contacts, reading).expect("valid");
    match pointer_events(&events).last().expect("an event") {
        PointerEvent::Down(event) => event.sample.time,
        PointerEvent::Up(event) => event.sample.time,
        PointerEvent::Move(event) => event.current().time,
        other => panic!("unexpected {other:?}"),
    }
    .as_nanos()
}

fn timed(contact: u64, action: ReadingAction, time: ReadingTime) -> NativeReading {
    NativeReading::new(NativeContact(contact), PointerKind::Touch, action, time)
        .at(1.0, 1.0, 1.0)
        .from_device(NativeDevice(1))
}

fn clocks_rebase_wrap_and_stay_monotonic() {
    let clock = counter_clock();
    let tick = |tick, observed_ms| {
        clock
            .stamp(
                ReadingTime::coarse(NativeTime::TickMillis { tick, observed_ms }),
                flui_platform::shared::pointer_assembly::ClockChoice::Coarse,
            )
            .expect("valid")
            .as_nanos()
    };
    assert_eq!(
        tick(5_100, 5_200),
        1_100_000_000,
        "a queued tick before arrival"
    );
    let wrap = u64::from(u32::MAX) + 1;
    let wrapped = NativeClock::new(
        EventTime::from_nanos(1_000_000),
        Duration::from_millis(wrap - 10),
        None,
    );
    assert_eq!(
        wrapped
            .stamp(
                ReadingTime::coarse(NativeTime::TickMillis {
                    tick: 7,
                    observed_ms: wrap + 30
                }),
                flui_platform::shared::pointer_assembly::ClockChoice::Coarse,
            )
            .expect("valid")
            .as_nanos(),
        18_000_000,
        "a tick past the 32-bit wrap"
    );
    assert_eq!(
        tick(5_201, 5_200),
        1_201_000_000,
        "a tick ahead of the coarse reference"
    );
    let counter = |count| ReadingTime {
        coarse: NativeTime::TickMillis {
            tick: 5_000,
            observed_ms: 5_000,
        },
        counter: Some(QpcTicks(count)),
    };
    let mut contacts = ContactRegistry::new(clock);
    assert_eq!(
        stamped(&mut contacts, &timed(1, PRESS, counter(1_000_010))),
        1_000_001_000,
        "counted"
    );
    assert_eq!(
        stamped(
            &mut contacts,
            &timed(1, ReadingAction::Move, counter(999_990))
        ),
        1_000_001_000,
        "a sequence never runs backwards"
    );
    assert_eq!(
        stamped(
            &mut contacts,
            &timed(
                1,
                ReadingAction::Move,
                ReadingTime::coarse(NativeTime::TickMillis {
                    tick: 5_000,
                    observed_ms: 5_000
                })
            )
        ),
        1_000_001_000,
        "a coarse reading lagging the counter is held at the sequence's last stamp"
    );
    stamped(&mut contacts, &timed(1, RELEASE, counter(1_000_020)));
    assert_eq!(
        stamped(&mut contacts, &timed(2, PRESS, counter(1_000_030))),
        1_000_000_000,
        "a device that once lacked the counter stays on its coarse stamp"
    );
    let mut fresh = ContactRegistry::new(clock);
    assert_eq!(
        stamped(&mut fresh, &timed(1, PRESS, counter(500_000))),
        950_000_000,
        "a count before the epoch"
    );
    assert_eq!(
        stamped(
            &mut fresh,
            &timed(1, ReadingAction::Move, counter(u64::MAX))
        ),
        u64::MAX,
        "saturates past the range"
    );
    let apple = NativeClock::new(
        EventTime::from_nanos(2_000_000_000),
        Duration::from_secs(100),
        None,
    );
    assert_eq!(
        apple
            .stamp(
                ReadingTime::coarse(NativeTime::UptimeSeconds(99.5)),
                flui_platform::shared::pointer_assembly::ClockChoice::Coarse
            )
            .expect("valid")
            .as_nanos(),
        1_500_000_000
    );
    assert!(
        apple
            .stamp(
                ReadingTime::coarse(NativeTime::UptimeSeconds(f64::NAN)),
                flui_platform::shared::pointer_assembly::ClockChoice::Coarse
            )
            .is_none()
    );
}

#[test]
fn pointer_assembly_contracts() {
    super::run_table(
        "pointer_assembly",
        &[
            (
                "a_released_label_starts_a_new_contact",
                a_released_label_starts_a_new_contact,
            ),
            (
                "pan_zoom_carries_only_scale_and_rotation",
                pan_zoom_carries_only_scale_and_rotation,
            ),
            (
                "two_finger_pan_is_phased_scroll_on_every_backend",
                two_finger_pan_is_phased_scroll_on_every_backend,
            ),
            (
                "a_second_finger_is_additional_until_the_first_lifts",
                a_second_finger_is_additional_until_the_first_lifts,
            ),
            (
                "a_mouse_keeps_its_identity_across_presses",
                a_mouse_keeps_its_identity_across_presses,
            ),
            (
                "a_pen_sequence_is_its_contact_not_its_barrel_button",
                a_pen_sequence_is_its_contact_not_its_barrel_button,
            ),
            (
                "a_lifted_pen_hovers_on_its_identity_until_it_leaves",
                a_lifted_pen_hovers_on_its_identity_until_it_leaves,
            ),
            (
                "the_originating_reading_lives_for_its_sequence",
                the_originating_reading_lives_for_its_sequence,
            ),
            (
                "a_hot_plugged_device_is_announced_and_retired",
                a_hot_plugged_device_is_announced_and_retired,
            ),
            (
                "exhausted_identities_refuse_instead_of_wrapping",
                exhausted_identities_refuse_instead_of_wrapping,
            ),
            (
                "clicks_count_within_the_os_double_click_interval",
                clicks_count_within_the_os_double_click_interval,
            ),
            (
                "pinch_and_rotation_accumulate_into_one_gesture",
                pinch_and_rotation_accumulate_into_one_gesture,
            ),
            (
                "a_batch_attaches_its_readings_in_order",
                a_batch_attaches_its_readings_in_order,
            ),
            (
                "an_undecodable_reading_cancels_its_contact_as_invalid_input",
                an_undecodable_reading_cancels_its_contact_as_invalid_input,
            ),
            (
                "an_orphan_reading_emits_nothing",
                an_orphan_reading_emits_nothing,
            ),
            (
                "a_press_without_any_position_is_suppressed_with_its_release",
                a_press_without_any_position_is_suppressed_with_its_release,
            ),
            (
                "clocks_rebase_wrap_and_stay_monotonic",
                clocks_rebase_wrap_and_stay_monotonic,
            ),
        ],
    );
}
