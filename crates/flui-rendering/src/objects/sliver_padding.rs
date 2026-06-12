//! `RenderSliverPadding` — single-child sliver that pads its inner sliver on
//! all four sides (main- and cross-axis), honouring the sliver layout
//! protocol (scroll/paint/cache extents, scroll-offset correction passthrough,
//! viewport overlap reduction).
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's
//! [`RenderSliverPadding`](https://api.flutter.dev/flutter/rendering/RenderSliverPadding-class.html)
//! (`packages/flutter/lib/src/rendering/sliver_padding.dart`). The
//! geometry math (`mainAxisPaintPadding`, `paintExtent`,
//! `layoutExtent`, `hitTestExtent`, `cacheExtent` composition) is a
//! direct translation of `RenderSliverEdgeInsetsPadding.performLayout`.
//!
//! Scroll-offset correction (`SliverGeometry.scrollOffsetCorrection`)
//! returned by the child propagates through unchanged — the viewport
//! reruns the layout pass on the next frame with the corrected scroll
//! offset.
//!
//! # Rust-native improvements
//!
//! * Pure-function math helpers
//!   ([`RenderSliverPadding::padded_geometry`],
//!   [`RenderSliverPadding::empty_geometry`],
//!   [`RenderSliverPadding::child_constraints`]) are factored out of
//!   `perform_layout` so the geometry composition is directly
//!   unit-testable without standing up a full pipeline /
//!   `SliverLayoutContext`. The `perform_layout` body becomes a thin
//!   driver over those helpers + the context's `layout_child` /
//!   `child_parent_data_mut` calls.
//! * `set_padding` returns a `bool` change-flag for pipeline
//!   `mark_needs_layout` short-circuit.
//! * Sliver-protocol calculate_paint_offset / calculate_cache_offset are
//!   inlined as private associated functions (`paint_offset`,
//!   `cache_offset`) so helpers can be `&self`-free pure functions and
//!   remain test-friendly.

use flui_tree::Single;
use flui_types::{Axis, EdgeInsets, Offset, Pixels, Rect, geometry::px, layout::AxisDirection};

