//! Unit tests for `realm_dispatch.rs`, declared there as its child module `realm_dispatch_tests`
//! through `#[path]`, so `super::*` still reaches the dispatcher's private items.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use flui_interaction::{
    HitTestResult,
    events::{PointerType, make_down_event},
};
use flui_platform::traits::{PlatformInput, PlatformWindow};
use flui_types::geometry::{Offset, Pixels};
use flui_view::View;

use super::super::host::{
    OwnerHostClearGuard, install_exit_policy_hook, install_owner_platform, with_owner_platform,
};
use super::super::secondary_window::{open_secondary_window, open_secondary_window_impl};
use super::super::{install_close_request_wiring, request_presentation_close};
use super::*;
use crate::app::raster_lane::RealmRaster as _;
use crate::app::raster_test_support::TestRasterBackend;
use crate::app::runtime::{ExitPolicy, WindowPolicy};
use crate::app::{AppConfig, FrameFailureDetail};

static_assertions::assert_impl_all!(PlatformToUi: Send);

fn down_input(offset: f32) -> PlatformInput {
    PlatformInput::Pointer(make_down_event(
        Offset::new(Pixels(offset), Pixels(offset)),
        PointerType::Mouse,
    ))
}

fn test_window() -> std::sync::Arc<dyn flui_platform::traits::PlatformWindow> {
    crate::app::window_test_support::headless_test_window()
}

/// A fresh headless window as a runner hands it to
/// `install_presentation_alongside`: through `presentation_window`, so the
/// headless backend's accessibility bridge is wired as in production.
fn test_presentation_window() -> crate::app::presentation::PresentationWindow {
    super::super::presentation_window(crate::app::window_test_support::headless_test_host_window())
}

fn install_test_realm() -> RealmDispatcher {
    install_platform_realm(crate::app::ui_realm::UiRealm::for_test(), &test_window())
}

#[test]
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

fn with_quit_notification_loop(
    test: impl FnOnce(flui_platform::SharedPlatform, Rc<flui_platform::HeadlessExitReevaluation>)
    + 'static,
) {
    let _clear = OwnerHostClearGuard::arm();
    let platform = flui_platform::HeadlessPlatform::new();
    let reevaluation = Rc::new(platform.exit_reevaluation());
    let platform: Box<dyn flui_platform::Platform> = Box::new(platform);
    platform
        .run(Box::new(move |owner| {
            let shared = owner.shared();
            install_owner_platform(owner).expect("install owner wake transport");
            shared.set_exit_policy_hook(Box::new(|| true));
            super::super::host::install_platform_quit_hook();
            test(shared, reevaluation);
            teardown_platform_realm();
            Ok(())
        }))
        .expect("headless loop");
}

#[test]
fn quit_notification_old_platform_hook_cannot_detach_a_new_loop() {
    let old_handles = Rc::new(RefCell::new(None));
    let saved = Rc::clone(&old_handles);
    with_quit_notification_loop(move |shared, reevaluation| {
        let previous = saved.borrow_mut().replace((shared, reevaluation));
        drop(previous);
    });
    let (old_shared, old_reevaluation) = old_handles
        .borrow_mut()
        .take()
        .expect("retained old platform");
    with_quit_notification_loop(move |shared, reevaluation| {
        let primary = install_test_realm();
        resume_for_quit(primary);
        old_shared.request_exit_policy_reevaluation();
        assert!(old_reevaluation.drive());
        APP_RUNTIME.with(|slot| {
            let state = slot.borrow();
            assert_eq!(
                state.quit_notification,
                crate::app::runtime::QuitNotification::Active
            );
            assert_eq!(
                state
                    .realms
                    .iter()
                    .next()
                    .expect("primary")
                    .1
                    .realm
                    .as_ref()
                    .expect("restored")
                    .scheduler()
                    .lifecycle_state(),
                AppLifecycleState::Resumed
            );
        });
        shared.request_exit_policy_reevaluation();
        assert!(reevaluation.drive());
        assert_quit_notification_restored();
    });
}

#[test]
fn quit_notification_removed_realm_drop_preserves_dispatch_panic_and_notifies_survivor() {
    struct PanickingDrop;
    impl Drop for PanickingDrop {
        fn drop(&mut self) {
            panic!("removed realm listener drop");
        }
    }
    for visit in [false, true] {
        with_quit_notification_loop(move |shared, reevaluation| {
            let primary = install_test_realm();
            let doomed =
                install_realm_alongside(crate::app::ui_realm::UiRealm::for_test(), &test_window())
                    .expect("second removed realm");
            let secondary =
                install_realm_alongside(crate::app::ui_realm::UiRealm::for_test(), &test_window())
                    .expect("survivor");
            resume_for_quit(secondary);
            for dispatcher in [primary, doomed] {
                dispatch_platform_realm(
                    dispatcher,
                    RealmTask::Frame(Box::new(|realm| {
                        let captured = PanickingDrop;
                        realm.focus_manager().add_listener(Rc::new(move |_, _| {
                            let _keep_capture = &captured;
                        }));
                    })),
                )
                .expect("listener registered");
            }
            let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let task = move |_: &crate::app::ui_realm::UiRealm| {
                    uninstall_platform_realm(primary.address.realm_id);
                    uninstall_platform_realm(doomed.address.realm_id);
                    shared.request_exit_policy_reevaluation();
                    assert!(reevaluation.drive());
                    panic!("original dispatch failure");
                };
                if visit {
                    let mut task = Some(task);
                    for_each_installed_realm(|realm| {
                        if let Some(task) = task.take() {
                            task(realm);
                        }
                    });
                } else {
                    dispatch_platform_realm(primary, RealmTask::Frame(Box::new(task)))
                        .expect("dispatch admitted");
                }
            }))
            .expect_err("first panic resumed");
            assert_quit_notification_restored();
            assert_eq!(
                failure.downcast_ref::<&str>(),
                Some(&"original dispatch failure")
            );
        });
    }
}

#[test]
fn window_execution_is_local_reversible_and_cannot_override_host_or_terminal_stop() {
    use flui_platform::WindowExecutionState::{Detached, Running, Suspended};
    with_quit_notification_loop(|_, _| {
        let a = install_test_realm();
        let b = install_presentation_alongside(a, test_presentation_window())
            .expect("shared presentation");
        resume_for_quit(a);
        dispatch_platform_realm(
            a,
            RealmTask::Event(PlatformToUi::WindowExecution(Suspended)),
        )
        .expect("suspend A");
        dispatch_platform_realm(
            b,
            RealmTask::Frame(Box::new(move |realm| {
                assert_eq!(
                    realm.scheduler().lifecycle_state(),
                    AppLifecycleState::Resumed
                );
                assert!(realm.scheduler().frames_enabled());
                assert_eq!(
                    realm
                        .presentation_widgets_for_test(a.address.presentation_id)
                        .lifecycle_source()
                        .current(),
                    Some(AppLifecycleState::Paused)
                );
            })),
        )
        .expect("sibling survives");
        for (execution, expected) in [
            (Detached, AppLifecycleState::Detached),
            (Running, AppLifecycleState::Inactive),
        ] {
            dispatch_platform_realm(
                a,
                RealmTask::Event(PlatformToUi::WindowExecution(execution)),
            )
            .expect("observation");
            dispatch_platform_realm(
                a,
                RealmTask::Frame(Box::new(move |realm| {
                    assert_eq!(
                        realm
                            .presentation_widgets_for_test(a.address.presentation_id)
                            .lifecycle_source()
                            .current(),
                        Some(expected)
                    );
                })),
            )
            .expect("local stream");
        }
        dispatch_platform_realm(
            a,
            RealmTask::Event(PlatformToUi::Lifecycle(AppLifecycleState::Paused)),
        )
        .expect("host pause");
        dispatch_platform_realm(a, RealmTask::Event(PlatformToUi::WindowExecution(Running)))
            .expect("late running");
        dispatch_platform_realm(
            a,
            RealmTask::Frame(Box::new(move |realm| {
                assert_eq!(
                    realm.scheduler().lifecycle_state(),
                    AppLifecycleState::Paused
                );
                assert!(!realm.scheduler().frames_enabled());
                realm.stop_presentation_for_test(a.address.presentation_id);
                realm.update_window_execution(a.address.presentation_id, Running);
                realm.update_window_focus(a.address.presentation_id, true);
                assert_eq!(
                    realm
                        .presentation_widgets_for_test(a.address.presentation_id)
                        .lifecycle_source()
                        .current(),
                    Some(AppLifecycleState::Detached)
                );
            })),
        )
        .expect("host and terminal fences");
    });
}

#[test]
fn window_execution_snapshot_notifies_paused_without_transient_resumed() {
    with_quit_notification_loop(|_, _| {
        let a = install_test_realm();
        dispatch_platform_realm(
            a,
            RealmTask::Frame(Box::new(move |realm| {
                let history = Rc::new(RefCell::new(Vec::new()));
                let observed = Rc::clone(&history);
                let handle = realm
                    .presentation_widgets_for_test(a.address.presentation_id)
                    .lifecycle_source()
                    .handle();
                let (_, _subscription) = handle
                    .subscribe(move |state| observed.borrow_mut().push(state))
                    .expect("subscription");
                realm.synchronize_window_snapshot(
                    a.address.presentation_id,
                    flui_platform::WindowExecutionState::Suspended,
                    true,
                    true,
                );
                assert_eq!(
                    handle.snapshot().expect("live"),
                    Some(AppLifecycleState::Paused)
                );
                assert!(!realm.scheduler().frames_enabled());
                assert!(!history.borrow().contains(&AppLifecycleState::Resumed));
                realm.update_host_lifecycle(AppLifecycleState::Inactive);
                realm.update_window_focus(a.address.presentation_id, true);
                assert_eq!(
                    handle.snapshot().expect("live"),
                    Some(AppLifecycleState::Paused)
                );
            })),
        )
        .expect("initial suspended snapshot");
    });
}

#[test]
fn window_lifecycle_separate_realms_do_not_share_visibility_facts() {
    with_quit_notification_loop(|_, _| {
        let a = install_test_realm();
        let b = install_realm_alongside(crate::app::ui_realm::UiRealm::for_test(), &test_window())
            .expect("second realm");
        for dispatcher in [a, b] {
            resume_for_quit(dispatcher);
        }
        dispatch_platform_realm(a, RealmTask::Event(PlatformToUi::WindowVisibility(false)))
            .expect("hide A");
        dispatch_platform_realm(b, RealmTask::Event(PlatformToUi::WindowFocus(false)))
            .expect("blur B");
        dispatch_platform_realm(
            b,
            RealmTask::Frame(Box::new(|realm| {
                assert_eq!(
                    realm.scheduler().lifecycle_state(),
                    AppLifecycleState::Inactive,
                    "visible realm B must become inactive independently of hidden realm A"
                );
                assert!(realm.scheduler().frames_enabled());
            })),
        )
        .expect("inspect B");
    });
}

#[test]
fn window_lifecycle_shared_realm_visible_sibling_keeps_frames_enabled() {
    with_quit_notification_loop(|_, _| {
        let a = install_test_realm();
        let b = install_presentation_alongside(a, test_presentation_window())
            .expect("shared presentation");
        resume_for_quit(a);
        dispatch_platform_realm(b, RealmTask::Event(PlatformToUi::WindowFocus(true)))
            .expect("focus B");
        dispatch_platform_realm(a, RealmTask::Event(PlatformToUi::WindowVisibility(false)))
            .expect("hide A");
        dispatch_platform_realm(
            b,
            RealmTask::Frame(Box::new(move |realm| {
                assert_eq!(
                    realm.scheduler().lifecycle_state(),
                    AppLifecycleState::Resumed,
                    "visible focused B must keep its shared scheduler running"
                );
                assert!(realm.scheduler().frames_enabled());
                assert_eq!(
                    realm.presentation_hidden_for_test(a.address.presentation_id),
                    Some(true)
                );
                assert_eq!(
                    realm.presentation_hidden_for_test(b.address.presentation_id),
                    Some(false)
                );
            })),
        )
        .expect("inspect shared realm");
    });
}

#[test]
fn window_lifecycle_shared_install_preserves_host_suspension() {
    with_quit_notification_loop(|_, _| {
        let primary = install_test_realm();
        resume_for_quit(primary);
        dispatch_platform_realm(
            primary,
            RealmTask::Event(PlatformToUi::Lifecycle(AppLifecycleState::Paused)),
        )
        .expect("pause");
        let (secondary, _window) =
            open_secondary_window_impl(AppConfig::default(), WindowPolicy::SharedRealm)
                .expect("open")
                .expect("headless ready");
        assert_eq!(primary.address.realm_id, secondary.address.realm_id);
        dispatch_platform_realm(
            secondary,
            RealmTask::Frame(Box::new(|realm| {
                assert_eq!(realm.presentation_count(), 2);
                assert_eq!(
                    realm.scheduler().lifecycle_state(),
                    AppLifecycleState::Paused
                );
                assert!(!realm.scheduler().frames_enabled());
            })),
        )
        .expect("inspect");
    });
}

#[test]
fn window_lifecycle_close_unregisters_after_terminal_observer_panics() {
    struct TerminalPanic(RealmDispatcher, Arc<std::sync::atomic::AtomicUsize>);
    impl flui_view::WidgetsBindingObserver for TerminalPanic {
        fn did_change_app_lifecycle_state(&self, state: AppLifecycleState) {
            if state == AppLifecycleState::Detached {
                assert!(
                    dispatch_platform_realm(
                        self.0,
                        RealmTask::Event(PlatformToUi::Input(down_input(1.0)),)
                    )
                    .is_err(),
                    "terminal callback cannot readmit addressed input"
                );
                self.1.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                assert!(close_presentation(self.0, self.0.address.presentation_id).is_err());
                panic!("terminal observer");
            }
        }
    }
    for shared in [false, true] {
        with_quit_notification_loop(move |_, _| {
            let a = install_test_realm();
            let closing = if shared {
                install_presentation_alongside(a, test_presentation_window()).expect("B")
            } else {
                a
            };
            resume_for_quit(a);
            let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let observed_calls = Arc::clone(&calls);
            dispatch_platform_realm(
                closing,
                RealmTask::Frame(Box::new(move |realm| {
                    realm
                        .presentation_widgets_for_test(closing.address.presentation_id)
                        .add_observer(Arc::new(TerminalPanic(closing, observed_calls)));
                })),
            )
            .expect("observer");
            let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                close_presentation(closing, closing.address.presentation_id)
                    .expect("close admitted");
            }))
            .expect_err("terminal observer panic preserved");
            assert_eq!(failure.downcast_ref::<&str>(), Some(&"terminal observer"));
            assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
            APP_RUNTIME.with(|slot| {
                let state = slot.borrow();
                assert!(!state.registry.contains_address(closing.address));
                assert_eq!(state.realms.iter().count(), usize::from(shared));
                if shared {
                    assert_eq!(
                        state
                            .realms
                            .iter()
                            .next()
                            .expect("survivor")
                            .1
                            .realm
                            .as_ref()
                            .expect("restored")
                            .presentation_count(),
                        1
                    );
                }
            });
        });
    }
}

fn resume_for_quit(dispatcher: RealmDispatcher) {
    dispatch_platform_realm(
        dispatcher,
        RealmTask::Event(PlatformToUi::Lifecycle(AppLifecycleState::Resumed)),
    )
    .expect("resume realm");
}

fn assert_quit_notification_restored() {
    APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        assert_eq!(
            state.quit_notification,
            crate::app::runtime::QuitNotification::Notified
        );
        assert!(state.dispatched_realm_id.is_none());
        assert!(!state.iterating_all_realms);
        for (_, installed) in state.realms.iter() {
            let realm = installed.realm.as_ref().expect("restored realm");
            assert_eq!(
                realm.scheduler().lifecycle_state(),
                AppLifecycleState::Detached
            );
            assert!(!realm.scheduler().frames_enabled());
        }
    });
}

#[test]
fn quit_notification_survives_primary_removal_and_visits_shared_realm_once() {
    with_quit_notification_loop(|shared, reevaluation| {
        let primary = install_test_realm();
        let secondary =
            install_realm_alongside(crate::app::ui_realm::UiRealm::for_test(), &test_window())
                .expect("secondary");
        let _other_presentation =
            install_presentation_alongside(secondary, test_presentation_window())
                .expect("shared realm presentation");
        resume_for_quit(secondary);
        let detached = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = Arc::clone(&detached);
        dispatch_platform_realm(
            secondary,
            RealmTask::Frame(Box::new(move |realm| {
                realm
                    .scheduler()
                    .add_lifecycle_state_listener(Arc::new(move |state| {
                        if state == AppLifecycleState::Detached {
                            observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        }
                    }));
            })),
        )
        .expect("listener installed");
        uninstall_platform_realm(primary.address.realm_id);
        shared.request_exit_policy_reevaluation();
        assert!(reevaluation.drive());
        assert_eq!(detached.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_quit_notification_restored();
    });
}

#[test]
fn quit_notification_preserves_dispatch_panic_and_finishes_siblings_after_observer_panic() {
    with_quit_notification_loop(|shared, reevaluation| {
        let primary = install_test_realm();
        let secondary =
            install_realm_alongside(crate::app::ui_realm::UiRealm::for_test(), &test_window())
                .expect("secondary");
        for dispatcher in [primary, secondary] {
            resume_for_quit(dispatcher);
        }
        dispatch_platform_realm(
            primary,
            RealmTask::Frame(Box::new(|realm| {
                realm
                    .scheduler()
                    .add_lifecycle_state_listener(Arc::new(|state| {
                        assert_ne!(state, AppLifecycleState::Detached, "observer panic");
                    }));
            })),
        )
        .expect("listener installed");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            dispatch_platform_realm(
                primary,
                RealmTask::Frame(Box::new(move |_| {
                    shared.request_exit_policy_reevaluation();
                    assert!(reevaluation.drive());
                    APP_RUNTIME.with(|slot| {
                        assert_eq!(
                            slot.borrow().quit_notification,
                            crate::app::runtime::QuitNotification::Requested
                        );
                    });
                    panic!("original dispatch panic");
                })),
            )
            .expect("dispatch admitted");
        }));
        let payload = result.expect_err("original panic resumes after restoration");
        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&"original dispatch panic")
        );
        assert_quit_notification_restored();
    });
}

#[test]
fn quit_notification_includes_install_accepted_before_request_and_fences_new_opens() {
    with_quit_notification_loop(|shared, reevaluation| {
        let primary = install_test_realm();
        resume_for_quit(primary);
        let accepted = Rc::new(Cell::new(None));
        let installed = Rc::clone(&accepted);
        dispatch_platform_realm(
            primary,
            RealmTask::Frame(Box::new(move |_| {
                let new_realm = crate::app::ui_realm::UiRealm::for_test();
                new_realm.enter(|realm| {
                    realm.update_host_lifecycle(AppLifecycleState::Resumed);
                });
                let secondary = install_realm_alongside(new_realm, &test_window())
                    .expect("pre-request deferred install accepted");
                installed.set(Some(secondary.address.realm_id));
                shared.request_exit_policy_reevaluation();
                assert!(reevaluation.drive());
                let refused =
                    open_secondary_window(AppConfig::default(), WindowPolicy::SeparateRealms);
                assert!(
                    refused.is_err(),
                    "quit fences native window creation immediately"
                );
            })),
        )
        .expect("dispatch completes");
        APP_RUNTIME.with(|slot| {
            assert!(
                slot.borrow()
                    .realms
                    .contains_key(&accepted.get().expect("accepted realm"))
            );
            assert_eq!(slot.borrow().realms.iter().count(), 2);
        });
        assert_quit_notification_restored();
    });
}

#[test]
fn quit_notification_observer_reentry_is_once_and_observer_panic_does_not_skip_siblings() {
    with_quit_notification_loop(|shared, reevaluation| {
        let primary = install_test_realm();
        let secondary =
            install_realm_alongside(crate::app::ui_realm::UiRealm::for_test(), &test_window())
                .expect("secondary");
        for dispatcher in [primary, secondary] {
            resume_for_quit(dispatcher);
        }
        let observed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed_in_listener = Arc::clone(&observed);
        dispatch_platform_realm(
            primary,
            RealmTask::Frame(Box::new(move |realm| {
                realm
                    .scheduler()
                    .add_lifecycle_state_listener(Arc::new(move |state| {
                        if state == AppLifecycleState::Detached {
                            observed_in_listener.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            request_quit_notification();
                            panic!("first observer panic");
                        }
                    }));
            })),
        )
        .expect("listener installed");
        shared.request_exit_policy_reevaluation();
        let payload =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| reevaluation.drive()))
                .expect_err("observer panic preserved");
        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&"first observer panic")
        );
        assert_eq!(observed.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_quit_notification_restored();
        request_quit_notification();
        assert_eq!(observed.load(std::sync::atomic::Ordering::SeqCst), 1);
    });
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
#[test]
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

/// The teardown STAGING between the two shutdown families (issue
/// #558): services get their cooperative flush window BEFORE the
/// execution pools close. The probe service only writes its flush flag
/// AFTER observing ITS OWN cancellation — exactly what a real service
/// does with final state.
///
/// If reverted: move `shutdown_lifecycles` after `shutdown_execution`
/// in `teardown_platform_realm` and this fails — the pools' root-token
/// cancellation hard-drops the service future at its await point
/// before it can flush, so the flag stays false. Removing the
/// registry's cancel stage fails it too (the service never wakes; the
/// join times out with the flag unset).
#[test]
fn teardown_gives_services_their_flush_window_before_the_pools_close() {
    use crate::app::lifecycle::{ServiceDefinition, ServiceLifetime};

    install_test_realm();
    let flushed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flushed_in_service = std::sync::Arc::clone(&flushed);
    APP_RUNTIME.with(|slot| {
        slot.borrow_mut()
            .start_service(&ServiceDefinition::new(
                "flush-probe",
                ServiceLifetime::StopsWithLastWindow,
                move |context| {
                    let signal = context.cancellation().clone();
                    let flushed = std::sync::Arc::clone(&flushed_in_service);
                    Box::pin(async move {
                        signal.cancelled().await;
                        flushed.store(true, std::sync::atomic::Ordering::Release);
                    })
                },
            ))
            .expect("service must start on the loop's real IO pool");
    });

    teardown_platform_realm();
    assert!(
        flushed.load(std::sync::atomic::Ordering::Acquire),
        "the service must observe cancellation and flush BEFORE the pools close; \
         a false flag means the pool shutdown dropped the future unflushed"
    );

    // Post-teardown admission is closed end to end: a late service
    // start is refused instead of spawning work nothing will join.
    APP_RUNTIME.with(|slot| {
        let result = slot.borrow_mut().start_service(&ServiceDefinition::new(
            "late",
            ServiceLifetime::StopsWithLastWindow,
            |_context| Box::pin(async {}),
        ));
        assert!(
            result.is_err(),
            "a service start after loop-exit teardown must be refused"
        );
    });
}

