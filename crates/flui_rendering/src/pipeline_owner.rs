//! PipelineOwner - Manages the rendering pipeline for RenderObjects.
//!
//! This module implements Flutter's PipelineOwner pattern, managing dirty tracking
//! and flush operations for the render tree.
//!
//! # Flutter Reference
//!
//! **Source:** `flutter/packages/flutter/lib/src/rendering/object.dart`
//! **Lines:** 1019-1718 (Flutter 3.24)
//!
//! This is equivalent to Flutter's `PipelineOwner` class. It manages:
//! - Dirty tracking for layout/paint/compositing/semantics
//! - Flush operations that process dirty nodes in correct order
//! - Root render object management
//! - Child PipelineOwner hierarchy
//!
//! # Architecture
//!
//! ```text
//! PipelineOwner
//!   ├── render_tree: RenderTree           (storage for RenderObjects)
//!   ├── root_node: Option<RenderId>       (root render object)
//!   ├── _nodes_needing_layout: Vec        (dirty layout nodes)
//!   ├── _nodes_needing_paint: Vec         (dirty paint nodes)
//!   ├── _nodes_needing_compositing_bits_update: Vec
//!   ├── _nodes_needing_semantics: HashSet
//!   ├── _children: HashSet<PipelineOwner> (child owners for multi-view)
//!   └── callbacks (onNeedVisualUpdate, etc.)
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_rendering::PipelineOwner;
//!
//! let mut owner = PipelineOwner::new();
//!
//! // Insert render object
//! let id = owner.insert(my_render_node);
//! owner.set_root_node(Some(id));
//!
//! // Mark dirty
//! owner.mark_needs_layout(id);
//!
//! // Flush phases (in order!)
//! owner.flush_layout();
//! owner.flush_compositing_bits();
//! owner.flush_paint();
//! owner.flush_semantics();
//! ```

use std::collections::HashSet;

use flui_foundation::RenderId;

use crate::render_tree::{RenderNode, RenderTree};

// Re-export semantics types from flui-semantics
pub use flui_semantics::SemanticsOwner;
pub use flui_semantics::SemanticsUpdateCallback;

// ============================================================================
// CALLBACKS (Flutter pattern)
// ============================================================================

/// Callback for visual update requests.
///
/// Called when a render object wishes to update its visual appearance.
/// Typical implementations schedule a frame.
pub type OnNeedVisualUpdate = Box<dyn Fn() + Send + Sync>;

/// Callback for semantics owner creation.
pub type OnSemanticsOwnerCreated = Box<dyn Fn() + Send + Sync>;

/// Callback for semantics owner disposal.
pub type OnSemanticsOwnerDisposed = Box<dyn Fn() + Send + Sync>;

/// Re-export SemanticsUpdateCallback for convenience.
/// This is `Arc<dyn Fn(&[SemanticsUpdate]) + Send + Sync>` from flui-semantics.
pub use flui_semantics::owner::SemanticsUpdate;

// ============================================================================
// PIPELINE MANIFOLD (Flutter pattern)
// ============================================================================

/// Trait for pipeline manifold - coordinates multiple PipelineOwners.
///
/// In Flutter, PipelineManifold is the binding point that connects
/// PipelineOwners to the engine. It manages:
/// - Visual update requests
/// - Semantics enabled state
/// - Multiple views (multi-window support)
///
/// # Flutter Equivalence
///
/// ```dart
/// abstract class PipelineManifold implements Listenable {
///   bool get semanticsEnabled;
///   void requestVisualUpdate();
/// }
/// ```
pub trait PipelineManifold: Send + Sync {
    /// Whether semantics are enabled.
    fn semantics_enabled(&self) -> bool;

    /// Requests a visual update (schedules a frame).
    fn request_visual_update(&self);

    /// Adds a listener for semantics enabled changes.
    fn add_listener(&self, listener: Box<dyn Fn() + Send + Sync>);

    /// Removes a listener.
    fn remove_listener(&self, listener: &dyn Fn());
}

/// Unique identifier for a PipelineOwner.
///
/// Used for tracking child owners and parent-child relationships.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PipelineOwnerId(u64);

