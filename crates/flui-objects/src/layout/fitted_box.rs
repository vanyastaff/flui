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
//! * The scale + alignment transform is a single composed [`Matrix4`]
//!   (`effective_transform`) that `paint` pushes and
//!   `apply_paint_transform` folds into coordinate mapping —
//!   one matrix, two consumers, so paint and `localToGlobal` cannot drift.
//!   Flutter splits the same two duties the same way.
//! * `has_visual_overflow()` is a public post-layout query method
//!   (Flutter keeps the equivalent flag private and only consults it
//!   internally for clip-decision branching).
//!
//! # Divergence found and fixed (widget-parity port, `parity/fitted_box_test.rs`)
//!
//! Porting Flutter's `'Child can cover'` (`fitted_box_test.dart`, 3.44.0)
//! surfaced a real bug, but **not in this file** — it lived one layer down,
//! in `flui_types::layout::BoxFit::apply`. Every branch there answered
//! `source: input_size` unconditionally, so `BoxFit::Cover`/`FitWidth`/
//! `FitHeight` never actually cropped the source the way Flutter's
//! `applyBoxFit` does (`box_fit.dart`, 3.44.0) — instead of a cropped
//! source with an exactly-filled destination, FLUI produced a full source
//! with an OVERFLOWING destination. `RenderFittedBox` here faithfully
//! consumed whatever `apply()` handed it, so this file's own math was never
//! the problem; it just had nothing to compute an offset from.
//!
//! Two things changed as a result:
//! 1. `BoxFit::apply` (`flui-types`) now crops the source exactly as
//!    `applyBoxFit` does for `Cover`/`FitWidth`/`FitHeight`/`None`.
//! 2. This render object gained a `source_offset` field — the cropped
//!    source region's own top-left within the child, i.e. Flutter's
//!    `sourceRect.left`/`top` (`RenderFittedBox._updatePaintData`,
//!    `proxy_box.dart`) — folded into `RenderFittedBox::effective_transform` as a
//!    third `translate(-source_offset)` term alongside the pre-existing
//!    `translate(align_offset) * scale`. Before the `flui-types` fix, this
//!    term would have been a permanent no-op (`source_offset` could never
//!    be anything but zero); it is now live for any crop under a
//!    non-degenerate alignment, including the default `CENTER`.
//!
//! # Clipping
//!
//! `paint` clips to this box when the fit cropped the source, matching
//! Flutter's `_hasVisualOverflow && clipBehavior != Clip.none` branch. This
//! module used to record active clipping as deferred, "awaiting the
//! layer-level clip integration"; that had gone stale — the machinery
//! (`PaintCx::with_clip_rect`, already used by `RenderClip` and
//! `RenderPhysicalModel`) was in place.
//!
//! What genuinely blocked it was **ordering**, and it is why `paint` pushes
//! the fit transform itself rather than leaving it to `paint_transform`: the
//! paint walk emits a node's `paint_transform` layer *before* replaying the
//! fragment ops that node's `paint` recorded, so a clip opened in `paint`
//! would land inside the transform — clipping a rectangle stated in this
//! box's coordinates against the child's scaled ones. Pushing both from
//! `paint` puts them the right way round; `apply_paint_transform` then keeps
//! coordinate mapping working without re-emitting the layer.
//!
//! Verified at both layers: `flui-types`' own unit tests pin every
//! `BoxFit::apply` variant against oracle-computed `(source, destination)`
//! pairs; this crate's `tests/render_object_harness.rs` drives
//! `perform_layout` through the real pipeline
//! (`harness_fitted_box_cover_crops_the_source_and_offsets_the_transform`)
//! to prove `source_offset` is genuinely reachable, not just a field no
//! call path ever sets to a nonzero value.

