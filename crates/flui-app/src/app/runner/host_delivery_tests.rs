use super::*;
use crate::app::lifecycle::{ServiceDefinition, ServiceLifetime};
use crate::app::runner::window_routes::OwnerOpenRouted as _;
use crate::app::runtime::{ExitPolicy, WindowPolicy};
use flui_native::{HeadlessPlatform, Platform};
use std::sync::atomic::AtomicUsize;

fn service_factory_can_use_owner_window_capabilities() {
    let attempted = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&attempted);
    let service = ServiceDefinition::new(
        "window-capability",
        ServiceLifetime::StopsWithLastWindow,
        move |_| {
            let answer = super::super::secondary_window::open_secondary_window(
                AppConfig::default(),
                WindowPolicy::Shared,
            );
            assert!(
                matches!(answer, Err(AppWindowError::UnsupportedPolicy { .. })),
                "no shared runtime exists, so refusal must be typed"
            );
            observed.store(true, Ordering::SeqCst);
            Box::pin(async {})
        },
    );
    let app = Application::new(|_| flui_widgets::Text::new("unused"))
        .with_startup_window(StartupWindow::None)
        .with_config(
            AppConfig::new()
                .with_exit_policy(ExitPolicy::ExplicitQuit)
                .with_service(service),
        )
        .on_ready(|handle| handle.request_quit().expect("ordinary quit"));
    let result = run_with_platform(app, Box::new(HeadlessPlatform::new()));
    assert!(
        result.is_ok(),
        "service factory must run outside the host borrow: {result:?}"
    );
    assert!(attempted.load(Ordering::SeqCst));
}

