//! Mouse tracking (enter / exit / cursor) across devices and layout changes,
//! and the localization of every coordinate a hit entry receives.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use flui_foundation::RenderId;
use flui_foundation::geometry::{Matrix4, Offset};
use flui_interaction::events::{
    Modifiers, PointerButtons, PointerEvent, PointerInfo, PointerKind, PointerPosition,
    ScrollDelta, ScrollEvent, make_move_event_for_id, pointer::ScrollUnit,
};
use flui_interaction::routing::{
    DeviceId, InteractionDispatchHandle, InteractionLane, MouseRegionCallbacks, MouseRegionTarget,
    MouseTracker, MouseTrackerAnnotation, PointerMotionKind,
};
use flui_interaction::{CursorIcon, EventPropagation, HitTestEntry, HitTestResult, PointerId};
use flui_platform_api::pointer::{PanZoomEvent, PanZoomPhase, PanZoomTransform, PenTool};

const MOUSE: u64 = 2;
const PEN: u64 = 3;

/// A buttonless move, as a platform reports hover, `time_ms` after start.
fn hover(pointer: u64, pointer_type: PointerKind, position: Offset, time_ms: u64) -> PointerEvent {
    let id = PointerId::new(std::num::NonZeroU64::new(pointer).expect("nonzero pointer id"));
    let mut event =
        make_move_event_for_id(id, position, pointer_type).expect("valid fixture sample");
    if let PointerEvent::Move(update) = &mut event {
        update.buttons = PointerButtons::NONE;
        update.pointer = update.pointer.with_device(device(pointer));
        {
            let mut sample = *update.current();
            sample.time = flui_platform_api::EventTime::from_nanos(time_ms * 1_000_000);
            *update =
                flui_interaction::events::PointerMove::new(update.pointer, update.buttons, sample)
                    .with_modifiers(update.modifiers)
                    .with_coalesced(update.coalesced().to_vec())
                    .with_predicted(update.predicted().to_vec());
        };
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
                entered.borrow_mut().push(format!(
                    "enter {name} {}",
                    device
                        .device
                        .expect("fixture reports hardware identity")
                        .get()
                        .get()
                ));
            })),
            on_exit: Some(Rc::new(move |device, _| {
                exited.borrow_mut().push(format!(
                    "exit {name} {}",
                    device
                        .device
                        .expect("fixture reports hardware identity")
                        .get()
                        .get()
                ));
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
            &hover(MOUSE, PointerKind::Mouse, at, 1),
            PointerMotionKind::Hover,
            &over,
        );
        tracker.update_with_motion(
            &hover(PEN, PointerKind::Pen { tool: PenTool::Tip }, at, 2),
            PointerMotionKind::Hover,
            &over,
        );
        tracker.update_with_motion(
            &hover(PEN, PointerKind::Pen { tool: PenTool::Tip }, at, 3),
            PointerMotionKind::Hover,
            &away,
        );
        tracker.update_with_motion(
            &hover(MOUSE, PointerKind::Mouse, at, 4),
            PointerMotionKind::Hover,
            &away,
        );
        tracker.update_with_motion(
            &hover(MOUSE, PointerKind::Mouse, at, 5),
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
            &hover(MOUSE, PointerKind::Mouse, at, 1),
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
            &hover(MOUSE, PointerKind::Mouse, at, 2),
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
            &hover(MOUSE, PointerKind::Mouse, mouse_at, 1),
            PointerMotionKind::Hover,
            &regions_at(mouse_at),
        );
        tracker.update_with_motion(
            &hover(PEN, PointerKind::Pen { tool: PenTool::Tip }, pen_at, 2),
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
    assert_eq!(
        message,
        Some("first region exit"),
        "the lower device identity's failure remains authoritative"
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
            &hover(MOUSE, PointerKind::Mouse, mouse_at, 1),
            PointerMotionKind::Hover,
            &path(&[(1, mouse_region)]),
        );
        tracker.update_with_motion(
            &hover(PEN, PointerKind::Pen { tool: PenTool::Tip }, pen_at, 2),
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

fn refresh_hit_test_failure_precedes_a_competing_callback_failure() {
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let tracker = MouseTracker::new();
    let exits = Rc::new(Cell::new(0));
    let enters = Rc::new(Cell::new(0));
    let mouse_region =
        lane.enter(|| panicking_exit(&handle, &exits, &enters, "callback failure after probe"));
    let mouse_at = Offset::new(10.0, 10.0);
    let pen_at = Offset::new(20.0, 20.0);
    lane.enter(|| {
        tracker.update_with_motion(
            &hover(MOUSE, PointerKind::Mouse, mouse_at, 1),
            PointerMotionKind::Hover,
            &path(&[(1, mouse_region)]),
        );
        tracker.update_with_motion(
            &hover(PEN, PointerKind::Pen { tool: PenTool::Tip }, pen_at, 2),
            PointerMotionKind::Hover,
            &HitTestResult::new(),
        );
    });

    let payload = catch_unwind(AssertUnwindSafe(|| {
        lane.enter(|| {
            tracker.update_all_devices(|position| {
                assert!(position != pen_at, "probe failure first");
                HitTestResult::new()
            });
        });
    }))
    .expect_err("the probe failure resumes after delivering the committed exit");
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"probe failure first"));
    assert_eq!(exits.get(), 1, "the competing exit callback still runs");

    lane.enter(|| {
        tracker.update_all_devices(|position| {
            if position == mouse_at {
                path(&[(1, mouse_region)])
            } else {
                HitTestResult::new()
            }
        });
    });
    assert_eq!(
        enters.get(),
        2,
        "the next refresh can enter the region again"
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
            &hover(MOUSE, PointerKind::Mouse, at, 1),
            PointerMotionKind::Hover,
            &path(&[(1, region)]),
        );
        // The region unmounts while hovered: the tracker now holds its last
        // callbacks.
        handle
            .unregister_mouse_region(region)
            .expect("unregister region");
        tracker.update_with_motion(
            &hover(MOUSE, PointerKind::Mouse, at, 2),
            PointerMotionKind::Hover,
            &HitTestResult::new(),
        );
    });
    assert!(reentered.get(), "the released callbacks were destroyed");
}

