//! Unit tests for `owner_dispatch.rs`, declared there as its child module `owner_dispatch_tests`
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

static_assertions::assert_impl_all!(RuntimeEvent: Send);

fn observe_runtime<T: 'static>(
    dispatcher: PresentationDispatcher,
    observe: impl FnOnce(&crate::app::ui_runtime::UiRuntime) -> T + 'static,
) -> T {
    let result = Rc::new(RefCell::new(None));
    let delivered = Rc::clone(&result);
    dispatch_platform_ui_runtime(
        dispatcher,
        RuntimeTask::TestCallback(Box::new(move |runtime| {
            *delivered.borrow_mut() = Some(observe(runtime));
        })),
    )
    .expect("observe an idle installed runtime");
    result
        .borrow_mut()
        .take()
        .expect("observation completed synchronously")
}

struct ReentrantIdentityWindow {
    inner: crate::app::window_test_support::TestWindow,
    probe: flui_foundation::PresentationAddress,
    queried: Arc<std::sync::atomic::AtomicBool>,
    on_scale: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl PlatformWindow for ReentrantIdentityWindow {
    fn id(&self) -> flui_platform::traits::WindowId {
        self.queried
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(matches!(
            crate::request_presentation_close(self.probe),
            Err(crate::CloseRequestError::UnknownPresentation { .. }
                | crate::CloseRequestError::NoHostedRuntime)
        ));
        self.inner.id()
    }

    fn physical_size(&self) -> flui_foundation::geometry::Size<i32> {
        self.inner.physical_size()
    }
    fn logical_size(&self) -> flui_foundation::geometry::Size<f64> {
        self.inner.logical_size()
    }
    fn scale_factor(&self) -> f64 {
        if let Some(on_scale) = &self.on_scale {
            on_scale();
        }
        self.inner.scale_factor()
    }
    fn request_redraw(&self) {
        self.inner.request_redraw();
    }
    fn is_focused(&self) -> bool {
        self.inner.is_focused()
    }
    fn is_visible(&self) -> bool {
        self.inner.is_visible()
    }
    fn set_cursor(
        &self,
        cursor: flui_platform_api::CursorIcon,
    ) -> Result<(), flui_platform_api::CursorError> {
        self.inner.set_cursor(cursor)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn native_identity_is_observed_before_registry_publication() {
    use std::sync::atomic::{AtomicBool, Ordering};
    enum Install {
        ReplaceRuntime,
        IndependentRuntime,
        SharedPresentation,
    }
    for mode in [
        Install::ReplaceRuntime,
        Install::IndependentRuntime,
        Install::SharedPresentation,
    ] {
        let _clear = OwnerHostClearGuard::arm();
        let primary = install_test_ui_runtime();
        let uninstalled = crate::app::ui_runtime::UiRuntime::for_test();
        let probe = flui_foundation::PresentationAddress {
            ui_runtime_id: uninstalled.id(),
            presentation_id: uninstalled.presentation_id(),
        };
        drop(uninstalled);
        let queried = Arc::new(AtomicBool::new(false));
        let window: Arc<dyn PlatformWindow> = Arc::new(ReentrantIdentityWindow {
            inner: crate::app::window_test_support::TestWindow::new().with_id(100),
            probe,
            queried: Arc::clone(&queried),
            on_scale: None,
        });
        let installed = match mode {
            Install::ReplaceRuntime => {
                install_platform_ui_runtime(crate::app::ui_runtime::UiRuntime::for_test(), &window)
            }
            Install::IndependentRuntime => {
                install_ui_runtime_alongside(crate::app::ui_runtime::UiRuntime::for_test(), &window)
                    .expect("independent runtime installs")
            }
            Install::SharedPresentation => install_presentation_alongside(primary, window)
                .expect("shared presentation installs"),
        };
        assert!(
            queried.load(Ordering::SeqCst),
            "identity callback was exercised"
        );
        dispatch_platform_ui_runtime(
            installed,
            RuntimeTask::Event(RuntimeEvent::WindowFocus(true)),
        )
        .expect("published target accepts its window observation");
        teardown_platform_ui_runtime();
    }
}

fn presentation_assembly_reentry_revalidates_its_authorizer() {
    use std::sync::atomic::{AtomicBool, Ordering};

    for (close_authorizer, keep_sibling) in [(false, false), (true, false), (true, true)] {
        let _clear = OwnerHostClearGuard::arm();
        let primary = install_test_ui_runtime();
        let sibling = keep_sibling.then(|| {
            let window: Arc<dyn PlatformWindow> =
                Arc::new(crate::app::window_test_support::TestWindow::new().with_id(99));
            install_presentation_alongside(primary, window).expect("initial sibling installs")
        });
        let uninstalled = crate::app::ui_runtime::UiRuntime::for_test();
        let probe = flui_foundation::PresentationAddress {
            ui_runtime_id: uninstalled.id(),
            presentation_id: uninstalled.presentation_id(),
        };
        drop(uninstalled);
        let assembled = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&assembled);
        let window: Arc<dyn PlatformWindow> = Arc::new(ReentrantIdentityWindow {
            inner: crate::app::window_test_support::TestWindow::new().with_id(100),
            probe,
            queried: Arc::new(AtomicBool::new(false)),
            on_scale: Some(Arc::new(move || {
                if observed.swap(true, Ordering::SeqCst) {
                    return;
                }
                assert!(matches!(
                    crate::request_presentation_close(probe),
                    Err(crate::CloseRequestError::UnknownPresentation { .. }
                        | crate::CloseRequestError::NoHostedRuntime)
                ));
                if close_authorizer {
                    close_this_window(primary);
                }
            })),
        });
        let result = install_presentation_alongside(primary, Arc::clone(&window));
        assert!(
            assembled.load(Ordering::SeqCst),
            "assembly called the window"
        );
        if close_authorizer {
            assert_eq!(
                result.expect_err("assembly cannot publish through a closed authorizer"),
                if keep_sibling {
                    InstallPresentationError::StalePresentation
                } else {
                    InstallPresentationError::RuntimeUnavailable
                }
            );
            let replacement = install_ui_runtime_alongside(
                crate::app::ui_runtime::UiRuntime::for_test(),
                &window,
            )
            .expect("refused assembly left no native registration");
            dispatch_platform_ui_runtime(
                replacement,
                RuntimeTask::Event(RuntimeEvent::WindowFocus(true)),
            )
            .expect("next installation is usable");
        } else {
            let installed = result.expect("reentrant observation permits a live authorizer");
            dispatch_platform_ui_runtime(
                installed,
                RuntimeTask::Event(RuntimeEvent::WindowFocus(true)),
            )
            .expect("assembled presentation is dispatchable");
        }
        if let Some(sibling) = sibling {
            dispatch_platform_ui_runtime(
                sibling,
                RuntimeTask::Event(RuntimeEvent::WindowFocus(true)),
            )
            .expect("existing sibling survives the refused installation");
        }
        teardown_platform_ui_runtime();
    }
}

fn down_input(offset: f64) -> PlatformInput {
    PlatformInput::Pointer(make_down_event(
        Offset::new(offset, offset),
        PointerType::Mouse,
    ))
}

fn test_window() -> std::sync::Arc<dyn flui_platform::traits::PlatformWindow> {
    crate::app::window_test_support::headless_test_window()
}

fn install_test_ui_runtime() -> PresentationDispatcher {
    install_platform_ui_runtime(
        crate::app::ui_runtime::UiRuntime::for_test(),
        &test_window(),
    )
}

fn background_owner_pump_drains_before_polling_without_a_frame() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let _clear = OwnerHostClearGuard::arm();
    let ui_runtime = crate::app::ui_runtime::UiRuntime::for_test();
    let scheduler = ui_runtime.scheduler().clone();
    let driver = ui_runtime.owner_frame().async_driver();
    let sender = ui_runtime.command_sender();
    let dispatcher = install_platform_ui_runtime(ui_runtime, &test_window());
    let authority = dispatcher.runtime();
    let sibling = install_presentation_alongside(dispatcher, test_window()).expect("sibling");
    sender.request_redraw();
    authority.background().expect("background turn");
    dispatch_platform_ui_runtime(
        dispatcher,
        RuntimeTask::TestCallback(Box::new(|ui_runtime| {
            assert!(
                !ui_runtime.take_redraw_request(),
                "the owner inbox was drained"
            );
        })),
    )
    .expect("inspect inbox");

    close_this_window(dispatcher);
    let dispatcher = sibling;

    let polled = Arc::new(AtomicBool::new(false));
    let polled_in_task = Arc::clone(&polled);
    let _token = driver.spawn_local(Box::pin(async move {
        polled_in_task.store(true, Ordering::SeqCst);
        sender.request_redraw();
    }));
    let frames_before = scheduler.frame_count();
    authority
        .background()
        .expect("poll after original window closed");
    assert!(polled.load(Ordering::SeqCst));
    assert_eq!(
        scheduler.frame_count(),
        frames_before,
        "no frame transaction"
    );
    dispatch_platform_ui_runtime(
        dispatcher,
        RuntimeTask::TestCallback(Box::new(|ui_runtime| {
            assert!(
                ui_runtime.take_redraw_request(),
                "poll-generated work survives until the next turn"
            );
        })),
    )
    .expect("inspect next-turn work");
    teardown_platform_ui_runtime();
    let replacement = install_test_ui_runtime();
    assert_eq!(authority.background(), Err(DispatchError::StaleRuntime));
    assert_eq!(
        authority.lifecycle(AppLifecycleState::Resumed),
        Err(DispatchError::StaleRuntime)
    );
    replacement
        .runtime()
        .background()
        .expect("replacement progresses");
    teardown_platform_ui_runtime();
}

fn explicit_platform_quit_detaches_every_installed_ui_runtime() {
    let _clear = OwnerHostClearGuard::arm();
    let platform = flui_platform::HeadlessPlatform::new();
    let reevaluation = platform.exit_reevaluation();
    let platform: Box<dyn flui_platform::Platform> = Box::new(platform);
    platform
        .run(Box::new(move |owner| {
            let shared = owner.shared();
            install_owner_platform(owner).expect("install owner wake transport");
            let primary = install_test_ui_runtime();
            let secondary = install_ui_runtime_alongside(
                crate::app::ui_runtime::UiRuntime::for_test(),
                &test_window(),
            )
            .expect("secondary ui_runtime");
            for dispatcher in [primary, secondary] {
                dispatcher
                    .runtime()
                    .lifecycle(AppLifecycleState::Resumed)
                    .expect("resume ui_runtime");
            }
            super::super::host::install_platform_quit_hook();
            shared.set_exit_policy_hook(Box::new(|| true));
            shared.request_exit_policy_reevaluation();
            assert!(reevaluation.drive(), "registered on_quit callback ran");
            for dispatcher in [primary, secondary] {
                observe_runtime(dispatcher, |ui_runtime| {
                    assert_eq!(
                        ui_runtime.scheduler().lifecycle_state(),
                        AppLifecycleState::Detached,
                        "every ui_runtime must detach through the installed quit callback"
                    );
                    assert!(!ui_runtime.scheduler().frames_enabled());
                });
            }
            teardown_platform_ui_runtime();
            Ok(())
        }))
        .expect("headless run");
}

/// Installing a UI runtime resolves the loop-scoped execution services
/// (issue #557) at the same known point as `SharedEngineServices` — and
/// full loop-exit teardown shuts them down AND clears the slot, so a
/// SECOND platform loop hosted on this same thread (an embedder running
/// `run_app` twice in one process; a headless-restart harness) gets
/// fresh, working services that honor its own injected executors
/// instead of inheriting an instance whose admission is permanently
/// closed.
///
/// If reverted: remove the `ensure_execution` call from
/// `install_platform_ui_runtime` and the first assertion fails; remove the
/// `shutdown_execution` call from `teardown_platform_ui_runtime` (or make it
/// leave the slot filled) and the second loop's assertions fail.
fn install_resolves_execution_services_and_teardown_shuts_them_down() {
    use flui_runtime::execution::DeterministicExecutors;

    APP_RUNTIME.with(|slot| {
        assert!(
            slot.borrow().execution().is_none(),
            "no execution services before any ui_runtime is installed"
        );
    });

    // ── first loop: default pools ───────────────────────────────────
    install_test_ui_runtime();
    APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        let services = state
            .execution()
            .expect("install_platform_ui_runtime must resolve execution services");
        assert!(services.owns_default_pools());
        assert!(
            !services.default_pools_started(),
            "resolution must not start worker threads; pools start on first spawn"
        );
    });

    teardown_platform_ui_runtime();
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
    install_test_ui_runtime();
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

    teardown_platform_ui_runtime();
}

