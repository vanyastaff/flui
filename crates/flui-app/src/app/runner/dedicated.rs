//! Destination bootstrap. Only the construction closure crosses a thread;
//! window installation, native dispatch, text services and raster stay local.

use super::host::{APP_RUNTIME, OwnerHostClearGuard, install_owner_platform, with_owner_platform};
use crate::app::application_control::{OwnerRoute, Reply, contain};
use crate::app::{AppWindowError, RuntimeWindowConfig, WindowPolicy, WindowRequest};
use flui_view::View;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Native admission remains disabled until the integrated acceptance matrix.
#[derive(Clone, Copy, Default)]
pub(crate) enum DedicatedBackend {
    #[default]
    Unavailable,
    #[cfg(feature = "testing")]
    Headless,
    #[cfg(all(feature = "testing", windows))]
    Win32Qualification,
}

impl DedicatedBackend {
    pub(super) fn available(self) -> bool {
        !matches!(self, Self::Unavailable)
    }
}

/// Construct a window and UI runtime on a fresh, permanent owner.
///
/// Only `factory` crosses threads. Its root, callbacks and providers are
/// created, used and destroyed on the destination. Dropping the receipt does
/// not cancel an accepted launch. Native admission is currently unavailable
/// pending integrated native acceptance; the testing kit exercises this path.
///
/// # Errors
/// Refuses unsupported placement or exhausted capacity before invoking the
/// factory. Installation failures are delivered through the returned receipt.
pub(crate) fn open<V, F>(factory: F) -> Result<WindowRequest, AppWindowError>
where
    V: View + Clone + 'static,
    F: FnOnce() -> (V, RuntimeWindowConfig) + Send + 'static,
{
    let (route, backend) = APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        (state.owner_route.clone(), state.dedicated_backend)
    });
    if !backend.available() {
        return Err(AppWindowError::UnsupportedPolicy {
            reason: "dedicated ownership is not qualified for this host/backend",
        });
    }
    let parent = route.ok_or(AppWindowError::NoOwnerLoop)?;
    let services = parent.ingress.services();
    if services.has_development_hooks {
        return Err(AppWindowError::UnsupportedPolicy {
            reason: "dedicated development-hook routing has not been qualified",
        });
    }
    let failure_detail = services.failure_detail;
    let (route, budget) = parent.ingress.reserve_owner()?;
    let (reply, mut request) = WindowRequest::new();
    request.budget = Some(Arc::clone(&budget));
    let completion = Arc::new(LaunchCompletion {
        route: route.clone(),
        reply,
        settled: AtomicBool::new(false),
        budget: parking_lot::Mutex::new(Some(budget)),
        failure: parking_lot::Mutex::new(None),
    });
    // Declared before rollback: cancellation settles/quiesces first, and only
    // then lets the helper acknowledge that no UI thread was started.
    let mut retirement = None;
    // Covers an OS spawn failure, including unwinding while the rejected
    // closure's user captures are destroyed on the caller.
    let mut rollback = OwnerLifetime {
        completion: Arc::clone(&completion),
        route: route.clone(),
        armed: true,
        retire_here: true,
    };
    retirement.replace(
        crate::app::application_control::ThreadRetirement::prepare(route.clone()).map_err(
            |error| AppWindowError::Native {
                source: Arc::new(error),
            },
        )?,
    );
    rollback.retire_here = false;
    let worker_completion = Arc::clone(&completion);
    let worker_route = route;
    let thread = std::thread::Builder::new()
        .name("flui-ui-owner".to_owned())
        .spawn(move || {
            let _lifetime = OwnerLifetime {
                completion: Arc::clone(&worker_completion),
                route: worker_route.clone(),
                armed: true,
                retire_here: false,
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_owner(
                    factory,
                    backend,
                    worker_route,
                    services,
                    Arc::clone(&worker_completion),
                )
            }));
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => worker_completion.failed(error),
                Err(payload) => {
                    let message =
                        flui_runtime::frame_failure::panic_text(failure_detail, payload.as_ref());
                    flui_foundation::panic::discard_payload(payload);
                    worker_completion.failed(AppWindowError::InstallerPanicked { message });
                }
            }
        });
    match thread {
        Ok(thread) => {
            retirement
                .take()
                .expect("BUG: join helper precedes owner spawn")
                .handoff(thread);
            // The worker now owns retirement. This guard must not run twice.
            rollback.armed = false;
        }
        Err(error) => {
            completion.complete(Err(AppWindowError::Native {
                source: Arc::new(error),
            }));
        }
    }
    Ok(request)
}

