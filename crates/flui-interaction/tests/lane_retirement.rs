//! Terminal lane ownership follows ADR-0127 through the public dispatch seam.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use flui_foundation::geometry::Offset;
use flui_interaction::events::{PointerKind, make_down_event};
use flui_interaction::pointer::{
    EventPropagation, MouseRegionCallbacks, MouseTracker, MouseTrackerAnnotation, PointerMotionKind,
};
use flui_interaction::{
    HitTestEntry, InteractionDispatchError, InteractionDispatchHandle, InteractionLane, RenderId,
};
use flui_painting::paint::{Path, Shader};
use flui_painting::styling::Color;

#[derive(Clone, Copy, Debug)]
enum Family {
    Pointer,
    MouseEnter,
    MouseExit,
    MouseHover,
    Scroll,
    PanZoom,
    Clip,
    Mask,
    Payload,
    Route,
}

const FAMILIES: [Family; 10] = [
    Family::Pointer,
    Family::MouseEnter,
    Family::MouseExit,
    Family::MouseHover,
    Family::Scroll,
    Family::PanZoom,
    Family::Clip,
    Family::Mask,
    Family::Payload,
    Family::Route,
];

struct Capture {
    log: Rc<RefCell<Vec<&'static str>>>,
    name: &'static str,
    fails: bool,
    reenter: Option<Box<dyn Fn()>>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.log.borrow_mut().push(self.name);
        if let Some(reenter) = &self.reenter {
            reenter();
        }
        if self.fails {
            std::panic::panic_any(self.name);
        }
    }
}

fn capture(log: &Rc<RefCell<Vec<&'static str>>>, name: &'static str, fails: bool) -> Capture {
    Capture {
        log: Rc::clone(log),
        name,
        fails,
        reenter: None,
    }
}

fn register(handle: &InteractionDispatchHandle, family: Family, capture: Capture) {
    match family {
        Family::Pointer | Family::Route => {
            let target = handle
                .register_pointer(move |_| {
                    let _ = &capture;
                })
                .expect("pointer");
            if matches!(family, Family::Route) {
                handle
                    .resolve_pointer_route(&[
                        HitTestEntry::new(RenderId::new(1)).pointer_target(target)
                    ])
                    .expect("saved route");
                handle
                    .unregister_pointer(target)
                    .expect("route owns capture");
            }
        }
        Family::MouseEnter | Family::MouseExit | Family::MouseHover => {
            let callback = Rc::new(move |_, _| {
                let _ = &capture;
            });
            let mut callbacks = MouseRegionCallbacks::default();
            match family {
                Family::MouseEnter => callbacks.on_enter = Some(callback),
                Family::MouseExit => callbacks.on_exit = Some(callback),
                _ => callbacks.on_hover = Some(callback),
            }
            handle.register_mouse_region(callbacks).expect("mouse");
        }
        Family::Scroll => {
            handle
                .register_scroll(move |_| {
                    let _ = &capture;
                    EventPropagation::Continue
                })
                .expect("scroll");
        }
        Family::PanZoom => {
            handle
                .register_pan_zoom(move |_| {
                    let _ = &capture;
                    EventPropagation::Continue
                })
                .expect("pan zoom");
        }
        Family::Clip => {
            handle
                .register_path_clipper(move |_| {
                    let _ = &capture;
                    Path::new()
                })
                .expect("clipper");
        }
        Family::Mask => {
            handle
                .register_shader_mask(move |_| {
                    let _ = &capture;
                    Shader::solid(Color::BLACK)
                })
                .expect("shader");
        }
        Family::Payload => {
            handle
                .register_local_payload(Rc::new(capture))
                .expect("payload");
        }
    }
}