/// The input wiring every runner installs answers the platform with the
/// UI runtime's own decision: an Alt+F4 nothing handled keeps the platform
/// default (the native backend then closes the window), and one a shortcut
/// consumed prevents it. Pointer input no handler consumes is still reported
/// handled, since Android redraws only for handled input.
fn system_key_default_follows_the_ui_runtimes_decision() {
    use flui_interaction::events::{Code, Modifiers};
    use flui_interaction::testing::input::KeyEventBuilder;

    let window = test_window();
    let dispatcher =
        install_platform_ui_runtime(crate::app::ui_runtime::UiRuntime::for_test(), &window);
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
    dispatch_platform_ui_runtime(
        dispatcher,
        RuntimeTask::TestCallback(Box::new(move |ui_runtime| {
            ui_runtime.focus_manager().add_global_key_handler(Rc::new(
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

    dispatch_platform_ui_runtime(
        dispatcher,
        RuntimeTask::TestCallback(Box::new(|ui_runtime| {
            ui_runtime.focus_manager().add_global_key_handler(Rc::new(
                |event: &flui_interaction::events::KeyboardEvent| event.code == Code::F4,
            ));
        })),
    )
    .expect("install the shortcut");
    assert!(
        native.inject_event(alt_f4()).default_prevented,
        "a consumed system key prevents the platform default"
    );
    teardown_platform_ui_runtime();
}

fn late_event_never_crosses_ui_runtime_incarnations() {
    let stale = install_test_ui_runtime();
    teardown_platform_ui_runtime();
    assert_eq!(
        dispatch_platform_ui_runtime(stale, RuntimeTask::TestCallback(Box::new(|_| {}))),
        Err(DispatchError::RuntimeUnavailable)
    );

    let current = install_test_ui_runtime();
    assert_eq!(
        dispatch_platform_ui_runtime(stale, RuntimeTask::TestCallback(Box::new(|_| {}))),
        Err(DispatchError::StaleRuntime)
    );
    dispatch_platform_ui_runtime(current, RuntimeTask::TestCallback(Box::new(|_| {})))
        .expect("current incarnation dispatches");
}

fn panic_restores_dispatch_host_for_next_event() {
    let dispatcher = install_test_ui_runtime();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = dispatch_platform_ui_runtime(
            dispatcher,
            RuntimeTask::TestCallback(Box::new(|_| panic!("test panic"))),
        );
    }));
    assert!(panic.is_err());

    let ran = Rc::new(RefCell::new(false));
    let ran_in_event = Rc::clone(&ran);
    dispatch_platform_ui_runtime(
        dispatcher,
        RuntimeTask::TestCallback(Box::new(move |_| {
            *ran_in_event.borrow_mut() = true;
        })),
    )
    .expect("host restored");
    assert!(*ran.borrow());
}

// ========================================================================
// Issue #555 red exploits: multi-ui_runtime `AppRuntime` hosting,
// separate-ui_runtimes policy, exit policy, hot-restart.
//
// Every test below was structurally impossible to even set up before this
// change: `AppRuntime` had exactly one `Option<UiRuntime>` slot, so a
// second `install_platform_ui_runtime`/`install_ui_runtime_alongside` call always
// displaced the first rather than coexisting with it. There was no red
// run to capture (the code to call did not exist), so "red" here means
// "would not compile / had no second-ui_runtime install path to call" rather
// than "compiled and failed an assertion" -- stated rather than
// fabricated.
// ========================================================================

/// Installs two coexisting UI runtimes, A (through the legacy displacing
/// `install_platform_ui_runtime`) and B (through `install_ui_runtime_alongside`,
/// non-displacing). Both windows come from ONE shared headless platform
/// instance, not two separate `test_window()` calls: `HeadlessPlatform`
/// mints window ids from an instance-local counter, so two windows from
/// two SEPARATE `headless_platform()` calls can alias the same id —
/// fatal for two UI runtimes meant to coexist, since `WindowRegistry::
/// register` REPLACES on a matching id, silently dropping ui_runtime
/// A's window mapping the moment UI runtime B's aliased-id window installs.
fn install_two_test_ui_runtimes() -> (PresentationDispatcher, PresentationDispatcher) {
    let platform = flui_platform::headless_platform();
    let window_a: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create window a");
    let window_b: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create window b");
    let dispatcher_a =
        install_platform_ui_runtime(crate::app::ui_runtime::UiRuntime::for_test(), &window_a);
    let dispatcher_b =
        install_ui_runtime_alongside(crate::app::ui_runtime::UiRuntime::for_test(), &window_b)
            .expect("ui_runtime B installs alongside ui_runtime A cleanly");
    (dispatcher_a, dispatcher_b)
}

fn carried_work_shares_one_callback_budget_across_runtimes() {
    let _clear = OwnerHostClearGuard::arm();
    let (a, b) = install_two_test_ui_runtimes();
    let posted = Rc::new(Cell::new(0));
    let wake_posts = Rc::clone(&posted);
    APP_RUNTIME.with(|slot| {
        slot.borrow_mut().owner_turn_wake = Some(Rc::new(move || {
            wake_posts.set(wake_posts.get() + 1);
            true
        }));
    });
    let order = Rc::new(RefCell::new(Vec::new()));
    let admitted = Rc::clone(&order);
    dispatch_platform_ui_runtime(
        a,
        RuntimeTask::TestCallback(Box::new(move |_| {
            for index in 0..80 {
                let delivered = Rc::clone(&admitted);
                dispatch_platform_ui_runtime(
                    if index % 2 == 0 { a } else { b },
                    RuntimeTask::TestCallback(Box::new(move |_| {
                        delivered.borrow_mut().push(index);
                    })),
                )
                .expect("reentrant work accepted");
            }
        })),
    )
    .expect("initial turn");
    assert_eq!(*order.borrow(), (0..31).collect::<Vec<_>>());
    assert_eq!(posted.get(), 1, "one continuation for the accepted tail");

    with_owner_callback(|_| {
        let delivered = Rc::clone(&order);
        dispatch_platform_ui_runtime(
            a,
            RuntimeTask::TestCallback(Box::new(move |_| {
                delivered.borrow_mut().push(100);
            })),
        )
        .expect("fresh native root joins the carried FIFO");
        // A nested native callback must not replenish the enclosing budget.
        with_owner_callback(|_| ());
    });
    let mut expected: Vec<_> = (0..63).collect();
    assert_eq!(*order.borrow(), expected);
    assert_eq!(
        posted.get(),
        2,
        "remaining work gets one further opportunity"
    );

    with_owner_callback(|_| ());
    expected.extend(63..80);
    expected.push(100);
    assert_eq!(
        *order.borrow(),
        expected,
        "every accepted operation runs once"
    );
    assert_eq!(posted.get(), 2, "settled FIFO posts no extra continuation");
    teardown_platform_ui_runtime();
}

fn test_resize_driver(
    resize: impl FnMut(flui_foundation::geometry::Size<f64>, f64) + 'static,
) -> super::super::frame_driver::FrameDriver {
    use super::super::frame_driver::{FrameDriver, TestFrameDriver};
    FrameDriver::Test(TestFrameDriver {
        installed: None,
        sink: flui_runtime::testing::ScriptedSink::always_presents(),
        prelude: None,
        resize: Some(Box::new(resize)),
    })
}

fn native_keyboard_cannot_overtake_queued_window_changes_and_keys() {
    use flui_interaction::{events::Code, testing::input::KeyEventBuilder};

    for posted in [false, true] {
        let _clear = OwnerHostClearGuard::arm();
        let window = test_window();
        let runtime = crate::app::ui_runtime::UiRuntime::for_test();
        let trace = Rc::new(RefCell::new(Vec::new()));
        let key_trace = Rc::clone(&trace);
        runtime
            .focus_manager()
            .add_global_key_handler(Rc::new(move |event| {
                key_trace.borrow_mut().push(if event.code == Code::F4 {
                    "old key"
                } else {
                    "new key"
                });
                false
            }));
        let mut installation = prepare_replacement_ui_runtime(runtime, Arc::clone(&window));
        let dispatcher = installation.dispatcher();
        install_input_wiring(dispatcher, window.as_ref());
        let resize_trace = Rc::clone(&trace);
        installation
            .frame_driver(test_resize_driver(move |_, _| {
                resize_trace.borrow_mut().push("resize");
            }))
            .expect("prepare resize driver");
        installation
            .submit()
            .outcome()
            .expect("idle host")
            .expect("published keyboard window");
        APP_RUNTIME.with(|slot| slot.borrow_mut().owner_turn_wake = Some(Rc::new(move || posted)));
        dispatch_platform_ui_runtime(
            dispatcher,
            RuntimeTask::TestCallback(Box::new(move |_| {
                for _ in 0..31 {
                    dispatch_platform_ui_runtime(
                        dispatcher,
                        RuntimeTask::TestCallback(Box::new(|_| {})),
                    )
                    .expect("queued work");
                }
                for width in [810.0, 820.0] {
                    dispatch_platform_ui_runtime(
                        dispatcher,
                        RuntimeTask::Event(RuntimeEvent::Resized {
                            size: flui_foundation::geometry::Size::new(width, 600.0),
                            scale_factor: 1.0,
                        }),
                    )
                    .expect("resize");
                    if width == 810.0 {
                        dispatch_platform_input(
                            dispatcher,
                            PlatformInput::Keyboard(KeyEventBuilder::new(Code::F4).build()),
                        );
                    }
                }
            })),
        )
        .expect("first callback");
        assert!(trace.borrow().is_empty());
        with_owner_callback(|_| {
            let native = window
                .as_any()
                .downcast_ref::<flui_platform::MockWindow>()
                .expect("native mock");
            let result = native.inject_event(PlatformInput::Keyboard(
                KeyEventBuilder::new(Code::F5).build(),
            ));
            assert!(
                result.default_prevented,
                "deferred key suppresses native default"
            );
        });
        assert_eq!(*trace.borrow(), ["resize", "old key", "resize", "new key"]);
    }
}

fn reentrant_owner_turns_preserve_global_fifo_across_ui_runtimes() {
    let (dispatcher_a, dispatcher_b) = install_two_test_ui_runtimes();

    let order = Rc::new(RefCell::new(Vec::new()));
    let order_in_outer_a = Rc::clone(&order);
    let order_in_b = Rc::clone(&order);
    let order_in_queued_a = Rc::clone(&order);

    dispatch_platform_ui_runtime(
        dispatcher_a,
        RuntimeTask::TestCallback(Box::new(move |_ui_runtime| {
            order_in_outer_a.borrow_mut().push("a:outer:start");
            dispatch_platform_ui_runtime(
                dispatcher_b,
                RuntimeTask::TestCallback(Box::new(move |_| {
                    order_in_b.borrow_mut().push("b");
                })),
            )
            .expect("ui_runtime B turn is admitted");
            dispatch_platform_ui_runtime(
                dispatcher_a,
                RuntimeTask::TestCallback(Box::new(move |_| {
                    order_in_queued_a.borrow_mut().push("a:queued");
                })),
            )
            .expect("second ui_runtime A turn is admitted");
            order_in_outer_a.borrow_mut().push("a:outer:end");
        })),
    )
    .expect("the outer turn drains every admitted turn");

    assert_eq!(
        order.borrow().as_slice(),
        ["a:outer:start", "a:outer:end", "b", "a:queued"]
    );

    teardown_platform_ui_runtime();
}

/// Installs UI runtime A via the legacy single-window path (mirroring
/// `install_test_ui_runtime`), through a REAL `OwnerPlatform` (unlike
/// `install_test_ui_runtime`/`install_two_test_ui_runtimes`, which never install
/// one) — both windows this helper and its caller open come from the
/// SAME headless platform instance, so their native window identities
/// cannot collide once both are registered in the one `WindowRegistry`.
fn install_ui_runtime_a_through_a_real_owner_platform()
-> (PresentationDispatcher, OwnerHostClearGuard) {
    install_ui_runtime_a_with_driver(None)
}

fn install_ui_runtime_a_with_driver(
    driver: Option<super::super::frame_driver::FrameDriver>,
) -> (PresentationDispatcher, OwnerHostClearGuard) {
    use std::{cell::Cell, rc::Rc};

    let clear_guard = OwnerHostClearGuard::arm();
    let platform = flui_platform::headless_platform();
    let dispatcher_a_slot: Rc<Cell<Option<PresentationDispatcher>>> = Rc::new(Cell::new(None));
    let dispatcher_a_slot_for_on_ready = Rc::clone(&dispatcher_a_slot);
    let ready = platform.run(Box::new(move |owner| {
        install_owner_platform(owner).expect("install owner wake transport");
        let window_a: Arc<dyn PlatformWindow> =
            with_owner_platform(|owner| owner.open_window(flui_platform::WindowOptions::default()))
                .expect("BUG: install_owner_platform just ran above")
                .and_then(flui_platform::WindowOpen::try_ready)
                .expect("headless open_window is always Ready");
        let mut installation =
            prepare_replacement_ui_runtime(crate::app::ui_runtime::UiRuntime::for_test(), window_a);
        if let Some(driver) = driver {
            installation
                .frame_driver(driver)
                .expect("prepare primary driver");
        }
        let dispatcher = installation.dispatcher();
        installation
            .submit()
            .outcome()
            .expect("idle host")
            .expect("published primary");
        dispatcher_a_slot_for_on_ready.set(Some(dispatcher));
        Ok(())
    }));
    ready.expect("installing ui_runtime A must not fail");
    (
        dispatcher_a_slot.get().expect("set inside on_ready above"),
        clear_guard,
    )
}

/// `WindowPolicy::Isolated`, driven through the REAL embedder seam
/// (`open_secondary_window`) rather than a direct
/// `install_ui_runtime_alongside` call — proves the POLICY-driven fork itself
/// routes to the share-nothing path, not just the underlying primitive.
/// The oracle: a pointer dispatched only to UI runtime A must leave UI runtime B's
/// gesture arena completely untouched.
fn two_ui_runtimes_via_isolated_policy_share_nothing() {
    let (dispatcher_a, _clear_guard) = install_ui_runtime_a_through_a_real_owner_platform();

    open_secondary_window(AppConfig::default(), WindowPolicy::Isolated)
        .expect("WindowPolicy::Isolated must install a second ui_runtime cleanly");

    let dispatcher_b = APP_RUNTIME
        .with(|slot| {
            let state = slot.borrow();
            let host = state.installed_host.logical();
            let id = host.runtime_ids().expect("host readable").into_iter()
                .find(|id| *id != dispatcher_a.address.ui_runtime_id)?;
            Some(PresentationDispatcher {
                owner_thread: state.owner_thread?,
                address: host.runtime_status(id).ok()?.primary,
            })
        })
        .expect(
            "open_secondary_window(WindowPolicy::Isolated) must install a second, distinct ui_runtime",
        );

    assert_ne!(
        dispatcher_a.address.ui_runtime_id, dispatcher_b.address.ui_runtime_id,
        "WindowPolicy::Isolated must install a genuinely distinct ui_runtime, never \
         displace or merge into ui_runtime A"
    );

    dispatch_platform_ui_runtime(
        dispatcher_a,
        RuntimeTask::TestCallback(Box::new(|ui_runtime| {
            let down = down_input(4.0);
            if let PlatformInput::Pointer(event) = down {
                ui_runtime
                    .gestures()
                    .handle_pointer_event(&event, |_| HitTestResult::new());
            }
            assert_eq!(
                ui_runtime.gestures().active_pointer_count(),
                1,
                "ui_runtime A observes its own pointer"
            );
        })),
    )
    .expect("A dispatches");

    dispatch_platform_ui_runtime(
        dispatcher_b,
        RuntimeTask::TestCallback(Box::new(|ui_runtime| {
            assert_eq!(
                ui_runtime.gestures().active_pointer_count(),
                0,
                "the policy-driven secondary ui_runtime must share no gesture-arena state with \
                 ui_runtime A -- open_secondary_window(WindowPolicy::Isolated) must route through \
                 install_ui_runtime_alongside, never a path that merges state with a sibling"
            );
        })),
    )
    .expect("B dispatches independently of A");

    teardown_platform_ui_runtime();
}

fn first_build_text_scale(runtime: &mut crate::app::ui_runtime::UiRuntime) -> f64 {
    use flui_view::prelude::*;

    #[derive(Clone, StatelessView)]
    struct InitialPreferences(Rc<RefCell<Vec<f64>>>);

    impl StatelessView for InitialPreferences {
        fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
            let scale = flui_widgets::MediaQuery::text_scale_factor_of(ctx)
                .expect("runtime inherited preferences");
            self.0.borrow_mut().push(scale);
            flui_widgets::SizedBox::new(20.0 * scale, 20.0)
        }
    }

    let observed = Rc::default();
    runtime
        .attach_root_widget_with_size(&InitialPreferences(Rc::clone(&observed)), 800.0, 600.0)
        .expect("mount preference reader");
    let mut sink = flui_runtime::testing::ScriptedSink::always_presents();
    assert!(
        runtime
            .pump(
                &mut flui_runtime::pump::SampledClock(web_time::Instant::now()),
                &mut sink,
            )
            .presented()
    );
    let values = observed.borrow();
    assert_eq!(values.len(), 1, "one initial build");
    values[0]
}

fn deferred_native_reads_preserve_accepted_preferences() {
    let _clear = OwnerHostClearGuard::arm();
    flui_platform::headless_platform()
        .run(Box::new(|owner| {
            install_owner_platform(owner)?;
            super::super::host::refresh_preferences_with(|_| {
                Ok(flui_platform_api::SystemPreferences::default()
                    .with_text_scale(2.0)
                    .expect("valid scale"))
            })?;
            super::super::host::refresh_preferences_with(|_| {
                Err(flui_platform::PlatformError::PreferencesDeferred)
            })
            .expect("deferred observation is not a new read failure");
            let mut runtime = super::super::host::build_ui_runtime(
                &super::super::host::runtime_wake_callback(),
                test_window(),
                1.0,
            )?;
            assert_eq!(
                first_build_text_scale(&mut runtime),
                2.0,
                "deferred read replaced accepted preferences"
            );
            super::super::host::refresh_preferences_with(|_| {
                Ok(flui_platform_api::SystemPreferences::default()
                    .with_text_scale(3.0)
                    .expect("valid scale"))
            })?;
            let mut runtime = super::super::host::build_ui_runtime(
                &super::super::host::runtime_wake_callback(),
                test_window(),
                1.0,
            )?;
            assert_eq!(
                first_build_text_scale(&mut runtime),
                3.0,
                "later native observation was lost"
            );
            teardown_platform_ui_runtime();
            Ok(())
        }))
        .expect("deferred source recovery");
}

fn owner_wake_refreshes_installed_preference_consumers() {
    use super::super::frame_driver::{FrameDriver, TestFrameDriver};
    use flui_platform::{HeadlessPlatform, Platform};
    use flui_view::prelude::*;

    #[derive(Clone, StatelessView)]
    struct Reader(Rc<RefCell<Vec<f64>>>);
    impl StatelessView for Reader {
        fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
            let scale = flui_widgets::MediaQuery::text_scale_factor_of(ctx).expect("preferences");
            self.0.borrow_mut().push(scale);
            flui_widgets::SizedBox::new(20.0 * scale, 20.0)
        }
    }
    let _clear = OwnerHostClearGuard::arm();
    let platform = HeadlessPlatform::new();
    let turns = platform.owner_turns();
    let observed = Rc::new(RefCell::new(Vec::new()));
    let reader = Rc::clone(&observed);
    let frame = Rc::new(RefCell::new(None));
    let output = Rc::clone(&frame);
    Box::new(platform)
        .run(Box::new(move |owner| {
            install_owner_platform(owner)?;
            let host = APP_RUNTIME.with(|slot| slot.borrow().installed_host.clone());
            host.logical().update_preferences(
                flui_platform_api::SystemPreferences::default().with_text_scale(2.0)?,
                host.effects(),
            )?;
            let runtime = super::super::host::build_ui_runtime(
                &super::super::host::runtime_wake_callback(),
                test_window(),
                1.0,
            )?;
            runtime.attach_root_widget_with_size(&Reader(reader), 800.0, 600.0)?;
            let mut installation = prepare_platform_ui_runtime(runtime, test_window());
            let dispatcher = installation.dispatcher();
            let frame = installation
                .frame_driver(FrameDriver::Test(TestFrameDriver {
                    installed: None,
                    sink: flui_runtime::testing::ScriptedSink::always_presents(),
                    prelude: None,
                    resize: None,
                }))
                .expect("prepare driver");
            installation
                .submit()
                .outcome()
                .expect("idle publication")
                .expect("installed");
            output.replace(Some((dispatcher, frame.binding)));
            Ok(())
        }))
        .expect("headless bootstrap");
    let (dispatcher, frame) = frame.borrow().expect("installed frame");
    dispatch_platform_ui_runtime(dispatcher, RuntimeTask::Frame(frame)).expect("initial frame");
    assert_eq!(&*observed.borrow(), &[2.0]);
    with_owner_platform(|owner| owner.proxy().wake())
        .expect("owner")
        .expect("wake");
    turns.drive();
    dispatch_platform_ui_runtime(dispatcher, RuntimeTask::Frame(frame)).expect("refreshed frame");
    assert_eq!(
        &*observed.borrow(),
        &[2.0, 1.0],
        "owner wake did not deliver the headless source's absent text-scale observation"
    );
    teardown_platform_ui_runtime();
}

fn obsolete_native_observation_cannot_update_a_replacement_host() {
    for replace_platform in [false, true] {
        for read_fails in [false, true] {
            let _clear = OwnerHostClearGuard::arm();
            flui_platform::headless_platform()
                .run(Box::new(move |owner| {
                    install_owner_platform(owner)?;
                    let result = super::super::host::refresh_preferences_with(|_| {
                        if replace_platform {
                            flui_platform::headless_platform()
                                .run(Box::new(|owner| {
                                    install_owner_platform(owner)?;
                                    Ok(())
                                }))
                                .expect("replace native owner during getter");
                        } else {
                            let previous = APP_RUNTIME.with(|slot| {
                                std::mem::replace(
                                    &mut slot.borrow_mut().installed_host,
                                    super::super::InstalledHost::new(),
                                )
                            });
                            previous.shutdown();
                        }
                        let host = APP_RUNTIME.with(|slot| slot.borrow().installed_host.clone());
                        host.logical()
                            .update_preferences(
                                flui_platform_api::SystemPreferences::default()
                                    .with_text_scale(3.0)
                                    .expect("valid scale"),
                                host.effects(),
                            )
                            .expect("new host accepts its observation");
                        if read_fails {
                            Err(flui_platform::PlatformError::Preferences {
                                message: "obsolete native read failed".into(),
                            })
                        } else {
                            Ok(flui_platform_api::SystemPreferences::default()
                                .with_text_scale(2.0)
                                .expect("valid scale"))
                        }
                    });
                    assert!(
                        result.is_ok(),
                        "an obsolete source must not report failure against its replacement"
                    );
                    let mut runtime = super::super::host::build_ui_runtime(
                        &super::super::host::runtime_wake_callback(),
                        test_window(),
                        1.0,
                    )?;
                    assert_eq!(
                        first_build_text_scale(&mut runtime),
                        3.0,
                        "obsolete observation overwrote replacement source"
                    );
                    teardown_platform_ui_runtime();
                    Ok(())
                }))
                .expect("native observation replacement matrix");
        }
    }
}

#[cfg(target_os = "windows")]
fn windows_bootstrap_accepts_preferences_before_the_first_window() {
    use flui_platform::Platform;

    let _clear = OwnerHostClearGuard::arm();
    Box::new(flui_platform::WindowsPlatform::new().expect("native platform"))
        .run(Box::new(|owner| {
            let expected = owner
                .preferences()?
                .text_scale()
                .expect("Windows text scale");
            let proxy = owner.proxy();
            install_owner_platform(owner)?;
            let host = APP_RUNTIME.with(|slot| slot.borrow().installed_host.clone());
            assert!(
                host.logical().preferences()?.is_some(),
                "bootstrap never accepted its native observation"
            );
            let mut runtime = super::super::host::build_ui_runtime(
                &super::super::host::runtime_wake_callback(),
                test_window(),
                1.0,
            )?;
            assert_eq!(first_build_text_scale(&mut runtime), expected);
            teardown_platform_ui_runtime();
            proxy.request_quit()?;
            Ok(())
        }))
        .expect("native preference bootstrap");
}

fn bootstrap_keeps_the_host_that_seeded_the_first_build() {
    let _clear = OwnerHostClearGuard::arm();
    flui_platform::headless_platform()
        .run(Box::new(|owner| {
            install_owner_platform(owner)?;
            let host = APP_RUNTIME.with(|slot| slot.borrow().installed_host.clone());
            host.logical().update_preferences(
                flui_platform_api::SystemPreferences::default().with_text_scale(2.0)?,
                host.effects(),
            )?;
            let window: Arc<dyn PlatformWindow> = with_owner_platform(|owner| {
                owner.open_window(flui_platform::WindowOptions::default())
            })
            .expect("owner installed")?
            .try_ready()?;
            let mut runtime = super::super::host::build_ui_runtime(
                &super::super::host::runtime_wake_callback(),
                Arc::clone(&window),
                1.0,
            )?;
            assert_eq!(
                first_build_text_scale(&mut runtime),
                2.0,
                "first build must use accepted host preferences"
            );
            let installation = prepare_platform_ui_runtime(runtime, window);
            let dispatcher = installation.dispatcher();
            installation
                .submit()
                .outcome()
                .expect("idle publication")
                .expect("bootstrap retains the authorizing host");
            assert!(
                host.logical()
                    .presentation_dispatcher(dispatcher.address)
                    .is_ok(),
                "the source host also owns the published runtime"
            );
            teardown_platform_ui_runtime();
            Ok(())
        }))
        .expect("headless bootstrap");
}

fn a_replacement_platform_starts_a_new_preference_owner() {
    let _first_clear = OwnerHostClearGuard::arm();
    let previous = Rc::new(RefCell::new(None));
    let captured = Rc::clone(&previous);
    flui_platform::headless_platform()
        .run(Box::new(move |owner| {
            install_owner_platform(owner)?;
            let host = APP_RUNTIME.with(|slot| slot.borrow().installed_host.clone());
            host.logical().update_preferences(
                flui_platform_api::SystemPreferences::default().with_text_scale(2.0)?,
                host.effects(),
            )?;
            *captured.borrow_mut() = Some(host);
            Ok(())
        }))
        .expect("first native owner");
    let _second_clear = OwnerHostClearGuard::arm();
    flui_platform::headless_platform()
        .run(Box::new(move |owner| {
            install_owner_platform(owner)?;
            let previous = previous.borrow_mut().take().expect("first host retained");
            assert!(
                matches!(
                    previous.logical().preferences(),
                    Err(flui_runtime::owner::DispatchError::Closed)
                ),
                "replacing the native owner retires its logical source"
            );
            let mut runtime = super::super::host::build_ui_runtime(
                &super::super::host::runtime_wake_callback(),
                test_window(),
                1.0,
            )?;
            assert_eq!(
                first_build_text_scale(&mut runtime),
                1.0,
                "a replacement must not inherit another platform's observations"
            );
            teardown_platform_ui_runtime();
            Ok(())
        }))
        .expect("replacement native owner");
}

fn platform_replacement_contains_reentrant_retirement() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct RetiredProducer {
        retired: Arc<AtomicUsize>,
        scale: f64,
        failure: Option<&'static str>,
    }
    impl flui_platform_api::Clipboard for RetiredProducer {
        fn read_text(&self) -> Option<String> {
            None
        }
        fn write_text(&self, _: String) {}
    }
    impl Drop for RetiredProducer {
        fn drop(&mut self) {
            let current = APP_RUNTIME.with(|slot| slot.borrow().installed_host.clone());
            current
                .logical()
                .update_preferences(
                    flui_platform_api::SystemPreferences::default()
                        .with_text_scale(self.scale)
                        .expect("valid observation"),
                    current.effects(),
                )
                .expect("retirement reenters the published replacement");
            self.retired.fetch_add(1, Ordering::SeqCst);
            if let Some(failure) = self.failure {
                std::panic::panic_any(failure);
            }
        }
    }

    for (wake_fails, clipboard_fails) in
        [(false, false), (true, false), (false, true), (true, true)]
    {
        let _first_clear = OwnerHostClearGuard::arm();
        let retired = Arc::new(AtomicUsize::new(0));
        let first_retired = Arc::clone(&retired);
        flui_platform::headless_platform()
            .run(Box::new(move |owner| {
                install_owner_platform(owner)?;
                let hook = RetiredProducer {
                    retired: Arc::clone(&first_retired),
                    scale: 1.5,
                    failure: wake_fails.then_some("retired wake"),
                };
                let clipboard = Arc::new(RetiredProducer {
                    retired: first_retired,
                    scale: 2.5,
                    failure: clipboard_fails.then_some("retired clipboard"),
                });
                let (old_hook, old_clipboard) = APP_RUNTIME.with(|slot| {
                    let mut state = slot.borrow_mut();
                    let old_hook = state.owner_turn_wake.replace(Rc::new(move || {
                        std::hint::black_box(&hook);
                        true
                    }));
                    (old_hook, state.set_platform_clipboard(clipboard))
                });
                drop(old_hook);
                drop(old_clipboard);
                Ok(())
            }))
            .expect("first owner");
        let _second_clear = OwnerHostClearGuard::arm();
        flui_platform::headless_platform()
            .run(Box::new(move |owner| {
                let result = catch_unwind(AssertUnwindSafe(|| install_owner_platform(owner)));
                if wake_fails || clipboard_fails {
                    let failure = result.expect_err("retirement failure escapes after cleanup");
                    assert_eq!(
                        failure.downcast_ref::<&str>(),
                        Some(&if wake_fails {
                            "retired wake"
                        } else {
                            "retired clipboard"
                        })
                    );
                } else {
                    result
                        .expect("healthy retirement")
                        .expect("replacement installed");
                }
                assert_eq!(
                    retired.load(Ordering::SeqCst),
                    2,
                    "every outgoing producer retired"
                );
                let mut runtime = super::super::host::build_ui_runtime(
                    &super::super::host::runtime_wake_callback(),
                    test_window(),
                    1.0,
                )?;
                assert_eq!(
                    first_build_text_scale(&mut runtime),
                    2.5,
                    "reentrant updates remain usable after competing retirement failures"
                );
                teardown_platform_ui_runtime();
                Ok(())
            }))
            .expect("replacement remains usable");
    }
}

