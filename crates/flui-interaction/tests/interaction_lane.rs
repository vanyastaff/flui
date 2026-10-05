//! Public-contract tests for the inert ADR-0027 owner-local interaction lane.

use flui_interaction::routing::MouseRegionTarget;
use flui_interaction::{
    HitTestEntry, InteractionDispatchError, InteractionDispatchHandle, InteractionLane,
    PointerTarget, RenderId, ResolvedRouteToken,
};
use static_assertions::{assert_impl_all, assert_not_impl_any};

/// A transform-less hit entry addressing `target`, for resolver tests.
fn hit_entry(target: PointerTarget) -> HitTestEntry {
    HitTestEntry::new(RenderId::new(1)).pointer_target(target)
}

assert_not_impl_any!(InteractionLane: Send, Sync);
assert_impl_all!(InteractionDispatchHandle: Clone, Send, Sync);
assert_impl_all!(PointerTarget: Copy, Send, Sync);
assert_impl_all!(MouseRegionTarget: Copy, Send, Sync);
assert_impl_all!(ResolvedRouteToken: Copy, Send, Sync);

// Lane capability matrix: least-privilege handle and realm recreation.
#[test]
fn lane_capability_matrix() {
    let cases: &[(&str, fn())] = &[
        (
            "lane_mints_a_send_safe_least_privilege_handle",
            lane_mints_a_send_safe_least_privilege_handle,
        ),
        (
            "realm_recreation_rejects_every_old_capability",
            realm_recreation_rejects_every_old_capability,
        ),
    ];
    for &(name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            eprintln!("matrix case `{name}` failed");
            std::panic::resume_unwind(payload);
        }
    }
}

fn lane_mints_a_send_safe_least_privilege_handle() {
    let lane = InteractionLane::try_new().expect("lane identity should be available");
    let handle = lane.dispatch_handle();
    assert_eq!(format!("{handle:?}"), "InteractionDispatchHandle { .. }");
}

fn realm_recreation_rejects_every_old_capability() {
    let old_lane = InteractionLane::try_new().expect("old lane");
    let old_handle = old_lane.dispatch_handle();
    let (old_target, old_route) = old_lane.enter(|| {
        let target = old_handle.register_pointer(|_| {}).expect("old target");
        let route = old_handle
            .resolve_pointer_route(&[hit_entry(target)])
            .expect("old route")
            .token();
        (target, route)
    });
    drop(old_lane);

    assert_eq!(
        old_handle.register_pointer(|_| {}),
        Err(InteractionDispatchError::OwnerGone)
    );

    let replacement_lane = InteractionLane::try_new().expect("replacement lane");
    let replacement_handle = replacement_lane.dispatch_handle();
    replacement_lane.enter(|| {
        assert_eq!(
            replacement_handle.unregister_pointer(old_target),
            Err(InteractionDispatchError::WrongRealm)
        );
        assert_eq!(
            replacement_handle.release_route(old_route),
            Err(InteractionDispatchError::WrongRealm)
        );
    });
}

// ---------------------------------------------------------------------------
// Fresh hit test — the capability a drag needs to discover targets it has moved
// over, which a replayed pointer-down route can never see.
//
// The handle pairs a realm ticket with ONE presentation's probe: identity is
// realm-wide, the tree is not, and a realm may host several presentations each
// with its own render tree.
// ---------------------------------------------------------------------------

#[test]
fn pointer_route_retirement_reenters_and_preserves_the_next_contact() {
    let cases: &[(&str, fn())] = &[
        ("capture_reentry", pointer_route_capture_reentry),
        (
            "capture_panic_after_reentry",
            pointer_route_capture_panic_after_reentry,
        ),
    ];
    let mut failures = Vec::new();
    for &(name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            flui_foundation::panic::retain_opaque_payload(payload);
            failures.push(name);
        }
    }
    assert!(failures.is_empty(), "failed retirement rows: {failures:?}");
}

fn pointer_route_capture_reentry() {
    assert_pointer_route_retirement(false);
}

fn pointer_route_capture_panic_after_reentry() {
    assert_pointer_route_retirement(true);
}