impl PipelineOwnerId {
    /// Creates a new unique ID.
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for PipelineOwnerId {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// PIPELINE OWNER BUILDER
// ============================================================================

/// Builder for creating a configured PipelineOwner.
///
/// Provides a fluent API for setting up callbacks and initial capacity.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::PipelineOwner;
/// use std::sync::Arc;
///
/// let owner = PipelineOwner::builder()
///     .capacity(1000)
///     .on_need_visual_update(|| {
///         // Schedule a frame
///     })
///     .on_semantics_owner_created(|| {
///         // Semantics tree created
///     })
///     .build();
/// ```
#[derive(Default)]
pub struct PipelineOwnerBuilder {
    capacity: Option<usize>,
    on_need_visual_update: Option<OnNeedVisualUpdate>,
    on_semantics_owner_created: Option<OnSemanticsOwnerCreated>,
    on_semantics_owner_disposed: Option<OnSemanticsOwnerDisposed>,
    on_semantics_update: Option<SemanticsUpdateCallback>,
}

impl PipelineOwnerBuilder {
    /// Creates a new builder with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the initial capacity for render nodes.
    ///
    /// Pre-allocates space for the specified number of render objects,
    /// reducing reallocations during tree construction.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let owner = PipelineOwner::builder()
    ///     .capacity(500)  // Pre-allocate for 500 render objects
    ///     .build();
    /// ```
    pub fn capacity(mut self, capacity: usize) -> Self {
        self.capacity = Some(capacity);
        self
    }

    /// Sets the callback for visual update requests.
    ///
    /// Called when a render object wishes to update its visual appearance.
    /// Typical implementations schedule a frame.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let owner = PipelineOwner::builder()
    ///     .on_need_visual_update(|| {
    ///         window.request_redraw();
    ///     })
    ///     .build();
    /// ```
    pub fn on_need_visual_update<F>(mut self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_need_visual_update = Some(Box::new(callback));
        self
    }

    /// Sets the callback for semantics owner creation.
    ///
    /// Called when a SemanticsOwner is created for this pipeline.
    pub fn on_semantics_owner_created<F>(mut self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_semantics_owner_created = Some(Box::new(callback));
        self
    }

    /// Sets the callback for semantics owner disposal.
    ///
    /// Called when the SemanticsOwner is disposed.
    pub fn on_semantics_owner_disposed<F>(mut self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_semantics_owner_disposed = Some(Box::new(callback));
        self
    }

    /// Sets the callback for semantics updates.
    ///
    /// Called when semantics data changes and needs to be sent to the platform.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use std::sync::Arc;
    ///
    /// let owner = PipelineOwner::builder()
    ///     .on_semantics_update(Arc::new(|updates| {
    ///         // Send updates to platform accessibility services
    ///         for update in updates {
    ///             platform.send_semantics(update);
    ///         }
    ///     }))
    ///     .build();
    /// ```
    pub fn on_semantics_update(mut self, callback: SemanticsUpdateCallback) -> Self {
        self.on_semantics_update = Some(callback);
        self
    }

    /// Builds the configured PipelineOwner.
    pub fn build(self) -> PipelineOwner {
        let mut owner = match self.capacity {
            Some(cap) => PipelineOwner::with_capacity(cap),
            None => PipelineOwner::new(),
        };

        owner.on_need_visual_update = self.on_need_visual_update;
        owner.on_semantics_owner_created = self.on_semantics_owner_created;
        owner.on_semantics_owner_disposed = self.on_semantics_owner_disposed;
        owner.on_semantics_update = self.on_semantics_update;

        owner
    }
}

// ============================================================================
// PIPELINE OWNER
// ============================================================================

/// Manages the rendering pipeline for RenderObjects.
///
/// Like Flutter's `PipelineOwner`, this struct:
/// - Owns the RenderTree
/// - Tracks which nodes need layout/paint/compositing/semantics
/// - Provides flush methods to process dirty nodes
/// - Supports child PipelineOwners for multi-view scenarios
///
/// # Flutter Protocol
///
/// The flush methods must be called in order:
/// 1. `flush_layout()` - Updates layout (shallowest first)
/// 2. `flush_compositing_bits()` - Updates compositing flags
/// 3. `flush_paint()` - Paints dirty nodes (deepest first)
/// 4. `flush_semantics()` - Updates accessibility tree
///
/// # Dirty Tracking
///
/// Dirty tracking uses `RenderId` (not `ElementId`) because:
/// - RenderObjects are self-contained for layout/paint
/// - Decouples rendering from element tree
/// - Matches Flutter's architecture
pub struct PipelineOwner {
    // ========================================================================
    // RENDER TREE
    // ========================================================================
    /// The render tree storing all RenderObjects
    render_tree: RenderTree,

    /// Root render object (Flutter: rootNode)
    root_node: Option<RenderId>,

    // ========================================================================
    // DIRTY TRACKING (Flutter: _nodesNeeding*)
    // ========================================================================
    /// Render objects that need layout (Flutter: _nodesNeedingLayout)
    nodes_needing_layout: Vec<RenderId>,

    /// Render objects that need paint (Flutter: _nodesNeedingPaint)
    nodes_needing_paint: Vec<RenderId>,

    /// Render objects that need compositing bits update
    /// (Flutter: _nodesNeedingCompositingBitsUpdate)
    nodes_needing_compositing_bits_update: Vec<RenderId>,

    /// Render objects that need semantics update
    /// (Flutter: _nodesNeedingSemantics)
    nodes_needing_semantics: HashSet<RenderId>,

    // ========================================================================
    // LAYOUT STATE (Flutter pattern)
    // ========================================================================
    /// Whether to merge dirty nodes during layout (Flutter: _shouldMergeDirtyNodes)
    ///
    /// Set to true when `invoke_layout_callback` returns, to handle
    /// LayoutBuilder mutations correctly.
    should_merge_dirty_nodes: bool,

    // ========================================================================
    // DEBUG FLAGS (Flutter pattern)
    // ========================================================================
    /// Whether currently in layout phase (Flutter: _debugDoingLayout)
    #[cfg(debug_assertions)]
    debug_doing_layout: bool,

    /// Whether currently laying out children (Flutter: _debugDoingChildLayout)
    #[cfg(debug_assertions)]
    debug_doing_child_layout: bool,

    /// Whether currently in paint phase (Flutter: _debugDoingPaint)
    #[cfg(debug_assertions)]
    debug_doing_paint: bool,

    /// Whether currently in semantics phase (Flutter: _debugDoingSemantics)
    #[cfg(debug_assertions)]
    debug_doing_semantics: bool,

    // ========================================================================
    // CALLBACKS (Flutter pattern)
    // ========================================================================
    /// Called when a render object wishes to update its visual appearance.
    /// (Flutter: onNeedVisualUpdate)
    on_need_visual_update: Option<OnNeedVisualUpdate>,

    /// Called when semantics owner is created.
    /// (Flutter: onSemanticsOwnerCreated)
    on_semantics_owner_created: Option<OnSemanticsOwnerCreated>,

    /// Called when semantics owner is disposed.
    /// (Flutter: onSemanticsOwnerDisposed)
    on_semantics_owner_disposed: Option<OnSemanticsOwnerDisposed>,

    /// Called when semantics owner emits an update.
    /// (Flutter: onSemanticsUpdate)
    on_semantics_update: Option<SemanticsUpdateCallback>,

    // ========================================================================
    // SEMANTICS (Flutter pattern)
    // ========================================================================
    /// The semantics owner for this pipeline.
    /// (Flutter: _semanticsOwner)
    semantics_owner: Option<SemanticsOwner>,

    /// Outstanding semantics handles count.
    /// (Flutter: _outstandingSemanticsHandles)
    outstanding_semantics_handles: usize,

    // ========================================================================
    // CHILD OWNERS (Flutter: _children)
    // ========================================================================
    /// Child PipelineOwners for multi-view support.
    /// (Flutter: _children)
    children: HashSet<PipelineOwnerId>,

    /// Reference to parent's manifold.
    /// (Flutter: _manifold)
    manifold: Option<std::sync::Arc<dyn PipelineManifold>>,

    /// Unique identifier for this PipelineOwner.
    id: PipelineOwnerId,

    /// Parent PipelineOwner ID (for debugging).
    #[cfg(debug_assertions)]
    debug_parent: Option<PipelineOwnerId>,
}

// ============================================================================
// CONSTRUCTION
// ============================================================================

impl PipelineOwner {
    /// Creates a new PipelineOwner with an empty render tree.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// PipelineOwner({
    ///   this.onNeedVisualUpdate,
    ///   this.onSemanticsOwnerCreated,
    ///   this.onSemanticsUpdate,
    ///   this.onSemanticsOwnerDisposed,
    /// });
    /// ```
    pub fn new() -> Self {
        Self {
            render_tree: RenderTree::new(),
            root_node: None,
            nodes_needing_layout: Vec::new(),
            nodes_needing_paint: Vec::new(),
            nodes_needing_compositing_bits_update: Vec::new(),
            nodes_needing_semantics: HashSet::new(),
            should_merge_dirty_nodes: false,
            #[cfg(debug_assertions)]
            debug_doing_layout: false,
            #[cfg(debug_assertions)]
            debug_doing_child_layout: false,
            #[cfg(debug_assertions)]
            debug_doing_paint: false,
            #[cfg(debug_assertions)]
            debug_doing_semantics: false,
            on_need_visual_update: None,
            on_semantics_owner_created: None,
            on_semantics_owner_disposed: None,
            on_semantics_update: None,
            semantics_owner: None,
            outstanding_semantics_handles: 0,
            children: HashSet::new(),
            manifold: None,
            id: PipelineOwnerId::new(),
            #[cfg(debug_assertions)]
            debug_parent: None,
        }
    }

    /// Creates a PipelineOwner with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            render_tree: RenderTree::with_capacity(capacity),
            root_node: None,
            nodes_needing_layout: Vec::with_capacity(capacity),
            nodes_needing_paint: Vec::with_capacity(capacity),
            nodes_needing_compositing_bits_update: Vec::with_capacity(capacity),
            nodes_needing_semantics: HashSet::with_capacity(capacity),
            should_merge_dirty_nodes: false,
            #[cfg(debug_assertions)]
            debug_doing_layout: false,
            #[cfg(debug_assertions)]
            debug_doing_child_layout: false,
            #[cfg(debug_assertions)]
            debug_doing_paint: false,
            #[cfg(debug_assertions)]
            debug_doing_semantics: false,
            on_need_visual_update: None,
            on_semantics_owner_created: None,
            on_semantics_owner_disposed: None,
            on_semantics_update: None,
            semantics_owner: None,
            outstanding_semantics_handles: 0,
            children: HashSet::new(),
            manifold: None,
            id: PipelineOwnerId::new(),
            #[cfg(debug_assertions)]
            debug_parent: None,
        }
    }

