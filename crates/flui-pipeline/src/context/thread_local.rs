//! Thread-local BuildContext access
//!
//! Provides RAII-based thread-local storage for BuildContext,
//! allowing views to access context without explicit parameter passing.
//!
//! # Usage
//!
//! ```rust,ignore
//! // Framework sets up context
//! let ctx = PipelineBuildContext::new(...);
//! with_build_context(&ctx, || {
//!     // Inside this closure, views can access context
//!     let current = current_build_context();
//! });
//! ```

use std::cell::RefCell;
use std::marker::PhantomData;
use std::ptr::NonNull;

use flui_view::BuildContext;

// Thread-local storage for current build context
thread_local! {
    static CURRENT_CONTEXT: RefCell<Option<NonNull<dyn BuildContext>>> = const { RefCell::new(None) };
}

/// Get the current build context from thread-local storage.
///
/// # Panics
///
/// Panics if called outside of a `with_build_context` scope.
///
/// # Safety
///
/// The returned reference is only valid within the current `with_build_context` scope.
/// Do not store or leak this reference.
///
/// # Example
///
/// ```rust,ignore
/// with_build_context(&ctx, || {
///     let current = current_build_context();
///     println!("Building element: {:?}", current.element_id());
/// });
/// ```
pub fn current_build_context<'a>() -> &'a dyn BuildContext {
    CURRENT_CONTEXT.with(|cell| {
        let borrowed = cell.borrow();
        match *borrowed {
            Some(ptr) => {
                // SAFETY: The pointer is valid for the duration of with_build_context scope.
                // The caller must not store or leak this reference beyond that scope.
                unsafe { &*ptr.as_ptr() }
            }
            None => panic!(
                "current_build_context() called outside of with_build_context scope. \
                 This is a framework bug - views should only be built within a build context."
            ),
        }
    })
}

/// Try to get the current build context, returning None if not in a build scope.
///
/// Useful for optional context access without panicking.
pub fn try_current_build_context<'a>() -> Option<&'a dyn BuildContext> {
    CURRENT_CONTEXT.with(|cell| {
        let borrowed = cell.borrow();
        borrowed.map(|ptr| {
            // SAFETY: Same as current_build_context
            unsafe { &*ptr.as_ptr() }
        })
    })
}

/// Check if we're currently inside a build context scope.
pub fn has_build_context() -> bool {
    CURRENT_CONTEXT.with(|cell| cell.borrow().is_some())
}

/// Execute a closure with a build context set as the current context.
///
/// The context is available via `current_build_context()` for the duration
/// of the closure.
///
/// # Example
///
/// ```rust,ignore
/// let ctx = PipelineBuildContext::new(element_id, tree, dirty_set);
///
/// with_build_context(&ctx, || {
///     // Views built here can access context
///     let element = view.build(current_build_context());
/// });
/// ```
pub fn with_build_context<F, R>(context: &dyn BuildContext, f: F) -> R
where
    F: FnOnce() -> R,
{
    let _guard = BuildContextGuard::new(context);
    f()
}

/// RAII guard that sets/clears thread-local build context.
///
/// When created, sets the current context. When dropped, restores the previous context.
/// Supports nested contexts (stack-like behavior).
///
/// # Note
///
/// This guard is intentionally not Send/Sync - it's tied to the creating thread.
pub struct BuildContextGuard {
    /// Previous context to restore on drop
    previous: Option<NonNull<dyn BuildContext>>,
    /// Prevent Send/Sync
    _not_send_sync: PhantomData<*mut ()>,
}

impl BuildContextGuard {
    /// Create a new guard, setting the given context as current.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `context` outlives this guard.
    /// This is typically enforced by using `with_build_context` instead.
    pub fn new(context: &dyn BuildContext) -> Self {
        // Convert to raw pointer to erase lifetime
        let ptr = context as *const dyn BuildContext as *mut dyn BuildContext;
        let non_null = NonNull::new(ptr).expect("context pointer should not be null");

        let previous = CURRENT_CONTEXT.with(|cell| {
            let mut borrowed = cell.borrow_mut();
            let prev = *borrowed;
            *borrowed = Some(non_null);
            prev
        });

        Self {
            previous,
            _not_send_sync: PhantomData,
        }
    }
}

impl Drop for BuildContextGuard {
    fn drop(&mut self) {
        CURRENT_CONTEXT.with(|cell| {
            *cell.borrow_mut() = self.previous;
        });
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_foundation::ElementId;
    use std::any::Any;

    // Mock context for testing
    struct MockContext {
        id: ElementId,
    }

    impl BuildContext for MockContext {
        fn element_id(&self) -> ElementId {
            self.id
        }

        fn parent_id(&self) -> Option<ElementId> {
            None
        }

        fn depth(&self) -> usize {
            0
        }

        fn mark_dirty(&self) {}

        fn schedule_rebuild(&self, _element_id: ElementId) {}

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    #[test]
    fn test_with_build_context() {
        let ctx = MockContext {
            id: ElementId::new(42),
        };

        assert!(!has_build_context());

        with_build_context(&ctx, || {
            assert!(has_build_context());
            let current = current_build_context();
            assert_eq!(current.element_id(), ElementId::new(42));
        });

        assert!(!has_build_context());
    }

    #[test]
    fn test_nested_contexts() {
        let ctx1 = MockContext {
            id: ElementId::new(1),
        };
        let ctx2 = MockContext {
            id: ElementId::new(2),
        };

        with_build_context(&ctx1, || {
            assert_eq!(current_build_context().element_id(), ElementId::new(1));

            with_build_context(&ctx2, || {
                assert_eq!(current_build_context().element_id(), ElementId::new(2));
            });

            // Restored to ctx1
            assert_eq!(current_build_context().element_id(), ElementId::new(1));
        });
    }

    #[test]
    fn test_try_current_build_context() {
        assert!(try_current_build_context().is_none());

        let ctx = MockContext {
            id: ElementId::new(42),
        };

        with_build_context(&ctx, || {
            assert!(try_current_build_context().is_some());
        });

        assert!(try_current_build_context().is_none());
    }

    #[test]
    #[should_panic(expected = "called outside of with_build_context scope")]
    fn test_panic_outside_context() {
        let _ = current_build_context();
    }
}
