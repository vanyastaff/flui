//! Installed frame resources, addressed by their originating surface.
//!
//! Native callbacks carry only a binding. A driver is leased out before any
//! frame work, and its return does not depend on the app's TLS still existing.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
#[cfg(any(all(test, not(target_os = "android")), target_arch = "wasm32"))]
use std::rc::Weak;

use super::native_retirement::NativeRetirement;
use flui_foundation::PresentationAddress;
use flui_runtime::ui_runtime::UiRuntime;

/// The immutable presentation incarnation that installed a frame driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) struct FrameBinding(PresentationAddress);

impl FrameBinding {
    pub(super) fn address(self) -> PresentationAddress {
        self.0
    }
}

/// A weak readiness authority for owner-local async renderer completions.
#[cfg(any(all(test, not(target_os = "android")), target_arch = "wasm32"))]
#[derive(Clone)]
pub(super) struct FrameLiveness(Weak<FrameRecord>);

#[cfg(any(all(test, not(target_os = "android")), target_arch = "wasm32"))]
impl FrameLiveness {
    pub(super) fn is_live(&self) -> bool {
        self.0.upgrade().is_some_and(|record| {
            record.admission.get() == FrameAdmission::Open
                && matches!(
                    *record.state.borrow(),
                    DriverState::Ready(_) | DriverState::Leased
                )
        })
    }

    /// Publishes an async result only into its still-open native binding.
    /// Both refusal and replacement return owned values after all guards end.
    pub(super) fn publish<T>(
        &self,
        slot: &parking_lot::Mutex<Option<T>>,
        value: T,
    ) -> Result<Option<T>, T> {
        if self.is_live() {
            Ok(slot.lock().replace(value))
        } else {
            Err(value)
        }
    }
}

pub(super) enum FrameDriver {
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    Desktop(super::desktop::DesktopFrameDriver),
    #[cfg(target_os = "android")]
    Android(super::android::AndroidFrameDriver),
    #[cfg(target_os = "ios")]
    Ios(super::ios::IosFrameDriver),
    #[cfg(target_arch = "wasm32")]
    Web(super::web::WebFrameDriver),
    /// Private failure seam: uses the product pump and a scripted sink.
    #[cfg(all(test, not(target_os = "android"), not(target_arch = "wasm32")))]
    Test(TestFrameDriver),
}

#[cfg(all(test, not(target_os = "android"), not(target_arch = "wasm32")))]
type TestResize = Box<dyn FnMut(flui_foundation::geometry::Size<f64>, f64)>;

#[cfg(all(test, not(target_os = "android"), not(target_arch = "wasm32")))]
pub(super) struct TestFrameDriver {
    pub(super) sink: flui_runtime::testing::ScriptedSink,
    pub(super) installed: Option<Box<dyn FnOnce()>>,
    pub(super) prelude: Option<Box<dyn FnMut()>>,
    pub(super) resize: Option<TestResize>,
}