fn assert_surviving_lane(lane: &InteractionLane) {
    let handle = lane.dispatch_handle();
    let delivered = Rc::new(Cell::new(0));
    lane.enter(|| {
        let count = Rc::clone(&delivered);
        let target = handle
            .register_pointer(move |_| count.set(count.get() + 1))
            .expect("survivor target");
        let token = handle
            .resolve_pointer_route(&[HitTestEntry::new(RenderId::new(1)).pointer_target(target)])
            .expect("survivor route")
            .token();
        let event = make_down_event(Offset::ZERO, PointerKind::Touch).expect("event");
        assert!(
            handle
                .invoke_pointer_route(token, &event)
                .expect("dispatch")
                .is_none()
        );
        assert_eq!(delivered.get(), 1);
        handle.release_route(token).expect("release");
        handle.unregister_pointer(target).expect("unregister");
    });
}

fn healthy_every_storage_family() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    lane.enter(|| {
        for family in FAMILIES {
            register(&handle, family, capture(&log, "healthy", false));
        }
    });
    drop(lane);
    assert_eq!(log.borrow().as_slice(), ["healthy"; 10]);
    assert_eq!(
        handle.check_ui_runtime(),
        Err(InteractionDispatchError::OwnerGone)
    );
}

fn failures_across_storage_families() {
    for family in FAMILIES {
        for competing in [false, true] {
            let survivor = InteractionLane::try_new().expect("survivor");
            let log = Rc::new(RefCell::new(Vec::new()));
            let lane = InteractionLane::try_new().expect("lane");
            let handle = lane.dispatch_handle();
            lane.enter(|| {
                register(&handle, family, capture(&log, "first", true));
                register(&handle, family, capture(&log, "tail", competing));
            });
            let failure = catch_unwind(AssertUnwindSafe(|| drop(lane))).expect_err("first failure");
            assert_eq!(failure.downcast_ref::<&str>(), Some(&"first"), "{family:?}");
            assert_eq!(
                log.borrow().as_slice(),
                ["first"],
                "{family:?}: tail retained"
            );
            assert_eq!(
                handle.check_ui_runtime(),
                Err(InteractionDispatchError::OwnerGone)
            );
            assert_surviving_lane(&survivor);
        }
    }
}

fn first_failure_covers_later_storage_families() {
    let survivor = InteractionLane::try_new().expect("survivor");
    let log = Rc::new(RefCell::new(Vec::new()));
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    lane.enter(|| {
        register(&handle, Family::Route, capture(&log, "first", true));
        for family in FAMILIES {
            register(&handle, family, capture(&log, "tail", true));
        }
    });
    let failure = catch_unwind(AssertUnwindSafe(|| drop(lane))).expect_err("first route failure");
    assert_eq!(failure.downcast_ref::<&str>(), Some(&"first"));
    assert_eq!(log.borrow().as_slice(), ["first"]);
    assert_surviving_lane(&survivor);
}

fn mouse_callbacks_retire_individually() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let lane = InteractionLane::try_new().expect("lane");
    lane.enter(|| {
        let first = capture(&log, "enter", true);
        let second = capture(&log, "exit", true);
        let third = capture(&log, "hover", true);
        lane.dispatch_handle()
            .register_mouse_region(MouseRegionCallbacks {
                on_enter: Some(Rc::new(move |_, _| {
                    let _ = &first;
                })),
                on_exit: Some(Rc::new(move |_, _| {
                    let _ = &second;
                })),
                on_hover: Some(Rc::new(move |_, _| {
                    let _ = &third;
                })),
            })
            .expect("mouse callbacks");
    });
    let failure = catch_unwind(AssertUnwindSafe(|| drop(lane))).expect_err("enter failure");
    assert_eq!(failure.downcast_ref::<&str>(), Some(&"enter"));
    assert_eq!(log.borrow().as_slice(), ["enter"]);
}

