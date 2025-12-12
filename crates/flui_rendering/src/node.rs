//! RenderNode - Flutter RenderObject equivalent for FLUI.
//!
//! This module implements a complete Flutter-compatible RenderNode that mirrors
//! Flutter's `RenderObject` class with all its state and lifecycle management.
//!
//! # Architecture
//!
//! In Flutter, `RenderObject` is a class with both state and behavior:
//! ```dart
//! abstract class RenderObject {
//!   RenderObject? _parent;
//!   int _depth;
//!   bool _needsLayout;
//!   bool _needsPaint;
//!   Constraints? _constraints;
//!   // ... + abstract methods
//! }
//! ```
//!
//! In FLUI, we separate this into:
//! - **RenderNode** - State container (this file) ≈ Flutter RenderObject fields
//! - **RenderObject trait** - Behavior interface ≈ Flutter abstract methods
//!
//! # Flutter Protocol Compliance
//!
//! | Flutter Field | FLUI Field | Notes |
//! |---------------|------------|-------|
//! | `_parent` | `parent` | Parent ID reference |
//! | `_depth` | `depth` | Tree depth for ordering |
//! | `_needsLayout` | `needs_layout` | Layout dirty flag |
//! | `_needsPaint` | `needs_paint` | Paint dirty flag |
//! | `_needsCompositingBitsUpdate` | `needs_compositing_bits_update` | Compositing dirty |
//! | `_isRelayoutBoundary` | `relayout_boundary` | Layout boundary flag |
//! | `_needsCompositing` | `needs_compositing` | Compositing needed |
//! | `_wasRepaintBoundary` | `was_repaint_boundary` | Previous boundary state |
//! | `_constraints` | `constraints` | Last layout constraints |
//! | `_layerHandle` | `layer_handle` | Compositing layer |
//! | `parentData` | `parent_data` | Parent-specific data |
//! | `_debugDisposed` | `disposed` | Disposal state |
//!
//! # Typestate Pattern
//!
//! RenderNode uses the typestate pattern for compile-time safety:
//! - `RenderNode<Unmounted>` - Detached from tree
//! - `RenderNode<Mounted>` - Attached to tree with valid parent/depth

use std::fmt;
use std::marker::PhantomData;

use crate::geometry::{RenderConstraints, RenderGeometry};
use flui_foundation::{ElementId, RenderId};
use flui_tree::{Depth, Mountable, Mounted, NodeState, Unmountable, Unmounted};
use flui_types::constraints::BoxConstraints;
use flui_types::{Offset, Size};

use crate::{LayerHandle, ParentData, RenderLifecycle, RenderObject};

// ============================================================================
// RENDER NODE STRUCTURE
// ============================================================================

/// RenderNode - Complete Flutter RenderObject equivalent.
///
/// This struct contains all the state that Flutter's `RenderObject` class has,
/// organized for Rust's ownership model with the typestate pattern.
///
/// # Type Parameters
///
/// - `S: NodeState` - Compile-time state marker (`Mounted` or `Unmounted`)
///
/// # Examples
///
/// ```rust,ignore
/// // Create unmounted node
/// let node = RenderNode::new(my_render_object);
///
/// // Mount into tree
/// let mounted = node.mount(Some(parent_id), parent_depth);
///
/// // Access state (only when mounted)
/// assert!(mounted.is_attached());
/// let parent = mounted.parent();
/// ```
pub struct RenderNode<S: NodeState> {
    // ========================================================================
    // BEHAVIOR (delegated to trait object)
    // ========================================================================
    /// The type-erased RenderObject providing behavior.
    ///
    /// This is the "abstract methods" part of Flutter's RenderObject.
    /// Layout, paint, and hit-test logic live here.
    render_object: Box<dyn RenderObject>,

    // ========================================================================
    // TREE STRUCTURE (Flutter: _parent, _depth)
    // ========================================================================
    /// Parent node ID in the render tree.
    ///
    /// `None` for the root node or when unmounted.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// RenderObject? get parent => _parent;
    /// RenderObject? _parent;
    /// ```
    parent: Option<RenderId>,

