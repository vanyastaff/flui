//! Settings are admitted by contacts and remain stable until terminal delivery.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use flui_foundation::geometry::{DevicePixelRatio, Point, Size};
use flui_interaction::{
    DoubleTapGestureRecognizer, DragAxis, DragGestureRecognizer, EagerGestureRecognizer,
    ForcePressGestureRecognizer, GestureArena, GestureArenaMember, GestureRecognizer,
    GestureSettings, GestureSettingsSource, LongPressGestureRecognizer, ManualClock,
    MonotonicClock, MultiTapGestureRecognizer, PointerId, ScaleGestureRecognizer,
    TapAndDragGestureRecognizer, TapGestureRecognizer,
    arena::run_pointer_lifecycle,
    events::{PointerEvent, PointerEventExt, PointerKind},
    routing::PointerDispatch,
};
use flui_platform_api::{
    EventTime, GestureGeometry, GesturePreferences,
    pointer::{
        PointerButton, PointerButtons, PointerInfo, PointerMove, PointerPosition, PointerPress,
        PointerRelease, PointerSample, Pressure,
    },
};

fn sample(millis: u64, x: f64, y: f64) -> PointerSample {
    PointerSample::new(
        EventTime::from_nanos(millis * 1_000_000),
        PointerPosition::try_new(Point::new(x, y)).expect("finite fixture"),
    )
}

fn event(id: u64, kind: PointerKind, millis: u64, x: f64, y: f64, phase: u8) -> PointerEvent {
    let info = PointerInfo::new(PointerId::try_from(id).expect("nonzero fixture"), kind);
    let buttons = PointerButtons::NONE.with(PointerButton::PRIMARY);
    let sample = sample(millis, x, y);
    match phase {
        0 => PointerEvent::Down(PointerPress::new(
            info,
            PointerButton::PRIMARY,
            buttons,
            sample,
        )),
        1 => PointerEvent::Move(PointerMove::new(info, buttons, sample)),
        2 => PointerEvent::Up(PointerRelease::new(
            info,
            PointerButton::PRIMARY,
            PointerButtons::NONE,
            sample,
        )),
        _ => unreachable!("fixture phase"),
    }
}

fn send(recognizer: &dyn GestureRecognizer, arena: &GestureArena, event: &PointerEvent) {
    if matches!(event, PointerEvent::Down(_)) {
        recognizer.add_pointer(PointerDispatch::at_root(event));
    } else {
        recognizer.handle_event(PointerDispatch::at_root(event));
    }
    run_pointer_lifecycle(arena, event);
    arena.drain_deferred_resolutions();
}

struct Rival;
impl GestureArenaMember for Rival {
    fn accept_gesture(&self, _: PointerId) {}
    fn reject_gesture(&self, _: PointerId) {}
}

fn contested_down(
    recognizer: &dyn GestureRecognizer,
    arena: &GestureArena,
    event: &PointerEvent,
    rival: &Rc<Rival>,
) {
    recognizer.add_pointer(PointerDispatch::at_root(event));
    let pointer = event.pointer_id().expect("contact event");
    arena.add(pointer, rival);
    arena.close(pointer);
    arena.drain_deferred_resolutions();
}

fn multi_tap_retains_whole_attempt_policy() {
    let arena = GestureArena::new();
    let old = GestureSettings::touch_defaults();
    let source = GestureSettingsSource::new(old.clone());
    let taps = Rc::new(Cell::new(0));
    let output = taps.clone();
    let tap = MultiTapGestureRecognizer::builder(arena.clone(), 2)
        .settings(source.provider())
        .on_multi_tap(move |_| output.set(output.get() + 1))
        .build();
    for (a, expected) in [(40, 1), (42, 1), (44, 2)] {
        send(
            &*tap,
            &arena,
            &event(a, PointerKind::Touch, a * 100, 0.0, 0.0, 0),
        );
        if a == 40 {
            source.replace(old.clone().try_with_touch_slop(2.0).expect("valid slop"));
        }
        send(
            &*tap,
            &arena,
            &event(a + 1, PointerKind::Touch, a * 100 + 1, 100.0, 0.0, 0),
        );
        send(
            &*tap,
            &arena,
            &event(a + 1, PointerKind::Touch, a * 100 + 10, 105.0, 0.0, 1),
        );
        send(
            &*tap,
            &arena,
            &event(a, PointerKind::Touch, a * 100 + 20, 0.0, 0.0, 2),
        );
        send(
            &*tap,
            &arena,
            &event(a + 1, PointerKind::Touch, a * 100 + 21, 105.0, 0.0, 2),
        );
        assert_eq!(taps.get(), expected, "attempt beginning {a}");
        if a == 42 {
            source.replace(old.clone());
        }
    }
}

fn scale_retains_first_contact_profile_until_last_release() {
    let arena = GestureArena::new();
    let old = GestureSettings::touch_defaults();
    let source = GestureSettingsSource::new(old.clone());
    let starts = Rc::new(Cell::new(0));
    let output = starts.clone();
    let scale = ScaleGestureRecognizer::builder(arena.clone())
        .start_mode(flui_interaction::recognizers::scale::ScaleStartMode::PanOrScale)
        .settings(source.provider())
        .on_start(move |_| output.set(output.get() + 1))
        .build();
    let rival = Rc::new(Rival);
    contested_down(
        &*scale,
        &arena,
        &event(50, PointerKind::Touch, 0, 0.0, 0.0, 0),
        &rival,
    );
    source.replace(old.clone().try_with_pan_slop(2.0).expect("valid slop"));
    contested_down(
        &*scale,
        &arena,
        &event(51, PointerKind::Touch, 1, 1000.0, 0.0, 0),
        &rival,
    );
    send(
        &*scale,
        &arena,
        &event(50, PointerKind::Touch, 10, 5.0, 0.0, 1),
    );
    send(
        &*scale,
        &arena,
        &event(51, PointerKind::Touch, 11, 1005.0, 0.0, 1),
    );
    assert_eq!(
        starts.get(),
        0,
        "second contact inherits the session profile"
    );
    send(
        &*scale,
        &arena,
        &event(51, PointerKind::Touch, 20, 1005.0, 0.0, 2),
    );
    send(
        &*scale,
        &arena,
        &event(50, PointerKind::Touch, 21, 10.0, 0.0, 1),
    );
    assert_eq!(
        starts.get(),
        0,
        "remaining contact retains the session profile"
    );
    scale.cancel();
    contested_down(
        &*scale,
        &arena,
        &event(52, PointerKind::Touch, 30, 0.0, 0.0, 0),
        &rival,
    );
    send(
        &*scale,
        &arena,
        &event(52, PointerKind::Touch, 40, 5.0, 0.0, 1),
    );
    assert_eq!(starts.get(), 1, "new session observes replacement");
    scale.cancel();
    source.replace(old);
    contested_down(
        &*scale,
        &arena,
        &event(53, PointerKind::Touch, 50, 0.0, 0.0, 0),
        &rival,
    );
    send(
        &*scale,
        &arena,
        &event(53, PointerKind::Touch, 60, 5.0, 0.0, 1),
    );
    assert_eq!(starts.get(), 1, "restored session waits again");
    scale.cancel();
}