    /// Creates a PipelineOwner with callbacks (Flutter-style constructor).
    ///
    /// # Arguments
    ///
    /// * `on_need_visual_update` - Called when visual update is needed
    /// * `on_semantics_owner_created` - Called when semantics owner is created
    /// * `on_semantics_update` - Called when semantics are updated
    /// * `on_semantics_owner_disposed` - Called when semantics owner is disposed
    pub fn with_callbacks(on_need_visual_update: Option<OnNeedVisualUpdate>) -> Self {
        let mut owner = Self::new();
        owner.on_need_visual_update = on_need_visual_update;
        owner
    }

    /// Creates a builder for configuring a PipelineOwner.
    ///
    /// This provides a fluent API for setting up callbacks and initial capacity.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_rendering::PipelineOwner;
    ///
    /// let owner = PipelineOwner::builder()
    ///     .capacity(1000)
    ///     .on_need_visual_update(|| schedule_frame())
    ///     .on_semantics_update(|updates| send_to_platform(updates))
    ///     .build();
    /// ```
    pub fn builder() -> PipelineOwnerBuilder {
        PipelineOwnerBuilder::new()
    }

    /// Returns the unique identifier for this PipelineOwner.
    #[inline]
    pub fn id(&self) -> PipelineOwnerId {
        self.id
    }
}

impl Default for PipelineOwner {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// RENDER TREE ACCESS
// ============================================================================

impl PipelineOwner {
    /// Returns a reference to the render tree.
    #[inline]
    pub fn render_tree(&self) -> &RenderTree {
        &self.render_tree
    }

    /// Returns a mutable reference to the render tree.
    #[inline]
    pub fn render_tree_mut(&mut self) -> &mut RenderTree {
        &mut self.render_tree
    }

    /// Inserts a mounted render node into the tree.
    ///
    /// **Note**: Node must be in `Mounted` state. Use `node.mount()` first.
    #[inline]
    pub fn insert(&mut self, node: RenderNode<flui_tree::Mounted>) -> RenderId {
        self.render_tree.insert(node)
    }

    /// Gets a render node by ID.
    #[inline]
    pub fn get(&self, id: RenderId) -> Option<&RenderNode<flui_tree::Mounted>> {
        self.render_tree.get(id)
    }

    /// Gets a mutable render node by ID.
    #[inline]
    pub fn get_mut(&mut self, id: RenderId) -> Option<&mut RenderNode<flui_tree::Mounted>> {
        self.render_tree.get_mut(id)
    }

    /// Removes a render node from the tree.
    ///
    /// Returns the mounted node (still in `Mounted` state).
    /// Call `.unmount()` on the result to transition to `Unmounted`.
    #[inline]
    pub fn remove(&mut self, id: RenderId) -> Option<RenderNode<flui_tree::Mounted>> {
        // Also remove from dirty lists
        self.nodes_needing_layout.retain(|&x| x != id);
        self.nodes_needing_paint.retain(|&x| x != id);
        self.nodes_needing_compositing_bits_update
            .retain(|&x| x != id);
        self.nodes_needing_semantics.remove(&id);
        self.render_tree.remove(id)
    }

    /// Adds a child to a parent render node.
    #[inline]
    pub fn add_child(&mut self, parent: RenderId, child: RenderId) {
        self.render_tree.add_child(parent, child);
    }

    /// Removes a child from a parent render node.
    #[inline]
    pub fn remove_child(&mut self, parent: RenderId, child: RenderId) {
        self.render_tree.remove_child(parent, child);
    }
}

// ============================================================================
// ROOT NODE MANAGEMENT
// ============================================================================

impl PipelineOwner {
    /// Gets the root render object ID.
    #[inline]
    pub fn root_node(&self) -> Option<RenderId> {
        self.root_node
    }

    /// Sets the root render object ID.
    ///
    /// Detaches the old root and attaches the new root to this pipeline owner.
    pub fn set_root_node(&mut self, root: Option<RenderId>) {
        if self.root_node == root {
            return;
        }

        // Detach old root (Flutter: _rootNode?.detach())
        if let Some(old_root) = self.root_node {
            if let Some(node) = self.render_tree.get_mut(old_root) {
                let mut lifecycle = node.lifecycle();
                lifecycle.detach();
                node.set_lifecycle(lifecycle);
            }
        }

        self.root_node = root;

        // Attach new root (Flutter: _rootNode?.attach(this))
        // Only attach if not already attached (node may have been mounted with mount_root())
        if let Some(new_root) = root {
            if let Some(node) = self.render_tree.get_mut(new_root) {
                let lifecycle = node.lifecycle();
                if !lifecycle.is_attached() {
                    let mut lifecycle = lifecycle;
                    lifecycle.attach();
                    node.set_lifecycle(lifecycle);
                }
            }
        }
    }

    // Backward compatibility alias
    #[inline]
    #[deprecated(since = "0.2.0", note = "Use root_node() instead")]
    pub fn root(&self) -> Option<RenderId> {
        self.root_node()
    }

