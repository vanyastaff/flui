//! BuildContext - context for building widgets
//!
//! Provides access to the element tree, InheritedWidgets, and hooks during build phase.

use crate::ElementId;
use crate::hooks::HookContext;
use crate::pipeline::{ElementTree, PipelineOwner};
use parking_lot::RwLock;
use std::cell::RefCell;
use std::sync::Arc;

/// BuildContext - provides access to tree and pipeline during widget build
///
/// BuildContext is passed to `build()` methods and allows widgets to:
/// - Access InheritedWidgets from ancestors
/// - Register dependencies for automatic rebuilds
/// - Query tree structure
/// - Trigger state updates via `set_state()`
/// - Schedule rebuilds via the pipeline
///
/// # Example
///
/// ```rust,ignore
/// impl Component for MyView {
///     fn build(&self, context: &BuildContext) -> View {
///         // Access theme with dependency (auto-rebuild on change)
///         let theme = context.depend_on::<Theme>().unwrap();
///
///         // Use theme data
///         Box::new(Text::new(format!("Color: {:?}", theme.color)))
///     }
/// }
/// ```
#[derive(Clone)]
pub struct BuildContext {
    /// Shared reference to the element tree (with interior mutability for dependency tracking)
    tree: Arc<RwLock<ElementTree>>,

    /// Shared reference to the pipeline owner for scheduling rebuilds
    pipeline: Arc<RwLock<PipelineOwner>>,

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
    /// - `pipeline`: Shared reference to the pipeline owner
    /// - `element_id`: ID of the element being built
    pub fn new(
        tree: Arc<RwLock<ElementTree>>,
        pipeline: Arc<RwLock<PipelineOwner>>,
        element_id: ElementId,
    ) -> Self {
        Self {
            tree,
            pipeline,
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
    /// - `pipeline`: Shared reference to the pipeline owner
    /// - `element_id`: ID of the element being built
    /// - `hook_context`: Shared hook context
    pub fn with_hook_context(
        tree: Arc<RwLock<ElementTree>>,
        pipeline: Arc<RwLock<PipelineOwner>>,
        element_id: ElementId,
        hook_context: Arc<RefCell<HookContext>>,
    ) -> Self {
        Self {
            tree,
            pipeline,
            element_id,
            hook_context,
        }
    }

    /// Get mutable access to the hook context
    ///
    /// This is the primary way for hooks to access their context.
    /// Uses interior mutability via RefCell.
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
        f(&mut self.hook_context.borrow_mut())
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
    pub fn tree(&self) -> Arc<RwLock<ElementTree>> {
        Arc::clone(&self.tree)
    }

    /// Schedule a rebuild for this element
    ///
    /// This schedules the element for rebuild in the next frame.
    /// The element will be marked dirty and added to the build pipeline.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // In a button click handler
    /// ctx.schedule_rebuild();
    /// ```
    pub fn schedule_rebuild(&self) {
        let depth = self.depth();
        let mut pipeline = self.pipeline.write();
        pipeline.schedule_build_for(self.element_id, depth);
    }

    /// Schedule a rebuild for a specific element
    ///
    /// This allows scheduling rebuilds for child elements or other elements in the tree.
    ///
    /// # Parameters
    ///
    /// - `element_id`: ID of the element to rebuild
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Schedule rebuild for a child element
    /// ctx.schedule_rebuild_for(child_id);
    /// ```
    pub fn schedule_rebuild_for(&self, element_id: ElementId) {
        // Calculate depth for the target element
        let tree = self.tree.read();
        let mut depth = 0;
        let mut current = element_id;
        while let Some(parent) = tree.parent(current) {
            depth += 1;
            current = parent;
        }
        drop(tree);

        let mut pipeline = self.pipeline.write();
        pipeline.schedule_build_for(element_id, depth);
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
        if let Some(element) = tree.get(self.element_id)
            && let Some(render_element) = element.as_render()
        {
            let render_state = render_element.render_state();
            return render_state.read().size();
        }

        None
    }

}

// Tests removed - need to be rewritten with View API
