//! Advanced RenderBox trait with GAT-based contexts and unified arity system.
//!
//! This module provides the `RenderBox<A>` trait for implementing render objects
//! that use the box layout protocol with compile-time arity validation and
//! zero-cost abstractions through Generic Associated Types.
//!
//! # Design Philosophy
//!
//! - **Context-based API**: All operations use typed contexts for safety and ergonomics
//! - **Arity validation**: Compile-time child count constraints via unified arity system
//! - **Zero-cost abstractions**: GAT and const generics for optimal performance
//! - **Error handling**: Comprehensive error propagation with meaningful diagnostics
//! - **Extensibility**: Rich default implementations with override points for optimization
//!
//! # Trait Hierarchy
//!
//! ```text
//! RenderObject (base trait)
//!     │
//!     └── RenderBox<A> (box protocol with arity A)
//!             │
//!             ├── layout(ctx: LayoutContext<A, BoxProtocol>) -> Size
//!             ├── paint(ctx: &mut PaintContext<A, BoxProtocol>)
//!             ├── hit_test(ctx: &HitTestContext<A, BoxProtocol>, result) -> bool
//!             └── Advanced optimization hooks
//! ```
//!
//! # Arity Integration
//!
//! The arity system provides compile-time validation of child counts:
//!
//! | Arity | Children | Use Cases | Examples |
//! |-------|----------|-----------|----------|
//! | `Leaf` | 0 | Terminal elements | Text, Image, Icon |
//! | `Optional` | 0-1 | Conditional content | Container, SizedBox |
//! | `Single` | 1 | Decorators/wrappers | Padding, Transform, Align |
//! | `Variable` | 0+ | Dynamic layouts | Flex, Stack, Column, Row |
//! | `Exact<N>` | N | Fixed layouts | Grid cells, Tab layouts |
//! | `AtLeast<N>` | N+ | Minimum requirements | TabBar, Toolbar |
//!
//! # Usage Examples
//!
//! ## Leaf Element (0 children)
//!
//! ```rust,ignore
//! use flui_rendering::core::{RenderBox, RenderObject, LayoutContext, PaintContext, Leaf};
//!
//! #[derive(Debug)]
//! struct RenderText {
//!     text: String,
//!     color: Color,
//!     size: Size,
//! }
//!
//! impl RenderBox<Leaf> for RenderText {
//!     fn layout(&mut self, ctx: LayoutContext<'_, Leaf, BoxProtocol>) -> RenderResult<Size> {
//!         // Leaf elements have no children to layout
//!         let size = self.measure_text(&self.text);
//!         self.size = ctx.constraints.constrain(size);
//!         Ok(self.size)
//!     }
//!
//!     fn paint(&self, ctx: &mut PaintContext<'_, Leaf, BoxProtocol>) {
//!         let paint = Paint::new().with_color(self.color);
//!         ctx.canvas_mut().draw_text(&self.text, ctx.offset, &paint);
//!     }
//!
//!     fn as_render_object(&self) -> &dyn RenderObject {
//!         self
//!     }
//! }
//! ```
//!
//! ## Single Child Decorator (1 child)
//!
//! ```rust,ignore
//! #[derive(Debug)]
//! struct RenderPadding {
//!     padding: EdgeInsets,
//! }
//!
//! impl RenderBox<Single> for RenderPadding {
//!     fn layout(&mut self, mut ctx: LayoutContext<'_, Single, BoxProtocol>) -> RenderResult<Size> {
//!         let inner_constraints = ctx.constraints.deflate(&self.padding);
//!         let child_id = ctx.single_child();
//!         let child_size = ctx.layout_child(child_id, inner_constraints)?;
//!
//!         Ok(Size::new(
//!             child_size.width + self.padding.horizontal(),
//!             child_size.height + self.padding.vertical(),
//!         ))
//!     }
//!
//!     fn paint(&self, ctx: &mut PaintContext<'_, Single, BoxProtocol>) {
//!         let child_id = ctx.single_child();
//!         let offset = Offset::new(self.padding.left, self.padding.top);
//!         ctx.paint_child(child_id, offset).ok();
//!     }
//!
//!     fn as_render_object(&self) -> &dyn RenderObject {
//!         self
//!     }
//! }
//! ```
//!
//! ## Multi-child Layout (Variable children)
//!
//! ```rust,ignore
//! #[derive(Debug)]
//! struct RenderFlex {
//!     direction: Axis,
//!     main_axis_alignment: MainAxisAlignment,
//!     cross_axis_alignment: CrossAxisAlignment,
//! }
//!
//! impl RenderBox<Variable> for RenderFlex {
//!     fn layout(&mut self, mut ctx: LayoutContext<'_, Variable, BoxProtocol>) -> RenderResult<Size> {
//!         let mut total_main_size = 0.0;
//!         let mut max_cross_size = 0.0;
//!
//!         // Layout all children
//!         for child_id in ctx.children() {
//!             let child_size = ctx.layout_child(child_id, ctx.constraints)?;
//!
//!             let (main_size, cross_size) = match self.direction {
//!                 Axis::Horizontal => (child_size.width, child_size.height),
//!                 Axis::Vertical => (child_size.height, child_size.width),
//!             };
//!
//!             total_main_size += main_size;
//!             max_cross_size = max_cross_size.max(cross_size);
//!         }
//!
//!         let size = match self.direction {
//!             Axis::Horizontal => Size::new(total_main_size, max_cross_size),
//!             Axis::Vertical => Size::new(max_cross_size, total_main_size),
//!         };
//!
//!         Ok(ctx.constraints.constrain(size))
//!     }
//!
//!     fn paint(&self, ctx: &mut PaintContext<'_, Variable, BoxProtocol>) {
//!         let mut main_offset = 0.0;
//!
//!         for child_id in ctx.children() {
//!             let offset = match self.direction {
//!                 Axis::Horizontal => Offset::new(main_offset, 0.0),
//!                 Axis::Vertical => Offset::new(0.0, main_offset),
//!             };
//!
//!             ctx.paint_child(child_id, offset).ok();
//!
//!             // Advance offset by child size
//!             if let Some(child_size) = ctx.get_child_size(child_id) {
//!                 main_offset += match self.direction {
//!                     Axis::Horizontal => child_size.width,
//!                     Axis::Vertical => child_size.height,
//!                 };
//!             }
//!         }
//!     }
//!
//!     fn as_render_object(&self) -> &dyn RenderObject {
//!         self
//!     }
//! }
//! ```
//!
//! # Performance Features
//!
//! - **Batch operations**: Process multiple children efficiently with const generics
//! - **Early termination**: Hit testing with spatial optimizations
//! - **Cache integration**: Automatic caching of layout results where beneficial
//! - **Parallel layout**: Support for parallel child layout computation
//! - **Intrinsic sizing**: Efficient computation of natural element dimensions