fn region_retirement_preserves_first_failure_and_recovers() {
    const SELECTED: &str = "FLUI_MOUSE_REGION_RETIREMENT_CASE";
    if let Ok(selected) = std::env::var(SELECTED) {
        assert_region_retirement_recovery(selected == "competing");
        return;
    }

    for selected in ["single", "competing"] {
        let mut child =
            std::process::Command::new(std::env::current_exe().expect("test executable"))
                .args([
                    "--exact",
                    "mouse_tracking::released_region_destructor_reenters_tracker",
                    "--nocapture",
                ])
                .env(SELECTED, selected)
                .env("RUST_BACKTRACE", "0")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("mouse retirement child");
        let start = std::time::Instant::now();
        while child.try_wait().expect("child status").is_none() {
            if start.elapsed() > std::time::Duration::from_secs(10) {
                child.kill().expect("kill stalled mouse retirement child");
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            child.wait().expect("child exit").success(),
            "mouse retirement {selected} failed"
        );
    }
}

fn assert_region_retirement_recovery(competing: bool) {
    struct Capture {
        tracker: MouseTracker,
        drops: Rc<Cell<usize>>,
        panic_message: Option<&'static str>,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            let _ = self.tracker.device_cursor(device(MOUSE));
            self.drops.set(self.drops.get() + 1);
            if let Some(message) = self.panic_message {
                std::panic::panic_any(message);
            }
        }
    }

    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let tracker = MouseTracker::new();
    let first_drops = Rc::new(Cell::new(0));
    let second_drops = Rc::new(Cell::new(0));
    let healthy_drops = Rc::new(Cell::new(0));
    let regions = lane.enter(|| {
        let mut regions = Vec::new();
        for (index, (drops, message)) in [
            (&first_drops, Some("first capture failure")),
            (&second_drops, competing.then_some("second capture failure")),
            (&healthy_drops, None),
        ]
        .into_iter()
        .enumerate()
        {
            let capture = Capture {
                tracker: tracker.clone(),
                drops: Rc::clone(drops),
                panic_message: message,
            };
            let target = handle
                .register_mouse_region(MouseRegionCallbacks {
                    on_enter: Some(Rc::new(move |_, _| {
                        let _ = &capture;
                    })),
                    ..MouseRegionCallbacks::default()
                })
                .expect("register captured region");
            regions.push((index + 1, target));
        }
        regions
    });
    let at = Offset::new(10.0, 10.0);
    lane.enter(|| {
        tracker.update_with_motion(
            &hover(MOUSE, PointerKind::Mouse, at, 1),
            PointerMotionKind::Hover,
            &path(&regions),
        );
        for &(_, target) in &regions {
            handle
                .unregister_mouse_region(target)
                .expect("unregister captured region");
        }
    });
    let payload = catch_unwind(AssertUnwindSafe(|| {
        lane.enter(|| {
            tracker.update_with_motion(
                &hover(MOUSE, PointerKind::Mouse, at, 2),
                PointerMotionKind::Hover,
                &HitTestResult::new(),
            );
        });
    }))
    .expect_err("first capture failure resumes");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"first capture failure")
    );
    assert_eq!(first_drops.get(), 1);
    assert_eq!(
        second_drops.get(),
        0,
        "the retired tail is retained after the first failure"
    );
    assert_eq!(
        healthy_drops.get(),
        0,
        "healthy opaque captures obey the same retention policy"
    );

    let log = Log::default();
    lane.enter(|| {
        let target = logging_region(&handle, "healthy", &log);
        tracker.update_with_motion(
            &hover(MOUSE, PointerKind::Mouse, at, 3),
            PointerMotionKind::Hover,
            &path(&[(4, target)]),
        );
        tracker.update_with_motion(
            &hover(MOUSE, PointerKind::Mouse, at, 4),
            PointerMotionKind::Hover,
            &HitTestResult::new(),
        );
    });
    assert_eq!(take(&log), ["enter healthy 2", "exit healthy 2"]);
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
        &hover(MOUSE, PointerKind::Mouse, at, 1),
        PointerMotionKind::Hover,
        &text_area,
    );
    tracker.update_with_motion(
        &hover(MOUSE, PointerKind::Mouse, at, 2),
        PointerMotionKind::Hover,
        &button,
    );
    assert_eq!(*changes.borrow(), [CursorIcon::Text, CursorIcon::Default]);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CursorReentryCase {
    Arrow,
    SameCursor,
    OtherDevice,
    Replacement,
    HookReentry,
    Failure,
    ReentrantFailure,
    ReentrantSuccess,
    HookReplacement,
    CompetingFailure,
    Close,
}

