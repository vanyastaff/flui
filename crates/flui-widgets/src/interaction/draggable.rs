//! [`Draggable`] — a widget that can be picked up and dragged, carrying typed
//! `data` for a [`DragTarget`](crate::DragTarget) to receive on drop.
//!
//! A long-press variant and a configurable drag-anchor strategy are named
//! deferrals; see the notes below.
//!
//! # Limits (framework-surface gaps)
//!
//! 1. **Feedback paints, but at a displacement, not a global position.**
//!    `DraggableState` resolves the ancestor `Overlay` (`Overlay::maybe_of`,
//!    ADR-0076) in `did_change_dependencies` and, on drag start, inserts
//!    `feedback` as a real `OverlayEntry`. What remains open is *where* it
//!    paints: ideally anchored at the drag's true global position (a
//!    drag-anchor strategy plus the pointer's live global coordinates — see
//!    note #4 below). There is neither a drag-anchor strategy nor the
//!    global-origin term note #4 names as missing, so the feedback entry
//!    positions itself at `feedback_offset` plus the same
//!    **displacement-since-start** `DragSession` already tracks for
//!    `DraggableDetails.offset` — visibly correct only for a `Draggable`
//!    sitting at the screen origin, honestly wrong (by exactly that origin)
//!    everywhere else, the same shape of gap as #4. A root-overlay choice,
//!    `ignoring_feedback_*` options, and scaled/rotated-ancestor correctness
//!    are separate, still-open gaps (ADR-0076's deferrals).
//! 2. **Live drag-target discovery, reached through a private origin probe.**
//!    Every move hit-tests at the pointer's *current* global position,
//!    independent of wherever the drag's own pointer went down, and walks the
//!    result for metadata-tagged `DragTarget`s.
//!    `LifecycleContext::hit_test_handle()` (acquired in `init_state` /
//!    `did_change_dependencies`, never from a frame phase) runs a fresh test
//!    against the live render tree, and [`DragTarget`](crate::DragTarget)
//!    publishes its interaction-lane ticket as hit-test metadata for the walk
//!    to find and resolve back to its owner-local `DragTargetSlot`. Pointer dispatch still resolves its own route once at
//!    `PointerDown` and replays it; the fresh probe is deliberately
//!    independent of that route, which is the whole point.
//!
//!    **Where the position comes from.** FLUI's pointer events are
//!    `ui_events` types with room for one position, so dispatch delivers the
//!    global/local pair beside the event, as a `PointerDispatch`.
//!    `GestureRecognizer::handle_event` takes the dispatch, and every
//!    `Drag*Details` reports its `global_position` from the untransformed
//!    half. So a drag knows both spaces, and a consumer reaching for the
//!    global one gets a global one.
//!
//!    What survives is the ORIGIN probe below, for a different reason: it
//!    needs the position of the draggable's own node, not of the pointer, and
//!    no event carries that. The conversion between exactly those two spaces
//!    is `PipelineOwner::local_to_global`, which needs the
//!    `RenderId` of the node the local point belongs to, and [`DragOrigin`] —
//!    a payload-free view mounted as the `Listener`'s direct child — is how
//!    this widget learns it: its `find_render_object()` stops at the
//!    `Listener`'s own render node, the node dispatch localized against. The
//!    conversion composes the whole ancestor chain, so it is exact under scale
//!    and rotation, not just translation.
//!
//!    Handing the recognizer the global event instead would not remove the
//!    probe honestly — it would make `global_position` truthful and
//!    `local_position` a lie. What removes it is giving the recognizers a
//!    paired local/global position, which is its own change; see mapping
//!    decision 3 in `crates/flui-widgets/ARCHITECTURE.md` for what that costs.
//!
//!    Two consequences worth naming rather than discovering later. A drag
//!    carrying **no data** discovers nothing at all: a target's data-type
//!    filter has nothing to match, and a null-data drag that would enter
//!    *every* target has no representation here (`ErasedDragData` erases a
//!    concrete value, not an `Option`). And `axis` restriction is applied to
//!    deltas in the `Listener`'s space rather than the root's, which differs
//!    only under a rotating ancestor.
//!
//! 3. **No long-press variant.** It would need a delayed multi-drag
//!    recognizer, which does not exist in `flui-interaction` yet (only the
//!    immediate `MultiDragGestureRecognizer` does). Deferred rather than
//!    hand-rolling a new recognizer as a side effect of this widget.
//! 4. **No configurable drag-anchor strategy, `affinity`, `hit_test_behavior`,
//!    `ignoring_feedback_*`, root-overlay choice or allowed-buttons filter.**
//!    `ignoring_feedback_*`/root-overlay only affect the feedback overlay
//!    (moot per point 1). `affinity` selects which single-axis recognizer
//!    competes for the *start* of the gesture — a named deferral, unrelated
//!    to `Draggable::axis` (implemented), which restricts *reported*
//!    movement after the drag has already started. A drag-anchor strategy is
//!    **not** merely cosmetic feedback positioning: it defines the drag start
//!    point, which would be subtracted from every reported global position to
//!    produce `DraggableDetails.offset` / `DragTargetDetails.offset`.
//!
//!    **A further, separately-named gap in the reported offset itself.**
//!    With a child-anchored strategy the start point is a LOCAL offset in the
//!    draggable's own render object, while the reported position is GLOBAL.
//!    Writing `globalOrigin` for `Draggable`'s own render object's global
//!    top-left corner, the reported offset reduces to
//!    `globalOrigin + Σ(axis-restricted deltas since the drag started)` —
//!    **not** just the running sum. The running sum alone (which is all
//!    [`DragSession::offset`] tracks: seeded at `Offset::ZERO`, never given a
//!    `globalOrigin` term) is correct only for a `Draggable` whose render
//!    object sits at the screen origin; for any other position, the reported
//!    offset is short by exactly that origin. **The blocker this note used to
//!    name is gone**: point 2's origin probe now converts a `Listener`-local
//!    point to the root's space through `PipelineOwner::local_to_global`,
//!    which is exactly the `globalOrigin` term this formula wants. Closing it
//!    is nonetheless a separate change — it alters what
//!    `DraggableDetails.offset` and `on_draggable_canceled` report to existing
//!    callers, and a drag-anchor strategy (which decides the start point) has
//!    to land with it or the "fix" would be a different wrong value. What
//!    ships is **displacement since the drag started**, not a
//!    globally-anchored value; no test pins it.
//!
//!    Separately, anchoring at the pointer (`Offset::ZERO`) is not selectable
//!    at all — that is the actual, named deferral for *strategy choice*,
//!    distinct from the offset gap above.
//! 5. **Unmounting mid-drag still cancels immediately.** Ideally the
//!    recognizer and overlay lifetime would transfer to active drags until
//!    their real pointer-up. Both resources stay on `DraggableState`, so
//!    unmount disposes the recognizer and removes feedback immediately.
//!    `MultiDragHandle` is owner-local, removing the former type-system
//!    obstacle; the remaining work is a real lifetime transfer, not a
//!    threading workaround.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::geometry::Axis;
use flui_foundation::geometry::{Matrix4, Offset};
use flui_interaction::{
    DragUpdateDetails, GestureRecognizer, HitTestEntry, HitTestHandle, InteractionDispatchError,
    LocalPayloadTarget, MultiDragAxis, MultiDragEndDetails, MultiDragGestureRecognizer,
    MultiDragHandle, MultiDragStartCallback, MultiDragUpdateDetails, PointerEventExt as _,
    PointerId, Velocity, resolve_local_payload,
};
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{EventCx, EventOutcome, RebuildHandle, WriterSource};
use parking_lot::Mutex;