    /// Tree depth for processing order.
    ///
    /// Ensures parents are processed before children during layout,
    /// and children before parents during paint.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// int get depth => _depth;
    /// int _depth = 0;
    /// ```
    depth: Depth,

    /// Children node IDs.
    ///
    /// Stored here for efficient iteration. The actual child RenderObjects
    /// are stored in the RenderTree.
    children: Vec<RenderId>,

    // ========================================================================
    // DIRTY FLAGS (Flutter: _needsLayout, _needsPaint, etc.)
    // ========================================================================
    /// Whether this node needs layout.
    ///
    /// Set by `mark_needs_layout()`, cleared after `perform_layout()`.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// bool _needsLayout = true;
    /// ```
    needs_layout: bool,

    /// Whether this node needs paint.
    ///
    /// Set by `mark_needs_paint()`, cleared after `paint()`.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// bool _needsPaint = true;
    /// ```
    needs_paint: bool,

    /// Whether compositing bits need update.
    ///
    /// Set when child compositing status changes.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// bool _needsCompositingBitsUpdate = false;
    /// ```
    needs_compositing_bits_update: bool,

    /// Whether this node needs semantics update.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// bool _needsSemanticsUpdate = true;
    /// ```
    needs_semantics_update: bool,

    // ========================================================================
    // LAYOUT STATE (Flutter: _constraints, _isRelayoutBoundary)
    // ========================================================================
    /// Last constraints used for layout.
    ///
    /// Stored for cache validation - if constraints unchanged, may skip layout.
    /// Uses `RenderConstraints` enum to support both Box and Sliver protocols.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// Constraints? _constraints;
    /// ```
    constraints: Option<RenderConstraints>,

    /// Cached geometry from last layout.
    ///
    /// This is the "output" of layout - Size for Box, SliverGeometry for Sliver.
    /// Uses `RenderGeometry` enum to support both protocols.
    ///
    /// # Flutter Equivalence
    /// - RenderBox: `Size? _size`
    /// - RenderSliver: `SliverGeometry? _geometry`
    geometry: Option<RenderGeometry>,

    /// Whether this is a relayout boundary.
    ///
    /// Computed during `layout()` based on:
    /// - `!parent_uses_size`
    /// - `sized_by_parent`
    /// - `constraints.is_tight()`
    /// - `parent == null`
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// bool? _isRelayoutBoundary;
    /// ```
    relayout_boundary: Option<bool>,

    // ========================================================================
    // COMPOSITING STATE (Flutter: _needsCompositing, _wasRepaintBoundary)
    // ========================================================================
    /// Whether this node or subtree needs compositing.
    ///
    /// True if:
    /// - This is a repaint boundary
    /// - `always_needs_compositing()` returns true
    /// - Any child needs compositing
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// late bool _needsCompositing;
    /// bool get needsCompositing => _needsCompositing;
    /// ```
    needs_compositing: bool,

    /// Previous repaint boundary status for transition detection.
    ///
    /// Used to detect when a node transitions to/from being a repaint boundary.
    /// This affects compositing bits propagation.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// late bool _wasRepaintBoundary;
    /// ```
    was_repaint_boundary: bool,

    /// Compositing layer handle for repaint boundaries.
    ///
    /// Only set when `is_repaint_boundary()` returns true.
    /// Used to cache paint results.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// final LayerHandle<ContainerLayer> _layerHandle = LayerHandle<ContainerLayer>();
    /// ```
    layer_handle: Option<LayerHandle>,

    // ========================================================================
    // PARENT DATA (Flutter: parentData)
    // ========================================================================
    /// Parent-specific data stored on the child.
    ///
    /// Set by parent via `setup_parent_data()`. Used for:
    /// - Position offset (BoxParentData)
    /// - Flex factor (FlexParentData)
    /// - Stack positioning (StackParentData)
    /// - etc.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// ParentData? parentData;
    /// ```
    parent_data: Option<Box<dyn ParentData>>,

