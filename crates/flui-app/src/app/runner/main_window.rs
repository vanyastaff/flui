//! The designated rendered window belongs to the loop, not its previous ui_runtime.
#[cfg(test)]
#[path = "host_delivery_tests.rs"]
mod host_delivery_tests;
use super::raster_host::RasterFactory;
use super::{
    desktop::{OpeningWindow, RenderedMain, install_desktop_window},
    host::{
        APP_RUNTIME, OwnerHostClearGuard, install_exit_policy_hook, install_owner_platform,
        install_platform_quit_hook, runtime_wake_callback, with_owner_platform,
    },
    owner_dispatch::{ExitWatchdog, TeardownStep, teardown_platform_ui_runtime_logged},
};
use crate::app::agent::Disconnect;
use crate::app::{
    AppConfig, LaunchError,
    application::WindowErrorObserver,
    application::{DesktopRun, StartupWindow},
    application_control::{AppHandle, AppWindowError, Ingress, contain},
    hot_reload::LoopReload,
};
use flui_engine::RasterPlacement;
use flui_native::{HostWindow, PendingWindow, PlatformProxy, WindowOpen};
use flui_view::View;
use flui_view::dev_reload::{CodeVersion, Unapplied};
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
        Rc<dyn HostWindow>,
        flui_scheduler::AppLifecycleState,
        OpeningWindow,
    ) -> Result<RenderedMain, AppWindowError>,
>;

/// Where the install into the window the loop kept stands: the restart's
/// outcome waits for it, since an install commits in a later owner turn.
enum Adopting {
    No,
    Installing,
    Installed,
    Failed(AppWindowError),
}

pub(in crate::app) struct MainController {
    ingress: Arc<Ingress>,
    identity: Arc<()>,
    installer: Option<Installer>,
    config: AppConfig,
    open: Option<RenderedMain>,
    installing: Option<RenderedMain>,
    pending: Option<PendingWindow>,
    /// What the window being opened carries, until it is installed.
    opening: Option<OpeningWindow>,
    /// The window waiting for the read of its placement before it opens.
    recalling: Option<OpeningWindow>,
    pending_wake: Option<Arc<OpenWake>>,
    closing: bool,
    initial: bool,
    /// While the composition installs into the window the loop kept
    /// (`adopt_kept_main`), the failure of that install, which fails the
    /// restart instead of reaching the observer or ending the loop.
    adopting: Adopting,
    error: Option<WindowErrorObserver>,
    fatal: Rc<RefCell<Option<LaunchError>>>,
}
// Each field is dropped on its own, so one that panics does not keep the
// rest; the first panic resumes once all are gone (ADR-0258 §5), and the
// exit holds it as its terminal failure. Inside an unwind it is discarded.
impl Drop for MainController {
    fn drop(&mut self) {
        self.cancel_pending();
        let mut first = flui_foundation::panic::FirstPanic::new();
        let installer = self.installer.take();
        first.contain(|| drop(installer));
        let observer = self.error.take();
        first.contain(|| drop(observer));
        // Discarded instead of resumed during an unwind.
        first.resume();
    }
}
enum CompletionState {
    Waiting,
    Claimed,
    Failed(AppWindowError),
}
struct OpenWake {
    proxy: PlatformProxy,
    ingress: std::sync::Weak<Ingress>,
    batch: Arc<()>,
    ready: AtomicBool,
    state: parking_lot::Mutex<CompletionState>,
}
impl OpenWake {
    fn failure(&self) -> Option<AppWindowError> {
        match &*self.state.lock() {
            CompletionState::Failed(error) => Some(error.clone()),
            CompletionState::Waiting | CompletionState::Claimed => None,
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
            super::host::reevaluate_exit_policy(&self.proxy);
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
                    if let Ok(window) = result.and_then(super::opened_window) {
                        contain(|| {
                            super::discard_window(
                                window.as_ref(),
                                "a main window nobody waits for",
                            );
                        });
                    }
                    self.fail(completion.failure().unwrap_or(AppWindowError::Cancelled));
                    return;
                }
                match result.and_then(super::opened_window) {
                    Ok(window) => self.install(window),
                    Err(error) => self.fail(AppWindowError::Native {
                        source: Arc::new(error),
                    }),
                }
            }
            return;
        }
        // A window waiting for its placement read opens once the read
        // settles: the turn storage's answer wakes, or the one at its bound.
        if let Some(recalling) = self.recalling.as_mut() {
            if recalling.recall_settled() {
                let opening = self
                    .recalling
                    .take()
                    .expect("BUG: the window waiting for its placement was just polled");
                self.open_window(opening);
            }
            return;
        }
        if !self.ingress.begin() {
            return;
        }
        if let Some(open) = &self.open {
            // Shown, then asked for focus: a window system that cannot give
            // focus (Wayland) still shows the window.
            use flui_platform::{CommandRefusal, WindowCommand};
            let result = WindowCommand::Show
                .admit_and_apply(&*open.window)
                .and_then(
                    |()| match WindowCommand::Activate.admit_and_apply(&*open.window) {
                        Err(CommandRefusal::Admission(
                            flui_platform::AdmissionError::Unsupported(_),
                        ))
                        | Ok(()) => Ok(()),
                        Err(error) => Err(error),
                    },
                )
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
        // The placement read starts before the window opens; until it settles
        // the window waits, and the owner wakes for the answer or the bound.
        let mut opening = OpeningWindow::new(&self.config);
        if !opening.recall_settled() {
            self.recalling = Some(opening);
            return;
        }
        self.open_window(opening);
    }
    /// When the main window stops waiting for its placement read, for the
    /// loop's wake; `None` when no window waits.
    pub(in crate::app) fn recall_deadline(&self) -> Option<std::time::Instant> {
        self.recalling
            .as_ref()
            .and_then(OpeningWindow::recall_deadline)
    }
    fn open_window(&mut self, opening: OpeningWindow) {
        // Deferred reveal: this window is installed through
        // `install_desktop_window`, which performs the reveal.
        let opened = with_owner_platform(|owner| opening.open(&self.config, owner));
        self.opening = Some(opening);
        if !self.is_current() {
            match opened {
                Some(Ok(WindowOpen::Ready(window))) => contain(|| {
                    super::discard_window(window.as_ref(), "a main window nobody waits for");
                }),
                Some(Ok(WindowOpen::Pending(mut pending))) => {
                    if let Some(Ok(window)) = pending
                        .try_take()
                        .map(|taken| taken.and_then(super::opened_window))
                    {
                        contain(|| {
                            super::discard_window(
                                window.as_ref(),
                                "a main window nobody waits for",
                            );
                        });
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
                    proxy: with_owner_platform(flui_native::OwnerPlatform::proxy)
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
    fn install(&mut self, window: Rc<dyn HostWindow>) {
        let opening = self
            .opening
            .take()
            .expect("BUG: a main window is installed once, by the open that carries its routes");
        opening.adopt(&window);
        if !self.is_current() {
            contain(|| {
                super::discard_window(window.as_ref(), "a main window nobody waits for");
            });
            self.cancel();
            return;
        }
        let host = APP_RUNTIME.with(|slot| slot.borrow().main_host_lifecycle);
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: an installer's panic is held so the window install is cancelled"
        )]
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            (self
                .installer
                .as_mut()
                .expect("BUG: live controller owns installer"))(
                &self.ingress.handle(),
                Rc::clone(&window),
                host,
                opening,
            )
        }));
        let result = match result {
            Ok(result) => result,
            Err(payload) => {
                let message = flui_runtime::frame_failure::panic_text(
                    self.config.frame_failure_detail,
                    payload.as_ref(),
                );
                flui_foundation::panic::discard_payload(payload);
                Err(AppWindowError::InstallerPanicked { message })
            }
        };
        match result {
            Ok(open) if self.is_current() => {
                self.installing = Some(open);
                self.finish_install();
            }
            Ok(open) => {
                contain(|| {
                    super::discard_window(window.as_ref(), "a main window nobody waits for");
                });
                contain(|| drop(open));
                if self.is_current() {
                    self.fail(AppWindowError::Cancelled);
                } else {
                    self.cancel();
                }
            }
            Err(error) => {
                contain(|| {
                    super::discard_window(window.as_ref(), "a main window nobody waits for");
                });
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
                if matches!(self.adopting, Adopting::Installing) {
                    self.adopting = Adopting::Installed;
                }
                self.ingress.settle(Ok(address));
            }
            outcome => {
                contain(|| {
                    super::discard_window(open.window.as_ref(), "a main window nobody waits for");
                });
                contain(|| drop(open));
                if self.is_current() {
                    self.fail(match outcome {
                        Err(source) => source.into(),
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
            contain(|| {
                super::discard_window(installing.window.as_ref(), "a main window nobody waits for");
            });
            contain(|| drop(installing));
        }
        if let Some(mut pending) = self.pending.take() {
            if let Some(Ok(window)) = pending
                .try_take()
                .map(|taken| taken.and_then(super::opened_window))
            {
                contain(|| {
                    super::discard_window(window.as_ref(), "a main window nobody waits for");
                });
            }
            contain(|| drop(pending));
        }
        // An open that failed or was cancelled installs nothing: its routes
        // and the storage its placement was read from go with it.
        if let Some(opening) = self.opening.take() {
            contain(|| drop(opening));
        }
        // A window still waiting for its placement read opens no more: the
        // read is dropped unanswered and its name is free.
        if let Some(recalling) = self.recalling.take() {
            contain(|| drop(recalling));
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
        with_owner_platform(flui_native::OwnerPlatform::request_exit_policy_reevaluation);
    }
    fn report_failure(&mut self, error: AppWindowError) {
        contain(|| tracing::error!(%error, "main window request failed"));
        if matches!(self.adopting, Adopting::Installing) {
            self.adopting = Adopting::Failed(error);
        } else if self.initial {
            self.initial = false;
            let old = self
                .fatal
                .borrow_mut()
                .replace(LaunchError::initial_window(error));
            contain(|| drop(old));
            let _ = self.ingress.quit_now();
        } else if let Some(mut observer) = self.error.take() {
            #[expect(
                clippy::disallowed_methods,
                reason = "ADR-0287: an error observer's panic is contained and the observer dropped"
            )]
            let observed =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| observer(&error)));
            match observed {
                Ok(()) => self.error = Some(observer),
                Err(payload) => {
                    flui_foundation::panic::discard_payload(payload);
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
                    .application
                    .main_ingress
                    .as_ref()
                    .is_some_and(|ingress| ingress.accepting())
                && runtime.application.main_controller.is_none()
            {
                runtime.application.main_controller = controller.take();
            }
        });
        if let Some(mut controller) = controller {
            contain(|| controller.cancel());
            contain(|| drop(controller));
        }
    }
}

/// Open the main window a restart composed with no window kept (an earlier
/// restart failed installing into it), as a start opens it, except that its
/// failure is reported to the observer and never ends the loop: the session
/// waits for the next version.
pub(super) fn open_main_window_after_restart() {
    APP_RUNTIME.with(|slot| {
        if let Some(controller) = slot.borrow_mut().application.main_controller.as_mut() {
            controller.initial = false;
        }
    });
    drive_main_window();
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
            .application
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
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: the owner turn records a frame's panic and exits (ADR-0258)"
    )]
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| controller.drive()));
    if let Err(payload) = result {
        let message = flui_runtime::frame_failure::panic_text(
            controller.config.frame_failure_detail,
            payload.as_ref(),
        );
        flui_foundation::panic::discard_payload(payload);
        controller.fail(AppWindowError::InstallerPanicked { message });
    }
}
/// Ask every presentation whether the quit the application asked for
/// ([`AppHandle::request_quit`](crate::app::AppHandle::request_quit)) may go
/// ahead, as a close for [`CloseReason::Program`](crate::app::CloseReason::Program): its guard first, then its
/// handler. A presentation the deferred quit waits on and that is gone
/// consents. Runs on the owner's turn, outside every runtime lease.
pub(super) fn drive_program_quit() {
    let Some((ingress, router)) = APP_RUNTIME.with(|slot| {
        let runtime = slot.borrow();
        runtime
            .application
            .main_ingress
            .clone()
            .map(|ingress| (ingress, runtime.close_requests()))
    }) else {
        return;
    };
    // Most turns have no quit: they collect no presentations, so a steady
    // frame allocates nothing. A quit asked after this check is the next
    // turn's.
    if !ingress.is_quit_pending() {
        return;
    }
    let registered = router.addresses();
    ingress.quit_forget_gone(&registered);
    let Some(consultation) = ingress.begin_quit_consult() else {
        return;
    };
    // The consultation keeps a panic of the code it runs (the drop of a
    // handler that closed its own window) and cancels the quit; the panic
    // resumes once the quit is finished and nothing of the host is held.
    let answered = consultation.ask(&router, registered);
    ingress.finish_quit_consult(answered).resume();
}

