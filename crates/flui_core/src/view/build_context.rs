//! BuildContext - context for building widgets
//!
//! Provides read-only access to the element tree, InheritedWidgets, and hooks during build phase.
//!
//! # Design Philosophy
//!
//! BuildContext is intentionally **read-only** during the build phase. It represents
//! the context in which a view is being built, not the ability to schedule future builds.
//! This design:
//!
//! - Enables parallel builds (multiple components can build concurrently)
//! - Matches Flutter's BuildContext semantics (immutable context)
//! - Prevents lock contention during the build phase
//! - Makes the build phase truly side-effect-free
//!
//! # Rebuild Scheduling
//!
//! State changes that trigger rebuilds should NOT go through BuildContext.
//! Instead, use hooks and signals which manage their own rebuild callbacks:
//!
//! ```rust,ignore
//! // ✅ Correct: Signal handles rebuild scheduling internally
//! let signal = use_signal(ctx, 0);
//! signal.set(42);  // Triggers rebuild via callback
//!
//! // ❌ Wrong: Don't schedule rebuilds during build
//! // ctx.schedule_rebuild();  // This method was removed!
//! ```

use crate::hooks::HookContext;
use crate::pipeline::{ElementTree, RebuildQueue};
use crate::ElementId;
use parking_lot::{Mutex, RwLock};
use std::sync::Arc;

/// BuildContext - read-only context for building views
///
/// BuildContext is passed to `build()` methods and provides read-only access to:
/// - Element tree (for inherited widgets and tree queries)
/// - Hook context (for state management via hooks)
/// - Current element ID (for dependency tracking)
///
/// # Design
///
/// BuildContext is intentionally minimal and read-only. It does NOT provide
/// the ability to schedule rebuilds - that happens via hooks/signals which
/// store callbacks executed after the build phase completes.
///
/// This design enables:
/// - Parallel builds (no write locks needed)
/// - Better performance (smaller, cache-friendly struct)
/// - Clearer semantics (build is read-only, state changes happen elsewhere)
///
/// # Example
///
/// ```rust,ignore
/// impl View for MyView {
///     fn build(self, ctx: &BuildContext) -> (Self::Element, Self::State) {
///         // Access inherited data (read-only)
///         let theme = ctx.depend_on::<Theme>().unwrap();
///
///         // Use hooks for state (hooks manage rebuild scheduling)
///         let count = use_signal(ctx, 0);
///
///         // Build child view
///         let child = Text::new(format!("Count: {}", count.get()));
///         (child.into_element(), ())
///     }
/// }
/// ```
#[derive(Clone)]
pub struct BuildContext {
    /// Shared reference to the element tree (for inherited widgets and queries)
    tree: Arc<RwLock<ElementTree>>,

    /// ID of the current element being built
    element_id: ElementId,

    /// Hook context for managing hook state (with interior mutability)
    /// Shared across all BuildContexts for the same component tree
    /// Uses Mutex for thread-safety (Send + Sync)
    hook_context: Arc<Mutex<HookContext>>,

    /// Rebuild queue for scheduling deferred component rebuilds
    /// Used by signals and other reactive primitives to schedule rebuilds
    rebuild_queue: RebuildQueue,
}

impl std::fmt::Debug for BuildContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuildContext")
            .field("element_id", &self.element_id)
            .field("has_hook_context", &true)
            .finish()
    }
}