fn latest_cursor_publication_survives_reentry_replacement_and_failure() {
    for case in [
        CursorReentryCase::Arrow,
        CursorReentryCase::SameCursor,
        CursorReentryCase::OtherDevice,
        CursorReentryCase::Replacement,
        CursorReentryCase::HookReentry,
        CursorReentryCase::Failure,
        CursorReentryCase::ReentrantFailure,
        CursorReentryCase::ReentrantSuccess,
        CursorReentryCase::HookReplacement,
        CursorReentryCase::CompetingFailure,
        CursorReentryCase::Close,
    ] {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let tracker = MouseTracker::new();
        let observed = Rc::new(RefCell::new(Vec::new()));
        let next_observed = Rc::clone(&observed);
        let replacement: flui_interaction::routing::CursorChangeCallback =
            Rc::new(move |pointer, cursor| {
                next_observed.borrow_mut().push(("new", pointer.id, cursor));
            });
        let failed = Rc::new(Cell::new(matches!(
            case,
            CursorReentryCase::Failure
                | CursorReentryCase::ReentrantFailure
                | CursorReentryCase::CompetingFailure
        )));
        let old_observed = Rc::clone(&observed);
        let hook_tracker = tracker.clone();
        let next_hook = Rc::clone(&replacement);
        let hook_failed = Rc::clone(&failed);
        let entered_hook = Cell::new(false);
        tracker.set_cursor_change_callback(Rc::new(move |pointer, cursor| {
            old_observed.borrow_mut().push(("old", pointer.id, cursor));
            if case == CursorReentryCase::HookReentry && !entered_hook.replace(true) {
                hook_tracker.set_cursor_change_callback(Rc::clone(&next_hook));
                hook_tracker.update_with_motion(
                    &hover(MOUSE, PointerKind::Mouse, Offset::new(7.0, 7.0), 2),
                    PointerMotionKind::Hover,
                    &cursor_path(&[Some(CursorIcon::Text)]),
                );
            }
            if matches!(
                case,
                CursorReentryCase::ReentrantFailure | CursorReentryCase::ReentrantSuccess
            ) && !entered_hook.replace(true)
            {
                hook_tracker.update_with_motion(
                    &hover(MOUSE, PointerKind::Mouse, Offset::new(7.0, 7.0), 2),
                    PointerMotionKind::Hover,
                    &cursor_path(&[Some(CursorIcon::Text)]),
                );
            }
            if case == CursorReentryCase::HookReplacement && !entered_hook.replace(true) {
                hook_tracker.set_cursor_change_callback(Rc::clone(&next_hook));
            }
            assert!(!hook_failed.get(), "cursor publication failure");
        }));
        let enter_tracker = tracker.clone();
        let enter_failed = Rc::clone(&failed);
        let nested_cursor = if case == CursorReentryCase::SameCursor {
            CursorIcon::Text
        } else {
            CursorIcon::Default
        };
        let target = lane.enter(|| {
            handle
                .register_mouse_region(MouseRegionCallbacks {
                    on_enter: Some(Rc::new(move |_, _| match case {
                        CursorReentryCase::Arrow
                        | CursorReentryCase::SameCursor
                        | CursorReentryCase::OtherDevice => {
                            let source = if case == CursorReentryCase::OtherDevice {
                                PEN
                            } else {
                                MOUSE
                            };
                            enter_tracker.update_with_motion(
                                &hover(source, PointerKind::Mouse, Offset::new(7.0, 7.0), 2),
                                PointerMotionKind::Hover,
                                &cursor_path(&[Some(nested_cursor)]),
                            );
                        }
                        CursorReentryCase::Replacement => {
                            enter_tracker.set_cursor_change_callback(Rc::clone(&replacement));
                        }
                        CursorReentryCase::Close => {
                            flui_interaction::__runtime::close_mouse_tracker(
                                &enter_tracker,
                                flui_interaction::__runtime::CloseMode::Ordinary,
                            );
                        }
                        CursorReentryCase::CompetingFailure => {
                            assert!(!enter_failed.get(), "cursor enter first failure");
                        }
                        CursorReentryCase::HookReentry
                        | CursorReentryCase::Failure
                        | CursorReentryCase::ReentrantFailure
                        | CursorReentryCase::ReentrantSuccess
                        | CursorReentryCase::HookReplacement => {}
                    })),
                    ..MouseRegionCallbacks::default()
                })
                .expect("region")
        });
        let region = RenderId::new(1);
        let mut outer = HitTestResult::new();
        outer.add(
            HitTestEntry::new(region)
                .cursor(CursorIcon::Text)
                .mouse_annotation(MouseTrackerAnnotation::new(region, target)),
        );
        let event = hover(MOUSE, PointerKind::Mouse, Offset::new(5.0, 5.0), 1);
        let result = catch_unwind(AssertUnwindSafe(|| {
            lane.enter(|| tracker.update_with_motion(&event, PointerMotionKind::Hover, &outer));
        }));
        if failed.get() {
            let payload = result.expect_err("publication resumes after committed work");
            assert_eq!(
                payload.downcast_ref::<&str>(),
                Some(&if case == CursorReentryCase::CompetingFailure {
                    "cursor enter first failure"
                } else {
                    "cursor publication failure"
                })
            );
            failed.set(false);
            lane.enter(|| tracker.update_with_motion(&event, PointerMotionKind::Hover, &outer));
            assert_eq!(
                observed.borrow().len(),
                2,
                "identical state must retry failed publication"
            );
        } else {
            assert!(result.is_ok());
        }
        let pointer = PointerId::try_from(MOUSE).expect("pointer");
        match case {
            CursorReentryCase::Arrow => {
                assert_eq!(*observed.borrow(), [("old", pointer, CursorIcon::Default)]);
                assert_eq!(tracker.device_cursor(device(MOUSE)), CursorIcon::Default);
            }
            CursorReentryCase::SameCursor => assert_eq!(
                *observed.borrow(),
                [("old", pointer, CursorIcon::Text)],
                "new observation must deliver an unpublished identical cursor"
            ),
            CursorReentryCase::OtherDevice => assert_eq!(
                *observed.borrow(),
                [(
                    "old",
                    PointerId::try_from(PEN).expect("pointer"),
                    CursorIcon::Default
                )],
                "new window observation from another device wins"
            ),
            CursorReentryCase::Replacement => {
                assert_eq!(*observed.borrow(), [("new", pointer, CursorIcon::Text)]);
            }
            CursorReentryCase::HookReentry => assert_eq!(
                *observed.borrow(),
                [
                    ("old", pointer, CursorIcon::Text),
                    ("new", pointer, CursorIcon::Text)
                ],
                "old success cannot acknowledge replacement debt"
            ),
            CursorReentryCase::Close => assert!(
                observed.borrow().is_empty(),
                "closed owner cannot publish cursor"
            ),
            CursorReentryCase::Failure
            | CursorReentryCase::ReentrantFailure
            | CursorReentryCase::CompetingFailure => {}
            CursorReentryCase::ReentrantSuccess | CursorReentryCase::HookReplacement => {
                assert_eq!(*observed.borrow(), [("old", pointer, CursorIcon::Text)]);
                lane.enter(|| {
                    tracker.update_with_motion(
                        &event,
                        PointerMotionKind::Hover,
                        &cursor_path(&[Some(CursorIcon::Text)]),
                    );
                });
                assert_eq!(
                    *observed.borrow(),
                    [
                        ("old", pointer, CursorIcon::Text),
                        (
                            if case == CursorReentryCase::HookReplacement {
                                "new"
                            } else {
                                "old"
                            },
                            pointer,
                            CursorIcon::Text
                        )
                    ],
                    "old successful publication cannot clear newly accepted debt"
                );
            }
        }
        if case != CursorReentryCase::Close {
            let published = observed.borrow().len();
            let source = if case == CursorReentryCase::OtherDevice {
                PEN
            } else {
                MOUSE
            };
            let cursor = if matches!(
                case,
                CursorReentryCase::Arrow | CursorReentryCase::OtherDevice
            ) {
                CursorIcon::Default
            } else {
                CursorIcon::Text
            };
            lane.enter(|| {
                tracker.update_with_motion(
                    &hover(source, PointerKind::Mouse, Offset::new(7.0, 7.0), 3),
                    PointerMotionKind::Hover,
                    &cursor_path(&[Some(cursor)]),
                );
            });
            assert_eq!(
                observed.borrow().len(),
                published,
                "healthy unchanged motion does not duplicate a publication"
            );
        }
        tracker.clear_cursor_change_callback();
    }
}