pub(super) fn main_window_closing(address: flui_foundation::PresentationAddress) {
    let ingress = APP_RUNTIME.with(|slot| {
        let mut runtime = slot.borrow_mut();
        if !runtime.installed_host.native().contains_address(address) {
            return None;
        }
        if let Some(controller) = runtime.application.main_controller.as_mut()
            && controller
                .open
                .as_ref()
                .is_some_and(|open| open.address == address)
        {
            controller.closing = true;
        }
        runtime.application.main_ingress.clone()
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
        let removed = runtime
            .application
            .main_controller
            .as_mut()
            .and_then(|controller| {
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
        (removed, runtime.application.main_ingress.clone())
    });
    contain(|| drop(removed));
    if let Some(ingress) = ingress {
        ingress.finish_close(address);
        ingress.wake_owner();
    }
}

/// Shut the main window down: its intents are refused and its controller,
/// with the application's factory and what it captured, dropped. A panic
/// there is the loop's terminal failure (ADR-0258 §2), which the exit
/// resumes once it has written what the application persisted.
pub(super) fn shutdown_main_window() {
    let (controller, ingress, dev_agent, reload) = APP_RUNTIME.with(|slot| {
        let mut runtime = slot.borrow_mut();
        (
            runtime.application.main_controller.take(),
            runtime.application.main_ingress.take(),
            runtime.loop_dev_agent.end(),
            runtime.desktop_loop.take(),
        )
    });
    // The loop's hooks detach and drop first, outside the borrow and
    // contained like the controller's: an agent stops before the windows it
    // serves go, so its driver sees the connection end rather than each
    // window answer `gone`.
    super::terminal::contain_terminal("development agents' release", || drop(dev_agent));
    #[cfg(native_journal)]
    super::native_journal::end();
    if let Some(ingress) = ingress {
        ingress.close();
    }
    let mut controller = controller;
    if let Some(controller) = controller.as_mut() {
        super::terminal::contain_terminal("main window cancellation", || controller.cancel());
    }
    // The development reload detaches after the window's pending open is
    // cancelled and before its factory goes.
    super::terminal::contain_terminal("development reload detach", || drop(reload));
    if let Some(controller) = controller {
        super::terminal::contain_terminal("main window retirement", || drop(controller));
    }
}

/// The execution services a starting platform may resolve, before any
/// teardown is armed: dropped armed (a refused platform, an unwind out of
/// the host's start), it shuts them down and clears the executors the run
/// stashed, so the next run on this thread resolves its own (ADR-0327 §3).
struct StartupExecution {
    armed: bool,
}

impl StartupExecution {
    /// The platform is up: the loop's teardown owns the services from here.
    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for StartupExecution {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let shut = APP_RUNTIME.with(|slot| {
            slot.try_borrow_mut()
                .map(|mut runtime| runtime.shutdown_execution(std::time::Instant::now()))
                .is_ok()
        });
        if !shut {
            tracing::warn!(
                "the runtime was borrowed when the platform failed to start; its execution services stay"
            );
        }
    }
}

/// Retires the loop's main window and UI runtimes when dropped: after
/// `Platform::run` returns, or while a panic unwinds out of it. Held inside
/// the [`OwnerHostClearGuard`], so the windows go before the owner platform
/// that created them. On unwind each step is contained, so a second panic
/// cannot abort the first one's unwind; that first panic stays the one raised.
/// Each step that ends is written to its log.
///
/// It starts the exit watchdog first and leaves it in `watchdog`, whose
/// holder ends the watch once the exit's outcome is known (ADR-0258 §7).
#[derive(Default)]
struct LoopTeardown {
    log: Rc<RefCell<Vec<TeardownStep>>>,
    watchdog: Rc<RefCell<Option<Rc<ExitWatchdog>>>>,
}

impl Drop for LoopTeardown {
    fn drop(&mut self) {
        let watchdog = Rc::new(ExitWatchdog::start());
        self.watchdog.replace(Some(Rc::clone(&watchdog)));
        let log = Rc::clone(&self.log);
        let mut record = move |step| {
            watchdog.reached(step);
            log.borrow_mut().push(step);
        };
        if std::thread::panicking() {
            contain(shutdown_main_window);
            record(TeardownStep::MainWindow);
            contain(|| teardown_platform_ui_runtime_logged(&mut record));
        } else {
            shutdown_main_window();
            record(TeardownStep::MainWindow);
            teardown_platform_ui_runtime_logged(&mut record);
        }
    }
}

pub(in crate::app) fn run_application<V, F>(
    application: DesktopRun<V, F>,
) -> Result<(), LaunchError>
where
    V: View + Clone + 'static,
    F: FnMut(&AppHandle) -> V + 'static,
{
    // An uninstaller's run removes the registration and starts nothing,
    // before a running instance could be handed the launch.
    #[cfg(windows)]
    if let Some(unregistered) =
        super::associations::unregister_if_requested(&application.config.application_identity)
    {
        return unregistered;
    }
    let one_instance = application
        .opening
        .keeps_one_instance(&application.config.application_identity);
    let app_id = application.config.application_identity.app_id().cloned();
    // The capabilities' providers name the application by the id its windows
    // carry (ADR-0351).
    APP_RUNTIME.with(|slot| slot.borrow().host_bridge().name_application(app_id.clone()));
    // Held for the main window's first frame; a launch the running instance
    // takes drops it, so only the instance that runs registers.
    #[cfg(windows)]
    {
        let pending = super::associations::PendingRegistration::of(
            &application.opening,
            &application.config.application_identity,
        );
        APP_RUNTIME.with(|slot| slot.borrow_mut().pending_registration = pending);
    }
    let started = start_or_forward(
        application,
        move || {
            let host = flui_native::current_host().map_err(LaunchError::from_platform)?;
            // Before the platform, whose registries report only to a capture already installed;
            // ended if the platform fails to start or another instance takes the launch. The
            // loop reads the launch and settles it.
            // A build without the journal has no backend to name.
            #[cfg(native_journal)]
            let backend = Some(host.backend());
            #[cfg(not(native_journal))]
            let backend = None;
            let journal = super::journal_start::JournalStart::begin(backend);
            let placement = super::raster_host::desktop_placement(host);
            let started = start_host(host, app_id.as_ref(), one_instance)?;
            if started.is_some() {
                journal.release_to_loop();
            }
            Ok(started.map(|platform| (platform, placement)))
        },
        Rc::default(),
        RasterFactory::Gpu,
    )?;
    started.map_or(Ok(()), StartedLoop::finish)
}

/// Start `host` for the application identified by `app_id` (which its Linux
/// windows carry as their class and app id), as its one instance when
/// `one_instance` holds and there is an id to name it by
/// (`CurrentHost::start_single`):
/// `None` when another process of the application took this launch. The
/// Wayland and X11 hosts run their IO sources (XIM, IBus, logind, the
/// settings portal, the session bus), and the Windows host its single-instance
/// pipe, on the application's IO runtime, so a process runs one (ADR-0327 §3);
/// every other host, and an application that brought its own executors,
/// starts on the host's own.
fn start_host(
    host: flui_native::CurrentHost,
    app_id: Option<&crate::AppId>,
    one_instance: bool,
) -> Result<Option<Box<dyn flui_native::Platform>>, LaunchError> {
    let single = app_id.filter(|_| one_instance);
    let executor =
        host_executor(host, single.is_some())?.unwrap_or_else(flui_native::NativeExecutor::own);
    #[cfg(feature = "single-instance")]
    if let Some(app_id) = single {
        return match host
            .start_single(executor, app_id)
            .map_err(LaunchError::from_platform)?
        {
            flui_native::HostStart::Started(platform) => Ok(Some(platform)),
            flui_native::HostStart::Forwarded => Ok(None),
        };
    }
    host.start_with(executor, app_id)
        .map(Some)
        .map_err(LaunchError::from_platform)
}

/// The executor `host` runs its IO on: the application's IO runtime for the
/// Wayland and X11 hosts, and for the Windows host when the application runs
/// one instance (its pipe is served there), when the application has one;
/// `None` for the host's own.
#[cfg_attr(
    not(any(target_os = "linux", windows)),
    expect(
        clippy::unnecessary_wraps,
        reason = "only the Linux and Windows hosts take the application's IO runtime, which can \
                  fail to start"
    )
)]
fn host_executor(
    host: flui_native::CurrentHost,
    single: bool,
) -> Result<Option<flui_native::NativeExecutor>, LaunchError> {
    #[cfg(target_os = "linux")]
    let takes_io = matches!(
        host,
        flui_native::CurrentHost::Wayland | flui_native::CurrentHost::X11
    );
    #[cfg(windows)]
    let takes_io = single && matches!(host, flui_native::CurrentHost::Windows);
    #[cfg(any(target_os = "linux", windows))]
    if takes_io {
        let services = APP_RUNTIME.with(|slot| Arc::clone(slot.borrow_mut().ensure_execution()));
        let runtime = services
            .io_runtime()
            .map_err(|error| LaunchError::Platform {
                source: Arc::new(error),
            })?;
        return Ok(runtime.map(flui_native::NativeExecutor::from_tokio));
    }
    let _ = (host, single);
    Ok(None)
}