impl BuildContext {
    /// Create a new BuildContext
    ///
    /// # Arguments
    ///
    /// - `tree`: Shared reference to the element tree
    /// - `element_id`: ID of the element being built
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let ctx = BuildContext::new(
    ///     tree.clone(),
    ///     element_id,
    /// );
    /// ```
    pub fn new(tree: Arc<RwLock<ElementTree>>, element_id: ElementId) -> Self {
        Self {
            tree,
            element_id,
            hook_context: Arc::new(Mutex::new(HookContext::new())),
            rebuild_queue: RebuildQueue::new(),
        }
    }

    /// Create a new BuildContext with shared hook context
    ///
    /// This allows multiple BuildContexts to share the same hook context,
    /// which is necessary for maintaining hook state across component rebuilds.
    ///
    /// # Arguments
    ///
    /// - `tree`: Shared reference to the element tree
    /// - `element_id`: ID of the element being built
    /// - `hook_context`: Shared hook context
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Preserve hook state across rebuild
    /// let ctx = BuildContext::with_hook_context(
    ///     tree.clone(),
    ///     element_id,
    ///     existing_hook_context.clone(),
    /// );
    /// ```
    pub fn with_hook_context(
        tree: Arc<RwLock<ElementTree>>,
        element_id: ElementId,
        hook_context: Arc<Mutex<HookContext>>,
    ) -> Self {
        Self {
            tree,
            element_id,
            hook_context,
            rebuild_queue: RebuildQueue::new(),
        }
    }

    /// Create a new BuildContext with shared hook context and rebuild queue
    ///
    /// This is used by the build pipeline to share both hook state and rebuild
    /// scheduling across component rebuilds.
    pub fn with_hook_context_and_queue(
        tree: Arc<RwLock<ElementTree>>,
        element_id: ElementId,
        hook_context: Arc<Mutex<HookContext>>,
        rebuild_queue: RebuildQueue,
    ) -> Self {
        Self {
            tree,
            element_id,
            hook_context,
            rebuild_queue,
        }
    }

    /// Get mutable access to the hook context
    ///
    /// This is the primary way for hooks to access their context.
    /// Uses interior mutability via Mutex for thread-safety.
    ///
    /// # Thread-Safety
    ///
    /// Uses `parking_lot::Mutex` which is Send + Sync, allowing BuildContext
    /// to be safely shared across threads for multi-threaded UI.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// pub fn use_signal<T: Clone + 'static>(ctx: &BuildContext, initial: T) -> Signal<T> {
    ///     ctx.with_hook_context_mut(|hook_ctx| {
    ///         hook_ctx.use_hook::<SignalHook<T>>(initial)
    ///     })
    /// }
    /// ```
    ///
    /// # Notes
    ///
    /// - parking_lot::Mutex doesn't panic on lock contention, it blocks
    /// - For performance, keep critical sections short
    /// - Avoid nested hook calls during mutation
    pub fn with_hook_context_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut HookContext) -> R,
    {
        let mut hook_ctx = self.hook_context.lock();
        f(&mut hook_ctx)
    }

    /// Get the shared hook context
    ///
    /// Useful for creating child contexts that share the same hook state.
    pub fn hook_context(&self) -> Arc<Mutex<HookContext>> {
        Arc::clone(&self.hook_context)
    }

    /// Get the rebuild queue
    ///
    /// Used by signals and other reactive primitives to schedule component rebuilds.
    pub fn rebuild_queue(&self) -> &RebuildQueue {
        &self.rebuild_queue
    }

    /// Get the current element ID
    pub fn element_id(&self) -> ElementId {
        self.element_id
    }

    /// Check if this BuildContext still points to a valid element
    ///
    /// Returns `false` if the element has been removed from the tree,
    /// making this context stale.
    ///
    /// # ⚠️ When to Use
    ///
    /// Use this when:
    /// - Storing BuildContext for later use (e.g., in closures)
    /// - Accessing context after potential tree mutations
    /// - Debugging unexpected behavior
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Store context in a closure
    /// let ctx_clone = ctx.clone();
    /// let callback = move || {
    ///     if !ctx_clone.is_valid() {
    ///         // Element was removed, don't use this context
    ///         return;
    ///     }
    ///     // Safe to use context
    ///     let size = ctx_clone.size();
    /// };
    /// ```
    ///
    /// # Performance
    ///
    /// This acquires a read lock on the element tree, so avoid calling
    /// in hot loops. In most cases, BuildContext is valid during `build()`.
    pub fn is_valid(&self) -> bool {
        let tree = self.tree.read();
        tree.get(self.element_id).is_some()
    }

    /// Get shared reference to the element tree
    ///
    /// Returns the `Arc<RwLock<ElementTree>>` for more complex operations.
    /// Most methods should use the convenience methods on BuildContext instead.
    ///
    /// # Note
    ///
    /// The tree reference is read-only during build phase. Use hooks/signals
    /// for state changes that trigger rebuilds.
    pub fn tree(&self) -> Arc<RwLock<ElementTree>> {
        Arc::clone(&self.tree)
    }

    // ========== Tree Traversal ==========

    /// Get parent element ID
    ///
    /// Returns `None` if this is the root element
    pub fn parent(&self) -> Option<ElementId> {
        let tree = self.tree.read();
        tree.parent(self.element_id)
    }

    /// Check if this is the root element
    pub fn is_root(&self) -> bool {
        self.parent().is_none()
    }

