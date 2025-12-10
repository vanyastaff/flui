//! Core render object trait - minimal base for all render objects.
//!
//! This module provides the foundation for all render objects in FLUI:
//! - [`RenderObject`] - Base trait for all render objects (metadata only)
//!
//! # Architecture
//!
//! FLUI uses a two-level API:
//! - **RenderObject** - Minimal base trait (debug, flags, lifecycle)
//! - **RenderBox<A>/RenderSliver<A>** - Typed traits with context-based operations
//!
//! Layout, paint, and hit-test operations are handled by the typed traits
//! using context objects (BoxLayoutContext, BoxPaintContext, etc.).
//!
//! # Flutter Protocol Compliance
//!
//! | Flutter | FLUI | Notes |
//! |---------|------|-------|
//! | `RenderObject` | `RenderObject` | Base trait (minimal) |
//! | `RenderBox` | `RenderBox<A>` | Box protocol + arity |
//! | `RenderSliver` | `RenderSliver<A>` | Sliver protocol + arity |
//! | `sizedByParent` | `sized_by_parent()` | Optimization flag |
//! | `visitChildren()` | `visit_children()` | Tree traversal |

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use downcast_rs::{impl_downcast, DowncastSync};

use flui_foundation::{DiagnosticsProperty, RenderId};
use flui_interaction::HitTestResult;
use flui_painting::Canvas;
use flui_types::events::MouseCursor;
use flui_types::geometry::Matrix4;
use flui_types::semantics::{SemanticsAction, SemanticsProperties};
use flui_types::Offset;
use parking_lot::RwLock;

use crate::HitTestTree;

// ============================================================================
// LAYER HANDLE
// ============================================================================

/// Type alias for a shared layer reference.
///
/// This is used by repaint boundaries to cache their compositing layer.
/// The layer is wrapped in `Arc<RwLock<_>>` to allow:
/// - Shared ownership between render object and layer tree
/// - Interior mutability for updates during paint
/// - Thread-safe access from multiple threads
pub type LayerHandle = Arc<RwLock<LayerRef>>;

/// Reference to a compositor layer.
///
/// This wraps `flui_engine::Layer` to avoid direct dependency on
/// flui_engine in flui_rendering. The actual layer types are defined
/// in flui_engine and stored here as type-erased Any pointers.
#[derive(Debug)]
pub struct LayerRef {
    /// Type-erased layer (actually `flui_engine::Layer`)
    inner: Box<dyn Any + Send + Sync>,

    /// Layer type identifier for debugging
    layer_type: &'static str,

    /// Whether this layer needs recompositing
    needs_recomposite: bool,
}

impl LayerRef {
    /// Creates a new layer reference.
    pub fn new<T: Any + Send + Sync + 'static>(layer: T) -> Self {
        Self {
            layer_type: std::any::type_name::<T>(),
            inner: Box::new(layer),
            needs_recomposite: true,
        }
    }

    /// Gets the inner layer as a concrete type.
    pub fn get<T: Any + Send + Sync + 'static>(&self) -> Option<&T> {
        self.inner.downcast_ref::<T>()
    }

    /// Gets the inner layer as a mutable concrete type.
    pub fn get_mut<T: Any + Send + Sync + 'static>(&mut self) -> Option<&mut T> {
        self.inner.downcast_mut::<T>()
    }

    /// Returns the layer type name for debugging.
    pub fn layer_type(&self) -> &'static str {
        self.layer_type
    }

    /// Marks the layer as needing recomposition.
    pub fn mark_needs_recomposite(&mut self) {
        self.needs_recomposite = true;
    }

    /// Returns whether the layer needs recomposition.
    pub fn needs_recomposite(&self) -> bool {
        self.needs_recomposite
    }

    /// Clears the needs_recomposite flag after compositing.
    pub fn clear_needs_recomposite(&mut self) {
        self.needs_recomposite = false;
    }

    /// Replaces the inner layer with a new one.
    pub fn update<T: Any + Send + Sync + 'static>(&mut self, layer: T) {
        self.inner = Box::new(layer);
        self.layer_type = std::any::type_name::<T>();
        self.needs_recomposite = true;
    }
}

/// Creates a new `LayerHandle` wrapping the given layer.
pub fn new_layer_handle<T: Any + Send + Sync + 'static>(layer: T) -> LayerHandle {
    Arc::new(RwLock::new(LayerRef::new(layer)))
}

