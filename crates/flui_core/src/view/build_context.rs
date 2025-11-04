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

use crate::ElementId;
use crate::hooks::HookContext;
use crate::pipeline::ElementTree;
use parking_lot::RwLock;
use std::cell::RefCell;
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
    hook_context: Arc<RefCell<HookContext>>,
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
    pub fn new(
        tree: Arc<RwLock<ElementTree>>,
        element_id: ElementId,
    ) -> Self {
        Self {
            tree,
            element_id,
            hook_context: Arc::new(RefCell::new(HookContext::new())),
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
        hook_context: Arc<RefCell<HookContext>>,
    ) -> Self {
        Self {
            tree,
            element_id,
            hook_context,
        }
    }

    /// Get mutable access to the hook context
    ///
    /// This is the primary way for hooks to access their context.
    /// Uses interior mutability via RefCell.
    ///
    /// # Panics
    ///
    /// Panics if the hook context is already borrowed mutably. This typically
    /// happens when trying to call hooks from within hook callbacks:
    ///
    /// ```rust,ignore
    /// // ❌ WRONG: Nested hook calls will panic
    /// ctx.with_hook_context_mut(|_| {
    ///     let signal = use_signal(&ctx, 0);  // PANIC: Double borrow
    /// });
    ///
    /// // ✅ CORRECT: Call hooks sequentially at the same level
    /// let signal = use_signal(ctx, 0);
    /// let memo = use_memo(ctx, |hook_ctx| {
    ///     // Inside memo, use hook_ctx directly, not ctx
    ///     signal.get() * 2
    /// });
    /// ```
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
    pub fn with_hook_context_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut HookContext) -> R,
    {
        match self.hook_context.try_borrow_mut() {
            Ok(mut hook_ctx) => f(&mut hook_ctx),
            Err(_) => {
                panic!(
                    "BuildContext hook context is already borrowed!\n\
                    \n\
                    This typically occurs when:\n\
                    1. Calling hooks from within hook callbacks (nested hook calls)\n\
                    2. Holding a hook context borrow across a hook call\n\
                    \n\
                    Example of the problem:\n\
                    ```\n\
                    ctx.with_hook_context_mut(|_| {{\n\
                        let signal = use_signal(&ctx, 0);  // ← Double borrow!\n\
                    }});\n\
                    ```\n\
                    \n\
                    Solution: Call hooks sequentially at the component level, not nested:\n\
                    ```\n\
                    let signal = use_signal(ctx, 0);  // ← Correct\n\
                    let memo = use_memo(ctx, |hook_ctx| {{\n\
                        signal.get() * 2  // Use hook_ctx, not ctx\n\
                    }});\n\
                    ```\n\
                    \n\
                    For more details, see the hook documentation."
                )
            }
        }
    }

    /// Get the shared hook context
    ///
    /// Useful for creating child contexts that share the same hook state.
    pub fn hook_context(&self) -> Arc<RefCell<HookContext>> {
        Arc::clone(&self.hook_context)
    }

    /// Get the current element ID
    pub fn element_id(&self) -> ElementId {
        self.element_id
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
    /// The notification bubbles up from this element to the root,
    /// allowing ancestor NotificationListener widgets to intercept it.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_core::ScrollNotification;
    ///
    /// let notification = ScrollNotification::new(10.0, 100.0, 1000.0);
    /// context.dispatch_notification(&notification);
    /// ```
    pub fn dispatch_notification(&self, _notification: &dyn crate::foundation::DynNotification) {
        // FIXME: Implement notification bubbling - walk up tree calling handlers
        // This requires:
        // 1. Walk up the tree from current element to root
        // 2. For each ancestor, check if it's a NotificationListener
        // 3. If so, call its handler and check if it stops bubbling
        // 4. Continue until stopped or reach root

        // For now, this is a stub. Full implementation requires:
        // - Element enum to expose notification handling
        // - NotificationListener to be properly integrated
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

// Tests removed - need to be rewritten with View API
