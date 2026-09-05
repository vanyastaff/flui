//! [`Draggable`] — a widget that can be picked up and dragged, carrying typed
//! `data` for a [`DragTarget`](crate::DragTarget) to receive on drop.
//!
//! Flutter parity: `widgets/drag_target.dart` (tag `3.44.0`) — `Draggable`,
//! `_DraggableState`, `DraggableDetails`, `_DragAvatar`. `LongPressDraggable`
//! and `DragAnchorStrategy` are named deferrals; see the module docs below.
//!
//! # Deliberate divergences from the oracle (framework-surface gaps)
//!
//! 1. **Feedback paints, but at a displacement, not a global position.**
//!    `Overlay::maybe_of` (ADR-0036) closed the lookup gap this divergence
//!    used to name in full: `DraggableState` now resolves the ancestor
//!    `Overlay` in `did_change_dependencies` and, on drag start, inserts
//!    `feedback` as a real `OverlayEntry` — matching the oracle's
//!    `_DragAvatar`, which does the same through `Overlay.of(context)`. What
//!    remains a divergence is *where* it paints: the oracle's `_lastOffset`
//!    is anchored at the drag's true global position (`dragAnchorStrategy`
//!    plus the pointer's live global coordinates — see divergence #4 below).
//!    This port has neither a `dragAnchorStrategy` nor the global-origin term
//!    divergence #4 already names as missing, so the feedback entry positions
//!    itself at `feedback_offset` plus the same **displacement-since-start**
//!    `DragSession` already tracks for `DraggableDetails.offset` — visibly
//!    correct only for a `Draggable` sitting at the screen origin, honestly
//!    wrong (by exactly that origin) everywhere else, same shape of divergence
//!    as #4. `rootOverlay`, `ignoringFeedback*`, and scaled/rotated-ancestor
//!    correctness are separate, still-open gaps (ADR-0036's deferrals).
//! 2. **Live drag-target discovery, reached through a private origin probe.**
//!    The oracle's `_DragAvatar.updateDrag` hit-tests at the pointer's
//!    *current* global position on every move, independent of wherever the
//!    drag's own pointer went down, and walks the result for
//!    `RenderMetaData`-tagged `DragTarget`s. FLUI does the same now:
//!    `BuildContext::hit_test_handle()` (acquired in `init_state` /
//!    `did_change_dependencies`, never from a frame phase) runs a fresh test
//!    against the live render tree, and [`DragTarget`](crate::DragTarget)
//!    publishes an `Arc<DragTargetSlot>` as its hit-test payload for the walk
//!    to find. Pointer dispatch still resolves its own route once at
//!    `PointerDown` and replays it; the fresh probe is deliberately
//!    independent of that route, which is the whole point.
//!
//!    **The divergence is where the position comes from.** Flutter's
//!    `PointerEvent` carries `position` (global) *and* `localPosition`, so a
//!    widget always has both. FLUI's pointer events are `ui_events` types with
//!    room for one position, and dispatch rewrites it into the receiving
//!    entry's local space (`HitTestResult`'s per-entry
//!    `transform_pointer_event`) — so a gesture callback knows only where the
//!    pointer is inside its *own* node. A hit test needs the root's space. The
//!    conversion between exactly those two spaces is
//!    `PipelineOwner::local_to_global`, which needs the `RenderId` of the node
//!    the local point belongs to, and [`DragOrigin`] — a payload-free view
//!    mounted as the `Listener`'s direct child — is how this widget learns it:
//!    its `find_render_object()` stops at the `Listener`'s own render node,
//!    the node dispatch localized against. The conversion composes the whole
//!    ancestor chain, so it is exact under scale and rotation, not just
//!    translation.
//!
//!    Two consequences worth naming rather than discovering later. A drag
//!    carrying **no data** discovers nothing at all: a target's
//!    `isExpectedDataType` filter has nothing to match, and the oracle's
//!    null-data drag — which enters *every* target — has no representation
//!    here (`ErasedDragData` erases a concrete value, not an `Option`). And
//!    `axis` restriction is applied to deltas in the `Listener`'s space rather
//!    than the root's, which differs from the oracle only under a rotating
//!    ancestor.
//!
//! 3. **No `LongPressDraggable`.** The oracle's variant swaps in a
//!    `DelayedMultiDragGestureRecognizer`, which does not exist in
//!    `flui-interaction` yet (only the immediate `MultiDragGestureRecognizer`
//!    is ported). Deferred rather than hand-rolling a new recognizer as a
//!    side effect of this port.
//! 4. **No configurable `dragAnchorStrategy`, `affinity`, `hitTestBehavior`,
//!    `ignoringFeedback*`, `rootOverlay`, `allowedButtonsFilter`.**
//!    `ignoringFeedback*`/`rootOverlay` only affect the feedback overlay
//!    (moot per point 1). `affinity` selects which single-axis recognizer
//!    competes for the *start* of the gesture — a named deferral, unrelated
//!    to `Draggable::axis` (implemented), which restricts *reported*
//!    movement after the drag has already started
//!    (`_DragAvatar._restrictAxis`). `dragAnchorStrategy` is **not** merely
//!    cosmetic feedback positioning: it defines `dragStartPoint`, which the
//!    oracle subtracts from every reported global position to produce
//!    `DraggableDetails.offset` / `DragTargetDetails.offset`
//!    (`_DragAvatar.updateDrag`'s `_lastOffset = globalPosition -
//!    dragStartPoint`).
//!
//!    **A further, separately-named divergence in `_lastOffset` itself,**
//!    found while pinning this down precisely: under the default
//!    `childDragAnchorStrategy`, `dragStartPoint = renderObject.globalToLocal(initialPosition)`
//!    — a LOCAL offset — while `globalPosition` in the formula above is
//!    GLOBAL. Writing `globalOrigin` for `Draggable`'s own render object's
//!    global top-left corner, `initialPosition = globalOrigin +
//!    dragStartPoint` by definition of `globalToLocal`, so the formula
//!    reduces to `_lastOffset(t) = globalOrigin + Σ(axis-restricted deltas
//!    since the drag started)` — **not** just the running sum. The running
//!    sum alone (which is all [`DragSession::offset`] tracks: seeded at
//!    `Offset::ZERO`, never given a `globalOrigin` term) is correct only for
//!    a `Draggable` whose render object sits at the screen origin; for any
//!    other position, this port's reported offset is short by exactly that
//!    origin. **The blocker this note used to name is gone**: point 2's origin
//!    probe now converts a `Listener`-local point to the root's space through
//!    `PipelineOwner::local_to_global`, which is exactly the `globalOrigin`
//!    term this formula wants. Closing it is nonetheless a separate change —
//!    it alters what `DraggableDetails.offset` and `on_draggable_canceled`
//!    report to existing callers, and `dragAnchorStrategy` (which decides
//!    `dragStartPoint`) has to land with it or the "fix" would be a different
//!    wrong value. What this port ships is still **displacement since the drag
//!    started**, not the oracle's globally-anchored value — pinned by
//!    `draggable_test.rs`'s `reported_offset_is_displacement_not_global_position`,
//!    which lays the `Draggable` under a nonzero `Padding` specifically so a
//!    future accidental "fix" that seeds the offset with *some* base instead
//!    of `Offset::ZERO` is still caught red-handed for shipping the *wrong*
//!    base rather than silently looking correct at the origin.
//!
//!    Separately, `pointerDragAnchorStrategy` (anchor at `Offset.zero`) is
//!    not selectable at all — that is the actual, named deferral for
//!    *strategy choice*, distinct from the `_lastOffset` divergence above.
//! 5. **Unmounting mid-drag still cancels immediately.** The oracle's
//!    `_disposeRecognizerIfInactive` transfers the recognizer and overlay
//!    lifetime to active drag avatars until their real pointer-up. This port
//!    still keeps both resources on `DraggableState`, so unmount disposes the
//!    recognizer and removes feedback immediately. `MultiDragHandle` is now
//!    correctly owner-local, removing the former type-system obstacle; the
//!    remaining work is a real lifetime transfer, not a threading workaround.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_interaction::{
    DragUpdateDetails, GestureRecognizer, HitTestEntry, HitTestHandle, MultiDragAxis,
    MultiDragEndDetails, MultiDragGestureRecognizer, MultiDragHandle, MultiDragStartCallback,
    MultiDragUpdateDetails, PointerEvent, PointerEventExt as _, PointerId, Velocity,
};
use flui_types::{
    Offset,
    geometry::{Matrix4, PixelDelta, Pixels},
    layout::Axis,
};
use flui_view::RebuildHandle;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use parking_lot::Mutex;

use crate::overlay::{InsertPosition, Overlay, OverlayEntry, OverlayHandle};
use crate::{
    DragPosition, DragTargetSlot, ErasedDragData, GestureArenaScope, Listener, Positioned, Stack,
    StackFit,
};

/// A no-argument callback retained in the current shared drag-config snapshot.
///
/// `MultiDragHandle` itself is owner-local; these `Arc` bounds are legacy
/// storage shape, not a cross-thread callback contract.
type StartedCallback = Arc<dyn Fn() + Send + Sync>;
/// Called for each pointer move while a drag is in progress.
type DragUpdateCallback = Arc<dyn Fn(DragUpdateDetails) + Send + Sync>;
/// Called once when a drag ends, accepted or not.
type DragEndCallback = Arc<dyn Fn(DraggableDetails) + Send + Sync>;
/// Called when a drag ends without being accepted by a target.
type DraggableCanceledCallback = Arc<dyn Fn(Velocity, Offset<Pixels>) + Send + Sync>;