    #[inline]
    #[deprecated(since = "0.2.0", note = "Use set_root_node() instead")]
    pub fn set_root(&mut self, root: Option<RenderId>) {
        self.set_root_node(root)
    }
}

// ============================================================================
// VISUAL UPDATE REQUESTS (Flutter pattern)
// ============================================================================

impl PipelineOwner {
    /// Requests a visual update.
    ///
    /// Called when a render object wishes to update its visual appearance.
    /// This triggers the `on_need_visual_update` callback if set, otherwise
    /// falls back to the manifold.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void requestVisualUpdate() {
    ///   if (onNeedVisualUpdate != null) {
    ///     onNeedVisualUpdate!();
    ///   } else {
    ///     _manifold?.requestVisualUpdate();
    ///   }
    /// }
    /// ```
    pub fn request_visual_update(&self) {
        if let Some(ref callback) = self.on_need_visual_update {
            callback();
        } else if let Some(ref manifold) = self.manifold {
            manifold.request_visual_update();
        }
    }

    /// Sets the visual update callback.
    pub fn set_on_need_visual_update(&mut self, callback: Option<OnNeedVisualUpdate>) {
        self.on_need_visual_update = callback;
    }

    /// Sets the semantics update callback.
    pub fn set_on_semantics_update(&mut self, callback: Option<SemanticsUpdateCallback>) {
        self.on_semantics_update = callback;
    }

    /// Sets the semantics owner created callback.
    pub fn set_on_semantics_owner_created(&mut self, callback: Option<OnSemanticsOwnerCreated>) {
        self.on_semantics_owner_created = callback;
    }

    /// Sets the semantics owner disposed callback.
    pub fn set_on_semantics_owner_disposed(&mut self, callback: Option<OnSemanticsOwnerDisposed>) {
        self.on_semantics_owner_disposed = callback;
    }
}

// ============================================================================
// SEMANTICS OWNER MANAGEMENT (Flutter pattern)
// ============================================================================

impl PipelineOwner {
    /// Returns the semantics owner, if any.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// SemanticsOwner? get semanticsOwner => _semanticsOwner;
    /// ```
    #[inline]
    pub fn semantics_owner(&self) -> Option<&SemanticsOwner> {
        self.semantics_owner.as_ref()
    }

    /// Returns the number of outstanding semantics handles.
    ///
    /// Deprecated in Flutter - use SemanticsBinding.debugOutstandingSemanticsHandles instead.
    #[inline]
    pub fn debug_outstanding_semantics_handles(&self) -> usize {
        self.outstanding_semantics_handles
    }

    /// Updates the semantics owner based on current state.
    ///
    /// Creates or disposes the semantics owner based on:
    /// - Whether the manifold has semantics enabled
    /// - Whether there are outstanding semantics handles
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void _updateSemanticsOwner() {
    ///   if ((_manifold?.semanticsEnabled ?? false) || _outstandingSemanticsHandles > 0) {
    ///     if (_semanticsOwner == null) {
    ///       _semanticsOwner = SemanticsOwner(onSemanticsUpdate: onSemanticsUpdate!);
    ///       onSemanticsOwnerCreated?.call();
    ///     }
    ///   } else if (_semanticsOwner != null) {
    ///     _semanticsOwner?.dispose();
    ///     _semanticsOwner = null;
    ///     onSemanticsOwnerDisposed?.call();
    ///   }
    /// }
    /// ```
    fn update_semantics_owner(&mut self) {
        let semantics_enabled = self
            .manifold
            .as_ref()
            .map(|m| m.semantics_enabled())
            .unwrap_or(false);

        if semantics_enabled || self.outstanding_semantics_handles > 0 {
            if self.semantics_owner.is_none() {
                // Create semantics owner with the callback
                if let Some(ref callback) = self.on_semantics_update {
                    // SemanticsUpdateCallback is Arc, so we can clone it
                    self.semantics_owner = Some(SemanticsOwner::new(callback.clone()));
                    if let Some(ref on_created) = self.on_semantics_owner_created {
                        on_created();
                    }
                }
            }
        } else if self.semantics_owner.is_some() {
            // Dispose semantics owner
            if let Some(ref mut owner) = self.semantics_owner {
                owner.dispose();
            }
            self.semantics_owner = None;
            if let Some(ref on_disposed) = self.on_semantics_owner_disposed {
                on_disposed();
            }
        }
    }

    /// Called when a semantics handle is disposed.
    pub(crate) fn did_dispose_semantics_handle(&mut self) {
        debug_assert!(self.semantics_owner.is_some());
        self.outstanding_semantics_handles = self.outstanding_semantics_handles.saturating_sub(1);
        self.update_semantics_owner();
    }
}

// ============================================================================
// CHILD PIPELINE OWNERS
// ============================================================================

/// Signature for the callback to [`PipelineOwner::visit_children`].
pub type PipelineOwnerVisitor = Box<dyn FnMut(PipelineOwnerId)>;

impl PipelineOwner {
    /// Returns the child PipelineOwner IDs.
    #[inline]
    pub fn children(&self) -> &HashSet<PipelineOwnerId> {
        &self.children
    }

    /// Adopts a child PipelineOwner.
    ///
    /// The child will be flushed during this owner's flush operations.
    pub fn adopt_child(&mut self, child_id: PipelineOwnerId) {
        self.children.insert(child_id);
        // Note: In full implementation, we'd also:
        // - Set child's debug_parent to self.id
        // - Attach child to our manifold if we have one
    }

    /// Drops a child PipelineOwner.
    ///
    /// The child will no longer be flushed during this owner's flush operations.
    pub fn drop_child(&mut self, child_id: PipelineOwnerId) {
        self.children.remove(&child_id);
        // Note: In full implementation, we'd also:
        // - Clear child's debug_parent
        // - Detach child from manifold
    }

    /// Calls `visitor` for each immediate child of this PipelineOwner.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// owner.visit_children(|child_id| {
    ///     println!("Child: {:?}", child_id);
    /// });
    /// ```
    pub fn visit_children<F>(&self, mut visitor: F)
    where
        F: FnMut(PipelineOwnerId),
    {
        for &child_id in &self.children {
            visitor(child_id);
        }
    }
}

// ============================================================================
// PIPELINE MANIFOLD ATTACHMENT (Flutter: attach/detach)
// ============================================================================

impl PipelineOwner {
    /// Returns the attached manifold, if any.
    #[inline]
    pub fn manifold(&self) -> Option<&std::sync::Arc<dyn PipelineManifold>> {
        self.manifold.as_ref()
    }

