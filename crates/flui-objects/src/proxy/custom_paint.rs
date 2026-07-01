//! `RenderCustomPaint` — delegates painting (and optionally hit testing) to
//! user-supplied [`CustomPainter`]s around a single child.
//!
//! Flutter parity: `rendering/custom_paint.dart` `RenderCustomPaint`. Paint
//! order is background painter → child → foreground painter; hit-test order
//! is foreground → child → background (oracle L559-570). Sizing: to the
//! child when present, else `constraints.constrain(preferred_size)` (oracle
//! `computeSizeForNoChild`, L579).
//!
//! Deferred vs. the oracle (documented, not silently dropped): the
//! `Listenable` repaint wiring (`CustomPainter.addListener`/`removeListener`
//! driving `markNeedsPaint`), `semanticsBuilder`, and the `isComplex`/
//! `willChange` raster-cache hints have no FLUI-side plumbing yet — FLUI's
//! [`PaintCx`] has no `setIsComplexHint`/`setWillChangeHint` equivalent. The
//! two hint fields are carried on this type for Flutter-shape parity but are
//! currently inert.

use std::sync::Arc;

use flui_painting::Canvas;
use flui_tree::Single;
use flui_types::{Offset, Pixels, Size};

use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext, PaintCx},
    delegates::CustomPainter,
    parent_data::BoxParentData,
    traits::RenderBox,
};

/// A render object that paints custom graphics before and/or after its
/// child via user-supplied [`CustomPainter`] delegates.
///
/// Sizes to its child when one is present; otherwise sizes to
/// [`Self::preferred_size`] (constrained by the incoming
/// [`BoxConstraints`]).
#[derive(Debug, Clone)]
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
    /// Returns `true` when the swap requires a repaint: `None` ↔ `Some` in
    /// either direction, or a `Some → Some` swap whose
    /// [`CustomPainter::should_repaint`] reports a difference (Flutter
    /// `RenderCustomPaint._didUpdatePainter`'s repaint half, oracle
    /// L450-459). Paint-only state: the caller is responsible for the
    /// repaint mark.
    pub fn set_painter(&mut self, painter: Option<Arc<dyn CustomPainter>>) -> bool {
        let changed = painter_changed(self.painter.as_deref(), painter.as_deref());
        self.painter = painter;
        changed
    }

    /// Replaces the foreground painter. See [`Self::set_painter`] for the
    /// change-detection rule.
    pub fn set_foreground_painter(&mut self, painter: Option<Arc<dyn CustomPainter>>) -> bool {
        let changed = painter_changed(self.foreground_painter.as_deref(), painter.as_deref());
        self.foreground_painter = painter;
        changed
    }

    /// Replaces the preferred size used when childless.
    ///
    /// Returns `true` when the value actually changed — layout-affecting
    /// state, so the pipeline should invalidate layout in that case
    /// (mirrors [`crate::RenderConstrainedBox::set_additional_constraints`]'s
    /// bool-return convention).
    pub fn set_preferred_size(&mut self, preferred_size: Size) -> bool {
        if self.preferred_size == preferred_size {
            return false;
        }
        self.preferred_size = preferred_size;
        true
    }
}

/// Flutter `_didUpdatePainter`'s repaint-decision half (oracle L450-459):
/// `None` ↔ `Some` in either direction is always a change; a `Some → Some`
/// swap defers to [`CustomPainter::should_repaint`].
fn painter_changed(old: Option<&dyn CustomPainter>, new: Option<&dyn CustomPainter>) -> bool {
    match (old, new) {
        (None, None) => false,
        (None, Some(_)) | (Some(_), None) => true,
        (Some(old), Some(new)) => new.should_repaint(old),
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
fn finite_extent_or_zero(extent: Pixels) -> f32 {
    if extent.is_finite() {
        extent.get()
    } else {
        0.0
    }
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

    fn compute_min_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() > 0 {
            ctx.child_min_intrinsic_width(0, height)
        } else {
            finite_extent_or_zero(self.preferred_size.width)
        }
    }

    fn compute_max_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() > 0 {
            ctx.child_max_intrinsic_width(0, height)
        } else {
            finite_extent_or_zero(self.preferred_size.width)
        }
    }

    fn compute_min_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() > 0 {
            ctx.child_min_intrinsic_height(0, width)
        } else {
            finite_extent_or_zero(self.preferred_size.height)
        }
    }

    fn compute_max_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
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
}

#[cfg(test)]
mod tests {
    use std::any::Any;

    use flui_rendering::context::intrinsics_test_support::{leaf_dry_layout, leaf_intrinsics};
    use flui_types::geometry::px;

    use super::*;

    #[derive(Debug)]
    struct StubPainter {
        tag: &'static str,
    }