fn assert_pointer_route_retirement(panic_after_reentry: bool) {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::{Rc, Weak};

    use flui_interaction::events::{PointerType, make_down_event, make_up_event};
    use flui_interaction::{GestureBinding, HitTestResult, Offset, PointerId};

    struct RetireRoute {
        binding: Weak<GestureBinding>,
        deliveries: Rc<Cell<usize>>,
        retired: Rc<Cell<bool>>,
        panic_after_reentry: bool,
    }

    impl Drop for RetireRoute {
        fn drop(&mut self) {
            let binding = self.binding.upgrade().expect("binding outlives removal");
            let router = binding.pointer_router();
            assert!(
                !router.has_routes(PointerId::PRIMARY),
                "the retired route must already be absent during destruction"
            );
            // Removing an already-absent ID must also be reentrant.
            router.remove_all_routes(PointerId::PRIMARY);
            let deliveries = Rc::clone(&self.deliveries);
            router.add_route(
                PointerId::PRIMARY,
                Rc::new(move |_| deliveries.set(deliveries.get() + 1)),
            );
            self.retired.set(true);
            assert!(!self.panic_after_reentry, "retired route capture panic");
        }
    }

    let binding = Rc::new(GestureBinding::new());
    let deliveries = Rc::new(Cell::new(0));
    let retired = Rc::new(Cell::new(false));
    let retire = RetireRoute {
        binding: Rc::downgrade(&binding),
        deliveries: Rc::clone(&deliveries),
        retired: Rc::clone(&retired),
        panic_after_reentry,
    };
    binding.pointer_router().add_route(
        PointerId::PRIMARY,
        Rc::new(move |_| {
            let _keep_capture_alive = &retire;
            panic!("a retired route must not receive the next contact");
        }),
    );

    let removal = catch_unwind(AssertUnwindSafe(|| {
        binding
            .pointer_router()
            .remove_all_routes(PointerId::PRIMARY);
    }));
    if panic_after_reentry {
        let payload = removal.expect_err("capture panic must propagate");
        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&"retired route capture panic")
        );
    } else {
        removal.expect("capture reentry must not borrow an occupied registry");
    }
    assert!(
        retired.get(),
        "capture must finish its reentrant replacement"
    );

    // Deliver a real new contact through the production binding, rather than
    // checking only registry counters or calling the replacement directly.
    binding.handle_pointer_event(&make_down_event(Offset::ZERO, PointerType::Touch), |_| {
        HitTestResult::new()
    });
    binding.handle_pointer_event(&make_up_event(Offset::ZERO, PointerType::Touch), |_| {
        HitTestResult::new()
    });
    assert_eq!(deliveries.get(), 2);
    binding
        .pointer_router()
        .remove_all_routes(PointerId::PRIMARY);
}

#[derive(Clone, Copy, Debug)]
enum RouterCleanup {
    Pointer,
    Global,
    All,
}

impl RouterCleanup {
    fn remove(self, router: &flui_interaction::PointerRouter) {
        match self {
            Self::Pointer => router.remove_all_routes(flui_interaction::PointerId::PRIMARY),
            Self::Global => router.clear_global_handlers(),
            Self::All => router.clear(),
        }
    }
}

