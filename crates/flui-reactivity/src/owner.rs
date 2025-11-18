//! Owner-based lifecycle management for reactive effects.
//!
//! Inspired by leptos reactive_graph, this module provides hierarchical
//! cleanup and scoped effect management.

use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Unique identifier for an owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OwnerId(u64);

impl OwnerId {
    /// Create a new owner ID.
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for OwnerId {
    fn default() -> Self {
        Self::new()
    }
}

/// Cleanup callback for owned resources.
pub type CleanupFn = Box<dyn FnOnce() + Send>;

/// Owner manages the lifecycle of effects and provides hierarchical cleanup.
///
/// This enables automatic cleanup of effects when they go out of scope,
/// preventing memory leaks and ensuring proper resource management.
///
/// # Example
///
/// ```rust,ignore
/// use flui_reactivity::Owner;
///
/// let owner = Owner::new();
///
/// owner.with(|| {
///     // Effects created here are automatically cleaned up
///     // when owner is dropped
///     let signal = Signal::new(0);
///     signal.subscribe(|| println!("Changed!"));
/// });
///
/// // When owner drops, all effects are cleaned up
/// ```
#[derive(Debug, Clone)]
pub struct Owner {
    inner: Arc<OwnerInner>,
}

struct OwnerInner {
    id: OwnerId,
    cleanups: Mutex<Vec<CleanupFn>>,
    children: Mutex<Vec<Owner>>,
    parent: Mutex<Option<Owner>>,
    disposed: AtomicBool,
}

impl std::fmt::Debug for OwnerInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnerInner")
            .field("id", &self.id)
            .field("cleanups_count", &self.cleanups.lock().len())
            .field("children_count", &self.children.lock().len())
            .field("has_parent", &self.parent.lock().is_some())
            .field("disposed", &self.disposed.load(Ordering::Relaxed))
            .finish()
    }
}

impl Owner {
    /// Create a new root owner with no parent.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(OwnerInner {
                id: OwnerId::new(),
                cleanups: Mutex::new(Vec::new()),
                children: Mutex::new(Vec::new()),
                parent: Mutex::new(None),
                disposed: AtomicBool::new(false),
            }),
        }
    }

    /// Get the owner's ID.
    pub fn id(&self) -> OwnerId {
        self.inner.id
    }

    /// Create a child owner.
    ///
    /// The child will be automatically cleaned up when the parent is cleaned up.
    pub fn child(&self) -> Self {
        let child = Self {
            inner: Arc::new(OwnerInner {
                id: OwnerId::new(),
                cleanups: Mutex::new(Vec::new()),
                children: Mutex::new(Vec::new()),
                parent: Mutex::new(Some(self.clone())),
                disposed: AtomicBool::new(false),
            }),
        };

        self.inner.children.lock().push(child.clone());
        child
    }

    /// Register a cleanup function to be called when this owner is disposed.
    ///
    /// Cleanup functions are called in reverse order of registration (LIFO).
    pub fn on_cleanup<F>(&self, cleanup: F)
    where
        F: FnOnce() + Send + 'static,
    {
        self.inner.cleanups.lock().push(Box::new(cleanup));
    }

    /// Run a function with this owner as the current owner.
    ///
    /// Any effects created during the function will be owned by this owner.
    pub fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        CURRENT_OWNER.with(|current| {
            let prev = current.borrow().clone();
            *current.borrow_mut() = Some(self.clone());

            let result = f();

            *current.borrow_mut() = prev;
            result
        })
    }

    /// Get the current owner from thread-local storage.
    pub fn current() -> Option<Self> {
        CURRENT_OWNER.with(|current| current.borrow().clone())
    }

    /// Dispose this owner and all its children, running all cleanup functions.
    ///
    /// Cleanup functions are run in reverse order (LIFO).
    /// Children are cleaned up before the parent.
    pub fn cleanup(&self) {
        // Atomically check and set disposed flag to prevent double cleanup
        if self
            .inner
            .disposed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            // Already cleaned up
            return;
        }

        // Clean up children first
        let children = std::mem::take(&mut *self.inner.children.lock());
        for child in children {
            child.cleanup();
        }

        // Run cleanup functions in reverse order
        let cleanups = std::mem::take(&mut *self.inner.cleanups.lock());
        for cleanup in cleanups.into_iter().rev() {
            cleanup();
        }
    }

    /// Get the parent owner, if any.
    pub fn parent(&self) -> Option<Self> {
        self.inner.parent.lock().clone()
    }

    /// Check if this owner has been disposed.
    pub fn is_disposed(&self) -> bool {
        self.inner.disposed.load(Ordering::Acquire)
    }
}