    /// Attaches this PipelineOwner to a manifold.
    ///
    /// Typically called only on the root PipelineOwner.
    /// Children are automatically attached when adopted.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void attach(PipelineManifold manifold) {
    ///   assert(_manifold == null);
    ///   _manifold = manifold;
    ///   _manifold!.addListener(_updateSemanticsOwner);
    ///   _updateSemanticsOwner();
    ///   for (final child in _children) {
    ///     child.attach(manifold);
    ///   }
    /// }
    /// ```
    pub fn attach(&mut self, manifold: std::sync::Arc<dyn PipelineManifold>) {
        debug_assert!(
            self.manifold.is_none(),
            "PipelineOwner is already attached to a manifold"
        );
        self.manifold = Some(manifold);
        // TODO: Add listener for semantics enabled changes
        self.update_semantics_owner();
        // Note: In full implementation, we'd also attach all children
    }

    /// Detaches this PipelineOwner from its manifold.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void detach() {
    ///   assert(_manifold != null);
    ///   _manifold!.removeListener(_updateSemanticsOwner);
    ///   _manifold = null;
    ///   for (final child in _children) {
    ///     child.detach();
    ///   }
    /// }
    /// ```
    pub fn detach(&mut self) {
        debug_assert!(
            self.manifold.is_some(),
            "PipelineOwner is not attached to a manifold"
        );
        // TODO: Remove listener for semantics enabled changes
        self.manifold = None;
        // Note: In full implementation, we'd also detach all children
    }

    /// Whether this PipelineOwner is attached to a manifold.
    #[inline]
    pub fn is_attached(&self) -> bool {
        self.manifold.is_some()
    }
}

// ============================================================================
// DIRTY TRACKING (Flutter: markNeeds* methods)
// ============================================================================

impl PipelineOwner {
    /// Marks a render object as needing layout.
    ///
    /// This is called when:
    /// - Constraints change
    /// - A render object's intrinsic dimensions change
    /// - Children are added/removed
    ///
    /// # Flutter Equivalence
    ///
    /// In Flutter, this is called internally by `RenderObject.markNeedsLayout()`.
    /// The PipelineOwner adds the node to `_nodesNeedingLayout`.
    pub fn mark_needs_layout(&mut self, id: RenderId) {
        if !self.nodes_needing_layout.contains(&id) {
            self.nodes_needing_layout.push(id);
        }
    }

    /// Marks a render object as needing paint.
    ///
    /// This is called when visual properties change (color, opacity, etc.)
    /// but layout remains the same.
    pub fn mark_needs_paint(&mut self, id: RenderId) {
        if !self.nodes_needing_paint.contains(&id) {
            self.nodes_needing_paint.push(id);
        }
    }

    /// Marks a render object as needing compositing bits update.
    ///
    /// This is called when:
    /// - `isRepaintBoundary` changes
    /// - `needsCompositing` changes
    pub fn mark_needs_compositing_bits_update(&mut self, id: RenderId) {
        if !self.nodes_needing_compositing_bits_update.contains(&id) {
            self.nodes_needing_compositing_bits_update.push(id);
        }
    }

    /// Marks a render object as needing semantics update.
    pub fn mark_needs_semantics_update(&mut self, id: RenderId) {
        self.nodes_needing_semantics.insert(id);
    }

    /// Returns the nodes that need layout.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// @protected
    /// Iterable<RenderObject> get nodesNeedingLayout => _nodesNeedingLayout;
    /// ```
    #[inline]
    pub fn nodes_needing_layout(&self) -> &[RenderId] {
        &self.nodes_needing_layout
    }

    /// Returns the nodes that need paint.
    #[inline]
    pub fn nodes_needing_paint(&self) -> &[RenderId] {
        &self.nodes_needing_paint
    }

    /// Returns the nodes that need compositing bits update.
    #[inline]
    pub fn nodes_needing_compositing_bits_update(&self) -> &[RenderId] {
        &self.nodes_needing_compositing_bits_update
    }

    /// Checks if there are any dirty render objects.
    pub fn has_dirty_nodes(&self) -> bool {
        !self.nodes_needing_layout.is_empty()
            || !self.nodes_needing_paint.is_empty()
            || !self.nodes_needing_compositing_bits_update.is_empty()
            || !self.nodes_needing_semantics.is_empty()
    }

    /// Checks if any render object needs layout.
    #[inline]
    pub fn has_needs_layout(&self) -> bool {
        !self.nodes_needing_layout.is_empty()
    }

    /// Checks if any render object needs paint.
    #[inline]
    pub fn has_needs_paint(&self) -> bool {
        !self.nodes_needing_paint.is_empty()
    }

    /// Clears all dirty tracking.
    pub fn clear_dirty(&mut self) {
        self.nodes_needing_layout.clear();
        self.nodes_needing_paint.clear();
        self.nodes_needing_compositing_bits_update.clear();
        self.nodes_needing_semantics.clear();
    }

    // Backward compatibility (returns slice instead of HashSet)
    #[deprecated(since = "0.2.0", note = "Use nodes_needing_layout() instead")]
    pub fn needs_layout(&self) -> std::collections::HashSet<RenderId> {
        self.nodes_needing_layout.iter().copied().collect()
    }

    #[deprecated(since = "0.2.0", note = "Use nodes_needing_paint() instead")]
    pub fn needs_paint(&self) -> std::collections::HashSet<RenderId> {
        self.nodes_needing_paint.iter().copied().collect()
    }

    #[deprecated(
        since = "0.2.0",
        note = "Use nodes_needing_compositing_bits_update() instead"
    )]
    pub fn needs_compositing_bits_update(&self) -> std::collections::HashSet<RenderId> {
        self.nodes_needing_compositing_bits_update
            .iter()
            .copied()
            .collect()
    }
}

// ============================================================================
// DEBUG STATE (Flutter pattern)
// ============================================================================

impl PipelineOwner {
    /// Whether this pipeline is currently in the layout phase.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// bool get debugDoingLayout => _debugDoingLayout;
    /// ```
    #[cfg(debug_assertions)]
    #[inline]
    pub fn debug_doing_layout(&self) -> bool {
        self.debug_doing_layout
    }

    /// Whether this pipeline is currently in the paint phase.
    #[cfg(debug_assertions)]
    #[inline]
    pub fn debug_doing_paint(&self) -> bool {
        self.debug_doing_paint
    }

    /// Whether this pipeline is currently in the semantics phase.
    #[cfg(debug_assertions)]
    #[inline]
    pub fn debug_doing_semantics(&self) -> bool {
        self.debug_doing_semantics
    }
}

// ============================================================================
// FLUSH OPERATIONS (Flutter PipelineOwner pattern)
// ============================================================================