#[test]
fn pointer_router_competing_retirement_preserves_first_failure_and_recovery() {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    const SELECTED: &str = "FLUI_POINTER_ROUTER_RETIREMENT_CASE";
    let cases = [
        ("pointer_one", RouterCleanup::Pointer, false, false),
        ("pointer_two", RouterCleanup::Pointer, true, false),
        ("pointer_unwind", RouterCleanup::Pointer, true, true),
        ("global_one", RouterCleanup::Global, false, false),
        ("global_two", RouterCleanup::Global, true, false),
        ("global_unwind", RouterCleanup::Global, true, true),
        ("all_one", RouterCleanup::All, false, false),
        ("all_two", RouterCleanup::All, true, false),
        ("all_unwind", RouterCleanup::All, true, true),
    ];
    if let Ok(selected) = std::env::var(SELECTED) {
        if let Some((competing, active_unwind)) = match selected.as_str() {
            "owner_one" => Some((false, false)),
            "owner_two" => Some((true, false)),
            "owner_unwind" => Some((true, true)),
            _ => None,
        } {
            assert_router_owner_retirement(competing, active_unwind);
            return;
        }
        let &(_, cleanup, competing, active_unwind) = cases
            .iter()
            .find(|(name, _, _, _)| *name == selected)
            .expect("known router retirement child case");
        assert_router_retirement_recovery(cleanup, competing, active_unwind);
        return;
    }

    let mut failures = Vec::new();
    for name in
        cases
            .iter()
            .map(|(name, _, _, _)| *name)
            .chain(["owner_one", "owner_two", "owner_unwind"])
    {
        let mut child = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "interaction_lane::pointer_router_competing_retirement_preserves_first_failure_and_recovery",
                "--nocapture",
            ])
            .env(SELECTED, name)
            .env("RUST_BACKTRACE", "0")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("router retirement child");
        let mut stdout = child.stdout.take().expect("child stdout");
        let mut stderr = child.stderr.take().expect("child stderr");
        let stdout_reader = std::thread::spawn(move || {
            let mut text = String::new();
            stdout.read_to_string(&mut text).expect("read child stdout");
            text
        });
        let stderr_reader = std::thread::spawn(move || {
            let mut text = String::new();
            stderr.read_to_string(&mut text).expect("read child stderr");
            text
        });
        let started = Instant::now();
        while child.try_wait().expect("child status").is_none() {
            if started.elapsed() > Duration::from_secs(10) {
                child.kill().expect("kill deadlocked retirement child");
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let status = child.wait().expect("child exit");
        let stdout = stdout_reader.join().expect("stdout reader");
        let stderr = stderr_reader.join().expect("stderr reader");
        if !status.success() || !stdout.contains("1 passed; 0 failed") {
            failures.push(format!("{name}: {status}\n{stdout}\n{stderr}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn assert_router_retirement_recovery(cleanup: RouterCleanup, competing: bool, active_unwind: bool) {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::{Rc, Weak};

    use flui_interaction::events::{PointerType, make_down_event, make_up_event};
    use flui_interaction::{GestureBinding, HitTestResult, Offset, PointerId};

    struct RetireCapture {
        binding: Weak<GestureBinding>,
        drops: Rc<Cell<usize>>,
        deliveries: Rc<Cell<usize>>,
        message: &'static str,
    }
    impl Drop for RetireCapture {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
            let binding = self.binding.upgrade().expect("live binding");
            let deliveries = Rc::clone(&self.deliveries);
            binding.pointer_router().add_route(
                PointerId::PRIMARY,
                Rc::new(move |_| deliveries.set(deliveries.get() + 1)),
            );
            std::panic::panic_any(self.message);
        }
    }
    struct UnwindCleanup {
        binding: Rc<GestureBinding>,
        cleanup: RouterCleanup,
    }
    impl Drop for UnwindCleanup {
        fn drop(&mut self) {
            self.cleanup.remove(self.binding.pointer_router());
        }
    }

    let binding = Rc::new(GestureBinding::new());
    let deliveries = Rc::new(Cell::new(0));
    let first_drops = Rc::new(Cell::new(0));
    let second_drops = Rc::new(Cell::new(0));
    for (index, (drops, message)) in [
        (&first_drops, "first route capture failure"),
        (&second_drops, "second route capture failure"),
    ]
    .into_iter()
    .enumerate()
    {
        if index == 1 && !competing {
            break;
        }
        let capture = RetireCapture {
            binding: Rc::downgrade(&binding),
            drops: Rc::clone(drops),
            deliveries: Rc::clone(&deliveries),
            message,
        };
        let handler: flui_interaction::PointerRouteHandler = Rc::new(move |_| {
            let _keep_capture_alive = &capture;
            panic!("retired callback must not receive the next contact");
        });
        if matches!(cleanup, RouterCleanup::Global)
            || (matches!(cleanup, RouterCleanup::All) && index == 1)
        {
            binding.pointer_router().add_global_handler(handler);
        } else {
            binding
                .pointer_router()
                .add_route(PointerId::PRIMARY, handler);
        }
    }

    // A handler the test still owns sits in the retired tail: dropping the
    // router's clone runs no user code, so retirement must release it.
    let shared: flui_interaction::PointerRouteHandler = Rc::new(|_| {});
    if matches!(cleanup, RouterCleanup::Pointer) {
        binding
            .pointer_router()
            .add_route(PointerId::PRIMARY, Rc::clone(&shared));
    } else {
        binding
            .pointer_router()
            .add_global_handler(Rc::clone(&shared));
    }

    let removal = catch_unwind(AssertUnwindSafe(|| {
        if active_unwind {
            let _cleanup = UnwindCleanup {
                binding: Rc::clone(&binding),
                cleanup,
            };
            panic!("outer failure owns unwind");
        }
        cleanup.remove(binding.pointer_router());
    }));
    let payload = removal.expect_err("the first failure must propagate");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&if active_unwind {
            "outer failure owns unwind"
        } else {
            "first route capture failure"
        })
    );
    assert_eq!(first_drops.get(), usize::from(!active_unwind));
    assert_eq!(second_drops.get(), 0, "the opaque tail must be retained");
    assert_eq!(
        Rc::strong_count(&shared),
        1,
        "a clone another owner still holds is released, not leaked"
    );

    if active_unwind {
        let deliveries = Rc::clone(&deliveries);
        binding.pointer_router().add_route(
            PointerId::PRIMARY,
            Rc::new(move |_| deliveries.set(deliveries.get() + 1)),
        );
    }
    binding.handle_pointer_event(&make_down_event(Offset::ZERO, PointerType::Touch), |_| {
        HitTestResult::new()
    });
    binding.handle_pointer_event(&make_up_event(Offset::ZERO, PointerType::Touch), |_| {
        HitTestResult::new()
    });
    assert_eq!(
        deliveries.get(),
        2,
        "next contact reaches only the healthy route"
    );
    binding.pointer_router().clear();
}

fn assert_router_owner_retirement(competing: bool, active_unwind: bool) {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::{Rc, Weak};

    use flui_interaction::{PointerId, PointerRouter};

    struct OwnerCapture {
        owner: Weak<PointerRouter>,
        drops: Rc<Cell<usize>>,
        missing_owner: Rc<Cell<bool>>,
        message: &'static str,
    }
    impl Drop for OwnerCapture {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
            // Last-owner teardown has no live public handle to reenter. The
            // weak identity must refuse resurrection rather than name a new owner.
            self.missing_owner.set(self.owner.upgrade().is_none());
            std::panic::panic_any(self.message);
        }
    }

    let owner = Rc::new(PointerRouter::new());
    let weak_owner = Rc::downgrade(&owner);
    let first_drops = Rc::new(Cell::new(0));
    let second_drops = Rc::new(Cell::new(0));
    let missing_owner = Rc::new(Cell::new(false));
    for (index, (drops, message)) in [
        (&first_drops, "first owner capture failure"),
        (&second_drops, "second owner capture failure"),
    ]
    .into_iter()
    .enumerate()
    {
        if index == 1 && !competing {
            break;
        }
        let capture = OwnerCapture {
            owner: Rc::downgrade(&owner),
            drops: Rc::clone(drops),
            missing_owner: Rc::clone(&missing_owner),
            message,
        };
        let handler: flui_interaction::PointerRouteHandler = Rc::new(move |_| {
            let _keep_capture_alive = &capture;
        });
        if index == 0 {
            owner.add_route(PointerId::PRIMARY, handler);
        } else {
            owner.add_global_handler(handler);
        }
    }
    let retirement = catch_unwind(AssertUnwindSafe(move || {
        if active_unwind {
            let _last_owner = owner;
            panic!("outer owner failure");
        }
        drop(owner);
    }));
    let payload = retirement.expect_err("original owner failure must propagate");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&if active_unwind {
            "outer owner failure"
        } else {
            "first owner capture failure"
        })
    );
    assert_eq!(first_drops.get(), usize::from(!active_unwind));
    assert_eq!(
        second_drops.get(),
        0,
        "the owner retirement tail is retained"
    );
    assert_eq!(missing_owner.get(), !active_unwind);
    assert!(
        weak_owner.upgrade().is_none(),
        "retired owner cannot resurrect"
    );
}

