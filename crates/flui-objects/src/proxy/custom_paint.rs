//! `RenderCustomPaint` — delegates painting (and optionally hit testing) to
//! user-supplied [`CustomPainter`]s around a single child.
//!
//! Paint order is background painter → child → foreground painter; hit-test
//! order is foreground → child → background. Sizing: to the child when
//! present, else `constraints.constrain(preferred_size)`.
//!
//! Repaint wiring (a painter's listenable driving `mark_needs_paint`) is
//! implemented via ADR-0013: [`CustomPainter::repaint`]
//! returns an optional [`Listenable`](flui_foundation::Listenable) that
//! [`RenderBox::attach`] subscribes to
//! (marking this node needing paint on notify) and [`RenderBox::detach`] tears
//! down; a painter swap migrates the subscription.

use std::rc::Rc;

use flui_foundation::Single;
use flui_foundation::geometry::{Offset, Size};
use flui_foundation::panic::PanicRecovery;
use flui_painting::Canvas;

use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext, PaintCx},
    delegates::CustomPainter,
    parent_data::BoxParentData,
    pipeline::LocalRenderInvalidationHandle,
    traits::RenderBox,
};

use crate::subscription::RenderSubscription;

/// A render object that paints custom graphics before and/or after its
/// child via user-supplied [`CustomPainter`] delegates.
///
/// Sizes to its child when one is present; otherwise sizes to
/// [`Self::preferred_size`] (constrained by the incoming
/// [`BoxConstraints`]).
// NOT `Clone`: this render object holds live per-node lifecycle state (a
// `LocalRenderInvalidationHandle` bound to its `RenderId` plus repaint-`Listenable`
// subscription ids). Cloning would duplicate ids the clone does not own,
// so — like `RenderAnimatedSize` (the ADR-0013 sibling) — it is move-only.
#[derive(Debug)]
pub struct RenderCustomPaint {
    /// Painted behind the child.
    painter: Option<Rc<dyn CustomPainter>>,
    /// Painted in front of the child.
    foreground_painter: Option<Rc<dyn CustomPainter>>,
    /// Size used when there is no child.
    preferred_size: Size,
    /// Whether we have a child (tracked for paint/hit-test gating).
    has_child: bool,
    /// Self-dirty handle, held between [`RenderBox::attach`] and
    /// [`RenderBox::detach`] so a painter's repaint `Listenable` can mark this
    /// node needing paint (ADR-0013). `None` while detached.
    render_invalidation_handle: Option<LocalRenderInvalidationHandle>,
    /// Active `add_listener` id on `painter`'s repaint listenable, torn down
    /// in `detach` and on a background-painter swap.
    painter_listener: Option<RenderSubscription>,
    /// Active `add_listener` id on `foreground_painter`'s repaint listenable.
    foreground_listener: Option<RenderSubscription>,
}