impl FrameDriver {
    fn installed(&mut self, record: &Rc<FrameRecord>) {
        #[cfg(not(target_arch = "wasm32"))]
        let _ = record;
        match self {
            #[cfg(all(
                not(target_os = "android"),
                not(target_os = "ios"),
                not(target_arch = "wasm32")
            ))]
            Self::Desktop(_) => super::desktop::DesktopFrameDriver::installed(),
            #[cfg(target_os = "android")]
            Self::Android(_) => {}
            #[cfg(target_os = "ios")]
            Self::Ios(driver) => driver.installed(),
            #[cfg(target_arch = "wasm32")]
            Self::Web(driver) => driver.installed(FrameLiveness(Rc::downgrade(record))),
            #[cfg(all(test, not(target_os = "android"), not(target_arch = "wasm32")))]
            Self::Test(driver) => {
                if let Some(installed) = driver.installed.take() {
                    installed();
                }
            }
        }
    }
    fn resize(&mut self, size: flui_foundation::geometry::Size<f64>, scale_factor: f64) {
        match self {
            #[cfg(all(
                not(target_os = "android"),
                not(target_os = "ios"),
                not(target_arch = "wasm32")
            ))]
            Self::Desktop(driver) => driver.resize(size, scale_factor),
            #[cfg(target_os = "android")]
            Self::Android(driver) => driver.resize(size, scale_factor),
            #[cfg(target_os = "ios")]
            Self::Ios(driver) => driver.resize(size, scale_factor),
            #[cfg(target_arch = "wasm32")]
            Self::Web(driver) => driver.resize(size, scale_factor),
            #[cfg(all(test, not(target_os = "android"), not(target_arch = "wasm32")))]
            Self::Test(driver) => {
                if let Some(resize) = driver.resize.as_mut() {
                    resize(size, scale_factor);
                }
            }
        }
    }

    fn wake(&mut self, runtime: &mut UiRuntime, record: &Rc<FrameRecord>) {
        #[cfg(not(target_arch = "wasm32"))]
        let _ = record;
        match self {
            #[cfg(all(
                not(target_os = "android"),
                not(target_os = "ios"),
                not(target_arch = "wasm32")
            ))]
            Self::Desktop(driver) => driver.wake(runtime),
            #[cfg(target_os = "android")]
            Self::Android(driver) => driver.wake(runtime),
            #[cfg(target_os = "ios")]
            Self::Ios(driver) => driver.wake(runtime),
            #[cfg(target_arch = "wasm32")]
            Self::Web(driver) => driver.wake(runtime, FrameLiveness(Rc::downgrade(record))),
            #[cfg(all(test, not(target_os = "android"), not(target_arch = "wasm32")))]
            Self::Test(driver) => {
                if let Some(prelude) = driver.prelude.as_mut() {
                    prelude();
                }
                let now = web_time::Instant::now();
                let _ = runtime.pump(&mut flui_runtime::pump::SampledClock(now), &mut driver.sink);
            }
        }
    }
}

enum DriverState {
    Ready(FrameDriver),
    Leased,
    Retiring,
    Retired,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FrameAdmission {
    Prepared,
    Open,
    Closing,
}

struct FrameRecord {
    binding: FrameBinding,
    state: RefCell<DriverState>,
    /// Closing forbids new publication while earlier admitted frames finish.
    admission: Cell<FrameAdmission>,
}

/// Owns a complete driver before logical/native publication. Its weak async
/// authority stays inactive until the native registry commits this record.
pub(super) struct PreparedFrame(Rc<FrameRecord>);

impl PreparedFrame {
    pub(super) fn new(
        address: PresentationAddress,
        driver: FrameDriver,
    ) -> (Self, FrameRegistration) {
        let binding = FrameBinding(address);
        let record = Rc::new(FrameRecord {
            binding,
            state: RefCell::new(DriverState::Ready(driver)),
            admission: Cell::new(FrameAdmission::Prepared),
        });
        let registration = FrameRegistration {
            binding,
            #[cfg(all(test, not(target_os = "android"), not(target_arch = "wasm32")))]
            liveness: FrameLiveness(Rc::downgrade(&record)),
        };
        (Self(record), registration)
    }

    pub(super) fn address(&self) -> PresentationAddress {
        self.0.binding.0
    }
}

/// Native resource storage, separate from logical runtime membership.
pub(in crate::app) struct FrameDrivers {
    records: Vec<Rc<FrameRecord>>,
    retirement: NativeRetirement,
}

pub(super) struct FrameRegistration {
    pub(super) binding: FrameBinding,
    #[cfg(all(test, not(target_os = "android"), not(target_arch = "wasm32")))]
    pub(super) liveness: FrameLiveness,
}

#[derive(Debug, thiserror::Error)]
pub(super) enum FrameInstallError {
    #[error("the UI runtime already has an installed frame driver")]
    AlreadyInstalled,
}

impl FrameDrivers {
    pub(in crate::app) fn new(retirement: NativeRetirement) -> Self {
        Self {
            records: Vec::new(),
            retirement,
        }
    }
    pub(super) fn reserve_prepared(
        &mut self,
        frame: &PreparedFrame,
    ) -> Result<(), FrameInstallError> {
        if self
            .records
            .iter()
            .any(|record| record.binding.0.ui_runtime_id == frame.address().ui_runtime_id)
        {
            return Err(FrameInstallError::AlreadyInstalled);
        }
        self.records.reserve(1);
        Ok(())
    }

