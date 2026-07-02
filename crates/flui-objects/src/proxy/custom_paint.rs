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
use flui_painting::Canvas;
use flui_tree::Single;
use flui_types::{Offset, Pixels, Size};

use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext, PaintCx},
    delegates::CustomPainter,
    parent_data::BoxParentData,
    pipeline::RepaintHandle,
    traits::RenderBox,
};

/// A render object that paints custom graphics before and/or after its
/// child via user-supplied [`CustomPainter`] delegates.
///
/// Sizes to its child when one is present; otherwise sizes to
/// [`Self::preferred_size`] (constrained by the incoming
/// [`BoxConstraints`]).
// NOT `Clone`: this render object holds live per-node lifecycle state (a
// `RepaintHandle` bound to its `RenderId` plus repaint-`Listenable`
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
    repaint_handle: Option<RepaintHandle>,
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
            repaint_handle: None,
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
        let handle = self.repaint_handle.as_ref()?;
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
    /// Returns `true` when the swap requires a repaint: `None` ↔ `Some` in
    /// either direction, or a `Some → Some` swap whose
    /// [`CustomPainter::should_repaint`] reports a difference (Flutter
    /// `RenderCustomPaint._didUpdatePainter`'s repaint half, oracle
    /// L450-459). Paint-only state: the caller is responsible for the
    /// repaint mark.
    pub fn set_painter(&mut self, painter: Option<Arc<dyn CustomPainter>>) -> bool {
        let changed = painter_changed(self.painter.as_deref(), painter.as_deref());
        // Migrate the repaint subscription to the new painter (no-op while
        // detached: `unsubscribe` skips a `None` id and `subscribe` returns
        // `None` without a handle).
        Self::unsubscribe(self.painter.as_ref(), self.painter_listener.take());
        self.painter = painter;
        self.painter_listener = self.subscribe(self.painter.as_ref());
        changed
    }

    /// Replaces the foreground painter. See [`Self::set_painter`] for the
    /// change-detection rule.
    pub fn set_foreground_painter(&mut self, painter: Option<Arc<dyn CustomPainter>>) -> bool {
        let changed = painter_changed(self.foreground_painter.as_deref(), painter.as_deref());
        Self::unsubscribe(
            self.foreground_painter.as_ref(),
            self.foreground_listener.take(),
        );
        self.foreground_painter = painter;
        self.foreground_listener = self.subscribe(self.foreground_painter.as_ref());
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

    /// Subscribes both painters' repaint listenables (ADR-0013): a notify from
    /// a painter's `repaint()` listenable now marks this node needing paint,
    /// so an animation-driven painter repaints without a widget rebuild.
    fn attach(&mut self, handle: RepaintHandle) {
        self.repaint_handle = Some(handle);
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
        self.repaint_handle = None;
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

    /// A painter whose `repaint()` returns a caller-controlled
    /// [`ChangeNotifier`](flui_foundation::ChangeNotifier), so a test can fire
    /// a notify and observe the resulting paint mark.
    #[derive(Debug)]
    struct RepaintingPainter {
        repaint: Arc<flui_foundation::ChangeNotifier>,
    }

    impl CustomPainter for RepaintingPainter {
        fn paint(&self, _canvas: &mut Canvas, _size: Size) {}

        fn should_repaint(&self, _old_delegate: &dyn CustomPainter) -> bool {
            false
        }

        fn repaint(&self) -> Option<Arc<dyn flui_foundation::Listenable>> {
            Some(self.repaint.clone() as Arc<dyn flui_foundation::Listenable>)
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
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
    fn repaint_listenable_marks_needs_paint_on_notify() {
        use flui_foundation::ChangeNotifier;
        use flui_rendering::pipeline::PipelineOwner;
        use flui_rendering::protocol::BoxProtocol;
        use flui_rendering::traits::RenderObject;

        let notifier = Arc::new(ChangeNotifier::new());
        let painter = Arc::new(RepaintingPainter {
            repaint: notifier.clone(),
        });
        let node = RenderCustomPaint::new(Some(painter), None, Size::ZERO);

        let mut owner = PipelineOwner::new();
        // `insert` fires the real `attach(handle)`, subscribing to the
        // painter's repaint listenable with a live handle from the pipeline.
        let id = owner.insert(Box::new(node) as Box<dyn RenderObject<BoxProtocol>>);

        // A fresh node is paint-dirty by default; clear so the notify's mark
        // is isolated (this is the red→green discriminator — without the
        // attach-subscribe wiring, the node stays clean below).
        owner.clear_all_dirty_nodes();
        assert!(
            !owner.nodes_needing_paint().iter().any(|d| d.id == id),
            "precondition: node must be clean after clear_all_dirty_nodes",
        );

        // Firing the painter's repaint listenable must re-mark the node.
        notifier.notify_listeners();
        owner.drain_pending_dirty();
        assert!(
            owner.nodes_needing_paint().iter().any(|d| d.id == id),
            "a notify on the painter's repaint listenable must mark the node \
             needing paint (ADR-0013 attach-subscribe wiring)",
        );
    }

    #[test]
    fn set_painter_while_detached_registers_no_subscription() {
        // Without `attach` there is no self-dirty handle, so a painter with a
        // repaint listenable must not register a subscription (no leak, no
        // mark against a node that isn't in the tree).
        let notifier = Arc::new(flui_foundation::ChangeNotifier::new());
        let mut node = RenderCustomPaint::new(None, None, Size::ZERO);
        node.set_painter(Some(Arc::new(RepaintingPainter { repaint: notifier })));
        assert!(
            node.painter_listener.is_none(),
            "no subscription may exist while detached",
        );
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
