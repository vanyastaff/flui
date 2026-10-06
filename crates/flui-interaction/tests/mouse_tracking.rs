//! Mouse tracking (enter / exit / cursor) across devices and layout changes,
//! and the localization of every coordinate a hit entry receives.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use flui_foundation::RenderId;
use flui_foundation::geometry::{Matrix4, Offset};
use flui_interaction::events::{
    Modifiers, PointerButtons, PointerEvent, PointerType, ScrollDelta, ScrollEventData,
    make_move_event_for_id, make_scroll_event,
};
use flui_interaction::routing::{
    DeviceId, InteractionDispatchHandle, InteractionLane, MouseRegionCallbacks, MouseRegionTarget,
    MouseTracker, MouseTrackerAnnotation, PointerMotionKind,
};
use flui_interaction::{CursorIcon, EventPropagation, HitTestEntry, HitTestResult, PointerId};

const MOUSE: u64 = 2;
const PEN: u64 = 3;

/// A buttonless move, as a platform reports hover, `time_ms` after start.
fn hover(pointer: u64, pointer_type: PointerType, position: Offset, time_ms: u64) -> PointerEvent {
    let id = PointerId::new(pointer).expect("nonzero pointer id");
    let mut event = make_move_event_for_id(id, position, pointer_type);
    if let PointerEvent::Move(update) = &mut event {
        update.current.buttons = PointerButtons::new();
        update.current.time = time_ms * 1_000_000;
    }
    event
}

fn device(pointer: u64) -> DeviceId {
    DeviceId::try_from(pointer).expect("small pointer id")
}

/// A hit path, leaf-first, over the given regions.
fn path(regions: &[(usize, MouseRegionTarget)]) -> HitTestResult {
    let mut result = HitTestResult::new();
    for &(region, target) in regions {
        let id = RenderId::new(region);
        result.add(HitTestEntry::new(id).mouse_annotation(MouseTrackerAnnotation::new(id, target)));
    }
    result
}

type Log = Rc<RefCell<Vec<String>>>;

/// A region that logs `enter <name> <device>` and `exit <name> <device>`.
fn logging_region(
    handle: &InteractionDispatchHandle,
    name: &'static str,
    log: &Log,
) -> MouseRegionTarget {
    let entered = Rc::clone(log);
    let exited = Rc::clone(log);
    handle
        .register_mouse_region(MouseRegionCallbacks {
            on_enter: Some(Rc::new(move |device, _| {
                entered.borrow_mut().push(format!("enter {name} {device}"));
            })),
            on_exit: Some(Rc::new(move |device, _| {
                exited.borrow_mut().push(format!("exit {name} {device}"));
            })),
            ..MouseRegionCallbacks::default()
        })
        .expect("register region")
}

fn take(log: &Log) -> Vec<String> {
    std::mem::take(&mut *log.borrow_mut())
}

fn shared_region_exits_once_per_device() {
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let tracker = MouseTracker::new();
    let log = Log::default();
    let region = lane.enter(|| logging_region(&handle, "R", &log));
    let over = path(&[(1, region)]);
    let away = HitTestResult::new();
    let at = Offset::new(10.0, 10.0);

    lane.enter(|| {
        tracker.update_with_motion(
            &hover(MOUSE, PointerType::Mouse, at, 1),
            PointerMotionKind::Hover,
            &over,
        );
        tracker.update_with_motion(
            &hover(PEN, PointerType::Pen, at, 2),
            PointerMotionKind::Hover,
            &over,
        );
        tracker.update_with_motion(
            &hover(PEN, PointerType::Pen, at, 3),
            PointerMotionKind::Hover,
            &away,
        );
        tracker.update_with_motion(
            &hover(MOUSE, PointerType::Mouse, at, 4),
            PointerMotionKind::Hover,
            &away,
        );
        tracker.update_with_motion(
            &hover(MOUSE, PointerType::Mouse, at, 5),
            PointerMotionKind::Hover,
            &away,
        );
    });
    assert_eq!(
        take(&log),
        ["enter R 2", "enter R 3", "exit R 3", "exit R 2"],
        "each device leaving the shared region gets exactly one exit"
    );
}

