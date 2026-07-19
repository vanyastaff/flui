//! [`Draggable`] — a widget that can be picked up and dragged, carrying typed
//! `data` for a [`DragTarget`](crate::DragTarget) to receive on drop.
//!
//! Flutter parity: `widgets/drag_target.dart` (tag `3.44.0`) — `Draggable`,
//! `_DraggableState`, `DraggableDetails`. `LongPressDraggable` and
//! `DragAnchorStrategy` are named deferrals; see the module docs below.
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
//! 2. **No live drag-target discovery.** The oracle's `_DragAvatar.updateDrag`
//!    performs an ad hoc `WidgetsBinding.instance.hitTestInView` at the
//!    pointer's *current* global position on every move, independent of
//!    wherever the drag's own pointer originally went down, and walks the
//!    result for `RenderMetaData`-tagged `DragTarget`s. FLUI's pointer
//!    dispatch — both the production path
//!    (`GestureBinding::handle_pointer_event`,
//!    `crates/flui-interaction/src/binding.rs`) and the widget test harness's
//!    arena-scoped helper — resolves the hit-test path **once, at
//!    `PointerDown`**, and replays that *cached* route for every subsequent
//!    `Move`/`Up`. There is no capability reachable from widget or
//!    gesture-callback code to run a fresh, arbitrary-position hit test later
//!    (`RenderObjectContext` exposes only owner-lane registration;
//!    `PipelineOwner::hit_test` lives one layer up and is reachable only from
//!    binding-internal code). Adding that reachability is a legitimate,
//!    separate-scope change — the same shape of gap as point 1 above, also
//!    tracked in `docs/ROADMAP.md`'s Cross.H section — and one this port
//!    does not invent silently mid-task.
//!
//!    Consequently: **`Draggable`'s own gesture lifecycle is fully real** —
//!    start/update/end/cancel, `child`/`child_when_dragging` swap,
//!    `max_simultaneous_drags` gating, and every lifecycle callback fire
//!    through genuine [`MultiDragGestureRecognizer`] dispatch. But because no
//!    target is ever discovered, a drag can never be *accepted*: every drag
//!    ends in [`Draggable::on_draggable_canceled`], never
//!    [`Draggable::on_drag_completed`], and [`DraggableDetails::was_accepted`]
//!    is always `false`. [`DragTarget`](crate::DragTarget)'s accept/candidate/
//!    reject/leave protocol is implemented and tested directly against its
//!    state machine (the load-bearing, testable core), not wired end-to-end
//!    to a live `Draggable` session.
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
//!    origin. Getting the real `globalOrigin` needs the same
//!    global-position/frame-aware event delivery capability point 2 above
//!    names as missing (`PipelineOwner::hit_test`/`local_to_global`,
//!    reachable only from binding-internal code) — this is the *same*
//!    Cross.H-tracked family of gaps, not a new one, and is **not** attempted
//!    here. What this port ships is honestly **displacement since the drag
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
//! 5. **Unmounting mid-drag cancels immediately rather than tracking to the
//!    real pointer-up.** The oracle's `_disposeRecognizerIfInactive` keeps
//!    the recognizer alive until every active drag naturally finishes, even
//!    past `_DraggableState` unmounting. This port's recognizer
//!    (`MultiDragGestureRecognizer`) is owner-local (`!Send + !Sync`, per
//!    ADR-0027), while a `DragSession` — the thing that would need to hold a
//!    reference to it to dispose it later — must be `Send + Sync` to satisfy
//!    `MultiDragHandle`'s bound; it structurally cannot hold that reference.
//!    `DraggableState::dispose` therefore disposes the recognizer
//!    unconditionally on unmount, which cancels any still-active drag right
//!    there (correctly firing `on_drag_end`/`on_draggable_canceled` — see
//!    [`DragSession`]'s own docs) instead of continuing to track the pointer
//!    after the widget is gone.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_interaction::arena::GestureArena;
use flui_interaction::{
    DragUpdateDetails, GestureRecognizer, MultiDragAxis, MultiDragEndDetails,
    MultiDragGestureRecognizer, MultiDragHandle, MultiDragStartCallback, MultiDragUpdateDetails,
    PointerEvent, PointerEventExt as _, Velocity,
};
use flui_types::{
    Offset,
    geometry::{PixelDelta, Pixels},
    layout::Axis,
};
use flui_view::RebuildHandle;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use parking_lot::Mutex;

use crate::overlay::{InsertPosition, Overlay, OverlayEntry, OverlayHandle};
use crate::{GestureArenaScope, Listener, Positioned, Stack, StackFit};