struct DragRetirementProbe(Box<dyn Fn()>);

impl Drop for DragRetirementProbe {
    fn drop(&mut self) {
        (self.0)();
    }
}

#[test]
fn drag_callback_ownership_and_retirement() {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    const SELECTED: &str = "FLUI_DRAG_CALLBACK_RETIREMENT_CASE";
    let cases: &[(&str, fn())] = &[
        ("dispose reentry", drag_dispose_capture_reentry),
        (
            "all replacement slots",
            drag_callback_replacements_commit_before_retirement,
        ),
        ("closed admission", drag_dispose_closes_callback_admission),
        (
            "two dispose failures",
            drag_dispose_preserves_first_capture_failure,
        ),
        ("dispose active unwind", drag_dispose_during_active_unwind),
        (
            "previous caller failure",
            drag_caller_keeps_previously_caught_failure,
        ),
        (
            "healthy shared owner",
            drag_callbacks_live_until_the_final_shared_owner,
        ),
        (
            "two owner failures",
            drag_final_callback_owner_preserves_first_failure,
        ),
        (
            "owner active unwind",
            drag_final_callback_owner_during_active_unwind,
        ),
        (
            "callback self dispose",
            drag_callback_can_dispose_its_own_recognizer,
        ),
        (
            "callback body and capture",
            drag_callback_body_failure_retains_its_capture,
        ),
        (
            "rejection diagnostic failure",
            drag_disposal_commits_tracking_before_rejection_diagnostics,
        ),
        (
            "stop diagnostic failure",
            drag_completion_commits_tracking_before_stop_diagnostics,
        ),
        (
            "stop sweep reentry",
            stop_tracking_preserves_reentrant_contact_after_sweep,
        ),
        (
            "stop sweep reentry failure",
            stop_tracking_preserves_reentrant_contact_after_sweep_failure,
        ),
    ];
    if let Ok(selected) = std::env::var(SELECTED) {
        cases
            .iter()
            .find(|(name, _)| *name == selected)
            .expect("known drag retirement case")
            .1();
        return;
    }
    let mut failures = Vec::new();
    for &(name, _) in cases {
        let mut child = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "interaction_lane::drag_callback_ownership_and_retirement",
                "--nocapture",
            ])
            .env(SELECTED, name)
            .env("RUST_BACKTRACE", "0")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("drag retirement child");
        let mut stdout = child.stdout.take().expect("child stdout");
        let mut stderr = child.stderr.take().expect("child stderr");
        let stdout_reader = std::thread::spawn(move || {
            let mut text = String::new();
            stdout.read_to_string(&mut text).expect("child stdout");
            text
        });
        let stderr_reader = std::thread::spawn(move || {
            let mut text = String::new();
            stderr.read_to_string(&mut text).expect("child stderr");
            text
        });
        let started = Instant::now();
        while child.try_wait().expect("child status").is_none() {
            if started.elapsed() > Duration::from_secs(10) {
                child.kill().expect("kill deadlocked drag retirement child");
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let status = child.wait().expect("child exit");
        let stdout = stdout_reader.join().expect("stdout reader");
        let stderr = stderr_reader.join().expect("stderr reader");
        if !status.success() || !stdout.contains("1 passed; 0 failed") {
            failures.push(format!("{name}: {status}\n{stdout}\n{stderr}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn fresh_drag_completes_after_retirement() {
    use flui_interaction::arena::GestureArena;
    use flui_interaction::events::{PointerType, make_move_event, make_up_event};
    use flui_interaction::routing::PointerDispatch;
    use flui_interaction::{DragAxis, DragGestureRecognizer, GestureRecognizer, Offset, PointerId};
    use std::{cell::Cell, rc::Rc};

    let arena = GestureArena::new();
    let started = Rc::new(Cell::new(0));
    let ended = Rc::new(Cell::new(0));
    let observed_start = started.clone();
    let observed_end = ended.clone();
    let recognizer = DragGestureRecognizer::new(arena.clone(), DragAxis::Horizontal)
        .with_on_start(move |_| observed_start.set(observed_start.get() + 1))
        .with_on_end(move |_| observed_end.set(observed_end.get() + 1));
    recognizer.add_pointer(PointerId::PRIMARY, Offset::ZERO, Offset::ZERO);
    arena.close(PointerId::PRIMARY);
    arena.drain_deferred_resolutions();
    let movement = make_move_event(Offset::new(30.0, 0.0), PointerType::Touch);
    recognizer.handle_event(PointerDispatch::at_root(&movement));
    let release = make_up_event(Offset::new(30.0, 0.0), PointerType::Touch);
    recognizer.handle_event(PointerDispatch::at_root(&release));
    assert_eq!((started.get(), ended.get()), (1, 1));
    recognizer.dispose();
}

fn drag_dispose_capture_reentry() {
    use flui_interaction::arena::GestureArena;
    use flui_interaction::{DragAxis, DragGestureRecognizer, GestureRecognizer};
    use std::{cell::Cell, rc::Rc, sync::Arc};

    let recognizer = DragGestureRecognizer::new(GestureArena::new(), DragAxis::Horizontal);
    let weak = Arc::downgrade(&recognizer);
    let retired = Rc::new(Cell::new(false));
    let observed = retired.clone();
    let probe = DragRetirementProbe(Box::new(move || {
        let recognizer = weak.upgrade().expect("live public recognizer");
        recognizer.dispose();
        assert!(recognizer.primary_pointer().is_none());
        observed.set(true);
    }));
    let recognizer = recognizer.with_on_down(move |_| {
        let _capture = &probe;
    });
    recognizer.dispose();
    assert!(retired.get());
    recognizer.dispose();
    fresh_drag_completes_after_retirement();
}

fn drag_callback_replacements_commit_before_retirement() {
    use flui_interaction::arena::GestureArena;
    use flui_interaction::{DragAxis, DragGestureRecognizer, GestureRecognizer};
    use std::{cell::Cell, rc::Rc, sync::Arc};

    for slot in 0..5 {
        let recognizer = DragGestureRecognizer::new(GestureArena::new(), DragAxis::Horizontal);
        let weak = Arc::downgrade(&recognizer);
        let retired = Rc::new(Cell::new(false));
        let observed = retired.clone();
        let probe = DragRetirementProbe(Box::new(move || {
            let recognizer = weak.upgrade().expect("replacement pins public owner");
            recognizer.dispose();
            observed.set(true);
        }));
        let recognizer = match slot {
            0 => recognizer.with_on_down(move |_| {
                let _capture = &probe;
            }),
            1 => recognizer.with_on_start(move |_| {
                let _capture = &probe;
            }),
            2 => recognizer.with_on_update(move |_| {
                let _capture = &probe;
            }),
            3 => recognizer.with_on_end(move |_| {
                let _capture = &probe;
            }),
            _ => recognizer.with_on_cancel(move || {
                let _capture = &probe;
            }),
        };
        let recognizer = match slot {
            0 => recognizer.with_on_down(|_| {}),
            1 => recognizer.with_on_start(|_| {}),
            2 => recognizer.with_on_update(|_| {}),
            3 => recognizer.with_on_end(|_| {}),
            _ => recognizer.with_on_cancel(|| {}),
        };
        assert!(retired.get(), "slot {slot} retires the replaced capture");
        recognizer.dispose();
    }
    fresh_drag_completes_after_retirement();
}

fn drag_dispose_closes_callback_admission() {
    use flui_interaction::arena::GestureArena;
    use flui_interaction::{DragAxis, DragGestureRecognizer, GestureRecognizer};
    use std::{cell::Cell, rc::Rc};

    let recognizer = DragGestureRecognizer::new(GestureArena::new(), DragAxis::Horizontal);
    recognizer.dispose();
    let retired = Rc::new(Cell::new(0));
    for slot in 0..5 {
        let observed = retired.clone();
        let probe = DragRetirementProbe(Box::new(move || observed.set(observed.get() + 1)));
        let recognizer = recognizer.clone();
        let _recognizer = match slot {
            0 => recognizer.with_on_down(move |_| {
                let _capture = &probe;
            }),
            1 => recognizer.with_on_start(move |_| {
                let _capture = &probe;
            }),
            2 => recognizer.with_on_update(move |_| {
                let _capture = &probe;
            }),
            3 => recognizer.with_on_end(move |_| {
                let _capture = &probe;
            }),
            _ => recognizer.with_on_cancel(move || {
                let _capture = &probe;
            }),
        };
        assert_eq!(
            retired.get(),
            slot + 1,
            "closed admission retires immediately"
        );
    }
    fresh_drag_completes_after_retirement();
}

fn drag_dispose_preserves_first_capture_failure() {
    drag_independent_capture_failures(false, false);
}
fn drag_dispose_during_active_unwind() {
    drag_independent_capture_failures(false, true);
}
fn drag_final_callback_owner_preserves_first_failure() {
    drag_independent_capture_failures(true, false);
}
fn drag_final_callback_owner_during_active_unwind() {
    drag_independent_capture_failures(true, true);
}

#[expect(
    clippy::arc_with_non_send_sync,
    reason = "the owner-local recognizer API requires Arc aliases to exercise shared callback ownership"
)]
fn drag_independent_capture_failures(final_owner: bool, active_unwind: bool) {
    use flui_interaction::arena::GestureArena;
    use flui_interaction::{DragAxis, DragGestureRecognizer, GestureRecognizer};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::{cell::Cell, rc::Rc, sync::Arc};

    let first_drops = Rc::new(Cell::new(0));
    let second_drops = Rc::new(Cell::new(0));
    let first = first_drops.clone();
    let second = second_drops.clone();
    let down = DragRetirementProbe(Box::new(move || {
        first.set(first.get() + 1);
        panic!("first drag capture failure");
    }));
    let start = DragRetirementProbe(Box::new(move || {
        second.set(second.get() + 1);
        panic!("second drag capture failure");
    }));
    let recognizer = DragGestureRecognizer::new(GestureArena::new(), DragAxis::Horizontal)
        .with_on_down(move |_| {
            let _capture = &down;
        })
        .with_on_start(move |_| {
            let _capture = &start;
        });
    // A separate recognizer value shares the physical Rc callback owner.
    let alias = Arc::new((*recognizer).clone());
    drop(recognizer);
    assert_eq!((first_drops.get(), second_drops.get()), (0, 0));
    struct DisposeOnDrop(Arc<DragGestureRecognizer>);
    impl Drop for DisposeOnDrop {
        fn drop(&mut self) {
            self.0.dispose();
        }
    }
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        if active_unwind {
            if final_owner {
                let _owner = alias;
                panic!("incoming drag failure");
            }
            let _cleanup = DisposeOnDrop(alias);
            panic!("incoming drag failure");
        }
        if final_owner {
            drop(alias);
        } else {
            alias.dispose();
        }
    }));
    let payload = outcome.expect_err("first drag failure propagates");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&if active_unwind {
            "incoming drag failure"
        } else {
            "first drag capture failure"
        })
    );
    assert_eq!(first_drops.get(), usize::from(!active_unwind));
    assert_eq!(
        second_drops.get(),
        0,
        "independent tail capture is retained"
    );
    fresh_drag_completes_after_retirement();
}

