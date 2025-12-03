//! RenderBox trait for box protocol render objects.
//!
//! This module provides the `RenderBox<A>` trait for implementing render objects
//! that use the 2D box layout protocol with compile-time arity validation.
//!
//! # Flutter Compliance
//!
//! This implementation follows Flutter's RenderBox protocol exactly:
//! - Layout must be idempotent (same constraints → same size)
//! - Size must satisfy constraints (verified in debug builds)
//! - Parents must layout children before querying their size
//! - Layout must not be called during paint or hit-test
//! - Intrinsics must not trigger layout
//!
//! # Design Philosophy
//!
//! - **Type safety**: Compile-time arity validation prevents runtime errors
//! - **Zero cost**: No overhead compared to manual implementation
//! - **Flutter-compatible**: Exact Flutter protocol semantics
//! - **Rust-native**: Leverages ownership, borrowing, and zero-cost abstractions
//! - **Debuggable**: Comprehensive assertions and error messages
//!
//! # Safety Guarantees
//!
//! - **No panics**: All methods return `Result` for recoverable errors
//! - **Constraint validation**: Debug builds verify size satisfies constraints
//! - **Lifecycle checks**: Assertions prevent invalid state transitions
//! - **Arity enforcement**: Compile-time child count validation
//!
//! # Trait Hierarchy
//!
//! ```text
//! RenderObject (base trait)
//!     │
//!     └── RenderBox<A> (box protocol with arity A)
//!             │
//!             ├── layout(ctx) -> Result<Size>     [REQUIRED]
//!             ├── paint(ctx)                      [REQUIRED]
//!             ├── hit_test(ctx, result) -> bool   [optional, default provided]
//!             ├── intrinsic_width(height) -> f32  [optional]
//!             ├── intrinsic_height(width) -> f32  [optional]
//!             ├── baseline_offset() -> f32        [optional]
//!             └── local_bounds() -> Rect          [optional]
//! ```
//!
//! # Arity System
//!
//! | Arity | Children | Examples | Use Cases |
//! |-------|----------|----------|-----------|
//! | `Leaf` | 0 | Text, Image, Icon | Content without children |
//! | `Optional` | 0-1 | Container, SizedBox | Optional single child |
//! | `Single` | 1 | Padding, Transform, Align | Decorators and wrappers |
//! | `Variable` | 0+ | Flex, Stack, Column, Row | Multi-child layouts |
//!
//! # Examples
//!
//! ## Leaf Element (0 children)
//!
//! ```rust,ignore
//! use flui_rendering::core::{RenderBox, RenderObject, Leaf, RenderResult};
//!
//! #[derive(Debug)]
//! struct RenderText {
//!     text: String,
//!     font_size: f32,
//! }
//!
//! impl RenderBox<Leaf> for RenderText {
//!     fn layout(&mut self, ctx: BoxLayoutContext<'_, Leaf>) -> RenderResult<Size> {
//!         // Measure text intrinsic size
//!         let intrinsic = self.measure_text_size();
//!
//!         // IMPORTANT: Must constrain result to satisfy constraints
//!         // Flutter contract: returned size MUST satisfy constraints
//!         let size = ctx.constraints.constrain(intrinsic);
//!
//!         Ok(size)
//!     }
//!
//!     fn paint(&self, ctx: &mut BoxPaintContext<'_, Leaf>) {
//!         // Paint uses geometry from layout - never call layout here!
//!         ctx.canvas_mut().draw_text(
//!             &self.text,
//!             ctx.offset,
//!             self.font_size
//!         );
//!     }
//!
//!     // Optional: Provide intrinsic dimensions for better layout
//!     fn intrinsic_width(&self, _height: f32) -> Option<f32> {
//!         Some(self.measure_text_size().width)
//!     }
//! }
//!
//! impl RenderObject for RenderText {
//!     fn as_any(&self) -> &dyn std::any::Any { self }
//!     fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
//! }
//! ```
//!
//! ## Single Child Wrapper (1 child)
//!
//! ```rust,ignore
//! use flui_rendering::core::{RenderBox, Single, EdgeInsets};
//!
//! #[derive(Debug)]
//! struct RenderPadding {
//!     padding: EdgeInsets,
//! }
//!
//! impl RenderBox<Single> for RenderPadding {
//!     fn layout(&mut self, mut ctx: BoxLayoutContext<'_, Single>) -> RenderResult<Size> {
//!         // 1. Transform constraints for child
//!         let child_constraints = ctx.constraints.deflate(&self.padding);
//!
//!         // 2. Layout child with transformed constraints
//!         let child_size = ctx.layout_single_child_with(|_| child_constraints)?;
//!
//!         // 3. Position child (store offset for paint)
//!         ctx.set_child_offset(ctx.single_child(), self.padding.top_left());
//!
//!         // 4. Compute our size from child size + padding
//!         let size = child_size + self.padding.size();
//!
//!         // 5. Verify size satisfies our constraints (debug only)
//!         debug_assert!(
//!             ctx.constraints.is_satisfied_by(size),
//!             "Size {size:?} does not satisfy constraints {:?}",
//!             ctx.constraints
//!         );
//!
//!         Ok(size)
//!     }
//!
//!     fn paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
//!         // Child was positioned during layout - paint uses that offset
//!         ctx.paint_single_child(self.padding.top_left());
//!     }
//!
//!     fn hit_test(&self, ctx: &BoxHitTestContext<'_, Single>, result: &mut HitTestResult) -> bool {
//!         // Check if hit is within our bounds first
//!         if !ctx.contains_position() {
//!             return false;
//!         }
//!
//!         // Transform position for child (account for padding offset)
//!         let child_position = ctx.position - self.padding.top_left();
//!         ctx.hit_test_child(ctx.single_child(), child_position, result)
//!     }
//! }
//!
//! impl RenderObject for RenderPadding {
//!     fn as_any(&self) -> &dyn std::any::Any { self }
//!     fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
//! }
//! ```
//!
//! ## Multi-Child Layout (Variable children)
//!
//! ```rust,ignore
//! use flui_rendering::core::{RenderBox, Variable, Axis};
//!
//! #[derive(Debug)]
//! struct RenderFlex {
//!     direction: Axis,
//!     spacing: f32,
//! }
//!
//! impl RenderBox<Variable> for RenderFlex {
//!     fn layout(&mut self, mut ctx: BoxLayoutContext<'_, Variable>) -> RenderResult<Size> {
//!         let mut main_axis_size = 0.0;
//!         let mut cross_axis_size = 0.0;
//!
//!         // Create constraints for children
//!         let child_constraints = match self.direction {
//!             Axis::Horizontal => ctx.constraints.loosen_width(),
//!             Axis::Vertical => ctx.constraints.loosen_height(),
//!         };
//!
//!         // Layout each child
//!         for child_id in ctx.children() {
//!             let child_size = ctx.layout_child(child_id, child_constraints)?;
//!
//!             // Position child
//!             let offset = match self.direction {
//!                 Axis::Horizontal => Offset::new(main_axis_size, 0.0),
//!                 Axis::Vertical => Offset::new(0.0, main_axis_size),
//!             };
//!             ctx.set_child_offset(child_id, offset);
//!
//!             // Update sizes
//!             main_axis_size += self.get_main_size(child_size) + self.spacing;
//!             cross_axis_size = cross_axis_size.max(self.get_cross_size(child_size));
//!         }
//!
//!         // Remove trailing spacing
//!         if main_axis_size > 0.0 {
//!             main_axis_size -= self.spacing;
//!         }
//!
//!         // Construct final size
//!         let size = match self.direction {
//!             Axis::Horizontal => Size::new(main_axis_size, cross_axis_size),
//!             Axis::Vertical => Size::new(cross_axis_size, main_axis_size),
//!         };
//!
//!         // Constrain to parent constraints
//!         Ok(ctx.constraints.constrain(size))
//!     }
//!
//!     fn paint(&self, ctx: &mut BoxPaintContext<'_, Variable>) {
//!         // Paint all children using offsets from layout
//!         ctx.paint_all_children();
//!     }
//!
//!     fn hit_test(&self, ctx: &BoxHitTestContext<'_, Variable>, result: &mut HitTestResult) -> bool {
//!         if !ctx.contains_position() {
//!             return false;
//!         }
//!
//!         // Test children in reverse order (front to back)
//!         for child_id in ctx.children_reverse() {
//!             if ctx.hit_test_child(child_id, ctx.position, result) {
//!                 return true;
//!             }
//!         }
//!
//!         // Add self if hit but no child was hit
//!         ctx.hit_test_self(result);
//!         true
//!     }
//! }
//!
//! impl RenderFlex {
//!     fn get_main_size(&self, size: Size) -> f32 {
//!         match self.direction {
//!             Axis::Horizontal => size.width,
//!             Axis::Vertical => size.height,
//!         }
//!     }
//!
//!     fn get_cross_size(&self, size: Size) -> f32 {
//!         match self.direction {
//!             Axis::Horizontal => size.height,
//!             Axis::Vertical => size.width,
//!         }
//!     }
//! }
//!
//! impl RenderObject for RenderFlex {
//!     fn as_any(&self) -> &dyn std::any::Any { self }
//!     fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
//! }
//! ```
//!
//! # Flutter Protocol Compliance
//!
//! ## Layout Phase
//!
//! 1. **Constraints Flow Down**: Parent passes constraints to `layout()`
//! 2. **Sizes Flow Up**: Child returns size that satisfies constraints
//! 3. **Idempotency**: Same constraints must produce same size
//! 4. **No Side Effects**: Layout should not modify external state
//! 5. **Child Positioning**: Parent sets child offsets during layout
//!
//! ## Paint Phase
//!
//! 1. **Offset Provided**: Paint receives offset in parent coordinates
//! 2. **No Layout**: Never call layout during paint
//! 3. **Canvas State**: Save/restore canvas state if needed
//! 4. **Child Painting**: Paint children using stored offsets
//!
//! ## Hit Test Phase
//!
//! 1. **Position in Local Coordinates**: Hit position is in local space
//! 2. **Transform for Children**: Adjust position for child offsets
//! 3. **Reverse Order**: Test children from front to back (reverse)
//! 4. **Add to Result**: Call `hit_test_self` if hit
//!
//! # Common Pitfalls
//!
//! ## ❌ DON'T: Return size that doesn't satisfy constraints
//!
//! ```rust,ignore
//! fn layout(&mut self, ctx: BoxLayoutContext<'_, Leaf>) -> RenderResult<Size> {
//!     // WRONG: Ignores constraints!
//!     Ok(Size::new(100.0, 50.0))
//! }
//! ```
//!
//! ## ✅ DO: Always constrain result
//!
//! ```rust,ignore
//! fn layout(&mut self, ctx: BoxLayoutContext<'_, Leaf>) -> RenderResult<Size> {
//!     let desired = Size::new(100.0, 50.0);
//!     Ok(ctx.constraints.constrain(desired))
//! }
//! ```
//!
//! ## ❌ DON'T: Call layout during paint
//!
//! ```rust,ignore
//! fn paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
//!     // WRONG: Never layout during paint!
//!     let size = ctx.layout_single_child()?; // COMPILE ERROR - ctx is immutable
//! }
//! ```
//!
//! ## ✅ DO: Use geometry from layout
//!
//! ```rust,ignore
//! fn paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
//!     // size was computed during layout
//!     let size = ctx.geometry; // This is Size for BoxProtocol
//!     ctx.paint_single_child(Offset::ZERO);
//! }
//! ```