    // ========================================================================
    // LIFECYCLE STATE
    // ========================================================================
    /// Current lifecycle phase.
    ///
    /// Tracks: Detached → Attached → NeedsLayout → LaidOut → NeedsPaint → Painted
    lifecycle: RenderLifecycle,

    /// Whether this node has been disposed.
    ///
    /// Once disposed, the node cannot be reused.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// bool _debugDisposed = false;
    /// ```
    disposed: bool,

    // ========================================================================
    // CROSS-TREE REFERENCES
    // ========================================================================
    /// Associated Element ID for cross-tree references.
    ///
    /// Links this RenderNode to its corresponding Element in the Element tree.
    element_id: Option<ElementId>,

    // ========================================================================
    // TYPESTATE MARKER
    // ========================================================================
    /// Zero-sized marker for compile-time state tracking.
    _state: PhantomData<S>,
}

// ============================================================================
// UNMOUNTED NODE IMPLEMENTATION
// ============================================================================

impl RenderNode<Unmounted> {
    /// Creates a new unmounted RenderNode.
    ///
    /// The node starts with:
    /// - `needs_layout = true` (needs initial layout)
    /// - `needs_paint = true` (needs initial paint)
    /// - `disposed = false`
    ///
    /// # Arguments
    ///
    /// * `render_object` - The behavior implementation
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let node = RenderNode::new(RenderPadding::new(padding));
    /// ```
    pub fn new<R: RenderObject + 'static>(render_object: R) -> Self {
        let is_repaint_boundary = render_object.is_repaint_boundary();
        let always_needs_compositing = render_object.always_needs_compositing();

        Self {
            render_object: Box::new(render_object),
            parent: None,
            depth: Depth::root(),
            children: Vec::new(),
            needs_layout: true,
            needs_paint: true,
            needs_compositing_bits_update: false,
            needs_semantics_update: true,
            constraints: None,
            geometry: None,
            relayout_boundary: None,
            needs_compositing: is_repaint_boundary || always_needs_compositing,
            was_repaint_boundary: is_repaint_boundary,
            layer_handle: None,
            parent_data: None,
            lifecycle: RenderLifecycle::Detached,
            disposed: false,
            element_id: None,
            _state: PhantomData,
        }
    }

    /// Creates a RenderNode from a boxed RenderObject.
    ///
    /// Useful when the concrete type is already erased.
    pub fn from_boxed(render_object: Box<dyn RenderObject>) -> Self {
        let is_repaint_boundary = render_object.is_repaint_boundary();
        let always_needs_compositing = render_object.always_needs_compositing();

        Self {
            render_object,
            parent: None,
            depth: Depth::root(),
            children: Vec::new(),
            needs_layout: true,
            needs_paint: true,
            needs_compositing_bits_update: false,
            needs_semantics_update: true,
            constraints: None,
            geometry: None,
            relayout_boundary: None,
            needs_compositing: is_repaint_boundary || always_needs_compositing,
            was_repaint_boundary: is_repaint_boundary,
            layer_handle: None,
            parent_data: None,
            lifecycle: RenderLifecycle::Detached,
            disposed: false,
            element_id: None,
            _state: PhantomData,
        }
    }

    /// Attaches an ElementId (builder pattern).
    pub fn with_element_id(mut self, element_id: ElementId) -> Self {
        self.element_id = Some(element_id);
        self
    }
}

// ============================================================================
// MOUNTED NODE IMPLEMENTATION
// ============================================================================

impl RenderNode<Mounted> {
    // ========================================================================
    // TREE NAVIGATION (Flutter: parent, depth, children)
    // ========================================================================

    /// Returns the parent node ID.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// RenderObject? get parent => _parent;
    /// ```
    #[inline]
    pub fn parent(&self) -> Option<RenderId> {
        self.parent
    }