fn drag_caller_keeps_previously_caught_failure() {
    use flui_interaction::arena::GestureArena;
    use flui_interaction::{DragAxis, DragGestureRecognizer, GestureRecognizer};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let caller_failure = catch_unwind(|| panic!("caller already caught failure"))
        .expect_err("caller owns prior payload");
    let probe = DragRetirementProbe(Box::new(|| panic!("new retirement failure")));
    let recognizer = DragGestureRecognizer::new(GestureArena::new(), DragAxis::Horizontal)
        .with_on_down(move |_| {
            let _capture = &probe;
        });
    let outcome = catch_unwind(AssertUnwindSafe(|| recognizer.dispose()));
    assert_eq!(
        outcome
            .expect_err("healthy call reports its own failure")
            .downcast_ref::<&str>(),
        Some(&"new retirement failure")
    );
    assert_eq!(
        caller_failure.downcast_ref::<&str>(),
        Some(&"caller already caught failure")
    );
    fresh_drag_completes_after_retirement();
}

#[expect(
    clippy::arc_with_non_send_sync,
    reason = "the owner-local recognizer API requires Arc aliases to exercise final shared callback ownership"
)]
fn drag_callbacks_live_until_the_final_shared_owner() {
    use flui_interaction::arena::GestureArena;
    use flui_interaction::{DragAxis, DragGestureRecognizer};
    use std::{cell::Cell, rc::Rc, sync::Arc};
    let dropped = Rc::new(Cell::new(0));
    let mut recognizer = DragGestureRecognizer::new(GestureArena::new(), DragAxis::Horizontal);
    for slot in 0..5 {
        let observed = dropped.clone();
        let probe = DragRetirementProbe(Box::new(move || observed.set(observed.get() + 1)));
        recognizer = match slot {
            0 => recognizer.with_on_down(move |_| {
                let _capture = &probe;
            }),
            1 => recognizer.with_on_start(move |_| {
                let _capture = &probe;
            }),
            2 => recognizer.with_on_update(move |_| {
                let _capture = &probe;
            }),
            3 => recognizer.with_on_end(move |_| {
                let _capture = &probe;
            }),
            _ => recognizer.with_on_cancel(move || {
                let _capture = &probe;
            }),
        };
    }
    let alias = Arc::new((*recognizer).clone());
    drop(recognizer);
    assert_eq!(dropped.get(), 0, "a recognizer alias still owns callbacks");
    drop(alias);
    assert_eq!(
        dropped.get(),
        5,
        "healthy final owner destroys every capture"
    );
    fresh_drag_completes_after_retirement();
}