fn ambient_refresh_preserves_the_latest_physical_cursor_owner() {
    let tracker = MouseTracker::new();
    let observed = Rc::new(RefCell::new(Vec::new()));
    let callback_log = Rc::clone(&observed);
    tracker.set_cursor_change_callback(Rc::new(move |pointer, cursor| {
        callback_log.borrow_mut().push((pointer.id, cursor));
    }));
    let mouse_at = Offset::new(5.0, 5.0);
    let pen_at = Offset::new(15.0, 15.0);
    let text = cursor_path(&[Some(CursorIcon::Text)]);
    let arrow = cursor_path(&[Some(CursorIcon::Default)]);
    tracker.update_with_motion(
        &hover(MOUSE, PointerKind::Mouse, mouse_at, 1),
        PointerMotionKind::Hover,
        &text,
    );
    tracker.update_with_motion(
        &hover(PEN, PointerKind::Pen { tool: PenTool::Tip }, pen_at, 2),
        PointerMotionKind::Hover,
        &arrow,
    );
    tracker.update_with_motion(
        &hover(MOUSE, PointerKind::Mouse, mouse_at, 3),
        PointerMotionKind::Hover,
        &text,
    );
    let count = observed.borrow().len();
    tracker.update_all_devices(|position| {
        if position == mouse_at {
            text.clone()
        } else {
            arrow.clone()
        }
    });
    assert_eq!(
        observed.borrow().last(),
        Some(&(PointerId::try_from(MOUSE).expect("mouse"), CursorIcon::Text)),
        "unchanged ambient probes cannot transfer the window cursor to BTree's unrelated last source"
    );
    assert_eq!(
        observed.borrow().len(),
        count,
        "unchanged ambient geometry must not republish cursor"
    );
    tracker.update_all_devices(|_| arrow.clone());
    assert_eq!(
        observed.borrow().last(),
        Some(&(
            PointerId::try_from(MOUSE).expect("mouse"),
            CursorIcon::Default
        )),
        "layout may change the current physical owner's cursor"
    );
    tracker.update_with_motion(
        &hover(MOUSE, PointerKind::Mouse, mouse_at, 4),
        PointerMotionKind::Hover,
        &text,
    );
    tracker.remove_device(device(MOUSE));
    assert_eq!(
        observed.borrow().last(),
        Some(&(
            PointerId::try_from(MOUSE).expect("mouse"),
            CursorIcon::Default
        )),
        "removing the owner deliberately resets to arrow rather than another source"
    );
    let count = observed.borrow().len();
    tracker.update_all_devices(|_| text.clone());
    assert_eq!(
        observed.borrow().len(),
        count,
        "remaining stationary sources cannot acquire absent cursor ownership"
    );
    tracker.update_with_motion(
        &hover(PEN, PointerKind::Pen { tool: PenTool::Tip }, pen_at, 5),
        PointerMotionKind::Hover,
        &text,
    );
    assert_eq!(
        observed.borrow().last(),
        Some(&(PointerId::try_from(PEN).expect("pen"), CursorIcon::Text)),
        "fresh physical motion acquires ownership after removal"
    );
    tracker.clear_cursor_change_callback();
}