fn existing_unwind_retains_every_family() {
    let survivor = InteractionLane::try_new().expect("survivor");
    let log = Rc::new(RefCell::new(Vec::new()));
    let failure = catch_unwind(AssertUnwindSafe(|| {
        let lane = InteractionLane::try_new().expect("lane");
        lane.enter(|| {
            for family in FAMILIES {
                register(
                    &lane.dispatch_handle(),
                    family,
                    capture(&log, "unsafe drop", true),
                );
            }
        });
        std::panic::panic_any("body failure");
    }))
    .expect_err("body failure");
    assert_eq!(failure.downcast_ref::<&str>(), Some(&"body failure"));
    assert!(log.borrow().is_empty());
    assert_surviving_lane(&survivor);
}

fn non_last_payload_owners_are_released() {
    for unwind in [false, true] {
        let log = Rc::new(RefCell::new(Vec::new()));
        let saved = Rc::new(capture(&log, "saved", false));
        let failure = catch_unwind(AssertUnwindSafe(|| {
            let lane = InteractionLane::try_new().expect("lane");
            lane.enter(|| {
                for _ in 0..2 {
                    lane.dispatch_handle()
                        .register_local_payload(saved.clone())
                        .expect("shared payload");
                }
                if !unwind {
                    register(
                        &lane.dispatch_handle(),
                        Family::Pointer,
                        capture(&log, "first", true),
                    );
                }
            });
            if unwind {
                std::panic::panic_any("body failure");
            }
            drop(lane);
        }))
        .expect_err("failure");
        assert_eq!(
            failure.downcast_ref::<&str>(),
            Some(&if unwind { "body failure" } else { "first" })
        );
        drop(saved);
        assert_eq!(
            log.borrow().last(),
            Some(&"saved"),
            "remaining external owner releases normally"
        );
    }
}

fn non_last_mouse_callbacks_are_released() {
    for unwind in [false, true] {
        for cached in [false, true] {
            let log = Rc::new(RefCell::new(Vec::new()));
            let tracker = MouseTracker::new();
            let region = RenderId::new(1);
            let saved_capture = capture(&log, "saved", false);
            let saved: flui_interaction::pointer::MouseEnterCallback = Rc::new(move |_, _| {
                let _ = &saved_capture;
            });
            let failure = catch_unwind(AssertUnwindSafe(|| {
                let lane = InteractionLane::try_new().expect("lane");
                lane.enter(|| {
                    let target = lane
                        .dispatch_handle()
                        .register_mouse_region(MouseRegionCallbacks {
                            on_enter: Some(Rc::clone(&saved)),
                            on_exit: Some(Rc::clone(&saved)),
                            on_hover: Some(Rc::clone(&saved)),
                        })
                        .expect("shared callbacks");
                    if cached {
                        tracker.register_annotation(MouseTrackerAnnotation::new(region, target));
                    }
                    if !unwind {
                        register(
                            &lane.dispatch_handle(),
                            Family::Pointer,
                            capture(&log, "first", true),
                        );
                    }
                });
                if unwind {
                    std::panic::panic_any("body failure");
                }
                drop(lane);
            }))
            .expect_err("failure");
            assert_eq!(
                failure.downcast_ref::<&str>(),
                Some(&if unwind { "body failure" } else { "first" })
            );
            tracker.unregister_annotation(region);
            drop(saved);
            assert_eq!(log.borrow().last(), Some(&"saved"));
        }
    }
}

fn retirement_reentry_observes_closed_lane() {
    for fails in [false, true] {
        let survivor = Rc::new(InteractionLane::try_new().expect("survivor"));
        let log = Rc::new(RefCell::new(Vec::new()));
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let observed = Rc::new(Cell::new(false));
        let observer = Rc::clone(&observed);
        let surviving = Rc::clone(&survivor);
        let closed = handle.clone();
        let mut value = capture(&log, "reenter", fails);
        value.reenter = Some(Box::new(move || {
            assert_eq!(
                closed.check_ui_runtime(),
                Err(InteractionDispatchError::OwnerGone)
            );
            assert_eq!(
                closed.register_pointer(|_| {}),
                Err(InteractionDispatchError::OwnerGone)
            );
            assert_surviving_lane(&surviving);
            observer.set(true);
        }));
        lane.enter(|| register(&handle, Family::Payload, value));
        let result = catch_unwind(AssertUnwindSafe(|| drop(lane)));
        if fails {
            assert_eq!(
                result
                    .expect_err("reentrant destructor failure")
                    .downcast_ref::<&str>(),
                Some(&"reenter")
            );
        } else {
            result.expect("healthy reentrant destructor");
        }
        assert!(observed.get());
        assert_eq!(log.borrow().as_slice(), ["reenter"]);
        assert_surviving_lane(&survivor);
    }
}