fn drag_callback_can_dispose_its_own_recognizer() {
    drag_self_dispose_from_callback(false);
}
fn drag_callback_body_failure_retains_its_capture() {
    drag_self_dispose_from_callback(true);
}

fn drag_self_dispose_from_callback(body_failure: bool) {
    use flui_interaction::arena::GestureArena;
    use flui_interaction::{DragAxis, DragGestureRecognizer, GestureRecognizer, Offset, PointerId};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::{cell::Cell, rc::Rc, sync::Arc};

    let recognizer = DragGestureRecognizer::new(GestureArena::new(), DragAxis::Horizontal);
    let weak = Arc::downgrade(&recognizer);
    let drops = Rc::new(Cell::new(0));
    let observed = drops.clone();
    let probe = DragRetirementProbe(Box::new(move || {
        observed.set(observed.get() + 1);
        assert!(!body_failure, "drag body capture failure");
    }));
    let recognizer = recognizer.with_on_down(move |_| {
        let _capture = &probe;
        weak.upgrade().expect("live recognizer callback").dispose();
        assert!(!body_failure, "drag callback body failure");
    });
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        recognizer.add_pointer(PointerId::PRIMARY, Offset::ZERO, Offset::ZERO);
    }));
    if body_failure {
        assert_eq!(
            outcome
                .expect_err("body failure propagates")
                .downcast_ref::<&str>(),
            Some(&"drag callback body failure")
        );
        assert_eq!(drops.get(), 0, "failed body retains its opaque capture");
    } else {
        outcome.expect("callback self disposal is reentrant");
        assert_eq!(drops.get(), 1, "healthy callback snapshot retires normally");
    }
    assert!(recognizer.primary_pointer().is_none());
    recognizer.dispose();
    fresh_drag_completes_after_retirement();
}