fn double_tap_candidate_retains_policy() {
    let clock = Arc::new(ManualClock::new());
    let arena = GestureArena::with_clock(clock.clone());
    let old = GestureSettings::touch_defaults();
    let source = GestureSettingsSource::new(old.clone());
    let doubles = Rc::new(Cell::new(0));
    let output = doubles.clone();
    let double = DoubleTapGestureRecognizer::builder(arena.clone())
        .settings(source.provider())
        .on_double_tap(move |_| output.set(output.get() + 1))
        .build();
    send(
        &*double,
        &arena,
        &event(60, PointerKind::Touch, 0, 0.0, 0.0, 0),
    );
    send(
        &*double,
        &arena,
        &event(60, PointerKind::Touch, 1, 0.0, 0.0, 2),
    );
    source.replace(
        old.clone()
            .with_double_tap_timeout(Duration::from_millis(60))
            .try_with_touch_slop(2.0)
            .expect("valid slop"),
    );
    clock.advance(Duration::from_millis(100));
    send(
        &*double,
        &arena,
        &event(61, PointerKind::Touch, 100, 0.0, 0.0, 0),
    );
    send(
        &*double,
        &arena,
        &event(61, PointerKind::Touch, 110, 5.0, 0.0, 1),
    );
    send(
        &*double,
        &arena,
        &event(61, PointerKind::Touch, 120, 5.0, 0.0, 2),
    );
    assert_eq!(
        doubles.get(),
        1,
        "candidate and second contact retain old policy"
    );
    clock.advance(Duration::from_millis(400));
    for id in [62, 63] {
        send(
            &*double,
            &arena,
            &event(id, PointerKind::Touch, id * 100, 0.0, 0.0, 0),
        );
        send(
            &*double,
            &arena,
            &event(id, PointerKind::Touch, id * 100 + 1, 0.0, 0.0, 2),
        );
        clock.advance(Duration::from_millis(100));
    }
    assert_eq!(doubles.get(), 1, "new candidate uses shorter interval");
    double.cancel();
    source.replace(old);
    for id in [64, 65] {
        send(
            &*double,
            &arena,
            &event(id, PointerKind::Touch, id * 100, 0.0, 0.0, 0),
        );
        send(
            &*double,
            &arena,
            &event(id, PointerKind::Touch, id * 100 + 1, 0.0, 0.0, 2),
        );
        clock.advance(Duration::from_millis(100));
    }
    assert_eq!(
        doubles.get(),
        2,
        "restored candidate accepts interval again"
    );
}

fn tap_and_drag_retains_consecutive_candidate_profile() {
    let clock = Arc::new(ManualClock::new());
    let arena = GestureArena::with_clock(clock.clone());
    let old = GestureSettings::touch_defaults();
    let source = GestureSettingsSource::new(old.clone());
    let counts = Rc::new(RefCell::new(Vec::new()));
    let output = counts.clone();
    let tap = TapAndDragGestureRecognizer::builder(arena.clone())
        .settings(source.provider())
        .on_tap_up(move |details| output.borrow_mut().push(details.consecutive_tap_count))
        .build();
    for id in [70, 71, 72, 73] {
        send(
            &*tap,
            &arena,
            &event(id, PointerKind::Touch, id * 100, 0.0, 0.0, 0),
        );
        send(
            &*tap,
            &arena,
            &event(id, PointerKind::Touch, id * 100 + 1, 0.0, 0.0, 2),
        );
        if id == 70 {
            source.replace(
                old.clone()
                    .with_double_tap_timeout(Duration::from_millis(60)),
            );
        }
        clock.advance(Duration::from_millis(if id == 71 { 400 } else { 100 }));
    }
    assert_eq!(
        &*counts.borrow(),
        &[1, 2, 1, 1],
        "old candidate retains interval; new chain uses replacement"
    );
}

fn eager_source_replacement_does_not_retire_accepted_contact() {
    let arena = GestureArena::new();
    let source = GestureSettingsSource::new(GestureSettings::touch_defaults());
    let eager = EagerGestureRecognizer::builder(arena.clone())
        .settings(source.provider())
        .build();
    eager.add_pointer(PointerDispatch::at_root(&event(
        80,
        PointerKind::Touch,
        0,
        0.0,
        0.0,
        0,
    )));
    source.replace(GestureSettings::mouse_defaults());
    eager.add_pointer(PointerDispatch::at_root(&event(
        81,
        PointerKind::Touch,
        1,
        0.0,
        0.0,
        0,
    )));
    assert!(
        !arena.contains(PointerId::try_from(81).expect("nonzero fixture")),
        "active eager owner stays busy"
    );
    eager.cancel();
    eager.add_pointer(PointerDispatch::at_root(&event(
        81,
        PointerKind::Touch,
        2,
        0.0,
        0.0,
        0,
    )));
    assert!(
        arena.contains(PointerId::try_from(81).expect("nonzero fixture")),
        "next contact remains admissible"
    );
    eager.cancel();
}

