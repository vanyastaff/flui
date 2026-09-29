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
//! * `set_padding` returns the exact pipeline impact.
//!   `mark_needs_layout` short-circuit.
//! * Sliver-protocol calculate_paint_offset / calculate_cache_offset are
//!   inlined as private associated functions (`paint_offset`,
//!   `cache_offset`) so helpers can be `&self`-free pure functions and
//!   remain test-friendly.

use flui_foundation::Single;
use flui_foundation::geometry::Axis;
use flui_foundation::geometry::{EdgeInsets, Offset};
use flui_rendering::constraints::AxisDirection;

use flui_rendering::{
    constraints::{SliverConstraints, SliverGeometry},
    context::{SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverPhysicalParentData,
    traits::RenderSliver,
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
}

impl RenderSliverPadding {
    /// Creates a sliver-padding render object with the given insets.
    ///
    /// # Panics
    ///
    /// Debug builds panic if any inset is negative: the padded geometry adds
    /// these insets to the child's scroll extent and offsets the child by the
    /// leading one, so a negative inset would place the child ahead of its own
    /// paint origin and shrink the sliver below the extent it actually
    /// occupies. Mirrors the Dart `assert(padding.isNonNegative)`, and like it
    /// is stripped from a release build.
    ///
    /// The message carries no inset values because a `const fn` may only panic
    /// with a literal — formatting is not available in a const context.
    #[must_use]
    pub const fn new(padding: EdgeInsets) -> Self {
        debug_assert!(
            padding.is_non_negative(),
            "RenderSliverPadding insets must be non-negative"
        );
        Self { padding }
    }

    /// Creates a sliver-padding render object with all sides equal.
    #[must_use]
    pub fn all(value: f64) -> Self {
        Self::new(EdgeInsets::all(value))
    }

    /// Creates a sliver-padding render object with symmetric horizontal /
    /// vertical insets.
    ///
    /// Order matches Flutter's `EdgeInsets.symmetric(horizontal:,
    /// vertical:)`. Internally we forward to
    /// [`EdgeInsets::symmetric`] whose signature is
    /// `(vertical, horizontal)`.
    #[must_use]
    pub fn symmetric(horizontal: f64, vertical: f64) -> Self {
        Self::new(EdgeInsets::symmetric(vertical, horizontal))
    }

    /// Returns the current padding.
    #[inline]
    pub fn padding(&self) -> EdgeInsets {
        self.padding
    }

    /// Updates the padding and returns the exact pipeline impact.
    ///
    /// # Panics
    ///
    /// Debug builds panic if any *incoming* inset is negative, for the reason
    /// given on [`RenderSliverPadding::new`].
    ///
    /// **Documented divergence.** Dart's `RenderSliverPadding` setter asserts
    /// `padding.isNonNegative` — the getter, i.e. the value already stored —
    /// so upstream the check inspects the outgoing value and a negative one
    /// assigned to a previously-valid object passes unnoticed. Its box
    /// counterpart in `shifted_box.dart` asserts `value.isNonNegative`, which
    /// is the check both were meant to be. FLUI asserts the incoming value in
    /// both: the invariant being protected is a property of what gets stored,
    /// and a guard that can only fire on a value it is too late to reject is
    /// not one worth porting.
    pub fn set_padding(&mut self, padding: EdgeInsets) -> flui_rendering::RenderUpdateImpact {
        debug_assert!(
            padding.is_non_negative(),
            "RenderSliverPadding insets must be non-negative, got {padding:?}"
        );
        if self.padding == padding {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.padding = padding;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    // ════════════════════════════════════════════════════════════════════════
    // Math helpers (pure — unit-testable in isolation)
    // ════════════════════════════════════════════════════════════════════════

    /// Returns `(before, after, main_total, cross_total)` padding in
    /// constraint-axis–oriented form.
    ///
    /// * `before` — main-axis padding nearest the zero scroll offset after
    ///   applying [`GrowthDirection`](flui_rendering::constraints::GrowthDirection)
    ///   to the axis direction.
    /// * `after` — the opposite main-axis padding.
    /// * `main_total` — `before + after`.
    /// * `cross_total` — total padding on the cross axis.
    #[inline]
    fn resolve(&self, constraints: &SliverConstraints) -> (f64, f64, f64, f64) {
        let main = match constraints.axis() {
            Axis::Vertical => self.padding.vertical_total(),
            Axis::Horizontal => self.padding.horizontal_total(),
        };
        let cross = match constraints.axis() {
            Axis::Vertical => self.padding.horizontal_total(),
            Axis::Horizontal => self.padding.vertical_total(),
        };
        let (before, after) = match constraints
            .growth_direction
            .apply_to_axis_direction(constraints.axis_direction)
        {
            AxisDirection::TopToBottom => (self.padding.top, self.padding.bottom),
            AxisDirection::BottomToTop => (self.padding.bottom, self.padding.top),
            AxisDirection::LeftToRight => (self.padding.left, self.padding.right),
            AxisDirection::RightToLeft => (self.padding.right, self.padding.left),
        };
        (before, after, main, cross)
    }

    /// Sliver `calculatePaintOffset` (Flutter source-of-truth) inlined as
    /// a pure function so the math helpers below stay independent of
    /// `self`. Mirrors the trait default in
    /// [`RenderSliver::calculate_paint_offset`].
    #[inline]
    fn paint_offset(constraints: &SliverConstraints, from: f64, to: f64) -> f64 {
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
    fn cache_offset(constraints: &SliverConstraints, from: f64, to: f64) -> f64 {
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
                self.padding.bottom + child_geometry.scroll_extent,
                self.padding.vertical_total() + child_geometry.scroll_extent,
            ),
            AxisDirection::RightToLeft => Self::paint_offset(
                parent,
                self.padding.right + child_geometry.scroll_extent,
                self.padding.horizontal_total() + child_geometry.scroll_extent,
            ),
            AxisDirection::LeftToRight => Self::paint_offset(parent, 0.0, self.padding.left),
            AxisDirection::TopToBottom => Self::paint_offset(parent, 0.0, self.padding.top),
        };

        let cross_before = match axis {
            Axis::Horizontal => self.padding.top,
            Axis::Vertical => self.padding.left,
        };
        let paint_offset = match axis {
            Axis::Horizontal => Offset::new(calculated_offset, cross_before),
            Axis::Vertical => Offset::new(cross_before, calculated_offset),
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
    ) -> SliverGeometry {
        let constraints = *ctx.constraints();

        // No-child fast path — sliver still consumes its own padded
        // scroll extent so subsequent slivers compose correctly.
        if ctx.child_count() == 0 {
            return self.empty_geometry(&constraints);
        }

        let child_constraints = self.child_constraints(&constraints);
        let child_geometry = ctx.layout_child(0, child_constraints);

        // Scroll-offset correction propagates upward unchanged — the
        // viewport reruns layout next frame with the corrected offset.
        if let Some(correction) = child_geometry.scroll_offset_correction {
            return SliverGeometry::scroll_offset_correction(correction);
        }

        let (geometry, child_paint_offset) = self.padded_geometry(&constraints, &child_geometry);

        // Set the child's paint offset within the padded box. The
        // layout walk commits this into the child's RenderState so
        // later paint and hit-test phases use the same placement.
        ctx.position_child(0, child_paint_offset);
        geometry
    }

    fn child_main_axis_position(
        &self,
        constraints: &SliverConstraints,
        _child: &dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::SliverProtocol>,
    ) -> f64 {
        let (before, _, _, _) = self.resolve(constraints);
        Self::paint_offset(constraints, 0.0, before)
    }

    fn child_cross_axis_position(
        &self,
        constraints: &SliverConstraints,
        _child: &dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::SliverProtocol>,
    ) -> f64 {
        // This MATCHES the reference; there is no LTR assumption to remove.
        //
        // An earlier TODO here claimed `RenderSliverPadding.childCrossAxisPosition`
        // resolves the cross-axis start from `TextDirection`. It does not:
        // `rendering/sliver_padding.dart` returns `resolvedPadding.top` for a
        // horizontal axis and `resolvedPadding.left` for a vertical one -- the
        // same two expressions as below. The reference's direction handling
        // lives entirely in `_resolvedPadding = padding.resolve(textDirection)`,
        // which converts an `EdgeInsetsDirectional` (start/end) into an
        // `EdgeInsets` (left/right).
        //
        // FLUI has no directional inset type at all -- `EdgeInsets` is
        // `EdgeInsets` with `top`/`right`/`bottom`/`left`, already resolved
        // -- so there is nothing to resolve and nothing to flip. Adding a
        // direction branch here would INTRODUCE a divergence, not remove one.
        // If a directional inset type ever lands, the resolution belongs at its
        // construction site, not in this method.
        match constraints.axis() {
            Axis::Vertical => self.padding.left,
            Axis::Horizontal => self.padding.top,
        }
    }

    fn child_scroll_offset(
        &self,
        constraints: &SliverConstraints,
        _child: &dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::SliverProtocol>,
    ) -> Option<f64> {
        let (before, _, _, _) = self.resolve(constraints);
        Some(before)
    }

    fn hit_test(
        &self,
        ctx: &mut SliverHitTestContext<'_, Single, SliverPhysicalParentData>,
    ) -> bool {
        ctx.hit_test_child_at_layout_offset(0)
    }
}

// ============================================================================
// Tests
// ============================================================================