fn stationary_device_follows_layout_without_duplicates() {
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let tracker = MouseTracker::new();
    let log = Log::default();
    let (inner, outer, sibling) = lane.enter(|| {
        (
            logging_region(&handle, "inner", &log),
            logging_region(&handle, "outer", &log),
            logging_region(&handle, "sibling", &log),
        )
    });
    let nested = path(&[(1, inner), (2, outer)]);
    let moved = path(&[(3, sibling)]);
    let at = Offset::new(10.0, 10.0);

    lane.enter(|| {
        tracker.update_with_motion(
            &hover(MOUSE, PointerType::Mouse, at, 1),
            PointerMotionKind::Hover,
            &HitTestResult::new(),
        );
        // Layout moves the nested regions under the still cursor.
        tracker.update_all_devices(|_| nested.clone());
        tracker.update_all_devices(|_| nested.clone());
        assert_eq!(
            take(&log),
            ["enter outer 2", "enter inner 2"],
            "outermost enter first, once"
        );

        // Layout replaces them with a sibling: every exit precedes the enter.
        tracker.update_all_devices(|_| moved.clone());
        tracker.update_with_motion(
            &hover(MOUSE, PointerType::Mouse, at, 2),
            PointerMotionKind::Hover,
            &moved,
        );
    });
    assert_eq!(
        take(&log),
        ["exit inner 2", "exit outer 2", "enter sibling 2"],
        "innermost exit first, exits before enters, no duplicate on the next move"
    );
}

/// A region whose exit counts and then panics with `message`.
fn panicking_exit(
    handle: &InteractionDispatchHandle,
    exits: &Rc<Cell<usize>>,
    enters: &Rc<Cell<usize>>,
    message: &'static str,
) -> MouseRegionTarget {
    let exits = Rc::clone(exits);
    let enters = Rc::clone(enters);
    handle
        .register_mouse_region(MouseRegionCallbacks {
            on_enter: Some(Rc::new(move |_, _| enters.set(enters.get() + 1))),
            on_exit: Some(Rc::new(move |_, _| {
                exits.set(exits.get() + 1);
                std::panic::panic_any(message);
            })),
            ..MouseRegionCallbacks::default()
        })
        .expect("register region")
}

fn refresh_callback_panic_reaches_every_device() {
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let tracker = MouseTracker::new();
    let exits = Rc::new(Cell::new(0));
    let enters = Rc::new(Cell::new(0));
    let (first, second) = lane.enter(|| {
        (
            panicking_exit(&handle, &exits, &enters, "first region exit"),
            panicking_exit(&handle, &exits, &enters, "second region exit"),
        )
    });
    let mouse_at = Offset::new(10.0, 10.0);
    let pen_at = Offset::new(20.0, 20.0);
    let regions_at = |position: Offset| {
        if position == mouse_at {
            path(&[(1, first)])
        } else {
            path(&[(2, second)])
        }
    };
    lane.enter(|| {
        tracker.update_with_motion(
            &hover(MOUSE, PointerType::Mouse, mouse_at, 1),
            PointerMotionKind::Hover,
            &regions_at(mouse_at),
        );
        tracker.update_with_motion(
            &hover(PEN, PointerType::Pen, pen_at, 2),
            PointerMotionKind::Hover,
            &regions_at(pen_at),
        );
    });
    assert_eq!(enters.get(), 2);

    let failure = catch_unwind(AssertUnwindSafe(|| {
        lane.enter(|| tracker.update_all_devices(|_| HitTestResult::new()));
    }));
    let payload = failure.expect_err("an exit panic resumes after the refresh");
    let message = payload.downcast_ref::<&str>().copied();
    assert!(
        matches!(message, Some("first region exit" | "second region exit")),
        "the resumed panic is an exit callback's: {message:?}"
    );
    assert_eq!(exits.get(), 2, "the other device's exit still runs");

    lane.enter(|| tracker.update_all_devices(regions_at));
    assert_eq!(enters.get(), 4, "the next refresh works for both devices");
}