/// Every UI runtime a runner builds shapes over the app's one font collection
/// (ADR-0092 §2): two `WindowPolicy::Isolated` windows go through
/// `host::build_ui_runtime`, the one call every runner site (desktop, web,
/// Android, iOS, secondary windows) builds its UI runtime with, and each UI runtime's
/// `TextContext` must be built over `runtime_font_collection()`. Runtime A comes
/// from `UiRuntime::for_test`, which builds its own collection, so it is not
/// asserted on. Fails if that call hands a UI runtime a fresh collection (a UI runtime
/// fed from the host again would be one), or if the runtime resolves a new one
/// per call. `the_runtime_launches_one_host_feed_for_every_ui_runtime` pins that
/// the runtime's collection is the one its host feed feeds.
fn isolated_windows_shape_over_the_runtimes_font_collection() {
    let (dispatcher_a, _clear_guard) = install_ui_runtime_a_through_a_real_owner_platform();

    for _ in 0..2 {
        open_secondary_window(AppConfig::default(), WindowPolicy::Isolated)
            .expect("WindowPolicy::Isolated must install a second ui_runtime cleanly");
    }

    let secondaries: Vec<PresentationDispatcher> = APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        let owner_thread = state.owner_thread.expect("the owner platform is installed");
        state
            .installed_host
            .logical()
            .runtime_ids()
            .expect("host readable")
            .into_iter()
            .filter(|id| *id != dispatcher_a.address.ui_runtime_id)
            .map(|id| PresentationDispatcher {
                owner_thread,
                address: state
                    .installed_host
                    .logical()
                    .runtime_status(id)
                    .expect("runtime installed")
                    .primary,
            })
            .collect()
    });
    assert_eq!(
        secondaries.len(),
        2,
        "two WindowPolicy::Isolated windows, two ui_runtimes"
    );

    let app_fonts = super::super::host::runtime_font_collection();
    for dispatcher in secondaries {
        let app_fonts = app_fonts.clone();
        dispatch_platform_ui_runtime(
            dispatcher,
            RuntimeTask::TestCallback(Box::new(move |ui_runtime| {
                assert!(
                    ui_runtime.text_context_for_test().with(|text| {
                        flui_painting::FontCollection::ptr_eq(text.fonts(), &app_fonts)
                    }),
                    "a runner-built ui_runtime must own a text context over the app's font collection"
                );
            })),
        )
        .expect("the secondary ui_runtime dispatches");
    }

    teardown_platform_ui_runtime();
}

