//! Composite tap-and-drag gesture recogniser.
//!
//! Flutter parity: `gestures/tap_and_drag.dart` `BaseTapAndDragGestureRecognizer`.
//! The recogniser arbitrates between two gesture outcomes for a single
//! primary pointer:
//!
//! - **Tap**: pointer up before crossing drag slop → fire
//!   `on_tap_down` / `on_tap_up`.
//! - **Drag**: pointer crosses drag slop before up → fire
//!   `on_drag_start` / `on_drag_update` / `on_drag_end`.
//!
//! The recogniser does *not* eagerly decide in `handle_event`; instead it
//! lets the gesture arena resolve between competing recognisers. The
//! [`TapAndDragGestureRecognizer`] is a `OneSequenceGestureRecognizer`
//! subclass that tracks a single primary pointer, captures a tap-down
//! details payload, and — once accepted by the arena — either resolves
//! as a tap (on pointer up) or a drag (on slop crossing + drag
//! lifecycle).
//!
//! # When to use
//!
//! Use this recogniser when a single widget should react to *both* a
//! quick tap and a drag. Examples include text-selection handles
//! (Flutter's canonical use), draggable list items with tap-to-select
//! semantics, and map pins (tap to inspect, drag to reposition).
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::recognizers::tap_and_drag::TapAndDragGestureRecognizer;
//!
//! let arena = GestureArena::new();
//! let recogniser = TapAndDragGestureRecognizer::new(arena)
//!     .with_on_tap_down(|d| { let _ = d; })
//!     .with_on_drag_start(|d| { let _ = d; })
//!     .with_on_drag_update(|d| { let _ = d; })
//!     .with_on_drag_end(|d| { let _ = d; })
//!     .with_on_tap_up(|d| { let _ = d; });
//! ```

use std::{cell::RefCell, rc::Rc, sync::Arc};

use flui_foundation::geometry::Offset;
use parking_lot::Mutex;

use super::recognizer::{GestureRecognizer, RecognizerBase};
use crate::{
    arena::GestureArenaMember,
    events::{PointerEvent, PointerType},
    ids::PointerId,
    processing::{Velocity, VelocityTracker},
    routing::PointerDispatch,
    settings::GestureSettings,
    traits::PointerEventExtTrait,
};

// ============================================================================
// Details types
// ============================================================================

/// Position+kind+consecutive-tap-count details for tap-down.
#[derive(Debug, Clone, PartialEq)]
pub struct TapDragDownDetails {
    /// Global position where pointer contacted the screen.
    pub global_position: Offset<f64>,
    /// Local position (relative to widget).
    pub local_position: Offset<f64>,
    /// Pointer device kind.
    pub kind: PointerType,
}

/// Position+kind details for tap-up.
#[derive(Debug, Clone, PartialEq)]
pub struct TapDragUpDetails {
    /// Global position where pointer was released.
    pub global_position: Offset<f64>,
    /// Local position (relative to widget).
    pub local_position: Offset<f64>,
    /// Pointer device kind.
    pub kind: PointerType,
}

/// Details for drag-start.
#[derive(Debug, Clone)]
pub struct TapDragStartDetails {
    /// Global position where the drag started (down position).
    pub global_position: Offset<f64>,
    /// Local position.
    pub local_position: Offset<f64>,
    /// Pointer device kind.
    pub kind: PointerType,
}

/// Details for drag-update.
#[derive(Debug, Clone, PartialEq)]
pub struct TapDragUpdateDetails {
    /// Current global position.
    pub global_position: Offset<f64>,
    /// Current local position.
    pub local_position: Offset<f64>,
    /// Delta since the previous update.
    pub delta: Offset<f64>,
    /// Pointer device kind.
    pub kind: PointerType,
}

/// Details for drag-end.
#[derive(Debug, Clone, PartialEq)]
pub struct TapDragEndDetails {
    /// Velocity at the end of the drag.
    pub velocity: Velocity,
    /// Final global position.
    pub global_position: Offset<f64>,
    /// Final local position.
    pub local_position: Offset<f64>,
}

