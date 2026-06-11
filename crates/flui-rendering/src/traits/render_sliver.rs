//! RenderSliver trait for scrollable content layout.

use flui_tree::Arity;
use flui_types::{Pixels, Rect, Size, geometry::px, prelude::AxisDirection};

use crate::{
    constraints::{SliverConstraints, SliverGeometry},
    context::{SliverHitTestContext, SliverLayoutContext},
    parent_data::ParentData,
    protocol::SliverProtocol,
    traits::RenderObject,
};

// ============================================================================
// RenderSliver Trait
// ============================================================================

/// Trait for render objects that provide scrollable content.
///
/// RenderSliver is the layout protocol for scrollable content. Slivers:
/// - Receive [`SliverConstraints`] with scroll position and viewport info
/// - Compute what portion is visible and space consumed
/// - Return [`SliverGeometry`] with scroll/paint extents
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `RenderSliver` abstract class in
/// `rendering/sliver.dart`.
///
/// # Layout Protocol
///
/// 1. Parent (viewport) calls `perform_layout()` with context
/// 2. Sliver determines visible portion based on scroll offset
/// 3. Sliver completes layout via `ctx.complete(geometry)`
/// 4. Viewport composes geometries to build scrollable view
///
/// # Key Concepts
///
/// - **Scroll Extent**: Total scrollable size of the sliver
/// - **Paint Extent**: How much the sliver paints in the viewport
/// - **Layout Extent**: How much the sliver consumes in the viewport
/// - **Cache Extent**: Extra area to keep rendered for smooth scrolling
///
/// # Example
///
/// ```ignore
/// impl RenderSliver for MySliverList {
///     type Arity = Variable;
///     type ParentData = SliverMultiBoxAdaptorParentData;
///
///     fn perform_layout(&mut self, ctx: &mut SliverLayoutContext<Variable, Self::ParentData>) {
///         let scroll_offset = ctx.constraints().scroll_offset;
///         // ... compute visible items ...
///         ctx.complete(SliverGeometry { ... });
///     }
/// }
/// ```
///
/// Implementations are automatically bridged to `RenderObject<SliverProtocol>`
/// via blanket impl.
pub trait RenderSliver: flui_foundation::Diagnosticable + Send + Sync + 'static {
    /// The arity of this render sliver (Leaf, Optional, Variable, etc.)
    type Arity: Arity;

    /// The parent data type for children of this render sliver.
    type ParentData: ParentData + Default;

    // ========================================================================
    // Layout
    // ========================================================================

    /// Computes the layout of this sliver.
    ///
    /// The context provides:
    /// - Constraints via `ctx.constraints()`
    /// - Child layout via `ctx.layout_child()`
    /// - Completion via `ctx.complete(geometry)`
    fn perform_layout(&mut self, ctx: &mut SliverLayoutContext<'_, Self::Arity, Self::ParentData>);

    /// Returns the current geometry of this sliver.
    ///
    /// Only valid after `perform_layout` has been called.
    fn geometry(&self) -> &SliverGeometry;

    /// Returns the constraints this sliver was laid out with.
    ///
    /// Only valid after `perform_layout` has been called.
    fn constraints(&self) -> &SliverConstraints;

    /// Sets the geometry for this sliver.
    ///
    /// Called during `perform_layout` to report the computed geometry.
    fn set_geometry(&mut self, geometry: SliverGeometry);

    // ========================================================================
    // Positioning
    // ========================================================================

    /// Returns the scroll offset adjustment for center slivers.
    ///
    /// This is used by viewports with a center sliver to adjust the
    /// scroll offset to account for slivers that grow in both directions.
    /// Only the center sliver and slivers before it should return a non-zero
    /// value.
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to `RenderSliver.centerOffsetAdjustment` in Flutter.
    fn center_offset_adjustment(&self) -> f32 {
        0.0
    }

    /// Computes the portion of this sliver that is visible in the viewport.
    ///
    /// Given a `from` and `to` range in the sliver's coordinate space,
    /// this returns the offset at which the visible portion begins.
    ///
    /// # Arguments
    ///
    /// * `from` - Start of the range in sliver coordinates
    /// * `to` - End of the range in sliver coordinates
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to `RenderSliver.calculatePaintOffset` in Flutter.
    fn calculate_paint_offset(&self, constraints: &SliverConstraints, from: f32, to: f32) -> f32 {
        debug_assert!(from <= to);
        let remaining_painted_extent = constraints.remaining_paint_extent;
        let scroll_offset = constraints.scroll_offset;

        let a = scroll_offset;
        let b = scroll_offset + remaining_painted_extent;

        (to.min(b) - from.max(a)).max(0.0)
    }

    /// Computes the portion of this sliver that is in the cache area.
    ///
    /// Similar to `calculate_paint_offset` but includes the cache extent.
    ///
    /// # Arguments
    ///
    /// * `from` - Start of the range in sliver coordinates
    /// * `to` - End of the range in sliver coordinates
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to `RenderSliver.calculateCacheOffset` in Flutter.
    fn calculate_cache_offset(&self, constraints: &SliverConstraints, from: f32, to: f32) -> f32 {
        debug_assert!(from <= to);
        let remaining_cache_extent = constraints.remaining_cache_extent;
        let cache_origin = constraints.cache_origin;

        let a = cache_origin;
        let b = cache_origin + remaining_cache_extent;

        (to.min(b) - from.max(a)).max(0.0)
    }

    /// Returns the position of a child along the main axis.
    ///
    /// # Arguments
    ///
    /// * `child` - The child to query
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to `RenderSliver.childMainAxisPosition` in Flutter.
    fn child_main_axis_position(&self, child: &dyn RenderObject<SliverProtocol>) -> f32 {
        let _ = child;
        0.0
    }

    /// Returns the position of a child along the cross axis.
    ///
    /// # Arguments
    ///
    /// * `child` - The child to query
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to `RenderSliver.childCrossAxisPosition` in Flutter.
    fn child_cross_axis_position(&self, child: &dyn RenderObject<SliverProtocol>) -> f32 {
        let _ = child;
        0.0
    }

    /// Returns the scroll offset of a child.
    ///
    /// Returns the scroll offset needed to bring the leading edge
    /// of the given child into view.
    ///
    /// # Arguments
    ///
    /// * `child` - The child to query
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to `RenderSliver.childScrollOffset` in Flutter.
    fn child_scroll_offset(&self, child: &dyn RenderObject<SliverProtocol>) -> Option<f32> {
        let _ = child;
        None
    }

    // ========================================================================
    // Size Helpers
    // ========================================================================

    /// Returns the absolute size in the main and cross axis.
    ///
    /// Given a paint extent and cross axis extent, returns the
    /// absolute size as (width, height) based on the axis direction.
    ///
    /// # Arguments
    ///
    /// * `paint_extent` - The extent along the main axis
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to `RenderSliver.getAbsoluteSize` in Flutter.
    fn get_absolute_size(&self, paint_extent: f32) -> Size {
        let constraints = self.constraints();
        let cross_axis_extent = constraints.cross_axis_extent;

        match constraints.axis_direction {
            AxisDirection::TopToBottom | AxisDirection::BottomToTop => {
                Size::new(px(cross_axis_extent), px(paint_extent))
            }
            AxisDirection::LeftToRight | AxisDirection::RightToLeft => {
                Size::new(px(paint_extent), px(cross_axis_extent))
            }
        }
    }

    /// Returns the absolute size relative to the origin.
    ///
    /// Like `get_absolute_size`, but takes into account the growth
    /// direction and axis direction to position relative to origin.
    ///
    /// # Arguments
    ///
    /// * `paint_extent` - The extent along the main axis
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to `RenderSliver.getAbsoluteSizeRelativeToOrigin` in
    /// Flutter.
    fn get_absolute_size_relative_to_origin(&self, paint_extent: f32) -> Size {
        // By default, same as get_absolute_size
        // Override for slivers that need special handling
        self.get_absolute_size(paint_extent)
    }

    // ========================================================================
    // Painting
    // ========================================================================

    /// Records this sliver's paint fragment.
    ///
    /// Same sans-IO fragment model as
    /// [`RenderBox::paint`](crate::traits::RenderBox::paint): the
    /// canvas is pre-translated to the sliver's origin (draw in local
    /// coordinates) and children are spliced via the arity-gated
    /// `paint_child` surface. Visibility culling stays the sliver's
    /// job — splice only the visible child range.
    ///
    /// The default implementation splices all children in tree order.
    fn paint(&self, ctx: &mut crate::context::PaintCx<'_, Self::Arity>) {
        ctx.paint_children_in_order();
    }

    // ========================================================================
    // Hit Testing
    // ========================================================================

    /// Hit tests this sliver.
    ///
    /// The context provides:
    /// - Position via `ctx.main_axis()`, `ctx.cross_axis()`
    /// - Child testing via `ctx.hit_test_child()`
    /// - Result management via `ctx.add_self(id)`
    fn hit_test(&self, ctx: &mut SliverHitTestContext<'_, Self::Arity, Self::ParentData>) -> bool;

    /// Hit tests just this sliver (not children).
    fn hit_test_self(&self, _main: f32, _cross: f32) -> bool {
        false
    }

    // ========================================================================
    // Paint Bounds
    // ========================================================================

    /// Returns the paint bounds of this render sliver.
    fn sliver_paint_bounds(&self) -> Rect {
        let geometry = RenderSliver::geometry(self);
        let size = self.get_absolute_size(geometry.paint_extent);
        Rect::new(Pixels::ZERO, Pixels::ZERO, size.width, size.height)
    }

    // ========================================================================
    // Parent Data
    // ========================================================================

    /// Creates default parent data for a child.
    fn create_default_parent_data() -> Self::ParentData {
        Self::ParentData::default()
    }
}

// ============================================================================
// Blanket Implementation of RenderObject<SliverProtocol> for RenderSliver
// ============================================================================

/// Automatic implementation of `RenderObject<SliverProtocol>` for all
/// RenderSliver types.
///
/// This blanket impl bridges the typed RenderSliver API (with Arity/ParentData)
/// and the protocol-specific `RenderObject<P>` trait needed for storage.
///
/// # Architecture Note
///
/// The `perform_layout_raw` and `hit_test_raw` methods are **protocol bridges
/// only**. See the RenderBox blanket impl documentation for detailed
/// explanation.
impl<T> RenderObject<SliverProtocol> for T
where
    T: RenderSliver
        + flui_foundation::Diagnosticable
        + crate::traits::PaintEffectsCapability
        + crate::traits::SemanticsCapability
        + crate::traits::HotReloadCapability,
{
    fn perform_layout_raw(
        &mut self,
        ctx: &mut <SliverProtocol as crate::protocol::Protocol>::LayoutCtxErased<'_>,
    ) -> crate::error::RenderResult<crate::protocol::ProtocolGeometry<SliverProtocol>> {
        // Core.2 W3 — live bridge, mirroring the Box analog in
        // `render_box.rs`.
        //
        // The pipeline hands us `&mut dyn SliverLayoutCtxErased`
        // (the GAT `SliverProtocol::LayoutCtxErased<'_>` resolves to
        // exactly this). We reconstruct a typed
        // `SliverLayoutCtx<T::Arity, T::ParentData>` via `from_erased`
        // (Proxy storage — caches constraints, writes geometry through
        // to the erased ctx on completion), wrap it in the ergonomic
        // `SliverLayoutContext` so the user's `perform_layout` body
        // receives `ctx.constraints()`, `ctx.complete(geometry)`, etc.,
        // and call `T::perform_layout`.
        //
        // The user's `perform_layout` body must call `ctx.complete(…)`
        // to record the computed geometry; we read it back via
        // `layout_ctx.inner().geometry()` and return to the caller.
        //
        // If `perform_layout` returns without calling `ctx.complete(…)`
        // we return `Err(RenderError::ContractViolation)` — typed
        // propagation, no panic.
        //
        // Child layout operations delegate through `SliverLayoutCtxErased`,
        // so non-leaf slivers such as `RenderSliverPadding` can synchronously
        // lay out their sliver children during the pipeline walk.
        let typed_inner =
            crate::protocol::SliverLayoutCtx::<T::Arity, T::ParentData>::from_erased(ctx);
        let mut layout_ctx =
            crate::context::SliverLayoutContext::<T::Arity, T::ParentData>::new(typed_inner);
        T::perform_layout(self, &mut layout_ctx);
        layout_ctx.inner().geometry().copied().ok_or_else(|| {
            crate::error::RenderError::contract_violation(
                self.debug_name(),
                "RenderSliver::perform_layout returned without calling \
                 ctx.complete(...)",
            )
        })
    }

    fn paint_raw(&self, recorder: &mut crate::context::FragmentRecorder, child_count: usize) {
        // Same paint bridge shape as the BoxProtocol blanket: wrap the
        // recorder in the typed PaintCx<T::Arity> and call the user's
        // RenderSliver::paint.
        let mut cx = crate::context::PaintCx::<T::Arity>::new(recorder, child_count);
        T::paint(self, &mut cx);
    }

    fn hit_test_raw(
        &self,
        _position: crate::protocol::ProtocolPosition<SliverProtocol>,
        _child_count: usize,
        _hit_child: &mut (
                 dyn FnMut(usize, Option<crate::protocol::ProtocolPosition<SliverProtocol>>) -> bool
                     + Send
                     + Sync
             ),
    ) -> bool {
        // Sliver hit testing lands with the sliver layout walk
        // (Core.2); until then a sliver subtree reports a miss rather
        // than a false hit.
        false
    }

    fn geometry(&self) -> &crate::protocol::ProtocolGeometry<SliverProtocol> {
        RenderSliver::geometry(self)
    }

    fn set_geometry(&mut self, geometry: crate::protocol::ProtocolGeometry<SliverProtocol>) {
        self.set_geometry(geometry);
    }

    fn paint_bounds(&self) -> Rect {
        self.sliver_paint_bounds()
    }
}

// ============================================================================
// Proxy Sliver
// ============================================================================

/// Trait for slivers with a single sliver child.
///
/// Generic over the child type `C` which must implement `RenderSliver`.
pub trait RenderProxySliver<C: RenderSliver>: RenderSliver {
    /// Returns the child sliver, if any.
    fn child(&self) -> Option<&C>;

    /// Returns the child sliver mutably, if any.
    fn child_mut(&mut self) -> Option<&mut C>;

    /// Sets the child sliver.
    fn set_child(&mut self, child: Option<C>);
}

// ============================================================================
// Tests — leaf bridge (Core.2 W3.1)
// ============================================================================

#[cfg(test)]
mod tests {
    use flui_tree::{Leaf, Single};
    use flui_types::layout::AxisDirection;

    use super::*;
    use crate::{
        LayoutContextApi,
        constraints::{GrowthDirection, SliverConstraints, SliverGeometry},
        context::SliverHitTestContext,
        protocol::{Protocol, SliverProtocol},
        traits::{HotReloadCapability, PaintEffectsCapability, SemanticsCapability},
        view::ScrollDirection,
    };

    // ────────────────────────────────────────────────────────────────────────
    // Test helpers
    // ────────────────────────────────────────────────────────────────────────

    /// Minimal vertical-scroll constraints focused on scroll/paint extents.
    fn vertical_constraints(scroll_offset: f32, remaining_paint_extent: f32) -> SliverConstraints {
        SliverConstraints::new(
            AxisDirection::TopToBottom,
            GrowthDirection::Forward,
            ScrollDirection::Idle,
            scroll_offset,
            0.0, // preceding_scroll_extent
            0.0, // overlap
            remaining_paint_extent,
            400.0, // cross_axis_extent
            AxisDirection::LeftToRight,
            remaining_paint_extent, // viewport_main_axis_extent
            remaining_paint_extent, // remaining_cache_extent
            0.0,                    // cache_origin
        )
    }

    // ────────────────────────────────────────────────────────────────────────
    // Test double — completing leaf
    //
    // Models a simple fixed-height list item: paint_extent =
    // min(item_height − scroll_offset, remaining_paint_extent).
    // ────────────────────────────────────────────────────────────────────────

    struct FixedHeightSliver {
        item_height: f32,
        constraints: SliverConstraints,
        geometry: SliverGeometry,
    }

    impl std::fmt::Debug for FixedHeightSliver {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("FixedHeightSliver")
                .field("item_height", &self.item_height)
                .finish_non_exhaustive()
        }
    }

    impl FixedHeightSliver {
        fn new(item_height: f32) -> Self {
            Self {
                item_height,
                constraints: vertical_constraints(0.0, 0.0),
                geometry: SliverGeometry::ZERO,
            }
        }
    }

    impl flui_foundation::Diagnosticable for FixedHeightSliver {
        fn debug_fill_properties(&self, _properties: &mut flui_foundation::DiagnosticsBuilder) {}
    }
    impl PaintEffectsCapability for FixedHeightSliver {}
    impl SemanticsCapability for FixedHeightSliver {}
    impl HotReloadCapability for FixedHeightSliver {}

    impl RenderSliver for FixedHeightSliver {
        type Arity = Leaf;
        type ParentData = crate::parent_data::SliverParentData;

        fn perform_layout(&mut self, ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>) {
            let c = *ctx.constraints();
            self.constraints = c;

            let visible_height = (self.item_height - c.scroll_offset).max(0.0);
            let paint_extent = visible_height.min(c.remaining_paint_extent);
            let geom = SliverGeometry::new(
                self.item_height, // scroll_extent — full item height
                paint_extent,
                0.0, // paint_origin
            );
            self.geometry = geom;
            ctx.complete(geom);
        }

        fn geometry(&self) -> &SliverGeometry {
            &self.geometry
        }

        fn constraints(&self) -> &SliverConstraints {
            &self.constraints
        }

        fn set_geometry(&mut self, geometry: SliverGeometry) {
            self.geometry = geometry;
        }

        fn hit_test(&self, _ctx: &mut SliverHitTestContext<'_, Leaf, Self::ParentData>) -> bool {
            false
        }
    }

    // ────────────────────────────────────────────────────────────────────────
    // Test double — non-completing leaf (contract-violation probe)
    // ────────────────────────────────────────────────────────────────────────

    struct NonCompletingSliver {
        constraints: SliverConstraints,
        geometry: SliverGeometry,
    }

    impl std::fmt::Debug for NonCompletingSliver {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("NonCompletingSliver")
                .finish_non_exhaustive()
        }
    }

    impl NonCompletingSliver {
        fn new() -> Self {
            Self {
                constraints: vertical_constraints(0.0, 0.0),
                geometry: SliverGeometry::ZERO,
            }
        }
    }

    impl flui_foundation::Diagnosticable for NonCompletingSliver {
        fn debug_fill_properties(&self, _properties: &mut flui_foundation::DiagnosticsBuilder) {}
    }
    impl PaintEffectsCapability for NonCompletingSliver {}
    impl SemanticsCapability for NonCompletingSliver {}
    impl HotReloadCapability for NonCompletingSliver {}

    impl RenderSliver for NonCompletingSliver {
        type Arity = Leaf;
        type ParentData = crate::parent_data::SliverParentData;

        fn perform_layout(&mut self, _ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>) {
            // Intentionally does NOT call ctx.complete(…) — exercises the
            // contract-violation path in `perform_layout_raw`.
        }

        fn geometry(&self) -> &SliverGeometry {
            &self.geometry
        }

        fn constraints(&self) -> &SliverConstraints {
            &self.constraints
        }

        fn set_geometry(&mut self, geometry: SliverGeometry) {
            self.geometry = geometry;
        }

        fn hit_test(&self, _ctx: &mut SliverHitTestContext<'_, Leaf, Self::ParentData>) -> bool {
            false
        }
    }

    // ────────────────────────────────────────────────────────────────────────
    // Tests
    // ────────────────────────────────────────────────────────────────────────

    /// Drives `perform_layout_raw` via the real erased path and asserts the
    /// returned `SliverGeometry` matches what `FixedHeightSliver` produced.
    ///
    /// Item fully in view: scroll_offset=0, remaining_paint_extent=600,
    /// item_height=200 → paint_extent=200, scroll_extent=200.
    #[test]
    fn sliver_leaf_bridge_completing_fully_visible() {
        let constraints = vertical_constraints(0.0, 600.0);
        let mut sliver = FixedHeightSliver::new(200.0);

        let result = SliverProtocol::with_leaf_erased_ctx(constraints, |erased| {
            use crate::protocol::RenderObject;
            sliver.perform_layout_raw(erased)
        });

        let geom = result.expect("bridge must succeed when perform_layout completes");
        assert_eq!(geom.scroll_extent, 200.0, "scroll_extent = item_height");
        assert_eq!(geom.paint_extent, 200.0, "paint_extent = min(200, 600)");
    }

    /// Item partially scrolled: scroll_offset=50, remaining_paint_extent=600,
    /// item_height=200 → visible=150, paint_extent=150.
    #[test]
    fn sliver_leaf_bridge_completing_partially_scrolled() {
        let constraints = vertical_constraints(50.0, 600.0);
        let mut sliver = FixedHeightSliver::new(200.0);

        let result = SliverProtocol::with_leaf_erased_ctx(constraints, |erased| {
            use crate::protocol::RenderObject;
            sliver.perform_layout_raw(erased)
        });

        let geom = result.expect("bridge must succeed when perform_layout completes");
        assert_eq!(geom.scroll_extent, 200.0, "scroll_extent = item_height");
        // Relative check: paint_extent should be item_height - scroll_offset
        assert!(
            (geom.paint_extent - 150.0).abs() < 1e-4,
            "paint_extent ≈ 150.0, got {}",
            geom.paint_extent
        );
    }

    /// Viewport smaller than item: remaining_paint_extent=80, item_height=200
    /// → paint_extent clamped to 80.
    #[test]
    fn sliver_leaf_bridge_completing_clamped_to_viewport() {
        let constraints = vertical_constraints(0.0, 80.0);
        let mut sliver = FixedHeightSliver::new(200.0);

        let result = SliverProtocol::with_leaf_erased_ctx(constraints, |erased| {
            use crate::protocol::RenderObject;
            sliver.perform_layout_raw(erased)
        });

        let geom = result.expect("bridge must succeed when perform_layout completes");
        assert_eq!(geom.scroll_extent, 200.0);
        assert!(
            (geom.paint_extent - 80.0).abs() < 1e-4,
            "paint_extent clamped to remaining_paint_extent=80, got {}",
            geom.paint_extent
        );
    }

    /// A `perform_layout` that never calls `ctx.complete(…)` must cause
    /// `perform_layout_raw` to return `Err(ContractViolation)`.
    #[test]
    fn sliver_leaf_bridge_non_completing_yields_contract_violation() {
        let constraints = vertical_constraints(0.0, 600.0);
        let mut sliver = NonCompletingSliver::new();

        let result = SliverProtocol::with_leaf_erased_ctx(constraints, |erased| {
            use crate::protocol::RenderObject;
            sliver.perform_layout_raw(erased)
        });

        assert!(
            matches!(
                result,
                Err(crate::error::RenderError::ContractViolation { .. })
            ),
            "expected ContractViolation, got {:?}",
            result
        );
    }

    /// Regression guard for the Direct-storage path: `SliverLayoutCtx::new`
    /// must still work after the storage refactor.
    #[test]
    fn sliver_layout_ctx_direct_path_smoke() {
        use crate::protocol::sliver_protocol::SliverLayoutCtx;

        let c = vertical_constraints(0.0, 300.0);
        let mut ctx = SliverLayoutCtx::<Leaf, crate::parent_data::SliverParentData>::new(c);

        assert!(!ctx.is_complete());
        assert_eq!(ctx.remaining_paint_extent(), 300.0);

        ctx.complete_layout(SliverGeometry::new(100.0, 100.0, 0.0));
        assert!(ctx.is_complete());
        let geom = ctx.geometry().copied().unwrap();
        assert_eq!(geom.scroll_extent, 100.0);
        assert_eq!(geom.paint_extent, 100.0);
    }

    // ────────────────────────────────────────────────────────────────────────
    // Test double — non-leaf (Single) arity: completes geometry through
    // the same erased bridge as leaf slivers.
    // ────────────────────────────────────────────────────────────────────────

    struct SingleAritySliver {
        constraints: SliverConstraints,
        geometry: SliverGeometry,
    }

    impl std::fmt::Debug for SingleAritySliver {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("SingleAritySliver").finish_non_exhaustive()
        }
    }

    impl flui_foundation::Diagnosticable for SingleAritySliver {
        fn debug_fill_properties(&self, _properties: &mut flui_foundation::DiagnosticsBuilder) {}
    }
    impl PaintEffectsCapability for SingleAritySliver {}
    impl SemanticsCapability for SingleAritySliver {}
    impl HotReloadCapability for SingleAritySliver {}

    impl RenderSliver for SingleAritySliver {
        type Arity = Single;
        type ParentData = crate::parent_data::SliverParentData;

        fn perform_layout(&mut self, ctx: &mut SliverLayoutContext<'_, Single, Self::ParentData>) {
            // A well-behaved body that DOES complete — proving the bridge
            // rejects on arity *before* running it, not because of a missing
            // completion call.
            let c = *ctx.constraints();
            self.constraints = c;
            let geom = SliverGeometry::new(c.remaining_paint_extent, c.remaining_paint_extent, 0.0);
            self.geometry = geom;
            ctx.complete(geom);
        }

        fn geometry(&self) -> &SliverGeometry {
            &self.geometry
        }

        fn constraints(&self) -> &SliverConstraints {
            &self.constraints
        }

        fn set_geometry(&mut self, geometry: SliverGeometry) {
            self.geometry = geometry;
        }

        fn hit_test(&self, _ctx: &mut SliverHitTestContext<'_, Single, Self::ParentData>) -> bool {
            false
        }
    }

    /// A non-`Leaf` sliver that completes layout must now pass through the
    /// bridge; child-aware slivers use the same path and call `layout_child`.
    #[test]
    fn sliver_non_leaf_bridge_completing_succeeds() {
        let constraints = vertical_constraints(0.0, 600.0);
        let mut sliver = SingleAritySliver {
            constraints,
            geometry: SliverGeometry::ZERO,
        };

        let result = SliverProtocol::with_leaf_erased_ctx(constraints, |erased| {
            use crate::protocol::RenderObject;
            sliver.perform_layout_raw(erased)
        });

        let geom = result.expect("non-leaf sliver bridge must accept completed layout");
        assert_eq!(geom.scroll_extent, 600.0);
        assert_eq!(geom.paint_extent, 600.0);
    }
}
