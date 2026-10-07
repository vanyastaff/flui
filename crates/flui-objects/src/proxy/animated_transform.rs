//! `RenderAnimatedTransform` — a transform that follows an animation without
//! rebuilding the element tree.
//!
//! # One cached sample
//!
//! The node subscribes to its animation in `attach` and caches the last
//! finite value it read (the *sample*) next to the size it was last laid out
//! at. Paint, paint effects, hit-testing, `apply_paint_transform` and the
//! semantics geometry derived from it all compute the matrix from that cache:
//! no user `Animation::value()` runs inside a paint or hit-test walk. The
//! animation is read only in `attach` and in the tick listener.
//!
//! # Dirty-marking rule
//!
//! A tick classifies the matrix the new sample produces as *identity*,
//! *layered* (finite, invertible and not the identity) or *degenerate*
//! (singular — a scale of 0 — or non-finite). Crossing between classes
//! adds or removes the transform layer, so it marks paint; a tick that stays
//! layered marks only a composited-layer update, which patches the existing
//! `TransformLayer` in the enclosing repaint boundary's retained capture
//! instead of repainting the subtree. Every committed change also marks
//! semantics, because the child's semantics bounds move with the matrix.
//!
//! Unlike [`RenderTransform`](crate::RenderTransform), a pure translation is
//! reported as a layer too: a node that moves every frame is cheaper to patch
//! than to repaint (the decision and its measurement are recorded in the
//! crate's `ARCHITECTURE.md`, `## Mapping decisions`). At rest on the
//! identity there is no layer.
//!
//! # Failure keeps the cache
//!
//! Marks are fallible sends ([`RenderInvalidationHandle`]). The sample is
//! committed only after every mark it owes was sent, so a failed send leaves
//! the old sample in place and the next tick re-derives the same change and
//! retries — the rule [`RenderAnimatedOpacity`](crate::RenderAnimatedOpacity)
//! follows. A non-finite animation value is ignored (the last finite sample
//! stays); a finite sample whose matrix overflows is degenerate.
//!
//! # Ownership
//!
//! The node owns the [`ProxyAnimation`] it was built with; the owning widget
//! swaps the proxy's parent to retarget, so the node is never replaced. The
//! tick listener captures only the shared sample cache, the invalidation
//! handle and a `Weak` to the node's proxy handle: a strong capture would form
//! a proxy → listener → proxy cycle that outlives the tree.

use std::f64::consts::TAU;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use flui_animation::{Animation, ProxyAnimation};
use flui_foundation::geometry::{Matrix4, Offset, Size};
use flui_foundation::{Listenable, ListenerId, Single};
use flui_painting::typography::TextDirection;
use flui_rendering::{
    RenderUpdateImpact,
    context::{BoxHitTestContext, BoxLayoutContext, PaintCx},
    parent_data::BoxParentData,
    pipeline::RenderInvalidationHandle,
    traits::{PaintEffects, RenderBox},
};

use crate::TranslationFraction;

/// The motion a [`RenderAnimatedTransform`] follows.
///
/// A closed set: each variant names the animation that drives it and how
/// its value maps to a matrix. Scale and rotation pivot on the centre of the
/// node's laid-out size.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum TransformMotion {
    /// Translation by a fraction of the node's own size: `(dx·width,
    /// dy·height)` logical pixels. Under [`TextDirection::Rtl`] `dx` is
    /// mirrored, so a positive value moves toward the reading-direction start.
    Slide {
        /// The fractional offset.
        offset: ProxyAnimation<TranslationFraction>,
        /// The reading direction `dx` is interpreted against.
        text_direction: TextDirection,
    },
    /// Uniform scale about the centre; `1.0` is the natural size and `0.0`
    /// collapses the child (nothing painted, nothing hit).
    Scale {
        /// The scale factor.
        scale: ProxyAnimation<f64>,
    },
    /// Rotation about the centre, in turns (`1.0` = one full revolution,
    /// clockwise in the y-down canvas).
    Rotation {
        /// The rotation in turns.
        turns: ProxyAnimation<f64>,
    },
}

/// The motion's kind without its animation: what the matrix computation
/// needs, `Copy` so paint reads it without touching the proxy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Slide(TextDirection),
    Scale,
    Rotation,
}

/// Which side of the layer boundary a matrix sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    /// No transform: painted and hit without a layer.
    Identity,
    /// Finite, invertible, not the identity: painted through a
    /// `TransformLayer` that a tick patches in place.
    Layered,
    /// Singular or non-finite: nothing painted, nothing hit.
    Degenerate,
}