impl PipelineOwner {
    /// Flushes the layout phase.
    ///
    /// Processes all render objects marked as needing layout, in depth order
    /// (parents before children). This matches Flutter's `flushLayout()`.
    ///
    /// # Algorithm
    ///
    /// 1. Swap dirty list with empty list
    /// 2. Sort by depth (shallowest first = parents before children)
    /// 3. For each dirty node, call `_layoutWithoutResize()` if still dirty
    /// 4. Handle dynamic dirty node merging (for LayoutBuilder)
    /// 5. Repeat until no more dirty nodes
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void flushLayout() {
    ///   while (_nodesNeedingLayout.isNotEmpty) {
    ///     final dirtyNodes = _nodesNeedingLayout;
    ///     _nodesNeedingLayout = [];
    ///     dirtyNodes.sort((a, b) => a.depth - b.depth);
    ///     for (var i = 0; i < dirtyNodes.length; i++) {
    ///       if (_shouldMergeDirtyNodes) {
    ///         _shouldMergeDirtyNodes = false;
    ///         if (_nodesNeedingLayout.isNotEmpty) {
    ///           _nodesNeedingLayout.addAll(dirtyNodes.getRange(i, dirtyNodes.length));
    ///           break;
    ///         }
    ///       }
    ///       final node = dirtyNodes[i];
    ///       if (node._needsLayout && node.owner == this) {
    ///         node._layoutWithoutResize();
    ///       }
    ///     }
    ///   }
    /// }
    /// ```
    #[tracing::instrument(skip(self), fields(dirty_count = self.nodes_needing_layout.len()))]
    pub fn flush_layout(&mut self) {
        #[cfg(debug_assertions)]
        {
            self.debug_doing_layout = true;
        }

        while !self.nodes_needing_layout.is_empty() {
            debug_assert!(!self.should_merge_dirty_nodes);

            // Swap dirty list with empty list (Flutter pattern)
            let mut dirty_nodes = std::mem::take(&mut self.nodes_needing_layout);

            // Collect depths and sort by depth (shallowest first)
            let mut nodes_with_depth: Vec<(RenderId, usize)> = dirty_nodes
                .drain(..)
                .filter_map(|id| {
                    self.render_tree
                        .get(id)
                        .map(|node| (id, node.depth().get()))
                })
                .collect();

            nodes_with_depth.sort_by_key(|(_, depth)| *depth);

            for (i, (id, depth)) in nodes_with_depth.iter().enumerate() {
                // Handle dynamic dirty node merging (Flutter: _shouldMergeDirtyNodes)
                if self.should_merge_dirty_nodes {
                    self.should_merge_dirty_nodes = false;
                    if !self.nodes_needing_layout.is_empty() {
                        // Add remaining nodes back to dirty list
                        for (remaining_id, _) in nodes_with_depth.iter().skip(i) {
                            self.nodes_needing_layout.push(*remaining_id);
                        }
                        break;
                    }
                }

                if let Some(_node) = self.render_tree.get_mut(*id) {
                    // TODO: Check if node._needsLayout && node.owner == this
                    // TODO: Call node._layoutWithoutResize()
                    tracing::trace!(
                        ?id,
                        depth,
                        "flush_layout: processing node (shallowest first)"
                    );
                }
            }

            self.should_merge_dirty_nodes = false;
        }

        #[cfg(debug_assertions)]
        {
            self.debug_doing_child_layout = true;
        }

        // TODO: Flush child PipelineOwners
        // for child in &mut self.children {
        //     child.flush_layout();
        // }

        #[cfg(debug_assertions)]
        {
            self.debug_doing_layout = false;
            self.debug_doing_child_layout = false;
        }
    }

    /// Flushes the compositing bits update phase.
    ///
    /// Updates the `needsCompositing` flag on render objects and their subtrees.
    /// Must be called after `flush_layout()` and before `flush_paint()`.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void flushCompositingBits() {
    ///   _nodesNeedingCompositingBitsUpdate.sort((a, b) => a.depth - b.depth);
    ///   for (final node in _nodesNeedingCompositingBitsUpdate) {
    ///     if (node._needsCompositingBitsUpdate && node.owner == this) {
    ///       node._updateCompositingBits();
    ///     }
    ///   }
    ///   _nodesNeedingCompositingBitsUpdate.clear();
    /// }
    /// ```
    #[tracing::instrument(skip(self), fields(dirty_count = self.nodes_needing_compositing_bits_update.len()))]
    pub fn flush_compositing_bits(&mut self) {
        if self.nodes_needing_compositing_bits_update.is_empty() {
            return;
        }

        // Collect with depths and sort (shallowest first)
        let mut nodes_with_depth: Vec<(RenderId, usize)> = self
            .nodes_needing_compositing_bits_update
            .drain(..)
            .filter_map(|id| {
                self.render_tree
                    .get(id)
                    .map(|node| (id, node.depth().get()))
            })
            .collect();

        nodes_with_depth.sort_by_key(|(_, depth)| *depth);

        for (id, _depth) in nodes_with_depth {
            if self.render_tree.contains(id) {
                let changed = self.render_tree.update_compositing_bits(id);

                if changed {
                    tracing::trace!(
                        ?id,
                        needs_compositing = self
                            .render_tree
                            .get(id)
                            .map(|n| n.needs_compositing())
                            .unwrap_or(false),
                        "flush_compositing_bits: compositing changed, marking for repaint"
                    );

                    // If compositing needs changed, mark for repaint (Flutter pattern)
                    self.mark_needs_paint(id);
                }
            }
        }

        // TODO: Flush child PipelineOwners
        // for child in &mut self.children {
        //     child.flush_compositing_bits();
        // }
    }

    /// Flushes the paint phase.
    ///
    /// Processes all render objects marked as needing paint, in reverse depth order
    /// (children before parents). This matches Flutter's `flushPaint()`.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void flushPaint() {
    ///   final dirtyNodes = _nodesNeedingPaint;
    ///   _nodesNeedingPaint = [];
    ///   for (final node in dirtyNodes..sort((a, b) => b.depth - a.depth)) {
    ///     if ((node._needsPaint || node._needsCompositedLayerUpdate) && node.owner == this) {
    ///       if (node._layerHandle.layer!.attached) {
    ///         if (node._needsPaint) {
    ///           PaintingContext.repaintCompositedChild(node);
    ///         } else {
    ///           PaintingContext.updateLayerProperties(node);
    ///         }
    ///       } else {
    ///         node._skippedPaintingOnLayer();
    ///       }
    ///     }
    ///   }
    /// }
    /// ```
    #[tracing::instrument(skip(self), fields(dirty_count = self.nodes_needing_paint.len()))]
    pub fn flush_paint(&mut self) {
        #[cfg(debug_assertions)]
        {
            self.debug_doing_paint = true;
        }

        // Swap dirty list with empty list
        let dirty_nodes = std::mem::take(&mut self.nodes_needing_paint);

        // Collect with depths
        let mut nodes_with_depth: Vec<(RenderId, usize)> = dirty_nodes
            .into_iter()
            .filter_map(|id| {
                self.render_tree
                    .get(id)
                    .map(|node| (id, node.depth().get()))
            })
            .collect();

        // Sort by depth: DEEPEST FIRST (children before parents)
        nodes_with_depth.sort_by_key(|(_, depth)| std::cmp::Reverse(*depth));

        for (id, depth) in nodes_with_depth {
            if let Some(_node) = self.render_tree.get_mut(id) {
                // TODO: Check layer.attached
                // TODO: Call PaintingContext.repaintCompositedChild(node)
                //       or PaintingContext.updateLayerProperties(node)
                tracing::trace!(?id, depth, "flush_paint: processing node (deepest first)");
            }
        }

        // TODO: Flush child PipelineOwners
        // for child in &mut self.children {
        //     child.flush_paint();
        // }

        #[cfg(debug_assertions)]
        {
            self.debug_doing_paint = false;
        }
    }

    /// Flushes the semantics phase.
    ///
    /// Updates the accessibility tree for render objects marked as needing
    /// semantics update.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void flushSemantics() {
    ///   if (_semanticsOwner == null) return;
    ///   final nodesToProcess = _nodesNeedingSemantics
    ///       .where((object) => !object._needsLayout && object.owner == this)
    ///       .toList()
    ///     ..sort((a, b) => a.depth - b.depth);
    ///   _nodesNeedingSemantics.clear();
    ///   // ... process nodes
    /// }
    /// ```
    #[tracing::instrument(skip(self), fields(dirty_count = self.nodes_needing_semantics.len()))]
    pub fn flush_semantics(&mut self) {
        // TODO: Check if semantics owner exists
        // if self.semantics_owner.is_none() { return; }

        if self.nodes_needing_semantics.is_empty() {
            return;
        }

        #[cfg(debug_assertions)]
        {
            self.debug_doing_semantics = true;
        }

        // Collect nodes that don't need layout and sort by depth
        let mut nodes_to_process: Vec<(RenderId, usize)> = self
            .nodes_needing_semantics
            .drain()
            .filter_map(|id| {
                self.render_tree.get(id).and_then(|node| {
                    // Skip nodes that need layout (Flutter pattern)
                    if !node.lifecycle().in_needs_layout_phase() {
                        Some((id, node.depth().get()))
                    } else {
                        None
                    }
                })
            })
            .collect();

        // Sort shallowest first (top-to-down order for geometry calculation)
        nodes_to_process.sort_by_key(|(_, depth)| *depth);

        for (id, _depth) in nodes_to_process {
            if let Some(_node) = self.render_tree.get(id) {
                // TODO: Process semantics
                tracing::trace!(?id, "flush_semantics: processing node");
            }
        }

        // TODO: Flush child PipelineOwners
        // for child in &mut self.children {
        //     child.flush_semantics();
        // }

        #[cfg(debug_assertions)]
        {
            self.debug_doing_semantics = false;
        }
    }

    /// Performs a complete flush cycle: layout → compositing bits → paint.
    ///
    /// This is the main entry point for processing a frame.
    /// Does NOT include semantics (call flush_semantics() separately if needed).
    pub fn flush_pipeline(&mut self) {
        self.flush_layout();
        self.flush_compositing_bits();
        self.flush_paint();
    }

    /// Performs a complete flush cycle including semantics.
    pub fn flush_pipeline_with_semantics(&mut self) {
        self.flush_layout();
        self.flush_compositing_bits();
        self.flush_paint();
        self.flush_semantics();
    }
}

// ============================================================================
// LAYOUT CALLBACK SUPPORT (Flutter: invokeLayoutCallback)
// ============================================================================

impl PipelineOwner {
    /// Enables mutations to dirty subtrees during layout callback.
    ///
    /// This is used by LayoutBuilder to allow adding children during layout.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void _enableMutationsToDirtySubtrees(VoidCallback callback) {
    ///   assert(_debugDoingLayout);
    ///   try {
    ///     callback();
    ///   } finally {
    ///     _shouldMergeDirtyNodes = true;
    ///   }
    /// }
    /// ```
    pub fn enable_mutations_to_dirty_subtrees<F, R>(&mut self, callback: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        #[cfg(debug_assertions)]
        debug_assert!(
            self.debug_doing_layout,
            "enable_mutations_to_dirty_subtrees must be called during layout"
        );

        let result = callback(self);
        self.should_merge_dirty_nodes = true;
        result
    }
}

// ============================================================================
// DISPOSAL (Flutter pattern)
// ============================================================================

impl PipelineOwner {
    /// Releases any resources held by this pipeline owner.
    ///
    /// The object is no longer usable after calling dispose.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void dispose() {
    ///   assert(_children.isEmpty);
    ///   assert(rootNode == null);
    ///   _semanticsOwner?.dispose();
    ///   _nodesNeedingLayout.clear();
    ///   _nodesNeedingCompositingBitsUpdate.clear();
    ///   _nodesNeedingPaint.clear();
    ///   _nodesNeedingSemantics.clear();
    /// }
    /// ```
    pub fn dispose(&mut self) {
        // TODO: Assert children is empty
        // TODO: Assert rootNode is null
        // TODO: Dispose semantics owner
        self.clear_dirty();
    }
}

// ============================================================================
// DEBUG IMPLEMENTATION
// ============================================================================

impl std::fmt::Debug for PipelineOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PipelineOwner")
            .field("id", &self.id)
            .field("root_node", &self.root_node)
            .field("nodes_needing_layout", &self.nodes_needing_layout.len())
            .field("nodes_needing_paint", &self.nodes_needing_paint.len())
            .field(
                "nodes_needing_compositing_bits_update",
                &self.nodes_needing_compositing_bits_update.len(),
            )
            .field(
                "nodes_needing_semantics",
                &self.nodes_needing_semantics.len(),
            )
            .field("children", &self.children.len())
            .field("has_manifold", &self.manifold.is_some())
            .field("has_semantics_owner", &self.semantics_owner.is_some())
            .field(
                "has_on_need_visual_update",
                &self.on_need_visual_update.is_some(),
            )
            .finish_non_exhaustive()
    }
}