fn ambient_probe_cannot_replace_reentrant_physical_observation(moved: bool, retirement: u8) {
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let tracker = MouseTracker::new();
    let regions = Log::default();
    let initial = lane.enter(|| logging_region(&handle, "initial", &regions));
    let fresh = lane.enter(|| logging_region(&handle, "fresh", &regions));
    let stale = lane.enter(|| logging_region(&handle, "stale", &regions));
    let make_path = |id, target, cursor| {
        let mut result = HitTestResult::new();
        let id = RenderId::new(id);
        result.add(
            HitTestEntry::new(id)
                .mouse_annotation(MouseTrackerAnnotation::new(id, target))
                .cursor(cursor),
        );
        result
    };
    let initial_path = make_path(51, initial, CursorIcon::Text);
    let fresh_path = make_path(52, fresh, CursorIcon::Pointer);
    let stale_path = make_path(53, stale, CursorIcon::Crosshair);
    let original_position = Offset::new(5.0, 5.0);
    let fresh_position = if moved {
        Offset::new(25.0, 25.0)
    } else {
        original_position
    };
    let observed = Rc::new(RefCell::new(Vec::new()));
    let callback_log = Rc::clone(&observed);
    tracker.set_cursor_change_callback(Rc::new(move |_, cursor| {
        callback_log.borrow_mut().push(cursor);
    }));
    lane.enter(|| {
        tracker.update_with_motion(
            &hover(MOUSE, PointerKind::Mouse, original_position, 1),
            PointerMotionKind::Hover,
            &initial_path,
        );
        take(&regions);
        tracker.update_all_devices(|position| {
            assert_eq!(
                position, original_position,
                "probe uses its admitted snapshot"
            );
            match retirement {
                0 => {}
                1 => tracker.remove_device(device(MOUSE)),
                2 => tracker.dispatch_window_left(),
                _ => unreachable!("fixture retirement"),
            }
            tracker.update_with_motion(
                &hover(MOUSE, PointerKind::Mouse, fresh_position, 2),
                PointerMotionKind::Hover,
                &fresh_path,
            );
            stale_path.clone()
        });
        assert_eq!(tracker.device_position(device(MOUSE)), Some(fresh_position));
        assert_eq!(
            tracker.device_cursor(device(MOUSE)),
            CursorIcon::Pointer,
            "stale probe cannot overwrite a newer physical observation, even at the same position"
        );
        assert_eq!(observed.borrow().last(), Some(&CursorIcon::Pointer));
        let transitions = take(&regions);
        assert!(transitions.iter().any(|entry| entry == "enter fresh 2"));
        assert!(
            transitions.iter().all(|entry| !entry.contains("stale")),
            "discarded probe must not publish stale region transitions: {transitions:?}"
        );
        tracker.update_all_devices(|position| {
            assert_eq!(position, fresh_position);
            fresh_path.clone()
        });
        assert!(
            take(&regions).is_empty(),
            "fresh hover state remains committed"
        );
    });
    tracker.clear_cursor_change_callback();
}

fn ambient_probe_preserves_reentrant_motion() {
    ambient_probe_cannot_replace_reentrant_physical_observation(true, 0);
}

fn ambient_probe_preserves_same_position_reentry() {
    ambient_probe_cannot_replace_reentrant_physical_observation(false, 0);
}

fn ambient_probe_preserves_removed_and_readmitted_source() {
    ambient_probe_cannot_replace_reentrant_physical_observation(false, 1);
}

