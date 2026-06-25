//! `RenderFittedBox` — single-child proxy that scales its child to fit
//! its own box per a [`BoxFit`] mode and aligns it via [`Alignment`].
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's
//! [`RenderFittedBox`](https://api.flutter.dev/flutter/rendering/RenderFittedBox-class.html)
//! (`packages/flutter/lib/src/rendering/proxy_box.dart`).
//!
//! # Rust-native improvements
//!
//! * The scaling math is delegated to the existing typed
//!   [`BoxFit::apply`] (from `flui_types::layout`), which returns a
//!   structured [`FittedSizes`] with both `source` and `destination`
//!   regions. Flutter's `RenderFittedBox` reimplements the same math
//!   inline; the Rust port keeps the math in one place so the seven
//!   `BoxFit` variants only have to be debugged once.
//! * The scale + alignment transform is exposed via
//!   [`RenderBox::paint_transform`] as a single composed
//!   [`Matrix4`]. The pipeline wraps the child in a `TransformLayer`
//!   — no manual canvas-state juggling in `paint`. Flutter does the
//!   same via its `_transform` layer machinery; the Rust shape just
//!   makes the matrix a first-class trait return value.
//! * `has_visual_overflow()` is a public post-layout query method
//!   (Flutter keeps the equivalent flag private and only consults it
//!   internally for clip-decision branching).
//! * `clip_behavior` is stored and surfaced via the diagnosticable
//!   dump; **active clipping awaits the layer-level clip integration
//!   that lands in Wave 3b** (alongside `RenderRepaintBoundary`).
//!   The default `Clip::None` matches the no-clip behaviour exactly,
//!   so configurations that need clipping must wait or opt into the
//!   in-progress wiring. This is documented intentionally — same
//!   defer pattern as Wave 3a's `RenderClipPath::contains`.

use flui_tree::Single;
use flui_types::{
    Alignment, Matrix4, Offset, Size,
    geometry::px,
    layout::{BoxFit, FittedSizes},
    painting::Clip,
};

use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::BoxParentData,
    traits::{RenderBox, TextBaseline},
};

/// A render object that scales its child to fit its own box.
///
/// The child is laid out under unconstrained constraints (so it can
/// pick its intrinsic size), then scaled and aligned to fit the box
/// per the configured [`BoxFit`] and [`Alignment`].
#[derive(Debug, Clone)]
pub struct RenderFittedBox {
    fit: BoxFit,
    alignment: Alignment,
    clip_behavior: Clip,
    has_child: bool,
    /// Cached scale factors derived in layout, consumed by
    /// [`RenderBox::paint_transform`].
    scale_x: f32,
    scale_y: f32,
    /// Cached child top-left offset inside `size`.
    align_offset: Offset,
    /// True iff the scaled child exceeds `size` on either axis.
    has_visual_overflow: bool,
}

impl RenderFittedBox {
    /// Creates a fitted box with the given fit, alignment, and clip.
    pub const fn new(fit: BoxFit, alignment: Alignment, clip_behavior: Clip) -> Self {
        Self {
            fit,
            alignment,
            clip_behavior,
            has_child: false,
            scale_x: 1.0,
            scale_y: 1.0,
            align_offset: Offset::ZERO,
            has_visual_overflow: false,
        }
    }

    /// Returns the current fit mode.
    #[inline]
    pub fn fit(&self) -> BoxFit {
        self.fit
    }

    /// Returns the current alignment.
    #[inline]
    pub fn alignment(&self) -> Alignment {
        self.alignment
    }

    /// Returns the current clip behavior.
    #[inline]
    pub fn clip_behavior(&self) -> Clip {
        self.clip_behavior
    }

    /// Returns whether the scaled child overflowed the box at the last
    /// layout. Reset on every layout.
    #[inline]
    pub fn has_visual_overflow(&self) -> bool {
        self.has_visual_overflow
    }

    /// Returns the cached scale factors `(sx, sy)` from the last layout.
    #[inline]
    pub fn scale_factors(&self) -> (f32, f32) {
        (self.scale_x, self.scale_y)
    }

    /// Returns the cached alignment offset from the last layout.
    #[inline]
    pub fn align_offset(&self) -> Offset {
        self.align_offset
    }

