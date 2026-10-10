//! `RenderAnimatedTransform` â€” a transform that follows an animation without
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
//! (singular â€” a scale of 0 â€” or non-finite). Crossing between classes
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
//! Marks are fallible sends ([`RenderInvalidationHandle`]) that request the
//! owner's next frame, so a tick publishes the new sample *before* marking.
//! Render samples stay on the UI owner (ADR-0175). Delivery
//! is tracked apart from the sample: the cache also keeps the last sample
//! whose marks were all sent, and the next tick classifies against that one,
//! so a failed send leaves the debt in place and is retried even when the
//! next value equals the published one. A non-finite animation value is
//! ignored (the last finite sample stays); a finite sample whose matrix
//! overflows is degenerate: nothing is painted or hit, and coordinate
//! conversion falls back to the untransformed position rather than
//! publishing non-finite geometry.
//!
//! # Ownership
//!
//! The node owns the [`ProxyAnimation`] it was built with; the owning widget
//! swaps the proxy's parent to retarget, so the node is never replaced. The
//! tick listener captures only the shared sample cache, the invalidation
//! handle and a `Weak` to the node's proxy handle: a strong capture would form
//! a proxy â†’ listener â†’ proxy cycle that outlives the tree.

use std::cell::Cell;
use std::f64::consts::TAU;
use std::rc::{Rc, Weak};

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
    /// Translation by a fraction of the node's own size: `(dxÂ·width,
    /// dyÂ·height)` logical pixels. Under [`TextDirection::Rtl`] `dx` is
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

/// A sample tagged with the order its reading was taken in.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Versioned {
    generation: u64,
    value: (f64, f64),
}

/// Publishes a newer reading without letting reentry restore an older one.
/// No user code runs between the generation check and the whole-value store.
fn publish_newer(cell: &Cell<Versioned>, entry: Versioned) -> Option<u64> {
    let previous = cell.get();
    if entry.generation <= previous.generation {
        return None;
    }
    cell.set(entry);
    Some(previous.generation)
}

/// The published sample, the last delivered sample and the size they are
/// mapped against.
///
/// Reads and callbacks run on the UI owner. Each commit takes a
/// [`ticket`](Self::ticket) before calling the animation: a reentrant newer
/// reading supersedes the outgoing one. Published and delivered records stay
/// separate so a failed mark leaves delivery debt even after A -> B -> A.
#[derive(Debug)]
struct SampleCell {
    tickets: Cell<u64>,
    published: Cell<Versioned>,
    delivered: Cell<Versioned>,
    size: Cell<Size>,
}

impl SampleCell {
    fn new(sample: (f64, f64)) -> Self {
        let seed = Versioned {
            generation: 0,
            value: sample,
        };
        Self {
            tickets: Cell::new(0),
            published: Cell::new(seed),
            delivered: Cell::new(seed),
            size: Cell::new(Size::ZERO),
        }
    }

    /// The generation for a reading about to be taken. Saturates: a cell
    /// that exhausted `u64` keeps its last sample rather than reissuing an
    /// older generation.
    fn ticket(&self) -> u64 {
        let next = self.tickets.get().saturating_add(1);
        self.tickets.set(next);
        next
    }

    /// The published sample, never a mix of two writes.
    fn sample(&self) -> (f64, f64) {
        self.published.get().value
    }

    fn size(&self) -> Size {
        self.size.get()
    }

    fn store_size(&self, size: Size) {
        self.size.set(size);
    }
}

/// The motion's matrix for `sample` over `size`.
fn matrix(kind: Kind, (first, second): (f64, f64), size: Size) -> Matrix4 {
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
        // Reduced to one turn first: rotation is periodic, and a large finite
        // turn count would overflow the multiplication into a non-finite
        // (degenerate) matrix.
        Kind::Rotation => about_centre(Matrix4::rotation_z(first.rem_euclid(1.0) * TAU)),
    }
}

/// Whether every entry of `m` is finite. A degenerate matrix that passes is
/// singular (a scale of 0 maps the child to a point); one that fails
/// overflowed and must not be composed into coordinate conversion.
fn is_finite(m: &Matrix4) -> bool {
    m.m.iter().all(|entry| entry.is_finite())
}