    /// Returns the tree depth.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// int get depth => _depth;
    /// ```
    #[inline]
    pub fn depth(&self) -> Depth {
        self.depth
    }

    /// Returns true if this is the root node.
    #[inline]
    pub fn is_root(&self) -> bool {
        self.parent.is_none()
    }

    /// Returns children IDs.
    #[inline]
    pub fn children(&self) -> &[RenderId] {
        &self.children
    }

    /// Returns the number of children.
    #[inline]
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Returns true if this node has children.
    #[inline]
    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    // ========================================================================
    // TREE MUTATIONS (internal)
    // ========================================================================

    /// Sets the parent (internal use).
    #[inline]
    pub(crate) fn set_parent(&mut self, parent: Option<RenderId>) {
        self.parent = parent;
    }

    /// Sets the depth (internal use).
    #[inline]
    pub(crate) fn set_depth(&mut self, depth: Depth) {
        self.depth = depth;
    }

    /// Adds a child (internal use).
    #[inline]
    pub(crate) fn add_child(&mut self, child: RenderId) {
        self.children.push(child);
    }

    /// Removes a child (internal use).
    #[inline]
    pub(crate) fn remove_child(&mut self, child: RenderId) {
        self.children.retain(|&id| id != child);
    }

    // ========================================================================
    // ATTACHMENT STATE (Flutter: attached, owner)
    // ========================================================================

    /// Returns whether this node is attached to a tree.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// bool get attached => _owner != null;
    /// ```
    #[inline]
    pub fn is_attached(&self) -> bool {
        self.lifecycle.is_attached()
    }

    // ========================================================================
    // DIRTY FLAGS (Flutter: _needsLayout, _needsPaint, etc.)
    // ========================================================================

    /// Returns whether this node needs layout.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// bool get debugNeedsLayout => _needsLayout;
    /// ```
    #[inline]
    pub fn needs_layout(&self) -> bool {
        self.needs_layout
    }

    /// Returns whether this node needs paint.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// bool get debugNeedsPaint => _needsPaint;
    /// ```
    #[inline]
    pub fn needs_paint(&self) -> bool {
        self.needs_paint
    }

    /// Returns whether compositing bits need update.
    #[inline]
    pub fn needs_compositing_bits_update(&self) -> bool {
        self.needs_compositing_bits_update
    }

    /// Returns whether semantics need update.
    #[inline]
    pub fn needs_semantics_update(&self) -> bool {
        self.needs_semantics_update
    }

    /// Marks this node as needing layout.
    ///
    /// **Note**: This only sets the flag. Propagation to parent and
    /// registration with PipelineOwner is handled by `RenderTree`.
    ///
    /// # Flutter Equivalence (partial)
    /// ```dart
    /// void markNeedsLayout() {
    ///   _needsLayout = true;
    ///   // ... propagation logic in RenderTree
    /// }
    /// ```
    #[inline]
    pub fn mark_needs_layout(&mut self) {
        self.needs_layout = true;
    }

    /// Marks this node as needing paint.
    #[inline]
    pub fn mark_needs_paint(&mut self) {
        self.needs_paint = true;
    }

    /// Marks compositing bits as needing update.
    #[inline]
    pub fn mark_needs_compositing_bits_update(&mut self) {
        self.needs_compositing_bits_update = true;
    }

    /// Marks semantics as needing update.
    #[inline]
    pub fn mark_needs_semantics_update(&mut self) {
        self.needs_semantics_update = true;
    }

    /// Clears the needs_layout flag after successful layout.
    #[inline]
    pub fn clear_needs_layout(&mut self) {
        self.needs_layout = false;
    }

    /// Clears the needs_paint flag after successful paint.
    #[inline]
    pub fn clear_needs_paint(&mut self) {
        self.needs_paint = false;
    }

    /// Clears the compositing bits update flag.
    #[inline]
    pub fn clear_needs_compositing_bits_update(&mut self) {
        self.needs_compositing_bits_update = false;
    }