pub(super) fn activation_restores_exact_owner() {
    let outer = InteractionLane::try_new().expect("outer");
    let inner = InteractionLane::try_new().expect("inner");
    let outer_handle = outer.dispatch_handle();
    let inner_handle = inner.dispatch_handle();
    assert_eq!(
        outer_handle.check_ui_runtime(),
        Err(InteractionDispatchError::InactiveRuntime)
    );
    let foreign_thread = outer_handle.clone();
    assert_eq!(
        std::thread::spawn(move || foreign_thread.check_ui_runtime())
            .join()
            .expect("thread"),
        Err(InteractionDispatchError::WrongThread)
    );
    outer.enter(|| {
        inner.enter(|| assert_eq!(inner_handle.check_ui_runtime(), Ok(())));
        assert_eq!(outer_handle.check_ui_runtime(), Ok(()));
        let target = outer_handle.register_pointer(|_| {}).expect("outer target");
        let failure = catch_unwind(AssertUnwindSafe(|| {
            inner.enter(|| {
                assert_eq!(
                    outer_handle.check_ui_runtime(),
                    Err(InteractionDispatchError::WrongRuntime)
                );
                assert_eq!(
                    inner_handle.unregister_pointer(target),
                    Err(InteractionDispatchError::WrongRuntime)
                );
                assert_eq!(inner_handle.check_ui_runtime(), Ok(()));
                std::panic::panic_any("nested activation");
            })
        }))
        .expect_err("nested failure");
        assert_eq!(failure.downcast_ref::<&str>(), Some(&"nested activation"));
        assert_eq!(outer_handle.check_ui_runtime(), Ok(()));
        assert_eq!(
            inner_handle.check_ui_runtime(),
            Err(InteractionDispatchError::WrongRuntime)
        );
        outer_handle
            .unregister_pointer(target)
            .expect("foreign attempt left target intact");
    });
    assert_eq!(
        inner_handle.check_ui_runtime(),
        Err(InteractionDispatchError::InactiveRuntime)
    );
    assert_surviving_lane(&outer);
    assert_surviving_lane(&inner);
}

fn saved_mouse_cell_follows_terminal_retirement_policy() {
    for (preserving, unwind) in [(false, false), (true, false), (true, true)] {
        let log = Rc::new(RefCell::new(Vec::new()));
        let tracker = MouseTracker::new();
        let region = RenderId::new(1);
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let lane = InteractionLane::try_new().expect("lane");
            lane.enter(|| {
                let saved = capture(&log, "saved mouse", false);
                let target = lane
                    .dispatch_handle()
                    .register_mouse_region(MouseRegionCallbacks {
                        on_enter: Some(Rc::new(move |_, _| {
                            let _ = &saved;
                        })),
                        ..MouseRegionCallbacks::default()
                    })
                    .expect("mouse target");
                tracker.register_annotation(MouseTrackerAnnotation::new(region, target));
                if preserving && !unwind {
                    register(
                        &lane.dispatch_handle(),
                        Family::Pointer,
                        capture(&log, "first", true),
                    );
                }
            });
            if unwind {
                std::panic::panic_any("body failure");
            }
            drop(lane);
        }));
        if preserving {
            let failure = outcome.expect_err("failure");
            assert_eq!(
                failure.downcast_ref::<&str>(),
                Some(&if unwind { "body failure" } else { "first" })
            );
        } else {
            outcome.expect("healthy close");
        }
        assert!(
            !log.borrow().contains(&"saved mouse"),
            "tracker still owns callback"
        );
        tracker.unregister_annotation(region);
        assert_eq!(
            log.borrow().contains(&"saved mouse"),
            !preserving,
            "last framework snapshot follows its withdrawn capture policy"
        );
    }
}

