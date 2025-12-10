//! RenderTree - Advanced Slab-based storage with Typestate Pattern
//!
//! This module implements FLUI's RenderTree using advanced Rust patterns:
//!
//! # Advanced Features
//!
//! - **Typestate Pattern**: `RenderNode<S: NodeState>` for compile-time state safety
//! - **GATs**: Generic Associated Types for protocol-specific constraints
//! - **HRTBs**: Higher-Rank Trait Bounds for flexible visitors
//! - **Zero-cost Abstractions**: All state transitions at compile-time
//!
//! # Architecture
//!
//! ```text
//! RenderTree<S: NodeState>
//!   ├─ nodes: Slab<RenderNode<S>>
//!   └─ root: Option<RenderId>
//!
//! RenderNode<S: NodeState> (typestate pattern)
//!   ├─ Common fields:
//!   │  ├─ render_object: Box<dyn RenderObject>
//!   │  ├─ lifecycle: RenderLifecycle
//!   │  └─ element_id: Option<ElementId>
//!   │
//!   └─ State-specific:
//!      ├─ Unmounted: detached, no tree position
//!      └─ Mounted: parent, depth, children, cached metadata
//! ```
//!
//! # Typestate Transitions
//!
//! ```text
//! RenderNode<Unmounted> ─mount()─► RenderNode<Mounted>
//!                                        │
//!                                        │ layout()
//!                                        │ paint()
//!                                        ▼
//!                                  RenderNode<Mounted>
//!                                        │
//!                                  unmount()
//!                                        │
//!                                        ▼
//!                            RenderNode<Unmounted>
//! ```
//!
//! # flui-tree Integration
//!
//! Implements `TreeRead<I>`, `TreeNav<I>`, `TreeWrite<I>` with:
//! - RPITIT for zero-cost iterators
//! - HRTB for universal predicates
//! - GAT for protocol-specific metadata

use slab::Slab;
use std::marker::PhantomData;

use flui_tree::iter::{AllSiblings, Ancestors, DescendantsWithDepth};
use flui_tree::{Depth, Mountable, Mounted, NodeState, TreeNav, TreeRead, TreeWrite, Unmountable, Unmounted};

use flui_foundation::{ElementId, RenderId};
use flui_types::{Matrix4, Offset, Size};

use crate::{LayerHandle, ParentData, RenderLifecycle, RenderObject};

// ============================================================================
// RENDER NODE WITH TYPESTATE PATTERN
// ============================================================================

/// RenderNode with compile-time state tracking via typestate pattern.
///
/// This struct uses the typestate pattern to enforce correct usage at compile-time:
/// - `RenderNode<Unmounted>`: Detached node, no tree position
/// - `RenderNode<Mounted>`: Attached node with parent, depth, children
///
/// # Type Parameters
///
/// - `S: NodeState` - Compile-time state marker (Mounted/Unmounted)
///
/// # Examples
///
/// ```rust,ignore
/// // Create unmounted node
/// let unmounted = RenderNode::new(render_object);
///
/// // Mount into tree
/// let mounted = unmounted.mount(Some(parent_id), parent_depth);
///
/// // Access tree position (only available when Mounted)
/// let parent = mounted.parent();
/// let depth = mounted.depth();
///
/// // Unmount from tree
/// let unmounted = mounted.unmount();
/// ```
pub struct RenderNode<S: NodeState> {
    // ========== Common Fields (both Mounted and Unmounted) ==========
    /// The type-erased RenderObject
    render_object: Box<dyn RenderObject>,

    /// Current lifecycle state
    lifecycle: RenderLifecycle,

    /// Associated ElementId (for cross-tree references)
    element_id: Option<ElementId>,

    // ========== Mounted-only Fields ==========
    /// Parent in the render tree (only valid when Mounted)
    parent: Option<RenderId>,

    /// Tree depth (only valid when Mounted)
    depth: Depth,

    /// Children in the render tree (only valid when Mounted)
    children: Vec<RenderId>,

    /// Cached size from last layout (only valid when Mounted)
    cached_size: Option<Size>,

    /// Whether this node is a relayout boundary (Flutter optimization)
    ///
    /// A relayout boundary isolates layout changes to its subtree.
    /// Determined by: !parent_uses_size || sized_by_parent || constraints.is_tight || parent == null
    ///
    /// When true, `mark_needs_layout()` stops propagating up the tree.
    is_relayout_boundary: bool,

    /// Whether this node or its subtree needs compositing (Flutter protocol)
    ///
    /// A node needs compositing if:
    /// - It's a repaint boundary (`is_repaint_boundary()`)
    /// - It always needs compositing (`always_needs_compositing()`)
    /// - Any of its children need compositing
    ///
    /// This is updated during `flush_compositing_bits()` phase and determines
    /// whether a compositing layer should be created for this node.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// bool _needsCompositing = false;
    /// bool get needsCompositing => _needsCompositing;
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if node.needs_compositing() {
    ///     // This node needs its own compositing layer
    ///     painting_context.push_layer(layer);
    /// }
    /// ```
    needs_compositing: bool,

    /// Compositing layer handle (only for repaint boundaries)
    ///
    /// Repaint boundaries create their own compositing layer to cache paint results.
    /// This field stores the layer handle when `is_repaint_boundary()` returns true.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// LayerHandle<ContainerLayer> _layerHandle = LayerHandle<ContainerLayer>();
    /// ```
    layer_handle: Option<LayerHandle>,

    /// Parent-specific data set by parent via setup_parent_data()
    ///
    /// Different parent types store different data on their children:
    /// - Stack stores offset
    /// - Flex stores flex factor
    /// - etc.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// ParentData? parentData;
    /// ```
    parent_data: Option<Box<dyn ParentData>>,

    // ========== Typestate Marker ==========
    /// Zero-sized marker for compile-time state tracking
    _state: PhantomData<S>,
}

// ============================================================================
// UNMOUNTED NODE IMPLEMENTATION
// ============================================================================

impl RenderNode<Unmounted> {
    /// Creates a new unmounted RenderNode.
    ///
    /// # Arguments
    ///
    /// * `object` - The render object to wrap
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let node = RenderNode::new(my_render_object);
    /// assert!(node.lifecycle() == RenderLifecycle::Detached);
    /// ```
    pub fn new<R: RenderObject + 'static>(object: R) -> Self {
        Self {
            render_object: Box::new(object),
            lifecycle: RenderLifecycle::Detached,
            element_id: None,
            parent: None,
            depth: Depth::root(),
            children: Vec::new(),
            cached_size: None,
            is_relayout_boundary: false,
            needs_compositing: false,
            layer_handle: None,
            parent_data: None,
            _state: PhantomData,
        }
    }

    /// Creates an unmounted RenderNode from a boxed RenderObject.
    ///
    /// Useful when the concrete type is already erased.
    pub fn from_boxed(render_object: Box<dyn RenderObject>) -> Self {
        Self {
            render_object,
            lifecycle: RenderLifecycle::Detached,
            element_id: None,
            parent: None,
            depth: Depth::root(),
            children: Vec::new(),
            cached_size: None,
            is_relayout_boundary: false,
            needs_compositing: false,
            layer_handle: None,
            parent_data: None,
            _state: PhantomData,
        }
    }

    /// Attaches an ElementId to this unmounted node (builder pattern).
    pub fn with_element_id(mut self, element_id: ElementId) -> Self {
        self.element_id = Some(element_id);
        self
    }
}

// ============================================================================
// MOUNTED NODE IMPLEMENTATION
// ============================================================================

impl RenderNode<Mounted> {
    // ========== Tree Navigation (only available when Mounted) ==========

    /// Gets the parent RenderId.
    ///
    /// Returns `None` if this is the root node.
    #[inline]
    pub fn parent(&self) -> Option<RenderId> {
        self.parent
    }

    /// Gets the tree depth.
    ///
    /// Root nodes have depth 0.
    #[inline]
    pub fn depth(&self) -> Depth {
        self.depth
    }

    /// Checks if this is the root node.
    #[inline]
    pub fn is_root(&self) -> bool {
        self.parent.is_none()
    }

    /// Gets all children RenderIds.
    #[inline]
    pub fn children(&self) -> &[RenderId] {
        &self.children
    }