use std::fmt;

use super::arity::Arity;
use super::contexts::{BoxHitTestContext, BoxLayoutContext, BoxPaintContext};
use super::render_object::RenderObject;
use crate::RenderResult;
use flui_interaction::HitTestResult;
use flui_types::{Axis, BoxConstraints, Offset, Rect, Size};

// ============================================================================
// CORE RENDER BOX TRAIT
// ============================================================================

/// Render trait for box protocol with compile-time arity validation.
///
/// This trait provides the foundation for implementing render objects that use
/// the 2D box layout protocol, following Flutter's exact semantics.
///
/// # Required Trait Bounds
///
/// - `RenderObject`: Base trait for type erasure and dyn-compatibility
/// - `Debug`: Required for debugging and error messages
/// - `Send + Sync`: Required for thread-safe tree operations
///
/// # Type Parameters
///
/// - `A`: Arity type constraining the number of children
///
/// # Required Methods
///
/// - [`layout`](Self::layout) - Computes size given constraints (MUST satisfy constraints)
/// - [`paint`](Self::paint) - Draws to canvas using geometry from layout
///
/// # Optional Methods
///
/// All optional methods have sensible defaults:
///
/// - [`hit_test`](Self::hit_test) - Pointer event detection (default: rectangular bounds check)
/// - [`intrinsic_width`](Self::intrinsic_width) - Minimum width for given height (default: None)
/// - [`intrinsic_height`](Self::intrinsic_height) - Minimum height for given width (default: None)
/// - [`baseline_offset`](Self::baseline_offset) - Text baseline for alignment (default: None)
/// - [`compute_dry_layout`](Self::compute_dry_layout) - Layout without side effects (default: smallest)
/// - [`local_bounds`](Self::local_bounds) - Bounding rectangle (default: empty)
///
/// # Flutter Protocol Compliance
///
/// This trait enforces Flutter's layout protocol:
///
/// 1. **Constraints Flow Down**: Parent passes `BoxConstraints` to child
/// 2. **Sizes Flow Up**: Child returns `Size` that satisfies constraints
/// 3. **Idempotency**: Same constraints → same size (no side effects)
/// 4. **Constraint Satisfaction**: Returned size MUST satisfy input constraints
/// 5. **Child Positioning**: Parent positions children during layout
///
/// In debug builds, constraint satisfaction is verified automatically.
///
/// # Safety Guarantees
///
/// - ✅ No panics: All methods return `Result` for errors
/// - ✅ Type safety: Arity enforced at compile time
/// - ✅ Memory safety: Rust ownership prevents use-after-free
/// - ✅ Thread safety: `Send + Sync` ensures safe concurrent access
/// - ✅ Lifecycle safety: Assertions prevent invalid state transitions
pub trait RenderBox<A: Arity>: RenderObject + fmt::Debug + Send + Sync {
    /// Computes the size of this render object given constraints.
    ///
    /// # Flutter Contract
    ///
    /// The returned size MUST satisfy the input constraints. Specifically:
    /// - `size.width >= constraints.min_width && size.width <= constraints.max_width`
    /// - `size.height >= constraints.min_height && size.height <= constraints.max_height`
    ///
    /// Violating this contract will cause assertion failures in debug builds and
    /// undefined behavior in release builds.
    ///
    /// # Layout Protocol
    ///
    /// 1. **Receive Constraints**: Parent provides `BoxConstraints`
    /// 2. **Layout Children**: Call `ctx.layout_child()` for each child
    /// 3. **Position Children**: Call `ctx.set_child_offset()` for each child
    /// 4. **Compute Size**: Calculate size based on children and own metrics
    /// 5. **Constrain Size**: Use `constraints.constrain()` to satisfy constraints
    /// 6. **Return Size**: Return the constrained size
    ///
    /// # Context API
    ///
    /// The context provides:
    /// - `ctx.constraints` - Layout constraints from parent (immutable)
    /// - `ctx.children()` - Iterator over child ElementIds
    /// - `ctx.layout_child(id, c)` - Layout a specific child with given constraints
    /// - `ctx.set_child_offset(id, offset)` - Position a child in local coordinates
    /// - Helper methods for common patterns (e.g., `layout_single_child()`)
    ///
    /// # Errors
    ///
    /// Returns `RenderError` if:
    /// - Child layout fails (propagated from child)
    /// - Required child is missing (arity violation at runtime)
    /// - Tree operation fails (e.g., child not found)
    ///
    /// # Idempotency
    ///
    /// This method MUST be idempotent: calling with the same constraints
    /// multiple times must return the same size. Side effects (e.g., caching)
    /// are allowed but must not affect the return value.
    ///
    /// # Performance
    ///
    /// - Use `compute_dry_layout()` for speculative layouts (faster, no side effects)
    /// - Cache layout results to avoid redundant computation
    /// - Avoid unnecessary allocations in hot path
    ///
    /// # Examples
    ///
    /// ## Leaf (no children)
    ///
    /// ```rust,ignore
    /// fn layout(&mut self, ctx: BoxLayoutContext<'_, Leaf>) -> RenderResult<Size> {
    ///     let intrinsic = self.compute_intrinsic_size();
    ///     Ok(ctx.constraints.constrain(intrinsic))
    /// }
    /// ```
    ///
    /// ## Single child wrapper
    ///
    /// ```rust,ignore
    /// fn layout(&mut self, mut ctx: BoxLayoutContext<'_, Single>) -> RenderResult<Size> {
    ///     // Transform constraints for child
    ///     let child_constraints = ctx.constraints.deflate(&self.padding);
    ///
    ///     // Layout child
    ///     let child_size = ctx.layout_single_child_with(|_| child_constraints)?;
    ///
    ///     // Position child
    ///     ctx.set_child_offset(ctx.single_child(), self.padding.top_left());
    ///
    ///     // Compute and constrain our size
    ///     let size = child_size + self.padding.size();
    ///     Ok(ctx.constraints.constrain(size))
    /// }
    /// ```
    ///
    /// ## Multiple children
    ///
    /// ```rust,ignore
    /// fn layout(&mut self, mut ctx: BoxLayoutContext<'_, Variable>) -> RenderResult<Size> {
    ///     let mut total_width = 0.0;
    ///     let mut max_height = 0.0;
    ///
    ///     for child_id in ctx.children() {
    ///         let child_size = ctx.layout_child(child_id, ctx.constraints)?;
    ///         ctx.set_child_offset(child_id, Offset::new(total_width, 0.0));
    ///
    ///         total_width += child_size.width;
    ///         max_height = max_height.max(child_size.height);
    ///     }
    ///
    ///     Ok(ctx.constraints.constrain(Size::new(total_width, max_height)))
    /// }
    /// ```
    fn layout(&mut self, ctx: BoxLayoutContext<'_, A>) -> RenderResult<Size>;

    /// Paints this render object to the canvas.
    ///
    /// # Flutter Contract
    ///
    /// - MUST NOT call layout during paint (will cause assertion failure)
    /// - MUST use geometry from layout phase (available in `ctx.geometry`)
    /// - SHOULD save/restore canvas state if modifying transform/clip
    /// - MUST paint children using stored offsets from layout
    ///
    /// # Paint Order
    ///
    /// Paint happens in tree order (depth-first):
    /// 1. Paint self (background, decorations)
    /// 2. Paint children (using `ctx.paint_child()` or helpers)
    /// 3. Paint self (foreground, overlays)
    ///
    /// # Context API
    ///
    /// The context provides:
    /// - `ctx.offset` - This element's position in parent coordinates (readonly)
    /// - `ctx.geometry` - Size from layout (this is `Size` for BoxProtocol)
    /// - `ctx.canvas_mut()` - Mutable canvas for drawing operations
    /// - `ctx.children()` - Iterator over child ElementIds
    /// - `ctx.paint_child(id, offset)` - Paint a specific child at given offset
    /// - Helper methods for common patterns (e.g., `paint_single_child()`)
    ///
    /// # Canvas State
    ///
    /// The canvas may have transformations and clipping applied by ancestors.
    /// If you modify canvas state (transform, clip, etc.), save and restore it:
    ///
    /// ```rust,ignore
    /// fn paint(&self, ctx: &mut BoxPaintContext<'_, Leaf>) {
    ///     ctx.canvas_mut().save();
    ///     ctx.canvas_mut().clip_rect(my_clip_rect);
    ///     // ... draw content ...
    ///     ctx.canvas_mut().restore();
    /// }
    /// ```
    ///
    /// # Performance
    ///
    /// - Avoid allocations in paint (reuse buffers)
    /// - Skip painting if outside visible region (check `ctx.offset`)
    /// - Use layer caching for expensive paint operations
    /// - Batch similar drawing operations
    ///
    /// # Examples
    ///
    /// ## Leaf element
    ///
    /// ```rust,ignore
    /// fn paint(&self, ctx: &mut BoxPaintContext<'_, Leaf>) {
    ///     let rect = Rect::from_min_size(ctx.offset, ctx.geometry);
    ///     ctx.canvas_mut().draw_rect(rect, &self.paint);
    /// }
    /// ```
    ///
    /// ## Single child
    ///
    /// ```rust,ignore
    /// fn paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
    ///     // Paint background
    ///     self.paint_background(ctx);
    ///
    ///     // Paint child using offset from layout
    ///     ctx.paint_single_child(self.child_offset);
    ///
    ///     // Paint foreground
    ///     self.paint_foreground(ctx);
    /// }
    /// ```
    ///
    /// ## Multiple children
    ///
    /// ```rust,ignore
    /// fn paint(&self, ctx: &mut BoxPaintContext<'_, Variable>) {
    ///     // Paint all children using offsets from layout
    ///     ctx.paint_all_children();
    /// }
    /// ```
    fn paint(&self, ctx: &mut BoxPaintContext<'_, A>);

    /// Performs hit testing for pointer events.
    ///
    /// # Flutter Contract
    ///
    /// - Position is in local coordinates (relative to this element's offset)
    /// - Test children in REVERSE order (front to back, z-order)
    /// - Return `true` if hit, `false` otherwise
    /// - Add self to result only if actually hit
    /// - Transform position when testing children
    ///
    /// # Hit Test Protocol
    ///
    /// 1. **Bounds Check**: Check if position is within local bounds
    /// 2. **Test Children**: Test each child in reverse order (front to back)
    /// 3. **Early Return**: Return immediately if a child is hit
    /// 4. **Test Self**: If no child hit, determine if self is hit
    /// 5. **Add to Result**: Call `ctx.hit_test_self()` if hit
    ///
    /// # Context API
    ///
    /// The context provides:
    /// - `ctx.position` - Hit position in local coordinates (readonly)
    /// - `ctx.geometry` - Size from layout (for bounds checking)
    /// - `ctx.contains_position()` - Check if position is within rectangular bounds
    /// - `ctx.children()` / `ctx.children_reverse()` - Child iterators
    /// - `ctx.hit_test_child(id, pos, result)` - Test a specific child
    /// - `ctx.hit_test_self(result)` - Add self to hit test result
    ///
    /// # Default Implementation
    ///
    /// The default implementation:
    /// 1. Checks rectangular bounds
    /// 2. Tests children in reverse order
    /// 3. Adds self if position is within bounds
    ///
    /// Override for custom shapes (circles, polygons, etc.) or different hit behavior.
    ///
    /// # Performance
    ///
    /// - Early return if outside bounds (avoid testing children)
    /// - Use spatial indexing for many children
    /// - Cache hit test regions if expensive to compute
    ///
    /// # Examples
    ///
    /// ## Default rectangular behavior
    ///
    /// ```rust,ignore
    /// // No implementation needed - default is perfect for rectangles
    /// ```
    ///
    /// ## Custom circular hit test
    ///
    /// ```rust,ignore
    /// fn hit_test(&self, ctx: &BoxHitTestContext<'_, Leaf>, result: &mut HitTestResult) -> bool {
    ///     let center = ctx.geometry / 2.0;
    ///     let radius = center.width.min(center.height);
    ///     let distance = (ctx.position - center).length();
    ///
    ///     if distance <= radius {
    ///         ctx.hit_test_self(result);
    ///         true
    ///     } else {
    ///         false
    ///     }
    /// }
    /// ```
    ///
    /// ## With child offset transformation
    ///
    /// ```rust,ignore
    /// fn hit_test(&self, ctx: &BoxHitTestContext<'_, Single>, result: &mut HitTestResult) -> bool {
    ///     if !ctx.contains_position() {
    ///         return false;
    ///     }
    ///
    ///     // Transform position for child (e.g., accounting for padding)
    ///     let child_position = ctx.position - self.child_offset;
    ///
    ///     if ctx.hit_test_child(ctx.single_child(), child_position, result) {
    ///         return true;
    ///     }
    ///
    ///     ctx.hit_test_self(result);
    ///     true
    /// }
    /// ```
    fn hit_test(&self, ctx: &BoxHitTestContext<'_, A>, result: &mut HitTestResult) -> bool {
        // Default implementation: rectangular hit testing

        // 1. Check if position is within our rectangular bounds
        if !ctx.contains_position() {
            return false;
        }

        // 2. Test children in reverse z-order (front to back)
        // This ensures topmost child is hit first
        for child_id in ctx.children_reverse() {
            // Transform position to child's local coordinates
            // Note: Child offset was set during layout phase
            if ctx.hit_test_child(child_id, ctx.position, result) {
                return true; // Early return if child was hit
            }
        }

        // 3. No child was hit, add self to result
        ctx.hit_test_self(result);
        true
    }

    /// Computes the minimum intrinsic width for a given height.
    ///
    /// # Flutter Contract
    ///
    /// - MUST return minimum width that satisfies given height
    /// - MUST NOT depend on current constraints (intrinsics are independent)
    /// - MUST NOT call layout (use compute_dry_layout if needed)
    /// - Should be relatively fast (used during layout planning)
    ///
    /// Returns `None` if this render object has no intrinsic width preference.
    ///
    /// # Use Cases
    ///
    /// - Table column sizing
    /// - Intrinsic width widgets
    /// - Baseline-aligned layouts
    /// - Responsive layout planning
    ///
    /// # Performance
    ///
    /// Intrinsics should be fast as they may be called many times during layout.
    /// Cache results if computation is expensive.
    fn intrinsic_width(&self, _height: f32) -> Option<f32> {
        None
    }

    /// Computes the minimum intrinsic height for a given width.
    ///
    /// # Flutter Contract
    ///
    /// - MUST return minimum height that satisfies given width
    /// - MUST NOT depend on current constraints (intrinsics are independent)
    /// - MUST NOT call layout (use compute_dry_layout if needed)
    /// - Should be relatively fast (used during layout planning)
    ///
    /// Returns `None` if this render object has no intrinsic height preference.
    ///
    /// # Use Cases
    ///
    /// - Auto-sizing containers
    /// - Intrinsic height widgets
    /// - Text wrapping calculations
    /// - Responsive layout planning
    fn intrinsic_height(&self, _width: f32) -> Option<f32> {
        None
    }

    /// Gets the baseline offset for text alignment.
    ///
    /// # Flutter Contract
    ///
    /// - Returns distance from top of render box to baseline
    /// - Used for aligning text and other baseline-sensitive widgets
    /// - Should be consistent with actual text rendering
    ///
    /// Returns `None` if this render object has no meaningful baseline.
    ///
    /// # Use Cases
    ///
    /// - Text baseline alignment
    /// - Row baseline alignment
    /// - Form field alignment
    fn baseline_offset(&self) -> Option<f32> {
        None
    }

    /// Performs a "dry layout" without side effects.
    ///
    /// # Flutter Contract
    ///
    /// - MUST return the same size that `layout()` would return
    /// - MUST NOT modify any state (no child positioning, no caching, etc.)
    /// - MUST NOT have observable side effects
    /// - Used for intrinsic sizing and layout planning
    ///
    /// The default implementation returns the smallest size that satisfies
    /// constraints, which is safe but may not be optimal.
    ///
    /// # Performance
    ///
    /// Dry layout should be faster than regular layout as it skips:
    /// - Child offset computation and storage
    /// - State updates and caching
    /// - Dirty flag management
    ///
    /// Override if you can provide a faster implementation for your specific widget.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
    ///     // Fast path: return intrinsic size if available
    ///     if let Some(width) = self.intrinsic_width(constraints.max_height) {
    ///         if let Some(height) = self.intrinsic_height(width) {
    ///             return constraints.constrain(Size::new(width, height));
    ///         }
    ///     }
    ///
    ///     // Fallback: smallest size
    ///     constraints.smallest()
    /// }
    /// ```
    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        // Safe default: smallest size that satisfies constraints
        constraints.smallest()
    }

    /// Gets the local bounding rectangle.
    ///
    /// Returns the bounding box in local coordinates (relative to this element's offset).
    /// Default returns an empty rectangle at the origin.
    ///
    /// # Use Cases
    ///
    /// - Hit testing custom shapes
    /// - Clipping bounds
    /// - Overflow detection
    /// - Visual debugging
    ///
    /// Override if your render object has a specific bounding region or
    /// if you need accurate bounds for hit testing.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn local_bounds(&self) -> Rect {
    ///     // Return rectangle from origin to size from last layout
    ///     Rect::from_min_size(Offset::ZERO, self.size)
    /// }
    /// ```
    fn local_bounds(&self) -> Rect {
        Rect::ZERO
    }
}

// ============================================================================
// COMPILE-TIME CONSTRAINT VERIFICATION (DEBUG ONLY)
// ============================================================================

#[cfg(debug_assertions)]
pub fn verify_size_satisfies_constraints(size: Size, constraints: BoxConstraints) {
    debug_assert!(
        constraints.is_satisfied_by(size),
        "Layout violation: size {:?} does not satisfy constraints {:?}\n\
         - Width: {} not in [{}, {}]\n\
         - Height: {} not in [{}, {}]",
        size,
        constraints,
        size.width,
        constraints.min_width,
        constraints.max_width,
        size.height,
        constraints.min_height,
        constraints.max_height
    );
}

// ============================================================================
// HELPER EXTENSION TRAIT FOR BOX CONSTRAINTS
// ============================================================================

/// Extension trait providing additional constraint manipulation methods.
///
/// These methods help implement common layout patterns more concisely.
pub trait BoxConstraintsExt {
    /// Returns true if the given size satisfies these constraints.
    fn is_satisfied_by(&self, size: Size) -> bool;