fn late_mouse_retirement_preserves_original_lane_failure() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let tracker = MouseTracker::new();
    let lane = InteractionLane::try_new().expect("lane");
    let region = RenderId::new(1);
    lane.enter(|| {
        let value = capture(&log, "late mouse destructor", true);
        let target = lane
            .dispatch_handle()
            .register_mouse_region(MouseRegionCallbacks {
                on_enter: Some(Rc::new(move |_, _| {
                    let _ = &value;
                })),
                ..MouseRegionCallbacks::default()
            })
            .expect("mouse target");
        tracker.register_annotation(MouseTrackerAnnotation::new(region, target));
        register(
            &lane.dispatch_handle(),
            Family::Pointer,
            capture(&log, "original lane failure", true),
        );
    });
    let original =
        catch_unwind(AssertUnwindSafe(|| drop(lane))).expect_err("original lane failure");
    assert_eq!(
        original.downcast_ref::<&str>(),
        Some(&"original lane failure")
    );
    let late = catch_unwind(AssertUnwindSafe(|| tracker.unregister_annotation(region)));
    assert!(
        late.is_ok(),
        "a late final framework snapshot must retain captures from the failed close"
    );
    assert_eq!(log.borrow().as_slice(), ["original lane failure"]);
    assert_surviving_lane(&InteractionLane::try_new().expect("survivor"));
}

fn raw_rejection_observes_only_the_active_close_window() {
    use flui_interaction::__runtime::CloseWindow;
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut window = CloseWindow::new();
    window.dispatch(&handle);
    let original =
        catch_unwind(|| std::panic::panic_any("outer close failure")).expect_err("outer failure");
    window.preserve();
    drop(lane);
    let rejected = capture(&log, "unsafe rejected capture", true);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        handle.register_pointer(move |_| {
            let _ = &rejected;
        })
    }));
    assert_eq!(
        outcome.expect("active preserving close must not destroy rejected capture"),
        Err(InteractionDispatchError::OwnerGone)
    );
    assert!(log.borrow().is_empty());
    assert_eq!(
        original.downcast_ref::<&str>(),
        Some(&"outer close failure")
    );
    drop(window);
    let healthy = capture(&log, "healthy post-close rejection", false);
    assert_eq!(
        handle.register_pointer(move |_| {
            let _ = &healthy;
        }),
        Err(InteractionDispatchError::OwnerGone)
    );
    assert_eq!(log.borrow().as_slice(), ["healthy post-close rejection"]);
    assert_surviving_lane(&InteractionLane::try_new().expect("survivor"));
}

fn saved_routes_are_stale_after_exceptional_retirement() {
    let survivor = InteractionLane::try_new().expect("survivor");
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let calls = Rc::new(Cell::new(0));
    let log = Rc::new(RefCell::new(Vec::new()));
    let token = lane.enter(|| {
        register(&handle, Family::Pointer, capture(&log, "first", true));
        let count = Rc::clone(&calls);
        let target = handle
            .register_pointer(move |_| count.set(count.get() + 1))
            .expect("target");
        handle
            .resolve_pointer_route(&[HitTestEntry::new(RenderId::new(1)).pointer_target(target)])
            .expect("route")
            .token()
    });
    let failure = catch_unwind(AssertUnwindSafe(|| drop(lane))).expect_err("first failure");
    assert_eq!(failure.downcast_ref::<&str>(), Some(&"first"));
    let event = make_down_event(Offset::ZERO, PointerKind::Touch).expect("event");
    assert!(matches!(
        handle.invoke_pointer_route(token, &event),
        Err(InteractionDispatchError::OwnerGone)
    ));
    survivor.enter(|| {
        assert!(matches!(
            survivor
                .dispatch_handle()
                .invoke_pointer_route(token, &event),
            Err(InteractionDispatchError::WrongRuntime)
        ));
    });
    assert_eq!(
        calls.get(),
        0,
        "saved identity cannot deliver to retired captures"
    );
    assert_surviving_lane(&survivor);
}