fn ambient_probe_preserves_left_and_readmitted_source() {
    ambient_probe_cannot_replace_reentrant_physical_observation(false, 2);
}

fn removed_cursor_owner_failure_keeps_default_publication_deliverable() {
    let tracker = MouseTracker::new();
    let failed = Rc::new(Cell::new(false));
    let callback_failed = Rc::clone(&failed);
    let observed = Rc::new(RefCell::new(Vec::new()));
    let callback_log = Rc::clone(&observed);
    tracker.set_cursor_change_callback(Rc::new(move |pointer, cursor| {
        callback_log.borrow_mut().push((pointer.id, cursor));
        assert!(
            !(cursor == CursorIcon::Default && callback_failed.get()),
            "removed owner cursor failure"
        );
    }));
    tracker.update_with_motion(
        &hover(MOUSE, PointerKind::Mouse, Offset::new(5.0, 5.0), 1),
        PointerMotionKind::Hover,
        &cursor_path(&[Some(CursorIcon::Text)]),
    );
    failed.set(true);
    let payload = catch_unwind(AssertUnwindSafe(|| tracker.remove_device(device(MOUSE))))
        .expect_err("default publication failure resumes after ownership withdrawal");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"removed owner cursor failure")
    );
    assert!(tracker.device_position(device(MOUSE)).is_none());
    failed.set(false);
    tracker.update_all_devices(|_| panic!("removed source must not be probed"));
    assert_eq!(
        *observed.borrow(),
        [
            (PointerId::try_from(MOUSE).expect("mouse"), CursorIcon::Text),
            (
                PointerId::try_from(MOUSE).expect("mouse"),
                CursorIcon::Default
            ),
            (
                PointerId::try_from(MOUSE).expect("mouse"),
                CursorIcon::Default
            )
        ],
        "ownerless default debt survives until successful publication"
    );
    let count = observed.borrow().len();
    tracker.update_all_devices(|_| panic!("removed source must not be probed"));
    assert_eq!(
        observed.borrow().len(),
        count,
        "successful default has no repeated debt"
    );
    tracker.clear_cursor_change_callback();
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
    let mut event = hover(MOUSE, PointerKind::Mouse, Offset::new(80.0, 70.0), 3);
    if let PointerEvent::Move(update) = &mut event {
        let coalesced = [(90.0, 60.0), (84.0, 66.0)].map(|(x, y)| {
            let mut sample = *update.current();
            sample.position = PointerPosition::try_new(flui_foundation::geometry::Point::new(x, y))
                .expect("finite historical position");
            sample
        });
        let mut predicted = *update.current();
        predicted.position =
            PointerPosition::try_new(flui_foundation::geometry::Point::new(76.0, 74.0))
                .expect("finite predicted position");
        *update = update
            .clone()
            .with_coalesced(coalesced.to_vec())
            .with_predicted(vec![predicted]);
    }
    let seen = pointer_events_seen_locally(&[event]);
    let [PointerEvent::Move(local)] = seen.as_slice() else {
        panic!("one localized move: {seen:?}");
    };
    let point = |state: &flui_interaction::events::PointerSample| {
        (state.position.get().x, state.position.get().y)
    };
    assert_close(
        point(local.current()),
        expected_local((80.0, 70.0)),
        "current",
    );
    assert_eq!(local.coalesced().len(), 2);
    assert_close(
        point(&local.coalesced()[0]),
        expected_local((90.0, 60.0)),
        "coalesced[0]",
    );
    assert_close(
        point(&local.coalesced()[1]),
        expected_local((84.0, 66.0)),
        "coalesced[1]",
    );
    assert_eq!(local.predicted().len(), 1);
    assert_close(
        point(&local.predicted()[0]),
        expected_local((76.0, 74.0)),
        "predicted",
    );
}