    /// Returns the number of children.
    #[inline]
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Checks if this node has any children.
    #[inline]
    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    /// Checks if this is a leaf node (no children).
    #[inline]
    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }

    // ========== Tree Mutations (Rust naming: snake_case) ==========

    /// Sets the depth of this node.
    ///
    /// **Internal use only**. Called by `RenderTree::redepth_child()`.
    #[inline]
    pub(crate) fn set_depth(&mut self, depth: Depth) {
        self.depth = depth;
    }

    /// Adds a child to this render node.
    ///
    /// **Note**: Does not update child's parent. Use `RenderTree::add_child` for that.
    #[inline]
    pub(crate) fn add_child(&mut self, child: RenderId) {
        self.children.push(child);
    }

    /// Removes a child from this render node.
    ///
    /// **Note**: Does not update child's parent. Use `RenderTree::remove_child` for that.
    #[inline]
    pub(crate) fn remove_child(&mut self, child: RenderId) {
        self.children.retain(|&id| id != child);
    }

    /// Sets the parent RenderId (internal use only).
    #[inline]
    pub(crate) fn set_parent(&mut self, parent: Option<RenderId>) {
        self.parent = parent;
    }

    // ========== Layout Cache ==========

    /// Gets the cached size from last layout.
    #[inline]
    pub fn cached_size(&self) -> Option<Size> {
        self.cached_size
    }

    /// Sets the cached size.
    #[inline]
    pub fn set_cached_size(&mut self, size: Option<Size>) {
        self.cached_size = size;
    }

    // ========== Relayout Boundary (Flutter Protocol) ==========

    /// Returns whether this node is a relayout boundary.
    ///
    /// A relayout boundary isolates layout changes to its subtree.
    /// When a child of a relayout boundary marks itself as needing layout,
    /// the invalidation stops at the boundary instead of propagating to the root.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// bool get isRelayoutBoundary => _isRelayoutBoundary ?? false;
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Root nodes are always relayout boundaries
    /// if node.is_root() {
    ///     assert!(node.is_relayout_boundary());
    /// }
    ///
    /// // Other nodes depend on layout constraints and properties
    /// if node.is_relayout_boundary() {
    ///     // Layout changes won't propagate to parent
    /// }
    /// ```
    #[inline]
    pub fn is_relayout_boundary(&self) -> bool {
        self.is_relayout_boundary
    }

    /// Sets whether this node is a relayout boundary.
    ///
    /// This is typically computed during layout based on:
    /// - Whether parent uses this node's size
    /// - Whether node is sized by parent constraints only
    /// - Whether constraints are tight
    /// - Whether this is the root node
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Compute boundary status
    /// let is_boundary = !parent_uses_size
    ///     || render_object.sized_by_parent()
    ///     || constraints.is_tight()
    ///     || node.is_root();
    ///
    /// node.set_relayout_boundary(is_boundary);
    /// ```
    #[inline]
    pub fn set_relayout_boundary(&mut self, is_boundary: bool) {
        self.is_relayout_boundary = is_boundary;
    }

    /// Computes whether this node should be a relayout boundary (Flutter protocol).
    ///
    /// A node becomes a relayout boundary when ANY of these conditions is true:
    /// - `!parent_uses_size` - Parent doesn't use child's computed size
    /// - `sized_by_parent` - Size depends only on constraints, not children
    /// - `constraints.is_tight()` - Constraints specify exact size
    /// - `is_root` - Root of render tree
    ///
    /// When a node is a relayout boundary, `mark_needs_layout()` stops propagating
    /// up the tree, limiting the scope of relayout work.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void layout(Constraints constraints, {bool parentUsesSize = false}) {
    ///   final bool isRelayoutBoundary = !parentUsesSize ||
    ///                                    sizedByParent ||
    ///                                    constraints.isTight ||
    ///                                    parent == null;
    ///   // ...
    /// }
    /// ```
    ///
    /// # Arguments
    ///
    /// * `parent_uses_size` - Whether parent reads child's size after layout
    /// * `constraints` - Box constraints passed to child
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_types::constraints::BoxConstraints;
    /// use flui_types::Size;
    ///
    /// // During layout, compute boundary status
    /// let parent_uses_size = true;
    /// let constraints = BoxConstraints::tight(Size::new(100.0, 50.0));
    ///
    /// let is_boundary = node.compute_relayout_boundary(parent_uses_size, &constraints);
    /// assert!(is_boundary); // true because constraints are tight
    ///
    /// node.set_relayout_boundary(is_boundary);
    /// ```
    pub fn compute_relayout_boundary(
        &self,
        parent_uses_size: bool,
        constraints: &flui_types::constraints::BoxConstraints,
    ) -> bool {
        // Root nodes are always relayout boundaries
        if self.parent.is_none() {
            return true;
        }

        // Apply Flutter protocol conditions
        !parent_uses_size
            || self.render_object.sized_by_parent()
            || constraints.is_tight()
    }

    // ========== Compositing Bits (Flutter Protocol) ==========

    /// Returns whether this node or its subtree needs compositing.
    ///
    /// A node needs compositing if:
    /// - It's a repaint boundary
    /// - It always needs compositing (e.g., video, platform views)
    /// - Any of its children need compositing
    ///
    /// This value is computed during `update_compositing_bits()` and cached
    /// until the next compositing bits update.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// bool get needsCompositing => _needsCompositing;
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if node.needs_compositing() {
    ///     // Create/update compositing layer
    ///     node.update_composited_layer();
    /// }
    /// ```
    #[inline]
    pub fn needs_compositing(&self) -> bool {
        self.needs_compositing
    }

    /// Sets whether this node or its subtree needs compositing.
    ///
    /// **Internal use only**. This is called by `update_compositing_bits()`
    /// during the compositing bits update phase.
    ///
    /// # Arguments
    ///
    /// * `needs_compositing` - Whether compositing is needed
    #[inline]
    pub(crate) fn set_needs_compositing(&mut self, needs_compositing: bool) {
        self.needs_compositing = needs_compositing;
    }

    // ========== Layer Handle (Flutter Protocol) ==========

    /// Returns the compositing layer handle if this node is a repaint boundary.
    ///
    /// Repaint boundaries create their own compositing layer to cache paint results.
    /// This isolates paint invalidation to the subtree.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// LayerHandle<ContainerLayer> get layerHandle => _layerHandle;
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if let Some(layer) = node.layer_handle() {
    ///     // This node has its own compositing layer
    ///     painting_context.paint_child_with_layer(child, layer);
    /// }
    /// ```
    #[inline]
    pub fn layer_handle(&self) -> Option<&LayerHandle> {
        self.layer_handle.as_ref()
    }

    /// Returns a mutable reference to the layer handle.
    #[inline]
    pub fn layer_handle_mut(&mut self) -> Option<&mut LayerHandle> {
        self.layer_handle.as_mut()
    }

    /// Sets the compositing layer handle.
    ///
    /// This is typically called during paint when a repaint boundary
    /// creates or updates its compositing layer.
    ///
    /// # Arguments
    ///
    /// * `handle` - The layer handle to set, or None to clear
    #[inline]
    pub fn set_layer_handle(&mut self, handle: Option<LayerHandle>) {
        self.layer_handle = handle;
    }

    /// Updates the composited layer for this repaint boundary.
    ///
    /// This method is called during the paint phase for nodes that are repaint boundaries.
    /// It ensures the layer handle is created if needed and updates the layer's properties.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void updateCompositedLayer({required Offset offset}) {
    ///   final ContainerLayer? oldLayer = _layerHandle.layer;
    ///
    ///   final OffsetLayer newLayer = updateCompositedLayerProperties(
    ///     oldLayer: oldLayer,
    ///     offset: offset,
    ///   );
    ///
    ///   _layerHandle.layer = newLayer;
    /// }
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // During paint phase for repaint boundary
    /// if node.render_object().is_repaint_boundary() {
    ///     node.update_composited_layer();
    /// }
    /// ```
    pub fn update_composited_layer(&mut self) {
        // Flutter protocol: Only repaint boundaries get composited layers
        if !self.render_object.is_repaint_boundary() {
            // Clear layer if node is no longer a repaint boundary
            if self.layer_handle.is_some() {
                self.layer_handle = None;
            }
            return;
        }

        // Create or reuse layer (Flutter pattern: reuse existing layer for performance)
        if self.layer_handle.is_none() {
            // Create new OffsetLayer for repaint boundary
            // OffsetLayer is the standard layer type for repaint boundaries in Flutter
            let mut handle = flui_layer::LayerHandle::new();
            let offset_layer = flui_layer::OffsetLayer::from_xy(0.0, 0.0);
            handle.set(flui_layer::Layer::Offset(offset_layer));

            self.layer_handle = Some(handle);

            tracing::trace!(
                render_object = self.render_object.debug_name(),
                "Created composited OffsetLayer for repaint boundary"
            );
        }
        // Layer reuse: existing layer is kept for next frame (Flutter optimization)
        // The layer will be updated during paint phase with new offset if needed
    }

    // ========== Parent Data (Flutter Protocol) ==========

    /// Returns the parent-specific data set by the parent container.
    ///
    /// Different parent types store different data on their children:
    /// - Stack stores offset position
    /// - Flex stores flex factor and fit
    /// - Table stores row/column indices
    /// - etc.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// ParentData? get parentData => _parentData;
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if let Some(parent_data) = node.parent_data() {
    ///     // Access parent-specific data
    ///     if let Some(stack_data) = parent_data.downcast_ref::<StackParentData>() {
    ///         let offset = stack_data.offset();
    ///     }
    /// }
    /// ```
    #[inline]
    pub fn parent_data(&self) -> Option<&dyn ParentData> {
        self.parent_data.as_ref().map(|boxed| &**boxed)
    }

    /// Returns a mutable reference to the parent data.
    #[inline]
    pub fn parent_data_mut(&mut self) -> Option<&mut dyn ParentData> {
        self.parent_data.as_mut().map(|boxed| &mut **boxed)
    }

    /// Sets the parent data.
    ///
    /// This is typically called by the parent's `setup_parent_data()` method
    /// when a child is added to ensure the child has the correct parent data type.
    ///
    /// # Arguments
    ///
    /// * `data` - The parent data to set, or None to clear
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Parent sets up child's parent data
    /// child.set_parent_data(Some(Box::new(StackParentData::default())));
    /// ```
    #[inline]
    pub fn set_parent_data(&mut self, data: Option<Box<dyn ParentData>>) {
        self.parent_data = data;
    }
}

// ============================================================================
// COMMON IMPLEMENTATION (for all states)
// ============================================================================

impl<S: NodeState> RenderNode<S> {
    // ========== RenderObject Access ==========

    /// Returns reference to the RenderObject.
    #[inline]
    pub fn render_object(&self) -> &dyn RenderObject {
        &*self.render_object
    }

    /// Returns mutable reference to the RenderObject.
    #[inline]
    pub fn render_object_mut(&mut self) -> &mut dyn RenderObject {
        &mut *self.render_object
    }

    // ========== Lifecycle ==========

    /// Gets the current lifecycle state.
    #[inline]
    pub fn lifecycle(&self) -> RenderLifecycle {
        self.lifecycle
    }

    /// Sets the lifecycle state.
    #[inline]
    pub fn set_lifecycle(&mut self, lifecycle: RenderLifecycle) {
        self.lifecycle = lifecycle;
    }

    // ========== Element Association ==========

    /// Gets the associated ElementId.
    #[inline]
    pub fn element_id(&self) -> Option<ElementId> {
        self.element_id
    }

    /// Sets the associated ElementId.
    #[inline]
    pub fn set_element_id(&mut self, element_id: Option<ElementId>) {
        self.element_id = element_id;
    }
}

// ============================================================================
// DEBUG IMPLEMENTATION
// ============================================================================

impl<S: NodeState> std::fmt::Debug for RenderNode<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(&format!("RenderNode<{}>", S::name()))
            .field("lifecycle", &self.lifecycle)
            .field("element_id", &self.element_id)
            .field("render_object", &self.render_object.debug_name())
            .finish_non_exhaustive()
    }
}

// ============================================================================
// MOUNTABLE / UNMOUNTABLE TRAITS
// ============================================================================

impl Mountable for RenderNode<Unmounted> {
    type Id = RenderId;
    type Mounted = RenderNode<Mounted>;

    fn mount(self, parent: Option<RenderId>, parent_depth: Depth) -> RenderNode<Mounted> {
        let depth = if parent.is_some() {
            parent_depth.child_depth()
        } else {
            Depth::root()
        };

        // Root nodes are always relayout boundaries
        let is_relayout_boundary = parent.is_none();

        RenderNode {
            render_object: self.render_object,
            lifecycle: RenderLifecycle::Attached,
            element_id: self.element_id,
            parent,
            depth,
            children: Vec::new(),
            cached_size: None,
            is_relayout_boundary,
            needs_compositing: false,
            layer_handle: None,
            parent_data: None,
            _state: PhantomData,
        }
    }
}

impl Unmountable for RenderNode<Mounted> {
    type Id = RenderId;
    type Unmounted = RenderNode<Unmounted>;

    fn parent(&self) -> Option<RenderId> {
        self.parent
    }

    fn depth(&self) -> Depth {
        self.depth
    }

    fn unmount(self) -> RenderNode<Unmounted> {
        RenderNode {
            render_object: self.render_object,
            lifecycle: RenderLifecycle::Detached,
            element_id: self.element_id,
            parent: None,
            depth: Depth::root(),
            children: Vec::new(),
            cached_size: None,
            is_relayout_boundary: false,
            needs_compositing: false,
            layer_handle: None,
            parent_data: None,
            _state: PhantomData,
        }
    }
}

// ============================================================================
// RENDER TREE (stores only Mounted nodes)
// ============================================================================

/// RenderTree - Advanced slab-based storage for mounted render nodes.
///
/// This is the third of FLUI's four trees, storing mounted RenderObjects
/// using the typestate pattern for compile-time safety.
///
/// # Type Safety
///
/// RenderTree stores only `RenderNode<Mounted>`, ensuring all nodes have:
/// - Valid parent references
/// - Correct tree depth
/// - Proper children tracking
///
/// Unmounted nodes cannot be inserted directly - use `mount()` first.
///
/// # Thread Safety
///
/// RenderTree itself is not thread-safe. Use `Arc<RwLock<RenderTree>>`
/// or `parking_lot::RwLock<RenderTree>` for multi-threaded access.
///
/// # Examples
///
/// ```rust,ignore
/// let mut tree = RenderTree::new();
///
/// // Mount node as root
/// let unmounted = RenderNode::new(render_object);
/// let mounted = unmounted.mount_root();
/// let root_id = tree.insert(mounted);
///
/// // Mount child
/// let child_unmounted = RenderNode::new(child_object);
/// let child_mounted = child_unmounted.mount_child(root_id, tree.get(root_id).unwrap().depth());
/// let child_id = tree.insert(child_mounted);
///
/// // Add relationship
/// tree.add_child(root_id, child_id);
/// ```
#[derive(Debug)]
pub struct RenderTree {
    /// Slab storage for mounted RenderNodes (0-based indexing internally)
    nodes: Slab<RenderNode<Mounted>>,