fn independent_multidrags_snapshot_each_admission() {
    use flui_interaction::recognizers::{MultiDragAxis, MultiDragGestureRecognizer};
    let arena = GestureArena::new();
    let source = GestureSettingsSource::new(GestureSettings::touch_defaults());
    let started = Rc::new(RefCell::new(Vec::new()));
    let output = started.clone();
    let drag = MultiDragGestureRecognizer::builder(arena.clone(), MultiDragAxis::Free)
        .settings(source.provider())
        .on_start(move |pointer, _| {
            output.borrow_mut().push(pointer);
            None
        })
        .build();
    let rival = Rc::new(Rival);
    contested_down(
        &*drag,
        &arena,
        &event(90, PointerKind::Touch, 0, 0.0, 0.0, 0),
        &rival,
    );
    source.replace(
        GestureSettings::touch_defaults()
            .try_with_touch_slop(2.0)
            .expect("valid slop"),
    );
    contested_down(
        &*drag,
        &arena,
        &event(91, PointerKind::Touch, 1, 0.0, 0.0, 0),
        &rival,
    );
    send(
        &*drag,
        &arena,
        &event(90, PointerKind::Touch, 10, 5.0, 0.0, 1),
    );
    send(
        &*drag,
        &arena,
        &event(91, PointerKind::Touch, 11, 5.0, 0.0, 1),
    );
    assert_eq!(
        &*started.borrow(),
        &[PointerId::try_from(91).expect("nonzero")]
    );
    send(
        &*drag,
        &arena,
        &event(90, PointerKind::Touch, 20, 20.0, 0.0, 1),
    );
    assert_eq!(
        started.borrow().len(),
        2,
        "old contact keeps its larger threshold"
    );
    source.replace(GestureSettings::touch_defaults());
    contested_down(
        &*drag,
        &arena,
        &event(92, PointerKind::Touch, 30, 0.0, 0.0, 0),
        &rival,
    );
    send(
        &*drag,
        &arena,
        &event(92, PointerKind::Touch, 40, 5.0, 0.0, 1),
    );
    assert_eq!(started.borrow().len(), 2, "restored contact waits again");
    drag.cancel();
}

fn consecutive_candidates_do_not_cross_pointer_kinds() {
    let clock = Arc::new(ManualClock::new());
    let arena = GestureArena::with_clock(clock.clone());
    let doubles = Rc::new(Cell::new(0));
    let output = doubles.clone();
    let double = DoubleTapGestureRecognizer::builder(arena.clone())
        .on_double_tap(move |_| output.set(output.get() + 1))
        .build();
    for (id, kind) in [(100, PointerKind::Touch), (101, PointerKind::Mouse)] {
        send(&*double, &arena, &event(id, kind, id * 100, 0.0, 0.0, 0));
        send(
            &*double,
            &arena,
            &event(id, kind, id * 100 + 1, 0.0, 0.0, 2),
        );
        clock.advance(Duration::from_millis(100));
    }
    assert_eq!(
        doubles.get(),
        0,
        "touch candidate cannot become mouse double-click"
    );
    double.cancel();
    for id in [102, 103] {
        send(
            &*double,
            &arena,
            &event(id, PointerKind::Touch, id * 100, 0.0, 0.0, 0),
        );
        send(
            &*double,
            &arena,
            &event(id, PointerKind::Touch, id * 100 + 1, 0.0, 0.0, 2),
        );
        clock.advance(Duration::from_millis(100));
    }
    assert_eq!(
        doubles.get(),
        1,
        "different touch IDs remain a valid consecutive pair"
    );
    let counts = Rc::new(RefCell::new(Vec::new()));
    let output = counts.clone();
    let tap = TapAndDragGestureRecognizer::builder(arena.clone())
        .on_tap_up(move |details| output.borrow_mut().push(details.consecutive_tap_count))
        .build();
    for (id, kind) in [
        (104, PointerKind::Touch),
        (105, PointerKind::Mouse),
        (106, PointerKind::Touch),
        (107, PointerKind::Touch),
    ] {
        send(&*tap, &arena, &event(id, kind, id * 100, 0.0, 0.0, 0));
        send(&*tap, &arena, &event(id, kind, id * 100 + 1, 0.0, 0.0, 2));
        clock.advance(Duration::from_millis(100));
    }
    assert_eq!(&*counts.borrow(), &[1, 1, 1, 2]);
}

fn consecutive_candidates_compare_device_identity() {
    use flui_platform_api::pointer::DeviceId;
    for (first, second, expected) in [
        (Some(9), Some(10), 0),
        (Some(9), None, 0),
        (Some(9), Some(9), 1),
        (None, None, 1),
    ] {
        let clock = Arc::new(ManualClock::new());
        let arena = GestureArena::with_clock(clock.clone());
        let doubles = Rc::new(Cell::new(0));
        let output = doubles.clone();
        let double = DoubleTapGestureRecognizer::builder(arena.clone())
            .on_double_tap(move |_| output.set(output.get() + 1))
            .build();
        for (id, device) in [(130, first), (131, second)] {
            for phase in [0, 2] {
                let mut sample = event(
                    id,
                    PointerKind::Touch,
                    id * 100 + u64::from(phase),
                    0.0,
                    0.0,
                    phase,
                );
                let device =
                    device.map(|raw| DeviceId::try_from(raw as u64).expect("nonzero device"));
                match &mut sample {
                    PointerEvent::Down(data) => data.pointer.device = device,
                    PointerEvent::Up(data) => data.pointer.device = device,
                    _ => unreachable!("fixture phase"),
                }
                send(&*double, &arena, &sample);
            }
            clock.advance(Duration::from_millis(100));
        }
        assert_eq!(
            doubles.get(),
            expected,
            "device transition {first:?} -> {second:?}"
        );
        double.cancel();
        let counts = Rc::new(RefCell::new(Vec::new()));
        let output = counts.clone();
        let tap = TapAndDragGestureRecognizer::builder(arena.clone())
            .on_tap_up(move |details| output.borrow_mut().push(details.consecutive_tap_count))
            .build();
        for (id, device) in [(132, first), (133, second)] {
            for phase in [0, 2] {
                let mut sample = event(
                    id,
                    PointerKind::Touch,
                    id * 100 + u64::from(phase),
                    0.0,
                    0.0,
                    phase,
                );
                let device =
                    device.map(|raw| DeviceId::try_from(raw as u64).expect("nonzero device"));
                match &mut sample {
                    PointerEvent::Down(data) => data.pointer.device = device,
                    PointerEvent::Up(data) => data.pointer.device = device,
                    _ => unreachable!("fixture phase"),
                }
                send(&*tap, &arena, &sample);
            }
            clock.advance(Duration::from_millis(100));
        }
        assert_eq!(
            &*counts.borrow(),
            &[1, expected + 1],
            "tap-and-drag device transition {first:?} -> {second:?}"
        );
    }
}