/// A one-shot gate a test service parks on until the test releases it.
///
/// The flag and the parked waker live under one lock so that "check the
/// flag, park the waker" on the service side and "set the flag, take the
/// waker" on the test side cannot interleave: whichever runs second sees
/// the other's write, so no wakeup is lost.
#[derive(Default)]
struct ReleaseGate {
    released: bool,
    parked: Option<std::task::Waker>,
}

impl ReleaseGate {
    fn poll(&mut self, context: &std::task::Context<'_>) -> std::task::Poll<()> {
        if self.released {
            std::task::Poll::Ready(())
        } else {
            self.parked = Some(context.waker().clone());
            std::task::Poll::Pending
        }
    }

    /// Opens the gate and hands back the waker to wake, if the service
    /// has parked; the caller wakes it after releasing the lock.
    fn release(&mut self) -> Option<std::task::Waker> {
        self.released = true;
        self.parked.take()
    }
}

/// The messenger scenario end to end (issue #558): the last window's
/// close is VETOED by a running keep-alive service — and when that
/// service later completes on a worker-pool thread, its completion
/// must re-open the exit question and end the loop, with no window
/// left to produce any event. Drives the full production chain: the
/// real exit-policy hook (vetoing through `AppRuntime::should_exit`),
/// the registry's keep-alive completion notifier, the platform's
/// parked re-evaluation request, and the owner-thread re-check that
/// finally quits.
///
/// If reverted: remove the notifier fire from `ServiceRegistry::start`'s
/// completion wrapper (or the notifier installation from
/// `install_exit_policy_hook`) and the bounded wait for the parked
/// request fails — the veto is permanent and the app would linger
/// forever, which is exactly the production bug this pins.
#[test]
fn keep_alive_completion_reopens_the_exit_question_after_the_last_window_closed() {
    let _clear_guard = OwnerHostClearGuard::arm();
    let platform = flui_platform::HeadlessPlatform::new();
    let reevaluation = platform.exit_reevaluation();
    let platform: Box<dyn flui_platform::Platform> = Box::new(platform);

    let quit_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let quit_calls_for_on_ready = Arc::clone(&quit_calls);
    type Installed = (
        RealmDispatcher,
        std::sync::Arc<dyn flui_platform::traits::PlatformWindow>,
    );
    let installed_slot: Rc<RefCell<Option<Installed>>> = Rc::new(RefCell::new(None));
    let installed_slot_for_on_ready = Rc::clone(&installed_slot);
    platform
        .run(Box::new(move |owner| {
            install_owner_platform(owner).expect("install owner wake transport");
            // Installs BOTH halves of the production wiring: the
            // exit-policy hook and the keep-alive completion notifier.
            install_exit_policy_hook(ExitPolicy::OnLastWindowClosed);

            let quit_calls_for_handler = Arc::clone(&quit_calls_for_on_ready);
            with_owner_platform(|owner| {
                owner.shared().on_quit(Box::new(move || {
                    quit_calls_for_handler.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }));
            });

            let window: Arc<dyn PlatformWindow> = with_owner_platform(|owner| {
                owner.open_window(flui_platform::WindowOptions::default())
            })
            .expect("owner installed above")
            .and_then(flui_platform::WindowOpen::try_ready)
            .expect("headless open_window is always Ready");
            let dispatcher =
                install_platform_realm(crate::app::ui_realm::UiRealm::for_test(), &window);
            window.on_close(Box::new(move || {
                close_this_window(dispatcher);
            }));
            let _prev = installed_slot_for_on_ready
                .borrow_mut()
                .replace((dispatcher, window));
            Ok(())
        }))
        .expect("headless run must not fail");
    let (_dispatcher, window) = installed_slot
        .borrow_mut()
        .take()
        .expect("set inside on_ready above");

    // The messenger's background service: parked on the REAL IO pool
    // until the test releases it, then completes. The release flag and the
    // parked waker share one lock: checked and parked apart, the service
    // could read "not released", the test then release and find no waker,
    // and the service park a waker nothing ever wakes.
    let gate = Arc::new(parking_lot::Mutex::new(ReleaseGate::default()));
    {
        use crate::app::lifecycle::{ServiceDefinition, ServiceLifetime};
        let gate_in_service = Arc::clone(&gate);
        APP_RUNTIME.with(|slot| {
            slot.borrow_mut()
                .start_service(&ServiceDefinition::new(
                    "messenger-sync",
                    ServiceLifetime::KeepsAppAlive,
                    move |_context| {
                        let gate = Arc::clone(&gate_in_service);
                        Box::pin(async move {
                            std::future::poll_fn(move |context| gate.lock().poll(context)).await;
                        })
                    },
                ))
                .expect("service must start on the loop's real IO pool");
        });
    }

    // Last window closes: the keep-alive service vetoes the exit.
    window.close();
    assert_eq!(
        quit_calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "a running keep-alive service must veto exit at the last window's close"
    );

    // Consume the request the last window's close already parked. Closing
    // it removed the realm, and `dispatch_platform_realm`'s tail asks the
    // platform to re-consult whenever a deferred realm-map mutation
    // applies — correctly, and the answer here is the veto just asserted.
    // Draining it now is what lets the wait below observe the SERVICE's
    // own request rather than this one: `requested()` is a boolean and
    // cannot tell two requests apart.
    assert!(
        !reevaluation.drive(),
        "the close's own re-ask must still be vetoed: the keep-alive \
         service is running"
    );
    assert!(
        !reevaluation.requested(),
        "and it must be consumed, so the wait below cannot pass on it"
    );

    // The service completes on its worker thread; its completion must
    // request the platform's exit re-evaluation. Bounded wait: this is
    // the only path that can ever end this app now.
    let parked = gate.lock().release();
    if let Some(waker) = parked {
        waker.wake();
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !reevaluation.requested() {
        assert!(
            std::time::Instant::now() < deadline,
            "the keep-alive service's completion must request an exit re-evaluation \
             within 10s -- without it the veto is permanent and the app lingers forever"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }

    // The owner-thread half (winit: the process_control arm; here: the
    // headless drive): re-consult the hook — realms empty, no running
    // keep-alive service left — and exit.
    assert!(
        reevaluation.drive(),
        "the re-check must pass once the vetoing service has completed"
    );
    assert_eq!(
        quit_calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "the re-check must end the loop through the platform's quit path"
    );

    teardown_platform_realm();
}

#[test]
fn install_platform_realm_does_not_inherit_window_facts_from_a_prior_realm() {
    let old = install_test_realm();
    dispatch_platform_realm(old, RealmTask::Event(PlatformToUi::WindowVisibility(false)))
        .expect("hide old");
    teardown_platform_realm();
    let fresh = install_test_realm();
    resume_for_quit(fresh);
    dispatch_platform_realm(
        fresh,
        RealmTask::Frame(Box::new(|realm| {
            assert_eq!(
                realm.scheduler().lifecycle_state(),
                AppLifecycleState::Resumed
            );
        })),
    )
    .expect("fresh native facts");
    teardown_platform_realm();
}

/// Panic-recovery reinstall: `install_platform_realm` called again while
/// a realm from a prior incarnation is still installed (a mid-`on_ready`
/// failure that never reached `teardown_platform_realm`). The displaced
/// realm's queue must never leak stale-incarnation events into the new
/// realm, the displaced realm/queue/applier must drop only after the TLS
/// borrow releases (a reentrant dispatch triggered by that drop must see
/// a clean, already-released borrow — never a double-borrow panic), and
/// `draining` must not be left stuck from whatever state the displaced
/// realm was in.
///
/// If reverted: without `mem::take`-ing the queue before overwriting
/// `state.realm`, the pre-existing stale event is delivered to the NEW
/// realm FIFO-first and this fails on the `delivered` assertion.
#[test]
fn reinstall_without_teardown_drops_the_displaced_realm_and_queue_outside_the_borrow() {
    struct ReenterOnDrop {
        dispatcher: RealmDispatcher,
        result: Rc<RefCell<Option<Result<(), RealmDispatchError>>>>,
    }

    impl Drop for ReenterOnDrop {
        fn drop(&mut self) {
            let result =
                dispatch_platform_realm(self.dispatcher, RealmTask::Frame(Box::new(|_| {})));
            *self.result.borrow_mut() = Some(result);
        }
    }

    let dispatcher_a = install_test_realm();
    let reentry_result = Rc::new(RefCell::new(None));
    let reentry_result_in_probe = Rc::clone(&reentry_result);
    let probe = ReenterOnDrop {
        dispatcher: dispatcher_a,
        result: reentry_result_in_probe,
    };

    let delivered = Rc::new(RefCell::new(false));
    let delivered_in_event = Rc::clone(&delivered);

    // Enqueue directly (not through `dispatch_platform_realm`, which
    // would drain immediately) — this is the stale-incarnation queue
    // state a mid-`on_ready` panic can leave behind before bootstrap
    // retries `install_platform_realm` without ever calling
    // `teardown_platform_realm`. Also force `draining = true`, matching
    // a realm that was mid-drain when the panic hit.
    APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        let realm_slot = state
            .realms
            .get_mut(&dispatcher_a.address.realm_id)
            .expect("dispatcher_a's realm is installed above");
        let stamp = dispatcher_a.address.presentation_id;
        realm_slot.queue.push_back((
            stamp,
            RealmTask::Frame(Box::new(move |_| {
                *delivered_in_event.borrow_mut() = true;
            })),
        ));
        realm_slot
            .queue
            .push_back((stamp, RealmTask::Frame(Box::new(move |_| drop(probe)))));
        realm_slot.draining = true;
    });

    // The panic-recovery reinstall itself: no teardown in between.
    let dispatcher_b = install_test_realm();

    assert!(
        !*delivered.borrow(),
        "a queued event from the displaced incarnation must never reach the new realm"
    );
    assert_eq!(
        *reentry_result.borrow(),
        Some(Err(RealmDispatchError::StaleRealm)),
        "the displaced realm/queue must drop only after install_platform_realm's TLS \
         borrow releases — a drop still inside that borrow would panic this reentrant \
         dispatch with a double-borrow instead of returning a clean Err"
    );

    let new_realm_ran = Rc::new(RefCell::new(false));
    let new_realm_ran_in_event = Rc::clone(&new_realm_ran);
    dispatch_platform_realm(
        dispatcher_b,
        RealmTask::Frame(Box::new(move |_| {
            *new_realm_ran_in_event.borrow_mut() = true;
        })),
    )
    .expect("the new realm dispatches normally");
    assert!(
        *new_realm_ran.borrow(),
        "draining must not be left stuck from the displaced incarnation — the new \
         realm must actually drain, not just enqueue forever"
    );
    teardown_platform_realm();
}

/// `install_platform_realm` only ever touches the realm-facing fields
/// (`realm`, `queue`, `owner_thread`, `address`, `surface_applier`,
/// `visible`, `focused`) — `owner_platform` is a separate, loop-scoped
/// concern that must survive both branches: a fresh install, and a
/// replace-without-teardown reinstall that displaces a realm never torn
/// down (the same panic-recovery path
/// `reinstall_without_teardown_drops_the_displaced_realm_and_queue_outside_the_borrow`
/// exercises above).
#[test]
fn install_platform_realm_never_touches_owner_platform() {
    use flui_platform::headless_platform;

    let _clear_guard = OwnerHostClearGuard::arm();
    let platform = headless_platform();
    let result = platform.run(Box::new(|owner| {
        install_owner_platform(owner).expect("install owner wake transport");
        assert!(
            with_owner_platform(|_| ()).is_some(),
            "owner_platform must be installed before the first realm install"
        );

        // Fresh-install branch.
        install_test_realm();
        assert!(
            with_owner_platform(|_| ()).is_some(),
            "a fresh realm install must not clear owner_platform"
        );

        // Replace-without-teardown branch: install a second realm while
        // the first is still live, without an intervening
        // `teardown_platform_realm` (the panic-recovery path).
        install_test_realm();
        assert!(
            with_owner_platform(|_| ()).is_some(),
            "a replace-without-teardown reinstall must not clear owner_platform"
        );

        teardown_platform_realm();
        Ok(())
    }));
    assert!(result.is_ok(), "on_ready returns Ok here");
}

/// A panic-recovery reinstall under a DIFFERENT native window must
/// remove the displaced realm's own window mapping from the registry —
/// not just leave it behind alongside the new one.
///
/// Both windows are opened from the *same* headless platform instance:
/// `HeadlessPlatform` mints window ids from its own instance-local
/// counter, so two windows from two separate `headless_platform()` calls
/// would alias the same id instead of differing — one instance, two
/// `open_window` calls, is what actually produces two distinct windows.
///
/// If reverted: skip the registry cleanup for the displaced realm in
/// `install_platform_realm` and this fails — the first window still
/// resolves, and the registry holds two entries instead of one.
#[test]
fn reinstall_with_a_different_window_removes_the_old_windows_registry_mapping() {
    let platform = flui_platform::headless_platform();
    let first_window: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create the first test window");
    let second_window: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create the second test window");
    let first_window_id = first_window.id();
    let second_window_id = second_window.id();
    assert_ne!(
        first_window_id, second_window_id,
        "the two windows must have distinct ids for this test to mean anything"
    );

    let _first_dispatcher =
        install_platform_realm(crate::app::ui_realm::UiRealm::for_test(), &first_window);
    // The panic-recovery reinstall itself, under the second window: no
    // teardown in between.
    let _second_dispatcher =
        install_platform_realm(crate::app::ui_realm::UiRealm::for_test(), &second_window);

    APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        assert_eq!(
            state.registry.resolve(first_window_id),
            None,
            "the displaced realm's old window mapping must be removed on reinstall"
        );
        assert!(
            state.registry.resolve(second_window_id).is_some(),
            "the new window must resolve to the new realm's address"
        );
        assert_eq!(
            state.registry.len(),
            1,
            "only the new window's mapping may remain after the reinstall"
        );
    });
    teardown_platform_realm();
}

#[test]
fn detached_realm_event_cancels_an_interrupted_pointer_sequence() {
    let dispatcher = install_test_realm();
    dispatch_platform_realm(
        dispatcher,
        RealmTask::Event(PlatformToUi::Lifecycle(AppLifecycleState::Resumed)),
    )
    .expect("test realm resumes");
    dispatch_platform_realm(
        dispatcher,
        RealmTask::Frame(Box::new(|realm| {
            let down = make_down_event(Offset::new(Pixels(4.0), Pixels(6.0)), PointerType::Touch);
            realm
                .gestures()
                .handle_pointer_event(&down, |_| HitTestResult::new());
            assert_eq!(realm.gestures().active_pointer_count(), 1);
        })),
    )
    .expect("pointer sequence starts");

    dispatch_platform_realm(
        dispatcher,
        RealmTask::Event(PlatformToUi::Lifecycle(AppLifecycleState::Detached)),
    )
    .expect("Detached lifecycle dispatches through the realm");
    dispatch_platform_realm(
        dispatcher,
        RealmTask::Frame(Box::new(|realm| {
            assert_eq!(realm.gestures().active_pointer_count(), 0);
            assert_eq!(realm.gestures().active_resampler_count(), 0);
            assert_eq!(realm.gestures().pending_move_count(), 0);
            assert!(realm.gestures().arena().is_empty());
        })),
    )
    .expect("clean state remains observable");

    dispatch_platform_realm(
        dispatcher,
        RealmTask::Event(PlatformToUi::Lifecycle(AppLifecycleState::Resumed)),
    )
    .expect("test realm lifecycle restores");
    teardown_platform_realm();
}

#[test]
fn reentrant_frame_event_is_queued_fifo() {
    let dispatcher = install_test_realm();
    let order = Rc::new(RefCell::new(Vec::new()));
    let outer = Rc::clone(&order);
    dispatch_platform_realm(
        dispatcher,
        RealmTask::Frame(Box::new(move |_| {
            outer.borrow_mut().push(1);
            let nested = Rc::clone(&outer);
            dispatch_platform_realm(
                dispatcher,
                RealmTask::Frame(Box::new(move |_| {
                    nested.borrow_mut().push(3);
                })),
            )
            .expect("nested event queues");
            outer.borrow_mut().push(2);
        })),
    )
    .expect("outer event dispatches");
    assert_eq!(*order.borrow(), vec![1, 2, 3]);
}

#[test]
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

/// The common path (ADR-0037 compare order, realm first): teardown +
/// reinstall mints both a fresh `RealmId` and a fresh `PresentationId`
/// from the same shared counter, so a stale dispatcher's realm half
/// never matches — the presentation half is never even compared.
///
/// If reverted: remove the realm-id compare from `dispatch_platform_realm`
/// and this fails — the stale dispatcher's input reaches the new realm.
#[test]
fn stale_realm_dispatch_is_dropped() {
    let stale = install_test_realm();
    teardown_platform_realm();
    let _current = install_test_realm();

    assert_eq!(
        dispatch_platform_realm(
            stale,
            RealmTask::Event(PlatformToUi::Input(down_input(1.0))),
        ),
        Err(RealmDispatchError::StaleRealm)
    );
    teardown_platform_realm();
}

/// The design-for-N path (reachable today only via a forged/mixed
/// address, since realm/presentation generations always advance
/// together): a dispatcher whose realm half matches the live realm but
/// whose presentation half names a different incarnation must be
/// dropped as `StalePresentation`, and the live realm's own gesture
/// state must be completely untouched by the attempt.
///
/// If reverted: remove the presentation-id compare from
/// `dispatch_platform_realm` and this fails — the forged dispatcher's
/// input reaches the live realm's arena.
#[test]
fn stale_presentation_with_live_realm_is_dropped() {
    let live = install_test_realm();
    let live_generation = live.address.presentation_id.generation();
    let forged_generation = std::num::NonZeroU32::new(live_generation.get() + 1)
        .expect("live_generation + 1 is nonzero");
    let forged = RealmDispatcher {
        owner_thread: live.owner_thread,
        address: flui_foundation::PresentationAddress {
            realm_id: live.address.realm_id,
            presentation_id: flui_foundation::PresentationId::new_gen(0, forged_generation),
        },
    };

    assert_eq!(
        dispatch_platform_realm(
            forged,
            RealmTask::Event(PlatformToUi::Input(down_input(1.0))),
        ),
        Err(RealmDispatchError::StalePresentation)
    );

    dispatch_platform_realm(
        live,
        RealmTask::Frame(Box::new(|realm| {
            assert_eq!(
                realm.gestures().active_pointer_count(),
                0,
                "the live realm's gesture state must be untouched by a dropped forged dispatch"
            );
        })),
    )
    .expect("the live dispatcher still dispatches");
    teardown_platform_realm();
}

/// The AC-named teardown test: events queued before teardown for a
/// removed presentation must never deliver, and the surface applier
/// (cleared at the same teardown point) must not fire for a queued
/// resize either.
///
/// If reverted: have `teardown_platform_realm` run the queue instead of
/// dropping it, and both assertions below fail.
#[test]
fn queued_events_for_a_removed_presentation_never_deliver() {
    let dispatcher = install_test_realm();
    let delivered = Rc::new(RefCell::new(false));
    let delivered_in_event = Rc::clone(&delivered);
    let applier_invoked = Rc::new(RefCell::new(false));
    let applier_invoked_in_closure = Rc::clone(&applier_invoked);
    install_surface_applier(dispatcher.address.realm_id, move |_size, _scale_factor| {
        *applier_invoked_in_closure.borrow_mut() = true;
    });

    APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        let realm_slot = state
            .realms
            .get_mut(&dispatcher.address.realm_id)
            .expect("just installed above");
        let stamp = dispatcher.address.presentation_id;
        realm_slot.queue.push_back((
            stamp,
            RealmTask::Frame(Box::new(move |_| {
                *delivered_in_event.borrow_mut() = true;
            })),
        ));
        realm_slot.queue.push_back((
            stamp,
            RealmTask::Event(PlatformToUi::Resized {
                size: flui_types::Size::new(
                    flui_types::geometry::px(100.0),
                    flui_types::geometry::px(100.0),
                ),
                scale_factor: 1.0,
            }),
        ));
    });

    teardown_platform_realm();
    let _new_realm = install_test_realm();

    assert!(
        !*delivered.borrow(),
        "a queued event for a removed presentation must never deliver"
    );
    assert!(
        !*applier_invoked.borrow(),
        "the surface applier must not fire for a queued resize after teardown"
    );
    teardown_platform_realm();
}