    /// Root RenderNode ID (None if tree is empty)
    root: Option<RenderId>,

    /// Nodes needing layout (Flutter _nodesNeedingLayout)
    ///
    /// When `mark_needs_layout()` is called and a node is a relayout boundary,
    /// it's added to this set instead of propagating to parent.
    ///
    /// Processed during layout phase in depth order (parents before children).
    nodes_needing_layout: std::collections::HashSet<RenderId>,

    /// Nodes needing paint (Flutter _nodesNeedingPaint)
    ///
    /// When `mark_needs_paint()` is called and a node is a repaint boundary,
    /// it's added to this set instead of propagating to parent.
    ///
    /// Processed during paint phase.
    nodes_needing_paint: std::collections::HashSet<RenderId>,

    /// Nodes needing compositing bits update (Flutter _nodesNeedingCompositingBitsUpdate)
    ///
    /// When `mark_needs_compositing_bits_update()` is called, the node is added to this set.
    ///
    /// Processed before paint phase to determine which nodes need compositing layers.
    nodes_needing_compositing_bits_update: std::collections::HashSet<RenderId>,
}

impl RenderTree {
    /// Creates a new empty RenderTree.
    pub fn new() -> Self {
        Self {
            nodes: Slab::new(),
            root: None,
            nodes_needing_layout: std::collections::HashSet::new(),
            nodes_needing_paint: std::collections::HashSet::new(),
            nodes_needing_compositing_bits_update: std::collections::HashSet::new(),
        }
    }