struct OwnerLifetime {
    completion: Arc<LaunchCompletion>,
    route: OwnerRoute,
    armed: bool,
    retire_here: bool,
}
impl Drop for OwnerLifetime {
    fn drop(&mut self) {
        if self.armed {
            self.route.quiesce();
            self.completion.finish_cancelled();
            if self.retire_here {
                self.route.retired();
            }
        }
    }
}

struct LaunchCompletion {
    route: OwnerRoute,
    reply: Arc<Reply>,
    settled: AtomicBool,
    budget: parking_lot::Mutex<Option<Arc<crate::app::application_control::ReceiptBudget>>>,
    failure: parking_lot::Mutex<Option<AppWindowError>>,
}
impl LaunchCompletion {
    fn defer_failure(&self, error: AppWindowError) {
        let refused = {
            let mut failure = self.failure.lock();
            if failure.is_none() {
                *failure = Some(error);
                None
            } else {
                Some(error)
            }
        };
        contain(|| drop(refused));
    }
    fn finish_cancelled(&self) {
        let failure = self
            .failure
            .lock()
            .take()
            .unwrap_or(AppWindowError::Cancelled);
        self.complete(Err(failure));
    }
    fn failed(&self, error: AppWindowError) {
        if self.settled.load(Ordering::Acquire) {
            self.route.ingress.record_owner_failure(error);
        } else {
            self.defer_failure(error);
        }
    }
    fn complete(&self, result: Result<flui_foundation::PresentationAddress, AppWindowError>) {
        if self.settled.swap(true, Ordering::AcqRel) {
            return;
        }
        let budget = self.budget.lock().take();
        self.route.launched();
        drop(budget);
        contain(|| {
            let _ = self.reply.deliver(result);
        });
    }
}

pub(crate) struct PendingDedicated {
    request: WindowRequest,
    completion: Arc<LaunchCompletion>,
}

/// Called after the existing secondary installer has published or rolled back.
pub(super) fn settle_launch() {
    let pending = APP_RUNTIME.with(|slot| slot.borrow_mut().dedicated_launch.take());
    let Some(mut pending) = pending else {
        return;
    };
    match pending.request.try_result() {
        Some(Ok(address)) => pending.completion.complete(Ok(address)),
        Some(Err(error)) => {
            pending.completion.defer_failure(error);
            with_owner_platform(|owner| owner.proxy().request_quit());
        }
        None => APP_RUNTIME.with(|slot| slot.borrow_mut().dedicated_launch = Some(pending)),
    }
}

struct LocalTeardown;
impl Drop for LocalTeardown {
    fn drop(&mut self) {
        contain(|| super::owner_dispatch::teardown_platform_ui_runtime_logged(&mut |_| {}));
        let pending = APP_RUNTIME.with(|slot| slot.borrow_mut().dedicated_launch.take());
        contain(|| drop(pending));
    }
}

struct DedicatedPlatform {
    platform: Box<dyn flui_native::Platform>,
    #[cfg(feature = "testing")]
    headless: Option<(
        flui_native::HeadlessOwnerTurns,
        flui_native::HeadlessExitReevaluation,
    )>,
}

fn make_platform(backend: DedicatedBackend) -> Result<DedicatedPlatform, AppWindowError> {
    match backend {
        DedicatedBackend::Unavailable => Err(AppWindowError::UnsupportedPolicy {
            reason: "dedicated host is unavailable",
        }),
        #[cfg(feature = "testing")]
        DedicatedBackend::Headless => {
            let platform = flui_native::HeadlessPlatform::new();
            let headless = Some((platform.owner_turns(), platform.exit_reevaluation()));
            Ok(DedicatedPlatform {
                platform: Box::new(platform),
                headless,
            })
        }
        #[cfg(all(feature = "testing", windows))]
        DedicatedBackend::Win32Qualification => Ok(DedicatedPlatform {
            platform: Box::new(flui_native::WindowsPlatform::try_new().map_err(|error| {
                AppWindowError::Native {
                    source: Arc::new(error),
                }
            })?),
            headless: None,
        }),
    }
}