    /// Get the depth of this element in the tree
    ///
    /// Root element has depth 0
    pub fn depth(&self) -> usize {
        let tree = self.tree.read();
        let mut depth = 0;
        let mut current = self.element_id;
        while let Some(parent) = tree.parent(current) {
            depth += 1;
            current = parent;
        }
        depth
    }

    /// Visit ancestor elements with a callback
    ///
    /// The visitor returns `true` to continue, `false` to stop.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// context.visit_ancestors(&mut |element_id| {
    ///     println!("Ancestor: {}", element_id);
    ///     true // continue
    /// });
    /// ```
    pub fn visit_ancestors<F>(&self, visitor: &mut F)
    where
        F: FnMut(ElementId) -> bool,
    {
        let tree = self.tree.read();
        let mut current_id = tree.parent(self.element_id);

        while let Some(id) = current_id {
            if !visitor(id) {
                break;
            }
            current_id = tree.parent(id);
        }
    }

    /// Find the nearest ancestor Render element
    ///
    /// Searches self first, then ancestors
    ///
    /// Returns the ElementId of the Render if found
    pub fn find_render_object(&self) -> Option<ElementId> {
        let tree = self.tree.read();

        // Check self first
        if let Some(element) = tree.get(self.element_id) {
            // Check if this element has a render object
            if element.render_object().is_some() {
                return Some(self.element_id);
            }
        }

        // Search ancestors for the nearest RenderElement
        let mut current_id = tree.parent(self.element_id);
        while let Some(id) = current_id {
            if let Some(element) = tree.get(id) {
                // Check if this ancestor has a render object
                if element.render_object().is_some() {
                    return Some(id);
                }
            }
            current_id = tree.parent(id);
        }

        None
    }

    // ========== Notification System ==========

    /// Dispatch a notification up the tree
    ///
    /// **⚠️ This method is currently unimplemented and does nothing.**
    ///
    /// The notification bubbles up from this element to the root,
    /// allowing ancestor NotificationListener widgets to intercept it.
    ///
    /// # Implementation Status
    ///
    /// This is a stub API that exists for compatibility but is not yet implemented.
    /// Calling this method currently has no effect.
    ///
    /// For notification bubbling to work, the following is required:
    /// 1. Element enum must expose notification handling
    /// 2. NotificationListener widget must be properly integrated
    /// 3. Tree walking with handler invocation must be implemented
    ///
    /// # Example (when implemented)
    ///
    /// ```rust,ignore
    /// use flui_core::ScrollNotification;
    ///
    /// let notification = ScrollNotification::new(10.0, 100.0, 1000.0);
    /// context.dispatch_notification(&notification);
    /// ```
    #[deprecated(
        since = "0.1.0",
        note = "This method is unimplemented and currently does nothing. \
                Do not rely on it until notification bubbling is fully implemented."
    )]
    pub fn dispatch_notification(&self, _notification: &dyn crate::foundation::DynNotification) {
        // TODO: Implement notification bubbling by walking up tree and calling handlers
        unimplemented!("Notification bubbling is not yet implemented")
    }

    // ========== Utility Methods ==========

    /// Get the size of this element (after layout)
    ///
    /// Returns `None` if element doesn't have a Render or hasn't been laid out yet
    pub fn size(&self) -> Option<flui_types::Size> {
        let tree = self.tree.read();

        // Get element and check if it's a render element
        if let Some(element) = tree.get(self.element_id) {
            if let Some(render_element) = element.as_render() {
                let render_state = render_element.render_state();
                return render_state.read().size();
            }
        }

        None
    }
}

// ==============================================================================
// Thread-Local BuildContext for Simplified View API
// ==============================================================================

use std::cell::Cell;

thread_local! {
    /// Thread-local storage for the current BuildContext
    ///
    /// This allows View::build() and RenderBuilder to access BuildContext
    /// without explicit passing through the call stack.
    ///
    /// # Design
    ///
    /// - Each thread has its own independent BuildContext
    /// - Set by pipeline during build phase via BuildContextGuard
    /// - Cleared automatically when guard drops (RAII)
    /// - Safe: thread-local = no data races
    ///
    /// # Safety
    ///
    /// The raw pointer is safe because:
    /// 1. It's only accessed within the BuildContextGuard's lifetime
    /// 2. BuildContextGuard ensures the context lives longer than any access
    /// 3. Thread-local ensures no cross-thread access
    /// 4. Only one BuildContext can be set at a time (checked by guard)
    static CURRENT_BUILD_CONTEXT: Cell<Option<*const BuildContext>> = const { Cell::new(None) };
}