#[expect(
    clippy::arc_with_non_send_sync,
    reason = "the owner-local arena accepts custom members through Arc identity"
)]
fn drag_disposal_commits_tracking_before_rejection_diagnostics() {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use flui_interaction::arena::GestureArena;
    use flui_interaction::sealed::CustomGestureRecognizer;
    use flui_interaction::{DragAxis, DragGestureRecognizer, GestureRecognizer, Offset, PointerId};

    struct RejectEventPanic(Arc<AtomicBool>);
    impl tracing::Subscriber for RejectEventPanic {
        fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
            metadata.name() == "recognizer.reject" && *metadata.level() == tracing::Level::DEBUG
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            assert_eq!(event.metadata().name(), "recognizer.reject");
            self.0.store(true, Ordering::Relaxed);
            panic!("recognizer rejection diagnostic failure");
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }
    struct Sibling(Rc<Cell<usize>>);
    impl CustomGestureRecognizer for Sibling {
        fn on_arena_accept(&self, _: PointerId) {
            self.0.set(self.0.get() + 1);
        }
        fn on_arena_reject(&self, _: PointerId) {
            panic!("disposing the drag must preserve its sibling");
        }
    }

    let arena = GestureArena::new();
    let recognizer = DragGestureRecognizer::new(arena.clone(), DragAxis::Horizontal);
    recognizer.add_pointer(PointerId::PRIMARY, Offset::ZERO, Offset::ZERO);
    let sibling_accepts = Rc::new(Cell::new(0));
    arena.add(
        PointerId::PRIMARY,
        Arc::new(Sibling(sibling_accepts.clone())),
    );
    arena.close(PointerId::PRIMARY);
    assert_eq!(arena.member_count(PointerId::PRIMARY), 2);

    let diagnostic_ran = Arc::new(AtomicBool::new(false));
    let result =
        tracing::subscriber::with_default(RejectEventPanic(diagnostic_ran.clone()), || {
            catch_unwind(AssertUnwindSafe(|| recognizer.dispose()))
        });
    let payload = result.expect_err("the actual debug event subscriber must fail");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"recognizer rejection diagnostic failure")
    );
    assert!(
        diagnostic_ran.load(Ordering::Relaxed),
        "the event actually ran"
    );
    assert!(
        recognizer.primary_pointer().is_none(),
        "tracking commits before diagnostic code"
    );
    assert_eq!(
        arena.member_count(PointerId::PRIMARY),
        1,
        "only the sibling remains admitted"
    );
    assert_eq!(arena.drain_deferred_resolutions(), 1);
    assert_eq!(
        sibling_accepts.get(),
        1,
        "the sibling still makes progress after diagnostic failure"
    );
    assert!(arena.is_empty());
    recognizer.dispose();
    fresh_drag_completes_after_retirement();
}

fn drag_completion_commits_tracking_before_stop_diagnostics() {
    use flui_interaction::arena::GestureArena;
    use flui_interaction::events::{PointerType, make_move_event, make_up_event};
    use flui_interaction::routing::PointerDispatch;
    use flui_interaction::{DragAxis, DragGestureRecognizer, GestureRecognizer, Offset, PointerId};
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    struct StopEventPanic(Arc<AtomicBool>);
    impl tracing::Subscriber for StopEventPanic {
        fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
            metadata.name() == "recognizer.stop_tracking"
                && *metadata.level() == tracing::Level::DEBUG
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            assert_eq!(event.metadata().name(), "recognizer.stop_tracking");
            self.0.store(true, Ordering::Relaxed);
            panic!("recognizer stop diagnostic failure");
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }
    let arena = GestureArena::new();
    let starts = Rc::new(Cell::new(0));
    let ends = Rc::new(Cell::new(0));
    let observed_start = starts.clone();
    let observed_end = ends.clone();
    let recognizer = DragGestureRecognizer::new(arena.clone(), DragAxis::Horizontal)
        .with_on_start(move |_| observed_start.set(observed_start.get() + 1))
        .with_on_end(move |_| observed_end.set(observed_end.get() + 1));
    recognizer.add_pointer(PointerId::PRIMARY, Offset::ZERO, Offset::ZERO);
    arena.close(PointerId::PRIMARY);
    arena.drain_deferred_resolutions();
    let movement = make_move_event(Offset::new(30.0, 0.0), PointerType::Touch);
    let release = make_up_event(Offset::new(30.0, 0.0), PointerType::Touch);
    recognizer.handle_event(PointerDispatch::at_root(&movement));
    assert_eq!(starts.get(), 1);

    let diagnostic_ran = Arc::new(AtomicBool::new(false));
    let result = tracing::subscriber::with_default(StopEventPanic(diagnostic_ran.clone()), || {
        catch_unwind(AssertUnwindSafe(|| {
            recognizer.handle_event(PointerDispatch::at_root(&release));
        }))
    });
    let payload = result.expect_err("the actual stop debug event must fail");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"recognizer stop diagnostic failure")
    );
    assert!(
        diagnostic_ran.load(Ordering::Relaxed),
        "the event actually ran"
    );
    assert!(
        recognizer.primary_pointer().is_none(),
        "terminal contact committed before diagnostics"
    );
    assert!(arena.is_empty());
    assert_eq!(
        ends.get(),
        0,
        "a failed lifecycle phase must not invoke the end callback"
    );

    // A second actual drag on the same recognizer proves the failed terminal
    // contact did not wedge its tracking or delete subsequent work.
    recognizer.add_pointer(PointerId::PRIMARY, Offset::ZERO, Offset::ZERO);
    arena.close(PointerId::PRIMARY);
    arena.drain_deferred_resolutions();
    recognizer.handle_event(PointerDispatch::at_root(&movement));
    recognizer.handle_event(PointerDispatch::at_root(&release));
    assert_eq!((starts.get(), ends.get()), (2, 1));
    assert!(recognizer.primary_pointer().is_none());
    recognizer.dispose();
    fresh_drag_completes_after_retirement();
}