fn run_owner<V, F>(
    factory: F,
    backend: DedicatedBackend,
    route: OwnerRoute,
    services: Arc<crate::app::runtime::RunServices>,
    completion: Arc<LaunchCompletion>,
) -> Result<(), AppWindowError>
where
    V: View + Clone + 'static,
    F: FnOnce() -> (V, RuntimeWindowConfig) + 'static,
{
    let DedicatedPlatform {
        platform,
        #[cfg(feature = "testing")]
        headless,
    } = make_platform(backend)?;
    let owner_guard = OwnerHostClearGuard::arm();
    let teardown = LocalTeardown;
    let result = platform.run(super::app_handler(move |owner| {
        let proxy = owner.proxy();
        install_owner_platform(owner)?;
        if !route.attach(proxy.clone()) {
            completion.defer_failure(AppWindowError::Cancelled);
            let _ = proxy.request_quit();
            return Ok(());
        }
        APP_RUNTIME.with(|slot| {
            let mut runtime = slot.borrow_mut();
            runtime.main_ingress = Some(Arc::clone(&route.ingress));
            runtime.owner_route = Some(route.clone());
            runtime.dedicated_backend = backend;
            runtime.main_host_lifecycle = flui_scheduler::AppLifecycleState::Resumed;
            runtime.adopt_run_services(&services);
        });
        super::host::install_exit_policy_hook(crate::app::ExitPolicy::OnLastWindowClosed);
        super::route_quit(super::owner_dispatch::request_quit_notification);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(factory));
        let result = match result {
            Ok((root, mut config)) => {
                if services.failure_detail == crate::app::FrameFailureDetail::Redacted {
                    config.window.frame_failure_detail = crate::app::FrameFailureDetail::Redacted;
                }
                let slots = APP_RUNTIME.with(|slot| slot.borrow().capability_slots());
                config
                    .capabilities
                    .into_registry(&slots)
                    .map_err(AppWindowError::from)
                    .and_then(|registry| {
                        if !services
                            .manifest
                            .same_generation(&route.ingress.services().manifest)
                        {
                            return Err(AppWindowError::CapabilityGenerationChanged);
                        }
                        if let Some(missing) = services
                            .manifest
                            .missing_application_registration(&registry.registration_manifest())
                        {
                            return Err(AppWindowError::MissingCapability {
                                name: missing.name(),
                                tier: missing.tier(),
                            });
                        }
                        APP_RUNTIME.with(|slot| slot.borrow_mut().capabilities = Some(registry));
                        super::secondary_window::open_window_with_content_impl(
                            config.window,
                            WindowPolicy::Isolated,
                            root,
                        )
                    })
            }
            Err(payload) => {
                let message = flui_runtime::frame_failure::panic_text(
                    services.failure_detail,
                    payload.as_ref(),
                );
                flui_foundation::panic::discard_payload(payload);
                Err(AppWindowError::FactoryPanicked { message })
            }
        };
        match result {
            Ok(request) => APP_RUNTIME.with(|slot| {
                slot.borrow_mut().dedicated_launch = Some(PendingDedicated {
                    request,
                    completion,
                });
            }),
            Err(error) => {
                completion.defer_failure(error);
                let _ = proxy.request_quit();
            }
        }
        Ok(())
    }));
    #[cfg(feature = "testing")]
    if result.is_ok()
        && let Some((turns, exit)) = headless
    {
        while turns.is_running() {
            turns.drive();
            exit.drive();
            std::thread::park_timeout(std::time::Duration::from_millis(1));
        }
    }
    drop(teardown);
    drop(owner_guard);
    if let Some(payload) = super::terminal::take_terminal() {
        std::panic::resume_unwind(payload);
    }
    result.map_err(|error| AppWindowError::Native {
        source: Arc::new(error),
    })
}
