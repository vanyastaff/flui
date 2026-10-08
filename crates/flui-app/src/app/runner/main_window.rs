//! The designated rendered window belongs to the loop, not its previous ui_runtime.
use super::{
    desktop::{RenderedMain, install_desktop_window},
    host::{
        APP_RUNTIME, OwnerHostClearGuard, install_exit_policy_hook, install_owner_platform,
        install_platform_quit_hook, runtime_wake_callback, with_owner_platform,
    },
    owner_dispatch::teardown_platform_ui_runtime,
};
use crate::app::{
    AppConfig, AppRunError, Application, StartupWindow,
    application::WindowErrorObserver,
    application_control::{AppHandle, AppWindowError, Ingress, contain},
    dev_agent::DevAgent,
    hot_reload::{WorkerReload, WorkerWatcherGuard},
};
use flui_platform::{PendingWindow, PlatformProxy, WindowOpen, traits::HostWindow};
use flui_runtime::dev_agent::DevAgentAttachment;
use flui_view::View;
use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
};

// One owner-local erasure boundary: the generic factory and rendered installer
// stay together. Nothing in this closure crosses a thread boundary.
type Installer = Box<
    dyn FnMut(
        &AppHandle,
        Arc<dyn HostWindow>,
        flui_scheduler::AppLifecycleState,
    ) -> Result<RenderedMain, AppWindowError>,
>;

