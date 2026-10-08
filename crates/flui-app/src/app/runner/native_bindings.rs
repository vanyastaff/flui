//! Installed native ownership retained independently of the app's TLS slot.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use flui_foundation::{PresentationAddress, geometry::Size};
use flui_runtime::ui_runtime::UiRuntime;

use crate::app::close_request::{CloseRequestRouter, PreparedCloseRequest};
use crate::app::window_registry::{PreparedWindowRegistration, RegistryError, WindowRegistry};

use super::frame_driver::{FrameDrivers, PreparedFrame};
use super::native_retirement::NativeRetirement;

#[derive(Clone)]
pub(in crate::app) struct NativeBindings(Rc<NativeState>);

struct NativeState {
    windows: RefCell<WindowRegistry>,
    drivers: RefCell<FrameDrivers>,
    close_requests: Arc<CloseRequestRouter>,
    retirement: NativeRetirement,
}

pub(super) struct PreparedNativeWindow {
    pub(super) registration: PreparedWindowRegistration,
    pub(super) frame: Option<PreparedFrame>,
    pub(super) close: Option<PreparedCloseRequest>,
}

impl Drop for PreparedNativeWindow {
    fn drop(&mut self) {
        // Withdraw both owners before invoking either destructor. Each resource
        // retires outside publication guards, even when its sibling panics.
        let frame = self.frame.take();
        let close = self.close.take();
        let mut first =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(frame))).err();
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(close))).err();
        crate::app::lifecycle_state::preserve_first_lifecycle_panic(
            &mut first,
            failure,
            "unpublished native window",
        );
        if let Some(first) = first {
            if std::thread::panicking() {
                std::mem::forget(first);
            } else {
                std::panic::resume_unwind(first);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(super) enum NativeInstallError {
    #[error(transparent)]
    Window(#[from] RegistryError),
    #[error("the runtime already has a frame driver")]
    FrameAlreadyInstalled,
    #[error("the presentation already has a close handler registration")]
    CloseAlreadyInstalled,
}

impl NativeBindings {
    /// Reserve every native membership before the first write. Refusal returns
    /// the complete proposal after all registry guards have been released.
    pub(super) fn publish(
        &self,
        address: PresentationAddress,
        mut prepared: PreparedNativeWindow,
    ) -> Result<(), (NativeInstallError, PreparedNativeWindow)> {
        let result = (|| {
            let mut windows = self.0.windows.borrow_mut();
            let mut drivers = self.0.drivers.borrow_mut();
            windows.reserve_registration(&prepared.registration)?;
            if let Some(frame) = &prepared.frame {
                debug_assert_eq!(frame.address(), address);
                drivers
                    .reserve_prepared(frame)
                    .map_err(|_| NativeInstallError::FrameAlreadyInstalled)?;
            }
            let close = prepared
                .close
                .as_ref()
                .map(|entry| {
                    self.0
                        .close_requests
                        .publication(entry)
                        .ok_or(NativeInstallError::CloseAlreadyInstalled)
                })
                .transpose()?;
            Ok((windows, drivers, close))
        })();
        let (mut windows, mut drivers, close) = match result {
            Ok(guards) => guards,
            Err(error) => return Err((error, prepared)),
        };
        windows.publish_registration(&prepared.registration, address);
        if let Some(frame) = prepared.frame.take() {
            drivers.publish_prepared(frame);
        }
        if let (Some(permit), Some(entry)) = (close, prepared.close.take()) {
            permit.publish(entry);
        }
        Ok(())
    }
    pub(in crate::app) fn new() -> Self {
        let retirement = NativeRetirement::default();
        Self(Rc::new(NativeState {
            windows: RefCell::new(WindowRegistry::new()),
            drivers: RefCell::new(FrameDrivers::new(retirement.clone())),
            close_requests: Arc::new(CloseRequestRouter::new()),
            retirement,
        }))
    }

    pub(in crate::app) fn retirement(&self) -> NativeRetirement {
        self.0.retirement.clone()
    }

    pub(in crate::app) fn close_requests(&self) -> Arc<CloseRequestRouter> {
        Arc::clone(&self.0.close_requests)
    }

    pub(in crate::app) fn contains_address(&self, address: PresentationAddress) -> bool {
        self.0.windows.borrow().contains_address(address)
    }

    pub(super) fn frame_address(&self, address: PresentationAddress, runtime: &mut UiRuntime) {
        let lease = self.0.drivers.borrow().checkout_presentation(address);
        if let Some(mut lease) = lease {
            lease.wake(runtime);
        }
    }

    pub(super) fn installed(&self, address: PresentationAddress) {
        let lease = self.0.drivers.borrow().checkout_presentation(address);
        if let Some(mut lease) = lease {
            lease.installed();
        }
    }

    pub(super) fn resize(&self, address: PresentationAddress, size: Size<f64>, scale_factor: f64) {
        let lease = self.0.drivers.borrow().checkout_presentation(address);
        if let Some(mut lease) = lease {
            lease.resize(size, scale_factor);
        }
    }

    pub(super) fn fence_presentation(&self, address: PresentationAddress) {
        self.0.drivers.borrow().fence_presentation(address);
    }

    pub(super) fn retire_presentation(&self, address: PresentationAddress) {
        self.0.windows.borrow_mut().remove_presentation(address);
        self.0.drivers.borrow_mut().retire_presentation(address);
        self.0
            .retirement
            .close_handlers(self.0.close_requests.take(address));
    }

    pub(super) fn retire_all(&self) -> usize {
        let removed = self.0.windows.borrow_mut().clear();
        self.0.drivers.borrow_mut().retire_all();
        self.0
            .retirement
            .close_handlers(self.0.close_requests.take_all());
        removed
    }

    pub(super) fn sweep_retired(&self) {
        self.0.drivers.borrow_mut().sweep_retired();
    }
}

impl Drop for NativeState {
    fn drop(&mut self) {
        self.windows.get_mut().clear();
        self.drivers.get_mut().retire_all();
        self.retirement
            .close_handlers(self.close_requests.take_all());
        let mut failure = None;
        self.retirement.drain(&mut failure);
        if let Some(failure) = failure {
            if std::thread::panicking() {
                std::mem::forget(failure);
            } else {
                std::panic::resume_unwind(failure);
            }
        }
    }
}
