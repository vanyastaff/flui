//! `RenderCustomPaint` — delegates painting (and optionally hit testing) to
//! user-supplied [`CustomPainter`]s around a single child.
//!
//! Flutter parity: `rendering/custom_paint.dart` `RenderCustomPaint`. Paint
//! order is background painter → child → foreground painter; hit-test order
//! is foreground → child → background (oracle L559-570). Sizing: to the
//! child when present, else `constraints.constrain(preferred_size)` (oracle
//! `computeSizeForNoChild`, L579).
//!
//! Repaint wiring (`CustomPainter.addListener`/`removeListener` driving
//! `markNeedsPaint`) is implemented via ADR-0013: [`CustomPainter::repaint`]
//! returns an optional [`Listenable`](flui_foundation::Listenable) that
//! [`RenderBox::attach`] subscribes to
//! (marking this node needing paint on notify) and [`RenderBox::detach`] tears
//! down; a painter swap migrates the subscription.
//!
//! Deferred vs. the oracle (documented, not silently dropped): `semanticsBuilder`
//! and the `isComplex`/`willChange` raster-cache hints have no FLUI-side
//! plumbing yet — FLUI's [`PaintCx`] has no `setIsComplexHint`/`setWillChangeHint`
//! equivalent. The two hint fields are carried on this type for Flutter-shape
//! parity but are currently inert.

use std::sync::Arc;

use flui_foundation::ListenerId;
use flui_foundation::Single;
use flui_foundation::geometry::{Offset, Size};
use flui_painting::Canvas;

use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext, PaintCx},
    delegates::CustomPainter,
    parent_data::BoxParentData,
    pipeline::RenderInvalidationHandle,
    traits::RenderBox,
};

/// A render object that paints custom graphics before and/or after its
/// child via user-supplied [`CustomPainter`] delegates.
///
/// Sizes to its child when one is present; otherwise sizes to
/// [`Self::preferred_size`] (constrained by the incoming
/// [`BoxConstraints`]).
// NOT `Clone`: this render object holds live per-node lifecycle state (a
// `RenderInvalidationHandle` bound to its `RenderId` plus repaint-`Listenable`
// subscription ids). Cloning would duplicate ids the clone does not own,
// so — like `RenderAnimatedSize` (the ADR-0013 sibling) — it is move-only.
#[derive(Debug)]
pub struct RenderCustomPaint {
    /// Painted behind the child.
    painter: Option<Arc<dyn CustomPainter>>,
    /// Painted in front of the child.
    foreground_painter: Option<Arc<dyn CustomPainter>>,
    /// Size used when there is no child.
    preferred_size: Size,
    /// Raster-cache "this layer is complex" hint — carried, not yet wired.
    is_complex: bool,
    /// Raster-cache "this layer will change" hint — carried, not yet wired.
    will_change: bool,
    /// Whether we have a child (tracked for paint/hit-test gating).
    has_child: bool,
    /// Self-dirty handle, held between [`RenderBox::attach`] and
    /// [`RenderBox::detach`] so a painter's repaint `Listenable` can mark this
    /// node needing paint (ADR-0013). `None` while detached.
    render_invalidation_handle: Option<RenderInvalidationHandle>,
    /// Active `add_listener` id on `painter`'s repaint listenable, torn down
    /// in `detach` and on a background-painter swap.
    painter_listener: Option<ListenerId>,
    /// Active `add_listener` id on `foreground_painter`'s repaint listenable.
    foreground_listener: Option<ListenerId>,
}

impl RenderCustomPaint {
    /// Creates a custom-paint proxy with the given painters and childless
    /// preferred size.
    #[must_use]
    pub fn new(
        painter: Option<Arc<dyn CustomPainter>>,
        foreground_painter: Option<Arc<dyn CustomPainter>>,
        preferred_size: Size,
    ) -> Self {
        Self {
            painter,
            foreground_painter,
            preferred_size,
            is_complex: false,
            will_change: false,
            has_child: false,
            render_invalidation_handle: None,
            painter_listener: None,
            foreground_listener: None,
        }
    }

