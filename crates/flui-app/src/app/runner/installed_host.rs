//! Strong logical/native ownership for one installed host incarnation.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_foundation::PresentationAddress;
use flui_runtime::owner::{
    InitializationOutcome, InstallInitialization, InstallToken, OwnerEffects, OwnerHost,
    PreparedInstall, PublicationError, RecoveryState,
};
use flui_runtime::ui_runtime::UiRuntime;

use crate::app::lifecycle_state::preserve_first_lifecycle_panic;

use super::host::APP_RUNTIME;
use super::native_bindings::{NativeBindings, NativeInstallError, PreparedNativeWindow};
use super::window_install::InstallationWindow;

type Failure = Option<Box<dyn std::any::Any + Send>>;

#[derive(Clone, Copy, Debug, thiserror::Error)]
pub(super) enum InstallError {
    #[error(transparent)]
    Logical(#[from] PublicationError),
    #[error(transparent)]
    Native(#[from] NativeInstallError),
    #[error("window initialization did not complete")]
    InitializationFailed,
    #[error("the native window closed before installation completed")]
    WindowClosed,
}

/// Admission is distinct from publication. A pending receipt never represents
/// a ready window; the completion tail observes its eventual result.
#[must_use = "observe installation completion before reporting a window ready"]
pub(super) struct Installation {
    #[cfg(any(
        all(test, not(target_os = "android"), not(target_arch = "wasm32")),
        target_os = "ios"
    ))]
    pub(super) address: PresentationAddress,
    result: Rc<Cell<Option<Result<(), InstallError>>>>,
    closed: Arc<AtomicBool>,
}

impl Installation {
    pub(super) fn outcome(&self) -> Option<Result<(), InstallError>> {
        self.result.get()
    }

    pub(super) fn cancel(&self) {
        self.closed.store(true, Ordering::Release);
    }
}

impl Drop for Installation {
    fn drop(&mut self) {
        if self.outcome().is_none() {
            self.cancel();
        }
    }
}

struct PendingInstall {
    prepared: PreparedInstall,
    native: PreparedNativeWindow,
    result: Rc<Cell<Option<Result<(), InstallError>>>>,
    initial: InstallInitialization,
    window: InstallationWindow,
}

struct InitializingInstall {
    address: PresentationAddress,
    result: Rc<Cell<Option<Result<(), InstallError>>>>,
    window: InstallationWindow,
}

#[derive(Clone)]
pub(in crate::app) struct InstalledHost(Rc<HostState>);

struct HostState {
    logical: OwnerHost,
    native: NativeBindings,
    pending: RefCell<Vec<PendingInstall>>,
    initializing: RefCell<Vec<InitializingInstall>>,
    #[cfg(not(target_arch = "wasm32"))]
    closed_presentations: RefCell<Vec<PresentationAddress>>,
}

impl InstalledHost {
    pub(in crate::app) fn new() -> Self {
        Self(Rc::new(HostState {
            logical: OwnerHost::new(),
            native: NativeBindings::new(),
            pending: RefCell::new(Vec::new()),
            initializing: RefCell::new(Vec::new()),
            #[cfg(not(target_arch = "wasm32"))]
            closed_presentations: RefCell::new(Vec::new()),
        }))
    }

    pub(in crate::app) fn logical(&self) -> &OwnerHost {
        &self.0.logical
    }

    pub(super) fn same_host(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    pub(in crate::app) fn native(&self) -> &NativeBindings {
        &self.0.native
    }

    pub(super) fn effects(&self) -> &dyn OwnerEffects {
        self.0.as_ref()
    }

    pub(super) fn shutdown(&self) {
        self.0.logical.shutdown(self.effects());
    }

    pub(super) fn submit_window(
        &self,
        mut prepared: PreparedInstall,
        native: PreparedNativeWindow,
        initial: InstallInitialization,
        window: InstallationWindow,
    ) -> Installation {
        #[cfg(any(
            all(test, not(target_os = "android"), not(target_arch = "wasm32")),
            target_os = "ios"
        ))]
        let address = prepared.address();
        let token = prepared
            .take_delivery()
            .expect("BUG: a native bundle takes its publication token once");
        let result = Rc::new(Cell::new(None));
        let cancellation = window.cancellation();
        self.0.pending.borrow_mut().push(PendingInstall {
            prepared,
            native,
            result: Rc::clone(&result),
            initial,
            window,
        });
        if let Err(refused) = self.0.logical.queue_install(token, self.effects()) {
            let pending = self.0.take_install(refused.token);
            result.set(Some(Err(InstallError::Logical(refused.error))));
            drop(pending);
        }
        Installation {
            #[cfg(any(
                all(test, not(target_os = "android"), not(target_arch = "wasm32")),
                target_os = "ios"
            ))]
            address,
            result,
            closed: cancellation,
        }
    }

    pub(in crate::app) fn has_pending_installs(&self) -> bool {
        !self.0.pending.borrow().is_empty() || !self.0.initializing.borrow().is_empty()
    }
}

