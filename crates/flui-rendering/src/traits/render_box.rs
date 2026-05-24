//! RenderBox trait for 2D box layout with Arity-based child management.

use flui_tree::Arity;
use flui_types::{Offset, Point, Rect, Size};

use crate::{
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxLayoutContext, BoxPaintContext, CanvasContext},
    hit_testing::HitTestBehavior,
    parent_data::ParentData,
    protocol::BoxProtocol,
    traits::RenderObject,
};

// ============================================================================
// RenderBox Trait with Arity and ParentData
// ============================================================================

/// Trait for render objects that use 2D cartesian coordinates.
///
/// ## Associated Types
///
/// - `Arity` - Defines child count at compile time (Leaf, Optional, Variable)
/// - `ParentData` - Metadata type that parent stores on children
///
/// ## Example
///
/// ```ignore
/// // Simple leaf with default BoxParentData
/// struct RenderColoredBox { color: Color, size: Size }
///
/// impl RenderBox for RenderColoredBox {
///     type Arity = Leaf;
///     type ParentData = BoxParentData;
///
///     fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<Leaf, BoxParentData>) -> Size {
///         ctx.constraints().constrain(self.size)
///     }
/// }
///
/// // Flex container with FlexParentData on children
/// struct RenderFlex { children: Vec<...> }
///
/// impl RenderBox for RenderFlex {
///     type Arity = Variable;
///     type ParentData = FlexParentData;  // Children get FlexParentData
///
///     fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<Variable, FlexParentData>) -> Size {
///         for child in ctx.iter_children() {
///             // Type-safe access to FlexParentData
///             let flex = child.parent_data().flex;
///             let fit = child.parent_data().fit;
///         }
///     }
/// }
///
/// // Stack container with StackParentData on children
/// struct RenderStack { ... }
///
/// impl RenderBox for RenderStack {
///     type Arity = Variable;
///     type ParentData = StackParentData;  // Children get positioning info
///     ...
/// }
/// ```
/// Trait for render objects that use 2D cartesian coordinates.
///
/// Users implement this trait for their custom render objects.
/// Render objects are automatically converted to `RenderObject<BoxProtocol>`
/// for storage in `RenderTree` via the From trait.
///
/// # Features
///
/// - Intrinsic dimension queries (min/max width/height)
/// - Baseline support for text alignment
/// - Dry layout (compute size without actual layout)
/// - Coordinate conversion (local ↔ global)
pub trait RenderBox: RenderObject<BoxProtocol> + flui_foundation::Diagnosticable {
    /// The arity of this render box (Leaf, Optional, Variable, etc.)
    type Arity: Arity;

    /// The parent data type for children of this render box.
    ///
    /// This determines what metadata the parent can store on each child:
    /// - `BoxParentData` - Basic offset only (default for simple containers)
    /// - `FlexParentData` - Flex factor, fit mode (for Row/Column)
    /// - `StackParentData` - Positioning constraints (for Stack)
    /// - `TableCellParentData` - Row/column span (for Table)
    type ParentData: ParentData + Default;

    // ========================================================================
    // Layout
    // ========================================================================