/// Details for [`Draggable::on_drag_end`] — the velocity and position at
/// release, and whether a [`DragTarget`](crate::DragTarget) accepted the drop.
///
/// Flutter parity: `DraggableDetails`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DraggableDetails {
    /// Whether a `DragTarget` accepted this drop — `true` when the drag
    /// ended over a target that had taken it as a candidate and whose element
    /// was still mounted to receive it.
    pub was_accepted: bool,
    /// Velocity at release.
    pub velocity: Velocity,
    /// Displacement since the drag started — the running sum of every
    /// axis-restricted delta, not a raw global position. See the module
    /// divergence note #4: the oracle's `_lastOffset` adds the draggable's
    /// global origin on top of this sum; this port does not (a named,
    /// pinned divergence, not a raw position either way).
    pub offset: Offset<Pixels>,
}

/// A widget that can be dragged, carrying `data` for a
/// [`DragTarget`](crate::DragTarget) to receive.
///
/// Flutter parity: `widgets/drag_target.dart` `Draggable`. See the module
/// docs for the divergences, notably where the drag's global position comes
/// from.
#[derive(Clone, StatefulView)]
pub struct Draggable<T: Clone + Send + Sync + 'static> {
    child: Child,
    child_when_dragging: Option<Rc<dyn Fn() -> BoxedView>>,
    feedback: Option<Rc<dyn Fn() -> BoxedView>>,
    data: Option<T>,
    axis: Option<Axis>,
    feedback_offset: Offset<Pixels>,
    max_simultaneous_drags: Option<usize>,
    on_drag_started: Option<StartedCallback>,
    on_drag_update: Option<DragUpdateCallback>,
    on_draggable_canceled: Option<DraggableCanceledCallback>,
    on_drag_end: Option<DragEndCallback>,
    on_drag_completed: Option<StartedCallback>,
}

impl<T: Clone + Send + Sync + 'static> std::fmt::Debug for Draggable<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Draggable")
            .field("has_data", &self.data.is_some())
            .field("axis", &self.axis)
            .field("max_simultaneous_drags", &self.max_simultaneous_drags)
            .finish_non_exhaustive()
    }
}

impl<T: Clone + Send + Sync + 'static> Draggable<T> {
    /// A draggable with `child` as both its at-rest and mid-drag appearance,
    /// no feedback, and no data. Build up with the setter methods.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            child: Child::some(child.into_view()),
            child_when_dragging: None,
            feedback: None,
            data: None,
            axis: None,
            feedback_offset: Offset::ZERO,
            max_simultaneous_drags: None,
            on_drag_started: None,
            on_drag_update: None,
            on_draggable_canceled: None,
            on_drag_end: None,
            on_drag_completed: None,
        }
    }

    /// The data this draggable carries — delivered to a `DragTarget` on drop.
    #[must_use]
    pub fn data(mut self, data: T) -> Self {
        self.data = Some(data);
        self
    }

    /// Restricts reported drag movement to one axis (`_DragAvatar._restrictAxis`).
    #[must_use]
    pub fn axis(mut self, axis: Axis) -> Self {
        self.axis = Some(axis);
        self
    }

    /// The widget shown instead of `child` while one or more drags are active.
    /// Built lazily (no data to carry) each time it is needed.
    #[must_use]
    pub fn child_when_dragging(mut self, builder: impl Fn() -> BoxedView + 'static) -> Self {
        self.child_when_dragging = Some(Rc::new(builder));
        self
    }

    /// The widget shown under the pointer during a drag, painted in an
    /// `OverlayEntry` if an ancestor `Overlay` is found (`Overlay::maybe_of`,
    /// ADR-0036) — positioned at a **displacement**, not the oracle's true
    /// global anchor; see the module divergence notes.
    #[must_use]
    pub fn feedback(mut self, builder: impl Fn() -> BoxedView + 'static) -> Self {
        self.feedback = Some(Rc::new(builder));
        self
    }

    /// Offset from the drag anchor to where `feedback` is painted, added to
    /// the tracked displacement (see the module divergence notes).
    #[must_use]
    pub fn feedback_offset(mut self, offset: Offset<Pixels>) -> Self {
        self.feedback_offset = offset;
        self
    }

    /// Caps how many drags may be active at once. `Some(0)` disables
    /// dragging entirely; `None` (default) allows unlimited concurrent drags.
    #[must_use]
    pub fn max_simultaneous_drags(mut self, max: usize) -> Self {
        self.max_simultaneous_drags = Some(max);
        self
    }

    /// Called when the recognizer wins its pointer's arena and starts a drag.
    ///
    /// A lone immediate draggable wins by the arena's deferred default after
    /// Down and therefore starts without movement. With competitors (for
    /// example, inside a scrollable), movement past the recognizer's slop can
    /// be what resolves the competition.
    #[must_use]
    pub fn on_drag_started(mut self, callback: impl Fn() + Send + Sync + 'static) -> Self {
        self.on_drag_started = Some(Arc::new(callback));
        self
    }

    /// Called for each pointer move while the drag is in progress.
    #[must_use]
    pub fn on_drag_update(
        mut self,
        callback: impl Fn(DragUpdateDetails) + Send + Sync + 'static,
    ) -> Self {
        self.on_drag_update = Some(Arc::new(callback));
        self
    }

    /// Called when the drag ends without a target accepting it — including
    /// every cancel, and every drop over nothing.
    #[must_use]
    pub fn on_draggable_canceled(
        mut self,
        callback: impl Fn(Velocity, Offset<Pixels>) + Send + Sync + 'static,
    ) -> Self {
        self.on_draggable_canceled = Some(Arc::new(callback));
        self
    }

    /// Called once the drag ends, accepted or not.
    #[must_use]
    pub fn on_drag_end(
        mut self,
        callback: impl Fn(DraggableDetails) + Send + Sync + 'static,
    ) -> Self {
        self.on_drag_end = Some(Arc::new(callback));
        self
    }

    /// Called when a target accepts the drop. Fires instead of
    /// [`on_draggable_canceled`](Self::on_draggable_canceled), never
    /// alongside it.
    #[must_use]
    pub fn on_drag_completed(mut self, callback: impl Fn() + Send + Sync + 'static) -> Self {
        self.on_drag_completed = Some(Arc::new(callback));
        self
    }
}

/// Persistent gesture state: the recognizer survives rebuilds (the pointer
/// stream is stateful) and is disposed on unmount — see
/// `DragSession`'s docs for why this diverges from the oracle's
/// `_disposeRecognizerIfInactive` keep-alive. Mirrors `GestureDetectorState`'s
/// init_state-acquires-the-arena shape.
pub struct DraggableState<T: Clone + Send + Sync + 'static> {
    /// How many drags this widget currently has active — gates
    /// `max_simultaneous_drags` and switches `child` vs `child_when_dragging`.
    active_count: Arc<AtomicUsize>,
    /// The live config the recognizer's `on_start` closure reads at drag-start
    /// time (data, callbacks, axis, max-drags). Refreshed each `build`.
    config: Arc<Mutex<DragConfig>>,
    /// The nearest ancestor `Overlay`'s handle, if any — resolved in
    /// `did_change_dependencies` (a lifecycle hook, per port-check trigger
    /// #22 and ADR-0018's pattern), not in `build` or from inside the
    /// `on_start` gesture callback, neither of which holds a `BuildContext`.
    /// `Arc<Mutex<_>>` so the `on_start` closure captured once in
    /// `init_state` always reads the latest resolution.
    overlay: Arc<Mutex<Option<OverlayHandle>>>,
    /// The fresh-hit-test capability, resolved in `init_state` /
    /// `did_change_dependencies` — a lifecycle hook, never `build` or a
    /// gesture callback (port-check trigger #22), because a hit test taken
    /// mid-frame reads a tree that phase is still mutating.
    ///
    /// Owner-local (`Rc<RefCell<_>>`, not `Arc<Mutex<_>>`): `HitTestHandle`
    /// holds an `Rc<dyn HitTestProbe>` and is `!Send` by construction, which
    /// is correct — the tree it probes is owner-affine. Shared by cell rather
    /// than copied into each session so a re-resolution reaches sessions that
    /// started before it.
    hit_test: Rc<RefCell<Option<HitTestHandle>>>,
    /// The render node pointer events reach this widget in the space of,
    /// published by the [`DragOrigin`] mounted under the `Listener`.
    listener_node: Rc<Cell<Option<flui_foundation::RenderId>>>,
    /// The render tree, for converting those local positions to the root's
    /// space. A lifecycle-acquired capability like the two above (port-check
    /// trigger #22): only ever read from a gesture callback, never from a
    /// frame phase.
    pipeline: Rc<RefCell<Option<flui_rendering::pipeline::PipelineCell>>>,
    /// The currently-mounted feedback layer, if any is showing. Owner-local
    /// (`Rc<RefCell<_>>`, not `Arc<Mutex<_>>`): only `on_start`, `build` and
    /// `dispose` — all owner-thread code — ever touch it. It remains
    /// state-owned in this implementation; [`FeedbackSignal`] lets the active
    /// session reposition it without retaining the entry itself.
    ///
    /// One slot, not one per session: with `max_simultaneous_drags > 1`,
    /// concurrent drags share this single feedback layer. A later session's
    /// `on_start` always evicts whatever an earlier one left here — removing
    /// a still-live occupant outright, not just a stale one an earlier
    /// session's own end/cancel hasn't gotten around to tearing down yet
    /// (`build`'s removal is deferred to the next rebuild, which a rapid
    /// restart — end, then a new drag starts before that rebuild drains —
    /// can easily race ahead of; evicting unconditionally here, not only
    /// when the slot looks "stale," is what closes that race).
    ///
    /// One named case this does **not** fix: if the session that currently
    /// owns the slot ends while some other, still-active session continues
    /// (and no new session ever starts to evict it), this slot stays `Some`
    /// — nothing clears it on that one session's end alone — so the layer is
    /// left mounted but frozen (no session left is writing to its
    /// `FeedbackSignal`) until `build` next observes zero active sessions
    /// and tears it down. An honest scope cut of the same root cause as
    /// eviction itself: no harness capability here can even drive two truly
    /// concurrent contacts to observe it directly (see this file's own
    /// module docs on that limitation).
    ///
    /// A second case this doc used to name is now fixed: eviction of a
    /// STILL-LIVE earlier session used to leave both sessions holding `Some`
    /// of the one shared [`FeedbackSignal`] carried on `DraggableState`, so
    /// both wrote offsets and the single mounted layer jittered between the
    /// two drags' displacements until either ended. `on_start` now mints a
    /// **fresh** [`FeedbackSignal::new`] for every inserted entry instead of
    /// reusing one shared field — the evicted session's `DragSession` keeps
    /// writing to *its own*, now-detached signal, which nothing reads (inert,
    /// the same shape as [`FeedbackSignal::reposition`]'s existing
    /// before-mount/after-unmount no-op).
    feedback_entry: Rc<RefCell<Option<OverlayEntry>>>,
    /// `feedback`/`feedback_offset`, refreshed each `build` — read by
    /// `on_start` at drag-start time. Owner-local (`Rc<RefCell<_>>`): see
    /// [`FeedbackConfig`]'s docs on why it cannot live in [`DragConfig`].
    feedback_config: Rc<RefCell<FeedbackConfig>>,
    /// Built once in `init_state` against the presentation arena.
    recognizer: Option<Arc<MultiDragGestureRecognizer>>,
    /// Ties this state to `Draggable<T>` even though no field stores a `T`
    /// directly (see [`DragConfig`]'s docs on why the session drops it).
    _data: std::marker::PhantomData<T>,
}

