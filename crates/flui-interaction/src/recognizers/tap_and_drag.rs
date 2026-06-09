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

use std::sync::Arc;

use flui_types::{
    Offset,
    geometry::{PixelDelta, Pixels},
};
use parking_lot::Mutex;

use super::recognizer::{GestureRecognizer, RecognizerBase};
use crate::{
    arena::GestureArenaMember,
    events::{PointerEvent, PointerType},
    ids::PointerId,
    processing::{Velocity, VelocityTracker},
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
    pub global_position: Offset<Pixels>,
    /// Local position (relative to widget).
    pub local_position: Offset<Pixels>,
    /// Pointer device kind.
    pub kind: PointerType,
}

/// Position+kind details for tap-up.
#[derive(Debug, Clone, PartialEq)]
pub struct TapDragUpDetails {
    /// Global position where pointer was released.
    pub global_position: Offset<Pixels>,
    /// Local position (relative to widget).
    pub local_position: Offset<Pixels>,
    /// Pointer device kind.
    pub kind: PointerType,
}

/// Details for drag-start.
#[derive(Debug, Clone)]
pub struct TapDragStartDetails {
    /// Global position where the drag started (down position).
    pub global_position: Offset<Pixels>,
    /// Local position.
    pub local_position: Offset<Pixels>,
    /// Pointer device kind.
    pub kind: PointerType,
}

/// Details for drag-update.
#[derive(Debug, Clone, PartialEq)]
pub struct TapDragUpdateDetails {
    /// Current global position.
    pub global_position: Offset<Pixels>,
    /// Current local position.
    pub local_position: Offset<Pixels>,
    /// Delta since the previous update.
    pub delta: Offset<PixelDelta>,
    /// Pointer device kind.
    pub kind: PointerType,
}

/// Details for drag-end.
#[derive(Debug, Clone, PartialEq)]
pub struct TapDragEndDetails {
    /// Velocity at the end of the drag.
    pub velocity: Velocity,
    /// Final global position.
    pub global_position: Offset<Pixels>,
    /// Final local position.
    pub local_position: Offset<Pixels>,
}

// ============================================================================
// Callbacks
// ============================================================================