fn stop_tracking_preserves_reentrant_contact_after_sweep() {
    assert_stop_tracking_preserves_reentrant_contact(false);
}

fn stop_tracking_preserves_reentrant_contact_after_sweep_failure() {
    assert_stop_tracking_preserves_reentrant_contact(true);
}

#[expect(
    clippy::arc_with_non_send_sync,
    reason = "the owner-local arena accepts the reentrant sweep member through Arc identity"
)]
fn assert_stop_tracking_preserves_reentrant_contact(fail_after_admission: bool) {
    use flui_interaction::arena::GestureArena;
    use flui_interaction::recognizers::RecognizerBase;
    use flui_interaction::sealed::CustomGestureRecognizer;
    use flui_interaction::{Offset, PointerId};
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;
    use std::sync::Arc;

    #[derive(Clone)]
    struct NextContact(Rc<Cell<usize>>);
    impl CustomGestureRecognizer for NextContact {
        fn on_arena_accept(&self, _: PointerId) {
            self.0.set(self.0.get() + 1);
        }
        fn on_arena_reject(&self, _: PointerId) {
            panic!("accepted next contact must not be rejected");
        }
    }
    #[derive(Clone)]
    struct ReentrantSweep {
        base: RecognizerBase,
        next_accepts: Rc<Cell<usize>>,
        fail_after_admission: bool,
    }
    impl CustomGestureRecognizer for ReentrantSweep {
        #[expect(
            clippy::arc_with_non_send_sync,
            reason = "reentrant admission uses the owner-local arena API's required Arc member identity"
        )]
        fn on_arena_accept(&self, pointer: PointerId) {
            assert!(
                self.base.primary_pointer().is_none(),
                "old contact withdrawn before sweep callback"
            );
            assert!(
                self.base.tracked_entry().is_none(),
                "old entry withdrawn before sweep callback"
            );
            let next = Arc::new(NextContact(self.next_accepts.clone()));
            // Reuse the platform pointer ID while the old exact-generation
            // sweep is delivering. The new arena slot belongs to this contact.
            self.base
                .start_tracking(pointer, Offset::new(7.0, 3.0), Offset::new(7.0, 3.0), &next);
            assert!(
                !self.fail_after_admission,
                "sweep failure after next contact admission"
            );
        }
        fn on_arena_reject(&self, _: PointerId) {
            panic!("swept front member must be accepted");
        }
    }
    let arena = GestureArena::new();
    let base = RecognizerBase::new(arena.clone());
    let next_accepts = Rc::new(Cell::new(0));
    let member = Arc::new(ReentrantSweep {
        base: base.clone(),
        next_accepts: next_accepts.clone(),
        fail_after_admission,
    });
    base.start_tracking(PointerId::PRIMARY, Offset::ZERO, Offset::ZERO, &member);
    let result = catch_unwind(AssertUnwindSafe(|| base.stop_tracking()));
    if fail_after_admission {
        assert_eq!(
            result
                .expect_err("sweep body failure propagates")
                .downcast_ref::<&str>(),
            Some(&"sweep failure after next contact admission")
        );
    } else {
        result.expect("reentrant sweep finishes");
    }
    assert_eq!(base.primary_pointer(), Some(PointerId::PRIMARY));
    assert_eq!(base.initial_position(), Some(Offset::new(7.0, 3.0)));
    assert!(base.tracked_entry().is_some());
    assert_eq!(arena.member_count(PointerId::PRIMARY), 1);
    assert!(arena.is_open(PointerId::PRIMARY));
    base.stop_tracking();
    assert!(base.primary_pointer().is_none());
    assert!(base.tracked_entry().is_none());
    assert_eq!(next_accepts.get(), 1, "accepted tail remains deliverable");
    assert!(arena.is_empty());
    fresh_drag_completes_after_retirement();
}
