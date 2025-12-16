//! Composition callbacks for layer compositing events
//!
//! This module provides a system for registering callbacks that are invoked
//! when a layer subtree is composited. This is useful for observing the
//! final transform and clip state without affecting the compositing process.
//!
//! # Use Cases
//!
//! - Observing the total transform to determine visibility
//! - Tracking when layers are composited for debugging
//! - Implementing custom effects based on compositing state
//!
//! # Example
//!
//! ```rust
//! use flui_layer::layer::composition_callback::CompositionCallbackRegistry;
//! use std::sync::Arc;
//! use std::sync::atomic::{AtomicBool, Ordering};
//!
//! let registry = CompositionCallbackRegistry::new();
//! let was_called = Arc::new(AtomicBool::new(false));
//! let was_called_clone = was_called.clone();
//!
//! let handle = registry.add(move || {
//!     was_called_clone.store(true, Ordering::SeqCst);
//! });
//!
//! // Fire all callbacks
//! registry.fire();
//! assert!(was_called.load(Ordering::SeqCst));
//!
//! // Remove callback by dropping handle
//! drop(handle);
//! ```

use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// A unique identifier for a composition callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CompositionCallbackId(u64);

impl CompositionCallbackId {
    fn next() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Self(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

/// A handle that removes the callback when dropped.
///
/// This implements RAII-style cleanup - when the handle is dropped,
/// the associated callback is automatically unregistered.
pub struct CompositionCallbackHandle {
    id: CompositionCallbackId,
    registry: Arc<Mutex<CallbackStorage>>,
}

impl std::fmt::Debug for CompositionCallbackHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositionCallbackHandle")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl CompositionCallbackHandle {
    /// Returns the callback ID.
    #[inline]
    pub fn id(&self) -> CompositionCallbackId {
        self.id
    }

    /// Manually removes the callback.
    ///
    /// This is equivalent to dropping the handle.
    pub fn remove(self) {
        // Drop will handle removal
    }
}

impl Drop for CompositionCallbackHandle {
    fn drop(&mut self) {
        let mut storage = self.registry.lock();
        storage.remove(self.id);
    }
}

/// Internal storage for callbacks.
struct CallbackStorage {
    callbacks: Vec<(CompositionCallbackId, Box<dyn Fn() + Send + Sync>)>,
}

impl CallbackStorage {
    fn new() -> Self {
        Self {
            callbacks: Vec::new(),
        }
    }

    fn add<F>(&mut self, callback: F) -> CompositionCallbackId
    where
        F: Fn() + Send + Sync + 'static,
    {
        let id = CompositionCallbackId::next();
        self.callbacks.push((id, Box::new(callback)));
        id
    }

    fn remove(&mut self, id: CompositionCallbackId) {
        self.callbacks.retain(|(callback_id, _)| *callback_id != id);
    }

    fn fire(&self) {
        for (_, callback) in &self.callbacks {
            callback();
        }
    }

    fn len(&self) -> usize {
        self.callbacks.len()
    }

    fn is_empty(&self) -> bool {
        self.callbacks.is_empty()
    }

    fn clear(&mut self) {
        self.callbacks.clear();
    }
}

/// Registry for composition callbacks.
///
/// Callbacks are invoked when the layer tree is composited, allowing
/// observation of the final transform and clip state.
#[derive(Clone)]
pub struct CompositionCallbackRegistry {
    storage: Arc<Mutex<CallbackStorage>>,
}

impl CompositionCallbackRegistry {
    /// Creates a new empty registry.
    #[inline]
    pub fn new() -> Self {
        Self {
            storage: Arc::new(Mutex::new(CallbackStorage::new())),
        }
    }

    /// Adds a callback to be invoked during compositing.
    ///
    /// Returns a handle that removes the callback when dropped.
    pub fn add<F>(&self, callback: F) -> CompositionCallbackHandle
    where
        F: Fn() + Send + Sync + 'static,
    {
        let id = self.storage.lock().add(callback);
        CompositionCallbackHandle {
            id,
            registry: self.storage.clone(),
        }
    }

