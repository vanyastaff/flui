//! Rebuild Handle for scheduling element rebuilds from callbacks.
//!
//! This module provides a clonable, thread-safe handle for scheduling
//! element rebuilds from callbacks, event handlers, and async contexts.
//!
//! # Architecture
//!
//! In FLUI's four-tree architecture, `BuildContext` is a temporary reference
//! that exists only during the build phase. To schedule rebuilds from callbacks
//! (which outlive the build phase), we need a persistent, clonable handle.
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  Build Phase                                                    │
//! │  ┌───────────────────────────────────────────────────────────┐  │
//! │  │ BuildContext (temporary, has tree access)                 │  │
//! │  │   └── rebuild_handle() → RebuildHandle (clonable)         │  │
//! │  └───────────────────────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────────┘
//!
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  Callbacks / Event Handlers                                     │
//! │  ┌───────────────────────────────────────────────────────────┐  │
//! │  │ RebuildHandle (Clone + Send + Sync)                       │  │
//! │  │   └── schedule_rebuild() → marks element dirty            │  │
//! │  └───────────────────────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_pipeline::RebuildHandle;
//! use flui_foundation::ElementId;
//!
//! // During build phase, get a handle from BuildContext
//! fn build(&self, ctx: &impl BuildContext) -> impl View {
//!     let handle = ctx.rebuild_handle();
//!
//!     Button::new("Click me")
//!         .on_click(move || {
//!             // This closure can be called anytime, even after build phase
//!             handle.schedule_rebuild();
//!         })
//! }
//! ```

use flui_tree::Identifier;
use std::sync::Arc;

use crate::DirtySet;

/// A clonable, thread-safe handle for scheduling element rebuilds.
///
/// This handle can be captured in closures and callbacks to trigger
/// element rebuilds from any context (event handlers, async tasks, etc.).
///
/// # Type Parameters
///
/// - `I`: The identifier type (defaults to `ElementId`)
///
/// # Thread Safety
///
/// `RebuildHandle` is `Clone + Send + Sync`, making it safe to:
/// - Clone and move into closures
/// - Send across threads
/// - Share between multiple callbacks
///
/// # Example
///
/// ```rust
/// use flui_pipeline::{RebuildHandle, DirtySet};
/// use flui_foundation::ElementId;
/// use std::sync::Arc;
///
/// // Create a dirty set (typically owned by TreeCoordinator)
/// let dirty_set = Arc::new(DirtySet::<ElementId>::new());
///
/// // Create a rebuild handle for a specific element
/// let id = ElementId::new(42);
/// let handle = RebuildHandle::new(id, Arc::clone(&dirty_set));
///
/// // Clone and use in multiple callbacks
/// let handle1 = handle.clone();
/// let handle2 = handle.clone();
///
/// // Schedule rebuild from anywhere
/// handle1.schedule_rebuild();
///
/// // The element is now marked dirty
/// assert!(dirty_set.is_dirty(id));
/// ```
#[derive(Debug, Clone)]
pub struct RebuildHandle<I: Identifier = flui_foundation::ElementId> {
    /// The element ID this handle can rebuild
    element_id: I,

    /// Shared reference to the dirty set
    dirty_set: Arc<DirtySet<I>>,
}

impl<I: Identifier> RebuildHandle<I> {
    /// Creates a new rebuild handle.
    ///
    /// # Parameters
    ///
    /// - `element_id`: The ID of the element this handle can rebuild
    /// - `dirty_set`: Shared reference to the dirty tracking set
    #[inline]
    #[must_use]
    pub fn new(element_id: I, dirty_set: Arc<DirtySet<I>>) -> Self {
        Self {
            element_id,
            dirty_set,
        }
    }

    /// Schedules a rebuild for this element.
    ///
    /// This marks the element as dirty in the shared dirty set.
    /// The actual rebuild will happen during the next build phase.
    ///
    /// # Thread Safety
    ///
    /// This method is lock-free and can be called from any thread.
    #[inline]
    pub fn schedule_rebuild(&self) {
        self.dirty_set.mark(self.element_id);
    }

