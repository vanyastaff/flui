//! Core render object trait with enhanced safety and Flutter compliance.
//!
//! This module provides the foundation for all render objects in FLUI:
//! - [`RenderObject`] - Base trait for all render objects (protocol-agnostic)
//! - [`RenderObjectExt`] - Extension trait for safe downcasting
//!
//! # Flutter Protocol Compliance
//!
//! This implementation follows Flutter's RenderObject protocol with enhanced Rust safety:
//!
//! - **Two-level API**: Dyn-compatible + Typed methods
//! - **sizedByParent optimization**: Separate resize/layout phases
//! - **Relayout/Repaint boundaries**: Performance isolation
//! - **Tree traversal**: Type-safe visit_children
//! - **Debug utilities**: Rich diagnostics using flui_foundation

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use downcast_rs::{impl_downcast, DowncastSync};

use flui_foundation::{DiagnosticsProperty, ElementId};
use flui_interaction::{HitTestEntry, HitTestResult};
use flui_painting::Canvas;
use flui_types::events::MouseCursor;
use flui_types::geometry::Matrix4;
use flui_types::semantics::{SemanticsAction, SemanticsProperties};
use flui_types::{Offset, Rect, Size};
use parking_lot::RwLock;

use crate::core::{BoxConstraints, HitTestTree};
use crate::RenderResult;

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
/// This enum wraps `flui_engine::Layer` to avoid direct dependency on
/// flui_engine in flui_rendering. The actual layer types are defined
/// in flui_engine and stored here as type-erased Any pointers.
///
/// # Architecture
///
/// ```text
/// RenderObject
///   ├─ layer: Option<LayerHandle>
///   │    └─ LayerRef
///   │         └─ Any (type-erased flui_engine::Layer)
///   └─ needs_compositing flag
/// ```
///
/// # Usage
///
/// For repaint boundaries:
/// 1. Framework assigns layer before paint (`update_composited_layer`)
/// 2. Render object paints to layer canvas
/// 3. Layer is composited into parent
///
/// For non-repaint boundaries:
/// - `layer` is `None` (paints directly to parent's canvas)
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
    ///
    /// # Type Safety
    ///
    /// The caller must ensure `T` is a valid layer type from flui_engine.
    /// In practice, this is always `flui_engine::Layer`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_engine::{Layer, CanvasLayer};
    ///
    /// let canvas_layer = Layer::Canvas(CanvasLayer::new());
    /// let layer_ref = LayerRef::new(canvas_layer);
    /// ```
    pub fn new<T: Any + Send + Sync + 'static>(layer: T) -> Self {
        Self {
            layer_type: std::any::type_name::<T>(),
            inner: Box::new(layer),
            needs_recomposite: true,
        }
    }

    /// Gets the inner layer as a concrete type.
    ///
    /// Returns `None` if the stored type doesn't match `T`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(layer) = layer_ref.get::<flui_engine::Layer>() {
    ///     layer.render(&mut renderer);
    /// }
    /// ```
    pub fn get<T: Any + Send + Sync + 'static>(&self) -> Option<&T> {
        self.inner.downcast_ref::<T>()
    }

    /// Gets the inner layer as a mutable concrete type.
    ///
    /// Returns `None` if the stored type doesn't match `T`.
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
    ///
    /// Automatically marks for recomposition.
    pub fn update<T: Any + Send + Sync + 'static>(&mut self, layer: T) {
        self.inner = Box::new(layer);
        self.layer_type = std::any::type_name::<T>();
        self.needs_recomposite = true;
    }
}

/// Creates a new `LayerHandle` wrapping the given layer.
///
/// This is a convenience function for creating shared layer references.
///
/// # Example
///
/// ```rust,ignore
/// use flui_engine::{Layer, CanvasLayer};
///
/// let layer = Layer::Canvas(CanvasLayer::new());
/// let handle = new_layer_handle(layer);
///
/// // Can now share across render object and layer tree
/// let handle_clone = handle.clone();
/// ```
pub fn new_layer_handle<T: Any + Send + Sync + 'static>(layer: T) -> LayerHandle {
    Arc::new(RwLock::new(LayerRef::new(layer)))
}

// ============================================================================
// RENDER OBJECT TRAIT
// ============================================================================

/// Base trait for all render objects (protocol-agnostic).
///
/// Provides two complementary APIs:
/// 1. **Dyn-compatible methods** - Type-erased operations (perform_layout, paint, hit_test)
/// 2. **Typed protocol traits** - High-performance RenderBox<A>/RenderSliver<A>
///
/// # Flutter Relationship
///
/// | Flutter | FLUI | Notes |
/// |---------|------|-------|
/// | `RenderObject` | `RenderObject` | Base trait |
/// | `performLayout()` | `perform_layout()` | Dyn-compatible |
/// | `paint()` | `paint()` | Dyn-compatible |
/// | `hitTest()` | `hit_test()` | Dyn-compatible |
/// | `sizedByParent` | `sized_by_parent()` | Optimization flag |
/// | `performResize()` | `perform_resize()` | Resize phase |
/// | `visitChildren()` | `visit_children()` | Tree traversal |
///
/// # Examples
///
/// ```rust,ignore
/// #[derive(Debug)]
/// struct RenderPadding {
///     padding: EdgeInsets,
/// }
///
/// // With downcast-rs, no need to implement as_any/as_any_mut manually
/// impl RenderObject for RenderPadding {
///     fn perform_layout(
///         &mut self,
///         element_id: ElementId,
///         constraints: BoxConstraints,
///         tree: &mut dyn LayoutTree,
///     ) -> RenderResult<Size> {
///         // Layout single child
///         let child_id = tree.children(element_id).next().unwrap();
///         let child_constraints = constraints.deflate(self.padding);
///         let child_size = tree.perform_layout(child_id, child_constraints)?;
///
///         // Position child
///         tree.set_offset(child_id, self.padding.top_left());
///
///         // Return padded size
///         Ok(constraints.constrain(child_size + self.padding.size()))
///     }
///
///     fn paint(
///         &self,
///         element_id: ElementId,
///         offset: Offset,
///         size: Size,
///         canvas: &mut Canvas,
///         tree: &dyn PaintTree,
///     ) {
///         // Paint child at padded offset
///         let child_id = tree.children(element_id).next().unwrap();
///         if let Some(child_offset) = tree.get_offset(child_id) {
///             let _ = tree.perform_paint(child_id, offset + child_offset);
///         }
///     }
/// }
/// ```
pub trait RenderObject: DowncastSync + fmt::Debug {
    // ============================================================================
    // DYN-COMPATIBLE LAYOUT (Required for Box protocol)
    // ============================================================================