use crate::overlay::{InsertPosition, Overlay, OverlayEntry, OverlayHandle};
use crate::support::{EventCallback, ValueCallback, event_callback, value_callback};
use crate::{
    DragPosition, DragTargetSlot, ErasedDragData, GestureArenaScope, IgnorePointer, Listener,
    Positioned, Stack, StackFit,
};

/// A no-argument drag callback: started, completed. Owner-local, like the
/// [`MultiDragHandle`] that invokes it, and run inside a write the
/// draggable's [`WriterSource`] opens (ADR-0086).
type StartedCallback = EventCallback;
/// Called for each pointer move while a drag is in progress.
type DragUpdateCallback = ValueCallback<DragUpdateDetails>;
/// Called once when a drag ends, accepted or not.
type DragEndCallback = ValueCallback<DraggableDetails>;
/// Called when a drag ends without being accepted by a target.
type DraggableCanceledCallback = ValueCallback<DraggableCanceledDetails>;

/// Details for [`Draggable::on_drag_end`] — the velocity and position at
/// release, and whether a [`DragTarget`](crate::DragTarget) accepted the drop.
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
    /// note #4: the draggable's global origin is not added on top of this sum
    /// (a named, pinned gap, not a raw position either way).
    pub offset: Offset<f64>,
}

/// Details for [`Draggable::on_draggable_canceled`]: the velocity and the
/// displacement at the moment the drag ended without a target accepting it.
///
/// The two are one value so the callback's shape stays `|cx, details|`, which a
/// `let`-bound closure can name through [`callback_with`];
/// see mapping decision 38 in `crates/flui-widgets/ARCHITECTURE.md`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DraggableCanceledDetails {
    /// Velocity at release; zero for a cancelled pointer or an unmount.
    pub velocity: Velocity,
    /// Displacement since the drag started, with the same meaning as
    /// [`DraggableDetails::offset`].
    pub offset: Offset<f64>,
}