/// `WindowPolicy::Shared`, driven through the REAL embedder seam
/// (`open_secondary_window`) — proves this really is forest-membership
/// routing (a second PRESENTATION of the SAME UI runtime), not a second UI runtime
/// in disguise: the hosted-UI runtime count must stay at one, while the
/// UI runtime's own presentation count grows from one to two.
fn one_ui_runtime_two_windows_policy_routes_by_presentation() {
    let (dispatcher_a, _clear_guard) = install_ui_runtime_a_through_a_real_owner_platform();

    let ui_runtime_count_before =
        APP_RUNTIME.with(|slot| slot.borrow().installed_host.logical().runtime_count());
    let presentation_count_before = observe_runtime(
        dispatcher_a,
        crate::app::ui_runtime::UiRuntime::presentation_count,
    );
    assert_eq!(ui_runtime_count_before, 1);
    assert_eq!(presentation_count_before, 1);

    open_secondary_window(AppConfig::default(), WindowPolicy::Shared).expect(
        "WindowPolicy::Shared must install a second presentation into ui_runtime A cleanly",
    );

    let ui_runtime_count_after =
        APP_RUNTIME.with(|slot| slot.borrow().installed_host.logical().runtime_count());
    let presentation_count_after = observe_runtime(
        dispatcher_a,
        crate::app::ui_runtime::UiRuntime::presentation_count,
    );
    assert_eq!(
        ui_runtime_count_after, ui_runtime_count_before,
        "WindowPolicy::Shared must NOT install a second ui_runtime -- it routes into the \
         existing one via a second presentation"
    );
    assert_eq!(
        presentation_count_after, 2,
        "WindowPolicy::Shared must install a genuine second presentation into ui_runtime \
         A's forest -- real forest-membership routing, not a second ui_runtime in disguise"
    );

    teardown_platform_ui_runtime();
}