/// The FIFO inversion every addressed media-query write has to survive: a
/// close for a sibling presentation and an event ADDRESSED to that same
/// presentation are both admitted at enqueue time (the registry still
/// holds it for each), so both reach the shared queue and are delivered in
/// order — the close removes the presentation first, and the report then
/// arrives for one this realm no longer hosts.
///
/// The realm-wide half of the event must still run, and the drain must
/// finish: an addressed write that panicked on the missing presentation
/// would abort the rest of that realm's queue, not just its own arm.
///
/// If reverted: look the presentation up infallibly in the safe-area arm
/// (`expect`) and this panics out of the dispatch instead of running the
/// task queued behind it.
#[test]
fn queued_safe_area_for_a_closed_sibling_presentation_is_dropped() {
    let platform = flui_platform::headless_platform();
    let window_a: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create window a");
    let window_b: Arc<dyn flui_platform::traits::PlatformWindow> = Arc::new(
        crate::app::window_test_support::TestWindow::new()
            .with_id(2)
            .focused(false),
    );

    let dispatcher_a = install_platform_realm(crate::app::ui_realm::UiRealm::for_test(), &window_a);
    let dispatcher_b = install_presentation_alongside(dispatcher_a, &window_b)
        .expect("B installs alongside A with a real window mapping");
    let b_id = dispatcher_b.address.presentation_id;

    // Queue the close and the report BEHIND it from inside one dispatched
    // task — the realm is mid-drain, so both go through the REAL admission
    // path (the registry still holds B for each) and land in the shared
    // queue in that order. The marker dispatched last proves the drain
    // reached the end of the queue.
    let drained = Rc::new(RefCell::new(false));
    let drained_in_task = Rc::clone(&drained);
    dispatch_platform_realm(
        dispatcher_a,
        RealmTask::Frame(Box::new(move |_| {
            dispatch_platform_realm(dispatcher_b, RealmTask::ClosePresentation(b_id))
                .expect("B's close is admitted while B is still registered");
            dispatch_platform_realm(
                dispatcher_b,
                RealmTask::Event(PlatformToUi::SafeAreaChanged(
                    flui_types::geometry::EdgeInsets::new(
                        flui_types::geometry::px(47.0),
                        flui_types::geometry::px(0.0),
                        flui_types::geometry::px(34.0),
                        flui_types::geometry::px(0.0),
                    ),
                )),
            )
            .expect("the report is still admitted: B's address outlives it until the close runs");
            dispatch_platform_realm(
                dispatcher_a,
                RealmTask::Frame(Box::new(move |_| {
                    *drained_in_task.borrow_mut() = true;
                })),
            )
            .expect("the drain marker is admitted");
        })),
    )
    .expect("the queued close, addressed report and drain marker all dispatch");

    assert!(
        *drained.borrow(),
        "the drain must continue past an addressed report for the closed presentation"
    );
    teardown_platform_realm();
}

/// Borrow-discipline test: a `Resized` event dispatched while the
/// registration-lifetime applier slot is empty (no applier installed
/// yet, or already torn down) must skip with a trace rather than
/// unwrap/panic — the take/call/restore protocol's `None` arm.
///
/// A genuinely *nested* re-entrant call (the applier's own call
/// triggering another `Resized` dispatch before it returns) cannot
/// observe an empty slot here: `dispatch_platform_realm` always defers a
/// call made while `draining` to the FIFO queue rather than running it
/// synchronously, and by the time that queued event is drained the
/// outer call has already restored the slot. The `None` arm exists for
/// the case this test exercises directly: a `Resized` event reaching
/// [`PlatformToUi::run`] before [`install_surface_applier`] has run (or
/// after it has been cleared), never for synchronous reentrancy.
///
/// If reverted: replace the `None` arm's trace-and-skip with
/// `.expect("applier installed")` and this panics instead of returning
/// `Ok`.
#[test]
fn resized_with_no_applier_installed_skips_instead_of_panicking() {
    let dispatcher = install_test_realm();
    // Deliberately no `install_surface_applier` call: the slot starts
    // (and stays) empty.

    let result = dispatch_platform_realm(
        dispatcher,
        RealmTask::Event(PlatformToUi::Resized {
            size: flui_types::Size::new(
                flui_types::geometry::px(20.0),
                flui_types::geometry::px(20.0),
            ),
            scale_factor: 1.0,
        }),
    );

    assert!(
        result.is_ok(),
        "a Resized event with no applier installed must not panic; it \
         coalesces onto the next real applier install instead"
    );
    teardown_platform_realm();
}

#[test]
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

/// A panic inside the surface applier's own call (caught by
/// `dispatch_platform_realm`'s outer `catch_unwind`, same as any other
/// panicking event) must not permanently strand resizing: the
/// `SurfaceApplierRestoreGuard` restores the applier into the TLS slot
/// during the unwinding drop, so the next `Resized` still reaches it.
///
/// If reverted: revert to restoring the applier only after a successful
/// call (no drop guard) and this fails — the second `Resized` silently
/// coalesces at the `None` arm instead of calling the applier again.
#[test]
fn surface_applier_panic_is_caught_and_the_applier_still_applies_next_time() {
    let dispatcher = install_test_realm();
    let calls = Rc::new(RefCell::new(0));
    let calls_in_closure = Rc::clone(&calls);
    install_surface_applier(dispatcher.address.realm_id, move |_size, _scale_factor| {
        *calls_in_closure.borrow_mut() += 1;
        assert_ne!(
            *calls_in_closure.borrow(),
            1,
            "surface applier panics on its first call (simulated backend failure)"
        );
    });

    let resize_event = |side: f32| {
        RealmTask::Event(PlatformToUi::Resized {
            size: flui_types::Size::new(
                flui_types::geometry::px(side),
                flui_types::geometry::px(side),
            ),
            scale_factor: 1.0,
        })
    };

    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = dispatch_platform_realm(dispatcher, resize_event(20.0));
    }));
    assert!(
        panic.is_err(),
        "the first Resized's applier call must panic"
    );

    dispatch_platform_realm(dispatcher, resize_event(30.0))
        .expect("dispatch after the caught panic still succeeds");

    assert_eq!(
        *calls.borrow(),
        2,
        "the second Resized must still reach the applier — a panic on the first call \
         must not permanently strand resizing"
    );
    teardown_platform_realm();
}

#[test]
fn callback_on_wrong_thread_is_rejected() {
    let dispatcher = install_test_realm();
    let result = std::thread::spawn(move || {
        dispatch_platform_realm(dispatcher, RealmTask::Frame(Box::new(|_| {})))
    })
    .join()
    .expect("worker test thread");
    assert_eq!(result, Err(RealmDispatchError::WrongThread));
}

#[test]
fn nested_resize_and_window_focus_wait_until_frame_returns() {
    let dispatcher = install_test_realm();
    let order = Rc::new(RefCell::new(Vec::new()));
    let outer = Rc::clone(&order);
    let applier_calls = Rc::clone(&order);
    install_surface_applier(dispatcher.address.realm_id, move |_size, _scale_factor| {
        applier_calls.borrow_mut().push(3);
    });
    dispatch_platform_realm(
        dispatcher,
        RealmTask::Frame(Box::new(move |_| {
            outer.borrow_mut().push(1);
            dispatch_platform_realm(
                dispatcher,
                RealmTask::Event(PlatformToUi::WindowFocus(true)),
            )
            .expect("window focus queues");
            dispatch_platform_realm(
                dispatcher,
                RealmTask::Event(PlatformToUi::Resized {
                    size: flui_types::Size::new(
                        flui_types::geometry::px(640.0),
                        flui_types::geometry::px(480.0),
                    ),
                    scale_factor: 2.0,
                }),
            )
            .expect("resize queues");
            outer.borrow_mut().push(2);
        })),
    )
    .expect("frame dispatches");
    // Two different `PlatformToUi` variants nested inside a `Frame` still
    // queue FIFO rather than running immediately — the property
    // `reentrant_frame_event_is_queued_fifo` proves for same-variant
    // nesting, this proves it holds across variant types too.
    assert_eq!(*order.borrow(), vec![1, 2, 3]);
}

#[test]
fn teardown_drops_queued_destructors_outside_tls_borrow() {
    struct ReenterOnDrop {
        dispatcher: RealmDispatcher,
        dropped: Rc<RefCell<bool>>,
    }

    impl Drop for ReenterOnDrop {
        fn drop(&mut self) {
            let result =
                dispatch_platform_realm(self.dispatcher, RealmTask::Frame(Box::new(|_| {})));
            assert_eq!(result, Err(RealmDispatchError::RealmUnavailable));
            *self.dropped.borrow_mut() = true;
        }
    }

    let dispatcher = install_test_realm();
    let dropped = Rc::new(RefCell::new(false));
    let probe = ReenterOnDrop {
        dispatcher,
        dropped: Rc::clone(&dropped),
    };
    APP_RUNTIME.with(|slot| {
        slot.borrow_mut()
            .realms
            .get_mut(&dispatcher.address.realm_id)
            .expect("just installed above")
            .queue
            .push_back((
                dispatcher.address.presentation_id,
                RealmTask::Frame(Box::new(move |_| drop(probe))),
            ));
    });
    teardown_platform_realm();
    assert!(*dropped.borrow());
}

#[cfg(feature = "hot-reload")]
#[test]
fn old_registered_hot_reload_hook_cannot_touch_recreated_realm() {
    use flui_hot_reload::{register_request_rebuild, request_rebuild};

    use crate::app::hot_reload::queued_hot_reload_hook;

    let runtime_a = crate::app::ui_realm::UiRealm::for_test();
    let sender_a = runtime_a.command_sender();
    let old_a_hook = queued_hot_reload_hook(sender_a.clone());
    let registration_a = register_request_rebuild(queued_hot_reload_hook(sender_a));
    let _realm_a = install_platform_realm(runtime_a, &test_window());
    teardown_platform_realm();

    let runtime_b = crate::app::ui_realm::UiRealm::for_test();
    let sender_b = runtime_b.command_sender();
    let realm_b = install_platform_realm(runtime_b, &test_window());
    let registration_b = register_request_rebuild(queued_hot_reload_hook(sender_b));
    drop(registration_a);

    old_a_hook();
    let after_old = Rc::new(RefCell::new(None));
    let after_old_in_frame = Rc::clone(&after_old);
    dispatch_platform_realm(
        realm_b,
        RealmTask::Frame(Box::new(move |realm| {
            *after_old_in_frame.borrow_mut() = Some(realm.drain_commands());
        })),
    )
    .expect("B frame dispatches");
    assert_eq!(
        *after_old.borrow(),
        Some(crate::app::ui_realm::DrainReport::default()),
        "stale A hook must not enqueue into B"
    );

    std::thread::spawn(request_rebuild)
        .join()
        .expect("worker-side rebuild request");
    let after_current = Rc::new(RefCell::new(None));
    let after_current_in_frame = Rc::clone(&after_current);
    dispatch_platform_realm(
        realm_b,
        RealmTask::Frame(Box::new(move |realm| {
            *after_current_in_frame.borrow_mut() = Some(realm.drain_commands());
        })),
    )
    .expect("B frame dispatches");
    assert_eq!(
        after_current.borrow().as_ref().map(|report| report.invoked),
        Some(1),
        "current B hook must dispatch exactly once"
    );

    drop(registration_b);
    teardown_platform_realm();
}

#[test]
fn whole_frame_event_keeps_realm_global_key_scope_active() {
    let realm = crate::app::ui_realm::UiRealm::for_test();
    let key = flui_view::GlobalKey::<()>::new();
    let element = flui_foundation::ElementId::new(91);
    realm
        .widgets()
        .with_build_owner_mut(|owner| owner.register_global_key(&key, element));
    let dispatcher = install_platform_realm(realm, &test_window());
    let key_after_frame = key.clone();

    assert_eq!(key.current_element(), None, "scope starts inactive");
    dispatch_platform_realm(
        dispatcher,
        RealmTask::Frame(Box::new(move |_| {
            assert_eq!(key.current_element(), Some(element));
        })),
    )
    .expect("frame dispatches");
    assert_eq!(
        key_after_frame.current_element(),
        None,
        "frame scope is restored"
    );
    teardown_platform_realm();
}

/// A disabled->enabled lifecycle edge must redirty the root when
/// delivered the way production actually delivers one: as a
/// `PlatformToUi::Lifecycle` event through `dispatch_platform_realm`,
/// which takes the realm OUT of `APP_RUNTIME` for the duration of the
/// dispatch and only restores it after `UiRealm::update_host_lifecycle`
/// returns. A fire-time `APP_RUNTIME` lookup (an `UpdateScheduler` lifecycle
/// listener, the previous shape of this fix) can never see the realm
/// during that exact window — driving a throwaway `UpdateScheduler` directly,
/// the previous version of this test's approach, never exercises that
/// window at all, which is why it never caught the bug.
#[test]
fn frames_reenable_redirties_root_when_dispatched_through_the_realm_queue() {
    use std::cell::Cell;

    #[derive(Clone)]
    struct LeafView;

    impl flui_view::RenderView for LeafView {
        type Protocol = flui_rendering::protocol::BoxProtocol;
        type RenderObject = flui_objects::RenderSizedBox;

        fn create_render_object(
            &self,
            _ctx: &flui_view::RenderObjectContext<'_>,
        ) -> Self::RenderObject {
            flui_objects::RenderSizedBox::shrink()
        }

        fn update_render_object(
            &self,
            _ctx: &flui_view::RenderObjectContext<'_>,
            render_object: &mut Self::RenderObject,
        ) -> flui_rendering::RenderUpdateImpact {
            render_object.set_size(
                Some(flui_types::Pixels::ZERO),
                Some(flui_types::Pixels::ZERO),
            )
        }
    }

    impl View for LeafView {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::render_variable(self)
        }
    }

    let dispatcher = install_test_realm();
    dispatch_platform_realm(
        dispatcher,
        RealmTask::Frame(Box::new(|realm| {
            realm
                .attach_root_widget(&LeafView)
                .expect("attach succeeds");
        })),
    )
    .expect("attach dispatches");

    // Consume the post-attach dirty flag with one real frame first, so
    // the pipeline is genuinely idle going into the lifecycle dance
    // below -- otherwise the later paint this test asserts on could be
    // explained by left-over dirt from attach, not by the redirty under
    // test.
    let initial_presented = Rc::new(Cell::new(false));
    let initial_presented_in_frame = Rc::clone(&initial_presented);
    dispatch_platform_realm(
        dispatcher,
        RealmTask::Frame(Box::new(move |realm| {
            let mut backend = TestRasterBackend::always_presents();
            initial_presented_in_frame.set(realm.render_frame_entered(&mut backend));
        })),
    )
    .expect("initial frame dispatches");
    assert!(
        initial_presented.get(),
        "precondition: the attached root must present on its first frame"
    );

    let root_is_clean = Rc::new(Cell::new(false));
    let root_is_clean_in_frame = Rc::clone(&root_is_clean);
    dispatch_platform_realm(
        dispatcher,
        RealmTask::Frame(Box::new(move |realm| {
            root_is_clean_in_frame.set(!realm.needs_redraw());
        })),
    )
    .expect("clean-check dispatches");
    assert!(
        root_is_clean.get(),
        "precondition: the root must be clean (Idle) going into the lifecycle dance"
    );

    // The disable edge, delivered the way production actually delivers
    // a lifecycle transition: as a `PlatformToUi::Lifecycle` event
    // through the realm dispatch queue, which takes the realm OUT of
    // `APP_RUNTIME` for the duration of the dispatch.
    dispatch_platform_realm(
        dispatcher,
        RealmTask::Event(PlatformToUi::Lifecycle(AppLifecycleState::Hidden)),
    )
    .expect("hidden dispatches");

    // The re-enable edge under test, same delivery path.
    dispatch_platform_realm(
        dispatcher,
        RealmTask::Event(PlatformToUi::Lifecycle(AppLifecycleState::Resumed)),
    )
    .expect("resumed dispatches");

    let repainted = Rc::new(Cell::new(false));
    let repainted_in_frame = Rc::clone(&repainted);
    let render_scene_calls = Rc::new(Cell::new(0u32));
    let render_scene_calls_in_frame = Rc::clone(&render_scene_calls);
    dispatch_platform_realm(
        dispatcher,
        RealmTask::Frame(Box::new(move |realm| {
            let mut backend = TestRasterBackend::always_presents();
            repainted_in_frame.set(realm.render_frame_entered(&mut backend));
            render_scene_calls_in_frame.set(backend.render_scene_calls);
        })),
    )
    .expect("post-reenable frame dispatches");

    assert!(
        repainted.get(),
        "a disabled->enabled lifecycle edge delivered through the real realm dispatch \
         queue must redirty the root so the next frame actually presents, not stay Idle \
         -- this is the exact stale-window-on-resume bug the redirty logic exists to \
         prevent"
    );
    assert_eq!(
        render_scene_calls.get(),
        1,
        "the redirty must produce real paint output, not merely flip a flag \
         render_frame_entered ignores"
    );

    teardown_platform_realm();
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

/// Two realms hosted by the SAME `AppRuntime` share no gesture-arena
/// state: a pointer dispatched only to realm A must leave realm B's
/// arena completely untouched. Both realms are reached only through
/// `install_platform_realm`/`install_realm_alongside` and
/// `dispatch_platform_realm` here -- never a directly-held `UiRealm`
/// handle -- so this is the "through `AppRuntime`" half of the
/// end-state invariant; `ui_realm/tests/mod.rs`'s `two_realms_coexist_same_thread`
/// family already proves the same disjointness at the `UiRealm`
/// construction level.
#[test]
fn two_window_realms_share_no_ui_state_through_app_runtime() {
    let (dispatcher_a, dispatcher_b) = install_two_test_realms();

    assert_ne!(
        dispatcher_a.address.realm_id, dispatcher_b.address.realm_id,
        "AppRuntime must never host two live realms under the same identity"
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
                "realm B's gesture arena must be untouched by a pointer dispatched only \
                 to realm A -- AppRuntime shares no mutable UI state between hosted realms"
            );
        })),
    )
    .expect("B dispatches independently of A");

    teardown_platform_realm();
}

/// `AppRuntime::next_wake` (the wall-clock wake seam) computes the
/// MIN wake deadline across every hosted realm on this loop thread — an
/// idle realm with an earlier-armed deadline must win over a busier
/// sibling with a later one, regardless of install order. Realm A gets
/// a far (5s) long-press timeout; realm B (installed second) gets a
/// near (50ms) one — the aggregate must reflect B's, not A's, and not
/// merely "whichever realm happens to be dispatched last".
///
/// The recognizers are kept alive in `recognizers` for the whole test:
/// `GestureArena`'s deadline registry holds only a `Weak` reference, so
/// a recognizer dropped at the end of its own dispatch closure would
/// silently vanish from `next_deadline`'s snapshot before this test ever
/// reads it.
#[test]
fn next_wake_is_the_min_deadline_across_every_installed_realm() {
    use std::cell::RefCell;
    use std::rc::Rc;

    use flui_interaction::{
        GestureRecognizer, GestureSettings, LongPressGestureRecognizer, PointerId,
    };

    let (dispatcher_a, dispatcher_b) = install_two_test_realms();
    // `Rc`, not `Arc`: `LongPressGestureRecognizer` is owner-thread-affine
    // (`!Send`/`!Sync`, like every gesture recognizer), and this vec
    // never needs to cross a thread -- both dispatches below run on this
    // same test thread.
    let recognizers: Rc<RefCell<Vec<Arc<LongPressGestureRecognizer>>>> =
        Rc::new(RefCell::new(Vec::new()));

    let recognizers_a = Rc::clone(&recognizers);
    dispatch_platform_realm(
        dispatcher_a,
        RealmTask::Frame(Box::new(move |realm| {
            let arena = realm.gestures().arena().clone();
            let recognizer = LongPressGestureRecognizer::with_settings(
                arena,
                GestureSettings::touch_defaults()
                    .with_long_press_timeout(std::time::Duration::from_secs(5)),
            )
            .with_on_long_press(|| {});
            let pointer = PointerId::new(2).expect("nonzero pointer id");
            recognizer.add_pointer(
                pointer,
                flui_types::Offset::new(
                    flui_types::geometry::px(10.0),
                    flui_types::geometry::px(10.0),
                ),
                flui_types::Offset::new(
                    flui_types::geometry::px(10.0),
                    flui_types::geometry::px(10.0),
                ),
            );
            recognizers_a.borrow_mut().push(recognizer);
        })),
    )
    .expect("A dispatches");

    let before_b = std::time::Instant::now();
    let recognizers_b = Rc::clone(&recognizers);
    dispatch_platform_realm(
        dispatcher_b,
        RealmTask::Frame(Box::new(move |realm| {
            let arena = realm.gestures().arena().clone();
            let recognizer = LongPressGestureRecognizer::with_settings(
                arena,
                GestureSettings::touch_defaults()
                    .with_long_press_timeout(std::time::Duration::from_millis(50)),
            )
            .with_on_long_press(|| {});
            let pointer = PointerId::new(3).expect("nonzero pointer id");
            recognizer.add_pointer(
                pointer,
                flui_types::Offset::new(
                    flui_types::geometry::px(20.0),
                    flui_types::geometry::px(20.0),
                ),
                flui_types::Offset::new(
                    flui_types::geometry::px(20.0),
                    flui_types::geometry::px(20.0),
                ),
            );
            recognizers_b.borrow_mut().push(recognizer);
        })),
    )
    .expect("B dispatches");

    let next_wake = APP_RUNTIME
        .with(|slot| slot.borrow().next_wake())
        .expect("two armed deadlines pending -- next_wake must be Some");

    let until_wake = next_wake.saturating_duration_since(before_b);
    assert!(
        until_wake < std::time::Duration::from_secs(1),
        "next_wake must reflect realm B's near (50ms) deadline, not realm A's far (5s) \
         one -- got {until_wake:?} until wake"
    );

    drop(recognizers);
    teardown_platform_realm();
}

/// A realm with no armed gesture deadline contributes no wall-clock
/// wake. The standalone timer service that once made this assertion
/// necessary has been retired; all remaining sources are realm-owned
/// and drained by the frame path that the wake re-enters.
#[test]
fn next_wake_is_none_when_no_realm_owned_deadline_is_armed() {
    let dispatcher = install_test_realm();
    dispatch_platform_realm(dispatcher, RealmTask::Frame(Box::new(|_realm| {})))
        .expect("realm dispatches with nothing armed");

    let next_wake = APP_RUNTIME.with(|slot| slot.borrow().next_wake());
    assert!(
        next_wake.is_none(),
        "an idle realm with no gesture deadline must let the event loop block"
    );

    teardown_platform_realm();
}