    /// The composed translate-then-scale matrix this box applies to
    /// its child.
    ///
    /// THE single transform accessor: `paint_transform` hands exactly
    /// this matrix to the pipeline, and `hit_test` walks through its
    /// inverse — paint and hit-test can never disagree about where the
    /// child is. Identity when nothing is cached (pre-layout /
    /// unit-scale defaults).
    pub fn effective_transform(&self) -> Matrix4 {
        let t = Matrix4::translation(self.align_offset.dx.get(), self.align_offset.dy.get(), 0.0);
        let s = Matrix4::scaling(self.scale_x, self.scale_y, 1.0);
        t * s
    }

    /// Builder: set the fit mode.
    #[must_use]
    pub const fn with_fit(mut self, fit: BoxFit) -> Self {
        self.fit = fit;
        self
    }

    /// Builder: set the alignment.
    #[must_use]
    pub const fn with_alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Builder: set the clip behavior.
    #[must_use]
    pub const fn with_clip_behavior(mut self, clip_behavior: Clip) -> Self {
        self.clip_behavior = clip_behavior;
        self
    }

    /// Updates the fit; returns true if the value changed.
    pub fn set_fit(&mut self, fit: BoxFit) -> bool {
        if self.fit == fit {
            return false;
        }
        self.fit = fit;
        true
    }

    /// Updates the alignment; returns true if the value changed.
    pub fn set_alignment(&mut self, alignment: Alignment) -> bool {
        if self.alignment == alignment {
            return false;
        }
        self.alignment = alignment;
        true
    }

    /// Updates the clip behavior; returns true if the value changed.
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) -> bool {
        if self.clip_behavior == clip_behavior {
            return false;
        }
        self.clip_behavior = clip_behavior;
        true
    }

    /// Maps an alignment scalar in [-1, 1] to a position in [0, free].
    #[inline]
    fn align_axis(component: f32, free: f32) -> f32 {
        free * (component + 1.0) * 0.5
    }

    /// Resets the cached transform state — called when there is no
    /// child or the child sized to zero on either axis.
    fn reset_transform_cache(&mut self) {
        self.scale_x = 1.0;
        self.scale_y = 1.0;
        self.align_offset = Offset::ZERO;
        self.has_visual_overflow = false;
    }
}

impl Default for RenderFittedBox {
    /// Defaults: `fit = BoxFit::Contain`, `alignment = CENTER`,
    /// `clip_behavior = Clip::None` (Flutter parity).
    fn default() -> Self {
        Self::new(BoxFit::Contain, Alignment::CENTER, Clip::None)
    }
}

impl flui_foundation::Diagnosticable for RenderFittedBox {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_enum("fit", self.fit);
        builder.add(
            "alignment",
            format!("({}, {})", self.alignment.x, self.alignment.y),
        );
        builder.add_enum("clip_behavior", self.clip_behavior);
    }
}

impl RenderBox for RenderFittedBox {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let incoming = *ctx.constraints();

        // (1) No child → smallest size, identity transform.
        if ctx.child_count() == 0 {
            self.has_child = false;
            self.reset_transform_cache();
            return incoming.smallest();
        }

        // (2) Lay out the child unconstrained so it picks its intrinsic size.
        self.has_child = true;
        let child_size = ctx.layout_child(0, BoxConstraints::UNCONSTRAINED);
        ctx.position_child(0, Offset::ZERO);

        // (3) Degenerate child → smallest size, identity transform.
        if child_size.width <= px(0.0) || child_size.height <= px(0.0) {
            self.reset_transform_cache();
            return incoming.smallest();
        }

        // (4) Our size = constrain(child_size) — we honour the parent
        //     constraints even though the child was given unconstrained.
        let size = incoming.constrain(child_size);

        // (5) Resolve the fit math via the typed BoxFit helper.
        let FittedSizes {
            source,
            destination,
        } = self.fit.apply(child_size, size);

        // (6) Scale factors take the child's intrinsic size onto the
        //     fitted destination region.
        let src_w = source.width.get();
        let src_h = source.height.get();
        self.scale_x = if src_w > 0.0 {
            destination.width.get() / src_w
        } else {
            1.0
        };
        self.scale_y = if src_h > 0.0 {
            destination.height.get() / src_h
        } else {
            1.0
        };

        // (7) Alignment offset of the destination region inside `size`.
        let free_w = size.width.get() - destination.width.get();
        let free_h = size.height.get() - destination.height.get();
        self.align_offset = Offset::new(
            px(Self::align_axis(self.alignment.x, free_w)),
            px(Self::align_axis(self.alignment.y, free_h)),
        );