    /// Clears the semantics update flag.
    #[inline]
    pub fn clear_needs_semantics_update(&mut self) {
        self.needs_semantics_update = false;
    }

    // ========================================================================
    // LAYOUT STATE (Flutter: constraints, _isRelayoutBoundary)
    // ========================================================================

    /// Returns the last constraints used for layout.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// Constraints get constraints => _constraints!;
    /// ```
    #[inline]
    pub fn constraints(&self) -> Option<&RenderConstraints> {
        self.constraints.as_ref()
    }

    /// Sets the constraints for layout.
    #[inline]
    pub fn set_constraints(&mut self, constraints: impl Into<RenderConstraints>) {
        self.constraints = Some(constraints.into());
    }

    /// Returns Box constraints (convenience for Box protocol).
    ///
    /// Returns `None` if no constraints or if constraints are Sliver.
    #[inline]
    pub fn box_constraints(&self) -> Option<&BoxConstraints> {
        self.constraints.as_ref().and_then(|c| c.as_box())
    }

    /// Clears the constraints (for relayout).
    #[inline]
    pub fn clear_constraints(&mut self) {
        self.constraints = None;
    }

    /// Returns the cached geometry from last layout.
    #[inline]
    pub fn geometry(&self) -> Option<&RenderGeometry> {
        self.geometry.as_ref()
    }

    /// Sets the cached geometry after layout.
    #[inline]
    pub fn set_geometry(&mut self, geometry: RenderGeometry) {
        self.geometry = Some(geometry);
    }

    /// Returns the cached size from last layout (convenience for Box protocol).
    ///
    /// Returns `None` if no geometry or if geometry is Sliver.
    #[inline]
    pub fn size(&self) -> Option<Size> {
        self.geometry.as_ref().and_then(|g| g.as_box())
    }

    /// Sets the cached size after layout (convenience for Box protocol).
    #[inline]
    pub fn set_size(&mut self, size: Size) {
        self.geometry = Some(RenderGeometry::Box(size));
    }

    /// Returns whether this is a relayout boundary.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// bool? get _isRelayoutBoundary;
    /// ```
    #[inline]
    pub fn is_relayout_boundary(&self) -> bool {
        self.relayout_boundary.unwrap_or(false)
    }

    /// Sets the relayout boundary status.
    #[inline]
    pub fn set_relayout_boundary(&mut self, is_boundary: bool) {
        self.relayout_boundary = Some(is_boundary);
    }

    /// Clears the relayout boundary status (after drop_child).
    #[inline]
    pub fn clear_relayout_boundary(&mut self) {
        self.relayout_boundary = None;
    }

    /// Computes relayout boundary status based on layout parameters.
    ///
    /// A node is a relayout boundary when ANY of:
    /// - `!parent_uses_size` - Parent ignores child's size
    /// - `sized_by_parent` - Size determined by constraints only
    /// - `constraints.is_tight()` - Only one valid size
    /// - `parent == null` - Root of tree
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// _relayoutBoundary = !parentUsesSize || sizedByParent ||
    ///                     constraints.isTight || parent == null;
    /// ```
    pub fn compute_relayout_boundary(&mut self, parent_uses_size: bool) {
        let is_boundary = self.parent.is_none()  // Root
            || !parent_uses_size
            || self.render_object.sized_by_parent()
            || self.constraints.as_ref().map(|c| c.is_tight()).unwrap_or(false);

        self.relayout_boundary = Some(is_boundary);
    }

    // ========================================================================
    // COMPOSITING STATE (Flutter: needsCompositing, _wasRepaintBoundary)
    // ========================================================================

    /// Returns whether this node or subtree needs compositing.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// bool get needsCompositing => _needsCompositing;
    /// ```
    #[inline]
    pub fn needs_compositing(&self) -> bool {
        self.needs_compositing
    }

    /// Sets the needs_compositing flag.
    #[inline]
    pub fn set_needs_compositing(&mut self, value: bool) {
        self.needs_compositing = value;
    }