/// A `Resized` stamped for a secondary window of a shared UI runtime, delivered
/// through `dispatch_platform_ui_runtime`, rescales that window's pipeline (and so
/// its semantics bounds) and leaves the primary's at its old ratio.
fn resized_rescales_only_the_addressed_presentation() {
    let (primary, _clear_guard) = install_ui_runtime_a_through_a_real_owner_platform();
    let (secondary, _window) = super::super::secondary_window::open_secondary_window_impl(
        AppConfig::default(),
        WindowPolicy::Shared,
    )
    .expect("WindowPolicy::Shared installs a second presentation into ui_runtime A")
    .expect("the headless platform opens windows Ready");
    assert_eq!(
        secondary.address.ui_runtime_id,
        primary.address.ui_runtime_id
    );
    assert_ne!(
        secondary.address.presentation_id,
        primary.address.presentation_id
    );

    let ratios = || {
        observe_runtime(primary, move |runtime| {
            let ratio = |id| {
                runtime
                    .presentation_device_pixel_ratio_for_test(id)
                    .expect("both presentations are resident")
            };
            (
                ratio(primary.address.presentation_id),
                ratio(secondary.address.presentation_id),
            )
        })
    };
    let (primary_before, secondary_before) = ratios();
    let rescaled = primary_before.max(secondary_before) + 1.5;

    dispatch_platform_ui_runtime(
        secondary,
        RuntimeTask::Event(RuntimeEvent::Resized {
            size: flui_foundation::geometry::Size::new(640.0, 480.0),
            scale_factor: rescaled,
        }),
    )
    .expect("the secondary presentation is live");

    let (primary_after, secondary_after) = ratios();
    assert!(
        (secondary_after - rescaled).abs() < f64::EPSILON,
        "the addressed secondary window must take the reported ratio, got {secondary_after}"
    );
    assert!(
        (primary_after - primary_before).abs() < f64::EPSILON,
        "the primary window must keep its own ratio, got {primary_after} (was {primary_before})"
    );

    teardown_platform_ui_runtime();
}