pub(in crate::app) struct MainController {
    ingress: Arc<Ingress>,
    identity: Arc<()>,
    installer: Option<Installer>,
    config: AppConfig,
    open: Option<RenderedMain>,
    installing: Option<RenderedMain>,
    pending: Option<PendingWindow>,
    pending_wake: Option<Arc<OpenWake>>,
    closing: bool,
    initial: bool,
    error: Option<WindowErrorObserver>,
    fatal: Rc<RefCell<Option<AppRunError>>>,
    watcher: Option<WorkerWatcherGuard>,
    /// The loop's development agent attachment; dropping it detaches the
    /// hook.
    agent: Option<DevAgentAttachment>,
}
impl Drop for MainController {
    fn drop(&mut self) {
        self.cancel_pending();
        let watcher = self.watcher.take();
        contain(|| drop(watcher));
        let agent = self.agent.take();
        contain(|| drop(agent));
        let installer = self.installer.take();
        contain(|| drop(installer));
        let observer = self.error.take();
        contain(|| drop(observer));
    }
}
enum CompletionState {
    Waiting,
    Claimed,
    Failed(AppWindowError),
}
struct OpenWake {
    proxy: PlatformProxy,
    shared: flui_platform::SharedPlatform,
    ingress: std::sync::Weak<Ingress>,
    batch: Arc<()>,
    ready: AtomicBool,
    state: parking_lot::Mutex<CompletionState>,
}
impl OpenWake {
    fn failure(&self) -> Option<AppWindowError> {
        match &*self.state.lock() {
            CompletionState::Failed(error) => Some(error.clone()),
            _ => None,
        }
    }
    fn take_failure(&self) -> Option<AppWindowError> {
        let previous = {
            let mut state = self.state.lock();
            std::mem::replace(&mut *state, CompletionState::Claimed)
        };
        match previous {
            CompletionState::Failed(error) => Some(error),
            CompletionState::Waiting | CompletionState::Claimed => None,
        }
    }
    fn claim(&self) -> bool {
        let mut state = self.state.lock();
        if matches!(*state, CompletionState::Waiting) {
            *state = CompletionState::Claimed;
            true
        } else {
            false
        }
    }
}
impl Wake for OpenWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        if !matches!(*self.state.lock(), CompletionState::Waiting) {
            return;
        }
        self.ready.store(true, Ordering::Release);
        if let Err(source) = self.proxy.wake() {
            let error = AppWindowError::Native {
                source: Arc::new(source),
            };
            {
                let mut state = self.state.lock();
                if !matches!(*state, CompletionState::Waiting) {
                    return;
                }
                *state = CompletionState::Failed(error.clone());
            }
            if let Some(ingress) = self.ingress.upgrade() {
                ingress.fail_pending(&self.batch, error.clone());
            }
            contain(|| tracing::error!(%error, "main-window completion notification failed"));
            // One bounded recovery post; never spin if the operating system refuses progress.
            if let Err(error) = self.proxy.wake() {
                contain(
                    || tracing::error!(%error, "main-window cleanup wake failed; cleanup requires a later owner event or shutdown"),
                );
            }
            self.shared.request_exit_policy_reevaluation();
        }
    }
}
impl MainController {
    fn is_current(&self) -> bool {
        self.ingress.accepting()
            && APP_RUNTIME.with(|slot| Arc::ptr_eq(&slot.borrow().loop_identity, &self.identity))
    }
    fn drive(&mut self) {
        if !self.is_current() {
            self.cancel();
            return;
        }
        if self.installing.is_some() {
            self.finish_install();
            return;
        }
        let removed = self
            .open
            .as_ref()
            .filter(|open| {
                !APP_RUNTIME.with(|slot| {
                    slot.borrow()
                        .installed_host
                        .native()
                        .contains_address(open.address)
                })
            })
            .map(|open| open.address);
        if let Some(address) = removed {
            self.ingress.finish_close(address);
            self.closing = false;
            let removed = self.open.take();
            contain(|| drop(removed));
        }
        if self.closing {
            return;
        }
        if let Some(pending) = self.pending.as_mut() {
            let completion = Arc::clone(
                self.pending_wake
                    .as_ref()
                    .expect("BUG: pending window has a completion waker"),
            );
            if let Some(error) = completion.failure() {
                self.fail(error);
                return;
            }
            if !completion.ready.swap(false, Ordering::AcqRel) {
                return;
            }
            let waker = Waker::from(Arc::clone(&completion));
            let result = Pin::new(pending).poll(&mut Context::from_waker(&waker));
            if let Poll::Ready(result) = result {
                self.pending = None;
                if !completion.claim() {
                    if let Ok(window) = result {
                        contain(|| window.close());
                    }
                    self.fail(completion.failure().unwrap_or(AppWindowError::Cancelled));
                    return;
                }
                match result {
                    Ok(window) => self.install(window),
                    Err(error) => self.fail(AppWindowError::Native {
                        source: Arc::new(error),
                    }),
                }
            }
            return;
        }
        if !self.ingress.begin() {
            return;
        }
        if let Some(open) = &self.open {
            let result =
                open.window
                    .show()
                    .map(|()| open.address)
                    .map_err(|error| AppWindowError::Show {
                        source: Arc::new(error),
                    });
            let still_registered = APP_RUNTIME.with(|slot| {
                slot.borrow()
                    .installed_host
                    .native()
                    .contains_address(open.address)
            });
            if !still_registered {
                self.ingress.finish_close(open.address);
                let closed = self.open.take();
                contain(|| drop(closed));
                self.fail(AppWindowError::Cancelled);
                return;
            }
            if !self.is_current() {
                self.cancel();
                return;
            }
            match result {
                Ok(address) => self.ingress.settle(Ok(address)),
                Err(error) => self.fail(error),
            }
            return;
        }
        // Deferred reveal: this window is installed through
        // `install_desktop_window`, which performs the reveal.
        let options = super::desktop::rendered_window_options(&self.config);
        let opened = with_owner_platform(|owner| owner.open_window(options));
        if !self.is_current() {
            match opened {
                Some(Ok(WindowOpen::Ready(window))) => contain(|| window.close()),
                Some(Ok(WindowOpen::Pending(mut pending))) => {
                    if let Some(Ok(window)) = pending.try_take() {
                        contain(|| window.close());
                    }
                    contain(|| drop(pending));
                }
                _ => {}
            }
            self.cancel();
            return;
        }
        match opened {
            Some(Ok(WindowOpen::Ready(window))) => self.install(window),
            Some(Ok(WindowOpen::Pending(pending))) => {
                self.pending = Some(pending);
                self.pending_wake = Some(Arc::new(OpenWake {
                    proxy: with_owner_platform(flui_platform::OwnerPlatform::proxy)
                        .expect("BUG: current owner"),
                    shared: with_owner_platform(flui_platform::OwnerPlatform::shared)
                        .expect("BUG: current owner"),
                    ingress: Arc::downgrade(&self.ingress),
                    batch: self.ingress.active_batch(),
                    ready: AtomicBool::new(true),
                    state: parking_lot::Mutex::new(CompletionState::Waiting),
                }));
                // Poll once to install the completion waker; no repeated idle polling.
                self.drive();
            }
            Some(Err(error)) => self.fail(AppWindowError::Native {
                source: Arc::new(error),
            }),
            None => self.fail(AppWindowError::Cancelled),
        }
    }
    fn install(&mut self, window: Arc<dyn HostWindow>) {
        if !self.is_current() {
            contain(|| window.close());
            self.cancel();
            return;
        }
        let host = APP_RUNTIME.with(|slot| slot.borrow().main_host_lifecycle);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            (self
                .installer
                .as_mut()
                .expect("BUG: live controller owns installer"))(
                &self.ingress.handle(),
                Arc::clone(&window),
                host,
            )
        }));
        let result = match result {
            Ok(result) => result,
            Err(payload) => {
                let message = flui_runtime::frame_failure::panic_text(
                    self.config.frame_failure_detail,
                    payload.as_ref(),
                );
                std::mem::forget(payload);
                Err(AppWindowError::InstallerPanicked { message })
            }
        };
        match result {
            Ok(open) if self.is_current() => {
                self.installing = Some(open);
                self.finish_install();
            }
            Ok(open) => {
                contain(|| window.close());
                contain(|| drop(open));
                if self.is_current() {
                    self.fail(AppWindowError::Cancelled);
                } else {
                    self.cancel();
                }
            }
            Err(error) => {
                contain(|| window.close());
                if self.is_current() {
                    self.fail(error);
                } else {
                    self.cancel();
                }
            }
        }
    }
    fn finish_install(&mut self) {
        let Some(outcome) = self
            .installing
            .as_ref()
            .and_then(|open| open.installation.outcome())
        else {
            return;
        };
        let open = self
            .installing
            .take()
            .expect("BUG: observed installation owns its window");
        let registered = self.is_current()
            && APP_RUNTIME.with(|slot| {
                slot.borrow()
                    .installed_host
                    .native()
                    .contains_address(open.address)
            });
        match outcome {
            Ok(()) if registered => {
                let address = open.address;
                self.open = Some(open);
                self.initial = false;
                self.ingress.settle(Ok(address));
            }
            outcome => {
                contain(|| open.window.close());
                contain(|| drop(open));
                if self.is_current() {
                    self.fail(match outcome {
                        Err(source) => AppWindowError::Mount {
                            source: Arc::new(source),
                        },
                        Ok(()) => AppWindowError::Cancelled,
                    });
                } else {
                    self.cancel();
                }
            }
        }
    }
    fn cancel_pending(&mut self) {
        if let Some(installing) = self.installing.take() {
            installing.installation.cancel();
            contain(|| installing.window.close());
            contain(|| drop(installing));
        }
        if let Some(mut pending) = self.pending.take() {
            if let Some(Ok(window)) = pending.try_take() {
                contain(|| window.close());
            }
            contain(|| drop(pending));
        }
    }
    fn take_pending_failure(&mut self) -> Option<AppWindowError> {
        // Transfer notification ownership before native cleanup can reenter.
        self.pending_wake
            .take()
            .and_then(|wake| wake.take_failure())
    }
    fn cancel(&mut self) {
        let failure = self.take_pending_failure();
        self.ingress.close();
        self.cancel_pending();
        if let Some(error) = failure {
            self.report_failure(error);
        }
    }
    fn fail(&mut self, error: AppWindowError) {
        let error = self.take_pending_failure().unwrap_or(error);
        self.cancel_pending();
        self.ingress.settle(Err(error.clone()));
        self.report_failure(error);
        with_owner_platform(|owner| owner.shared().request_exit_policy_reevaluation());
    }
    fn report_failure(&mut self, error: AppWindowError) {
        contain(|| tracing::error!(%error, "main window request failed"));
        if self.initial {
            self.initial = false;
            let old = self
                .fatal
                .borrow_mut()
                .replace(AppRunError::InitialWindow(error));
            contain(|| drop(old));
            let _ = self.ingress.handle().request_quit();
        } else if let Some(mut observer) = self.error.take() {
            let observed =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| observer(&error)));
            match observed {
                Ok(()) => self.error = Some(observer),
                Err(payload) => {
                    std::mem::forget(payload);
                    contain(|| drop(observer));
                }
            }
        }
    }
}

