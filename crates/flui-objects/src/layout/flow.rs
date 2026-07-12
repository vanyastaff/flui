//! `RenderFlow` — positions children with paint-time transform matrices
//! chosen by a [`FlowDelegate`], instead of layout-time offsets.
//!
//! Flutter parity: `rendering/flow.dart` `RenderFlow`. Every other
//! multi-child render object in this crate positions children during
//! layout (`ctx.position_child`); `RenderFlow` never does — every child is
//! positioned at [`Offset::ZERO`] by layout and repositioned purely by
//! paint-time [`FlowDelegate::paint_children`] transforms, so moving
//! children costs a repaint, not a relayout (oracle L165-168).
//!
//! # Hit-testing without mutable paint state
//!
//! Flutter's `RenderFlow` mutates `FlowParentData._transform` and
//! `_lastPaintOrder` *during* `paint()` (legal there — `paint()` isn't
//! `const`) and reads them back in `hitTestChildren()`. FLUI's
//! `RenderBox::paint`/`hit_test` are both `&self`, so there is nowhere to
//! cache "what transform did paint assign to child N" for hit-test to read
//! later. This port resolves that by having [`RenderFlow::hit_test`]
//! **replay** [`FlowDelegate::paint_children`] a second time against a
//! non-drawing [`FlowPaintingContext::for_replay`] context that only
//! records `(paint_order, transforms)` — legitimate because
//! `paint_children` is contractually a pure function of the delegate's own
//! state plus child sizes (the same assumption `should_repaint`/
//! `should_relayout` already rely on). This costs one extra O(children)
//! delegate call per hit-test, not per frame.
//!
//! # ParentData
//!
//! Flutter's `FlowParentData` exists solely to cache `_transform` for the
//! hit-test readback above; since this port replays the delegate instead
//! of caching, there is nothing to add to parent data — `RenderFlow` uses
//! plain [`BoxParentData`], a deliberate simplification, not an oversight.
//!
//! # Deferred (documented, not silently dropped)
//!
//! - `FlowDelegate`'s `Listenable? repaint` + `attach`/`detach` listener
//!   wiring (oracle L64-68, L230-233, L249-259) is **implemented** via
//!   ADR-0013: [`FlowDelegate::repaint`] returns an optional `Listenable`
//!   that [`RenderBox::attach`] subscribes to (marking this node needing paint
//!   on notify) and [`RenderBox::detach`] tears down; a delegate swap migrates
//!   the subscription (mirrors `RenderCustomPaint`).
//! - `FlowPaintingContext.paintChild`'s `opacity` parameter (oracle L352,
//!   `pushOpacity` wrapping) — FLUI's `FlowDelegate::paint_children`/
//!   `paint_child(index, transform)` signature has no opacity parameter
//!   already; this is a pre-existing scope cut, not a new one.
//! - ~~`RenderObject.applyPaintTransform`/`getTransformTo`/`localToGlobal`
//!   (oracle L455-462)~~ — **landed.** `apply_paint_transform`
//!   below replays `paint_children` to recover the child's paint matrix, the way
//!   `hit_test` already does; `getTransformTo` / `localToGlobal` live on
//!   `PipelineOwner`, because a FLUI render object has no parent link.
//! - `markNeedsSemanticsUpdate` on `clip_behavior` change (oracle L245) —
//!   FLUI has no semantics tree yet, consistent with every other render
//!   object in the catalog.

use std::sync::Arc;

use flui_foundation::ListenerId;
use flui_tree::Variable;
use flui_types::{Matrix4, Offset, Pixels, Point, Rect, Size, painting::Clip};

use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext, PaintCx},
    delegates::{FlowDelegate, FlowPaintingContext},
    parent_data::BoxParentData,
    pipeline::RepaintHandle,
    traits::RenderBox,
};

/// Outcome of [`RenderFlow::set_delegate`] — whether swapping delegates
/// requires relayout, just a repaint, or neither.
///
/// Flutter's `RenderFlow.delegate` setter (oracle L216-234) checks
/// `runtimeType` explicitly, in addition to `shouldRelayout`/
/// `shouldRepaint` — unlike `RenderCustomPaint`'s simpler `_didUpdatePainter`
/// (no such check), a delegate *type* swap always relayouts, even when the
/// new delegate's own `should_relayout` would say otherwise (it has no
/// same-type old delegate to meaningfully compare against).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelegateChange {
    /// Neither layout nor paint needs to be redone.
    None,
    /// Only paint needs to be redone.
    Repaint,
    /// Layout (and therefore paint) needs to be redone.
    Relayout,
}