use crate::{
    constraints::{SliverConstraints, SliverGeometry},
    context::{SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverPhysicalParentData,
    traits::{HotReloadCapability, PaintEffectsCapability, RenderSliver, SemanticsCapability},
};

// ============================================================================
// RenderSliverPadding
// ============================================================================

/// A sliver render object that inserts padding on all four sides of its
/// single sliver child.
///
/// The padding is specified in cross/main-axis–independent
/// [`EdgeInsets`] (top/right/bottom/left). The `axis` of the current
/// [`SliverConstraints`] determines which two sides apply along the
/// main (scroll) axis and which two along the cross axis.
#[derive(Debug, Clone)]
pub struct RenderSliverPadding {
    /// Padding to inflate around the child sliver.
    padding: EdgeInsets,
    /// Last-applied constraints (required by the [`RenderSliver`]
    /// trait — the framework reads it back for child positioning /
    /// debug introspection).
    constraints: SliverConstraints,
    /// Computed geometry from the most recent [`Self::perform_layout`].
    geometry: SliverGeometry,
}

impl RenderSliverPadding {
    /// Creates a sliver-padding render object with the given insets.
    #[must_use]
    pub const fn new(padding: EdgeInsets) -> Self {
        Self {
            padding,
            constraints: empty_sliver_constraints(),
            geometry: SliverGeometry::ZERO,
        }
    }

    /// Creates a sliver-padding render object with all sides equal.
    #[must_use]
    pub fn all(value: f32) -> Self {
        Self::new(EdgeInsets::all(px(value)))
    }

    /// Creates a sliver-padding render object with symmetric horizontal /
    /// vertical insets.
    ///
    /// Order matches Flutter's `EdgeInsets.symmetric(horizontal:,
    /// vertical:)`. Internally we forward to
    /// [`EdgeInsets::symmetric`] whose signature is
    /// `(vertical, horizontal)`.
    #[must_use]
    pub fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self::new(EdgeInsets::symmetric(px(vertical), px(horizontal)))
    }

    /// Returns the current padding.
    #[inline]
    pub fn padding(&self) -> EdgeInsets {
        self.padding
    }

    /// Updates the padding; returns `true` iff the value changed.
    pub fn set_padding(&mut self, padding: EdgeInsets) -> bool {
        if self.padding == padding {
            return false;
        }
        self.padding = padding;
        true
    }

    // ════════════════════════════════════════════════════════════════════════
    // Math helpers (pure — unit-testable in isolation)
    // ════════════════════════════════════════════════════════════════════════

    /// Returns `(before, after, main_total, cross_total)` padding in
    /// constraint-axis–oriented form.
    ///
    /// * `before` — main-axis padding nearest the zero scroll offset after
    ///   applying [`GrowthDirection`](crate::constraints::GrowthDirection)
    ///   to the axis direction.
    /// * `after` — the opposite main-axis padding.
    /// * `main_total` — `before + after`.
    /// * `cross_total` — total padding on the cross axis.
    #[inline]
    fn resolve(&self, constraints: &SliverConstraints) -> (f32, f32, f32, f32) {
        let main = match constraints.axis() {
            Axis::Vertical => self.padding.vertical_total().get(),
            Axis::Horizontal => self.padding.horizontal_total().get(),
        };
        let cross = match constraints.axis() {
            Axis::Vertical => self.padding.horizontal_total().get(),
            Axis::Horizontal => self.padding.vertical_total().get(),
        };
        let (before, after) = match constraints
            .growth_direction
            .apply_to_axis_direction(constraints.axis_direction)
        {
            AxisDirection::TopToBottom => (self.padding.top.get(), self.padding.bottom.get()),
            AxisDirection::BottomToTop => (self.padding.bottom.get(), self.padding.top.get()),
            AxisDirection::LeftToRight => (self.padding.left.get(), self.padding.right.get()),
            AxisDirection::RightToLeft => (self.padding.right.get(), self.padding.left.get()),
        };
        (before, after, main, cross)
    }

    /// Sliver `calculatePaintOffset` (Flutter source-of-truth) inlined as
    /// a pure function so the math helpers below stay independent of
    /// `self`. Mirrors the trait default in
    /// [`RenderSliver::calculate_paint_offset`].
    #[inline]
    fn paint_offset(constraints: &SliverConstraints, from: f32, to: f32) -> f32 {
        debug_assert!(
            from <= to,
            "paint_offset: from ({from}) must be <= to ({to})"
        );
        let a = constraints.scroll_offset;
        let b = constraints.scroll_offset + constraints.remaining_paint_extent;
        (to.min(b) - from.max(a)).max(0.0)
    }

    /// Sliver `calculateCacheOffset` inlined as a pure function. Mirrors
    /// the trait default in [`RenderSliver::calculate_cache_offset`].
    #[inline]
    fn cache_offset(constraints: &SliverConstraints, from: f32, to: f32) -> f32 {
        debug_assert!(
            from <= to,
            "cache_offset: from ({from}) must be <= to ({to})"
        );
        let a = constraints.scroll_offset + constraints.cache_origin;
        let b = constraints.scroll_offset + constraints.remaining_cache_extent;
        (to.min(b) - from.max(a))
            .max(0.0)
            .min(constraints.remaining_cache_extent)
    }

    /// Computes the sliver child's constraints given the parent
    /// constraints and the padding insets.
    ///
    /// Flutter's `SliverPadding` passes the child a copy of the parent
    /// constraints with:
    /// - `scroll_offset` reduced by the leading padding (clamped to 0),
    /// - `cache_origin` extended by the leading padding (clamped to 0
    ///   on the high side — `cache_origin` is always <= 0),
    /// - positive `overlap` reduced by the leading paint padding,
    /// - `remaining_paint_extent` reduced by the leading paint padding,
    /// - `remaining_cache_extent` reduced by the leading cache padding,
    /// - `cross_axis_extent` reduced by the total cross padding
    ///   (clamped to 0),
    /// - `preceding_scroll_extent` extended by the leading padding.
    pub fn child_constraints(&self, parent: &SliverConstraints) -> SliverConstraints {
        let (before, _after, _main, cross) = self.resolve(parent);
        let before_pad_paint = Self::paint_offset(parent, 0.0, before);
        let before_pad_cache = Self::cache_offset(parent, 0.0, before);

        let mut cc = *parent;
        cc.scroll_offset = (parent.scroll_offset - before).max(0.0);
        cc.cache_origin = (parent.cache_origin + before).min(0.0);
        cc.overlap = if parent.overlap > 0.0 {
            (parent.overlap - before_pad_paint).max(0.0)
        } else {
            parent.overlap
        };
        cc.remaining_paint_extent = parent.remaining_paint_extent - before_pad_paint;
        cc.remaining_cache_extent = parent.remaining_cache_extent - before_pad_cache;
        cc.cross_axis_extent = (parent.cross_axis_extent - cross).max(0.0);
        cc.preceding_scroll_extent = parent.preceding_scroll_extent + before;
        cc
    }

    /// Computes the empty-child geometry — used when this sliver has no
    /// child sliver. The padded region itself still consumes scroll
    /// extent, paints (up to the remaining paint budget), and caches.
    pub fn empty_geometry(&self, parent: &SliverConstraints) -> SliverGeometry {
        let (_before, _after, main, _cross) = self.resolve(parent);
        let paint_extent = Self::paint_offset(parent, 0.0, main).min(parent.remaining_paint_extent);
        let cache_extent = Self::cache_offset(parent, 0.0, main);

        SliverGeometry {
            scroll_extent: main,
            paint_extent,
            paint_origin: 0.0,
            layout_extent: paint_extent,
            max_paint_extent: main,
            max_scroll_obstruction_extent: 0.0,
            cross_axis_extent: None,
            hit_test_extent: paint_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: false,
            scroll_offset_correction: None,
            cache_extent,
        }
    }

    /// Composes the parent's final geometry given the child sliver's
    /// geometry, and computes the child's paint offset within the
    /// padded box.
    ///
    /// Direct port of the `RenderSliverEdgeInsetsPadding.performLayout`
    /// composition step. Returns `(final_geometry, child_paint_offset)`.
    pub fn padded_geometry(
        &self,
        parent: &SliverConstraints,
        child_geometry: &SliverGeometry,
    ) -> (SliverGeometry, Offset) {
        let axis = parent.axis();
        let (before, _after, main, _cross) = self.resolve(parent);

        let before_pad_paint = Self::paint_offset(parent, 0.0, before);
        let before_pad_cache = Self::cache_offset(parent, 0.0, before);
        let after_pad_paint = Self::paint_offset(
            parent,
            before + child_geometry.scroll_extent,
            main + child_geometry.scroll_extent,
        );
        let after_pad_cache = Self::cache_offset(
            parent,
            before + child_geometry.scroll_extent,
            main + child_geometry.scroll_extent,
        );
        let main_pad_paint = before_pad_paint + after_pad_paint;

        let paint_extent = (before_pad_paint
            + child_geometry
                .paint_extent
                .max(child_geometry.layout_extent + after_pad_paint))
        .min(parent.remaining_paint_extent);
        let layout_extent = (main_pad_paint + child_geometry.layout_extent).min(paint_extent);
        let cache_extent = (before_pad_cache + after_pad_cache + child_geometry.cache_extent)
            .min(parent.remaining_cache_extent);
        let hit_test_extent = (main_pad_paint + child_geometry.paint_extent)
            .max(before_pad_paint + child_geometry.hit_test_extent);

        let geometry = SliverGeometry {
            paint_origin: child_geometry.paint_origin,
            scroll_extent: main + child_geometry.scroll_extent,
            paint_extent,
            layout_extent,
            max_paint_extent: main + child_geometry.max_paint_extent,
            cache_extent,
            hit_test_extent,
            has_visual_overflow: child_geometry.has_visual_overflow,
            visible: paint_extent > 0.0,
            max_scroll_obstruction_extent: 0.0,
            cross_axis_extent: None,
            scroll_offset_correction: None,
        };

        let effective_axis_direction = parent
            .growth_direction
            .apply_to_axis_direction(parent.axis_direction);
        let calculated_offset = match effective_axis_direction {
            AxisDirection::BottomToTop => Self::paint_offset(
                parent,
                self.padding.bottom.get() + child_geometry.scroll_extent,
                self.padding.vertical_total().get() + child_geometry.scroll_extent,
            ),
            AxisDirection::RightToLeft => Self::paint_offset(
                parent,
                self.padding.right.get() + child_geometry.scroll_extent,
                self.padding.horizontal_total().get() + child_geometry.scroll_extent,
            ),
            AxisDirection::LeftToRight => Self::paint_offset(parent, 0.0, self.padding.left.get()),
            AxisDirection::TopToBottom => Self::paint_offset(parent, 0.0, self.padding.top.get()),
        };

        let cross_before = match axis {
            Axis::Horizontal => self.padding.top.get(),
            Axis::Vertical => self.padding.left.get(),
        };
        let paint_offset = match axis {
            Axis::Horizontal => {
                Offset::new(Pixels::new(calculated_offset), Pixels::new(cross_before))
            }
            Axis::Vertical => {
                Offset::new(Pixels::new(cross_before), Pixels::new(calculated_offset))
            }
        };

        (geometry, paint_offset)
    }
}