    /// Creates a RenderTree with pre-allocated capacity.
    ///
    /// # Arguments
    ///
    /// * `capacity` - Initial capacity for the slab allocator
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: Slab::with_capacity(capacity),
            root: None,
            nodes_needing_layout: std::collections::HashSet::with_capacity(capacity / 4),
            nodes_needing_paint: std::collections::HashSet::with_capacity(capacity / 4),
            nodes_needing_compositing_bits_update: std::collections::HashSet::with_capacity(capacity / 8),
        }
    }

    // ========== Root Management ==========

    /// Gets the root RenderNode ID.
    #[inline]
    pub fn root(&self) -> Option<RenderId> {
        self.root
    }

    /// Sets the root RenderNode ID.
    #[inline]
    pub fn set_root(&mut self, root: Option<RenderId>) {
        self.root = root;
    }

    // ========== Basic Operations ==========

    /// Checks if a RenderNode exists in the tree.
    #[inline]
    pub fn contains(&self, id: RenderId) -> bool {
        self.nodes.contains(id.get() - 1)
    }

    /// Returns the number of RenderNodes in the tree.
    #[inline]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns `true` if the tree is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Returns the parent of a node.
    ///
    /// This is a convenience method that delegates to the node itself.
    #[inline]
    pub fn parent(&self, id: RenderId) -> Option<RenderId> {
        self.get(id).map(|node| node.parent()).flatten()
    }

    /// Inserts a mounted RenderNode into the tree.
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies +1 offset: `nodes.insert()` returns 0-based index → `RenderId(1)`
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let unmounted = RenderNode::new(render_object);
    /// let mounted = unmounted.mount_root();
    /// let id = tree.insert(mounted);
    /// ```
    pub fn insert(&mut self, node: RenderNode<Mounted>) -> RenderId {
        let slab_index = self.nodes.insert(node);
        RenderId::new(slab_index + 1)
    }

    /// Returns a reference to a mounted RenderNode.
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies -1 offset: `RenderId(1)` → `nodes[0]`
    #[inline]
    pub fn get(&self, id: RenderId) -> Option<&RenderNode<Mounted>> {
        self.nodes.get(id.get() - 1)
    }

    /// Returns a mutable reference to a mounted RenderNode.
    #[inline]
    pub fn get_mut(&mut self, id: RenderId) -> Option<&mut RenderNode<Mounted>> {
        self.nodes.get_mut(id.get() - 1)
    }

    /// Removes a RenderNode from the tree, returning the unmounted node.
    ///
    /// **Note:** This does NOT remove children. Caller must handle tree cleanup.
    ///
    /// The returned node is unmounted and can be re-mounted elsewhere.
    pub fn remove(&mut self, id: RenderId) -> Option<RenderNode<Mounted>> {
        if self.root == Some(id) {
            self.root = None;
        }
        self.nodes.try_remove(id.get() - 1)
    }

    /// Clears all nodes from the tree.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.root = None;
    }

    /// Reserves capacity for at least `additional` more nodes.
    pub fn reserve(&mut self, additional: usize) {
        self.nodes.reserve(additional);
    }

    // ========== Tree Mutations (Rust naming conventions) ==========

    /// Updates the depth of a child node and its descendants.
    ///
    /// This implements Flutter's `redepthChild()` protocol:
    /// - If child's depth <= parent's depth, update it to parent.depth + 1
    /// - Recursively update all descendants
    ///
    /// This ensures correct depth ordering for pipeline flush operations
    /// (layout shallowest first, paint deepest first).
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void redepthChild(RenderObject child) {
    ///   if (child._depth <= depth) {
    ///     child._depth = depth + 1;
    ///     child.redepthChildren();
    ///   }
    /// }
    /// ```
    ///
    /// # Arguments
    ///
    /// * `parent_id` - ID of the parent node
    /// * `child_id` - ID of the child node to redepth
    pub fn redepth_child(&mut self, parent_id: RenderId, child_id: RenderId) {
        // Get parent depth
        let parent_depth = if let Some(parent) = self.get(parent_id) {
            parent.depth()
        } else {
            return; // Parent not found
        };

        // Get child's current depth
        let child_depth = if let Some(child) = self.get(child_id) {
            child.depth()
        } else {
            return; // Child not found
        };

        // Only update if child depth <= parent depth (Flutter pattern)
        if child_depth <= parent_depth {
            let new_depth = parent_depth.child_depth();

            // Set child's new depth
            if let Some(child) = self.get_mut(child_id) {
                child.set_depth(new_depth);
            }

            // Recursively update all descendants
            self.redepth_children(child_id);
        }
    }

    /// Recursively updates the depth of all descendants of a node.
    ///
    /// Called by `redepth_child()` to ensure all descendants have correct depth.
    ///
    /// # Arguments
    ///
    /// * `node_id` - ID of the node whose children should be redepthed
    fn redepth_children(&mut self, node_id: RenderId) {
        // Collect children IDs first to avoid borrow checker issues
        let children: Vec<RenderId> = self
            .get(node_id)
            .map(|node| node.children().to_vec())
            .unwrap_or_default();

        // Recursively update each child
        for child_id in children {
            self.redepth_child(node_id, child_id);
        }
    }

    /// Adds a child to a parent RenderNode.
    ///
    /// This implements Flutter's `adoptChild()` protocol:
    /// 1. Setup parent data for the child
    /// 2. Call parent's `adopt_child()` hook
    /// 3. Update parent/child relationships
    /// 4. Call child's `attach()` hook
    /// 5. Update child's depth (`redepth_child()`)
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void adoptChild(RenderObject child) {
    ///   setupParentData(child);
    ///   markNeedsLayout();
    ///   markNeedsCompositingBitsUpdate();
    ///   child._parent = this;
    ///   if (attached) child.attach(_owner!);
    ///   redepthChild(child);
    /// }
    /// ```
    ///
    /// # Arguments
    ///
    /// * `parent_id` - ID of the parent node
    /// * `child_id` - ID of the child node to add
    pub fn add_child(&mut self, parent_id: RenderId, child_id: RenderId) {
        if parent_id == child_id {
            return; // Cannot add node as its own child
        }

        unsafe {
            // SAFETY: We ensure parent_id != child_id above
            let tree_ptr = self as *mut Self;

            // Step 1: Setup parent data
            if let (Some(parent), Some(child)) = ((*tree_ptr).get(parent_id), (*tree_ptr).get_mut(child_id)) {
                let current_parent_data = child.parent_data();
                if let Some(new_parent_data) = parent.render_object().setup_parent_data(current_parent_data) {
                    child.set_parent_data(Some(new_parent_data));
                }
            }

            // Step 2: Call parent's adopt_child() hook
            if let Some(parent) = (*tree_ptr).get_mut(parent_id) {
                parent.render_object_mut().adopt_child(child_id);
                parent.add_child(child_id);
            }

            // Step 3: Update child's parent reference
            if let Some(child) = (*tree_ptr).get_mut(child_id) {
                child.set_parent(Some(parent_id));
            }

            // Step 4: Call child's attach() hook
            if let Some(child) = (*tree_ptr).get_mut(child_id) {
                child.render_object_mut().attach();
            }

            // Step 5: Update child depth
            self.redepth_child(parent_id, child_id);
        }
    }

    /// Removes a child from a parent RenderNode.
    ///
    /// This implements Flutter's `dropChild()` protocol:
    /// 1. Call parent's `drop_child()` hook
    /// 2. Update parent/child relationships
    /// 3. Clear child's parent data
    /// 4. Call child's `detach()` hook
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void dropChild(RenderObject child) {
    ///   child.parentData!.detach();
    ///   child.parentData = null;
    ///   child._parent = null;
    ///   if (attached) child.detach();
    ///   markNeedsLayout();
    ///   markNeedsCompositingBitsUpdate();
    /// }
    /// ```
    ///
    /// # Arguments
    ///
    /// * `parent_id` - ID of the parent node
    /// * `child_id` - ID of the child node to remove
    pub fn remove_child(&mut self, parent_id: RenderId, child_id: RenderId) {
        if parent_id == child_id {
            return; // Cannot remove node from itself
        }

        unsafe {
            // SAFETY: We ensure parent_id != child_id above
            let tree_ptr = self as *mut Self;

            // Step 1: Call parent's drop_child() hook
            if let Some(parent) = (*tree_ptr).get_mut(parent_id) {
                parent.render_object_mut().drop_child(child_id);
                parent.remove_child(child_id);
            }

            // Step 2: Clear child's parent reference
            // Step 3: Clear child's parent data
            if let Some(child) = (*tree_ptr).get_mut(child_id) {
                child.set_parent(None);
                child.set_parent_data(None);
            }

            // Step 4: Call child's detach() hook
            if let Some(child) = (*tree_ptr).get_mut(child_id) {
                child.render_object_mut().detach();
            }

            // Step 5: Clear layer handle to release GPU resources (Flutter protocol)
            if let Some(child) = (*tree_ptr).get_mut(child_id) {
                if child.layer_handle.is_some() {
                    child.layer_handle = None;
                    tracing::trace!(?child_id, "Cleared layer handle on detach");
                }
            }
        }
    }

    // ========== Compositing Bits Update (Flutter Protocol) ==========

    /// Updates the compositing bits for a node and its subtree.
    ///
    /// This implements Flutter's `_updateCompositingBits()` logic:
    /// 1. Recursively update children's compositing bits first (bottom-up)
    /// 2. Set `needs_compositing = true` if:
    ///    - This is a repaint boundary
    ///    - Render object always needs compositing
    ///    - Any child needs compositing
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void _updateCompositingBits() {
    ///   bool oldNeedsCompositing = _needsCompositing;
    ///   _needsCompositing = false;
    ///
    ///   visitChildren((RenderObject child) {
    ///     child._updateCompositingBits();
    ///     if (child.needsCompositing)
    ///       _needsCompositing = true;
    ///   });
    ///
    ///   if (isRepaintBoundary || alwaysNeedsCompositing)
    ///     _needsCompositing = true;
    ///
    ///   if (oldNeedsCompositing != _needsCompositing)
    ///     markNeedsPaint();
    ///
    ///   _needsCompositingBitsUpdate = false;
    /// }
    /// ```
    ///
    /// # Algorithm
    ///
    /// 1. Collect children IDs
    /// 2. Recursively update each child
    /// 3. Check if any child needs compositing
    /// 4. Set own needs_compositing based on repaint boundary + children
    ///
    /// # Returns
    ///
    /// Returns `true` if the node's `needs_compositing` changed.
    pub fn update_compositing_bits(&mut self, id: RenderId) -> bool {
        // Collect children IDs first (to avoid borrow checker issues)
        let children: Vec<RenderId> = self
            .get(id)
            .map(|node| node.children().to_vec())
            .unwrap_or_default();

        // Recursively update children first (bottom-up traversal)
        for child_id in &children {
            self.update_compositing_bits(*child_id);
        }

        // Check if any child needs compositing (before borrowing node mutably)
        let mut any_child_needs_compositing = false;
        for child_id in &children {
            if let Some(child) = self.get(*child_id) {
                if child.needs_compositing() {
                    any_child_needs_compositing = true;
                    break;
                }
            }
        }

        // Now update this node's compositing bits
        if let Some(node) = self.get_mut(id) {
            let old_needs_compositing = node.needs_compositing();

            // Check if this node is a repaint boundary or always needs compositing
            let render_obj = node.render_object();
            let self_needs_compositing =
                render_obj.is_repaint_boundary() || render_obj.always_needs_compositing();

            // Combine: needs compositing if this node or any child needs it
            let new_needs_compositing = self_needs_compositing || any_child_needs_compositing;

            // Update the flag
            node.set_needs_compositing(new_needs_compositing);

            // Return true if changed
            old_needs_compositing != new_needs_compositing
        } else {
            false
        }
    }

    // ========== Transform Operations (Flutter Protocol) ==========

    /// Gets the transform from a render object to an ancestor.
    ///
    /// This implements Flutter's `getTransformTo()` algorithm:
    /// 1. Build path from `from_id` to `to_id` by following parent chain
    /// 2. Accumulate transforms by traversing path backward
    /// 3. Call `apply_paint_transform()` on each parent
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// Matrix4 getTransformTo(RenderObject? ancestor) {
    ///   final path = <RenderObject>[];
    ///   for (RenderObject? node = this; node != ancestor; node = node.parent) {
    ///     assert(node != null);
    ///     path.add(node!);
    ///   }
    ///
    ///   final transform = Matrix4.identity();
    ///   for (int i = path.length - 1; i >= 1; i--) {
    ///     path[i].applyPaintTransform(path[i - 1], transform);
    ///   }
    ///
    ///   return transform;
    /// }
    /// ```
    ///
    /// # Arguments
    ///
    /// * `from_id` - Starting render object ID
    /// * `to_id` - Ancestor render object ID (or `None` for root)
    ///
    /// # Returns
    ///
    /// Returns `Some(Matrix4)` with accumulated transform, or `None` if:
    /// - `from_id` doesn't exist
    /// - `to_id` doesn't exist (when specified)
    /// - `to_id` is not an ancestor of `from_id`
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Get transform from child to parent
    /// let transform = tree.get_transform_to(child_id, Some(parent_id))?;
    ///
    /// // Get transform from node to root
    /// let transform = tree.get_transform_to(node_id, None)?;
    /// ```
    pub fn get_transform_to(
        &self,
        from_id: RenderId,
        to_id: Option<RenderId>,
    ) -> Option<Matrix4> {
        // Build path from 'from' to 'to' by following parent chain
        let mut path = Vec::new();
        let mut current = from_id;

        loop {
            path.push(current);

            // Reached target ancestor?
            if Some(current) == to_id {
                break;
            }

            // Get parent and continue up
            let node = self.get(current)?;
            match node.parent() {
                Some(parent) => current = parent,
                None => {
                    // Reached root - only valid if to_id is None
                    if to_id.is_none() {
                        break;
                    } else {
                        // to_id is not an ancestor of from_id
                        return None;
                    }
                }
            }
        }

        // Build transform by traversing path backward
        // Start with identity, then accumulate each parent's transform
        let mut transform = Matrix4::identity();

        for i in (1..path.len()).rev() {
            let child_id = path[i - 1];

            // Apply default transform (translation by offset from parent data)
            // Note: Custom transforms (rotation, scale) would be handled by
            // RenderObject::apply_paint_transform(), but that requires HitTestTree
            // which RenderTree doesn't implement. For now, we just apply translation.
            if let Some(child) = self.get(child_id) {
                if let Some(parent_data) = child.parent_data() {
                    // Try to downcast to ParentDataWithOffset
                    use crate::parent_data::ParentDataWithOffset;
                    if let Some(offset_data) = parent_data.as_any().downcast_ref::<crate::parent_data::BoxParentData>() {
                        let offset = offset_data.offset();
                        transform = Matrix4::translation(offset.dx, offset.dy, 0.0) * transform;
                    } else if let Some(offset_data) = parent_data.as_any().downcast_ref::<crate::parent_data::ContainerBoxParentData<RenderId>>() {
                        let offset = offset_data.offset();
                        transform = Matrix4::translation(offset.dx, offset.dy, 0.0) * transform;
                    }
                }
            }
        }

        Some(transform)
    }

    /// Converts a point from one render object's coordinate space to another's.
    ///
    /// # Arguments
    ///
    /// * `from_id` - Source render object ID
    /// * `to_id` - Target render object ID (or `None` for root)
    /// * `point` - Point in source coordinate space
    ///
    /// # Returns
    ///
    /// Returns `Some(Offset)` with transformed point, or `None` if transform fails.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Transform point from child to parent coordinates
    /// let parent_point = tree.transform_point(child_id, Some(parent_id), child_point)?;
    /// ```
    pub fn transform_point(
        &self,
        from_id: RenderId,
        to_id: Option<RenderId>,
        point: Offset,
    ) -> Option<Offset> {
        let transform = self.get_transform_to(from_id, to_id)?;
        let (x, y) = transform.transform_point(point.dx, point.dy);
        Some(Offset::new(x, y))
    }

    /// Converts a point from global (root) coordinates to a render object's local coordinates.
    ///
    /// # Arguments
    ///
    /// * `id` - Render object ID
    /// * `global_point` - Point in global (root) coordinate space
    ///
    /// # Returns
    ///
    /// Returns `Some(Offset)` with point in local coordinates, or `None` if:
    /// - Node doesn't exist
    /// - Transform is not invertible (e.g., zero scale)
    pub fn global_to_local(&self, id: RenderId, global_point: Offset) -> Option<Offset> {
        let transform = self.get_transform_to(id, None)?;
        let inverse = transform.try_inverse()?;
        let (x, y) = inverse.transform_point(global_point.dx, global_point.dy);
        Some(Offset::new(x, y))
    }

    /// Converts a point from a render object's local coordinates to global (root) coordinates.
    ///
    /// # Arguments
    ///
    /// * `id` - Render object ID
    /// * `local_point` - Point in local coordinate space
    ///
    /// # Returns
    ///
    /// Returns `Some(Offset)` with point in global coordinates, or `None` if node doesn't exist.
    pub fn local_to_global(&self, id: RenderId, local_point: Offset) -> Option<Offset> {
        self.transform_point(id, None, local_point)
    }

    // ========== Iteration ==========

    /// Returns an iterator over slab entries (slab_index, node).
    ///
    /// **Note**: The index is the internal 0-based slab index, NOT RenderId.
    /// To get RenderId, use `RenderId::new(index + 1)`.
    #[inline]
    pub fn iter_slab(&self) -> slab::Iter<'_, RenderNode<Mounted>> {
        self.nodes.iter()
    }

    /// Returns a mutable iterator over slab entries.
    ///
    /// **Note**: The index is the internal 0-based slab index, NOT RenderId.
    #[inline]
    pub fn iter_slab_mut(&mut self) -> slab::IterMut<'_, RenderNode<Mounted>> {
        self.nodes.iter_mut()
    }

    // ========== HRTB-based Visitors (Advanced Pattern) ==========

    /// Visits all render objects with a closure using HRTB.
    ///
    /// This method demonstrates Higher-Rank Trait Bounds for flexible visitor patterns.
    ///
    /// # Arguments
    ///
    /// * `visitor` - Closure called for each (id, render_object) pair
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// tree.visit_all(|id, obj| {
    ///     println!("{}: {}", id, obj.debug_name());
    /// });
    /// ```
    pub fn visit_all<F>(&self, mut visitor: F)
    where
        F: for<'a> FnMut(RenderId, &'a dyn RenderObject),
    {
        for (slab_idx, node) in self.nodes.iter() {
            let id = RenderId::new(slab_idx + 1);
            visitor(id, node.render_object());
        }
    }

    /// Finds the first render object matching a predicate using HRTB.
    ///
    /// # Arguments
    ///
    /// * `predicate` - HRTB predicate function
    ///
    /// # Returns
    ///
    /// `Some(RenderId)` if found, `None` otherwise.
    pub fn find_where<P>(&self, mut predicate: P) -> Option<RenderId>
    where
        P: for<'a> FnMut(&'a dyn RenderObject) -> bool,
    {
        for (slab_idx, node) in self.nodes.iter() {
            if predicate(node.render_object()) {
                return Some(RenderId::new(slab_idx + 1));
            }
        }
        None
    }

    // ========== Dirty Flag Management (Flutter Protocol) ==========

    /// Marks a node as needing layout with boundary-aware propagation (Flutter markNeedsLayout).
    ///
    /// This implements Flutter's relayout boundary optimization:
    /// - If the node is NOT a relayout boundary, propagate to parent recursively
    /// - If the node IS a relayout boundary, add to dirty list and stop propagation
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void markNeedsLayout() {
    ///   if (_needsLayout) return;
    ///
    ///   if (_relayoutBoundary != this) {
    ///     markParentNeedsLayout();
    ///   } else {
    ///     _needsLayout = true;
    ///     if (owner != null) {
    ///       owner!._nodesNeedingLayout.add(this);
    ///       owner!.requestVisualUpdate();
    ///     }
    ///   }
    /// }
    /// ```
    ///
    /// # Arguments
    ///
    /// * `id` - Node ID to mark as needing layout
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Mark node as needing layout
    /// tree.mark_needs_layout(node_id);
    ///
    /// // If node is relayout boundary, propagation stops here
    /// assert!(tree.nodes_needing_layout.contains(&node_id));
    ///
    /// // If not boundary, parent is marked instead (recursively)
    /// ```
    pub fn mark_needs_layout(&mut self, id: RenderId) {
        // Get the node to check if already dirty and if it's a boundary
        let node = match self.get(id) {
            Some(node) => node,
            None => return, // Node doesn't exist
        };

        // Check if already needs layout (early exit optimization)
        if node.lifecycle().in_needs_layout_phase() {
            return;
        }

        let is_boundary = node.is_relayout_boundary();
        let parent = node.parent();

        // Mark lifecycle as needing layout
        if let Some(node) = self.get_mut(id) {
            let mut lifecycle = node.lifecycle();
            lifecycle.mark_needs_layout();
            node.set_lifecycle(lifecycle);
        }

        // Apply boundary-aware propagation
        if is_boundary {
            // We ARE the relayout boundary - add to dirty list
            self.nodes_needing_layout.insert(id);
            // Note: requestVisualUpdate() would go here in full implementation
        } else if let Some(parent_id) = parent {
            // NOT a boundary - propagate to parent recursively
            self.mark_needs_layout(parent_id);
        } else {
            // Root node with no boundary - add to dirty list
            self.nodes_needing_layout.insert(id);
        }
    }

    /// Marks a node as needing paint with repaint boundary support (Flutter markNeedsPaint).
    ///
    /// This implements Flutter's repaint boundary optimization:
    /// - If the node is NOT a repaint boundary, propagate to parent recursively
    /// - If the node IS a repaint boundary, add to dirty list and stop propagation
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void markNeedsPaint() {
    ///   if (_needsPaint) return;
    ///
    ///   _needsPaint = true;
    ///
    ///   if (isRepaintBoundary) {
    ///     if (owner != null) {
    ///       owner!._nodesNeedingPaint.add(this);
    ///       owner!.requestVisualUpdate();
    ///     }
    ///   } else if (parent is RenderObject) {
    ///     parent.markNeedsPaint();
    ///   } else {
    ///     if (owner != null) {
    ///       owner!.requestVisualUpdate();
    ///     }
    ///   }
    /// }
    /// ```
    ///
    /// # Arguments
    ///
    /// * `id` - Node ID to mark as needing paint
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Mark node as needing paint
    /// tree.mark_needs_paint(node_id);
    ///
    /// // If node is repaint boundary, propagation stops here
    /// assert!(tree.nodes_needing_paint.contains(&node_id));
    ///
    /// // If not boundary, parent is marked instead (recursively)
    /// ```
    pub fn mark_needs_paint(&mut self, id: RenderId) {
        // Get the node to check state
        let node = match self.get(id) {
            Some(node) => node,
            None => return, // Node doesn't exist
        };

        let lifecycle = node.lifecycle();
        let is_repaint_boundary = node.render_object().is_repaint_boundary();
        let parent = node.parent();

        // Check if already in NeedsPaint state AND already in dirty list (idempotent)
        if lifecycle == RenderLifecycle::NeedsPaint && (
            (is_repaint_boundary && self.nodes_needing_paint.contains(&id)) ||
            (!is_repaint_boundary && parent.is_some())
        ) {
            return; // Already marked, skip
        }

        // Mark lifecycle as needing paint
        if let Some(node) = self.get_mut(id) {
            let mut lifecycle = node.lifecycle();
            lifecycle.mark_needs_paint();
            node.set_lifecycle(lifecycle);
        }

        // Apply repaint boundary-aware propagation
        if is_repaint_boundary {
            // We ARE a repaint boundary - add to dirty list
            self.nodes_needing_paint.insert(id);
            // Note: requestVisualUpdate() would go here in full implementation
        } else if let Some(parent_id) = parent {
            // NOT a repaint boundary - propagate to parent recursively
            self.mark_needs_paint(parent_id);
        } else {
            // Root node - add to dirty list
            self.nodes_needing_paint.insert(id);
        }
    }

    /// Returns whether a node needs layout.
    #[inline]
    pub fn needs_layout(&self, id: RenderId) -> bool {
        self.get(id)
            .map(|node| node.lifecycle().in_needs_layout_phase())
            .unwrap_or(false)
    }

    /// Returns whether a node needs paint.
    #[inline]
    pub fn needs_paint(&self, id: RenderId) -> bool {
        self.get(id)
            .map(|node| node.lifecycle().in_needs_paint_phase())
            .unwrap_or(false)
    }

    /// Clears the dirty layout list (called after flushing layout).
    #[inline]
    pub fn clear_needs_layout_list(&mut self) {
        self.nodes_needing_layout.clear();
    }

    /// Clears the dirty paint list (called after flushing paint).
    #[inline]
    pub fn clear_needs_paint_list(&mut self) {
        self.nodes_needing_paint.clear();
    }

    /// Returns iterator over nodes needing layout (for flush_layout phase).
    #[inline]
    pub fn nodes_needing_layout(&self) -> impl Iterator<Item = RenderId> + '_ {
        self.nodes_needing_layout.iter().copied()
    }

    /// Returns iterator over nodes needing paint (for flush_paint phase).
    #[inline]
    pub fn nodes_needing_paint(&self) -> impl Iterator<Item = RenderId> + '_ {
        self.nodes_needing_paint.iter().copied()
    }
}