    /// Computes the layout of this render object.
    ///
    /// The context provides:
    /// - Constraints from parent via `ctx.constraints()`
    /// - Type-safe child access via `ctx.layout_child()`,
    ///   `ctx.position_child()`
    /// - Completion via `ctx.complete_with_size()`
    ///
    /// # Example
    ///
    /// ```ignore
    /// fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<Single, BoxParentData>) {
    ///     let child_size = ctx.layout_single_child_loose();
    ///     ctx.position_single_child_at_origin();
    ///     ctx.complete_with_size(ctx.constrain(child_size));
    /// }
    /// ```
    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Self::Arity, Self::ParentData>);

    /// Returns a reference to the current size of this render object.
    ///
    /// This method must return a reference to allow `RenderObject<BoxProtocol>`
    /// blanket implementation to work correctly.
    fn size(&self) -> &Size;

    /// Returns a mutable reference to the size of this render object.
    ///
    /// This allows direct mutation of the size field without set_size().
    fn size_mut(&mut self) -> &mut Size;

    /// Returns whether this render object has undergone layout and has a size.
    fn has_size(&self) -> bool {
        true
    }

    // ========================================================================
    // Hit Testing
    // ========================================================================

    /// Returns the hit test behavior for this render object.
    fn hit_test_behavior(&self) -> HitTestBehavior {
        HitTestBehavior::Opaque
    }

    /// Hit tests this render object.
    ///
    /// The context provides:
    /// - Position via `ctx.position()` or `ctx.x()`, `ctx.y()`
    /// - Bounds checking via `ctx.is_within_size(w, h)`
    /// - Child testing via `ctx.hit_test_child_at_offset()`
    /// - Result management via `ctx.add_self(id)`
    ///
    /// # Example
    ///
    /// ```ignore
    /// fn hit_test(&self, ctx: &mut BoxHitTestContext<Single, BoxParentData>) -> bool {
    ///     if !ctx.is_within_size(self.size.width, self.size.height) {
    ///         return false;
    ///     }
    ///     // Test children first
    ///     if ctx.hit_test_child_at_offset(0, child_offset) {
    ///         return true;
    ///     }
    ///     ctx.add_self(self.id);
    ///     true
    /// }
    /// ```
    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Self::Arity, Self::ParentData>) -> bool;

    // ========================================================================
    // Parent Data
    // ========================================================================

    /// Creates default parent data for a child.
    ///
    /// Called when a child is adopted. Override if you need custom
    /// initialization.
    fn create_default_parent_data() -> Self::ParentData {
        Self::ParentData::default()
    }

    // ========================================================================
    // Coordinate Conversion
    // ========================================================================

    /// Converts a point from global coordinates to local coordinates.
    fn global_to_local(&self, point: Point) -> Point {
        point
    }

    /// Converts a point from local coordinates to global coordinates.
    fn local_to_global(&self, point: Point) -> Point {
        point
    }

    // ========================================================================
    // Intrinsic Dimensions
    // ========================================================================

    /// Returns the minimum intrinsic width for a given height.
    fn get_min_intrinsic_width(&self, height: f32) -> f32 {
        self.compute_min_intrinsic_width(height)
    }

    /// Returns the maximum intrinsic width for a given height.
    fn get_max_intrinsic_width(&self, height: f32) -> f32 {
        self.compute_max_intrinsic_width(height)
    }

    /// Returns the minimum intrinsic height for a given width.
    fn get_min_intrinsic_height(&self, width: f32) -> f32 {
        self.compute_min_intrinsic_height(width)
    }

    /// Returns the maximum intrinsic height for a given width.
    fn get_max_intrinsic_height(&self, width: f32) -> f32 {
        self.compute_max_intrinsic_height(width)
    }

    /// Computes the minimum intrinsic width for a given height.
    fn compute_min_intrinsic_width(&self, _height: f32) -> f32 {
        0.0
    }

    /// Computes the maximum intrinsic width for a given height.
    fn compute_max_intrinsic_width(&self, _height: f32) -> f32 {
        0.0
    }

    /// Computes the minimum intrinsic height for a given width.
    fn compute_min_intrinsic_height(&self, _width: f32) -> f32 {
        0.0
    }

    /// Computes the maximum intrinsic height for a given width.
    fn compute_max_intrinsic_height(&self, _width: f32) -> f32 {
        0.0
    }

    // ========================================================================
    // Dry Layout
    // ========================================================================

    /// Returns the size this box would like to be given the constraints.
    fn get_dry_layout(&self, constraints: BoxConstraints) -> Size {
        self.compute_dry_layout(constraints)
    }

    /// Computes the size this box would have given the constraints,
    /// without actually laying out.
    fn compute_dry_layout(&self, _constraints: BoxConstraints) -> Size {
        Size::ZERO
    }

    // ========================================================================
    // Baseline
    // ========================================================================

    /// Returns the distance from the top of the box to the first baseline.
    fn get_distance_to_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        self.compute_distance_to_actual_baseline(baseline)
    }

    /// Returns the distance from the top of the box to its first baseline
    /// for the given constraints (dry layout).
    fn get_dry_baseline(&self, constraints: BoxConstraints, baseline: TextBaseline) -> Option<f32> {
        self.compute_dry_baseline(constraints, baseline)
    }

    /// Computes the distance from the top of the box to its first baseline.
    fn compute_distance_to_actual_baseline(&self, _baseline: TextBaseline) -> Option<f32> {
        None
    }

    /// Computes the dry baseline for the given constraints.
    fn compute_dry_baseline(
        &self,
        _constraints: BoxConstraints,
        _baseline: TextBaseline,
    ) -> Option<f32> {
        None
    }

    // ========================================================================
    // Paint Bounds
    // ========================================================================

    /// Returns the paint bounds of this render box.
    fn box_paint_bounds(&self) -> Rect {
        let size = self.size();
        Rect::new(
            flui_types::Pixels::ZERO,
            flui_types::Pixels::ZERO,
            size.width,
            size.height,
        )
    }

    // ========================================================================
    // Painting
    // ========================================================================

    /// Paints this render object and its children.
    ///
    /// The context provides:
    /// - Current offset via `ctx.offset()`
    /// - Canvas for drawing via `ctx.canvas()`
    /// - Child painting via `ctx.paint_child()` (arity-specific)
    ///
    /// # Example
    ///
    /// ```ignore
    /// fn paint(&self, ctx: &mut BoxPaintContext<'_, Single, BoxParentData>) {
    ///     // Draw background
    ///     let rect = Rect::from_size(self.size).translate(ctx.offset());
    ///     ctx.canvas().draw_rect(rect, &Paint::fill(self.color));
    ///
    ///     // Paint child
    ///     ctx.paint_child();
    /// }
    /// ```
    ///
    /// Default implementation paints children only (for containers that
    /// don't draw themselves).
    fn paint(&self, _ctx: &mut BoxPaintContext<'_, Self::Arity, Self::ParentData>) {
        // Default: no-op - pipeline handles child painting if not overridden
    }

    // ========================================================================
    // Effect Layers
    // ========================================================================
    //
    // paint_alpha / paint_transform are NOT defined on RenderBox itself.
    // They live on PaintEffectsCapability (a supertrait of
    // RenderObject<BoxProtocol>); concrete render objects implement that
    // capability trait directly. See Mythos Step 11.
}

