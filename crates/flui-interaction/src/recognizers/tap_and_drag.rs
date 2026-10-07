//! Composite tap-and-drag gesture recogniser.
//!
//! The recogniser arbitrates between two gesture outcomes for a single
//! primary pointer:
//!
//! - **Tap**: pointer up before crossing drag slop → fire
//!   `on_tap_down` / `on_tap_up`.
//! - **Drag**: pointer crosses drag slop before up → fire
//!   `on_drag_start` / `on_drag_update` / `on_drag_end`.
//!
//! Neither outcome reaches user code before the gesture arena has accepted
//! this recogniser: crossing drag slop *claims* the arena and the drag starts
//! on acceptance; a tap's up waits for the arena verdict (the binding's sweep
//! on pointer up, or a competitor releasing its hold) and fires on
//! acceptance. A loss after `on_tap_down` or mid-drag fires `on_cancel`.
//!
//! Every outcome carries a **consecutive tap count**: `1` for an isolated
//! contact, `2` for a contact that lands within the double-tap timeout and
//! slop of the previous completed tap, `3` for the next, and so on — what a
//! text field needs to select a word on a double click and a line on a triple
//! click, and to extend that selection by dragging.
//!
//! # When to use
//!
//! Use this recogniser when a single widget should react to *both* a
//! quick tap and a drag. Examples include text-selection handles
//! (a canonical use), draggable list items with tap-to-select
//! semantics, and map pins (tap to inspect, drag to reposition).
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::recognizers::tap_and_drag::TapAndDragGestureRecognizer;
//!
//! let recogniser = TapAndDragGestureRecognizer::new(binding.arena().clone())
//!     .with_on_tap_up(|d| select_by_count(d.local_position, d.consecutive_tap_count))
//!     .with_on_drag_update(|d| extend_selection(d.local_position));
//! ```

use std::{cell::RefCell, rc::Rc, sync::Arc};

use flui_foundation::geometry::Offset;
use parking_lot::Mutex;
use web_time::Instant;

use super::{
    recognizer::{GestureRecognizer, RecognizerBase},
    recognizer::{finish_containment, invoke_callback, retire_callback},
};
use crate::{
    arena::{GestureArenaEntry, GestureArenaMember, GestureDisposition, SweepModel},
    events::{PointerEvent, PointerType},
    ids::PointerId,
    processing::{Velocity, VelocityTracker},
    routing::{PointerDispatch, RoutePanic},
    settings::GestureSettings,
    traits::PointerEventExtTrait,
};

// ============================================================================
// Details types
// ============================================================================

/// Position, kind and consecutive-tap-count details for tap-down.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct TapDragDownDetails {
    /// Global position where pointer contacted the screen.
    pub global_position: Offset<f64>,
    /// Local position (relative to widget).
    pub local_position: Offset<f64>,
    /// Pointer device kind.
    pub kind: PointerType,
    /// This contact's place in a run of consecutive taps, starting at `1`.
    pub consecutive_tap_count: u32,
}

/// Position, kind and consecutive-tap-count details for tap-up.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct TapDragUpDetails {
    /// Global position where pointer was released.
    pub global_position: Offset<f64>,
    /// Local position (relative to widget).
    pub local_position: Offset<f64>,
    /// Pointer device kind.
    pub kind: PointerType,
    /// This tap's place in a run of consecutive taps, starting at `1`.
    pub consecutive_tap_count: u32,
}

/// Details for drag-start.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct TapDragStartDetails {
    /// Global position where the drag started (down position).
    pub global_position: Offset<f64>,
    /// Local position.
    pub local_position: Offset<f64>,
    /// Pointer device kind.
    pub kind: PointerType,
    /// The dragging contact's place in a run of consecutive taps: `2` for a
    /// drag that follows one tap (double-click-drag).
    pub consecutive_tap_count: u32,
}