fn refresh_hit_test_panic_keeps_the_device_for_the_next_refresh() {
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let tracker = MouseTracker::new();
    let log = Log::default();
    let (mouse_region, pen_region) = lane.enter(|| {
        (
            logging_region(&handle, "M", &log),
            logging_region(&handle, "P", &log),
        )
    });
    let mouse_at = Offset::new(10.0, 10.0);
    let pen_at = Offset::new(20.0, 20.0);
    lane.enter(|| {
        tracker.update_with_motion(
            &hover(MOUSE, PointerType::Mouse, mouse_at, 1),
            PointerMotionKind::Hover,
            &path(&[(1, mouse_region)]),
        );
        tracker.update_with_motion(
            &hover(PEN, PointerType::Pen, pen_at, 2),
            PointerMotionKind::Hover,
            &path(&[(2, pen_region)]),
        );
    });
    take(&log);

    let failure = catch_unwind(AssertUnwindSafe(|| {
        lane.enter(|| {
            tracker.update_all_devices(|position| {
                assert!(position != pen_at, "hit test failed");
                HitTestResult::new()
            });
        });
    }));
    failure.expect_err("the hit-test panic resumes after the refresh");
    assert_eq!(
        take(&log),
        ["exit M 2"],
        "the healthy device's transition is delivered"
    );

    lane.enter(|| tracker.update_all_devices(|_| HitTestResult::new()));
    assert_eq!(
        take(&log),
        ["exit P 3"],
        "the failed device is retried, once"
    );
}

/// Reenters the tracker from a region callback's destructor.
struct ReentersTracker {
    tracker: MouseTracker,
    reentered: Rc<Cell<bool>>,
}

impl Drop for ReentersTracker {
    fn drop(&mut self) {
        let _ = self.tracker.device_cursor(device(MOUSE));
        self.reentered.set(true);
    }
}

fn region_destructor_may_reenter_the_tracker() {
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let tracker = MouseTracker::new();
    let reentered = Rc::new(Cell::new(false));
    let probe = ReentersTracker {
        tracker: tracker.clone(),
        reentered: Rc::clone(&reentered),
    };
    let region = lane.enter(|| {
        handle
            .register_mouse_region(MouseRegionCallbacks {
                on_enter: Some(Rc::new(move |_, _| {
                    let _ = &probe;
                })),
                ..MouseRegionCallbacks::default()
            })
            .expect("register region")
    });
    let at = Offset::new(10.0, 10.0);
    lane.enter(|| {
        tracker.update_with_motion(
            &hover(MOUSE, PointerType::Mouse, at, 1),
            PointerMotionKind::Hover,
            &path(&[(1, region)]),
        );
        // The region unmounts while hovered: the tracker now holds its last
        // callbacks.
        handle
            .unregister_mouse_region(region)
            .expect("unregister region");
        tracker.update_with_motion(
            &hover(MOUSE, PointerType::Mouse, at, 2),
            PointerMotionKind::Hover,
            &HitTestResult::new(),
        );
    });
    assert!(reentered.get(), "the released callbacks were destroyed");
}

fn cursor_path(cursors: &[Option<CursorIcon>]) -> HitTestResult {
    let mut result = HitTestResult::new();
    for (index, cursor) in cursors.iter().enumerate() {
        let entry = HitTestEntry::new(RenderId::new(index + 1));
        result.add(match cursor {
            Some(cursor) => entry.cursor(*cursor),
            None => entry,
        });
    }
    result
}

fn explicit_arrow_overrides_an_ancestor_cursor() {
    let resolved =
        cursor_path(&[Some(CursorIcon::Default), Some(CursorIcon::Text)]).resolve_cursor();
    assert_eq!(resolved, CursorIcon::Default);
}

fn deferring_child_shows_the_ancestor_cursor() {
    let resolved = cursor_path(&[None, Some(CursorIcon::Text)]).resolve_cursor();
    assert_eq!(resolved, CursorIcon::Text);
}

fn path_without_requests_shows_the_arrow() {
    assert_eq!(
        cursor_path(&[None, None]).resolve_cursor(),
        CursorIcon::Default
    );
}