/// A `Resized` stamped for a secondary window of a shared UI runtime never reaches
/// the installed frame driver, which belongs to the primary window's
/// renderer: the primary's surface keeps its size, so the constraints its
/// next frame is laid out under (surface / primary ratio) stay put. A
/// `Resized` for the primary still applies.
fn resizing_a_secondary_leaves_the_primary_surface_alone() {
    use std::{cell::Cell, rc::Rc};

    let surface = Rc::new(Cell::new((800_u32, 600_u32)));
    let applied = Rc::clone(&surface);
    let driver = test_resize_driver(move |size, scale_factor| {
        applied.set((
            (size.width * scale_factor) as u32,
            (size.height * scale_factor) as u32,
        ));
    });
    let (primary, _clear_guard) = install_ui_runtime_a_with_driver(Some(driver));
    let (secondary, _window) = super::super::secondary_window::open_secondary_window_impl(
        AppConfig::default(),
        WindowPolicy::Shared,
    )
    .expect("WindowPolicy::Shared installs a second presentation into ui_runtime A")
    .expect("the headless platform opens windows Ready");
    let primary_ratio = || {
        observe_runtime(primary, move |runtime| {
            runtime
                .presentation_device_pixel_ratio_for_test(primary.address.presentation_id)
                .expect("primary presentation is resident")
        })
    };
    let primary_constraints = || {
        let (width, height) = surface.get();
        let ratio = primary_ratio();
        (f64::from(width) / ratio, f64::from(height) / ratio)
    };
    let before = primary_constraints();

    dispatch_platform_ui_runtime(
        secondary,
        RuntimeTask::Event(RuntimeEvent::Resized {
            size: flui_foundation::geometry::Size::new(320.0, 200.0),
            scale_factor: 2.5,
        }),
    )
    .expect("the secondary presentation is live");
    assert_eq!(
        surface.get(),
        (800, 600),
        "a secondary window's resize must not reach the primary's surface"
    );
    assert_eq!(
        primary_constraints(),
        before,
        "the primary's layout constraints must not move with a secondary's resize"
    );

    dispatch_platform_ui_runtime(
        primary,
        RuntimeTask::Event(RuntimeEvent::Resized {
            size: flui_foundation::geometry::Size::new(500.0, 400.0),
            scale_factor: 2.0,
        }),
    )
    .expect("the primary presentation is live");
    assert_eq!(
        surface.get(),
        (1000, 800),
        "the primary window's own resize still reaches its surface"
    );

    teardown_platform_ui_runtime();
}