fn native_mouse_interval_starts_at_first_down() {
    for (kind, native, expected) in [
        (PointerKind::Mouse, true, 0),
        (PointerKind::Mouse, false, 1),
        (PointerKind::Touch, true, 1),
    ] {
        let clock = Arc::new(ManualClock::new());
        let arena = GestureArena::with_clock(clock.clone());
        let baseline =
            GestureSettings::touch_defaults().with_double_tap_timeout(Duration::from_millis(300));
        let settings = if native {
            GestureSettings::resolve_preferences(
                &baseline,
                &GesturePreferences::default()
                    .with_double_click_interval(Duration::from_millis(300))
                    .with_double_tap_interval(Duration::from_millis(300)),
                None,
            )
            .expect("valid projection")
        } else {
            baseline
        };
        let doubles = Rc::new(Cell::new(0));
        let output = doubles.clone();
        let double = DoubleTapGestureRecognizer::builder(arena.clone())
            .settings(settings)
            .on_double_tap(move |_| output.set(output.get() + 1))
            .build();
        send(&*double, &arena, &event(110, kind, 0, 0.0, 0.0, 0));
        clock.advance(Duration::from_millis(250));
        send(&*double, &arena, &event(110, kind, 250, 0.0, 0.0, 2));
        clock.advance(Duration::from_millis(100));
        send(&*double, &arena, &event(111, kind, 350, 0.0, 0.0, 0));
        send(&*double, &arena, &event(111, kind, 360, 0.0, 0.0, 2));
        assert_eq!(doubles.get(), expected, "{kind:?}, native={native}");
        double.cancel();
    }
}

fn native_double_click_full_area_is_halved_per_axis() {
    for (x, y, expected) in [(3.0, 4.0, 1), (3.01, 0.0, 0), (0.0, 4.01, 0)] {
        let clock = Arc::new(ManualClock::new());
        let arena = GestureArena::with_clock(clock.clone());
        let geometry = GestureGeometry::new(DevicePixelRatio::ONE)
            .with_mouse_double_click_area(Size::new(6.0, 8.0))
            .expect("valid full area");
        let settings = GestureSettings::resolve_preferences(
            &GestureSettings::touch_defaults(),
            &GesturePreferences::default(),
            Some(&geometry),
        )
        .expect("representable area");
        let doubles = Rc::new(Cell::new(0));
        let output = doubles.clone();
        let double = DoubleTapGestureRecognizer::builder(arena.clone())
            .settings(settings)
            .on_double_tap(move |_| output.set(output.get() + 1))
            .build();
        send(
            &*double,
            &arena,
            &event(160, PointerKind::Mouse, 0, 0.0, 0.0, 0),
        );
        send(
            &*double,
            &arena,
            &event(160, PointerKind::Mouse, 1, 0.0, 0.0, 2),
        );
        clock.advance(Duration::from_millis(100));
        send(
            &*double,
            &arena,
            &event(161, PointerKind::Mouse, 100, x, y, 0),
        );
        send(
            &*double,
            &arena,
            &event(161, PointerKind::Mouse, 101, x, y, 2),
        );
        assert_eq!(doubles.get(), expected, "full-area displacement ({x}, {y})");
        double.cancel();
    }
}

fn native_touch_projection_preserves_authored_tier_ratios() {
    use flui_platform_api::Distance;
    let baseline = GestureSettings::touch_defaults()
        .try_with_touch_slop(10.0)
        .expect("valid hit tier")
        .try_with_pan_slop(30.0)
        .expect("valid free tier")
        .try_with_pan_slop_horizontal(40.0)
        .expect("valid horizontal tier")
        .try_with_pan_slop_vertical(50.0)
        .expect("valid vertical tier");
    for (baseline, observed, axis, boundary) in [
        (baseline.clone(), 4.0, DragAxis::Free, 12.0),
        (baseline.clone(), 4.0, DragAxis::Horizontal, 16.0),
        (baseline.clone(), 4.0, DragAxis::Vertical, 20.0),
        (
            baseline
                .clone()
                .try_with_touch_slop(0.0)
                .expect("valid zero"),
            4.0,
            DragAxis::Free,
            30.0,
        ),
        (
            baseline
                .clone()
                .try_with_touch_slop(1e200)
                .expect("valid hit")
                .try_with_pan_slop(1e250)
                .expect("valid pan"),
            1e-100,
            DragAxis::Free,
            1e-50,
        ),
    ] {
        let geometry = GestureGeometry::new(DevicePixelRatio::ONE)
            .with_touch_slop(Distance::new(observed).expect("valid observation"));
        let settings = GestureSettings::resolve_preferences(
            &baseline,
            &GesturePreferences::default(),
            Some(&geometry),
        )
        .expect("representable native projection");
        let arena = GestureArena::new();
        let starts = Rc::new(Cell::new(0));
        let output = starts.clone();
        let drag = DragGestureRecognizer::builder(arena.clone(), axis)
            .settings(settings)
            .on_start(move |_| output.set(output.get() + 1))
            .build();
        let rival = Rc::new(Rival);
        contested_down(
            &*drag,
            &arena,
            &event(120, PointerKind::Touch, 0, 0.0, 0.0, 0),
            &rival,
        );
        let position = |distance| {
            if axis == DragAxis::Vertical {
                (0.0, distance)
            } else {
                (distance, 0.0)
            }
        };
        let (x, y) = position(boundary * 0.99);
        send(&*drag, &arena, &event(120, PointerKind::Touch, 10, x, y, 1));
        assert_eq!(
            starts.get(),
            0,
            "retains projected {axis:?} tier {boundary}"
        );
        let (x, y) = position(boundary * 1.01);
        send(&*drag, &arena, &event(120, PointerKind::Touch, 20, x, y, 1));
        assert_eq!(
            starts.get(),
            1,
            "crosses projected {axis:?} tier {boundary}"
        );
        drag.cancel();
    }
    let unrepresentable = baseline
        .try_with_touch_slop(1.0)
        .expect("valid hit")
        .try_with_pan_slop(f64::MAX)
        .expect("valid authored tier");
    let geometry = GestureGeometry::new(DevicePixelRatio::ONE)
        .with_touch_slop(Distance::new(2.0).expect("valid observation"));
    assert!(
        matches!(
            GestureSettings::resolve_preferences(
                &unrepresentable,
                &GesturePreferences::default(),
                Some(&geometry)
            ),
            Err(flui_interaction::GestureSettingsError::UnrepresentableProjection { .. })
        ),
        "unrepresentable derived tier is explicitly recoverable"
    );
    let baseline = GestureSettings::touch_defaults()
        .try_with_touch_slop(1e200)
        .expect("valid hit")
        .try_with_pan_slop(1e-200)
        .expect("valid authored tier");
    let geometry = GestureGeometry::new(DevicePixelRatio::ONE)
        .with_touch_slop(Distance::new(1e-200).expect("valid observation"));
    assert!(
        matches!(
            GestureSettings::resolve_preferences(
                &baseline,
                &GesturePreferences::default(),
                Some(&geometry)
            ),
            Err(flui_interaction::GestureSettingsError::UnrepresentableProjection { .. })
        ),
        "positive unrepresentable projection cannot silently become zero"
    );
}