#[cfg(test)]
fn run_with_platform<V, F>(
    application: DesktopRun<V, F>,
    platform: Box<dyn flui_native::Platform>,
) -> Result<(), LaunchError>
where
    V: View + Clone + 'static,
    F: FnMut(&AppHandle) -> V + 'static,
{
    run_with_platform_factory(application, || Ok(platform))
}

#[cfg(test)]
fn run_with_platform_factory<V, F>(
    application: DesktopRun<V, F>,
    platform: impl FnOnce() -> Result<Box<dyn flui_native::Platform>, LaunchError>,
) -> Result<(), LaunchError>
where
    V: View + Clone + 'static,
    F: FnMut(&AppHandle) -> V + 'static,
{
    start_loop(application, platform, Rc::default(), RasterFactory::Gpu)?.finish()
}

/// A loop whose [`Platform::run`](flui_native::Platform::run) returned, with
/// its teardown still owed: a desktop loop's `run` returns when the loop
/// ends, the headless platform's once it has bootstrapped, so a test drives
/// the application between `start_loop` (with the `testing` feature) and
/// [`StartedLoop::finish`].
/// Dropped unfinished, it tears the loop down all the same.
pub(crate) struct StartedLoop {
    // Fields drop in this order: the main window and the ui_runtimes (and the
    // native windows they own) are retired while the owner platform still
    // lives.
    teardown: LoopTeardown,
    owner_guard: OwnerHostClearGuard,
    // Last: the watch the teardown started ends once the owner platform is
    // gone too.
    watchdog: Rc<RefCell<Option<Rc<ExitWatchdog>>>>,
    fatal: Rc<RefCell<Option<LaunchError>>>,
    result: Result<(), flui_native::PlatformError>,
    /// Whether the driver agent quit the loop because its driver went.
    disconnect: Arc<Disconnect>,
}