impl RenderCustomPaint {
    /// Creates a custom-paint proxy with the given painters and childless
    /// preferred size.
    #[must_use]
    pub fn new(
        painter: Option<Rc<dyn CustomPainter>>,
        foreground_painter: Option<Rc<dyn CustomPainter>>,
        preferred_size: Size,
    ) -> Self {
        Self {
            painter,
            foreground_painter,
            preferred_size,
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
    fn subscribe(
        painter: Option<&Rc<dyn CustomPainter>>,
        handle: Option<&LocalRenderInvalidationHandle>,
        recovery: &mut PanicRecovery,
    ) -> Option<RenderSubscription> {
        let painter = painter?;
        RenderSubscription::subscribe(
            || painter.repaint(),
            handle,
            flui_rendering::RenderUpdateImpact::PAINT
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
            recovery,
        )
    }

    /// Tears down a subscription created by [`Self::subscribe`], removing it
    /// from the *same* painter's repaint listenable it was added to.
    fn unsubscribe(subscription: Option<RenderSubscription>, recovery: &mut PanicRecovery) {
        if let Some(subscription) = subscription {
            subscription.close(recovery);
        }
    }

    /// The background painter, if any.
    #[must_use]
    pub fn painter(&self) -> Option<&Rc<dyn CustomPainter>> {
        self.painter.as_ref()
    }

    /// The foreground painter, if any.
    #[must_use]
    pub fn foreground_painter(&self) -> Option<&Rc<dyn CustomPainter>> {
        self.foreground_painter.as_ref()
    }

    /// The size used when this proxy has no child.
    #[must_use]
    pub fn preferred_size(&self) -> Size {
        self.preferred_size
    }

    /// Replaces the background painter.
    ///
    /// Returns the independently evaluated paint and semantics work required
    /// by a painter swap. Absence and concrete-type
    /// transitions require both; same-type replacements consult
    /// [`CustomPainter::should_repaint`] and
    /// [`CustomPainter::should_rebuild_semantics`] separately.
    pub fn set_painter(
        &mut self,
        painter: Option<Rc<dyn CustomPainter>>,
    ) -> flui_rendering::RenderUpdateImpact {
        Self::replace_painter(
            &mut self.painter,
            &mut self.painter_listener,
            painter,
            self.render_invalidation_handle.as_ref(),
        )
    }

    /// Replaces the foreground painter. See [`Self::set_painter`] for the
    /// change-detection rule.
    pub fn set_foreground_painter(
        &mut self,
        painter: Option<Rc<dyn CustomPainter>>,
    ) -> flui_rendering::RenderUpdateImpact {
        Self::replace_painter(
            &mut self.foreground_painter,
            &mut self.foreground_listener,
            painter,
            self.render_invalidation_handle.as_ref(),
        )
    }

    fn replace_painter(
        current: &mut Option<Rc<dyn CustomPainter>>,
        subscription: &mut Option<RenderSubscription>,
        painter: Option<Rc<dyn CustomPainter>>,
        handle: Option<&LocalRenderInvalidationHandle>,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut recovery = PanicRecovery::new();
        let mut impact = None;
        recovery.run(|| impact = Some(painter_update_impact(current.as_ref(), painter.as_ref())));
        let Some(impact) = impact else {
            recovery.retire(painter);
            recovery.finish();
            return flui_rendering::RenderUpdateImpact::NONE;
        };
        let outgoing = std::mem::replace(current, painter);
        let outgoing_subscription = subscription.take();
        Self::unsubscribe(outgoing_subscription, &mut recovery);
        *subscription = Self::subscribe(current.as_ref(), handle, &mut recovery);
        recovery.retire(outgoing);
        if recovery.has_failure()
            && let Some(handle) = handle
        {
            recovery.run(|| {
                let _ = handle.request_update(impact);
            });
        }
        recovery.finish();
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

/// The impact of a painter swap, with paint and semantics decisions kept
/// independent for same-type delegates.
fn painter_update_impact(
    old: Option<&Rc<dyn CustomPainter>>,
    new: Option<&Rc<dyn CustomPainter>>,
) -> flui_rendering::RenderUpdateImpact {
    match (old, new) {
        (None, None) => flui_rendering::RenderUpdateImpact::NONE,
        (None, Some(_)) | (Some(_), None) => {
            flui_rendering::RenderUpdateImpact::PAINT
                | flui_rendering::RenderUpdateImpact::SEMANTICS
        }
        (Some(old), Some(new)) if Rc::ptr_eq(old, new) => flui_rendering::RenderUpdateImpact::NONE,
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
/// pair.
///
/// No offset translation: the fragment recorder pre-translates `canvas` to
/// this node's local origin before paint runs.
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
/// finite, else `0.0` (the same formula serves min and max on both axes).
fn finite_extent_or_zero(extent: f64) -> f64 {
    if extent.is_finite() { extent } else { 0.0 }
}

impl flui_foundation::inspect::Inspect for RenderCustomPaint {
    fn kind(&self) -> &'static str {
        "RenderCustomPaint"
    }

    fn inspect(&self, out: &mut flui_foundation::inspect::Fields<'_>) {
        use flui_foundation::inspect::{Unit, Value};
        out.field("has_painter", &self.painter.is_some())
            .default(&false);
        out.field("has_foreground_painter", &self.foreground_painter.is_some())
            .default(&false);
        // An infinite extent means "as much as the parent allows".
        let size = self.preferred_size;
        out.field("preferred_width", &size.width)
            .unit(Unit::LogicalPx);
        out.field("preferred_height", &size.height)
            .unit(Unit::LogicalPx);
        // `CustomPainter: Debug` is already a supertrait bound (every
        // implementer must derive/implement it), so this reuses an
        // already-required impl rather than adding a new trait method —
        // lets a mounted-widget test read a concrete painter's resolved
        // field values back out (e.g. a themed color reaching the real
        // painter instance a build produced) without this render object
        // needing to know anything about what a painter actually paints.
        out.value("painter", Value::Debug(&self.painter));
        out.value("foreground_painter", Value::Debug(&self.foreground_painter));
    }
}

impl RenderBox for RenderCustomPaint {
    type Arity = Single;
    type ParentData = BoxParentData;

    flui_rendering::forward_single_child_baselines!();

    fn semantics_regions(&self, size: Size) -> flui_rendering::semantics::SemanticsRegions {
        flui_rendering::semantics::SemanticsRegions {
            before_children: self
                .painter
                .as_ref()
                .map_or_else(Vec::new, |p| p.semantics_regions(size)),
            after_children: self
                .foreground_painter
                .as_ref()
                .map_or_else(Vec::new, |p| p.semantics_regions(size)),
        }
    }

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Single, Self::ParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        let constraints = *ctx.constraints();
        if ctx.child_count() > 0 {
            self.has_child = true;
            let child_size = ctx.layout_child(0, constraints)?;
            ctx.position_child(0, Offset::ZERO);
            Ok(child_size)
        } else {
            self.has_child = false;
            Ok(constraints.constrain(self.preferred_size))
        }
    }

    fn compute_min_intrinsic_width(
        &self,
        height: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        Ok(if ctx.child_count() > 0 {
            ctx.child_min_intrinsic_width(0, height)?
        } else {
            finite_extent_or_zero(self.preferred_size.width)
        })
    }

    fn compute_max_intrinsic_width(
        &self,
        height: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        Ok(if ctx.child_count() > 0 {
            ctx.child_max_intrinsic_width(0, height)?
        } else {
            finite_extent_or_zero(self.preferred_size.width)
        })
    }

    fn compute_min_intrinsic_height(
        &self,
        width: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        Ok(if ctx.child_count() > 0 {
            ctx.child_min_intrinsic_height(0, width)?
        } else {
            finite_extent_or_zero(self.preferred_size.height)
        })
    }

    fn compute_max_intrinsic_height(
        &self,
        width: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        Ok(if ctx.child_count() > 0 {
            ctx.child_max_intrinsic_height(0, width)?
        } else {
            finite_extent_or_zero(self.preferred_size.height)
        })
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> flui_rendering::RenderResult<Size> {
        if ctx.child_count() > 0 {
            ctx.child_dry_layout(0, constraints)
        } else {
            Ok(constraints.constrain(self.preferred_size))
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
        // Order: bounds gate, then foreground →
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
    fn attach(&mut self, handle: LocalRenderInvalidationHandle) {
        self.render_invalidation_handle = Some(handle);
        let mut recovery = PanicRecovery::new();
        self.painter_listener = Self::subscribe(
            self.painter.as_ref(),
            self.render_invalidation_handle.as_ref(),
            &mut recovery,
        );
        self.foreground_listener = Self::subscribe(
            self.foreground_painter.as_ref(),
            self.render_invalidation_handle.as_ref(),
            &mut recovery,
        );
        recovery.finish();
    }

    /// Tears down both repaint subscriptions and drops the self-dirty handle.
    fn detach(&mut self) {
        self.render_invalidation_handle = None;
        let background = self.painter_listener.take();
        let foreground = self.foreground_listener.take();
        let mut recovery = PanicRecovery::new();
        Self::unsubscribe(background, &mut recovery);
        Self::unsubscribe(foreground, &mut recovery);
        recovery.finish();
    }
}

impl Drop for RenderCustomPaint {
    fn drop(&mut self) {
        self.render_invalidation_handle = None;
        let background_subscription = self.painter_listener.take();
        let foreground_subscription = self.foreground_listener.take();
        let background = self.painter.take();
        let foreground = self.foreground_painter.take();
        let mut recovery = PanicRecovery::new();
        Self::unsubscribe(background_subscription, &mut recovery);
        Self::unsubscribe(foreground_subscription, &mut recovery);
        recovery.retire(background);
        recovery.retire(foreground);
        recovery.finish();
    }
}