/// A no-argument, thread-safe callback — [`Draggable::on_drag_started`] /
/// [`Draggable::on_drag_completed`]. `Arc<dyn Fn + Send + Sync>` (not the
/// `Rc`-based shape most `flui-widgets` callbacks use) because it is invoked
/// from inside a [`MultiDragHandle`] impl, and that trait requires
/// `Send + Sync + 'static` on its implementor (`flui-interaction`'s
/// multi-pointer recognizer stores handles behind `Arc<Mutex<_>>`, matching
/// its per-pointer arena-competition state).
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
/// Flutter parity: `DraggableDetails`. See the module-level divergence notes:
/// `was_accepted` is always `false` in this cut (no live target discovery).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DraggableDetails {
    /// Whether a `DragTarget` accepted this drop.
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
/// docs for what is and is not wired up in this cut.
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

    /// Called when a drag begins (a contact crosses the drag slop).
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

    /// Called when the drag ends without a target accepting it. Always
    /// invoked in this cut — see the module divergence notes.
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

    /// Called when a target accepts the drop. Never fires in this cut — see
    /// the module divergence notes.
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
    /// The currently-mounted feedback layer, if any is showing. Owner-local
    /// (`Rc<RefCell<_>>`, not `Arc<Mutex<_>>`): only `on_start`, `build` and
    /// `dispose` — all owner-thread code — ever touch it. `DragSession`
    /// cannot hold this directly (`OverlayEntry` is `!Send`); see
    /// [`FeedbackSignal`]'s docs for the `Send + Sync` bridge it uses
    /// instead.
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
    /// Built once in `init_state` against the ambient (or private) arena.
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
/// `Draggable::data` and `on_drag_completed` are deliberately **not** carried
/// here: with no live target discovery (see the module docs), a drop is
/// never accepted, so neither is ever consulted by a session — they stay on
/// the public [`Draggable`] view only, ready for a future live-wiring pass
/// rather than threaded into internal state that would silently do nothing
/// with them. `feedback`/`feedback_offset` are **not** carried here either,
/// even though a session does read them at start: `feedback` is
/// `Rc<dyn Fn() -> BoxedView>`, which is `!Send` (owner-local, ADR-0027) and
/// would make this whole `Send + Sync`-bound struct `!Send` by infection —
/// see [`FeedbackConfig`], the separate owner-local cell `on_start` (itself
/// `Rc`-based, not `Send`-bound) reads directly instead.
struct DragConfig {
    axis: Option<Axis>,
    max_simultaneous_drags: Option<usize>,
    on_drag_started: Option<StartedCallback>,
    on_drag_update: Option<DragUpdateCallback>,
    on_draggable_canceled: Option<DraggableCanceledCallback>,
    on_drag_end: Option<DragEndCallback>,
}