    /// Performs layout using box constraints (callback-based).
    ///
    /// This is the type-erased entry point for layout. It receives constraints
    /// and a callback for laying out children.
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter equivalent:
    /// @override
    /// void performLayout() {
    ///   size = constraints.biggest;
    ///   if (child != null) {
    ///     child.layout(constraints, parentUsesSize: true);
    ///   }
    /// }
    /// ```
    ///
    /// # Arguments
    ///
    /// * `element_id` - ID of this element in the tree
    /// * `constraints` - Layout constraints from parent
    /// * `layout_child` - Callback for laying out children (trait object for dyn-compatibility)
    ///
    /// # Callback Signature
    ///
    /// The callback takes `(child_id, child_constraints)` and returns child's size.
    /// This enables layout without self-referential borrows.
    ///
    /// # Returns
    ///
    /// Computed size that satisfies constraints.
    ///
    /// # Default Implementation
    ///
    /// Default returns constraints.smallest() for leaf nodes.
    /// Override for custom layout logic.
    ///
    /// # Performance
    ///
    /// Callback-based design eliminates:
    /// - Borrow checker conflicts
    /// - Multiple downcast operations
    /// - Complex unsafe patterns
    ///
    /// Trade-off: Dynamic dispatch on callback (negligible overhead vs safety gains)
    fn perform_layout(
        &mut self,
        _element_id: ElementId,
        constraints: BoxConstraints,
        _layout_child: &mut dyn FnMut(ElementId, BoxConstraints) -> RenderResult<Size>,
    ) -> RenderResult<Size> {
        // Default: leaf node returns minimum size
        Ok(constraints.smallest())
    }

    // ============================================================================
    // DYN-COMPATIBLE PAINT (Required)
    // ============================================================================

    /// Paints this render object to canvas (callback-based).
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter equivalent:
    /// @override
    /// void paint(PaintingContext context, Offset offset) {
    ///   if (child != null) {
    ///     context.paintChild(child, offset + childParentData.offset);
    ///   }
    /// }
    /// ```
    ///
    /// # Arguments
    ///
    /// * `element_id` - ID of this element
    /// * `offset` - Paint offset in global coordinates
    /// * `size` - Computed size from layout
    /// * `canvas` - Canvas to draw on
    /// * `paint_child` - Callback for painting children
    ///
    /// # Callback Signature
    ///
    /// The callback takes `(child_id, child_offset, canvas)` for painting children.
    /// This enables painting without self-referential borrows.
    ///
    /// # Default Implementation
    ///
    /// Default is no-op. Override this method to paint content.
    /// Child painting is handled via callback.
    fn paint(
        &self,
        _element_id: ElementId,
        _offset: Offset,
        _size: Size,
        _canvas: &mut Canvas,
        _paint_child: &mut dyn FnMut(ElementId, Offset, &mut Canvas),
    ) {
        // Default: no-op. Override this method to paint content.
        // Child painting is coordinated via paint_child callback.
    }

    // ============================================================================
    // DYN-COMPATIBLE HIT TEST (Optional)
    // ============================================================================

    /// Hit tests at position (callback-based).
    ///
    /// Default: rectangular bounds check + test children.
    ///
    /// # Arguments
    ///
    /// * `element_id` - ID of this element
    /// * `position` - Position to test in local coordinates
    /// * `size` - Computed size from layout
    /// * `result` - Hit test result accumulator
    /// * `hit_test_child` - Callback for testing children
    ///
    /// # Callback Signature
    ///
    /// The callback takes `(child_id, child_position, result)` and returns bool.
    /// This enables hit testing without self-referential borrows.
    ///
    /// # Returns
    ///
    /// `true` if hit, `false` otherwise.
    fn hit_test(
        &self,
        element_id: ElementId,
        position: Offset,
        size: Size,
        result: &mut HitTestResult,
        hit_test_child: &mut dyn FnMut(ElementId, Offset, &mut HitTestResult) -> bool,
    ) -> bool {
        // Check bounds
        if position.dx < 0.0
            || position.dx > size.width
            || position.dy < 0.0
            || position.dy > size.height
        {
            return false;
        }

        // Test children (reverse order = front to back)
        let mut any_hit = false;
        self.visit_children(&mut |child_id| {
            // Child offset will be provided by the callback
            // The callback is responsible for position transformation
            if hit_test_child(child_id, position, result) {
                any_hit = true;
            }
        });

        // Add self if hit
        if any_hit || self.handles_pointer_events() {
            let bounds = Rect::from_min_size(Offset::ZERO, size);
            result.add(HitTestEntry::new(element_id, position, bounds));
            return true;
        }

        false
    }