fn scroll_delta_is_localized_as_a_vector() {
    let pixels = PointerEvent::Scroll(ScrollEvent::new(
        PointerInfo::new(
            PointerId::new(std::num::NonZeroU64::MIN),
            PointerKind::Mouse,
        ),
        flui_platform_api::EventTime::from_nanos(0),
        PointerPosition::try_new(flui_foundation::geometry::Point::new(80.0, 70.0))
            .expect("finite scroll position"),
        ScrollDelta::try_new(ScrollUnit::Pixels, 0.0, 30.0).expect("finite pixel delta"),
    ));
    let mut lines = pixels.clone();
    if let PointerEvent::Scroll(scroll) = &mut lines {
        scroll.delta =
            ScrollDelta::try_new(ScrollUnit::Lines, 4.0, 0.0).expect("finite line delta");
    }
    let seen = pointer_events_seen_locally(&[pixels, lines]);
    let [PointerEvent::Scroll(pixels), PointerEvent::Scroll(lines)] = seen.as_slice() else {
        panic!("two localized scrolls: {seen:?}");
    };
    assert_close(
        (pixels.position.get().x, pixels.position.get().y),
        expected_local((80.0, 70.0)),
        "scroll position",
    );
    let delta = pixels.delta;
    assert_eq!(delta.unit(), ScrollUnit::Pixels, "pixel delta stays pixels");
    assert_close(
        (delta.x(), delta.y()),
        expected_local_delta((0.0, 30.0)),
        "pixel delta",
    );
    assert_eq!(
        lines.delta.unit(),
        ScrollUnit::Lines,
        "line delta stays lines"
    );
    assert_close((lines.delta.x(), lines.delta.y()), (4.0, 0.0), "line delta");
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
        let event = ScrollEvent::new(
            PointerInfo::new(
                PointerId::new(std::num::NonZeroU64::MIN),
                PointerKind::Mouse,
            ),
            flui_platform_api::EventTime::from_nanos(1_000),
            PointerPosition::try_new(flui_foundation::geometry::Point::new(80.0, 70.0))
                .expect("finite scroll position"),
            ScrollDelta::try_new(ScrollUnit::Pixels, 12.0, 0.0).expect("finite scroll delta"),
        )
        .with_modifiers(Modifiers::NONE);
        result.dispatch_scroll(&event);
    });
    let seen = seen.take();
    let [local] = seen.as_slice() else {
        panic!("one localized scroll: {seen:?}");
    };
    assert_close(
        (local.position.get().x, local.position.get().y),
        expected_local((80.0, 70.0)),
        "position",
    );
    assert_close(
        (local.delta.x(), local.delta.y()),
        expected_local_delta((12.0, 0.0)),
        "delta",
    );
    assert_eq!(local.delta.unit(), ScrollUnit::Pixels);
    assert_eq!(local.time, flui_platform_api::EventTime::from_nanos(1_000));
    assert_eq!(local.modifiers, Modifiers::NONE);
    assert_eq!(local.pointer.kind, PointerKind::Mouse);
}

fn perspective_vectors_follow_the_current_focal_and_preserve_counts() {
    use flui_foundation::geometry::Point;
    use flui_platform_api::{
        EventTime,
        pointer::{ScrollPhase, ScrollPrecision},
    };

    // Forward projection is (x, y) / (1 - x/2). Thus screen (0,2)
    // is local (0,2), and screen (1,2) is local (2/3,4/3).
    let forward = Matrix4::from([
        1.0, 0.0, 0.0, -0.5, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]);
    let pointer = PointerInfo::new(
        PointerId::new(std::num::NonZeroU64::MIN),
        PointerKind::Mouse,
    );
    let position = |x, y| PointerPosition::try_new(Point::new(x, y)).expect("finite focal");
    let scroll = |unit, x, dx| {
        PointerEvent::Scroll(
            ScrollEvent::new(
                pointer,
                EventTime::from_nanos(77),
                position(x, 2.0),
                ScrollDelta::try_new(unit, dx, 0.0).expect("finite delta"),
            )
            .with_precision(ScrollPrecision::Precise)
            .with_phase(ScrollPhase::MomentumChanged)
            .with_modifiers(Modifiers::SHIFT),
        )
    };
    let pan = |x, dx| {
        PointerEvent::PanZoom(
            PanZoomEvent::new(
                pointer,
                EventTime::from_nanos(78),
                position(x, 2.0),
                PanZoomPhase::Update(
                    PanZoomTransform::try_new(Offset::new(dx, 0.0), 1.25, 0.3)
                        .expect("finite cumulative transform"),
                ),
            )
            .with_modifiers(Modifiers::CONTROL),
        )
    };
    let events = [
        scroll(ScrollUnit::Pixels, 0.0, 1.0),
        scroll(ScrollUnit::Lines, 0.0, -2.0),
        scroll(ScrollUnit::Pages, 0.0, -3.0),
        pan(0.0, 1.0),
        pan(2.0, 1.0),
    ];
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let claimed = Rc::new(RefCell::new(Vec::new()));
    lane.enter(|| {
        let sink = seen.clone();
        let target = handle
            .register_pointer(move |dispatch| {
                sink.borrow_mut()
                    .push((dispatch.local.clone(), dispatch.global.clone()));
            })
            .expect("pointer target");
        let sink = claimed.clone();
        let claim = handle
            .register_scroll(move |event| {
                sink.borrow_mut().push(*event);
                EventPropagation::Continue
            })
            .expect("scroll claim");
        let mut result = HitTestResult::new();
        result
            .with_paint_transform(forward, |result| {
                result.add(
                    HitTestEntry::new(RenderId::new(1))
                        .pointer_target(target)
                        .scroll_target(claim),
                );
            })
            .expect("invertible projective plane");
        for event in &events {
            result.dispatch(event);
            if let PointerEvent::Scroll(scroll) = event {
                result.dispatch_scroll(scroll);
            }
        }
        // The focal is admitted; only the metric endpoint is on/beyond the
        // horizon, or overflows during finite-input endpoint addition.
        for (x, dx) in [
            (0.0, -2.0),
            (0.0, -3.0),
            (0.0, -2.0 + f64::EPSILON),
            (f64::MAX, f64::MAX),
        ] {
            for event in [scroll(ScrollUnit::Pixels, x, dx), pan(x, dx)] {
                result.dispatch(&event);
                if let PointerEvent::Scroll(scroll) = event {
                    result.dispatch_scroll(&scroll);
                }
            }
        }
    });
    let seen = seen.borrow();
    assert_eq!(
        seen.len(),
        events.len(),
        "invalid metric endpoints are refused"
    );
    assert_eq!(
        claimed.borrow().len(),
        3,
        "claims share checked localization"
    );
    for (index, (local, global)) in seen.iter().enumerate() {
        assert_eq!(global, &events[index], "source provenance");
        match (local, global) {
            (PointerEvent::Scroll(local), PointerEvent::Scroll(global)) => {
                let expected = if index == 0 {
                    (2.0 / 3.0, -2.0 / 3.0)
                } else {
                    (global.delta.x(), global.delta.y())
                };
                assert_close(
                    (local.delta.x(), local.delta.y()),
                    expected,
                    "anchored pixels or source counts",
                );
                let mut metadata = *local;
                metadata.position = global.position;
                metadata.delta = global.delta;
                assert_eq!(&metadata, global, "scroll metadata");
                assert_eq!(
                    claimed.borrow()[index],
                    *local,
                    "claim and pointer localization agree"
                );
            }
            (PointerEvent::PanZoom(local), PointerEvent::PanZoom(global)) => {
                let PanZoomPhase::Update(value) = local.phase else {
                    panic!("update");
                };
                let expected = if index == 3 {
                    (2.0 / 3.0, -2.0 / 3.0)
                } else {
                    (1.0 / 5.0, -1.0 / 5.0)
                };
                assert_close(
                    (value.pan().dx, value.pan().dy),
                    expected,
                    "current-focal cumulative pan",
                );
                let mut metadata = *local;
                metadata.position = global.position;
                metadata.phase = global.phase;
                assert_eq!(&metadata, global, "pan metadata");
            }
            _ => panic!("same event family"),
        }
    }
}