fn tracker_reports_the_explicit_arrow() {
    let tracker = MouseTracker::new();
    let changes = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&changes);
    tracker.set_cursor_change_callback(Rc::new(move |_, cursor| sink.borrow_mut().push(cursor)));
    let at = Offset::new(10.0, 10.0);
    let text_area = cursor_path(&[None, Some(CursorIcon::Text)]);
    let button = cursor_path(&[Some(CursorIcon::Default), Some(CursorIcon::Text)]);
    tracker.update_with_motion(
        &hover(MOUSE, PointerType::Mouse, at, 1),
        PointerMotionKind::Hover,
        &text_area,
    );
    tracker.update_with_motion(
        &hover(MOUSE, PointerType::Mouse, at, 2),
        PointerMotionKind::Hover,
        &button,
    );
    assert_eq!(*changes.borrow(), [CursorIcon::Text, CursorIcon::Default]);
}

/// Local of a global point under `translate(100, 50) · rotate(90°) · scale(2)`:
/// forward maps local `(x, y)` to `(100 - 2y, 50 + 2x)`.
fn expected_local(global: (f64, f64)) -> (f64, f64) {
    ((global.1 - 50.0) / 2.0, (100.0 - global.0) / 2.0)
}

/// A delta has no translation: global `(dx, dy)` is local `(dy / 2, -dx / 2)`.
fn expected_local_delta(global: (f64, f64)) -> (f64, f64) {
    (global.1 / 2.0, -global.0 / 2.0)
}

fn assert_close(actual: (f64, f64), expected: (f64, f64), what: &str) {
    assert!(
        (actual.0 - expected.0).abs() < 1e-9 && (actual.1 - expected.1).abs() < 1e-9,
        "{what}: {actual:?} != {expected:?}"
    );
}

/// A hit path with one entry inside the rotated, scaled, translated subtree.
fn transformed_entry(entry: HitTestEntry) -> HitTestResult {
    let mut result = HitTestResult::new();
    result
        .with_paint_transform(Matrix4::translation(100.0, 50.0, 0.0), |result| {
            result.with_paint_transform(
                Matrix4::rotation_z(std::f64::consts::FRAC_PI_2),
                |result| {
                    result.with_paint_transform(Matrix4::scaling(2.0, 2.0, 1.0), |result| {
                        result.add(entry);
                    })
                },
            )
        })
        .flatten()
        .flatten()
        .expect("finite transforms are admitted");
    result
}

