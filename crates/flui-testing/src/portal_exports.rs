//! A headless window's simulated `xdg-foreign` exports
//! ([`HeadlessWindow::with_portal_export`](crate::host::HeadlessWindow::with_portal_export)).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use parking_lot::Mutex;

/// A headless window's simulated `xdg-foreign` exports: the handle each portal
/// request is answered with, and how many exports requests still hold.
/// Cheap to clone; clones share the count.
#[derive(Clone, Debug)]
pub struct HeadlessPortalExports {
    handle: Arc<str>,
    /// Each export a request may still hold; one released is `true`.
    // `Arc` and atomics: `PortalParent::wayland` takes an `Any + Send` export.
    exports: Arc<Mutex<Vec<Weak<AtomicBool>>>>,
}

impl HeadlessPortalExports {
    /// Exports answered with `handle`, none made yet.
    pub(crate) fn new(handle: Arc<str>) -> Self {
        Self {
            handle,
            exports: Arc::default(),
        }
    }

    /// How many exports portal requests hold now: one per request between
    /// its ask and its end, none once the window closed.
    #[must_use]
    pub fn live(&self) -> usize {
        self.exports
            .lock()
            .iter()
            .filter_map(Weak::upgrade)
            .filter(|released| !released.load(Ordering::SeqCst))
            .count()
    }

    /// A new export, counted until the parent holding it drops or the window
    /// closes.
    pub(crate) fn export(&self) -> flui_platform::capability::PortalParent {
        /// One export; its drop is the release.
        struct Export(Arc<AtomicBool>);

        impl Drop for Export {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }

        let released = Arc::new(AtomicBool::new(false));
        let mut exports = self.exports.lock();
        exports.retain(|export| export.strong_count() > 0);
        exports.push(Arc::downgrade(&released));
        drop(exports);
        flui_platform::capability::PortalParent::wayland(&self.handle, Export(released))
    }

    /// The window closed: every export is released, as a compositor revokes
    /// a destroyed toplevel's, whoever still holds its parent.
    pub(crate) fn release_all(&self) {
        for export in self.exports.lock().iter().filter_map(Weak::upgrade) {
            export.store(true, Ordering::SeqCst);
        }
    }
}