// ============================================================================
// Callbacks
// ============================================================================

/// Callback fired when the primary pointer contacts the screen.
pub type TapDragDownCallback = Rc<dyn Fn(TapDragDownDetails)>;
/// Callback fired when the pointer lifts before crossing drag slop (a tap).
pub type TapDragUpCallback = Rc<dyn Fn(TapDragUpDetails)>;
/// Callback fired when the pointer crosses drag slop and the drag begins.
pub type TapDragStartCallback = Rc<dyn Fn(TapDragStartDetails)>;
/// Callback fired for each pointer move while the drag is in progress.
pub type TapDragUpdateCallback = Rc<dyn Fn(TapDragUpdateDetails)>;
/// Callback fired when the pointer lifts and the drag completes.
pub type TapDragEndCallback = Rc<dyn Fn(TapDragEndDetails)>;
/// Callback fired when the sequence is cancelled (arena loss or pointer
/// cancel) — neither the tap nor the drag outcome will fire.
pub type TapDragCancelCallback = Rc<dyn Fn()>;

// ============================================================================
// Recogniser
// ============================================================================

/// Internal FSM phase. Tracks whether the primary pointer is currently
/// held, whether a drag has been accepted, and whether a tap is still
/// viable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// No pointer in flight.
    Ready,
    /// Pointer down, slop not yet crossed, tap still viable.
    Down,
    /// Slop crossed; drag is in progress and the tap outcome is void.
    Dragging,
    /// Sequence complete; awaiting reset.
    Finished,
}

// Field names keep Flutter's `onTapDown`/`onDragStart`-style callback names
// (parity with `BaseTapAndDragGestureRecognizer`).
#[expect(clippy::struct_field_names)]
#[derive(Default)]
struct TapDragCallbacks {
    on_tap_down: Option<TapDragDownCallback>,
    on_tap_up: Option<TapDragUpCallback>,
    on_drag_start: Option<TapDragStartCallback>,
    on_drag_update: Option<TapDragUpdateCallback>,
    on_drag_end: Option<TapDragEndCallback>,
    on_cancel: Option<TapDragCancelCallback>,
}

#[derive(Debug, Clone)]
struct DragState {
    /// Initial position at down.
    initial: Option<Offset<f64>>,
    /// The same contact as `initial`, in the root's space — stored because
    /// dispatch localises the event before this recognizer sees it, so the
    /// global position exists only on arrival (issue #908).
    initial_global: Option<Offset<f64>>,
    /// Last update position.
    last: Option<Offset<f64>>,
    /// The same contact as `last`, in the root's space.
    last_global: Option<Offset<f64>>,
    /// Velocity tracker for end-of-drag velocity.
    velocity_tracker: VelocityTracker,
    /// `true` while a tap outcome is still possible. Set `false` once the
    /// pointer wanders past tap slop (but not yet drag slop) — Flutter parity:
    /// such a move voids the tap so a later up fires nothing.
    tap_viable: bool,
}

impl Default for DragState {
    fn default() -> Self {
        Self {
            initial: None,
            initial_global: None,
            last: None,
            last_global: None,
            velocity_tracker: VelocityTracker::new(),
            tap_viable: true,
        }
    }
}

/// Composite tap-and-drag recogniser.
///
/// See [module-level docs](self) for the full design.
#[derive(Clone)]
pub struct TapAndDragGestureRecognizer {
    state: RecognizerBase,
    phase: Arc<Mutex<Phase>>,
    drag_state: Arc<Mutex<DragState>>,
    callbacks: Rc<RefCell<TapDragCallbacks>>,
    settings: Arc<Mutex<GestureSettings>>,
    /// Arena verdict for the in-flight sequence: `None` until resolved,
    /// `Some(true)` once this recogniser wins, `Some(false)` once it loses.
    /// Tap callbacks fire only on `Some(true)` so a losing tap-and-drag never
    /// emits `on_tap_*` to user code (a competing recogniser added earlier can
    /// take the pointer on the resolving sweep).
    accepted: Arc<Mutex<Option<bool>>>,
}