fn long_press_retains_deadline_and_next_contact_observes_replacement() {
    let clock = Arc::new(ManualClock::new());
    let arena = GestureArena::binding_driven(clock.clone());
    let old = GestureSettings::touch_defaults().with_long_press_timeout(Duration::from_millis(500));
    let source = GestureSettingsSource::new(old.clone());
    let starts = Rc::new(Cell::new(0));
    let output = starts.clone();
    let press = LongPressGestureRecognizer::builder(arena.clone())
        .settings(source.provider())
        .on_long_press(move || output.set(output.get() + 1))
        .build();
    send(
        &*press,
        &arena,
        &event(1, PointerKind::Touch, 0, 0.0, 0.0, 0),
    );
    source.replace(
        old.clone()
            .with_long_press_timeout(Duration::from_millis(100)),
    );
    clock.advance(Duration::from_millis(250));
    press.poll_deadline(clock.now());
    assert_eq!(starts.get(), 0, "active contact retains 500ms");
    clock.advance(Duration::from_millis(251));
    press.poll_deadline(clock.now());
    assert_eq!(starts.get(), 1);
    send(
        &*press,
        &arena,
        &event(1, PointerKind::Touch, 501, 0.0, 0.0, 2),
    );
    send(
        &*press,
        &arena,
        &event(2, PointerKind::Touch, 501, 0.0, 0.0, 0),
    );
    clock.advance(Duration::from_millis(101));
    press.poll_deadline(clock.now());
    assert_eq!(starts.get(), 2, "next contact observes 100ms");
    send(
        &*press,
        &arena,
        &event(2, PointerKind::Touch, 602, 0.0, 0.0, 2),
    );
    source.replace(old);
    send(
        &*press,
        &arena,
        &event(3, PointerKind::Touch, 602, 0.0, 0.0, 0),
    );
    clock.advance(Duration::from_millis(101));
    press.poll_deadline(clock.now());
    assert_eq!(starts.get(), 2, "restored contact waits again");
    press.cancel();
}

fn terminal_fling_retains_profile_and_preserves_measured_velocity() {
    use flui_interaction::processing::VelocityEstimator;
    for estimator in [
        VelocityEstimator::LeastSquares,
        VelocityEstimator::Impulse,
        VelocityEstimator::Ios,
        VelocityEstimator::Macos,
    ] {
        let arena = GestureArena::binding_driven(Arc::new(ManualClock::new()));
        let old = GestureSettings::touch_defaults()
            .with_velocity_estimator(estimator)
            .try_with_fling_velocity(50.0, 15000.0)
            .expect("valid range");
        let source = GestureSettingsSource::new(old.clone());
        let ends = Rc::new(RefCell::new(Vec::new()));
        let output = ends.clone();
        let drag = DragGestureRecognizer::builder(arena.clone(), DragAxis::Free)
            .settings(source.provider())
            .on_end(move |details| output.borrow_mut().push(details))
            .build();
        for (id, expected) in [(10, 15000.0), (11, 5000.0), (12, 15000.0)] {
            send(
                &*drag,
                &arena,
                &event(id, PointerKind::Touch, id * 100, 0.0, 0.0, 0),
            );
            if id == 10 {
                source.replace(
                    old.clone()
                        .try_with_fling_velocity(50.0, 5000.0)
                        .expect("valid range"),
                );
            }
            for (dt, x, phase) in [
                (10, 200.0, 1),
                (20, 400.0, 1),
                (30, 600.0, 1),
                (40, 800.0, 2),
            ] {
                send(
                    &*drag,
                    &arena,
                    &event(id, PointerKind::Touch, id * 100 + dt, x, 0.0, phase),
                );
            }
            let details = ends.borrow().last().cloned().expect("terminal callback");
            assert!(
                (details.velocity.pixels_per_second.dx - 20000.0).abs() < 1.0,
                "measurement is independent of fling policy: {details:?}"
            );
            assert!(
                (details.fling_velocity().pixels_per_second.dx - expected).abs() < 1.0,
                "admitted policy, including values above 8000: {details:?}"
            );
            if id == 11 {
                source.replace(old.clone());
            }
        }
    }
}