/// Positions children with paint-time transform matrices chosen by a
/// [`FlowDelegate`], rather than layout-time offsets.
///
/// See the module docs for why this needs no custom parent data and how
/// hit-testing works without mutable paint state.
// NOT `Clone`: holds live per-node lifecycle state (a `RepaintHandle` bound to
// its `RenderId` + a repaint-`Listenable` subscription id). Cloning would
// duplicate an id the clone does not own — move-only, like `RenderCustomPaint`
// / `RenderAnimatedSize` (the ADR-0013 siblings).
#[derive(Debug)]
pub struct RenderFlow {
    delegate: Arc<dyn FlowDelegate>,
    clip_behavior: Clip,
    /// Cached during [`RenderBox::perform_layout`]; read by
    /// [`RenderBox::paint`] and [`RenderBox::hit_test`] to hand
    /// [`FlowPaintingContext`] the child sizes without touching the
    /// (layout-only) child-layout context again.
    child_sizes: Vec<Size>,
    /// Self-dirty handle, held between [`RenderBox::attach`] and
    /// [`RenderBox::detach`] so the delegate's repaint `Listenable` can mark
    /// this node needing paint (ADR-0013). `None` while detached.
    repaint_handle: Option<RepaintHandle>,
    /// Active `add_listener` id on the delegate's repaint listenable, torn
    /// down in `detach` and migrated on a delegate swap.
    delegate_listener: Option<ListenerId>,
}

impl RenderFlow {
    /// Creates a flow render object with `clip_behavior = Clip::HardEdge`
    /// (the oracle's default, L191/L240).
    pub fn new(delegate: Arc<dyn FlowDelegate>) -> Self {
        Self {
            delegate,
            clip_behavior: Clip::HardEdge,
            child_sizes: Vec::new(),
            repaint_handle: None,
            delegate_listener: None,
        }
    }

    /// Subscribes the delegate's repaint [`Listenable`](flui_foundation::Listenable)
    /// (if any) to this node's self-dirty handle, so a notify marks the node
    /// needing paint. Returns the subscription id, or `None` when detached or
    /// the delegate has no repaint listenable.
    fn subscribe(&self) -> Option<ListenerId> {
        let handle = self.repaint_handle.as_ref()?;
        let listenable = self.delegate.repaint()?;
        let mark = handle.clone();
        Some(listenable.add_listener(Arc::new(move || {
            // A stale handle (node removed) is a silent no-op by design.
            let _ = mark.mark_needs_paint();
        })))
    }

    /// Tears down a subscription created by [`Self::subscribe`], removing it
    /// from the *same* delegate's repaint listenable it was added to.
    fn unsubscribe(delegate: &Arc<dyn FlowDelegate>, id: Option<ListenerId>) {
        if let Some(id) = id
            && let Some(listenable) = delegate.repaint()
        {
            listenable.remove_listener(id);
        }
    }

    /// Builder: overrides the default clip behavior.
    #[must_use]
    pub fn with_clip_behavior(mut self, clip_behavior: Clip) -> Self {
        self.clip_behavior = clip_behavior;
        self
    }

    /// Returns the current clip behavior.
    #[inline]
    pub fn clip_behavior(&self) -> Clip {
        self.clip_behavior
    }

    /// The oracle's `_getSize` (L261-264) — the single sizing formula
    /// reused by layout, dry layout, and all four intrinsics.
    fn get_size(&self, constraints: BoxConstraints) -> Size {
        constraints.constrain(self.delegate.get_size(constraints))
    }

    /// Shared by both width intrinsics — the oracle reuses the identical
    /// formula for `computeMinIntrinsicWidth` and `computeMaxIntrinsicWidth`
    /// (its own "dubious" TODO, L269-271: intrinsics never touch children).
    fn intrinsic_width(&self, height: f32) -> f32 {
        let width = self
            .get_size(BoxConstraints::tight_for_finite(
                Pixels::INFINITY,
                Pixels::new(height),
            ))
            .width;
        if width.is_finite() { width.get() } else { 0.0 }
    }