/// A rejected nested cross-realm dispatch must never smuggle its task
/// into the target realm's queue: the task must not run during the
/// rejected attempt, AND must not be silently delivered by the NEXT
/// legitimate dispatch to that same realm either. Before the ordering
/// fix, the event was pushed into realm B's queue BEFORE the
/// nested-dispatch guard ran, so nothing ever removed a rejected
/// event — the very next legitimate dispatch to B would pop and run it.
#[test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "the nested-dispatch guard fires via debug_assert!(false, ...) in this build; \
              release builds return Err without panicking, so realm A's own outer dispatch \
              never unwinds and this test's catch_unwind expectation does not apply"
)]
fn rejected_nested_dispatch_never_delivers_its_task_on_the_next_legitimate_dispatch() {
    let (dispatcher_a, dispatcher_b) = install_two_test_realms();

    let rejected_ran = Rc::new(Cell::new(false));
    let rejected_ran_in_task = Rc::clone(&rejected_ran);

    // From inside realm A's own dispatched task, attempt a NESTED
    // dispatch to realm B -- a DIFFERENT, resident-and-idle realm --
    // which the nested-cross-realm-dispatch guard must reject before
    // ever touching B's queue. The guard's debug_assert! panics in this
    // build; the panic unwinds through A's own dispatch, which this
    // catch_unwind survives.
    let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        dispatch_platform_realm(
            dispatcher_a,
            RealmTask::Frame(Box::new(move |_realm| {
                let _ = dispatch_platform_realm(
                    dispatcher_b,
                    RealmTask::Frame(Box::new(move |_| {
                        rejected_ran_in_task.set(true);
                    })),
                );
            })),
        )
    }));
    assert!(
        attempt.is_err(),
        "the nested cross-realm dispatch attempt must panic via the guard's debug_assert!"
    );

    // A LEGITIMATE, top-level dispatch to realm B, driven afterward.
    dispatch_platform_realm(dispatcher_b, RealmTask::Frame(Box::new(|_| {})))
        .expect("realm B still dispatches normally after the rejected nested attempt");

    assert!(
        !rejected_ran.get(),
        "the REJECTED task must never run -- not during the rejected attempt, and not \
         smuggled into realm B's queue for a later legitimate dispatch to deliver"
    );

    teardown_platform_realm();
}

/// Realm A tearing itself down from WITHIN its own dispatched task must
/// not disturb sibling realm B's ability to keep dispatching and
/// producing (presenting) frames.
#[test]
fn teardown_realm_a_mid_dispatch_leaves_realm_b_frame_producing() {
    use flui_widgets::SizedBox;

    let (dispatcher_a, dispatcher_b) = install_two_test_realms();
    let realm_a_id = dispatcher_a.address.realm_id;

    dispatch_platform_realm(
        dispatcher_b,
        RealmTask::Frame(Box::new(|realm| {
            realm
                .attach_root_widget_with_size(&SizedBox::new(20.0, 20.0), 20.0, 20.0)
                .expect("B mounts its own root");
        })),
    )
    .expect("B's attach dispatches");

    // Realm A tears itself down from WITHIN its own dispatched task. The
    // defer-to-idle discipline (`AppRuntime::request_realm_uninstall`)
    // means this queues rather than nests -- applied once this task's
    // own dispatch restores below.
    dispatch_platform_realm(
        dispatcher_a,
        RealmTask::Frame(Box::new(move |_realm| {
            uninstall_platform_realm(realm_a_id);
        })),
    )
    .expect("A's own mid-dispatch teardown task runs");

    assert!(
        APP_RUNTIME.with(|slot| slot.borrow().realms.get(&realm_a_id).is_none()),
        "realm A must be gone once its own mid-dispatch uninstall request has applied"
    );

    let presented = Rc::new(RefCell::new(false));
    let presented_in_frame = Rc::clone(&presented);
    dispatch_platform_realm(
        dispatcher_b,
        RealmTask::Frame(Box::new(move |realm| {
            let mut backend = TestRasterBackend::always_presents().with_size(64, 64);
            *presented_in_frame.borrow_mut() = realm.render_frame_entered(&mut backend);
        })),
    )
    .expect("realm B still dispatches after realm A's mid-dispatch teardown");
    assert!(
        *presented.borrow(),
        "realm B must still produce (and present) a frame after a sibling realm tore \
         itself down mid-dispatch"
    );

    teardown_platform_realm();
}

/// `install_realm_alongside` must REFUSE a window-id collision, not
/// silently re-route the colliding window's mapping onto the new realm
/// (`WindowRegistry::register_window`'s replace semantics would do
/// exactly that -- the wrong tool for two realms meant to coexist, and
/// the exact failure mode this crate's own `HeadlessPlatform`
/// window-id-aliasing gotcha produced during development).
#[test]
fn install_realm_alongside_refuses_a_colliding_window_id_instead_of_silently_rerouting() {
    let platform = flui_platform::headless_platform();
    let window: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create a test window");
    let dispatcher_a = install_platform_realm(crate::app::ui_realm::UiRealm::for_test(), &window);

    let result = install_realm_alongside(crate::app::ui_realm::UiRealm::for_test(), &window);
    match result {
        Err(crate::app::window_registry::RegistryError::WindowAlreadyMapped { existing }) => {
            assert_eq!(
                existing, dispatcher_a.address,
                "the refusal must report realm A's own address as the existing mapping"
            );
        }
        other => {
            panic!("a colliding window id must be refused as WindowAlreadyMapped, got {other:?}")
        }
    }

    assert!(
        APP_RUNTIME.with(|slot| slot
            .borrow()
            .realms
            .get(&dispatcher_a.address.realm_id)
            .is_some()),
        "realm A's own registration must be untouched by the refused collision"
    );

    teardown_platform_realm();
}

/// The DEFERRED half of the collision-refusal path:
/// `drain_pending_realm_mutations`'s own `Install` arm, reached only
/// when a collision is requested mid-visit/mid-dispatch rather than at
/// idle. Must behave identically to the immediate path above -- refuse,
/// leave realm A's own registration untouched, and never corrupt the
/// registry -- and must do so without ever dropping the rejected
/// `RealmSlot` (and the `UiRealm` it owns) while this visit's own
/// `APP_RUNTIME` borrow is live: `apply_install`/
/// `drain_pending_realm_mutations` route a rejected slot through
/// `removed` instead, the same bucket a removed `Uninstall` slot uses,
/// dropped only once the caller's borrow has released.
#[test]
fn deferred_install_collision_is_refused_without_corrupting_the_registry() {
    let platform = flui_platform::headless_platform();
    let window: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create a test window");
    let dispatcher_a = install_platform_realm(crate::app::ui_realm::UiRealm::for_test(), &window);
    let realm_a_id = dispatcher_a.address.realm_id;

    let colliding_realm = crate::app::ui_realm::UiRealm::for_test();
    let colliding_address = flui_foundation::PresentationAddress {
        realm_id: colliding_realm.realm_id(),
        presentation_id: colliding_realm.presentation_id(),
    };
    let mut colliding_realm = Some(colliding_realm);

    for_each_installed_realm(|realm| {
        assert_eq!(
            realm.realm_id(),
            realm_a_id,
            "the sole installed realm is realm A"
        );
        let deferred = APP_RUNTIME.with(|slot| {
            slot.borrow_mut().request_realm_install(
                colliding_address.realm_id,
                RealmSlot {
                    realm: colliding_realm.take(),
                    queue: VecDeque::new(),
                    draining: false,
                    address: colliding_address,
                    surface_applier: None,
                },
                std::sync::Arc::clone(&window),
            )
        });
        assert!(
            deferred.is_ok(),
            "an Install requested mid-visit must defer (Ok), not fail synchronously -- \
             the collision is only discovered once the deferred drain applies it"
        );
    });

    assert!(
        APP_RUNTIME.with(|slot| slot
            .borrow()
            .realms
            .get(&colliding_address.realm_id)
            .is_none()),
        "the colliding realm must never enter the registry -- its window id collided with \
         realm A's own mapping when the deferred drain applied it"
    );
    assert!(
        APP_RUNTIME.with(|slot| slot.borrow().realms.get(&realm_a_id).is_some()),
        "realm A's own registration must survive the sibling's refused, deferred collision"
    );

    teardown_platform_realm();
}

/// Closing one of two hosted windows must not exit the loop while a
/// sibling realm is still hosted (the default `ExitPolicy::OnLastWindowClosed`).
#[test]
fn closing_one_of_two_windows_does_not_exit_the_loop() {
    let (dispatcher_a, dispatcher_b) = install_two_test_realms();

    uninstall_platform_realm(dispatcher_a.address.realm_id);

    let should_exit = APP_RUNTIME.with(|slot| {
        let (exit, removed) = slot
            .borrow_mut()
            .should_exit(ExitPolicy::OnLastWindowClosed);
        drop(removed);
        exit
    });
    assert!(
        !should_exit,
        "closing one of two windows must not exit the loop while a sibling realm is \
         still hosted"
    );
    assert!(
        APP_RUNTIME.with(|slot| slot
            .borrow()
            .realms
            .get(&dispatcher_b.address.realm_id)
            .is_some()),
        "the surviving realm B must still be hosted"
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
#[test]
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

/// `WindowPolicy::SharedRealm`, driven through the REAL embedder seam
/// (`open_secondary_window`) — proves this really is forest-membership
/// routing (a second PRESENTATION of the SAME realm), not a second realm
/// in disguise: the hosted-realm count must stay at one, while the
/// realm's own presentation count grows from one to two.
#[test]
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

/// The live-loop counterpart of `closing_one_of_two_windows_does_not_
/// exit_the_loop` (which drives `AppRuntime::should_exit` directly).
/// This one goes through the REAL platform seam end to end:
/// `install_exit_policy_hook` installs the hook exactly like
/// `run_desktop`'s bootstrap does, and `window.close()` on each REAL
/// (headless) native window drives flui-platform's own
/// `notify_closed`/`CloseRequested`-equivalent bookkeeping — THAT is
/// what consults the hook, never a direct call into `AppRuntime`.
/// `uninstall_platform_realm` before each `close()` mirrors
/// `run_desktop`'s own `on_close` ordering exactly (see that callback's
/// doc for why the ordering is load-bearing): the registry must already
/// reflect the closing window's realm being gone by the time the hook
/// reads it. Closing one of two windows must not fire the platform's
/// own `quit` callback; closing the last one must fire it exactly once.
#[test]
fn closing_one_of_two_windows_does_not_exit_through_the_real_platform_hook_closing_both_does() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let _clear_guard = OwnerHostClearGuard::arm();
    let platform = flui_platform::headless_platform();
    let quit_calls = Arc::new(AtomicUsize::new(0));
    let quit_calls_for_on_ready = Arc::clone(&quit_calls);
    let ready = platform.run(Box::new(move |owner| {
        install_owner_platform(owner).expect("install owner wake transport");
        install_exit_policy_hook(ExitPolicy::OnLastWindowClosed);

        let quit_calls_for_handler = Arc::clone(&quit_calls_for_on_ready);
        with_owner_platform(|owner| {
            owner.shared().on_quit(Box::new(move || {
                quit_calls_for_handler.fetch_add(1, Ordering::SeqCst);
            }));
        });

        let window_a: Arc<dyn PlatformWindow> =
            with_owner_platform(|owner| owner.open_window(flui_platform::WindowOptions::default()))
                .expect("owner installed above")
                .and_then(flui_platform::WindowOpen::try_ready)
                .expect("headless open_window is always Ready");
        let window_b: Arc<dyn PlatformWindow> =
            with_owner_platform(|owner| owner.open_window(flui_platform::WindowOptions::default()))
                .expect("owner installed above")
                .and_then(flui_platform::WindowOpen::try_ready)
                .expect("headless open_window is always Ready");

        let dispatcher_a =
            install_platform_realm(crate::app::ui_realm::UiRealm::for_test(), &window_a);
        let dispatcher_b =
            install_realm_alongside(crate::app::ui_realm::UiRealm::for_test(), &window_b)
                .expect("realm B installs alongside realm A cleanly");

        // Close A: realm B is still resident, so the hook must veto.
        uninstall_platform_realm(dispatcher_a.address.realm_id);
        window_a.close();
        assert_eq!(
            quit_calls_for_on_ready.load(Ordering::SeqCst),
            0,
            "closing one of two windows must not fire the platform's own quit callback"
        );

        // Close B (the last one): the registry is now empty, so the
        // hook must allow the exit and the platform's own quit must
        // fire exactly once.
        uninstall_platform_realm(dispatcher_b.address.realm_id);
        window_b.close();
        assert_eq!(
            quit_calls_for_on_ready.load(Ordering::SeqCst),
            1,
            "closing the LAST window must fire the platform's own quit callback exactly once"
        );

        Ok(())
    }));
    ready.expect("on_ready must not fail");

    teardown_platform_realm();
}

/// Hot-restart's own visit primitive (`for_each_installed_realm`) must
/// see exactly what each `WindowPolicy` claims to have installed --
/// proven for both modes, each opened through the REAL
/// `open_secondary_window` seam (not a direct
/// `install_realm_alongside`/`install_presentation_alongside` call).
#[test]
fn hot_restart_with_two_realms_and_with_two_presentations() {
    // Mode 1: WindowPolicy::SeparateRealms -- hot-restart must visit TWO
    // distinct realms.
    {
        let (dispatcher_a, _clear_guard) = install_realm_a_through_a_real_owner_platform();
        open_secondary_window(AppConfig::default(), WindowPolicy::SeparateRealms)
            .expect("WindowPolicy::SeparateRealms must install a second realm cleanly");

        let mut visited_realms = Vec::new();
        for_each_installed_realm(|realm| {
            visited_realms.push(realm.realm_id());
        });

        assert_eq!(
            visited_realms.len(),
            2,
            "hot-restart must visit both policy-installed realms"
        );
        assert!(
            visited_realms.contains(&dispatcher_a.address.realm_id),
            "hot-restart's visit must include realm A"
        );

        teardown_platform_realm();
    }

    // Mode 2: WindowPolicy::SharedRealm -- hot-restart must visit
    // exactly ONE realm, carrying BOTH presentations.
    {
        let (dispatcher_a, _clear_guard) = install_realm_a_through_a_real_owner_platform();
        open_secondary_window(AppConfig::default(), WindowPolicy::SharedRealm).expect(
            "WindowPolicy::SharedRealm must install a second presentation into realm A \
             cleanly",
        );

        let mut visited: Vec<(RealmId, usize)> = Vec::new();
        for_each_installed_realm(|realm| {
            visited.push((realm.realm_id(), realm.presentation_count()));
        });

        assert_eq!(
            visited,
            vec![(dispatcher_a.address.realm_id, 2)],
            "hot-restart must visit exactly the one SharedRealm-hosted realm, carrying both \
             presentations through the visit -- never split into two realms"
        );

        teardown_platform_realm();
    }
}

/// Like [`install_realm_a_through_a_real_owner_platform`], but ALSO
/// installs the exit-policy hook (mirroring `run_desktop`'s own
/// bootstrap exactly) and an `on_quit` counter — for tests that need to
/// drive a real window close through the REAL platform hook end to end,
/// not a direct `AppRuntime::should_exit` call.
fn install_realm_a_with_exit_policy_and_quit_counter() -> (
    RealmDispatcher,
    std::sync::Arc<dyn flui_platform::traits::PlatformWindow>,
    Arc<std::sync::atomic::AtomicUsize>,
    OwnerHostClearGuard,
) {
    let (dispatcher, window, quit_calls, guard, _reevaluation) =
        install_realm_a_with_exit_policy_quit_counter_and_reevaluation();
    (dispatcher, window, quit_calls, guard)
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

/// `open_secondary_window`'s own `on_close` wiring, not a test manually
/// calling `uninstall_platform_realm`/`close_presentation` itself, must
/// be what tears the secondary window's realm down. Before
/// the fix this window's `on_close` only logged — closing it left its
/// realm resident in `AppRuntime` forever, so closing the app's LAST
/// window afterward never actually emptied the registry and the
/// exit-policy hook vetoed the exit forever (a production hang: zero
/// windows left, app still alive). Drives BOTH closes through their own
/// registered `on_close`: `close_this_window` for the primary — the
/// exact primitive `run_desktop`'s bootstrap itself calls, since a full
/// GPU-backed desktop bootstrap cannot run in a unit test — and a REAL
/// `window_b.close()` for the secondary, exercising
/// `open_secondary_window`'s own wiring, never a hand-rolled substitute.
#[test]
fn separate_realms_secondary_window_close_tears_down_its_own_realm_and_allows_exit() {
    use std::sync::atomic::Ordering;

    let (dispatcher_a, window_a, quit_calls, _clear_guard) =
        install_realm_a_with_exit_policy_and_quit_counter();

    let config = AppConfig::default().with_frame_failure_detail(FrameFailureDetail::Redacted);
    let (dispatcher_b, window_b) = open_secondary_window_impl(config, WindowPolicy::SeparateRealms)
        .expect("WindowPolicy::SeparateRealms must install a second realm cleanly")
        .expect("headless open_window is always Ready, never Pending");
    assert_ne!(
        dispatcher_a.address.realm_id, dispatcher_b.address.realm_id,
        "SeparateRealms must install a genuinely distinct realm"
    );
    assert_eq!(
        APP_RUNTIME.with(|slot| {
            slot.borrow()
                .realms
                .get(&dispatcher_b.address.realm_id)
                .and_then(|realm_slot| realm_slot.realm.as_ref())
                .expect("secondary realm installed")
                .frame_failure_detail_for_test()
        }),
        FrameFailureDetail::Redacted,
        "the Ready arm must carry the secondary config's detail policy into its new realm"
    );

    // Close A first, through its OWN real on_close (siblings survive)
    // -- must not exit.
    window_a.close();
    assert_eq!(
        quit_calls.load(Ordering::SeqCst),
        0,
        "closing one of two windows must not exit while the sibling realm survives"
    );

    // Close B through its OWN registered `on_close` -- a REAL
    // `window.close()`, never a manual `uninstall_platform_realm`/
    // `close_presentation` call from this test. This is the exact call
    // that was log-only before the fix.
    window_b.close();
    assert_eq!(
        APP_RUNTIME.with(|slot| slot.borrow().realms.iter().count()),
        0,
        "closing the secondary window through its own real on_close must have uninstalled \
         its realm -- if open_secondary_window's on_close is ever log-only again, this \
         realm stays resident forever"
    );
    assert_eq!(
        quit_calls.load(Ordering::SeqCst),
        1,
        "closing the LAST window (the secondary, through its own real on_close) must allow \
         the exit exactly once"
    );

    teardown_platform_realm();
}

/// The `WindowPolicy::SharedRealm` counterpart: closing the secondary
/// presentation through its OWN real `on_close` must remove only ITSELF
/// — the shared realm and the still-open primary must survive
/// untouched — and must NOT exit while the primary survives. Before the
/// fix, `open_secondary_window`'s on_close was log-only for this policy
/// too, so the secondary presentation would have stayed forest-resident
/// (and, worse, focus/keyboard-addressable) forever after its native
/// window was gone.
#[test]
fn shared_realm_secondary_presentation_close_removes_only_itself_and_keeps_the_realm_alive() {
    use std::sync::atomic::Ordering;

    let (dispatcher_a, window_a, quit_calls, _clear_guard) =
        install_realm_a_with_exit_policy_and_quit_counter();

    dispatch_platform_realm(
        dispatcher_a,
        RealmTask::Frame(Box::new(|realm| {
            realm.set_frame_failure_detail(FrameFailureDetail::Redacted);
        })),
    )
    .expect("primary realm accepts its detail policy");

    let (dispatcher_b, window_b) = open_secondary_window_impl(
        AppConfig::default().with_frame_failure_detail(FrameFailureDetail::Verbatim),
        WindowPolicy::SharedRealm,
    )
    .expect("WindowPolicy::SharedRealm must install a second presentation into realm A cleanly")
    .expect("headless open_window is always Ready, never Pending");
    assert_eq!(
        dispatcher_a.address.realm_id, dispatcher_b.address.realm_id,
        "SharedRealm must route into the SAME realm as the primary"
    );
    assert_eq!(
        APP_RUNTIME.with(|slot| {
            slot.borrow()
                .realms
                .get(&dispatcher_a.address.realm_id)
                .and_then(|realm_slot| realm_slot.realm.as_ref())
                .expect("shared realm remains installed")
                .frame_failure_detail_for_test()
        }),
        FrameFailureDetail::Redacted,
        "a secondary SharedRealm config must not override the existing realm policy"
    );

    // Close B (a live non-sole presentation) through its own real
    // on_close -- must remove only itself, never the shared realm.
    window_b.close();
    let (realm_count, presentation_count) = APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        let realm_count = state.realms.iter().count();
        let presentation_count = state
            .realms
            .get(&dispatcher_a.address.realm_id)
            .and_then(|slot| slot.realm.as_ref())
            .expect(
                "the shared realm must still be resident -- B's own close must not \
                     have torn down the whole realm out from under the still-open primary",
            )
            .presentation_count();
        (realm_count, presentation_count)
    });
    assert_eq!(
        realm_count, 1,
        "the shared realm must survive B's own close"
    );
    assert_eq!(
        presentation_count, 1,
        "only B's own presentation must be removed; the primary's survives"
    );
    assert_eq!(
        quit_calls.load(Ordering::SeqCst),
        0,
        "the realm (and its primary presentation) still lives -- this must not exit"
    );

    // Now close the primary too, through its own real on_close (the
    // realm's last presentation) -- must exit.
    window_a.close();
    assert_eq!(
        APP_RUNTIME.with(|slot| slot.borrow().realms.iter().count()),
        0,
        "closing the realm's last surviving presentation must uninstall the whole realm"
    );
    assert_eq!(
        quit_calls.load(Ordering::SeqCst),
        1,
        "closing the last presentation of the last realm must allow the exit exactly once"
    );

    teardown_platform_realm();
}