    /// Subscribes `painter`'s repaint [`Listenable`](flui_foundation::Listenable)
    /// (if any) to this node's
    /// self-dirty handle, so a notify marks the node needing paint. Returns
    /// the subscription id, or `None` when detached or the painter has no
    /// repaint listenable.
    fn subscribe(&self, painter: Option<&Arc<dyn CustomPainter>>) -> Option<ListenerId> {
        let handle = self.render_invalidation_handle.as_ref()?;
        let listenable = painter?.repaint()?;
        let mark = handle.clone();
        Some(listenable.add_listener(Arc::new(move || {
            // A stale handle (node removed) is a silent no-op by design.
            let _ = mark.mark_needs_paint();
        })))
    }

    /// Tears down a subscription created by [`Self::subscribe`], removing it
    /// from the *same* painter's repaint listenable it was added to.
    fn unsubscribe(painter: Option<&Arc<dyn CustomPainter>>, id: Option<ListenerId>) {
        if let (Some(painter), Some(id)) = (painter, id)
            && let Some(listenable) = painter.repaint()
        {
            listenable.remove_listener(id);
        }
    }

    /// Hints that this layer's painting is complex enough to benefit from
    /// raster caching.
    ///
    /// Carried for Flutter constructor-shape parity (`RenderCustomPaint`'s
    /// `isComplex` is a plain field with no custom setter) but currently
    /// inert: FLUI's [`PaintCx`] has no raster-cache-hint API to forward it
    /// to. See the module docs for the full deferred list.
    #[must_use]
    pub fn with_is_complex(mut self, is_complex: bool) -> Self {
        self.is_complex = is_complex;
        self
    }

    /// Hints that this layer's painting will change on the next frame,
    /// discouraging raster caching. See [`Self::with_is_complex`] for why
    /// this hint currently has no effect.
    #[must_use]
    pub fn with_will_change(mut self, will_change: bool) -> Self {
        self.will_change = will_change;
        self
    }

    /// The background painter, if any.
    #[must_use]
    pub fn painter(&self) -> Option<&Arc<dyn CustomPainter>> {
        self.painter.as_ref()
    }

    /// The foreground painter, if any.
    #[must_use]
    pub fn foreground_painter(&self) -> Option<&Arc<dyn CustomPainter>> {
        self.foreground_painter.as_ref()
    }

    /// The size used when this proxy has no child.
    #[must_use]
    pub fn preferred_size(&self) -> Size {
        self.preferred_size
    }

    /// Whether the raster-cache "is complex" hint was requested (see
    /// [`Self::with_is_complex`]).
    #[must_use]
    pub fn is_complex(&self) -> bool {
        self.is_complex
    }

    /// Whether the raster-cache "will change" hint was requested (see
    /// [`Self::with_is_complex`]).
    #[must_use]
    pub fn will_change(&self) -> bool {
        self.will_change
    }

    /// Replaces the background painter.
    ///
    /// Returns the independently evaluated paint and semantics work required
    /// by Flutter's `_didUpdatePainter` contract. Absence and concrete-type
    /// transitions require both; same-type replacements consult
    /// [`CustomPainter::should_repaint`] and
    /// [`CustomPainter::should_rebuild_semantics`] separately.
    pub fn set_painter(
        &mut self,
        painter: Option<Arc<dyn CustomPainter>>,
    ) -> flui_rendering::RenderUpdateImpact {
        let impact = painter_update_impact(self.painter.as_ref(), painter.as_ref());
        // Migrate the repaint subscription to the new painter (no-op while
        // detached: `unsubscribe` skips a `None` id and `subscribe` returns
        // `None` without a handle).
        Self::unsubscribe(self.painter.as_ref(), self.painter_listener.take());
        self.painter = painter;
        self.painter_listener = self.subscribe(self.painter.as_ref());
        impact
    }