    /// Shared by both height intrinsics — see [`Self::intrinsic_width`].
    fn intrinsic_height(&self, width: f32) -> f32 {
        let height = self
            .get_size(BoxConstraints::tight_for_finite(
                Pixels::new(width),
                Pixels::INFINITY,
            ))
            .height;
        if height.is_finite() {
            height.get()
        } else {
            0.0
        }
    }

    /// Replaces the delegate, reporting whether the swap needs relayout,
    /// just a repaint, or neither.
    ///
    /// Mirrors the oracle's `delegate` setter (L216-234): a delegate
    /// *type* change always relayouts; otherwise `should_relayout` on the
    /// new delegate (compared against the old one) wins, falling back to
    /// `should_repaint`. The caller is responsible for actually marking
    /// the render object dirty — this is paint/layout-affecting state,
    /// not a side-effecting setter.
    pub fn set_delegate(&mut self, delegate: Arc<dyn FlowDelegate>) -> DelegateChange {
        let type_changed = self.delegate.as_any().type_id() != delegate.as_any().type_id();
        let relayout = type_changed || delegate.should_relayout(&*self.delegate);
        let repaint = !relayout && delegate.should_repaint(&*self.delegate);
        // Migrate the repaint subscription to the new delegate (no-op while
        // detached: `unsubscribe` skips a `None` id and `subscribe` returns
        // `None` without a handle).
        Self::unsubscribe(&self.delegate, self.delegate_listener.take());
        self.delegate = delegate;
        self.delegate_listener = self.subscribe();
        if relayout {
            DelegateChange::Relayout
        } else if repaint {
            DelegateChange::Repaint
        } else {
            DelegateChange::None
        }
    }

    /// Updates the clip behavior; returns `true` if the value changed.
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) -> bool {
        if self.clip_behavior == clip_behavior {
            return false;
        }
        self.clip_behavior = clip_behavior;
        true
    }
}

impl flui_foundation::Diagnosticable for RenderFlow {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        // The oracle's own `RenderFlow` does not override
        // `debugFillProperties` at all (no delegate info surfaced), so
        // clip_behavior is the only field worth reporting.
        builder.add_enum("clip_behavior", self.clip_behavior);
    }
}