/// A widget that can be dragged, carrying `data` for a
/// [`DragTarget`](crate::DragTarget) to receive.
///
/// See the module docs for the limits, notably where the drag's global
/// position comes from.
#[derive(Clone, StatefulView)]
pub struct Draggable<T: Clone + Send + Sync + 'static> {
    child: Child,
    child_when_dragging: Option<Rc<dyn Fn() -> BoxedView>>,
    feedback: Option<Rc<dyn Fn() -> BoxedView>>,
    data: Option<T>,
    axis: Option<Axis>,
    feedback_offset: Offset<f64>,
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

    /// Restricts reported drag movement to one axis.
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
    /// ADR-0076) — positioned at a **displacement**, not a true
    /// global anchor; see the module notes.
    #[must_use]
    pub fn feedback(mut self, builder: impl Fn() -> BoxedView + 'static) -> Self {
        self.feedback = Some(Rc::new(builder));
        self
    }

    /// Offset from the drag anchor to where `feedback` is painted, added to
    /// the tracked displacement (see the module divergence notes).
    #[must_use]
    pub fn feedback_offset(mut self, offset: Offset<f64>) -> Self {
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
    ///
    /// Every drag callback receives the dispatch's `&mut EventCx<'_>` first
    /// and may return `()` or a `Result` whose refusal is reported (ADR-0086).
    #[must_use]
    pub fn on_drag_started<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_drag_started = Some(event_callback(callback));
        self
    }

    /// Called for each pointer move while the drag is in progress.
    #[must_use]
    pub fn on_drag_update<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, DragUpdateDetails) -> R + 'static,
        R: EventOutcome,
    {
        self.on_drag_update = Some(value_callback(callback));
        self
    }

    /// Called when the drag ends without a target accepting it — including
    /// every cancel, every drop over nothing, and an unmount mid-drag.
    #[must_use]
    pub fn on_draggable_canceled<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, DraggableCanceledDetails) -> R + 'static,
        R: EventOutcome,
    {
        self.on_draggable_canceled = Some(value_callback(callback));
        self
    }

    /// Called once the drag ends, accepted or not.
    #[must_use]
    pub fn on_drag_end<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, DraggableDetails) -> R + 'static,
        R: EventOutcome,
    {
        self.on_drag_end = Some(value_callback(callback));
        self
    }

    /// Called when a target accepts the drop. Fires instead of
    /// [`on_draggable_canceled`](Self::on_draggable_canceled), never
    /// alongside it.
    #[must_use]
    pub fn on_drag_completed<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_drag_completed = Some(event_callback(callback));
        self
    }
}

/// Persistent gesture state: the recognizer survives rebuilds (the pointer
/// stream is stateful) and is disposed on unmount — see
/// `DragSession`'s docs for why it does not outlive the element.
/// Follows `GestureDetectorState`'s init_state-acquires-the-arena shape.
pub struct DraggableState<T: Clone + Send + Sync + 'static> {
    /// How many drags this widget currently has active — gates
    /// `max_simultaneous_drags` and switches `child` vs `child_when_dragging`.
    active_count: Arc<AtomicUsize>,
    /// The live config the recognizer's `on_start` closure reads at drag-start
    /// time (data, callbacks, axis, max-drags). Refreshed each `build`.
    config: Rc<RefCell<DragConfig>>,
    /// The nearest ancestor `Overlay`'s handle, if any — resolved in
    /// `did_change_dependencies` (a lifecycle hook, per
    /// ADR-0018's pattern), not in `build` or from inside the
    /// `on_start` gesture callback, neither of which holds a `BuildContext`.
    /// `Arc<Mutex<_>>` so the `on_start` closure captured once in
    /// `init_state` always reads the latest resolution.
    overlay: Arc<Mutex<Option<OverlayHandle>>>,
    /// The fresh-hit-test capability, resolved in `init_state` /
    /// `did_change_dependencies` — a lifecycle hook, never `build` or a
    /// gesture callback, because a hit test taken
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
    /// space. A lifecycle-acquired capability like the two above:
    /// only ever read from a gesture callback, never from a
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
    /// `on_start` at drag-start time. Owner-local (`Rc<RefCell<_>>`), like
    /// [`DragConfig`]; see [`FeedbackConfig`] for why the two are separate.
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
/// throughout its lifetime.
///
/// Owner-local: the handle is `!Send` (it holds the `Rc` event callbacks), as
/// is the [`MultiDragHandle`] that reads it. Every reader clones what it
/// needs out of the cell and releases the borrow before any user callback
/// runs, so a callback that rebuilds the widget cannot collide with a live
/// borrow.
///
/// `Draggable::data` is carried **erased** (`ErasedDragData`): a session hands
/// it to targets that cannot name `T`, and `Arc<dyn Any + Send + Sync>` is
/// what a hit-test-discovered target's callbacks downcast from. The payload
/// no longer crosses hit-test metadata (a target publishes only its lane
/// ticket there); its `Send + Sync` bound goes with the `T: Send + Sync`
/// bound on `Draggable` and `DragTarget`, a separate change. What a live drag reads is its own
/// [`DragStart`] snapshot, not this. `feedback`/`feedback_offset` live in the
/// separate [`FeedbackConfig`] cell.
struct DragConfig {
    axis: Option<Axis>,
    max_simultaneous_drags: Option<usize>,
    /// The CURRENT payload, type-erased, refreshed on every build. A drag
    /// snapshots it once at start ([`DragStart`]) and never reads it again, so
    /// this is only ever what the *next* drag to start will carry.
    ///
    /// `None` when the `Draggable` carries no data — such a drag discovers
    /// nothing, since a target's data-type filter has nothing to
    /// match against (a null-data drag that enters every target has no
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

/// `feedback`/`feedback_offset`, refreshed each `build`. Kept apart from
/// [`DragConfig`], which a live session reads throughout the drag: only
/// `on_start` reads this, once, and a session never sees it.
struct FeedbackConfig {
    feedback: Option<Rc<dyn Fn() -> BoxedView>>,
    feedback_offset: Offset<f64>,
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
    offset: Arc<Mutex<Offset<f64>>>,
    /// The mounted [`FeedbackAnchor`] element's own rebuild capability,
    /// published by [`FeedbackAnchorState::init_state`] (never from `build`)
    /// so [`DragSession::update`] can reposition it
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

