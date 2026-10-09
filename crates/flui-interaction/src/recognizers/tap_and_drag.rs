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
//! ```rust
//! use flui_interaction::{GestureArena, TapAndDragGestureRecognizer};
//!
//! let recogniser = TapAndDragGestureRecognizer::builder(GestureArena::new())
//!     .on_tap_up(|details| { let _ = details.consecutive_tap_count; })
//!     .on_drag_update(|details| { let _ = details.local_position; })
//!     .build();
//! ```

use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

use flui_foundation::geometry::Offset;
use web_time::Instant;

use super::{
    ArenaMembership, CancelOutcome, PrimaryContact,
    callback_containment::{finish_containment, invoke_callback, retire_callbacks},
    recognizer::{EventTimeline, GestureRecognizer, event_time, is_primary_down, motion_history},
};
use crate::{
    arena::{GestureArenaEntry, GestureArenaMember, GestureDisposition, SweepModel},
    events::{PointerEvent, PointerEventExt, PointerKind},
    ids::PointerId,
    processing::{Velocity, VelocityTracker},
    routing::{PointerDispatch, RoutePanic},
    settings::{GestureSettings, GestureSettingsProvider},
};
use flui_platform_api::pointer::DeviceId;

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
    pub kind: PointerKind,
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
    pub kind: PointerKind,
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
    pub kind: PointerKind,
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
    pub kind: PointerKind,
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

impl Drop for TapDragCallbacks {
    fn drop(&mut self) {
        retire_callbacks!(self; on_tap_down, on_tap_up, on_drag_start, on_drag_update, on_drag_end, on_cancel);
    }
}

/// The last completed tap, which the next contact may continue.
#[derive(Debug, Clone)]
struct LastTap {
    up_time: Instant,
    down_time: Instant,
    down_position: Offset<f64>,
    count: u32,
    settings: GestureSettings,
    kind: PointerKind,
    device: Option<DeviceId>,
}

#[derive(Debug, Clone)]
struct TapDragState {
    /// Policy retained by this attempt and its consecutive-tap candidate.
    settings: GestureSettings,
    phase: Phase,
    pointer: Option<PointerId>,
    entry: Option<GestureArenaEntry>,
    /// The arena accepted this recogniser for the tracked contact.
    won: bool,
    tap_down_delivered: bool,
    /// `false` once the pointer wandered past tap slop.
    tap_viable: bool,
    kind: PointerKind,
    device: Option<DeviceId>,
    count: u32,
    initial: Offset<f64>,
    initial_global: Offset<f64>,
    last: Offset<f64>,
    last_global: Offset<f64>,
    /// Position of the last published drag update.
    last_reported: Offset<f64>,
    velocity_tracker: VelocityTracker,
    timeline: EventTimeline,
    pending_up: Option<TapDragUpDetails>,
    up_time: Option<Instant>,
    down_time: Option<Instant>,
    /// Survives the sequence reset; a drag, cancel or loss clears it.
    last_tap: Option<LastTap>,
}