fn mouse_rectangle_is_compared_per_axis() {
    for (width, height, positions) in [
        (3.0, 4.0, [(3.0, 4.0), (3.01, 0.0), (0.0, 4.01), (2.5, 3.5)]),
        (
            0.0,
            4.0,
            [
                (0.0, 4.0),
                (f64::MIN_POSITIVE, 0.0),
                (0.0, 4.01),
                (0.0, 3.5),
            ],
        ),
        (
            0.25,
            0.5,
            [(0.25, 0.5), (0.251, 0.0), (0.0, 0.501), (0.2, 0.4)],
        ),
    ] {
        let baseline = GestureSettings::touch_defaults();
        let geometry = GestureGeometry::new(DevicePixelRatio::ONE)
            .with_mouse_drag_tolerance(Size::new(width, height))
            .expect("valid rectangle");
        let settings = GestureSettings::resolve_preferences(
            &baseline,
            &GesturePreferences::default(),
            Some(&geometry),
        )
        .expect("representable projection");
        let arena = GestureArena::new();
        let taps = Rc::new(Cell::new(0));
        let output = taps.clone();
        let tap = TapGestureRecognizer::builder(arena.clone())
            .settings(settings)
            .on_tap(move |_| output.set(output.get() + 1))
            .build();
        for ((id, expected), (x, y)) in [(20, 1), (21, 1), (22, 1), (23, 2)]
            .into_iter()
            .zip(positions)
        {
            send(
                &*tap,
                &arena,
                &event(id, PointerKind::Mouse, id * 100, 0.0, 0.0, 0),
            );
            send(
                &*tap,
                &arena,
                &event(id, PointerKind::Mouse, id * 100 + 10, x, y, 1),
            );
            send(
                &*tap,
                &arena,
                &event(id, PointerKind::Mouse, id * 100 + 20, x, y, 2),
            );
            assert_eq!(taps.get(), expected, "mouse displacement ({x}, {y})");
        }
    }
}

fn drag_continuation_retains_group_profile_on_handoff() {
    use flui_interaction::recognizers::drag::DragPointerStrategy;
    let arena = GestureArena::binding_driven(Arc::new(ManualClock::new()));
    let old = GestureSettings::touch_defaults()
        .try_with_fling_velocity(50.0, 15000.0)
        .expect("valid range");
    let source = GestureSettingsSource::new(old.clone());
    let ends = Rc::new(RefCell::new(Vec::new()));
    let output = ends.clone();
    let drag = DragGestureRecognizer::builder(arena.clone(), DragAxis::Horizontal)
        .pointer_strategy(DragPointerStrategy::ContinueWithRemaining)
        .settings(source.provider())
        .on_end(move |details| output.borrow_mut().push(details))
        .build();
    send(
        &*drag,
        &arena,
        &event(140, PointerKind::Touch, 0, 0.0, 0.0, 0),
    );
    send(
        &*drag,
        &arena,
        &event(140, PointerKind::Touch, 10, 200.0, 0.0, 1),
    );
    source.replace(
        old.try_with_fling_velocity(50.0, 5000.0)
            .expect("valid range"),
    );
    for (id, time, x, phase) in [
        (141, 15, 1000.0, 0),
        (141, 20, 1100.0, 1),
        (141, 25, 1200.0, 1),
        (141, 30, 1300.0, 1),
        (140, 35, 200.0, 2),
        (141, 40, 1500.0, 1),
        (141, 45, 1600.0, 2),
    ] {
        send(
            &*drag,
            &arena,
            &event(id, PointerKind::Touch, time, x, 0.0, phase),
        );
    }
    let ends = ends.borrow();
    assert_eq!(ends.len(), 1, "handoff keeps one terminal group");
    assert!(
        (ends[0].velocity.pixels_per_second.dx - 20000.0).abs() < 1.0,
        "successor keeps independent measured trajectory"
    );
    assert!(
        (ends[0].fling_velocity().pixels_per_second.dx - 15000.0).abs() < 1.0,
        "joining contact inherits group's admitted fling policy"
    );
}

fn terminal_fling_handles_diagonal_slow_zero_and_extreme_measurements() {
    use flui_interaction::processing::VelocityEstimator;
    for estimator in [
        VelocityEstimator::LeastSquares,
        VelocityEstimator::Impulse,
        VelocityEstimator::Ios,
        VelocityEstimator::Macos,
    ] {
        for (dx, dy, min, max, expected_x, expected_y) in [
            (12000.0, 16000.0, 50.0, 15000.0, 9000.0, 12000.0),
            (30.0, 40.0, 51.0, 1000.0, 0.0, 0.0),
            (0.0, 0.0, 50.0, 1000.0, 0.0, 0.0),
            (
                1.5e308,
                1.5e308,
                50.0,
                1000.0,
                1000.0 / std::f64::consts::SQRT_2,
                1000.0 / std::f64::consts::SQRT_2,
            ),
        ] {
            let arena = GestureArena::binding_driven(Arc::new(ManualClock::new()));
            let settings = GestureSettings::touch_defaults()
                .with_velocity_estimator(estimator)
                .try_with_fling_velocity(min, max)
                .expect("valid policy");
            let ends = Rc::new(RefCell::new(Vec::new()));
            let output = ends.clone();
            let drag = DragGestureRecognizer::builder(arena.clone(), DragAxis::Free)
                .settings(settings)
                .on_end(move |details| output.borrow_mut().push(details))
                .build();
            for (time, phase) in [(0, 0), (10, 1), (20, 1), (30, 1), (40, 2)] {
                let seconds = time as f64 / 1000.0;
                send(
                    &*drag,
                    &arena,
                    &event(
                        150,
                        PointerKind::Touch,
                        time,
                        dx * seconds,
                        dy * seconds,
                        phase,
                    ),
                );
            }
            let ends = ends.borrow();
            let details = ends.last().expect("terminal callback");
            let raw = details.velocity.pixels_per_second;
            assert!(
                details.primary_velocity.is_finite(),
                "terminal scalar presentation remains finite"
            );
            if !raw.dx.hypot(raw.dy).is_finite() {
                assert_eq!(
                    details.primary_velocity,
                    f64::MAX,
                    "unrepresentable free magnitude uses its documented scalar ceiling"
                );
            }
            assert!(
                raw.dx.is_finite() && raw.dy.is_finite(),
                "raw measurement is finite"
            );
            if dx != 0.0 {
                assert!(
                    (raw.dx / dx - 1.0).abs() < 1e-10,
                    "raw component preserved: {details:?}"
                );
            }
            if dy != 0.0 {
                assert!(
                    (raw.dy / dy - 1.0).abs() < 1e-10,
                    "raw component preserved: {details:?}"
                );
            }
            let derived = details.fling_velocity().pixels_per_second;
            assert!(
                (derived.dx - expected_x).abs() < 1e-6 && (derived.dy - expected_y).abs() < 1e-6,
                "finite admitted terminal clamp preserves direction: {details:?}"
            );
        }
    }
}