    /// Called only after reservation, while the same driver-store borrow is held.
    pub(super) fn publish_prepared(&mut self, frame: PreparedFrame) {
        frame.0.admission.set(FrameAdmission::Open);
        self.records.push(frame.0);
    }

    pub(super) fn checkout_presentation(
        &self,
        address: PresentationAddress,
    ) -> Option<FrameDriverLease> {
        let record = self
            .records
            .iter()
            .find(|record| record.binding.0 == address)?;
        let driver = {
            let mut state = record.state.borrow_mut();
            if !matches!(*state, DriverState::Ready(_)) {
                return None;
            }
            let DriverState::Ready(driver) = std::mem::replace(&mut *state, DriverState::Leased)
            else {
                unreachable!("BUG: checked ready driver changed without releasing its borrow");
            };
            driver
        };
        Some(FrameDriverLease {
            record: Rc::clone(record),
            retirement: self.retirement.clone(),
            driver: Some(driver),
        })
    }

    pub(super) fn retire_presentation(&mut self, address: PresentationAddress) {
        for record in &self.records {
            if record.binding.0 == address {
                self.retire_record(record);
            }
        }
    }

    pub(super) fn fence_presentation(&self, address: PresentationAddress) {
        for record in &self.records {
            if record.binding.0 == address {
                record.admission.set(FrameAdmission::Closing);
            }
        }
    }

    pub(super) fn retire_all(&mut self) {
        for record in &self.records {
            self.retire_record(record);
        }
    }

    fn retire_record(&self, record: &FrameRecord) {
        let driver = {
            let mut state = record.state.borrow_mut();
            match std::mem::replace(&mut *state, DriverState::Retired) {
                DriverState::Ready(driver) => Some(driver),
                DriverState::Leased | DriverState::Retiring => {
                    *state = DriverState::Retiring;
                    None
                }
                DriverState::Retired => None,
            }
        };
        if let Some(driver) = driver {
            self.retirement.frame(driver);
        }
    }

    /// Retired records own only metadata; outgoing resources live in retirement.
    pub(super) fn sweep_retired(&mut self) {
        self.records
            .retain(|record| !matches!(*record.state.borrow(), DriverState::Retired));
    }
}

pub(super) struct FrameDriverLease {
    record: Rc<FrameRecord>,
    retirement: NativeRetirement,
    driver: Option<FrameDriver>,
}

impl FrameDriverLease {
    pub(super) fn installed(&mut self) {
        self.driver
            .as_mut()
            .expect("BUG: active frame lease owns its driver")
            .installed(&self.record);
    }
    pub(super) fn resize(&mut self, size: flui_foundation::geometry::Size<f64>, scale_factor: f64) {
        self.driver
            .as_mut()
            .expect("BUG: active frame lease owns its driver")
            .resize(size, scale_factor);
    }
    pub(super) fn wake(&mut self, runtime: &mut UiRuntime) {
        self.driver
            .as_mut()
            .expect("BUG: active frame lease owns its driver")
            .wake(runtime, &self.record);
    }
}

impl Drop for FrameDriverLease {
    fn drop(&mut self) {
        let Some(driver) = self.driver.take() else {
            return;
        };
        let mut driver = Some(driver);
        {
            let mut state = self.record.state.borrow_mut();
            if matches!(*state, DriverState::Leased) {
                *state = DriverState::Ready(
                    driver
                        .take()
                        .expect("BUG: returning frame lease owns its driver"),
                );
            } else if matches!(*state, DriverState::Retiring) {
                *state = DriverState::Retired;
            }
        }
        if let Some(driver) = driver {
            self.retirement.frame(driver);
        }
    }
}

#[cfg(all(test, not(target_os = "android"), not(target_arch = "wasm32")))]
#[path = "frame_driver/tests.rs"]
mod tests;
