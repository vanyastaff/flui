//! The designated rendered window belongs to the loop, not its previous realm.
use super::{
    desktop::{RenderedMain, install_desktop_window},
    host::{
        APP_RUNTIME, OwnerHostClearGuard, install_exit_policy_hook, install_owner_platform,
        install_platform_quit_hook, runtime_wake_callback, with_owner_platform,
    },
    realm_dispatch::teardown_platform_realm,
};
use crate::app::{
    AppConfig, AppRunError, Application, StartupWindow,
    application::WindowErrorObserver,
    application_control::{AppHandle, AppWindowError, Ingress, contain},
    hot_reload::{WorkerReload, WorkerWatcherGuard},
};
use flui_platform::{PendingWindow, PlatformProxy, WindowOpen, traits::HostWindow};
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
    pending: Option<PendingWindow>,
    pending_wake: Option<Arc<OpenWake>>,
    closing: bool,
    initial: bool,
    error: Option<WindowErrorObserver>,
    fatal: Rc<RefCell<Option<AppRunError>>>,
    watcher: Option<WorkerWatcherGuard>,
}
impl Drop for MainController {
    fn drop(&mut self) {
        let watcher = self.watcher.take();
        contain(|| drop(watcher));
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
        let removed = self
            .open
            .as_ref()
            .filter(|open| {
                !APP_RUNTIME.with(|slot| slot.borrow().registry.contains_address(open.address))
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
            let still_registered =
                APP_RUNTIME.with(|slot| slot.borrow().registry.contains_address(open.address));
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
            Ok(open)
                if self.is_current()
                    && APP_RUNTIME
                        .with(|slot| slot.borrow().registry.contains_address(open.address)) =>
            {
                let address = open.address;
                self.open = Some(open);
                self.initial = false;
                self.ingress.settle(Ok(address));
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
    fn cancel_pending(&mut self) {
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
        if runtime.dispatched_realm_id.is_some() || runtime.iterating_all_realms {
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
        if !runtime.registry.contains_address(address) {
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
        // Closing while a realm is checked out queues disposal. Keep its close
        // fence until the restored dispatcher has actually removed the address.
        if runtime.registry.contains_address(address) {
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
        let ingress = Ingress::new(proxy.clone(), startup == StartupWindow::Open);
        let handle = ingress.handle();
        install_owner_platform(owner)?;
        APP_RUNTIME.with(|slot| {
            let mut runtime = slot.borrow_mut();
            runtime.main_ingress = Some(Arc::clone(&ingress));
            runtime.main_host_lifecycle = flui_scheduler::AppLifecycleState::Resumed;
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
                pending: None,
                pending_wake: None,
                closing: false,
                initial: startup == StartupWindow::Open,
                error: window_error,
                fatal: recorded,
                watcher,
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
    shutdown_main_window();
    teardown_platform_realm();
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

    use flui_platform::{HeadlessPlatform, Platform};

    fn install_test_controller(
        owner: flui_platform::OwnerPlatform,
        installer: Installer,
        observer: Option<WindowErrorObserver>,
    ) -> AppHandle {
        let proxy = owner.proxy();
        install_owner_platform(owner).expect("owner signal");
        let ingress = Ingress::new(proxy.clone(), false);
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
                pending: None,
                pending_wake: None,
                closing: false,
                initial: false,
                error: observer,
                fatal: Rc::new(RefCell::new(None)),
                watcher: None,
            });
        });
        install_platform_quit_hook();
        handle
    }
    struct Cleanup;
    impl Drop for Cleanup {
        fn drop(&mut self) {
            shutdown_main_window();
            teardown_platform_realm();
        }
    }

    #[test]
    fn main_window_pending_coalesces_and_recovers_after_installer_panic() {
        let _owner = OwnerHostClearGuard::arm();
        let _cleanup = Cleanup;
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

    #[test]
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
}