fn pointer_events_seen_locally(events: &[PointerEvent]) -> Vec<PointerEvent> {
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let seen = Rc::new(RefCell::new(Vec::new()));
    lane.enter(|| {
        let sink = Rc::clone(&seen);
        let target = handle
            .register_pointer(move |dispatch| sink.borrow_mut().push(dispatch.local.clone()))
            .expect("register pointer");
        let result = transformed_entry(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
        for event in events {
            result.dispatch(event);
        }
    });
    seen.take()
}

fn move_samples_are_localized() {
    let mut event = hover(MOUSE, PointerType::Mouse, Offset::new(80.0, 70.0), 3);
    if let PointerEvent::Move(update) = &mut event {
        for (x, y) in [(90.0, 60.0), (84.0, 66.0)] {
            let mut sample = update.current.clone();
            sample.position = dpi::PhysicalPosition::new(x, y);
            update.coalesced.push(sample);
        }
        let mut predicted = update.current.clone();
        predicted.position = dpi::PhysicalPosition::new(76.0, 74.0);
        update.predicted.push(predicted);
    }
    let seen = pointer_events_seen_locally(&[event]);
    let [PointerEvent::Move(local)] = seen.as_slice() else {
        panic!("one localized move: {seen:?}");
    };
    let point =
        |state: &flui_interaction::events::PointerState| (state.position.x, state.position.y);
    assert_close(
        point(&local.current),
        expected_local((80.0, 70.0)),
        "current",
    );
    assert_eq!(local.coalesced.len(), 2);
    assert_close(
        point(&local.coalesced[0]),
        expected_local((90.0, 60.0)),
        "coalesced[0]",
    );
    assert_close(
        point(&local.coalesced[1]),
        expected_local((84.0, 66.0)),
        "coalesced[1]",
    );
    assert_eq!(local.predicted.len(), 1);
    assert_close(
        point(&local.predicted[0]),
        expected_local((76.0, 74.0)),
        "predicted",
    );
}

fn scroll_delta_is_localized_as_a_vector() {
    let pixels = make_scroll_event(Offset::new(80.0, 70.0), Offset::new(0.0, 30.0));
    let mut lines = pixels.clone();
    if let PointerEvent::Scroll(scroll) = &mut lines {
        scroll.delta = ScrollDelta::LineDelta(4.0, 0.0);
    }
    let seen = pointer_events_seen_locally(&[pixels, lines]);
    let [PointerEvent::Scroll(pixels), PointerEvent::Scroll(lines)] = seen.as_slice() else {
        panic!("two localized scrolls: {seen:?}");
    };
    assert_close(
        (pixels.state.position.x, pixels.state.position.y),
        expected_local((80.0, 70.0)),
        "scroll position",
    );
    let ScrollDelta::PixelDelta(delta) = pixels.delta else {
        panic!("pixel delta stays pixels: {:?}", pixels.delta);
    };
    assert_close(
        (delta.x, delta.y),
        expected_local_delta((0.0, 30.0)),
        "pixel delta",
    );
    let ScrollDelta::LineDelta(x, y) = lines.delta else {
        panic!("line delta stays lines: {:?}", lines.delta);
    };
    assert_close(
        (f64::from(x), f64::from(y)),
        expected_local_delta((4.0, 0.0)),
        "line delta",
    );
}

fn scroll_target_delta_is_localized_as_a_vector() {
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let seen = Rc::new(RefCell::new(Vec::new()));
    lane.enter(|| {
        let sink = Rc::clone(&seen);
        let target = handle
            .register_scroll(move |event| {
                sink.borrow_mut().push(*event);
                EventPropagation::Continue
            })
            .expect("register scroll");
        let result = transformed_entry(HitTestEntry::new(RenderId::new(1)).scroll_target(target));
        let event = ScrollEventData::new(
            Offset::new(80.0, 70.0),
            Offset::new(12.0, 0.0),
            Modifiers::empty(),
        );
        result.dispatch_scroll(&event);
    });
    let seen = seen.take();
    let [local] = seen.as_slice() else {
        panic!("one localized scroll: {seen:?}");
    };
    assert_close(
        (local.position.dx, local.position.dy),
        expected_local((80.0, 70.0)),
        "position",
    );
    assert_close(
        (local.delta.dx, local.delta.dy),
        expected_local_delta((12.0, 0.0)),
        "delta",
    );
}

#[test]
fn mouse_tracking_and_localization() {
    let mut failures = Vec::new();
    for (name, case) in [
        (
            "shared region exits once per device",
            shared_region_exits_once_per_device as fn(),
        ),
        (
            "stationary device follows layout",
            stationary_device_follows_layout_without_duplicates as fn(),
        ),
        (
            "refresh callback panic",
            refresh_callback_panic_reaches_every_device as fn(),
        ),
        (
            "refresh hit-test panic",
            refresh_hit_test_panic_keeps_the_device_for_the_next_refresh as fn(),
        ),
        (
            "region destructor reenters",
            region_destructor_may_reenter_the_tracker as fn(),
        ),
        (
            "explicit arrow cursor",
            explicit_arrow_overrides_an_ancestor_cursor as fn(),
        ),
        (
            "deferring cursor",
            deferring_child_shows_the_ancestor_cursor as fn(),
        ),
        (
            "no cursor request",
            path_without_requests_shows_the_arrow as fn(),
        ),
        (
            "tracker explicit arrow",
            tracker_reports_the_explicit_arrow as fn(),
        ),
        ("move samples localized", move_samples_are_localized as fn()),
        (
            "scroll delta localized",
            scroll_delta_is_localized_as_a_vector as fn(),
        ),
        (
            "scroll target delta localized",
            scroll_target_delta_is_localized_as_a_vector as fn(),
        ),
    ] {
        if let Err(payload) = catch_unwind(case) {
            flui_foundation::panic::retain_opaque_payload(payload);
            failures.push(name);
        }
    }
    assert!(
        failures.is_empty(),
        "mouse tracking cases failed: {failures:?}"
    );
}