impl Default for Owner {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for OwnerInner {
    fn drop(&mut self) {
        // Atomically check and set disposed flag to prevent double cleanup
        // Use compare_exchange to avoid race condition where two threads
        // both see disposed=false and both run cleanup
        if self
            .disposed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            // Already cleaned up via manual cleanup() call
            return;
        }

        // Auto-cleanup on drop
        let cleanups = std::mem::take(self.cleanups.get_mut());
        for cleanup in cleanups.into_iter().rev() {
            cleanup();
        }

        let children = std::mem::take(self.children.get_mut());
        for child in children {
            child.cleanup();
        }
    }
}

thread_local! {
    static CURRENT_OWNER: std::cell::RefCell<Option<Owner>> = const { std::cell::RefCell::new(None) };
}

/// Set the current owner for the duration of a function.
///
/// This is a lower-level API; prefer using `Owner::with()`.
pub fn with_owner<F, R>(owner: &Owner, f: F) -> R
where
    F: FnOnce() -> R,
{
    owner.with(f)
}

/// Create a new root owner and run a function with it.
///
/// The owner will be automatically cleaned up when the function returns.
pub fn create_root<F, R>(f: F) -> R
where
    F: FnOnce(Owner) -> R,
{
    let owner = Owner::new();
    let result = owner.with(|| f(owner.clone()));
    owner.cleanup();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_owner_creation() {
        let owner = Owner::new();
        assert!(!owner.is_disposed());
    }

    #[test]
    fn test_cleanup_order() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let counter = Arc::new(AtomicU32::new(0));

        let owner = Owner::new();

        // Register cleanups
        let c1 = counter.clone();
        owner.on_cleanup(move || {
            assert_eq!(c1.fetch_add(1, Ordering::SeqCst), 1); // Should run second
        });

        let c2 = counter.clone();
        owner.on_cleanup(move || {
            assert_eq!(c2.fetch_add(1, Ordering::SeqCst), 0); // Should run first (LIFO)
        });

        owner.cleanup();
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_hierarchical_cleanup() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let parent_cleaned = Arc::new(AtomicBool::new(false));
        let child_cleaned = Arc::new(AtomicBool::new(false));

        let parent = Owner::new();
        let child = parent.child();

        let pc = parent_cleaned.clone();
        parent.on_cleanup(move || {
            pc.store(true, Ordering::SeqCst);
        });

        let cc = child_cleaned.clone();
        child.on_cleanup(move || {
            cc.store(true, Ordering::SeqCst);
        });

        parent.cleanup();

        assert!(parent_cleaned.load(Ordering::SeqCst));
        assert!(child_cleaned.load(Ordering::SeqCst));
    }

    #[test]
    fn test_current_owner() {
        let owner = Owner::new();

        assert!(Owner::current().is_none());

        owner.with(|| {
            assert!(Owner::current().is_some());
            assert_eq!(Owner::current().unwrap().id(), owner.id());
        });

        assert!(Owner::current().is_none());
    }

    #[test]
    fn test_nested_owners() {
        let outer = Owner::new();
        let inner = Owner::new();

        outer.with(|| {
            assert_eq!(Owner::current().unwrap().id(), outer.id());

            inner.with(|| {
                assert_eq!(Owner::current().unwrap().id(), inner.id());
            });

            assert_eq!(Owner::current().unwrap().id(), outer.id());
        });
    }

    #[test]
    fn test_create_root() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let cleaned = Arc::new(AtomicBool::new(false));
        let c = cleaned.clone();

        create_root(|owner| {
            owner.on_cleanup(move || {
                c.store(true, Ordering::SeqCst);
            });
        });

        // Should have cleaned up automatically
        assert!(cleaned.load(Ordering::SeqCst));
    }
}