// ============================================================================
// RENDER OBJECT TRAIT
// ============================================================================

/// Base trait for all render objects (metadata only).
///
/// This is a minimal trait that provides:
/// - Debug information
/// - Tree traversal
/// - Lifecycle management
/// - Boundary flags
///
/// **Layout, paint, and hit-test operations are NOT part of this trait.**
/// Those are handled by protocol-specific traits:
/// - `RenderBox<A>` for box protocol
/// - `RenderSliver<A>` for sliver protocol
///
/// # Why Minimal?
///
/// The previous design had callback-based `perform_layout()` and `paint()`
/// methods here, but they were redundant with the typed `RenderBox<A>` API.
/// This led to confusion about which API to use.
///
/// Now:
/// - `RenderObject` = metadata, flags, lifecycle
/// - `RenderBox<A>` = context-based layout/paint/hit_test
///
/// # Examples
///
/// ```rust,ignore
/// #[derive(Debug)]
/// struct RenderPadding {
///     padding: EdgeInsets,
/// }
///
/// impl RenderObject for RenderPadding {
///     fn debug_name(&self) -> &'static str {
///         "RenderPadding"
///     }
/// }
///
/// impl RenderBox<Single> for RenderPadding {
///     fn layout(&mut self, ctx: BoxLayoutContext<'_, Single>) -> RenderResult<Size> {
///         let child_constraints = ctx.constraints.deflate(&self.padding);
///         let child_size = ctx.layout_single_child_with(|_| child_constraints)?;
///         Ok(child_size + self.padding.size())
///     }
///
///     fn paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
///         let offset = Offset::new(self.padding.left, self.padding.top);
///         ctx.paint_single_child(offset);
///     }
/// }
/// ```
pub trait RenderObject: DowncastSync + fmt::Debug {
    // ============================================================================
    // DEBUG METHODS
    // ============================================================================

