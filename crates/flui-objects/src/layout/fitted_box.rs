//! `RenderFittedBox` — single-child proxy that scales its child to fit
//! its own box per a [`BoxFit`] mode and aligns it via [`Alignment`].
//!
//! # Design
//!
//! * The scaling math is delegated to the typed [`BoxFit::apply`] (from
//!   `flui_painting`), which returns a structured [`FittedSizes`] with both
//!   `source` and `destination` regions, so the seven `BoxFit` variants are
//!   implemented once.
//! * The scale + alignment transform is a single composed [`Matrix4`]
//!   (`effective_transform`) that `paint` pushes and
//!   `apply_paint_transform` folds into coordinate mapping —
//!   one matrix, two consumers, so paint and `local_to_global` cannot drift.
//! * `has_visual_overflow()` is a public post-layout query method.
//!
//! # Cropping
//!
//! `BoxFit::Cover`/`FitWidth`/`FitHeight`/`None` crop the source: the result
//! is a cropped source with an exactly-filled destination, not a full source
//! with an overflowing one. This render object therefore carries a
//! `source_offset` — the cropped source region's own top-left within the
//! child — folded into `RenderFittedBox::effective_transform` as a third
//! `translate(-source_offset)` term alongside
//! `translate(align_offset) * scale`. The term is live for any crop under a
//! non-degenerate alignment, including the default `CENTER`.
//!
//! # Clipping
//!
//! `paint` clips to this box when the fit cropped the source and
//! `clip_behavior` is not `Clip::None`. The machinery
//! (`PaintCx::with_clip_rect`) is shared with the rest of
//! the overflow-gated family that clips from `paint` rather than
//! `paint_effects` — `RenderViewport`, `RenderConstraintsTransformBox`,
//! `RenderWrap`, `RenderStack`, `RenderAnimatedSize`. (`RenderClip` is not
//! among them: it reports its clip through `paint_effects` instead, as a
//! composited-layer-patchable property rather than a canvas clip scope —
//! see `crates/flui-rendering/ARCHITECTURE.md`'s clip-producer accounting.)
//!
//! The constraint is **ordering**, and it is why `paint` pushes
//! the fit transform itself rather than leaving it to `paint_effects`: the
//! paint walk emits a node's `paint_effects` transform layer *before*
//! replaying the fragment ops that node's `paint` recorded, so a clip opened
//! in `paint`
//! would land inside the transform — clipping a rectangle stated in this
//! box's coordinates against the child's scaled ones. Pushing both from
//! `paint` puts them the right way round; `apply_paint_transform` then keeps
//! coordinate mapping working without re-emitting the layer.