impl std::fmt::Debug for TapAndDragGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TapAndDragGestureRecognizer")
            .field("state", &self.state)
            .field("phase", &*self.phase.lock())
            .field("drag_state", &*self.drag_state.lock())
            .field("settings", &*self.settings.lock())
            .finish_non_exhaustive()
    }
}

impl TapAndDragGestureRecognizer {
    /// Create a new tap-and-drag recogniser.
    pub fn new(arena: crate::arena::GestureArena) -> Arc<Self> {
        Arc::new(Self {
            state: RecognizerBase::new(arena),
            phase: Arc::new(Mutex::new(Phase::Ready)),
            drag_state: Arc::new(Mutex::new(DragState::default())),
            callbacks: Rc::new(RefCell::new(TapDragCallbacks::default())),
            settings: Arc::new(Mutex::new(GestureSettings::default())),
            accepted: Arc::new(Mutex::new(None)),
        })
    }

    /// Create with custom gesture settings.
    pub fn with_settings(
        arena: crate::arena::GestureArena,
        settings: GestureSettings,
    ) -> Arc<Self> {
        Arc::new(Self {
            state: RecognizerBase::new(arena),
            phase: Arc::new(Mutex::new(Phase::Ready)),
            drag_state: Arc::new(Mutex::new(DragState::default())),
            callbacks: Rc::new(RefCell::new(TapDragCallbacks::default())),
            settings: Arc::new(Mutex::new(settings)),
            accepted: Arc::new(Mutex::new(None)),
        })
    }

    /// Get the current settings.
    pub fn settings(&self) -> GestureSettings {
        self.settings.lock().clone()
    }

    /// Update settings.
    pub fn set_settings(&self, settings: GestureSettings) {
        *self.settings.lock() = settings;
    }

    /// Drag slop threshold for `kind` (uses
    /// [`GestureSettings::pan_slop_for`]).
    ///
    /// This recognizer drags in a free plane, so it takes the *pan* tier —
    /// `computePanSlop`, as `TapAndPanGestureRecognizer` does
    /// (`tap_and_drag.dart:1445`). The axis-locked variants take the plain hit
    /// tier instead (`:1410`); if this recognizer ever grows a vertical- or
    /// horizontal-only mode, that mode reads [`Self::tap_slop`]'s tier, not
    /// this one.
    fn drag_slop(&self, kind: PointerType) -> f64 {
        self.settings.lock().pan_slop_for(kind)
    }

    /// Tap slop threshold for `kind` (uses [`GestureSettings::hit_slop`]).
    ///
    /// The tap-viability check is a *hit* test, not a pan one, so it takes the
    /// plain tier — `computeHitSlop` at `tap_and_drag.dart:532`.
    fn tap_slop(&self, kind: PointerType) -> f64 {
        self.settings.lock().hit_slop(kind)
    }

    // ========================================================================
    // Builder-style callback setters
    // ========================================================================

