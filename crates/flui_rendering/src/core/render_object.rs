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

use flui_foundation::{DiagnosticsProperty, ElementId};
use flui_interaction::{HitTestEntry, HitTestResult};
use flui_painting::Canvas;
use flui_types::{Offset, Rect, Size};

use crate::core::{BoxConstraints, HitTestTree, LayoutTree, PaintTree};
use crate::RenderResult;

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
/// impl RenderObject for RenderPadding {
///     fn as_any(&self) -> &dyn Any { self }
///     fn as_any_mut(&mut self) -> &mut dyn Any { self }
///
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
pub trait RenderObject: Send + Sync + fmt::Debug + 'static {
    // ============================================================================
    // TYPE ERASURE (Required)
    // ============================================================================

    /// Returns `&dyn Any` for safe downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Returns `&mut dyn Any` for safe downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any;

    // ============================================================================
    // DYN-COMPATIBLE LAYOUT (Required for Box protocol)
    // ============================================================================

    /// Performs layout using box constraints (dyn-compatible).
    ///
    /// This is the type-erased entry point for layout. It receives constraints
    /// as `BoxConstraints` and returns a `Size`.
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
    /// * `tree` - Tree for accessing children
    ///
    /// # Returns
    ///
    /// Computed size that satisfies constraints.
    ///
    /// # Default Implementation
    ///
    /// Default returns constraints.smallest() for leaf nodes.
    /// Override for custom layout logic.
    fn perform_layout(
        &mut self,
        _element_id: ElementId,
        constraints: BoxConstraints,
        _tree: &mut dyn LayoutTree,
    ) -> RenderResult<Size> {
        // Default: leaf node returns minimum size
        Ok(constraints.smallest())
    }

    // ============================================================================
    // DYN-COMPATIBLE PAINT (Required)
    // ============================================================================

    /// Paints this render object to canvas (dyn-compatible).
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
    /// * `tree` - Tree for accessing children
    ///
    /// # Default Implementation
    ///
    /// Default paints all children at their stored offsets.
    fn paint(
        &self,
        _element_id: ElementId,
        _offset: Offset,
        _size: Size,
        _canvas: &mut Canvas,
        _tree: &dyn PaintTree,
    ) {
        // Default: no-op. Override this method to paint content.
        // Child painting is coordinated by the pipeline, not by individual render objects.
    }

    // ============================================================================
    // DYN-COMPATIBLE HIT TEST (Optional)
    // ============================================================================

    /// Hit tests at position (dyn-compatible).
    ///
    /// Default: rectangular bounds check + test children.
    ///
    /// # Returns
    ///
    /// `true` if hit, `false` otherwise.
    fn hit_test(
        &self,
        element_id: ElementId,
        position: Offset,
        result: &mut HitTestResult,
        tree: &dyn HitTestTree,
    ) -> bool {
        // Get geometry
        let size = tree
            .render_object(element_id)
            .and_then(|any| any.downcast_ref::<Size>())
            .copied()
            .unwrap_or(Size::ZERO);

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
            if let Some(child_offset) = tree.get_offset(child_id) {
                let child_position = position - child_offset;
                if tree.hit_test(child_id, child_position, result) {
                    any_hit = true;
                }
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

    // ============================================================================
    // INTERACTION
    // ============================================================================

    /// Whether this render object handles pointer events.
    fn handles_pointer_events(&self) -> bool {
        false
    }
}

// ============================================================================
// EXTENSION TRAIT FOR DOWNCASTING
// ============================================================================

/// Extension trait for safe downcasting.
pub trait RenderObjectExt {
    /// Attempts to downcast to concrete type.
    fn downcast_ref<T: RenderObject>(&self) -> Option<&T>;

    /// Attempts to mutably downcast to concrete type.
    fn downcast_mut<T: RenderObject>(&mut self) -> Option<&mut T>;

    /// Checks if this is type `T`.
    fn is<T: RenderObject>(&self) -> bool;
}

impl RenderObjectExt for dyn RenderObject {
    fn downcast_ref<T: RenderObject>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }

    fn downcast_mut<T: RenderObject>(&mut self) -> Option<&mut T> {
        self.as_any_mut().downcast_mut::<T>()
    }

    fn is<T: RenderObject>(&self) -> bool {
        self.as_any().is::<T>()
    }
}

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
        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }

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
        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }

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