impl<T: Clone + Send + Sync + 'static> std::fmt::Debug for DraggableState<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DraggableState")
            .field("active_count", &self.active_count.load(Ordering::Acquire))
            .field("initialized", &self.recognizer.is_some())
            .finish_non_exhaustive()
    }
}

/// The live, per-rebuild configuration a drag session reads at start time and
/// throughout its lifetime. `Send + Sync` because it is read from inside a
/// [`MultiDragHandle`] impl.
///
/// `Draggable::data` is carried **erased** (`ErasedDragData`): a session hands
/// it to targets that cannot name `T`, and `Arc<dyn Any + Send + Sync>` is
/// what a hit-test-discovered target's callbacks downcast from.
/// `feedback`/`feedback_offset` are **not** carried here,
/// even though a session does read them at start: `feedback` is
/// `Rc<dyn Fn() -> BoxedView>`, which is `!Send` (owner-local, ADR-0027) and
/// would make this whole `Send + Sync`-bound struct `!Send` by infection —
/// see [`FeedbackConfig`], the separate owner-local cell `on_start` (itself
/// `Rc`-based, not `Send`-bound) reads directly instead.
struct DragConfig {
    axis: Option<Axis>,
    max_simultaneous_drags: Option<usize>,
    /// The drag's payload, type-erased for delivery to targets. `None` when
    /// the `Draggable` carries no data — such a drag discovers nothing, since
    /// a target's `isExpectedDataType` filter has nothing to match against
    /// (the oracle's null-data drag, which enters every target, has no
    /// representation here — a named gap, see the module docs).
    data: Option<ErasedDragData>,
    on_drag_started: Option<StartedCallback>,
    on_drag_update: Option<DragUpdateCallback>,
    on_draggable_canceled: Option<DraggableCanceledCallback>,
    on_drag_end: Option<DragEndCallback>,
    on_drag_completed: Option<StartedCallback>,
}

impl DragConfig {
    fn from_view<T: Clone + Send + Sync + 'static>(view: &Draggable<T>) -> Self {
        Self {
            axis: view.axis,
            max_simultaneous_drags: view.max_simultaneous_drags,
            data: view
                .data
                .clone()
                .map(|payload| Arc::new(payload) as ErasedDragData),
            on_drag_started: view.on_drag_started.clone(),
            on_drag_update: view.on_drag_update.clone(),
            on_draggable_canceled: view.on_draggable_canceled.clone(),
            on_drag_end: view.on_drag_end.clone(),
            on_drag_completed: view.on_drag_completed.clone(),
        }
    }
}

/// `feedback`/`feedback_offset`, refreshed each `build` — split out of
/// [`DragConfig`] because `Rc<dyn Fn() -> BoxedView>` is `!Send` (ADR-0027);
/// only `on_start` (itself `Rc`-based, not `Send`-bound — see its call site)
/// ever reads this.
struct FeedbackConfig {
    feedback: Option<Rc<dyn Fn() -> BoxedView>>,
    feedback_offset: Offset<Pixels>,
}

impl FeedbackConfig {
    fn from_view<T: Clone + Send + Sync + 'static>(view: &Draggable<T>) -> Self {
        Self {
            feedback: view.feedback.clone(),
            feedback_offset: view.feedback_offset,
        }
    }
}

/// Mutable signal shared by one drag session and its mounted feedback anchor.
///
/// The signal is owner-local in practice. Its current `Arc<Mutex<_>>` shape is
/// retained until feedback-entry ownership moves from `DraggableState` into
/// each session.
#[derive(Clone)]
struct FeedbackSignal {
    /// The displacement `feedback` is painted at (`feedback_offset` plus
    /// this). Written by [`DragSession::update`], read by
    /// [`FeedbackAnchorState::build`].
    offset: Arc<Mutex<Offset<Pixels>>>,
    /// The mounted [`FeedbackAnchor`] element's own rebuild capability,
    /// published by [`FeedbackAnchorState::init_state`] (never from `build` —
    /// port-check trigger #22) so [`DragSession::update`] can reposition it
    /// without reaching into any `Rc`-backed type.
    rebuild: Arc<Mutex<Option<RebuildHandle>>>,
}

impl FeedbackSignal {
    fn new() -> Self {
        Self {
            offset: Arc::new(Mutex::new(Offset::ZERO)),
            rebuild: Arc::new(Mutex::new(None)),
        }
    }

    /// Whether two handles name the same signal. Identity, not structural
    /// equality — mirrors [`OverlayHandle::is_same`]/[`OverlayEntry::is_same`].
    #[cfg(test)]
    fn is_same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.offset, &other.offset)
    }

    fn offset(&self) -> Offset<Pixels> {
        *self.offset.lock()
    }

    fn set_offset(&self, offset: Offset<Pixels>) {
        *self.offset.lock() = offset;
    }

    fn publish_rebuild(&self, handle: RebuildHandle) {
        *self.rebuild.lock() = Some(handle);
    }

    /// Reposition the mounted anchor, if one is currently published. A no-op
    /// before the anchor's first build, or after it unmounts — same shape as
    /// [`OverlayEntry::mark_needs_build`]'s own before-mount/after-unmount
    /// inertness.
    fn reposition(&self) {
        // Clone the handle out and drop the lock before calling into the
        // framework: `RebuildHandle::schedule` must never run with this
        // (or any other) lock still held.
        let handle = self.rebuild.lock().clone();
        if let Some(handle) = handle {
            handle.schedule(flui_view::RebuildReason::StateChange);
        }
    }
}

/// The feedback layer's mounted content: `feedback` wrapped in a `Positioned`
/// inside its own `Stack`. `RenderTheater` (the `Overlay`'s render object)
/// does not run `RenderStack`'s positioned split on its direct children — a
/// bare `Positioned` as an entry's root is silently dropped to the origin
/// (pinned by
/// `overlay::tests::positioned_inside_an_overlay_entry_is_laid_out_by_an_inner_stack`,
/// ADR-0021) — so the inner `Stack` is load-bearing, not decorative.
///
/// A real, `Rc`-backed `StatefulView` (not a bare closure) specifically so its
/// `init_state` can acquire a `RebuildHandle` the ADR-0018 way and publish it
/// to [`FeedbackSignal`], allowing the session to reposition this content
/// without retaining a framework element reference.
#[derive(Clone)]
struct FeedbackAnchor {
    feedback: Rc<dyn Fn() -> BoxedView>,
    feedback_offset: Offset<Pixels>,
    signal: FeedbackSignal,
}

impl View for FeedbackAnchor {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for FeedbackAnchor {
    type State = FeedbackAnchorState;

    fn create_state(&self) -> Self::State {
        FeedbackAnchorState {
            signal: self.signal.clone(),
        }
    }
}

struct FeedbackAnchorState {
    signal: FeedbackSignal,
}

impl ViewState<FeedbackAnchor> for FeedbackAnchorState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.signal.publish_rebuild(ctx.rebuild_handle());
    }

    fn build(&self, view: &FeedbackAnchor, _ctx: &dyn BuildContext) -> impl IntoView {
        let displacement = self.signal.offset();
        Stack::new(vec![
            Positioned::new((view.feedback)())
                .left((view.feedback_offset.dx + displacement.dx).0)
                .top((view.feedback_offset.dy + displacement.dy).0)
                .into_view()
                .boxed(),
        ])
        .fit(StackFit::Expand)
    }
}