    /// Returns the previous repaint boundary status.
    ///
    /// Used for transition detection.
    #[inline]
    pub fn was_repaint_boundary(&self) -> bool {
        self.was_repaint_boundary
    }

    /// Updates was_repaint_boundary to current status.
    ///
    /// Called after compositing bits update.
    #[inline]
    pub fn update_was_repaint_boundary(&mut self) {
        self.was_repaint_boundary = self.render_object.is_repaint_boundary();
    }

    /// Updates compositing bits for this node.
    ///
    /// Returns true if `needs_compositing` changed.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// void _updateCompositingBits() {
    ///   final bool oldNeedsCompositing = _needsCompositing;
    ///   _needsCompositing = false;
    ///   visitChildren((child) {
    ///     if (child.needsCompositing) _needsCompositing = true;
    ///   });
    ///   if (isRepaintBoundary || alwaysNeedsCompositing)
    ///     _needsCompositing = true;
    ///   // ...
    /// }
    /// ```
    pub fn update_compositing_bits(&mut self, any_child_needs_compositing: bool) -> bool {
        let old = self.needs_compositing;

        self.needs_compositing = any_child_needs_compositing
            || self.render_object.is_repaint_boundary()
            || self.render_object.always_needs_compositing();

        self.needs_compositing_bits_update = false;

        old != self.needs_compositing
    }

    // ========================================================================
    // LAYER HANDLE (Flutter: _layerHandle)
    // ========================================================================

    /// Returns the compositing layer handle.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// ContainerLayer? get layer => _layerHandle.layer;
    /// ```
    #[inline]
    pub fn layer_handle(&self) -> Option<&LayerHandle> {
        self.layer_handle.as_ref()
    }

    /// Returns mutable layer handle.
    #[inline]
    pub fn layer_handle_mut(&mut self) -> Option<&mut LayerHandle> {
        self.layer_handle.as_mut()
    }

    /// Sets the layer handle.
    #[inline]
    pub fn set_layer_handle(&mut self, handle: Option<LayerHandle>) {
        self.layer_handle = handle;
    }

    /// Creates or reuses the compositing layer for repaint boundaries.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// OffsetLayer updateCompositedLayer({required OffsetLayer? oldLayer}) {
    ///   return oldLayer ?? OffsetLayer();
    /// }
    /// ```
    pub fn update_composited_layer(&mut self, offset: Offset) {
        if !self.render_object.is_repaint_boundary() {
            // Not a repaint boundary - clear any existing layer
            if self.layer_handle.is_some() {
                self.layer_handle = None;
            }
            return;
        }

        // Create or reuse layer
        if self.layer_handle.is_none() {
            let mut handle = flui_layer::LayerHandle::new();
            let offset_layer = flui_layer::OffsetLayer::new(offset);
            handle.set(flui_layer::Layer::Offset(offset_layer));
            self.layer_handle = Some(handle);
        } else if let Some(ref mut handle) = self.layer_handle {
            // Update existing layer's offset
            if let Some(layer) = handle.get_mut() {
                if let Some(offset_layer) = layer.as_offset_mut() {
                    offset_layer.set_offset(offset);
                }
            }
        }
    }

    // ========================================================================
    // PARENT DATA (Flutter: parentData)
    // ========================================================================

    /// Returns the parent data.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// ParentData? get parentData;
    /// ```
    #[inline]
    pub fn parent_data(&self) -> Option<&dyn ParentData> {
        self.parent_data.as_deref()
    }

    /// Returns mutable parent data.
    #[inline]
    pub fn parent_data_mut(&mut self) -> Option<&mut dyn ParentData> {
        self.parent_data.as_deref_mut()
    }

    /// Sets the parent data.
    #[inline]
    pub fn set_parent_data(&mut self, data: Option<Box<dyn ParentData>>) {
        self.parent_data = data;
    }