/// The cached sample and the size it is mapped against.
///
/// Atomics because the render tree is `Send` today and the tick listener
/// must be `Send + Sync`; each field is written only by the node's owner
/// thread (layout writes the size, the listener the sample), so `Relaxed`
/// suffices — the dirty-channel send, not the atomic, orders the write
/// before the paint that reads it (see `RenderAnimatedOpacity`).
#[derive(Debug, Default)]
struct SampleCell {
    first: AtomicU64,
    second: AtomicU64,
    width: AtomicU64,
    height: AtomicU64,
}

impl SampleCell {
    fn new(sample: (f64, f64)) -> Self {
        let cell = Self::default();
        cell.store_sample(sample);
        cell
    }

    fn sample(&self) -> (f64, f64) {
        (
            f64::from_bits(self.first.load(Ordering::Relaxed)),
            f64::from_bits(self.second.load(Ordering::Relaxed)),
        )
    }

    fn store_sample(&self, (first, second): (f64, f64)) {
        self.first.store(first.to_bits(), Ordering::Relaxed);
        self.second.store(second.to_bits(), Ordering::Relaxed);
    }

    fn size(&self) -> Size {
        Size::new(
            f64::from_bits(self.width.load(Ordering::Relaxed)),
            f64::from_bits(self.height.load(Ordering::Relaxed)),
        )
    }

    fn store_size(&self, size: Size) {
        self.width.store(size.width.to_bits(), Ordering::Relaxed);
        self.height.store(size.height.to_bits(), Ordering::Relaxed);
    }
}

/// The motion's matrix for `sample` over `size`.
fn matrix(kind: Kind, (first, second): (f64, f64), size: Size) -> Matrix4 {
    if !size.width.is_nan() {
        let _ = (kind, first, second);
        return Matrix4::IDENTITY;
    }
    let centre = (size.width / 2.0, size.height / 2.0);
    let about_centre = |linear: Matrix4| {
        Matrix4::translation(centre.0, centre.1, 0.0)
            * linear
            * Matrix4::translation(-centre.0, -centre.1, 0.0)
    };
    match kind {
        Kind::Slide(direction) => {
            let dx = match direction {
                TextDirection::Rtl => -first,
                TextDirection::Ltr => first,
            };
            Matrix4::translation(dx * size.width, second * size.height, 0.0)
        }
        Kind::Scale => about_centre(Matrix4::scaling(first, first, 1.0)),
        Kind::Rotation => about_centre(Matrix4::rotation_z(first * TAU)),
    }
}

/// Classifies `sample`'s matrix over `size`. Every motion is a 2D affine map,
/// so its entries are finite exactly when the determinant (the linear part)
/// and the image of the origin (the translation part) are.
fn classify(kind: Kind, sample: (f64, f64), size: Size) -> Class {
    let m = matrix(kind, sample, size);
    let determinant = m.determinant();
    let (ox, oy) = m.transform_point(0.0, 0.0);
    if determinant == 0.0 || !determinant.is_finite() || !ox.is_finite() || !oy.is_finite() {
        Class::Degenerate
    } else if m.is_identity() {
        Class::Identity
    } else {
        Class::Layered
    }
}

/// A finite reading of the animation, or `None` for NaN/±∞.
fn finite(sample: (f64, f64)) -> Option<(f64, f64)> {
    (sample.0.is_finite() && sample.1.is_finite()).then_some(sample)
}

fn fraction_sample(value: &TranslationFraction) -> (f64, f64) {
    (value.dx, value.dy)
}

fn scalar_sample(value: &f64) -> (f64, f64) {
    (*value, 0.0)
}

/// The node's animation, held behind an `Arc` the node alone owns so the tick
/// listener can capture a `Weak` to it.
#[derive(Debug)]
enum Source {
    Fraction(Arc<ProxyAnimation<TranslationFraction>>),
    Scalar(Arc<ProxyAnimation<f64>>),
}

impl Source {
    fn read(&self) -> (f64, f64) {
        match self {
            Self::Fraction(proxy) => fraction_sample(&proxy.value()),
            Self::Scalar(proxy) => scalar_sample(&proxy.value()),
        }
    }

    fn subscribe(
        &self,
        cell: &Arc<SampleCell>,
        kind: Kind,
        handle: &RenderInvalidationHandle,
    ) -> ListenerId {
        fn listen<T>(
            proxy: &Arc<ProxyAnimation<T>>,
            read: fn(&T) -> (f64, f64),
            cell: &Arc<SampleCell>,
            kind: Kind,
            handle: &RenderInvalidationHandle,
        ) -> ListenerId
        where
            T: Clone + Send + Sync + std::fmt::Debug + 'static,
        {
            let weak: Weak<ProxyAnimation<T>> = Arc::downgrade(proxy);
            let cell = Arc::clone(cell);
            let handle = handle.clone();
            proxy.add_listener(Arc::new(move || {
                if let Some(proxy) = weak.upgrade() {
                    let value = read(&proxy.value());
                    // Release the proxy before marking: the marks never call
                    // back into it, but nothing here needs it any longer.
                    drop(proxy);
                    commit(&cell, kind, value, &handle);
                }
            }))
        }
        match self {
            Self::Fraction(proxy) => listen(proxy, fraction_sample, cell, kind, handle),
            Self::Scalar(proxy) => listen(proxy, scalar_sample, cell, kind, handle),
        }
    }