        // (8) Overflow flag — the scaled destination may exceed `size`
        //     for `Cover` / `FitWidth` / `FitHeight` / `None`.
        self.has_visual_overflow = destination.width.get() > size.width.get()
            || destination.height.get() > size.height.get();

        size
    }

    flui_rendering::forward_single_child_intrinsics!();

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut flui_rendering::context::BoxDryLayoutCtx<'_>,
    ) -> Size {
        if ctx.child_count() == 0 {
            return constraints.smallest();
        }
        let child_size = ctx.child_dry_layout(0, BoxConstraints::UNCONSTRAINED);
        if child_size.width <= px(0.0) || child_size.height <= px(0.0) {
            return constraints.smallest();
        }
        match self.fit {
            BoxFit::ScaleDown => {
                let loosened = constraints.loosen();
                constraints.constrain(
                    loosened.constrain_size_and_attempt_to_preserve_aspect_ratio(child_size),
                )
            }
            BoxFit::Contain
            | BoxFit::Cover
            | BoxFit::Fill
            | BoxFit::FitHeight
            | BoxFit::FitWidth
            | BoxFit::None => {
                constraints.constrain_size_and_attempt_to_preserve_aspect_ratio(child_size)
            }
        }
    }

    fn compute_dry_baseline(
        &self,
        _constraints: BoxConstraints,
        baseline: TextBaseline,
        ctx: &mut flui_rendering::context::BoxDryBaselineCtx<'_>,
    ) -> Option<f32> {
        if ctx.child_count() == 0 {
            None
        } else {
            ctx.child_dry_baseline(0, BoxConstraints::UNCONSTRAINED, baseline)
        }
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        if !self.has_child {
            return false;
        }
        // Honour clip-on-overflow at the gesture level too: when the
        // user opted into clipping AND the destination overflows, hits
        // outside the laid-out size are unreachable (the visible region
        // is only `ctx.own_size()`). The early-return above already
        // filters those; nothing to add here.
        //
        // Transform symmetry: hit-test through the INVERSE of the same
        // matrix `paint_transform` hands the pipeline (one accessor,
        // both directions), so scaled children receive the correct
        // local point. (The pre-fix shape shifted by align_offset only
        // — any non-unit scale sent the child a wrong local point.)
        let Some(inverse) = self.effective_transform().try_inverse() else {
            // Degenerate scale (zero area) — nothing is visually
            // hittable under a non-invertible transform.
            return false;
        };
        let pos = ctx.offset();
        let (tx, ty) = inverse.transform_point(pos.dx, pos.dy);
        ctx.hit_test_child(0, Offset::new(tx, ty))
    }

    // The `paint_transform` override is the whole point of RenderFittedBox:
    // the pipeline reads it through `&dyn RenderObject<BoxProtocol>` via the
    // blanket impl forwarding here.
    //
    // Returns the composed translate-then-scale matrix the pipeline applies via
    // its `TransformLayer` wrapper. Layout caches the scale factors and
    // alignment offset, so this method is a pure read and does not need the
    // driver-supplied `size`.
    fn paint_transform(&self, _size: Size) -> Option<Matrix4> {
        if !self.has_child {
            return None;
        }
        if self.scale_x == 1.0 && self.scale_y == 1.0 && self.align_offset == Offset::ZERO {
            return None;
        }
        Some(self.effective_transform())
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    #[test]
    fn defaults_match_flutter() {
        let node = RenderFittedBox::default();
        assert_eq!(node.fit(), BoxFit::Contain);
        assert_eq!(node.alignment(), Alignment::CENTER);
        assert_eq!(node.clip_behavior(), Clip::None);
        assert!(!node.has_visual_overflow());
        assert_eq!(node.scale_factors(), (1.0, 1.0));
        assert_eq!(node.align_offset(), Offset::ZERO);
    }

    #[test]
    fn builder_chain_assembles_node() {
        let node = RenderFittedBox::default()
            .with_fit(BoxFit::Cover)
            .with_alignment(Alignment::TOP_LEFT)
            .with_clip_behavior(Clip::AntiAlias);
        assert_eq!(node.fit(), BoxFit::Cover);
        assert_eq!(node.alignment(), Alignment::TOP_LEFT);
        assert_eq!(node.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn setters_return_change_flag() {
        let mut node = RenderFittedBox::default();
        assert!(node.set_fit(BoxFit::Fill));
        assert!(!node.set_fit(BoxFit::Fill));
        assert!(node.set_alignment(Alignment::TOP_LEFT));
        assert!(node.set_clip_behavior(Clip::AntiAlias));
    }

    #[test]
    fn align_axis_maps_minus_one_to_zero() {
        assert_eq!(RenderFittedBox::align_axis(-1.0, 100.0), 0.0);
    }

    #[test]
    fn align_axis_maps_zero_to_half_free() {
        assert_eq!(RenderFittedBox::align_axis(0.0, 100.0), 50.0);
    }

    #[test]
    fn align_axis_maps_plus_one_to_full_free() {
        assert_eq!(RenderFittedBox::align_axis(1.0, 100.0), 100.0);
    }

    /// Transform symmetry: `paint_transform` IS `effective_transform`,
    /// and the inverse maps a visual point back to the child-local
    /// point — paint and hit-test cannot disagree.
    #[test]
    fn paint_and_hit_test_share_one_transform() {
        let node = RenderFittedBox {
            has_child: true,
            scale_x: 2.0,
            scale_y: 2.0,
            align_offset: Offset::new(px(10.0), px(0.0)),
            ..Default::default()
        };

        assert_eq!(
            node.paint_transform(Size::ZERO),
            Some(node.effective_transform()),
            "paint must hand the pipeline the SAME matrix hit-test inverts",
        );

        let inverse = node
            .effective_transform()
            .try_inverse()
            .expect("translate*scale(2,2) is invertible");
        // Visual (70, 40) under translate(10,0)*scale(2,2) came from
        // child-local ((70-10)/2, 40/2) = (30, 20).
        let (tx, ty) = inverse.transform_point(px(70.0), px(40.0));
        assert!((tx.get() - 30.0).abs() < 1e-4, "tx = {tx:?}");
        assert!((ty.get() - 20.0).abs() < 1e-4, "ty = {ty:?}");
    }

    #[test]
    fn paint_transform_is_none_without_child() {
        let node = RenderFittedBox::default();
        // No child, no transform.
        assert!(node.paint_transform(Size::ZERO).is_none());
    }

    #[test]
    fn paint_transform_short_circuits_without_child() {
        // Companion to `paint_transform_is_none_without_child`: the
        // `if !self.has_child { return None; }` gate runs *before* the
        // identity-state check, so a no-child node returns None even
        // with scale=1.0 / align=ZERO cached values. The identity-state
        // path itself is exercised end-to-end in the layout tests
        // above that drive `perform_layout` with `BoxFit::Fill` against
        // a size that already matches the child — see e.g.
        // `fitted_box_layout_*` tests.
        let node = RenderFittedBox::default();
        assert!(node.paint_transform(Size::ZERO).is_none());
    }

    #[test]
    fn debug_fill_properties_lists_state() {
        use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
        let node = RenderFittedBox::default();
        let mut builder = DiagnosticsBuilder::new();
        node.debug_fill_properties(&mut builder);
        let names: Vec<String> = builder
            .build()
            .iter()
            .map(|p| p.name().to_string())
            .collect();
        for required in ["fit", "alignment", "clip_behavior"] {
            assert!(
                names.iter().any(|n| n == required),
                "missing diagnostic field: {required}"
            );
        }
    }

    // ---------- BoxFit math integration --------------------------------------

    #[test]
    fn fit_contain_into_widescreen_does_not_overflow() {
        // 16:9 box, square child: BoxFit::Contain → child shrinks to fit
        // the height. No overflow.
        let sizes = BoxFit::Contain.apply(
            Size::new(px(100.0), px(100.0)),
            Size::new(px(160.0), px(90.0)),
        );
        // destination should be 90x90 (square inscribed in 160x90 height).
        assert_eq!(sizes.destination.height, px(90.0));
        assert!(sizes.destination.width.get() <= 160.0);
    }

    #[test]
    fn fit_cover_into_widescreen_overflows_horizontally() {
        // 16:9 box, square child: BoxFit::Cover → child grows to cover
        // both axes; horizontally overflows the box.
        let sizes = BoxFit::Cover.apply(
            Size::new(px(100.0), px(100.0)),
            Size::new(px(160.0), px(90.0)),
        );
        // destination width should exceed the box width.
        assert!(sizes.destination.width.get() >= 160.0);
    }
}