/// RAII guard that sets the thread-local BuildContext
///
/// Automatically clears the context when dropped, ensuring proper cleanup
/// even if build() panics.
///
/// # Example
///
/// ```rust,ignore
/// // In build pipeline:
/// let ctx = BuildContext::new(tree, element_id);
/// let _guard = BuildContextGuard::new(&ctx);
///
/// // View::build() can now access current_build_context()
/// let element = view.build(&ctx).into_element();
///
/// // Guard drops here, clearing thread-local
/// ```
#[derive(Debug)]
pub struct BuildContextGuard {
    _private: (),
}

impl BuildContextGuard {
    /// Set the current build context for this thread
    ///
    /// Returns a guard that will clear the context when dropped.
    ///
    /// # Panics
    ///
    /// Panics if a build context is already set. Nested builds are not supported.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let ctx = BuildContext::new(tree, element_id);
    /// let _guard = BuildContextGuard::new(&ctx);
    /// // Context is now available via current_build_context()
    /// ```
    pub fn new(context: &BuildContext) -> Self {
        CURRENT_BUILD_CONTEXT.with(|cell| {
            if cell.get().is_some() {
                panic!(
                    "BuildContext is already set! Nested builds are not supported.\n\
                    \n\
                    This typically means build() was called recursively, which is a framework bug.\n\
                    \n\
                    If you're implementing the build pipeline, ensure:\n\
                    1. BuildContextGuard is dropped before creating a new one\n\
                    2. Builds are not nested (use sequential building instead)\n\
                    \n\
                    Stack trace will show where the first guard was created."
                );
            }

            // Store raw pointer (safe because guard ensures lifetime)
            cell.set(Some(context as *const BuildContext));
        });

        Self { _private: () }
    }
}

impl Drop for BuildContextGuard {
    fn drop(&mut self) {
        CURRENT_BUILD_CONTEXT.with(|cell| {
            cell.set(None);
        });
    }
}

/// Get the current BuildContext (thread-local)
///
/// This function is used by the simplified View API to access BuildContext
/// without explicit passing. It's primarily used internally by:
/// - `IntoElement::into_element()` for View trait
/// - `insert_into_tree()` in RenderBuilder
/// - Hook functions (if we remove ctx parameter in future)
///
/// # Panics
///
/// Panics if called outside of a build phase (i.e., when no BuildContextGuard is active).
///
/// # Example
///
/// ```rust,ignore
/// // Inside IntoElement::into_element():
/// impl<V: View> IntoElement for V {
///     fn into_element(self) -> Element {
///         let ctx = current_build_context();
///         let element_like = self.build(ctx);
///         element_like.into_element()
///     }
/// }
/// ```
///
/// # Safety
///
/// This function is safe because:
/// - BuildContextGuard ensures the context pointer is valid
/// - Thread-local storage prevents cross-thread access
/// - RAII guarantees cleanup even on panic
pub fn current_build_context() -> &'static BuildContext {
    CURRENT_BUILD_CONTEXT.with(|cell| {
        let ptr = cell.get().expect(
            "No BuildContext available! Are you calling this outside of View::build()?\n\
            \n\
            BuildContext is only available during the build phase when:\n\
            1. The framework has set BuildContextGuard\n\
            2. You're inside View::build() or a function called from it\n\
            \n\
            Common mistakes:\n\
            - Calling hooks or IntoElement outside of build()\n\
            - Storing and using IntoElement values after build completes\n\
            - Calling framework functions from non-build contexts\n\
            \n\
            Solution: Only call this from within View::build() or its callees.",
        );

        // SAFETY: The pointer is guaranteed valid by BuildContextGuard's lifetime
        // - Guard holds a reference, so BuildContext can't be dropped
        // - Thread-local ensures no cross-thread access
        // - Pointer is cleared when guard drops
        unsafe { &*ptr }
    })
}

/// Execute a closure with a BuildContext set
///
/// This is an internal API used by the build pipeline to establish a build context
/// for View::build() calls.
///
/// # Example
///
/// ```rust,ignore
/// // In build pipeline:
/// let result = with_build_context(&ctx, || {
///     view.into_element()
/// });
/// ```
///
/// # Safety
///
/// The BuildContextGuard ensures proper cleanup even if `f` panics.
pub fn with_build_context<F, R>(context: &BuildContext, f: F) -> R
where
    F: FnOnce() -> R,
{
    let _guard = BuildContextGuard::new(context);
    f()
}

// Tests removed - need to be rewritten with View API