impl StartedLoop {
    /// Tear the loop down, then report its outcome (ADR-0258 §3): a terminal
    /// failure resumes, else the first fatal failure the loop recorded is
    /// returned, else what `run` returned. A loop the driver agent quit
    /// because its driver went (ADR-0239 §5) ends the process with
    /// [`EXIT_DISCONNECTED`](flui_protocol::launch::EXIT_DISCONNECTED)
    /// instead of returning.
    ///
    /// # Panics
    ///
    /// With the terminal failure's payload, once the teardown has written
    /// what the application persisted.
    pub(crate) fn finish(self) -> Result<(), LaunchError> {
        let Self {
            teardown,
            owner_guard,
            watchdog,
            fatal,
            result,
            disconnect,
        } = self;
        drop(teardown);
        drop(owner_guard);
        drop(watchdog);
        if let Some(payload) = super::terminal::take_terminal() {
            std::panic::resume_unwind(payload);
        }
        let failure = fatal.borrow_mut().take();
        if let Some(error) = failure {
            return Err(error);
        }
        if disconnect.happened() {
            // The teardown above wrote what the application persisted; the
            // driver that launched the app tells this exit from a normal one
            // and from a panic by its code.
            std::process::exit(flui_protocol::launch::EXIT_DISCONNECTED);
        }
        result.map_err(LaunchError::from_platform)
    }
}

/// Run `application` on the platform `platform` makes, up to the point
/// [`Platform::run`](flui_native::Platform::run) returns, writing each
/// teardown step that ends to `teardown_log`, on unwind too. Its windows
/// render with what `renderers` builds, on an executor the owner turns
/// inline, as in every test and on the headless host (ADR-0305 §10).
#[cfg(any(test, feature = "testing"))]
pub(crate) fn start_loop<V, F>(
    application: DesktopRun<V, F>,
    platform: impl FnOnce() -> Result<Box<dyn flui_native::Platform>, LaunchError>,
    teardown_log: Rc<RefCell<Vec<TeardownStep>>>,
    renderers: RasterFactory,
) -> Result<StartedLoop, LaunchError>
where
    V: View + Clone + 'static,
    F: FnMut(&AppHandle) -> V + 'static,
{
    let started = start_or_forward(
        application,
        || platform().map(|platform| Some((platform, RasterPlacement::Inline))),
        teardown_log,
        renderers,
    )?;
    Ok(started.expect("BUG: a platform the factory made starts a loop"))
}

/// `start_loop` on the host `platform` starts, its executor placed as the
/// closure says with the platform, or `None` when the
/// application runs as one instance and another process of it took this
/// launch: nothing started, and the services the start installed are down.
fn start_or_forward<V, F>(
    mut application: DesktopRun<V, F>,
    platform: impl FnOnce() -> Result<
        Option<(Box<dyn flui_native::Platform>, RasterPlacement)>,
        LaunchError,
    >,
    teardown_log: Rc<RefCell<Vec<TeardownStep>>>,
    renderers: RasterFactory,
) -> Result<Option<StartedLoop>, LaunchError>
where
    V: View + Clone + 'static,
    F: FnMut(&AppHandle) -> V + 'static,
{
    // Before anything starts, on every platform alike: window options no
    // window can be asked for start nothing.
    super::check_window_options(&application.config)?;
    // Before anything starts, so a conflict opens no window and installs
    // nothing (ADR-0152 §1).
    let capabilities = std::mem::take(&mut application.capabilities);
    // A plugin's `register` is user code: it runs with no borrow held.
    let slots = APP_RUNTIME.with(|slot| slot.borrow().capability_slots());
    let registry = capabilities
        .into_registry(&slots)
        .map_err(LaunchError::CapabilityConflict)?;
    let _logging = super::super::logging::init_managed_logging(&application.config);
    // Before the platform: a host that runs IO on the application's runtime
    // takes it while it starts (ADR-0327 §3). Until the platform is up no
    // teardown is armed, so the guard takes the services down with a
    // refusal or an unwind out of the host's start.
    let startup = StartupExecution { armed: true };
    if let Some(executors) = application.config.executors.clone() {
        APP_RUNTIME.with(|slot| slot.borrow_mut().install_host_executors(executors));
    }
    let Some((platform, placement)) = platform()? else {
        // The running instance took the launch; the guard takes the services
        // down as for a refused start.
        return Ok(None);
    };
    startup.disarm();
    let fatal = Rc::new(RefCell::new(None));
    let disconnect = Arc::new(Disconnect::default());
    // The loop's handle on it: the loop arms it and installs the agent.
    let ended = Arc::clone(&disconnect);
    let recorded = Rc::clone(&fatal);
    // Declared before the owner guard, so the watch outlives it on unwind.
    let watchdog = Rc::default();
    let owner_guard = OwnerHostClearGuard::arm();
    // Declared after the owner guard, so it drops first: the main window and
    // the ui_runtimes (and the native windows they own) are retired while the
    // owner platform still lives, on unwind too.
    let teardown = LoopTeardown {
        log: teardown_log,
        watchdog: Rc::clone(&watchdog),
    };
    // The handler owns it from the start, so a URL the platform reports
    // before `ready` finds the application's declarations.
    let opening = std::mem::take(&mut application.opening);
    // A panic out of the platform's own loop is terminal (ADR-0258 §3): held
    // for the exit's end, and the teardown runs as after any other end.
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: the platform loop's panic is held so the teardown still runs (ADR-0258)"
    )]
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        platform.run(crate::app::runner::opening_app_handler(
            opening,
            move |owner| {
                let mut application = application;
                let proxy = owner.proxy();
                install_owner_platform(owner)?;
                APP_RUNTIME.with(|slot| {
                    let mut runtime = slot.borrow_mut();
                    runtime.main_host_lifecycle = flui_scheduler::AppLifecycleState::Resumed;
                    runtime.raster_host =
                        super::raster_host::SharedRasterHost::new(renderers, placement);
                    runtime.ensure_execution();
                });
                install_platform_quit_hook();
                // The driver agent (ADR-0239), beside the application's own hooks,
                // when launched with an endpoint; a driver that goes under
                // `EXIT_ON_DISCONNECT` quits past every close guard.
                let served = crate::app::agent::install_from_launch(
                    &mut application.config,
                    |key| with_owner_platform(|owner| owner.launch_value(key)).flatten(),
                    &ended,
                    super::journals_native(),
                );
                super::keep_native_journal(served);
                // The loop takes the hooks and owns them until it ends
                // (`shutdown_main_window`), across a development restart too.
                super::dev_agents::start(&application.config);
                // A restart a frame finds due runs in the owner turn this asks
                // for. The hook's wake asks for one too, in which a loop with
                // no UI runtime polls it.
                let turn: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                    let _ = proxy.wake();
                });
                let frame = runtime_wake_callback();
                let woken = Arc::clone(&turn);
                let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                    frame();
                    woken();
                });
                let reload = LoopReload::attach(&application.config, wake, turn);
                APP_RUNTIME.with(|slot| {
                    slot.borrow_mut().desktop_loop = Some(DesktopLoop {
                        reload,
                        disconnect: ended,
                        fatal: recorded,
                        kept: None,
                        restart_turn_asked: false,
                        restarting: None,
                    });
                });
                match compose(application, registry) {
                    Ok(()) => {}
                    Err(failure) => {
                        let ready_panicked = matches!(failure, LaunchError::OnReadyPanicked);
                        let ingress = APP_RUNTIME.with(|slot| {
                            let runtime = slot.borrow();
                            if let Some(desktop) = runtime.desktop_loop.as_ref() {
                                desktop.fatal.replace(Some(failure));
                            }
                            runtime.application.main_ingress.clone()
                        });
                        if let Some(ingress) = ingress {
                            let _ = ingress.quit_now();
                        }
                        // A refused service leaves the window unopened; a
                        // `ready` that panicked still lets the controller
                        // see the quit it asked for.
                        if !ready_panicked {
                            return Ok(());
                        }
                    }
                }
                drive_main_window();
                with_owner_platform(flui_native::OwnerPlatform::request_exit_policy_reevaluation);
                Ok(())
            },
        ))
    }));
    let result = run.unwrap_or_else(|payload| {
        super::terminal::hold_terminal(payload, "platform loop");
        Ok(())
    });
    Ok(Some(StartedLoop {
        teardown,
        owner_guard,
        watchdog,
        fatal,
        result,
        disconnect,
    }))
}

