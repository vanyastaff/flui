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

/// The input wiring every runner installs answers the platform with the
/// realm's own decision: an Alt+F4 nothing handled keeps the platform
/// default (the native backend then closes the window), and one a shortcut
/// consumed prevents it. Pointer input no handler consumes is still reported
/// handled, since Android redraws only for handled input.
fn system_key_default_follows_the_realms_decision() {
    use flui_interaction::events::{Code, Modifiers};
    use flui_interaction::testing::input::KeyEventBuilder;

    let window = test_window();
    let dispatcher = install_platform_realm(crate::app::ui_realm::UiRealm::for_test(), &window);
    install_input_wiring(dispatcher, window.as_ref());
    let native = window
        .as_any()
        .downcast_ref::<flui_platform::MockWindow>()
        .expect("headless test window");
    let alt_f4 = || {
        PlatformInput::Keyboard(
            KeyEventBuilder::new(Code::F4)
                .with_modifiers(Modifiers::ALT)
                .build(),
        )
    };

    assert!(
        !native.inject_event(alt_f4()).default_prevented,
        "an unconsumed system key keeps the platform default"
    );
    assert!(
        native.inject_event(down_input(4.0)).default_prevented,
        "pointer input stays handled whether or not anything consumed it"
    );

    // A key arriving while an owner turn is in flight queues behind it, so
    // its outcome is unknown when the platform asks: the default is
    // prevented then, and the key is still delivered once the turn ends.
    let delivered = Rc::new(std::cell::Cell::new(0_usize));
    let in_turn = Rc::new(std::cell::Cell::new(None));
    let (delivered_in_handler, delivered_in_turn, in_turn_result) = (
        Rc::clone(&delivered),
        Rc::clone(&delivered),
        Rc::clone(&in_turn),
    );
    let turn_window = std::sync::Arc::clone(&window);
    dispatch_platform_realm(
        dispatcher,
        RealmTask::Frame(Box::new(move |realm| {
            realm.focus_manager().add_global_key_handler(Rc::new(
                move |_: &flui_interaction::events::KeyboardEvent| {
                    delivered_in_handler.set(delivered_in_handler.get() + 1);
                    false
                },
            ));
            let native = turn_window
                .as_any()
                .downcast_ref::<flui_platform::MockWindow>()
                .expect("headless test window");
            in_turn_result.set(Some((
                native.inject_event(alt_f4()).default_prevented,
                delivered_in_turn.get(),
            )));
        })),
    )
    .expect("the owner turn runs, then the queued key");
    assert_eq!(
        in_turn.get(),
        Some((true, 0)),
        "a key queued behind an owner turn prevents the default before delivery"
    );
    assert_eq!(
        delivered.get(),
        1,
        "the queued key is delivered after the turn"
    );

    dispatch_platform_realm(
        dispatcher,
        RealmTask::Frame(Box::new(|realm| {
            realm.focus_manager().add_global_key_handler(Rc::new(
                |event: &flui_interaction::events::KeyboardEvent| event.code == Code::F4,
            ));
        })),
    )
    .expect("install the shortcut");
    assert!(
        native.inject_event(alt_f4()).default_prevented,
        "a consumed system key prevents the platform default"
    );
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
/// routes to the share-nothing path, not just the underlying primitive.
/// The oracle: a pointer dispatched only to realm A must leave realm B's
/// gesture arena completely untouched.
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
/// asserted on. Fails if that call hands a realm a fresh collection (a realm
/// fed from the host again would be one), or if the runtime resolves a new one
/// per call. `the_runtime_launches_one_host_feed_for_every_realm` pins that
/// the runtime's collection is the one its host feed feeds.
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

/// Installs realm A under `ExitPolicy::OnLastWindowClosed` with a quit
/// counter, and returns the parked re-evaluation handle with them.
///
/// The headless mock has no event loop, so a
/// `Platform::request_exit_policy_reevaluation` is PARKED and its
/// owner-thread half runs only when a test calls
/// `HeadlessExitReevaluation::drive`. A test asserting on an exit that a
/// re-evaluation produces therefore needs the handle.
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

// Each registration row adds a face of its own: the rows share this thread's
// runtime, which refuses bytes it registered before, and the process font
// system, which keeps every face.
const PROBE_VARIABLE: &[u8] =
    include_bytes!("../../../../../flui-painting/assets/fonts/probe-variable-wght.ttf");
const PROBE_MONO_SEMIBOLD: &[u8] =
    include_bytes!("../../../../../flui-painting/assets/fonts/probe-mono-600.ttf");
const PROBE_SANS: &[u8] =
    include_bytes!("../../../../../flui-painting/assets/fonts/probe-sans-400.ttf");
const PROBE_MONO_THIN: &[u8] =
    include_bytes!("../../../../../flui-painting/assets/fonts/probe-mono-100.ttf");
const DECOY: &[u8] =
    include_bytes!("../../../../../flui-painting/assets/fonts/decoy-wide-space.ttf");

/// Clears `dispatcher`'s realm's redraw request.
fn clear_redraw(dispatcher: RealmDispatcher) {
    dispatch_platform_realm(
        dispatcher,
        RealmTask::Frame(Box::new(crate::app::ui_realm::UiRealm::mark_rendered)),
    )
    .expect("the realm dispatches");
}

/// Whether `dispatcher`'s realm has a redraw requested.
fn redraw_requested(dispatcher: RealmDispatcher) -> bool {
    let requested = Rc::new(Cell::new(false));
    let requested_in_task = Rc::clone(&requested);
    dispatch_platform_realm(
        dispatcher,
        RealmTask::Frame(Box::new(move |realm| {
            requested_in_task.set(realm.needs_redraw());
        })),
    )
    .expect("the realm dispatches");
    requested.get()
}

/// A face registered while two realm windows run tells both: each draws its
/// next frame, where its pipeline lays out again the text measured before.
/// Fails if the registration notifies no realm, or only one.
fn a_registration_notifies_every_realm_window() {
    let (dispatcher_a, dispatcher_b) = install_two_test_realms();
    for dispatcher in [dispatcher_a, dispatcher_b] {
        clear_redraw(dispatcher);
    }
    let generation = super::super::host::runtime_font_collection().generation();

    super::super::register_font(PROBE_VARIABLE).expect("the probe face registers");

    assert_eq!(
        super::super::host::runtime_font_collection().generation(),
        generation + 1,
        "the face went into the app's collection"
    );
    for dispatcher in [dispatcher_a, dispatcher_b] {
        assert!(
            redraw_requested(dispatcher),
            "{dispatcher:?} draws its next frame"
        );
    }

    teardown_platform_realm();
}

/// The host font feed landing off the owner thread tells every realm window
/// at the next owner turn, which the feed's wake brings: the wake, called on
/// the feed's thread, asks the loop for a turn, and at that turn each window
/// draws its next frame, where its pipeline lays out again the text measured
/// before. Fails if the feed is launched with a wake that does not reach the
/// loop, or if an owner turn does not announce a generation that moved
/// without a registration on this thread.
fn a_landed_host_feed_wakes_every_realm_window() {
    use flui_painting::testing::{PROBE_MONO_100, feed_with_host, host_fonts_from};

    let (dispatcher_a, dispatcher_b) = install_two_test_realms();
    for dispatcher in [dispatcher_a, dispatcher_b] {
        clear_redraw(dispatcher);
    }
    let generation = super::super::host::runtime_font_collection().generation();
    let crate::app::runtime::ParkedHostFeed { feed, wake } =
        crate::app::runtime::take_parked_host_feeds()
            .pop()
            .expect("the runtime launched its host feed when its services resolved");
    let loop_redraw = super::super::host::runtime_needs_redraw_handle();
    loop_redraw.store(false, std::sync::atomic::Ordering::Relaxed);

    std::thread::spawn(move || {
        feed_with_host(feed, host_fonts_from(&[PROBE_MONO_100])).run();
        wake();
    })
    .join()
    .expect("the feed lands");
    assert!(
        loop_redraw.load(std::sync::atomic::Ordering::Relaxed),
        "the feed's wake asks the loop for the turn that tells the realms"
    );
    assert_eq!(
        super::super::host::runtime_font_collection().generation(),
        generation + 1,
        "the feed landed on the app's collection"
    );
    // The owner turn the wake brings; the notices queue behind its event.
    dispatch_platform_realm(dispatcher_a, RealmTask::Frame(Box::new(|_| {})))
        .expect("the realm dispatches");

    for dispatcher in [dispatcher_a, dispatcher_b] {
        assert!(
            redraw_requested(dispatcher),
            "{dispatcher:?} draws its next frame"
        );
    }

    teardown_platform_realm();
}

/// The same bytes registered twice are refused the second time: the
/// collection does not change and no realm is woken. Fails if a repeated
/// registration adds the face again or lays text out again for nothing.
fn a_duplicate_registration_is_refused_and_notifies_nothing() {
    let dispatcher = install_test_realm();
    super::super::register_font(PROBE_MONO_SEMIBOLD).expect("the first registration adds it");
    clear_redraw(dispatcher);
    let generation = super::super::host::runtime_font_collection().generation();

    assert_eq!(
        super::super::register_font(PROBE_MONO_SEMIBOLD),
        Err(super::super::FontRegistrationError::AlreadyRegistered)
    );

    assert_eq!(
        super::super::host::runtime_font_collection().generation(),
        generation
    );
    assert!(!redraw_requested(dispatcher), "no realm is woken");

    teardown_platform_realm();
}

/// Bytes with no face are refused: the collection does not change and no
/// realm is woken. Fails if a refused registration notifies the realms.
fn bytes_with_no_face_are_refused_and_notify_nothing() {
    let dispatcher = install_test_realm();
    clear_redraw(dispatcher);
    let generation = super::super::host::runtime_font_collection().generation();

    assert!(matches!(
        super::super::register_font(b"not a font"),
        Err(super::super::FontRegistrationError::Font(_))
    ));

    assert_eq!(
        super::super::host::runtime_font_collection().generation(),
        generation
    );
    assert!(!redraw_requested(dispatcher), "no realm is woken");

    teardown_platform_realm();
}

/// A registration made from inside one realm's task reaches that realm once
/// the task returns, and its sibling as well: the calling realm is checked
/// out while its task runs, so its notice waits in the owner queue. Fails if
/// the notice to the running realm is dropped, or re-enters it.
fn a_registration_from_inside_a_realm_task_reaches_that_realm_after_it_returns() {
    let (dispatcher_a, dispatcher_b) = install_two_test_realms();
    for dispatcher in [dispatcher_a, dispatcher_b] {
        clear_redraw(dispatcher);
    }

    dispatch_platform_realm(
        dispatcher_a,
        RealmTask::Frame(Box::new(|realm| {
            super::super::register_font(PROBE_SANS).expect("the probe face registers");
            assert!(
                !realm.needs_redraw(),
                "the notice waits for the running task to return"
            );
        })),
    )
    .expect("realm A's task runs, then the queued notices");

    for dispatcher in [dispatcher_a, dispatcher_b] {
        assert!(
            redraw_requested(dispatcher),
            "{dispatcher:?} draws its next frame"
        );
    }

    teardown_platform_realm();
}

/// A face registered on a thread that runs no app does not reach the app's
/// collection, and wakes no realm. Fails if the call registers on a
/// collection a window of the app reads.
fn a_registration_on_a_thread_that_runs_no_app_leaves_the_app_alone() {
    let dispatcher = install_test_realm();
    clear_redraw(dispatcher);
    let fonts_before = super::super::host::runtime_font_collection().generation();

    std::thread::spawn(|| super::super::register_font(PROBE_MONO_THIN))
        .join()
        .expect("the worker does not panic")
        .expect("the bytes hold a face");

    assert_eq!(
        super::super::host::runtime_font_collection().generation(),
        fonts_before,
        "the app's collection gained no face"
    );
    assert!(!redraw_requested(dispatcher), "no realm is woken");

    teardown_platform_realm();
}

/// A face registered before a thread builds its first realm is held, and
/// lands when the collection is built: the first window measures, paints
/// and places carets with it. Fails if a registration before the start is
/// lost, or accepts bytes with no face because nothing judges them yet.
fn a_registration_before_the_first_realm_lands_with_the_collection() {
    std::thread::spawn(|| {
        assert!(
            matches!(
                super::super::register_font(b"not a font"),
                Err(super::super::FontRegistrationError::Font(_))
            ),
            "bytes with no face are refused before the collection exists"
        );
        super::super::register_font(DECOY).expect("the bytes hold a face");
        assert_eq!(
            super::super::register_font(DECOY),
            Err(super::super::FontRegistrationError::AlreadyRegistered),
            "a held face counts as registered"
        );

        // What the runner does as it builds the first realm.
        let fonts = super::super::host::runtime_font_collection();
        assert_eq!(fonts.generation(), 1, "the collection gained the face");
    })
    .join()
    .expect("the registration lands with the collection");
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
                "system_key_default_follows_the_realms_decision",
                system_key_default_follows_the_realms_decision as fn(),
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
            (
                "a_registration_notifies_every_realm_window",
                a_registration_notifies_every_realm_window as fn(),
            ),
            (
                "a_landed_host_feed_wakes_every_realm_window",
                a_landed_host_feed_wakes_every_realm_window as fn(),
            ),
            (
                "a_duplicate_registration_is_refused_and_notifies_nothing",
                a_duplicate_registration_is_refused_and_notifies_nothing as fn(),
            ),
            (
                "bytes_with_no_face_are_refused_and_notify_nothing",
                bytes_with_no_face_are_refused_and_notify_nothing as fn(),
            ),
            (
                "a_registration_from_inside_a_realm_task_reaches_that_realm_after_it_returns",
                a_registration_from_inside_a_realm_task_reaches_that_realm_after_it_returns
                    as fn(),
            ),
            (
                "a_registration_on_a_thread_that_runs_no_app_leaves_the_app_alone",
                a_registration_on_a_thread_that_runs_no_app_leaves_the_app_alone as fn(),
            ),
            (
                "a_registration_before_the_first_realm_lands_with_the_collection",
                a_registration_before_the_first_realm_lands_with_the_collection as fn(),
            ),
        ],
    );
}

