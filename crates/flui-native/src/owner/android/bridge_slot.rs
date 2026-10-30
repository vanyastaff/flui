//! Where `FluiView`'s node provider finds the accessibility bridge of the
//! window over the view. The view is the owner layer's and the bridge the
//! accessibility feature's, so the two meet through this slot, which the
//! feature fills as a window opens and empties as it closes. Pure Rust, so
//! its rules run on every host.
//!
//! A window can open over the view before the previous one is torn down, so
//! a bridge is withdrawn by identity: the closing window takes out its own
//! bridge, never the one of the window that replaced it.

use std::sync::Arc;

use parking_lot::Mutex;

/// The bridge, if a window holds the view now.
pub(crate) struct BridgeSlot<T: ?Sized> {
    held: Mutex<Option<Arc<T>>>,
}

impl<T: ?Sized> Default for BridgeSlot<T> {
    fn default() -> Self {
        Self {
            held: Mutex::new(None),
        }
    }
}

impl<T: ?Sized> BridgeSlot<T> {
    /// Hold `bridge` from now on; the bridge it replaces is returned, so it
    /// is dropped outside the lock.
    pub(crate) fn provide(&self, bridge: Arc<T>) -> Option<Arc<T>> {
        self.held.lock().replace(bridge)
    }

    /// Take `bridge` out if it is the one held; `None` when another took its
    /// place or it is already gone.
    pub(crate) fn withdraw(&self, bridge: &Arc<T>) -> Option<Arc<T>> {
        let mut held = self.held.lock();
        // By address: a vtable pointer can differ between codegen units for
        // the same object.
        let same = held
            .as_ref()
            .is_some_and(|held| std::ptr::addr_eq(Arc::as_ptr(held), Arc::as_ptr(bridge)));
        if same { held.take() } else { None }
    }

    /// The bridge held now, cloned out of the lock so the caller runs no
    /// call under it.
    pub(crate) fn get(&self) -> Option<Arc<T>> {
        self.held.lock().clone()
    }
}

impl<T: ?Sized> std::fmt::Debug for BridgeSlot<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BridgeSlot")
            .field("held", &self.held.lock().is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::BridgeSlot;

    trait Bridge: Send + Sync {
        fn window(&self) -> u32;
    }

    struct Window(u32);

    impl Bridge for Window {
        fn window(&self) -> u32 {
            self.0
        }
    }

    /// A window that opened over the view before the previous one closed
    /// keeps its bridge when the previous one withdraws, and each window
    /// withdraws only once.
    #[test]
    fn a_closing_window_withdraws_only_its_own_bridge() {
        let slot = BridgeSlot::<dyn Bridge>::default();
        let first: Arc<dyn Bridge> = Arc::new(Window(1));
        // Equal by value, a different bridge.
        let twin: Arc<dyn Bridge> = Arc::new(Window(1));
        let second: Arc<dyn Bridge> = Arc::new(Window(2));

        assert!(slot.provide(Arc::clone(&first)).is_none());
        assert!(slot.withdraw(&twin).is_none(), "identity, not value");
        let replaced = slot.provide(Arc::clone(&second));
        assert!(replaced.is_some_and(|replaced| Arc::ptr_eq(&replaced, &first)));

        assert!(slot.withdraw(&first).is_none(), "the newer window's stays");
        assert_eq!(slot.get().map(|bridge| bridge.window()), Some(2));

        assert!(slot.withdraw(&second).is_some());
        assert!(slot.withdraw(&second).is_none(), "withdrawn once");
        assert!(slot.get().is_none());
    }
}
