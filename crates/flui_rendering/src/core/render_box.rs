//! RenderBox - Box Protocol Render Trait with Arity System
//!
//! This is the complete guide to RenderBox in FLUI, including:
//! - Current implementation
//! - Flutter compliance
//! - Examples for each Arity type
//! - Best practices
//! - Common pitfalls
//!
//! # Architecture
//!
//! ```text
//! RenderObject (base)
//!       ↓
//! RenderBox<A: Arity>  ← Protocol-specific + arity validation
//!       ↓
//! Concrete implementations:
//!  ├─ RenderPadding: RenderBox<Single>
//!  ├─ RenderText: RenderBox<Leaf>
//!  ├─ RenderFlex: RenderBox<Variable>
//!  └─ RenderContainer: RenderBox<Optional>
//! ```

use std::fmt;

use flui_interaction::HitTestResult;
use flui_types::{BoxConstraints, Offset, Rect, Size};

use super::arity::Arity;
use super::contexts::{BoxHitTestContext, BoxLayoutContext, BoxPaintContext};
use super::render_object::RenderObject;
use crate::RenderResult;

// ============================================================================
// RENDER BOX TRAIT
// ============================================================================

/// Render trait for box protocol with compile-time arity validation.
///
/// # Flutter RenderBox Protocol
///
/// Flutter's RenderBox follows strict layout protocol:
///
/// 1. **Constraints go down**: Parent passes BoxConstraints to child
/// 2. **Sizes come up**: Child returns Size that satisfies constraints
/// 3. **Parent sets position**: Parent positions child after layout
///
/// ```dart
/// // Flutter RenderBox contract:
/// abstract class RenderBox extends RenderObject {
///   Size get size => _size!;
///
///   @override
///   void performLayout() {
///     // Child MUST return size that satisfies constraints
///     size = computeSize(constraints);
///   }
/// }
/// ```
///
/// # FLUI RenderBox<A>
///
/// FLUI adds compile-time arity validation:
///
/// ```text
/// RenderBox<Leaf>      → 0 children (Text, Image)
/// RenderBox<Optional>  → 0-1 child (Container, SizedBox)
/// RenderBox<Single>    → 1 child (Padding, Transform)
/// RenderBox<Variable>  → 0+ children (Flex, Stack, Column)
/// ```
///
/// # Required Methods
///
/// ## layout() - REQUIRED
///
/// Computes size given constraints. **MUST** satisfy constraints.
///
/// ```rust,ignore
/// fn layout(&mut self, ctx: BoxLayoutContext<'_, A>) -> RenderResult<Size>;
/// ```
///
/// **Contract:**
/// - Input: `BoxConstraints` from parent
/// - Output: `Size` that satisfies constraints
/// - Must be idempotent (same constraints → same size)
///
/// ## paint() - REQUIRED
///
/// Draws to canvas using geometry from layout.
///
/// ```rust,ignore
/// fn paint(&self, ctx: &mut BoxPaintContext<'_, A>);
/// ```
///
/// **Contract:**
/// - Uses `ctx.geometry` (Size) from layout
/// - Never calls layout during paint
/// - Properly manages canvas state (save/restore)
///
/// # Optional Methods (with defaults)
///
/// ## hit_test() - Pointer detection
///
/// ```rust,ignore
/// fn hit_test(&self, ctx: &BoxHitTestContext<'_, A>, result: &mut HitTestResult) -> bool {
///     // Default: rectangular bounds check
///     ctx.local_bounds().contains(ctx.position)
/// }
/// ```
///
/// ## intrinsic_width() / intrinsic_height() - Size queries
///
/// ```rust,ignore
/// fn intrinsic_width(&self, height: f32) -> Option<f32> { None }
/// fn intrinsic_height(&self, width: f32) -> Option<f32> { None }
/// ```
///
/// ## baseline_offset() - Text baseline
///
/// ```rust,ignore
/// fn baseline_offset(&self) -> Option<f32> { None }
/// ```
///
/// ## local_bounds() - Bounding rectangle
///
/// ```rust,ignore
/// fn local_bounds(&self) -> Rect {
///     Rect::from_size(ctx.geometry)  // Default: full size
/// }
/// ```
///
/// # Flutter Compliance Checklist
///
/// ✅ **MUST** satisfy constraints:
/// ```rust,ignore
/// let size = desired_size;
/// // WRONG:
/// return size;  // May violate constraints
///
/// // CORRECT:
/// return ctx.constraints.constrain(size);  // Guaranteed to satisfy constraints
/// ```
///
/// ✅ **MUST** be idempotent:
/// ```rust,ignore
/// // Same constraints → same size every time
/// fn layout(&mut self, ctx) -> Size {
///     // ❌ WRONG: random size
///     Size::new(rand(), rand())
///
///     // ✅ CORRECT: deterministic
///     compute_stable_size(ctx.constraints)
/// }
/// ```
///
/// ✅ **MUST NOT** call layout during paint:
/// ```rust,ignore
/// fn paint(&self, ctx: &mut BoxPaintContext) {
///     // ❌ WRONG: ctx is &mut, layout needs &mut tree
///     // This won't even compile!
///
///     // ✅ CORRECT: use cached geometry
///     let size = ctx.geometry;
/// }
/// ```
///
/// ✅ **MUST** layout children before querying size:
/// ```rust,ignore
/// fn layout(&mut self, mut ctx) -> Size {
///     // ✅ CORRECT order:
///     let child_size = ctx.layout_single_child()?;  // 1. Layout child
///     compute_self_size(child_size)                  // 2. Use child size
///
///     // ❌ WRONG order:
///     let size = compute_size();
///     ctx.layout_single_child()?;  // Too late!
/// }
/// ```
pub trait RenderBox<A: Arity>: RenderObject + fmt::Debug + Send + Sync {
    /// Computes size given constraints.
    ///
    /// # Contract (Flutter RenderBox protocol)
    ///
    /// **MUST satisfy constraints:**
    /// ```ignore
    /// result.width >= constraints.min_width && result.width <= constraints.max_width
    /// result.height >= constraints.min_height && result.height <= constraints.max_height
    /// ```
    ///
    /// Debug builds verify this automatically.
    ///
    /// # Layout Protocol Steps
    ///
    /// 1. Receive constraints from parent
    /// 2. Layout children (if any)
    /// 3. Compute own size
    /// 4. Position children (store offsets)
    /// 5. Return size
    ///
    /// # Examples by Arity
    ///
    /// ## Leaf (0 children)
    ///
    /// ```rust,ignore
    /// impl RenderBox<Leaf> for RenderText {
    ///     fn layout(&mut self, ctx: BoxLayoutContext<'_, Leaf>) -> RenderResult<Size> {
    ///         // Measure intrinsic size
    ///         let intrinsic = self.measure_text();
    ///
    ///         // MUST constrain to satisfy constraints
    ///         Ok(ctx.constraints.constrain(intrinsic))
    ///     }
    /// }
    /// ```
    ///
    /// ## Single (1 child)
    ///
    /// ```rust,ignore
    /// impl RenderBox<Single> for RenderPadding {
    ///     fn layout(&mut self, mut ctx: BoxLayoutContext<'_, Single>) -> RenderResult<Size> {
    ///         // 1. Deflate constraints by padding
    ///         let inner_constraints = ctx.constraints.deflate(&self.padding);
    ///
    ///         // 2. Layout child with deflated constraints
    ///         let child_size = ctx.layout_single_child_with(|_| inner_constraints)?;
    ///
    ///         // 3. Position child (offset by padding)
    ///         let child_offset = Offset::new(self.padding.left, self.padding.top);
    ///         ctx.set_child_offset(ctx.single_child(), child_offset);
    ///
    ///         // 4. Compute own size (child + padding)
    ///         let size = Size::new(
    ///             child_size.width + self.padding.horizontal(),
    ///             child_size.height + self.padding.vertical(),
    ///         );
    ///
    ///         Ok(ctx.constraints.constrain(size))
    ///     }
    /// }
    /// ```
    ///
    /// ## Variable (0+ children)
    ///
    /// ```rust,ignore
    /// impl RenderBox<Variable> for RenderFlex {
    ///     fn layout(&mut self, mut ctx: BoxLayoutContext<'_, Variable>) -> RenderResult<Size> {
    ///         let mut total_main = 0.0;
    ///         let mut max_cross = 0.0;
    ///
    ///         // 1. Layout each child
    ///         for child_id in ctx.children() {
    ///             let child_constraints = self.compute_child_constraints(&ctx.constraints);
    ///             let child_size = ctx.layout_child(child_id, child_constraints)?;
    ///
    ///             // 2. Position child
    ///             let offset = self.compute_child_offset(total_main, child_size);
    ///             ctx.set_child_offset(child_id, offset);
    ///
    ///             // 3. Accumulate sizes
    ///             total_main += child_size.main_axis(self.direction);
    ///             max_cross = max_cross.max(child_size.cross_axis(self.direction));
    ///         }
    ///
    ///         // 4. Compute own size
    ///         let size = match self.direction {
    ///             Axis::Horizontal => Size::new(total_main, max_cross),
    ///             Axis::Vertical => Size::new(max_cross, total_main),
    ///         };
    ///
    ///         Ok(ctx.constraints.constrain(size))
    ///     }
    /// }
    /// ```
    fn layout(&mut self, ctx: BoxLayoutContext<'_, A>) -> RenderResult<Size>;

    /// Paints to canvas using geometry from layout.
    ///
    /// # Contract
    ///
    /// - **Never** call layout during paint
    /// - Use `ctx.geometry` (Size) from layout phase
    /// - Properly save/restore canvas state
    /// - Paint children in correct order
    ///
    /// # Canvas State Management
    ///
    /// ```rust,ignore
    /// fn paint(&self, ctx: &mut BoxPaintContext) {
    ///     ctx.canvas_mut().save();           // Save state
    ///     ctx.canvas_mut().set_opacity(0.5); // Modify
    ///     ctx.paint_single_child(offset);    // Paint child
    ///     ctx.canvas_mut().restore();        // Restore state
    /// }
    /// ```
    ///
    /// # Examples by Arity
    ///
    /// ## Leaf (draw content)
    ///
    /// ```rust,ignore
    /// impl RenderBox<Leaf> for RenderText {
    ///     fn paint(&self, ctx: &mut BoxPaintContext<'_, Leaf>) {
    ///         let rect = Rect::from_min_size(ctx.offset, ctx.geometry);
    ///         ctx.canvas_mut().draw_text(&self.text, rect, &self.style);
    ///     }
    /// }
    /// ```
    ///
    /// ## Single (paint child)
    ///
    /// ```rust,ignore
    /// impl RenderBox<Single> for RenderOpacity {
    ///     fn paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
    ///         if self.opacity < 0.01 {
    ///             return;  // Don't paint if invisible
    ///         }
    ///
    ///         if self.opacity < 1.0 {
    ///             ctx.canvas_mut().save();
    ///             ctx.canvas_mut().set_opacity(self.opacity);
    ///         }
    ///
    ///         // Child offset set during layout
    ///         ctx.paint_single_child(Offset::ZERO);
    ///
    ///         if self.opacity < 1.0 {
    ///             ctx.canvas_mut().restore();
    ///         }
    ///     }
    /// }
    /// ```
    ///
    /// ## Variable (paint all children)
    ///
    /// ```rust,ignore
    /// impl RenderBox<Variable> for RenderStack {
    ///     fn paint(&self, ctx: &mut BoxPaintContext<'_, Variable>) {
    ///         // Paint children in order (back to front)
    ///         for child_id in ctx.children() {
    ///             // Offset stored during layout
    ///             ctx.paint_child(child_id);
    ///         }
    ///     }
    /// }
    /// ```
    fn paint(&self, ctx: &mut BoxPaintContext<'_, A>);

    /// Hit tests for pointer events.
    ///
    /// Returns `true` if this element handled the hit test.
    ///
    /// # Default Implementation
    ///
    /// Default checks if position is within rectangular bounds:
    ///
    /// ```rust,ignore
    /// fn hit_test(&self, ctx: &BoxHitTestContext<A>, result: &mut HitTestResult) -> bool {
    ///     let local_bounds = self.local_bounds();
    ///     if !local_bounds.contains(ctx.position) {
    ///         return false;
    ///     }
    ///
    ///     result.add(HitTestEntry::new(ctx.element_id));
    ///     true
    /// }
    /// ```
    ///
    /// # Override for Custom Shapes
    ///
    /// ```rust,ignore
    /// impl RenderBox<Leaf> for RenderCircle {
    ///     fn hit_test(&self, ctx: &BoxHitTestContext<Leaf>, result: &mut HitTestResult) -> bool {
    ///         let center = ctx.geometry.center();
    ///         let radius = ctx.geometry.width.min(ctx.geometry.height) / 2.0;
    ///         let distance = (ctx.position - center).distance();
    ///
    ///         if distance <= radius {
    ///             result.add(HitTestEntry::new(ctx.element_id));
    ///             return true;
    ///         }
    ///
    ///         false
    ///     }
    /// }
    /// ```
    fn hit_test(&self, ctx: &BoxHitTestContext<'_, A>, result: &mut HitTestResult) -> bool {
        // Default: rectangular bounds check
        let local_bounds = self.local_bounds();
        if !local_bounds.contains(ctx.position) {
            return false;
        }

        // Add self to hit test result
        let bounds = Rect::from_min_size(Offset::ZERO, ctx.geometry);
        result.add(flui_interaction::HitTestEntry::new(
            ctx.element_id(),
            ctx.position,
            bounds,
        ));
        true
    }

    /// Returns intrinsic width for given height.
    ///
    /// This is the minimum width this render object would need if constrained
    /// to the given height.
    ///
    /// # Flutter Semantics
    ///
    /// - Should not trigger layout
    /// - Used for intrinsic size calculations
    /// - Returns `None` if no meaningful intrinsic width
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl RenderBox<Leaf> for RenderText {
    ///     fn intrinsic_width(&self, height: f32) -> Option<f32> {
    ///         Some(self.measure_text_width(height))
    ///     }
    /// }
    /// ```
    fn intrinsic_width(&self, _height: f32) -> Option<f32> {
        None
    }

    /// Returns intrinsic height for given width.
    ///
    /// This is the minimum height this render object would need if constrained
    /// to the given width.
    fn intrinsic_height(&self, _width: f32) -> Option<f32> {
        None
    }

    /// Returns baseline offset for text alignment.
    ///
    /// Used by baseline alignment in Flex layouts.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl RenderBox<Leaf> for RenderText {
    ///     fn baseline_offset(&self) -> Option<f32> {
    ///         Some(self.font_metrics.ascent)
    ///     }
    /// }
    /// ```
    fn baseline_offset(&self) -> Option<f32> {
        None
    }

    /// Computes size without side effects (dry layout).
    ///
    /// Used for intrinsic size calculations where full layout is too expensive.
    ///
    /// # Default
    ///
    /// Returns smallest size that satisfies constraints.
    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        constraints.smallest()
    }

    /// Returns local bounding rectangle.
    ///
    /// # Default
    ///
    /// Returns rectangle from (0, 0) to (width, height).
    ///
    /// Override for custom shapes:
    ///
    /// ```rust,ignore
    /// impl RenderBox<Leaf> for RenderCircle {
    ///     fn local_bounds(&self) -> Rect {
    ///         // Circle inscribed in box
    ///         let size = self.size();
    ///         let radius = size.width.min(size.height) / 2.0;
    ///         let center = size.center();
    ///         Rect::from_center_and_radius(center, radius)
    ///     }
    /// }
    /// ```
    fn local_bounds(&self) -> Rect {
        Rect::ZERO // Default: will be set from geometry
    }
}

// Note: BoxConstraints methods (constrain, smallest, biggest, deflate, loosen, tight, loose, is_satisfied_by)
// are defined in flui_types::BoxConstraints. See examples in the trait documentation above.

// ============================================================================
// COMMON PITFALLS AND SOLUTIONS
// ============================================================================

/// Common mistakes when implementing RenderBox
///
/// # ❌ Pitfall 1: Not constraining result
///
/// ```rust,ignore
/// // WRONG:
/// fn layout(&mut self, ctx) -> Size {
///     Size::new(100.0, 50.0)  // Ignores constraints!
/// }
///
/// // CORRECT:
/// fn layout(&mut self, ctx) -> Size {
///     let desired = Size::new(100.0, 50.0);
///     ctx.constraints.constrain(desired)
/// }
/// ```
///
/// # ❌ Pitfall 2: Calling layout during paint
///
/// ```rust,ignore
/// // WRONG:
/// fn paint(&self, ctx: &mut BoxPaintContext) {
///     // Won't compile - ctx is &mut, can't get &mut tree
///     let size = ctx.layout_child(...)?;  // ERROR!
/// }
///
/// // CORRECT:
/// fn paint(&self, ctx: &mut BoxPaintContext) {
///     // Use cached geometry from layout
///     let size = ctx.geometry;
/// }
/// ```
///
/// # ❌ Pitfall 3: Not positioning children
///
/// ```rust,ignore
/// // WRONG:
/// fn layout(&mut self, ctx) -> Size {
///     let child_size = ctx.layout_single_child()?;
///     // Forgot to set child offset!
///     Size::new(100.0, child_size.height)
/// }
///
/// // CORRECT:
/// fn layout(&mut self, ctx) -> Size {
///     let child_size = ctx.layout_single_child()?;
///     ctx.set_child_offset(
///         ctx.single_child(),
///         Offset::new(10.0, 10.0)  // Position child
///     );
///     Size::new(100.0, child_size.height)
/// }
/// ```
///
/// # ❌ Pitfall 4: Non-idempotent layout
///
/// ```rust,ignore
/// // WRONG:
/// fn layout(&mut self, ctx) -> Size {
///     let random = rand::random::<f32>();  // Different every time!
///     Size::new(100.0 * random, 50.0)
/// }
///
/// // CORRECT:
/// fn layout(&mut self, ctx) -> Size {
///     // Always same size for same constraints
///     ctx.constraints.constrain(self.intrinsic_size)
/// }
/// ```
///
/// # ❌ Pitfall 5: Forgetting canvas save/restore
///
/// ```rust,ignore
/// // WRONG:
/// fn paint(&self, ctx: &mut BoxPaintContext) {
///     ctx.canvas_mut().set_opacity(0.5);  // Modifies state
///     ctx.paint_single_child(offset);
///     // State leak! Next sibling will be semi-transparent
/// }
///
/// // CORRECT:
/// fn paint(&self, ctx: &mut BoxPaintContext) {
///     ctx.canvas_mut().save();
///     ctx.canvas_mut().set_opacity(0.5);
///     ctx.paint_single_child(offset);
///     ctx.canvas_mut().restore();  // Clean state
/// }
/// ```
mod _docs {}

// ============================================================================
// IMPLEMENTATION EXAMPLES
// ============================================================================

#[cfg(doc)]
mod examples {
    use super::super::arity::{Leaf, Single, Variable};
    use super::*;

    /// Example: Leaf render object (no children)
    #[derive(Debug)]
    struct RenderColoredBox {
        color: u32,
        width: f32,
        height: f32,
    }

    impl RenderBox<Leaf> for RenderColoredBox {
        fn layout(&mut self, ctx: BoxLayoutContext<'_, Leaf>) -> RenderResult<Size> {
            let desired = Size::new(self.width, self.height);
            Ok(ctx.constraints.constrain(desired))
        }

        fn paint(&self, ctx: &mut BoxPaintContext<'_, Leaf>) {
            let rect = Rect::from_min_size(ctx.offset, ctx.geometry);
            // ctx.canvas_mut().draw_rect(rect, self.color);
        }
    }

    impl RenderObject for RenderColoredBox {
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    /// Example: Single-child container
    #[derive(Debug)]
    struct RenderPadding {
        padding: flui_types::EdgeInsets,
    }

    impl RenderBox<Single> for RenderPadding {
        fn layout(&mut self, mut ctx: BoxLayoutContext<'_, Single>) -> RenderResult<Size> {
            // Deflate constraints
            let inner = ctx.constraints.deflate(&self.padding);

            // Layout child
            let child_size = ctx.layout_single_child_with(|_| inner)?;

            // Position child
            let offset = Offset::new(self.padding.left, self.padding.top);
            ctx.set_child_offset(ctx.single_child(), offset);

            // Compute size
            let size = Size::new(
                child_size.width + self.padding.horizontal(),
                child_size.height + self.padding.vertical(),
            );

            Ok(ctx.constraints.constrain(size))
        }

        fn paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
            ctx.paint_single_child(Offset::new(self.padding.left, self.padding.top));
        }
    }

    impl RenderObject for RenderPadding {
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    /// Example: Multi-child container
    #[derive(Debug)]
    struct RenderColumn {
        spacing: f32,
    }

    impl RenderBox<Variable> for RenderColumn {
        fn layout(&mut self, mut ctx: BoxLayoutContext<'_, Variable>) -> RenderResult<Size> {
            let mut y_offset = 0.0;
            let mut max_width = 0.0;

            // Layout children vertically
            for child_id in ctx.children() {
                // Layout child
                let child_size = ctx.layout_child(child_id, ctx.constraints)?;

                // Position child
                ctx.set_child_offset(child_id, Offset::new(0.0, y_offset));

                // Accumulate
                y_offset += child_size.height + self.spacing;
                max_width = max_width.max(child_size.width);
            }

            let size = Size::new(max_width, y_offset - self.spacing);
            Ok(ctx.constraints.constrain(size))
        }

        fn paint(&self, ctx: &mut BoxPaintContext<'_, Variable>) {
            for child_id in ctx.children() {
                ctx.paint_child(child_id);
            }
        }
    }

    impl RenderObject for RenderColumn {
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }
}
