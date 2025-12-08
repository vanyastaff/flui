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
use flui_types::prelude::TextBaseline;
use flui_types::{BoxConstraints, Offset, Rect, Size};

use crate::arity::Arity;
use crate::hit_test_context::BoxHitTestContext;
use crate::layout_context::BoxLayoutContext;
use crate::object::RenderObject;
use crate::paint_context::BoxPaintContext;
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

    // ========================================================================
    // FLUTTER INTRINSIC DIMENSIONS API
    // ========================================================================
    //
    // Flutter provides four intrinsic dimension methods:
    // - getMinIntrinsicWidth(height) / getMaxIntrinsicWidth(height)
    // - getMinIntrinsicHeight(width) / getMaxIntrinsicHeight(width)
    //
    // These are used for:
    // - IntrinsicWidth/IntrinsicHeight widgets
    // - Table cell sizing
    // - Wrap widget measurement
    // - Text paragraph layout
    //
    // IMPORTANT: These methods should NOT trigger actual layout. They compute
    // sizes based on intrinsic content properties only.
    // ========================================================================

    /// Returns the minimum width that this box could be without failing to
    /// correctly paint its contents within itself, without clipping.
    ///
    /// The height argument may give a specific height to assume. The given
    /// height can be infinite, meaning that the intrinsic width should be
    /// determined assuming unbounded height constraints.
    ///
    /// # Flutter Semantics
    ///
    /// This is the narrowest width this render object can have while still
    /// being able to paint correctly. For example:
    /// - Text: Width of the widest word (for wrapping text)
    /// - Image: 0 (can scale down to nothing)
    /// - Row: Sum of children's min intrinsic widths
    ///
    /// # Contract
    ///
    /// - MUST NOT trigger layout
    /// - MUST be deterministic (same input → same output)
    /// - SHOULD be relatively cheap to compute
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl RenderBox<Leaf> for RenderText {
    ///     fn compute_min_intrinsic_width(&self, _height: f32) -> f32 {
    ///         // Width of the widest word
    ///         self.measure_min_width()
    ///     }
    /// }
    /// ```
    fn compute_min_intrinsic_width(&self, _height: f32) -> f32 {
        0.0
    }

    /// Returns the smallest width beyond which increasing the width never
    /// decreases the preferred height.
    ///
    /// For example, for text, this is the width at which the text would
    /// not need to wrap.
    ///
    /// # Flutter Semantics
    ///
    /// This is the widest width this render object would ever naturally want.
    /// For example:
    /// - Text: Width of text laid out on a single line
    /// - Image: Natural image width
    /// - Row: Sum of children's max intrinsic widths
    ///
    /// # Contract
    ///
    /// - MUST NOT trigger layout
    /// - MUST be >= compute_min_intrinsic_width(height)
    /// - MUST be deterministic
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl RenderBox<Leaf> for RenderText {
    ///     fn compute_max_intrinsic_width(&self, _height: f32) -> f32 {
    ///         // Width of text on a single line
    ///         self.measure_max_width()
    ///     }
    /// }
    /// ```
    fn compute_max_intrinsic_width(&self, _height: f32) -> f32 {
        0.0
    }

    /// Returns the minimum height that this box could be without failing to
    /// correctly paint its contents within itself, without clipping.
    ///
    /// The width argument may give a specific width to assume. The given
    /// width can be infinite, meaning that the intrinsic height should be
    /// determined assuming unbounded width constraints.
    ///
    /// # Flutter Semantics
    ///
    /// This is the shortest height this render object can have while still
    /// being able to paint correctly. For example:
    /// - Text: Height of text at given width (with wrapping)
    /// - Image: 0 (can scale down to nothing)
    /// - Column: Sum of children's min intrinsic heights
    ///
    /// # Contract
    ///
    /// - MUST NOT trigger layout
    /// - MUST be deterministic
    /// - Width of INFINITY means unbounded
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl RenderBox<Leaf> for RenderText {
    ///     fn compute_min_intrinsic_height(&self, width: f32) -> f32 {
    ///         // Height of text wrapped at given width
    ///         self.measure_height_at_width(width)
    ///     }
    /// }
    /// ```
    fn compute_min_intrinsic_height(&self, _width: f32) -> f32 {
        0.0
    }

    /// Returns the smallest height beyond which increasing the height never
    /// decreases the preferred width.
    ///
    /// If the layout algorithm is width-in-height-out (like text), this
    /// should return the same as compute_min_intrinsic_height.
    ///
    /// # Flutter Semantics
    ///
    /// This is the tallest height this render object would ever naturally want.
    /// For example:
    /// - Text: Same as min intrinsic height (width determines height)
    /// - Image: Natural image height
    /// - Column: Sum of children's max intrinsic heights
    ///
    /// # Contract
    ///
    /// - MUST NOT trigger layout
    /// - MUST be >= compute_min_intrinsic_height(width)
    /// - MUST be deterministic
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl RenderBox<Leaf> for RenderImage {
    ///     fn compute_max_intrinsic_height(&self, width: f32) -> f32 {
    ///         // Natural image height (aspect ratio preserved)
    ///         self.natural_height_for_width(width)
    ///     }
    /// }
    /// ```
    fn compute_max_intrinsic_height(&self, _width: f32) -> f32 {
        0.0
    }

    /// Returns intrinsic width for given height (legacy API).
    ///
    /// This is a simplified API that returns the preferred width. For more
    /// precise control, use `compute_min_intrinsic_width` and
    /// `compute_max_intrinsic_width` instead.
    ///
    /// # Default Implementation
    ///
    /// Delegates to `compute_max_intrinsic_width`.
    fn intrinsic_width(&self, height: f32) -> Option<f32> {
        let max_width = self.compute_max_intrinsic_width(height);
        if max_width > 0.0 {
            Some(max_width)
        } else {
            None
        }
    }

    /// Returns intrinsic height for given width (legacy API).
    ///
    /// This is a simplified API that returns the preferred height. For more
    /// precise control, use `compute_min_intrinsic_height` and
    /// `compute_max_intrinsic_height` instead.
    ///
    /// # Default Implementation
    ///
    /// Delegates to `compute_max_intrinsic_height`.
    fn intrinsic_height(&self, width: f32) -> Option<f32> {
        let max_height = self.compute_max_intrinsic_height(width);
        if max_height > 0.0 {
            Some(max_height)
        } else {
            None
        }
    }

    // ========================================================================
    // FLUTTER BASELINE API
    // ========================================================================
    //
    // Flutter's baseline system allows text to align across different widgets.
    // This is critical for:
    // - Row with CrossAxisAlignment.baseline
    // - RichText with mixed font sizes
    // - Form fields with labels
    //
    // There are two baseline types:
    // - Alphabetic: Bottom of letters like 'a', 'e', 'm' (most Latin text)
    // - Ideographic: Bottom of ideographic characters (CJK text)
    // ========================================================================

    /// Returns the distance from the top of the box to the first baseline
    /// of the given type.
    ///
    /// # Arguments
    ///
    /// * `baseline_type` - The type of baseline to measure
    ///
    /// # Flutter Semantics
    ///
    /// - For text: Distance to actual text baseline
    /// - For containers: Distance to first child's baseline
    /// - For non-text: Typically None
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl RenderBox<Leaf> for RenderText {
    ///     fn get_distance_to_baseline(&self, baseline: TextBaseline) -> Option<f32> {
    ///         match baseline {
    ///             TextBaseline::Alphabetic => Some(self.font_metrics.ascent),
    ///             TextBaseline::Ideographic => Some(self.font_metrics.ideographic_baseline),
    ///         }
    ///     }
    /// }
    /// ```
    fn get_distance_to_baseline(&self, _baseline: TextBaseline) -> Option<f32> {
        None
    }

    /// Computes the distance to baseline without triggering layout (dry baseline).
    ///
    /// This is used for intrinsic sizing calculations and should not have
    /// side effects. It receives the child layout function as a parameter
    /// to compute child sizes without actual layout.
    ///
    /// # Flutter Semantics
    ///
    /// Same as `get_distance_to_baseline` but:
    /// - MUST NOT trigger actual layout
    /// - Uses constraints to determine size
    /// - May be called during intrinsic measurement
    ///
    /// # Default Implementation
    ///
    /// Returns None. Override for text-containing render objects.
    fn get_dry_baseline(
        &self,
        _constraints: BoxConstraints,
        _baseline: TextBaseline,
    ) -> Option<f32> {
        None
    }

    /// Returns baseline offset for text alignment (legacy API).
    ///
    /// This is a simplified API. For Flutter-compliant baseline handling,
    /// use `get_distance_to_baseline` instead.
    ///
    /// # Default Implementation
    ///
    /// Delegates to `get_distance_to_baseline(TextBaseline::Alphabetic)`.
    fn baseline_offset(&self) -> Option<f32> {
        self.get_distance_to_baseline(TextBaseline::Alphabetic)
    }

    // ========================================================================
    // FLUTTER DRY LAYOUT API
    // ========================================================================
    //
    // Dry layout computes what size would be returned by layout() given
    // certain constraints, WITHOUT actually performing layout (no side effects).
    //
    // This is critical for:
    // - Intrinsic size calculations that need child sizes
    // - LayoutBuilder that measures before building
    // - Widgets that need to measure alternatives
    //
    // The "dry" prefix means: no mutations, no child positioning, no caching.
    // ========================================================================

    /// Computes the size this box would have if given the provided constraints,
    /// without actually performing layout.
    ///
    /// # Flutter Semantics
    ///
    /// This method must:
    /// - Return the same size that layout() would return
    /// - NOT trigger actual layout
    /// - NOT modify any state
    /// - NOT position children
    ///
    /// # When to Override
    ///
    /// Override if your layout depends on children:
    /// - Use `ctx.get_dry_layout(child_id, child_constraints)` to get child sizes
    /// - Compute your size based on child sizes
    /// - Do NOT call `ctx.layout_child()` (that's actual layout)
    ///
    /// # Default Implementation
    ///
    /// Returns `constraints.smallest()`. This is correct for:
    /// - Leaf nodes with no intrinsic size
    /// - Nodes that always want minimum size
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl RenderBox<Single> for RenderPadding {
    ///     fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
    ///         let inner_constraints = constraints.deflate(&self.padding);
    ///         // Would need child's dry layout here - simplified for example
    ///         let child_size = Size::ZERO;
    ///         Size::new(
    ///             child_size.width + self.padding.horizontal(),
    ///             child_size.height + self.padding.vertical(),
    ///         )
    ///     }
    /// }
    /// ```
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