fn force_press_rejects_measured_excursion_before_pressure_returns_to_origin() {
    let arena = GestureArena::new();
    let starts = Rc::new(Cell::new(0));
    let output = starts.clone();
    let force = ForcePressGestureRecognizer::builder(arena.clone())
        .on_start(move |_| output.set(output.get() + 1))
        .build();
    for (id, excursion, expected) in [(30, 100.0, 0), (31, 1.0, 1)] {
        let mut down = event(id, PointerKind::Touch, id * 100, 0.0, 0.0, 0);
        if let PointerEvent::Down(data) = &mut down {
            data.sample.pressure = Some(Pressure::try_new(0.2).expect("valid pressure"));
        }
        send(&*force, &arena, &down);
        let info = PointerInfo::new(
            PointerId::try_from(id).expect("nonzero fixture"),
            PointerKind::Touch,
        );
        let buttons = PointerButtons::NONE.with(PointerButton::PRIMARY);
        let current = sample(id * 100 + 20, 0.0, 0.0)
            .with_pressure(Pressure::try_new(0.7).expect("valid pressure"));
        let historical = sample(id * 100 + 10, excursion, 0.0)
            .with_pressure(Pressure::try_new(0.2).expect("valid pressure"));
        let predicted = sample(id * 100 + 30, 100.0, 0.0)
            .with_pressure(Pressure::try_new(0.9).expect("valid prediction"));
        let movement = PointerEvent::Move(
            PointerMove::new(info, buttons, current)
                .with_coalesced(vec![historical])
                .with_predicted(vec![predicted]),
        );
        send(&*force, &arena, &movement);
        assert_eq!(starts.get(), expected, "measured excursion {excursion}");
        force.cancel();
    }
}

fn nonrepresentable_component_estimate_is_refused() {
    use flui_interaction::processing::VelocityEstimator;
    for estimator in [
        VelocityEstimator::LeastSquares,
        VelocityEstimator::Impulse,
        VelocityEstimator::Ios,
        VelocityEstimator::Macos,
    ] {
        let arena = GestureArena::binding_driven(Arc::new(ManualClock::new()));
        let ends = Rc::new(RefCell::new(Vec::new()));
        let output = ends.clone();
        let drag = DragGestureRecognizer::builder(arena.clone(), DragAxis::Free)
            .settings(GestureSettings::touch_defaults().with_velocity_estimator(estimator))
            .on_end(move |details| output.borrow_mut().push(details))
            .build();
        for (time, position, phase) in [
            (0, 0.0, 0),
            (10, 5e307, 1),
            (20, 1e308, 1),
            (30, 1.5e308, 1),
            (40, 1.5e308, 2),
        ] {
            send(
                &*drag,
                &arena,
                &event(170, PointerKind::Touch, time, position, 0.0, phase),
            );
        }
        let ends = ends.borrow();
        let details = ends.last().expect("terminal callback");
        assert_eq!(
            details.reason,
            flui_interaction::GestureEndReason::Completed
        );
        assert_eq!(
            details.velocity,
            flui_interaction::Velocity::ZERO,
            "{estimator:?} refuses an unrepresentable component"
        );
        assert_eq!(details.fling_velocity(), flui_interaction::Velocity::ZERO);
        assert_eq!(details.primary_velocity, 0.0);
    }
}

fn force_press_snapshots_live_drift_policy_at_down() {
    let arena = GestureArena::new();
    let old = GestureSettings::touch_defaults();
    let source = GestureSettingsSource::new(old.clone());
    let starts = Rc::new(Cell::new(0));
    let output = starts.clone();
    let force = ForcePressGestureRecognizer::builder(arena.clone())
        .settings(source.provider())
        .on_start(move |_| output.set(output.get() + 1))
        .build();
    for (id, expected) in [(190, 1), (191, 1), (192, 2)] {
        let mut down = event(id, PointerKind::Touch, id * 100, 0.0, 0.0, 0);
        if let PointerEvent::Down(data) = &mut down {
            data.sample.pressure = Some(Pressure::try_new(0.2).expect("valid pressure"));
        }
        send(&*force, &arena, &down);
        if id == 190 {
            source.replace(old.clone().try_with_touch_slop(2.0).expect("valid slop"));
        }
        let info = PointerInfo::new(
            PointerId::try_from(id).expect("nonzero pointer"),
            PointerKind::Touch,
        );
        let movement = PointerEvent::Move(PointerMove::new(
            info,
            PointerButtons::NONE.with(PointerButton::PRIMARY),
            sample(id * 100 + 10, 5.0, 0.0)
                .with_pressure(Pressure::try_new(0.7).expect("valid pressure")),
        ));
        send(&*force, &arena, &movement);
        assert_eq!(
            starts.get(),
            expected,
            "contact{id} retains admitted drift policy"
        );
        force.cancel();
        if id == 191 {
            source.replace(old.clone());
        }
    }
}