/// Details for drag-update.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct TapDragUpdateDetails {
    /// Current global position.
    pub global_position: Offset<f64>,
    /// Current local position.
    pub local_position: Offset<f64>,
    /// Delta since the previous update (since the down position for the
    /// first update).
    pub delta: Offset<f64>,
    /// Pointer device kind.
    pub kind: PointerType,
    /// The dragging contact's place in a run of consecutive taps.
    pub consecutive_tap_count: u32,
}

/// Details for drag-end.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct TapDragEndDetails {
    /// Velocity at the end of the drag.
    pub velocity: Velocity,
    /// Final global position.
    pub global_position: Offset<f64>,
    /// Final local position.
    pub local_position: Offset<f64>,
    /// The dragging contact's place in a run of consecutive taps.
    pub consecutive_tap_count: u32,
}

// ============================================================================
// Callbacks
// ============================================================================

/// Callback fired once the arena has accepted the contact (at the latest
/// immediately before `on_tap_up` or `on_drag_start`).
pub type TapDragDownCallback = Rc<dyn Fn(TapDragDownDetails)>;
/// Callback fired when the pointer lifts before crossing drag slop (a tap).
pub type TapDragUpCallback = Rc<dyn Fn(TapDragUpDetails)>;
/// Callback fired when the drag begins (slop crossed and arena accepted).
pub type TapDragStartCallback = Rc<dyn Fn(TapDragStartDetails)>;
/// Callback fired for each pointer move while the drag is in progress.
pub type TapDragUpdateCallback = Rc<dyn Fn(TapDragUpdateDetails)>;
/// Callback fired when the pointer lifts and the drag completes.
pub type TapDragEndCallback = Rc<dyn Fn(TapDragEndDetails)>;
/// Callback fired when a sequence that already delivered `on_tap_down` or
/// `on_drag_start` ends without `on_tap_up` or `on_drag_end` (arena loss,
/// pointer cancel, a tap voided by drift).
pub type TapDragCancelCallback = Rc<dyn Fn()>;

// ============================================================================
// Recogniser
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// No pointer in flight.
    Ready,
    /// Pointer down, drag slop not crossed.
    Down,
    /// Drag slop crossed; the arena claim is pending.
    Claiming,
    /// Drag in progress.
    Dragging,
    /// Pointer up as a viable tap; waiting for the arena verdict.
    TapPending,
}

// Field names keep the `on_tap_down`/`on_drag_start`-style callback names.
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

impl TapDragCallbacks {
    fn retire(self, first: &mut Option<RoutePanic>) {
        retire_callback(self.on_tap_down, first);
        retire_callback(self.on_tap_up, first);
        retire_callback(self.on_drag_start, first);
        retire_callback(self.on_drag_update, first);
        retire_callback(self.on_drag_end, first);
        retire_callback(self.on_cancel, first);
    }
}

/// The last completed tap, which the next contact may continue.
#[derive(Debug, Clone, Copy)]
struct LastTap {
    up_time: Instant,
    down_position: Offset<f64>,
    count: u32,
}

#[derive(Debug, Clone)]
struct TapDragState {
    /// Bumped whenever the sequence resets, so callbacks queued for a
    /// sequence that a reentrant transition already ended are not delivered.
    generation: u64,
    phase: Phase,
    pointer: Option<PointerId>,
    entry: Option<GestureArenaEntry>,
    /// The arena accepted this recogniser for the tracked contact.
    won: bool,
    tap_down_delivered: bool,
    /// `false` once the pointer wandered past tap slop.
    tap_viable: bool,
    kind: PointerType,
    count: u32,
    initial: Offset<f64>,
    initial_global: Offset<f64>,
    last: Offset<f64>,
    last_global: Offset<f64>,
    /// Position of the last published drag update.
    last_reported: Offset<f64>,
    velocity_tracker: VelocityTracker,
    pending_up: Option<TapDragUpDetails>,
    up_time: Option<Instant>,
    /// Survives the sequence reset; a drag, cancel or loss clears it.
    last_tap: Option<LastTap>,
}