    impl CustomPainter for StubPainter {
        fn paint(&self, _canvas: &mut Canvas, _size: Size) {}

        fn should_repaint(&self, old_delegate: &dyn CustomPainter) -> bool {
            match old_delegate.as_any().downcast_ref::<Self>() {
                Some(old) => old.tag != self.tag,
                None => true,
            }
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    #[derive(Debug)]
    struct OtherPainter;

    impl CustomPainter for OtherPainter {
        fn paint(&self, _canvas: &mut Canvas, _size: Size) {}

        fn should_repaint(&self, _old_delegate: &dyn CustomPainter) -> bool {
            true
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    fn stub(tag: &'static str) -> Arc<dyn CustomPainter> {
        Arc::new(StubPainter { tag })
    }

    #[test]
    fn set_painter_none_to_some_reports_changed() {
        let mut node = RenderCustomPaint::new(None, None, Size::ZERO);
        assert!(node.set_painter(Some(stub("a"))));
    }

    #[test]
    fn set_painter_some_to_none_reports_changed() {
        let mut node = RenderCustomPaint::new(Some(stub("a")), None, Size::ZERO);
        assert!(node.set_painter(None));
    }

    #[test]
    fn set_painter_same_type_defers_to_should_repaint() {
        let mut node = RenderCustomPaint::new(Some(stub("a")), None, Size::ZERO);
        // Same tag -> StubPainter::should_repaint returns false.
        assert!(!node.set_painter(Some(stub("a"))));
        // Different tag -> should_repaint returns true.
        assert!(node.set_painter(Some(stub("b"))));
    }

    #[test]
    fn set_painter_type_change_always_repaints() {
        let mut node = RenderCustomPaint::new(Some(stub("a")), None, Size::ZERO);
        assert!(node.set_painter(Some(Arc::new(OtherPainter))));
    }

    #[test]
    fn set_painter_none_to_none_reports_unchanged() {
        let mut node = RenderCustomPaint::new(None, None, Size::ZERO);
        assert!(!node.set_painter(None));
    }

    #[test]
    fn set_preferred_size_reports_change_flag() {
        let mut node = RenderCustomPaint::new(None, None, Size::ZERO);
        assert!(node.set_preferred_size(Size::new(px(10.0), px(10.0))));
        assert!(!node.set_preferred_size(Size::new(px(10.0), px(10.0))));
    }

    #[test]
    fn dry_layout_childless_constrains_to_preferred_size() {
        let node = RenderCustomPaint::new(None, None, Size::new(px(20.0), px(30.0)));
        let constraints = BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0));
        let size = leaf_dry_layout(|ctx| node.compute_dry_layout(constraints, ctx));
        assert_eq!(size, Size::new(px(20.0), px(30.0)));
    }

    #[test]
    fn dry_layout_childless_clamps_oversized_preferred_size() {
        let node = RenderCustomPaint::new(None, None, Size::new(px(2000.0), px(100.0)));
        let constraints = BoxConstraints::new(px(0.0), px(800.0), px(0.0), px(600.0));
        let size = leaf_dry_layout(|ctx| node.compute_dry_layout(constraints, ctx));
        assert_eq!(size, Size::new(px(800.0), px(100.0)));
    }

    #[test]
    fn intrinsics_childless_use_preferred_size_when_finite() {
        let node = RenderCustomPaint::new(None, None, Size::new(px(20.0), px(30.0)));
        assert_eq!(
            leaf_intrinsics(|ctx| node.compute_min_intrinsic_width(f32::INFINITY, ctx)),
            20.0
        );
        assert_eq!(
            leaf_intrinsics(|ctx| node.compute_max_intrinsic_width(f32::INFINITY, ctx)),
            20.0
        );
        assert_eq!(
            leaf_intrinsics(|ctx| node.compute_min_intrinsic_height(f32::INFINITY, ctx)),
            30.0
        );
        assert_eq!(
            leaf_intrinsics(|ctx| node.compute_max_intrinsic_height(f32::INFINITY, ctx)),
            30.0
        );
    }

    #[test]
    fn intrinsics_childless_infinite_preferred_size_reports_zero() {
        let node = RenderCustomPaint::new(None, None, Size::INFINITY);
        assert_eq!(
            leaf_intrinsics(|ctx| node.compute_min_intrinsic_width(f32::INFINITY, ctx)),
            0.0
        );
        assert_eq!(
            leaf_intrinsics(|ctx| node.compute_min_intrinsic_height(f32::INFINITY, ctx)),
            0.0
        );
    }

    #[test]
    fn with_is_complex_and_will_change_are_carried() {
        let node = RenderCustomPaint::new(None, None, Size::ZERO)
            .with_is_complex(true)
            .with_will_change(true);
        assert!(node.is_complex());
        assert!(node.will_change());
    }
}