/// Restores checked-out controller ownership even if arbitrary user code unwinds.
struct ControllerLease {
    controller: Option<MainController>,
    identity: Arc<()>,
}
impl Drop for ControllerLease {
    fn drop(&mut self) {
        let mut controller = self.controller.take();
        APP_RUNTIME.with(|slot| {
            let mut runtime = slot.borrow_mut();
            if Arc::ptr_eq(&runtime.loop_identity, &self.identity)
                && runtime
                    .main_ingress
                    .as_ref()
                    .is_some_and(|ingress| ingress.accepting())
                && runtime.main_controller.is_none()
            {
                runtime.main_controller = controller.take();
            }
        });
        if let Some(mut controller) = controller {
            contain(|| controller.cancel());
            contain(|| drop(controller));
        }
    }
}
pub(super) fn drive_main_window() {
    let leased = APP_RUNTIME.with(|slot| {
        let mut runtime = slot.borrow_mut();
        if runtime
            .installed_host
            .logical()
            .is_executing()
            .unwrap_or(true)
        {
            return None;
        }
        runtime
            .main_controller
            .take()
            .map(|controller| ControllerLease {
                controller: Some(controller),
                identity: Arc::clone(&runtime.loop_identity),
            })
    });
    let Some(mut lease) = leased else {
        return;
    };
    let controller = lease
        .controller
        .as_mut()
        .expect("BUG: controller lease is populated until Drop");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| controller.drive()));
    if let Err(payload) = result {
        let message = flui_runtime::frame_failure::panic_text(
            controller.config.frame_failure_detail,
            payload.as_ref(),
        );
        std::mem::forget(payload);
        controller.fail(AppWindowError::InstallerPanicked { message });
    }
}
pub(super) fn main_window_closing(address: flui_foundation::PresentationAddress) {
    let ingress = APP_RUNTIME.with(|slot| {
        let mut runtime = slot.borrow_mut();
        if !runtime.installed_host.native().contains_address(address) {
            return None;
        }
        if let Some(controller) = runtime.main_controller.as_mut()
            && controller
                .open
                .as_ref()
                .is_some_and(|open| open.address == address)
        {
            controller.closing = true;
        }
        runtime.main_ingress.clone()
    });
    if let Some(ingress) = ingress {
        ingress.begin_close(address);
    }
}
pub(super) fn main_window_closed(address: flui_foundation::PresentationAddress) {
    let (removed, ingress) = APP_RUNTIME.with(|slot| {
        let mut runtime = slot.borrow_mut();
        // Closing while a ui_runtime is checked out queues disposal. Keep its close
        // fence until the restored dispatcher has actually removed the address.
        if runtime.installed_host.native().contains_address(address) {
            return (None, None);
        }
        let removed = runtime.main_controller.as_mut().and_then(|controller| {
            if !controller
                .open
                .as_ref()
                .is_some_and(|open| open.address == address)
            {
                return None;
            }
            controller.closing = false;
            controller.open.take()
        });
        (removed, runtime.main_ingress.clone())
    });
    contain(|| drop(removed));
    if let Some(ingress) = ingress {
        ingress.finish_close(address);
        ingress.wake_owner();
    }
}
pub(super) fn shutdown_main_window() {
    let (controller, ingress) = APP_RUNTIME.with(|slot| {
        let mut runtime = slot.borrow_mut();
        (runtime.main_controller.take(), runtime.main_ingress.take())
    });
    if let Some(ingress) = ingress {
        ingress.close();
    }
    if let Some(mut controller) = controller {
        contain(|| controller.cancel());
        contain(|| drop(controller));
    }
}