    /// Replaces the foreground painter. See [`Self::set_painter`] for the
    /// change-detection rule.
    pub fn set_foreground_painter(
        &mut self,
        painter: Option<Arc<dyn CustomPainter>>,
    ) -> flui_rendering::RenderUpdateImpact {
        let impact = painter_update_impact(self.foreground_painter.as_ref(), painter.as_ref());
        Self::unsubscribe(
            self.foreground_painter.as_ref(),
            self.foreground_listener.take(),
        );
        self.foreground_painter = painter;
        self.foreground_listener = self.subscribe(self.foreground_painter.as_ref());
        impact
    }

    /// Replaces the preferred size used when childless.
    ///
    /// Returns `LAYOUT` when the value actually changed, otherwise `NONE`.
    pub fn set_preferred_size(
        &mut self,
        preferred_size: Size,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.preferred_size == preferred_size {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.preferred_size = preferred_size;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }
}

/// Flutter `_didUpdatePainter` (oracle L450-469), with paint and semantics
/// decisions kept independent for same-type delegates.
fn painter_update_impact(
    old: Option<&Arc<dyn CustomPainter>>,
    new: Option<&Arc<dyn CustomPainter>>,
) -> flui_rendering::RenderUpdateImpact {
    match (old, new) {
        (None, None) => flui_rendering::RenderUpdateImpact::NONE,
        (None, Some(_)) | (Some(_), None) => {
            flui_rendering::RenderUpdateImpact::PAINT
                | flui_rendering::RenderUpdateImpact::SEMANTICS
        }
        (Some(old), Some(new)) if Arc::ptr_eq(old, new) => flui_rendering::RenderUpdateImpact::NONE,
        (Some(old), Some(new)) if old.as_any().type_id() != new.as_any().type_id() => {
            flui_rendering::RenderUpdateImpact::PAINT
                | flui_rendering::RenderUpdateImpact::SEMANTICS
        }
        (Some(old), Some(new)) => {
            let mut impact = flui_rendering::RenderUpdateImpact::NONE;
            if new.should_repaint(old.as_ref()) {
                impact |= flui_rendering::RenderUpdateImpact::PAINT;
            }
            if new.should_rebuild_semantics(old.as_ref()) {
                impact |= flui_rendering::RenderUpdateImpact::SEMANTICS;
            }
            impact
        }
    }
}

/// Runs `painter.paint(canvas, size)` inside a balanced `save()`/`restore()`
/// pair (Flutter `RenderCustomPaint._paintWithPainter`, oracle L583-636).
///
/// No offset translation: the fragment recorder pre-translates `canvas` to
/// this node's local origin before paint runs (unlike the oracle, which
/// paints in the parent's coordinate space and translates explicitly).
fn paint_with_painter(canvas: &mut Canvas, size: Size, painter: &dyn CustomPainter) {
    canvas.save();
    let save_count = canvas.save_count();
    painter.paint(canvas, size);
    debug_assert_eq!(
        canvas.save_count(),
        save_count,
        "{painter:?} must pair every canvas.save()/save_layer() with a \
         matching restore() before paint() returns",
    );
    canvas.restore();
}

/// Childless intrinsic answer for one axis: the preferred extent when
/// finite, else `0.0` (Flutter `computeMinIntrinsicWidth` et al., oracle
/// L513-543 — the same formula serves min and max on both axes).
fn finite_extent_or_zero(extent: f64) -> f64 {
    if extent.is_finite() { extent } else { 0.0 }
}

impl flui_foundation::Diagnosticable for RenderCustomPaint {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add_flag("has_painter", self.painter.is_some(), "has painter");
        properties.add_flag(
            "has_foreground_painter",
            self.foreground_painter.is_some(),
            "has foreground painter",
        );
        properties.add_size(
            "preferred_size",
            self.preferred_size.width,
            self.preferred_size.height,
        );
        properties.add_flag("is_complex", self.is_complex, "is complex");
        properties.add_flag("will_change", self.will_change, "will change");
        // `CustomPainter: Debug` is already a supertrait bound (every
        // implementer must derive/implement it), so this reuses an
        // already-required impl rather than adding a new trait method —
        // lets a mounted-widget test read a concrete painter's resolved
        // field values back out (e.g. a themed color reaching the real
        // painter instance a build produced) without this render object
        // needing to know anything about what a painter actually paints.
        properties.add("painter", format!("{:?}", self.painter));
        properties.add(
            "foreground_painter",
            format!("{:?}", self.foreground_painter),
        );
    }
}

