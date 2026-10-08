//! `RenderFlow` — positions children with paint-time transform matrices
//! chosen by a [`FlowDelegate`], instead of layout-time offsets.
//!
//! Every other multi-child render object in this crate positions children
//! during layout (`ctx.position_child`); `RenderFlow` never does — every
//! child is positioned at [`Offset::ZERO`] by layout and repositioned purely
//! by paint-time [`FlowDelegate::paint_children`] transforms, so moving
//! children costs a repaint, not a relayout.
//!
//! # Hit-testing without mutable paint state
//!
//! FLUI's `RenderBox::paint`/`hit_test` are both `&self`, so there is nowhere
//! to cache "what transform did paint assign to child N" for hit-test to read
//! later. Instead [`RenderFlow::hit_test`]
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
//! Nothing needs caching from paint, since the delegate is replayed instead,
//! so there is nothing to add to parent data — `RenderFlow` uses plain
//! [`BoxParentData`], a deliberate simplification, not an oversight.
//!
//! # Repaint listenable
//!
//! Per ADR-0013, [`FlowDelegate::repaint`] returns an optional `Listenable`
//! that [`RenderBox::attach`] subscribes to (marking this node needing paint
//! on notify) and [`RenderBox::detach`] tears down; a delegate swap migrates
//! the subscription (as `RenderCustomPaint` does).
//!
//! # Coordinate mapping
//!
//! `apply_paint_transform` replays `paint_children` to recover the child's
//! paint matrix, the way `hit_test` does; the transform-to and
//! local-to-global queries live on `PipelineOwner`, because a FLUI render
//! object has no parent link.
//!
//! # Not supported
//!
//! - `FlowPaintingContext::paint_child(index, transform)` has no opacity
//!   parameter.
//! - No semantics update on `clip_behavior` change: FLUI has no semantics
//!   tree yet, consistent with every other render object in the catalog.

use std::sync::Arc;

use flui_foundation::ListenerId;
use flui_foundation::Variable;
use flui_foundation::geometry::{Matrix4, Offset, Point, Rect, Size};
use flui_painting::paint::Clip;

use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext, PaintCx},
    delegates::{FlowDelegate, FlowPaintingContext},
    parent_data::BoxParentData,
    pipeline::RenderInvalidationHandle,
    traits::{PaintClip, PaintEffects, RenderBox},
};

/// Positions children with paint-time transform matrices chosen by a
/// [`FlowDelegate`], rather than layout-time offsets.
///
/// See the module docs for why this needs no custom parent data and how
/// hit-testing works without mutable paint state.
// NOT `Clone`: holds live per-node lifecycle state (a `RenderInvalidationHandle` bound to
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
    render_invalidation_handle: Option<RenderInvalidationHandle>,
    /// Active `add_listener` id on the delegate's repaint listenable, torn
    /// down in `detach` and migrated on a delegate swap.
    delegate_listener: Option<ListenerId>,
}

impl RenderFlow {
    /// Creates a flow render object with `clip_behavior = Clip::HardEdge`.
    pub fn new(delegate: Arc<dyn FlowDelegate>) -> Self {
        Self {
            delegate,
            clip_behavior: Clip::HardEdge,
            child_sizes: Vec::new(),
            render_invalidation_handle: None,
            delegate_listener: None,
        }
    }

    /// Subscribes the delegate's repaint [`Listenable`](flui_foundation::Listenable)
    /// (if any) to this node's self-dirty handle, so a notify marks the node
    /// needing paint. Returns the subscription id, or `None` when detached or
    /// the delegate has no repaint listenable.
    fn subscribe(&self) -> Option<ListenerId> {
        let handle = self.render_invalidation_handle.as_ref()?;
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

    /// The single sizing formula reused by layout, dry layout, and all four
    /// intrinsics.
    fn get_size(&self, constraints: BoxConstraints) -> Size {
        constraints.constrain(self.delegate.get_size(constraints))
    }

    /// Shared by both width intrinsics: min and max use the identical
    /// formula, and intrinsics never touch children.
    fn intrinsic_width(&self, height: f64) -> f64 {
        let width = self
            .get_size(BoxConstraints::tight_for_finite(f64::INFINITY, height))
            .width;
        if width.is_finite() { width } else { 0.0 }
    }

    /// Shared by both height intrinsics — see [`Self::intrinsic_width`].
    fn intrinsic_height(&self, width: f64) -> f64 {
        let height = self
            .get_size(BoxConstraints::tight_for_finite(width, f64::INFINITY))
            .height;
        if height.is_finite() { height } else { 0.0 }
    }

    /// Replaces the delegate, reporting whether the swap needs relayout,
    /// just a repaint, or neither.
    ///
    /// A delegate *type* change always relayouts; otherwise `should_relayout` on the
    /// new delegate (compared against the old one) wins, falling back to
    /// `should_repaint`. The caller is responsible for actually marking
    /// the render object dirty — this is paint/layout-affecting state,
    /// not a side-effecting setter.
    pub fn set_delegate(
        &mut self,
        delegate: Arc<dyn FlowDelegate>,
    ) -> flui_rendering::RenderUpdateImpact {
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
            flui_rendering::RenderUpdateImpact::LAYOUT
        } else if repaint {
            flui_rendering::RenderUpdateImpact::PAINT
        } else {
            flui_rendering::RenderUpdateImpact::NONE
        }
    }

