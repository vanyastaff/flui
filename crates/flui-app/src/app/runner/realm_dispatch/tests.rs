//! Unit tests for `realm_dispatch.rs`, declared there as its child module `realm_dispatch_tests`
//! through `#[path]`, so `super::*` still reaches the dispatcher's private items.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use flui_foundation::geometry::Offset;
use flui_interaction::{
    HitTestResult,
    events::{PointerType, make_down_event},
};
use flui_platform::traits::{PlatformInput, PlatformWindow};

use super::super::host::{
    OwnerHostClearGuard, install_exit_policy_hook, install_owner_platform, with_owner_platform,
};
use super::super::secondary_window::open_secondary_window;
use super::*;
use crate::app::AppConfig;
use crate::app::runtime::{ExitPolicy, WindowPolicy};

static_assertions::assert_impl_all!(PlatformToUi: Send);

fn down_input(offset: f64) -> PlatformInput {
    PlatformInput::Pointer(make_down_event(
        Offset::new(offset, offset),
        PointerType::Mouse,
    ))
}

fn test_window() -> std::sync::Arc<dyn flui_platform::traits::PlatformWindow> {
    crate::app::window_test_support::headless_test_window()
}

fn install_test_realm() -> RealmDispatcher {
    install_platform_realm(crate::app::ui_realm::UiRealm::for_test(), &test_window())
}

fn background_owner_pump_drains_before_polling_without_a_frame() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let _clear = OwnerHostClearGuard::arm();
    let realm = crate::app::ui_realm::UiRealm::for_test();
    let scheduler = realm.scheduler().clone();
    let sender = realm.command_sender();
    let dispatcher = install_platform_realm(realm, &test_window());
    sender.request_redraw();
    dispatch_platform_realm(dispatcher, RealmTask::BackgroundPump).expect("background turn");
    dispatch_platform_realm(
        dispatcher,
        RealmTask::Frame(Box::new(|realm| {
            assert!(!realm.take_redraw_request(), "the owner inbox was drained");
        })),
    )
    .expect("inspect inbox");

    let polled = Arc::new(AtomicBool::new(false));
    let polled_in_task = Arc::clone(&polled);
    let _token = scheduler.spawn_local(Box::pin(async move {
        polled_in_task.store(true, Ordering::SeqCst);
        sender.request_redraw();
    }));
    let frames_before = scheduler.frame_count();
    dispatch_platform_realm(dispatcher, RealmTask::BackgroundPump).expect("poll background work");
    assert!(polled.load(Ordering::SeqCst));
    assert_eq!(
        scheduler.frame_count(),
        frames_before,
        "no frame transaction"
    );
    dispatch_platform_realm(
        dispatcher,
        RealmTask::Frame(Box::new(|realm| {
            assert!(
                realm.take_redraw_request(),
                "poll-generated work survives until the next turn"
            );
        })),
    )
    .expect("inspect next-turn work");
    teardown_platform_realm();
}

fn explicit_platform_quit_detaches_every_installed_realm() {
    let _clear = OwnerHostClearGuard::arm();
    let platform = flui_platform::HeadlessPlatform::new();
    let reevaluation = platform.exit_reevaluation();
    let platform: Box<dyn flui_platform::Platform> = Box::new(platform);
    platform
        .run(Box::new(move |owner| {
            let shared = owner.shared();
            install_owner_platform(owner).expect("install owner wake transport");
            let primary = install_test_realm();
            let secondary =
                install_realm_alongside(crate::app::ui_realm::UiRealm::for_test(), &test_window())
                    .expect("secondary realm");
            for dispatcher in [primary, secondary] {
                dispatch_platform_realm(
                    dispatcher,
                    RealmTask::Event(PlatformToUi::Lifecycle(AppLifecycleState::Resumed)),
                )
                .expect("resume realm");
            }
            super::super::host::install_platform_quit_hook();
            shared.set_exit_policy_hook(Box::new(|| true));
            shared.request_exit_policy_reevaluation();
            assert!(reevaluation.drive(), "registered on_quit callback ran");
            APP_RUNTIME.with(|slot| {
                for (_, installed) in slot.borrow().realms.iter() {
                    let realm = installed.realm.as_ref().expect("realm restored");
                    assert_eq!(
                        realm.scheduler().lifecycle_state(),
                        AppLifecycleState::Detached,
                        "every realm must detach through the installed quit callback"
                    );
                    assert!(!realm.scheduler().frames_enabled());
                }
            });
            teardown_platform_realm();
            Ok(())
        }))
        .expect("headless run");
}