/// Evicts whatever `feedback_entry_slot` currently holds and, if both an
/// ancestor overlay and a feedback builder are configured, mints a FRESH
/// [`FeedbackSignal`] for a newly-inserted entry — returning it so the caller
/// can attach it to the new [`DragSession`]. Returns `None` (having still
/// evicted any stale entry) when there is nowhere or nothing to show.
///
/// This is the exact code `on_start` calls at drag-start time — split out of
/// its closure body so the fresh-signal-per-entry fix is directly
/// unit-testable. `on_start` itself returns an opaque
/// `Option<Box<dyn MultiDragHandle>>` with no accessor back to the
/// `FeedbackSignal` a `DragSession` captured, so a test invoking `on_start`
/// twice cannot observe signal identity through its return value alone;
/// calling this function directly (twice, with the same `feedback_entry_slot`
/// and a bare, never-mounted `OverlayHandle` — see its own doc on why that
/// needs no element tree at all) is the real pin for the mint-fresh-signal
/// fix `feedback_entry`'s docs describe, not a parallel reimplementation of
/// it.
///
/// Every lock/borrow taken here is cloned out and dropped before the
/// framework calls (`stale.remove()`, `handle.insert()`) that follow — never
/// held across them.
fn evict_and_mount_feedback(
    feedback_entry_slot: &Rc<RefCell<Option<OverlayEntry>>>,
    overlay_handle: Option<OverlayHandle>,
    feedback_builder: Option<Rc<dyn Fn() -> BoxedView>>,
    feedback_offset: Offset<Pixels>,
) -> Option<FeedbackSignal> {
    let stale = feedback_entry_slot.borrow_mut().take();
    if let Some(stale) = stale {
        stale.remove();
    }
    match (overlay_handle, feedback_builder) {
        (Some(handle), Some(builder)) => {
            // A FRESH signal per inserted entry, not one shared across
            // sessions on `DraggableState` — see `feedback_entry`'s docs:
            // reusing one shared instance is exactly what let a still-live
            // evicted session keep writing into the surviving layer.
            // `FeedbackSignal::new` already starts at `Offset::ZERO`.
            let signal = FeedbackSignal::new();
            let entry = feedback_entry(builder, feedback_offset, signal.clone());
            handle.insert(&entry, &InsertPosition::Top);
            *feedback_entry_slot.borrow_mut() = Some(entry);
            Some(signal)
        }
        _ => None,
    }
}

/// Builds the `OverlayEntry` a drag session inserts at start: a
/// [`FeedbackAnchor`] wrapping `feedback`, sharing `signal` with the
/// [`DragSession`] that will reposition it.
fn feedback_entry(
    feedback: Rc<dyn Fn() -> BoxedView>,
    feedback_offset: Offset<Pixels>,
    signal: FeedbackSignal,
) -> OverlayEntry {
    OverlayEntry::new(move |_ctx| {
        FeedbackAnchor {
            feedback: Rc::clone(&feedback),
            feedback_offset,
            signal: signal.clone(),
        }
        .into_view()
        .boxed()
    })
}

/// Restricts `offset` to `axis`'s component (`_DragAvatar._restrictAxis`).
fn restrict_axis(offset: Offset<Pixels>, axis: Option<Axis>) -> Offset<Pixels> {
    match axis {
        Some(Axis::Horizontal) => Offset::new(offset.dx, Pixels(0.0)),
        Some(Axis::Vertical) => Offset::new(Pixels(0.0), offset.dy),
        None => offset,
    }
}

/// [`restrict_axis`], for the per-update delta's `PixelDelta` unit.
fn restrict_axis_delta(delta: Offset<PixelDelta>, axis: Option<Axis>) -> Offset<PixelDelta> {
    match axis {
        Some(Axis::Horizontal) => Offset::new(delta.dx, PixelDelta(0.0)),
        Some(Axis::Vertical) => Offset::new(PixelDelta(0.0), delta.dy),
        None => delta,
    }
}

/// Publishes the `RenderId` of the node whose coordinate space a
/// `Draggable`'s pointer events arrive in.
///
/// Pointer dispatch rewrites each event's position into the receiving entry's
/// LOCAL space (`HitTestResult`'s per-entry `transform_pointer_event`), and
/// `PointerEvent` is a `ui_events` type with nowhere to keep the global one
/// alongside. A drag therefore knows only where the pointer is inside its own
/// `Listener`; a hit test needs where it is in the root's space.
///
/// `PipelineOwner::local_to_global` converts between exactly those two spaces
/// — given the `RenderId` of the node the local point belongs to. This view is
/// how the drag learns it: mounted as the `Listener`'s direct child, its
/// `find_render_object()` walks strict ancestors and stops at the `Listener`'s
/// own render node, which is the node dispatch localized against.
///
/// The conversion composes the whole ancestor chain, so it is exact under
/// scale and rotation, not only translation.
#[derive(Clone)]
struct DragOrigin {
    node: Rc<Cell<Option<flui_foundation::RenderId>>>,
    child: BoxedView,
}

impl View for DragOrigin {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for DragOrigin {
    type State = DragOriginState;

    fn create_state(&self) -> Self::State {
        DragOriginState {
            node: Rc::clone(&self.node),
        }
    }
}

struct DragOriginState {
    node: Rc<Cell<Option<flui_foundation::RenderId>>>,
}

impl ViewState<DragOrigin> for DragOriginState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.node.set(ctx.find_render_object());
    }

    fn build(&self, view: &DragOrigin, _ctx: &dyn BuildContext) -> impl IntoView {
        view.child.clone()
    }
}

/// One [`DragTargetSlot`] this drag is currently over, with the drag's
/// position as that target sees it.
///
/// The position is refreshed on every discovery pass, so a drop reports where
/// the pointer actually was — including in the target's own local space,
/// which only the hit entry's transform can give.
#[derive(Clone)]
struct EnteredTarget {
    slot: Arc<DragTargetSlot>,
    at: DragPosition,
}

/// `global` mapped into the space `transform` describes.
///
/// A hit entry's transform already maps global to local — `HitTestResult`
/// folds each level's own inverse as it descends — so this applies it
/// directly rather than inverting again. An entry with no transform is
/// already in the root's space.
fn localize(global: Offset<Pixels>, transform: Option<&Matrix4>) -> Offset<Pixels> {
    let Some(transform) = transform else {
        return global;
    };
    let (x, y) = transform.transform_point(global.dx, global.dy);
    Offset::new(x, y)
}

/// The drag targets on `path`, leaf-first, that will take `data`.
///
/// Flutter parity: `_DragAvatar._getDragTargets` — walk the hit path for
/// metadata-tagged targets and keep those whose `T` matches the drag's
/// payload (`isExpectedDataType`). Order is the path's own, which is what
/// makes the innermost of a set of nested targets win.
fn drag_targets_on(
    path: &[HitTestEntry],
    data: &ErasedDragData,
    global: Offset<Pixels>,
) -> Vec<EnteredTarget> {
    path.iter()
        .filter_map(|entry| {
            let payload = Arc::clone(entry.metadata.as_ref()?);
            let slot = payload.downcast::<DragTargetSlot>().ok()?; // PORT-CHECK-OK-DOWNCAST: the hit-test payload channel is `dyn Any` by construction (`HitTestEntry::metadata`); this is the `metaData is _DragTargetState` test of the oracle's `_getDragTargets`.
            slot.accepts_data_type(data).then(|| EnteredTarget {
                at: DragPosition {
                    global,
                    local: localize(global, entry.transform.as_ref()),
                },
                slot,
            })
        })
        .collect()
}

/// The `_DragAvatar` analogue: one instance per active drag, held by the
/// recognizer for the pointer's lifetime. It owns the drag's standing with
/// every [`DragTargetSlot`] it has entered, and re-discovers that set on
/// every move.
///
/// **Current divergence from `_disposeRecognizerIfInactive`:** the recognizer
/// and feedback entry still belong to `DraggableState`, rather than being
/// transferred to active sessions. Unmount therefore cancels the session
/// immediately. The handle is owner-local and can carry that ownership in a
/// future parity pass; no `Send + Sync` constraint prevents it.
struct DragSession {
    active_count: Arc<AtomicUsize>,
    rebuild: RebuildHandle,
    config: Arc<Mutex<DragConfig>>,
    /// The contact this session follows. Every transition a target receives
    /// is keyed by it, which is what keeps simultaneous drags independent.
    pointer: PointerId,
    /// The fresh-hit-test capability, acquired by `DraggableState` in
    /// `init_state` / `did_change_dependencies` and shared by cell so a later
    /// re-resolution reaches a session that started before it. `None` when the
    /// embedder installed none, in which case this drag discovers nothing.
    hit_test: Rc<RefCell<Option<HitTestHandle>>>,
    /// `feedback_offset`, read live: the oracle hit-tests at
    /// `globalPosition + feedbackOffset`, so the probe follows the feedback
    /// layer rather than the bare pointer.
    feedback_config: Rc<RefCell<FeedbackConfig>>,
    /// The render node the drag's pointer positions are local to, and the
    /// tree that can convert them to the root's space — see [`DragOrigin`].
    listener_node: Rc<Cell<Option<flui_foundation::RenderId>>>,
    pipeline: Rc<RefCell<Option<flui_rendering::pipeline::PipelineCell>>>,
    /// The drag's current position (`_DragAvatar._position`): the contact's
    /// down position plus every axis-restricted delta since.
    ///
    /// In the `Listener`'s LOCAL space, because that is the space every
    /// pointer event reaches a widget in — [`to_global`](Self::to_global)
    /// converts it for the hit test and for what targets are told.
    ///
    /// Distinct from [`offset`](Self::offset), which is the same sum without
    /// the starting point — see that field, and the module's divergence
    /// note #4 on why `DraggableDetails.offset` keeps that narrower meaning.
    position: Mutex<Offset<Pixels>>,
    /// Every target this drag is currently inside, outermost-last, and the
    /// drag position each one last saw (`_DragAvatar._enteredTargets`).
    entered: RefCell<Vec<EnteredTarget>>,
    /// The first entered target that accepted the drag, if any
    /// (`_DragAvatar._activeTarget`) — the one a drop is delivered to, and the
    /// position it last saw, so the drop reports where the pointer actually
    /// was instead of re-deriving it.
    active: RefCell<Option<EnteredTarget>>,
    /// Running sum of every axis-restricted delta since the drag started —
    /// displacement, seeded at `Offset::ZERO`. **Not** the oracle's
    /// `_lastOffset`: that adds the draggable's global origin on top of this
    /// same sum (see the module's divergence note #4 — a named, pinned
    /// divergence, not attempted here). Reported as `DraggableDetails.offset`.
    offset: Mutex<Offset<Pixels>>,
    /// Signal to this session's feedback layer, if one is showing — `None`
    /// when there is no ancestor `Overlay`
    /// (`Overlay::maybe_of` found nothing) or no `feedback` builder is
    /// configured. The entry is still owned by `DraggableState`; actual
    /// removal happens in `build`/`dispose`, triggered by
    /// [`end_active`](Self::end_active)'s `rebuild.schedule(reason)`.
    feedback: Option<FeedbackSignal>,
}