impl Default for RenderTree {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// TREE READ IMPLEMENTATION (Generic abstraction)
// ============================================================================

impl TreeRead<RenderId> for RenderTree {
    /// The node type is now `RenderNode<Mounted>` - only mounted nodes in the tree.
    type Node = RenderNode<Mounted>;

    /// Default capacity for render trees (tuned for typical UI hierarchies).
    const DEFAULT_CAPACITY: usize = 64;

    /// Threshold for inline vs heap allocation.
    const INLINE_THRESHOLD: usize = 16;

    #[inline]
    fn get(&self, id: RenderId) -> Option<&Self::Node> {
        RenderTree::get(self, id)
    }

    #[inline]
    fn contains(&self, id: RenderId) -> bool {
        RenderTree::contains(self, id)
    }

    #[inline]
    fn len(&self) -> usize {
        RenderTree::len(self)
    }

    #[inline]
    fn node_ids(&self) -> impl Iterator<Item = RenderId> + '_ {
        self.iter_slab().map(|(index, _)| RenderId::new(index + 1))
    }
}

// ============================================================================
// TREE WRITE IMPLEMENTATION (Mutable operations)
// ============================================================================

impl TreeWrite<RenderId> for RenderTree {
    #[inline]
    fn get_mut(&mut self, id: RenderId) -> Option<&mut Self::Node> {
        RenderTree::get_mut(self, id)
    }

    /// Inserts a mounted node into the tree.
    ///
    /// **Note**: Node must be in `Mounted` state. Use `node.mount()` first.
    #[inline]
    fn insert(&mut self, node: Self::Node) -> RenderId {
        RenderTree::insert(self, node)
    }

    /// Removes a node from the tree.
    ///
    /// Returns the mounted node (still in `Mounted` state).
    /// Call `.unmount()` on the result to transition to `Unmounted`.
    #[inline]
    fn remove(&mut self, id: RenderId) -> Option<Self::Node> {
        RenderTree::remove(self, id)
    }

    #[inline]
    fn clear(&mut self) {
        RenderTree::clear(self)
    }

    #[inline]
    fn reserve(&mut self, additional: usize) {
        RenderTree::reserve(self, additional)
    }
}

// ============================================================================
// TREE NAV IMPLEMENTATION (Navigation with RPITIT)
// ============================================================================

impl TreeNav<RenderId> for RenderTree {
    /// Maximum depth for render trees (typical UI hierarchies).
    const MAX_DEPTH: usize = 32;

    /// Average children per render node (used for optimization).
    const AVG_CHILDREN: usize = 4;

    #[inline]
    fn parent(&self, id: RenderId) -> Option<RenderId> {
        RenderTree::parent(self, id)
    }

    /// Returns iterator over children using RPITIT (zero-cost abstraction).
    #[inline]
    fn children(&self, id: RenderId) -> impl Iterator<Item = RenderId> + '_ {
        self.get(id)
            .map(|node| node.children().iter().copied())
            .into_iter()
            .flatten()
    }

    /// Returns iterator over ancestors using flui-tree's `Ancestors` iterator.
    #[inline]
    fn ancestors(&self, start: RenderId) -> impl Iterator<Item = RenderId> + '_ {
        Ancestors::new(self, start)
    }

    /// Returns iterator over descendants with depth tracking.
    #[inline]
    fn descendants(&self, root: RenderId) -> impl Iterator<Item = (RenderId, usize)> + '_ {
        DescendantsWithDepth::new(self, root)
    }

    /// Returns iterator over siblings (all children of parent except self).
    #[inline]
    fn siblings(&self, id: RenderId) -> impl Iterator<Item = RenderId> + '_ {
        AllSiblings::new(self, id)
    }

    #[inline]
    fn child_count(&self, id: RenderId) -> usize {
        self.get(id).map(|node| node.child_count()).unwrap_or(0)
    }

    #[inline]
    fn has_children(&self, id: RenderId) -> bool {
        self.get(id)
            .map(|node| node.has_children())
            .unwrap_or(false)
    }
}

// ============================================================================
// LAYOUT TREE IMPLEMENTATION (Phase 6 - Relayout Boundary Integration)
// ============================================================================

use crate::tree::{LayoutTree, PaintTree, HitTestTree};
use crate::error::RenderError;
use flui_types::{BoxConstraints, SliverConstraints, SliverGeometry};
use flui_interaction::HitTestResult;

impl LayoutTree for RenderTree {
    fn perform_layout(
        &mut self,
        id: RenderId,
        constraints: BoxConstraints,
        parent_uses_size: bool,
    ) -> Result<flui_types::Size, RenderError> {
        // Compute relayout boundary status (Flutter protocol - P0-2 Step 3)
        // This integrates the boundary computation with the layout flow
        let is_boundary = {
            let node = self.get(id).ok_or_else(|| RenderError::LayoutFailed {
                render_object: "RenderNode",
                reason: "Node not found".into(),
            })?;
            node.compute_relayout_boundary(parent_uses_size, &constraints)
        };

        // Set the computed boundary status
        if let Some(node) = self.get_mut(id) {
            node.set_relayout_boundary(is_boundary);
        }

        // TODO: Full layout implementation requires:
        // 1. Early return if clean and constraints unchanged
        // 2. Set constraints in state
        // 3. Call render object's layout method (requires downcasting infrastructure)
        // 4. Store geometry result
        // 5. Mark needs paint
        //
        // For now, return a placeholder to enable testing of boundary computation
        Ok(constraints.biggest())
    }

    fn perform_sliver_layout(
        &mut self,
        id: RenderId,
        constraints: SliverConstraints,
        parent_uses_size: bool,
    ) -> Result<SliverGeometry, RenderError> {
        // Compute relayout boundary status for slivers
        // Note: For slivers, similar logic applies (Flutter protocol)
        let is_boundary = {
            let node = self.get(id).ok_or_else(|| RenderError::LayoutFailed {
                render_object: "RenderNode",
                reason: "Node not found".into(),
            })?;
            // Sliver boundary: !parent_uses_size || sized_by_parent || !has_parent
            !parent_uses_size
                || node.render_object().sized_by_parent()
                || node.parent().is_none()
        };

        // Set the computed boundary status
        if let Some(node) = self.get_mut(id) {
            node.set_relayout_boundary(is_boundary);
        }

        // TODO: Full sliver layout implementation
        Ok(SliverGeometry::zero())
    }

    fn set_offset(&mut self, _id: RenderId, _offset: flui_types::Offset) {
        // TODO: Implement offset storage in parent_data
    }

    fn get_offset(&self, _id: RenderId) -> Option<flui_types::Offset> {
        // TODO: Implement offset retrieval from parent_data
        None
    }

    fn mark_needs_layout(&mut self, id: RenderId) {
        RenderTree::mark_needs_layout(self, id)
    }