/// Installing a realm resolves the loop-scoped execution services
/// (issue #557) at the same known point as `SharedEngineServices` — and
/// full loop-exit teardown shuts them down AND clears the slot, so a
/// SECOND platform loop hosted on this same thread (an embedder running
/// `run_app` twice in one process; a headless-restart harness) gets
/// fresh, working services that honor its own injected executors
/// instead of inheriting an instance whose admission is permanently
/// closed.
///
/// If reverted: remove the `ensure_execution` call from
/// `install_platform_realm` and the first assertion fails; remove the
/// `shutdown_execution` call from `teardown_platform_realm` (or make it
/// leave the slot filled) and the second loop's assertions fail.
fn install_resolves_execution_services_and_teardown_shuts_them_down() {
    use flui_runtime::execution::DeterministicExecutors;

    APP_RUNTIME.with(|slot| {
        assert!(
            slot.borrow().execution().is_none(),
            "no execution services before any realm is installed"
        );
    });

    // ── first loop: default pools ───────────────────────────────────
    install_test_realm();
    APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        let services = state
            .execution()
            .expect("install_platform_realm must resolve execution services");
        assert!(services.owns_default_pools());
        assert!(
            !services.default_pools_started(),
            "resolution must not start worker threads; pools start on first spawn"
        );
    });

    teardown_platform_realm();
    APP_RUNTIME.with(|slot| {
        assert!(
            slot.borrow().execution().is_none(),
            "full loop-exit teardown must clear the slot, not leave a dead instance"
        );
    });

    // ── second loop on the same thread: its own injected executors ──
    let deterministic = DeterministicExecutors::new();
    APP_RUNTIME.with(|slot| {
        slot.borrow_mut()
            .install_host_executors(deterministic.host_executors());
    });
    install_test_realm();
    let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        let services = state
            .execution()
            .expect("the second loop's install must re-resolve execution services");
        assert!(
            !services.owns_default_pools(),
            "the second loop's injected executors must be honored, not ignored as late"
        );
        let ran_for_job = std::sync::Arc::clone(&ran);
        services
            .spawn_compute(Box::new(move || {
                ran_for_job.store(true, std::sync::atomic::Ordering::Release);
            }))
            .expect("the second loop's admission must be open");
    });
    deterministic.run_until_idle();
    assert!(ran.load(std::sync::atomic::Ordering::Acquire));

    teardown_platform_realm();
}

fn late_event_never_crosses_realm_incarnations() {
    let stale = install_test_realm();
    let removed = APP_RUNTIME.with(|slot| slot.borrow_mut().realms.remove(&stale.address.realm_id));
    drop(removed);
    assert_eq!(
        dispatch_platform_realm(stale, RealmTask::Frame(Box::new(|_| {}))),
        Err(RealmDispatchError::RealmUnavailable)
    );

    let current = install_test_realm();
    assert_eq!(
        dispatch_platform_realm(stale, RealmTask::Frame(Box::new(|_| {}))),
        Err(RealmDispatchError::StaleRealm)
    );
    dispatch_platform_realm(current, RealmTask::Frame(Box::new(|_| {})))
        .expect("current incarnation dispatches");
}

fn panic_restores_dispatch_host_for_next_event() {
    let dispatcher = install_test_realm();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = dispatch_platform_realm(
            dispatcher,
            RealmTask::Frame(Box::new(|_| panic!("test panic"))),
        );
    }));
    assert!(panic.is_err());

    let ran = Rc::new(RefCell::new(false));
    let ran_in_event = Rc::clone(&ran);
    dispatch_platform_realm(
        dispatcher,
        RealmTask::Frame(Box::new(move |_| {
            *ran_in_event.borrow_mut() = true;
        })),
    )
    .expect("host restored");
    assert!(*ran.borrow());
}