// ========================================================================
// Close-request veto (issue #558)
// ========================================================================

// ========================================================================
// Storage
// ========================================================================

/// A storage directory in the run's configuration reaches every realm the
/// host builds afterwards: the host resolves it once at start
/// (`AppRuntime::install_host_storage`, as the runners call it with the main
/// configuration), and a realm built later holds it in its build owner, which
/// every `LifecycleContext::storage` under it reads. The later realm comes
/// from `open_secondary_window`'s `SeparateRealms` path, which reaches
/// `host::build_runtime_realm` the way every runner site does, without a GPU.
#[cfg(feature = "persist")]
#[test]
#[ignore = "contract: the host gives a configured storage directory to every realm it builds"]
fn a_configured_storage_dir_reaches_lifecycle_context() {
    let (dispatcher_a, _clear_guard) = install_realm_a_through_a_real_owner_platform();
    let main_config = AppConfig::new().with_storage_dir(
        flui_platform_api::StorageName::from_static("storage-host-test"),
    );
    APP_RUNTIME.with(|slot| slot.borrow_mut().install_host_storage(&main_config));
    open_secondary_window(AppConfig::default(), WindowPolicy::SeparateRealms)
        .expect("WindowPolicy::SeparateRealms installs a second realm");

    let secondary = APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        let owner_thread = state.owner_thread.expect("the owner platform is installed");
        state
            .realms
            .iter()
            .find(|(id, _)| *id != dispatcher_a.address.realm_id)
            .map(|(_, slot)| RealmDispatcher {
                owner_thread,
                address: slot.address,
            })
            .expect("the secondary realm is hosted")
    });
    let reached = Rc::new(Cell::new(None));
    let seen = Rc::clone(&reached);
    dispatch_platform_realm(
        secondary,
        RealmTask::Frame(Box::new(move |realm| {
            seen.set(Some(
                realm
                    .widgets()
                    .with_build_owner(|owner| owner.storage().is_some()),
            ));
        })),
    )
    .expect("the secondary realm dispatches");
    teardown_platform_realm();

    assert_eq!(
        reached.get(),
        Some(true),
        "a realm built after the host started with a storage directory holds storage"
    );
}