    fn needs_layout(&self, id: RenderId) -> bool {
        RenderTree::needs_layout(self, id)
    }

    fn render_object(&self, id: RenderId) -> Option<&dyn std::any::Any> {
        self.get(id).map(|node| node.render_object().as_any())
    }

    fn render_object_mut(&mut self, id: RenderId) -> Option<&mut dyn std::any::Any> {
        self.get_mut(id).map(|node| node.render_object_mut().as_any_mut())
    }

    fn setup_child_parent_data(&mut self, _parent_id: RenderId, _child_id: RenderId) {
        // TODO: Implement parent data setup protocol
        // This will call parent.render_object().setup_parent_data(child.render_object())
    }
}

impl PaintTree for RenderTree {
    fn perform_paint(&mut self, _id: RenderId, _offset: flui_types::Offset) -> Result<flui_painting::Canvas, RenderError> {
        // TODO: Implement paint tree operations
        Ok(flui_painting::Canvas::new())
    }

    fn mark_needs_paint(&mut self, id: RenderId) {
        RenderTree::mark_needs_paint(self, id)
    }

    fn needs_paint(&self, id: RenderId) -> bool {
        RenderTree::needs_paint(self, id)
    }

    fn render_object(&self, id: RenderId) -> Option<&dyn std::any::Any> {
        self.get(id).map(|node| node.render_object().as_any())
    }

    fn render_object_mut(&mut self, id: RenderId) -> Option<&mut dyn std::any::Any> {
        self.get_mut(id).map(|node| node.render_object_mut().as_any_mut())
    }

    fn get_offset(&self, _id: RenderId) -> Option<flui_types::Offset> {
        // TODO: Implement offset retrieval from parent_data
        None
    }
}

impl HitTestTree for RenderTree {
    fn hit_test(
        &self,
        _id: RenderId,
        _position: flui_types::Offset,
        _result: &mut HitTestResult,
    ) -> bool {
        // TODO: Implement hit test operations
        false
    }

    fn render_object(&self, id: RenderId) -> Option<&dyn std::any::Any> {
        self.get(id).map(|node| node.render_object().as_any())
    }

    fn children(&self, id: RenderId) -> Box<dyn Iterator<Item = RenderId> + '_> {
        if let Some(node) = self.get(id) {
            Box::new(node.children().iter().copied())
        } else {
            Box::new(std::iter::empty())
        }
    }

    fn get_offset(&self, _id: RenderId) -> Option<flui_types::Offset> {
        // TODO: Implement offset retrieval from parent_data
        None
    }
}