/// `open_secondary_window`'s own `window.on_input` wiring dispatches
/// `RealmTask::Event(PlatformToUi::Input(..))` addressed to the
/// secondary window's own dispatcher — proves BOTH halves of that
/// routing: the secondary's OWN gesture arena receives it (receipt,
/// untested by `two_realms_via_separate_windows_policy_share_nothing`,
/// which only asserts sibling non-receipt), and the primary's arena
/// stays untouched (the established non-primary lesson —
/// `input_stamped_for_b_never_reaches_as_arena`'s own review-driven fix
/// — applied here through the POLICY-DRIVEN seam, not inverted to only
/// prove non-receipt as the original share-nothing test did).
#[test]
fn separate_realms_secondary_window_input_reaches_its_own_realm_not_the_primarys() {
    let (dispatcher_a, _clear_guard) = install_realm_a_through_a_real_owner_platform();

    let (dispatcher_b, _window_b) =
        open_secondary_window_impl(AppConfig::default(), WindowPolicy::SeparateRealms)
            .expect("WindowPolicy::SeparateRealms must install a second realm cleanly")
            .expect("headless open_window is always Ready, never Pending");

    // Exactly the call `open_secondary_window`'s own `window.on_input`
    // callback makes for window B's native input.
    dispatch_platform_realm(
        dispatcher_b,
        RealmTask::Event(PlatformToUi::Input(down_input(4.0))),
    )
    .expect("B dispatches");

    dispatch_platform_realm(
        dispatcher_b,
        RealmTask::Frame(Box::new(|realm| {
            assert_eq!(
                realm.gestures().active_pointer_count(),
                1,
                "the secondary window's own realm must receive input dispatched through \
                 its own on_input wiring"
            );
        })),
    )
    .expect("B dispatches");

    dispatch_platform_realm(
        dispatcher_a,
        RealmTask::Frame(Box::new(|realm| {
            assert_eq!(
                realm.gestures().active_pointer_count(),
                0,
                "input addressed to the secondary window's realm must never reach the \
                 primary's -- the established non-primary lesson, not inverted"
            );
        })),
    )
    .expect("A dispatches");

    teardown_platform_realm();
}

/// The P1 fix's own end-to-end probe: `open_secondary_window` must not
/// assume `OwnerPlatform::open_window` always resolves `Ready` — the
/// real winit owner lane returns `Pending` for any call after
/// `on_ready`, exactly the calling convention this function documents
/// as its own intended use (from a callback the FIRST window already
/// registered). `HeadlessPlatform::enable_deferred_window_open`
/// exercises that same arm without a real event loop. Drives the
/// request all the way through: Pending -> a manual `resolve_next` ->
/// the driver realm's own frame pump completing the install via
/// `finish_open_secondary_window` -> both windows closing cleanly and
/// the exit-policy hook firing exactly once.
#[test]
fn open_secondary_window_completes_through_the_pending_arm_like_the_real_winit_owner_lane() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Needed to call `.run` on the concrete `HeadlessPlatform` boxed
    // below (a `Box<dyn Platform>`, unlike `headless_platform()`'s own
    // return type, needs the trait in scope for dot-call resolution on
    // a boxed concrete receiver).
    use flui_platform::traits::Platform;

    let clear_guard = OwnerHostClearGuard::arm();
    let platform = flui_platform::HeadlessPlatform::new();
    let deferred = platform.enable_deferred_window_open();
    let quit_calls = Arc::new(AtomicUsize::new(0));
    let quit_calls_for_on_ready = Arc::clone(&quit_calls);

    let ready = Box::new(platform).run(Box::new(move |owner| {
        install_owner_platform(owner).expect("install owner wake transport");
        install_exit_policy_hook(ExitPolicy::OnLastWindowClosed);

        let quit_calls_for_handler = Arc::clone(&quit_calls_for_on_ready);
        with_owner_platform(|owner| {
            owner.shared().on_quit(Box::new(move || {
                quit_calls_for_handler.fetch_add(1, Ordering::SeqCst);
            }));
        });

        // First window: also deferred, since `enable_deferred_window_open`
        // switches EVERY `open_owner_window` call on this platform, not
        // just the second one -- resolve it the same way real code
        // resolving a `Pending` open would (`try_take` after delivery),
        // proving the mechanism generically before window B ever uses it.
        let mut pending_a = match with_owner_platform(|owner| {
            owner.open_window(flui_platform::WindowOptions::default())
        })
        .expect("BUG: install_owner_platform just ran above")
        .expect("deferred mode still accepts the request")
        {
            flui_platform::WindowOpen::Pending(pending) => pending,
            flui_platform::WindowOpen::Ready(_) => {
                panic!("deferred mode must return Pending, not Ready")
            }
        };
        let resolved_a = deferred
            .resolve_next()
            .expect("window A's request is the only one pending");
        let window_a = pending_a
            .try_take()
            .expect("resolve_next delivers synchronously")
            .expect("mock window creation cannot fail");
        assert!(
            Arc::ptr_eq(&resolved_a, &window_a),
            "resolve_next's own return value must be the SAME window it delivered \
             through the PendingWindow"
        );
        let window_a: Arc<dyn PlatformWindow> = window_a;

        let dispatcher_a =
            install_platform_realm(crate::app::ui_realm::UiRealm::for_test(), &window_a);
        window_a.on_close(Box::new(move || {
            close_this_window(dispatcher_a);
        }));

        // Window B: the real subject of this test. `open_secondary_window`
        // must accept the Pending arm and return `None` instead of either
        // erroring or assuming `Ready`.
        let opened = open_secondary_window_impl(
            AppConfig::default().with_frame_failure_detail(FrameFailureDetail::Redacted),
            WindowPolicy::SeparateRealms,
        )
        .expect("the Pending arm must be accepted, not treated as an error");
        assert!(
            opened.is_none(),
            "a Pending open must return None -- the install completes asynchronously, \
             not inline"
        );
        assert_eq!(
            APP_RUNTIME.with(|slot| slot.borrow().realms.iter().count()),
            1,
            "nothing must be installed for window B before its Pending open resolves"
        );

        // Resolve the deferred open -- delivers the mock window through
        // the ClaimSlot (waking the spawned task's Waker, which per
        // AsyncDriver's own contract requests a fresh frame) and hands
        // the SAME window handle back here, so this test can drive a
        // REAL platform-level close on it later -- the exact handle
        // `spawn_pending_secondary_window_completion`'s own async block
        // receives and threads into `finish_open_secondary_window`.
        let window_b = deferred
            .resolve_next()
            .expect("exactly one request (window B's) is pending at this point");

        // Drive the driver realm's (realm A's) own frame pump -- the same
        // mechanism a real frame tick uses to poll a woken async task to
        // completion, not a hand-rolled shortcut.
        dispatch_platform_realm(
            dispatcher_a,
            RealmTask::Frame(Box::new(|realm| {
                realm.scheduler().drive_async_tasks();
            })),
        )
        .expect("realm A dispatches its own frame pump");

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
            .expect(
                "the driver realm's frame pump must have completed window B's install -- \
                 if the Pending arm is ever silently dropped again, no second realm ever \
                 appears here",
            );
        assert_ne!(
            dispatcher_a.address.realm_id, dispatcher_b.address.realm_id,
            "WindowPolicy::SeparateRealms must install a genuinely distinct realm"
        );
        assert_eq!(
            APP_RUNTIME.with(|slot| {
                slot.borrow()
                    .realms
                    .get(&dispatcher_b.address.realm_id)
                    .and_then(|realm_slot| realm_slot.realm.as_ref())
                    .expect("pending completion installed the secondary realm")
                    .frame_failure_detail_for_test()
            }),
            FrameFailureDetail::Redacted,
            "the Pending arm must retain the requested detail policy until installation"
        );

        // Close B through a REAL platform close (`resolve_next`'s own
        // return value, the SAME handle `finish_open_secondary_window`
        // wired) -- exercises `finish_open_secondary_window`'s own
        // `on_close`/`notify_closed` bookkeeping exactly like a real OS
        // close event would, proving the Pending-resolved window is
        // wired identically to the Ready-arm path the two tests above
        // already cover, not merely present in the forest.
        window_b.close();
        assert_eq!(
            quit_calls.load(Ordering::SeqCst),
            0,
            "closing the secondary while the primary survives must not exit"
        );

        window_a.close();
        assert_eq!(
            APP_RUNTIME.with(|slot| slot.borrow().realms.iter().count()),
            0,
            "closing the last surviving realm must uninstall it"
        );
        assert_eq!(
            quit_calls.load(Ordering::SeqCst),
            1,
            "closing the LAST window must allow the exit exactly once"
        );

        Ok(())
    }));
    ready.expect("on_ready must not fail");

    teardown_platform_realm();
    drop(clear_guard);
}

/// The missing Pending-arm matrix cell: `WindowPolicy::SharedRealm` must
/// ALSO complete through the Pending arm, not just `SeparateRealms`
/// above — the harder case. `finish_open_secondary_window`'s
/// `SharedRealm` branch installs a PRESENTATION into the driver realm
/// itself, the exact realm `UpdateScheduler::drive_async_tasks` is called
/// from INSIDE a dispatch of (`dispatched_realm_id` is `Some` for the
/// whole poll) — and `install_presentation_alongside` has no
/// defer-to-idle queue of its own; it hard-refuses with
/// `InstallPresentationError::DispatchInFlight` while a dispatch is in
/// flight. `spawn_pending_secondary_window_completion`'s own
/// deferred-completion queue (drained only once the driver realm's
/// dispatch checkout clears, in `dispatch_platform_realm`'s own tail) is
/// what makes this arm succeed at all; reverting that deferral
/// (completing inline from inside the poll again, as an earlier
/// revision did) reproduces exactly the `DispatchInFlight`
/// refusal/panic this test exists to rule out.
#[test]
fn shared_realm_completes_through_the_pending_arm_like_the_real_winit_owner_lane() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_platform::traits::Platform;

    let clear_guard = OwnerHostClearGuard::arm();
    let platform = flui_platform::HeadlessPlatform::new();
    let deferred = platform.enable_deferred_window_open();
    let quit_calls = Arc::new(AtomicUsize::new(0));
    let quit_calls_for_on_ready = Arc::clone(&quit_calls);

    let ready = Box::new(platform).run(Box::new(move |owner| {
        install_owner_platform(owner).expect("install owner wake transport");
        install_exit_policy_hook(ExitPolicy::OnLastWindowClosed);

        let quit_calls_for_handler = Arc::clone(&quit_calls_for_on_ready);
        with_owner_platform(|owner| {
            owner.shared().on_quit(Box::new(move || {
                quit_calls_for_handler.fetch_add(1, Ordering::SeqCst);
            }));
        });

        // First window: also deferred (see the SeparateRealms probe
        // above for why) -- resolve it the same way real code resolving
        // a Pending open would.
        let mut pending_a = match with_owner_platform(|owner| {
            owner.open_window(flui_platform::WindowOptions::default())
        })
        .expect("BUG: install_owner_platform just ran above")
        .expect("deferred mode still accepts the request")
        {
            flui_platform::WindowOpen::Pending(pending) => pending,
            flui_platform::WindowOpen::Ready(_) => {
                panic!("deferred mode must return Pending, not Ready")
            }
        };
        let resolved_a = deferred
            .resolve_next()
            .expect("window A's request is the only one pending");
        let window_a = pending_a
            .try_take()
            .expect("resolve_next delivers synchronously")
            .expect("mock window creation cannot fail");
        assert!(
            Arc::ptr_eq(&resolved_a, &window_a),
            "resolve_next's own return value must be the SAME window it delivered \
             through the PendingWindow"
        );
        let window_a: Arc<dyn PlatformWindow> = window_a;

        let dispatcher_a =
            install_platform_realm(crate::app::ui_realm::UiRealm::for_test(), &window_a);
        window_a.on_close(Box::new(move || {
            close_this_window(dispatcher_a);
        }));

        // Window B: SharedRealm -- installs a second PRESENTATION of
        // realm A, never a second realm.
        let opened = open_secondary_window_impl(AppConfig::default(), WindowPolicy::SharedRealm)
            .expect("the Pending arm must be accepted, not treated as an error");
        assert!(
            opened.is_none(),
            "a Pending open must return None -- the install completes asynchronously, \
             not inline"
        );

        let presentation_count_before = APP_RUNTIME.with(|slot| {
            slot.borrow()
                .realms
                .get(&dispatcher_a.address.realm_id)
                .and_then(|realm_slot| realm_slot.realm.as_ref())
                .expect("realm A is resident")
                .presentation_count()
        });
        assert_eq!(
            presentation_count_before, 1,
            "nothing must be installed for window B before its Pending open resolves"
        );

        // Resolve the deferred open -- delivers the mock window through
        // the ClaimSlot and hands the SAME window handle back here, so
        // this test can drive a REAL platform-level close on it later.
        let window_b = deferred
            .resolve_next()
            .expect("exactly one request (window B's) is pending at this point");

        // Drive the driver realm's (realm A's) own frame pump. THIS is
        // the exact call that used to panic (`debug_assert!` in
        // `install_presentation_alongside`) / return `DispatchInFlight`
        // (release builds) before the completion was deferred to this
        // dispatch's own tail instead of running inline from inside the
        // poll.
        dispatch_platform_realm(
            dispatcher_a,
            RealmTask::Frame(Box::new(|realm| {
                realm.scheduler().drive_async_tasks();
            })),
        )
        .expect("realm A dispatches its own frame pump");

        assert_eq!(
            APP_RUNTIME.with(|slot| slot.borrow().realms.iter().count()),
            1,
            "SharedRealm must route into the SAME realm as the primary, never install a \
             second one"
        );
        let presentation_count_after = APP_RUNTIME.with(|slot| {
            slot.borrow()
                .realms
                .get(&dispatcher_a.address.realm_id)
                .and_then(|realm_slot| realm_slot.realm.as_ref())
                .expect("realm A is still resident")
                .presentation_count()
        });
        assert_eq!(
            presentation_count_after, 2,
            "the driver realm's own frame pump must have completed window B's presentation \
             install -- if the deferred-completion queue is ever bypassed and \
             finish_open_secondary_window runs directly from inside the poll again, this \
             assertion is never reached (DispatchInFlight panics/errors first)"
        );

        // Close B through a REAL platform close (`resolve_next`'s own
        // return value, the SAME handle `finish_open_secondary_window`
        // wired) -- must remove only itself, never the shared realm.
        window_b.close();
        let (realm_count, presentation_count) = APP_RUNTIME.with(|slot| {
            let state = slot.borrow();
            let realm_count = state.realms.iter().count();
            let presentation_count = state
                .realms
                .get(&dispatcher_a.address.realm_id)
                .and_then(|realm_slot| realm_slot.realm.as_ref())
                .expect(
                    "the shared realm must still be resident -- B's own close must not \
                     have torn down the whole realm out from under the still-open primary",
                )
                .presentation_count();
            (realm_count, presentation_count)
        });
        assert_eq!(
            realm_count, 1,
            "the shared realm must survive B's own close"
        );
        assert_eq!(
            presentation_count, 1,
            "only B's own presentation must be removed; the primary's survives"
        );
        assert_eq!(
            quit_calls.load(Ordering::SeqCst),
            0,
            "the realm (and its primary presentation) still lives -- this must not exit"
        );

        window_a.close();
        assert_eq!(
            APP_RUNTIME.with(|slot| slot.borrow().realms.iter().count()),
            0,
            "closing the realm's last surviving presentation must uninstall the whole realm"
        );
        assert_eq!(
            quit_calls.load(Ordering::SeqCst),
            1,
            "closing the last presentation of the last realm must allow the exit exactly \
             once"
        );

        Ok(())
    }));
    ready.expect("on_ready must not fail");

    teardown_platform_realm();
    drop(clear_guard);
}

/// An accepted separate-realm request survives its originating window and
/// completes on a window-independent owner turn after the worker resolves it.
#[test]
fn pending_open_survives_origin_realm_close_and_worker_resolution() {
    use flui_platform::traits::Platform;

    let clear_guard = OwnerHostClearGuard::arm();
    let platform = flui_platform::HeadlessPlatform::new();
    let deferred = Arc::new(platform.enable_deferred_window_open());
    let turns = platform.owner_turns();
    let worker_deferred = Arc::clone(&deferred);

    let ready = Box::new(platform).run(Box::new(move |owner| {
        install_owner_platform(owner).expect("install owner wake transport");

        let mut pending_a = match with_owner_platform(|owner| {
            owner.open_window(flui_platform::WindowOptions::default())
        })
        .expect("BUG: install_owner_platform just ran above")
        .expect("deferred mode still accepts the request")
        {
            flui_platform::WindowOpen::Pending(pending) => pending,
            flui_platform::WindowOpen::Ready(_) => {
                panic!("deferred mode must return Pending, not Ready")
            }
        };
        let resolved_a = deferred
            .resolve_next()
            .expect("window A's request is the only one pending");
        let window_a = pending_a
            .try_take()
            .expect("resolve_next delivers synchronously")
            .expect("mock window creation cannot fail");
        assert!(Arc::ptr_eq(&resolved_a, &window_a));
        let window_a: Arc<dyn PlatformWindow> = window_a;

        let dispatcher_a =
            install_platform_realm(crate::app::ui_realm::UiRealm::for_test(), &window_a);
        window_a.on_close(Box::new(move || {
            close_this_window(dispatcher_a);
        }));

        // The accepted request belongs to the loop, not realm A.
        let opened = open_secondary_window_impl(AppConfig::default(), WindowPolicy::SeparateRealms)
            .expect("the Pending arm must be accepted, not treated as an error");
        assert!(opened.is_none());

        window_a.close();
        assert_eq!(
            APP_RUNTIME.with(|slot| slot.borrow().realms.iter().count()),
            0,
            "the driver realm must be gone before window B's open ever resolves"
        );

        Ok(())
    }));
    ready.expect("on_ready must not fail");
    turns.drive();
    let resolved =
        std::thread::spawn(move || worker_deferred.resolve_next().expect("accepted request"))
            .join()
            .expect("worker");
    assert_eq!(
        APP_RUNTIME.with(|slot| slot.borrow().realms.iter().count()),
        0
    );
    turns.drive();
    assert_eq!(
        APP_RUNTIME.with(|slot| slot.borrow().realms.iter().count()),
        1
    );
    assert_eq!(
        APP_RUNTIME.with(|slot| slot
            .borrow()
            .pending_window_reservations
            .load(std::sync::atomic::Ordering::Acquire)),
        0
    );
    resolved.close();
    teardown_platform_realm();
    drop(clear_guard);
}

