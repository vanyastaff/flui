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
//! Marks are fallible sends ([`RenderInvalidationHandle`]) that may wake a
//! frame on another thread before they return, so a tick publishes the new
//! sample *before* marking: the frame a mark wakes always reads it. Delivery
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

use std::f64::consts::TAU;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering, fence};
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

/// One buffer of a [`Register`]: a sequence counter (odd while written)
/// guarding a generation and a two-word value.
#[derive(Debug, Default)]
struct Slot {
    sequence: AtomicU64,
    generation: AtomicU64,
    first: AtomicU64,
    second: AtomicU64,
}

impl Slot {
    /// A consistent read, or `None` when a write overlapped it.
    fn try_read(&self) -> Option<Versioned> {
        let start = self.sequence.load(Ordering::Acquire);
        if !start.is_multiple_of(2) {
            return None;
        }
        let generation = self.generation.load(Ordering::Relaxed);
        let first = self.first.load(Ordering::Relaxed);
        let second = self.second.load(Ordering::Relaxed);
        fence(Ordering::Acquire);
        (self.sequence.load(Ordering::Relaxed) == start).then_some(Versioned {
            generation,
            value: (f64::from_bits(first), f64::from_bits(second)),
        })
    }

    /// Writes `entry` with the sequence odd for the duration.
    fn write(&self, entry: Versioned) {
        self.sequence.fetch_add(1, Ordering::Relaxed);
        fence(Ordering::Release);
        self.generation.store(entry.generation, Ordering::Relaxed);
        self.first.store(entry.value.0.to_bits(), Ordering::Relaxed);
        self.second
            .store(entry.value.1.to_bits(), Ordering::Relaxed);
        self.sequence.fetch_add(1, Ordering::Release);
    }
}

/// A two-word value that concurrent writers publish in generation order and
/// a paint or hit-test walk reads without a lock.
///
/// # Protocol
///
/// Two [`Slot`]s; `current` names the one holding the newest published
/// entry. Writers exclude each other with the `writing` flag, taken by
/// compare-exchange, held for a few stores and never across user code; a
/// reader never touches it. The holder compares its generation with the
/// current entry's: an older or equal one is discarded, so the published
/// generation only grows and an older reading never replaces a newer one.
/// A newer entry is written into the slot that is *not* current, then
/// `current` flips to it with `Release`.
///
/// A reader loads `current` and reads that slot under its sequence. A writer
/// stalled mid-write owns only the slot that is not current, so it never
/// blocks a reader. A read fails only when two newer entries were published
/// while it ran (the second reusing its slot), so every reader retry follows
/// another writer's completed publication, never a stalled one.
#[derive(Debug, Default)]
struct Register {
    writing: AtomicBool,
    slots: [Slot; 2],
    current: AtomicUsize,
}

impl Register {
    fn new(entry: Versioned) -> Self {
        let register = Self::default();
        register.slots[0].write(entry);
        register
    }

    fn load(&self) -> Versioned {
        loop {
            let current = self.current.load(Ordering::Acquire);
            if let Some(entry) = self.slots[current].try_read() {
                return entry;
            }
            std::hint::spin_loop();
        }
    }

    /// Publishes `entry` unless a newer-or-equal generation already is.
    /// Returns the generation it replaced, or `None` when it was discarded.
    fn publish(&self, entry: Versioned) -> Option<u64> {
        while self
            .writing
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            std::hint::spin_loop();
        }
        let current = self.current.load(Ordering::Relaxed);
        let newest = self.slots[current]
            .try_read()
            .expect("BUG: only the `writing` holder writes a slot, and never the current one");
        let published = (entry.generation > newest.generation).then_some(newest.generation);
        if published.is_some() {
            let next = 1 - current;
            self.slots[next].write(entry);
            self.current.store(next, Ordering::Release);
        }
        self.writing.store(false, Ordering::Release);
        published
    }
}

/// The published sample, the last delivered sample and the size they are
/// mapped against.
///
/// Atomics because the render tree is `Send` today and the tick listener
/// must be `Send + Sync`, so two notifications of one animation may commit
/// concurrently. Each commit takes a [`ticket`](Self::ticket) *before*
/// reading the animation, so the newest ticket's reading was taken after
/// every mutation already notified: generation order is reading order, and
/// the newest generation holds the final value. Both samples are
/// [`Register`]s ordered by that generation; the published one is written
/// with `Release` before any mark can wake a frame. The size is written only
/// by layout, so `Relaxed` suffices for it.
#[derive(Debug)]
struct SampleCell {
    tickets: AtomicU64,
    published: Register,
    delivered: Register,
    width: AtomicU64,
    height: AtomicU64,
}

