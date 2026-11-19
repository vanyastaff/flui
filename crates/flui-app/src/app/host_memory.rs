//! The memory the host gives back when the system asks for it (ADR-0237
//! decision 9): the receivers the application registered
//! ([`AppConfig::memory_reclaim`]). Each UI runtime trims its own caches,
//! its decoded images among them, after these. The GPU memory behind every
//! window (pooled buffers, the glyph atlas and image textures) is the raster
//! host's to trim, which the same notice reaches (`runner::raster_host`).
//!
//! One [`HostMemory`] per host is reclaimed once per notice, however many
//! windows it has, and a window opened later is reached along with the
//! receivers of the windows opened before it.
//!
//! Owner-thread state (ADR-0323 §1): the host's run start replaces the
//! receivers and the owner's turn for a memory-pressure notice reclaims
//! them, both on the loop's thread. The list is cloned out before any
//! receiver runs and a replaced list is dropped after the new one is
//! stored, so a receiver's `reclaim` or `Drop` finds the list free.
//!
//! [`AppConfig::memory_reclaim`]: crate::app::AppConfig::memory_reclaim

use std::cell::RefCell;
use std::rc::Rc;

use flui_foundation::memory::{MemoryPressureLevel, ReclaimMemory, reclaim_each};
use flui_foundation::panic::FirstPanic;

/// Receivers an application registered, shared by the clones of its
/// configuration.
pub(crate) type SharedReceiver = Rc<dyn ReclaimMemory>;

/// What a host trims on a memory-pressure notice.
#[derive(Default)]
pub(crate) struct HostMemory {
    receivers: RefCell<Vec<SharedReceiver>>,
}

impl HostMemory {
    /// Replace the application's receivers with `receivers`, in the order they
    /// are trimmed. A run starts from its own configuration, not the previous
    /// run's.
    pub(crate) fn replace_receivers(&self, receivers: Vec<SharedReceiver>) {
        let previous = self.receivers.replace(receivers);
        drop(previous);
    }
}

impl ReclaimMemory for HostMemory {
    fn reclaim(&self, level: MemoryPressureLevel) {
        let receivers = self.receivers.borrow().clone();
        let mut failure = FirstPanic::new();
        reclaim_each(
            receivers.iter().map(|receiver| &**receiver),
            level,
            &mut failure,
        );
        failure.resume();
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use flui_foundation::memory::{MemoryPressureLevel, ReclaimMemory};

    use super::HostMemory;

    struct Named(&'static str, Rc<RefCell<Vec<String>>>);

    impl ReclaimMemory for Named {
        fn reclaim(&self, level: MemoryPressureLevel) {
            self.1.borrow_mut().push(format!("{} {level:?}", self.0));
        }
    }

    #[test]
    fn the_registered_receivers_are_trimmed_in_order_and_a_new_run_replaces_them() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let memory = HostMemory::default();
        memory.replace_receivers(vec![
            Rc::new(Named("assets", Rc::clone(&log))),
            Rc::new(Named("cache", Rc::clone(&log))),
        ]);
        memory.reclaim(MemoryPressureLevel::Critical);
        memory.replace_receivers(vec![Rc::new(Named("next run", Rc::clone(&log)))]);
        memory.reclaim(MemoryPressureLevel::Moderate);
        assert_eq!(
            *log.borrow(),
            ["assets Critical", "cache Critical", "next run Moderate"]
        );
    }
}