impl Default for RenderSliverPadding {
    /// Defaults to zero padding — equivalent to a transparent passthrough.
    fn default() -> Self {
        Self::new(EdgeInsets::ZERO)
    }
}

impl flui_foundation::Diagnosticable for RenderSliverPadding {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_enum("padding", self.padding);
    }
}

impl RenderSliver for RenderSliverPadding {
    type Arity = Single;
    type ParentData = SliverPhysicalParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Single, SliverPhysicalParentData>,
    ) {
        let constraints = *ctx.constraints();
        self.constraints = constraints;

        // No-child fast path — sliver still consumes its own padded
        // scroll extent so subsequent slivers compose correctly.
        if ctx.child_count() == 0 {
            let geometry = self.empty_geometry(&constraints);
            self.geometry = geometry;
            ctx.complete(geometry);
            return;
        }

        let child_constraints = self.child_constraints(&constraints);
        let child_geometry = ctx.layout_child(0, child_constraints);

        // Scroll-offset correction propagates upward unchanged — the
        // viewport reruns layout next frame with the corrected offset.
        if let Some(correction) = child_geometry.scroll_offset_correction {
            let geometry = SliverGeometry::scroll_offset_correction(correction);
            self.geometry = geometry;
            ctx.complete(geometry);
            return;
        }

        let (geometry, child_paint_offset) = self.padded_geometry(&constraints, &child_geometry);

        // Set the child's paint offset within the padded box. The
        // layout walk commits this into the child's RenderState so
        // later paint and hit-test phases use the same placement.
        ctx.position_child(0, child_paint_offset);
        self.geometry = geometry;
        ctx.complete(geometry);
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

    fn child_main_axis_position(
        &self,
        _child: &dyn crate::traits::RenderObject<crate::protocol::SliverProtocol>,
    ) -> f32 {
        let (before, _, _, _) = self.resolve(&self.constraints);
        Self::paint_offset(&self.constraints, 0.0, before)
    }

    fn child_cross_axis_position(
        &self,
        _child: &dyn crate::traits::RenderObject<crate::protocol::SliverProtocol>,
    ) -> f32 {
        // TODO(Wave 3.3+): resolve cross-axis start from `TextDirection` /
        // `cross_axis_direction` when FLUI lands RTL sliver layout. Flutter's
        // `RenderSliverPadding.childCrossAxisPosition` uses text direction;
        // this LTR assumption matches today's cross-axis posture.
        match self.constraints.axis() {
            Axis::Vertical => self.padding.left.get(),
            Axis::Horizontal => self.padding.top.get(),
        }
    }

    fn child_scroll_offset(
        &self,
        _child: &dyn crate::traits::RenderObject<crate::protocol::SliverProtocol>,
    ) -> Option<f32> {
        let (before, _, _, _) = self.resolve(&self.constraints);
        Some(before)
    }

    fn hit_test(
        &self,
        ctx: &mut SliverHitTestContext<'_, Single, SliverPhysicalParentData>,
    ) -> bool {
        if self.geometry.hit_test_extent <= 0.0 {
            return false;
        }
        ctx.hit_test_child_at_layout_offset(0)
    }

    fn sliver_paint_bounds(&self) -> Rect {
        let size = self.get_absolute_size(self.geometry.paint_extent);
        Rect::from_origin_size(flui_types::Point::ZERO, size)
    }
}

