//! [`Draggable`] — a widget that can be picked up and dragged, carrying typed
//! `data` for a [`DragTarget`](crate::DragTarget) to receive on drop.
//!
//! Flutter parity: `widgets/drag_target.dart` (tag `3.44.0`) — `Draggable`,
//! `_DraggableState`, `DraggableDetails`. `LongPressDraggable` and
//! `DragAnchorStrategy` are named deferrals; see the module docs below.
//!
//! # Deliberate divergences from the oracle (framework-surface gaps)
//!
//! 1. **No feedback overlay.** The oracle's `_DragAvatar` inserts `feedback`
//!    into the nearest ancestor [`Overlay`](crate::overlay) as an
//!    `OverlayEntry` that repositions itself under the pointer every update,
//!    and *requires* an `Overlay` ancestor (`debugCheckHasOverlay`). FLUI's
//!    `Overlay`/`OverlayHandle` are `pub(crate)` with no `Overlay.of(context)`
//!    equivalent — nothing publishes an ancestor overlay for a descendant to
//!    find (`Navigator` constructs and holds its own `OverlayHandle` directly;
//!    there is no `InheritedWidget`-style lookup). Building that lookup is a
//!    separate, sizable feature. `feedback` is accepted and stored (so the
//!    constructor shape is future-compatible) but is **not painted anywhere**
//!    in this cut — a widget-visible, honestly-deferred gap, not a silent one.
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
//!    separate-scope change — the same shape of gap as point 1 above, and one
//!    this port does not invent silently mid-task.
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
//! 4. **No `dragAnchorStrategy`, `affinity`, `hitTestBehavior`,
//!    `ignoringFeedback*`, `rootOverlay`, `allowedButtonsFilter`.** All but
//!    `affinity`/`hitTestBehavior` only affect the feedback overlay (moot per
//!    point 1). `affinity` selects which single-axis recognizer competes for
//!    the *start* of the gesture; `Draggable::axis` (implemented) instead
//!    restricts *reported* movement after the drag has already started,
//!    mirroring the oracle's `_DragAvatar._restrictAxis`. Both are named
//!    deferrals rather than dead fields.

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
use flui_view::prelude::*;
use parking_lot::Mutex;

use crate::{GestureArenaScope, Listener};

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
    /// Global position at release.
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

    /// The widget shown under the pointer during a drag. Stored, but not
    /// painted in this cut — see the module divergence notes.
    #[must_use]
    pub fn feedback(mut self, builder: impl Fn() -> BoxedView + 'static) -> Self {
        self.feedback = Some(Rc::new(builder));
        self
    }

    /// Offset from the drag anchor to where `feedback` would be painted, were
    /// it painted (see the module divergence notes).
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
/// stream is stateful) and is disposed on unmount. Mirrors
/// `GestureDetectorState`'s init_state-acquires-the-arena shape.
pub struct DraggableState<T: Clone + Send + Sync + 'static> {
    /// How many drags this widget currently has active — gates
    /// `max_simultaneous_drags` and switches `child` vs `child_when_dragging`.
    active_count: Arc<AtomicUsize>,
    /// The live config the recognizer's `on_start` closure reads at drag-start
    /// time (data, callbacks, axis, max-drags). Refreshed each `build`.
    config: Arc<Mutex<DragConfig>>,
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
/// with them.
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
/// recognizer for the pointer's lifetime (it must outlive `DraggableState`
/// if the widget unmounts mid-drag — mirroring the oracle's own comment on
/// `_DragAvatar`'s "dubious" lifetime).
struct DragSession {
    active_count: Arc<AtomicUsize>,
    rebuild: RebuildHandle,
    config: Arc<Mutex<DragConfig>>,
}