impl Default for TapDragState {
    fn default() -> Self {
        Self {
            generation: 0,
            phase: Phase::Ready,
            pointer: None,
            entry: None,
            won: false,
            tap_down_delivered: false,
            tap_viable: true,
            kind: PointerType::Touch,
            count: 1,
            initial: Offset::ZERO,
            initial_global: Offset::ZERO,
            last: Offset::ZERO,
            last_global: Offset::ZERO,
            last_reported: Offset::ZERO,
            velocity_tracker: VelocityTracker::new(),
            pending_up: None,
            up_time: None,
            last_tap: None,
        }
    }
}

/// A user callback owed once the state lock is released.
enum Notice {
    TapDown(TapDragDownDetails),
    TapUp(TapDragUpDetails),
    DragStart(TapDragStartDetails),
    DragUpdate(TapDragUpdateDetails),
    DragEnd(TapDragEndDetails),
    Cancel,
}

/// Arena work owed once the state lock is released, run before notices.
enum ArenaStep {
    None,
    Claim(GestureArenaEntry),
    Withdraw(GestureArenaEntry),
    /// A self-driven arena is swept by its recogniser on pointer up.
    Sweep(GestureArenaEntry),
}

impl TapDragState {
    /// Clear the sequence, keeping the consecutive-tap chain.
    fn reset_sequence(&mut self) {
        let last_tap = self.last_tap;
        // Only compared for change; wrapping after 2^64 resets is harmless.
        let generation = self.generation.wrapping_add(1);
        *self = Self {
            generation,
            last_tap,
            ..Self::default()
        };
    }

    fn deliver_tap_down(&mut self, out: &mut Vec<Notice>) {
        if !self.tap_down_delivered {
            self.tap_down_delivered = true;
            out.push(Notice::TapDown(TapDragDownDetails {
                global_position: self.initial_global,
                local_position: self.initial,
                kind: self.kind,
                consecutive_tap_count: self.count,
            }));
        }
    }

    fn start_drag(&mut self, out: &mut Vec<Notice>) {
        self.deliver_tap_down(out);
        self.phase = Phase::Dragging;
        self.last_tap = None;
        out.push(Notice::DragStart(TapDragStartDetails {
            global_position: self.initial_global,
            local_position: self.initial,
            kind: self.kind,
            consecutive_tap_count: self.count,
        }));
        out.push(Notice::DragUpdate(TapDragUpdateDetails {
            global_position: self.last_global,
            local_position: self.last,
            delta: (self.last - self.initial).to_delta(),
            kind: self.kind,
            consecutive_tap_count: self.count,
        }));
        self.last_reported = self.last;
    }

    fn complete_tap(&mut self, out: &mut Vec<Notice>) {
        self.deliver_tap_down(out);
        if let (Some(up), Some(up_time)) = (self.pending_up.take(), self.up_time) {
            out.push(Notice::TapUp(up));
            self.last_tap = Some(LastTap {
                up_time,
                down_position: self.initial,
                count: self.count,
            });
        }
        self.reset_sequence();
    }

    /// End the sequence without an outcome. Returns the arena entry, which
    /// the caller withdraws when it is still unresolved.
    fn abandon(&mut self, out: &mut Vec<Notice>) -> Option<GestureArenaEntry> {
        if self.tap_down_delivered || self.phase == Phase::Dragging {
            out.push(Notice::Cancel);
        }
        let entry = self.entry.take();
        self.last_tap = None;
        self.reset_sequence();
        entry
    }
}

/// Composite tap-and-drag recogniser.
///
/// See [module-level docs](self) for the design. A second contact while one
/// is down does not join; the same pointer going down again retires its
/// previous sequence first. Callbacks run after the recogniser has committed
/// its state, with no borrow held, so a callback may dispose the recogniser.
/// A panicking callback does not stop the transition: its remaining callbacks
/// are still delivered, and the first panic resumes afterwards. Callbacks
/// queued for a sequence that a callback ended reentrantly are dropped.
#[derive(Clone)]
pub struct TapAndDragGestureRecognizer {
    state: RecognizerBase,
    gesture_state: Arc<Mutex<TapDragState>>,
    callbacks: Rc<RefCell<TapDragCallbacks>>,
    settings: Arc<Mutex<GestureSettings>>,
}