use flui_tree::Single;
use flui_types::{
    Alignment, Matrix4, Offset, Point, Rect, Size,
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
    /// Cached top-left offset of the (possibly cropped) source region
    /// *within the child* — nonzero whenever [`BoxFit::apply`] crops the
    /// child (`Cover`/`FitWidth`/`FitHeight`/an overflowing `None`) under an
    /// off-center `alignment`. See [`Self::effective_transform`].
    source_offset: Offset,
    /// True iff the scaled child exceeds `size` on either axis.
    has_visual_overflow: bool,
    /// True iff the child laid out to a zero-area size.
    ///
    /// Tracked separately from `has_child` because a degenerate child is still
    /// *a* child — intrinsics and layout must keep forwarding to it — but it
    /// must not paint or be hit-tested, which is a property of its measured
    /// size and so is only knowable after layout.
    child_is_empty: bool,
    /// Last valid `(own_size, child_size)` pair, used to recompute paint data
    /// when fit or alignment changes without layout.
    last_layout_sizes: Option<(Size, Size)>,
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
            source_offset: Offset::ZERO,
            has_visual_overflow: false,
            child_is_empty: false,
            last_layout_sizes: None,
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

    /// Returns the cached source-region offset from the last layout — see
    /// [`Self::effective_transform`].
    #[inline]
    pub fn source_offset(&self) -> Offset {
        self.source_offset
    }

    /// The composed translate-scale-translate matrix this box applies to
    /// its child.
    ///
    /// THE single transform accessor: `paint_transform` hands exactly
    /// this matrix to the pipeline, and `hit_test` walks through its
    /// inverse — paint and hit-test can never disagree about where the
    /// child is. Identity when nothing is cached (pre-layout /
    /// unit-scale defaults).
    ///
    /// Three parts, matching Flutter's `RenderFittedBox._updatePaintData`
    /// (`proxy_box.dart`) exactly: translate to the destination region's
    /// top-left, scale, then translate by the NEGATIVE of the source
    /// region's top-left within the child. That third term only matters
    /// when [`BoxFit::apply`] crops the child (`Cover`/`FitWidth`/
    /// `FitHeight`/an overflowing `None`) under an off-center `alignment` —
    /// `Contain`/`Fill`/`ScaleDown` never crop, so `source_offset` is
    /// always `Offset::ZERO` for them and this term is a no-op. Dropping
    /// it (as an earlier version of this method did) left both paint and
    /// hit-testing consistently wrong together for a cropped, off-center
    /// `Cover` — silently, since the invariant this method exists to
    /// enforce (paint and hit-test can't disagree WITH EACH OTHER) still
    /// held; they simply agreed on the wrong point.
    pub fn effective_transform(&self) -> Matrix4 {
        let t = Matrix4::translation(self.align_offset.dx.get(), self.align_offset.dy.get(), 0.0);
        let s = Matrix4::scaling(self.scale_x, self.scale_y, 1.0);
        let pre = Matrix4::translation(
            -self.source_offset.dx.get(),
            -self.source_offset.dy.get(),
            0.0,
        );
        t * s * pre
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

    /// Updates the fit and reports whether layout or paint is required.
    pub fn set_fit(&mut self, fit: BoxFit) -> flui_rendering::RenderUpdateImpact {
        if self.fit == fit {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        let fit_affected_layout = self.fit == BoxFit::ScaleDown || fit == BoxFit::ScaleDown;
        self.fit = fit;
        if fit_affected_layout {
            flui_rendering::RenderUpdateImpact::LAYOUT
        } else {
            if let Some((size, child_size)) = self.last_layout_sizes {
                self.update_paint_data(size, child_size);
            }
            flui_rendering::RenderUpdateImpact::PAINT
        }
    }

    /// Updates the alignment and reports paint when the value changed.
    pub fn set_alignment(&mut self, alignment: Alignment) -> flui_rendering::RenderUpdateImpact {
        if self.alignment == alignment {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.alignment = alignment;
        if let Some((size, child_size)) = self.last_layout_sizes {
            self.update_paint_data(size, child_size);
        }
        flui_rendering::RenderUpdateImpact::PAINT
    }

    /// Updates the overflow clip and reports paint plus semantics when changed.
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) -> flui_rendering::RenderUpdateImpact {
        if self.clip_behavior == clip_behavior {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.clip_behavior = clip_behavior;
        flui_rendering::RenderUpdateImpact::PAINT | flui_rendering::RenderUpdateImpact::SEMANTICS
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
        self.source_offset = Offset::ZERO;
        self.has_visual_overflow = false;
        self.last_layout_sizes = None;
    }

    /// Recomputes the fit transform and overflow state from the most recent
    /// layout sizes. Flutter performs this lazily in `_updatePaintData`; FLUI
    /// computes it eagerly so paint, hit testing, and coordinate mapping share
    /// one current cache even when only paint was invalidated.
    fn update_paint_data(&mut self, size: Size, child_size: Size) {
        let FittedSizes {
            source,
            destination,
        } = self.fit.apply(child_size, size);

        let source_width = source.width.get();
        let source_height = source.height.get();
        self.scale_x = if source_width > 0.0 {
            destination.width.get() / source_width
        } else {
            1.0
        };
        self.scale_y = if source_height > 0.0 {
            destination.height.get() / source_height
        } else {
            1.0
        };

        let free_width = size.width.get() - destination.width.get();
        let free_height = size.height.get() - destination.height.get();
        self.align_offset = Offset::new(
            px(Self::align_axis(self.alignment.x, free_width)),
            px(Self::align_axis(self.alignment.y, free_height)),
        );

        let source_free_width = child_size.width.get() - source_width;
        let source_free_height = child_size.height.get() - source_height;
        self.source_offset = Offset::new(
            px(Self::align_axis(self.alignment.x, source_free_width)),
            px(Self::align_axis(self.alignment.y, source_free_height)),
        );

        self.has_visual_overflow =
            source_width < child_size.width.get() || source_height < child_size.height.get();
        self.last_layout_sizes = Some((size, child_size));
    }

    /// The box's own size: honours the parent `constraints` while preserving
    /// the child's aspect ratio. Flutter `RenderFittedBox.performLayout`
    /// (`proxy_box.dart`) uses `constrainSizeAndAttemptToPreserveAspectRatio`;
    /// `ScaleDown` loosens first then re-constrains. Shared by `perform_layout`
    /// and `compute_dry_layout` so the wet and dry sizes can never drift.
    fn fitted_size(&self, constraints: BoxConstraints, child_size: Size) -> Size {
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
            self.child_is_empty = true;
            return incoming.smallest();
        }
        self.child_is_empty = false;

        // (4) Our size honours the parent constraints while preserving the
        //     child's aspect ratio (Flutter uses
        //     constrainSizeAndAttemptToPreserveAspectRatio, not a plain
        //     constrain). Shared with compute_dry_layout via `fitted_size` so
        //     the wet and dry sizes agree.
        let size = self.fitted_size(incoming, child_size);

        // (5) Resolve transform and overflow paint data. The retained size
        // pair also lets paint-only fit/alignment setters refresh this cache
        // without forcing layout, matching Flutter's `_updatePaintData`.
        self.update_paint_data(size, child_size);

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
        self.fitted_size(constraints, child_size)
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
        // Flutter's `hitTestChildren` carries the same pair of guards its
        // `paint` does: `if (size.isEmpty || (child?.size.isEmpty ?? false))`.
        // A child that cannot be painted must not be reachable by a pointer
        // either — the box's own size gate above covers the empty-box half.
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
        let transform = self.effective_transform();
        let Some(inverse) = transform.try_inverse() else {
            // Degenerate scale (zero area) — nothing is visually
            // hittable under a non-invertible transform.
            return false;
        };
        let pos = ctx.offset();
        let (tx, ty) = inverse.transform_point(pos.dx, pos.dy);
        // Push the FORWARD (paint-direction) transform onto the ctx-level
        // stack so the driver folds its inverse into `HitTestEntry.transform`
        // — this render object has no `hit_test_transform` override (unlike
        // `RenderTransform`/`RenderRotatedBox`, which record via that
        // driver-level hook instead), so without this push the descent above
        // would still land on the right child pixel while the delivered
        // event position stayed un-localized.
        ctx.with_transform(transform, |ctx| ctx.hit_test_child(0, Offset::new(tx, ty)))
    }

    /// Paints the child through the fit transform, clipped to this box when
    /// the fit cropped the source.
    ///
    /// Flutter's `RenderFittedBox.paint` wraps the transformed child paint in
    /// `pushClipRect` when `_hasVisualOverflow && clipBehavior != Clip.none`,
    /// so the composited chain reads clip-outside-transform
    /// (`[…, ClipRectLayer, TransformLayer, …]`).
    ///
    /// **The order is why this pushes the transform itself instead of leaving
    /// it to `paint_transform`.** The paint walk emits a node's
    /// `paint_transform` layer *before* replaying the fragment ops that
    /// node's `paint` recorded, so a clip opened here would land *inside* the
    /// transform — clipping a rectangle stated in this box's coordinates
    /// against the child's scaled ones. Pushing both from `paint`, in this
    /// order, is what puts them the right way round.
    ///
    /// `paint_transform` is retained for coordinate mapping (`transform_to`
    /// and the `localToGlobal` family read it), which is a separate concern
    /// from layer emission — Flutter likewise keeps `applyPaintTransform`
    /// alongside `paint`.
    fn paint(&self, ctx: &mut flui_rendering::context::PaintCx<'_, Single>) {
        if !self.has_child {
            return;
        }

        let size = ctx.size();
        // Flutter: `if (child == null || size.isEmpty || child!.size.isEmpty)
        // return;`. All three clauses matter — a zero-area child still paints
        // whatever its own `paint` draws (a `CustomPaint` ignores its size
        // happily), and the fit transform onto or from a zero extent is
        // degenerate, so neither a collapsed box nor a collapsed child may
        // reach the child's paint.
        if self.child_is_empty || size.width.get() <= 0.0 || size.height.get() <= 0.0 {
            return;
        }

        let transform = self.effective_transform();
        // A fit that neither scales nor crops leaves a pure translation, which
        // is cheaper — and, per the oracle, correct — to apply as a child
        // offset than as a compositing layer. Flutter's
        // `_paintChildWithTransform` forks on `MatrixUtils.getAsTranslation`
        // for exactly this; `BoxFit::None` under an off-centre alignment is
        // the case that reaches it.
        let paint_transformed_child = |ctx: &mut flui_rendering::context::PaintCx<'_, Single>| {
            if let Some((dx, dy)) = transform.as_translation() {
                ctx.paint_child_at(Offset::new(px(dx), px(dy)));
            } else {
                // Not a redundant closure: `paint_child` is inherent on PaintCx
                // for both `Exact<1>` and `Variable`, so the bare path is
                // ambiguous (E0034) and the lint's rewrite does not compile.
                #[allow(clippy::redundant_closure_for_method_calls)]
                ctx.with_transform(transform, |ctx| ctx.paint_child());
            }
        };

        if self.has_visual_overflow && self.clip_behavior != Clip::None {
            let bounds = Rect::from_origin_size(Point::ZERO, size);
            ctx.with_clip_rect(bounds, self.clip_behavior, paint_transformed_child);
        } else {
            paint_transformed_child(ctx);
        }
    }

    /// Folds the fit transform into a child-to-parent coordinate mapping.
    ///
    /// Deliberately overridden instead of leaving the default, which derives
    /// the same thing from `paint_transform`. `paint` above emits the
    /// transform layer itself, so `paint_transform` must stay `None` or the
    /// paint walk would push a *second* transform around the one `paint`
    /// already opened — applying the fit twice. Mapping still needs the
    /// matrix, so it is supplied here.
    ///
    /// Flutter splits the same two duties the same way: `RenderFittedBox`
    /// overrides `paint` and `applyPaintTransform` separately, each
    /// multiplying in `_transform` for its own purpose.
    fn apply_paint_transform(
        &self,
        _child: usize,
        child_offset: Offset,
        _size: Size,
        transform: &mut Matrix4,
    ) {
        if self.has_child {
            *transform *= self.effective_transform();
        }
        *transform *= Matrix4::translation(child_offset.dx.0, child_offset.dy.0, 0.0);
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
    fn setters_report_exact_flutter_impacts() {
        let mut node = RenderFittedBox::default();
        assert_eq!(
            node.set_fit(BoxFit::Fill),
            flui_rendering::RenderUpdateImpact::PAINT
        );
        assert_eq!(
            node.set_fit(BoxFit::Fill),
            flui_rendering::RenderUpdateImpact::NONE
        );
        assert_eq!(
            node.set_fit(BoxFit::ScaleDown),
            flui_rendering::RenderUpdateImpact::LAYOUT
        );
        assert_eq!(
            node.set_fit(BoxFit::Contain),
            flui_rendering::RenderUpdateImpact::LAYOUT
        );
        assert_eq!(
            node.set_alignment(Alignment::TOP_LEFT),
            flui_rendering::RenderUpdateImpact::PAINT
        );
        assert_eq!(
            node.set_clip_behavior(Clip::AntiAlias),
            flui_rendering::RenderUpdateImpact::PAINT
                | flui_rendering::RenderUpdateImpact::SEMANTICS
        );
    }

    #[test]
    fn paint_cache_handles_zero_source_axes_and_vertical_only_overflow() {
        let mut zero_width = RenderFittedBox::default();
        zero_width.update_paint_data(
            Size::new(px(100.0), px(100.0)),
            Size::new(px(0.0), px(20.0)),
        );
        assert_eq!(zero_width.scale_x, 1.0);
        assert!(zero_width.scale_y.is_finite());

        let mut zero_height = RenderFittedBox::default();
        zero_height.update_paint_data(
            Size::new(px(100.0), px(100.0)),
            Size::new(px(20.0), px(0.0)),
        );
        assert_eq!(zero_height.scale_y, 1.0);
        assert!(zero_height.scale_x.is_finite());

        let mut vertical_overflow = RenderFittedBox::default().with_fit(BoxFit::Cover);
        vertical_overflow.update_paint_data(
            Size::new(px(200.0), px(100.0)),
            Size::new(px(100.0), px(200.0)),
        );
        assert!(vertical_overflow.has_visual_overflow());
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

    /// Transform symmetry: every consumer of the fit transform reads the
    /// SAME `effective_transform`, and its inverse maps a visual point back
    /// to the child-local point — paint, coordinate mapping, and hit-test
    /// cannot disagree.
    ///
    /// Asserted through `apply_paint_transform` rather than `paint_transform`
    /// because `paint` now emits the transform layer itself (so the box can
    /// open its clip *outside* that layer). `paint_transform` must therefore
    /// stay at its `None` default — a `Some` there would make the paint walk
    /// push a second transform around the one `paint` opened, applying the fit
    /// twice — and that absence is asserted here so the two cannot drift back
    /// into double-application.
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
            RenderBox::paint_transform(&node, Size::ZERO),
            None,
            "paint emits the transform layer itself; a Some here would double it",
        );

        let mut mapped = Matrix4::IDENTITY;
        node.apply_paint_transform(0, Offset::ZERO, Size::ZERO, &mut mapped);
        assert_eq!(
            mapped,
            node.effective_transform(),
            "coordinate mapping must fold in the SAME matrix hit-test inverts",
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

    /// Pure composition-math check for `effective_transform`'s third term:
    /// GIVEN a nonzero `source_offset` (as `perform_layout` now produces for
    /// a cropped `Cover`/`FitWidth`/`FitHeight` — see the integration-level
    /// proof in `crates/flui-objects/tests/render_object_harness.rs`'s
    /// `harness_fitted_box_cover_crops_the_source_and_offsets_the_transform`,
    /// which drives the real pipeline end to end), the matrix must fold in
    /// `translate(-source_offset)` alongside `translate(align_offset) *
    /// scale`. This test fixes the three cached fields directly to isolate
    /// the matrix math itself from `perform_layout`'s own computation of
    /// them — it does NOT claim `perform_layout` reaches this state (the
    /// harness test above does).
    ///
    /// A 100×50 child covering a 200×200 box scales by `max(200/100,
    /// 200/50) = 4`; that overflows the width, so `BoxFit::Cover` crops the
    /// source to `50×50` (`200/4`), centered — inset `(100 - 50) / 2 = 25`
    /// on the width axis, `0` on the height axis (no crop there). The
    /// composed transform must map child-local `(50, 25)` (the crop
    /// window's center) to visual `(100, 100)` — the box's own center —
    /// through `translate(destRect) * scale(4,4) * translate(-sourceRect)`;
    /// the two-part `translate(destRect) * scale(4,4)` this test guards
    /// against maps it to `(200, 100)` instead.
    #[test]
    fn effective_transform_composition_includes_the_source_offset_term() {
        let node = RenderFittedBox {
            has_child: true,
            fit: BoxFit::Cover,
            scale_x: 4.0,
            scale_y: 4.0,
            align_offset: Offset::ZERO,
            source_offset: Offset::new(px(25.0), px(0.0)),
            ..Default::default()
        };

        let (x, y) = node
            .effective_transform()
            .transform_point(px(50.0), px(25.0));
        assert!(
            (x.get() - 100.0).abs() < 1e-4 && (y.get() - 100.0).abs() < 1e-4,
            "child-local (50, 25) (the crop window's center) must map to the \
             box's own center (100, 100), got ({}, {})",
            x.get(),
            y.get(),
        );
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

    /// Flutter parity: `applyBoxFit(BoxFit.cover, ...)` (`box_fit.dart`,
    /// 3.44.0) never lets `destination` exceed `output` — it crops
    /// `source` instead. A `100×100` square child into a `160×90` (16:9)
    /// box: the box is proportionally WIDER than the child, so Cover crops
    /// the child's HEIGHT to `100 * 90/160 = 56.25` and fills the box
    /// exactly (`destination == output`, never overflowing it).
    #[test]
    fn fit_cover_into_widescreen_crops_the_source_height() {
        let sizes = BoxFit::Cover.apply(
            Size::new(px(100.0), px(100.0)),
            Size::new(px(160.0), px(90.0)),
        );
        assert_eq!(sizes.destination, Size::new(px(160.0), px(90.0)));
        assert_eq!(sizes.source, Size::new(px(100.0), px(56.25)));
    }
}