impl HostState {
    fn is_current(&self) -> bool {
        APP_RUNTIME
            .try_with(|slot| {
                slot.try_borrow()
                    .is_ok_and(|state| std::ptr::eq(state.installed_host.0.as_ref(), self))
            })
            .unwrap_or(false)
    }

    fn take_install(&self, token: InstallToken) -> PendingInstall {
        let mut pending = self.pending.borrow_mut();
        let index = pending
            .iter()
            .position(|entry| entry.prepared.address() == token.address())
            .expect("BUG: accepted install token retains its complete native bundle");
        pending.remove(index)
    }

    fn complete_native(&self, first: &mut Failure) {
        self.native.sweep_retired();
        self.native.retirement().drain(first);
    }

    fn installation_is_live(&self, entry: &InitializingInstall) -> bool {
        self.native.contains_address(entry.address)
            && self.logical.presentation_dispatcher(entry.address).is_ok()
            && !entry.window.is_closed()
    }
}

impl OwnerEffects for HostState {
    fn runtime_lifecycle(
        &self,
        _: flui_foundation::UiRuntimeId,
        state: flui_scheduler::AppLifecycleState,
    ) {
        #[cfg(all(
            not(target_os = "android"),
            not(target_os = "ios"),
            not(target_arch = "wasm32")
        ))]
        if self.is_current() {
            APP_RUNTIME.with(|slot| slot.borrow_mut().main_host_lifecycle = state);
        }
        #[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
        let _ = state;
    }

    fn runtimes_stopped(&self, _: RecoveryState) {
        #[cfg(all(
            not(target_os = "android"),
            not(target_os = "ios"),
            not(target_arch = "wasm32")
        ))]
        if self.is_current() {
            APP_RUNTIME.with(|slot| {
                let mut state = slot.borrow_mut();
                if state.quit_notification == crate::app::runtime::QuitNotification::Notifying {
                    state.quit_notification = crate::app::runtime::QuitNotification::Notified;
                }
            });
        }
    }

    fn retire_host(&self, _: &[PresentationAddress], _: RecoveryState) {
        // Each HostState owns its own native registry. No replacement shares it.
        self.native.retire_all();
        let mut first = None;
        self.complete_native(&mut first);
        finish(first);
    }

    fn commit_install(
        &self,
        token: InstallToken,
        _: RecoveryState,
    ) -> Option<InstallInitialization> {
        let pending = self.take_install(token);
        if pending.window.is_closed() {
            pending.result.set(Some(Err(InstallError::WindowClosed)));
            drop(pending);
            return None;
        }
        self.initializing.borrow_mut().reserve(1);
        let PendingInstall {
            prepared,
            native,
            result,
            initial,
            window,
        } = pending;
        let address = prepared.address();
        let permit = match self.logical.publication(prepared) {
            Ok(permit) => permit,
            Err(refused) => {
                result.set(Some(Err(InstallError::Logical(refused.error))));
                drop(refused);
                return None;
            }
        };
        if let Err((error, native)) = self.native.publish(address, native) {
            result.set(Some(Err(InstallError::Native(error))));
            drop(permit);
            drop(native);
            return None;
        }
        let published = permit.commit();
        debug_assert_eq!(published, address);
        self.initializing.borrow_mut().push(InitializingInstall {
            address,
            result,
            window,
        });
        Some(initial)
    }

    fn finish_install(
        &self,
        address: PresentationAddress,
        outcome: InitializationOutcome,
        _: RecoveryState,
    ) {
        let mut entry = {
            let mut initializing = self.initializing.borrow_mut();
            let index = initializing
                .iter()
                .position(|entry| entry.address == address)
                .expect("BUG: published installation retains its completion receipt");
            initializing.remove(index)
        };
        let mut first = None;
        if outcome == InitializationOutcome::Ready {
            contain(&mut first, || entry.window.native().request_redraw());
            if first.is_none() && self.installation_is_live(&entry) {
                contain(&mut first, || self.native.installed(address));
            }
            // Native activation can close the window or tear down its host.
            // The development hook sees only a still-live installation and
            // may itself close it before the readiness receipt is settled.
            #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
            if first.is_none() && self.installation_is_live(&entry) {
                contain(&mut first, || entry.window.notify_opened());
            }
            if first.is_none() && self.installation_is_live(&entry) {
                entry.window.publish();
                entry.result.set(Some(Ok(())));
                return;
            }
        }
        entry
            .result
            .set(Some(Err(InstallError::InitializationFailed)));
        contain(&mut first, || {
            if let Ok(target) = self.logical.presentation_dispatcher(address) {
                self.native.fence_presentation(address);
                let _ = target.close(self);
            }
        });
        self.native.retire_presentation(address);
        self.complete_native(&mut first);
        contain(&mut first, || drop(entry.window));
        finish(first);
    }

    fn cancel_install(&self, token: InstallToken, _: RecoveryState) {
        let pending = self.take_install(token);
        pending
            .result
            .set(Some(Err(InstallError::Logical(PublicationError::Closed))));
        drop(pending);
    }

    fn resize_surface(
        &self,
        address: PresentationAddress,
        size: flui_foundation::geometry::Size<f64>,
        scale_factor: f64,
    ) {
        self.native.resize(address, size, scale_factor);
    }

    fn frame(&self, address: PresentationAddress, runtime: &mut UiRuntime) {
        self.native.frame_address(address, runtime);
    }

    fn retire_presentation(&self, address: PresentationAddress, _: Option<PresentationAddress>) {
        self.native.retire_presentation(address);
        #[cfg(not(target_arch = "wasm32"))]
        self.closed_presentations.borrow_mut().push(address);
        let mut first = None;
        self.complete_native(&mut first);
        finish(first);
    }

    fn after_turn(&self, turn_recovery: RecoveryState) {
        #[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
        let _ = turn_recovery;
        let mut first = None;
        if self.is_current() && turn_recovery == RecoveryState::Healthy {
            contain(&mut first, || {
                self.native.refresh_text_sizing(&self.logical, self, false);
            });
        }
        self.complete_native(&mut first);
        #[cfg(not(target_arch = "wasm32"))]
        let closed = std::mem::take(&mut *self.closed_presentations.borrow_mut());
        if self.is_current() {
            #[cfg(all(
                not(target_os = "android"),
                not(target_os = "ios"),
                not(target_arch = "wasm32")
            ))]
            for address in &closed {
                contain(&mut first, || {
                    super::main_window::main_window_closed(*address);
                });
            }
            #[cfg(not(target_arch = "wasm32"))]
            if !closed.is_empty() {
                let wake =
                    APP_RUNTIME.with(|slot| slot.borrow().exit_policy_reevaluation_notifier());
                if let Some(wake) = wake {
                    contain(&mut first, || wake());
                }
            }
            #[cfg(all(
                not(target_os = "android"),
                not(target_os = "ios"),
                not(target_arch = "wasm32")
            ))]
            {
                contain(&mut first, super::owner_dispatch::drain_quit_notification);
                // Retire accepted work above, but do not start new window
                // construction while preserving a failure from this turn.
                let recovery = if first.is_some() {
                    RecoveryState::PreservingFailure
                } else {
                    turn_recovery
                };
                contain(&mut first, || {
                    super::secondary_window::drain_pending_secondary_window_completions(recovery);
                });
                if turn_recovery == RecoveryState::Healthy && first.is_none() {
                    contain(&mut first, super::main_window::drive_main_window);
                }
            }
        }
        finish(first);
    }

    fn request_continuation(&self) -> bool {
        if !self.is_current() {
            return false;
        }
        let wake = APP_RUNTIME.with(|slot| slot.borrow().owner_turn_wake.clone());
        wake.is_some_and(|wake| wake())
    }

    fn text_sizing(
        &self,
        frontier: flui_runtime::owner::TextSizingFrontier,
        recovery: RecoveryState,
    ) -> Option<flui_runtime::owner::TextSizingFrontier> {
        if recovery == RecoveryState::PreservingFailure || !self.is_current() {
            return Some(frontier);
        }
        self.native.resolve_text_sizing(frontier, self)
    }
}

pub(super) fn contain(first: &mut Failure, run: impl FnOnce()) {
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).err();
    preserve_first_lifecycle_panic(first, failure, "native host completion");
}

pub(super) fn finish(first: Failure) {
    if let Some(first) = first {
        if std::thread::panicking() {
            std::mem::forget(first);
        } else {
            std::panic::resume_unwind(first);
        }
    }
}

impl Drop for HostState {
    fn drop(&mut self) {
        self.logical.shutdown(self);
    }
}