    /// Returns parent data as specific type.
    pub fn parent_data_as<T: ParentData + 'static>(&self) -> Option<&T> {
        self.parent_data
            .as_ref()
            .and_then(|pd| pd.as_any().downcast_ref::<T>())
    }

    /// Returns mutable parent data as specific type.
    pub fn parent_data_as_mut<T: ParentData + 'static>(&mut self) -> Option<&mut T> {
        self.parent_data
            .as_mut()
            .and_then(|pd| pd.as_any_mut().downcast_mut::<T>())
    }

    // ========================================================================
    // LIFECYCLE METHODS (Flutter: dispose, reassemble)
    // ========================================================================

    /// Returns whether this node has been disposed.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// bool? get debugDisposed => _debugDisposed;
    /// ```
    #[inline]
    pub fn is_disposed(&self) -> bool {
        self.disposed
    }

    /// Releases resources held by this node.
    ///
    /// After calling dispose:
    /// - Layer handle is cleared
    /// - Parent data is cleared
    /// - Node cannot be reused
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// @mustCallSuper
    /// void dispose() {
    ///   _layerHandle.layer = null;
    ///   _debugDisposed = true;
    /// }
    /// ```
    pub fn dispose(&mut self) {
        debug_assert!(!self.disposed, "RenderNode already disposed");

        // Release GPU resources
        self.layer_handle = None;

        // Clear cached data
        self.parent_data = None;
        self.geometry = None;
        self.constraints = None;

        // Mark as disposed
        self.disposed = true;
        self.lifecycle = RenderLifecycle::Disposed;

        tracing::trace!(
            render_object = self.render_object.debug_name(),
            "RenderNode disposed"
        );
    }

    /// Marks entire subtree as dirty for hot reload.
    ///
    /// Returns children IDs for recursive processing.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// void reassemble() {
    ///   markNeedsLayout();
    ///   markNeedsCompositingBitsUpdate();
    ///   markNeedsPaint();
    ///   markNeedsSemanticsUpdate();
    ///   visitChildren((child) => child.reassemble());
    /// }
    /// ```
    pub fn reassemble(&mut self) -> Vec<RenderId> {
        debug_assert!(!self.disposed, "Cannot reassemble disposed node");

        self.needs_layout = true;
        self.needs_paint = true;
        self.needs_compositing_bits_update = true;
        self.needs_semantics_update = true;

        // Reset compositing (will be recomputed)
        self.needs_compositing = self.render_object.is_repaint_boundary()
            || self.render_object.always_needs_compositing();

        self.children.clone()
    }

    // ========================================================================
    // LIFECYCLE STATE
    // ========================================================================

    /// Sets the lifecycle state.
    #[inline]
    pub fn set_lifecycle(&mut self, lifecycle: RenderLifecycle) {
        self.lifecycle = lifecycle;
    }
}

// ============================================================================
// COMMON IMPLEMENTATION (both Mounted and Unmounted)
// ============================================================================

impl<S: NodeState> RenderNode<S> {
    /// Returns reference to the RenderObject behavior.
    #[inline]
    pub fn render_object(&self) -> &dyn RenderObject {
        &*self.render_object
    }

    /// Returns mutable reference to the RenderObject behavior.
    #[inline]
    pub fn render_object_mut(&mut self) -> &mut dyn RenderObject {
        &mut *self.render_object
    }

    /// Returns the associated ElementId.
    #[inline]
    pub fn element_id(&self) -> Option<ElementId> {
        self.element_id
    }

    /// Sets the associated ElementId.
    #[inline]
    pub fn set_element_id(&mut self, element_id: Option<ElementId>) {
        self.element_id = element_id;
    }