    /// Returns human-readable debug name.
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Returns full type name with module path.
    fn type_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Returns short type name without module path.
    fn short_type_name(&self) -> &'static str {
        let full_name = std::any::type_name::<Self>();
        full_name.rsplit("::").next().unwrap_or(full_name)
    }

    /// Fills diagnostic properties (Flutter debugFillProperties).
    #[cfg(debug_assertions)]
    fn debug_fill_properties(&self, _properties: &mut Vec<DiagnosticsProperty>) {
        // Override to add custom properties
    }

    /// Paints debug visualization (Flutter debugPaint).
    #[cfg(debug_assertions)]
    fn debug_paint(&self, _canvas: &mut Canvas, _geometry: &dyn Any) {
        // Override for custom debug visualization
    }

    // ============================================================================
    // FLUTTER SIZED-BY-PARENT OPTIMIZATION
    // ============================================================================

    /// Whether size is determined solely by constraints (Flutter sizedByParent).
    ///
    /// If `true`, framework separates layout into:
    /// 1. Resize phase: `perform_resize()` with constraints only
    /// 2. Layout phase: position children
    fn sized_by_parent(&self) -> bool {
        false
    }

    // ============================================================================
    // TREE TRAVERSAL
    // ============================================================================

    /// Visits all immediate children (Flutter visitChildren).
    ///
    /// Note: This is for generic tree operations. Layout/paint use
    /// the arity-based children accessor from contexts.
    fn visit_children(&self, _visitor: &mut dyn FnMut(RenderId)) {
        // Default: no children (Leaf)
    }

    /// Counts immediate children.
    fn child_count(&self) -> usize {
        let mut count = 0;
        self.visit_children(&mut |_| count += 1);
        count
    }

    // ============================================================================
    // BOUNDARY FLAGS
    // ============================================================================

    /// Whether this is a relayout boundary (stops layout propagation).
    fn is_relayout_boundary(&self) -> bool {
        false
    }

    /// Whether this is a repaint boundary (enables layer caching).
    fn is_repaint_boundary(&self) -> bool {
        false
    }

    /// Whether this render object always needs compositing.
    fn always_needs_compositing(&self) -> bool {
        false
    }

    /// Returns whether this render object currently needs compositing.
    fn needs_compositing(&self) -> bool {
        self.always_needs_compositing()
    }

    // ============================================================================
    // INTERACTION
    // ============================================================================

    /// Whether this render object handles pointer events.
    fn handles_pointer_events(&self) -> bool {
        false
    }

    /// Returns the mouse cursor for this render object.
    fn cursor(&self) -> MouseCursor {
        MouseCursor::Defer
    }

    // ============================================================================
    // TRANSFORM METHODS (Flutter Protocol)
    // ============================================================================

    /// Applies the transform for painting a child to the given transform matrix.
    ///
    /// This method is called by `RenderTree::get_transform_to()` when computing
    /// coordinate transforms between render objects. Override this to apply custom
    /// transformations (rotation, scale, perspective, etc.) to children.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void applyPaintTransform(RenderObject child, Matrix4 transform) {
    ///   final BoxParentData childParentData = child.parentData as BoxParentData;
    ///   final Offset offset = childParentData.offset;
    ///   transform.translate(offset.dx, offset.dy);
    /// }
    /// ```
    ///
    /// # Default Implementation
    ///
    /// The default implementation applies only translation based on the child's
    /// offset from parent data. Render objects that apply additional transforms
    /// (e.g., `RenderTransform`, `RenderRotation`) should override this method.
    ///
    /// # Arguments
    ///
    /// * `child_id` - ID of the child render object
    /// * `transform` - Transform matrix to modify (in-place)
    /// * `tree` - Tree for accessing parent data and offsets
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Custom transform with rotation
    /// fn apply_paint_transform(
    ///     &self,
    ///     child_id: RenderId,
    ///     transform: &mut Matrix4,
    ///     tree: &dyn HitTestTree,
    /// ) {
    ///     // Apply translation (default behavior)
    ///     if let Some(offset) = tree.get_offset(child_id) {
    ///         *transform = Matrix4::translation(offset.dx, offset.dy, 0.0) * *transform;
    ///     }
    ///
    ///     // Apply rotation around center
    ///     let rotation = Matrix4::rotation_z(self.angle);
    ///     *transform = rotation * *transform;
    /// }
    /// ```
    fn apply_paint_transform(
        &self,
        child_id: RenderId,
        transform: &mut Matrix4,
        tree: &dyn HitTestTree,
    ) {
        // Default: Apply translation based on child's offset
        if let Some(offset) = tree.get_offset(child_id) {
            *transform = Matrix4::translation(offset.dx, offset.dy, 0.0) * *transform;
        }
    }

    /// Gets the transform from this render object to the given ancestor.
    ///
    /// **Deprecated**: This trait method is no longer used. Use `RenderTree::get_transform_to()`
    /// instead, which implements the full Flutter protocol with proper ancestor path building.
    ///
    /// # Migration
    ///
    /// ```rust,ignore
    /// // Old (trait method):
    /// let transform = obj.get_transform_to(id, ancestor, tree)?;
    ///
    /// // New (tree method):
    /// let transform = tree.get_transform_to(id, ancestor)?;
    /// ```
    fn get_transform_to(
        &self,
        _element_id: RenderId,
        _ancestor: Option<RenderId>,
        _tree: &dyn HitTestTree,
    ) -> Option<Matrix4> {
        // Stub: Use RenderTree::get_transform_to() instead
        Some(Matrix4::identity())
    }

    /// Converts a point from global coordinates to local coordinates.
    fn global_to_local(&self, global_point: Offset, _tree: &dyn HitTestTree) -> Offset {
        global_point
    }

    /// Converts a point from local coordinates to global coordinates.
    fn local_to_global(&self, local_point: Offset, _tree: &dyn HitTestTree) -> Offset {
        local_point
    }

    // ============================================================================
    // HIT TEST HELPERS
    // ============================================================================

    /// Helper to hit test a child with proper transform handling.
    fn hit_test_child(
        &self,
        child_id: RenderId,
        position: Offset,
        result: &mut HitTestResult,
        tree: &dyn HitTestTree,
    ) -> bool {
        let mut transform = Matrix4::identity();
        self.apply_paint_transform(child_id, &mut transform, tree);

        let child_position = if let Some(inverse) = transform.try_inverse() {
            let point = inverse.transform_point(position.dx, position.dy);
            Offset::new(point.0, point.1)
        } else {
            return false;
        };

        let guard = result.push_transform(transform);
        let hit = tree.hit_test(child_id, child_position, result);
        result.pop_to_depth(guard);

        hit
    }

    /// Helper to hit test a child with only offset (no rotation/scale).
    fn hit_test_child_with_offset(
        &self,
        child_id: RenderId,
        position: Offset,
        child_offset: Offset,
        result: &mut HitTestResult,
        tree: &dyn HitTestTree,
    ) -> bool {
        let child_position = position - child_offset;
        let guard = result.push_offset(child_offset);
        let hit = tree.hit_test(child_id, child_position, result);
        result.pop_to_depth(guard);
        hit
    }

    // ============================================================================
    // SEMANTICS / ACCESSIBILITY
    // ============================================================================

    /// Describes the semantic properties for accessibility.
    fn describe_semantics(&self) -> Option<SemanticsProperties> {
        None
    }

    /// Returns the set of semantic actions this render object supports.
    fn semantics_actions(&self) -> &[SemanticsAction] {
        &[]
    }

    /// Performs a semantic action triggered by accessibility services.
    fn perform_semantics_action(&mut self, _action: SemanticsAction) -> bool {
        false
    }

    /// Returns whether this render object is a semantics boundary.
    fn is_semantics_boundary(&self) -> bool {
        false
    }

    /// Returns whether this render object blocks semantics from its children.
    fn blocks_child_semantics(&self) -> bool {
        false
    }

    // ============================================================================
    // PARENT DATA
    // ============================================================================

    /// Creates default ParentData for a child of this render object.
    ///
    /// This is called by `setup_parent_data()` when the child has no parent data
    /// or has parent data of the wrong type.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// ParentData createParentData() {
    ///   return BoxParentData();
    /// }
    /// ```
    fn create_parent_data(&self) -> Box<dyn crate::ParentData> {
        Box::new(crate::BoxParentData::default())
    }

    /// Sets up the parent data for a child of this render object.
    ///
    /// This method is called when a child is adopted by this render object.
    /// It ensures the child has the correct type of parent data for this parent.
    ///
    /// The default implementation:
    /// 1. If child has no parent data, creates new parent data via `create_parent_data()`
    /// 2. If child has wrong type of parent data, replaces it with correct type
    /// 3. Otherwise, keeps existing parent data
    ///
    /// Override this method only if you need custom parent data setup logic.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void setupParentData(covariant RenderObject child) {
    ///   if (child.parentData is! BoxParentData) {
    ///     child.parentData = BoxParentData();
    ///   }
    /// }
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // In RenderStack
    /// fn setup_parent_data(&self, child_data: Option<&dyn ParentData>) -> Option<Box<dyn ParentData>> {
    ///     match child_data {
    ///         Some(data) if data.is::<StackParentData>() => None, // Already correct type
    ///         _ => Some(Box::new(StackParentData::default())), // Need to create/replace
    ///     }
    /// }
    /// ```
    fn setup_parent_data(&self, child_data: Option<&dyn crate::ParentData>) -> Option<Box<dyn crate::ParentData>> {
        // Default implementation: create new parent data if missing
        match child_data {
            Some(_) => None, // Keep existing parent data
            None => Some(self.create_parent_data()), // Create new parent data
        }
    }

    // ============================================================================
    // LAYER MANAGEMENT
    // ============================================================================

    /// Returns the compositing layer for this render object, if any.
    fn layer(&self) -> Option<&LayerHandle> {
        None
    }

    /// Sets the compositing layer for this render object.
    fn set_layer(&mut self, _layer: Option<LayerHandle>) {
        // Default: no-op
    }

    /// Called to update the composited layer before paint.
    fn update_composited_layer(&mut self, _offset: Offset) {
        // Default: no-op
    }

    /// Drops the layer and releases associated GPU resources.
    fn drop_layer(&mut self) {
        self.set_layer(None);
    }

    // ============================================================================
    // LIFECYCLE (Flutter Protocol)
    // ============================================================================

    /// Called when this render object is attached to a render tree.
    ///
    /// This is called by `RenderTree::add_child()` when the node is added to the tree.
    /// Override to perform initialization that requires being in the tree
    /// (e.g., registering for dirty tracking with PipelineOwner).
    ///
    /// **Important**: This is called for both the node being added AND all its descendants.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void attach(PipelineOwner owner) {
    ///   _owner = owner;
    ///
    ///   // Re-register dirty flags with owner
    ///   if (_needsLayout && _isRelayoutBoundary != null) {
    ///     _needsLayout = false;
    ///     markNeedsLayout();
    ///   }
    ///   if (_needsPaint && _layerHandle.layer != null) {
    ///     _needsPaint = false;
    ///     markNeedsPaint();
    ///   }
    ///   if (_needsCompositingBitsUpdate) {
    ///     _needsCompositingBitsUpdate = false;
    ///     markNeedsCompositingBitsUpdate();
    ///   }
    /// }
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn attach(&mut self) {
    ///     // Custom initialization logic
    ///     tracing::debug!("RenderBox attached to tree");
    /// }
    /// ```
    fn attach(&mut self) {
        // Default: no-op
        // Override to perform initialization when added to tree
    }

    /// Called when this render object is detached from the render tree.
    ///
    /// This is called by `RenderTree::remove_child()` when the node is removed from the tree.
    /// Override to perform cleanup that requires being in the tree
    /// (e.g., unregistering from PipelineOwner, releasing resources).
    ///
    /// **Important**: This is called for both the node being removed AND all its descendants.
    ///
    /// The default implementation calls `drop_layer()` to release GPU resources.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void detach() {
    ///   _owner = null;
    ///   // Dirty flags remain set so they can be re-registered on re-attach
    /// }
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn detach(&mut self) {
    ///     // Custom cleanup logic
    ///     self.cancel_animations();
    ///
    ///     // Call default to drop layers
    ///     self.drop_layer();
    /// }
    /// ```
    fn detach(&mut self) {
        // Default: drop compositing layers to release GPU resources
        self.drop_layer();
    }

    /// Called when the render object adopts a child.
    ///
    /// This is called by `RenderTree::add_child()` on the parent when a child is added.
    /// Use this to perform parent-specific child setup or tracking.
    ///
    /// **Note**: Parent data setup is handled separately by `setup_parent_data()`.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void adoptChild(RenderObject child) {
    ///   setupParentData(child);
    ///   markNeedsLayout();
    ///   markNeedsCompositingBitsUpdate();
    ///   markNeedsSemanticsUpdate();
    ///   child._parent = this;
    ///   if (attached) child.attach(_owner!);
    ///   redepthChild(child);
    /// }
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn adopt_child(&mut self, child_id: RenderId) {
    ///     // Track child in custom data structure
    ///     self.child_ids.push(child_id);
    /// }
    /// ```
    fn adopt_child(&mut self, _child_id: RenderId) {
        // Default: no-op
        // Override to track children or perform custom child setup
    }

    /// Called when the render object drops a child.
    ///
    /// This is called by `RenderTree::remove_child()` on the parent when a child is removed.
    /// Use this to perform parent-specific child cleanup or tracking.
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
    ///   markNeedsSemanticsUpdate();
    /// }
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn drop_child(&mut self, child_id: RenderId) {
    ///     // Remove child from tracking
    ///     self.child_ids.retain(|&id| id != child_id);
    /// }
    /// ```
    fn drop_child(&mut self, _child_id: RenderId) {
        // Default: no-op
        // Override to untrack children or perform custom child cleanup
    }
}

// Enable downcasting for RenderObject
impl_downcast!(sync RenderObject);

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestRenderObject;

    impl RenderObject for TestRenderObject {
        fn debug_name(&self) -> &'static str {
            "TestRenderObject"
        }
    }

    #[test]
    fn test_debug_name() {
        let obj = TestRenderObject;
        assert_eq!(obj.debug_name(), "TestRenderObject");
    }

    #[test]
    fn test_default_flags() {
        let obj = TestRenderObject;
        assert!(!obj.sized_by_parent());
        assert!(!obj.is_relayout_boundary());
        assert!(!obj.is_repaint_boundary());
        assert!(!obj.always_needs_compositing());
        assert!(!obj.handles_pointer_events());
    }

    #[test]
    fn test_downcast() {
        let obj: Box<dyn RenderObject> = Box::new(TestRenderObject);
        assert!(obj.as_any().downcast_ref::<TestRenderObject>().is_some());
    }

    #[test]
    fn test_layer_ref() {
        let layer = LayerRef::new(42u32);
        assert_eq!(layer.get::<u32>(), Some(&42));
        assert_eq!(layer.get::<String>(), None);
        assert!(layer.needs_recomposite());
    }
}