    /// Register the tap-down callback (fires on pointer contact, once the
    /// arena has accepted this recogniser).
    pub fn with_on_tap_down(
        self: Arc<Self>,
        cb: impl Fn(TapDragDownDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_tap_down = Some(Rc::new(cb));
        self
    }

    /// Register the tap-up callback (fires when the pointer lifts before
    /// crossing drag slop, resolving the sequence as a tap).
    pub fn with_on_tap_up(self: Arc<Self>, cb: impl Fn(TapDragUpDetails) + 'static) -> Arc<Self> {
        self.callbacks.borrow_mut().on_tap_up = Some(Rc::new(cb));
        self
    }

    /// Register the drag-start callback (fires when the pointer crosses drag
    /// slop, voiding the tap outcome).
    pub fn with_on_drag_start(
        self: Arc<Self>,
        cb: impl Fn(TapDragStartDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_drag_start = Some(Rc::new(cb));
        self
    }

    /// Register the drag-update callback (fires for each pointer move while
    /// the drag is in progress).
    pub fn with_on_drag_update(
        self: Arc<Self>,
        cb: impl Fn(TapDragUpdateDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_drag_update = Some(Rc::new(cb));
        self
    }

    /// Register the drag-end callback (fires when the pointer lifts after a
    /// drag, with end-of-drag velocity).
    pub fn with_on_drag_end(
        self: Arc<Self>,
        cb: impl Fn(TapDragEndDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_drag_end = Some(Rc::new(cb));
        self
    }

    /// Register the cancel callback (fires when the sequence is cancelled by
    /// an arena loss or a pointer-cancel event).
    pub fn with_on_cancel(self: Arc<Self>, cb: impl Fn() + 'static) -> Arc<Self> {
        self.callbacks.borrow_mut().on_cancel = Some(Rc::new(cb));
        self
    }

    // ========================================================================
    // Internal helpers
    // ========================================================================

    /// Reset FSM and per-gesture tracking state to Ready. Called after
    /// tap-up, drag-end, or cancel.
    fn reset(&self) {
        *self.phase.lock() = Phase::Ready;
        *self.accepted.lock() = None;
        let mut ds = self.drag_state.lock();
        ds.initial = None;
        ds.initial_global = None;
        ds.last = None;
        ds.tap_viable = true;
        ds.velocity_tracker.reset();
    }

    /// Distance from initial position to `current` (or 0 if no initial).
    fn distance_from_initial(&self, current: Offset<f64>) -> f64 {
        let initial = self.drag_state.lock().initial;
        match initial {
            Some(initial) => (current - initial).distance(),
            None => 0.0,
        }
    }
}

impl GestureRecognizer for TapAndDragGestureRecognizer {
    fn add_pointer(
        self: &Arc<Self>,
        pointer: PointerId,
        position: Offset<f64>,
        global_position: Offset<f64>,
    ) {
        // per-impl span (trait fn disallows `#[instrument]`).
        let _span = tracing::info_span!(
            "tap_and_drag.add_pointer",
            pointer = ?pointer,
            event = %crate::observability::GestureEvent::RecognizerAdded,
        );
        if !self.state.assert_not_disposed("add_pointer") {
            return;
        }
        self.state
            .start_tracking(pointer, position, global_position, self);

        // Initialise drag state for the new pointer.
        {
            let mut ds = self.drag_state.lock();
            ds.initial = Some(position);
            ds.initial_global = Some(global_position);
            ds.last = Some(position);
            ds.tap_viable = true;
            ds.velocity_tracker.reset();
            // Read the arena's clock, not the OS clock: production binds it to
            // `SystemClock` (identical there), but a headless frame driver binds a
            // `ManualClock`, so a replayed gesture's own sample spacing decides the
            // velocity instead of however the test process happened to be scheduled.
            ds.velocity_tracker.add_position(self.state.now(), position);
        }
        *self.phase.lock() = Phase::Down;
    }

    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        let event = dispatch.local;
        // per-impl span (trait fn disallows `#[instrument]`).
        let _span = tracing::info_span!(
            "tap_and_drag.handle_event",
            kind = %crate::observability::pointer_event_kind(event),
            event = %crate::observability::GestureEvent::EventReceived,
        );
        if !self.state.assert_not_disposed("handle_event") {
            return;
        }
        let Some(primary) = self.state.primary_pointer() else {
            return;
        };
        // Filter to the primary pointer we are tracking.
        if event.pointer_id() != primary {
            return;
        }
        // Read once, here: this is the only point at which the untransformed
        // position is available at all (issue #908).
        let global_position = dispatch.global.position();

        match event {
            PointerEvent::Move(data) => {
                let pos = data.current.position;
                let position = Offset::new(pos.x, pos.y);
                let kind = data.pointer.pointer_type;
                self.handle_move(position, global_position, kind);
            }
            PointerEvent::Up(data) => {
                let pos = data.state.position;
                let position = Offset::new(pos.x, pos.y);
                self.handle_up(position, global_position, data.pointer.pointer_type);
            }
            PointerEvent::Cancel(info) => {
                if let Some(pos) = self.state.initial_position() {
                    self.handle_cancel(Some(pos), Some(global_position), info.pointer_type);
                } else {
                    self.handle_cancel(None, None, info.pointer_type);
                }
            }
            _ => {}
        }
    }

    fn dispose(&self) {
        self.state.mark_disposed();
        self.state.reject();
        let mut cbs = self.callbacks.borrow_mut();
        cbs.on_tap_down = None;
        cbs.on_tap_up = None;
        cbs.on_drag_start = None;
        cbs.on_drag_update = None;
        cbs.on_drag_end = None;
        cbs.on_cancel = None;
    }

    fn primary_pointer(&self) -> Option<PointerId> {
        self.state.primary_pointer()
    }
}

impl TapAndDragGestureRecognizer {
    fn handle_move(&self, position: Offset<f64>, global_position: Offset<f64>, kind: PointerType) {
        let phase = *self.phase.lock();
        match phase {
            Phase::Down => {
                let distance = self.distance_from_initial(position);
                if distance > self.drag_slop(kind) {
                    // Slop crossed: lock in the drag outcome. Fire
                    // `on_tap_down` (we did get a down) then promote
                    // to drag, fire `on_drag_start` with the down
                    // position. Then immediately fire `on_drag_update`
                    // so observers see the crossing move.
                    //
                    // Defensive: if `drag_state.initial` is `None` (e.g. a
                    // future refactor breaks the `add_pointer` invariant),
                    // warn and bail rather than panicking in a gesture
                    // hot path. The recogniser will simply not promote
                    // this move to a drag — the next move can retry.
                    let (initial_opt, initial_global_opt) = {
                        let ds = self.drag_state.lock();
                        (ds.initial, ds.initial_global)
                    };
                    let Some(initial) = initial_opt else {
                        tracing::warn!(
                            target: "crate::tap_and_drag",
                            "drag_state.initial unset in handle_move; \
                             add_pointer must be called before any move event"
                        );
                        return;
                    };

                    // Snapshot callbacks under lock, fire outside.
                    let down_cb = self.callbacks.borrow().on_tap_down.clone();
                    if let Some(cb) = down_cb {
                        cb(TapDragDownDetails {
                            global_position: initial_global_opt.unwrap_or(initial),
                            local_position: initial,
                            kind,
                        });
                    }

                    *self.phase.lock() = Phase::Dragging;
                    {
                        let mut ds = self.drag_state.lock();
                        ds.last = Some(position);
                        ds.last_global = Some(global_position);
                        ds.velocity_tracker.reset();
                        ds.velocity_tracker.add_position(self.state.now(), position);
                    }

                    let start_cb = self.callbacks.borrow().on_drag_start.clone();
                    if let Some(cb) = start_cb {
                        cb(TapDragStartDetails {
                            global_position: initial_global_opt.unwrap_or(initial),
                            local_position: initial,
                            kind,
                        });
                    }

                    // Fire an update with the crossing move.
                    let delta = (position - initial).to_delta();
                    let update_cb = self.callbacks.borrow().on_drag_update.clone();
                    if let Some(cb) = update_cb {
                        cb(TapDragUpdateDetails {
                            global_position,
                            local_position: position,
                            delta,
                            kind,
                        });
                    }
                } else if distance > self.tap_slop(kind) {
                    // Past tap slop but not drag slop: the pointer wandered too
                    // far to still count as a tap (Flutter parity). Void the tap
                    // so a later up does not fire `on_tap_*`.
                    self.drag_state.lock().tap_viable = false;
                }
                // Always update last so subsequent distance checks are
                // relative to the most recent move.
                self.drag_state.lock().last = Some(position);
            }
            Phase::Dragging => {
                // Compute delta from last position and update.
                let last = self.drag_state.lock().last;
                let delta = match last {
                    Some(last_pos) => (position - last_pos).to_delta(),
                    None => Offset::new(0.0, 0.0),
                };
                {
                    let mut ds = self.drag_state.lock();
                    ds.last = Some(position);
                    ds.velocity_tracker.add_position(self.state.now(), position);
                }
                let cb = self.callbacks.borrow().on_drag_update.clone();
                if let Some(cb) = cb {
                    cb(TapDragUpdateDetails {
                        global_position,
                        local_position: position,
                        delta,
                        kind,
                    });
                }
            }
            _ => {}
        }
    }

    fn handle_up(&self, position: Offset<f64>, global_position: Offset<f64>, kind: PointerType) {
        let phase = *self.phase.lock();
        match phase {
            Phase::Down => {
                let (initial, initial_global, tap_viable) = {
                    let ds = self.drag_state.lock();
                    (ds.initial, ds.initial_global, ds.tap_viable)
                };
                *self.phase.lock() = Phase::Finished;

                // Resolve the arena BEFORE firing any tap callback. `stop_tracking`
                // synchronously sweeps and dispatches `accept_gesture` /
                // `reject_gesture`, which records the verdict in `self.accepted`.
                // A tap-and-drag competing with an earlier-added recogniser can
                // lose this sweep, so firing before resolution would let a loser
                // emit `on_tap_*` (mirrors the `TapGestureRecognizer` pending-up
                // pattern).
                self.state.stop_tracking();

                // Fire only if the tap stayed viable (no move past tap slop, see
                // `handle_move`) AND the arena confirmed our win.
                if tap_viable && self.accepted.lock().unwrap_or(false) {
                    if let Some(initial) = initial {
                        let down_cb = self.callbacks.borrow().on_tap_down.clone();
                        if let Some(cb) = down_cb {
                            cb(TapDragDownDetails {
                                global_position: initial_global.unwrap_or(initial),
                                local_position: initial,
                                kind,
                            });
                        }
                    }
                    let up_cb = self.callbacks.borrow().on_tap_up.clone();
                    if let Some(cb) = up_cb {
                        cb(TapDragUpDetails {
                            global_position,
                            local_position: position,
                            kind,
                        });
                    }
                }
                self.reset();
            }
            Phase::Dragging => {
                // Drag ended at up: fire on_drag_end with final velocity.
                let velocity = self.drag_state.lock().velocity_tracker.get_velocity();
                let end_cb = self.callbacks.borrow().on_drag_end.clone();
                if let Some(cb) = end_cb {
                    cb(TapDragEndDetails {
                        velocity,
                        global_position,
                        local_position: position,
                    });
                }
                *self.phase.lock() = Phase::Finished;
                self.state.stop_tracking();
                self.reset();
            }
            _ => {}
        }
    }

    fn handle_cancel(
        &self,
        position: Option<Offset<f64>>,
        global_position: Option<Offset<f64>>,
        _kind: PointerType,
    ) {
        let phase = *self.phase.lock();
        if phase == Phase::Ready || phase == Phase::Finished {
            return;
        }
        // We were mid-gesture. Withdraw and reset before invoking user code.
        let cb = self.callbacks.borrow().on_cancel.clone();
        // Cancel carries no details, in either space, so both positions are
        // accepted and dropped rather than being made to look meaningful.
        let _ = (position, global_position);
        *self.phase.lock() = Phase::Finished;
        self.state.reject();
        self.reset();
        if let Some(cb) = cb {
            cb();
        }
    }
}

impl crate::recognizers::OneSequenceGestureRecognizer for TapAndDragGestureRecognizer {
    fn tracked_pointers(&self) -> Vec<PointerId> {
        self.state
            .primary_pointer()
            .map(|p| vec![p])
            .unwrap_or_default()
    }

    fn resolve_pointer(&self, _pointer: PointerId, disposition: crate::arena::GestureDisposition) {
        match disposition {
            crate::arena::GestureDisposition::Accepted => {
                // Record the win; `handle_up` reads `self.accepted` after the
                // resolving sweep and fires the deferred tap callbacks only
                // then. Firing here is a lock-during-callback hazard.
                *self.accepted.lock() = Some(true);
            }
            crate::arena::GestureDisposition::Rejected => {
                // Record the loss so the deferred tap callbacks never fire.
                // Reentrancy guard: don't call `self.state.reject()` from
                // here. The arena is already inside a synchronous
                // `entry.lock()` while dispatching to us; calling
                // `arena.resolve` again would re-lock the same entry and
                // deadlock. The handle_* paths (handle_cancel, dispose)
                // own the actual `state.reject()` call.
                *self.accepted.lock() = Some(false);
                *self.phase.lock() = Phase::Ready;
            }
        }
    }

    fn stop_tracking_pointer(&self, _pointer: PointerId) {
        self.state.stop_tracking();
    }
}

impl GestureArenaMember for TapAndDragGestureRecognizer {
    fn accept_gesture(&self, _pointer: PointerId) {
        // Record the arena win; `handle_up` reads this after the resolving
        // sweep and fires the deferred tap callbacks. Do NOT invoke user
        // callbacks here — the arena holds its entry lock while dispatching
        // and user code may re-enter it (lock-during-callback hazard).
        *self.accepted.lock() = Some(true);
    }

    fn reject_gesture(&self, _pointer: PointerId) {
        // Record the loss so the deferred tap callbacks never fire.
        *self.accepted.lock() = Some(false);

        // The arena is already holding its entry-lock while dispatching
        // `reject_gesture`; calling `self.state.reject()` here would
        // re-enter `arena.resolve` on the same pointer and try to take
        // the entry-lock again, deadlocking the single-threaded test
        // harness (and any other consumer that resolves synchronously).
        //
        // Clean up recogniser-owned state directly without touching the
        // arena so the next add_pointer cycle starts fresh.
        *self.phase.lock() = Phase::Ready;
        let mut ds = self.drag_state.lock();
        ds.initial = None;
        ds.initial_global = None;
        ds.last = None;
        ds.velocity_tracker.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        arena::GestureArena,
        events::{make_move_event, make_up_event},
    };

    #[test]
    fn down_then_move_past_drag_slop_fires_drag() {
        let arena = GestureArena::new();
        let tap_down = Arc::new(Mutex::new(false));
        let drag_start = Arc::new(Mutex::new(false));
        let drag_update_count = Arc::new(Mutex::new(0u32));
        let drag_end = Arc::new(Mutex::new(false));

        let rec = TapAndDragGestureRecognizer::new(arena)
            .with_on_tap_down({
                let tap_down = tap_down.clone();
                move |_| *tap_down.lock() = true
            })
            .with_on_drag_start({
                let drag_start = drag_start.clone();
                move |_| *drag_start.lock() = true
            })
            .with_on_drag_update({
                let drag_update_count = drag_update_count.clone();
                move |_| *drag_update_count.lock() += 1
            })
            .with_on_drag_end({
                let drag_end = drag_end.clone();
                move |_| *drag_end.lock() = true
            });

        let pointer = PointerId::PRIMARY;
        let pos = Offset::new(0.0, 0.0);
        rec.add_pointer(pointer, pos, pos);

        // Big move (40px) — past the default 18px drag slop.
        let big_pos = Offset::new(40.0, 0.0);
        rec.handle_event(PointerDispatch::at_root(&make_move_event(
            big_pos,
            PointerType::Touch,
        )));

        // Drag started on slop crossing.
        assert!(*drag_start.lock(), "drag_start fires when slop crossed");
        assert!(
            *tap_down.lock(),
            "tap_down fires once at the slop-crossing point"
        );

        // One more move.
        rec.handle_event(PointerDispatch::at_root(&make_move_event(
            Offset::new(60.0, 0.0),
            PointerType::Touch,
        )));

        // We expect 2 updates: one from the slop-crossing event itself,
        // one from the follow-up move.
        assert_eq!(*drag_update_count.lock(), 2, "two drag updates expected");

        // Up — drag ends.
        rec.handle_event(PointerDispatch::at_root(&make_up_event(
            Offset::new(60.0, 0.0),
            PointerType::Touch,
        )));
        assert!(*drag_end.lock(), "drag_end fires on pointer up");
    }

    // Sanity: constructor builder pattern.
}
