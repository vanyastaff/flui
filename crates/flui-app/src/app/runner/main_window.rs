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
use flui_platform::{PendingWindow, PlatformProxy, WindowOpen, traits::PlatformWindow};
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
        Arc<dyn PlatformWindow>,
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
        let options = (&self.config).into();
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
    fn install(&mut self, window: Arc<dyn PlatformWindow>) {
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
                let message = self
                    .config
                    .frame_failure_detail
                    .panic_text(payload.as_ref())
                    .0;
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
        let message = controller
            .config
            .frame_failure_detail
            .panic_text(payload.as_ref())
            .0;
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
        let clipboard = with_owner_platform(|owner| owner.shared().clipboard())
            .expect("BUG: owner installed above");
        APP_RUNTIME.with(|slot| slot.borrow().set_platform_clipboard(clipboard));
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
                let message = installer_config
                    .frame_failure_detail
                    .panic_text(payload.as_ref())
                    .0;
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
    use crate::app::ExitPolicy;
    use flui_platform::{HeadlessPlatform, Platform};
    use std::sync::atomic::AtomicUsize;

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
    fn main_window_error_observer_panic_retires_it_without_poisoning_admission() {
        let _owner = OwnerHostClearGuard::arm();
        let _cleanup = Cleanup;
        let platform = HeadlessPlatform::new();
        let turns = platform.owner_turns();
        let observed = Rc::new(Cell::new(0));
        let witness = Rc::clone(&observed);
        let saved = Rc::new(RefCell::new(None));
        let output = Rc::clone(&saved);
        Box::new(platform)
            .run(Box::new(move |owner| {
                let handle = install_test_controller(
                    owner,
                    Box::new(|_, _, _| Err(AppWindowError::Cancelled)),
                    Some(Box::new(move |_| {
                        witness.set(witness.get() + 1);
                        panic!("observer failure");
                    })),
                );
                output.replace(Some(handle));
                Ok(())
            }))
            .expect("bootstrap");
        let handle = saved.take().expect("control");
        for _ in 0..2 {
            let mut request = handle
                .request_show_main_window()
                .expect("admission survives");
            turns.drive();
            assert!(matches!(
                request.try_result(),
                Some(Err(AppWindowError::Cancelled))
            ));
        }
        assert_eq!(observed.get(), 1);
    }
    #[test]
    fn main_window_shutdown_isolates_factory_and_observer_capture_destruction() {
        const CHILD: &str = "FLUI_MAIN_CAPTURE_DROP_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let status = std::process::Command::new(std::env::current_exe().expect("test binary"))
                .args(["--exact", "app::runner::main_window::tests::main_window_shutdown_isolates_factory_and_observer_capture_destruction", "--nocapture"])
                .env(CHILD, "1")
                .status().expect("child starts");
            assert!(
                status.success(),
                "capture cleanup must not abort normal shutdown"
            );
            return;
        }
        struct Hostile(Rc<Cell<usize>>);
        impl Drop for Hostile {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
                panic!("independent capture destruction");
            }
        }
        let drops = Rc::new(Cell::new(0));
        let factory_capture = Hostile(Rc::clone(&drops));
        let observer_capture = Hostile(Rc::clone(&drops));
        let app = Application::new(move |_| {
            let _capture = &factory_capture;
            flui_widgets::Text::new("not mounted")
        })
        .with_startup_window(StartupWindow::None)
        .on_window_error(move |_| {
            let _capture = &observer_capture;
        })
        .on_ready(|handle| handle.request_quit().expect("quit"));
        run_with_platform(app, Box::new(HeadlessPlatform::new())).expect("normal shutdown");
        assert_eq!(drops.get(), 2);
    }
    #[test]
    fn main_window_ready_quit_suppresses_reserved_startup_factory() {
        let calls = Rc::new(Cell::new(0));
        let factory_calls = Rc::clone(&calls);
        let app = Application::new(move |_| {
            factory_calls.set(factory_calls.get() + 1);
            flui_widgets::Text::new("never mounted")
        })
        .on_ready(|handle| handle.request_quit().expect("quit admitted"));
        run_with_platform(app, Box::new(HeadlessPlatform::new())).expect("ordinary return");
        assert_eq!(calls.get(), 0);
    }
    #[test]
    fn main_window_failed_post_reports_once_after_terminal_cleanup() {
        struct ExitOnFailure {
            handle: AppHandle,
            automatic: bool,
            rejected: Arc<AtomicBool>,
        }
        impl Wake for ExitOnFailure {
            fn wake(self: Arc<Self>) {
                self.wake_by_ref();
            }
            fn wake_by_ref(self: &Arc<Self>) {
                if self.automatic {
                    self.rejected
                        .store(!self.handle.ingress.try_auto_quit(), Ordering::Release);
                } else {
                    self.rejected
                        .store(self.handle.request_quit().is_err(), Ordering::Release);
                }
            }
        }
        for mode in 0..3 {
            for observer_panics in [false, true] {
                let _owner = OwnerHostClearGuard::arm();
                let _cleanup = Cleanup;
                let platform = HeadlessPlatform::new();
                let deferred = platform.enable_deferred_window_open();
                let turns = platform.owner_turns();
                let saved = Rc::new(RefCell::new(None::<AppHandle>));
                let output = Rc::clone(&saved);
                let observer_handle = Rc::clone(&saved);
                let observations = Rc::new(Cell::new(0));
                let observed = Rc::clone(&observations);
                let admitted_after_quit = Rc::new(Cell::new(false));
                let late_admission = Rc::clone(&admitted_after_quit);
                let quit_rejected = Arc::new(AtomicBool::new(false));
                let closes = Arc::new(AtomicUsize::new(0));
                let closed_before_observer = Arc::clone(&closes);
                Box::new(platform)
                    .run(Box::new(move |owner| {
                        output.replace(Some(install_test_controller(
                            owner,
                            Box::new(|_, _, _| panic!("failed completion must not install")),
                            Some(Box::new(move |error| {
                                assert!(matches!(error, AppWindowError::Native { .. }));
                                assert_eq!(closed_before_observer.load(Ordering::SeqCst), 1);
                                observed.set(observed.get() + 1);
                                if mode != 2 {
                                    late_admission.set(
                                        observer_handle
                                            .borrow()
                                            .as_ref()
                                            .expect("handle")
                                            .request_show_main_window()
                                            .is_ok(),
                                    );
                                }
                                drive_main_window();
                                assert!(!observer_panics, "observer panic after recording");
                            })),
                        )));
                        Ok(())
                    }))
                    .expect("bootstrap");
                let handle = saved.borrow().as_ref().expect("handle").clone();
                let mut request = handle.request_show_main_window().expect("show");
                if mode != 2 {
                    let waker = Waker::from(Arc::new(ExitOnFailure {
                        handle: handle.clone(),
                        automatic: mode == 0,
                        rejected: Arc::clone(&quit_rejected),
                    }));
                    assert!(
                        Pin::new(&mut request)
                            .poll(&mut Context::from_waker(&waker))
                            .is_pending()
                    );
                }
                turns.drive();
                turns.fail_next_wake();
                let window = deferred.resolve_next().expect("resolve");
                let closed = Arc::clone(&closes);
                window.on_close(Box::new(move || {
                    closed.fetch_add(1, Ordering::SeqCst);
                }));
                assert!(matches!(
                    request.try_result(),
                    Some(Err(AppWindowError::Native { .. }))
                ));
                assert!(!quit_rejected.load(Ordering::Acquire));
                if mode == 1 {
                    drive_main_window();
                } // !is_current cancellation entrance
                turns.drive();
                assert_eq!(
                    observations.get(),
                    1,
                    "failure observer must survive terminal cancellation"
                );
                assert_eq!(closes.load(Ordering::SeqCst), 1);
                assert!(!admitted_after_quit.get());
                if mode == 2 {
                    handle.request_quit().expect("ordinary recovery then quit");
                }
                turns.drive();
                shutdown_main_window();
                assert_eq!(
                    observations.get(),
                    1,
                    "consumed failure must never be replayed"
                );
            }
        }
    }
    #[test]
    fn main_window_failed_completion_post_settles_without_unrelated_input() {
        let _owner = OwnerHostClearGuard::arm();
        let _cleanup = Cleanup;
        let platform = HeadlessPlatform::new();
        let deferred = Arc::new(platform.enable_deferred_window_open());
        let turns = platform.owner_turns();
        let saved = Rc::new(RefCell::new(None));
        let output = Rc::clone(&saved);
        let installs = Rc::new(Cell::new(0));
        let observed = Rc::clone(&installs);
        Box::new(platform)
            .run(Box::new(move |owner| {
                output.replace(Some(install_test_controller(
                    owner,
                    Box::new(move |_, _, _| {
                        observed.set(observed.get() + 1);
                        Err(AppWindowError::Cancelled)
                    }),
                    None,
                )));
                Ok(())
            }))
            .expect("bootstrap");
        let handle = saved.take().expect("handle");
        let mut request = handle.request_show_main_window().expect("admit");
        turns.drive();
        let old_waker = APP_RUNTIME.with(|slot| {
            Arc::clone(
                slot.borrow()
                    .main_controller
                    .as_ref()
                    .expect("controller")
                    .pending_wake
                    .as_ref()
                    .expect("waker"),
            )
        });
        turns.fail_next_wake();
        let worker_completion = Arc::clone(&deferred);
        let window =
            std::thread::spawn(move || worker_completion.resolve_next().expect("completion"))
                .join()
                .expect("worker");
        let closes = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&closes);
        window.on_close(Box::new(move || {
            observed.fetch_add(1, Ordering::SeqCst);
        }));
        assert!(
            matches!(
                request.try_result(),
                Some(Err(AppWindowError::Native { .. }))
            ),
            "failed post must settle without an unrelated owner event"
        );
        turns.drive(); // Only the recovery notification may schedule this turn.
        assert_eq!(closes.load(Ordering::SeqCst), 1);
        assert_eq!(installs.get(), 0);
        let mut next = handle
            .request_show_main_window()
            .expect("new batch admitted");
        turns.drive();
        old_waker.wake_by_ref();
        assert!(
            next.try_result().is_none(),
            "old failure cannot settle replacement batch"
        );
        deferred.resolve_next().expect("next native completion");
        turns.drive();
        assert!(matches!(
            next.try_result(),
            Some(Err(AppWindowError::Cancelled))
        ));
        assert_eq!(installs.get(), 1, "later batch reaches installer");
        handle.request_quit().expect("quit remains usable");
        turns.drive();
    }
    #[test]
    fn main_window_quit_closes_ready_unpolled_window_and_cancels_reply() {
        let _owner = OwnerHostClearGuard::arm();
        let _cleanup = Cleanup;
        let platform = HeadlessPlatform::new();
        let deferred = platform.enable_deferred_window_open();
        let turns = platform.owner_turns();
        let saved = Rc::new(RefCell::new(None));
        let output = Rc::clone(&saved);
        Box::new(platform)
            .run(Box::new(move |owner| {
                output.replace(Some(install_test_controller(
                    owner,
                    Box::new(|_, _, _| panic!("must not install after quit")),
                    None,
                )));
                Ok(())
            }))
            .expect("bootstrap");
        let handle = saved.take().expect("control");
        let mut request = handle.request_show_main_window().expect("admit");
        turns.drive();
        let window = deferred.resolve_next().expect("native completion");
        let closes = Arc::new(AtomicUsize::new(0));
        let closed = Arc::clone(&closes);
        window.on_close(Box::new(move || {
            closed.fetch_add(1, Ordering::SeqCst);
        }));
        handle.request_quit().expect("quit takes priority");
        turns.drive();
        assert_eq!(closes.load(Ordering::SeqCst), 1);
        assert!(matches!(
            request.try_result(),
            Some(Err(AppWindowError::Cancelled))
        ));
        assert!(handle.request_show_main_window().is_err());
    }

    #[test]
    fn main_window_suspended_loop_snapshot_reaches_new_installer() {
        let _owner = OwnerHostClearGuard::arm();
        let _cleanup = Cleanup;
        let platform = HeadlessPlatform::new();
        let turns = platform.owner_turns();
        let observed = Rc::new(RefCell::new(Vec::new()));
        let witness = Rc::clone(&observed);
        let saved = Rc::new(RefCell::new(None));
        let output = Rc::clone(&saved);
        Box::new(platform)
            .run(Box::new(move |owner| {
                output.replace(Some(install_test_controller(
                    owner,
                    Box::new(move |_, _, host| {
                        witness.borrow_mut().push(host);
                        Err(AppWindowError::Cancelled)
                    }),
                    None,
                )));
                APP_RUNTIME.with(|slot| {
                    slot.borrow_mut().main_host_lifecycle =
                        flui_scheduler::AppLifecycleState::Paused;
                });
                Ok(())
            }))
            .expect("bootstrap");
        let handle = saved.take().expect("control");
        let _request = handle.request_show_main_window().expect("admit");
        turns.drive();
        assert_eq!(
            &*observed.borrow(),
            &[flui_scheduler::AppLifecycleState::Paused]
        );
    }

    #[test]
    fn main_window_reentrant_close_during_show_drops_stale_open_and_recreates() {
        let _owner = OwnerHostClearGuard::arm();
        let _cleanup = Cleanup;
        let platform = HeadlessPlatform::new();
        let turns = platform.owner_turns();
        let calls = Rc::new(Cell::new(0));
        let installations = Rc::clone(&calls);
        let saved = Rc::new(RefCell::new(None));
        let output = Rc::clone(&saved);
        Box::new(platform)
            .run(Box::new(move |owner| {
                let handle = install_test_controller(
                    owner,
                    Box::new(move |_, window, host| {
                        installations.set(installations.get() + 1);
                        // This fixture exercises controller ownership with a registered
                        // realm; the separate native fixture proves actual GPU rendering.
                        let realm = crate::app::ui_realm::UiRealm::new(
                            Arc::new(|| {}),
                            Arc::clone(&window),
                            1.0,
                            Arc::new(AtomicBool::new(false)),
                        )
                        .expect("realm");
                        realm.enter(|realm| realm.update_host_lifecycle(host));
                        let sender = realm.command_sender();
                        let dispatch =
                            super::super::realm_dispatch::install_realm_alongside(realm, &window)
                                .expect("install realm");
                        let id = window.id();
                        window.on_close(Box::new(move || {
                            main_window_closing(dispatch.address);
                            super::super::realm_dispatch::close_this_window(dispatch);
                            main_window_closed(dispatch.address);
                        }));
                        let wrapped: Arc<dyn PlatformWindow> = Arc::new(
                            crate::app::window_test_support::TestWindow::new()
                                .with_id(id.0)
                                .with_show_callback(Arc::new(move || window.close())),
                        );
                        Ok(RenderedMain {
                            window: wrapped,
                            address: dispatch.address,
                            _rebuild_registration: WorkerReload::from_config(&AppConfig::new())
                                .register_rebuild_hook(sender),
                        })
                    }),
                    None,
                );
                output.replace(Some(handle));
                Ok(())
            }))
            .expect("bootstrap");
        let handle = saved.take().expect("control");
        let mut first = handle.request_show_main_window().expect("first");
        turns.drive();
        assert!(matches!(first.try_result(), Some(Ok(_))));
        let mut reveal = handle.request_show_main_window().expect("show existing");
        turns.drive();
        assert!(matches!(
            reveal.try_result(),
            Some(Err(AppWindowError::Cancelled))
        ));
        assert_eq!(calls.get(), 1);
        let mut next = handle
            .request_show_main_window()
            .expect("recreate after callback close");
        turns.drive();
        assert!(matches!(next.try_result(), Some(Ok(_))));
        assert_eq!(
            calls.get(),
            2,
            "closed window must not remain the main target"
        );
    }

    #[test]
    fn main_window_show_requested_during_close_waits_for_fresh_install() {
        for deferred_disposal in [false, true] {
            let _owner = OwnerHostClearGuard::arm();
            let _cleanup = Cleanup;
            let platform = HeadlessPlatform::new();
            let turns = platform.owner_turns();
            let calls = Rc::new(Cell::new(0));
            let installations = Rc::clone(&calls);
            let requested = Arc::new(parking_lot::Mutex::new(Vec::new()));
            let next_requests = Arc::clone(&requested);
            let saved = Rc::new(RefCell::new(None));
            let output = Rc::clone(&saved);
            Box::new(platform)
                .run(Box::new(move |owner| {
                    let handle = install_test_controller(
                        owner,
                        Box::new(move |handle, window, host| {
                            installations.set(installations.get() + 1);
                            // This fixture exercises controller ownership with a registered
                            // realm; the separate native fixture proves actual GPU rendering.
                            let realm = crate::app::ui_realm::UiRealm::new(
                                Arc::new(|| {}),
                                Arc::clone(&window),
                                1.0,
                                Arc::new(AtomicBool::new(false)),
                            )
                            .expect("realm");
                            realm.enter(|realm| realm.update_host_lifecycle(host));
                            let sender = realm.command_sender();
                            let dispatch = super::super::realm_dispatch::install_realm_alongside(
                                realm, &window,
                            )
                            .expect("install realm");
                            let id = window.id();
                            let control = handle.clone();
                            let requests = Arc::clone(&next_requests);
                            window.on_close(Box::new(move || {
                                main_window_closing(dispatch.address);
                                let during = control
                                    .request_show_main_window()
                                    .expect("show during close admitted");
                                requests.lock().push(during);
                                super::super::realm_dispatch::close_this_window(dispatch);
                                main_window_closed(dispatch.address);
                                let after = control
                                    .request_show_main_window()
                                    .expect("show after close before old reveal returns");
                                requests.lock().push(after);
                            }));
                            let wrapped: Arc<dyn PlatformWindow> = Arc::new(
                                crate::app::window_test_support::TestWindow::new()
                                    .with_id(id.0)
                                    .with_show_callback(Arc::new(move || {
                                        if deferred_disposal {
                                            let closing = Arc::clone(&window);
                                            super::super::realm_dispatch::dispatch_platform_realm(
                                                dispatch,
                                                super::super::realm_dispatch::RealmTask::Frame(
                                                    Box::new(move |_| closing.close()),
                                                ),
                                            )
                                            .expect("owner dispatch");
                                        } else {
                                            window.close();
                                        }
                                    })),
                            );
                            Ok(RenderedMain {
                                window: wrapped,
                                address: dispatch.address,
                                _rebuild_registration: WorkerReload::from_config(&AppConfig::new())
                                    .register_rebuild_hook(sender),
                            })
                        }),
                        None,
                    );
                    output.replace(Some(handle));
                    Ok(())
                }))
                .expect("bootstrap");
            let handle = saved.take().expect("control");
            let mut first = handle.request_show_main_window().expect("first");
            turns.drive();
            assert!(matches!(first.try_result(), Some(Ok(_))));
            let mut reveal = handle.request_show_main_window().expect("show existing");
            turns.drive();
            assert!(matches!(
                reveal.try_result(),
                Some(Err(AppWindowError::Cancelled))
            ));
            assert_eq!(calls.get(), 1);
            let mut pending = std::mem::take(&mut *requested.lock());
            assert_eq!(pending.len(), 2, "both close phases admitted requests");
            assert!(
                pending.iter_mut().all(|reply| reply.try_result().is_none()),
                "old reveal must not settle the next-generation requests"
            );
            turns.drive();
            assert!(
                pending
                    .iter_mut()
                    .all(|reply| matches!(reply.try_result(), Some(Ok(_))))
            );
            assert_eq!(
                calls.get(),
                2,
                "closed window must not remain the main target"
            );
        }
    }

    #[test]
    fn main_window_loop_replacement_during_install_cancels_old_result_without_restoring_old_controller()
     {
        let _owner = OwnerHostClearGuard::arm();
        let _cleanup = Cleanup;
        let platform = HeadlessPlatform::new();
        let turns = platform.owner_turns();
        let closes = Arc::new(AtomicUsize::new(0));
        let closed = Arc::clone(&closes);
        let saved = Rc::new(RefCell::new(None));
        let output = Rc::clone(&saved);
        Box::new(platform)
            .run(Box::new(move |owner| {
                output.replace(Some(install_test_controller(
                    owner,
                    Box::new(move |_, window, _| {
                        let closed = Arc::clone(&closed);
                        window.on_close(Box::new(move || {
                            closed.fetch_add(1, Ordering::SeqCst);
                        }));
                        Box::new(HeadlessPlatform::new())
                            .run(Box::new(|owner| {
                                install_owner_platform(owner)?;
                                Ok(())
                            }))
                            .expect("replacement loop");
                        Err(AppWindowError::Cancelled)
                    }),
                    None,
                )));
                Ok(())
            }))
            .expect("bootstrap");
        let handle = saved.take().expect("control");
        let mut request = handle.request_show_main_window().expect("admit old intent");
        turns.drive();
        assert_eq!(closes.load(Ordering::SeqCst), 1);
        assert!(matches!(
            request.try_result(),
            Some(Err(AppWindowError::Cancelled))
        ));
        assert!(handle.request_show_main_window().is_err());
        assert!(APP_RUNTIME.with(|slot| slot.borrow().main_controller.is_none()));
        assert!(with_owner_platform(|_| true).expect("replacement owner remains installed"));
    }

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

    #[test]
    fn main_window_error_observer_capture_drop_panic_does_not_leak_reservation() {
        struct HostileDrop(Rc<Cell<usize>>);
        impl Drop for HostileDrop {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
                panic!("observer capture drop");
            }
        }
        let _owner = OwnerHostClearGuard::arm();
        let _cleanup = Cleanup;
        let platform = HeadlessPlatform::new();
        let turns = platform.owner_turns();
        let drops = Rc::new(Cell::new(0));
        let capture = HostileDrop(Rc::clone(&drops));
        let saved = Rc::new(RefCell::new(None));
        let output = Rc::clone(&saved);
        Box::new(platform)
            .run(Box::new(move |owner| {
                output.replace(Some(install_test_controller(
                    owner,
                    Box::new(|_, _, _| Err(AppWindowError::Cancelled)),
                    Some(Box::new(move |_| {
                        let _capture = &capture;
                        panic!("observer body");
                    })),
                )));
                Ok(())
            }))
            .expect("bootstrap");
        let handle = saved.take().expect("control");
        let mut request = handle.request_show_main_window().expect("admit");
        turns.drive();
        assert_eq!(drops.get(), 1);
        assert!(matches!(
            request.try_result(),
            Some(Err(AppWindowError::Cancelled))
        ));
        assert!(
            handle.request_show_main_window().is_ok(),
            "reservation released after both panics"
        );
    }

    #[cfg(feature = "hot-reload")]
    #[test]
    fn main_window_active_artifact_watcher_survives_failed_reopens_and_stops_with_loop() {
        let lifetime = Rc::new(RefCell::new(None));
        let captured = Rc::clone(&lifetime);
        let app = Application::new(|_| -> flui_widgets::Text {
            panic!("factory failure before GPU setup");
        })
        .with_startup_window(StartupWindow::None)
        .with_config(
            AppConfig::new()
                .with_exit_policy(ExitPolicy::ExplicitQuit)
                .with_worker_plugin_path("/nonexistent/flui-resident-test-worker.dylib"),
        )
        .on_ready(move |handle| {
            let initial = APP_RUNTIME.with(|slot| {
                slot.borrow()
                    .main_controller
                    .as_ref()
                    .expect("controller installed")
                    .watcher
                    .as_ref()
                    .expect("real configured watcher")
                    .test_lifetime()
            });
            assert!(!initial.1.load(Ordering::Acquire));
            for _ in 0..2 {
                let mut request = handle.request_show_main_window().expect("admit retry");
                drive_main_window();
                assert!(matches!(
                    request.try_result(),
                    Some(Err(AppWindowError::FactoryPanicked { .. }))
                ));
                let current = APP_RUNTIME.with(|slot| {
                    slot.borrow()
                        .main_controller
                        .as_ref()
                        .expect("controller restored")
                        .watcher
                        .as_ref()
                        .expect("watcher retained")
                        .test_lifetime()
                });
                assert_eq!(
                    current.0, initial.0,
                    "same actual worker thread across retries"
                );
                assert!(Arc::ptr_eq(&current.1, &initial.1));
                assert!(!current.1.load(Ordering::Acquire));
            }
            captured.replace(Some(initial));
        });
        run_with_platform(app, Box::new(HeadlessPlatform::new())).expect("ordinary owner teardown");
        let (_, stopped) = lifetime.take().expect("observed real watcher");
        assert!(
            stopped.load(Ordering::Acquire),
            "loop teardown stopped and joined the real watcher"
        );
    }

    #[test]
    fn main_window_empty_start_services_once_without_factory() {
        let starts = Arc::new(AtomicUsize::new(0));
        let started = Arc::clone(&starts);
        let config = AppConfig::new()
            .with_exit_policy(ExitPolicy::ExplicitQuit)
            .with_service(crate::app::ServiceDefinition::new(
                "resident-test",
                crate::app::ServiceLifetime::KeepsAppAlive,
                move |context| {
                    started.fetch_add(1, Ordering::SeqCst);
                    Box::pin(async move {
                        context.cancellation().cancelled().await;
                    })
                },
            ));
        let factories = Rc::new(Cell::new(0));
        let called = Rc::clone(&factories);
        let app = Application::new(move |_| {
            called.set(called.get() + 1);
            flui_widgets::Text::new("unused")
        })
        .with_config(config)
        .with_startup_window(StartupWindow::None);
        run_with_platform(app, Box::new(HeadlessPlatform::new()))
            .expect("headless owner lifetime completes");
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        assert_eq!(factories.get(), 0);
    }
}