/// Retires the loop's main window and UI runtimes when dropped: after
/// `Platform::run` returns, or while a panic unwinds out of it. Held inside
/// the [`OwnerHostClearGuard`], so the windows go before the owner platform
/// that created them. On unwind each step is contained, so a second panic
/// cannot abort the first one's unwind; that first panic stays the one raised.
struct LoopTeardown;

impl Drop for LoopTeardown {
    fn drop(&mut self) {
        if std::thread::panicking() {
            contain(shutdown_main_window);
            contain(teardown_platform_ui_runtime);
        } else {
            shutdown_main_window();
            teardown_platform_ui_runtime();
        }
    }
}

pub(in crate::app) fn run_application<V, F>(
    application: Application<V, F>,
) -> Result<(), AppRunError>
where
    V: View + Clone + 'static,
    F: FnMut(&AppHandle) -> V + 'static,
{
    let platform = flui_platform::current_platform().map_err(|error| AppRunError::Platform {
        source: Arc::new(error),
    })?;
    run_with_platform(application, platform)
}
fn run_with_platform<V, F>(
    application: Application<V, F>,
    platform: Box<dyn flui_platform::traits::Platform>,
) -> Result<(), AppRunError>
where
    V: View + Clone + 'static,
    F: FnMut(&AppHandle) -> V + 'static,
{
    let fatal = Rc::new(RefCell::new(None));
    let recorded = Rc::clone(&fatal);
    let _owner_guard = OwnerHostClearGuard::arm();
    // Declared after the owner guard, so it drops first: the main window and
    // the ui_runtimes (and the native windows they own) are retired while the
    // owner platform still lives, on unwind too.
    let teardown = LoopTeardown;
    let result = platform.run(Box::new(move |owner| {
        let Application {
            mut factory,
            config,
            startup,
            ready,
            window_error,
            ..
        } = application;
        let proxy = owner.proxy();
        let ingress = Ingress::new(proxy, startup == StartupWindow::Open);
        let handle = ingress.handle();
        install_owner_platform(owner)?;
        APP_RUNTIME.with(|slot| {
            let mut runtime = slot.borrow_mut();
            runtime.main_ingress = Some(Arc::clone(&ingress));
            runtime.main_host_lifecycle = flui_scheduler::AppLifecycleState::Resumed;
            runtime.install_host_storage(&config);
            if let Some(executors) = config.executors.clone() {
                runtime.install_host_executors(executors);
            }
            runtime.reopen_lifecycles();
            runtime.ensure_execution();
        });
        install_exit_policy_hook(config.exit_policy);
        install_platform_quit_hook();
        let reopen = Arc::downgrade(&ingress);
        with_owner_platform(|owner| {
            owner.shared().on_reopen(Box::new(move || {
                if let Some(ingress) = reopen.upgrade() {
                    ingress.request_native_show();
                }
            }));
        });
        let reload = WorkerReload::from_config(&config);
        let watcher = reload.spawn_watcher(runtime_wake_callback());
        let agent = config.dev_agent.as_ref().and_then(DevAgent::attach);
        let installer_config = config.clone();
        let identity = APP_RUNTIME.with(|slot| Arc::clone(&slot.borrow().loop_identity));
        let factory_identity = Arc::clone(&identity);
        let installer = Box::new(move |handle: &AppHandle, window, host| {
            let root = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| factory(handle)))
                .map_err(|payload| {
                let message = flui_runtime::frame_failure::panic_text(
                    installer_config.frame_failure_detail,
                    payload.as_ref(),
                );
                std::mem::forget(payload);
                AppWindowError::FactoryPanicked { message }
            })?;
            if !handle.ingress.accepting()
                || !APP_RUNTIME
                    .with(|slot| Arc::ptr_eq(&slot.borrow().loop_identity, &factory_identity))
            {
                return Err(AppWindowError::Cancelled);
            }
            install_desktop_window(root, &installer_config, reload.clone(), window, host)
        });
        APP_RUNTIME.with(|slot| {
            slot.borrow_mut().main_controller = Some(MainController {
                ingress: Arc::clone(&ingress),
                identity,
                installer: Some(installer),
                config: config.clone(),
                open: None,
                installing: None,
                pending: None,
                pending_wake: None,
                closing: false,
                initial: startup == StartupWindow::Open,
                error: window_error,
                fatal: recorded,
                watcher,
                agent,
            });
        });
        for service in &config.services {
            if let Err(error) = APP_RUNTIME.with(|slot| slot.borrow_mut().start_service(service)) {
                let fatal = APP_RUNTIME.with(|slot| {
                    slot.borrow()
                        .main_controller
                        .as_ref()
                        .map(|controller| Rc::clone(&controller.fatal))
                });
                if let Some(fatal) = fatal {
                    fatal.replace(Some(AppRunError::Service {
                        source: Arc::new(error),
                    }));
                }
                let _ = handle.request_quit();
                return Ok(());
            }
        }
        if let Some(ready) = ready
            && let Err(payload) =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ready(&handle)))
        {
            std::mem::forget(payload);
            APP_RUNTIME.with(|slot| {
                if let Some(controller) = slot.borrow().main_controller.as_ref() {
                    controller
                        .fatal
                        .borrow_mut()
                        .replace(AppRunError::OnReadyPanicked);
                }
            });
            let _ = handle.request_quit();
        }
        drive_main_window();
        with_owner_platform(|owner| owner.shared().request_exit_policy_reevaluation());
        Ok(())
    }));
    drop(teardown);
    let failure = fatal.borrow_mut().take();
    if let Some(error) = failure {
        return Err(error);
    }
    result.map_err(|error| AppRunError::Platform {
        source: Arc::new(error),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::AtomicUsize;

    use flui_platform::{HeadlessPlatform, Platform};

    use crate::app::runtime::ExitPolicy;

    fn install_test_controller(
        owner: flui_platform::OwnerPlatform,
        installer: Installer,
        observer: Option<WindowErrorObserver>,
    ) -> AppHandle {
        let proxy = owner.proxy();
        install_owner_platform(owner).expect("owner signal");
        let ingress = Ingress::new(proxy, false);
        let handle = ingress.handle();
        APP_RUNTIME.with(|slot| {
            let mut runtime = slot.borrow_mut();
            runtime.main_ingress = Some(Arc::clone(&ingress));
            runtime.main_controller = Some(MainController {
                identity: Arc::clone(&runtime.loop_identity),
                ingress,
                installer: Some(installer),
                config: AppConfig::new(),
                open: None,
                installing: None,
                pending: None,
                pending_wake: None,
                closing: false,
                initial: false,
                error: observer,
                fatal: Rc::new(RefCell::new(None)),
                watcher: None,
                agent: None,
            });
        });
        install_platform_quit_hook();
        handle
    }
    fn main_window_pending_coalesces_and_recovers_after_installer_panic() {
        let _owner = OwnerHostClearGuard::arm();
        let _cleanup = LoopTeardown;
        let platform = HeadlessPlatform::new();
        let deferred = platform.enable_deferred_window_open();
        let turns = platform.owner_turns();
        let calls = Rc::new(Cell::new(0));
        let seen = Rc::clone(&calls);
        let saved = Rc::new(RefCell::new(None));
        let output = Rc::clone(&saved);
        Box::new(platform)
            .run(Box::new(move |owner| {
                let handle = install_test_controller(
                    owner,
                    Box::new(move |_, _, _| {
                        seen.set(seen.get() + 1);
                        panic!("injected installer panic");
                    }),
                    None,
                );
                output.replace(Some(handle));
                Ok(())
            }))
            .expect("headless bootstrap");
        let handle = saved.take().expect("control");
        let mut first = handle.request_show_main_window().expect("first");
        let mut second = handle.request_show_main_window().expect("second");
        turns.drive();
        assert_eq!(calls.get(), 0, "native completion is still pending");
        deferred.resolve_next().expect("one native request");
        turns.drive();
        assert_eq!(calls.get(), 1, "coalesced intents invoke one installer");
        assert!(matches!(
            first.try_result(),
            Some(Err(AppWindowError::InstallerPanicked { .. }))
        ));
        assert!(matches!(
            second.try_result(),
            Some(Err(AppWindowError::InstallerPanicked { .. }))
        ));
        let mut next = handle
            .request_show_main_window()
            .expect("recovery admits a fresh intent");
        turns.drive();
        deferred.resolve_next().expect("second native request");
        turns.drive();
        assert_eq!(calls.get(), 2);
        assert!(matches!(
            next.try_result(),
            Some(Err(AppWindowError::InstallerPanicked { .. }))
        ));
    }
    use std::cell::Cell;

    fn main_window_waits_for_deferred_runtime_publication() {
        use super::super::frame_driver::{FrameDriver, TestFrameDriver};
        use super::super::owner_dispatch::{
            RuntimeTask, dispatch_platform_ui_runtime, install_platform_ui_runtime,
            prepare_ui_runtime_alongside,
        };
        use flui_runtime::ui_runtime::UiRuntime;

        let _owner = OwnerHostClearGuard::arm();
        let _cleanup = LoopTeardown;
        Box::new(HeadlessPlatform::new())
            .run(Box::new(move |owner| {
                let handle = install_test_controller(
                    owner,
                    Box::new(|_, window, host| {
                        let runtime = UiRuntime::for_test();
                        runtime
                            .attach_root_widget(&flui_widgets::SizedBox::new(20.0, 30.0))
                            .expect("mount root");
                        let window: Arc<dyn flui_platform::traits::PlatformWindow> = window;
                        let mut prepared =
                            prepare_ui_runtime_alongside(runtime, Arc::clone(&window));
                        let address = prepared.dispatcher().address;
                        prepared
                            .frame_driver(FrameDriver::Test(TestFrameDriver {
                                installed: None,
                                sink: flui_runtime::testing::ScriptedSink::new(|_, _| {
                                    flui_runtime::sink::SubmitVerdict::Presented
                                }),
                                prelude: None,
                                resize: None,
                            }))
                            .expect("prepare frame driver");
                        prepared.lifecycle(host);
                        let installation = prepared.submit();
                        assert!(
                            installation.outcome().is_none(),
                            "completion tail cannot publish during the current owner turn"
                        );
                        Ok(RenderedMain {
                            window,
                            address,
                            installation,
                        })
                    }),
                    None,
                );
                let outer = install_platform_ui_runtime(
                    UiRuntime::for_test(),
                    &crate::app::window_test_support::headless_test_window(),
                );
                let reply = Rc::new(RefCell::new(None));
                let requested = Rc::clone(&reply);
                dispatch_platform_ui_runtime(
                    outer,
                    RuntimeTask::TestCallback(Box::new(move |_| {
                        let mut request = handle
                            .request_show_main_window()
                            .expect("admit show request");
                        assert!(
                            request.try_result().is_none(),
                            "request is not ready during the caller's operation"
                        );
                        *requested.borrow_mut() = Some(request);
                    })),
                )
                .expect("complete caller and installation");
                let result = reply
                    .borrow_mut()
                    .as_mut()
                    .expect("request submitted")
                    .try_result();
                assert!(
                    matches!(result, Some(Ok(_))),
                    "ready follows publication: {result:?}"
                );
                Ok(())
            }))
            .expect("headless bootstrap");
    }

    fn owner_failure_defers_new_window_work_until_recovery() {
        use super::super::owner_dispatch::{
            RuntimeTask, dispatch_platform_ui_runtime, install_platform_ui_runtime,
        };
        let _owner = OwnerHostClearGuard::arm();
        let _cleanup = LoopTeardown;
        Box::new(HeadlessPlatform::new())
            .run(Box::new(move |owner| {
                let calls = Rc::new(Cell::new(0));
                let called = Rc::clone(&calls);
                let handle = install_test_controller(
                    owner,
                    Box::new(move |_, _, _| {
                        called.set(called.get() + 1);
                        Err(AppWindowError::Cancelled)
                    }),
                    None,
                );
                let outer = install_platform_ui_runtime(
                    flui_runtime::ui_runtime::UiRuntime::for_test(),
                    &crate::app::window_test_support::headless_test_window(),
                );
                let reply = Rc::new(RefCell::new(None));
                let requested = Rc::clone(&reply);
                let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    dispatch_platform_ui_runtime(
                        outer,
                        RuntimeTask::TestCallback(Box::new(move |_| {
                            *requested.borrow_mut() = Some(
                                handle
                                    .request_show_main_window()
                                    .expect("admit request before failure"),
                            );
                            panic!("owner callback failed");
                        })),
                    )
                    .expect("admitted callback");
                }))
                .expect_err("preserve callback failure");
                assert_eq!(
                    failure.downcast_ref::<&str>(),
                    Some(&"owner callback failed")
                );
                assert_eq!(calls.get(), 0, "recovery must not invoke a fresh installer");
                assert!(
                    reply
                        .borrow_mut()
                        .as_mut()
                        .expect("accepted request")
                        .try_result()
                        .is_none()
                );
                dispatch_platform_ui_runtime(outer, RuntimeTask::TestCallback(Box::new(|_| {})))
                    .expect("next healthy owner opportunity");
                assert_eq!(calls.get(), 1, "accepted request survives the failed turn");
                assert!(matches!(
                    reply
                        .borrow_mut()
                        .as_mut()
                        .expect("accepted request")
                        .try_result(),
                    Some(Err(AppWindowError::Cancelled))
                ));
                Ok(())
            }))
            .expect("headless recovery host");
    }

    fn main_window_factory_panic_is_typed_and_initial_window_is_fatal() {
        let app = Application::new(|_| -> flui_widgets::Text {
            panic!("factory witness");
        });
        let error = run_with_platform(app, Box::new(HeadlessPlatform::new()))
            .expect_err("initial factory fails");
        assert!(matches!(
            error,
            AppRunError::InitialWindow(AppWindowError::FactoryPanicked { .. })
        ));
        assert!(APP_RUNTIME.with(|slot| slot.borrow().main_ingress.is_none()));
    }

    /// A panic that unwinds out of `Platform::run` after the loop was set up
    /// still retires the main window and the UI runtimes, before the owner
    /// platform goes: the windows they own must not outlive it.
    ///
    /// The panic is raised by a subscriber on the headless platform's last
    /// log line, after `on_ready` returned, the one point in `run` no
    /// containment covers.
    fn main_window_unwinding_out_of_run_retires_the_loop() {
        struct PanicsWhenReady;
        struct Message(bool);
        impl tracing::field::Visit for Message {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.0 |= format!("{value:?}") == "Headless platform ready";
                }
            }
        }
        impl tracing::Subscriber for PanicsWhenReady {
            fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
                true
            }
            fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
                tracing::span::Id::from_u64(1)
            }
            fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
            fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
            fn event(&self, event: &tracing::Event<'_>) {
                let mut message = Message(false);
                event.record(&mut message);
                assert!(!message.0, "injected panic out of Platform::run");
            }
            fn enter(&self, _: &tracing::span::Id) {}
            fn exit(&self, _: &tracing::span::Id) {}
        }

        let app = Application::new(|_| -> flui_widgets::Text { flui_widgets::Text::new("") })
            .with_startup_window(StartupWindow::None)
            .with_config(AppConfig::new().with_exit_policy(ExitPolicy::ExplicitQuit));
        let unwound = tracing::subscriber::with_default(PanicsWhenReady, || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_with_platform(app, Box::new(HeadlessPlatform::new()))
            }))
        });
        assert!(unwound.is_err(), "the injected panic unwinds out of run");
        APP_RUNTIME.with(|slot| {
            let runtime = slot.borrow();
            assert!(
                runtime.main_controller.is_none() && runtime.main_ingress.is_none(),
                "the unwind retired the main window"
            );
        });
    }

    #[test]
    fn main_window_installer_matrix() {
        crate::table_test::run_table(
            "main_window_installer_matrix",
            &[
                ("owner_failure_defers_new_window_work_until_recovery", owner_failure_defers_new_window_work_until_recovery as fn()),
                (
                    "main_window_waits_for_deferred_runtime_publication",
                    main_window_waits_for_deferred_runtime_publication as fn(),
                ),
                (
                    "main_window_pending_coalesces_and_recovers_after_installer_panic",
                    main_window_pending_coalesces_and_recovers_after_installer_panic as fn(),
                ),
                (
                    "main_window_factory_panic_is_typed_and_initial_window_is_fatal",
                    main_window_factory_panic_is_typed_and_initial_window_is_fatal as fn(),
                ),
                (
                    "main_window_unwinding_out_of_run_retires_the_loop",
                    main_window_unwinding_out_of_run_retires_the_loop as fn(),
                ),
                (
                    "main_window_reload_hook_stays_attached_across_failed_reopens_and_detaches_with_loop",
                    main_window_reload_hook_stays_attached_across_failed_reopens_and_detaches_with_loop as fn(),
                ),
                (
                    "main_window_agent_hook_stays_attached_across_failed_reopens_and_detaches_with_loop",
                    main_window_agent_hook_stays_attached_across_failed_reopens_and_detaches_with_loop as fn(),
                ),
            ],
        );
    }

    /// The loop's development reload hook is attached once, when the loop
    /// starts, stays attached while main-window opens fail and are retried,
    /// and is detached exactly once, when the loop ends.
    fn main_window_reload_hook_stays_attached_across_failed_reopens_and_detaches_with_loop() {
        #[derive(Default)]
        struct Counts {
            attaches: AtomicUsize,
            detaches: AtomicUsize,
        }
        impl Counts {
            fn get(&self) -> (usize, usize) {
                (
                    self.attaches.load(Ordering::SeqCst),
                    self.detaches.load(Ordering::SeqCst),
                )
            }
        }
        struct Counting(Arc<Counts>);
        impl flui_view::dev_reload::DevReloadHook for Counting {
            fn attach(&mut self, _wake: flui_view::dev_reload::ReloadWake) {
                self.0.attaches.fetch_add(1, Ordering::SeqCst);
            }
            fn detach(&mut self) {
                self.0.detaches.fetch_add(1, Ordering::SeqCst);
            }
            fn poll(&mut self) -> flui_view::dev_reload::ReloadEvent {
                flui_view::dev_reload::ReloadEvent::Unchanged
            }
        }

        let counts = Arc::new(Counts::default());
        let observed = Arc::clone(&counts);
        let app = Application::new(|_| -> flui_widgets::Text {
            panic!("factory failure before GPU setup");
        })
        .with_startup_window(StartupWindow::None)
        .with_config(
            AppConfig::new()
                .with_exit_policy(ExitPolicy::ExplicitQuit)
                .with_dev_reload(Counting(Arc::clone(&counts))),
        )
        .on_ready(move |handle| {
            assert_eq!(observed.get(), (1, 0), "attached when the loop starts");
            assert!(
                APP_RUNTIME.with(|slot| slot
                    .borrow()
                    .main_controller
                    .as_ref()
                    .expect("controller installed")
                    .watcher
                    .is_some()),
                "the loop holds the attachment"
            );
            for _ in 0..2 {
                let mut request = handle.request_show_main_window().expect("admit retry");
                drive_main_window();
                assert!(matches!(
                    request.try_result(),
                    Some(Err(AppWindowError::FactoryPanicked { .. }))
                ));
                assert_eq!(
                    observed.get(),
                    (1, 0),
                    "a failed open neither re-attaches nor detaches"
                );
            }
        });
        run_with_platform(app, Box::new(HeadlessPlatform::new())).expect("ordinary owner teardown");
        assert_eq!(
            counts.get(),
            (1, 1),
            "loop teardown detached the hook exactly once"
        );
    }

    /// The loop's development agent hook is attached once, when the loop
    /// starts, stays attached while main-window opens fail and are retried,
    /// and is detached exactly once, when the loop ends.
    ///
    /// The factory panics before GPU setup, so no window is ever vended here:
    /// the zero hand-over count pins only that a failed open hands nothing
    /// over, not the vend-then-hand-over order after a committed install,
    /// which runs after GPU setup and no headless test reaches.
    fn main_window_agent_hook_stays_attached_across_failed_reopens_and_detaches_with_loop() {
        #[derive(Default)]
        struct Counts {
            attaches: AtomicUsize,
            detaches: AtomicUsize,
            opened: AtomicUsize,
        }
        impl Counts {
            fn get(&self) -> (usize, usize, usize) {
                (
                    self.attaches.load(Ordering::SeqCst),
                    self.detaches.load(Ordering::SeqCst),
                    self.opened.load(Ordering::SeqCst),
                )
            }
        }
        struct Counting(Arc<Counts>);
        impl flui_view::dev_agent::DevAgentHook for Counting {
            fn attach(&mut self) -> bool {
                self.0.attaches.fetch_add(1, Ordering::SeqCst);
                true
            }
            fn detach(&mut self) {
                self.0.detaches.fetch_add(1, Ordering::SeqCst);
            }
            fn window_opened(&mut self, _window: flui_view::dev_agent::AgentWindow) {
                self.0.opened.fetch_add(1, Ordering::SeqCst);
            }
        }

        let counts = Arc::new(Counts::default());
        let observed = Arc::clone(&counts);
        let app = Application::new(|_| -> flui_widgets::Text {
            panic!("factory failure before GPU setup");
        })
        .with_startup_window(StartupWindow::None)
        .with_config(
            AppConfig::new()
                .with_exit_policy(ExitPolicy::ExplicitQuit)
                .with_dev_agent(Counting(Arc::clone(&counts))),
        )
        .on_ready(move |handle| {
            assert_eq!(observed.get(), (1, 0, 0), "attached when the loop starts");
            assert!(
                APP_RUNTIME.with(|slot| slot
                    .borrow()
                    .main_controller
                    .as_ref()
                    .expect("controller installed")
                    .agent
                    .is_some()),
                "the loop holds the attachment"
            );
            for _ in 0..2 {
                let mut request = handle.request_show_main_window().expect("admit retry");
                drive_main_window();
                assert!(matches!(
                    request.try_result(),
                    Some(Err(AppWindowError::FactoryPanicked { .. }))
                ));
                assert_eq!(
                    observed.get(),
                    (1, 0, 0),
                    "a failed open neither re-attaches, detaches nor hands a window over"
                );
            }
        });
        run_with_platform(app, Box::new(HeadlessPlatform::new())).expect("ordinary owner teardown");
        assert_eq!(
            counts.get(),
            (1, 1, 0),
            "loop teardown detached the hook exactly once"
        );
    }

    /// A storage directory in the main configuration reaches the UI runtimes the
    /// host builds: the run resolves the host's storage once, at start, and
    /// a UI runtime built afterwards holds it in its build owner, which every
    /// `LifecycleContext::storage` under it reads. Driven through
    /// `run_with_platform` itself, so the host's storage comes from the
    /// runner; the UI runtime is an `Isolated` window opened from `on_ready`,
    /// which reaches `host::build_ui_runtime` as every runner site does,
    /// without a GPU.
    #[cfg(feature = "persist")]
    #[test]
    #[ignore = "contract: the host gives a configured storage directory to every ui_runtime it builds"]
    fn a_configured_storage_dir_reaches_lifecycle_context() {
        let reached = Rc::new(Cell::new(None));
        let seen = Rc::clone(&reached);
        let app = Application::new(|_| -> flui_widgets::Text {
            panic!("no main window is opened");
        })
        .with_startup_window(StartupWindow::None)
        .with_config(
            AppConfig::new()
                .with_exit_policy(ExitPolicy::ExplicitQuit)
                .with_storage_dir(flui_platform_api::StorageName::from_static(
                    "storage-host-test",
                )),
        )
        .on_ready(move |_| {
            let (dispatcher, _window) = super::super::secondary_window::open_secondary_window_impl(
                AppConfig::default(),
                crate::app::runtime::WindowPolicy::Isolated,
            )
            .expect("WindowPolicy::Isolated installs a ui_runtime")
            .expect("headless window publishes synchronously");
            let observed = Rc::clone(&seen);
            super::super::owner_dispatch::dispatch_platform_ui_runtime(
                dispatcher,
                super::super::owner_dispatch::RuntimeTask::TestCallback(Box::new(
                    move |ui_runtime| {
                        observed.set(Some(
                            ui_runtime
                                .widgets()
                                .with_build_owner(|owner| owner.storage().is_some()),
                        ));
                    },
                )),
            )
            .expect("observe storage through the installed owner");
        });
        run_with_platform(app, Box::new(HeadlessPlatform::new())).expect("ordinary owner teardown");

        assert_eq!(
            reached.get(),
            Some(true),
            "a ui_runtime built after the host started with a storage directory holds storage"
        );
    }
}