fn native_touch_span_preserves_baseline_ratio() {
    use flui_platform_api::Distance;
    let baseline = GestureSettings::touch_defaults()
        .try_with_touch_slop(10.0)
        .expect("valid hit")
        .try_with_pan_slop(30.0)
        .expect("valid pan");
    let geometry = GestureGeometry::new(DevicePixelRatio::ONE)
        .with_touch_slop(Distance::new(4.0).expect("valid observation"));
    let settings = GestureSettings::resolve_preferences(
        &baseline,
        &GesturePreferences::default(),
        Some(&geometry),
    )
    .expect("representable tiers");
    let arena = GestureArena::new();
    let starts = Rc::new(Cell::new(0));
    let output = starts.clone();
    let scale = ScaleGestureRecognizer::builder(arena.clone())
        .settings(settings)
        .on_start(move |_| output.set(output.get() + 1))
        .build();
    let rival = Rc::new(Rival);
    contested_down(
        &*scale,
        &arena,
        &event(180, PointerKind::Touch, 0, 0.0, 0.0, 0),
        &rival,
    );
    contested_down(
        &*scale,
        &arena,
        &event(181, PointerKind::Touch, 1, 1000.0, 0.0, 0),
        &rival,
    );
    send(
        &*scale,
        &arena,
        &event(181, PointerKind::Touch, 10, 1012.0, 0.0, 1),
    );
    assert_eq!(starts.get(), 0, "six pixels stays below projected span7.2");
    send(
        &*scale,
        &arena,
        &event(181, PointerKind::Touch, 20, 1016.0, 0.0, 1),
    );
    assert_eq!(
        starts.get(),
        1,
        "eight pixels crosses span; focal and dimensionless ratio remain below their tiers"
    );
    scale.cancel();
}

fn native_scale_retains_estimator_until_end_and_readmits_next_begin() {
    use flui_foundation::geometry::Offset;
    use flui_interaction::{processing::VelocityEstimator, routing::PanZoomDispatch};
    use flui_platform_api::pointer::{PanZoomEvent, PanZoomPhase, PanZoomTransform};

    let old =
        GestureSettings::touch_defaults().with_velocity_estimator(VelocityEstimator::LeastSquares);
    let new = old
        .clone()
        .with_velocity_estimator(VelocityEstimator::Impulse);
    let source = GestureSettingsSource::new(old.clone());
    let ends = Rc::new(RefCell::new(Vec::new()));
    let output = ends.clone();
    let scale = ScaleGestureRecognizer::builder(GestureArena::new())
        .settings(source.provider())
        .on_end(move |details| output.borrow_mut().push(details.focal_velocity.dx()))
        .build();
    let info = PointerInfo::new(
        PointerId::try_from(510_u64).expect("nonzero"),
        PointerKind::Touch,
    );
    for (sequence, expected) in [500.0, 1_589.925_798_583_198_2, 500.0]
        .into_iter()
        .enumerate()
    {
        let base = u64::try_from(sequence).expect("small sequence") * 100;
        let deliver = |millis, phase| {
            let event = PanZoomEvent::new(
                info,
                EventTime::from_nanos((base + millis) * 1_000_000),
                PointerPosition::try_new(Point::ZERO).expect("finite anchor"),
                phase,
            );
            scale.handle_pan_zoom(PanZoomDispatch {
                local: &event,
                global: &event,
            })
        };
        assert!(!deliver(0, PanZoomPhase::Start));
        if sequence == 0 {
            source.replace(new.clone());
        } else if sequence == 1 {
            source.replace(old.clone());
        }
        for (millis, x) in [(10, 30.0), (20, 50.0), (30, 60.0)] {
            assert!(deliver(
                millis,
                PanZoomPhase::Update(
                    PanZoomTransform::try_new(Offset::new(x, 0.0), 1.0, 0.0)
                        .expect("finite native trajectory"),
                )
            ));
        }
        assert!(deliver(30, PanZoomPhase::End));
        let delivered = ends.borrow();
        assert_eq!(delivered.len(), sequence + 1);
        assert!(
            (delivered[sequence] - expected).abs() < 1e-6,
            "native session {sequence}: got {}, expected {expected}",
            delivered[sequence]
        );
    }
}

#[test]
fn admitted_gesture_settings_contract() {
    let cases: &[(&str, fn())] = &[
        (
            "native scale estimator admission",
            native_scale_retains_estimator_until_end_and_readmits_next_begin,
        ),
        (
            "force press live drift admission",
            force_press_snapshots_live_drift_policy_at_down,
        ),
        (
            "native touch span ratio",
            native_touch_span_preserves_baseline_ratio,
        ),
        (
            "nonrepresentable measurement refusal",
            nonrepresentable_component_estimate_is_refused,
        ),
        (
            "native full double click area",
            native_double_click_full_area_is_halved_per_axis,
        ),
        (
            "terminal fling numerical cases",
            terminal_fling_handles_diagonal_slow_zero_and_extreme_measurements,
        ),
        (
            "drag continuation group profile",
            drag_continuation_retains_group_profile_on_handoff,
        ),
        (
            "native touch tier ratios",
            native_touch_projection_preserves_authored_tier_ratios,
        ),
        (
            "independent multi drag admissions",
            independent_multidrags_snapshot_each_admission,
        ),
        (
            "native mouse timing origin",
            native_mouse_interval_starts_at_first_down,
        ),
        (
            "multi tap whole attempt",
            multi_tap_retains_whole_attempt_policy,
        ),
        (
            "scale first contact session",
            scale_retains_first_contact_profile_until_last_release,
        ),
        ("double tap candidate", double_tap_candidate_retains_policy),
        (
            "tap and drag candidate",
            tap_and_drag_retains_consecutive_candidate_profile,
        ),
        (
            "eager source replacement",
            eager_source_replacement_does_not_retire_accepted_contact,
        ),
        (
            "long press retains deadline",
            long_press_retains_deadline_and_next_contact_observes_replacement,
        ),
        (
            "terminal fling retains profile",
            terminal_fling_retains_profile_and_preserves_measured_velocity,
        ),
        (
            "mouse rectangle uses each axis",
            mouse_rectangle_is_compared_per_axis,
        ),
        (
            "force press measured excursion",
            force_press_rejects_measured_excursion_before_pressure_returns_to_origin,
        ),
        (
            "candidate kind compatibility",
            consecutive_candidates_do_not_cross_pointer_kinds,
        ),
        (
            "candidate device compatibility",
            consecutive_candidates_compare_device_identity,
        ),
    ];
    let mut failures = Vec::new();
    for &(name, case) in cases {
        if std::panic::catch_unwind(case).is_err() {
            eprintln!("admitted settings contract {name} failed");
            failures.push(name);
        }
    }
    assert!(
        failures.is_empty(),
        "admitted settings cases failed: {failures:?}"
    );
}