    // ============================================================================
    // DEBUG METHODS (Optional)
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
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn debug_fill_properties(&self, properties: &mut Vec<DiagnosticsProperty>) {
    ///     properties.push(DiagnosticsProperty::new("padding", self.padding));
    ///     properties.push(DiagnosticsProperty::new("alignment", self.alignment));
    /// }
    /// ```
    #[cfg(debug_assertions)]
    fn debug_fill_properties(&self, _properties: &mut Vec<DiagnosticsProperty>) {
        // Override to add custom properties
    }

    /// Paints debug visualization (Flutter debugPaint).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn debug_paint(&self, canvas: &mut Canvas, geometry: &dyn Any) {
    ///     if let Some(size) = geometry.downcast_ref::<Size>() {
    ///         let rect = Rect::from_xywh(0.0, 0.0, size.width, size.height);
    ///         canvas.rect(rect, &Paint::stroke(Color::RED, 1.0));
    ///     }
    /// }
    /// ```
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
    /// 2. Layout phase: `perform_layout()` to position children
    ///
    /// # When to return true
    ///
    /// - Size = f(constraints) only (children don't affect size)
    /// - Examples: SizedBox, ConstrainedBox, LimitedBox
    ///
    /// # Performance
    ///
    /// When constraints unchanged:
    /// - ✅ Skip `perform_resize()` entirely
    /// - ✅ Only run `perform_layout()` if children dirty
    fn sized_by_parent(&self) -> bool {
        false // Default: size depends on children
    }

    /// Computes size from constraints only (Flutter performResize).
    ///
    /// Called when `sized_by_parent() == true`. Must be pure function of constraints.
    ///
    /// # Contract
    ///
    /// - MUST NOT access children
    /// - MUST NOT read cached child sizes
    /// - MUST set size fields for later use
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn perform_resize(&mut self, constraints: &dyn Any) -> RenderResult<()> {
    ///     let box_constraints = constraints.downcast_ref::<BoxConstraints>()?;
    ///     self.cached_size = box_constraints.biggest();
    ///     Ok(())
    /// }
    /// ```
    fn perform_resize(&mut self, _constraints: &dyn Any) -> RenderResult<()> {
        Ok(()) // Default: no-op
    }

    // ============================================================================
    // TREE TRAVERSAL
    // ============================================================================

    /// Visits all immediate children (Flutter visitChildren).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Single child
    /// fn visit_children(&self, visitor: &mut dyn FnMut(ElementId)) {
    ///     if let Some(child) = self.child {
    ///         visitor(child);
    ///     }
    /// }
    ///
    /// // Multiple children
    /// fn visit_children(&self, visitor: &mut dyn FnMut(ElementId)) {
    ///     for &child_id in &self.children {
    ///         visitor(child_id);
    ///     }
    /// }
    /// ```
    fn visit_children(&self, _visitor: &mut dyn FnMut(ElementId)) {
        // Default: no children (Leaf)
    }

    /// Counts immediate children (derived from visit_children).
    ///
    /// Default: O(n). Override for O(1) if cached.
    fn child_count(&self) -> usize {
        let mut count = 0;
        self.visit_children(&mut |_| count += 1);
        count
    }

    // ============================================================================
    // INTRINSIC PROPERTIES (Optional)
    // ============================================================================

    /// Natural size independent of constraints.
    ///
    /// # When to override
    ///
    /// - Image: intrinsic image dimensions
    /// - Text: natural text size
    /// - Icon: natural icon size
    fn intrinsic_size(&self) -> Option<Size> {
        None
    }

    /// Baseline offset for text alignment.
    fn baseline_offset(&self) -> Option<f32> {
        None
    }

    // ============================================================================
    // BOUNDARY FLAGS (Optimization)
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
    ///
    /// Override to return `true` if this render object always requires a
    /// compositing layer, regardless of its children. This is typically
    /// needed for render objects that use effects requiring offscreen
    /// rendering (e.g., opacity, backdrop filter, shader effects).
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter equivalent:
    /// @override
    /// bool get alwaysNeedsCompositing => true;
    /// ```
    ///
    /// # When to Return True
    ///
    /// - **RenderOpacity** with opacity < 1.0
    /// - **RenderBackdropFilter** - requires backdrop capture
    /// - **RenderShaderMask** - requires shader composition
    /// - **RenderColorFilter** - requires color matrix application
    /// - **RenderClipPath** with complex path - may need layer
    ///
    /// # Performance Impact
    ///
    /// Returning `true` forces layer creation, which:
    /// - Increases GPU memory usage
    /// - Adds compositing overhead
    /// - But enables proper isolation of effects
    ///
    /// Only return `true` when absolutely necessary.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Opacity render object
    /// fn always_needs_compositing(&self) -> bool {
    ///     self.opacity < 1.0
    /// }
    ///
    /// // Backdrop filter always needs compositing
    /// fn always_needs_compositing(&self) -> bool {
    ///     true
    /// }
    /// ```
    fn always_needs_compositing(&self) -> bool {
        false
    }

    // ============================================================================
    // INTERACTION
    // ============================================================================

    /// Whether this render object handles pointer events.
    fn handles_pointer_events(&self) -> bool {
        false
    }

    // ============================================================================
    // TRANSFORM METHODS (Flutter applyPaintTransform)
    // ============================================================================

    /// Applies the transform for painting a child to the given matrix.
    ///
    /// This method captures how this render object transforms its children.
    /// Override if your render object applies transforms (rotation, scale, etc.)
    /// when painting children.
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter equivalent:
    /// @override
    /// void applyPaintTransform(RenderObject child, Matrix4 transform) {
    ///   final BoxParentData childParentData = child.parentData as BoxParentData;
    ///   transform.translate(childParentData.offset.dx, childParentData.offset.dy);
    /// }
    /// ```
    ///
    /// # Default Implementation
    ///
    /// Default applies only the child's offset translation (from ParentData).
    /// Override for custom transforms like rotation, scale, or perspective.
    ///
    /// # Use Cases
    ///
    /// - **RenderTransform**: Applies rotation/scale/skew matrix
    /// - **RenderFractionalTranslation**: Applies fractional offset
    /// - **RenderRotatedBox**: Applies 90° rotation increments
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // For a Transform render object:
    /// fn apply_paint_transform(&self, child_id: ElementId, transform: &mut Matrix4, tree: &dyn HitTestTree) {
    ///     // Apply this object's transform
    ///     *transform = self.transform * *transform;
    ///
    ///     // Then apply child offset
    ///     if let Some(offset) = tree.get_offset(child_id) {
    ///         *transform = Matrix4::translation(offset.dx, offset.dy, 0.0) * *transform;
    ///     }
    /// }
    /// ```
    fn apply_paint_transform(
        &self,
        child_id: ElementId,
        transform: &mut Matrix4,
        tree: &dyn HitTestTree,
    ) {
        // Default: apply only child offset translation
        if let Some(offset) = tree.get_offset(child_id) {
            *transform = Matrix4::translation(offset.dx, offset.dy, 0.0) * *transform;
        }
    }

    /// Gets the transform from this render object to the given ancestor.
    ///
    /// Returns the composed transform that maps points from this object's
    /// coordinate space to the ancestor's coordinate space.
    ///
    /// # Arguments
    ///
    /// * `ancestor` - Target ancestor (None = root/global coordinates)
    /// * `tree` - Tree for accessing parent chain
    ///
    /// # Returns
    ///
    /// Transform matrix, or None if ancestor is not in parent chain.
    fn get_transform_to(
        &self,
        _element_id: ElementId,
        _ancestor: Option<ElementId>,
        _tree: &dyn HitTestTree,
    ) -> Option<Matrix4> {
        // Default implementation - override for proper tree traversal
        Some(Matrix4::identity())
    }

    /// Converts a point from global coordinates to local coordinates.
    ///
    /// # Arguments
    ///
    /// * `global_point` - Point in global (root) coordinate space
    /// * `tree` - Tree for accessing transforms
    ///
    /// # Returns
    ///
    /// Point in this object's local coordinate space.
    fn global_to_local(&self, global_point: Offset, _tree: &dyn HitTestTree) -> Offset {
        // Default: identity transform (no conversion)
        global_point
    }

    /// Converts a point from local coordinates to global coordinates.
    ///
    /// # Arguments
    ///
    /// * `local_point` - Point in this object's local coordinate space
    /// * `tree` - Tree for accessing transforms
    ///
    /// # Returns
    ///
    /// Point in global (root) coordinate space.
    fn local_to_global(&self, local_point: Offset, _tree: &dyn HitTestTree) -> Offset {
        // Default: identity transform (no conversion)
        local_point
    }

    // ============================================================================
    // HIT TEST HELPERS
    // ============================================================================

    /// Helper to hit test a child with proper transform handling.
    ///
    /// This is the recommended way to hit test children when your render object
    /// applies transforms. It:
    /// 1. Pushes the child's transform onto the HitTestResult stack
    /// 2. Transforms the position to child's coordinate space
    /// 3. Calls hit_test on the child
    /// 4. Pops the transform (via guard)
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter equivalent in hitTestChildren:
    /// final Matrix4 transform = Matrix4.identity();
    /// applyPaintTransform(child, transform);
    /// return result.addWithPaintTransform(
    ///   transform: transform,
    ///   position: position,
    ///   hitTest: (result, position) => child.hitTest(result, position: position),
    /// );
    /// ```
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn hit_test(&self, element_id: ElementId, position: Offset,
    ///             result: &mut HitTestResult, tree: &dyn HitTestTree) -> bool {
    ///     let mut hit = false;
    ///     self.visit_children(&mut |child_id| {
    ///         if self.hit_test_child(child_id, position, result, tree) {
    ///             hit = true;
    ///         }
    ///     });
    ///     hit
    /// }
    /// ```
    fn hit_test_child(
        &self,
        child_id: ElementId,
        position: Offset,
        result: &mut HitTestResult,
        tree: &dyn HitTestTree,
    ) -> bool {
        // Build transform for this child
        let mut transform = Matrix4::identity();
        self.apply_paint_transform(child_id, &mut transform, tree);

        // Try to invert transform to convert position to child space
        let child_position = if let Some(inverse) = transform.try_inverse() {
            let point = inverse.transform_point(position.dx, position.dy);
            Offset::new(point.0, point.1)
        } else {
            // Transform not invertible (degenerate) - child not hittable
            return false;
        };

        // Push transform onto result stack
        let guard = result.push_transform(transform);

        // Hit test child
        let hit = tree.hit_test(child_id, child_position, result);

        // Pop transform
        result.pop_to_depth(guard);

        hit
    }

    /// Helper to hit test a child with only offset (no rotation/scale).
    ///
    /// This is a simplified version of `hit_test_child` for the common case
    /// where children are only translated (not rotated or scaled).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn hit_test(&self, element_id: ElementId, position: Offset,
    ///             result: &mut HitTestResult, tree: &dyn HitTestTree) -> bool {
    ///     self.visit_children(&mut |child_id| {
    ///         if let Some(offset) = tree.get_offset(child_id) {
    ///             self.hit_test_child_with_offset(child_id, position, offset, result, tree);
    ///         }
    ///     });
    ///     // ...
    /// }
    /// ```
    fn hit_test_child_with_offset(
        &self,
        child_id: ElementId,
        position: Offset,
        child_offset: Offset,
        result: &mut HitTestResult,
        tree: &dyn HitTestTree,
    ) -> bool {
        // Transform position to child space
        let child_position = position - child_offset;

        // Push offset transform
        let guard = result.push_offset(child_offset);

        // Hit test child
        let hit = tree.hit_test(child_id, child_position, result);

        // Pop transform
        result.pop_to_depth(guard);

        hit
    }

    /// Returns the mouse cursor for this render object.
    ///
    /// Override to provide a custom cursor when the mouse hovers over this object.
    /// The cursor is resolved by the hit test system and applied by MouseTracker.
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter uses MouseRegion widget, but cursor can also be on RenderObject:
    /// @override
    /// MouseCursor get cursor => SystemMouseCursors.click;
    /// ```
    ///
    /// # Default Implementation
    ///
    /// Returns `MouseCursor::Defer`, which defers cursor selection to the
    /// next object in the hit test chain.
    ///
    /// # Common Cursors
    ///
    /// - `MouseCursor::Defer` - Defer to parent (default)
    /// - `MouseCursor::BASIC` - Arrow cursor
    /// - `MouseCursor::CLICK` - Pointer/hand for clickable elements
    /// - `MouseCursor::TEXT` - I-beam for text fields
    /// - `MouseCursor::GRAB` / `MouseCursor::GRABBING` - For draggable items
    /// - `MouseCursor::FORBIDDEN` - Not allowed
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Button render object
    /// fn cursor(&self) -> MouseCursor {
    ///     if self.enabled {
    ///         MouseCursor::CLICK
    ///     } else {
    ///         MouseCursor::FORBIDDEN
    ///     }
    /// }
    ///
    /// // Text field render object
    /// fn cursor(&self) -> MouseCursor {
    ///     MouseCursor::TEXT
    /// }
    ///
    /// // Draggable render object
    /// fn cursor(&self) -> MouseCursor {
    ///     if self.is_dragging {
    ///         MouseCursor::GRABBING
    ///     } else {
    ///         MouseCursor::GRAB
    ///     }
    /// }
    /// ```
    fn cursor(&self) -> MouseCursor {
        MouseCursor::Defer
    }

    // ============================================================================
    // SEMANTICS / ACCESSIBILITY
    // ============================================================================

    /// Describes the semantic properties of this render object for accessibility.
    ///
    /// Override to provide semantic information for screen readers and other
    /// assistive technologies. This is similar to Flutter's `describeSemanticsConfiguration`.
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter equivalent:
    /// @override
    /// void describeSemanticsConfiguration(SemanticsConfiguration config) {
    ///   super.describeSemanticsConfiguration(config);
    ///   config.isSemanticBoundary = true;
    ///   config.label = 'Button: Submit';
    /// }
    /// ```
    ///
    /// # When to Override
    ///
    /// - Interactive elements (buttons, links, form controls)
    /// - Content with meaning (images with descriptions, text)
    /// - Structural elements (headers, lists)
    ///
    /// # Default Implementation
    ///
    /// Returns `None`, indicating no semantic information. Non-semantic
    /// container nodes should keep the default.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Button render object
    /// fn describe_semantics(&self) -> Option<SemanticsProperties> {
    ///     Some(SemanticsProperties::new()
    ///         .with_role(SemanticsRole::Button)
    ///         .with_label(&self.label)
    ///         .with_enabled(self.enabled))
    /// }
    ///
    /// // Image with alt text
    /// fn describe_semantics(&self) -> Option<SemanticsProperties> {
    ///     self.alt_text.as_ref().map(|alt| {
    ///         SemanticsProperties::new()
    ///             .with_role(SemanticsRole::Image)
    ///             .with_label(alt)
    ///     })
    /// }
    ///
    /// // Slider with value
    /// fn describe_semantics(&self) -> Option<SemanticsProperties> {
    ///     Some(SemanticsProperties::new()
    ///         .with_role(SemanticsRole::Slider)
    ///         .with_value(format!("{:.0}%", self.value * 100.0))
    ///         .with_hint("Swipe up or down to adjust"))
    /// }
    /// ```
    fn describe_semantics(&self) -> Option<SemanticsProperties> {
        None
    }

    /// Returns the set of semantic actions this render object supports.
    ///
    /// Override to indicate which accessibility actions can be performed.
    /// The framework will only allow actions that are listed here.
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter equivalent in SemanticsConfiguration:
    /// config.onTap = () => handleTap();
    /// config.onLongPress = () => handleLongPress();
    /// ```
    ///
    /// # Default Implementation
    ///
    /// Returns an empty slice, indicating no actions are supported.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Button supports tap
    /// fn semantics_actions(&self) -> &[SemanticsAction] {
    ///     static ACTIONS: &[SemanticsAction] = &[SemanticsAction::Tap];
    ///     ACTIONS
    /// }
    ///
    /// // Slider supports increase/decrease
    /// fn semantics_actions(&self) -> &[SemanticsAction] {
    ///     static ACTIONS: &[SemanticsAction] = &[
    ///         SemanticsAction::Tap,
    ///         SemanticsAction::Increase,
    ///         SemanticsAction::Decrease,
    ///     ];
    ///     ACTIONS
    /// }
    ///
    /// // Scrollable list
    /// fn semantics_actions(&self) -> &[SemanticsAction] {
    ///     static ACTIONS: &[SemanticsAction] = &[
    ///         SemanticsAction::ScrollUp,
    ///         SemanticsAction::ScrollDown,
    ///     ];
    ///     ACTIONS
    /// }
    /// ```
    fn semantics_actions(&self) -> &[SemanticsAction] {
        &[]
    }

    /// Performs a semantic action triggered by accessibility services.
    ///
    /// Called when a user performs an action via screen reader or other
    /// assistive technology. Override to handle the specific actions
    /// returned by `semantics_actions()`.
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter handles this via callbacks in SemanticsConfiguration:
    /// config.onTap = () => handleTap();
    /// config.onIncrease = () => handleIncrease();
    /// ```
    ///
    /// # Arguments
    ///
    /// * `action` - The action requested by the accessibility service
    ///
    /// # Returns
    ///
    /// `true` if the action was handled, `false` otherwise.
    ///
    /// # Default Implementation
    ///
    /// Returns `false` (action not handled). Override to handle actions.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn perform_semantics_action(&mut self, action: SemanticsAction) -> bool {
    ///     match action {
    ///         SemanticsAction::Tap => {
    ///             self.handle_tap();
    ///             true
    ///         }
    ///         SemanticsAction::Increase => {
    ///             self.value = (self.value + 0.1).min(1.0);
    ///             true
    ///         }
    ///         SemanticsAction::Decrease => {
    ///             self.value = (self.value - 0.1).max(0.0);
    ///             true
    ///         }
    ///         _ => false,
    ///     }
    /// }
    /// ```
    fn perform_semantics_action(&mut self, _action: SemanticsAction) -> bool {
        false
    }

    /// Returns whether this render object is a semantics boundary.
    ///
    /// A semantics boundary creates a new node in the semantics tree.
    /// Override to return `true` for elements that should be individually
    /// focusable by screen readers.
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter equivalent:
    /// config.isSemanticBoundary = true;
    /// ```
    ///
    /// # When to Return True
    ///
    /// - Interactive elements (buttons, links, form controls)
    /// - Important content (images with alt text, headers)
    /// - Elements that should be individually navigable
    ///
    /// # When to Return False
    ///
    /// - Container/layout elements (Row, Column, Padding)
    /// - Decorative elements (borders, backgrounds)
    /// - Elements that should merge with parent semantics
    ///
    /// # Default Implementation
    ///
    /// Returns `false`. Most render objects are not semantic boundaries.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Button is a semantics boundary
    /// fn is_semantics_boundary(&self) -> bool {
    ///     true
    /// }
    ///
    /// // Decorative icon is not
    /// fn is_semantics_boundary(&self) -> bool {
    ///     self.semantic_label.is_some() // Only if it has a label
    /// }
    /// ```
    fn is_semantics_boundary(&self) -> bool {
        false
    }

    /// Returns whether this render object blocks semantics from its children.
    ///
    /// When `true`, the accessibility tree will not include this node's
    /// descendants. Use for elements where children are purely decorative
    /// or where child semantics would be confusing.
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter equivalent:
    /// config.isBlockingSemanticsOfPreviouslyPaintedNodes = true;
    /// // Or using ExcludeSemantics widget
    /// ```
    ///
    /// # When to Return True
    ///
    /// - Custom painted content where children are visual only
    /// - Modal overlays that should hide background content
    /// - Decorative containers with non-semantic children
    ///
    /// # Default Implementation
    ///
    /// Returns `false`. Children are normally included in semantics.
    fn blocks_child_semantics(&self) -> bool {
        false
    }

    // ============================================================================
    // PARENT DATA (Flutter setupParentData)
    // ============================================================================

    /// Creates default ParentData for a child of this render object.
    ///
    /// Called by the framework when a child is added to this parent. The parent
    /// returns the appropriate ParentData type that the child should use.
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter equivalent:
    /// @override
    /// void setupParentData(RenderObject child) {
    ///   if (child.parentData is! BoxParentData) {
    ///     child.parentData = BoxParentData();
    ///   }
    /// }
    /// ```
    ///
    /// # When to Override
    ///
    /// Override when your render object needs specific ParentData:
    /// - `RenderFlex` → `FlexParentData` (flex factor, fit)
    /// - `RenderStack` → `StackParentData` (positioned rect)
    /// - `RenderViewport` → `SliverPhysicalParentData` (paint offset)
    ///
    /// # Default Implementation
    ///
    /// Returns `BoxParentData` which is suitable for most box layouts.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Custom ParentData for Flex layout
    /// fn create_parent_data(&self) -> Box<dyn ParentData> {
    ///     Box::new(FlexParentData::new())
    /// }
    ///
    /// // Custom ParentData for Stack layout
    /// fn create_parent_data(&self) -> Box<dyn ParentData> {
    ///     Box::new(StackParentData::new())
    /// }
    /// ```
    fn create_parent_data(&self) -> Box<dyn crate::core::ParentData> {
        Box::new(crate::core::BoxParentData::default())
    }

    // ============================================================================
    // LAYER MANAGEMENT (Flutter layer property)
    // ============================================================================

    /// Returns the compositing layer for this render object, if any.
    ///
    /// This is the FLUI equivalent of Flutter's `RenderObject.layer` property.
    /// For render objects that are repaint boundaries, this returns the
    /// cached compositing layer that isolates this subtree's painting.
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter equivalent:
    /// ContainerLayer? get layer => _layer;
    /// set layer(ContainerLayer? newLayer) {
    ///   _layer = newLayer;
    /// }
    /// ```
    ///
    /// # When This Is Non-None
    ///
    /// - **Repaint boundaries**: Framework assigns `OffsetLayer` before paint
    /// - **Effects requiring compositing**: `RenderOpacity`, `RenderBackdropFilter`
    /// - **Transform/clip with layering**: Some transform effects
    ///
    /// # When This Is None
    ///
    /// - Non-repaint boundary render objects (most widgets)
    /// - After `needs_compositing` becomes false (layer cleared)
    ///
    /// # Default Implementation
    ///
    /// Returns `None`. Override only for render objects that manage their
    /// own layer (repaint boundaries, effect render objects).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Repaint boundary render object
    /// struct RenderRepaintBoundary {
    ///     layer: Option<LayerHandle>,
    /// }
    ///
    /// impl RenderObject for RenderRepaintBoundary {
    ///     fn layer(&self) -> Option<&LayerHandle> {
    ///         self.layer.as_ref()
    ///     }
    ///
    ///     fn set_layer(&mut self, layer: Option<LayerHandle>) {
    ///         self.layer = layer;
    ///     }
    ///
    ///     fn is_repaint_boundary(&self) -> bool {
    ///         true
    ///     }
    /// }
    /// ```
    fn layer(&self) -> Option<&LayerHandle> {
        None
    }

    /// Sets the compositing layer for this render object.
    ///
    /// This is called by the framework to assign a layer to repaint boundaries
    /// before paint, or to clear the layer when compositing is no longer needed.
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter equivalent:
    /// set layer(ContainerLayer? newLayer) {
    ///   assert(
    ///     !isRepaintBoundary || (newLayer == null) == (layer == null),
    ///     'Repaint boundaries cannot have their layer changed.',
    ///   );
    ///   _layer = newLayer;
    /// }
    /// ```
    ///
    /// # Framework Behavior
    ///
    /// 1. **Before first paint** (repaint boundary):
    ///    - Framework calls `set_layer(Some(new_layer_handle(...)))`
    ///    - Creates `OffsetLayer` (or appropriate layer type)
    ///
    /// 2. **During paint** (repaint boundary):
    ///    - Paint uses `layer()` to get the layer
    ///    - Paints into layer's canvas
    ///
    /// 3. **When compositing not needed**:
    ///    - Framework calls `set_layer(None)` to release GPU resources
    ///
    /// # Default Implementation
    ///
    /// No-op. Override only for render objects that store their own layer.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn set_layer(&mut self, layer: Option<LayerHandle>) {
    ///     self.layer = layer;
    /// }
    /// ```
    fn set_layer(&mut self, _layer: Option<LayerHandle>) {
        // Default: no-op (non-repaint boundary render objects don't store layers)
    }

    /// Called by the framework to update the composited layer before paint.
    ///
    /// This is the FLUI equivalent of Flutter's `updateCompositedLayer()`.
    /// Override this to configure layer properties (offset, clip, transform)
    /// before the layer is used for painting.
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter equivalent:
    /// void updateCompositedLayer({required covariant OffsetLayer? oldLayer}) {
    ///   // Default implementation just sets offset
    ///   layer = oldLayer ?? OffsetLayer();
    ///   layer.offset = paintBounds.topLeft;
    /// }
    /// ```
    ///
    /// # When Called
    ///
    /// - Before paint, if this is a repaint boundary
    /// - After `needs_compositing` changes
    /// - When layer needs reconfiguration
    ///
    /// # Default Implementation
    ///
    /// No-op. Override for custom layer configuration.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn update_composited_layer(&mut self, offset: Offset) {
    ///     if let Some(layer_handle) = self.layer() {
    ///         let mut layer_ref = layer_handle.write();
    ///         // Update layer properties
    ///         if let Some(offset_layer) = layer_ref.get_mut::<OffsetLayer>() {
    ///             offset_layer.set_offset(offset);
    ///         }
    ///     }
    /// }
    /// ```
    fn update_composited_layer(&mut self, _offset: Offset) {
        // Default: no-op
    }

    /// Returns whether this render object currently needs compositing.
    ///
    /// This is distinct from `always_needs_compositing()`:
    /// - `always_needs_compositing()` - Static property of the render object type
    /// - `needs_compositing()` - Dynamic state based on self and descendants
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter equivalent:
    /// bool get needsCompositing => _needsCompositing;
    /// ```
    ///
    /// # Computation
    ///
    /// Compositing is needed when ANY of:
    /// - `always_needs_compositing()` returns true
    /// - Any descendant `needs_compositing()` returns true
    ///
    /// # Default Implementation
    ///
    /// Delegates to `always_needs_compositing()`. Override if you track
    /// descendant compositing status.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn needs_compositing(&self) -> bool {
    ///     self.always_needs_compositing() || self.child_needs_compositing
    /// }
    /// ```
    fn needs_compositing(&self) -> bool {
        self.always_needs_compositing()
    }

    /// Drops the layer and releases associated GPU resources.
    ///
    /// Called when:
    /// - Render object is disposed
    /// - `needs_compositing` becomes false
    /// - Layer needs to be recreated
    ///
    /// # Default Implementation
    ///
    /// Calls `set_layer(None)`.
    fn drop_layer(&mut self) {
        self.set_layer(None);
    }

    // ============================================================================
    // LIFECYCLE (Flutter attach/detach)
    // ============================================================================

    /// Called when this render object is attached to a render tree.
    ///
    /// This is the FLUI equivalent of Flutter's `RenderObject.attach()`.
    /// Override this to:
    /// - Set up tickers for animations
    /// - Register listeners
    /// - Initialize resources that require tree context
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter equivalent:
    /// @override
    /// void attach(PipelineOwner owner) {
    ///   super.attach(owner);
    ///   _ticker = owner.createTicker(_onTick);
    /// }
    /// ```
    ///
    /// # When Called
    ///
    /// - When element is mounted to the tree
    /// - When element is re-attached after being removed
    /// - Before first layout
    ///
    /// # Important
    ///
    /// - Must call `attach_children()` or visit children manually
    /// - Ticker/scheduler access is available after attach
    /// - Layer assignment happens after attach
    ///
    /// # Default Implementation
    ///
    /// No-op. Override for render objects that need lifecycle hooks.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Animated render object
    /// fn attach(&mut self, owner: &dyn PipelineOwner) {
    ///     // Create ticker for animations
    ///     if let Some(scheduler) = owner.scheduler() {
    ///         self.ticker = Some(ScheduledTicker::new(scheduler));
    ///     }
    /// }
    ///
    /// fn detach(&mut self) {
    ///     // Stop and release ticker
    ///     if let Some(ticker) = self.ticker.take() {
    ///         ticker.stop();
    ///     }
    /// }
    /// ```
    fn attach(&mut self) {
        // Default: no-op
    }

    /// Called when this render object is detached from the render tree.
    ///
    /// This is the FLUI equivalent of Flutter's `RenderObject.detach()`.
    /// Override this to:
    /// - Stop and dispose tickers
    /// - Unregister listeners
    /// - Release resources
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter equivalent:
    /// @override
    /// void detach() {
    ///   _ticker?.dispose();
    ///   _ticker = null;
    ///   super.detach();
    /// }
    /// ```
    ///
    /// # When Called
    ///
    /// - When element is unmounted from tree
    /// - Before element is disposed
    /// - When subtree is moved (detach then re-attach)
    ///
    /// # Important
    ///
    /// - Must call `detach_children()` or visit children manually
    /// - After detach, scheduler/ticker access is no longer valid
    /// - Layer is typically cleared during detach
    ///
    /// # Default Implementation
    ///
    /// Calls `drop_layer()` to release GPU resources.
    fn detach(&mut self) {
        self.drop_layer();
    }

    /// Called when the render object should adopt a child.
    ///
    /// Override this to:
    /// - Set up parent data on the child
    /// - Mark layout as dirty if needed
    /// - Track children in data structures
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter equivalent:
    /// @override
    /// void adoptChild(RenderObject child) {
    ///   setupParentData(child);
    ///   super.adoptChild(child);
    /// }
    /// ```
    ///
    /// # Default Implementation
    ///
    /// No-op. Override for render objects that manage children.
    fn adopt_child(&mut self, _child_id: ElementId) {
        // Default: no-op
    }

    /// Called when the render object should drop a child.
    ///
    /// Override this to:
    /// - Clean up parent data
    /// - Remove from tracking data structures
    /// - Mark layout as dirty if needed
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter equivalent:
    /// @override
    /// void dropChild(RenderObject child) {
    ///   child.parentData = null;
    ///   super.dropChild(child);
    /// }
    /// ```
    ///
    /// # Default Implementation
    ///
    /// No-op. Override for render objects that manage children.
    fn drop_child(&mut self, _child_id: ElementId) {
        // Default: no-op
    }

    // ============================================================================
    // TICKER SUPPORT (Animation integration)
    // ============================================================================

    /// Returns whether this render object uses tickers for animation.
    ///
    /// Override to return `true` if this render object creates tickers
    /// during `attach()`. This helps the framework optimize ticker management.
    ///
    /// # When to Return True
    ///
    /// - `RenderAnimatedOpacity` - animates opacity
    /// - `RenderAnimatedSize` - animates size changes
    /// - `RenderProgress` - animated progress indicators
    /// - Any render object with `ScheduledTicker` field
    ///
    /// # Default Implementation
    ///
    /// Returns `false`. Override for animated render objects.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn uses_ticker(&self) -> bool {
    ///     true // This render object animates
    /// }
    /// ```
    fn uses_ticker(&self) -> bool {
        false
    }

    /// Called to mute all tickers owned by this render object.
    ///
    /// This is called when the render tree is hidden or backgrounded.
    /// Muted tickers don't fire callbacks but preserve their state.
    ///
    /// # Flutter Protocol
    ///
    /// In Flutter, this is handled via `TickerMode` widget which propagates
    /// mute state down the tree. FLUI provides direct render object support.
    ///
    /// # Default Implementation
    ///
    /// No-op. Override for render objects with tickers.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn mute_tickers(&mut self) {
    ///     if let Some(ticker) = &mut self.ticker {
    ///         ticker.mute();
    ///     }
    /// }
    /// ```
    fn mute_tickers(&mut self) {
        // Default: no-op
    }

    /// Called to unmute all tickers owned by this render object.
    ///
    /// This is called when the render tree becomes visible again.
    /// Unmuted tickers resume firing callbacks.
    ///
    /// # Default Implementation
    ///
    /// No-op. Override for render objects with tickers.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn unmute_tickers(&mut self) {
    ///     if let Some(ticker) = &mut self.ticker {
    ///         ticker.unmute();
    ///     }
    /// }
    /// ```
    fn unmute_tickers(&mut self) {
        // Default: no-op
    }

    /// Returns whether tickers are currently muted.
    ///
    /// # Default Implementation
    ///
    /// Returns `false`. Override for render objects with tickers.
    fn tickers_muted(&self) -> bool {
        false
    }
}

impl_downcast!(sync RenderObject);

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestRenderLeaf {
        size: Size,
    }

    impl RenderObject for TestRenderLeaf {
        fn perform_layout(
            &mut self,
            _element_id: ElementId,
            constraints: BoxConstraints,
            _tree: &mut dyn LayoutTree,
        ) -> RenderResult<Size> {
            Ok(constraints.constrain(self.size))
        }

        fn intrinsic_size(&self) -> Option<Size> {
            Some(self.size)
        }

        #[cfg(debug_assertions)]
        fn debug_fill_properties(&self, properties: &mut Vec<DiagnosticsProperty>) {
            properties.push(DiagnosticsProperty::new("size", format!("{:?}", self.size)));
        }
    }

    #[derive(Debug)]
    struct TestRenderContainer {
        children: Vec<ElementId>,
    }

    impl RenderObject for TestRenderContainer {
        fn visit_children(&self, visitor: &mut dyn FnMut(ElementId)) {
            for &child_id in &self.children {
                visitor(child_id);
            }
        }

        fn child_count(&self) -> usize {
            self.children.len() // O(1) override
        }
    }

    #[test]
    fn test_downcast() {
        let obj = TestRenderLeaf {
            size: Size::new(100.0, 50.0),
        };
        let trait_obj: &dyn RenderObject = &obj;

        // Use downcast-rs for downcasting
        assert!(trait_obj.downcast_ref::<TestRenderLeaf>().is_some());
        assert!(trait_obj.is::<TestRenderLeaf>());
    }

    #[test]
    fn test_visit_children() {
        let obj = TestRenderContainer {
            children: vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)],
        };

        let mut visited = Vec::new();
        obj.visit_children(&mut |id| visited.push(id));

        assert_eq!(visited.len(), 3);
        assert_eq!(obj.child_count(), 3);
    }

    #[test]
    fn test_default_boundaries() {
        let obj = TestRenderLeaf {
            size: Size::new(100.0, 50.0),
        };

        assert!(!obj.is_relayout_boundary());
        assert!(!obj.is_repaint_boundary());
        assert!(!obj.sized_by_parent());
        assert!(!obj.handles_pointer_events());
    }

    #[cfg(debug_assertions)]
    #[test]
    fn test_debug_properties() {
        let obj = TestRenderLeaf {
            size: Size::new(100.0, 50.0),
        };

        let mut props = Vec::new();
        obj.debug_fill_properties(&mut props);

        assert_eq!(props.len(), 1);
        assert_eq!(props[0].name(), "size");
    }
}