impl MultiDragHandle for DragSession {
    fn update(&self, details: MultiDragUpdateDetails) {
        let config = self.config.lock();
        if let Some(callback) = &config.on_drag_update {
            let delta = restrict_axis_delta(details.delta, config.axis);
            // Flutter's `primaryDelta` is only defined for a single-axis
            // recognizer; this drag's own recognizer is always `Free` (axis
            // restriction is applied here, post-hoc), so it is 0.0 without a
            // configured axis.
            let primary_delta = match config.axis {
                Some(Axis::Horizontal) => delta.dx.0,
                Some(Axis::Vertical) => delta.dy.0,
                None => 0.0,
            };
            callback(DragUpdateDetails {
                global_position: details.global_position,
                local_position: details.local_position,
                delta,
                primary_delta,
                kind: details.kind,
            });
        }
    }

    fn end(&self, details: MultiDragEndDetails) {
        self.active_count.fetch_sub(1, Ordering::AcqRel);
        self.rebuild.schedule();

        let config = self.config.lock();
        // No live target discovery (see module docs): every drag ends
        // uncaptured.
        let was_accepted = false;
        let velocity = Velocity {
            pixels_per_second: restrict_axis(details.velocity.pixels_per_second, config.axis),
        };
        if let Some(callback) = &config.on_drag_end {
            callback(DraggableDetails {
                was_accepted,
                velocity,
                offset: details.global_position,
            });
        }
        if let Some(callback) = &config.on_draggable_canceled {
            callback(velocity, details.global_position);
        }
    }

    fn cancel(&self) {
        self.active_count.fetch_sub(1, Ordering::AcqRel);
        self.rebuild.schedule();

        // Flutter's `_DragAvatar.cancel` also routes through `finishDrag`,
        // which fires `onDragEnd` unconditionally (zero velocity, not
        // accepted) before `onDraggableCanceled` — not a cancel-only path.
        let config = self.config.lock();
        if let Some(callback) = &config.on_drag_end {
            callback(DraggableDetails {
                was_accepted: false,
                velocity: Velocity::ZERO,
                offset: Offset::ZERO,
            });
        }
        if let Some(callback) = &config.on_draggable_canceled {
            callback(Velocity::ZERO, Offset::ZERO);
        }
    }
}

impl<T: Clone + Send + Sync + 'static> StatefulView for Draggable<T> {
    type State = DraggableState<T>;

    fn create_state(&self) -> Self::State {
        DraggableState {
            active_count: Arc::new(AtomicUsize::new(0)),
            config: Arc::new(Mutex::new(DragConfig::from_view(self))),
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

        let active_count = Arc::clone(&self.active_count);
        let config = Arc::clone(&self.config);
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
            Some(Box::new(DragSession {
                active_count: Arc::clone(&active_count),
                rebuild: rebuild.clone(),
                config: Arc::clone(&config),
            }) as Box<dyn MultiDragHandle>) // PORT-CHECK-OK-DYN: see flui-interaction's MultiDragStartCallback — the per-pointer handle `MultiDragGestureRecognizer::with_on_start` requires.
        });

        self.recognizer = Some(
            MultiDragGestureRecognizer::new(arena, MultiDragAxis::Free).with_on_start(on_start),
        );
    }

    fn build(&self, view: &Draggable<T>, _ctx: &dyn BuildContext) -> impl IntoView {
        *self.config.lock() = DragConfig::from_view(view);

        let recognizer = self
            .recognizer
            .clone()
            .expect("init_state builds the recognizer before the first build");
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

        let showing_child_when_dragging =
            self.active_count.load(Ordering::Acquire) > 0 && view.child_when_dragging.is_some();

        if showing_child_when_dragging {
            let builder = view
                .child_when_dragging
                .clone()
                .expect("checked is_some above");
            listener.child(builder())
        } else {
            match view.child.clone().into_inner() {
                Some(child) => listener.child(child),
                None => listener,
            }
        }
    }

    fn dispose(&mut self) {
        if let Some(recognizer) = self.recognizer.as_ref() {
            recognizer.dispose();
        }
    }
}