impl DragConfig {
    fn from_view<T: Clone + Send + Sync + 'static>(view: &Draggable<T>) -> Self {
        Self {
            axis: view.axis,
            max_simultaneous_drags: view.max_simultaneous_drags,
            on_drag_started: view.on_drag_started.clone(),
            on_drag_update: view.on_drag_update.clone(),
            on_draggable_canceled: view.on_draggable_canceled.clone(),
            on_drag_end: view.on_drag_end.clone(),
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

/// The `Send + Sync` bridge between a [`DragSession`] (which must itself be
/// `Send + Sync` — [`MultiDragHandle`]'s bound) and its feedback layer's real,
/// mounted content (a [`FeedbackAnchor`], reached through an [`OverlayEntry`],
/// both `Rc`-backed/owner-local per ADR-0027 and therefore `!Send`).
/// `DragSession` never touches the `OverlayEntry` — only this handle, which
/// holds nothing but `Send + Sync`-safe primitives.
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
            handle.schedule();
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
/// to [`FeedbackSignal`] — the one door a `Send + Sync`-bound `DragSession`
/// has into repositioning this owner-local content.
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

/// The `_DragAvatar` analogue: one instance per active drag, held by the
/// recognizer for the pointer's lifetime.
///
/// **Divergence from the oracle's `_disposeRecognizerIfInactive`:** the
/// oracle keeps its recognizer alive until every active drag naturally
/// finishes, even if `_DraggableState` itself unmounts first. This port
/// cannot: `MultiDragGestureRecognizer` is owner-local (`Rc`/`RefCell`
/// internally, per ADR-0027) and therefore `!Send + !Sync`, while
/// `MultiDragHandle` (this session's trait) requires `Send + Sync + 'static`
/// — a `DragSession` structurally cannot hold a live reference to the
/// recognizer it came from to dispose it later itself. `DraggableState::dispose`
/// therefore disposes the recognizer unconditionally, immediately, on
/// unmount — which cancels any still-active drag right there (`dispose`
/// calls `.cancel()` on every accepted handle) rather than letting it run to
/// the user's real pointer-up. `on_drag_end`/`on_draggable_canceled` still
/// fire correctly for that cancellation (`DragSession::cancel`); what does
/// NOT happen is tracking the pointer any further after unmount.
struct DragSession {
    active_count: Arc<AtomicUsize>,
    rebuild: RebuildHandle,
    config: Arc<Mutex<DragConfig>>,
    /// Running sum of every axis-restricted delta since the drag started —
    /// displacement, seeded at `Offset::ZERO`. **Not** the oracle's
    /// `_lastOffset`: that adds the draggable's global origin on top of this
    /// same sum (see the module's divergence note #4 — a named, pinned
    /// divergence, not attempted here). Reported as `DraggableDetails.offset`.
    offset: Mutex<Offset<Pixels>>,
    /// The `Send + Sync` bridge to this session's feedback layer, if one is
    /// showing — `None` when there is no ancestor `Overlay`
    /// (`Overlay::maybe_of` found nothing) or no `feedback` builder is
    /// configured. `DragSession` cannot hold the `OverlayEntry` itself (it is
    /// `!Send`, ADR-0027); see [`FeedbackSignal`]'s docs. Actual removal of
    /// the entry happens in `DraggableState::build`/`dispose`, triggered by
    /// [`end_active`](Self::end_active)'s `rebuild.schedule()` — this session
    /// only needs to stop repositioning it, which needs no `Rc` access.
    feedback: Option<FeedbackSignal>,
}

impl DragSession {
    /// Decrements the active count and schedules a rebuild so the widget can
    /// swap back from `child_when_dragging` to `child` — and, if this was the
    /// last active drag, so `DraggableState::build` tears down the feedback
    /// layer (see [`feedback`](Self::feedback)'s docs).
    fn end_active(&self) {
        self.active_count.fetch_sub(1, Ordering::AcqRel);
        self.rebuild.schedule();
    }
}

impl MultiDragHandle for DragSession {
    fn update(&self, details: MultiDragUpdateDetails) {
        let config = self.config.lock();
        let restricted = restrict_axis_delta(details.delta, config.axis);
        let moved = restricted.dx.0 != 0.0 || restricted.dy.0 != 0.0;
        if !moved {
            return;
        }
        *self.offset.lock() += Offset::new(Pixels(restricted.dx.0), Pixels(restricted.dy.0));
        if let Some(feedback) = &self.feedback {
            feedback.set_offset(*self.offset.lock());
            feedback.reposition();
        }

        // Flutter's `update` passes the RAW (unrestricted) `details` through
        // to `onDragUpdate` unchanged — only the *gate* ("did the restricted
        // position move") is axis-aware, not the reported delta.
        if let Some(callback) = &config.on_drag_update {
            let primary_delta = match config.axis {
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
        self.end_active();

        let config = self.config.lock();
        // No live target discovery (see module docs): every drag ends
        // uncaptured.
        let velocity = Velocity {
            pixels_per_second: restrict_axis(details.velocity.pixels_per_second, config.axis),
        };
        let offset = *self.offset.lock();
        if let Some(callback) = &config.on_drag_end {
            callback(DraggableDetails {
                was_accepted: false,
                velocity,
                offset,
            });
        }
        if let Some(callback) = &config.on_draggable_canceled {
            callback(velocity, offset);
        }
    }

    fn cancel(&self) {
        self.end_active();

        // Flutter's `_DragAvatar.cancel` also routes through `finishDrag`,
        // which fires `onDragEnd` unconditionally (zero velocity, not
        // accepted, but the real `_lastOffset` — not zero) before
        // `onDraggableCanceled` — not a cancel-only path.
        let config = self.config.lock();
        let offset = *self.offset.lock();
        if let Some(callback) = &config.on_drag_end {
            callback(DraggableDetails {
                was_accepted: false,
                velocity: Velocity::ZERO,
                offset,
            });
        }
        if let Some(callback) = &config.on_draggable_canceled {
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
            feedback_entry: Rc::new(RefCell::new(None)),
            feedback_config: Rc::new(RefCell::new(FeedbackConfig::from_view(self))),
            recognizer: None,
            _data: std::marker::PhantomData,
        }
    }
}

impl<T: Clone + Send + Sync + 'static> ViewState<Draggable<T>> for DraggableState<T> {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        let arena = ctx
            .get::<GestureArenaScope, _>(|scope| scope.arena().clone())
            .unwrap_or_else(GestureArena::new);
        let rebuild = ctx.rebuild_handle();

        // The *initial* resolution, not just re-resolution: `depend_on`
        // (which `Overlay::maybe_of` calls) only registers this element as a
        // dependent — it does not, by itself, guarantee `did_change_dependencies`
        // fires on first mount with no prior dependency to notify about. Same
        // two-call shape `FocusScopeState` uses for `enclosing_focus_parent`
        // (`interaction/focus.rs`): resolve here for the first value, and
        // again in `did_change_dependencies` for later changes.
        *self.overlay.lock() = Overlay::maybe_of(ctx);

        let active_count = Arc::clone(&self.active_count);
        let config = Arc::clone(&self.config);
        let overlay = Arc::clone(&self.overlay);
        let feedback_entry_slot = Rc::clone(&self.feedback_entry);
        let feedback_config = Rc::clone(&self.feedback_config);
        let on_start: MultiDragStartCallback = Rc::new(move |_pointer, _position| {
            {
                let guard = config.lock();
                if let Some(max) = guard.max_simultaneous_drags
                    && active_count.load(Ordering::Acquire) >= max
                {
                    return None;
                }
            }
            active_count.fetch_add(1, Ordering::AcqRel);
            rebuild.schedule();
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
                offset: Mutex::new(Offset::ZERO),
                feedback,
            }) as Box<dyn MultiDragHandle>) // PORT-CHECK-OK-DYN: see flui-interaction's MultiDragStartCallback — the per-pointer handle `MultiDragGestureRecognizer::with_on_start` requires.
        });

