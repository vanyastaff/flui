//! The memory the host gives back when the system asks for it (ADR-0237
//! decision 9): the receivers the application registered
//! ([`AppConfig::with_memory_trim`]) and the GPU memory behind each raster lane.
//!
//! One [`HostMemory`] per host is every UI runtime's single host receiver, so a
//! window opened later reaches the lanes and receivers of the windows opened
//! before it. The lists are cloned out before any receiver runs, so a receiver
//! may register or release another without meeting this lock.
//!
//! [`AppConfig::with_memory_trim`]: crate::app::AppConfig::with_memory_trim

use std::sync::Arc;

use flui_foundation::memory::{MemoryPressureLevel, MemoryTrim, trim_all};
use parking_lot::Mutex;

/// Receivers an application registered, shared with every UI runtime.
pub(crate) type SharedReceiver = Arc<dyn MemoryTrim + Send + Sync>;

/// What a host trims on a memory-pressure notice.
#[derive(Default)]
pub(crate) struct HostMemory {
    receivers: Mutex<Vec<SharedReceiver>>,
    #[cfg(not(target_arch = "wasm32"))]
    rasters: Mutex<Vec<flui_engine::RasterHandle>>,
}

impl HostMemory {
    /// Replace the application's receivers with `receivers`, in the order they
    /// are trimmed. A run starts from its own configuration, not the previous
    /// run's.
    pub(crate) fn replace_receivers(&self, receivers: Vec<SharedReceiver>) {
        let previous = std::mem::replace(&mut *self.receivers.lock(), receivers);
        drop(previous);
    }

    /// Trim the GPU memory behind `lane` along with the application's
    /// receivers, until the lane's owner is gone.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn track_raster(&self, lane: flui_engine::RasterHandle) {
        let mut rasters = self.rasters.lock();
        rasters.retain(flui_engine::RasterHandle::is_live);
        rasters.push(lane);
    }
}

impl MemoryTrim for HostMemory {
    fn trim(&self, level: MemoryPressureLevel) {
        let receivers = self.receivers.lock().clone();
        let first = trim_all(receivers.iter().map(|receiver| &**receiver), level);
        #[cfg(not(target_arch = "wasm32"))]
        let first = {
            let rasters = {
                let mut rasters = self.rasters.lock();
                rasters.retain(flui_engine::RasterHandle::is_live);
                rasters.clone()
            };
            match (first, trim_all(rasters.iter(), level)) {
                (Some(first), Some(later)) => {
                    std::mem::forget(later);
                    Some(first)
                }
                (first, later) => first.or(later),
            }
        };
        if let Some(payload) = first {
            std::panic::resume_unwind(payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::Mutex;

    use flui_foundation::memory::{MemoryPressureLevel, MemoryTrim};

    use super::HostMemory;

    struct Named(&'static str, Arc<Mutex<Vec<String>>>);

    impl MemoryTrim for Named {
        fn trim(&self, level: MemoryPressureLevel) {
            self.1
                .lock()
                .expect("log")
                .push(format!("{} {level:?}", self.0));
        }
    }

    #[test]
    fn the_registered_receivers_are_trimmed_in_order_and_a_new_run_replaces_them() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let memory = HostMemory::default();
        memory.replace_receivers(vec![
            Arc::new(Named("assets", Arc::clone(&log))),
            Arc::new(Named("cache", Arc::clone(&log))),
        ]);
        memory.trim(MemoryPressureLevel::Critical);
        memory.replace_receivers(vec![Arc::new(Named("next run", Arc::clone(&log)))]);
        memory.trim(MemoryPressureLevel::Moderate);
        assert_eq!(
            *log.lock().expect("log"),
            ["assets Critical", "cache Critical", "next run Moderate"]
        );
    }
}