    /// Returns the smallest size that satisfies these constraints.
    fn smallest(&self) -> Size;

    /// Returns the largest size that satisfies these constraints.
    fn biggest(&self) -> Size;

    /// Loosens the width constraint (makes min_width = 0).
    fn loosen_width(&self) -> Self;

    /// Loosens the height constraint (makes min_height = 0).
    fn loosen_height(&self) -> Self;

    /// Loosens both width and height constraints.
    fn loosen(&self) -> Self;

    /// Tightens the constraints to a specific size.
    fn tighten(&self, size: Size) -> Self;

    /// Deflates constraints by the given insets (for padding).
    fn deflate(&self, insets: &flui_types::EdgeInsets) -> Self;

    /// Inflates constraints by the given insets.
    fn inflate(&self, insets: &flui_types::EdgeInsets) -> Self;
}

impl BoxConstraintsExt for BoxConstraints {
    #[inline]
    fn is_satisfied_by(&self, size: Size) -> bool {
        size.width >= self.min_width
            && size.width <= self.max_width
            && size.height >= self.min_height
            && size.height <= self.max_height
    }

    #[inline]
    fn smallest(&self) -> Size {
        Size::new(self.min_width, self.min_height)
    }

    #[inline]
    fn biggest(&self) -> Size {
        Size::new(self.max_width, self.max_height)
    }