impl DragSession {
    /// Decrements the active count and schedules a rebuild so the widget can
    /// swap back from `child_when_dragging` to `child` — and, if this was the
    /// last active drag, so `DraggableState::build` tears down the feedback
    /// layer (see [`feedback`](Self::feedback)'s docs).
    fn end_active(&self) {
        self.active_count.fetch_sub(1, Ordering::AcqRel);
        self.rebuild.schedule(flui_view::RebuildReason::StateChange);
    }

    /// `local` — a point in the `Listener`'s space, which is the only space a
    /// pointer event reaches widget code in — as a point in the root's.
    ///
    /// `None` when the conversion cannot be made honestly: before the origin
    /// probe has mounted, without the pipeline capability, while a frame holds
    /// the tree, or when the composed transform is singular (a zero-scale
    /// ancestor). Every one of those is a reason to leave the drag's target
    /// standing untouched, never to guess a position.
    fn to_global(&self, local: Offset<Pixels>) -> Option<Offset<Pixels>> {
        let node = self.listener_node.get()?;
        let pipeline = self.pipeline.borrow().clone()?;
        let global = pipeline.try_with(|owner| {
            owner.local_to_global(node, flui_types::Point::new(local.dx, local.dy), None)
        })??;
        Some(Offset::new(global.x, global.y))
    }

    /// The targets under the drag right now, or `None` when the question
    /// could not be asked.
    ///
    /// `None` and `Some(vec![])` are deliberately different answers.
    /// `Some(vec![])` means the tree replied and the drag is over nothing, so
    /// every entered target must be left. `None` means no reply was
    /// obtainable — no capability installed, no payload to match, no global
    /// position to ask about, or the tree reported itself busy or closed — and
    /// the correct response is to change nothing at all. Collapsing the two
    /// would fire a spurious leave on every target each time a frame happened
    /// to hold the tree.
    fn discover(&self, global: Offset<Pixels>) -> Option<(ErasedDragData, Vec<EnteredTarget>)> {
        let handle = self.hit_test.borrow().clone()?;
        // Read once and carry it onwards: the payload that decided which
        // targets match must be the payload those targets are then handed.
        let data = self.config.lock().data.clone()?;
        let probe_at = global + self.feedback_config.borrow().feedback_offset;
        match handle.hit_test_at(probe_at) {
            Ok(snapshot) => {
                let targets = drag_targets_on(snapshot.path(), &data, global);
                Some((data, targets))
            }
            Err(error) => {
                tracing::debug!(
                    ?error,
                    "drag-target discovery skipped: the render tree could not answer"
                );
                None
            }
        }
    }

    /// Re-discover the targets under `global` and drive the resulting
    /// enter/move/leave transitions.
    ///
    /// Flutter parity: `_DragAvatar.updateDrag`, including its prefix-match
    /// fast path. The oracle bails to move-only when the new target list
    /// starts with exactly the entered list AND either something has already
    /// accepted (deeper targets below the active one are correctly ignored) or
    /// the lists are the same length (nothing has accepted, so `_enteredTargets`
    /// holds every hit target and a longer list means a new one appeared).
    fn update_drag(&self, local: Offset<Pixels>) {
        let Some(global) = self.to_global(local) else {
            return;
        };
        self.update_drag_at(global);
    }

    /// [`update_drag`](Self::update_drag) from a position already in the
    /// root's space.
    ///
    /// Split from the conversion so the sequencing rules can be driven
    /// directly against a controllable probe: the difference between "the tree
    /// says nothing is here" and "the tree could not answer" is not reachable
    /// through a mounted harness (making a real tree report itself busy means
    /// holding it checked out, which the harness's own dispatch path cannot do
    /// while delivering a pointer event).
    fn update_drag_at(&self, global: Offset<Pixels>) {
        let Some((data, targets)) = self.discover(global) else {
            return;
        };

        let prefix_matches = {
            let entered = self.entered.borrow();
            !entered.is_empty()
                && targets.len() >= entered.len()
                && entered
                    .iter()
                    .zip(targets.iter())
                    .all(|(was, now)| Arc::ptr_eq(&was.slot, &now.slot))
        };
        let unchanged_length = targets.len() == self.entered.borrow().len();

        if prefix_matches && (self.active.borrow().is_some() || unchanged_length) {
            // Same targets, new position: refresh what each one last saw, then
            // report the move. Both borrows end before any callback runs.
            let moving: Vec<EnteredTarget> = {
                let mut entered = self.entered.borrow_mut();
                for (was, now) in entered.iter_mut().zip(targets.iter()) {
                    was.at = now.at;
                }
                entered.clone()
            };
            for target in moving {
                target.slot.did_move(self.pointer, target.at);
            }
            return;
        }

        self.leave_all_entered();

        // Enter targets leaf-first, stopping at the first that accepts —
        // everything under it is shadowed by the acceptance.
        let mut newly_entered: Vec<EnteredTarget> = Vec::new();
        let mut new_active = None;
        for target in targets {
            newly_entered.push(target.clone());
            if target.slot.did_enter(self.pointer, &data, target.at) {
                new_active = Some(target);
                break;
            }
        }
        *self.entered.borrow_mut() = newly_entered;
        *self.active.borrow_mut() = new_active;

        // Cloned out and the borrow dropped before any callback runs, the same
        // as the move-only path above.
        let moving: Vec<EnteredTarget> = self.entered.borrow().clone();
        for target in moving {
            target.slot.did_move(self.pointer, target.at);
        }
    }

    /// Leave every target this drag has entered, in entry order
    /// (`_DragAvatar._leaveAllEntered`).
    fn leave_all_entered(&self) {
        let leaving: Vec<EnteredTarget> = self.entered.borrow_mut().drain(..).collect();
        for target in leaving {
            target.slot.did_leave(self.pointer);
        }
    }

    /// Deliver the drop, if this drag ends over an accepting target, then
    /// leave everything. Returns whether a target took the data.
    ///
    /// Flutter parity: `_DragAvatar.finishDrag`. The one divergence is the
    /// return value: the oracle records `wasAccepted = true` whenever an
    /// active target exists, even when that target's own `didDrop` returned
    /// early because it had left the tree. [`DragTargetSlot::did_drop`]
    /// answers whether the data was actually taken, and this reports that.
    fn finish_drag(&self, dropped: bool) -> bool {
        let mut was_accepted = false;
        if dropped && let Some(active) = self.active.borrow_mut().take() {
            was_accepted = active.slot.did_drop(self.pointer, active.at);
            self.entered
                .borrow_mut()
                .retain(|target| !Arc::ptr_eq(&target.slot, &active.slot));
        }
        self.leave_all_entered();
        *self.active.borrow_mut() = None;
        was_accepted
    }
}

impl MultiDragHandle for DragSession {
    fn update(&self, details: MultiDragUpdateDetails) {
        let axis = self.config.lock().axis;
        let restricted = restrict_axis_delta(details.delta, axis);
        let moved = restricted.dx.0 != 0.0 || restricted.dy.0 != 0.0;
        if moved {
            let step = Offset::new(Pixels(restricted.dx.0), Pixels(restricted.dy.0));
            *self.offset.lock() += step;
            *self.position.lock() += step;
            if let Some(feedback) = &self.feedback {
                feedback.set_offset(*self.offset.lock());
                feedback.reposition();
            }
        }

        // Unconditional, like the oracle's `updateDrag(_position)` call: only
        // `onDragUpdate` is gated on the restricted position having moved.
        // Targets still expect a move report for a sample that did not move
        // them, and a rebuild elsewhere can change what is under the pointer
        // without the pointer itself moving at all.
        self.update_drag(*self.position.lock());

        if !moved {
            return;
        }
        // Flutter's `update` passes the RAW (unrestricted) `details` through
        // to `onDragUpdate` unchanged — only the *gate* ("did the restricted
        // position move") is axis-aware, not the reported delta.
        let on_drag_update = self.config.lock().on_drag_update.clone();
        if let Some(callback) = on_drag_update {
            let primary_delta = match axis {
                Some(Axis::Horizontal) => details.delta.dx.0,
                Some(Axis::Vertical) => details.delta.dy.0,
                None => 0.0,
            };
            callback(DragUpdateDetails {
                global_position: details.global_position,
                local_position: details.local_position,
                delta: details.delta,
                primary_delta,
                kind: details.kind,
            });
        }
    }