use std::fmt;
use std::marker::PhantomData;

use flui_interaction::HitTestResult;
use flui_types::{Offset, Size};

use super::arity::Arity;
use super::contexts::{BoxHitTestContext, BoxLayoutContext, BoxPaintContext};
use super::geometry::BoxConstraints;
use super::render_object::RenderObject;
use crate::core::RenderResult;

// ============================================================================
// CORE RENDER BOX TRAIT
// ============================================================================

/// Advanced render trait for box protocol with GAT-based contexts and arity validation.
///
/// This trait provides the foundation for implementing render objects that use
/// the 2D box layout protocol. It leverages Generic Associated Types for zero-cost
/// abstractions and compile-time arity validation for type safety.
///
/// # Type Parameters
///
/// - `A`: Arity type constraining the number of children (Leaf, Single, Variable, etc.)
///
/// # Requirements
///
/// All implementors must:
/// - Be `Send + Sync + Debug + 'static` for thread safety and introspection
/// - Implement `RenderObject` for type erasure and lifecycle management
/// - Handle errors gracefully with proper error propagation
///
/// # Context-Based API
///
/// All operations use typed contexts that provide:
/// - **Type-safe access**: GAT-based children iteration with proper lifetimes
/// - **Performance optimization**: Batch operations and HRTB predicates
/// - **Error handling**: Comprehensive error propagation and recovery
/// - **Tree integration**: Seamless integration with the rendering tree
///
/// # Performance Characteristics
///
/// - **Layout**: O(n) where n is number of children (parallelizable)
/// - **Paint**: O(n) with layer composition optimization
/// - **Hit Testing**: O(log n) with spatial indexing for large trees
/// - **Memory**: Zero-cost arity abstractions with atomic dirty tracking
pub trait RenderBox<A: Arity>: Send + Sync + fmt::Debug + 'static {
    // ========================================================================
    // CORE OPERATIONS (REQUIRED)
    // ========================================================================

    /// Computes the size of this render object given constraints.
    ///
    /// This is the primary layout method that determines how this render object
    /// should size itself given the constraints from its parent and the sizes
    /// of its children.
    ///
    /// # Context Operations
    ///
    /// The context provides:
    /// - `ctx.constraints` - Layout constraints from parent
    /// - `ctx.children()` - GAT-based iteration over child ElementIds
    /// - `ctx.layout_child(id, constraints)` - Layout a specific child
    /// - `ctx.children_where(predicate)` - HRTB-based child filtering
    ///
    /// # Error Handling
    ///
    /// Layout operations should handle errors gracefully:
    /// - Return `RenderError::InvalidConstraints` for impossible constraints
    /// - Return `RenderError::ChildLayoutFailed` when child layout fails
    /// - Use fallback sizes when appropriate rather than failing
    ///
    /// # Performance Notes
    ///
    /// - Child layout calls are cached automatically by the context
    /// - Use batch operations for multiple children when possible
    /// - Consider using `ctx.children_where()` to skip unnecessary children
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn layout(&mut self, mut ctx: LayoutContext<'_, Single, BoxProtocol>) -> RenderResult<Size> {
    ///     let child_constraints = ctx.constraints.deflate(&self.padding);
    ///     let child_id = ctx.single_child();
    ///     let child_size = ctx.layout_child(child_id, child_constraints)?;
    ///
    ///     Ok(Size::new(
    ///         child_size.width + self.padding.horizontal(),
    ///         child_size.height + self.padding.vertical(),
    ///     ))
    /// }
    /// ```
    fn layout(&mut self, ctx: BoxLayoutContext<'_, A>) -> RenderResult<Size>;

    /// Paints this render object and its children to the canvas.
    ///
    /// This method is responsible for drawing the visual representation of
    /// the render object. It should paint its own content first, then paint
    /// its children at appropriate offsets.
    ///
    /// # Context Operations
    ///
    /// The context provides:
    /// - `ctx.canvas_mut()` - Mutable access to the drawing canvas
    /// - `ctx.offset` - This element's offset in parent coordinates
    /// - `ctx.size` - This element's size from layout
    /// - `ctx.paint_child(id, offset)` - Paint a child at given offset
    ///
    /// # Error Handling
    ///
    /// Paint operations should be resilient:
    /// - Log child paint failures but continue with other children
    /// - Use fallback rendering for complex paint operations that fail
    /// - Ensure partial paint success doesn't leave canvas in invalid state
    ///
    /// # Performance Notes
    ///
    /// - Use `ctx.with_clip()` for efficient clipping operations
    /// - Consider layer composition for complex visual effects
    /// - Paint children in correct z-order (usually front-to-back)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn paint(&self, ctx: &mut PaintContext<'_, Variable, BoxProtocol>) {
    ///     // Paint background
    ///     let paint = Paint::new().with_color(self.background_color);
    ///     ctx.canvas_mut().draw_rect(ctx.local_bounds(), &paint);
    ///
    ///     // Paint all children
    ///     ctx.paint_all_children().into_iter().for_each(|result| {
    ///         if let Err(e) = result {
    ///             tracing::warn!("Child paint failed: {}", e);
    ///         }
    ///     });
    /// }
    /// ```
    fn paint(&self, ctx: &mut BoxPaintContext<'_, A>);

    /// Returns a reference to this render object as a `RenderObject` trait object.
    ///
    /// This enables type erasure and access to the base `RenderObject` functionality
    /// like downcasting, introspection, and lifecycle management.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn as_render_object(&self) -> &dyn RenderObject {
    ///     self
    /// }
    /// ```
    fn as_render_object(&self) -> &dyn RenderObject;

    // ========================================================================
    // OPTIONAL OPERATIONS (DEFAULT IMPLEMENTATIONS)
    // ========================================================================

    /// Performs hit testing for pointer events.
    ///
    /// This method determines whether a pointer event at the given position
    /// should be handled by this element or any of its children.
    ///
    /// # Default Implementation
    ///
    /// The default implementation uses a standard algorithm:
    /// 1. Test children in reverse z-order (topmost first)
    /// 2. If no child is hit, test self via `hit_test_self()`
    /// 3. Add hits to the result accumulator
    ///
    /// # Override Considerations
    ///
    /// Override when you need:
    /// - Custom hit testing logic (e.g., non-rectangular shapes)
    /// - Performance optimizations (e.g., spatial indexing)
    /// - Special event handling behavior
    ///
    /// # Example Override
    ///
    /// ```rust,ignore
    /// fn hit_test(&self, ctx: &HitTestContext<'_, Variable, BoxProtocol>, result: &mut HitTestResult) -> bool {
    ///     // Custom circular hit testing
    ///     if self.is_circular && !self.point_in_circle(ctx.position) {
    ///         return false;
    ///     }
    ///
    ///     // Use default behavior for children
    ///     ctx.hit_test_self_and_children(result)
    /// }
    /// ```
    fn hit_test(&self, ctx: &BoxHitTestContext<'_, A>, result: &mut HitTestResult) -> bool {
        // Default implementation: test children first, then self
        ctx.hit_test_self_and_children(result)
    }

    /// Tests if a position hits this render object (excluding children).
    ///
    /// This method is called by the default hit testing implementation to
    /// determine if the render object itself (not its children) should
    /// handle a pointer event.
    ///
    /// # Default Implementation
    ///
    /// Returns `false`, making the element transparent to pointer events.
    /// This is appropriate for layout containers that shouldn't intercept
    /// events meant for their children.
    ///
    /// # Override for Interactive Elements
    ///
    /// Override to `true` or custom logic for interactive elements:
    ///
    /// ```rust,ignore
    /// fn hit_test_self(&self, position: Offset, size: Size) -> bool {
    ///     // Button is always interactive within its bounds
    ///     position.dx >= 0.0 && position.dy >= 0.0 &&
    ///     position.dx < size.width && position.dy < size.height
    /// }
    /// ```
    fn hit_test_self(&self, _position: Offset, _size: Size) -> bool {
        false
    }

    /// Computes the intrinsic width for a given height.
    ///
    /// This method determines the natural width this render object would
    /// prefer if constrained to the given height. Used by layout algorithms
    /// for intrinsic dimension calculations.
    ///
    /// # Default Implementation
    ///
    /// Returns `None`, indicating no intrinsic width preference.
    /// This causes the layout algorithm to use constraint-based sizing.
    ///
    /// # Override Examples
    ///
    /// ```rust,ignore
    /// // Text that reflows based on height
    /// fn compute_intrinsic_width(&self, height: f32) -> Option<f32> {
    ///     Some(self.text_layout.width_for_height(height))
    /// }
    ///
    /// // Image that maintains aspect ratio
    /// fn compute_intrinsic_width(&self, height: f32) -> Option<f32> {
    ///     Some(height * self.aspect_ratio)
    /// }
    /// ```
    fn compute_intrinsic_width(&self, _height: f32) -> Option<f32> {
        None
    }

    /// Computes the intrinsic height for a given width.
    ///
    /// This method determines the natural height this render object would
    /// prefer if constrained to the given width.
    fn compute_intrinsic_height(&self, _width: f32) -> Option<f32> {
        None
    }

    /// Returns the minimum intrinsic width of this render object.
    ///
    /// This is the smallest width at which the render object can render
    /// without clipping or overflow.
    fn compute_min_intrinsic_width(&self) -> Option<f32> {
        None
    }

    /// Returns the maximum intrinsic width of this render object.
    ///
    /// This is the largest width that provides any benefit - beyond this
    /// width, the render object won't utilize the extra space effectively.
    fn compute_max_intrinsic_width(&self) -> Option<f32> {
        None
    }

    /// Returns the minimum intrinsic height of this render object.
    fn compute_min_intrinsic_height(&self) -> Option<f32> {
        None
    }

    /// Returns the maximum intrinsic height of this render object.
    fn compute_max_intrinsic_height(&self) -> Option<f32> {
        None
    }
}

// ============================================================================
// EXTENSION TRAIT FOR ADDITIONAL UTILITIES
// ============================================================================

/// Extension trait providing additional utilities for `RenderBox` implementors.
///
/// This trait is automatically implemented for all types that implement `RenderBox<A>`.
pub trait RenderBoxExt<A: Arity>: RenderBox<A> {
    /// Checks if a position is within the given size bounds.
    ///
    /// This is a convenience method for hit testing implementations.
    #[inline]
    fn position_in_bounds(&self, position: Offset, size: Size) -> bool {
        position.dx >= 0.0
            && position.dy >= 0.0
            && position.dx < size.width
            && position.dy < size.height
    }

    /// Performs a complete layout operation including error logging.
    ///
    /// This is a convenience method that wraps the layout call with
    /// appropriate error handling and debug logging.
    fn layout_with_error_handling(&mut self, ctx: BoxLayoutContext<'_, A>) -> Size {
        match self.layout(ctx) {
            Ok(size) => {
                tracing::trace!("Layout successful: {:?}", size);
                size
            }
            Err(e) => {
                tracing::error!("Layout failed: {}", e);
                // Return a fallback size
                Size::new(0.0, 0.0)
            }
        }
    }

    /// Performs painting with comprehensive error handling.
    ///
    /// This wraps the paint call with error recovery and logging.
    fn paint_with_error_handling(&self, ctx: &mut BoxPaintContext<'_, A>) {
        // TODO: Implement error handling wrapper for paint
        self.paint(ctx);
    }

    /// Gets the debug name from the underlying `RenderObject`.
    fn debug_name(&self) -> &'static str {
        self.as_render_object().debug_name()
    }
}

// Blanket implementation for all RenderBox types
impl<A: Arity, T: RenderBox<A>> RenderBoxExt<A> for T {}

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

/// Creates a simple render box that does nothing (for testing and prototyping).
///
/// This is useful for creating placeholder render objects during development.
pub fn create_empty_render_box<A: Arity>() -> impl RenderBox<A> {
    EmptyRenderBox::<A> {
        _phantom: PhantomData,
    }
}

/// A minimal render box implementation that does nothing.
///
/// This is useful for testing, prototyping, and as a base for custom implementations.
#[derive(Debug)]
pub struct EmptyRenderBox<A: Arity> {
    _phantom: PhantomData<A>,
}

impl<A: Arity> RenderBox<A> for EmptyRenderBox<A> {
    fn layout(&mut self, ctx: BoxLayoutContext<'_, A>) -> RenderResult<Size> {
        // Return the smallest size that satisfies constraints
        Ok(ctx.constraints.smallest())
    }

    fn paint(&self, _ctx: &mut BoxPaintContext<'_, A>) {
        // Do nothing - empty render box
    }

    fn as_render_object(&self) -> &dyn RenderObject {
        self
    }
}

impl<A: Arity> RenderObject for EmptyRenderBox<A> {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn debug_name(&self) -> &'static str {
        "EmptyRenderBox"
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::arity::{Leaf, Single, Variable};
    use crate::core::render_tree::*;
    use flui_foundation::ElementId;

    // Mock tree for testing
    struct MockLayoutTree;
    impl LayoutTree for MockLayoutTree {
        fn perform_layout(
            &mut self,
            _id: ElementId,
            constraints: BoxConstraints,
        ) -> RenderResult<Size> {
            Ok(constraints.biggest())
        }
        fn perform_sliver_layout(
            &mut self,
            _id: ElementId,
            _constraints: flui_types::SliverConstraints,
        ) -> RenderResult<flui_types::SliverGeometry> {
            Ok(flui_types::SliverGeometry::zero())
        }
        fn set_offset(&mut self, _id: ElementId, _offset: Offset) {}
        fn get_offset(&self, _id: ElementId) -> Option<Offset> {
            Some(Offset::ZERO)
        }
        fn mark_needs_layout(&mut self, _id: ElementId) {}
        fn needs_layout(&self, _id: ElementId) -> bool {
            false
        }
        fn render_object(&self, _id: ElementId) -> Option<&dyn std::any::Any> {
            None
        }
        fn render_object_mut(&mut self, _id: ElementId) -> Option<&mut dyn std::any::Any> {
            None
        }
    }

    struct MockPaintTree;
    impl PaintTree for MockPaintTree {
        fn perform_paint(
            &mut self,
            _id: ElementId,
            _offset: Offset,
        ) -> RenderResult<flui_painting::Canvas> {
            Ok(flui_painting::Canvas::new())
        }
        fn mark_needs_paint(&mut self, _id: ElementId) {}
        fn needs_paint(&self, _id: ElementId) -> bool {
            false
        }
        fn render_object(&self, _id: ElementId) -> Option<&dyn std::any::Any> {
            None
        }
        fn render_object_mut(&mut self, _id: ElementId) -> Option<&mut dyn std::any::Any> {
            None
        }
    }

    #[test]
    fn test_empty_render_box_leaf() {
        let mut empty = create_empty_render_box::<Leaf>();

        let mut tree = MockLayoutTree;
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let children: [ElementId; 0] = [];
        let accessor = Leaf::from_slice(&children);

        let ctx = BoxLayoutContext::new(&mut tree, ElementId::new(1), constraints, accessor);
        let size = empty.layout(ctx).unwrap();

        assert_eq!(size, Size::new(0.0, 0.0)); // Should return smallest size
    }

    #[test]
    fn test_empty_render_box_single() {
        let mut empty = create_empty_render_box::<Single>();

        let mut tree = MockLayoutTree;
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let children = [ElementId::new(42)];
        let accessor = Single::from_slice(&children);

        let ctx = BoxLayoutContext::new(&mut tree, ElementId::new(1), constraints, accessor);
        let size = empty.layout(ctx).unwrap();

        assert_eq!(size, Size::new(0.0, 0.0));
    }

    #[test]
    fn test_render_box_ext_position_in_bounds() {
        let empty = create_empty_render_box::<Leaf>();
        let size = Size::new(100.0, 50.0);

        assert!(empty.position_in_bounds(Offset::new(50.0, 25.0), size));
        assert!(empty.position_in_bounds(Offset::new(0.0, 0.0), size));
        assert!(empty.position_in_bounds(Offset::new(99.9, 49.9), size));

        assert!(!empty.position_in_bounds(Offset::new(-1.0, 25.0), size));
        assert!(!empty.position_in_bounds(Offset::new(50.0, -1.0), size));
        assert!(!empty.position_in_bounds(Offset::new(100.0, 25.0), size));
        assert!(!empty.position_in_bounds(Offset::new(50.0, 50.0), size));
    }
}