/// Text baseline types for baseline alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextBaseline {
    /// The alphabetic baseline.
    Alphabetic,
    /// The ideographic baseline.
    Ideographic,
}

// ============================================================================
// Blanket Implementation of RenderObject<BoxProtocol> for RenderBox
// ============================================================================

/// Automatic implementation of `RenderObject<BoxProtocol>` for all RenderBox
/// types.
///
/// This blanket impl bridges the typed RenderBox API (with Arity/ParentData)
/// and the protocol-specific `RenderObject<P>` trait needed for storage.
///
/// # Architecture Note (D-block PR-A1b U19 / companion memo D5)
///
/// The `perform_layout_raw` body **is** the real bridge — it receives a
/// protocol-erased `&mut dyn BoxLayoutCtxErased`, reconstructs a typed
/// `BoxLayoutCtx<T::Arity, T::ParentData>` via the in-crate
/// `BoxLayoutCtx::from_erased` ctor (`pub(crate)` — see
/// [`crate::protocol::BoxLayoutCtx`]), wraps it in a `BoxLayoutContext`
/// (the rich ergonomic wrapper), and calls
/// [`RenderBox::perform_layout`]. The completion size is read back from
/// the inner context's geometry and returned to the pipeline.
///
/// Pre-U19 this method returned `*self.size()` as a no-op placeholder
/// (companion memo §D5: D-1 AE1 demonstrably returned `Size::ZERO` for
/// fresh boxes). The real bridge is now live.
///
/// `hit_test_raw` is still a placeholder — hit testing flows through
/// `RenderBox::hit_test()` with `BoxHitTestContext` and is wired
/// separately by the hit-test pipeline. The painting flow is also still
/// out-of-band: the pipeline constructs `BoxPaintContext` and calls
/// `RenderBox::paint()` directly.
///
/// Note: This requires T to also implement Diagnosticable since `RenderObject<P>`
/// requires it.
impl<T> RenderObject<BoxProtocol> for T
where
    T: RenderBox
        + flui_foundation::Diagnosticable
        + crate::traits::PaintEffectsCapability
        + crate::traits::SemanticsCapability
        + crate::traits::HotReloadCapability,
{
    fn perform_layout_raw(
        &mut self,
        ctx: &mut <BoxProtocol as crate::protocol::Protocol>::LayoutCtxErased<'_>,
    ) -> crate::error::RenderResult<crate::protocol::ProtocolGeometry<BoxProtocol>> {
        // D-block PR-A1b U19 / memo D5 — the real bridge.
        //
        // The pipeline / `RenderEntry::layout_leaf_only` hands us a
        // `&mut dyn BoxLayoutCtxErased` (the GAT for `BoxProtocol`
        // resolves to exactly this). We reconstruct a typed
        // `BoxLayoutCtx<T::Arity, T::ParentData>` via the `from_erased`
        // ctor (Proxy storage that delegates child / completion ops
        // back through the erased trait), wrap it in the ergonomic
        // `BoxLayoutContext` so user widgets get `complete_with_size`
        // etc., and call `T::perform_layout`.
        //
        // The user's `perform_layout` body must call
        // `ctx.complete_with_size(...)` (or equivalent) to record the
        // computed size; we read it back from the inner BoxLayoutCtx
        // and return to the caller.
        //
        // # Contract-violation handling (follow-up to PR #141 #5 Option A)
        //
        // If `perform_layout` returns without calling
        // `ctx.complete_with_size(...)`, we return
        // [`RenderError::ContractViolation`] as `Err(...)` directly —
        // no `panic_any`, no `catch_unwind` dance. The PR #141 review
        // (finding #5) called out the previous panic_any path as a
        // Constitution Principle 6 violation; this PR closes that
        // technical debt by making the signature
        // `RenderResult<ProtocolGeometry<P>>` so contract violations
        // propagate as typed errors through `?`.
        //
        // `catch_unwind` in `RenderEntry::layout_leaf_only` is retained
        // only for genuine third-party panics (panic! / unwrap in user
        // widget code) which still surface as `RenderError::Poisoned`.
        let typed_inner =
            crate::protocol::BoxLayoutCtx::<T::Arity, T::ParentData>::from_erased(ctx);
        let mut layout_ctx =
            crate::context::BoxLayoutContext::<T::Arity, T::ParentData>::new(typed_inner);
        T::perform_layout(self, &mut layout_ctx);
        layout_ctx.inner().geometry().copied().ok_or_else(|| {
            crate::error::RenderError::contract_violation(
                self.debug_name(),
                "RenderBox::perform_layout returned without calling \
                     ctx.complete_with_size(...)",
            )
        })
    }

    fn paint(&self, _context: &mut CanvasContext, _offset: Offset) {
        // Protocol bridge only - no-op.
        // Real painting flows through RenderBox::paint() with BoxPaintContext,
        // which provides children access and paint_child() callbacks.
        // The pipeline creates the proper context and calls RenderBox::paint()
        // directly.
    }

    fn hit_test_raw(
        &self,
        _result: &mut crate::protocol::ProtocolHitResult<BoxProtocol>,
        _position: crate::protocol::ProtocolPosition<BoxProtocol>,
    ) -> bool {
        // Protocol bridge only - returns false.
        // Real hit testing flows through RenderBox::hit_test() with BoxHitTestContext.
        false
    }

    fn geometry(&self) -> &crate::protocol::ProtocolGeometry<BoxProtocol> {
        self.size()
    }

    fn set_geometry(&mut self, geometry: crate::protocol::ProtocolGeometry<BoxProtocol>) {
        *self.size_mut() = geometry;
    }

    fn paint_bounds(&self) -> Rect {
        self.box_paint_bounds()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hit_test_behavior_default() {
        // HitTestBehavior is now imported from flui_interaction via hit_testing
        let behavior = HitTestBehavior::default();
        assert_eq!(behavior, HitTestBehavior::DeferToChild);
    }

    // BoxHitTestResult and BoxHitTestEntry tests are now in
    // hit_testing/result.rs
}