// ========================================================================
// Issue #555 red exploits: multi-realm `AppRuntime` hosting,
// separate-realms policy, exit policy, hot-restart.
//
// Every test below was structurally impossible to even set up before this
// change: `AppRuntime` had exactly one `Option<UiRealm>` slot, so a
// second `install_platform_realm`/`install_realm_alongside` call always
// displaced the first rather than coexisting with it. There was no red
// run to capture (the code to call did not exist), so "red" here means
// "would not compile / had no second-realm install path to call" rather
// than "compiled and failed an assertion" -- stated rather than
// fabricated.
// ========================================================================

/// Installs two coexisting realms, A (through the legacy displacing
/// `install_platform_realm`) and B (through `install_realm_alongside`,
/// non-displacing). Both windows come from ONE shared headless platform
/// instance, not two separate `test_window()` calls: `HeadlessPlatform`
/// mints window ids from an instance-local counter, so two windows from
/// two SEPARATE `headless_platform()` calls can alias the same id —
/// fatal for two realms meant to coexist, since `WindowRegistry::
/// register_window` REPLACES on a matching id, silently dropping realm
/// A's window mapping the moment realm B's aliased-id window installs.
fn install_two_test_realms() -> (RealmDispatcher, RealmDispatcher) {
    let platform = flui_platform::headless_platform();
    let window_a: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create window a");
    let window_b: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create window b");
    let dispatcher_a = install_platform_realm(crate::app::ui_realm::UiRealm::for_test(), &window_a);
    let dispatcher_b =
        install_realm_alongside(crate::app::ui_realm::UiRealm::for_test(), &window_b)
            .expect("realm B installs alongside realm A cleanly");
    (dispatcher_a, dispatcher_b)
}

fn reentrant_owner_turns_preserve_global_fifo_across_realms() {
    let (dispatcher_a, dispatcher_b) = install_two_test_realms();

    let order = Rc::new(RefCell::new(Vec::new()));
    let order_in_outer_a = Rc::clone(&order);
    let order_in_b = Rc::clone(&order);
    let order_in_queued_a = Rc::clone(&order);

    dispatch_platform_realm(
        dispatcher_a,
        RealmTask::Frame(Box::new(move |_realm| {
            order_in_outer_a.borrow_mut().push("a:outer:start");
            dispatch_platform_realm(
                dispatcher_b,
                RealmTask::Frame(Box::new(move |_| {
                    order_in_b.borrow_mut().push("b");
                })),
            )
            .expect("realm B turn is admitted");
            dispatch_platform_realm(
                dispatcher_a,
                RealmTask::Frame(Box::new(move |_| {
                    order_in_queued_a.borrow_mut().push("a:queued");
                })),
            )
            .expect("second realm A turn is admitted");
            order_in_outer_a.borrow_mut().push("a:outer:end");
        })),
    )
    .expect("the outer turn drains every admitted turn");

    assert_eq!(
        order.borrow().as_slice(),
        ["a:outer:start", "a:outer:end", "b", "a:queued"]
    );

    teardown_platform_realm();
}

/// Installs realm A via the legacy single-window path (mirroring
/// `install_test_realm`), through a REAL `OwnerPlatform` (unlike
/// `install_test_realm`/`install_two_test_realms`, which never install
/// one) — both windows this helper and its caller open come from the
/// SAME headless platform instance, so their native window identities
/// cannot collide once both are registered in the one `WindowRegistry`.
fn install_realm_a_through_a_real_owner_platform() -> (RealmDispatcher, OwnerHostClearGuard) {
    use std::{cell::Cell, rc::Rc};

    let clear_guard = OwnerHostClearGuard::arm();
    let platform = flui_platform::headless_platform();
    let dispatcher_a_slot: Rc<Cell<Option<RealmDispatcher>>> = Rc::new(Cell::new(None));
    let dispatcher_a_slot_for_on_ready = Rc::clone(&dispatcher_a_slot);
    let ready = platform.run(Box::new(move |owner| {
        install_owner_platform(owner).expect("install owner wake transport");
        let window_a: Arc<dyn PlatformWindow> =
            with_owner_platform(|owner| owner.open_window(flui_platform::WindowOptions::default()))
                .expect("BUG: install_owner_platform just ran above")
                .and_then(flui_platform::WindowOpen::try_ready)
                .expect("headless open_window is always Ready");
        dispatcher_a_slot_for_on_ready.set(Some(install_platform_realm(
            crate::app::ui_realm::UiRealm::for_test(),
            &window_a,
        )));
        Ok(())
    }));
    ready.expect("installing realm A must not fail");
    (
        dispatcher_a_slot.get().expect("set inside on_ready above"),
        clear_guard,
    )
}