/// Classifies `sample`'s matrix over `size`. Every motion is a 2D affine map,
/// so its entries are finite exactly when the determinant (the linear part)
/// and the image of the origin (the translation part) are.
fn classify(kind: Kind, sample: (f64, f64), size: Size) -> Class {
    classify_matrix(&matrix(kind, sample, size))
}

fn classify_matrix(m: &Matrix4) -> Class {
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

/// A finite reading of the animation, or `None` for NaN/Â±âˆž.
fn finite(sample: (f64, f64)) -> Option<(f64, f64)> {
    (sample.0.is_finite() && sample.1.is_finite()).then_some(sample)
}

fn fraction_sample(value: &TranslationFraction) -> (f64, f64) {
    (value.dx, value.dy)
}

fn scalar_sample(value: &f64) -> (f64, f64) {
    (*value, 0.0)
}

/// The node's animation, held behind an `Rc` the node alone owns so the tick
/// listener can capture a `Weak` to it.
#[derive(Debug)]
enum Source {
    Fraction(Rc<ProxyAnimation<TranslationFraction>>),
    Scalar(Rc<ProxyAnimation<f64>>),
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
        cell: &Rc<SampleCell>,
        kind: Kind,
        handle: &RenderInvalidationHandle,
    ) -> ListenerId {
        fn listen<T>(
            proxy: &Rc<ProxyAnimation<T>>,
            read: fn(&T) -> (f64, f64),
            cell: &Rc<SampleCell>,
            kind: Kind,
            handle: &RenderInvalidationHandle,
        ) -> ListenerId
        where
            T: Clone + std::fmt::Debug + 'static,
        {
            let weak: Weak<ProxyAnimation<T>> = Rc::downgrade(proxy);
            let cell = Rc::clone(cell);
            let handle = handle.clone();
            proxy.add_listener(std::rc::Rc::new(move || {
                if let Some(proxy) = weak.upgrade() {
                    // The ticket precedes the read: see `SampleCell`.
                    let generation = cell.ticket();
                    let value = read(&proxy.value());
                    // Release the proxy before marking: the marks never call
                    // back into it, but nothing here needs it any longer.
                    drop(proxy);
                    commit(&cell, kind, generation, value, &handle);
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

/// Applies a fresh animation reading: publishes it, then sends the marks the
/// change from the last *delivered* sample owes, and records it as delivered
/// only once every mark was sent. A reading whose `generation` is older than
/// the published one is discarded unmarked: the newer commit owns delivery.
/// Returns whether the change was delivered.
fn commit(
    cell: &SampleCell,
    kind: Kind,
    generation: u64,
    value: (f64, f64),
    handle: &RenderInvalidationHandle,
) -> bool {
    let Some(value) = finite(value) else {
        return false;
    };
    let entry = Versioned { generation, value };
    // Published first: a mark may run (or wake) the frame that reads it.
    let Some(replaced) = publish_newer(&cell.published, entry) else {
        return false;
    };
    let delivered = cell.delivered.get();
    let old = delivered.value;
    // A replaced publication newer than the delivery record is in flight (or
    // its marks failed): a frame may already show it, so equality with the
    // delivered value proves nothing and the class it painted is unknown.
    let in_flight = replaced > delivered.generation;
    if !in_flight && old.0.to_bits() == value.0.to_bits() && old.1.to_bits() == value.1.to_bits() {
        // This generation owes no marks. Record its completed delivery so a
        // later equal notification cannot mistake it for outstanding work.
        let _ = publish_newer(&cell.delivered, entry);
        return false;
    }
    let size = cell.size();
    let before = classify(kind, old, size);
    let after = classify(kind, value, size);
    let layer_mark = if in_flight || before != after {
        handle.mark_needs_paint()
    } else if after == Class::Layered {
        handle.mark_needs_composited_layer_update()
    } else {
        Ok(())
    };
    if let Err(error) = layer_mark.and_then(|()| handle.mark_needs_semantics()) {
        tracing::warn!(
            %error,
            "RenderAnimatedTransform: mark send failed; delivery stays owed so the next tick \
             retries"
        );
        return false;
    }
    // Ordered like the published sample: an older commit finishing late
    // cannot replace a newer delivery record.
    let _ = publish_newer(&cell.delivered, entry);
    true
}

/// A render object whose child is transformed by an animation, updated per
/// tick without rebuilding the element tree.
///
/// Built from a [`TransformMotion`]; see the module docs for the dirty-marking
/// rule, why a pure translation gets a layer, and the failure contract. Layout
/// is the child's: the transform is paint-, hit-test- and semantics-only.
///
/// ```rust
/// use std::{rc::Rc, time::Duration};
/// use flui_animation::{Animation, AnimationController, ProxyAnimation};
/// use flui_objects::{RenderAnimatedTransform, TransformMotion};
///
/// let controller = AnimationController::builder(Duration::from_millis(300)).build();
/// let proxy = ProxyAnimation::new(Rc::new(controller) as Rc<dyn Animation<f64>>);
/// let _node = RenderAnimatedTransform::new(TransformMotion::Scale { scale: proxy });
/// ```
pub struct RenderAnimatedTransform {
    source: Source,
    kind: Kind,
    cell: Rc<SampleCell>,
    /// The sample `hit_test_transform` chose for the current hit-test visit,
    /// consumed by the `hit_test` that follows it, so a tick between the two
    /// hooks cannot pair one matrix on the hit entry with another for the
    /// child's position.
    hit_sample: Cell<Option<(f64, f64)>>,
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
                Source::Fraction(Rc::new(offset)),
                Kind::Slide(text_direction),
                (0.0, 0.0),
            ),
            TransformMotion::Scale { scale } => {
                (Source::Scalar(Rc::new(scale)), Kind::Scale, (1.0, 0.0))
            }
            TransformMotion::Rotation { turns } => {
                (Source::Scalar(Rc::new(turns)), Kind::Rotation, (0.0, 0.0))
            }
        };
        let seed = finite(source.read()).unwrap_or(identity);
        Self {
            source,
            kind,
            cell: Rc::new(SampleCell::new(seed)),
            hit_sample: Cell::new(None),
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

    /// The class and matrix of one read of the cached sample over `size`.
    /// Always taken together: a tick between two reads could pair a class
    /// approved for one sample with the (possibly overflowed) matrix of the
    /// next.
    fn current(&self, size: Size) -> (Class, Matrix4) {
        Self::class_and_matrix(self.kind, self.cell.sample(), size)
    }

    fn class_and_matrix(kind: Kind, sample: (f64, f64), size: Size) -> (Class, Matrix4) {
        let m = matrix(kind, sample, size);
        (classify_matrix(&m), m)
    }

    /// Records the sample this hit-test visit uses; see `hit_sample`.
    fn arm_hit_sample(&self) -> (f64, f64) {
        let sample = self.cell.sample();
        self.hit_sample.set(Some(sample));
        sample
    }

    /// The sample `hit_test_transform` recorded for this visit, or a fresh
    /// read when `hit_test` runs alone.
    fn take_hit_sample(&self) -> (f64, f64) {
        self.hit_sample.take().unwrap_or_else(|| self.cell.sample())
    }
}

impl std::fmt::Debug for RenderAnimatedTransform {
    // From the cache alone: formatting the proxy would run the animation's
    // own `value()`/`status()`, which this node reads only in `attach` and
    // the tick listener.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderAnimatedTransform")
            .field("kind", &self.kind)
            .field("sample", &self.cell.sample())
            .field("transform_hit_tests", &self.transform_hit_tests)
            .field("has_child", &self.has_child)
            .finish_non_exhaustive()
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

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        let constraints = *ctx.constraints();
        let size = if ctx.child_count() > 0 {
            self.has_child = true;
            ctx.layout_child(0, constraints)?
        } else {
            self.has_child = false;
            constraints.smallest()
        };
        self.cell.store_size(size);
        Ok(size)
    }

    flui_rendering::forward_single_child_box_queries!();

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        // Only the child decides, as for `RenderTransform`: a moved or scaled
        // child is hittable where it paints, outside this node's own box.
        if !self.has_child {
            return false;
        }
        // The flag comes first: an untransformed hit does not depend on the
        // matrix, degenerate or not.
        if !self.transform_hit_tests {
            return ctx.hit_test_child(0, *ctx.position());
        }
        let size = ctx.own_size();
        let (class, matrix) = Self::class_and_matrix(self.kind, self.take_hit_sample(), size);
        match class {
            Class::Degenerate => false,
            Class::Identity => ctx.hit_test_child(0, *ctx.position()),
            Class::Layered => {
                let Some(inverse) = matrix.try_inverse() else {
                    return false;
                };
                let position = ctx.position();
                let Some((x, y)) = inverse.unproject_to_plane(position.dx, position.dy) else {
                    return false;
                };
                ctx.hit_test_child(0, Offset::new(x, y))
            }
        }
    }

    fn skip_paint(&self) -> bool {
        self.current(self.cell.size()).0 == Class::Degenerate
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        // The layered matrix is reported through `paint_effects`, which the
        // pipeline pushes before replaying this fragment; a bare splice here.
        if self.has_child {
            ctx.paint_child();
        }
    }

    fn paint_effects(&self, size: Size) -> PaintEffects {
        if !self.has_child {
            return PaintEffects::NONE;
        }
        match self.current(size) {
            (Class::Layered, matrix) => PaintEffects::NONE.with_transform(matrix),
            _ => PaintEffects::NONE,
        }
    }

    fn apply_paint_transform(
        &self,
        _child: usize,
        child_offset: Offset,
        size: Size,
        transform: &mut Matrix4,
    ) {
        // A singular matrix composes (the child maps to a point); an
        // overflowed one is left out, so coordinate conversion falls back to
        // the untransformed position instead of publishing NaN or infinity.
        let (class, matrix) = self.current(size);
        if class != Class::Identity && is_finite(&matrix) {
            *transform *= matrix;
        }
        *transform *= Matrix4::translation(child_offset.dx, child_offset.dy, 0.0);
    }

    fn hit_test_transform(&self, size: Size) -> Option<Matrix4> {
        // Must agree with `hit_test`: an untransformed hit gets no matrix on
        // its entry, or the delivered position would be mapped through a
        // transform the hit did not use.
        if !self.transform_hit_tests {
            return None;
        }
        let (class, matrix) = Self::class_and_matrix(self.kind, self.arm_hit_sample(), size);
        (class == Class::Layered).then_some(matrix)
    }

    fn attach(&mut self, handle: RenderInvalidationHandle) {
        self.listener_id = Some(self.source.subscribe(&self.cell, self.kind, &handle));
        // Catch up with a change made while nothing listened (a reattach).
        // Its ticket is taken before the read like a listener's, so a
        // listener delivery that read later wins over this reading.
        let generation = self.cell.ticket();
        commit(
            &self.cell,
            self.kind,
            generation,
            self.source.read(),
            &handle,
        );
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    /// An owner with one scale node, the handle bound to it, and the owner's
    /// visual-update wake set to `wake` (it fires inside every mark send).
    fn owner_with_handle(
        wake: impl Fn() + Send + Sync + 'static,
    ) -> (PipelineOwner, RenderInvalidationHandle) {
        let controller = AnimationController::builder(Duration::from_millis(100)).build();
        controller.set_value(1.0);
        let proxy =
            ProxyAnimation::new(std::rc::Rc::new(controller) as std::rc::Rc<dyn Animation<f64>>);
        let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
        owner.set_on_need_visual_update(wake);
        let anchor = owner.insert(
            Box::new(RenderAnimatedTransform::new(TransformMotion::Scale {
                scale: proxy,
            })) as Box<dyn flui_rendering::traits::RenderObject<BoxProtocol>>,
        );
        let handle = owner
            .render_invalidation_handle(anchor)
            .expect("just-inserted id must be live");
        (owner, handle)
    }

    fn scale_cell() -> SampleCell {
        let cell = SampleCell::new((1.0, 0.0));
        cell.store_size(Size::new(40.0, 40.0));
        cell
    }

    // A failure path the public surface cannot reach: the mark send fails
    // (the pipeline owner is gone). The delivery debt must survive it, so the
    // next tick re-sends the marks even when it carries the same value.
    #[test]
    fn failed_mark_retries() {
        let (owner, dead) = owner_with_handle(|| {});
        drop(owner);
        let cell = scale_cell();
        assert!(
            !commit(&cell, Kind::Scale, cell.ticket(), (0.5, 0.0), &dead),
            "a failed send must report the change undelivered"
        );
        let (_owner, live) = owner_with_handle(|| {});
        assert!(
            commit(&cell, Kind::Scale, cell.ticket(), (0.5, 0.0), &live),
            "the next tick with the same value must retry the owed marks"
        );
    }

    // A user read or invalidation hook can reenter. The newer ticket stays
    // published and delivered even if the outgoing call returns last.
    #[test]
    fn newer_generation_wins_in_either_commit_order() {
        let (_owner, handle) = owner_with_handle(|| {});
        for older_finishes_last in [false, true] {
            let cell = scale_cell();
            let older = cell.ticket();
            let newer = cell.ticket();
            let commits = [(older, (0.5, 0.0)), (newer, (0.25, 0.0))];
            let order: Vec<_> = if older_finishes_last {
                commits.iter().rev().collect()
            } else {
                commits.iter().collect()
            };
            for &&(generation, value) in &order {
                commit(&cell, Kind::Scale, generation, value, &handle);
            }
            assert_eq!(
                cell.sample(),
                (0.25, 0.0),
                "older finishes last = {older_finishes_last}: the newer reading must stay published"
            );
            assert_eq!(
                cell.delivered.get(),
                Versioned {
                    generation: newer,
                    value: (0.25, 0.0)
                },
                "older finishes last = {older_finishes_last}: delivery must record the newer commit"
            );
        }

        let cell = scale_cell();
        cell.tickets.set(u64::MAX - 1);
        assert!(commit(
            &cell,
            Kind::Scale,
            cell.ticket(),
            (0.5, 0.0),
            &handle
        ));
        for value in [(0.25, 0.0), (1.0, 0.0)] {
            assert!(
                !commit(&cell, Kind::Scale, cell.ticket(), value, &handle),
                "exhaustion must permanently refuse newer readings"
            );
            assert_eq!(cell.sample(), (0.5, 0.0));
            assert_eq!(cell.delivered.get().value, (0.5, 0.0));
        }
    }

    // A (delivered) -> B -> A: B's commit published and marked, then stalled
    // before recording delivery; the frame it woke may already show B. The
    // final A equals the stale delivery record yet must still mark, and B's
    // late record must not replace A's.
    #[test]
    fn restoring_a_delivered_value_while_a_newer_one_is_in_flight_still_marks() {
        let (_owner, handle) = owner_with_handle(|| {});
        let cell = scale_cell();
        let a = (0.5, 0.0);
        let b = (0.25, 0.0);
        assert!(commit(&cell, Kind::Scale, cell.ticket(), a, &handle));
        let stalled = Versioned {
            generation: cell.ticket(),
            value: b,
        };
        assert!(publish_newer(&cell.published, stalled).is_some());
        assert!(
            commit(&cell, Kind::Scale, cell.ticket(), a, &handle),
            "the restored value owes marks while B's delivery is in flight"
        );
        assert!(publish_newer(&cell.delivered, stalled).is_none());
        assert_eq!(cell.delivered.get().value, a);
    }

    fn rotation_node(turns: f64) -> RenderAnimatedTransform {
        let controller = AnimationController::builder(Duration::from_millis(1))
            .unbounded()
            .build();
        controller.set_value(turns);
        RenderAnimatedTransform::new(TransformMotion::Rotation {
            turns: ProxyAnimation::new(
                std::rc::Rc::new(controller) as std::rc::Rc<dyn Animation<f64>>
            ),
        })
    }

    // Rotation is periodic: a huge finite turn count is a whole number of
    // turns, so the child stays visible and untransformed rather than the
    // TAU multiplication overflowing into a degenerate matrix.
    #[test]
    fn huge_finite_turns_stay_visible() {
        let node = rotation_node(f64::MAX);
        let size = Size::new(40.0, 40.0);
        node.cell.store_size(size);
        let (class, m) = node.current(size);
        assert_ne!(class, Class::Degenerate, "{m:?}");
        assert!(is_finite(&m));
    }

    // A tick between `hit_test_transform` and `hit_test` must not change the
    // matrix `hit_test` positions the child with: both use the visit's one
    // sample.
    #[test]
    fn one_sample_per_hit_test_visit() {
        let controller = AnimationController::builder(Duration::from_millis(1))
            .unbounded()
            .build();
        controller.set_value(0.25);
        let node = RenderAnimatedTransform::new(TransformMotion::Rotation {
            turns: ProxyAnimation::new(
                std::rc::Rc::new(controller.clone()) as std::rc::Rc<dyn Animation<f64>>
            ),
        });
        let (_owner, handle) = owner_with_handle(|| {});
        let mut node = node;
        node.attach(handle);
        let size = Size::new(40.0, 40.0);
        let entry = node
            .hit_test_transform(size)
            .expect("a quarter turn is layered");
        controller.set_value(0.125);
        assert_eq!(
            node.cell.sample(),
            (0.125, 0.0),
            "the tick reached the cache"
        );
        node.has_child = true;
        let observed = std::cell::Cell::new(None);
        let mut hit_child = |index, position, _transform| {
            assert_eq!(index, 0);
            observed.set(position);
            true
        };
        let inner =
            flui_rendering::protocol::BoxHitTestCtx::<Single, BoxParentData>::with_child_callback(
                Offset::new(30.0, 20.0),
                &mut hit_child,
            );
        let mut ctx = BoxHitTestContext::new(inner, size);
        assert!(node.hit_test(&mut ctx));
        let position = observed.get().expect("child was hit");
        assert!((position.dx - 20.0).abs() < 1e-10);
        assert!(
            (position.dy - 10.0).abs() < 1e-10,
            "the child must use the quarter-turn entry, not the later eighth turn: {position:?}; {entry:?}"
        );
        node.detach();
    }

    #[derive(Debug)]
    struct Probe(Arc<AtomicBool>, flui_foundation::ChangeNotifier);

    impl Listenable for Probe {
        fn add_listener(&self, callback: flui_foundation::ListenerCallback) -> ListenerId {
            self.1.add_listener(callback)
        }
        fn remove_listener(&self, id: ListenerId) {
            self.1.remove_listener(id);
        }
        fn remove_all_listeners(&self) {
            self.1.remove_all_listeners();
        }
    }

    impl Animation<f64> for Probe {
        fn value(&self) -> f64 {
            self.0.store(true, Ordering::SeqCst);
            1.0
        }
        fn status(&self) -> flui_animation::AnimationStatus {
            self.0.store(true, Ordering::SeqCst);
            flui_animation::AnimationStatus::Dismissed
        }

        fn subscribe_status(
            &self,
            callback: flui_animation::StatusCallback,
        ) -> flui_animation::StatusSubscription {
            drop(callback);
            flui_animation::StatusSubscription::default()
        }
    }

    // Debug formats the cache, never the animation.
    #[test]
    fn debug_does_not_read_the_animation() {
        let called = Arc::new(AtomicBool::new(false));
        let node = RenderAnimatedTransform::new(TransformMotion::Scale {
            scale: ProxyAnimation::new(std::rc::Rc::new(Probe(
                Arc::clone(&called),
                flui_foundation::ChangeNotifier::default(),
            )) as std::rc::Rc<dyn Animation<f64>>),
        });
        called.store(false, Ordering::SeqCst);
        let text = format!("{node:?}");
        assert!(text.contains("sample"), "{text}");
        assert!(
            !called.load(Ordering::SeqCst),
            "Debug ran the animation: {text}"
        );
    }
}