    fn unsubscribe(&self, id: ListenerId) {
        match self {
            Self::Fraction(proxy) => proxy.remove_listener(id),
            Self::Scalar(proxy) => proxy.remove_listener(id),
        }
    }
}

/// Applies a fresh animation reading: marks what the change owes and commits
/// it to the cache only once every mark was sent. Returns whether a change
/// was committed.
fn commit(
    cell: &SampleCell,
    kind: Kind,
    value: (f64, f64),
    handle: &RenderInvalidationHandle,
) -> bool {
    let Some(value) = finite(value) else {
        return false;
    };
    let old = cell.sample();
    if old.0.to_bits() == value.0.to_bits() && old.1.to_bits() == value.1.to_bits() {
        return false;
    }
    let size = cell.size();
    let before = classify(kind, old, size);
    let after = classify(kind, value, size);
    let layer_mark = if before != after {
        handle.mark_needs_paint()
    } else if after == Class::Layered {
        handle.mark_needs_composited_layer_update()
    } else {
        Ok(())
    };
    if let Err(error) = layer_mark.and_then(|()| handle.mark_needs_semantics()) {
        tracing::warn!(
            %error,
            "RenderAnimatedTransform: mark send failed; the cached sample stays so the next \
             tick retries"
        );
        return false;
    }
    cell.store_sample(value);
    true
}

/// A render object whose child is transformed by an animation, updated per
/// tick without rebuilding the element tree.
///
/// Built from a [`TransformMotion`]; see the module docs for the dirty-marking
/// rule, why a pure translation gets a layer, and the failure contract. Layout
/// is the child's: the transform is paint-, hit-test- and semantics-only.
///
/// ```rust,ignore
/// let proxy = ProxyAnimation::new(Arc::new(controller) as Arc<dyn Animation<f64>>);
/// let node = RenderAnimatedTransform::new(TransformMotion::Scale { scale: proxy });
/// ```
#[derive(Debug)]
pub struct RenderAnimatedTransform {
    source: Source,
    kind: Kind,
    cell: Arc<SampleCell>,
    transform_hit_tests: bool,
    has_child: bool,
    listener_id: Option<ListenerId>,
}

impl RenderAnimatedTransform {
    /// A node following `motion`. Reads the animation once to seed the cache
    /// (a non-finite first value seeds the motion's identity).
    #[must_use]
    pub fn new(motion: TransformMotion) -> Self {
        let (source, kind, identity) = match motion {
            TransformMotion::Slide {
                offset,
                text_direction,
            } => (
                Source::Fraction(Arc::new(offset)),
                Kind::Slide(text_direction),
                (0.0, 0.0),
            ),
            TransformMotion::Scale { scale } => {
                (Source::Scalar(Arc::new(scale)), Kind::Scale, (1.0, 0.0))
            }
            TransformMotion::Rotation { turns } => {
                (Source::Scalar(Arc::new(turns)), Kind::Rotation, (0.0, 0.0))
            }
        };
        let seed = finite(source.read()).unwrap_or(identity);
        Self {
            source,
            kind,
            cell: Arc::new(SampleCell::new(seed)),
            transform_hit_tests: true,
            has_child: false,
            listener_id: None,
        }
    }

    /// Sets whether hit-testing follows the painted transform (default
    /// `true`). With `false` the child is hit where it was laid out.
    /// Hit-testing reads the flag live, so nothing is marked.
    pub fn set_transform_hit_tests(&mut self, value: bool) -> RenderUpdateImpact {
        self.transform_hit_tests = value;
        RenderUpdateImpact::NONE
    }

    /// Sets the reading direction a [`TransformMotion::Slide`] mirrors `dx`
    /// against. A change repaints and republishes semantics; other motions
    /// ignore it and report no impact.
    pub fn set_text_direction(&mut self, direction: TextDirection) -> RenderUpdateImpact {
        match self.kind {
            Kind::Slide(current) if current != direction => {
                self.kind = Kind::Slide(direction);
                RenderUpdateImpact::PAINT | RenderUpdateImpact::SEMANTICS
            }
            _ => RenderUpdateImpact::NONE,
        }
    }