/// What a desktop loop keeps for every application it composes (ADR-0393
/// §4): made when the loop starts, before any application code runs, and
/// kept across a development restart. Dropped at the loop's exit
/// (`shutdown_main_window`).
pub(in crate::app) struct DesktopLoop {
    /// The development reload the loop attached: the tool's, not the
    /// application's, so a restart does not install the later application's
    /// `AppConfig::dev_reload`.
    pub(super) reload: LoopReload,
    /// What the driver agent's disconnect quits; each composed application
    /// arms it with its own ingress.
    disconnect: Arc<Disconnect>,
    /// The loop's terminal launch failure, which the exit returns.
    fatal: Rc<RefCell<Option<LaunchError>>>,
    /// The main window a restart kept open, from the retirement of the
    /// earlier application until the next one is installed into it; it
    /// stays here when no application is composed.
    pub(super) kept: Option<KeptMain>,
    /// A restart was due while a UI runtime was checked out, and the owner's
    /// next turn was asked for once; cleared when a restart runs.
    pub(super) restart_turn_asked: bool,
    /// The version a restart composed and what it left unapplied, while its
    /// install into the kept window still commits: the restart is reported
    /// once that install settles, and the next one waits for it.
    pub(super) restarting: Option<(CodeVersion, Unapplied)>,
}

/// The main window a development restart keeps open (ADR-0393 §4): the
/// native window and the routes it was opened with.
pub(super) struct KeptMain {
    pub(super) window: Rc<dyn HostWindow>,
    pub(super) routes: Rc<super::window_routes::WindowRoutes>,
}

/// Compose `application` on the started loop, as its start and a
/// development restart do (ADR-0393 §4 step 3): the application scope's
/// ingress, storage, placements, exit policy and routes, the main window's
/// controller, the services and `ready`. When the loop kept a main window
/// ([`DesktopLoop::kept`]), the new application's first UI runtime is
/// installed into it at once; otherwise the controller opens the window
/// when the application asks for it.
///
/// The loop's own parts ([`DesktopLoop`], the owner platform, the raster
/// host, the pools) are made before and kept. `registry` is the
/// application's capabilities, checked for conflicts against this scope's
/// slots.
///
/// # Errors
///
/// A service that refuses to start ([`LaunchError::Service`]) and a `ready`
/// that panicked ([`LaunchError::OnReadyPanicked`]); the controller is in the
/// scope either way, for the caller to quit or retire.
pub(super) fn compose<V, F>(
    application: DesktopRun<V, F>,
    registry: flui_runtime::capability::CapabilityRegistry,
) -> Result<(), LaunchError>
where
    V: View + Clone + 'static,
    F: FnMut(&AppHandle) -> V + 'static,
{
    let DesktopRun {
        mut factory,
        config,
        startup,
        ready,
        window_error,
        ..
    } = application;
    let proxy = with_owner_platform(flui_native::OwnerPlatform::proxy)
        .expect("BUG: an application is composed on a started loop");
    let (reload, disconnect, fatal, keeps_window) = APP_RUNTIME.with(|slot| {
        let runtime = slot.borrow();
        let desktop = runtime
            .desktop_loop
            .as_ref()
            .expect("BUG: an application is composed on a started loop");
        (
            desktop.reload.reload(),
            Arc::clone(&desktop.disconnect),
            Rc::clone(&desktop.fatal),
            desktop.kept.is_some(),
        )
    });
    // A kept window is the main window already: nothing is opened at start.
    let opens_at_start = !keeps_window && startup == StartupWindow::Open;
    let ingress = Ingress::new(proxy, opens_at_start);
    let handle = ingress.handle();
    APP_RUNTIME.with(|slot| {
        let mut runtime = slot.borrow_mut();
        runtime.application.main_ingress = Some(Arc::clone(&ingress));
        runtime.application.capabilities = Some(registry);
        runtime.reopen_lifecycles();
    });
    super::host::install_host_storage(&config);
    // Window placements are kept in the storage the run's registry
    // provides, the application's override included.
    super::placement::install(APP_RUNTIME.with(|slot| slot.borrow().capability_registry()));
    install_exit_policy_hook(config.exit_policy);
    let reopen = Arc::downgrade(&ingress);
    super::host::route_reopen(move || {
        reopen
            .upgrade()
            .is_some_and(|ingress| ingress.request_native_show())
    });
    let asked = Arc::downgrade(&ingress);
    super::host::route_quit_asked(move || {
        asked
            .upgrade()
            .map_or(flui_native::QuitAnswer::Quit, |ingress| {
                ingress.platform_quit_asked()
            })
    });
    let quit = Arc::downgrade(&ingress);
    disconnect.arm(move || {
        if let Some(ingress) = quit.upgrade() {
            let _ = ingress.quit_now();
        }
    });
    let installer_config = config.clone();
    let identity = APP_RUNTIME.with(|slot| Arc::clone(&slot.borrow().loop_identity));
    let factory_identity = Arc::clone(&identity);
    let installer = Box::new(move |handle: &AppHandle, window, host, carried| {
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: a root factory's panic becomes a window install error"
        )]
        let root = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| factory(handle)))
            .map_err(|payload| {
                let message = flui_runtime::frame_failure::panic_text(
                    installer_config.frame_failure_detail,
                    payload.as_ref(),
                );
                flui_foundation::panic::discard_payload(payload);
                AppWindowError::FactoryPanicked { message }
            })?;
        if !handle.ingress.accepting()
            || !APP_RUNTIME
                .with(|slot| Arc::ptr_eq(&slot.borrow().loop_identity, &factory_identity))
        {
            return Err(AppWindowError::Cancelled);
        }
        install_desktop_window(
            root,
            &installer_config,
            reload.clone(),
            window,
            host,
            carried,
            true, // the run's main window
        )
    });
    APP_RUNTIME.with(|slot| {
        slot.borrow_mut().application.main_controller = Some(MainController {
            ingress: Arc::clone(&ingress),
            identity,
            installer: Some(installer),
            config: config.clone(),
            open: None,
            installing: None,
            pending: None,
            opening: None,
            recalling: None,
            pending_wake: None,
            closing: false,
            initial: opens_at_start,
            adopting: Adopting::No,
            error: window_error,
            fatal,
        });
    });
    if keeps_window {
        adopt_kept_main()?;
    }
    for service in &config.services {
        super::host::start_service(service).map_err(|error| LaunchError::Service {
            source: Arc::new(error),
        })?;
    }
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: a ready callback's panic is discarded; the caller decides what it costs"
    )]
    if let Some(ready) = ready
        && let Err(payload) =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ready(&handle)))
    {
        flui_foundation::panic::discard_payload(payload);
        return Err(LaunchError::OnReadyPanicked);
    }
    Ok(())
}

/// Install the composed application's main window into the window the loop
/// kept ([`DesktopLoop::kept`]), through the controller as an opened window
/// is. The loop holds the window until the controller has it.
///
/// # Errors
///
/// [`LaunchError::InitialWindow`] when the install failed: the install
/// closed the window, so the restart fails, and the next one opens a main
/// window anew.
fn adopt_kept_main() -> Result<(), LaunchError> {
    let kept = APP_RUNTIME.with(|slot| {
        slot.borrow_mut()
            .desktop_loop
            .as_mut()
            .and_then(|desktop| desktop.kept.take())
    });
    kept.map_or(Ok(()), adopt_main_into)
}

/// [`adopt_kept_main`] into `kept`. With no controller composed, `kept`
/// goes back to the loop.
fn adopt_main_into(kept: KeptMain) -> Result<(), LaunchError> {
    let leased = APP_RUNTIME.with(|slot| {
        let mut runtime = slot.borrow_mut();
        let controller = runtime.application.main_controller.take()?;
        Some(ControllerLease {
            controller: Some(controller),
            identity: Arc::clone(&runtime.loop_identity),
        })
    });
    let Some(mut lease) = leased else {
        // Dropped outside the borrow: a window's drop is platform code.
        let displaced = APP_RUNTIME.with(|slot| match slot.borrow_mut().desktop_loop.as_mut() {
            Some(desktop) => desktop.kept.replace(kept),
            None => Some(kept),
        });
        contain(|| drop(displaced));
        return Ok(());
    };
    let controller = lease
        .controller
        .as_mut()
        .expect("BUG: controller lease is populated until Drop");
    controller.opening = Some(OpeningWindow::kept(kept.routes, &controller.config));
    controller.adopting = Adopting::Installing;
    controller.install(kept.window);
    match std::mem::replace(&mut controller.adopting, Adopting::No) {
        Adopting::Failed(error) => Err(LaunchError::initial_window(error)),
        Adopting::Installed => Ok(()),
        // Submitted, and committing in a later owner turn: the restart's
        // outcome waits for it (`take_kept_install_outcome`).
        Adopting::Installing if controller.installing.is_some() => {
            controller.adopting = Adopting::Installing;
            Ok(())
        }
        // The install closed the window with no failure reported.
        Adopting::Installing | Adopting::No => {
            Err(LaunchError::initial_window(AppWindowError::Cancelled))
        }
    }
}