        self.recognizer = Some(
            MultiDragGestureRecognizer::new(arena, MultiDragAxis::Free).with_on_start(on_start),
        );
    }

    /// Resolves the nearest ancestor `Overlay`, if any — a lifecycle hook,
    /// not `build` (port-check trigger #22) and not the `on_start` gesture
    /// callback above, neither of which holds a `BuildContext`. Re-resolved
    /// on every dependency change, not just once: `Overlay::maybe_of` depends
    /// (ADR-0036), so a *different* enclosing overlay later replacing this
    /// one is exactly what re-fires this hook.
    fn did_change_dependencies(&mut self, ctx: &dyn BuildContext) {
        *self.overlay.lock() = Overlay::maybe_of(ctx);
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
        // this rebuild): tear down the feedback layer. `DragSession` cannot
        // do this itself (`OverlayEntry::remove` needs `Rc` access it
        // structurally lacks — see `feedback`'s docs on `DragSession`).
        if currently_active == 0 {
            // Taken out and the borrow dropped (this statement ends before
            // the `if let` runs) before the framework call below.
            let stale = self.feedback_entry.borrow_mut().take();
            if let Some(entry) = stale {
                entry.remove();
            }
        }

        if showing_child_when_dragging {
            let builder = view
                .child_when_dragging
                .clone()
                .expect("BUG: checked is_some above");
            listener.child(builder())
        } else {
            match view.child.clone().into_inner() {
                Some(child) => listener.child(child),
                None => listener,
            }
        }
    }

    /// Disposes the recognizer unconditionally, even if a drag is still
    /// active. See `DragSession`'s docs on why this diverges from the
    /// oracle's `_disposeRecognizerIfInactive` keep-alive: any drag still in
    /// flight is canceled right here (the recognizer's own `dispose` fires
    /// `.cancel()` on every accepted handle), rather than tracked to the
    /// user's real pointer-up.
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
    // `LaidOutScoped` capability can drive two truly concurrent pointer
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
        owner.schedule_build_for(root, 0);
        owner.build_scope(&mut tree);

        captured
            .borrow()
            .clone()
            .expect("init_state must have captured a handle")
    }

    fn empty_config() -> Arc<Mutex<DragConfig>> {
        Arc::new(Mutex::new(DragConfig {
            axis: None,
            max_simultaneous_drags: None,
            on_drag_started: None,
            on_drag_update: None,
            on_draggable_canceled: None,
            on_drag_end: None,
        }))
    }

    fn update_details(dx: f32, dy: f32) -> MultiDragUpdateDetails {
        MultiDragUpdateDetails {
            pointer_id: PointerId::new(1).expect("contact ids start at 1"),
            global_position: Offset::new(Pixels(dx), Pixels(dy)),
            local_position: Offset::new(Pixels(dx), Pixels(dy)),
            delta: Offset::new(PixelDelta(dx), PixelDelta(dy)),
            kind: PointerType::Mouse,
            timestamp: Instant::now(),
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
        let session_1 = DragSession {
            active_count: Arc::new(AtomicUsize::new(1)),
            rebuild: rebuild.clone(),
            config: empty_config(),
            offset: Mutex::new(Offset::ZERO),
            feedback: Some(signal_1.clone()),
        };

        // ...then a second session starts and evicts it — minting its OWN
        // signal per the fix, not `signal_1.clone()`.
        let signal_2 = FeedbackSignal::new();
        let session_2 = DragSession {
            active_count: Arc::new(AtomicUsize::new(1)),
            rebuild,
            config: empty_config(),
            offset: Mutex::new(Offset::ZERO),
            feedback: Some(signal_2.clone()),
        };

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
}