impl RenderBox for RenderCustomPaint {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, Self::ParentData>) -> Size {
        let constraints = *ctx.constraints();
        if ctx.child_count() > 0 {
            self.has_child = true;
            let child_size = ctx.layout_child(0, constraints);
            ctx.position_child(0, Offset::ZERO);
            child_size
        } else {
            self.has_child = false;
            constraints.constrain(self.preferred_size)
        }
    }

    fn compute_min_intrinsic_width(&self, height: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        if ctx.child_count() > 0 {
            ctx.child_min_intrinsic_width(0, height)
        } else {
            finite_extent_or_zero(self.preferred_size.width)
        }
    }

    fn compute_max_intrinsic_width(&self, height: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        if ctx.child_count() > 0 {
            ctx.child_max_intrinsic_width(0, height)
        } else {
            finite_extent_or_zero(self.preferred_size.width)
        }
    }

    fn compute_min_intrinsic_height(&self, width: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        if ctx.child_count() > 0 {
            ctx.child_min_intrinsic_height(0, width)
        } else {
            finite_extent_or_zero(self.preferred_size.height)
        }
    }

    fn compute_max_intrinsic_height(&self, width: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        if ctx.child_count() > 0 {
            ctx.child_max_intrinsic_height(0, width)
        } else {
            finite_extent_or_zero(self.preferred_size.height)
        }
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        if ctx.child_count() > 0 {
            ctx.child_dry_layout(0, constraints)
        } else {
            constraints.constrain(self.preferred_size)
        }
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        let size = ctx.size();
        if let Some(background) = &self.painter {
            paint_with_painter(ctx.canvas(), size, background.as_ref());
        }
        ctx.paint_child();
        if let Some(foreground) = &self.foreground_painter {
            paint_with_painter(ctx.canvas(), size, foreground.as_ref());
        }
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, Self::ParentData>) -> bool {
        // Flutter order (oracle L559-570): bounds gate, then foreground →
        // child → background, each painter's `None` falling back to its
        // documented default (foreground misses by default, background
        // hits by default).
        if !ctx.is_within_own_size() {
            return false;
        }
        let position = *ctx.position();
        if let Some(foreground) = &self.foreground_painter
            && foreground.hit_test(position).unwrap_or(false)
        {
            return true;
        }
        if self.has_child && ctx.hit_test_child_at_offset(0, Offset::ZERO) {
            return true;
        }
        match &self.painter {
            Some(background) => background.hit_test(position).unwrap_or(true),
            None => false,
        }
    }

    /// Subscribes both painters' repaint listenables (ADR-0013): a notify from
    /// a painter's `repaint()` listenable now marks this node needing paint,
    /// so an animation-driven painter repaints without a widget rebuild.
    fn attach(&mut self, handle: RenderInvalidationHandle) {
        self.render_invalidation_handle = Some(handle);
        self.painter_listener = self.subscribe(self.painter.as_ref());
        self.foreground_listener = self.subscribe(self.foreground_painter.as_ref());
    }

    /// Tears down both repaint subscriptions and drops the self-dirty handle.
    fn detach(&mut self) {
        Self::unsubscribe(self.painter.as_ref(), self.painter_listener.take());
        Self::unsubscribe(
            self.foreground_painter.as_ref(),
            self.foreground_listener.take(),
        );
        self.render_invalidation_handle = None;
    }
}