/// How the install into the window the loop kept settled, once it has:
/// `None` while it still commits. A controller that is gone (the
/// application retired) settled as cancelled.
pub(super) fn take_kept_install_outcome() -> Option<Result<(), AppWindowError>> {
    APP_RUNTIME.with(|slot| {
        let mut runtime = slot.borrow_mut();
        let Some(controller) = runtime.application.main_controller.as_mut() else {
            return Some(Err(AppWindowError::Cancelled));
        };
        match std::mem::replace(&mut controller.adopting, Adopting::No) {
            Adopting::Installing => {
                controller.adopting = Adopting::Installing;
                None
            }
            Adopting::Installed | Adopting::No => Some(Ok(())),
            Adopting::Failed(error) => Some(Err(error)),
        }
    })
}

impl MainController {
    /// Retire for a development restart: the application's window requests
    /// are refused, and the main window it has open, or is installing, is
    /// handed back for the loop to keep instead of discarded.
    pub(super) fn retire_keeping_window(&mut self) -> Option<RenderedMain> {
        let open = self.open.take().or_else(|| self.installing.take());
        self.cancel();
        open
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::AtomicUsize;

    use flui_native::Platform;
    use flui_native::testing::HeadlessPlatform;
    use flui_testing::window_commands::AdmitAndApply as _;

    use crate::app::runtime::ExitPolicy;

    fn install_test_controller(
        owner: flui_native::OwnerPlatform,
        installer: Installer,
        observer: Option<WindowErrorObserver>,
    ) -> AppHandle {
        let proxy = owner.proxy();
        install_owner_platform(owner).expect("owner signal");
        let ingress = Ingress::new(proxy, false);
        let handle = ingress.handle();
        APP_RUNTIME.with(|slot| {
            let mut runtime = slot.borrow_mut();
            runtime.application.main_ingress = Some(Arc::clone(&ingress));
            runtime.application.main_controller = Some(MainController {
                identity: Arc::clone(&runtime.loop_identity),
                ingress,
                installer: Some(installer),
                config: AppConfig::new(),
                open: None,
                installing: None,
                pending: None,
                opening: None,
                recalling: None,
                pending_wake: None,
                closing: false,
                initial: false,
                adopting: Adopting::No,
                error: observer,
                fatal: Rc::new(RefCell::new(None)),
            });
        });
        install_platform_quit_hook();
        handle
    }
    fn main_window_pending_coalesces_and_recovers_after_installer_panic() {
        let _owner = OwnerHostClearGuard::arm();
        let _cleanup = LoopTeardown::default();
        let platform = HeadlessPlatform::new();
        let deferred = platform.enable_deferred_window_open();
        let turns = platform.owner_turns();
        let calls = Rc::new(Cell::new(0));
        let seen = Rc::clone(&calls);
        let saved = Rc::new(RefCell::new(None));
        let output = Rc::clone(&saved);
        Box::new(platform)
            .run(crate::app::runner::app_handler(move |owner| {
                let handle = install_test_controller(
                    owner,
                    Box::new(move |_, _, _, _| {
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

        for close_before_publication in [false, true] {
            let _owner = OwnerHostClearGuard::arm();
            let _cleanup = LoopTeardown::default();
            Box::new(HeadlessPlatform::new())
                .run(crate::app::runner::app_handler(move |owner| {
                    let handle = install_test_controller(
                        owner,
                        Box::new(move |_, window, host, _| {
                            let runtime = UiRuntime::for_test();
                            runtime
                                .attach_root_widget(&flui_widgets::SizedBox::new(20.0, 30.0))
                                .expect("mount root");
                            let host_window = Rc::clone(&window);
                            let window: Rc<dyn flui_platform::PlatformWindow> = window;
                            let mut prepared =
                                prepare_ui_runtime_alongside(runtime, Rc::clone(&window));
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
                            if close_before_publication {
                                window.close_now();
                            }
                            Ok(RenderedMain {
                                window: host_window,
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
                        if close_before_publication {
                            matches!(result, Some(Err(AppWindowError::Cancelled)))
                        } else {
                            matches!(result, Some(Ok(_)))
                        },
                        "ready follows publication: {result:?}"
                    );
                    Ok(())
                }))
                .expect("headless bootstrap");
        }
    }

    /// An install into the window a restart kept that commits after the
    /// restart's call (here the call runs inside an owner callback, so the
    /// install publishes once it ends) settles there: until then the
    /// restart's outcome waits, and a window closed before the install
    /// commits fails it instead of reporting it installed.
    fn a_kept_install_that_commits_later_settles_when_it_does() {
        use super::super::frame_driver::{FrameDriver, TestFrameDriver};
        use super::super::owner_dispatch::{
            RuntimeTask, dispatch_platform_ui_runtime, install_platform_ui_runtime,
            prepare_ui_runtime_alongside,
        };
        use flui_runtime::ui_runtime::UiRuntime;

        for close_before_publication in [false, true] {
            let _owner = OwnerHostClearGuard::arm();
            let _cleanup = LoopTeardown::default();
            // Asserted after the run: a panic inside it is contained there.
            let seen = Rc::new(Cell::new(None));
            let settled = Rc::new(RefCell::new(None));
            let (seen_in_run, settled_in_run) = (Rc::clone(&seen), Rc::clone(&settled));
            Box::new(HeadlessPlatform::new())
                .run(crate::app::runner::app_handler(move |owner| {
                    let _handle = install_test_controller(
                        owner,
                        Box::new(move |_, window, host, _| {
                            let runtime = UiRuntime::for_test();
                            runtime
                                .attach_root_widget(&flui_widgets::SizedBox::new(20.0, 30.0))
                                .expect("mount root");
                            let host_window = Rc::clone(&window);
                            let window: Rc<dyn flui_platform::PlatformWindow> = window;
                            let mut prepared =
                                prepare_ui_runtime_alongside(runtime, Rc::clone(&window));
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
                            if close_before_publication {
                                window.close_now();
                            }
                            Ok(RenderedMain {
                                window: host_window,
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
                    // The callback's own panic would be contained by the
                    // dispatch: what it saw is asserted after it.
                    let seen = seen_in_run;
                    let inside = Rc::clone(&seen);
                    dispatch_platform_ui_runtime(
                        outer,
                        RuntimeTask::TestCallback(Box::new(move |_| {
                            let window =
                                crate::app::window_test_support::headless_test_host_window();
                            let routes =
                                super::super::window_routes::WindowRoutes::of(window.as_ref());
                            let adopted = adopt_main_into(KeptMain { window, routes }).is_ok();
                            inside.set(Some((adopted, take_kept_install_outcome().is_none())));
                        })),
                    )
                    .expect("complete caller and installation");
                    *settled_in_run.borrow_mut() = Some(take_kept_install_outcome());
                    Ok(())
                }))
                .expect("headless bootstrap");
            assert_eq!(
                seen.get(),
                Some((true, true)),
                "an install that has not settled fails nothing yet, and commits after the owner \
                 callback"
            );
            let outcome = settled.borrow_mut().take().flatten();
            assert!(
                if close_before_publication {
                    matches!(outcome, Some(Err(AppWindowError::Cancelled)))
                } else {
                    matches!(outcome, Some(Ok(())))
                },
                "the install settled with its window: {outcome:?}"
            );
        }
    }

    /// The window a link that launched the application opened in is closed
    /// after its installation was submitted, during an owner turn, and before
    /// it commits: the link is refused as its window closed, and no reopen
    /// brings the window the user closed back.
    fn a_link_into_a_window_closed_before_it_commits_is_refused() {
        use super::super::frame_driver::{FrameDriver, TestFrameDriver};
        use super::super::owner_dispatch::{
            RuntimeTask, dispatch_platform_ui_runtime, install_platform_ui_runtime,
            prepare_ui_runtime_alongside,
        };
        use flui_platform::{Launch, OpenTarget, Opened};
        use flui_runtime::ui_runtime::UiRuntime;

        flui_testing::log_capture::disarm_interest_cache();
        let installs = Rc::new(Cell::new(0_u32));
        let reopens = Rc::new(Cell::new(0_u32));
        let _owner = OwnerHostClearGuard::arm();
        let _cleanup = LoopTeardown::default();
        let platform = HeadlessPlatform::new();
        platform.host_events().open(
            Opened::bounded(
                ["myapp:///note/3".to_owned()],
                [],
                OpenTarget::Application,
                Launch::Cold,
            )
            .0,
        );
        let settings = crate::app::runner::OpenSettings {
            declarations: Some(
                flui_view::navigation::OpenDeclarations::new().url_schemes(&["myapp"]),
            ),
            ..crate::app::runner::OpenSettings::default()
        };
        let counted = Rc::clone(&installs);
        let installs_seen = Rc::clone(&installs);
        let reopened = Rc::clone(&reopens);
        let (ran, log) = flui_testing::log_capture::capture(|| {
            Box::new(platform).run(crate::app::runner::opening_app_handler(
                settings,
                move |owner| {
                    let handle = install_test_controller(
                        owner,
                        Box::new(move |_, window, host, _| {
                            counted.set(counted.get() + 1);
                            let (runtime, opened) = super::super::host::build_lending_ui_runtime(
                                super::super::presentation_window(Rc::clone(&window)),
                                1.0,
                                true, // the run's main window
                            )
                            .expect("the window's runtime is built");
                            assert!(opened.is_some(), "the held link is lent to the window");
                            runtime
                                .attach_root_widget(&flui_widgets::SizedBox::new(20.0, 30.0))
                                .expect("mount root");
                            let host_window = Rc::clone(&window);
                            let window: Rc<dyn flui_platform::PlatformWindow> = window;
                            let mut prepared =
                                prepare_ui_runtime_alongside(runtime, Rc::clone(&window));
                            prepared.lend_opened(opened);
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
                                "the installation commits after this owner turn"
                            );
                            window.close_now();
                            Ok(RenderedMain {
                                window: host_window,
                                address,
                                installation,
                            })
                        }),
                        None,
                    );
                    // A reopen the held link asks for before its window
                    // opens is its one attempt; one asked after the window
                    // was opened and closed would bring that window back.
                    let opened_once = Rc::clone(&installs_seen);
                    super::super::host::route_reopen(move || {
                        if opened_once.get() > 0 {
                            reopened.set(reopened.get() + 1);
                        }
                        true
                    });
                    let outer = install_platform_ui_runtime(
                        UiRuntime::for_test(),
                        &crate::app::window_test_support::headless_test_window(),
                    );
                    dispatch_platform_ui_runtime(
                        outer,
                        RuntimeTask::TestCallback(Box::new(move |_| {
                            drop(
                                handle
                                    .request_show_main_window()
                                    .expect("admit show request"),
                            );
                        })),
                    )
                    .expect("complete caller and installation");
                    Ok(())
                },
            ))
        });
        ran.expect("headless bootstrap");
        assert_eq!(installs.get(), 1, "one window was opened for the link");
        assert_eq!(
            reopens.get(),
            0,
            "the window the user closed is not reopened for the link"
        );
        assert_eq!(
            log.count_containing("its window closed"),
            1,
            "the link is refused as its window closed: {log}"
        );
    }

    fn a_contained_callback_failure_runs_accepted_window_work() {
        use super::super::owner_dispatch::{
            RuntimeTask, dispatch_platform_ui_runtime, install_platform_ui_runtime,
        };
        let _owner = OwnerHostClearGuard::arm();
        let _cleanup = LoopTeardown::default();
        Box::new(HeadlessPlatform::new())
            .run(crate::app::runner::app_handler(move |owner| {
                let calls = Rc::new(Cell::new(0));
                let called = Rc::clone(&calls);
                let handle = install_test_controller(
                    owner,
                    Box::new(move |_, _, _, _| {
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
                // The callback's panic is its operation's, contained (ADR-0258
                // §1): the turn stays healthy and runs the window work it
                // accepted.
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
                .expect("a contained callback failure leaves the dispatch accepted");
                assert_eq!(calls.get(), 1, "the accepted request ran in the same turn");
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

    /// A platform that refuses a system below FLUI's floor fails the run with
    /// that refusal, typed, before any window opens; another platform failure
    /// stays a platform failure.
    fn a_platform_below_the_floor_fails_the_run_as_unsupported() {
        use flui_platform::{Environment, OsRelease};

        use std::cell::Cell;

        use crate::app::UnsupportedSystem;

        fn run_refused(refusal: flui_native::PlatformError) -> (LaunchError, bool) {
            let built = Rc::new(Cell::new(false));
            let witness = Rc::clone(&built);
            let app = DesktopRun::new(move |_| -> flui_widgets::Text {
                witness.set(true);
                panic!("never built below the floor");
            });
            let error = run_with_platform_factory(app, || Err(LaunchError::from_platform(refusal)))
                .expect_err("a refused platform fails the run");
            (error, built.get())
        }

        let (error, built) = run_refused(flui_native::PlatformError::UnsupportedOs {
            environment: Environment::Windows,
            found: OsRelease::from_display("build 9600"),
            floor: OsRelease::from_display("build 10240"),
        });
        assert!(
            matches!(
                &error,
                LaunchError::UnsupportedSystem(UnsupportedSystem::OsBelowFloor { environment: Environment::Windows, found, floor })
                    if found.to_string() == "build 9600" && floor.to_string() == "build 10240"
            ),
            "{error:?}"
        );
        assert!(!built, "no window is built below the floor");

        let (error, built) = run_refused(flui_native::PlatformError::MissingRequirement {
            environment: Environment::Linux,
            requirement: "the X11 RandR 1.5 extension",
        });
        assert!(
            matches!(
                &error,
                LaunchError::UnsupportedSystem(UnsupportedSystem::MissingRequirement {
                    environment: Environment::Linux,
                    requirement: "the X11 RandR 1.5 extension",
                })
            ),
            "{error:?}"
        );
        assert!(!built, "no window is built without a requirement");

        let (error, _) = run_refused(flui_native::PlatformError::Init {
            message: "no window class".to_owned(),
        });
        assert!(matches!(error, LaunchError::Platform { .. }), "{error:?}");
    }

    /// The application's executors are in place before the platform starts,
    /// so a host that runs its IO on the application's runtime finds the
    /// services it was configured with (ADR-0327 §3); a platform that then
    /// refuses takes the services down with the run.
    fn the_platform_starts_after_the_application_executors_are_installed() {
        let executors = flui_runtime::execution::DeterministicExecutors::new();
        let app = DesktopRun::new(|_| -> flui_widgets::Text {
            panic!("never built when the platform refuses");
        })
        .config(crate::app::AppConfig::default().executors(executors.host_executors()));
        let seen = Rc::new(std::cell::Cell::new(None));
        let witness = Rc::clone(&seen);
        let error = run_with_platform_factory(app, move || {
            let host_pools =
                APP_RUNTIME.with(|slot| !slot.borrow_mut().ensure_execution().owns_default_pools());
            witness.set(Some(host_pools));
            Err(LaunchError::OnReadyPanicked)
        })
        .expect_err("a refused platform fails the run");
        assert!(matches!(error, LaunchError::OnReadyPanicked), "{error:?}");
        assert_eq!(
            seen.get(),
            Some(true),
            "the platform starts on the configured executors"
        );
        assert!(
            APP_RUNTIME.with(|slot| slot.borrow().execution().is_none()),
            "a refused platform leaves no execution services behind"
        );

        // A host whose start unwinds after the services resolved leaves none
        // behind either, and the next run resolves the executors it names.
        let app = DesktopRun::new(|_| -> flui_widgets::Text {
            panic!("never built when the platform's start unwinds");
        })
        .config(crate::app::AppConfig::default().executors(executors.host_executors()));
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_with_platform_factory(app, || {
                APP_RUNTIME.with(|slot| {
                    slot.borrow_mut().ensure_execution();
                });
                panic!("the host's start unwinds");
            })
        }));
        assert!(unwound.is_err(), "the start's panic resumes to the caller");
        assert!(
            APP_RUNTIME.with(|slot| slot.borrow().execution().is_none()),
            "an unwind out of the platform's start leaves no execution services behind"
        );
    }

    fn main_window_factory_panic_is_typed_and_initial_window_is_fatal() {
        let app = DesktopRun::new(|_| -> flui_widgets::Text {
            panic!("factory witness");
        });
        let error = run_with_platform(app, Box::new(HeadlessPlatform::new()))
            .expect_err("initial factory fails");
        assert!(matches!(
            error,
            LaunchError::InitialWindow(AppWindowError::FactoryPanicked { .. })
        ));
        assert!(APP_RUNTIME.with(|slot| slot.borrow().application.main_ingress.is_none()));
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

        let app = DesktopRun::new(|_| -> flui_widgets::Text { flui_widgets::Text::new("") })
            .startup_window(StartupWindow::None)
            .config(AppConfig::new().exit_policy(ExitPolicy::ExplicitQuit));
        let unwound = tracing::subscriber::with_default(PanicsWhenReady, || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_with_platform(app, Box::new(HeadlessPlatform::new()))
            }))
        });
        assert!(unwound.is_err(), "the injected panic unwinds out of run");
        APP_RUNTIME.with(|slot| {
            let runtime = slot.borrow();
            assert!(
                runtime.application.main_controller.is_none()
                    && runtime.application.main_ingress.is_none(),
                "the unwind retired the main window"
            );
        });
    }

    #[test]
    fn main_window_installer_matrix() {
        crate::table_test::run_table(
            "main_window_installer_matrix",
            &[
                ("a_contained_callback_failure_runs_accepted_window_work", a_contained_callback_failure_runs_accepted_window_work as fn()),
                (
                    "main_window_waits_for_deferred_runtime_publication",
                    main_window_waits_for_deferred_runtime_publication as fn(),
                ),
                (
                    "a_kept_install_that_commits_later_settles_when_it_does",
                    a_kept_install_that_commits_later_settles_when_it_does as fn(),
                ),
                (
                    "a_link_into_a_window_closed_before_it_commits_is_refused",
                    a_link_into_a_window_closed_before_it_commits_is_refused as fn(),
                ),
                (
                    "main_window_pending_coalesces_and_recovers_after_installer_panic",
                    main_window_pending_coalesces_and_recovers_after_installer_panic as fn(),
                ),
                (
                    "a_platform_below_the_floor_fails_the_run_as_unsupported",
                    a_platform_below_the_floor_fails_the_run_as_unsupported as fn(),
                ),
                (
                    "the_platform_starts_after_the_application_executors_are_installed",
                    the_platform_starts_after_the_application_executors_are_installed as fn(),
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
        let app = DesktopRun::new(|_| -> flui_widgets::Text {
            panic!("factory failure before GPU setup");
        })
        .startup_window(StartupWindow::None)
        .config(
            AppConfig::new()
                .exit_policy(ExitPolicy::ExplicitQuit)
                .dev_reload(Counting(Arc::clone(&counts))),
        )
        .on_ready(move |handle| {
            assert_eq!(observed.get(), (1, 0), "attached when the loop starts");
            assert!(
                APP_RUNTIME.with(|slot| slot
                    .borrow()
                    .desktop_loop
                    .as_ref()
                    .is_some_and(|desktop| desktop.reload.is_attached())),
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
        let app = DesktopRun::new(|_| -> flui_widgets::Text {
            panic!("factory failure before GPU setup");
        })
        .startup_window(StartupWindow::None)
        .config(
            AppConfig::new()
                .exit_policy(ExitPolicy::ExplicitQuit)
                .dev_agent(Counting(Arc::clone(&counts))),
        )
        .on_ready(move |handle| {
            assert_eq!(observed.get(), (1, 0, 0), "attached when the loop starts");
            assert!(
                super::super::dev_agents::is_serving(),
                "the loop holds the hooks, which every window it installs is handed to"
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
        assert!(
            !super::super::dev_agents::is_serving(),
            "the loop's end released its hooks"
        );
    }

    /// The run's storage directory reaches the UI runtimes the host builds:
    /// the run resolves the host's storage once, at start, and a UI runtime
    /// built afterwards provides it to its widgets as the
    /// `StorageCapability`. The directory is the configured one, else the
    /// application's identifier; a desktop application with neither has no
    /// storage. Driven through `run_with_platform` itself, so the host's
    /// storage comes from the runner; the UI runtime is an `Isolated` window
    /// opened from `on_ready`, which reaches `host::build_lending_ui_runtime` as every
    /// runner site does, without a GPU.
    #[test]
    fn the_run_storage_dir_reaches_lifecycle_context() {
        let dir = || {
            flui_platform::AppDataDir::try_new("storage-host-test")
                .expect("a plain name is a valid directory")
        };
        let identified = || {
            crate::AppIdentity::try_new("Storage host test")
                .expect("a display name")
                .with_app_id(crate::AppId::try_new("dev.flui.storage_host_test").expect("an id"))
        };
        let cases: [(&str, AppConfig, bool); 4] = [
            (
                "a configured directory",
                AppConfig::new().storage_dir(dir()),
                true,
            ),
            (
                "an application identifier alone",
                AppConfig::new().application_identity(identified()),
                true,
            ),
            (
                "a configured directory beside an identifier",
                AppConfig::new()
                    .application_identity(identified())
                    .storage_dir(dir()),
                true,
            ),
            ("neither, on a desktop", AppConfig::new(), false),
        ];
        for (case, config, expected) in cases {
            assert_eq!(
                storage_reaches_a_ui_runtime(config),
                Some(expected),
                "{case}"
            );
        }
    }

    /// Whether a UI runtime built after a run started with `config` can
    /// acquire the `StorageCapability`; `None` when it was never observed.
    fn storage_reaches_a_ui_runtime(config: AppConfig) -> Option<bool> {
        use flui_platform::capability::Capability as _;
        use flui_view::persist::StorageCapability;

        let reached = Rc::new(Cell::new(None));
        let seen = Rc::clone(&reached);
        let app = DesktopRun::new(|_| -> flui_widgets::Text {
            panic!("no main window is opened");
        })
        .startup_window(StartupWindow::None)
        .config(config.exit_policy(ExitPolicy::ExplicitQuit))
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
                        observed.set(Some(ui_runtime.widgets().with_build_owner(|owner| {
                            owner
                                .capabilities()
                                .capabilities
                                .as_ref()
                                .is_some_and(|source| {
                                    source
                                        .acquire(
                                            std::any::TypeId::of::<StorageCapability>(),
                                            StorageCapability::NAME,
                                        )
                                        .is_ok()
                                })
                        })));
                    },
                )),
            )
            .expect("observe storage through the installed owner");
        });
        run_with_platform(app, Box::new(HeadlessPlatform::new())).expect("ordinary owner teardown");
        reached.get()
    }

    /// An identifier the application may declare always names a data
    /// directory, so the run's default storage directory never refuses one.
    #[test]
    fn every_app_id_names_a_data_dir() {
        let long = format!("com.{}", "a".repeat(flui_foundation::APP_ID_MAX_LEN - 4));
        for id in [
            "com.example.notes",
            "com.aux.notes",
            "con1.example",
            "Lpt.example",
            "comx.app",
            long.as_str(),
        ] {
            let app_id = crate::AppId::try_new(id).expect("an accepted identifier");
            assert!(
                flui_platform::AppDataDir::try_new(app_id.as_str()).is_ok(),
                "{id}"
            );
        }
    }
}