    fn offset(&self) -> Offset<f64> {
        *self.offset.lock()
    }

    fn set_offset(&self, offset: Offset<f64>) {
        *self.offset.lock() = offset;
    }

    fn publish_rebuild(&self, handle: RebuildHandle) {
        let _prev = self.rebuild.lock().replace(handle);
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
/// (ADR-0021) — so the inner `Stack` is load-bearing, not decorative.
///
/// A real, `Rc`-backed `StatefulView` (not a bare closure) specifically so its
/// `init_state` can acquire a `RebuildHandle` the ADR-0018 way and publish it
/// to [`FeedbackSignal`], allowing the session to reposition this content
/// without retaining a framework element reference.
#[derive(Clone)]
struct FeedbackAnchor {
    feedback: Rc<dyn Fn() -> BoxedView>,
    feedback_offset: Offset<f64>,
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
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.signal.publish_rebuild(ctx.rebuild_handle());
    }

    fn build(&self, view: &FeedbackAnchor, _ctx: &dyn BuildContext) -> impl IntoView {
        let displacement = self.signal.offset();
        Stack::new(vec![
            // `IgnorePointer` is load-bearing, not decorative: this entry is
            // the TOPMOST one in the overlay and sits under the pointer by
            // construction, so hit-testable feedback claims the very position
            // the drag re-probes on every move and hides every `DragTarget`
            // beneath it. The drag would then leave the target it is visibly
            // hovering. Making it configurable (and its semantics counterpart)
            // stays a named deferral.
            Positioned::new(IgnorePointer::new().child((view.feedback)()))
                .left(view.feedback_offset.dx + displacement.dx)
                .top(view.feedback_offset.dy + displacement.dy)
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
    feedback_offset: Offset<f64>,
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
            let _prev = feedback_entry_slot.borrow_mut().replace(entry);
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
    feedback_offset: Offset<f64>,
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

/// Restricts `offset` to `axis`'s component.
fn restrict_axis(offset: Offset<f64>, axis: Option<Axis>) -> Offset<f64> {
    match axis {
        Some(Axis::Horizontal) => Offset::new(offset.dx, 0.0),
        Some(Axis::Vertical) => Offset::new(0.0, offset.dy),
        None => offset,
    }
}

/// [`restrict_axis`], for the per-update delta's `PixelDelta` unit.
fn restrict_axis_delta(delta: Offset<f64>, axis: Option<Axis>) -> Offset<f64> {
    match axis {
        Some(Axis::Horizontal) => Offset::new(delta.dx, 0.0),
        Some(Axis::Vertical) => Offset::new(0.0, delta.dy),
        None => delta,
    }
}

/// Publishes the `RenderId` of the node whose coordinate space a
/// `Draggable`'s pointer events arrive in.
///
/// Pointer dispatch carries both spaces to a `Listener` callback, but the
/// gesture recognizers below it still take a single-space `PointerEvent`, and
/// this widget reads its position from a recognizer's `Drag*Details` rather
/// than from the event. A drag therefore knows only where the pointer is
/// inside its own `Listener`; a hit test needs where it is in the root's
/// space.
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
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
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
    slot: Rc<DragTargetSlot>,
    at: DragPosition,
}

/// `global` mapped into the space `transform` describes.
///
/// A hit entry's transform already maps global to local — `HitTestResult`
/// folds each level's own inverse as it descends — so this applies it
/// directly rather than inverting again. An entry with no transform is
/// already in the root's space.
fn localize(global: Offset<f64>, transform: Option<&Matrix4>) -> Offset<f64> {
    let Some(transform) = transform else {
        return global;
    };
    let (x, y) = transform.transform_point(global.dx, global.dy);
    Offset::new(x, y)
}

/// The drag targets on `path`, leaf-first, that will take `data`.
///
/// Walks the hit path for
/// metadata-tagged targets and keeps those whose `T` matches the drag's
/// payload. Order is the path's own, which is what
/// makes the innermost of a set of nested targets win.
///
/// A target tags its node with a lane ticket, resolved here to its
/// owner-local slot; this runs inside pointer dispatch, where the UI runtime's
/// lane is active. A ticket that no longer resolves because its target
/// unmounted since the hit test is skipped, as a foreign payload is. Any
/// other lane error means the question could not be asked at all (no UI runtime
/// entered, or another UI runtime's), so the answer is `None`, not an empty list:
/// see [`DragSession::discover`].
fn drag_targets_on(
    path: &[HitTestEntry],
    data: &ErasedDragData,
    global: Offset<f64>,
) -> Option<Vec<EnteredTarget>> {
    let mut targets = Vec::new();
    for entry in path {
        let Some(target) = entry.metadata_as::<LocalPayloadTarget>().copied() else {
            continue;
        };
        let payload = match resolve_local_payload(target) {
            Ok(payload) => payload,
            Err(InteractionDispatchError::TargetGone) => {
                tracing::debug!("drag-target ticket outlived its target; skipped");
                continue;
            }
            Err(error) => {
                tracing::debug!(
                    ?error,
                    "drag-target discovery skipped: the lane could not answer"
                );
                return None;
            }
        };
        // A lane payload is `dyn Any` by construction; this is the check
        // that the tagged node is a drag target's slot.
        let Ok(slot) = payload.downcast::<DragTargetSlot>() else {
            continue;
        };
        if slot.accepts_data_type(data) {
            targets.push(EnteredTarget {
                at: DragPosition {
                    global,
                    local: localize(global, entry.transform.as_ref()),
                },
                slot,
            });
        }
    }
    Some(targets)
}

/// What a drag reads from its `Draggable` exactly once, when it starts.
///
/// A drag is constructed with the widget's `data` and
/// `feedback_offset` and never looks at the widget again, so a rebuild
/// under a live drag cannot change what that drag is carrying or where it
/// probes. Reading either live instead splits one drag in two: targets entered
/// before the rebuild hold the old payload (the slot stored it at `did_enter`)
/// while later ones are offered the new value, and the probe aims at an offset
/// the mounted feedback layer was never built with.
///
/// One struct rather than two fields on purpose: `feedback_offset` is read
/// here from the same borrow that mounts the feedback entry, so the offset the
/// probe is displaced by and the offset the visible layer is positioned at
/// cannot drift apart.
struct DragStart {
    /// The drag's payload, type-erased for delivery to targets that cannot
    /// name `T`. `None` when the `Draggable` carries no data — such a drag
    /// discovers nothing, since a target's data-type filter has
    /// nothing to match against.
    data: Option<ErasedDragData>,
    /// Displacement from the pointer to the feedback layer, and therefore from
    /// the pointer to the point the drag hit-tests (the global pointer
    /// position plus this offset).
    feedback_offset: Offset<f64>,
}

/// One instance per active drag, held by the
/// recognizer for the pointer's lifetime. It owns the drag's standing with
/// every [`DragTargetSlot`] it has entered, and re-discovers that set on
/// every move.
///
/// **Current limit:** the recognizer
/// and feedback entry still belong to `DraggableState`, rather than being
/// transferred to active sessions. Unmount therefore cancels the session
/// immediately. The handle is owner-local and can carry that ownership in a
/// future change; no `Send + Sync` constraint prevents it.
struct DragSession {
    active_count: Arc<AtomicUsize>,
    rebuild: RebuildHandle,
    config: Rc<RefCell<DragConfig>>,
    /// Opens the `EventCx` each user callback runs in (ADR-0086). A session
    /// ended by unmount runs its callbacks from `finalize_tree`, outside any
    /// build, so those writes land too.
    writer: WriterSource,
    /// The contact this session follows. Every transition a target receives
    /// is keyed by it, which is what keeps simultaneous drags independent.
    pointer: PointerId,
    /// The fresh-hit-test capability, acquired by `DraggableState` in
    /// `init_state` / `did_change_dependencies` and shared by cell so a later
    /// re-resolution reaches a session that started before it. `None` when the
    /// embedder installed none, in which case this drag discovers nothing.
    hit_test: Rc<RefCell<Option<HitTestHandle>>>,
    /// Everything this drag captured from its widget at start — see
    /// [`DragStart`] for why none of it is re-read.
    start: DragStart,
    /// The render node the drag's pointer positions are local to, and the
    /// tree that can convert them to the root's space — see [`DragOrigin`].
    listener_node: Rc<Cell<Option<flui_foundation::RenderId>>>,
    pipeline: Rc<RefCell<Option<flui_rendering::pipeline::PipelineCell>>>,
    /// The drag's current position: the contact's
    /// down position plus every axis-restricted delta since.
    ///
    /// In the `Listener`'s LOCAL space, because that is the space every
    /// pointer event reaches a widget in — [`to_global`](Self::to_global)
    /// converts it for the hit test and for what targets are told.
    ///
    /// Distinct from [`offset`](Self::offset), which is the same sum without
    /// the starting point — see that field, and the module's divergence
    /// note #4 on why `DraggableDetails.offset` keeps that narrower meaning.
    position: Mutex<Offset<f64>>,
    /// Every target this drag is currently inside, outermost-last, and the
    /// drag position each one last saw.
    entered: RefCell<Vec<EnteredTarget>>,
    /// The first entered target that accepted the drag, if any
    /// — the one a drop is delivered to, and the
    /// position it last saw, so the drop reports where the pointer actually
    /// was instead of re-deriving it.
    active: RefCell<Option<EnteredTarget>>,
    /// Running sum of every axis-restricted delta since the drag started —
    /// displacement, seeded at `Offset::ZERO`. It does **not** include the
    /// draggable's global origin (see the module's note #4 — a named, pinned
    /// gap, not attempted here). Reported as `DraggableDetails.offset`.
    offset: Mutex<Offset<f64>>,
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
    fn to_global(&self, local: Offset<f64>) -> Option<Offset<f64>> {
        let node = self.listener_node.get()?;
        let pipeline = self.pipeline.borrow().clone()?;
        let global = pipeline.try_with(|owner| {
            owner.local_to_global(
                node,
                flui_foundation::geometry::Point::new(local.dx, local.dy),
                None,
            )
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
    fn discover(&self, global: Offset<f64>) -> Option<(ErasedDragData, Vec<EnteredTarget>)> {
        let handle = self.hit_test.borrow().clone()?;
        // Both from the start-time snapshot, never re-read from the widget:
        // see `DragStart`. Carried onwards so the payload that decided which
        // targets match is the payload those targets are then handed.
        let data = self.start.data.clone()?;
        let probe_at = global + self.start.feedback_offset;
        match handle.hit_test_at(probe_at) {
            Ok(snapshot) => {
                let targets = drag_targets_on(snapshot.path(), &data, global)?;
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
    /// Includes a prefix-match fast path: it bails to move-only when the new
    /// target list
    /// starts with exactly the entered list AND either something has already
    /// accepted (deeper targets below the active one are correctly ignored) or
    /// the lists are the same length (nothing has accepted, so the entered list
    /// holds every hit target and a longer list means a new one appeared).
    fn update_drag(&self, local: Offset<f64>) {
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
    fn update_drag_at(&self, global: Offset<f64>) {
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
                    .all(|(was, now)| Rc::ptr_eq(&was.slot, &now.slot))
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
        let _prev = std::mem::replace(&mut *self.entered.borrow_mut(), newly_entered);
        let _prev = std::mem::replace(&mut *self.active.borrow_mut(), new_active);

        // Cloned out and the borrow dropped before any callback runs, the same
        // as the move-only path above.
        let moving: Vec<EnteredTarget> = self.entered.borrow().clone();
        for target in moving {
            target.slot.did_move(self.pointer, target.at);
        }
    }

    /// Leave every target this drag has entered, in entry order.
    fn leave_all_entered(&self) {
        let leaving: Vec<EnteredTarget> = self.entered.borrow_mut().drain(..).collect();
        for target in leaving {
            target.slot.did_leave(self.pointer);
        }
    }

    /// Deliver the drop, if this drag ends over an accepting target, then
    /// leave everything. Returns whether a target took the data.
    ///
    /// The return value reports whether the data was actually taken:
    /// [`DragTargetSlot::did_drop`] answers false for a target that had left
    /// the tree, even though an active target existed.
    fn finish_drag(&self, dropped: bool) -> bool {
        let mut was_accepted = false;
        let active = self.active.borrow_mut().take();
        if dropped && let Some(active) = active {
            was_accepted = active.slot.did_drop(self.pointer, active.at);
            self.entered
                .borrow_mut()
                .retain(|target| !Rc::ptr_eq(&target.slot, &active.slot));
        }
        self.leave_all_entered();
        let _prev = self.active.borrow_mut().take();
        was_accepted
    }
}

impl MultiDragHandle for DragSession {
    fn update(&self, details: MultiDragUpdateDetails) {
        let axis = self.config.borrow().axis;
        let restricted = restrict_axis_delta(details.delta, axis);
        let moved = restricted.dx != 0.0 || restricted.dy != 0.0;
        if moved {
            let step = Offset::new(restricted.dx, restricted.dy);
            *self.offset.lock() += step;
            *self.position.lock() += step;
            if let Some(feedback) = &self.feedback {
                feedback.set_offset(*self.offset.lock());
                feedback.reposition();
            }
        }

        // Unconditional: only
        // `on_drag_update` is gated on the restricted position having moved.
        // Targets still expect a move report for a sample that did not move
        // them, and a rebuild elsewhere can change what is under the pointer
        // without the pointer itself moving at all.
        self.update_drag(*self.position.lock());

        if !moved {
            return;
        }
        // The RAW (unrestricted) `details` pass through
        // to `on_drag_update` unchanged — only the *gate* ("did the restricted
        // position move") is axis-aware, not the reported delta.
        // Cloned out: the borrow ends before the callback runs.
        let on_drag_update = self.config.borrow().on_drag_update.clone();
        if let Some(callback) = on_drag_update {
            let primary_delta = match axis {
                Some(Axis::Horizontal) => details.delta.dx,
                Some(Axis::Vertical) => details.delta.dy,
                None => 0.0,
            };
            let update = DragUpdateDetails {
                global_position: details.global_position,
                local_position: details.local_position,
                delta: details.delta,
                primary_delta,
                kind: details.kind,
            };
            self.writer.write(|cx| callback(cx, update));
        }
    }

    fn end(&self, details: MultiDragEndDetails) {
        // The drop lands before the state change: `did_drop` runs, then
        // `on_drag_end` reports the outcome.
        let was_accepted = self.finish_drag(true);
        self.end_active();

        // Cloned out: the borrow ends before any callback runs.
        let (velocity, on_drag_end, on_drag_completed, on_draggable_canceled) = {
            let config = self.config.borrow();
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
            let details = DraggableDetails {
                was_accepted,
                velocity,
                offset,
            };
            self.writer.write(|cx| callback(cx, details));
        }
        if was_accepted {
            if let Some(callback) = on_drag_completed {
                self.writer.write(|cx| callback(cx));
            }
        } else if let Some(callback) = on_draggable_canceled {
            let details = DraggableCanceledDetails { velocity, offset };
            self.writer.write(|cx| callback(cx, details));
        }
    }

    fn cancel(&self) {
        // A cancelled drag delivers nothing, but must still leave every
        // target it had entered — otherwise a target keeps a candidate that
        // no live drag will ever remove.
        self.finish_drag(false);
        self.end_active();

        // A cancel also routes through the finish path, which fires
        // `on_drag_end` unconditionally (zero velocity, not accepted, but the
        // real offset — not zero) before `on_draggable_canceled` — not a
        // cancel-only path.
        // Cloned out: the borrow ends before any callback runs.
        let (on_drag_end, on_draggable_canceled) = {
            let config = self.config.borrow();
            (
                config.on_drag_end.clone(),
                config.on_draggable_canceled.clone(),
            )
        };
        let offset = *self.offset.lock();
        if let Some(callback) = on_drag_end {
            let details = DraggableDetails {
                was_accepted: false,
                velocity: Velocity::ZERO,
                offset,
            };
            self.writer.write(|cx| callback(cx, details));
        }
        if let Some(callback) = on_draggable_canceled {
            let details = DraggableCanceledDetails {
                velocity: Velocity::ZERO,
                offset,
            };
            self.writer.write(|cx| callback(cx, details));
        }
    }
}

impl<T: Clone + Send + Sync + 'static> StatefulView for Draggable<T> {
    type State = DraggableState<T>;

    fn create_state(&self) -> Self::State {
        DraggableState {
            active_count: Arc::new(AtomicUsize::new(0)),
            config: Rc::new(RefCell::new(DragConfig::from_view(self))),
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
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let arena = GestureArenaScope::of(ctx);
        let rebuild = ctx.rebuild_handle();
        let writer = ctx.writer_source();

        // The *initial* resolution, not just re-resolution: `depend_on`
        // (which `Overlay::maybe_of` calls) only registers this element as a
        // dependent — it does not, by itself, guarantee `did_change_dependencies`
        // fires on first mount with no prior dependency to notify about. Same
        // two-call shape `FocusScopeState` uses for `enclosing_focus_parent`
        // (`interaction/focus.rs`): resolve here for the first value, and
        // again in `did_change_dependencies` for later changes.
        let _prev = std::mem::replace(&mut *self.overlay.lock(), Overlay::maybe_of(ctx));
        let _prev = std::mem::replace(&mut *self.hit_test.borrow_mut(), ctx.hit_test_handle());
        let _prev = std::mem::replace(&mut *self.pipeline.borrow_mut(), ctx.pipeline_owner());

        let active_count = Arc::clone(&self.active_count);
        let config = Rc::clone(&self.config);
        let overlay = Arc::clone(&self.overlay);
        let feedback_entry_slot = Rc::clone(&self.feedback_entry);
        let feedback_config = Rc::clone(&self.feedback_config);
        let hit_test = Rc::clone(&self.hit_test);
        let listener_node = Rc::clone(&self.listener_node);
        let pipeline = Rc::clone(&self.pipeline);
        let on_start: MultiDragStartCallback = Rc::new(move |pointer, initial_position| {
            {
                let guard = config.borrow();
                if let Some(max) = guard.max_simultaneous_drags
                    && active_count.load(Ordering::Acquire) >= max
                {
                    return None;
                }
            }
            active_count.fetch_add(1, Ordering::AcqRel);
            rebuild.schedule(flui_view::RebuildReason::StateChange);
            // Cloned out: the borrow ends before the callback runs.
            let callback = config.borrow().on_drag_started.clone();
            if let Some(callback) = callback {
                writer.write(|cx| callback(cx));
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

            // The snapshot this drag lives on. `feedback_offset` is the very
            // value the entry above was mounted with, so the probe and the
            // visible layer cannot disagree.
            let start = DragStart {
                data: config.borrow().data.clone(),
                feedback_offset,
            };

            Some(Box::new(DragSession {
                active_count: Arc::clone(&active_count),
                rebuild: rebuild.clone(),
                config: Rc::clone(&config),
                writer: writer.clone(),
                pointer,
                hit_test: Rc::clone(&hit_test),
                start,
                listener_node: Rc::clone(&listener_node),
                pipeline: Rc::clone(&pipeline),
                // The contact's own down position, unlike `offset` below —
                // see `DragSession::position`.
                position: Mutex::new(initial_position),
                entered: RefCell::new(Vec::new()),
                active: RefCell::new(None),
                offset: Mutex::new(Offset::ZERO),
                feedback,
            }) as Box<dyn MultiDragHandle>) // see flui-interaction's MultiDragStartCallback — the per-pointer handle `MultiDragGestureRecognizer::with_on_start` requires.
        });

        self.recognizer = Some(
            MultiDragGestureRecognizer::new(arena, MultiDragAxis::Free).with_on_start(on_start),
        );
    }

    /// Re-resolves everything this widget reads from its `BuildContext`: the
    /// nearest ancestor `Overlay`, the fresh-hit-test capability, and the
    /// render tree.
    ///
    /// A lifecycle hook, not `build` and not the
    /// `on_start` gesture callback above, neither of which holds a
    /// `BuildContext`. Re-resolved on every dependency change, not just once:
    /// `Overlay::maybe_of` depends (ADR-0076), so a *different* enclosing
    /// overlay later replacing this one is exactly what re-fires this hook.
    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        let _prev = std::mem::replace(&mut *self.overlay.lock(), Overlay::maybe_of(ctx));
        let _prev = std::mem::replace(&mut *self.hit_test.borrow_mut(), ctx.hit_test_handle());
        let _prev = std::mem::replace(&mut *self.pipeline.borrow_mut(), ctx.pipeline_owner());
    }

    fn build(&self, view: &Draggable<T>, _ctx: &dyn BuildContext) -> impl IntoView {
        let _prev = std::mem::replace(&mut *self.config.borrow_mut(), DragConfig::from_view(view));
        let _prev = std::mem::replace(
            &mut *self.feedback_config.borrow_mut(),
            FeedbackConfig::from_view(view),
        );

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
            // The whole pair goes through. `DragSession`'s accumulated
            // position, `to_global` and the `DragOrigin` probe are all built
            // on the LOCAL half being the `Listener`'s own space, and stay
            // that way; the global half is what lets a recogniser report a
            // global position at all, since dispatch rewrote it away before
            // any handler here runs.
            .on_pointer_down(move |_cx, dispatch| {
                if let Some(max) = max
                    && active_count.load(Ordering::Acquire) >= max
                {
                    return;
                }
                let event = dispatch.local;
                down_recognizer.add_pointer(
                    event.pointer_id(),
                    event.position(),
                    dispatch.global.position(),
                );
            })
            .on_pointer_move(move |_cx, dispatch| move_recognizer.handle_event(dispatch))
            .on_pointer_up(move |_cx, dispatch| up_recognizer.handle_event(dispatch))
            .on_pointer_cancel(move |_cx, dispatch| cancel_recognizer.handle_event(dispatch));

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
    /// drag is canceled here instead of surviving unmount.
    ///
    /// Also removes the feedback layer directly, if one is still showing:
    /// `recognizer.dispose()`'s `cancel()` calls schedule a rebuild
    /// (`DragSession::end_active`), but this element is unmounting — no
    /// later `build` will ever run to act on it (see `build`'s own teardown
    /// check), so this is the last chance.
    ///
    /// The layer goes first. The cancel runs the user's `on_drag_end` and
    /// `on_draggable_canceled` (from `finalize_tree`, outside any build, so
    /// their writes land); a callback that panics there must not leave the
    /// overlay entry behind.
    fn dispose(&mut self) {
        let stale = self.feedback_entry.borrow_mut().take();
        if let Some(entry) = stale {
            entry.remove();
        }
        if let Some(recognizer) = self.recognizer.as_ref() {
            recognizer.dispose();
        }
    }
}