// Mythos Step 11: explicit (default) capability opt-outs.
impl PaintEffectsCapability for RenderSliverPadding {}
impl SemanticsCapability for RenderSliverPadding {}
impl HotReloadCapability for RenderSliverPadding {}

// ============================================================================
// Helpers
// ============================================================================

/// `SliverConstraints` constant used to initialise the cached
/// constraints field; `SliverConstraints::default()` is not `const`.
const fn empty_sliver_constraints() -> SliverConstraints {
    use flui_types::layout::AxisDirection;

    use crate::{constraints::GrowthDirection, view::ScrollDirection};

    SliverConstraints::new(
        AxisDirection::TopToBottom,
        GrowthDirection::Forward,
        ScrollDirection::Idle,
        0.0, // scroll_offset
        0.0, // preceding_scroll_extent
        0.0, // overlap
        0.0, // remaining_paint_extent
        0.0, // cross_axis_extent
        AxisDirection::LeftToRight,
        0.0, // viewport_main_axis_extent
        0.0, // remaining_cache_extent
        0.0, // cache_origin
    )
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;
    use crate::constraints::GrowthDirection;

    // ────────────────────────────────────────────────────────────────────────
    // Test helpers
    // ────────────────────────────────────────────────────────────────────────

    /// Builds a vertical-axis sliver constraint with sensible defaults
    /// and the provided scroll/paint extents — keeps each test focused
    /// on the fields it cares about.
    fn vertical_constraints(
        scroll_offset: f32,
        remaining_paint_extent: f32,
        remaining_cache_extent: f32,
        cross_axis_extent: f32,
    ) -> SliverConstraints {
        use flui_types::layout::AxisDirection;

        use crate::view::ScrollDirection;

        SliverConstraints::new(
            AxisDirection::TopToBottom,
            GrowthDirection::Forward,
            ScrollDirection::Idle,
            scroll_offset,
            0.0, // preceding_scroll_extent
            0.0, // overlap
            remaining_paint_extent,
            cross_axis_extent,
            AxisDirection::LeftToRight,
            remaining_paint_extent, // viewport_main_axis_extent
            remaining_cache_extent,
            0.0, // cache_origin
        )
    }

    /// Attaches constraints to a padding node for trait-helper unit tests.
    fn padding_with_constraints(
        padding: RenderSliverPadding,
        constraints: SliverConstraints,
    ) -> RenderSliverPadding {
        RenderSliverPadding {
            constraints,
            ..padding
        }
    }

    /// Builds a child sliver geometry with explicit fields for the
    /// composition test.
    fn child_geom(
        scroll_extent: f32,
        paint_extent: f32,
        layout_extent: f32,
        cache_extent: f32,
    ) -> SliverGeometry {
        SliverGeometry {
            scroll_extent,
            paint_extent,
            paint_origin: 0.0,
            layout_extent,
            max_paint_extent: paint_extent,
            max_scroll_obstruction_extent: 0.0,
            cross_axis_extent: None,
            hit_test_extent: paint_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: false,
            scroll_offset_correction: None,
            cache_extent,
        }
    }

    // ────────────────────────────────────────────────────────────────────────
    // Construction + accessors
    // ────────────────────────────────────────────────────────────────────────

    #[test]
    fn all_constructor_sets_uniform_padding() {
        let p = RenderSliverPadding::all(8.0);
        assert_eq!(p.padding(), EdgeInsets::all(px(8.0)));
        assert_eq!(p.padding().horizontal_total(), px(16.0));
        assert_eq!(p.padding().vertical_total(), px(16.0));
    }

    #[test]
    fn symmetric_constructor_matches_flutter_argument_order() {
        // symmetric(horizontal: 20, vertical: 10) → top+bottom = 20,
        // left+right = 40 (matches EdgeInsets::symmetric(vertical=10,
        // horizontal=20)).
        let p = RenderSliverPadding::symmetric(20.0, 10.0);
        assert_eq!(p.padding().horizontal_total(), px(40.0));
        assert_eq!(p.padding().vertical_total(), px(20.0));
    }

    #[test]
    fn default_is_zero_padding() {
        let p = RenderSliverPadding::default();
        assert_eq!(p.padding(), EdgeInsets::all(px(0.0)));
    }

    #[test]
    fn set_padding_returns_change_flag() {
        let mut p = RenderSliverPadding::all(4.0);
        assert!(!p.set_padding(EdgeInsets::all(px(4.0)))); // no-op
        assert!(p.set_padding(EdgeInsets::all(px(5.0))));
        assert_eq!(p.padding(), EdgeInsets::all(px(5.0)));
    }

    // ────────────────────────────────────────────────────────────────────────
    // Math helpers
    // ────────────────────────────────────────────────────────────────────────

    #[test]
    fn paint_offset_clamps_to_remaining() {
        let c = vertical_constraints(0.0, 50.0, 100.0, 300.0);
        // [0..30] entirely inside [0..50] → 30.
        assert_eq!(RenderSliverPadding::paint_offset(&c, 0.0, 30.0), 30.0);
        // [40..80] clamped to [40..50] → 10.
        assert_eq!(RenderSliverPadding::paint_offset(&c, 40.0, 80.0), 10.0);
        // [60..80] entirely past 50 → 0.
        assert_eq!(RenderSliverPadding::paint_offset(&c, 60.0, 80.0), 0.0);
    }

    #[test]
    fn cache_offset_uses_cache_origin_window() {
        let c = vertical_constraints(0.0, 50.0, 200.0, 300.0);
        // [0..150] entirely inside cache window [0..200] → 150.
        assert_eq!(RenderSliverPadding::cache_offset(&c, 0.0, 150.0), 150.0);
    }

    #[test]
    fn cache_offset_window_starts_at_scroll_offset_plus_cache_origin() {
        let mut c = vertical_constraints(50.0, 50.0, 100.0, 300.0);
        c.cache_origin = -20.0;

        // Flutter's cache window is [scroll_offset + cache_origin,
        // scroll_offset + remaining_cache_extent] = [30, 150]. The range
        // [0, 40] overlaps only [30, 40] → 10.
        assert_eq!(RenderSliverPadding::cache_offset(&c, 0.0, 40.0), 10.0);
    }

    #[test]
    fn resolve_picks_per_axis_padding_correctly() {
        let p = RenderSliverPadding::new(EdgeInsets {
            top: px(10.0),
            right: px(5.0),
            bottom: px(20.0),
            left: px(3.0),
        });

        // Vertical down scroll: main = top+bottom = 30, cross = left+right = 8.
        let constraints = vertical_constraints(0.0, 200.0, 200.0, 300.0);
        let (before_v, after_v, main_v, cross_v) = p.resolve(&constraints);
        assert_eq!(before_v, 10.0);
        assert_eq!(after_v, 20.0);
        assert_eq!(main_v, 30.0);
        assert_eq!(cross_v, 8.0);

        // Horizontal right scroll: main = left+right = 8, cross = top+bottom = 30.
        use flui_types::layout::AxisDirection;

        use crate::view::ScrollDirection;

        let constraints = SliverConstraints::new(
            AxisDirection::LeftToRight,
            GrowthDirection::Forward,
            ScrollDirection::Idle,
            0.0,
            0.0,
            0.0,
            200.0,
            300.0,
            AxisDirection::TopToBottom,
            200.0,
            200.0,
            0.0,
        );
        let (before_h, after_h, main_h, cross_h) = p.resolve(&constraints);
        assert_eq!(before_h, 3.0);
        assert_eq!(after_h, 5.0);
        assert_eq!(main_h, 8.0);
        assert_eq!(cross_h, 30.0);
    }

    #[test]
    fn empty_geometry_uses_padding_as_scroll_extent() {
        let p = RenderSliverPadding::symmetric(0.0, 30.0); // main=60 vertical
        let c = vertical_constraints(0.0, 200.0, 200.0, 300.0);
        let g = p.empty_geometry(&c);
        assert_eq!(g.scroll_extent, 60.0);
        assert_eq!(g.paint_extent, 60.0);
        assert_eq!(g.max_paint_extent, 60.0);
        assert_eq!(g.layout_extent, 60.0);
        assert_eq!(g.cache_extent, 60.0);
        assert!(g.visible);
    }

    #[test]
    fn child_constraints_deflate_cross_axis_and_extend_preceding() {
        let p = RenderSliverPadding::new(EdgeInsets {
            top: px(10.0),
            right: px(5.0),
            bottom: px(20.0),
            left: px(3.0),
        });
        let parent = vertical_constraints(0.0, 200.0, 200.0, 300.0);
        let cc = p.child_constraints(&parent);

        // Vertical scroll: cross axis is horizontal → deflate by left+right = 8.
        assert_eq!(cc.cross_axis_extent, 300.0 - 8.0);
        // Overlap reset.
        assert_eq!(cc.overlap, 0.0);
        // Preceding scroll extent extended by leading (top) padding.
        assert_eq!(cc.preceding_scroll_extent, 10.0);
        // Remaining paint reduced by leading paint padding (full 10 because scroll_offset=0).
        assert_eq!(cc.remaining_paint_extent, 200.0 - 10.0);
    }

    #[test]
    fn child_constraints_reduce_positive_overlap_by_before_paint_padding() {
        let p = RenderSliverPadding::new(EdgeInsets {
            top: px(10.0),
            right: px(0.0),
            bottom: px(0.0),
            left: px(0.0),
        });
        let mut parent = vertical_constraints(0.0, 200.0, 200.0, 300.0);
        parent.overlap = 30.0;

        let cc = p.child_constraints(&parent);

        assert_eq!(
            cc.overlap, 20.0,
            "positive overlap is reduced by beforePaddingPaintExtent, not reset",
        );
    }

    #[test]
    fn child_constraints_use_effective_growth_direction_for_before_padding() {
        let p = RenderSliverPadding::new(EdgeInsets {
            top: px(10.0),
            right: px(0.0),
            bottom: px(20.0),
            left: px(0.0),
        });
        let mut parent = vertical_constraints(15.0, 200.0, 200.0, 300.0);
        parent.growth_direction = GrowthDirection::Reverse;

        let cc = p.child_constraints(&parent);

        assert_eq!(
            cc.scroll_offset, 0.0,
            "reverse vertical growth uses bottom padding as beforePadding",
        );
        assert_eq!(cc.preceding_scroll_extent, 20.0);
        assert_eq!(
            cc.remaining_paint_extent, 195.0,
            "only the visible 5px tail of beforePadding is removed at scroll_offset=15",
        );
        assert_eq!(cc.remaining_cache_extent, 195.0);
    }

    // ────────────────────────────────────────────────────────────────────────
    // Critical: Flutter-formula geometry composition
    // ────────────────────────────────────────────────────────────────────────

    /// Numerical regression test against the Flutter
    /// `RenderSliverEdgeInsetsPadding.performLayout` math.
    ///
    /// Setup (vertical scroll):
    /// - Padding: top=10, bottom=20 → before=10, after=20, main=30.
    /// - Constraints: scroll_offset=0, remaining_paint_extent=200,
    ///   remaining_cache_extent=200, cross_axis_extent=300.
    /// - Child geometry: scroll_extent=100, paint_extent=80,
    ///   layout_extent=80, cache_extent=80, paint_origin=0.
    ///
    /// Expected:
    /// - before_pad_paint = paint_offset(0,10)   = 10
    /// - after_pad_paint  = paint_offset(110,130)= 20
    /// - main_pad_paint   = 30
    /// - paint_extent     = (10 + max(80, 80+20)).min(200) = (10+100).min(200) = 110
    /// - layout_extent    = (30 + 80).min(110) = 110
    /// - cache_extent     = (10 + max(80, 80+20)).min(200) = 110
    /// - scroll_extent    = 100 + 30 = 130
    /// - hit_test_extent  = (30 + 80).max(10+80) = 110
    #[test]
    fn padded_geometry_matches_flutter_formula() {
        let p = RenderSliverPadding::new(EdgeInsets {
            top: px(10.0),
            right: px(0.0),
            bottom: px(20.0),
            left: px(0.0),
        });
        let parent = vertical_constraints(0.0, 200.0, 200.0, 300.0);
        let child = child_geom(100.0, 80.0, 80.0, 80.0);

        let (geom, paint_offset) = p.padded_geometry(&parent, &child);

        assert_eq!(
            geom.scroll_extent, 130.0,
            "scroll_extent: child + main padding"
        );
        assert_eq!(geom.paint_extent, 110.0, "paint_extent: Flutter formula");
        assert_eq!(geom.layout_extent, 110.0, "layout_extent");
        assert_eq!(geom.cache_extent, 110.0, "cache_extent");
        // child.max_paint_extent in our helper = child.paint_extent = 80; final
        // max_paint_extent = main_padding + child.max_paint_extent = 30 + 80 = 110.
        assert_eq!(
            geom.max_paint_extent, 110.0,
            "max_paint = child.max_paint_extent + main padding",
        );
        assert_eq!(geom.hit_test_extent, 110.0, "hit_test_extent");
        assert!(geom.visible);

        // Vertical axis: paint_offset.x = cross_before (left = 0),
        // paint_offset.y = before_pad_paint (10).
        assert_eq!(paint_offset.dx, px(0.0));
        assert_eq!(paint_offset.dy, px(10.0));
    }

    #[test]
    fn padded_geometry_hit_test_extent_pairs_main_padding_with_paint_extent() {
        let p = RenderSliverPadding::new(EdgeInsets {
            top: px(10.0),
            right: px(0.0),
            bottom: px(20.0),
            left: px(0.0),
        });
        let parent = vertical_constraints(0.0, 200.0, 200.0, 300.0);
        let mut child = child_geom(100.0, 40.0, 40.0, 40.0);
        child.hit_test_extent = 100.0;

        let (geom, _paint_offset) = p.padded_geometry(&parent, &child);

        assert_eq!(
            geom.hit_test_extent, 110.0,
            "Flutter formula: max(main_pad_paint + child.paint_extent, \
            before_pad_paint + child.hit_test_extent)",
        );
    }

    #[test]
    fn padded_geometry_cache_extent_adds_padding_cache_to_child_cache_extent() {
        let p = RenderSliverPadding::new(EdgeInsets {
            top: px(10.0),
            right: px(0.0),
            bottom: px(20.0),
            left: px(0.0),
        });
        let parent = vertical_constraints(0.0, 200.0, 200.0, 300.0);
        let child = child_geom(100.0, 80.0, 80.0, 5.0);

        let (geom, _paint_offset) = p.padded_geometry(&parent, &child);

        assert_eq!(
            geom.cache_extent, 35.0,
            "Flutter formula: before+after cache padding (30) plus child cache extent (5)",
        );
    }

    #[test]
    fn padded_geometry_horizontal_axis_cross_uses_top() {
        use flui_types::layout::AxisDirection;

        use crate::view::ScrollDirection;

        // Horizontal scroll → cross axis is vertical → cross-before = top.
        let p = RenderSliverPadding::new(EdgeInsets {
            top: px(7.0),
            right: px(20.0),
            bottom: px(0.0),
            left: px(10.0),
        });
        let parent = SliverConstraints::new(
            AxisDirection::LeftToRight,
            GrowthDirection::Forward,
            ScrollDirection::Idle,
            0.0,
            0.0,
            0.0,
            200.0,
            300.0,
            AxisDirection::TopToBottom,
            200.0,
            200.0,
            0.0,
        );
        let child = child_geom(100.0, 80.0, 80.0, 80.0);

        let (_geom, paint_offset) = p.padded_geometry(&parent, &child);

        // Horizontal axis: paint_offset.x = before_pad_paint (left = 10),
        // paint_offset.y = cross_before (top = 7).
        assert_eq!(paint_offset.dx, px(10.0));
        assert_eq!(paint_offset.dy, px(7.0));
    }

    #[test]
    fn padded_geometry_with_partially_scrolled_leading_padding() {
        let p = RenderSliverPadding::new(EdgeInsets {
            top: px(10.0),
            right: px(0.0),
            bottom: px(20.0),
            left: px(0.0),
        });
        let parent = vertical_constraints(5.0, 200.0, 200.0, 300.0);
        let child = child_geom(100.0, 80.0, 80.0, 80.0);

        let (geom, paint_offset) = p.padded_geometry(&parent, &child);

        assert_eq!(geom.paint_extent, 105.0);
        assert_eq!(geom.layout_extent, 105.0);
        assert_eq!(paint_offset.dy, px(5.0));
        assert_eq!(paint_offset.dx, px(0.0));
    }

    #[test]
    fn padded_geometry_reverse_growth_child_paint_offset() {
        let p = RenderSliverPadding::new(EdgeInsets {
            top: px(10.0),
            right: px(0.0),
            bottom: px(20.0),
            left: px(0.0),
        });
        let mut parent = vertical_constraints(0.0, 200.0, 200.0, 300.0);
        parent.growth_direction = GrowthDirection::Reverse;
        let child = child_geom(100.0, 80.0, 80.0, 80.0);

        let (_geom, paint_offset) = p.padded_geometry(&parent, &child);

        // Reverse vertical growth uses bottom padding as leading; child is
        // positioned from the trailing end of the padded scroll extent.
        assert_eq!(paint_offset.dy, px(10.0));
    }

    #[test]
    fn empty_geometry_with_scrolled_leading_padding() {
        let p = RenderSliverPadding::symmetric(0.0, 15.0);
        let parent = vertical_constraints(5.0, 200.0, 200.0, 300.0);
        let geom = p.empty_geometry(&parent);

        assert_eq!(geom.scroll_extent, 30.0);
        assert_eq!(geom.paint_extent, 25.0);
        assert_eq!(geom.hit_test_extent, 25.0);
    }

    #[test]
    fn child_constraints_negative_overlap_passthrough() {
        let p = RenderSliverPadding::new(EdgeInsets {
            top: px(10.0),
            right: px(0.0),
            bottom: px(0.0),
            left: px(0.0),
        });
        let mut parent = vertical_constraints(0.0, 200.0, 200.0, 300.0);
        parent.overlap = -12.0;

        let cc = p.child_constraints(&parent);

        assert_eq!(cc.overlap, -12.0);
    }

    #[test]
    fn child_position_helpers_use_leading_padding() {
        use crate::protocol::SliverProtocol;
        use crate::test_support::NoopSliver;
        use crate::traits::{RenderObject, RenderSliver};

        let padding = RenderSliverPadding::new(EdgeInsets {
            top: px(10.0),
            right: px(0.0),
            bottom: px(20.0),
            left: px(3.0),
        });
        let constraints = vertical_constraints(5.0, 200.0, 200.0, 300.0);
        let p = padding_with_constraints(padding, constraints);
        let child = NoopSliver;
        let child_ref: &dyn RenderObject<SliverProtocol> = &child;

        assert_eq!(
            p.child_main_axis_position(child_ref),
            5.0,
            "forward growth: child main-axis position matches visible leading padding",
        );
        assert_eq!(
            p.child_scroll_offset(child_ref),
            Some(10.0),
            "forward growth: child scroll offset uses top leading padding",
        );
        assert_eq!(
            p.child_cross_axis_position(child_ref),
            3.0,
            "cross-axis position uses left inset under LTR assumptions",
        );
    }

    #[test]
    fn child_position_helpers_use_reverse_growth_leading_padding() {
        use crate::protocol::SliverProtocol;
        use crate::test_support::NoopSliver;
        use crate::traits::{RenderObject, RenderSliver};

        let padding = RenderSliverPadding::new(EdgeInsets {
            top: px(10.0),
            right: px(0.0),
            bottom: px(20.0),
            left: px(3.0),
        });
        let mut constraints = vertical_constraints(5.0, 200.0, 200.0, 300.0);
        constraints.growth_direction = GrowthDirection::Reverse;
        let p = padding_with_constraints(padding, constraints);
        let child = NoopSliver;
        let child_ref: &dyn RenderObject<SliverProtocol> = &child;

        assert_eq!(
            p.child_main_axis_position(child_ref),
            15.0,
            "reverse growth: child main-axis position uses bottom leading padding",
        );
        assert_eq!(
            p.child_scroll_offset(child_ref),
            Some(20.0),
            "reverse growth: child scroll offset uses bottom leading padding",
        );
        assert_eq!(
            p.child_cross_axis_position(child_ref),
            3.0,
            "cross-axis position is unchanged by growth direction",
        );
    }

    // ────────────────────────────────────────────────────────────────────────
    // Diagnostics
    // ────────────────────────────────────────────────────────────────────────

    #[test]
    fn debug_fill_properties_lists_padding_and_geometry() {
        use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
        let p = RenderSliverPadding::all(8.0);
        let mut builder = DiagnosticsBuilder::new();
        p.debug_fill_properties(&mut builder);
        let names: Vec<String> = builder
            .build()
            .iter()
            .map(|p| p.name().to_string())
            .collect();
        assert!(
            names.iter().any(|n| n == "padding"),
            "missing diagnostic field: padding"
        );
    }
}