    /// Returns the element ID this handle controls.
    #[inline]
    #[must_use]
    pub const fn element_id(&self) -> I {
        self.element_id
    }

    /// Checks if this element is currently marked for rebuild.
    #[inline]
    #[must_use]
    pub fn is_scheduled(&self) -> bool {
        self.dirty_set.is_dirty(self.element_id)
    }

    /// Creates a callback closure that schedules a rebuild when called.
    ///
    /// This is a convenience method for creating event handlers.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_pipeline::{RebuildHandle, DirtySet};
    /// use flui_foundation::ElementId;
    /// use std::sync::Arc;
    ///
    /// let dirty_set = Arc::new(DirtySet::<ElementId>::new());
    /// let handle = RebuildHandle::new(ElementId::new(1), dirty_set);
    ///
    /// let callback = handle.as_callback();
    /// callback(); // Schedules rebuild
    /// ```
    #[inline]
    #[must_use]
    pub fn as_callback(&self) -> impl Fn() + Send + Sync + Clone + 'static
    where
        I: 'static,
    {
        let handle = self.clone();
        move || handle.schedule_rebuild()
    }

    /// Creates a boxed callback closure.
    ///
    /// Useful when you need a `Box<dyn Fn()>` instead of a generic closure.
    #[inline]
    #[must_use]
    pub fn as_boxed_callback(&self) -> Box<dyn Fn() + Send + Sync>
    where
        I: 'static,
    {
        let handle = self.clone();
        Box::new(move || handle.schedule_rebuild())
    }
}

// Ensure RebuildHandle is Send + Sync
static_assertions::assert_impl_all!(RebuildHandle<flui_foundation::ElementId>: Send, Sync, Clone);

#[cfg(test)]
mod tests {
    use super::*;
    use flui_foundation::ElementId;
    use std::thread;

    #[test]
    fn test_schedule_rebuild() {
        let dirty_set = Arc::new(DirtySet::new());
        let id = ElementId::new(42);
        let handle = RebuildHandle::new(id, Arc::clone(&dirty_set));

        assert!(!handle.is_scheduled());
        handle.schedule_rebuild();
        assert!(handle.is_scheduled());
        assert!(dirty_set.is_dirty(id));
    }

    #[test]
    fn test_clone_and_share() {
        let dirty_set = Arc::new(DirtySet::new());
        let id = ElementId::new(1);
        let handle1 = RebuildHandle::new(id, Arc::clone(&dirty_set));
        let handle2 = handle1.clone();

        handle1.schedule_rebuild();
        assert!(handle2.is_scheduled()); // Same dirty set
    }

    #[test]
    fn test_callback() {
        let dirty_set = Arc::new(DirtySet::new());
        let id = ElementId::new(1);
        let handle = RebuildHandle::new(id, Arc::clone(&dirty_set));

        let callback = handle.as_callback();
        assert!(!dirty_set.is_dirty(id));

        callback();
        assert!(dirty_set.is_dirty(id));
    }

    #[test]
    fn test_send_to_thread() {
        let dirty_set = Arc::new(DirtySet::new());
        let id = ElementId::new(1);
        let handle = RebuildHandle::new(id, Arc::clone(&dirty_set));

        let join_handle = thread::spawn(move || {
            handle.schedule_rebuild();
        });

        join_handle.join().unwrap();
        assert!(dirty_set.is_dirty(id));
    }

    #[test]
    fn test_multiple_handles_same_element() {
        let dirty_set = Arc::new(DirtySet::new());
        let id = ElementId::new(1);

        let handles: Vec<_> = (0..10)
            .map(|_| RebuildHandle::new(id, Arc::clone(&dirty_set)))
            .collect();

        // All handles schedule the same element
        for handle in &handles {
            handle.schedule_rebuild();
        }

        assert!(dirty_set.is_dirty(id));
        assert_eq!(dirty_set.len(), 1); // Only one element marked
    }
}