    /// Fires all registered callbacks.
    ///
    /// This should be called during the compositing phase.
    pub fn fire(&self) {
        self.storage.lock().fire();
    }

    /// Returns the number of registered callbacks.
    #[inline]
    pub fn len(&self) -> usize {
        self.storage.lock().len()
    }

    /// Returns true if there are no registered callbacks.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.storage.lock().is_empty()
    }

    /// Removes all callbacks.
    pub fn clear(&self) {
        self.storage.lock().clear();
    }

    /// Returns true if this registry has any callbacks.
    ///
    /// This is useful for optimization - if there are no callbacks,
    /// the compositing process can skip callback-related work.
    #[inline]
    pub fn has_callbacks(&self) -> bool {
        !self.is_empty()
    }
}

impl Default for CompositionCallbackRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for CompositionCallbackRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositionCallbackRegistry")
            .field("callback_count", &self.len())
            .finish()
    }
}

/// Trait for types that support composition callbacks.
pub trait HasCompositionCallbacks {
    /// Returns true if this or any descendant has composition callbacks.
    fn has_composition_callbacks(&self) -> bool;

    /// Fires composition callbacks for this layer.
    ///
    /// If `include_children` is true, also fires callbacks for all descendants.
    fn fire_composition_callbacks(&self, include_children: bool);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn test_callback_id_uniqueness() {
        let id1 = CompositionCallbackId::next();
        let id2 = CompositionCallbackId::next();
        let id3 = CompositionCallbackId::next();

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_registry_add_and_fire() {
        let registry = CompositionCallbackRegistry::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let _handle = registry.add(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        assert_eq!(registry.len(), 1);
        assert!(!registry.is_empty());

        registry.fire();
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        registry.fire();
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_registry_multiple_callbacks() {
        let registry = CompositionCallbackRegistry::new();
        let counter = Arc::new(AtomicUsize::new(0));

        let counter1 = counter.clone();
        let counter2 = counter.clone();
        let counter3 = counter.clone();

        let _h1 = registry.add(move || {
            counter1.fetch_add(1, Ordering::SeqCst);
        });
        let _h2 = registry.add(move || {
            counter2.fetch_add(10, Ordering::SeqCst);
        });
        let _h3 = registry.add(move || {
            counter3.fetch_add(100, Ordering::SeqCst);
        });

        assert_eq!(registry.len(), 3);

        registry.fire();
        assert_eq!(counter.load(Ordering::SeqCst), 111);
    }

    #[test]
    fn test_handle_drop_removes_callback() {
        let registry = CompositionCallbackRegistry::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let handle = registry.add(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        assert_eq!(registry.len(), 1);
        registry.fire();
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        drop(handle);
        assert_eq!(registry.len(), 0);

        registry.fire();
        assert_eq!(counter.load(Ordering::SeqCst), 1); // Should not increment
    }

    #[test]
    fn test_registry_clear() {
        let registry = CompositionCallbackRegistry::new();

        let _h1 = registry.add(|| {});
        let _h2 = registry.add(|| {});
        let _h3 = registry.add(|| {});

        assert_eq!(registry.len(), 3);

        registry.clear();
        assert!(registry.is_empty());
    }

    #[test]
    fn test_registry_clone_shares_state() {
        let registry1 = CompositionCallbackRegistry::new();
        let registry2 = registry1.clone();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let _handle = registry1.add(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        // Both registries see the same callback
        assert_eq!(registry1.len(), 1);
        assert_eq!(registry2.len(), 1);

        // Firing from either registry invokes the callback
        registry2.fire();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_empty_registry() {
        let registry = CompositionCallbackRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert!(!registry.has_callbacks());

        // Firing an empty registry should be fine
        registry.fire();
    }
}