/// `WindowPolicy::SeparateRealms`, driven through the REAL embedder seam
/// (`open_secondary_window`) rather than a direct
/// `install_realm_alongside` call — proves the POLICY-driven fork itself
/// routes to the share-nothing path, not just the underlying primitive
/// (`two_window_realms_share_no_ui_state_through_app_runtime` already
/// covers that). Same oracle: a pointer dispatched only to realm A must
/// leave realm B's gesture arena completely untouched.
fn two_realms_via_separate_windows_policy_share_nothing() {
    let (dispatcher_a, _clear_guard) = install_realm_a_through_a_real_owner_platform();

    open_secondary_window(AppConfig::default(), WindowPolicy::SeparateRealms)
        .expect("WindowPolicy::SeparateRealms must install a second realm cleanly");

    let dispatcher_b = APP_RUNTIME
        .with(|slot| {
            let state = slot.borrow();
            let (_, slot_b) = state
                .realms
                .iter()
                .find(|(id, _)| *id != dispatcher_a.address.realm_id)?;
            Some(RealmDispatcher {
                owner_thread: state.owner_thread?,
                address: slot_b.address,
            })
        })
        .expect("open_secondary_window(SeparateRealms) must install a second, distinct realm");

    assert_ne!(
        dispatcher_a.address.realm_id, dispatcher_b.address.realm_id,
        "WindowPolicy::SeparateRealms must install a genuinely distinct realm, never \
         displace or merge into realm A"
    );

    dispatch_platform_realm(
        dispatcher_a,
        RealmTask::Frame(Box::new(|realm| {
            let down = down_input(4.0);
            if let PlatformInput::Pointer(event) = down {
                realm
                    .gestures()
                    .handle_pointer_event(&event, |_| HitTestResult::new());
            }
            assert_eq!(
                realm.gestures().active_pointer_count(),
                1,
                "realm A observes its own pointer"
            );
        })),
    )
    .expect("A dispatches");

    dispatch_platform_realm(
        dispatcher_b,
        RealmTask::Frame(Box::new(|realm| {
            assert_eq!(
                realm.gestures().active_pointer_count(),
                0,
                "the policy-driven secondary realm must share no gesture-arena state with \
                 realm A -- open_secondary_window(SeparateRealms) must route through \
                 install_realm_alongside, never a path that merges state with a sibling"
            );
        })),
    )
    .expect("B dispatches independently of A");

    teardown_platform_realm();
}

/// Every realm a runner builds shapes over the app's one font collection
/// (ADR-0092 §2): two `SeparateRealms` windows go through
/// `host::build_runtime_realm`, the one call every runner site (desktop, web,
/// Android, iOS, secondary windows) builds its realm with, and each realm's
/// `TextContext` must be built over `runtime_font_collection()`. Realm A comes
/// from `UiRealm::for_test`, which builds its own collection, so it is not
/// asserted on. Fails if that call hands a realm a fresh collection, or if
/// the runtime resolves a new one per call.
fn separate_realm_windows_shape_over_the_runtimes_font_collection() {
    let (dispatcher_a, _clear_guard) = install_realm_a_through_a_real_owner_platform();

    for _ in 0..2 {
        open_secondary_window(AppConfig::default(), WindowPolicy::SeparateRealms)
            .expect("WindowPolicy::SeparateRealms must install a second realm cleanly");
    }

    let secondaries: Vec<RealmDispatcher> = APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        let owner_thread = state.owner_thread.expect("the owner platform is installed");
        state
            .realms
            .iter()
            .filter(|(id, _)| *id != dispatcher_a.address.realm_id)
            .map(|(_, slot)| RealmDispatcher {
                owner_thread,
                address: slot.address,
            })
            .collect()
    });
    assert_eq!(
        secondaries.len(),
        2,
        "two SeparateRealms windows, two realms"
    );

    let app_fonts = super::super::host::runtime_font_collection();
    for dispatcher in secondaries {
        let app_fonts = app_fonts.clone();
        dispatch_platform_realm(
            dispatcher,
            RealmTask::Frame(Box::new(move |realm| {
                assert!(
                    realm.text_context_for_test().with(|text| {
                        flui_painting::FontCollection::ptr_eq(text.fonts(), &app_fonts)
                    }),
                    "a runner-built realm must own a text context over the app's font collection"
                );
            })),
        )
        .expect("the secondary realm dispatches");
    }

    teardown_platform_realm();
}