/// Closing a realm's SOLE presentation must dispatch `AppLifecycleState::
/// Detached` through it BEFORE the realm is uninstalled — shutdown must
/// cancel any in-flight pointer sequence and notify lifecycle observers
/// before the realm and its `UpdateScheduler` are gone (the same reason
/// `on_quit`'s own Detached dispatch exists, generalized to per-realm
/// teardown so it fires from an ordinary window close too, not only a
/// process-wide quit). Observed via an `UpdateScheduler` lifecycle listener
/// registered BEFORE the close: the listener's own `Arc` survives the
/// realm's eventual drop, so it remains checkable after `close_this_
/// window` returns even though the realm itself is gone by then.
#[test]
fn closing_the_sole_presentation_dispatches_detached_before_uninstalling() {
    use std::sync::Mutex;

    let (dispatcher, _clear_guard) = install_realm_a_through_a_real_owner_platform();

    let observed_states: Arc<Mutex<Vec<AppLifecycleState>>> = Arc::new(Mutex::new(Vec::new()));
    let observed_states_for_listener = Arc::clone(&observed_states);
    dispatch_platform_realm(
        dispatcher,
        RealmTask::Frame(Box::new(move |realm| {
            realm
                .scheduler()
                .add_lifecycle_state_listener(Arc::new(move |state| {
                    observed_states_for_listener
                        .lock()
                        .expect("test-local mutex is never poisoned")
                        .push(state);
                }));
        })),
    )
    .expect("registering the listener dispatches");

    close_this_window(dispatcher);

    assert_eq!(
        APP_RUNTIME.with(|slot| slot.borrow().realms.iter().count()),
        0,
        "closing the sole presentation must have uninstalled the whole realm"
    );
    assert_eq!(
        observed_states
            .lock()
            .expect("test-local mutex is never poisoned")
            .last(),
        Some(&AppLifecycleState::Detached),
        "the realm's own scheduler must have observed a transition to Detached before it \
         was torn down -- shutdown must cancel in-flight pointer sequences and notify \
         lifecycle observers before the realm is gone, not skip that step silently"
    );

    teardown_platform_realm();
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
#[test]
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

/// The drain-before-decide rule: closing the splash realm and installing
/// the main realm in the very same batch must never exit the loop --
/// the surviving main realm keeps it alive. Drives the deferral through
/// the REAL seam, `for_each_installed_realm`'s visit (hot-restart's own
/// primitive), rather than hand-poking `iterating_all_realms` -- a
/// splash's own dispose callback requesting the main window mid-visit is
/// exactly this shape.
#[test]
fn close_splash_then_open_main_does_not_exit() {
    let splash = install_test_realm();
    let splash_id = splash.address.realm_id;

    let platform = flui_platform::headless_platform();
    let main_window: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create the main window");
    let main_realm = crate::app::ui_realm::UiRealm::for_test();
    let main_id = main_realm.realm_id();
    let mut main_realm = Some(main_realm);

    for_each_installed_realm(|realm| {
        assert_eq!(
            realm.realm_id(),
            splash_id,
            "only the splash realm is resident at visit time"
        );
        uninstall_platform_realm(splash_id);
        let installed = install_realm_alongside(
            main_realm.take().expect("visited exactly once"),
            &main_window,
        );
        assert!(
            installed.is_ok(),
            "install_realm_alongside must defer mid-visit, not fail"
        );

        // Mid-visit: both requests just made are still queued (deferred
        // by `iterating_all_realms`), so the splash realm's own slot is
        // still resident (only marked for pending removal) and
        // `should_exit` must not report exit.
        let (exit, removed) = APP_RUNTIME.with(|slot| {
            slot.borrow_mut()
                .should_exit(ExitPolicy::OnLastWindowClosed)
        });
        drop(removed);
        assert!(
            !exit,
            "should_exit must not report exit while this visit is still in flight"
        );
    });

    // Both deferred mutations land once the whole visit completes (its
    // own tail drain).
    assert!(
        APP_RUNTIME.with(|slot| slot.borrow().realms.get(&splash_id).is_none()),
        "the deferred splash uninstall must have applied once the visit completed"
    );
    assert!(
        APP_RUNTIME.with(|slot| slot.borrow().realms.get(&main_id).is_some()),
        "the deferred main-window install must have applied once the visit completed"
    );

    let should_exit = APP_RUNTIME.with(|slot| {
        let (exit, removed) = slot
            .borrow_mut()
            .should_exit(ExitPolicy::OnLastWindowClosed);
        drop(removed);
        exit
    });
    assert!(
        !should_exit,
        "the drain-before-decide rule: closing the splash and installing the main realm in \
         the same pass must never exit the loop -- the surviving main realm keeps it alive"
    );

    teardown_platform_realm();
}

/// A frame callback that decides to open a second window must install
/// the second realm without ever attempting a nested dispatch: the
/// install is invisible in the registry until realm A's own dispatch
/// restores, at which point it lands.
#[test]
fn frame_callback_opening_a_window_installs_second_realm_without_nested_dispatch() {
    // Both windows from ONE shared platform instance -- see
    // `install_two_test_realms`'s doc for why two separate
    // `test_window()` calls would risk an aliased id here.
    let platform = flui_platform::headless_platform();
    let window_a: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create window a");
    let dispatcher_a = install_platform_realm(crate::app::ui_realm::UiRealm::for_test(), &window_a);

    let second_realm = crate::app::ui_realm::UiRealm::for_test();
    let second_realm_id = second_realm.realm_id();
    let second_window: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create the second window");
    let mut second_realm = Some(second_realm);

    dispatch_platform_realm(
        dispatcher_a,
        RealmTask::Frame(Box::new(move |_realm| {
            let installed = install_realm_alongside(
                second_realm.take().expect("dispatched exactly once"),
                &second_window,
            );
            assert!(
                installed.is_ok(),
                "install_realm_alongside must defer mid-dispatch (Ok, not yet visible), \
                 never fail"
            );
            assert!(
                APP_RUNTIME.with(|slot| slot.borrow().realms.get(&second_realm_id).is_none()),
                "the second realm must not be visible in the registry while realm A is \
                 still checked out for dispatch"
            );
        })),
    )
    .expect("A's frame callback dispatches");

    assert!(
        APP_RUNTIME.with(|slot| slot.borrow().realms.get(&second_realm_id).is_some()),
        "the deferred install must land once realm A's own dispatch restores"
    );

    teardown_platform_realm();
}

/// Hot-restart's `for_each_installed_realm` visits every hosted realm in
/// mount order; a realm-map mutation a visited realm's own callback
/// requests must defer until the WHOLE visit completes, never change the
/// set of realms still being walked.
#[test]
fn hot_restart_while_callback_mutates_realm_map_defers_mutation() {
    let (dispatcher_a, dispatcher_b) = install_two_test_realms();
    let realm_a_id = dispatcher_a.address.realm_id;
    let realm_b_id = dispatcher_b.address.realm_id;

    let visited = Rc::new(RefCell::new(Vec::new()));
    let visited_in_closure = Rc::clone(&visited);

    for_each_installed_realm(move |realm| {
        visited_in_closure.borrow_mut().push(realm.realm_id());
        if realm.realm_id() == realm_a_id {
            uninstall_platform_realm(realm_a_id);
            assert!(
                APP_RUNTIME.with(|slot| slot.borrow().realms.get(&realm_a_id).is_some()),
                "a mutation requested mid-visit must defer, not remove the realm immediately"
            );
        }
    });

    assert_eq!(
        *visited.borrow(),
        vec![realm_a_id, realm_b_id],
        "the visit must walk every realm in mount order, unaffected by the mid-visit \
         mutation request"
    );
    assert!(
        APP_RUNTIME.with(|slot| slot.borrow().realms.get(&realm_a_id).is_none()),
        "the deferred uninstall must apply once the whole visit completes"
    );
    assert!(
        APP_RUNTIME.with(|slot| slot.borrow().realms.get(&realm_b_id).is_some()),
        "realm B must be untouched by realm A's own deferred mutation"
    );

    teardown_platform_realm();
}

/// `should_exit` called from mid-visit must not drain a mutation the
/// SAME visit just deferred. Before this guard existed,
/// `drain_pending_realm_mutations` drained unconditionally, so this call
/// would have applied the visited realm's own queued self-uninstall
/// immediately -- removing its slot from the registry while its
/// `UiRealm` is still checked out here, held by `for_each_installed_realm`'s
/// loop, about to be restored into a slot that would no longer exist.
#[test]
fn should_exit_mid_visit_does_not_prematurely_drain_the_visited_realms_own_pending_uninstall() {
    let dispatcher = install_test_realm();
    let realm_id = dispatcher.address.realm_id;

    for_each_installed_realm(|realm| {
        assert_eq!(
            realm.realm_id(),
            realm_id,
            "the sole installed realm is the one visited"
        );
        uninstall_platform_realm(realm_id);

        let (exit, removed) = APP_RUNTIME.with(|slot| {
            slot.borrow_mut()
                .should_exit(ExitPolicy::OnLastWindowClosed)
        });
        drop(removed);
        assert!(
            !exit,
            "the registry is not empty: this realm's slot is still resident here, only its \
             checked-out UiRealm is held externally by this visit"
        );
        assert!(
            APP_RUNTIME.with(|slot| slot.borrow().realms.get(&realm_id).is_some()),
            "should_exit called mid-visit must not drain a mutation the SAME visit just \
             deferred -- draining it now would remove this realm's slot while its UiRealm \
             is still checked out here, and the visit's own restore step would then \
             silently drop it instead of the queued uninstall ever running its own \
             registry cleanup"
        );
    });

    assert!(
        APP_RUNTIME.with(|slot| slot.borrow().realms.get(&realm_id).is_none()),
        "the deferred uninstall must still apply once the whole visit completes"
    );

    teardown_platform_realm();
}

/// A panicking visitor (e.g. hot-restart's reassemble running arbitrary
/// user build code) must not permanently strand the checked-out realm
/// at `realm: None`, nor wedge `iterating_all_realms` at `true` forever
/// -- both of which would (after the fix above) silently defer every
/// future realm-map mutation request for the rest of the process.
#[test]
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

/// Fence-(c) two-realm probe: with realm A mid-frame-transaction and a
/// SECOND, fully idle realm B also resident, `with_owner_platform`'s
/// debug_assert must still trip. A two-realm registry is not an "OR of
/// all Idle" check that a lone idle sibling could vacuously satisfy —
/// this must actually catch the realm that IS mid-transaction.
#[test]
#[should_panic(
    expected = "with_owner_platform called while the installed realm's scheduler is inside"
)]
#[cfg_attr(
    not(debug_assertions),
    ignore = "the fence is a debug_assert!; release builds don't panic"
)]
fn owner_platform_accessor_fences_correctly_with_a_second_idle_realm_present() {
    use flui_platform::headless_platform;

    let _clear_guard = OwnerHostClearGuard::arm();
    // Both windows from ONE shared platform instance: `HeadlessPlatform`
    // mints window ids from an instance-local counter, so two separate
    // `headless_platform()` calls could alias the same id, which would
    // silently displace realm A's window mapping the moment realm B's
    // window registers.
    let platform = headless_platform();
    // The IDLE sibling installs FIRST (`install_platform_realm`, this
    // test's legacy-primary slot), the mid-transaction realm SECOND
    // (`install_realm_alongside`) -- deliberately the opposite of mount
    // order elsewhere in this file. `installed_realm_phase` falls back to
    // `phases.first()` when nothing scans as forbidden; installing the
    // idle realm first means that fallback alone would return `Idle` and
    // this test would pass FALSELY (the debug_assert would never trip,
    // failing `#[should_panic]`) if the `.find(is_frame_transaction_phase)`
    // scan were ever deleted. Only the real scan, checking every
    // resident slot rather than assuming the first one is representative,
    // makes this test kill that mutant.
    let window_b: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create window b");
    // Realm B is installed and then never touched again for the rest of
    // this test -- it stays resident and Idle throughout.
    let _dispatcher_b =
        install_platform_realm(crate::app::ui_realm::UiRealm::for_test(), &window_b);
    let window_a: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create window a");
    let dispatcher_a =
        install_realm_alongside(crate::app::ui_realm::UiRealm::for_test(), &window_a)
            .expect("realm A installs alongside the idle realm B cleanly");

    let (scheduler_a, local_post_frame_a) = APP_RUNTIME.with(|slot| {
        let borrowed = slot.borrow();
        let realm = borrowed
            .realms
            .get(&dispatcher_a.address.realm_id)
            .and_then(|realm_slot| realm_slot.realm.as_ref())
            .expect("realm A installed above");
        (
            realm.scheduler().clone(),
            realm.local_post_frame_lane().clone(),
        )
    });

    let now = web_time::Instant::now();
    scheduler_a.drive_frame_with_lane(
        now,
        flui_scheduler::IdleDeadline::far_future(now),
        || {
            let _ = with_owner_platform(|_owner| ());
        },
        &local_post_frame_a,
    );
}

/// The presentation-close dispatch seam (`close_presentation` ->
/// `dispatch_platform_realm` -> the special-cased `RealmTask::
/// ClosePresentation` branch -> `UiRealm::close_presentation_entered`)
/// runs presentation A's teardown with the exact same
/// `dispatched_realm_id` TLS state any other dispatched frame/event
/// callback runs under. One dispose hook on A proves both halves of
/// that claim:
///
/// 1. it can RESOLVE a `GlobalKey` registered in SIBLING presentation
///    B — the whole-frame composite `close_presentation_entered`'s own
///    `self.enter(...)` activates for its step 2–3 phase spans every
///    presentation still in the forest, not just the one being closed
///    (the same fact `whole_frame_event_keeps_realm_global_key_scope_active`
///    proves for an ordinary dispatched frame);
/// 2. an `install_realm_alongside` call it makes must DEFER — not
///    apply immediately, not reenter `APP_RUNTIME` — exactly the
///    discipline `frame_callback_opening_a_window_installs_second_realm_without_nested_dispatch`
///    proves for an ordinary dispatched frame callback.
///
/// Red-check performed by hand while writing this test (not left in the
/// tree): replacing the `close_presentation(dispatcher, a_id)` call
/// below with a direct `realm.close_presentation_entered(a_id)` reached
/// through a raw `APP_RUNTIME` borrow-and-take (bypassing
/// `dispatch_platform_realm` entirely, so `dispatched_realm_id` stays
/// `None` for the whole teardown) makes the `install_deferred`
/// assertion below fail: with no dispatch in flight,
/// `install_realm_alongside` applies the new realm IMMEDIATELY instead
/// of deferring, so this test observes it already installed at the
/// point it checks — proving the assertion is genuinely pinned to the
/// TLS deferral guard, not vacuously green regardless of which path
/// teardown takes.
#[test]
fn dispose_opening_a_window_mid_teardown_defers_and_does_not_reenter() {
    #[derive(Clone)]
    struct OpenWindowOnDisposeView {
        key_in_sibling: flui_view::GlobalKey<()>,
        new_window: std::sync::Arc<dyn flui_platform::traits::PlatformWindow>,
        resolved_sibling_element: Rc<Cell<Option<flui_foundation::ElementId>>>,
        install_deferred: Rc<Cell<Option<bool>>>,
        opened_realm_id: Rc<Cell<Option<flui_foundation::RealmId>>>,
    }

    struct OpenWindowOnDisposeState {
        key_in_sibling: flui_view::GlobalKey<()>,
        new_window: std::sync::Arc<dyn flui_platform::traits::PlatformWindow>,
        resolved_sibling_element: Rc<Cell<Option<flui_foundation::ElementId>>>,
        install_deferred: Rc<Cell<Option<bool>>>,
        opened_realm_id: Rc<Cell<Option<flui_foundation::RealmId>>>,
    }

    impl flui_view::StatefulView for OpenWindowOnDisposeView {
        type State = OpenWindowOnDisposeState;

        fn create_state(&self) -> Self::State {
            OpenWindowOnDisposeState {
                key_in_sibling: self.key_in_sibling.clone(),
                new_window: std::sync::Arc::clone(&self.new_window),
                resolved_sibling_element: Rc::clone(&self.resolved_sibling_element),
                install_deferred: Rc::clone(&self.install_deferred),
                opened_realm_id: Rc::clone(&self.opened_realm_id),
            }
        }
    }

    impl flui_view::ViewState<OpenWindowOnDisposeView> for OpenWindowOnDisposeState {
        fn build(
            &self,
            _view: &OpenWindowOnDisposeView,
            _ctx: &dyn flui_view::BuildContext,
        ) -> impl flui_view::IntoView {
            flui_widgets::SizedBox::new(0.0, 0.0)
        }

        fn dispose(&mut self) {
            // (1) Resolve a GlobalKey registered in the SIBLING
            // presentation B: only possible if the whole-frame composite
            // is still active for this dispose call, spanning every
            // presentation the realm hosts. This presentation's own
            // registry, whose binding lock the teardown walk holds,
            // reports itself busy and is skipped rather than re-entered
            // (`flui-view`'s `key::registry`, "Re-entrancy").
            self.resolved_sibling_element
                .set(self.key_in_sibling.current_element());

            // (2) Attempt to open a second realm/window mid-teardown.
            // Production contract: this must defer to loop idle, the
            // same as any other dispatched callback's install request.
            // `new_window` was pre-opened from the SAME shared platform
            // instance realm A's own window came from -- a second,
            // independently-constructed `HeadlessPlatform` here would
            // mint an aliasing window id from its own zeroed counter,
            // which `WindowRegistry` would then read as a collision
            // against A's window rather than a genuinely distinct one.
            let new_realm = crate::app::ui_realm::UiRealm::for_test();
            let new_realm_id = new_realm.realm_id();
            let installed = install_realm_alongside(new_realm, &self.new_window);
            assert!(
                installed.is_ok(),
                "install_realm_alongside must defer mid-teardown (Ok, not yet visible), \
                 never fail"
            );
            self.opened_realm_id.set(Some(new_realm_id));
            let visible_immediately =
                APP_RUNTIME.with(|slot| slot.borrow().realms.get(&new_realm_id).is_some());
            self.install_deferred.set(Some(!visible_immediately));
        }
    }

    impl View for OpenWindowOnDisposeView {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateful(self)
        }
    }

    // One shared platform instance for every window -- see
    // `install_two_test_realms`'s doc for why two independent
    // `headless_platform()` calls would risk an aliased window id.
    let platform = flui_platform::headless_platform();
    let window_a: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create window a");
    let window_b: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create window b");
    let window_for_new_realm: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create the second window");

    let realm = crate::app::ui_realm::UiRealm::for_test();
    let a_id = realm.presentation_id();

    let key_in_sibling = flui_view::GlobalKey::<()>::new();
    let element_in_sibling = flui_foundation::ElementId::new(1);

    let resolved_sibling_element = Rc::new(Cell::new(None));
    let install_deferred = Rc::new(Cell::new(None));
    let opened_realm_id = Rc::new(Cell::new(None));
    let probe = OpenWindowOnDisposeView {
        key_in_sibling: key_in_sibling.clone(),
        new_window: window_for_new_realm,
        resolved_sibling_element: Rc::clone(&resolved_sibling_element),
        install_deferred: Rc::clone(&install_deferred),
        opened_realm_id: Rc::clone(&opened_realm_id),
    };
    realm
        .enter(|realm| realm.attach_root_widget(&probe))
        .expect("A mounts the probe");
    // `attach_root_widget` only SCHEDULES the initial build; the probe's
    // concrete element (and therefore its `State`) is not actually
    // constructed until a frame runs it, exactly the same reason
    // `dispose_during_teardown_cannot_reach_sibling_or_dead_services`
    // and `closing_presentation_a_leaves_sibling_layer_tree_identical`
    // both draw a frame between mounting and closing.
    let _ = realm.draw_frame(flui_rendering::constraints::BoxConstraints::tight(
        flui_types::Size::new(
            flui_types::geometry::px(20.0),
            flui_types::geometry::px(20.0),
        ),
    ));

    let dispatcher = install_platform_realm(realm, &window_a);
    // B installs alongside A through the REAL production entry point
    // (issue #555's addressed-routing slice), with its own genuine
    // `WindowRegistry` mapping -- not the `cfg(test)`-only
    // `install_second_presentation_for_test` bypass this test used
    // before, which left B with no mapping of its own and is why
    // `teardown_platform_realm()` used to have to be skipped here.
    let b_dispatcher = install_presentation_alongside(dispatcher, &window_b)
        .expect("B installs alongside A with a real window mapping");
    let b_id = b_dispatcher.address.presentation_id;

    // Register the sibling key into B directly: no dispatch is in
    // flight yet (registration itself needs no active composite; only
    // resolution via `GlobalKey::current_element` does, which is what
    // A's dispose hook below exercises).
    APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        let realm_slot = state
            .realms
            .get(&dispatcher.address.realm_id)
            .expect("installed above");
        let realm = realm_slot.realm.as_ref().expect("not checked out yet");
        realm
            .presentation_widgets_for_test(b_id)
            .with_build_owner_mut(|owner| {
                owner.register_global_key(&key_in_sibling, element_in_sibling);
            });
    });

    close_presentation(dispatcher, a_id).expect("A closes through the real dispatch seam");

    assert_eq!(
        resolved_sibling_element.get(),
        Some(element_in_sibling),
        "A's dispose hook must resolve B's GlobalKey through the whole-frame composite \
         active during close_presentation_entered's step 2-3 phase"
    );
    assert_eq!(
        install_deferred.get(),
        Some(true),
        "A's dispose-time install_realm_alongside request must defer -- not yet visible \
         in the registry -- while A's own realm is still checked out for this dispatch"
    );

    let opened_realm_id = opened_realm_id
        .get()
        .expect("dispose must have attempted the install");
    assert!(
        APP_RUNTIME.with(|slot| slot.borrow().realms.get(&opened_realm_id).is_some()),
        "the deferred install must land once A's close dispatch restores"
    );

    // B now has a real `WindowRegistry` mapping of its own (minted by
    // `install_presentation_alongside` above), so after A closes and the
    // surviving realm's tracked address re-stamps to B, teardown's own
    // self-check ("every live realm's tracked address resolves to a
    // registered window") holds again -- the still-open design-for-N gap
    // this test used to skip teardown for is closed.
    teardown_platform_realm();
}

/// Closing presentation A (a sibling, B, stays open) must unregister
/// EXACTLY A's own window mapping -- and a caller still holding a
/// `RealmDispatcher` minted while A was live must be refused, never
/// silently delivered to B (the surviving primary) instead. Before the
/// `ClosePresentation` handling did both the window-registry removal
/// AND the `RealmSlot::address` re-stamp, a stale dispatcher would still
/// compare equal at the top of `dispatch_platform_realm` (the slot's
/// tracked `presentation_id` was never updated past A's original
/// install-time value) and the task would run against `self.
/// presentations.primary()` -- B, after A's removal shifts it into that
/// slot -- exactly the sibling-reach bug this test guards.
#[test]
fn closing_a_presentation_unregisters_its_window_mapping() {
    let platform = flui_platform::headless_platform();
    let window_a: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create window a");
    let window_b: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create window b");

    let realm = crate::app::ui_realm::UiRealm::for_test();
    let a_id = realm.presentation_id();

    let dispatcher = install_platform_realm(realm, &window_a);
    let realm_id = dispatcher.address.realm_id;
    // B installs alongside A through the real production entry point
    // (issue #555's addressed-routing slice), with its own genuine `WindowRegistry` mapping.
    let b_dispatcher = install_presentation_alongside(dispatcher, &window_b)
        .expect("B installs alongside A with a real window mapping");
    let b_id = b_dispatcher.address.presentation_id;

    assert_eq!(
        APP_RUNTIME.with(|slot| slot.borrow().registry.resolve(window_a.id())),
        Some(flui_foundation::PresentationAddress {
            realm_id,
            presentation_id: a_id,
        }),
        "precondition: A's window starts mapped to A's own address"
    );
    assert_eq!(
        APP_RUNTIME.with(|slot| slot.borrow().registry.resolve(window_b.id())),
        Some(b_dispatcher.address),
        "precondition: B's window starts mapped to B's own address"
    );

    close_presentation(dispatcher, a_id).expect("A closes through the real dispatch seam");

    assert_eq!(
        APP_RUNTIME.with(|slot| slot.borrow().registry.resolve(window_a.id())),
        None,
        "closing A must unregister its own window mapping -- leaving it mapped would let \
         a stale platform event for that exact window keep resolving to A's now-dead \
         PresentationId"
    );
    assert_eq!(
        APP_RUNTIME.with(|slot| slot.borrow().registry.resolve(window_b.id())),
        Some(b_dispatcher.address),
        "closing A must never touch B's own window mapping -- hop-1 demux is per-window, \
         not per-realm"
    );

    let delivered = Rc::new(Cell::new(false));
    let delivered_in_task = Rc::clone(&delivered);
    let result = dispatch_platform_realm(
        dispatcher,
        RealmTask::Frame(Box::new(move |_realm| {
            delivered_in_task.set(true);
        })),
    );
    assert!(
        matches!(result, Err(RealmDispatchError::StalePresentation)),
        "a dispatcher minted for the now-closed presentation must be refused as stale, \
         got {result:?}"
    );
    assert!(
        !delivered.get(),
        "the stale task must never run -- delivering it would land on B (the surviving \
         primary), exactly the sibling-reach bug this test guards"
    );

    // B has a real `WindowRegistry` mapping of its own (minted by
    // `install_presentation_alongside` above), so teardown's own
    // self-check ("every live realm's tracked address resolves to a
    // registered window") holds even after A closes and the surviving
    // realm's tracked address re-stamps to B -- the still-open
    // design-for-N gap this test used to skip teardown for is closed.
    teardown_platform_realm();
    let _ = b_id;
}