fn executor_refusal_retires_service_future_outside_host_borrow() {
    use flui_runtime::execution::{
        ComputeJob, HostComputePool, HostExecutors, HostIoPool, IoFuture, SpawnError,
    };
    struct Pool;
    impl HostComputePool for Pool {
        fn spawn_job(&self, job: ComputeJob) -> Result<(), SpawnError> {
            job();
            Ok(())
        }
    }
    impl HostIoPool for Pool {
        fn spawn_future(&self, future: IoFuture) -> Result<(), SpawnError> {
            let result = super::super::secondary_window::open_secondary_window(
                AppConfig::new(),
                WindowPolicy::Shared,
            );
            assert!(matches!(
                result,
                Err(AppWindowError::UnsupportedPolicy { .. })
            ));
            drop(future);
            Err(SpawnError::Saturated)
        }
    }
    struct Capture(Arc<AtomicBool>);
    impl Drop for Capture {
        fn drop(&mut self) {
            let result = super::super::secondary_window::open_secondary_window(
                AppConfig::new(),
                WindowPolicy::Shared,
            );
            assert!(matches!(
                result,
                Err(AppWindowError::UnsupportedPolicy { .. })
            ));
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let retired = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&retired);
    let service = ServiceDefinition::new(
        "executor-reentry",
        ServiceLifetime::StopsWithLastWindow,
        move |_| {
            let capture = Capture(Arc::clone(&observed));
            Box::pin(async move {
                drop(capture);
            })
        },
    );
    let app = Application::new(|_| flui_widgets::Text::new("unused"))
        .with_startup_window(StartupWindow::None)
        .with_config(
            AppConfig::new()
                .with_service(service)
                .with_executors(HostExecutors::new(Arc::new(Pool), Arc::new(Pool))),
        );
    let result = run_with_platform(app, Box::new(HeadlessPlatform::new()));
    let Err(AppRunError::Service { source }) = result else {
        panic!("executor refusal must reach startup");
    };
    assert!(matches!(
        source.downcast_ref::<crate::app::lifecycle::ServiceStartError>(),
        Some(crate::app::lifecycle::ServiceStartError::Spawn(
            SpawnError::Saturated
        ))
    ));
    assert!(
        retired.load(Ordering::SeqCst),
        "the refused service future retires normally"
    );
}

fn runtime_frame_wake_reaches_its_window_after_sibling_installation() {
    let _owner = OwnerHostClearGuard::arm();
    let _teardown = LoopTeardown;
    let a_calls = Arc::new(AtomicUsize::new(0));
    let b_calls = Arc::new(AtomicUsize::new(0));
    let a_seen = Arc::clone(&a_calls);
    let b_seen = Arc::clone(&b_calls);
    Box::new(HeadlessPlatform::new())
        .run(Box::new(move |owner| {
            install_owner_platform(owner)?;
            let a_host = with_owner_platform(|owner| {
                owner.open_routed(flui_platform::WindowOptions::default())
            })
            .expect("owner")
            .expect("open A")
            .try_ready()
            .expect("ready A");
            let b_host = with_owner_platform(|owner| {
                owner.open_routed(flui_platform::WindowOptions::default())
            })
            .expect("owner")
            .expect("open B")
            .try_ready()
            .expect("ready B");
            let a_window: Rc<dyn flui_platform::PlatformWindow> = a_host.clone();
            let b_window: Rc<dyn flui_platform::PlatformWindow> = b_host.clone();
            crate::app::runner::window_routes::WindowRoutes::of(&*a_window).route_frame_requested(
                move |_| {
                    a_seen.fetch_add(1, Ordering::SeqCst);
                },
            );
            crate::app::runner::window_routes::WindowRoutes::of(&*b_window).route_frame_requested(
                move |_| {
                    b_seen.fetch_add(1, Ordering::SeqCst);
                },
            );
            APP_RUNTIME.with(|slot| slot.borrow().set_redraw_window(Rc::clone(&a_window)));
            let mut a = super::super::host::build_ui_runtime(
                super::super::presentation_window(a_host),
                1.0,
            )
            .expect("runtime A");
            a.pump_background();
            APP_RUNTIME.with(|slot| slot.borrow().set_redraw_window(Rc::clone(&b_window)));
            let mut b = super::super::host::build_ui_runtime(
                super::super::presentation_window(b_host),
                1.0,
            )
            .expect("runtime B");
            b.pump_background();
            a_calls.store(0, Ordering::SeqCst);
            b_calls.store(0, Ordering::SeqCst);
            a.mark_rendered();
            b.mark_rendered();
            a.scheduler().frame_waker().request_frame();
            assert!(a.needs_redraw());
            assert!(!b.needs_redraw());
            b.mark_rendered();
            assert!(
                a.needs_redraw(),
                "a sibling frame cannot clear this runtime's delivery debt"
            );
            assert_eq!(
                (
                    a_calls.load(Ordering::SeqCst),
                    b_calls.load(Ordering::SeqCst)
                ),
                (1, 0),
                "a runtime wake must redraw its own window, without waking a sibling",
            );
            Ok(())
        }))
        .expect("headless loop");
}

fn service_factory_replacement_refuses_obsolete_work_and_preserves_the_new_owner() {
    use flui_runtime::execution::{
        ComputeJob, HostComputePool, HostExecutors, HostIoPool, IoFuture, SpawnError,
    };
    struct Pool(Arc<AtomicUsize>);
    impl HostComputePool for Pool {
        fn spawn_job(&self, job: ComputeJob) -> Result<(), SpawnError> {
            job();
            Ok(())
        }
    }
    impl HostIoPool for Pool {
        fn spawn_future(&self, future: IoFuture) -> Result<(), SpawnError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            drop(future);
            Ok(())
        }
    }
    struct Capture {
        owner: Arc<()>,
        retired: Arc<AtomicUsize>,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            APP_RUNTIME.with(|slot| {
                assert!(Arc::ptr_eq(&slot.borrow().loop_identity, &self.owner));
            });
            let answer = super::super::secondary_window::open_secondary_window(
                AppConfig::new(),
                WindowPolicy::Shared,
            );
            assert!(matches!(
                answer,
                Err(AppWindowError::UnsupportedPolicy { .. })
            ));
            self.retired.fetch_add(1, Ordering::SeqCst);
        }
    }
    let _owner = OwnerHostClearGuard::arm();
    let _teardown = LoopTeardown;
    let spawned = Arc::new(AtomicUsize::new(0));
    let retired = Arc::new(AtomicUsize::new(0));
    let seen_spawned = Arc::clone(&spawned);
    let seen_retired = Arc::clone(&retired);
    Box::new(HeadlessPlatform::new())
        .run(Box::new(move |owner| {
            install_owner_platform(owner)?;
            APP_RUNTIME.with(|slot| {
                let mut state = slot.borrow_mut();
                state.reopen_lifecycles();
                state.install_host_executors(HostExecutors::new(
                    Arc::new(Pool(Arc::clone(&seen_spawned))),
                    Arc::new(Pool(Arc::clone(&seen_spawned))),
                ));
            });
            let observed = Arc::clone(&seen_retired);
            let service = ServiceDefinition::new(
                "replacing-factory",
                ServiceLifetime::StopsWithLastWindow,
                move |_| {
                    let previous =
                        APP_RUNTIME.with(|slot| Arc::clone(&slot.borrow().loop_identity));
                    Box::new(HeadlessPlatform::new())
                        .run(Box::new(|replacement| {
                            install_owner_platform(replacement)?;
                            Ok(())
                        }))
                        .expect("replacement owner installed");
                    let owner = APP_RUNTIME.with(|slot| Arc::clone(&slot.borrow().loop_identity));
                    assert!(!Arc::ptr_eq(&owner, &previous));
                    let capture = Capture {
                        owner,
                        retired: Arc::clone(&observed),
                    };
                    Box::pin(async move { drop(capture) })
                },
            );
            assert!(matches!(
                super::super::host::start_service(&service),
                Err(crate::app::lifecycle::ServiceStartError::Spawn(
                    SpawnError::ShuttingDown
                ))
            ));
            assert_eq!(seen_spawned.load(Ordering::SeqCst), 0);
            assert_eq!(seen_retired.load(Ordering::SeqCst), 1);
            let successor = ServiceDefinition::new(
                "replacement-service",
                ServiceLifetime::StopsWithLastWindow,
                |_| Box::pin(async {}),
            );
            super::super::host::start_service(&successor)
                .expect("the replacement owner still admits services");
            assert_eq!(seen_spawned.load(Ordering::SeqCst), 1);
            Ok(())
        }))
        .expect("headless owner replacement");
    assert_eq!(spawned.load(Ordering::SeqCst), 1);
    assert_eq!(retired.load(Ordering::SeqCst), 1);
}

#[test]
fn host_delivery_contract() {
    crate::table_test::run_table(
        "host delivery",
        &[
            (
                "service_factory_can_use_owner_window_capabilities",
                service_factory_can_use_owner_window_capabilities as fn(),
            ),
            (
                "executor_refusal_retires_service_future_outside_host_borrow",
                executor_refusal_retires_service_future_outside_host_borrow as fn(),
            ),
            (
                "service_factory_replacement_refuses_obsolete_work_and_preserves_the_new_owner",
                service_factory_replacement_refuses_obsolete_work_and_preserves_the_new_owner
                    as fn(),
            ),
            (
                "runtime_frame_wake_reaches_its_window_after_sibling_installation",
                runtime_frame_wake_reaches_its_window_after_sibling_installation as fn(),
            ),
        ],
    );
}