    /// Updates the clip behavior, affecting paint and the semantics clip.
    ///
    /// Reports `PAINT`, not a composited-layer update: the flow's clip is
    /// gated on `Clip::None` (see `paint_effects`), so a change can add or
    /// remove the clip layer — a structural change a layer patch cannot
    /// express. A non-`None` → non-`None` change would be patchable, but one
    /// impact per setter keeps the contract simple and the case is rare.
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) -> flui_rendering::RenderUpdateImpact {
        if self.clip_behavior == clip_behavior {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.clip_behavior = clip_behavior;
        flui_rendering::RenderUpdateImpact::PAINT | flui_rendering::RenderUpdateImpact::SEMANTICS
    }
}

impl flui_foundation::Diagnosticable for RenderFlow {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        // No delegate info is surfaced, so clip_behavior is the only field
        // worth reporting.
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
            // Children are NEVER positioned by layout, only by the
            // paint-time transform `FlowDelegate::paint_children` chooses.
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
        // Children are never touched for dry layout either.
        self.get_size(constraints)
    }

    fn compute_min_intrinsic_width(&self, height: f64, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        self.intrinsic_width(height)
    }

    fn compute_max_intrinsic_width(&self, height: f64, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        self.intrinsic_width(height)
    }

    fn compute_min_intrinsic_height(&self, width: f64, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        self.intrinsic_height(width)
    }

    fn compute_max_intrinsic_height(&self, width: f64, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        self.intrinsic_height(width)
    }

    fn is_repaint_boundary(&self) -> bool {
        // Unconditional, as with `RenderRepaintBoundary`.
        true
    }

    /// The flow's own clip, reported as a paint effect so the pipeline opens
    /// it around the whole fragment `paint` records — outside every per-child
    /// transform scope the delegate pushes, exactly where `paint` used to
    /// open it itself.
    ///
    /// Gated on `!= Clip::None` rather than reported unconditionally, as
    /// `RenderStack` does (no layer at all when clipping is off), so
    /// `set_clip_behavior` across `Clip::None` is a layer-COUNT change and
    /// stays a structural `PAINT` — the one clip producer whose clip can
    /// appear and disappear.
    fn paint_effects(&self, size: Size) -> PaintEffects {
        if self.clip_behavior == Clip::None {
            return PaintEffects::NONE;
        }
        PaintEffects::NONE.with_clip(PaintClip::Rect {
            rect: Rect::from_origin_size(Point::ZERO, size),
            behavior: self.clip_behavior,
        })
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Variable>) {
        // The clip (when any) is reported through `paint_effects` above and
        // opened by the pipeline around this whole fragment; only the
        // delegate's per-child transform scopes are recorded here.
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
    }

    /// Multiplies in the transform the delegate painted the child under.
    ///
    /// **The default would be wrong here.** A flow paints each child under a
    /// per-child transform scope chosen by the delegate, not at its committed
    /// offset. FLUI caches no per-child transform (see the module docs), so this
    /// replays `paint_children` exactly as `hit_test` does.
    ///
    /// A child the delegate never painted contributes no transform and leaves
    /// the matrix alone.
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
        // see the module docs for why this stands in for a paint-time
        // transform cache.
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
        // Reverse paint order = top-most-painted-first.
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
            let Some((local_x, local_y)) = inverse.unproject_to_plane(position.dx, position.dy)
            else {
                continue;
            };
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
    fn attach(&mut self, handle: RenderInvalidationHandle) {
        self.render_invalidation_handle = Some(handle);
        self.delegate_listener = self.subscribe();
    }

    /// Tears down the repaint subscription and drops the self-dirty handle.
    fn detach(&mut self) {
        Self::unsubscribe(&self.delegate, self.delegate_listener.take());
        self.render_invalidation_handle = None;
    }
}