/// A dispose hook that re-enters `dispatch_platform_realm` DURING the
/// very close it is being disposed by -- using the dying presentation's
/// OWN dispatcher -- must be refused as stale
/// (`RealmDispatchError::StalePresentation`), never silently ACCEPTED
/// and enqueued to run later against whatever survives the close.
///
/// Ordering-critical: `RealmSlot::address` must re-stamp to the
/// surviving primary BEFORE this presentation's own dispose hooks run
/// (`close_presentation_entered`'s step 2-3), not after. Re-stamping
/// only after disposal would leave a window where a dispose-time
/// reentrant dispatch bearing `id` still compares EQUAL against the
/// not-yet-updated address, passes the `StalePresentation` check, and
/// gets enqueued behind the same-realm reentrancy guard (`realm_slot.
/// draining` is `true` for the whole checkout) instead of refused --
/// only to run a moment later, in this very drain loop, against
/// whichever presentation survives: a task genuinely executing against
/// the wrong presentation, not merely an error a caller has to notice.
#[test]
fn dispose_time_reentrant_dispatch_with_the_dying_presentations_own_dispatcher_is_refused() {
    #[derive(Clone)]
    struct ReenterOnDisposeView {
        dispatcher: Rc<Cell<Option<RealmDispatcher>>>,
        reentrant_result: Rc<Cell<Option<Result<(), RealmDispatchError>>>>,
        reentrant_task_ran: Rc<Cell<bool>>,
    }

    struct ReenterOnDisposeState {
        dispatcher: Rc<Cell<Option<RealmDispatcher>>>,
        reentrant_result: Rc<Cell<Option<Result<(), RealmDispatchError>>>>,
        reentrant_task_ran: Rc<Cell<bool>>,
    }

    impl flui_view::StatefulView for ReenterOnDisposeView {
        type State = ReenterOnDisposeState;

        fn create_state(&self) -> Self::State {
            ReenterOnDisposeState {
                dispatcher: Rc::clone(&self.dispatcher),
                reentrant_result: Rc::clone(&self.reentrant_result),
                reentrant_task_ran: Rc::clone(&self.reentrant_task_ran),
            }
        }
    }

    impl flui_view::ViewState<ReenterOnDisposeView> for ReenterOnDisposeState {
        fn build(
            &self,
            _view: &ReenterOnDisposeView,
            _ctx: &dyn flui_view::BuildContext,
        ) -> impl flui_view::IntoView {
            flui_widgets::SizedBox::new(0.0, 0.0)
        }

        fn dispose(&mut self) {
            let dispatcher = self
                .dispatcher
                .get()
                .expect("dispatcher stashed before A's close was requested");
            let ran = Rc::clone(&self.reentrant_task_ran);
            let result = dispatch_platform_realm(
                dispatcher,
                RealmTask::Frame(Box::new(move |_realm| {
                    ran.set(true);
                })),
            );
            self.reentrant_result.set(Some(result));
        }
    }

    impl View for ReenterOnDisposeView {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateful(self)
        }
    }

    // One shared platform instance for both windows -- see
    // `install_two_test_realms`'s doc for why two independent
    // `headless_platform()` calls would risk an aliased window id.
    let platform = flui_platform::headless_platform();
    let window_a: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create window a");
    let window_b: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create window b");

    let realm = crate::app::ui_realm::UiRealm::for_test();
    let a_id = realm.presentation_id();

    let dispatcher_slot = Rc::new(Cell::new(None));
    let reentrant_result = Rc::new(Cell::new(None));
    let reentrant_task_ran = Rc::new(Cell::new(false));
    let probe = ReenterOnDisposeView {
        dispatcher: Rc::clone(&dispatcher_slot),
        reentrant_result: Rc::clone(&reentrant_result),
        reentrant_task_ran: Rc::clone(&reentrant_task_ran),
    };
    realm
        .enter(|realm| realm.attach_root_widget(&probe))
        .expect("A mounts the probe");
    // `attach_root_widget` only SCHEDULES the initial build -- see
    // `dispose_opening_a_window_mid_teardown_defers_and_does_not_reenter`'s
    // own comment for why a real frame must run before the probe's
    // `State`, and therefore its `dispose()`, actually exists.
    let _ = realm.draw_frame(flui_rendering::constraints::BoxConstraints::tight(
        flui_types::Size::new(
            flui_types::geometry::px(20.0),
            flui_types::geometry::px(20.0),
        ),
    ));

    let dispatcher = install_platform_realm(realm, &window_a);
    // B installs alongside A through the real production entry point
    // (issue #555's addressed-routing slice), with its own genuine `WindowRegistry` mapping.
    let _b_dispatcher = install_presentation_alongside(dispatcher, &window_b)
        .expect("B installs alongside A with a real window mapping");
    dispatcher_slot.set(Some(dispatcher));

    close_presentation(dispatcher, a_id).expect("A closes through the real dispatch seam");

    assert!(
        matches!(
            reentrant_result.get(),
            Some(Err(RealmDispatchError::StalePresentation))
        ),
        "a dispose-time re-dispatch bearing the dying presentation's OWN dispatcher must \
         be refused as stale, got {:?}",
        reentrant_result.get()
    );
    assert!(
        !reentrant_task_ran.get(),
        "the reentrant task must never run -- accepting it (even merely queued for later) \
         means it eventually executes against whatever survives the close, misaddressed \
         rather than refused"
    );

    // B has a real `WindowRegistry` mapping of its own (minted by
    // `install_presentation_alongside` above), so teardown's own
    // self-check holds even after A closes and the surviving realm's
    // tracked address re-stamps to B -- see
    // `closing_a_presentation_unregisters_its_window_mapping`'s closing
    // comment for the full explanation of the gap this closes.
    teardown_platform_realm();
}

/// Closing a realm's SOLE presentation must not leave a live realm with
/// an empty `PresentationForest` (the next `primary()` call would panic
/// `"BUG: PresentationForest is never empty"`). `RealmTask::
/// ClosePresentation`'s handling routes this case into a full realm
/// uninstall instead of ever calling `close_presentation_entered` --
/// proven here by asserting the WHOLE realm disappears from the
/// registry, not just the one presentation.
#[test]
fn closing_the_sole_presentation_uninstalls_the_whole_realm() {
    let dispatcher = install_test_realm();
    let realm_id = dispatcher.address.realm_id;
    let a_id = dispatcher.address.presentation_id;

    assert!(
        APP_RUNTIME.with(|slot| slot.borrow().realms.get(&realm_id).is_some()),
        "precondition: the realm is installed before closing its sole presentation"
    );

    close_presentation(dispatcher, a_id).expect("the close request is accepted");

    assert!(
        APP_RUNTIME.with(|slot| slot.borrow().realms.get(&realm_id).is_none()),
        "closing the realm's only presentation must uninstall the WHOLE realm -- a \
         live realm with an empty forest is not a reachable state this codebase permits"
    );

    // Harmless no-op cleanup (the realm is already gone) -- matches
    // every other test in this module for the same TLS-state hygiene.
    teardown_platform_realm();
}

/// `FocusCoordinator`'s two write-side behaviors, end to end through the
/// REAL production path (`dispatch_platform_realm` running a genuine
/// `RealmTask::Event(PlatformToUi::WindowFocus(_))` task, not a direct
/// `notify_presentation_focus_gained` call): `WindowFocus(true)`
/// dispatched for a presentation moves the active presentation to it;
/// `WindowFocus(false)` dispatched for a DIFFERENT, non-active
/// presentation never moves it away from whichever presentation is
/// currently active, and must NOT suspend the realm-wide lifecycle
/// either (a sibling presentation still holds focus). The FINAL step
/// pins the other half of that same guard: `WindowFocus(false)`
/// dispatched for the presentation that IS currently active must still
/// suspend the realm normally -- the guard drops only NON-active
/// focus-loss, never focus-loss in general.
///
/// Mutants, both confirmed to make this fail:
/// - `PlatformToUi::run`'s `WindowFocus` arm calling
///   `notify_presentation_focus_gained` inside `if focused { .. }`
///   unconditionally (removing that guard, so `WindowFocus(false)` also
///   moves active) -- the active-presentation assertion after B gains
///   focus would observe A instead of B.
/// - The polarity mutant `if !focused { return; }` in place of
///   `if !focused && !realm.is_active_presentation(presentation_id) { return; }`
///   -- dropping EVERY focus loss unconditionally, so the realm can
///   never suspend at all. This one is invisible to the middle
///   assertions (A's focus loss is already supposed to be dropped) and
///   is caught ONLY by the final step below, where B -- the ACTUALLY
///   active presentation -- loses focus and the realm must still
///   suspend.
#[test]
fn window_focus_true_moves_active_presentation_end_to_end_and_false_does_not() {
    // One shared platform instance for both windows -- see
    // `install_two_test_realms`'s doc for why two independent
    // `headless_platform()` calls would risk an aliased window id.
    let platform = flui_platform::headless_platform();
    let window_a: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create window a");
    let window_b: Arc<dyn flui_platform::traits::PlatformWindow> = Arc::new(
        crate::app::window_test_support::TestWindow::new()
            .with_id(2)
            .focused(false),
    );

    let realm = crate::app::ui_realm::UiRealm::for_test();
    let dispatcher_a = install_platform_realm(realm, &window_a);
    let a_id = dispatcher_a.address.presentation_id;
    let dispatcher_b = install_presentation_alongside(dispatcher_a, &window_b)
        .expect("B installs alongside A with a real window mapping");
    let b_id = dispatcher_b.address.presentation_id;

    dispatch_platform_realm(
        dispatcher_a,
        RealmTask::Frame(Box::new(move |realm| {
            assert_eq!(
                realm.active_presentation_for_test(),
                a_id,
                "precondition: a freshly installed realm starts with its primary active"
            );
        })),
    )
    .expect("precondition frame task dispatches");

    // WindowFocus(true) dispatched for B, through the REAL dispatch
    // seam -- not a direct notify_presentation_focus_gained call.
    dispatch_platform_realm(
        dispatcher_b,
        RealmTask::Event(PlatformToUi::WindowFocus(true)),
    )
    .expect("WindowFocus(true) for B dispatches");
    dispatch_platform_realm(
        dispatcher_a,
        RealmTask::Frame(Box::new(move |realm| {
            assert_eq!(
                realm.active_presentation_for_test(),
                b_id,
                "WindowFocus(true) dispatched through the real production path must move \
                 the active presentation to the presentation it was addressed to"
            );
        })),
    )
    .expect("frame task dispatches");

    // WindowFocus(false) dispatched for A -- NOT the currently active
    // presentation (B is) -- must never move active away from B.
    dispatch_platform_realm(
        dispatcher_a,
        RealmTask::Event(PlatformToUi::WindowFocus(false)),
    )
    .expect("WindowFocus(false) for A dispatches");
    dispatch_platform_realm(
        dispatcher_a,
        RealmTask::Frame(Box::new(move |realm| {
            assert_eq!(
                realm.active_presentation_for_test(),
                b_id,
                "WindowFocus(false) must never move the active presentation, regardless \
                 of which presentation it was addressed to"
            );
            // The realm-wide lifecycle aggregate must ALSO stay
            // Resumed: A's focus loss is a normal consequence of focus
            // having already moved to B (a sibling presentation of
            // this SAME realm), not a sign that the realm itself lost
            // focus -- B still holds it. Before the fix, this
            // non-active-presentation `WindowFocus(false)` flipped the
            // still loop-scoped `focused` aggregate unconditionally,
            // incorrectly suspending the whole realm while B was
            // still focused.
            assert_eq!(
                realm.scheduler().lifecycle_state(),
                AppLifecycleState::Resumed,
                "a WindowFocus(false) from a non-active presentation must never suspend \
                 the realm while a sibling presentation still holds focus"
            );
        })),
    )
    .expect("frame task dispatches");

    // WindowFocus(false) dispatched for B -- the presentation that IS
    // currently active -- pins the OTHER half of the guard: dropping
    // non-active focus-loss must never widen into dropping ALL
    // focus-loss. The realm must actually suspend here.
    dispatch_platform_realm(
        dispatcher_b,
        RealmTask::Event(PlatformToUi::WindowFocus(false)),
    )
    .expect("WindowFocus(false) for B dispatches");
    dispatch_platform_realm(
        dispatcher_a,
        RealmTask::Frame(Box::new(move |realm| {
            assert_ne!(
                realm.scheduler().lifecycle_state(),
                AppLifecycleState::Resumed,
                "WindowFocus(false) from the presentation that IS currently active must \
                 still suspend the realm -- the guard drops only NON-active focus-loss, \
                 never focus-loss unconditionally"
            );
        })),
    )
    .expect("frame task dispatches");

    teardown_platform_realm();
}

/// Losing OS focus mid-contact must cancel in-flight pointer sequences
/// (alt-tab means the matching Up may never arrive), with two scoping
/// rules pinned through the REAL dispatch seam:
///
/// - Focus transfer cancels the former window's contact immediately.
///   A delayed duplicate `WindowFocus(false)` from that window leaves
///   the newly focused sibling's sequence untouched.
/// - The cancellation is ADDRESSED: pointer input lands in each
///   presentation's own gesture binding (`handle_input_addressed`), so
///   the defocused presentation's own binding drains while a sibling
///   presentation's sequences survive. A primary-only cancel would
///   invert both assertions under a shared-realm window policy.
#[test]
fn window_focus_loss_cancels_the_addressed_presentations_sequences_only() {
    let platform = flui_platform::headless_platform();
    let window_a: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create window a");
    let window_b: Arc<dyn flui_platform::traits::PlatformWindow> = Arc::new(
        crate::app::window_test_support::TestWindow::new()
            .with_id(2)
            .focused(false),
    );

    let realm = crate::app::ui_realm::UiRealm::for_test();
    let dispatcher_a = install_platform_realm(realm, &window_a);
    let a_id = dispatcher_a.address.presentation_id;
    let dispatcher_b = install_presentation_alongside(dispatcher_a, &window_b)
        .expect("B installs alongside A with a real window mapping");
    let b_id = dispatcher_b.address.presentation_id;

    // A contact lands in A's own binding through the real addressed
    // input path, while A is the active presentation.
    dispatch_platform_realm(
        dispatcher_a,
        RealmTask::Event(PlatformToUi::Input(down_input(8.0))),
    )
    .expect("down for A dispatches");

    dispatch_platform_realm(
        dispatcher_a,
        RealmTask::Frame(Box::new(move |realm| {
            assert_eq!(
                realm
                    .presentation_gestures_for_test(a_id)
                    .active_pointer_count(),
                1
            );
        })),
    )
    .expect("A contact before transfer");

    // Focus moves to B; a second contact lands in B's own binding.
    dispatch_platform_realm(
        dispatcher_b,
        RealmTask::Event(PlatformToUi::WindowFocus(true)),
    )
    .expect("WindowFocus(true) for B dispatches");
    dispatch_platform_realm(
        dispatcher_b,
        RealmTask::Event(PlatformToUi::Input(down_input(21.0))),
    )
    .expect("down for B dispatches");
    dispatch_platform_realm(
        dispatcher_a,
        RealmTask::Frame(Box::new(move |realm| {
            assert_eq!(
                realm
                    .presentation_gestures_for_test(a_id)
                    .active_pointer_count(),
                0,
                "focus transfer must already cancel A's old contact"
            );
            assert_eq!(
                realm
                    .presentation_gestures_for_test(b_id)
                    .active_pointer_count(),
                1,
                "precondition: B's own binding holds its contact"
            );
        })),
    )
    .expect("frame task dispatches");

    // Duplicate loss after transfer: B's new sequence must survive.
    dispatch_platform_realm(
        dispatcher_a,
        RealmTask::Event(PlatformToUi::WindowFocus(false)),
    )
    .expect("stale WindowFocus(false) for A dispatches");
    dispatch_platform_realm(
        dispatcher_a,
        RealmTask::Frame(Box::new(move |realm| {
            assert_eq!(
                realm
                    .presentation_gestures_for_test(a_id)
                    .active_pointer_count(),
                0,
                "duplicate loss leaves the already-cancelled A unchanged"
            );
            assert_eq!(
                realm
                    .presentation_gestures_for_test(b_id)
                    .active_pointer_count(),
                1,
                "a stale focus loss must never cancel the active sibling's sequence"
            );
        })),
    )
    .expect("frame task dispatches");

    // Authoritative focus loss (B IS active): B's own binding drains;
    // A's — the PRIMARY's — must be untouched, proving the cancel is
    // addressed rather than primary-blasted.
    dispatch_platform_realm(
        dispatcher_b,
        RealmTask::Event(PlatformToUi::WindowFocus(false)),
    )
    .expect("authoritative WindowFocus(false) for B dispatches");
    dispatch_platform_realm(
        dispatcher_a,
        RealmTask::Frame(Box::new(move |realm| {
            assert_eq!(
                realm
                    .presentation_gestures_for_test(b_id)
                    .active_pointer_count(),
                0,
                "an authoritative focus loss must cancel the addressed presentation's \
                 in-flight sequences -- the platform may never deliver the matching Up"
            );
            assert!(
                realm
                    .presentation_gestures_for_test(b_id)
                    .arena()
                    .is_empty()
            );
            assert_eq!(
                realm
                    .presentation_gestures_for_test(a_id)
                    .active_pointer_count(),
                0,
                "A remains cancelled after B also loses focus"
            );
        })),
    )
    .expect("frame task dispatches");

    teardown_platform_realm();
}

/// `PlatformToUi::WindowVisibility` (hidden-surface gating) is
/// per-presentation, addressed to exactly the presentation
/// that produced it — never the realm-wide aggregate alone. Driven
/// through the REAL dispatch seam (`dispatch_platform_realm`), not a
/// direct `UiRealm::set_presentation_hidden` call, so this exercises
/// `PlatformToUi::run`'s actual `WindowVisibility` arm.
///
/// Mutant, confirmed to make this fail: removing
/// `realm.set_presentation_hidden(presentation_id, !visible)` from that
/// arm (leaving only the pre-existing loop-scoped `AppLifecycleState`
/// derivation) -- both presentations' clocks would stay unhidden
/// regardless of which one the event was addressed to.
#[test]
fn window_visibility_gates_exactly_the_addressed_presentations_clock() {
    let platform = flui_platform::headless_platform();
    let window_a: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create window a");
    let window_b: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create window b");

    let realm = crate::app::ui_realm::UiRealm::for_test();
    let dispatcher_a = install_platform_realm(realm, &window_a);
    let a_id = dispatcher_a.address.presentation_id;
    let dispatcher_b = install_presentation_alongside(dispatcher_a, &window_b)
        .expect("B installs alongside A with a real window mapping");
    let b_id = dispatcher_b.address.presentation_id;

    dispatch_platform_realm(
        dispatcher_a,
        RealmTask::Frame(Box::new(move |realm| {
            assert_eq!(
                realm.presentation_hidden_for_test(a_id),
                Some(false),
                "precondition: neither presentation starts hidden"
            );
            assert_eq!(realm.presentation_hidden_for_test(b_id), Some(false));
        })),
    )
    .expect("precondition frame task dispatches");

    // WindowVisibility(false) (occluded) dispatched for B only.
    dispatch_platform_realm(
        dispatcher_b,
        RealmTask::Event(PlatformToUi::WindowVisibility(false)),
    )
    .expect("WindowVisibility(false) for B dispatches");
    dispatch_platform_realm(
        dispatcher_a,
        RealmTask::Frame(Box::new(move |realm| {
            assert_eq!(
                realm.presentation_hidden_for_test(b_id),
                Some(true),
                "WindowVisibility(false) dispatched through the real production path must \
                 hide exactly the presentation it was addressed to"
            );
            assert_eq!(
                realm.presentation_hidden_for_test(a_id),
                Some(false),
                "a sibling presentation's own clock must be untouched by an event \
                 addressed to a DIFFERENT presentation"
            );
        })),
    )
    .expect("frame task dispatches");

    // WindowVisibility(true) (unoccluded) dispatched for B undoes it,
    // still leaving A untouched.
    dispatch_platform_realm(
        dispatcher_b,
        RealmTask::Event(PlatformToUi::WindowVisibility(true)),
    )
    .expect("WindowVisibility(true) for B dispatches");
    dispatch_platform_realm(
        dispatcher_a,
        RealmTask::Frame(Box::new(move |realm| {
            assert_eq!(
                realm.presentation_hidden_for_test(b_id),
                Some(false),
                "WindowVisibility(true) must unhide exactly the presentation it was \
                 addressed to"
            );
            assert_eq!(realm.presentation_hidden_for_test(a_id), Some(false));
        })),
    )
    .expect("frame task dispatches");

    teardown_platform_realm();
}

/// If `window`'s id is already registered (forced here by reusing A's
/// OWN window as the "second" presentation's window), the forest must
/// be left EXACTLY as it was -- no presentation resident without a
/// registry mapping of its own (`single-native-window-map-authority`).
///
/// If reverted (installing into the forest BEFORE registering, with no
/// rollback on failure -- the pre-fix ordering): this fails --
/// `presentation_count()` observes `2` instead of `1`, a forest-resident
/// presentation this test's own colliding window never actually mapped.
#[test]
fn install_presentation_alongside_leaves_the_forest_unchanged_on_registration_failure() {
    let window_a = test_window();
    let realm = crate::app::ui_realm::UiRealm::for_test();
    let dispatcher = install_platform_realm(realm, &window_a);

    // Reusing A's own window forces `try_register_window` to fail --
    // its id is already mapped to A's own address.
    let result = install_presentation_alongside(dispatcher, &window_a);
    assert!(
        matches!(
            result,
            Err(InstallPresentationError::WindowAlreadyMapped(_))
        ),
        "a colliding window id must be refused as WindowAlreadyMapped, got {result:?}"
    );

    let count = Rc::new(Cell::new(None));
    let count_in_task = Rc::clone(&count);
    dispatch_platform_realm(
        dispatcher,
        RealmTask::Frame(Box::new(move |realm| {
            count_in_task.set(Some(realm.presentation_count()));
        })),
    )
    .expect("frame task dispatches");
    assert_eq!(
        count.get(),
        Some(1),
        "a registration failure must leave the forest exactly as it was -- the \
         assembled-but-unregistered presentation must never have been installed"
    );

    teardown_platform_realm();
}