impl std::fmt::Debug for TapAndDragGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TapAndDragGestureRecognizer")
            .field("state", &self.state)
            .field("gesture_state", &*self.gesture_state.lock())
            .field("settings", &*self.settings.lock())
            .finish_non_exhaustive()
    }
}

impl TapAndDragGestureRecognizer {
    /// Create a new tap-and-drag recogniser.
    pub fn new(arena: crate::arena::GestureArena) -> Arc<Self> {
        Self::with_settings(arena, GestureSettings::default())
    }

    /// Create with custom gesture settings.
    pub fn with_settings(
        arena: crate::arena::GestureArena,
        settings: GestureSettings,
    ) -> Arc<Self> {
        Arc::new(Self {
            state: RecognizerBase::new(arena),
            gesture_state: Arc::new(Mutex::new(TapDragState::default())),
            callbacks: Rc::new(RefCell::new(TapDragCallbacks::default())),
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

    // ========================================================================
    // Builder-style callback setters
    // ========================================================================

    /// Register the tap-down callback (fires once the arena has accepted the
    /// contact, at the latest right before `on_tap_up` or `on_drag_start`).
    pub fn with_on_tap_down(
        self: Arc<Self>,
        cb: impl Fn(TapDragDownDetails) + 'static,
    ) -> Arc<Self> {
        let old = self.callbacks.borrow_mut().on_tap_down.replace(Rc::new(cb));
        drop(old);
        self
    }

    /// Register the tap-up callback (fires when the pointer lifted before
    /// crossing tap slop and the arena accepted the tap).
    pub fn with_on_tap_up(self: Arc<Self>, cb: impl Fn(TapDragUpDetails) + 'static) -> Arc<Self> {
        let old = self.callbacks.borrow_mut().on_tap_up.replace(Rc::new(cb));
        drop(old);
        self
    }

    /// Register the drag-start callback (fires when the pointer crossed drag
    /// slop and the arena accepted the drag).
    pub fn with_on_drag_start(
        self: Arc<Self>,
        cb: impl Fn(TapDragStartDetails) + 'static,
    ) -> Arc<Self> {
        let old = self
            .callbacks
            .borrow_mut()
            .on_drag_start
            .replace(Rc::new(cb));
        drop(old);
        self
    }

    /// Register the drag-update callback (fires once with the crossing move
    /// right after `on_drag_start`, then for each move while dragging).
    pub fn with_on_drag_update(
        self: Arc<Self>,
        cb: impl Fn(TapDragUpdateDetails) + 'static,
    ) -> Arc<Self> {
        let old = self
            .callbacks
            .borrow_mut()
            .on_drag_update
            .replace(Rc::new(cb));
        drop(old);
        self
    }

    /// Register the drag-end callback (fires when the pointer lifts after a
    /// drag, with end-of-drag velocity).
    pub fn with_on_drag_end(
        self: Arc<Self>,
        cb: impl Fn(TapDragEndDetails) + 'static,
    ) -> Arc<Self> {
        let old = self.callbacks.borrow_mut().on_drag_end.replace(Rc::new(cb));
        drop(old);
        self
    }

    /// Register the cancel callback. See [`TapDragCancelCallback`] for when
    /// it fires.
    pub fn with_on_cancel(self: Arc<Self>, cb: impl Fn() + 'static) -> Arc<Self> {
        let old = self.callbacks.borrow_mut().on_cancel.replace(Rc::new(cb));
        drop(old);
        self
    }

    // ========================================================================
    // Internal helpers
    // ========================================================================

    /// Forget the base's record of the contact once its sequence is over.
    fn clear_base_tracking(&self) {
        self.state.set_primary_pointer(None);
        self.state.clear_initial_contact();
    }

    /// Run the arena step, then deliver the notices in order. The state was
    /// committed beforehand; the first panic is resumed after the arena step
    /// ran and every notice of the transition was delivered.
    fn finish(&self, step: ArenaStep, notices: Vec<Notice>) {
        let generation = self.gesture_state.lock().generation;
        let self_driven = self.state.arena().sweep_model() == SweepModel::SelfDriven;
        let mut first = match step {
            ArenaStep::None => None,
            ArenaStep::Claim(entry) => {
                RoutePanic::capture(|| entry.resolve(GestureDisposition::Accepted))
            }
            ArenaStep::Withdraw(entry) => RoutePanic::capture(|| {
                entry.resolve(GestureDisposition::Rejected);
                if self_driven {
                    entry.sweep();
                }
            }),
            ArenaStep::Sweep(entry) => RoutePanic::capture(|| {
                if self_driven {
                    entry.sweep();
                }
            }),
        };
        // Every notice of a committed transition is delivered, so a panic in
        // `on_tap_down` cannot strand a started drag without its end or a tap
        // without its up. The first failure resumes after the rest.
        for notice in notices {
            // A callback that ended this sequence reentrantly (a cancel, a
            // dispose) already delivered its own end; what is left belongs to
            // a sequence that no longer exists.
            if self.gesture_state.lock().generation != generation {
                break;
            }
            RoutePanic::preserve_first(
                &mut first,
                RoutePanic::capture(|| self.deliver(notice)),
                "tap and drag callback",
            );
        }
        // Entered while the thread is already unwinding, the failure is retained
        // rather than resumed: a second unwind would abort.
        finish_containment(first, std::thread::panicking());
    }

    fn deliver(&self, notice: Notice) {
        let callbacks = self.callbacks.borrow();
        match notice {
            Notice::TapDown(d) => {
                let cb = callbacks.on_tap_down.clone();
                drop(callbacks);
                invoke_callback(cb, || {}, |cb| cb(d));
            }
            Notice::TapUp(d) => {
                let cb = callbacks.on_tap_up.clone();
                drop(callbacks);
                invoke_callback(cb, || {}, |cb| cb(d));
            }
            Notice::DragStart(d) => {
                let cb = callbacks.on_drag_start.clone();
                drop(callbacks);
                invoke_callback(cb, || {}, |cb| cb(d));
            }
            Notice::DragUpdate(d) => {
                let cb = callbacks.on_drag_update.clone();
                drop(callbacks);
                invoke_callback(cb, || {}, |cb| cb(d));
            }
            Notice::DragEnd(d) => {
                let cb = callbacks.on_drag_end.clone();
                drop(callbacks);
                invoke_callback(cb, || {}, |cb| cb(d));
            }
            Notice::Cancel => {
                let cb = callbacks.on_cancel.clone();
                drop(callbacks);
                invoke_callback(cb, || {}, |cb| cb());
            }
        }
    }

    fn handle_move(&self, position: Offset<f64>, global_position: Offset<f64>, kind: PointerType) {
        if !position.is_finite() {
            return;
        }
        let (drag_slop, tap_slop) = {
            let settings = self.settings.lock();
            // A free-plane drag takes the pan tier; tap viability is a hit
            // test and takes the plain tier.
            (settings.pan_slop_for(kind), settings.hit_slop(kind))
        };
        let now = self.state.now();
        let mut notices = Vec::new();
        let mut step = ArenaStep::None;
        let mut state = self.gesture_state.lock();
        state.kind = kind;
        state.last = position;
        if global_position.is_finite() {
            state.last_global = global_position;
        }
        let global_position = state.last_global;
        state.velocity_tracker.add_position(now, position);
        match state.phase {
            Phase::Down => {
                let distance = (position - state.initial).distance();
                if distance > tap_slop {
                    state.tap_viable = false;
                }
                if distance > drag_slop {
                    if state.won {
                        state.start_drag(&mut notices);
                    } else {
                        state.phase = Phase::Claiming;
                        if let Some(entry) = state.entry.clone() {
                            step = ArenaStep::Claim(entry);
                        }
                    }
                }
            }
            Phase::Dragging => {
                let delta = (position - state.last_reported).to_delta();
                state.last_reported = position;
                notices.push(Notice::DragUpdate(TapDragUpdateDetails {
                    global_position,
                    local_position: position,
                    delta,
                    kind,
                    consecutive_tap_count: state.count,
                }));
            }
            Phase::Ready | Phase::Claiming | Phase::TapPending => {}
        }
        drop(state);
        self.finish(step, notices);
    }

    fn handle_up(&self, position: Offset<f64>, global_position: Offset<f64>, kind: PointerType) {
        let now = self.state.now();
        let mut notices = Vec::new();
        let mut state = self.gesture_state.lock();
        state.kind = kind;
        let position = if position.is_finite() {
            position
        } else {
            state.last
        };
        let global_position = if global_position.is_finite() {
            global_position
        } else {
            state.last_global
        };
        let step = match state.phase {
            Phase::Down if state.tap_viable => {
                state.pending_up = Some(TapDragUpDetails {
                    global_position,
                    local_position: position,
                    kind,
                    consecutive_tap_count: state.count,
                });
                state.up_time = Some(now);
                if state.won {
                    state.complete_tap(&mut notices);
                    ArenaStep::None
                } else {
                    // The verdict arrives through `accept_gesture` /
                    // `reject_gesture`: the binding sweeps after routing this
                    // up, a self-driven arena is swept below.
                    state.phase = Phase::TapPending;
                    state
                        .entry
                        .clone()
                        .map_or(ArenaStep::None, ArenaStep::Sweep)
                }
            }
            Phase::Dragging => {
                let velocity = state.velocity_tracker.get_velocity();
                notices.push(Notice::DragEnd(TapDragEndDetails {
                    velocity,
                    global_position,
                    local_position: position,
                    consecutive_tap_count: state.count,
                }));
                let entry = state.entry.take();
                state.last_tap = None;
                state.reset_sequence();
                entry.map_or(ArenaStep::None, ArenaStep::Sweep)
            }
            // Neither a tap (drifted) nor a drag (never accepted): give the
            // arena up so a competitor can take it.
            Phase::Down | Phase::Claiming => state
                .abandon(&mut notices)
                .map_or(ArenaStep::None, ArenaStep::Withdraw),
            Phase::Ready | Phase::TapPending => return,
        };
        drop(state);
        self.clear_base_tracking();
        self.finish(step, notices);
    }

    fn handle_cancel(&self) {
        let mut notices = Vec::new();
        let mut state = self.gesture_state.lock();
        if matches!(state.phase, Phase::Ready | Phase::TapPending) {
            return;
        }
        let step = state
            .abandon(&mut notices)
            .map_or(ArenaStep::None, ArenaStep::Withdraw);
        drop(state);
        self.clear_base_tracking();
        self.finish(step, notices);
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
        if !self.state.assert_not_disposed("add_pointer") || !position.is_finite() {
            return;
        }
        let (phase, tracked) = {
            let state = self.gesture_state.lock();
            (state.phase, state.pointer)
        };
        let finger_down = matches!(phase, Phase::Down | Phase::Claiming | Phase::Dragging);
        if finger_down && tracked != Some(pointer) {
            return;
        }
        if phase != Phase::Ready {
            // The previous sequence is still waiting for a verdict (or never
            // saw its up): it ends here, before the new contact starts.
            let mut notices = Vec::new();
            let step = self
                .gesture_state
                .lock()
                .abandon(&mut notices)
                .map_or(ArenaStep::None, ArenaStep::Withdraw);
            self.clear_base_tracking();
            self.finish(step, notices);
            // The retired sequence's `on_cancel` may have disposed this
            // recognizer or admitted a contact of its own; either way this
            // admission is void.
            if self.state.is_disposed() || self.gesture_state.lock().phase != Phase::Ready {
                return;
            }
        }

        let now = self.state.now();
        let (timeout, slop) = {
            let settings = self.settings.lock();
            (settings.double_tap_timeout(), settings.double_tap_slop())
        };
        self.state
            .start_tracking(pointer, position, global_position, self);
        let entry = self.state.tracked_entry();
        let mut state = self.gesture_state.lock();
        let count = state
            .last_tap
            .filter(|last| {
                now.saturating_duration_since(last.up_time) <= timeout
                    && (position - last.down_position).distance() <= slop
            })
            .map_or(1, |last| last.count.saturating_add(1));
        state.reset_sequence();
        state.phase = Phase::Down;
        state.pointer = Some(pointer);
        state.entry = entry;
        state.count = count;
        state.initial = position;
        // A non-finite global position is never published: with no earlier
        // sample in this sequence, the local position stands in for it.
        let global_position = if global_position.is_finite() {
            global_position
        } else {
            position
        };
        state.initial_global = global_position;
        state.last = position;
        state.last_global = global_position;
        state.last_reported = position;
        // Read the arena's clock: a headless frame driver binds a
        // `ManualClock`, so a replayed gesture's own sample spacing decides
        // the velocity.
        state.velocity_tracker.add_position(now, position);
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
        if self.gesture_state.lock().pointer != Some(event.pointer_id()) {
            return;
        }
        // Read once, here: this is the only point at which the untransformed
        // position is available at all (issue #908).
        let global_position = dispatch.global.position();

        match event {
            PointerEvent::Down(data) => {
                self.gesture_state.lock().kind = data.pointer.pointer_type;
            }
            PointerEvent::Move(data) => {
                let pos = data.current.position;
                self.handle_move(
                    Offset::new(pos.x, pos.y),
                    global_position,
                    data.pointer.pointer_type,
                );
            }
            PointerEvent::Up(data) => {
                let pos = data.state.position;
                self.handle_up(
                    Offset::new(pos.x, pos.y),
                    global_position,
                    data.pointer.pointer_type,
                );
            }
            PointerEvent::Cancel(_) => self.handle_cancel(),
            _ => {}
        }
    }

    fn dispose(&self) {
        let incoming_failure = std::thread::panicking();
        self.state.mark_disposed();
        let callbacks = std::mem::take(&mut *self.callbacks.borrow_mut());
        let entry = {
            let mut state = self.gesture_state.lock();
            let entry = state.entry.take();
            *state = TapDragState::default();
            entry
        };
        let mut first = RoutePanic::capture(|| {
            self.state.reject();
            if let Some(entry) = entry {
                entry.resolve(GestureDisposition::Rejected);
            }
        });
        callbacks.retire(&mut first);
        finish_containment(first, incoming_failure);
    }

    fn primary_pointer(&self) -> Option<PointerId> {
        self.state.primary_pointer()
    }
}

impl crate::recognizers::OneSequenceGestureRecognizer for TapAndDragGestureRecognizer {
    fn tracked_pointers(&self) -> Vec<PointerId> {
        self.gesture_state.lock().pointer.into_iter().collect()
    }

    fn resolve_pointer(&self, pointer: PointerId, disposition: GestureDisposition) {
        let entry = {
            let state = self.gesture_state.lock();
            (state.pointer == Some(pointer))
                .then(|| state.entry.clone())
                .flatten()
        };
        if let Some(entry) = entry {
            entry.resolve(disposition);
        }
    }

    fn stop_tracking_pointer(&self, pointer: PointerId) {
        if self.gesture_state.lock().pointer == Some(pointer) {
            self.handle_cancel();
        }
    }
}

impl GestureArenaMember for TapAndDragGestureRecognizer {
    fn accept_gesture(&self, pointer: PointerId) {
        let mut notices = Vec::new();
        let mut state = self.gesture_state.lock();
        if state.pointer != Some(pointer) {
            return;
        }
        state.won = true;
        match state.phase {
            Phase::Down => state.deliver_tap_down(&mut notices),
            Phase::Claiming => state.start_drag(&mut notices),
            Phase::TapPending => state.complete_tap(&mut notices),
            Phase::Dragging | Phase::Ready => {}
        }
        drop(state);
        self.finish(ArenaStep::None, notices);
    }

    fn reject_gesture(&self, pointer: PointerId) {
        let mut notices = Vec::new();
        let mut state = self.gesture_state.lock();
        if state.pointer != Some(pointer) {
            return;
        }
        // The entry is already resolved; dropping it is all that is left.
        let _resolved = state.abandon(&mut notices);
        drop(state);
        self.clear_base_tracking();
        self.finish(ArenaStep::None, notices);
    }
}
