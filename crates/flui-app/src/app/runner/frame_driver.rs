//! Installed frame resources, addressed by their originating surface.
//!
//! Native callbacks carry only a binding. A driver is leased out before any
//! frame work, and its return does not depend on the app's TLS still existing.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
#[cfg(any(test, target_arch = "wasm32"))]
use std::rc::Weak;

use super::native_retirement::NativeRetirement;
use flui_foundation::{PresentationAddress, UiRuntimeId};
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
#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone)]
pub(super) struct FrameLiveness(Weak<FrameRecord>);

#[cfg(any(test, target_arch = "wasm32"))]
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
    #[cfg(test)]
    Test(TestFrameDriver),
}

#[cfg(test)]
pub(super) struct TestFrameDriver {
    pub(super) sink: flui_runtime::testing::ScriptedSink,
    pub(super) prelude: Option<Box<dyn FnMut()>>,
}

impl FrameDriver {
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
            #[cfg(test)]
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
    Open,
    Closing,
}

struct FrameRecord {
    binding: FrameBinding,
    state: RefCell<DriverState>,
    /// Closing forbids new publication while earlier admitted frames finish.
    admission: Cell<FrameAdmission>,
}

/// Native resource storage, separate from logical runtime membership.
pub(in crate::app) struct FrameDrivers {
    records: Vec<Rc<FrameRecord>>,
    retirement: NativeRetirement,
}

pub(super) struct FrameRegistration {
    pub(super) binding: FrameBinding,
    #[cfg(any(test, target_arch = "wasm32"))]
    pub(super) liveness: FrameLiveness,
}

#[derive(Debug, thiserror::Error)]
pub(super) enum FrameInstallError {
    #[error("frame installation ran on a different owner thread")]
    WrongThread,
    #[error("the presentation closed before its frame driver was installed")]
    TargetRetired,
    #[error("the UI runtime already has an installed frame driver")]
    AlreadyInstalled,
}

pub(super) fn install_frame_driver(
    dispatcher: super::owner_dispatch::PresentationDispatcher,
    driver: FrameDriver,
) -> Result<FrameRegistration, FrameInstallError> {
    let result = if std::thread::current().id() == dispatcher.owner_thread {
        super::host::APP_RUNTIME.with(|slot| {
            let mut state = slot.borrow_mut();
            if !state.registry.contains_address(dispatcher.address)
                || state.closing_presentations.contains(&dispatcher.address)
            {
                return Err((FrameInstallError::TargetRetired, driver));
            }
            state
                .frame_drivers
                .install(dispatcher.address, driver)
                .map_err(|driver| (FrameInstallError::AlreadyInstalled, driver))
        })
    } else {
        Err((FrameInstallError::WrongThread, driver))
    };
    result.map_err(|(error, driver)| {
        // A rejected driver can own user reload captures. Retire outside TLS.
        drop(driver);
        error
    })
}

impl FrameDrivers {
    pub(in crate::app) fn new(retirement: NativeRetirement) -> Self {
        Self {
            records: Vec::new(),
            retirement,
        }
    }
    /// The caller validates logical/native authority before this pure insert.
    /// A binding is never replaced, including while its driver is leased out.
    pub(super) fn install(
        &mut self,
        address: PresentationAddress,
        driver: FrameDriver,
    ) -> Result<FrameRegistration, FrameDriver> {
        if self
            .records
            .iter()
            .any(|record| record.binding.0.ui_runtime_id == address.ui_runtime_id)
        {
            return Err(driver);
        }
        let binding = FrameBinding(address);
        let record = Rc::new(FrameRecord {
            binding,
            state: RefCell::new(DriverState::Ready(driver)),
            admission: Cell::new(FrameAdmission::Open),
        });
        #[cfg(any(test, target_arch = "wasm32"))]
        let liveness = FrameLiveness(Rc::downgrade(&record));
        self.records.push(record);
        Ok(FrameRegistration {
            binding,
            #[cfg(any(test, target_arch = "wasm32"))]
            liveness,
        })
    }

    pub(super) fn checkout(&self, binding: FrameBinding) -> Option<FrameDriverLease> {
        let record = self
            .records
            .iter()
            .find(|record| record.binding == binding)?;
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

    /// Revokes async and frame authority before observers or resource cleanup.
    /// Owned drivers move to retirement; no user destructor runs here.
    pub(in crate::app) fn retire_runtime(&mut self, id: UiRuntimeId) {
        for record in &self.records {
            if record.binding.0.ui_runtime_id == id {
                self.retire_record(record);
            }
        }
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