    fn end(&self, details: MultiDragEndDetails) {
        // The drop lands before the state change, matching the oracle's
        // `finishDrag`: `didDrop` runs, then `onDragEnd` reports the outcome.
        let was_accepted = self.finish_drag(true);
        self.end_active();

        let (velocity, on_drag_end, on_drag_completed, on_draggable_canceled) = {
            let config = self.config.lock();
            (
                Velocity {
                    pixels_per_second: restrict_axis(
                        details.velocity.pixels_per_second,
                        config.axis,
                    ),
                },
                config.on_drag_end.clone(),
                config.on_drag_completed.clone(),
                config.on_draggable_canceled.clone(),
            )
        };
        let offset = *self.offset.lock();
        if let Some(callback) = on_drag_end {
            callback(DraggableDetails {
                was_accepted,
                velocity,
                offset,
            });
        }
        if was_accepted {
            if let Some(callback) = on_drag_completed {
                callback();
            }
        } else if let Some(callback) = on_draggable_canceled {
            callback(velocity, offset);
        }
    }

    fn cancel(&self) {
        // A cancelled drag delivers nothing, but must still leave every
        // target it had entered — otherwise a target keeps a candidate that
        // no live drag will ever remove.
        self.finish_drag(false);
        self.end_active();

        // Flutter's `_DragAvatar.cancel` also routes through `finishDrag`,
        // which fires `onDragEnd` unconditionally (zero velocity, not
        // accepted, but the real `_lastOffset` — not zero) before
        // `onDraggableCanceled` — not a cancel-only path.
        let (on_drag_end, on_draggable_canceled) = {
            let config = self.config.lock();
            (
                config.on_drag_end.clone(),
                config.on_draggable_canceled.clone(),
            )
        };
        let offset = *self.offset.lock();
        if let Some(callback) = on_drag_end {
            callback(DraggableDetails {
                was_accepted: false,
                velocity: Velocity::ZERO,
                offset,
            });
        }
        if let Some(callback) = on_draggable_canceled {
            callback(Velocity::ZERO, offset);
        }
    }
}

impl<T: Clone + Send + Sync + 'static> StatefulView for Draggable<T> {
    type State = DraggableState<T>;

    fn create_state(&self) -> Self::State {
        DraggableState {
            active_count: Arc::new(AtomicUsize::new(0)),
            config: Arc::new(Mutex::new(DragConfig::from_view(self))),
            overlay: Arc::new(Mutex::new(None)),
            hit_test: Rc::new(RefCell::new(None)),
            listener_node: Rc::new(Cell::new(None)),
            pipeline: Rc::new(RefCell::new(None)),
            feedback_entry: Rc::new(RefCell::new(None)),
            feedback_config: Rc::new(RefCell::new(FeedbackConfig::from_view(self))),
            recognizer: None,
            _data: std::marker::PhantomData,
        }
    }
}