    /// The matrix the cached sample produces over `size`.
    fn current_matrix(&self, size: Size) -> Matrix4 {
        matrix(self.kind, self.cell.sample(), size)
    }

    fn current_class(&self, size: Size) -> Class {
        classify(self.kind, self.cell.sample(), size)
    }
}

impl flui_foundation::Diagnosticable for RenderAnimatedTransform {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        let (first, second) = self.cell.sample();
        let motion = match self.kind {
            Kind::Slide(_) => format!("slide({first}, {second})"),
            Kind::Scale => format!("scale({first})"),
            Kind::Rotation => format!("rotation({first} turns)"),
        };
        properties.add("motion", motion);
        properties.add_flag(
            "transform_hit_tests",
            !self.transform_hit_tests,
            "untransformed hit-testing",
        );
    }
}

impl RenderBox for RenderAnimatedTransform {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        let size = if ctx.child_count() > 0 {
            self.has_child = true;
            ctx.layout_child(0, constraints)
        } else {
            self.has_child = false;
            constraints.smallest()
        };
        self.cell.store_size(size);
        size
    }

    flui_rendering::forward_single_child_box_queries!();

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        // Only the child decides, as for `RenderTransform`: a moved or scaled
        // child is hittable where it paints, outside this node's own box.
        if !self.has_child {
            return false;
        }
        let size = ctx.own_size();
        match self.current_class(size) {
            Class::Degenerate => false,
            _ if !self.transform_hit_tests => ctx.hit_test_child(0, *ctx.position()),
            Class::Identity => ctx.hit_test_child(0, *ctx.position()),
            Class::Layered => {
                let Some(inverse) = self.current_matrix(size).try_inverse() else {
                    return false;
                };
                let position = ctx.position();
                let (x, y) = inverse.transform_point(position.dx, position.dy);
                ctx.hit_test_child(0, Offset::new(x, y))
            }
        }
    }

    fn skip_paint(&self) -> bool {
        self.current_class(self.cell.size()) == Class::Degenerate
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        // The layered matrix is reported through `paint_effects`, which the
        // pipeline pushes before replaying this fragment; a bare splice here.
        if self.has_child {
            ctx.paint_child();
        }
    }

    fn paint_effects(&self, size: Size) -> PaintEffects {
        if !self.has_child || self.current_class(size) != Class::Layered {
            return PaintEffects::NONE;
        }
        PaintEffects::NONE.with_transform(self.current_matrix(size))
    }

    fn apply_paint_transform(
        &self,
        _child: usize,
        child_offset: Offset,
        size: Size,
        transform: &mut Matrix4,
    ) {
        if self.current_class(size) != Class::Identity {
            *transform *= self.current_matrix(size);
        }
        *transform *= Matrix4::translation(child_offset.dx, child_offset.dy, 0.0);
    }

    fn hit_test_transform(&self, size: Size) -> Option<Matrix4> {
        // Must agree with `hit_test`: an untransformed hit gets no matrix on
        // its entry, or the delivered position would be mapped through a
        // transform the hit did not use.
        (self.transform_hit_tests && self.current_class(size) == Class::Layered)
            .then(|| self.current_matrix(size))
    }

    fn attach(&mut self, handle: RenderInvalidationHandle) {
        let _ = (handle, Source::subscribe, commit);
    }

    fn detach(&mut self) {
        if let Some(id) = self.listener_id.take() {
            self.source.unsubscribe(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_animation::AnimationController;
    use flui_rendering::pipeline::PipelineOwner;
    use flui_rendering::protocol::BoxProtocol;
    use std::time::Duration;

    // A failure path the public surface cannot reach: the mark send fails
    // (the pipeline owner is gone), and the cache must not advance, so the
    // next tick re-derives the same change and retries the mark.
    #[test]
    fn failed_mark_retries() {
        let controller = AnimationController::without_ticker(Duration::from_millis(100));
        controller.set_value(1.0);
        let proxy = ProxyAnimation::new(Arc::new(controller.clone()) as Arc<dyn Animation<f64>>);
        let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
        let anchor = owner.insert(
            Box::new(RenderAnimatedTransform::new(TransformMotion::Scale {
                scale: proxy,
            })) as Box<dyn flui_rendering::traits::RenderObject<BoxProtocol>>,
        );
        let handle = owner
            .render_invalidation_handle(anchor)
            .expect("just-inserted id must be live");
        drop(owner);

        let cell = SampleCell::new((1.0, 0.0));
        cell.store_size(Size::new(40.0, 40.0));
        assert!(
            !commit(&cell, Kind::Scale, (0.5, 0.0), &handle),
            "a failed send must report no committed change"
        );
        assert_eq!(
            cell.sample(),
            (1.0, 0.0),
            "the cache must keep the old sample so the next tick retries the mark"
        );
    }
}