impl Default for TapDragState {
    fn default() -> Self {
        Self {
            settings: GestureSettings::default(),
            phase: Phase::Ready,
            pointer: None,
            entry: None,
            won: false,
            tap_down_delivered: false,
            tap_viable: true,
            kind: PointerKind::Touch,
            device: None,
            count: 1,
            initial: Offset::ZERO,
            initial_global: Offset::ZERO,
            last: Offset::ZERO,
            last_global: Offset::ZERO,
            last_reported: Offset::ZERO,
            velocity_tracker: VelocityTracker::new(),
            timeline: EventTimeline::default(),
            pending_up: None,
            up_time: None,
            down_time: None,
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
        let last_tap = self.last_tap.take();
        *self = Self {
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
        if let (Some(up), Some(up_time), Some(down_time)) =
            (self.pending_up.take(), self.up_time, self.down_time)
        {
            out.push(Notice::TapUp(up));
            self.last_tap = Some(LastTap {
                up_time,
                down_time,
                down_position: self.initial,
                count: self.count,
                settings: self.settings.clone(),
                kind: self.kind,
                device: self.device,
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
/// is down does not join. Callbacks run after the recogniser has committed
/// its state, with no borrow held, so a callback may cancel the recogniser.
/// A panicking callback does not stop the transition: its remaining callbacks
/// are still delivered, and the first panic resumes afterwards. Callbacks
/// queued for a sequence that a callback ended reentrantly are dropped.
pub struct TapAndDragGestureRecognizer {
    contact: PrimaryContact,
    arena: crate::arena::GestureArena,
    gesture_state: RefCell<TapDragState>,
    callbacks: TapDragCallbacks,
    settings: GestureSettingsProvider,
}

impl std::fmt::Debug for TapAndDragGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TapAndDragGestureRecognizer")
            .field("contact", &self.contact)
            .field("gesture_state", &*self.gesture_state.borrow())
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

/// Immutable callback and gesture policy construction.
#[must_use]
pub struct TapAndDragGestureRecognizerBuilder {
    arena: crate::arena::GestureArena,
    callbacks: TapDragCallbacks,
    settings: GestureSettingsProvider,
}

impl std::fmt::Debug for TapAndDragGestureRecognizerBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TapAndDragGestureRecognizerBuilder")
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

impl TapAndDragGestureRecognizerBuilder {
    /// Freeze settings for contacts admitted by this recognizer.
    pub fn settings(mut self, settings: impl Into<GestureSettingsProvider>) -> Self {
        self.settings = settings.into();
        self
    }

    /// Allocate the recognizer with weak arena identity.
    pub fn build(self) -> Rc<TapAndDragGestureRecognizer> {
        Rc::new_cyclic(|this: &Weak<TapAndDragGestureRecognizer>| {
            let member: Weak<dyn GestureArenaMember> = this.clone();
            TapAndDragGestureRecognizer {
                contact: PrimaryContact::new(ArenaMembership::new(self.arena.clone(), member)),
                arena: self.arena,
                gesture_state: RefCell::new(TapDragState::default()),
                callbacks: self.callbacks,
                settings: self.settings,
            }
        })
    }

    /// Register the tap-down callback (fires once the arena has accepted the
    /// contact, at the latest right before `on_tap_up` or `on_drag_start`).
    pub fn on_tap_down(mut self, cb: impl Fn(TapDragDownDetails) + 'static) -> Self {
        self.callbacks.on_tap_down = Some(Rc::new(cb));
        self
    }

    /// Register the tap-up callback (fires when the pointer lifted before
    /// crossing tap slop and the arena accepted the tap).
    pub fn on_tap_up(mut self, cb: impl Fn(TapDragUpDetails) + 'static) -> Self {
        self.callbacks.on_tap_up = Some(Rc::new(cb));
        self
    }

    /// Register the drag-start callback (fires when the pointer crossed drag
    /// slop and the arena accepted the drag).
    pub fn on_drag_start(mut self, cb: impl Fn(TapDragStartDetails) + 'static) -> Self {
        self.callbacks.on_drag_start = Some(Rc::new(cb));
        self
    }

    /// Register the drag-update callback (fires once with the crossing move
    /// right after `on_drag_start`, then for each move while dragging).
    pub fn on_drag_update(mut self, cb: impl Fn(TapDragUpdateDetails) + 'static) -> Self {
        self.callbacks.on_drag_update = Some(Rc::new(cb));
        self
    }

    /// Register the drag-end callback (fires when the pointer lifts after a
    /// drag, with end-of-drag velocity).
    pub fn on_drag_end(mut self, cb: impl Fn(TapDragEndDetails) + 'static) -> Self {
        self.callbacks.on_drag_end = Some(Rc::new(cb));
        self
    }

    /// Register the cancel callback. See [`TapDragCancelCallback`] for when
    /// it fires.
    pub fn on_cancel(mut self, cb: impl Fn() + 'static) -> Self {
        self.callbacks.on_cancel = Some(Rc::new(cb));
        self
    }
}

impl TapAndDragGestureRecognizer {
    /// Begin construction with default gesture policy.
    pub fn builder(arena: crate::arena::GestureArena) -> TapAndDragGestureRecognizerBuilder {
        TapAndDragGestureRecognizerBuilder {
            arena,
            callbacks: TapDragCallbacks::default(),
            settings: GestureSettingsProvider::default(),
        }
    }

    // ========================================================================
    // Internal helpers
    // ========================================================================

    /// Run the arena step, then deliver the notices in order. The state was
    /// committed beforehand; the first panic is resumed after the arena step
    /// ran and every notice of the transition was delivered.
    fn finish(&self, step: ArenaStep, notices: Vec<Notice>) {
        let contact_id = self.contact.current().map(|contact| contact.id);
        let self_driven = self.arena.sweep_model() == SweepModel::SelfDriven;
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
            // cancellation) already delivered its own end; what is left belongs to
            // a sequence that no longer exists.
            if contact_id.is_some_and(|id| !self.contact.is_current(id)) {
                break;
            }
            RoutePanic::preserve_first(
                &mut first,
                RoutePanic::capture(|| self.deliver(notice)),
                "tap and drag callback",
            );
        }
        if self.gesture_state.borrow().phase == Phase::Ready
            && contact_id.is_some_and(|id| self.contact.is_current(id))
        {
            RoutePanic::preserve_first(
                &mut first,
                RoutePanic::capture(|| {
                    self.contact.finish();
                }),
                "tap and drag completion",
            );
        }
        // Entered while the thread is already unwinding, the failure is retained
        // rather than resumed: a second unwind would abort.
        finish_containment(first, std::thread::panicking());
    }

    fn deliver(&self, notice: Notice) {
        let callbacks = &self.callbacks;
        match notice {
            Notice::TapDown(d) => {
                self.gesture_state.borrow_mut().tap_down_delivered = true;
                let cb = callbacks.on_tap_down.clone();
                invoke_callback(cb, || {}, |cb| cb(d));
            }
            Notice::TapUp(d) => {
                let cb = callbacks.on_tap_up.clone();
                invoke_callback(cb, || {}, |cb| cb(d));
            }
            Notice::DragStart(d) => {
                let cb = callbacks.on_drag_start.clone();
                invoke_callback(cb, || {}, |cb| cb(d));
            }
            Notice::DragUpdate(d) => {
                let cb = callbacks.on_drag_update.clone();
                invoke_callback(cb, || {}, |cb| cb(d));
            }
            Notice::DragEnd(d) => {
                let cb = callbacks.on_drag_end.clone();
                invoke_callback(cb, || {}, |cb| cb(d));
            }
            Notice::Cancel => {
                let cb = callbacks.on_cancel.clone();
                invoke_callback(cb, || {}, |cb| cb());
            }
        }
    }

    fn handle_move(
        &self,
        position: Offset<f64>,
        global_position: Offset<f64>,
        stamp: Option<u64>,
        history: impl Iterator<Item = (Option<u64>, Offset<f64>)>,
    ) {
        if !position.is_finite() {
            return;
        }
        let Some(contact) = self.contact.current() else {
            return;
        };
        let kind = contact.kind;
        let settings = &contact.settings;
        let now = self.contact.now();
        if !self.contact.is_current(contact.id) {
            return;
        }
        let mut notices = Vec::new();
        let mut step = ArenaStep::None;
        let mut state = self.gesture_state.borrow_mut();
        let origin = match state.phase {
            Phase::Down | Phase::Claiming => state.initial,
            Phase::Dragging => state.last_reported,
            Phase::Ready | Phase::TapPending => position,
        };
        let delta = position - origin;
        if !delta.is_finite() {
            drop(state);
            self.cancel();
            return;
        }
        let mut exceeded_tap = false;
        let mut exceeded_drag = false;
        for (stamp, position) in history {
            let delta = position - state.initial;
            exceeded_tap |= settings.exceeds_hit_slop(kind, delta);
            exceeded_drag |= settings.exceeds_pan_slop_for(kind, delta);
            let timestamp = state.timeline.instant(stamp, now);
            state.velocity_tracker.add_position(timestamp, position);
        }
        let now = state.timeline.instant(stamp, now);
        state.kind = kind;
        state.last = position;
        if global_position.is_finite() {
            state.last_global = global_position;
        }
        let global_position = state.last_global;
        state.velocity_tracker.add_position(now, position);
        match state.phase {
            Phase::Down => {
                if exceeded_tap || settings.exceeds_hit_slop(kind, delta) {
                    state.tap_viable = false;
                }
                if exceeded_drag || settings.exceeds_pan_slop_for(kind, delta) {
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

    fn handle_up(&self, position: Offset<f64>, global_position: Offset<f64>, stamp: Option<u64>) {
        let Some(contact) = self.contact.current() else {
            return;
        };
        let kind = contact.kind;
        let now = self.contact.now();
        if !self.contact.is_current(contact.id) {
            return;
        }
        let mut notices = Vec::new();
        let mut state = self.gesture_state.borrow_mut();
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
                let sample_time = state.timeline.instant(stamp, now);
                let velocity = state.velocity_tracker.velocity_at(sample_time);
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
        self.finish(step, notices);
    }

    fn handle_cancel(&self) {
        self.cancel();
    }
}

impl GestureRecognizer for TapAndDragGestureRecognizer {
    fn add_pointer(&self, down: PointerDispatch<'_>) {
        if !is_primary_down(down.local) {
            return;
        }
        let PointerEvent::Down(data) = down.local else {
            return;
        };
        let (Some(pointer), Some(position), Some(global_position)) = (
            down.local.pointer_id(),
            down.local.position(),
            down.global.position(),
        ) else {
            return;
        };
        // per-impl span (trait fn disallows `#[instrument]`).
        let _span = tracing::info_span!(
            "tap_and_drag.add_pointer",
            pointer = ?pointer,
            event = %crate::observability::GestureEvent::RecognizerAdded,
        );
        let prospective_settings = self.settings.snapshot();
        let now = self.contact.now();
        let continuation = self
            .gesture_state
            .borrow()
            .last_tap
            .as_ref()
            .filter(|last| {
                if last.kind != data.pointer.kind || last.device != data.pointer.device {
                    return false;
                }
                let origin = if last.settings.double_tap_uses_down_time(last.kind) {
                    last.down_time
                } else {
                    last.up_time
                };
                now.saturating_duration_since(origin)
                    <= last.settings.double_tap_timeout_for(last.kind)
                    && !last
                        .settings
                        .exceeds_double_tap_slop(last.kind, position - last.down_position)
            })
            .cloned();
        let settings = continuation
            .as_ref()
            .map_or(prospective_settings, |last| last.settings.clone());
        if self.contact.begin(down, &settings).is_err() {
            return;
        }
        let entry = self.contact.entry();
        let mut state = self.gesture_state.borrow_mut();
        let count = continuation.map_or(1, |last| last.count.saturating_add(1));
        state.reset_sequence();
        state.settings = settings;
        state.down_time = Some(now);
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
        if let PointerEvent::Down(data) = down.local {
            state.kind = data.pointer.kind;
            state.device = data.pointer.device;
        }
        state.velocity_tracker =
            VelocityTracker::for_gesture(state.kind, state.settings.velocity_estimator());
        state.last = position;
        state.last_global = global_position;
        state.last_reported = position;
        // Until an event carries a timestamp, samples use the arena clock.
        let sample_time = state.timeline.instant(event_time(down.local), now);
        state.velocity_tracker.add_position(sample_time, position);
    }

    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        let event = dispatch.local;
        // per-impl span (trait fn disallows `#[instrument]`).
        let _span = tracing::info_span!(
            "tap_and_drag.handle_event",
            kind = %crate::observability::pointer_event_kind(event),
            event = %crate::observability::GestureEvent::EventReceived,
        );
        if self.gesture_state.borrow().pointer != event.pointer_id() {
            return;
        }
        // Read once, here: this is the only point at which the untransformed
        // position is available at all (issue #908).
        let global_position = dispatch
            .global
            .position()
            .unwrap_or_else(|| self.gesture_state.borrow().last_global);

        match event {
            PointerEvent::Down(data) => {
                let now = self.contact.now();
                let mut state = self.gesture_state.borrow_mut();
                state.kind = data.pointer.kind;
                state.timeline.instant(event_time(event), now);
            }
            PointerEvent::Move(data) => {
                let pos = data.current().position.get();
                let history = motion_history(event);
                self.handle_move(
                    Offset::new(pos.x, pos.y),
                    global_position,
                    event_time(event),
                    history,
                );
            }
            PointerEvent::Up(data) => {
                let pos = data.sample.position.get();
                self.handle_up(
                    Offset::new(pos.x, pos.y),
                    global_position,
                    event_time(event),
                );
            }
            PointerEvent::Cancel(_) => self.handle_cancel(),
            _ => {}
        }
    }

    fn cancel(&self) -> CancelOutcome {
        if self.contact.current().is_none() {
            return CancelOutcome::Idle;
        }
        let mut notices = Vec::new();
        self.gesture_state.borrow_mut().abandon(&mut notices);
        let mut first = RoutePanic::capture(|| {
            self.contact.cancel();
        });
        RoutePanic::preserve_first(
            &mut first,
            RoutePanic::capture(|| self.finish(ArenaStep::None, notices)),
            "tap and drag cancel",
        );
        finish_containment(first, std::thread::panicking());
        CancelOutcome::Cancelled
    }
}

impl GestureArenaMember for TapAndDragGestureRecognizer {
    fn accept_gesture(&self, pointer: PointerId) {
        let mut notices = Vec::new();
        let mut state = self.gesture_state.borrow_mut();
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
        let mut state = self.gesture_state.borrow_mut();
        if state.pointer != Some(pointer) {
            return;
        }
        // The entry is already resolved; dropping it is all that is left.
        let _resolved = state.abandon(&mut notices);
        drop(state);
        self.finish(ArenaStep::None, notices);
    }
}