/// Installs UI runtime A under `ExitPolicy::OnLastWindowClosed` with a quit
/// counter, and returns the parked re-evaluation handle with them.
///
/// The headless mock has no event loop, so a
/// `Platform::request_exit_policy_reevaluation` is PARKED and its
/// owner-thread half runs only when a test calls
/// `HeadlessExitReevaluation::drive`. A test asserting on an exit that a
/// re-evaluation produces therefore needs the handle.
fn install_ui_runtime_a_with_exit_policy_quit_counter_and_reevaluation() -> (
    PresentationDispatcher,
    std::sync::Arc<dyn flui_platform::traits::PlatformWindow>,
    Arc<std::sync::atomic::AtomicUsize>,
    OwnerHostClearGuard,
    flui_platform::HeadlessExitReevaluation,
) {
    use std::{cell::RefCell, rc::Rc, sync::atomic::AtomicUsize};

    type Installed = (
        PresentationDispatcher,
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
            install_platform_ui_runtime(crate::app::ui_runtime::UiRuntime::for_test(), &window_a);
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
    ready.expect("installing ui_runtime A must not fail");
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

/// A window closed REENTRANTLY, from inside a dispatched UI runtime callback,
/// still exits the app.
///
/// This was a tracked gap and this test pinned it. `close_this_window`'s
/// `RuntimeTask::ClosePresentation` is a same-runtime reentrant dispatch, so
/// it only enqueues onto the already-checked-out UI runtime's queue; the
/// uninstall applies at the OUTER dispatch's tail, strictly after
/// `window.close()`'s own `notify_closed` — and the exit-policy hook it
/// consulted — has already returned "don't exit". Nothing re-checked
/// afterwards, so the exit was missed rather than delayed.
///
/// The fix re-asks: `dispatch_platform_ui_runtime`'s tail requests a
/// re-evaluation whenever a deferred UI runtime-map mutation actually removed
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
        install_ui_runtime_a_with_exit_policy_quit_counter_and_reevaluation();

    dispatch_platform_ui_runtime(
        dispatcher,
        RuntimeTask::TestCallback(Box::new(move |_ui_runtime| {
            // Reentrant: this window's own `on_close` (wired by the
            // helper above, mirroring `run_desktop`) calls
            // `close_this_window(dispatcher)` -> `close_presentation` ->
            // a nested `dispatch_platform_ui_runtime` on the SAME ui_runtime this
            // Frame task's own dispatch already checked out.
            window_a.close();
        })),
    )
    .expect("the outer Frame dispatch itself must not be refused");

    assert_eq!(
        APP_RUNTIME.with(|slot| slot.borrow().installed_host.logical().runtime_count()),
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

    teardown_platform_ui_runtime();
}

fn panicking_stop_notifies_siblings_and_restores_runtime_delivery() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    for fail_second in [false, true] {
        let (first, second) = install_two_test_ui_runtimes();
        let notifications = Arc::new(AtomicUsize::new(0));
        let delivered = Arc::new(AtomicUsize::new(0));
        for (index, dispatcher) in [first, second].into_iter().enumerate() {
            dispatcher
                .runtime()
                .lifecycle(AppLifecycleState::Resumed)
                .expect("resume");
            let notifications = Arc::clone(&notifications);
            let delivered = Arc::clone(&delivered);
            dispatch_platform_ui_runtime(
                dispatcher,
                RuntimeTask::TestCallback(Box::new(move |runtime| {
                    runtime
                        .scheduler()
                        .add_lifecycle_state_listener(Arc::new(move |state| {
                            if state != AppLifecycleState::Detached {
                                return;
                            }
                            notifications.fetch_add(1, Ordering::SeqCst);
                            if index == 0 {
                                let notifications = Arc::clone(&notifications);
                                let delivered = Arc::clone(&delivered);
                                dispatch_platform_ui_runtime(
                                    second,
                                    RuntimeTask::TestCallback(Box::new(move |_| {
                                        assert_eq!(
                                            notifications.load(Ordering::SeqCst),
                                            2,
                                            "reentry waits for both terminal notifications"
                                        );
                                        delivered.fetch_add(1, Ordering::SeqCst);
                                    })),
                                )
                                .expect("accepted reentry");
                                std::panic::panic_any("first terminal listener");
                            }
                            if fail_second {
                                std::panic::panic_any("second terminal listener");
                            }
                        }));
                })),
            )
            .expect("install actual lifecycle listener");
        }
        let failure =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(stop_installed_ui_runtimes))
                .expect_err("listener failure resumes after cleanup");
        assert_eq!(
            failure.downcast_ref::<&str>(),
            Some(&"first terminal listener")
        );
        assert_eq!(notifications.load(Ordering::SeqCst), 2);
        assert_eq!(delivered.load(Ordering::SeqCst), 1);
        for dispatcher in [first, second] {
            let delivered = Arc::clone(&delivered);
            dispatch_platform_ui_runtime(
                dispatcher,
                RuntimeTask::TestCallback(Box::new(move |runtime| {
                    assert_eq!(
                        runtime.scheduler().lifecycle_state(),
                        AppLifecycleState::Detached
                    );
                    delivered.fetch_add(1, Ordering::SeqCst);
                })),
            )
            .expect("restored runtime executes");
        }
        assert_eq!(delivered.load(Ordering::SeqCst), 3);
        teardown_platform_ui_runtime();
    }
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

/// Clears `dispatcher`'s UI runtime's redraw request.
fn clear_redraw(dispatcher: PresentationDispatcher) {
    dispatch_platform_ui_runtime(
        dispatcher,
        RuntimeTask::TestCallback(Box::new(crate::app::ui_runtime::UiRuntime::mark_rendered)),
    )
    .expect("the ui_runtime dispatches");
}

/// Whether `dispatcher`'s UI runtime has a redraw requested.
fn redraw_requested(dispatcher: PresentationDispatcher) -> bool {
    let requested = Rc::new(Cell::new(false));
    let requested_in_task = Rc::clone(&requested);
    dispatch_platform_ui_runtime(
        dispatcher,
        RuntimeTask::TestCallback(Box::new(move |ui_runtime| {
            requested_in_task.set(ui_runtime.needs_redraw());
        })),
    )
    .expect("the ui_runtime dispatches");
    requested.get()
}

/// A recovered surface needs another scene even when the widget tree did not
/// change. Drive the same addressed operation as the mobile surface callbacks,
/// then the product pump: a redraw flag alone cannot satisfy this contract.
fn recovered_surface_notification_resubmits_the_scene() {
    let _clear = OwnerHostClearGuard::arm();
    let ui_runtime = crate::app::ui_runtime::UiRuntime::for_test();
    ui_runtime
        .attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0))
        .expect("root mounts");
    let mut installation = prepare_replacement_ui_runtime(ui_runtime, test_window());
    let dispatcher = installation.dispatcher();
    use super::super::frame_driver::{FrameDriver, TestFrameDriver};
    use std::sync::atomic::{AtomicUsize, Ordering};

    let submits = Arc::new(AtomicUsize::new(0));
    let sink = flui_runtime::testing::ScriptedSink::new({
        let submits = Arc::clone(&submits);
        move |_, _| {
            submits.fetch_add(1, Ordering::SeqCst);
            flui_runtime::sink::SubmitVerdict::Presented
        }
    });
    let frame = installation
        .frame_driver(FrameDriver::Test(TestFrameDriver {
            installed: None,
            sink,
            prelude: None,
            resize: None,
        }))
        .expect("prepare the frame driver");
    installation
        .submit()
        .outcome()
        .expect("idle host")
        .expect("published recovery window");
    let pump = || {
        dispatch_platform_ui_runtime(dispatcher, RuntimeTask::Frame(frame.binding))
            .expect("frame dispatches");
    };
    pump();
    assert_eq!(submits.load(Ordering::SeqCst), 1, "initial scene submitted");
    pump();
    assert_eq!(
        submits.load(Ordering::SeqCst),
        1,
        "idle pump submits nothing"
    );
    dispatch_platform_ui_runtime(
        dispatcher,
        RuntimeTask::Event(RuntimeEvent::PrimarySurfaceRestored),
    )
    .expect("surface notification accepted");
    pump();
    assert_eq!(
        submits.load(Ordering::SeqCst),
        2,
        "recovered surface gets a scene"
    );
}

/// A face registered while two UI runtime windows run tells both: each draws its
/// next frame, where its pipeline lays out again the text measured before.
/// Fails if the registration notifies no UI runtime, or only one.
fn a_registration_notifies_every_ui_runtime_window() {
    let (dispatcher_a, dispatcher_b) = install_two_test_ui_runtimes();
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

    teardown_platform_ui_runtime();
}

/// The host font feed landing off the owner thread tells every UI runtime window
/// at the next owner turn, which the feed's wake brings: the wake, called on
/// the feed's thread, asks the loop for a turn, and at that turn each window
/// draws its next frame, where its pipeline lays out again the text measured
/// before. Fails if the feed is launched with a wake that does not reach the
/// loop, or if an owner turn does not announce a generation that moved
/// without a registration on this thread.
fn a_landed_host_feed_wakes_every_ui_runtime_window() {
    use flui_painting::testing::{PROBE_MONO_100, feed_with_host, host_fonts_from};

    let (dispatcher_a, dispatcher_b) = install_two_test_ui_runtimes();
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
        "the feed's wake asks the loop for the turn that tells the ui_runtimes"
    );
    assert_eq!(
        super::super::host::runtime_font_collection().generation(),
        generation + 1,
        "the feed landed on the app's collection"
    );
    // The owner turn the wake brings; the notices queue behind its event.
    dispatch_platform_ui_runtime(dispatcher_a, RuntimeTask::TestCallback(Box::new(|_| {})))
        .expect("the ui_runtime dispatches");

    for dispatcher in [dispatcher_a, dispatcher_b] {
        assert!(
            redraw_requested(dispatcher),
            "{dispatcher:?} draws its next frame"
        );
    }

    teardown_platform_ui_runtime();
}

/// The same bytes registered twice are refused the second time: the
/// collection does not change and no UI runtime is woken. Fails if a repeated
/// registration adds the face again or lays text out again for nothing.
fn a_duplicate_registration_is_refused_and_notifies_nothing() {
    let dispatcher = install_test_ui_runtime();
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
    assert!(!redraw_requested(dispatcher), "no ui_runtime is woken");

    teardown_platform_ui_runtime();
}

/// Bytes with no face are refused: the collection does not change and no
/// UI runtime is woken. Fails if a refused registration notifies the UI runtimes.
fn bytes_with_no_face_are_refused_and_notify_nothing() {
    let dispatcher = install_test_ui_runtime();
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
    assert!(!redraw_requested(dispatcher), "no ui_runtime is woken");

    teardown_platform_ui_runtime();
}