impl<T: Clone + Send + Sync + 'static> ViewState<Draggable<T>> for DraggableState<T> {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        let arena = GestureArenaScope::of(ctx);
        let rebuild = ctx.rebuild_handle();

        // The *initial* resolution, not just re-resolution: `depend_on`
        // (which `Overlay::maybe_of` calls) only registers this element as a
        // dependent — it does not, by itself, guarantee `did_change_dependencies`
        // fires on first mount with no prior dependency to notify about. Same
        // two-call shape `FocusScopeState` uses for `enclosing_focus_parent`
        // (`interaction/focus.rs`): resolve here for the first value, and
        // again in `did_change_dependencies` for later changes.
        *self.overlay.lock() = Overlay::maybe_of(ctx);
        *self.hit_test.borrow_mut() = ctx.hit_test_handle();
        *self.pipeline.borrow_mut() = ctx.pipeline_owner();

        let active_count = Arc::clone(&self.active_count);
        let config = Arc::clone(&self.config);
        let overlay = Arc::clone(&self.overlay);
        let feedback_entry_slot = Rc::clone(&self.feedback_entry);
        let feedback_config = Rc::clone(&self.feedback_config);
        let hit_test = Rc::clone(&self.hit_test);
        let listener_node = Rc::clone(&self.listener_node);
        let pipeline = Rc::clone(&self.pipeline);
        let on_start: MultiDragStartCallback = Rc::new(move |pointer, initial_position| {
            {
                let guard = config.lock();
                if let Some(max) = guard.max_simultaneous_drags
                    && active_count.load(Ordering::Acquire) >= max
                {
                    return None;
                }
            }
            active_count.fetch_add(1, Ordering::AcqRel);
            rebuild.schedule(flui_view::RebuildReason::StateChange);
            if let Some(callback) = config.lock().on_drag_started.clone() {
                callback();
            }

            // A feedback layer needs both a builder to paint and somewhere to
            // paint it — absent either, this drag simply has no visible
            // feedback, same as before this wiring landed. One slot per
            // `Draggable`, not one per session — see `feedback_entry`'s docs
            // on the `max_simultaneous_drags > 1` scope cut this implies:
            // a later session always evicts an earlier one's layer here,
            // including a stale one an earlier session's own end/cancel
            // never got to remove yet (`build`'s teardown is deferred to the
            // next rebuild, which may not have drained before this call).
            let overlay_handle = overlay.lock().clone();
            let (feedback_builder, feedback_offset) = {
                let cfg = feedback_config.borrow();
                (cfg.feedback.clone(), cfg.feedback_offset)
            };
            let feedback = evict_and_mount_feedback(
                &feedback_entry_slot,
                overlay_handle,
                feedback_builder,
                feedback_offset,
            );

            Some(Box::new(DragSession {
                active_count: Arc::clone(&active_count),
                rebuild: rebuild.clone(),
                config: Arc::clone(&config),
                pointer,
                hit_test: Rc::clone(&hit_test),
                feedback_config: Rc::clone(&feedback_config),
                listener_node: Rc::clone(&listener_node),
                pipeline: Rc::clone(&pipeline),
                // The contact's own down position, unlike `offset` below —
                // see `DragSession::position`.
                position: Mutex::new(initial_position),
                entered: RefCell::new(Vec::new()),
                active: RefCell::new(None),
                offset: Mutex::new(Offset::ZERO),
                feedback,
            }) as Box<dyn MultiDragHandle>) // PORT-CHECK-OK-DYN: see flui-interaction's MultiDragStartCallback — the per-pointer handle `MultiDragGestureRecognizer::with_on_start` requires.
        });

        self.recognizer = Some(
            MultiDragGestureRecognizer::new(arena, MultiDragAxis::Free).with_on_start(on_start),
        );
    }

    /// Re-resolves everything this widget reads from its `BuildContext`: the
    /// nearest ancestor `Overlay`, the fresh-hit-test capability, and the
    /// render tree.
    ///
    /// A lifecycle hook, not `build` (port-check trigger #22) and not the
    /// `on_start` gesture callback above, neither of which holds a
    /// `BuildContext`. Re-resolved on every dependency change, not just once:
    /// `Overlay::maybe_of` depends (ADR-0036), so a *different* enclosing
    /// overlay later replacing this one is exactly what re-fires this hook.
    fn did_change_dependencies(&mut self, ctx: &dyn BuildContext) {
        *self.overlay.lock() = Overlay::maybe_of(ctx);
        *self.hit_test.borrow_mut() = ctx.hit_test_handle();
        *self.pipeline.borrow_mut() = ctx.pipeline_owner();
    }

    fn build(&self, view: &Draggable<T>, _ctx: &dyn BuildContext) -> impl IntoView {
        *self.config.lock() = DragConfig::from_view(view);
        *self.feedback_config.borrow_mut() = FeedbackConfig::from_view(view);

        let recognizer = self
            .recognizer
            .clone()
            .expect("BUG: init_state must build the recognizer before the first build");
        let max = view.max_simultaneous_drags;
        let active_count = Arc::clone(&self.active_count);

        let down_recognizer = Arc::clone(&recognizer);
        let move_recognizer = Arc::clone(&recognizer);
        let up_recognizer = Arc::clone(&recognizer);
        let cancel_recognizer = recognizer;

        let listener = Listener::new()
            .on_pointer_down(move |event: &PointerEvent| {
                if let Some(max) = max
                    && active_count.load(Ordering::Acquire) >= max
                {
                    return;
                }
                down_recognizer.add_pointer(event.pointer_id(), event.position());
            })
            .on_pointer_move(move |event| move_recognizer.handle_event(event))
            .on_pointer_up(move |event| up_recognizer.handle_event(event))
            .on_pointer_cancel(move |event| cancel_recognizer.handle_event(event));

        let currently_active = self.active_count.load(Ordering::Acquire);
        let showing_child_when_dragging =
            currently_active > 0 && view.child_when_dragging.is_some();

        // The last active drag just ended (`end_active` schedules exactly
        // this rebuild): tear down the state-owned feedback layer.
        if currently_active == 0 {
            // Taken out and the borrow dropped (this statement ends before
            // the `if let` runs) before the framework call below.
            let stale = self.feedback_entry.borrow_mut().take();
            if let Some(entry) = stale {
                entry.remove();
            }
        }

        // The origin probe goes UNDER the `Listener` and OVER the content:
        // its `find_render_object()` must land on the `Listener`'s own render
        // node, which is the node pointer dispatch localizes against. It
        // contributes no render node of its own, so the mounted render tree is
        // the same shape as without it.
        let origin = Rc::clone(&self.listener_node);
        if showing_child_when_dragging {
            let builder = view
                .child_when_dragging
                .clone()
                .expect("BUG: checked is_some above");
            listener.child(DragOrigin {
                node: origin,
                child: builder(),
            })
        } else {
            match view.child.clone().into_inner() {
                Some(child) => listener.child(DragOrigin {
                    node: origin,
                    child,
                }),
                None => listener,
            }
        }
    }

    /// Disposes the state-owned recognizer unconditionally, so an in-flight
    /// drag is canceled here instead of surviving unmount like Flutter's
    /// `_disposeRecognizerIfInactive` path.
    ///
    /// Also removes the feedback layer directly, if one is still showing:
    /// `recognizer.dispose()`'s `cancel()` calls schedule a rebuild
    /// (`DragSession::end_active`), but this element is unmounting — no
    /// later `build` will ever run to act on it (see `build`'s own teardown
    /// check), so this is the last chance.
    fn dispose(&mut self) {
        if let Some(recognizer) = self.recognizer.as_ref() {
            recognizer.dispose();
        }
        let stale = self.feedback_entry.borrow_mut().take();
        if let Some(entry) = stale {
            entry.remove();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use flui_interaction::PointerId;
    use flui_interaction::events::PointerType;

    use super::*;

    // ------------------------------------------------------------------
    // Fresh-`FeedbackSignal`-per-entry — the fix for the documented third
    // scope-cut `feedback_entry`'s docs used to name: a later session
    // evicting a STILL-LIVE earlier one used to leave both sessions writing
    // to the one shared `FeedbackSignal`, jittering the mounted layer.
    //
    // `evict_and_mount_feedback_mints_a_fresh_signal_every_call_not_a_shared_one`
    // below is the real pin: it calls `evict_and_mount_feedback` — the exact
    // code `on_start` calls at drag-start time — directly, twice, and checks
    // the two returned `FeedbackSignal`s are distinct instances. This is only
    // possible because that mounting logic was split out of `on_start`'s
    // closure body into a standalone function: `on_start` itself returns an
    // opaque `Option<Box<dyn MultiDragHandle>>` with no accessor back to the
    // `FeedbackSignal` a `DragSession` captured, so invoking `on_start` twice
    // cannot observe signal identity through its return value alone, and no
    // The headless presentation harness cannot drive two truly concurrent pointer
    // contacts through it end-to-end either (`tests/parity/draggable_test.rs`'s
    // own module doc; each `dispatch_pointer_down` reassigns the harness's
    // single "current contact").
    //
    // `evicted_sessions_write_only_reaches_its_own_detached_signal` further
    // down is a DIFFERENT, narrower characterization: not of the minting
    // fix itself, but of `DragSession::update`'s pre-existing "write only to
    // whichever `FeedbackSignal` I was constructed with" semantics — a real
    // property, just not the one that was broken.
    // ------------------------------------------------------------------

    /// A trivial `StatefulView` whose only job is to capture the
    /// `RebuildHandle` `init_state` acquires. `RebuildHandle` has no public
    /// standalone constructor (only a real mount ever mints one), so this
    /// mounts the smallest possible real element tree to get one. Mirrors
    /// `dismissible.rs`'s own `RebuildHandleCapture` fixture.
    #[derive(Clone, StatefulView)]
    struct RebuildHandleCapture {
        captured: Rc<RefCell<Option<RebuildHandle>>>,
    }

    struct RebuildHandleCaptureState {
        captured: Rc<RefCell<Option<RebuildHandle>>>,
    }

    impl StatefulView for RebuildHandleCapture {
        type State = RebuildHandleCaptureState;

        fn create_state(&self) -> Self::State {
            RebuildHandleCaptureState {
                captured: Rc::clone(&self.captured),
            }
        }
    }

    impl ViewState<RebuildHandleCapture> for RebuildHandleCaptureState {
        fn init_state(&mut self, ctx: &dyn BuildContext) {
            *self.captured.borrow_mut() = Some(ctx.rebuild_handle());
        }

        fn build(&self, _view: &RebuildHandleCapture, _ctx: &dyn BuildContext) -> impl IntoView {
            crate::SizedBox::shrink()
        }
    }

    fn mount_and_capture_rebuild_handle() -> RebuildHandle {
        use flui_view::{BuildOwner, ElementTree};

        let captured = Rc::new(RefCell::new(None));
        let view = RebuildHandleCapture {
            captured: Rc::clone(&captured),
        };
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let root = tree.mount_root(&view, &mut owner.element_owner_mut());
        owner.schedule_build_for(root, 0, flui_view::RebuildReason::InitialMount);
        owner.build_scope(&mut tree);

        captured
            .borrow()
            .clone()
            .expect("init_state must have captured a handle")
    }

    fn empty_config() -> Arc<Mutex<DragConfig>> {
        config_carrying(None)
    }

    fn config_carrying(data: Option<ErasedDragData>) -> Arc<Mutex<DragConfig>> {
        Arc::new(Mutex::new(DragConfig {
            axis: None,
            max_simultaneous_drags: None,
            data,
            on_drag_started: None,
            on_drag_update: None,
            on_draggable_canceled: None,
            on_drag_end: None,
            on_drag_completed: None,
        }))
    }

    fn pointer(n: u64) -> PointerId {
        PointerId::new(n).expect("contact ids start at 1")
    }

    fn update_details(dx: f32, dy: f32) -> MultiDragUpdateDetails {
        MultiDragUpdateDetails {
            pointer_id: pointer(1),
            global_position: Offset::new(Pixels(dx), Pixels(dy)),
            local_position: Offset::new(Pixels(dx), Pixels(dy)),
            delta: Offset::new(PixelDelta(dx), PixelDelta(dy)),
            kind: PointerType::Mouse,
            timestamp: Instant::now(),
        }
    }

    /// A session wired to nothing but `config` and `rebuild` — for the cases
    /// that exercise one behavior of a live drag without a mounted tree
    /// behind it. `hit_test`/`pipeline`/`listener_node` stay empty unless the
    /// caller fills them.
    fn test_session(
        pointer: PointerId,
        config: Arc<Mutex<DragConfig>>,
        rebuild: RebuildHandle,
    ) -> DragSession {
        DragSession {
            active_count: Arc::new(AtomicUsize::new(1)),
            rebuild,
            config,
            pointer,
            hit_test: Rc::new(RefCell::new(None)),
            feedback_config: Rc::new(RefCell::new(FeedbackConfig {
                feedback: None,
                feedback_offset: Offset::ZERO,
            })),
            listener_node: Rc::new(Cell::new(None)),
            pipeline: Rc::new(RefCell::new(None)),
            position: Mutex::new(Offset::ZERO),
            entered: RefCell::new(Vec::new()),
            active: RefCell::new(None),
            offset: Mutex::new(Offset::ZERO),
            feedback: None,
        }
    }

    /// Pins the real fix (see the module-doc comment above this block):
    /// `evict_and_mount_feedback` — the exact function `on_start` calls at
    /// drag-start time — must mint a FRESH `FeedbackSignal` on every call,
    /// never hand back (a clone of) a signal an earlier call already
    /// returned.
    ///
    /// `OverlayHandle::new()` is never mounted anywhere in this test — its
    /// own doc says mutating an unmounted handle is legal (the first real
    /// build reads whatever the list holds; `schedule_rebuild` on a `None`
    /// hook is a no-op) — so this needs no element tree, `BuildContext`, or
    /// harness at all to exercise the actual production code path.
    ///
    /// Red-check: reintroduce a shared signal inside `evict_and_mount_feedback`
    /// (e.g. read it from a `thread_local`/captured cell instead of calling
    /// `FeedbackSignal::new()`) — the identity assertion below fails. See this
    /// function's own doc comment for the exact red-check run against this fix.
    #[test]
    fn evict_and_mount_feedback_mints_a_fresh_signal_every_call_not_a_shared_one() {
        let slot: Rc<RefCell<Option<OverlayEntry>>> = Rc::new(RefCell::new(None));
        let overlay_handle = OverlayHandle::new();
        let builder: Rc<dyn Fn() -> BoxedView> =
            Rc::new(|| crate::SizedBox::new(1.0, 1.0).into_view().boxed());

        let signal_1 = evict_and_mount_feedback(
            &slot,
            Some(overlay_handle.clone()),
            Some(Rc::clone(&builder)),
            Offset::ZERO,
        )
        .expect("an overlay handle and a builder are both configured — must mint a signal");

        // A second call — exactly what a second overlapping `on_start` call
        // does while the first session is still live (nothing here has
        // ended the first session; this call alone evicts its entry).
        let signal_2 = evict_and_mount_feedback(
            &slot,
            Some(overlay_handle.clone()),
            Some(Rc::clone(&builder)),
            Offset::ZERO,
        )
        .expect("an overlay handle and a builder are both configured — must mint a signal");

        assert!(
            !signal_1.is_same(&signal_2),
            "each call must mint its own FeedbackSignal — a still-live \
             evicted session must not keep writing into the surviving one"
        );
    }

    /// `evict_and_mount_feedback` returns `None` — minting nothing — when
    /// there is nowhere to show feedback (no overlay) or nothing configured
    /// to show (no builder), the same as before this port's feedback wiring
    /// landed at all.
    #[test]
    fn evict_and_mount_feedback_mints_nothing_without_both_an_overlay_and_a_builder() {
        let slot: Rc<RefCell<Option<OverlayEntry>>> = Rc::new(RefCell::new(None));
        let builder: Rc<dyn Fn() -> BoxedView> =
            Rc::new(|| crate::SizedBox::new(1.0, 1.0).into_view().boxed());

        assert!(
            evict_and_mount_feedback(&slot, None, Some(builder), Offset::ZERO).is_none(),
            "no overlay ancestor: nowhere to paint feedback"
        );
        assert!(
            evict_and_mount_feedback(&slot, Some(OverlayHandle::new()), None, Offset::ZERO)
                .is_none(),
            "no feedback builder configured: nothing to paint"
        );
    }

    /// A narrower, separate characterization from the two tests above: not
    /// of the minting fix itself, but of `DragSession::update`'s pre-existing
    /// "write only to whichever `FeedbackSignal` I was constructed with"
    /// semantics — two sessions built with two independent signals never
    /// cross-write, regardless of how those signals were minted.
    #[test]
    fn drag_session_update_writes_only_to_its_own_constructed_signal() {
        let rebuild = mount_and_capture_rebuild_handle();

        // Session 1 "wins" the feedback slot first...
        let signal_1 = FeedbackSignal::new();
        let mut session_1 = test_session(pointer(1), empty_config(), rebuild.clone());
        session_1.feedback = Some(signal_1.clone());

        // ...then a second session starts and evicts it — minting its OWN
        // signal per the fix, not `signal_1.clone()`.
        let signal_2 = FeedbackSignal::new();
        let mut session_2 = test_session(pointer(2), empty_config(), rebuild);
        session_2.feedback = Some(signal_2.clone());

        // Session 1 is stale/evicted but still live (its own pointer hasn't
        // lifted yet) — its `update()` calls keep landing somewhere.
        session_1.update(update_details(10.0, 0.0));
        // Session 2 is the surviving session whose signal the mounted layer
        // actually reads.
        session_2.update(update_details(0.0, 25.0));

        assert_eq!(
            signal_2.offset(),
            Offset::new(Pixels(0.0), Pixels(25.0)),
            "the surviving signal must reflect only the surviving session's own moves"
        );
        assert_eq!(
            signal_1.offset(),
            Offset::new(Pixels(10.0), Pixels(0.0)),
            "the evicted session's own signal still tracks its own displacement locally"
        );
        assert_ne!(
            signal_1.offset(),
            signal_2.offset(),
            "the evicted session's writes must never bleed into the surviving signal"
        );
    }

    // ------------------------------------------------------------------
    // Live drag-target discovery, driven against a controllable probe.
    //
    // These reach the parts a mounted harness cannot: a render tree that
    // reports itself BUSY (which needs the tree checked out while a pointer
    // event is delivered — something the dispatch path itself cannot do), and
    // two genuinely concurrent contacts (the widget harness tracks one).
    // ------------------------------------------------------------------

    /// What the next probe call answers.
    #[derive(Clone)]
    enum ProbeAnswer {
        /// The tree replied with this path.
        Path(Vec<HitTestEntry>),
        /// A frame phase holds the tree.
        Busy,
    }

    /// A [`HitTestProbe`] whose answer the test sets.
    #[derive(Clone)]
    struct ScriptedProbe {
        answer: Rc<RefCell<ProbeAnswer>>,
    }

    impl ScriptedProbe {
        fn new() -> Self {
            Self {
                answer: Rc::new(RefCell::new(ProbeAnswer::Path(Vec::new()))),
            }
        }

        fn answer_with(&self, path: Vec<HitTestEntry>) {
            *self.answer.borrow_mut() = ProbeAnswer::Path(path);
        }

        fn report_busy(&self) {
            *self.answer.borrow_mut() = ProbeAnswer::Busy;
        }
    }

    impl flui_interaction::HitTestProbe for ScriptedProbe {
        fn probe(
            &self,
            _position: Offset<Pixels>,
            result: &mut flui_interaction::HitTestResult,
        ) -> Result<(), flui_interaction::InteractionDispatchError> {
            match self.answer.borrow().clone() {
                ProbeAnswer::Path(path) => {
                    *result.path_mut() = path;
                    Ok(())
                }
                ProbeAnswer::Busy => Err(flui_interaction::InteractionDispatchError::TreeBusy),
            }
        }
    }

    /// A hit entry carrying `slot`, as a mounted `DragTarget`'s `MetaData`
    /// node would produce.
    fn entry_for(slot: &Arc<DragTargetSlot>) -> HitTestEntry {
        HitTestEntry::new(flui_foundation::RenderId::new(1))
            .metadata(Arc::clone(slot) as Arc<dyn std::any::Any + Send + Sync>)
    }

    /// A target that counts its leaves. The returned state is what a mounted
    /// `DragTarget` would hold; `slot()` on it is what a drag discovers, and
    /// `candidate_data()` is how the test reads the target's standing back
    /// through the same public surface a builder does.
    fn counting_target(leaves: &Arc<AtomicUsize>) -> crate::DragTargetState<String> {
        let leaves = Arc::clone(leaves);
        let target = crate::DragTarget::<String>::new(|_candidates, _rejected| {
            crate::SizedBox::shrink().into_view().boxed()
        })
        .on_leave(move |_data| {
            leaves.fetch_add(1, Ordering::SeqCst);
        });
        flui_view::StatefulView::create_state(&target)
    }

    /// A tree that cannot answer is NOT a tree that answered "nothing here".
    ///
    /// `TreeBusy` must leave every entered target exactly where it was. Mapping
    /// it to an empty path instead would read as "the drag is over nothing" and
    /// fire a leave on every target, every time a frame happened to hold the
    /// tree mid-drag. The final arm is the discriminator: an actually-empty
    /// path DOES leave, so this test fails if the two are collapsed.
    #[test]
    fn a_busy_tree_changes_nothing_while_an_empty_path_leaves_everything() {
        let lane = flui_interaction::InteractionLane::try_new().expect("a fresh lane");
        lane.enter(|| {
            let leaves = Arc::new(AtomicUsize::new(0));
            let target = counting_target(&leaves);
            let slot = target.slot();
            let probe = ScriptedProbe::new();
            let session = test_session(
                pointer(1),
                config_carrying(Some(Arc::new("parcel".to_string()))),
                mount_and_capture_rebuild_handle(),
            );
            *session.hit_test.borrow_mut() = Some(flui_interaction::HitTestHandle::new(
                lane.dispatch_handle(),
                Rc::new(probe.clone()),
            ));

            probe.answer_with(vec![entry_for(&slot)]);
            session.update_drag_at(Offset::ZERO);
            assert_eq!(
                session.entered.borrow().len(),
                1,
                "premise: the drag is inside the target before the tree goes busy"
            );

            probe.report_busy();
            session.update_drag_at(Offset::ZERO);
            assert_eq!(
                leaves.load(Ordering::SeqCst),
                0,
                "a busy tree must leave the drag's standing untouched — it is \
                 an unanswered question, not an answer of 'over nothing'"
            );
            assert_eq!(
                session.entered.borrow().len(),
                1,
                "...and the entered list must be unchanged, not emptied"
            );

            probe.answer_with(Vec::new());
            session.update_drag_at(Offset::ZERO);
            assert_eq!(
                leaves.load(Ordering::SeqCst),
                1,
                "an EMPTY path is a real answer and must leave the target — \
                 without this arm the assertion above would also pass against \
                 an implementation that never leaves anything"
            );
        });
    }

    /// Two contacts dragging over the same target are tracked independently:
    /// one ending does not disturb the other's standing.
    ///
    /// The widget harness drives a single contact at a time, so this is the
    /// only level at which genuinely concurrent drags are reachable.
    #[test]
    fn two_simultaneous_drags_over_one_target_do_not_disturb_each_other() {
        let lane = flui_interaction::InteractionLane::try_new().expect("a fresh lane");
        lane.enter(|| {
            let leaves = Arc::new(AtomicUsize::new(0));
            let target = counting_target(&leaves);
            let slot = target.slot();
            let rebuild = mount_and_capture_rebuild_handle();

            let mut sessions = Vec::new();
            for contact in [pointer(1), pointer(2)] {
                let probe = ScriptedProbe::new();
                probe.answer_with(vec![entry_for(&slot)]);
                let session = test_session(
                    contact,
                    config_carrying(Some(Arc::new(format!("parcel {contact:?}")))),
                    rebuild.clone(),
                );
                *session.hit_test.borrow_mut() = Some(flui_interaction::HitTestHandle::new(
                    lane.dispatch_handle(),
                    Rc::new(probe),
                ));
                session.update_drag_at(Offset::ZERO);
                sessions.push(session);
            }

            assert_eq!(
                target.candidate_data().len(),
                2,
                "premise: both contacts are over the target at once"
            );

            // The first contact is cancelled; the second is untouched.
            sessions[0].finish_drag(false);
            assert_eq!(
                leaves.load(Ordering::SeqCst),
                1,
                "exactly the cancelled contact leaves"
            );
            assert_eq!(
                target.candidate_data().len(),
                1,
                "the surviving contact keeps its standing with the target"
            );

            assert!(
                sessions[1].finish_drag(true),
                "the surviving contact's own drop still lands on the target"
            );
            assert!(
                target.candidate_data().is_empty(),
                "and once both contacts are done the target holds nothing"
            );
        });
    }
}