pub type TapDragDownCallback = Arc<dyn Fn(TapDragDownDetails) + Send + Sync>;
pub type TapDragUpCallback = Arc<dyn Fn(TapDragUpDetails) + Send + Sync>;
pub type TapDragStartCallback = Arc<dyn Fn(TapDragStartDetails) + Send + Sync>;
pub type TapDragUpdateCallback = Arc<dyn Fn(TapDragUpdateDetails) + Send + Sync>;
pub type TapDragEndCallback = Arc<dyn Fn(TapDragEndDetails) + Send + Sync>;
pub type TapDragCancelCallback = Arc<dyn Fn() + Send + Sync>;

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
    initial: Option<Offset<Pixels>>,
    /// Last update position.
    last: Option<Offset<Pixels>>,
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
            last: None,
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
    callbacks: Arc<Mutex<TapDragCallbacks>>,
    settings: Arc<Mutex<GestureSettings>>,
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
            callbacks: Arc::new(Mutex::new(TapDragCallbacks::default())),
            settings: Arc::new(Mutex::new(GestureSettings::default())),
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
            callbacks: Arc::new(Mutex::new(TapDragCallbacks::default())),
            settings: Arc::new(Mutex::new(settings)),
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

    /// Drag slop threshold (uses [`GestureSettings::pan_slop`]).
    fn drag_slop(&self) -> f32 {
        self.settings.lock().pan_slop()
    }

    /// Tap slop threshold (uses [`GestureSettings::touch_slop`]).
    fn tap_slop(&self) -> f32 {
        self.settings.lock().touch_slop()
    }

    // ========================================================================
    // Builder-style callback setters
    // ========================================================================

    pub fn with_on_tap_down(
        self: Arc<Self>,
        cb: impl Fn(TapDragDownDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_tap_down = Some(Arc::new(cb));
        self
    }

    pub fn with_on_tap_up(
        self: Arc<Self>,
        cb: impl Fn(TapDragUpDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_tap_up = Some(Arc::new(cb));
        self
    }

    pub fn with_on_drag_start(
        self: Arc<Self>,
        cb: impl Fn(TapDragStartDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_drag_start = Some(Arc::new(cb));
        self
    }

    pub fn with_on_drag_update(
        self: Arc<Self>,
        cb: impl Fn(TapDragUpdateDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_drag_update = Some(Arc::new(cb));
        self
    }

    pub fn with_on_drag_end(
        self: Arc<Self>,
        cb: impl Fn(TapDragEndDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_drag_end = Some(Arc::new(cb));
        self
    }

    pub fn with_on_cancel(self: Arc<Self>, cb: impl Fn() + Send + Sync + 'static) -> Arc<Self> {
        self.callbacks.lock().on_cancel = Some(Arc::new(cb));
        self
    }

    // ========================================================================
    // Internal helpers
    // ========================================================================

    /// Reset FSM and per-gesture tracking state to Ready. Called after
    /// tap-up, drag-end, or cancel.
    fn reset(&self) {
        *self.phase.lock() = Phase::Ready;
        let mut ds = self.drag_state.lock();
        ds.initial = None;
        ds.last = None;
        ds.tap_viable = true;
        ds.velocity_tracker.reset();
    }

    /// Distance from initial position to `current` (or 0 if no initial).
    fn distance_from_initial(&self, current: Offset<Pixels>) -> f32 {
        match self.drag_state.lock().initial {
            Some(initial) => (current - initial).distance().0,
            None => 0.0,
        }
    }
}

impl GestureRecognizer for TapAndDragGestureRecognizer {
    fn add_pointer(&self, pointer: PointerId, position: Offset<Pixels>) {
        // per-impl span (trait fn disallows `#[instrument]`).
        let _span = tracing::info_span!(
            "tap_and_drag.add_pointer",
            pointer = ?pointer,
            event = %crate::observability::GestureEvent::RecognizerAdded,
        );
        if !self.state.assert_not_disposed("add_pointer") {
            return;
        }
        let recogniser = Arc::new(self.clone());
        self.state.start_tracking(pointer, position, &recogniser);

        // Initialise drag state for the new pointer.
        {
            let mut ds = self.drag_state.lock();
            ds.initial = Some(position);
            ds.last = Some(position);
            ds.tap_viable = true;
            ds.velocity_tracker.reset();
            ds.velocity_tracker
                .add_position(std::time::Instant::now(), position);
        }
        *self.phase.lock() = Phase::Down;
    }

    fn handle_event(&self, event: &PointerEvent) {
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

        match event {
            PointerEvent::Move(data) => {
                let pos = data.current.position;
                let position = Offset::new(Pixels(pos.x as f32), Pixels(pos.y as f32));
                let kind = data.pointer.pointer_type;
                self.handle_move(position, kind);
            }
            PointerEvent::Up(data) => {
                let pos = data.state.position;
                let position = Offset::new(Pixels(pos.x as f32), Pixels(pos.y as f32));
                self.handle_up(position, data.pointer.pointer_type);
            }
            PointerEvent::Cancel(info) => {
                if let Some(pos) = self.state.initial_position() {
                    self.handle_cancel(Some(pos), info.pointer_type);
                } else {
                    self.handle_cancel(None, info.pointer_type);
                }
            }
            _ => {}
        }
    }

    fn dispose(&self) {
        self.state.mark_disposed();
        self.state.reject();
        let mut cbs = self.callbacks.lock();
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
    fn handle_move(&self, position: Offset<Pixels>, kind: PointerType) {
        let phase = *self.phase.lock();
        match phase {
            Phase::Down => {
                let distance = self.distance_from_initial(position);
                if distance > self.drag_slop() {
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
                    let Some(initial) = self.drag_state.lock().initial else {
                        tracing::warn!(
                            target: "flui_interaction::tap_and_drag",
                            "drag_state.initial unset in handle_move; \
                             add_pointer must be called before any move event"
                        );
                        return;
                    };

                    // Snapshot callbacks under lock, fire outside.
                    let down_cb = self.callbacks.lock().on_tap_down.clone();
                    if let Some(cb) = down_cb {
                        cb(TapDragDownDetails {
                            global_position: initial,
                            local_position: initial,
                            kind,
                        });
                    }

                    *self.phase.lock() = Phase::Dragging;
                    {
                        let mut ds = self.drag_state.lock();
                        ds.last = Some(position);
                        ds.velocity_tracker.reset();
                        ds.velocity_tracker
                            .add_position(std::time::Instant::now(), position);
                    }

                    let start_cb = self.callbacks.lock().on_drag_start.clone();
                    if let Some(cb) = start_cb {
                        cb(TapDragStartDetails {
                            global_position: initial,
                            local_position: initial,
                            kind,
                        });
                    }

                    // Fire an update with the crossing move.
                    let delta = (position - initial).to_delta();
                    let update_cb = self.callbacks.lock().on_drag_update.clone();
                    if let Some(cb) = update_cb {
                        cb(TapDragUpdateDetails {
                            global_position: position,
                            local_position: position,
                            delta,
                            kind,
                        });
                    }
                } else if distance > self.tap_slop() {
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
                    None => Offset::new(PixelDelta::ZERO, PixelDelta::ZERO),
                };
                {
                    let mut ds = self.drag_state.lock();
                    ds.last = Some(position);
                    ds.velocity_tracker
                        .add_position(std::time::Instant::now(), position);
                }
                let cb = self.callbacks.lock().on_drag_update.clone();
                if let Some(cb) = cb {
                    cb(TapDragUpdateDetails {
                        global_position: position,
                        local_position: position,
                        delta,
                        kind,
                    });
                }
            }
            _ => {}
        }
    }

    fn handle_up(&self, position: Offset<Pixels>, kind: PointerType) {
        let phase = *self.phase.lock();
        match phase {
            Phase::Down => {
                let (initial, tap_viable) = {
                    let ds = self.drag_state.lock();
                    (ds.initial, ds.tap_viable)
                };
                // Only fire the tap if it is still viable; a move past tap slop
                // voids it (see `handle_move`), so the up resolves to nothing.
                if tap_viable {
                    if let Some(initial) = initial {
                        let down_cb = self.callbacks.lock().on_tap_down.clone();
                        if let Some(cb) = down_cb {
                            cb(TapDragDownDetails {
                                global_position: initial,
                                local_position: initial,
                                kind,
                            });
                        }
                    }
                    let up_cb = self.callbacks.lock().on_tap_up.clone();
                    if let Some(cb) = up_cb {
                        cb(TapDragUpDetails {
                            global_position: position,
                            local_position: position,
                            kind,
                        });
                    }
                }
                *self.phase.lock() = Phase::Finished;
                self.state.stop_tracking();
                self.reset();
            }
            Phase::Dragging => {
                // Drag ended at up: fire on_drag_end with final velocity.
                let velocity = self.drag_state.lock().velocity_tracker.get_velocity();
                let end_cb = self.callbacks.lock().on_drag_end.clone();
                if let Some(cb) = end_cb {
                    cb(TapDragEndDetails {
                        velocity,
                        global_position: position,
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

    fn handle_cancel(&self, position: Option<Offset<Pixels>>, _kind: PointerType) {
        let phase = *self.phase.lock();
        if phase == Phase::Ready || phase == Phase::Finished {
            return;
        }
        // We were mid-gesture; fire cancel and stop tracking.
        let cb = self.callbacks.lock().on_cancel.clone();
        if let Some(cb) = cb {
            cb();
        }
        let _ = position; // Currently unused — cancel details don't carry position.
        *self.phase.lock() = Phase::Finished;
        self.state.reject();
        self.reset();
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
                // No-op: this recogniser fires its own callbacks from
                // handle_move/handle_up based on FSM phase. The arena
                // accept only matters for ordering relative to competing
                // recognisers; tap/down callbacks fire either way.
            }
            crate::arena::GestureDisposition::Rejected => {
                // Reentrancy guard: don't call `self.state.reject()` from
                // here. The arena is already inside a synchronous
                // `entry.lock()` while dispatching to us; calling
                // `arena.resolve` again would re-lock the same entry and
                // deadlock. The handle_* paths (handle_cancel, dispose)
                // own the actual `state.reject()` call.
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
        // No-op: see `resolve_pointer` above.
    }

    fn reject_gesture(&self, _pointer: PointerId) {
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
    fn move_past_tap_slop_voids_the_tap() {
        // With touch_slop < pan_slop, a move past tap slop (but under drag slop)
        // voids the tap: a later up must fire neither on_tap_* nor a drag.
        let arena = GestureArena::new();
        let settings = GestureSettings::default()
            .with_touch_slop(10.0)
            .with_pan_slop(30.0);
        let tap_down = Arc::new(Mutex::new(false));
        let tap_up = Arc::new(Mutex::new(false));
        let drag_start = Arc::new(Mutex::new(false));

        let rec = TapAndDragGestureRecognizer::with_settings(arena, settings)
            .with_on_tap_down({
                let tap_down = tap_down.clone();
                move |_| *tap_down.lock() = true
            })
            .with_on_tap_up({
                let tap_up = tap_up.clone();
                move |_| *tap_up.lock() = true
            })
            .with_on_drag_start({
                let drag_start = drag_start.clone();
                move |_| *drag_start.lock() = true
            });

        let pointer = PointerId::PRIMARY;
        rec.add_pointer(pointer, Offset::new(Pixels(0.0), Pixels(0.0)));

        // Move 20px: past tap slop (10) but under drag slop (30) — voids the tap.
        rec.handle_event(&make_move_event(
            Offset::new(Pixels(20.0), Pixels(0.0)),
            PointerType::Touch,
        ));
        rec.handle_event(&make_up_event(
            Offset::new(Pixels(20.0), Pixels(0.0)),
            PointerType::Touch,
        ));

        assert!(!*tap_down.lock(), "voided tap must not fire on_tap_down");
        assert!(!*tap_up.lock(), "voided tap must not fire on_tap_up");
        assert!(
            !*drag_start.lock(),
            "under drag slop: on_drag_start must not fire"
        );
    }

    #[test]
    fn down_then_up_within_tap_slop_fires_tap() {
        let arena = GestureArena::new();
        let tap_down = Arc::new(Mutex::new(false));
        let tap_up = Arc::new(Mutex::new(false));
        let drag_start = Arc::new(Mutex::new(false));

        let rec = TapAndDragGestureRecognizer::new(arena)
            .with_on_tap_down({
                let tap_down = tap_down.clone();
                move |_| *tap_down.lock() = true
            })
            .with_on_tap_up({
                let tap_up = tap_up.clone();
                move |_| *tap_up.lock() = true
            })
            .with_on_drag_start({
                let drag_start = drag_start.clone();
                move |_| *drag_start.lock() = true
            });

        let pointer = PointerId::PRIMARY;
        let pos = Offset::new(Pixels(50.0), Pixels(50.0));
        rec.add_pointer(pointer, pos);

        // Tiny move (5px) — well under both tap and drag slop.
        rec.handle_event(&make_move_event(
            Offset::new(Pixels(53.0), Pixels(52.0)),
            PointerType::Touch,
        ));

        // Up — tap resolves.
        rec.handle_event(&make_up_event(pos, PointerType::Touch));

        assert!(*tap_down.lock(), "tap_down should fire on tap resolution");
        assert!(*tap_up.lock(), "tap_up should fire on tap resolution");
        assert!(!*drag_start.lock(), "drag_start must NOT fire for a tap");
    }

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
        let pos = Offset::new(Pixels(0.0), Pixels(0.0));
        rec.add_pointer(pointer, pos);

        // Big move (40px) — past the default 18px drag slop.
        let big_pos = Offset::new(Pixels(40.0), Pixels(0.0));
        rec.handle_event(&make_move_event(big_pos, PointerType::Touch));

        // Drag started on slop crossing.
        assert!(*drag_start.lock(), "drag_start fires when slop crossed");
        assert!(
            *tap_down.lock(),
            "tap_down fires once at the slop-crossing point"
        );

        // One more move.
        rec.handle_event(&make_move_event(
            Offset::new(Pixels(60.0), Pixels(0.0)),
            PointerType::Touch,
        ));

        // We expect 2 updates: one from the slop-crossing event itself,
        // one from the follow-up move.
        assert_eq!(*drag_update_count.lock(), 2, "two drag updates expected");

        // Up — drag ends.
        rec.handle_event(&make_up_event(
            Offset::new(Pixels(60.0), Pixels(0.0)),
            PointerType::Touch,
        ));
        assert!(*drag_end.lock(), "drag_end fires on pointer up");
    }

    #[test]
    fn cancel_fires_on_cancel_callback() {
        let arena = GestureArena::new();
        let cancelled = Arc::new(Mutex::new(false));

        let rec = TapAndDragGestureRecognizer::new(arena).with_on_cancel({
            let cancelled = cancelled.clone();
            move || *cancelled.lock() = true
        });

        let pointer = PointerId::PRIMARY;
        rec.add_pointer(pointer, Offset::new(Pixels(0.0), Pixels(0.0)));

        // Drive a cancel event. We need a cancel-shaped PointerEvent
        // — pull from the events module.
        let cancel = crate::events::make_cancel_event(PointerType::Touch);
        rec.handle_event(&cancel);

        assert!(*cancelled.lock());
        // Explicitly drop the recogniser to verify no Drop-induced hang.
        drop(rec);
    }

    #[test]
    fn reset_returns_to_ready_after_tap() {
        let arena = GestureArena::new();
        let rec = TapAndDragGestureRecognizer::new(arena);
        let pointer = PointerId::PRIMARY;
        let pos = Offset::new(Pixels(0.0), Pixels(0.0));

        rec.add_pointer(pointer, pos);
        rec.handle_event(&make_up_event(pos, PointerType::Touch));
        assert_eq!(*rec.phase.lock(), Phase::Ready);
        assert!(rec.drag_state.lock().initial.is_none());
    }

    #[test]
    fn reset_returns_to_ready_after_drag() {
        let arena = GestureArena::new();
        let rec = TapAndDragGestureRecognizer::new(arena);
        let pointer = PointerId::PRIMARY;
        let pos = Offset::new(Pixels(0.0), Pixels(0.0));

        rec.add_pointer(pointer, pos);
        // Cross slop.
        rec.handle_event(&make_move_event(
            Offset::new(Pixels(40.0), Pixels(0.0)),
            PointerType::Touch,
        ));
        // End drag.
        rec.handle_event(&make_up_event(
            Offset::new(Pixels(40.0), Pixels(0.0)),
            PointerType::Touch,
        ));

        assert_eq!(*rec.phase.lock(), Phase::Ready);
        assert!(rec.drag_state.lock().initial.is_none());
    }

    #[test]
    fn new_recogniser_is_in_ready_phase() {
        let arena = GestureArena::new();
        let rec = TapAndDragGestureRecognizer::new(arena);
        assert_eq!(*rec.phase.lock(), Phase::Ready);
    }

    // -----------------------------------------------------------------------
    // Sanity: unrelated Down event before add_pointer should be ignored.
    // -----------------------------------------------------------------------
    #[test]
    fn handle_event_without_primary_pointer_is_no_op() {
        let arena = GestureArena::new();
        let drag_start = Arc::new(Mutex::new(false));
        let rec = TapAndDragGestureRecognizer::new(arena).with_on_drag_start({
            let drag_start = drag_start.clone();
            move |_| *drag_start.lock() = true
        });

        // No add_pointer — primary_pointer() is None.
        rec.handle_event(&make_move_event(
            Offset::new(Pixels(40.0), Pixels(0.0)),
            PointerType::Touch,
        ));
        rec.handle_event(&make_up_event(
            Offset::new(Pixels(40.0), Pixels(0.0)),
            PointerType::Touch,
        ));

        assert!(!*drag_start.lock());
    }

    // Sanity: constructor builder pattern.
    #[test]
    fn builder_setters_store_callbacks() {
        let arena = GestureArena::new();
        let rec = TapAndDragGestureRecognizer::new(arena)
            .with_on_tap_down(|d| {
                let _ = d;
            })
            .with_on_tap_up(|d| {
                let _ = d;
            })
            .with_on_drag_start(|d| {
                let _ = d;
            })
            .with_on_drag_update(|d| {
                let _ = d;
            })
            .with_on_drag_end(|d| {
                let _ = d;
            })
            .with_on_cancel(|| {});
        // Drop the recogniser — callbacks are stored, no panic.
        drop(rec);
    }

    // Build event helper (re-exported for test scope).
}