/// `WindowPolicy::SharedRealm`, driven through the REAL embedder seam
/// (`open_secondary_window`) — proves this really is forest-membership
/// routing (a second PRESENTATION of the SAME realm), not a second realm
/// in disguise: the hosted-realm count must stay at one, while the
/// realm's own presentation count grows from one to two.
fn one_realm_two_windows_policy_routes_by_presentation() {
    let (dispatcher_a, _clear_guard) = install_realm_a_through_a_real_owner_platform();

    let (realm_count_before, presentation_count_before) = APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        let realm_count = state.realms.iter().count();
        let presentation_count = state
            .realms
            .get(&dispatcher_a.address.realm_id)
            .and_then(|slot| slot.realm.as_ref())
            .expect("realm A is resident")
            .presentation_count();
        (realm_count, presentation_count)
    });
    assert_eq!(realm_count_before, 1);
    assert_eq!(presentation_count_before, 1);

    open_secondary_window(AppConfig::default(), WindowPolicy::SharedRealm).expect(
        "WindowPolicy::SharedRealm must install a second presentation into realm A cleanly",
    );

    let (realm_count_after, presentation_count_after) = APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        let realm_count = state.realms.iter().count();
        let presentation_count = state
            .realms
            .get(&dispatcher_a.address.realm_id)
            .and_then(|slot| slot.realm.as_ref())
            .expect("realm A is still resident -- SharedRealm must not have replaced it")
            .presentation_count();
        (realm_count, presentation_count)
    });
    assert_eq!(
        realm_count_after, realm_count_before,
        "WindowPolicy::SharedRealm must NOT install a second realm -- it routes into the \
         existing one via a second presentation"
    );
    assert_eq!(
        presentation_count_after, 2,
        "WindowPolicy::SharedRealm must install a genuine second presentation into realm \
         A's forest -- real forest-membership routing, not a second realm in disguise"
    );

    teardown_platform_realm();
}

/// [`install_realm_a_with_exit_policy_and_quit_counter`] plus the parked
/// re-evaluation handle.
///
/// The headless mock has no event loop, so a
/// `Platform::request_exit_policy_reevaluation` is PARKED and its
/// owner-thread half runs only when a test calls
/// `HeadlessExitReevaluation::drive`. A test asserting on an exit that a
/// re-evaluation produces therefore needs the handle; the nine tests that
/// do not keep the shorter tuple.
fn install_realm_a_with_exit_policy_quit_counter_and_reevaluation() -> (
    RealmDispatcher,
    std::sync::Arc<dyn flui_platform::traits::PlatformWindow>,
    Arc<std::sync::atomic::AtomicUsize>,
    OwnerHostClearGuard,
    flui_platform::HeadlessExitReevaluation,
) {
    use std::{cell::RefCell, rc::Rc, sync::atomic::AtomicUsize};

    type Installed = (
        RealmDispatcher,
        std::sync::Arc<dyn flui_platform::traits::PlatformWindow>,
    );

    let clear_guard = OwnerHostClearGuard::arm();
    let platform = flui_platform::HeadlessPlatform::new();
    let reevaluation = platform.exit_reevaluation();
    let platform: Box<dyn flui_platform::Platform> = Box::new(platform);
    let quit_calls = Arc::new(AtomicUsize::new(0));
    let quit_calls_for_on_ready = Arc::clone(&quit_calls);
    let installed_slot: Rc<RefCell<Option<Installed>>> = Rc::new(RefCell::new(None));
    let installed_slot_for_on_ready = Rc::clone(&installed_slot);
    let ready = platform.run(Box::new(move |owner| {
        install_owner_platform(owner).expect("install owner wake transport");
        install_exit_policy_hook(ExitPolicy::OnLastWindowClosed);

        let quit_calls_for_handler = Arc::clone(&quit_calls_for_on_ready);
        with_owner_platform(|owner| {
            owner.shared().on_quit(Box::new(move || {
                quit_calls_for_handler.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }));
        });

        let window_a: Arc<dyn PlatformWindow> =
            with_owner_platform(|owner| owner.open_window(flui_platform::WindowOptions::default()))
                .expect("owner installed above")
                .and_then(flui_platform::WindowOpen::try_ready)
                .expect("headless open_window is always Ready");
        let dispatcher_a =
            install_platform_realm(crate::app::ui_realm::UiRealm::for_test(), &window_a);
        // Mirror `run_desktop`'s own `on_close` wiring exactly, so a
        // real `window_a.close()` in a test drives the SAME production
        // path a real desktop bootstrap would.
        window_a.on_close(Box::new(move || {
            close_this_window(dispatcher_a);
        }));
        let _prev = installed_slot_for_on_ready
            .borrow_mut()
            .replace((dispatcher_a, window_a));
        Ok(())
    }));
    ready.expect("installing realm A must not fail");
    let (dispatcher_a, window_a) = installed_slot
        .borrow_mut()
        .take()
        .expect("set inside on_ready above");
    (
        dispatcher_a,
        window_a,
        quit_calls,
        clear_guard,
        reevaluation,
    )
}