/// A `RealmDispatcher` for a presentation that has since CLOSED must be
/// refused, even though its realm survives (a sibling presentation kept
/// it alive) -- the same authorization `dispatch_platform_realm`
/// requires of every dispatched task. Before this fix,
/// `install_presentation_alongside` checked only realm existence, so a
/// caller still holding A's dispatcher after A closed could use it to
/// install a THIRD presentation alongside the survivor.
///
/// If reverted (the `registry.contains_address` check removed): this
/// fails -- the install succeeds instead of returning
/// `StalePresentation`, and `presentation_count()` observes `2` instead
/// of `1`.
#[test]
fn install_presentation_alongside_refuses_a_stale_dispatcher_even_though_its_realm_survives() {
    // One shared platform instance for every window -- see
    // `install_two_test_realms`'s doc for why two independent
    // `headless_platform()` calls would risk an aliased window id.
    let platform = flui_platform::headless_platform();
    let window_p: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create window p");
    let window_a: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create window a");
    let window_c: Arc<dyn PlatformWindow> = platform
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless platform should create window c");

    let realm = crate::app::ui_realm::UiRealm::for_test();
    let dispatcher_p = install_platform_realm(realm, &window_p);
    let dispatcher_a = install_presentation_alongside(dispatcher_p, &window_a)
        .expect("A installs alongside P with a real window mapping");
    let a_id = dispatcher_a.address.presentation_id;

    // Close A (non-primary) -- P survives, so the realm itself is
    // still perfectly live.
    close_presentation(dispatcher_p, a_id).expect("A closes through the real dispatch seam");

    // `dispatcher_a` is now stale: its own address is no longer
    // registered, even though `dispatcher_a.address.realm_id` still
    // names a live realm.
    let result = install_presentation_alongside(dispatcher_a, &window_c);
    assert!(
        matches!(result, Err(InstallPresentationError::StalePresentation)),
        "a dispatcher for a closed presentation must be refused as StalePresentation, even \
         though its realm survives, got {result:?}"
    );

    let count = Rc::new(Cell::new(None));
    let count_in_task = Rc::clone(&count);
    dispatch_platform_realm(
        dispatcher_p,
        RealmTask::Frame(Box::new(move |realm| {
            count_in_task.set(Some(realm.presentation_count()));
        })),
    )
    .expect("frame task dispatches");
    assert_eq!(
        count.get(),
        Some(1),
        "the rejected install must never have installed a third presentation -- only P \
         survives"
    );

    teardown_platform_realm();
}

// ========================================================================
// Close-request veto (issue #558)
// ========================================================================

/// Drives the platform's own USER-close path on a headless window --
/// the one that consults `on_should_close`, unlike
/// `PlatformWindow::close()`, which is the programmatic close and
/// deliberately bypasses the veto on every backend. Returns whether the
/// window actually closed.
fn simulate_user_close(window: &Arc<dyn flui_platform::traits::PlatformWindow>) -> bool {
    window
        .as_any()
        .downcast_ref::<flui_platform::MockWindow>()
        .expect("every window a HeadlessPlatform opens is a MockWindow")
        .simulate_close()
}

/// Whether this presentation is still hosted: both its realm entry and
/// its routable address in the window registry.
fn presentation_is_hosted(address: flui_foundation::PresentationAddress) -> bool {
    APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        state.realms.contains_key(&address.realm_id) && state.registry.contains_address(address)
    })
}

fn keep_open_handler(
    asked: &Arc<std::sync::atomic::AtomicUsize>,
    expected: flui_foundation::PresentationAddress,
) -> crate::app::close_request::CloseRequestHandler {
    use std::sync::atomic::Ordering;
    let asked = Arc::clone(asked);
    crate::app::close_request::CloseRequestHandler::new(move |request| {
        assert_eq!(
            request.address(),
            expected,
            "a handler must only ever be asked about the presentation it was registered for"
        );
        asked.fetch_add(1, Ordering::SeqCst);
        crate::app::close_request::CloseResponse::KeepOpen
    })
}

/// Criterion: an application can refuse a close for a named window, and
/// the window stays open.
///
/// Driven through the REAL embedder seam end to end --
/// `open_secondary_window_impl` installs the handler from the
/// `AppConfig` and wires `on_should_close` itself, and the close is a
/// real platform user-close on the real (headless) native window. No
/// step of the production path is stood in for.
///
/// The premise every assertion here rests on is checked, not assumed:
/// window B is genuinely open and hosted before the close, and a
/// CONTROL window opened the same way but with no handler closes under
/// the very same driver -- so "B is still hosted" cannot pass because
/// `simulate_user_close` never closes anything.
///
/// If reverted: restore the hard-coded `|| true` in
/// `install_close_request_wiring` and B closes anyway.
#[test]
fn a_close_request_handler_keeps_its_own_window_open() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let (dispatcher_a, _window_a, quit_calls, _clear_guard) =
        install_realm_a_with_exit_policy_and_quit_counter();

    let asked = Arc::new(AtomicUsize::new(0));
    let (dispatcher_b, window_b) =
        open_secondary_window_impl(AppConfig::default(), WindowPolicy::SeparateRealms)
            .expect("the guarded window must open")
            .expect("headless open_window is always Ready, never Pending");
    // Registered after the fact through the SAME production wiring the
    // bootstrap uses, because the address only exists once the window
    // is installed.
    install_close_request_wiring(
        dispatcher_b.address,
        &window_b,
        Some(keep_open_handler(&asked, dispatcher_b.address)),
    );

    let (_dispatcher_control, window_control) =
        open_secondary_window_impl(AppConfig::default(), WindowPolicy::SeparateRealms)
            .expect("the control window must open")
            .expect("headless open_window is always Ready, never Pending");

    // ── premises ────────────────────────────────────────────────────
    assert_ne!(
        dispatcher_a.address.realm_id, dispatcher_b.address.realm_id,
        "the guarded window must be a genuinely distinct presentation"
    );
    assert!(
        presentation_is_hosted(dispatcher_b.address),
        "premise: the guarded window is open and hosted before anything asks it to close"
    );
    assert_eq!(
        asked.load(Ordering::SeqCst),
        0,
        "premise: the handler has not been consulted yet"
    );

    // ── control: the same driver really does close a window ─────────
    assert!(
        simulate_user_close(&window_control),
        "premise: with no handler registered, a user close closes the window"
    );

    // ── the veto ────────────────────────────────────────────────────
    assert!(
        !simulate_user_close(&window_b),
        "a KeepOpen answer must stop the close at the platform seam"
    );
    assert_eq!(
        asked.load(Ordering::SeqCst),
        1,
        "the handler is asked exactly once per close request"
    );
    assert!(
        presentation_is_hosted(dispatcher_b.address),
        "a vetoed close must leave the presentation hosted -- no on_close, no teardown"
    );
    assert_eq!(
        quit_calls.load(Ordering::SeqCst),
        0,
        "windows remain open, so the loop must not have exited"
    );

    teardown_platform_realm();
}

/// Criterion: a window kept open can be closed programmatically
/// afterwards -- the deferral resolves.
///
/// This is what makes a stateless veto finite: the runtime holds no
/// pending obligation, and the application holds the means to finish
/// the close it asked for. `request_presentation_close` is the public
/// entry point, addressed by exactly the `PresentationAddress` the
/// handler received in its own `CloseRequest`.
///
/// If reverted: drop the `register` call from
/// `install_close_request_wiring` and the programmatic close reports
/// `UnknownPresentation` instead of closing the window.
#[test]
fn a_window_kept_open_closes_on_a_later_programmatic_request() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let (_dispatcher_a, _window_a, _quit_calls, _clear_guard) =
        install_realm_a_with_exit_policy_and_quit_counter();

    let asked = Arc::new(AtomicUsize::new(0));
    let (dispatcher_b, window_b) =
        open_secondary_window_impl(AppConfig::default(), WindowPolicy::SeparateRealms)
            .expect("the guarded window must open")
            .expect("headless open_window is always Ready, never Pending");

    // The address the application would have captured from the request
    // it refused -- taken from the handler's own argument, not from the
    // dispatcher, so the test resolves the close with the same value an
    // application would have to.
    let refused: Arc<parking_lot::Mutex<Option<flui_foundation::PresentationAddress>>> =
        Arc::new(parking_lot::Mutex::new(None));
    let refused_in_handler = Arc::clone(&refused);
    let asked_in_handler = Arc::clone(&asked);
    install_close_request_wiring(
        dispatcher_b.address,
        &window_b,
        Some(crate::app::close_request::CloseRequestHandler::new(
            move |request| {
                asked_in_handler.fetch_add(1, Ordering::SeqCst);
                *refused_in_handler.lock() = Some(request.address());
                crate::app::close_request::CloseResponse::KeepOpen
            },
        )),
    );

    assert!(
        !simulate_user_close(&window_b),
        "premise: the close is refused first, so there is something to resolve"
    );
    assert!(
        presentation_is_hosted(dispatcher_b.address),
        "premise: the window is still hosted after the refusal"
    );
    let refused = refused
        .lock()
        .expect("the handler ran and captured its address");

    // The application finished its work and finishes the close.
    request_presentation_close(refused).expect("the kept-open window must still be closable");

    assert_eq!(
        asked.load(Ordering::SeqCst),
        1,
        "a programmatic close must not re-ask the handler that refused -- asking again \
         would either loop forever or force the application to track its own closes"
    );
    assert!(
        !presentation_is_hosted(dispatcher_b.address),
        "the programmatic close must run the real teardown: on_close, close_this_window, \
         and the presentation gone from the registry"
    );
    assert_eq!(
        request_presentation_close(refused),
        Err(crate::app::close_request::CloseRequestError::UnknownPresentation { address: refused }),
        "the router entry goes with the presentation, so a second close is a typed refusal"
    );

    teardown_platform_realm();
}

/// Criterion: one window's veto does not reach its sibling.
///
/// Both windows carry a handler, answering oppositely, so the oracle
/// cannot pass by "the second window had nothing registered". Each
/// handler asserts the address it is asked about, and the counters
/// prove a close request reaches exactly one of them.
///
/// If reverted: key the router on `RealmId` instead of the full
/// `PresentationAddress`, or capture one shared address in
/// `install_close_request_wiring`'s callback, and the sibling
/// assertions fail.
#[test]
fn a_windows_veto_is_addressed_to_that_window_alone() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let (_dispatcher_a, _window_a, quit_calls, _clear_guard) =
        install_realm_a_with_exit_policy_and_quit_counter();

    let (dispatcher_guarded, window_guarded) =
        open_secondary_window_impl(AppConfig::default(), WindowPolicy::SeparateRealms)
            .expect("the guarded window must open")
            .expect("headless open_window is always Ready, never Pending");
    let (dispatcher_free, window_free) =
        open_secondary_window_impl(AppConfig::default(), WindowPolicy::SeparateRealms)
            .expect("the second window must open")
            .expect("headless open_window is always Ready, never Pending");

    assert_ne!(
        dispatcher_guarded.address, dispatcher_free.address,
        "premise: two genuinely distinct presentations"
    );
    assert!(
        presentation_is_hosted(dispatcher_guarded.address)
            && presentation_is_hosted(dispatcher_free.address),
        "premise: BOTH windows are open and hosted -- a sibling that was never open would \
         make every assertion below vacuous"
    );

    let guarded_asked = Arc::new(AtomicUsize::new(0));
    install_close_request_wiring(
        dispatcher_guarded.address,
        &window_guarded,
        Some(keep_open_handler(
            &guarded_asked,
            dispatcher_guarded.address,
        )),
    );

    let free_asked = Arc::new(AtomicUsize::new(0));
    let free_asked_in_handler = Arc::clone(&free_asked);
    let free_address = dispatcher_free.address;
    install_close_request_wiring(
        dispatcher_free.address,
        &window_free,
        Some(crate::app::close_request::CloseRequestHandler::new(
            move |request| {
                assert_eq!(request.address(), free_address);
                free_asked_in_handler.fetch_add(1, Ordering::SeqCst);
                crate::app::close_request::CloseResponse::Close
            },
        )),
    );

    assert!(
        simulate_user_close(&window_free),
        "the sibling answers Close for itself and must close"
    );
    assert_eq!(free_asked.load(Ordering::SeqCst), 1);
    assert_eq!(
        guarded_asked.load(Ordering::SeqCst),
        0,
        "the guarded window's handler must not be consulted about a sibling's close"
    );
    assert!(!presentation_is_hosted(dispatcher_free.address));
    assert!(
        presentation_is_hosted(dispatcher_guarded.address),
        "a sibling closing must not disturb the guarded presentation"
    );

    assert!(
        !simulate_user_close(&window_guarded),
        "the guarded window still refuses on its own account"
    );
    assert_eq!(guarded_asked.load(Ordering::SeqCst), 1);
    assert_eq!(
        free_asked.load(Ordering::SeqCst),
        1,
        "the already-closed sibling's handler must not be consulted again"
    );
    assert_eq!(quit_calls.load(Ordering::SeqCst), 0);

    teardown_platform_realm();
}

/// Criterion: the close-request veto and the keep-alive (process-exit)
/// veto are ordered, and the order is causal rather than chosen.
///
/// Two phases, one driver (`simulate_user_close`) and one observable
/// (`on_quit` plus whether the presentation survives), so the phases
/// discriminate:
///
/// 1. **A close veto short-circuits the exit question.** The sole
///    window refuses; it stays hosted and the loop never quits, because
///    the backend consults the exit-policy hook only after a close has
///    actually removed the last window.
/// 2. **A keep-alive veto cannot hold a window open.** With no close
///    handler and a running `ServiceLifetime::KeepsAppAlive` service,
///    the very same close SUCCEEDS -- window gone, presentation torn
///    down -- and only the process exit is deferred.
///
/// The reverse order is not merely worse, it is incoherent: "may the
/// process exit now that the last window is gone" cannot be asked about
/// a window that is still open. Phase 2 is what makes that concrete --
/// a process-level veto that could hold a window open would break the
/// messenger case (close the window, live in the tray) outright.
///
/// If reverted: make `install_close_request_wiring` answer `true`
/// unconditionally and phase 1's window closes; make
/// `AppRuntime::should_exit` ignore `keeps_app_alive` and phase 2's
/// quit fires.
#[test]
fn a_close_veto_short_circuits_the_keep_alive_exit_question() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    // ── phase 1: the close veto, with nothing else in play ──────────
    {
        let (dispatcher, window, quit_calls, _clear_guard) =
            install_realm_a_with_exit_policy_and_quit_counter();
        let asked = Arc::new(AtomicUsize::new(0));
        install_close_request_wiring(
            dispatcher.address,
            &window,
            Some(keep_open_handler(&asked, dispatcher.address)),
        );
        assert!(
            presentation_is_hosted(dispatcher.address),
            "premise: the sole window is hosted"
        );

        assert!(
            !simulate_user_close(&window),
            "the sole window refuses its own close"
        );
        assert_eq!(asked.load(Ordering::SeqCst), 1);
        assert!(
            presentation_is_hosted(dispatcher.address),
            "the window is still open, so the exit question was never reached"
        );
        assert_eq!(
            quit_calls.load(Ordering::SeqCst),
            0,
            "no window closed, so the exit-policy hook cannot have allowed a quit"
        );

        teardown_platform_realm();
    }

    // ── phase 2: a keep-alive service defers the EXIT, not the close ─
    {
        use std::sync::atomic::AtomicBool;

        use crate::app::lifecycle::{ServiceDefinition, ServiceLifetime};

        let (dispatcher, window, quit_calls, _clear_guard) =
            install_realm_a_with_exit_policy_and_quit_counter();

        let release = Arc::new(AtomicBool::new(false));
        let waker_slot: Arc<parking_lot::Mutex<Option<std::task::Waker>>> =
            Arc::new(parking_lot::Mutex::new(None));
        let release_in_service = Arc::clone(&release);
        let waker_in_service = Arc::clone(&waker_slot);
        APP_RUNTIME.with(|slot| {
            slot.borrow_mut()
                .start_service(&ServiceDefinition::new(
                    "tray-resident",
                    ServiceLifetime::KeepsAppAlive,
                    move |_context| {
                        let release = Arc::clone(&release_in_service);
                        let waker_slot = Arc::clone(&waker_in_service);
                        Box::pin(async move {
                            std::future::poll_fn(move |context| {
                                if release.load(Ordering::Acquire) {
                                    std::task::Poll::Ready(())
                                } else {
                                    let waker = context.waker().clone();
                                    let _prev = waker_slot.lock().replace(waker);
                                    std::task::Poll::Pending
                                }
                            })
                            .await;
                        })
                    },
                ))
                .expect("the keep-alive service must start on the loop's real IO pool");
        });

        assert!(
            simulate_user_close(&window),
            "a running keep-alive service must NOT hold a window open -- it defers the \
             process exit that follows the last window closing, a different question"
        );
        assert!(
            !presentation_is_hosted(dispatcher.address),
            "the window really closed: on_close ran and the presentation is gone"
        );
        assert_eq!(
            quit_calls.load(Ordering::SeqCst),
            0,
            "the keep-alive veto is consulted only now, after the close, and defers the exit"
        );

        release.store(true, Ordering::Release);
        let taken = waker_slot.lock().take();
        if let Some(waker) = taken {
            waker.wake();
        }
        teardown_platform_realm();
    }
}

/// A worker thread resolving a deferred close must be REFUSED with a
/// reason, never silently do nothing. `AppRuntime` is thread-local, so
/// without the owner-thread check the worker reads its own fresh,
/// empty runtime and the call reports "no such presentation" about an
/// address that is perfectly live — indistinguishable, to the caller,
/// from a window that already closed.
///
/// The premise is asserted rather than assumed: after the refusal the
/// SAME address closes successfully from the owner thread, so the
/// worker's error was about the thread, not about the address.
///
/// If reverted: drop the `owner_thread` match from
/// `request_presentation_close` and the worker gets
/// `UnknownPresentation` instead.
#[test]
fn resolving_a_deferred_close_from_a_worker_thread_is_a_typed_refusal() {
    use crate::app::close_request::CloseRequestError;

    let (_dispatcher_a, _window_a, _quit_calls, _clear_guard) =
        install_realm_a_with_exit_policy_and_quit_counter();

    let (dispatcher_b, window_b) =
        open_secondary_window_impl(AppConfig::default(), WindowPolicy::SeparateRealms)
            .expect("the deferred-close window must open")
            .expect("headless open_window is always Ready, never Pending");
    install_close_request_wiring(dispatcher_b.address, &window_b, None);
    assert!(
        presentation_is_hosted(dispatcher_b.address),
        "premise: the presentation is hosted, so the only thing wrong with the worker's \
         call below is the thread it is on"
    );

    let address = dispatcher_b.address;
    let refused = std::thread::spawn(move || request_presentation_close(address))
        .join()
        .expect("the worker thread must not panic");
    assert_eq!(
        refused,
        Err(CloseRequestError::NoHostedRuntime),
        "a worker thread hosts no runtime; it must learn that, not be told the presentation \
         does not exist"
    );
    assert!(
        presentation_is_hosted(address),
        "and the refusal must leave the presentation exactly as it was"
    );

    request_presentation_close(address).expect("the owner thread closes the very same address");
    assert!(!presentation_is_hosted(address));

    teardown_platform_realm();
}

/// Close-request registrations must not outlive the loop. Per-realm
/// teardown drops a realm's own entries, but full loop-exit teardown
/// does not go through that path at all — and this same thread-local
/// `AppRuntime` serves a SECOND `Platform::run` on this thread, so a
/// survivor would be consulted by the next loop's windows.
///
/// Two shapes in one run: a live presentation's entry, and an entry for
/// an address whose realm was never installed at all (the bootstrap
/// that wires a window and then fails), which no realm removal could
/// ever name.
///
/// If reverted: remove the `close_requests().clear()` call from
/// `teardown_platform_realm` and both assertions fail.
#[test]
fn full_loop_teardown_clears_close_request_registrations() {
    use crate::app::close_request::{CloseRequestHandler, CloseResponse};

    let (dispatcher_a, window_a, _quit_calls, _clear_guard) =
        install_realm_a_with_exit_policy_and_quit_counter();

    let keep_open = CloseRequestHandler::new(|_| CloseResponse::KeepOpen);
    install_close_request_wiring(dispatcher_a.address, &window_a, Some(keep_open.clone()));

    // The bootstrap-failure shape: a window wired before its realm
    // exists, under an address no realm teardown will ever mention.
    let orphan = flui_foundation::PresentationAddress {
        realm_id: flui_foundation::RealmId::new(9_999),
        presentation_id: flui_foundation::PresentationId::new(9_999),
    };
    install_close_request_wiring(orphan, &window_a, Some(keep_open));

    let router = APP_RUNTIME.with(|slot| slot.borrow().close_requests());
    assert_eq!(
        router.consult(dispatcher_a.address),
        CloseResponse::KeepOpen,
        "premise: both entries answer before teardown"
    );
    assert_eq!(router.consult(orphan), CloseResponse::KeepOpen);

    teardown_platform_realm();

    let router = APP_RUNTIME.with(|slot| slot.borrow().close_requests());
    assert_eq!(
        router.consult(dispatcher_a.address),
        CloseResponse::Close,
        "a registration must not survive the loop that made it"
    );
    assert_eq!(
        router.consult(orphan),
        CloseResponse::Close,
        "least of all one no realm teardown could have named"
    );
}