impl SampleCell {
    fn new(sample: (f64, f64)) -> Self {
        let seed = Versioned {
            generation: 0,
            value: sample,
        };
        Self {
            tickets: AtomicU64::new(0),
            published: Register::new(seed),
            delivered: Register::new(seed),
            width: AtomicU64::new(0),
            height: AtomicU64::new(0),
        }
    }

    /// The generation for a reading about to be taken. Saturates: a cell
    /// that exhausted `u64` keeps its last sample rather than reissuing an
    /// older generation.
    fn ticket(&self) -> u64 {
        self.tickets
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
            .map_or(u64::MAX, |n| n + 1)
    }

    /// The published sample, never a mix of two writes.
    fn sample(&self) -> (f64, f64) {
        self.published.load().value
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
    let Some(replaced) = cell.published.publish(entry) else {
        return false;
    };
    let delivered = cell.delivered.load();
    let old = delivered.value;
    // A replaced publication newer than the delivery record is in flight (or
    // its marks failed): a frame may already show it, so equality with the
    // delivered value proves nothing and the class it painted is unknown.
    let in_flight = replaced > delivered.generation;
    if !in_flight && old.0.to_bits() == value.0.to_bits() && old.1.to_bits() == value.1.to_bits() {
        // This generation owes no marks. Record its completed delivery so a
        // later equal notification cannot mistake it for outstanding work.
        let _ = cell.delivered.publish(entry);
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
    let _ = cell.delivered.publish(entry);
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
pub struct RenderAnimatedTransform {
    source: Source,
    kind: Kind,
    cell: Arc<SampleCell>,
    /// The sample `hit_test_transform` chose for the current hit-test visit,
    /// consumed by the `hit_test` that follows it, so a tick between the two
    /// hooks cannot pair one matrix on the hit entry with another for the
    /// child's position.
    hit_sample: Slot,
    hit_sample_armed: AtomicBool,
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
            hit_sample: Slot::default(),
            hit_sample_armed: AtomicBool::new(false),
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
        self.hit_sample.write(Versioned {
            generation: 0,
            value: sample,
        });
        self.hit_sample_armed.store(true, Ordering::Release);
        sample
    }

    /// The sample `hit_test_transform` recorded for this visit, or a fresh
    /// read when `hit_test` runs alone.
    fn take_hit_sample(&self) -> (f64, f64) {
        if self.hit_sample_armed.swap(false, Ordering::AcqRel)
            && let Some(entry) = self.hit_sample.try_read()
        {
            return entry.value;
        }
        self.cell.sample()
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
    use std::time::Duration;

    /// An owner with one scale node, the handle bound to it, and the owner's
    /// visual-update wake set to `wake` (it fires inside every mark send).
    fn owner_with_handle(
        wake: impl Fn() + Send + Sync + 'static,
    ) -> (PipelineOwner, RenderInvalidationHandle) {
        let controller = AnimationController::without_ticker(Duration::from_millis(100));
        controller.set_value(1.0);
        let proxy = ProxyAnimation::new(Arc::new(controller) as Arc<dyn Animation<f64>>);
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

    // A mark wakes the frame, which may run on another thread before the
    // send returns. Modelled by a wake that reads the cache synchronously,
    // as that frame would: it must observe the new sample.
    #[test]
    fn frame_woken_by_a_mark_reads_the_new_sample() {
        let cell = Arc::new(scale_cell());
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (_owner, handle) = owner_with_handle({
            let cell = Arc::clone(&cell);
            let seen = Arc::clone(&seen);
            move || seen.lock().expect("unpoisoned").push(cell.sample())
        });
        seen.lock().expect("unpoisoned").clear();
        assert!(commit(
            &cell,
            Kind::Scale,
            cell.ticket(),
            (0.5, 0.0),
            &handle
        ));
        let seen = seen.lock().expect("unpoisoned");
        assert!(!seen.is_empty(), "the mark must have woken a frame");
        assert!(
            seen.iter().all(|&sample| sample == (0.5, 0.0)),
            "every woken frame must read the new sample: {seen:?}"
        );
    }

    // Two notifications of one animation commit concurrently; whichever
    // finishes last, the reading taken last (the newer ticket) stays
    // published and delivered. The older-finishing-last order is also the
    // attach catch-up race: its ticket precedes a listener's that read later.
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
                cell.delivered.load(),
                Versioned {
                    generation: newer,
                    value: (0.25, 0.0)
                },
                "older finishes last = {older_finishes_last}: delivery must record the newer commit"
            );
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
        assert!(cell.published.publish(stalled).is_some());
        assert!(
            commit(&cell, Kind::Scale, cell.ticket(), a, &handle),
            "the restored value owes marks while B's delivery is in flight"
        );
        assert!(cell.delivered.publish(stalled).is_none());
        assert_eq!(cell.delivered.load().value, a);
    }

    fn rotation_node(turns: f64) -> RenderAnimatedTransform {
        let controller = AnimationController::unbounded_without_ticker(Duration::from_millis(1));
        controller.set_value(turns);
        RenderAnimatedTransform::new(TransformMotion::Rotation {
            turns: ProxyAnimation::new(Arc::new(controller) as Arc<dyn Animation<f64>>),
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
        let controller = AnimationController::unbounded_without_ticker(Duration::from_millis(1));
        controller.set_value(0.25);
        let node = RenderAnimatedTransform::new(TransformMotion::Rotation {
            turns: ProxyAnimation::new(Arc::new(controller.clone()) as Arc<dyn Animation<f64>>),
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
        fn add_status_listener(&self, _: flui_animation::StatusCallback) -> ListenerId {
            self.1.add_listener(Arc::new(|| {}))
        }
        fn remove_status_listener(&self, _: ListenerId) {}
    }

    // Debug formats the cache, never the animation.
    #[test]
    fn debug_does_not_read_the_animation() {
        let called = Arc::new(AtomicBool::new(false));
        let node = RenderAnimatedTransform::new(TransformMotion::Scale {
            scale: ProxyAnimation::new(Arc::new(Probe(
                Arc::clone(&called),
                flui_foundation::ChangeNotifier::default(),
            )) as Arc<dyn Animation<f64>>),
        });
        called.store(false, Ordering::SeqCst);
        let text = format!("{node:?}");
        assert!(text.contains("sample"), "{text}");
        assert!(
            !called.load(Ordering::SeqCst),
            "Debug ran the animation: {text}"
        );
    }

    // A writer preempted mid-write must not stall a paint-time read: it owns
    // only the slot that is not current.
    #[test]
    fn stalled_writer_does_not_block_readers() {
        let cell = Arc::new(scale_cell());
        let register = &cell.published;
        register.writing.store(true, Ordering::SeqCst);
        let idle = 1 - register.current.load(Ordering::SeqCst);
        register.slots[idle].sequence.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = std::sync::mpsc::channel();
        let reader = Arc::clone(&cell);
        std::thread::spawn(move || {
            let _ = sender.send(reader.sample());
        });
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(2)),
            Ok((1.0, 0.0)),
            "a read during a stalled write must return the last published sample"
        );
    }

    // Concurrent writers with interleaved tickets, a concurrent reader:
    // every read is a value some writer published, and the newest ticket's
    // value ends published.
    #[test]
    fn concurrent_writers_publish_the_newest_reading() {
        const PER_WRITER: u64 = 2_000;
        let cell = Arc::new(scale_cell());
        let (_owner, handle) = owner_with_handle(|| {});
        let done = Arc::new(AtomicBool::new(false));
        let reader = {
            let cell = Arc::clone(&cell);
            let done = Arc::clone(&done);
            std::thread::spawn(move || {
                while !done.load(Ordering::Acquire) {
                    let (first, second) = cell.sample();
                    // Writers publish (n, -n): a torn read breaks the pair.
                    assert!(
                        first == 1.0 && second == 0.0 || first == -second,
                        "torn read ({first}, {second})"
                    );
                }
            })
        };
        let writers: Vec<_> = (0..2)
            .map(|_| {
                let cell = Arc::clone(&cell);
                let handle = handle.clone();
                std::thread::spawn(move || {
                    for _ in 0..PER_WRITER {
                        let generation = cell.ticket();
                        #[expect(clippy::cast_precision_loss, reason = "test values stay small")]
                        let n = generation as f64;
                        commit(&cell, Kind::Scale, generation, (n, -n), &handle);
                    }
                })
            })
            .collect();
        for writer in writers {
            writer.join().expect("writer must not panic");
        }
        done.store(true, Ordering::Release);
        reader
            .join()
            .expect("reader must not observe a torn sample");
        #[expect(clippy::cast_precision_loss, reason = "test values stay small")]
        let last = (2 * PER_WRITER) as f64;
        assert_eq!(cell.sample(), (last, -last));
    }
}