/// A registration made from inside one UI runtime's task reaches that UI runtime once
/// the task returns, and its sibling as well: the calling UI runtime is checked
/// out while its task runs, so its notice waits in the owner queue. Fails if
/// the notice to the running UI runtime is dropped, or re-enters it.
fn a_registration_from_inside_a_ui_runtime_task_reaches_that_ui_runtime_after_it_returns() {
    let (dispatcher_a, dispatcher_b) = install_two_test_ui_runtimes();
    for dispatcher in [dispatcher_a, dispatcher_b] {
        clear_redraw(dispatcher);
    }

    dispatch_platform_ui_runtime(
        dispatcher_a,
        RuntimeTask::TestCallback(Box::new(|ui_runtime| {
            super::super::register_font(PROBE_SANS).expect("the probe face registers");
            assert!(
                !ui_runtime.needs_redraw(),
                "the notice waits for the running task to return"
            );
        })),
    )
    .expect("ui_runtime A's task runs, then the queued notices");

    for dispatcher in [dispatcher_a, dispatcher_b] {
        assert!(
            redraw_requested(dispatcher),
            "{dispatcher:?} draws its next frame"
        );
    }

    teardown_platform_ui_runtime();
}

fn font_notification_survives_the_primary_window_closing_before_delivery() {
    let primary = install_test_ui_runtime();
    let sibling = install_presentation_alongside(
        primary,
        crate::app::window_test_support::headless_test_window(),
    )
    .expect("shared presentation");
    clear_redraw(primary);
    dispatch_platform_ui_runtime(
        primary,
        RuntimeTask::TestCallback(Box::new(move |_| {
            close_this_window(primary);
            // Remove redraw caused by close itself before the font notice runs.
            dispatch_platform_ui_runtime(
                sibling,
                RuntimeTask::TestCallback(Box::new(
                    crate::app::ui_runtime::UiRuntime::mark_rendered,
                )),
            )
            .expect("clear after close");
            super::super::register_font(include_bytes!(
                "../../../../../flui-painting/assets/fonts/probe-arabic-ligature.ttf"
            ))
            .expect("new face");
        })),
    )
    .expect("close followed by font registration");
    assert!(
        redraw_requested(sibling),
        "surviving runtime receives font invalidation"
    );
    primary
        .runtime()
        .lifecycle(AppLifecycleState::Resumed)
        .expect("runtime lifecycle survives primary");
    dispatch_platform_ui_runtime(
        sibling,
        RuntimeTask::TestCallback(Box::new(|runtime| {
            assert_eq!(
                runtime.scheduler().lifecycle_state(),
                AppLifecycleState::Resumed
            );
        })),
    )
    .expect("inspect lifecycle");
    teardown_platform_ui_runtime();
}

/// A face registered on a thread that runs no app does not reach the app's
/// collection, and wakes no ui_runtime. Fails if the call registers on a
/// collection a window of the app reads.
fn a_registration_on_a_thread_that_runs_no_app_leaves_the_app_alone() {
    let dispatcher = install_test_ui_runtime();
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
    assert!(!redraw_requested(dispatcher), "no ui_runtime is woken");

    teardown_platform_ui_runtime();
}

/// A face registered before a thread builds its first UI runtime is held, and
/// lands when the collection is built: the first window measures, paints
/// and places carets with it. Fails if a registration before the start is
/// lost, or accepts bytes with no face because nothing judges them yet.
fn a_registration_before_the_first_ui_runtime_lands_with_the_collection() {
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

        // What the runner does as it builds the first ui_runtime.
        let fonts = super::super::host::runtime_font_collection();
        assert_eq!(fonts.generation(), 1, "the collection gained the face");
    })
    .join()
    .expect("the registration lands with the collection");
}

#[test]
fn owner_dispatch_matrix() {
    crate::table_test::run_table(
        "owner_dispatch_matrix",
        &[
            (
                "deferred_native_reads_preserve_accepted_preferences",
                deferred_native_reads_preserve_accepted_preferences as fn(),
            ),
            (
                "owner_wake_refreshes_installed_preference_consumers",
                owner_wake_refreshes_installed_preference_consumers as fn(),
            ),
            #[cfg(target_os = "windows")]
            (
                "windows_bootstrap_accepts_preferences_before_the_first_window",
                windows_bootstrap_accepts_preferences_before_the_first_window as fn(),
            ),
            (
                "obsolete_native_observation_cannot_update_a_replacement_host",
                obsolete_native_observation_cannot_update_a_replacement_host as fn(),
            ),
            (
                "platform_replacement_contains_reentrant_retirement",
                platform_replacement_contains_reentrant_retirement as fn(),
            ),
            (
                "a_replacement_platform_starts_a_new_preference_owner",
                a_replacement_platform_starts_a_new_preference_owner as fn(),
            ),
            (
                "bootstrap_keeps_the_host_that_seeded_the_first_build",
                bootstrap_keeps_the_host_that_seeded_the_first_build as fn(),
            ),
            (
                "font_notification_survives_the_primary_window_closing_before_delivery",
                font_notification_survives_the_primary_window_closing_before_delivery as fn(),
            ),
            (
                "native_identity_is_observed_before_registry_publication",
                native_identity_is_observed_before_registry_publication as fn(),
            ),
            (
                "presentation_assembly_reentry_revalidates_its_authorizer",
                presentation_assembly_reentry_revalidates_its_authorizer as fn(),
            ),
            (
                "recovered_surface_notification_resubmits_the_scene",
                recovered_surface_notification_resubmits_the_scene as fn(),
            ),
            (
                "background_owner_pump_drains_before_polling_without_a_frame",
                background_owner_pump_drains_before_polling_without_a_frame as fn(),
            ),
            (
                "explicit_platform_quit_detaches_every_installed_ui_runtime",
                explicit_platform_quit_detaches_every_installed_ui_runtime as fn(),
            ),
            (
                "install_resolves_execution_services_and_teardown_shuts_them_down",
                install_resolves_execution_services_and_teardown_shuts_them_down as fn(),
            ),
            (
                "system_key_default_follows_the_ui_runtimes_decision",
                system_key_default_follows_the_ui_runtimes_decision as fn(),
            ),
            (
                "native_keyboard_cannot_overtake_queued_window_changes_and_keys",
                native_keyboard_cannot_overtake_queued_window_changes_and_keys as fn(),
            ),
            (
                "late_event_never_crosses_ui_runtime_incarnations",
                late_event_never_crosses_ui_runtime_incarnations as fn(),
            ),
            (
                "panic_restores_dispatch_host_for_next_event",
                panic_restores_dispatch_host_for_next_event as fn(),
            ),
            (
                "reentrant_owner_turns_preserve_global_fifo_across_ui_runtimes",
                reentrant_owner_turns_preserve_global_fifo_across_ui_runtimes as fn(),
            ),
            (
                "carried_work_shares_one_callback_budget_across_runtimes",
                carried_work_shares_one_callback_budget_across_runtimes as fn(),
            ),
            (
                "two_ui_runtimes_via_isolated_policy_share_nothing",
                two_ui_runtimes_via_isolated_policy_share_nothing as fn(),
            ),
            (
                "one_ui_runtime_two_windows_policy_routes_by_presentation",
                one_ui_runtime_two_windows_policy_routes_by_presentation as fn(),
            ),
            (
                "resized_rescales_only_the_addressed_presentation",
                resized_rescales_only_the_addressed_presentation as fn(),
            ),
            (
                "resizing_a_secondary_leaves_the_primary_surface_alone",
                resizing_a_secondary_leaves_the_primary_surface_alone as fn(),
            ),
            (
                "closing_the_last_window_reentrantly_from_inside_a_dispatch_still_exits",
                closing_the_last_window_reentrantly_from_inside_a_dispatch_still_exits as fn(),
            ),
            (
                "panicking_stop_notifies_siblings_and_restores_runtime_delivery",
                panicking_stop_notifies_siblings_and_restores_runtime_delivery
                    as fn(),
            ),
            (
                "isolated_windows_shape_over_the_runtimes_font_collection",
                isolated_windows_shape_over_the_runtimes_font_collection as fn(),
            ),
            (
                "a_registration_notifies_every_ui_runtime_window",
                a_registration_notifies_every_ui_runtime_window as fn(),
            ),
            (
                "a_landed_host_feed_wakes_every_ui_runtime_window",
                a_landed_host_feed_wakes_every_ui_runtime_window as fn(),
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
                "a_registration_from_inside_a_ui_runtime_task_reaches_that_ui_runtime_after_it_returns",
                a_registration_from_inside_a_ui_runtime_task_reaches_that_ui_runtime_after_it_returns
                    as fn(),
            ),
            (
                "a_registration_on_a_thread_that_runs_no_app_leaves_the_app_alone",
                a_registration_on_a_thread_that_runs_no_app_leaves_the_app_alone as fn(),
            ),
            (
                "a_registration_before_the_first_ui_runtime_lands_with_the_collection",
                a_registration_before_the_first_ui_runtime_lands_with_the_collection as fn(),
            ),
        ],
    );
}

// ========================================================================
// Close-request veto (issue #558)
// ========================================================================