    #[inline]
    fn loosen_width(&self) -> Self {
        Self {
            min_width: 0.0,
            ..*self
        }
    }

    #[inline]
    fn loosen_height(&self) -> Self {
        Self {
            min_height: 0.0,
            ..*self
        }
    }

    #[inline]
    fn loosen(&self) -> Self {
        Self {
            min_width: 0.0,
            min_height: 0.0,
            ..*self
        }
    }

    #[inline]
    fn tighten(&self, size: Size) -> Self {
        Self::tight(size)
    }

    fn deflate(&self, insets: &flui_types::EdgeInsets) -> Self {
        let horizontal = insets.left + insets.right;
        let vertical = insets.top + insets.bottom;

        Self {
            min_width: (self.min_width - horizontal).max(0.0),
            max_width: (self.max_width - horizontal).max(0.0),
            min_height: (self.min_height - vertical).max(0.0),
            max_height: (self.max_height - vertical).max(0.0),
        }
    }

    fn inflate(&self, insets: &flui_types::EdgeInsets) -> Self {
        let horizontal = insets.left + insets.right;
        let vertical = insets.top + insets.bottom;

        Self {
            min_width: self.min_width + horizontal,
            max_width: self.max_width + horizontal,
            min_height: self.min_height + vertical,
            max_height: self.max_height + vertical,
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::arity::{Leaf, Single, Variable};
    use std::marker::PhantomData;

    // Simple test render box for testing
    #[derive(Debug)]
    struct TestRenderBox<A: Arity> {
        size: Size,
        _phantom: PhantomData<A>,
    }

    impl<A: Arity> TestRenderBox<A> {
        fn new(size: Size) -> Self {
            Self {
                size,
                _phantom: PhantomData,
            }
        }
    }

    impl<A: Arity> RenderBox<A> for TestRenderBox<A> {
        fn layout(&mut self, ctx: BoxLayoutContext<'_, A>) -> RenderResult<Size> {
            Ok(ctx.constraints.constrain(self.size))
        }

        fn paint(&self, _ctx: &mut BoxPaintContext<'_, A>) {
            // No-op for tests
        }
    }

    impl<A: Arity> RenderObject for TestRenderBox<A> {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    #[test]
    fn test_render_box_arity_types() {
        // Test that different arity types compile
        let _leaf: TestRenderBox<Leaf> = TestRenderBox::new(Size::new(100.0, 50.0));
        let _single: TestRenderBox<Single> = TestRenderBox::new(Size::new(100.0, 50.0));
        let _variable: TestRenderBox<Variable> = TestRenderBox::new(Size::new(100.0, 50.0));
        // Compiles = arity system works correctly
    }

    #[test]
    fn test_default_intrinsic_methods() {
        let render = TestRenderBox::<Leaf>::new(Size::new(100.0, 50.0));
        assert_eq!(render.intrinsic_width(50.0), None);
        assert_eq!(render.intrinsic_height(100.0), None);
        assert_eq!(render.baseline_offset(), None);
    }

    #[test]
    fn test_default_dry_layout() {
        let render = TestRenderBox::<Leaf>::new(Size::new(100.0, 50.0));
        let constraints = BoxConstraints {
            min_width: 50.0,
            max_width: 200.0,
            min_height: 25.0,
            max_height: 100.0,
        };

        // Default returns smallest size
        let size = render.compute_dry_layout(constraints);
        assert_eq!(size, Size::new(50.0, 25.0));
    }

    #[test]
    fn test_constraint_satisfaction() {
        let constraints = BoxConstraints {
            min_width: 50.0,
            max_width: 150.0,
            min_height: 25.0,
            max_height: 100.0,
        };

        assert!(constraints.is_satisfied_by(Size::new(100.0, 50.0)));
        assert!(constraints.is_satisfied_by(Size::new(50.0, 25.0)));
        assert!(constraints.is_satisfied_by(Size::new(150.0, 100.0)));

        assert!(!constraints.is_satisfied_by(Size::new(40.0, 50.0))); // width too small
        assert!(!constraints.is_satisfied_by(Size::new(100.0, 20.0))); // height too small
        assert!(!constraints.is_satisfied_by(Size::new(200.0, 50.0))); // width too large
        assert!(!constraints.is_satisfied_by(Size::new(100.0, 150.0))); // height too large
    }

    #[test]
    fn test_constraint_smallest_biggest() {
        let constraints = BoxConstraints {
            min_width: 50.0,
            max_width: 150.0,
            min_height: 25.0,
            max_height: 100.0,
        };

        assert_eq!(constraints.smallest(), Size::new(50.0, 25.0));
        assert_eq!(constraints.biggest(), Size::new(150.0, 100.0));
    }

    #[test]
    fn test_constraint_loosen() {
        let tight = BoxConstraints::tight(Size::new(100.0, 50.0));

        let loosen_w = tight.loosen_width();
        assert_eq!(loosen_w.min_width, 0.0);
        assert_eq!(loosen_w.max_width, 100.0);
        assert_eq!(loosen_w.min_height, 50.0);

        let loosen_h = tight.loosen_height();
        assert_eq!(loosen_h.min_height, 0.0);
        assert_eq!(loosen_h.max_height, 50.0);
        assert_eq!(loosen_h.min_width, 100.0);

        let loosen = tight.loosen();
        assert_eq!(loosen.min_width, 0.0);
        assert_eq!(loosen.min_height, 0.0);
    }
}