impl RenderBox for RenderFlow {
    type Arity = Variable;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        let size = self.get_size(constraints);
        let n = ctx.child_count();
        self.child_sizes.clear();
        self.child_sizes.reserve(n);
        for i in 0..n {
            let inner = self.delegate.get_constraints_for_child(i, constraints);
            let child_size = ctx.layout_child(i, inner);
            // Oracle L327: children are NEVER positioned by layout, only
            // by the paint-time transform `FlowDelegate::paint_children`
            // chooses.
            ctx.position_child(i, Offset::ZERO);
            self.child_sizes.push(child_size);
        }
        size
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        _ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        // Oracle L311-313: children are never touched for dry layout either.
        self.get_size(constraints)
    }

    fn compute_min_intrinsic_width(&self, height: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.intrinsic_width(height)
    }

    fn compute_max_intrinsic_width(&self, height: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.intrinsic_width(height)
    }

    fn compute_min_intrinsic_height(&self, width: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.intrinsic_height(width)
    }

    fn compute_max_intrinsic_height(&self, width: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.intrinsic_height(width)
    }

    fn is_repaint_boundary(&self) -> bool {
        // Oracle L266-267: unconditional. Same precedent as
        // `RenderRepaintBoundary`.
        true
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Variable>) {
        let bounds = Rect::from_origin_size(Point::ZERO, ctx.size());
        let body = |ctx: &mut PaintCx<'_, Variable>| {
            let n = self.child_sizes.len();
            let (mut painted, mut paint_order, mut transforms) =
                (vec![false; n], Vec::with_capacity(n), vec![None; n]);
            let mut flow_ctx = FlowPaintingContext::for_paint(
                ctx,
                &self.child_sizes,
                &mut paint_order,
                &mut transforms,
                &mut painted,
            );
            self.delegate.paint_children(&mut flow_ctx);
        };
        // Clip-gating on `!= Clip::None` mirrors `RenderStack`'s FLUI idiom
        // (fewer emitted layers when clipping is off) rather than the
        // oracle's unconditional `pushClipRect` call — same visible result.
        if self.clip_behavior == Clip::None {
            body(ctx);
        } else {
            ctx.with_clip_rect(bounds, self.clip_behavior, body);
        }
    }

    /// Flutter's `RenderFlow.applyPaintTransform` (`flow.dart:456-462`), which
    /// multiplies in the child's cached `FlowParentData._transform`.
    ///
    /// **The default would be wrong here.** A flow paints each child under a
    /// per-child transform scope chosen by the delegate, not at its committed
    /// offset. FLUI caches no per-child transform (see the module docs), so this
    /// replays `paint_children` exactly as `hit_test` does.
    ///
    /// A child the delegate never painted contributes no transform — Flutter's
    /// `_transform == null` branch, which likewise leaves the matrix alone.
    fn apply_paint_transform(
        &self,
        child: usize,
        _child_offset: Offset,
        size: Size,
        transform: &mut Matrix4,
    ) {
        let n = self.child_sizes.len();
        if child >= n {
            return;
        }
        let (mut painted, mut paint_order, mut transforms) =
            (vec![false; n], Vec::with_capacity(n), vec![None; n]);
        let mut flow_ctx = FlowPaintingContext::for_replay(
            size,
            &self.child_sizes,
            &mut paint_order,
            &mut transforms,
            &mut painted,
        );
        self.delegate.paint_children(&mut flow_ctx);

        if let Some(matrix) = transforms[child] {
            *transform *= matrix;
        }
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Variable, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }

        // Side-effect-free replay of the SAME delegate call `paint` made —
        // see the module docs for why this stands in for Flutter's
        // paint-time `FlowParentData._transform` cache.
        let n = self.child_sizes.len();
        let (mut painted, mut paint_order, mut transforms) =
            (vec![false; n], Vec::with_capacity(n), vec![None; n]);
        let mut flow_ctx = FlowPaintingContext::for_replay(
            ctx.own_size(),
            &self.child_sizes,
            &mut paint_order,
            &mut transforms,
            &mut painted,
        );
        self.delegate.paint_children(&mut flow_ctx);

        let position = *ctx.position();
        // Oracle L430: reverse paint order = top-most-painted-first.
        for &index in paint_order.iter().rev() {
            let Some(transform) = transforms[index] else {
                continue;
            };
            // A degenerate (non-invertible) transform means nothing of
            // the child is visible at any position — RenderTransform
            // parity, not a bug to propagate.
            let Some(inverse) = transform.try_inverse() else {
                continue;
            };
            let (local_x, local_y) = inverse.transform_point(position.dx, position.dy);
            let child_hit = ctx.with_transform(transform, |ctx| {
                ctx.hit_test_child(index, Offset::new(local_x, local_y))
            });
            if child_hit {
                return true;
            }
        }
        false
    }

    /// Subscribes the delegate's repaint listenable (ADR-0013): a notify from
    /// the delegate's `repaint()` listenable now marks this node needing paint,
    /// so an animation-driven flow repaints without a widget rebuild.
    fn attach(&mut self, handle: RepaintHandle) {
        self.repaint_handle = Some(handle);
        self.delegate_listener = self.subscribe();
    }

    /// Tears down the repaint subscription and drops the self-dirty handle.
    fn detach(&mut self) {
        Self::unsubscribe(&self.delegate, self.delegate_listener.take());
        self.repaint_handle = None;
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;

    use flui_rendering::context::intrinsics_test_support::{leaf_dry_layout, leaf_intrinsics};
    use flui_types::{Matrix4, geometry::px};

    use super::*;

    /// Lays children out in a single row, spaced by `spacing`, translating
    /// each by its running x-offset — matches the harness's real-transform
    /// fixture used by the `RenderFlow` catalog tests.
    #[derive(Debug)]
    struct LinearFlowDelegate {
        spacing: f32,
    }

    impl FlowDelegate for LinearFlowDelegate {
        fn get_size(&self, constraints: BoxConstraints) -> Size {
            constraints.biggest()
        }

        fn get_constraints_for_child(
            &self,
            _index: usize,
            _constraints: BoxConstraints,
        ) -> BoxConstraints {
            BoxConstraints::loose(Size::new(px(100.0), px(50.0)))
        }

        fn paint_children(&self, context: &mut FlowPaintingContext<'_, '_>) {
            let mut x: f32 = 0.0;
            for i in 0..context.child_count() {
                context.paint_child(i, Matrix4::translation(x, 0.0, 0.0));
                x += context.child_size(i).width.get() + self.spacing;
            }
        }

        fn should_relayout(&self, old_delegate: &dyn FlowDelegate) -> bool {
            match old_delegate.as_any().downcast_ref::<Self>() {
                Some(old) => (self.spacing - old.spacing).abs() > f32::EPSILON,
                None => true,
            }
        }

        fn should_repaint(&self, old_delegate: &dyn FlowDelegate) -> bool {
            self.should_relayout(old_delegate)
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    /// A delegate whose `repaint()` returns a caller-controlled
    /// [`ChangeNotifier`](flui_foundation::ChangeNotifier), for testing that a
    /// notify marks the host `RenderFlow` needing paint.
    #[derive(Debug)]
    struct RepaintingFlowDelegate {
        repaint: Arc<flui_foundation::ChangeNotifier>,
    }

    impl FlowDelegate for RepaintingFlowDelegate {
        fn get_size(&self, constraints: BoxConstraints) -> Size {
            constraints.biggest()
        }

        fn get_constraints_for_child(
            &self,
            _index: usize,
            constraints: BoxConstraints,
        ) -> BoxConstraints {
            constraints
        }

        fn paint_children(&self, _context: &mut FlowPaintingContext<'_, '_>) {}

        fn should_relayout(&self, _old_delegate: &dyn FlowDelegate) -> bool {
            false
        }

        fn should_repaint(&self, _old_delegate: &dyn FlowDelegate) -> bool {
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
    fn repaint_listenable_marks_needs_paint_on_notify() {
        use flui_foundation::ChangeNotifier;
        use flui_rendering::pipeline::PipelineOwner;
        use flui_rendering::protocol::BoxProtocol;
        use flui_rendering::traits::RenderObject;

        let notifier = Arc::new(ChangeNotifier::new());
        let delegate = Arc::new(RepaintingFlowDelegate {
            repaint: notifier.clone(),
        });
        let node = RenderFlow::new(delegate);

        let mut owner = PipelineOwner::new();
        // `insert` fires the real `attach(handle)`, subscribing the delegate's
        // repaint listenable to a live handle from the pipeline.
        let id = owner.insert(Box::new(node) as Box<dyn RenderObject<BoxProtocol>>);

        // A fresh node is paint-dirty by default; clear so the notify's mark is
        // isolated (the red→green discriminator — without attach-subscribe the
        // node stays clean below).
        owner.clear_all_dirty_nodes();
        assert!(
            !owner.nodes_needing_paint().iter().any(|d| d.id == id),
            "precondition: node must be clean after clear_all_dirty_nodes",
        );

        notifier.notify_listeners();
        owner.drain_pending_dirty();
        assert!(
            owner.nodes_needing_paint().iter().any(|d| d.id == id),
            "a notify on the delegate's repaint listenable must mark the node \
             needing paint (ADR-0013 attach-subscribe wiring)",
        );
    }

    #[derive(Debug)]
    struct OtherFlowDelegate;

    impl FlowDelegate for OtherFlowDelegate {
        fn get_size(&self, constraints: BoxConstraints) -> Size {
            constraints.biggest()
        }

        fn get_constraints_for_child(
            &self,
            _index: usize,
            constraints: BoxConstraints,
        ) -> BoxConstraints {
            constraints
        }

        fn paint_children(&self, _context: &mut FlowPaintingContext<'_, '_>) {}

        fn should_relayout(&self, _old_delegate: &dyn FlowDelegate) -> bool {
            false
        }

        fn should_repaint(&self, _old_delegate: &dyn FlowDelegate) -> bool {
            false
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    fn linear(spacing: f32) -> Arc<dyn FlowDelegate> {
        Arc::new(LinearFlowDelegate { spacing })
    }

    #[test]
    fn defaults_match_flutter() {
        let flow = RenderFlow::new(linear(0.0));
        assert_eq!(flow.clip_behavior(), Clip::HardEdge);
    }

    #[test]
    fn with_clip_behavior_overrides_default() {
        let flow = RenderFlow::new(linear(0.0)).with_clip_behavior(Clip::None);
        assert_eq!(flow.clip_behavior(), Clip::None);
    }

    #[test]
    fn set_clip_behavior_reports_change_flag() {
        let mut flow = RenderFlow::new(linear(0.0));
        assert!(
            !flow.set_clip_behavior(Clip::HardEdge),
            "same value, no change"
        );
        assert!(flow.set_clip_behavior(Clip::None));
    }

    #[test]
    fn set_delegate_same_type_relayout_defers_to_should_relayout() {
        let mut flow = RenderFlow::new(linear(10.0));
        assert_eq!(
            flow.set_delegate(linear(10.0)),
            DelegateChange::None,
            "identical spacing must not require relayout or repaint"
        );
        assert_eq!(
            flow.set_delegate(linear(20.0)),
            DelegateChange::Relayout,
            "different spacing must trigger relayout (should_relayout true)"
        );
    }

    #[test]
    fn set_delegate_type_change_always_relayouts() {
        let mut flow = RenderFlow::new(linear(10.0));
        // OtherFlowDelegate's should_relayout/should_repaint both return
        // false, but the type changed — the oracle's explicit runtimeType
        // check must still force a relayout.
        assert_eq!(
            flow.set_delegate(Arc::new(OtherFlowDelegate)),
            DelegateChange::Relayout,
        );
    }

    #[test]
    fn set_delegate_repaint_only_when_should_repaint_and_not_relayout() {
        #[derive(Debug)]
        struct RepaintOnlyDelegate;
        impl FlowDelegate for RepaintOnlyDelegate {
            fn get_size(&self, constraints: BoxConstraints) -> Size {
                constraints.biggest()
            }
            fn get_constraints_for_child(
                &self,
                _index: usize,
                constraints: BoxConstraints,
            ) -> BoxConstraints {
                constraints
            }
            fn paint_children(&self, _context: &mut FlowPaintingContext<'_, '_>) {}
            fn should_relayout(&self, _old_delegate: &dyn FlowDelegate) -> bool {
                false
            }
            fn should_repaint(&self, _old_delegate: &dyn FlowDelegate) -> bool {
                true
            }
            fn as_any(&self) -> &dyn Any {
                self
            }
        }

        let mut flow = RenderFlow::new(Arc::new(RepaintOnlyDelegate));
        assert_eq!(
            flow.set_delegate(Arc::new(RepaintOnlyDelegate)),
            DelegateChange::Repaint,
        );
    }

    #[test]
    fn get_size_formula_constrains_delegate_size() {
        let flow = RenderFlow::new(linear(0.0));
        let constraints = BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(100.0));
        let size = leaf_dry_layout(|ctx| flow.compute_dry_layout(constraints, ctx));
        assert_eq!(
            size,
            Size::new(px(200.0), px(100.0)),
            "delegate.get_size(constraints.biggest()) must be constrain()-ed by the incoming box"
        );
    }

    #[test]
    fn intrinsics_use_tight_for_finite_on_the_opposite_axis() {
        // LinearFlowDelegate.get_size ignores the constraints entirely
        // (`constraints.biggest()`), so with one axis pinned to INFINITY by
        // tight_for_finite, the intrinsic on that axis is unbounded -> the
        // oracle's finite-or-zero guard reports 0.0.
        let flow = RenderFlow::new(linear(0.0));
        assert_eq!(
            leaf_intrinsics(|ctx| flow.compute_min_intrinsic_width(50.0, ctx)),
            0.0,
        );
        assert_eq!(
            leaf_intrinsics(|ctx| flow.compute_max_intrinsic_width(50.0, ctx)),
            0.0,
        );
        assert_eq!(
            leaf_intrinsics(|ctx| flow.compute_min_intrinsic_height(50.0, ctx)),
            0.0,
        );
        assert_eq!(
            leaf_intrinsics(|ctx| flow.compute_max_intrinsic_height(50.0, ctx)),
            0.0,
        );
    }

    #[test]
    fn childless_flow_sizes_via_get_size_alone() {
        let flow = RenderFlow::new(linear(0.0));
        let constraints = BoxConstraints::new(px(10.0), px(300.0), px(10.0), px(150.0));
        let size = leaf_dry_layout(|ctx| flow.compute_dry_layout(constraints, ctx));
        assert_eq!(size, Size::new(px(300.0), px(150.0)));
    }
}