// ============================================================================
// BACKWARD COMPATIBILITY ALIAS
// ============================================================================

/// Alias for `PipelineOwner` for backward compatibility.
#[deprecated(since = "0.2.0", note = "Use PipelineOwner instead")]
pub type RenderPipelineOwner = PipelineOwner;

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RenderObject;

    #[derive(Debug)]
    struct TestRenderObject;

    impl flui_foundation::Diagnosticable for TestRenderObject {}

    impl flui_interaction::HitTestTarget for TestRenderObject {
        fn handle_event(
            &self,
            _event: &flui_types::events::PointerEvent,
            _entry: &flui_interaction::HitTestEntry,
        ) {
        }
    }

    impl RenderObject for TestRenderObject {}

    #[test]
    fn test_pipeline_owner_creation() {
        let pipeline = PipelineOwner::new();
        assert!(pipeline.root_node().is_none());
        assert!(!pipeline.has_dirty_nodes());
    }

    #[test]
    fn test_insert_and_mark_dirty() {
        use flui_tree::MountableExt;

        let mut pipeline = PipelineOwner::new();

        let node = RenderNode::new(TestRenderObject).mount_root();
        let id = pipeline.insert(node);
        pipeline.set_root_node(Some(id));

        assert_eq!(pipeline.root_node(), Some(id));
        assert!(!pipeline.has_dirty_nodes());

        pipeline.mark_needs_layout(id);
        assert!(pipeline.has_needs_layout());

        pipeline.flush_layout();
        assert!(!pipeline.has_needs_layout());
    }

    #[test]
    fn test_mark_needs_paint() {
        use flui_tree::MountableExt;

        let mut pipeline = PipelineOwner::new();

        let node = RenderNode::new(TestRenderObject).mount_root();
        let id = pipeline.insert(node);

        pipeline.mark_needs_paint(id);
        assert!(pipeline.has_needs_paint());
        assert!(!pipeline.has_needs_layout());

        pipeline.flush_paint();
        assert!(!pipeline.has_needs_paint());
    }

    #[test]
    fn test_remove_clears_dirty() {
        use flui_tree::MountableExt;

        let mut pipeline = PipelineOwner::new();

        let node = RenderNode::new(TestRenderObject).mount_root();
        let id = pipeline.insert(node);

        pipeline.mark_needs_layout(id);
        pipeline.mark_needs_paint(id);
        assert!(pipeline.has_dirty_nodes());

        pipeline.remove(id);
        assert!(!pipeline.nodes_needing_layout().contains(&id));
        assert!(!pipeline.nodes_needing_paint().contains(&id));
    }

    #[test]
    fn test_flush_pipeline() {
        use flui_tree::MountableExt;

        let mut pipeline = PipelineOwner::new();

        let node = RenderNode::new(TestRenderObject).mount_root();
        let id = pipeline.insert(node);

        pipeline.mark_needs_layout(id);
        pipeline.mark_needs_compositing_bits_update(id);

        assert!(pipeline.has_dirty_nodes());

        pipeline.flush_pipeline();

        assert!(!pipeline.has_dirty_nodes());
    }

    #[test]
    fn test_flush_layout_depth_sorting() {
        use flui_tree::{Depth, Mountable, MountableExt};

        let mut pipeline = PipelineOwner::new();

        // Create tree: root -> child1 -> grand1
        //                  -> child2 -> grand2

        let root_node = RenderNode::new(TestRenderObject).mount_root();
        let root_id = pipeline.insert(root_node);
        pipeline.set_root_node(Some(root_id));

        let child1_node = RenderNode::new(TestRenderObject).mount(Some(root_id), Depth::root());
        let child1_id = pipeline.insert(child1_node);
        pipeline.add_child(root_id, child1_id);

        let child2_node = RenderNode::new(TestRenderObject).mount(Some(root_id), Depth::root());
        let child2_id = pipeline.insert(child2_node);
        pipeline.add_child(root_id, child2_id);

        let grand1_depth = pipeline.get(child1_id).unwrap().depth();
        let grand1_node = RenderNode::new(TestRenderObject).mount(Some(child1_id), grand1_depth);
        let grand1_id = pipeline.insert(grand1_node);
        pipeline.add_child(child1_id, grand1_id);

        let grand2_depth = pipeline.get(child2_id).unwrap().depth();
        let grand2_node = RenderNode::new(TestRenderObject).mount(Some(child2_id), grand2_depth);
        let grand2_id = pipeline.insert(grand2_node);
        pipeline.add_child(child2_id, grand2_id);

        // Mark in random order
        pipeline.mark_needs_layout(grand2_id);
        pipeline.mark_needs_layout(grand1_id);
        pipeline.mark_needs_layout(child1_id);
        pipeline.mark_needs_layout(root_id);

        // Verify depths
        assert_eq!(pipeline.get(root_id).unwrap().depth().get(), 0);
        assert_eq!(pipeline.get(child1_id).unwrap().depth().get(), 1);
        assert_eq!(pipeline.get(grand1_id).unwrap().depth().get(), 2);

        pipeline.flush_layout();
        assert!(!pipeline.has_needs_layout());
    }

    #[test]
    fn test_flush_paint_depth_sorting() {
        use flui_tree::{Depth, Mountable, MountableExt};

        let mut pipeline = PipelineOwner::new();

        let root_node = RenderNode::new(TestRenderObject).mount_root();
        let root_id = pipeline.insert(root_node);
        pipeline.set_root_node(Some(root_id));

        let child1_node = RenderNode::new(TestRenderObject).mount(Some(root_id), Depth::root());
        let child1_id = pipeline.insert(child1_node);
        pipeline.add_child(root_id, child1_id);

        let grand1_depth = pipeline.get(child1_id).unwrap().depth();
        let grand1_node = RenderNode::new(TestRenderObject).mount(Some(child1_id), grand1_depth);
        let grand1_id = pipeline.insert(grand1_node);
        pipeline.add_child(child1_id, grand1_id);

        // Mark in shallowest to deepest order
        pipeline.mark_needs_paint(root_id);
        pipeline.mark_needs_paint(child1_id);
        pipeline.mark_needs_paint(grand1_id);

        // Verify depths
        assert_eq!(pipeline.get(root_id).unwrap().depth().get(), 0);
        assert_eq!(pipeline.get(child1_id).unwrap().depth().get(), 1);
        assert_eq!(pipeline.get(grand1_id).unwrap().depth().get(), 2);

        // Flush should process deepest first
        pipeline.flush_paint();
        assert!(!pipeline.has_needs_paint());
    }

    #[test]
    fn test_request_visual_update() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        let pipeline = PipelineOwner::with_callbacks(Some(Box::new(move || {
            called_clone.store(true, Ordering::SeqCst);
        })));

        pipeline.request_visual_update();
        assert!(called.load(Ordering::SeqCst));
    }
}