// ============================================================================
// TESTS (Updated for Typestate Pattern)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RenderObject;
    use flui_tree::MountableExt; // For mount_root() extension
    use flui_types::{Matrix4, Offset};

    // Simple test RenderObject (minimal - no layout/paint methods needed)
    #[derive(Debug)]
    struct TestRenderObject {
        name: String,
    }

    impl RenderObject for TestRenderObject {
        fn debug_name(&self) -> &'static str {
            "TestRenderObject"
        }
    }

    // ===== Typestate Tests =====

    #[test]
    fn test_render_node_unmounted() {
        let obj = TestRenderObject {
            name: "test".into(),
        };
        let node = RenderNode::new(obj);

        // Unmounted node properties
        assert_eq!(node.lifecycle(), RenderLifecycle::Detached);
        assert!(node.element_id().is_none());

        // These methods are only available on Mounted nodes:
        // node.parent() - compile error!
        // node.depth() - compile error!
        // node.children() - compile error!
    }

    #[test]
    fn test_render_node_mount_root() {
        let obj = TestRenderObject {
            name: "root".into(),
        };
        let unmounted = RenderNode::new(obj);

        // Mount as root
        let mounted = unmounted.mount_root();

        // Now we can access tree properties
        assert!(mounted.is_root());
        assert_eq!(mounted.parent(), None);
        assert_eq!(mounted.depth(), Depth::root());
        assert!(mounted.children().is_empty());
        assert_eq!(mounted.lifecycle(), RenderLifecycle::Attached);
    }

    #[test]
    fn test_render_node_mount_child() {
        let parent_obj = TestRenderObject {
            name: "parent".into(),
        };
        let child_obj = TestRenderObject {
            name: "child".into(),
        };

        let parent_unmounted = RenderNode::new(parent_obj);
        let child_unmounted = RenderNode::new(child_obj);

        // Mount parent as root
        let parent_mounted = parent_unmounted.mount_root();
        let parent_id = RenderId::new(1);

        // Mount child under parent
        let child_mounted = child_unmounted.mount_child(parent_id, parent_mounted.depth());

        assert!(!child_mounted.is_root());
        assert_eq!(child_mounted.parent(), Some(parent_id));
        assert_eq!(child_mounted.depth(), Depth::new(1));
    }

    #[test]
    fn test_render_node_unmount() {
        let obj = TestRenderObject {
            name: "test".into(),
        };

        // Create -> Mount -> Unmount
        let unmounted = RenderNode::new(obj);
        let mounted = unmounted.mount_root();
        let back_to_unmounted = mounted.unmount();

        // Back to detached state
        assert_eq!(back_to_unmounted.lifecycle(), RenderLifecycle::Detached);
    }

    // ===== RenderTree Tests =====

    #[test]
    fn test_render_tree_insert() {
        let mut tree = RenderTree::new();

        let obj = TestRenderObject {
            name: "root".into(),
        };

        // Must mount before inserting
        let mounted = RenderNode::new(obj).mount_root();
        let id = tree.insert(mounted);

        assert_eq!(id.get(), 1);
        assert!(tree.contains(id));
        assert_eq!(tree.len(), 1);
    }

    #[test]
    fn test_render_tree_parent_child() {
        let mut tree = RenderTree::new();

        // Create and mount parent
        let parent_obj = TestRenderObject {
            name: "parent".into(),
        };
        let parent_mounted = RenderNode::new(parent_obj).mount_root();
        let parent_id = tree.insert(parent_mounted);

        // Create and mount child
        let child_obj = TestRenderObject {
            name: "child".into(),
        };
        let parent_depth = tree.get(parent_id).unwrap().depth();
        let child_mounted = RenderNode::new(child_obj).mount_child(parent_id, parent_depth);
        let child_id = tree.insert(child_mounted);

        // Establish relationship
        tree.add_child(parent_id, child_id);

        assert_eq!(tree.get(child_id).unwrap().parent(), Some(parent_id));
        assert_eq!(tree.get(parent_id).unwrap().children(), &[child_id]);
    }

    #[test]
    fn test_render_tree_remove() {
        let mut tree = RenderTree::new();

        let obj = TestRenderObject {
            name: "test".into(),
        };
        let mounted = RenderNode::new(obj).mount_root();
        let id = tree.insert(mounted);

        assert!(tree.contains(id));

        let removed = tree.remove(id);
        assert!(removed.is_some());
        assert!(!tree.contains(id));

        // Can unmount the removed node
        let _unmounted = removed.unwrap().unmount();
    }

    // ========== TreeRead/TreeWrite/TreeNav Tests ==========

    fn make_mounted_node() -> RenderNode<Mounted> {
        RenderNode::new(TestRenderObject {
            name: "test".into(),
        })
        .mount_root()
    }

    #[test]
    fn test_tree_read_get() {
        let mut tree = RenderTree::new();
        let id = tree.insert(make_mounted_node());

        let node: Option<&RenderNode<Mounted>> = TreeRead::get(&tree, id);
        assert!(node.is_some());
    }

    #[test]
    fn test_tree_read_contains() {
        let mut tree = RenderTree::new();
        let id = tree.insert(make_mounted_node());

        assert!(TreeRead::contains(&tree, id));
        assert!(!TreeRead::contains(&tree, RenderId::new(999)));
    }

    #[test]
    fn test_tree_read_len() {
        let mut tree = RenderTree::new();
        assert_eq!(TreeRead::<RenderId>::len(&tree), 0);

        tree.insert(make_mounted_node());
        assert_eq!(TreeRead::<RenderId>::len(&tree), 1);
    }

    #[test]
    fn test_tree_write_insert_remove() {
        let mut tree = RenderTree::new();

        let id: RenderId = TreeWrite::insert(&mut tree, make_mounted_node());
        assert!(TreeRead::contains(&tree, id));

        let removed: Option<RenderNode<Mounted>> = TreeWrite::remove(&mut tree, id);
        assert!(removed.is_some());
        assert!(!TreeRead::contains(&tree, id));
    }

    #[test]
    fn test_tree_nav_parent() {
        let mut tree = RenderTree::new();
        let parent_id = tree.insert(make_mounted_node());
        let child_id = tree.insert(make_mounted_node());

        tree.add_child(parent_id, child_id);

        assert_eq!(TreeNav::parent(&tree, child_id), Some(parent_id));
        assert_eq!(TreeNav::parent(&tree, parent_id), None);
    }

    #[test]
    fn test_tree_nav_children() {
        let mut tree = RenderTree::new();
        let parent_id = tree.insert(make_mounted_node());
        let child1_id = tree.insert(make_mounted_node());
        let child2_id = tree.insert(make_mounted_node());

        tree.add_child(parent_id, child1_id);
        tree.add_child(parent_id, child2_id);

        let children: Vec<_> = TreeNav::children(&tree, parent_id).collect();
        assert_eq!(children.len(), 2);
        assert!(children.contains(&child1_id));
        assert!(children.contains(&child2_id));
    }

    #[test]
    fn test_tree_nav_ancestors() {
        let mut tree = RenderTree::new();
        let root_id = tree.insert(make_mounted_node());
        let child_id = tree.insert(make_mounted_node());
        let grandchild_id = tree.insert(make_mounted_node());

        tree.add_child(root_id, child_id);
        tree.add_child(child_id, grandchild_id);

        let ancestors: Vec<_> = TreeNav::ancestors(&tree, grandchild_id).collect();
        assert_eq!(ancestors, vec![grandchild_id, child_id, root_id]);
    }

    #[test]
    fn test_tree_nav_descendants() {
        let mut tree = RenderTree::new();
        let root_id = tree.insert(make_mounted_node());
        let child_id = tree.insert(make_mounted_node());
        let grandchild_id = tree.insert(make_mounted_node());

        tree.add_child(root_id, child_id);
        tree.add_child(child_id, grandchild_id);

        let descendants: Vec<_> = TreeNav::descendants(&tree, root_id).collect();
        assert_eq!(descendants.len(), 3);
        assert_eq!(descendants[0], (root_id, 0));
        assert_eq!(descendants[1], (child_id, 1));
        assert_eq!(descendants[2], (grandchild_id, 2));
    }

    #[test]
    fn test_tree_nav_siblings() {
        let mut tree = RenderTree::new();
        let parent_id = tree.insert(make_mounted_node());
        let child1_id = tree.insert(make_mounted_node());
        let child2_id = tree.insert(make_mounted_node());
        let child3_id = tree.insert(make_mounted_node());

        tree.add_child(parent_id, child1_id);
        tree.add_child(parent_id, child2_id);
        tree.add_child(parent_id, child3_id);

        let siblings: Vec<_> = TreeNav::siblings(&tree, child2_id).collect();
        assert_eq!(siblings.len(), 2);
        assert!(siblings.contains(&child1_id));
        assert!(siblings.contains(&child3_id));
    }

    #[test]
    fn test_tree_nav_child_count() {
        let mut tree = RenderTree::new();
        let parent_id = tree.insert(make_mounted_node());
        let child1_id = tree.insert(make_mounted_node());
        let child2_id = tree.insert(make_mounted_node());

        assert_eq!(TreeNav::child_count(&tree, parent_id), 0);

        tree.add_child(parent_id, child1_id);
        assert_eq!(TreeNav::child_count(&tree, parent_id), 1);

        tree.add_child(parent_id, child2_id);
        assert_eq!(TreeNav::child_count(&tree, parent_id), 2);
    }

    #[test]
    fn test_tree_nav_depth() {
        let mut tree = RenderTree::new();
        let root_id = tree.insert(make_mounted_node());
        let child_id = tree.insert(make_mounted_node());
        let grandchild_id = tree.insert(make_mounted_node());

        tree.add_child(root_id, child_id);
        tree.add_child(child_id, grandchild_id);

        assert_eq!(TreeNav::depth(&tree, root_id), 0);
        assert_eq!(TreeNav::depth(&tree, child_id), 1);
        assert_eq!(TreeNav::depth(&tree, grandchild_id), 2);
    }

    // ========== HRTB Visitor Tests ==========

    #[test]
    fn test_visit_all_hrtb() {
        let mut tree = RenderTree::new();
        let _id1 = tree.insert(make_mounted_node());
        let _id2 = tree.insert(make_mounted_node());

        let mut count = 0;
        tree.visit_all(|_id, obj| {
            assert_eq!(obj.debug_name(), "TestRenderObject");
            count += 1;
        });

        assert_eq!(count, 2);
    }

    #[test]
    fn test_find_where_hrtb() {
        let mut tree = RenderTree::new();
        let _id1 = tree.insert(make_mounted_node());
        let _id2 = tree.insert(make_mounted_node());

        let found = tree.find_where(|obj| obj.debug_name() == "TestRenderObject");
        assert!(found.is_some());

        let not_found = tree.find_where(|obj| obj.debug_name() == "NonExistent");
        assert!(not_found.is_none());
    }

    // ===== Transform Operation Tests =====

    #[test]
    fn test_get_transform_to_identity() {
        // Single node - transform to self should be identity
        let mut tree = RenderTree::new();
        let root_id = tree.insert(make_mounted_node());

        let transform = tree.get_transform_to(root_id, Some(root_id));
        assert!(transform.is_some());

        // Should be identity matrix (path length = 1, no transforms applied)
        let t = transform.unwrap();
        assert_eq!(t, Matrix4::identity());
    }

    #[test]
    fn test_get_transform_to_parent_child() {
        // Simple parent-child relationship
        let mut tree = RenderTree::new();
        let parent_id = tree.insert(make_mounted_node());
        let child_id = tree.insert(make_mounted_node());

        tree.add_child(parent_id, child_id);

        // Transform from child to parent
        let transform = tree.get_transform_to(child_id, Some(parent_id));
        assert!(transform.is_some());

        // Should be identity (default apply_paint_transform only translates if offset exists)
        let t = transform.unwrap();
        assert_eq!(t, Matrix4::identity());
    }

    #[test]
    fn test_get_transform_to_grandchild() {
        // Three-level hierarchy: root -> parent -> child
        let mut tree = RenderTree::new();
        let root_id = tree.insert(make_mounted_node());
        let parent_id = tree.insert(make_mounted_node());
        let child_id = tree.insert(make_mounted_node());

        tree.add_child(root_id, parent_id);
        tree.add_child(parent_id, child_id);

        // Transform from child to root
        let transform = tree.get_transform_to(child_id, Some(root_id));
        assert!(transform.is_some());

        // Should successfully compute transform through multiple levels
        let t = transform.unwrap();
        assert_eq!(t, Matrix4::identity());
    }

    #[test]
    fn test_get_transform_to_root() {
        // Transform to root (None ancestor)
        let mut tree = RenderTree::new();
        let root_id = tree.insert(make_mounted_node());
        let child_id = tree.insert(make_mounted_node());

        tree.add_child(root_id, child_id);

        // Transform from child to root (None means traverse to root)
        let transform = tree.get_transform_to(child_id, None);
        assert!(transform.is_some());
    }

    #[test]
    fn test_get_transform_to_nonexistent() {
        let mut tree = RenderTree::new();
        let root_id = tree.insert(make_mounted_node());

        // Non-existent source ID
        let bad_id = RenderId::new(999);
        let transform = tree.get_transform_to(bad_id, Some(root_id));
        assert!(transform.is_none());

        // Non-existent ancestor ID
        let transform = tree.get_transform_to(root_id, Some(bad_id));
        assert!(transform.is_none());
    }

    #[test]
    fn test_get_transform_to_not_ancestor() {
        // Two separate branches - neither is ancestor of the other
        let mut tree = RenderTree::new();
        let root_id = tree.insert(make_mounted_node());
        let child1_id = tree.insert(make_mounted_node());
        let child2_id = tree.insert(make_mounted_node());

        tree.add_child(root_id, child1_id);
        tree.add_child(root_id, child2_id);

        // child2 is not an ancestor of child1 (they're siblings)
        let transform = tree.get_transform_to(child1_id, Some(child2_id));
        assert!(transform.is_none());
    }

    #[test]
    fn test_transform_point() {
        let mut tree = RenderTree::new();
        let root_id = tree.insert(make_mounted_node());
        let child_id = tree.insert(make_mounted_node());

        tree.add_child(root_id, child_id);

        // Transform a point from child to parent
        let point = Offset::new(10.0, 20.0);
        let transformed = tree.transform_point(child_id, Some(root_id), point);
        assert!(transformed.is_some());

        // With identity transform, point should be unchanged
        let t = transformed.unwrap();
        assert!((t.dx - 10.0).abs() < 1e-6);
        assert!((t.dy - 20.0).abs() < 1e-6);
    }

    #[test]
    fn test_global_to_local() {
        let mut tree = RenderTree::new();
        let root_id = tree.insert(make_mounted_node());

        let global_point = Offset::new(100.0, 200.0);
        let local = tree.global_to_local(root_id, global_point);
        assert!(local.is_some());

        // For root with identity transform, should be same
        let l = local.unwrap();
        assert!((l.dx - 100.0).abs() < 1e-6);
        assert!((l.dy - 200.0).abs() < 1e-6);
    }

    #[test]
    fn test_local_to_global() {
        let mut tree = RenderTree::new();
        let root_id = tree.insert(make_mounted_node());

        let local_point = Offset::new(50.0, 75.0);
        let global = tree.local_to_global(root_id, local_point);
        assert!(global.is_some());

        // For root with identity transform, should be same
        let g = global.unwrap();
        assert!((g.dx - 50.0).abs() < 1e-6);
        assert!((g.dy - 75.0).abs() < 1e-6);
    }

    #[test]
    fn test_global_local_roundtrip() {
        let mut tree = RenderTree::new();
        let root_id = tree.insert(make_mounted_node());
        let child_id = tree.insert(make_mounted_node());

        tree.add_child(root_id, child_id);

        // Roundtrip: local -> global -> local
        let original = Offset::new(42.0, 84.0);
        let global = tree.local_to_global(child_id, original).unwrap();
        let back_to_local = tree.global_to_local(child_id, global).unwrap();

        // Should get back original point
        assert!((back_to_local.dx - original.dx).abs() < 1e-6);
        assert!((back_to_local.dy - original.dy).abs() < 1e-6);
    }

    // ========== Dirty Flag Tests (Phase 6) ==========

    #[test]
    fn test_mark_needs_layout_simple() {
        let mut tree = RenderTree::new();
        let mut root = make_mounted_node();

        // Start in Painted state (clean)
        root.set_lifecycle(RenderLifecycle::Painted);
        let root_id = tree.insert(root);

        // Initially doesn't need layout (Painted state)
        assert!(!tree.needs_layout(root_id));

        // Mark as needing layout
        tree.mark_needs_layout(root_id);

        // Should now need layout
        assert!(tree.needs_layout(root_id));

        // Should be in the dirty list
        assert!(tree.nodes_needing_layout().any(|id| id == root_id));
    }

    #[test]
    fn test_mark_needs_layout_propagation() {
        let mut tree = RenderTree::new();
        let mut root = make_mounted_node();
        let mut child = make_mounted_node();

        // Start both in Painted state (clean)
        root.set_lifecycle(RenderLifecycle::Painted);
        child.set_lifecycle(RenderLifecycle::Painted);

        let root_id = tree.insert(root);
        let child_id = tree.insert(child);

        tree.add_child(root_id, child_id);

        // Ensure child is NOT a relayout boundary
        if let Some(child_node) = tree.get_mut(child_id) {
            child_node.set_relayout_boundary(false);
        }

        // Mark child as needing layout
        tree.mark_needs_layout(child_id);

        // Child should need layout
        assert!(tree.needs_layout(child_id));

        // Parent should ALSO need layout (propagation)
        assert!(tree.needs_layout(root_id));

        // Root should be in dirty list (as the boundary)
        assert!(tree.nodes_needing_layout().any(|id| id == root_id));
    }

    #[test]
    fn test_mark_needs_layout_boundary_stops_propagation() {
        let mut tree = RenderTree::new();
        let mut root = make_mounted_node();
        let mut child = make_mounted_node();

        // Start both in Painted state (clean)
        root.set_lifecycle(RenderLifecycle::Painted);
        child.set_lifecycle(RenderLifecycle::Painted);

        let root_id = tree.insert(root);
        let child_id = tree.insert(child);

        tree.add_child(root_id, child_id);

        // Make child a relayout boundary
        if let Some(child_node) = tree.get_mut(child_id) {
            child_node.set_relayout_boundary(true);
        }

        // Mark child as needing layout
        tree.mark_needs_layout(child_id);

        // Child should need layout
        assert!(tree.needs_layout(child_id));

        // Parent should NOT need layout (boundary stops propagation)
        assert!(!tree.needs_layout(root_id));

        // Child should be in dirty list (it's the boundary)
        assert!(tree.nodes_needing_layout().any(|id| id == child_id));

        // Root should NOT be in dirty list
        assert!(!tree.nodes_needing_layout().any(|id| id == root_id));
    }

    #[test]
    fn test_mark_needs_paint_simple() {
        let mut tree = RenderTree::new();
        let mut root = make_mounted_node();

        // Set lifecycle to LaidOut first (paint only works on laid out nodes)
        root.set_lifecycle(RenderLifecycle::LaidOut);
        let root_id = tree.insert(root);

        // Initially doesn't need paint
        assert!(tree.needs_paint(root_id)); // LaidOut state means needs paint

        // Mark as painted
        if let Some(node) = tree.get_mut(root_id) {
            node.set_lifecycle(RenderLifecycle::Painted);
        }

        assert!(!tree.needs_paint(root_id));

        // Mark as needing paint
        tree.mark_needs_paint(root_id);

        // Should now need paint
        assert!(tree.needs_paint(root_id));

        // Should be in the dirty list
        assert!(tree.nodes_needing_paint().any(|id| id == root_id));
    }

    #[test]
    fn test_compute_relayout_boundary() {
        use flui_types::constraints::BoxConstraints;
        use flui_types::Size;

        let mut tree = RenderTree::new();
        let node = make_mounted_node();
        let node_id = tree.insert(node);

        let node = tree.get(node_id).unwrap();

        // Root nodes are always relayout boundaries
        assert!(node.compute_relayout_boundary(true, &BoxConstraints::loose(Size::new(100.0, 100.0))));

        // Create a non-root node
        let child = make_mounted_node();
        let child_id = tree.insert(child);
        tree.add_child(node_id, child_id);

        let child_node = tree.get(child_id).unwrap();

        // With parent_uses_size = false, should be boundary
        assert!(child_node.compute_relayout_boundary(false, &BoxConstraints::loose(Size::new(100.0, 100.0))));

        // With tight constraints, should be boundary
        assert!(child_node.compute_relayout_boundary(true, &BoxConstraints::tight(Size::new(100.0, 100.0))));

        // With parent_uses_size = true and loose constraints, should NOT be boundary
        assert!(!child_node.compute_relayout_boundary(true, &BoxConstraints::loose(Size::new(100.0, 100.0))));
    }

    // ========== P0-2: Relayout Boundary Integration Tests ==========

    #[test]
    fn test_perform_layout_sets_boundary_with_parent_uses_size_false() {
        use flui_types::constraints::BoxConstraints;
        use flui_types::Size;
        use crate::tree::LayoutTree;

        let mut tree = RenderTree::new();
        let node = make_mounted_node();
        let node_id = tree.insert(node);

        // Create parent-child relationship
        let child = make_mounted_node();
        let child_id = tree.insert(child);
        tree.add_child(node_id, child_id);

        let constraints = BoxConstraints::loose(Size::new(100.0, 100.0));

        // Perform layout with parent_uses_size = false
        let _ = tree.perform_layout(child_id, constraints, false);

        // After layout, child should be marked as boundary
        let child_node = tree.get(child_id).unwrap();
        assert!(child_node.is_relayout_boundary(),
            "Child should be relayout boundary when parent_uses_size = false");
    }

    #[test]
    fn test_perform_layout_sets_boundary_with_tight_constraints() {
        use flui_types::constraints::BoxConstraints;
        use flui_types::Size;
        use crate::tree::LayoutTree;

        let mut tree = RenderTree::new();
        let node = make_mounted_node();
        let node_id = tree.insert(node);

        let child = make_mounted_node();
        let child_id = tree.insert(child);
        tree.add_child(node_id, child_id);

        // Tight constraints = only one valid size
        let tight_constraints = BoxConstraints::tight(Size::new(100.0, 50.0));

        // Perform layout with parent_uses_size = true BUT tight constraints
        let _ = tree.perform_layout(child_id, tight_constraints, true);

        // Should still be boundary because constraints are tight
        let child_node = tree.get(child_id).unwrap();
        assert!(child_node.is_relayout_boundary(),
            "Child should be relayout boundary with tight constraints");
    }

    #[test]
    fn test_perform_layout_no_boundary_with_parent_uses_size_true() {
        use flui_types::constraints::BoxConstraints;
        use flui_types::Size;
        use crate::tree::LayoutTree;

        let mut tree = RenderTree::new();
        let node = make_mounted_node();
        let node_id = tree.insert(node);

        let child = make_mounted_node();
        let child_id = tree.insert(child);
        tree.add_child(node_id, child_id);

        let loose_constraints = BoxConstraints::loose(Size::new(100.0, 100.0));

        // Perform layout with parent_uses_size = true and loose constraints
        let _ = tree.perform_layout(child_id, loose_constraints, true);

        // Should NOT be boundary
        let child_node = tree.get(child_id).unwrap();
        assert!(!child_node.is_relayout_boundary(),
            "Child should NOT be relayout boundary when parent_uses_size = true with loose constraints");
    }

    #[test]
    fn test_perform_layout_root_is_always_boundary() {
        use flui_types::constraints::BoxConstraints;
        use flui_types::Size;
        use crate::tree::LayoutTree;

        let mut tree = RenderTree::new();
        let root = make_mounted_node();
        let root_id = tree.insert(root);

        let constraints = BoxConstraints::loose(Size::new(800.0, 600.0));

        // Perform layout on root (no parent)
        let _ = tree.perform_layout(root_id, constraints, true);

        // Root should always be boundary
        let root_node = tree.get(root_id).unwrap();
        assert!(root_node.is_relayout_boundary(),
            "Root node should always be a relayout boundary");
    }

    #[test]
    fn test_perform_layout_boundary_affects_mark_needs_layout() {
        use flui_types::constraints::BoxConstraints;
        use flui_types::Size;
        use crate::tree::LayoutTree;

        let mut tree = RenderTree::new();

        // Create root
        let mut root = make_mounted_node();
        root.set_lifecycle(RenderLifecycle::Painted);
        let root_id = tree.insert(root);

        // Create child
        let mut child = make_mounted_node();
        child.set_lifecycle(RenderLifecycle::Painted);
        let child_id = tree.insert(child);
        tree.add_child(root_id, child_id);

        let constraints = BoxConstraints::loose(Size::new(100.0, 100.0));

        // Perform layout with parent_uses_size = false to make child a boundary
        let _ = tree.perform_layout(child_id, constraints, false);

        // Verify child is boundary
        let child_node = tree.get(child_id).unwrap();
        assert!(child_node.is_relayout_boundary());

        // Clear dirty lists
        tree.clear_needs_layout_list();

        // Mark child as needing layout
        tree.mark_needs_layout(child_id);

        // Child should be in dirty list (boundary stops propagation)
        assert!(tree.nodes_needing_layout().any(|id| id == child_id),
            "Child boundary should be in dirty list");

        // Parent should NOT be in dirty list (propagation stopped at boundary)
        assert!(!tree.nodes_needing_layout().any(|id| id == root_id),
            "Parent should NOT be in dirty list when child is boundary");
    }

    #[test]
    fn test_perform_sliver_layout_sets_boundary() {
        use flui_types::{SliverConstraints, Axis};
        use flui_types::constraints::GrowthDirection;
        use flui_types::prelude::AxisDirection;
        use crate::tree::LayoutTree;

        let mut tree = RenderTree::new();
        let node = make_mounted_node();
        let node_id = tree.insert(node);

        let child = make_mounted_node();
        let child_id = tree.insert(child);
        tree.add_child(node_id, child_id);

        let sliver_constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            axis: Axis::Vertical,
            scroll_offset: 0.0,
            remaining_paint_extent: 600.0,
            viewport_main_axis_extent: 600.0,
            preceding_scroll_extent: 0.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
        };

        // Perform sliver layout with parent_uses_size = false
        let _ = tree.perform_sliver_layout(child_id, sliver_constraints, false);

        // Should be boundary
        let child_node = tree.get(child_id).unwrap();
        assert!(child_node.is_relayout_boundary(),
            "Sliver child should be relayout boundary when parent_uses_size = false");
    }

    #[test]
    fn test_clear_dirty_lists() {
        let mut tree = RenderTree::new();
        let mut root = make_mounted_node();

        // Start in Painted state (clean)
        root.set_lifecycle(RenderLifecycle::Painted);
        let root_id = tree.insert(root);

        // Mark as needing layout and paint
        tree.mark_needs_layout(root_id);

        if let Some(node) = tree.get_mut(root_id) {
            node.set_lifecycle(RenderLifecycle::LaidOut);
        }
        tree.mark_needs_paint(root_id);

        // Should be in both dirty lists
        assert!(tree.nodes_needing_layout().count() > 0);
        assert!(tree.nodes_needing_paint().count() > 0);

        // Clear dirty lists
        tree.clear_needs_layout_list();
        tree.clear_needs_paint_list();

        // Lists should be empty
        assert_eq!(tree.nodes_needing_layout().count(), 0);
        assert_eq!(tree.nodes_needing_paint().count(), 0);
    }

    // ========== Layer Management Tests ==========

    /// Test helper: Creates a mounted node with repaint boundary enabled
    fn make_repaint_boundary_node() -> RenderNode<Mounted> {
        #[derive(Debug)]
        struct RepaintBoundary;
        impl RenderObject for RepaintBoundary {
            fn debug_name(&self) -> &'static str { "RepaintBoundary" }
            fn is_repaint_boundary(&self) -> bool { true }
        }

        RenderNode::new(RepaintBoundary).mount(None, flui_tree::Depth::root())
    }

    #[test]
    fn test_update_composited_layer_creates_layer_for_boundary() {
        let mut node = make_repaint_boundary_node();

        // Initially no layer
        assert!(node.layer_handle().is_none());

        // Update composited layer
        node.update_composited_layer();

        // Should have created layer
        assert!(node.layer_handle().is_some());

        // Should be an OffsetLayer
        if let Some(handle) = node.layer_handle() {
            if let Some(layer) = handle.get() {
                assert!(layer.is_offset(), "Layer should be OffsetLayer");
            }
        }
    }

    #[test]
    fn test_update_composited_layer_clears_layer_for_non_boundary() {
        // Create repaint boundary node with layer
        let mut node = make_repaint_boundary_node();
        node.update_composited_layer();
        assert!(node.layer_handle().is_some());

        // Convert to non-repaint boundary by replacing render object
        #[derive(Debug)]
        struct NonBoundary;
        impl RenderObject for NonBoundary {
            fn debug_name(&self) -> &'static str { "NonBoundary" }
            fn is_repaint_boundary(&self) -> bool { false }
        }

        let mut non_boundary_node = RenderNode::new(NonBoundary).mount(None, flui_tree::Depth::root());
        // Manually set layer handle to simulate previous boundary state
        let mut handle = flui_layer::LayerHandle::new();
        handle.set(flui_layer::Layer::Offset(flui_layer::OffsetLayer::from_xy(0.0, 0.0)));
        non_boundary_node.set_layer_handle(Some(handle));

        assert!(non_boundary_node.layer_handle().is_some());

        // Update composited layer
        non_boundary_node.update_composited_layer();

        // Should have cleared layer
        assert!(non_boundary_node.layer_handle().is_none());
    }

    #[test]
    fn test_update_composited_layer_reuses_existing_layer() {
        let mut node = make_repaint_boundary_node();

        // Create initial layer
        node.update_composited_layer();
        let first_has_layer = node.layer_handle().is_some();
        assert!(first_has_layer);

        // Update again
        node.update_composited_layer();
        let second_has_layer = node.layer_handle().is_some();

        // Should reuse the same layer (Flutter optimization)
        assert!(second_has_layer);
        // Layer handle should still exist (not recreated)
        assert!(node.layer_handle().is_some());
    }

    #[test]
    fn test_layer_cleared_on_detach() {
        let mut tree = RenderTree::new();

        // Create repaint boundary with layer
        let parent = make_mounted_node();
        let parent_id = tree.insert(parent);

        let mut child = make_repaint_boundary_node();
        child.update_composited_layer();
        assert!(child.layer_handle().is_some());

        let child_id = tree.insert(child);
        tree.add_child(parent_id, child_id);

        // Verify child has layer
        assert!(tree.get(child_id).unwrap().layer_handle().is_some());

        // Remove child
        tree.remove_child(parent_id, child_id);

        // Layer should be cleared after detach
        assert!(tree.get(child_id).unwrap().layer_handle().is_none());
    }

    #[test]
    fn test_repaint_boundary_layer_lifecycle() {
        let mut tree = RenderTree::new();

        // Create repaint boundary node
        let node = make_repaint_boundary_node();
        let node_id = tree.insert(node);

        // Update composited layer
        if let Some(node) = tree.get_mut(node_id) {
            node.update_composited_layer();
        }

        // Should have layer
        assert!(tree.get(node_id).unwrap().layer_handle().is_some());

        // Remove from tree
        tree.remove(node_id);

        // Node no longer in tree
        assert!(tree.get(node_id).is_none());
    }
}