/// A window closed REENTRANTLY, from inside a dispatched realm callback,
/// still exits the app.
///
/// This was a tracked gap and this test pinned it. `close_this_window`'s
/// `RealmTask::ClosePresentation` is a SAME-REALM reentrant dispatch, so
/// it only enqueues onto the already-checked-out realm's queue; the
/// uninstall applies at the OUTER dispatch's tail, strictly after
/// `window.close()`'s own `notify_closed` — and the exit-policy hook it
/// consulted — has already returned "don't exit". Nothing re-checked
/// afterwards, so the exit was missed rather than delayed.
///
/// The fix re-asks: `dispatch_platform_realm`'s tail requests a
/// re-evaluation whenever a deferred realm-map mutation actually removed
/// something, through the platform's own coalesced owner-thread seam
/// (the same one a keep-alive service's completion uses). A spurious
/// request is a no-op by that seam's contract, so it is armed on every
/// applied mutation rather than on a detected shape.
///
/// The headless mock has no event loop, so the request is PARKED and its
/// owner-thread half runs on `drive()`. This asserts both halves: that a
/// request was made at all, and that driving it produces the quit. The
/// first is what actually pins the fix — without the tail change nothing
/// is parked, and `drive()` returns `false` with nothing to run.
fn closing_the_last_window_reentrantly_from_inside_a_dispatch_still_exits() {
    use std::sync::atomic::Ordering;

    let (dispatcher, window_a, quit_calls, _clear_guard, reevaluation) =
        install_realm_a_with_exit_policy_quit_counter_and_reevaluation();

    dispatch_platform_realm(
        dispatcher,
        RealmTask::Frame(Box::new(move |_realm| {
            // Reentrant: this window's own `on_close` (wired by the
            // helper above, mirroring `run_desktop`) calls
            // `close_this_window(dispatcher)` -> `close_presentation` ->
            // a nested `dispatch_platform_realm` on the SAME realm this
            // Frame task's own dispatch already checked out.
            window_a.close();
        })),
    )
    .expect("the outer Frame dispatch itself must not be refused");

    assert_eq!(
        APP_RUNTIME.with(|slot| slot.borrow().realms.iter().count()),
        0,
        "the deferred uninstall must still apply by the end of the outer dispatch's own \
         tail, even though the exit check that ran mid-dispatch missed it"
    );
    assert_eq!(
        quit_calls.load(Ordering::SeqCst),
        0,
        "the hook consulted mid-dispatch still saw a non-empty registry and vetoed, which \
         is correct at that moment -- the exit cannot have happened yet"
    );
    assert!(
        reevaluation.requested(),
        "the tail must have asked the platform to consult the exit policy again once the \
         deferred uninstall actually applied -- this is the assertion the fix adds, and \
         without it nothing is parked to drive"
    );

    assert!(
        reevaluation.drive(),
        "driving the parked request must reach the hook: no windows remain"
    );
    assert_eq!(
        quit_calls.load(Ordering::SeqCst),
        1,
        "and the hook, asked against the now-empty registry, must exit"
    );

    teardown_platform_realm();
}