/// Runs every row, then fails naming the rows that failed.
fn run_rows(table: &str, rows: &[(&str, fn())]) {
    let mut failures = Vec::new();
    for &(name, case) in rows {
        if let Err(payload) = catch_unwind(case) {
            flui_foundation::panic::retain_opaque_payload(payload);
            failures.push(name);
        }
    }
    assert!(failures.is_empty(), "{table} cases failed: {failures:?}");
}

#[test]
fn mouse_tracking_ordering_and_cursor_deferral() {
    run_rows(
        "mouse tracking",
        &[
            (
                "ambient probe reentrant motion",
                ambient_probe_preserves_reentrant_motion,
            ),
            (
                "ambient probe same-position reentry",
                ambient_probe_preserves_same_position_reentry,
            ),
            (
                "ambient probe removed source readmission",
                ambient_probe_preserves_removed_and_readmitted_source,
            ),
            (
                "ambient probe window-left readmission",
                ambient_probe_preserves_left_and_readmitted_source,
            ),
            (
                "ambient refresh preserves latest physical cursor owner",
                ambient_refresh_preserves_the_latest_physical_cursor_owner,
            ),
            (
                "removed cursor owner publication failure and recovery",
                removed_cursor_owner_failure_keeps_default_publication_deliverable,
            ),
            (
                "latest cursor observation, replacement and failure recovery",
                latest_cursor_publication_survives_reentry_replacement_and_failure,
            ),
            (
                "stationary device follows layout",
                stationary_device_follows_layout_without_duplicates,
            ),
            (
                "deferring cursor",
                deferring_child_shows_the_ancestor_cursor,
            ),
            ("no cursor request", path_without_requests_shows_the_arrow),
        ],
    );
}

#[test]
fn shared_region_exit_per_device() {
    run_rows(
        "shared region",
        &[(
            "shared region exits once per device",
            shared_region_exits_once_per_device,
        )],
    );
}

#[test]
fn ambient_refresh_contains_each_device() {
    run_rows(
        "ambient refresh",
        &[
            (
                "refresh callback panic",
                refresh_callback_panic_reaches_every_device,
            ),
            (
                "refresh hit-test panic",
                refresh_hit_test_panic_keeps_the_device_for_the_next_refresh,
            ),
            (
                "probe and callback compete",
                refresh_hit_test_failure_precedes_a_competing_callback_failure,
            ),
        ],
    );
}

#[test]
fn released_region_destructor_reenters_tracker() {
    run_rows(
        "region release",
        &[
            (
                "region destructor reenters",
                region_destructor_may_reenter_the_tracker,
            ),
            (
                "capture failure and recovery",
                region_retirement_preserves_first_failure_and_recovers,
            ),
        ],
    );
}

#[test]
fn explicit_arrow_cursor_wins() {
    run_rows(
        "explicit arrow",
        &[
            (
                "explicit arrow cursor",
                explicit_arrow_overrides_an_ancestor_cursor,
            ),
            ("tracker explicit arrow", tracker_reports_the_explicit_arrow),
        ],
    );
}

#[test]
fn transformed_entry_receives_local_samples_and_deltas() {
    run_rows(
        "localization",
        &[
            ("move samples localized", move_samples_are_localized),
            (
                "perspective vectors and symbolic counts",
                perspective_vectors_follow_the_current_focal_and_preserve_counts,
            ),
            (
                "scroll delta localized",
                scroll_delta_is_localized_as_a_vector,
            ),
            (
                "scroll target delta localized",
                scroll_target_delta_is_localized_as_a_vector,
            ),
        ],
    );
}