fn cached_mouse_exit_cannot_outlive_its_lane() {
    use flui_interaction::HitTestResult;
    use flui_interaction::events::make_move_event;
    for under_surviving_lane in [false, true] {
        let lane = InteractionLane::try_new().expect("lane");
        let survivor = InteractionLane::try_new().expect("survivor");
        let tracker = MouseTracker::new();
        let exits = Rc::new(Cell::new(0));
        let movement = make_move_event(Offset::ZERO, PointerKind::Mouse).expect("movement");
        lane.enter(|| {
            let count = Rc::clone(&exits);
            let target = lane
                .dispatch_handle()
                .register_mouse_region(MouseRegionCallbacks {
                    on_exit: Some(Rc::new(move |_, _| count.set(count.get() + 1))),
                    ..MouseRegionCallbacks::default()
                })
                .expect("mouse region");
            let mut path = HitTestResult::new();
            path.add(
                HitTestEntry::new(RenderId::new(1))
                    .mouse_annotation(MouseTrackerAnnotation::new(RenderId::new(1), target)),
            );
            tracker.update_with_motion(&movement, PointerMotionKind::Hover, &path);
        });
        drop(lane);
        let leave = || {
            tracker.update_with_motion(&movement, PointerMotionKind::Hover, &HitTestResult::new());
        };
        if under_surviving_lane {
            survivor.enter(leave);
        } else {
            leave();
        }
        assert_eq!(exits.get(), 0, "closed-lane cached exit must not execute");
        assert_surviving_lane(&survivor);
    }
}

fn snapshotted_mouse_exits_recheck_lane_after_reentry() {
    use flui_interaction::HitTestResult;
    use flui_interaction::events::make_move_event;
    let owner = Rc::new(RefCell::new(Some(
        InteractionLane::try_new().expect("lane"),
    )));
    let tracker = MouseTracker::new();
    let log = Rc::new(RefCell::new(Vec::new()));
    owner.borrow().as_ref().expect("owner").enter(|| {
        let lane = owner.borrow();
        let handle = lane.as_ref().expect("owner").dispatch_handle();
        let mut path = HitTestResult::new();
        for (index, name) in [(1, "first"), (2, "second")] {
            let close = Rc::clone(&owner);
            let calls = Rc::clone(&log);
            let target = handle
                .register_mouse_region(MouseRegionCallbacks {
                    on_exit: Some(Rc::new(move |_, _| {
                        calls.borrow_mut().push(name);
                        if name == "first" {
                            let lane = close.borrow_mut().take();
                            drop(lane);
                        }
                    })),
                    ..MouseRegionCallbacks::default()
                })
                .expect("mouse region");
            path.add(
                HitTestEntry::new(RenderId::new(index))
                    .mouse_annotation(MouseTrackerAnnotation::new(RenderId::new(index), target)),
            );
        }
        let movement = make_move_event(Offset::ZERO, PointerKind::Mouse).expect("movement");
        tracker.update_with_motion(&movement, PointerMotionKind::Hover, &path);
    });
    tracker.dispatch_window_left();
    assert_eq!(
        log.borrow().as_slice(),
        ["first"],
        "later admitted snapshots observe terminal lane authority"
    );
    let survivor = InteractionLane::try_new().expect("survivor");
    assert_surviving_lane(&survivor);
}