/// A panicking visitor (e.g. hot-restart's reassemble running arbitrary
/// user build code) must not permanently strand the checked-out realm
/// at `realm: None`, nor wedge `iterating_all_realms` at `true` forever
/// -- both of which would (after the fix above) silently defer every
/// future realm-map mutation request for the rest of the process.
fn panicking_visit_restores_the_checked_out_realm_and_clears_iterating_all_realms() {
    let dispatcher = install_test_realm();

    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        for_each_installed_realm(|_realm| {
            panic!("simulated visitor panic");
        });
    }));
    assert!(
        panic.is_err(),
        "the visitor's panic must propagate out of for_each_installed_realm, not be \
         swallowed"
    );

    assert!(
        APP_RUNTIME.with(|slot| !slot.borrow().iterating_all_realms),
        "a panicking visit must clear iterating_all_realms before its own panic resumes -- \
         otherwise every future realm-map mutation request would defer forever"
    );

    // A bare `.expect(Ok(()))` on the dispatch below is NOT sufficient
    // evidence the realm's slot was actually restored: if `realm: None`
    // were left stranded, `dispatch_platform_realm` would take its
    // enqueue-only early-return path (`realm_slot.realm.is_none()`) and
    // still return `Ok(())` — successfully queuing the task forever
    // without ever running it, indistinguishable from success at the
    // `Result` level alone. Give the task an observable side effect
    // instead: it only runs SYNCHRONOUSLY, inside this same call, if the
    // slot's `realm` was genuinely `Some(..)` at call time.
    let ran = Rc::new(Cell::new(false));
    let ran_in_task = Rc::clone(&ran);
    dispatch_platform_realm(
        dispatcher,
        RealmTask::Frame(Box::new(move |_| {
            ran_in_task.set(true);
        })),
    )
    .expect("dispatch must return cleanly after a panicking visit");
    assert!(
        ran.get(),
        "the dispatched task must actually RUN, not merely enqueue forever -- it only runs \
         if the visit's cleanup genuinely restored realm: Some(..) into this realm's slot; \
         a stranded realm: None slot would still return Ok(()) from the enqueue-only path \
         above without ever executing this closure"
    );

    teardown_platform_realm();
}

#[test]
fn realm_dispatch_matrix() {
    crate::table_test::run_table(
        "realm_dispatch_matrix",
        &[
            (
                "background_owner_pump_drains_before_polling_without_a_frame",
                background_owner_pump_drains_before_polling_without_a_frame as fn(),
            ),
            (
                "explicit_platform_quit_detaches_every_installed_realm",
                explicit_platform_quit_detaches_every_installed_realm as fn(),
            ),
            (
                "install_resolves_execution_services_and_teardown_shuts_them_down",
                install_resolves_execution_services_and_teardown_shuts_them_down as fn(),
            ),
            (
                "late_event_never_crosses_realm_incarnations",
                late_event_never_crosses_realm_incarnations as fn(),
            ),
            (
                "panic_restores_dispatch_host_for_next_event",
                panic_restores_dispatch_host_for_next_event as fn(),
            ),
            (
                "reentrant_owner_turns_preserve_global_fifo_across_realms",
                reentrant_owner_turns_preserve_global_fifo_across_realms as fn(),
            ),
            (
                "two_realms_via_separate_windows_policy_share_nothing",
                two_realms_via_separate_windows_policy_share_nothing as fn(),
            ),
            (
                "one_realm_two_windows_policy_routes_by_presentation",
                one_realm_two_windows_policy_routes_by_presentation as fn(),
            ),
            (
                "closing_the_last_window_reentrantly_from_inside_a_dispatch_still_exits",
                closing_the_last_window_reentrantly_from_inside_a_dispatch_still_exits as fn(),
            ),
            (
                "panicking_visit_restores_the_checked_out_realm_and_clears_iterating_all_realms",
                panicking_visit_restores_the_checked_out_realm_and_clears_iterating_all_realms
                    as fn(),
            ),
            (
                "separate_realm_windows_shape_over_the_runtimes_font_collection",
                separate_realm_windows_shape_over_the_runtimes_font_collection as fn(),
            ),
        ],
    );
}

// ========================================================================
// Close-request veto (issue #558)
// ========================================================================