use flui_foundation::Single;
use flui_foundation::geometry::{Matrix4, Offset, Point, Rect, Size};
use flui_painting::paint::Clip;
use flui_painting::{Alignment, BoxFit, FittedSizes};

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
    /// Cached scale factors derived in layout, folded into
    /// [`Self::effective_transform`] — which `paint` pushes (or, for a
    /// pure translation, applies as the child offset), `hit_test` inverts,
    /// and `apply_paint_transform` composes into coordinate mapping.
    scale_x: f64,
    scale_y: f64,
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
    pub fn scale_factors(&self) -> (f64, f64) {
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
    /// THE single transform accessor: `paint` pushes exactly this matrix
    /// as its transform scope (or, for a pure translation, applies it as
    /// the child offset), `apply_paint_transform` folds it into
    /// coordinate mapping, and `hit_test` walks through its inverse —
    /// paint, mapping, and hit-test can never disagree about where the
    /// child is. `paint_effects` is deliberately NOT a consumer of it: its
    /// `transform` field stays `None` (see `paint` for why). Identity when
    /// nothing is cached (pre-layout / unit-scale defaults).
    ///
    /// Three parts: translate to the destination region's
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
        let t = Matrix4::translation(self.align_offset.dx, self.align_offset.dy, 0.0);
        let s = Matrix4::scaling(self.scale_x, self.scale_y, 1.0);
        let pre = Matrix4::translation(-self.source_offset.dx, -self.source_offset.dy, 0.0);
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
    fn align_axis(component: f64, free: f64) -> f64 {
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
    /// layout sizes. It is computed eagerly (not lazily at paint time) so paint, hit testing, and coordinate mapping share
    /// one current cache even when only paint was invalidated.
    fn update_paint_data(&mut self, size: Size, child_size: Size) {
        let FittedSizes {
            source,
            destination,
        } = self.fit.apply(child_size, size);

        let source_width = source.width;
        let source_height = source.height;
        self.scale_x = if source_width > 0.0 {
            destination.width / source_width
        } else {
            1.0
        };
        self.scale_y = if source_height > 0.0 {
            destination.height / source_height
        } else {
            1.0
        };

        let free_width = size.width - destination.width;
        let free_height = size.height - destination.height;
        self.align_offset = Offset::new(
            Self::align_axis(self.alignment.x, free_width),
            Self::align_axis(self.alignment.y, free_height),
        );

        let source_free_width = child_size.width - source_width;
        let source_free_height = child_size.height - source_height;
        self.source_offset = Offset::new(
            Self::align_axis(self.alignment.x, source_free_width),
            Self::align_axis(self.alignment.y, source_free_height),
        );

        self.has_visual_overflow =
            source_width < child_size.width || source_height < child_size.height;
        self.last_layout_sizes = Some((size, child_size));
    }

    /// The box's own size: honours the parent `constraints` while preserving
    /// the child's aspect ratio (via
    /// `constrain_size_and_attempt_to_preserve_aspect_ratio`, not a plain
    /// `constrain`); `ScaleDown` loosens first then re-constrains. Shared by `perform_layout`
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
    /// `clip_behavior = Clip::None`.
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

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        let incoming = *ctx.constraints();

        // (1) No child → smallest size, identity transform.
        if ctx.child_count() == 0 {
            self.has_child = false;
            self.reset_transform_cache();
            return Ok(incoming.smallest());
        }

        // (2) Lay out the child unconstrained so it picks its intrinsic size.
        self.has_child = true;
        let child_size = ctx.layout_child(0, BoxConstraints::UNCONSTRAINED)?;
        ctx.position_child(0, Offset::ZERO);

        // (3) Degenerate child → smallest size, identity transform.
        if child_size.width <= 0.0 || child_size.height <= 0.0 {
            self.reset_transform_cache();
            self.child_is_empty = true;
            return Ok(incoming.smallest());
        }
        self.child_is_empty = false;

        // (4) Our size honours the parent constraints while preserving the
        //     child's aspect ratio (not a plain constrain). Shared with
        //     compute_dry_layout via `fitted_size` so the wet and dry sizes
        //     agree.
        let size = self.fitted_size(incoming, child_size);

        // (5) Resolve transform and overflow paint data. The retained size
        // pair also lets paint-only fit/alignment setters refresh this cache
        // without forcing layout.
        self.update_paint_data(size, child_size);

        Ok(size)
    }

    flui_rendering::forward_single_child_intrinsics!();

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut flui_rendering::context::BoxDryLayoutCtx<'_>,
    ) -> flui_rendering::RenderResult<Size> {
        if ctx.child_count() == 0 {
            return Ok(constraints.smallest());
        }
        let child_size = ctx.child_dry_layout(0, BoxConstraints::UNCONSTRAINED)?;
        if child_size.width <= 0.0 || child_size.height <= 0.0 {
            return Ok(constraints.smallest());
        }
        Ok(self.fitted_size(constraints, child_size))
    }

    fn compute_dry_baseline(
        &self,
        _constraints: BoxConstraints,
        baseline: TextBaseline,
        ctx: &mut flui_rendering::context::BoxDryBaselineCtx<'_>,
    ) -> flui_rendering::RenderResult<Option<f64>> {
        if ctx.child_count() == 0 {
            Ok(None)
        } else {
            ctx.child_dry_baseline(0, BoxConstraints::UNCONSTRAINED, baseline)
        }
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        // A child that cannot be painted must not be reachable by a pointer
        // either (the same guards `paint` applies) — the box's own size gate
        // above covers the empty-box half.
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
        // matrix `paint` pushes (one accessor, both directions), so scaled
        // children receive the correct local point. (The pre-fix shape
        // shifted by align_offset only — any non-unit scale sent the child
        // a wrong local point.)
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
    /// The transformed child paint is wrapped in a clip when
    /// `has_visual_overflow && clip_behavior != Clip::None`, so the composited
    /// chain reads clip-outside-transform
    /// (`[…, ClipRectLayer, TransformLayer, …]`).
    ///
    /// **The order is why this pushes the transform itself instead of leaving
    /// it to `paint_effects`.** The paint walk emits a node's `paint_effects`
    /// transform layer *before* replaying the fragment ops that node's
    /// `paint` recorded, so a clip opened here would land *inside* the
    /// transform — clipping a rectangle stated in this box's coordinates
    /// against the child's scaled ones. Pushing both from `paint`, in this
    /// order, is what puts them the right way round.
    ///
    /// `paint_effects` itself is not overridden and its `transform` field
    /// stays `None` — a `Some` there would make the walk push a transform
    /// layer of its own around whatever this method already applies (the
    /// transform scope, or the child offset for a pure translation), applying
    /// the fit twice. Coordinate mapping (`transform_to` and the
    /// local-to-global family) is a separate concern from layer emission and
    /// reads the `apply_paint_transform` override below.
    fn paint(&self, ctx: &mut flui_rendering::context::PaintCx<'_, Single>) {
        if !self.has_child {
            return;
        }

        let size = ctx.size();
        // Bail out when there is no child, the box is empty, or the child is
        // empty. All three clauses matter — a zero-area child still paints
        // whatever its own `paint` draws (a `CustomPaint` ignores its size
        // happily), and the fit transform onto or from a zero extent is
        // degenerate, so neither a collapsed box nor a collapsed child may
        // reach the child's paint.
        if self.child_is_empty || size.width <= 0.0 || size.height <= 0.0 {
            return;
        }

        let transform = self.effective_transform();
        // A fit that neither scales nor crops leaves a pure translation, which
        // is cheaper — and equally correct — to apply as a child offset than
        // as a compositing layer; `BoxFit::None` under an off-centre alignment
        // is the case that reaches it.
        let paint_transformed_child = |ctx: &mut flui_rendering::context::PaintCx<'_, Single>| {
            if let Some((dx, dy)) = transform.as_translation() {
                ctx.paint_child_at(Offset::new(dx, dy));
            } else {
                // Not a redundant closure: `paint_child` is inherent on PaintCx
                // for both `Exact<1>` and `Variable`, so the bare path is
                // ambiguous (E0034) and the lint's rewrite does not compile.
                #[expect(clippy::redundant_closure_for_method_calls)]
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
    /// the same thing from `paint_effects`'s `transform` field. `paint` above
    /// emits the transform layer itself, so that field must stay `None` or
    /// the paint walk would push a *second* transform around the one `paint`
    /// already opened — applying the fit twice. Mapping still needs the
    /// matrix, so it is supplied here.
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
        *transform *= Matrix4::translation(child_offset.dx, child_offset.dy, 0.0);
    }
}

// ===========================================================================
// Tests
// ===========================================================================