#[test]
fn lane_snapshot_authority_contract() {
    let cases: &[(&str, fn())] = &[
        (
            "cached_mouse_exit_cannot_outlive_its_lane",
            cached_mouse_exit_cannot_outlive_its_lane,
        ),
        (
            "snapshotted_mouse_exits_recheck_lane_after_reentry",
            snapshotted_mouse_exits_recheck_lane_after_reentry,
        ),
    ];
    for &(name, case) in cases {
        if let Err(failure) = catch_unwind(case) {
            eprintln!("lane snapshot authority case `{name}` failed");
            std::panic::resume_unwind(failure);
        }
    }
}

fn competing_payloads() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    lane.enter(|| {
        for name in ["first", "second"] {
            handle
                .register_local_payload(Rc::new(Capture {
                    log: Rc::clone(&log),
                    name,
                    fails: true,
                    reenter: None,
                }))
                .expect("payload");
        }
    });
    let failure = catch_unwind(AssertUnwindSafe(|| drop(lane)))
        .expect_err("first destructor failure propagates");
    assert_eq!(failure.downcast_ref::<&str>(), Some(&"first"));
    assert_eq!(*log.borrow(), ["first"], "the failing tail is retained");
}

#[test]
fn lane_terminal_retirement_contract() {
    if std::env::var_os("FLUI_LANE_RETIREMENT_CHILD").is_some() {
        let cases: &[(&str, fn())] = &[
            ("healthy_every_storage_family", healthy_every_storage_family),
            ("competing_payloads", competing_payloads),
            (
                "failures_across_storage_families",
                failures_across_storage_families,
            ),
            (
                "first_failure_covers_later_storage_families",
                first_failure_covers_later_storage_families,
            ),
            (
                "mouse_callbacks_retire_individually",
                mouse_callbacks_retire_individually,
            ),
            (
                "existing_unwind_retains_every_family",
                existing_unwind_retains_every_family,
            ),
            (
                "non_last_payload_owners_are_released",
                non_last_payload_owners_are_released,
            ),
            (
                "non_last_mouse_callbacks_are_released",
                non_last_mouse_callbacks_are_released,
            ),
            (
                "saved_mouse_cell_follows_terminal_retirement_policy",
                saved_mouse_cell_follows_terminal_retirement_policy,
            ),
            (
                "late_mouse_retirement_preserves_original_lane_failure",
                late_mouse_retirement_preserves_original_lane_failure,
            ),
            (
                "raw_rejection_observes_only_the_active_close_window",
                raw_rejection_observes_only_the_active_close_window,
            ),
            (
                "saved_routes_are_stale_after_exceptional_retirement",
                saved_routes_are_stale_after_exceptional_retirement,
            ),
            (
                "retirement_reentry_observes_closed_lane",
                retirement_reentry_observes_closed_lane,
            ),
        ];
        for &(name, case) in cases {
            if std::env::var("FLUI_LANE_RETIREMENT_CASE").is_ok_and(|selected| selected != name) {
                continue;
            }
            if let Err(failure) = catch_unwind(case) {
                eprintln!("lane retirement case `{name}` failed");
                std::panic::resume_unwind(failure);
            }
        }
        return;
    }
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let mut child = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "lane_retirement::lane_terminal_retirement_contract",
            "--nocapture",
        ])
        .env("FLUI_LANE_RETIREMENT_CHILD", "competing")
        .env("RUST_BACKTRACE", "0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("retirement child");
    let mut stdout = child.stdout.take().expect("child stdout");
    let mut stderr = child.stderr.take().expect("child stderr");
    let output = std::thread::spawn(move || {
        let mut text = String::new();
        stdout.read_to_string(&mut text).expect("stdout");
        text
    });
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        stderr.read_to_string(&mut text).expect("stderr");
        text
    });
    let started = Instant::now();
    while child.try_wait().expect("child status").is_none() {
        if started.elapsed() > Duration::from_secs(60) {
            child.kill().expect("kill stalled child");
            panic!("lane retirement child did not make progress within 60 seconds");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let status = child.wait().expect("child exit");
    assert!(
        status.success(),
        "retirement child failed:\n{}\n{}",
        output.join().expect("stdout reader"),
        errors.join().expect("stderr reader")
    );
}