    /// Downcasts the RenderObject to a specific type.
    pub fn downcast_ref<T: RenderObject + 'static>(&self) -> Option<&T> {
        self.render_object.as_any().downcast_ref::<T>()
    }

    /// Downcasts the RenderObject to a specific type (mutable).
    pub fn downcast_mut<T: RenderObject + 'static>(&mut self) -> Option<&mut T> {
        self.render_object.as_any_mut().downcast_mut::<T>()
    }

    /// Returns the current lifecycle state.
    #[inline]
    pub fn lifecycle(&self) -> RenderLifecycle {
        self.lifecycle
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

        // Root is always a relayout boundary
        let relayout_boundary = if parent.is_none() { Some(true) } else { None };

        RenderNode {
            render_object: self.render_object,
            parent,
            depth,
            children: Vec::new(),
            needs_layout: self.needs_layout,
            needs_paint: self.needs_paint,
            needs_compositing_bits_update: self.needs_compositing_bits_update,
            needs_semantics_update: self.needs_semantics_update,
            constraints: None,
            geometry: None,
            relayout_boundary,
            needs_compositing: self.needs_compositing,
            was_repaint_boundary: self.was_repaint_boundary,
            layer_handle: None,
            parent_data: None,
            lifecycle: RenderLifecycle::Attached,
            disposed: false,
            element_id: self.element_id,
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
            parent: None,
            depth: Depth::root(),
            children: Vec::new(),
            needs_layout: true,
            needs_paint: true,
            needs_compositing_bits_update: false,
            needs_semantics_update: true,
            constraints: None,
            geometry: None,
            relayout_boundary: None,
            needs_compositing: self.needs_compositing,
            was_repaint_boundary: self.was_repaint_boundary,
            layer_handle: None,
            parent_data: None,
            lifecycle: RenderLifecycle::Detached,
            disposed: false,
            element_id: self.element_id,
            _state: PhantomData,
        }
    }
}

// ============================================================================
// DEBUG IMPLEMENTATION
// ============================================================================

impl<S: NodeState> fmt::Debug for RenderNode<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(&format!("RenderNode<{}>", S::name()))
            .field("render_object", &self.render_object.debug_name())
            .field("lifecycle", &self.lifecycle)
            .field("needs_layout", &self.needs_layout)
            .field("needs_paint", &self.needs_paint)
            .field("disposed", &self.disposed)
            .finish_non_exhaustive()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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

    impl RenderObject for TestRenderObject {
        fn debug_name(&self) -> &'static str {
            "TestRenderObject"
        }
    }

    #[test]
    fn test_new_node() {
        let node = RenderNode::new(TestRenderObject);
        assert!(node.render_object().debug_name() == "TestRenderObject");
    }

    #[test]
    fn test_mount_unmount() {
        let node = RenderNode::new(TestRenderObject);
        let mounted = node.mount(None, Depth::root());

        assert!(mounted.is_root());
        assert!(mounted.is_attached());
        assert!(mounted.is_relayout_boundary()); // Root is always boundary

        let unmounted = mounted.unmount();
        assert!(!unmounted.render_object().is_repaint_boundary());
    }

    #[test]
    fn test_dirty_flags() {
        let node = RenderNode::new(TestRenderObject);
        let mut mounted = node.mount(None, Depth::root());

        assert!(mounted.needs_layout());
        assert!(mounted.needs_paint());

        mounted.clear_needs_layout();
        mounted.clear_needs_paint();

        assert!(!mounted.needs_layout());
        assert!(!mounted.needs_paint());

        mounted.mark_needs_layout();
        assert!(mounted.needs_layout());
    }

    #[test]
    fn test_dispose() {
        let node = RenderNode::new(TestRenderObject);
        let mut mounted = node.mount(None, Depth::root());

        assert!(!mounted.is_disposed());
        mounted.dispose();
        assert!(mounted.is_disposed());
    }

    #[test]
    fn test_reassemble() {
        let node = RenderNode::new(TestRenderObject);
        let mut mounted = node.mount(None, Depth::root());

        mounted.clear_needs_layout();
        mounted.clear_needs_paint();

        let _children = mounted.reassemble();

        assert!(mounted.needs_layout());
        assert!(mounted.needs_paint());
        assert!(mounted.needs_compositing_bits_update());
    }
}
