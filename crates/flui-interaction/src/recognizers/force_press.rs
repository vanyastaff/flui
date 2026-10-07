//! Force press gesture recognizer
//!
//! Recognizes force press (3D Touch / Force Touch / stylus) gestures based on
//! pressure.
//!
//! A force press is defined as:
//! - a contact from hardware that reports real pressure (see
//!   [`ForcePressGestureRecognizer`] for how a sensor is told apart from the
//!   sensor-less `0.5`);
//! - pressure rising past the start threshold (0.4 by default), which claims
//!   the gesture arena; the press starts once the arena accepts it;
//! - optional pressure updates as the contact presses harder or softer;
//! - an end when pressure falls below the start threshold, the contact
//!   drifts past slop, lifts, or is cancelled.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use crate::ForcePressDetails;
use flui_foundation::geometry::Offset;
use parking_lot::Mutex;

use super::{
    recognizer::{GestureRecognizer, RecognizerBase},
    recognizer::{finish_containment, invoke_callback, retire_callback},
};
use crate::{
    arena::{GestureArenaEntry, GestureArenaMember, GestureDisposition, SweepModel},
    events::{PointerEvent, PointerType},
    ids::PointerId,
    routing::{PointerDispatch, RoutePanic},
    settings::GestureSettings,
    traits::PointerEventExtTrait,
};

/// Default pressure threshold to start force press (40%)
pub const FORCE_PRESS_START_PRESSURE: f64 = 0.4;

/// Default pressure threshold for peak force press (85%)
pub const FORCE_PRESS_PEAK_PRESSURE: f64 = 0.85;

/// Callback for force press start events
pub type ForcePressStartCallback = Rc<dyn Fn(ForcePressDetails)>;

/// Callback for force press update events
pub type ForcePressUpdateCallback = Rc<dyn Fn(ForcePressDetails)>;

/// Callback for force press peak events
pub type ForcePressPeakCallback = Rc<dyn Fn(ForcePressDetails)>;

/// Callback for force press end events
pub type ForcePressEndCallback = Rc<dyn Fn(ForcePressDetails)>;

/// Recognizes force press gestures based on pressure sensitivity.
///
/// # Which devices can force press
///
/// `PointerEvent` carries a normalized pressure but no sensor range, and a
/// sensor-less device reports a constant while pressed: `0.5` on the W3C
/// convention, `1.0` for an Android touch or mouse. A force press therefore
/// starts only after the contact has reported two different non-zero
/// pressures, which only a real sensor produces, and never for a mouse.
/// Non-finite pressure samples are ignored.
///
/// # Arena
///
/// Crossing the start threshold claims the gesture arena; `on_start` fires
/// only once the arena has accepted this recognizer, so a loser never emits
/// it. A contact that lifts, drifts past slop or falls back before starting
/// withdraws from the arena, so a competing tap can still win it.
///
/// # Pressure Thresholds
///
/// - **Start threshold** (default 0.4): Pressure level to begin force press
/// - **Peak threshold** (default 0.85): Pressure level for "peak" callback
///
/// Callbacks run after the recognizer has committed its state, with no
/// borrow held: a callback may dispose the recognizer. A panicking callback
/// propagates to the dispatcher and the next press starts clean.
///
/// # Example
///
/// ```rust,ignore
/// use flui_interaction::prelude::*;
///
/// let recognizer = ForcePressGestureRecognizer::new(binding.arena().clone())
///     .with_on_start(|details| println!("Force press at {:?}", details.global_position))
///     .with_on_peak(|details| println!("Peak pressure {}", details.pressure));
/// ```
#[derive(Clone)]
pub struct ForcePressGestureRecognizer {
    /// Base state (arena, tracking, etc.)
    state: RecognizerBase,

    /// Callbacks
    callbacks: Rc<RefCell<ForcePressCallbacks>>,

    /// Current gesture state
    gesture_state: Arc<Mutex<ForcePressState>>,

    /// Gesture settings (device-specific tolerances)
    settings: Arc<Mutex<GestureSettings>>,

    /// Start and peak pressure thresholds, shared by every clone so a
    /// builder call never forks the recognizer's identity.
    thresholds: Arc<Mutex<Thresholds>>,
}

#[derive(Debug, Clone, Copy)]
struct Thresholds {
    start: f64,
    peak: f64,
}

// Field names keep the `on_start`/`on_update`-style callback names.
#[expect(clippy::struct_field_names)]
#[derive(Default)]
struct ForcePressCallbacks {
    on_start: Option<ForcePressStartCallback>,
    on_update: Option<ForcePressUpdateCallback>,
    on_peak: Option<ForcePressPeakCallback>,
    on_end: Option<ForcePressEndCallback>,
}

impl ForcePressCallbacks {
    fn retire(self, first: &mut Option<RoutePanic>) {
        retire_callback(self.on_start, first);
        retire_callback(self.on_update, first);
        retire_callback(self.on_peak, first);
        retire_callback(self.on_end, first);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ForcePressPhase {
    /// No contact is tracked.
    Ready,
    /// Contact down, below the start threshold or without sensor evidence.
    Possible,
    /// Start threshold crossed; the arena claim is pending.
    Claiming,
    /// Force press started.
    Started,
    /// Peak pressure reached.
    Peaked,
}

#[derive(Debug, Clone)]
struct ForcePressState {
    phase: ForcePressPhase,
    /// The tracked contact and its arena membership.
    pointer: Option<PointerId>,
    entry: Option<GestureArenaEntry>,
    /// The arena accepted this recognizer for the tracked contact.
    won: bool,
    /// The contact reported pressures only a real sensor produces.
    sensor: bool,
    /// The contact's first non-zero pressure, compared against later ones.
    first_reading: Option<f64>,
    position: Offset<f64>,
    global_position: Offset<f64>,
    pressure: f64,
}

impl Default for ForcePressState {
    fn default() -> Self {
        Self {
            phase: ForcePressPhase::Ready,
            pointer: None,
            entry: None,
            won: false,
            sensor: false,
            first_reading: None,
            position: Offset::ZERO,
            global_position: Offset::ZERO,
            pressure: 0.0,
        }
    }
}

impl ForcePressState {
    fn details(&self) -> ForcePressDetails {
        ForcePressDetails::new(self.global_position, self.position, self.pressure, 1.0)
    }

    fn is_active(&self) -> bool {
        matches!(
            self.phase,
            ForcePressPhase::Started | ForcePressPhase::Peaked
        )
    }

    /// Record one pressure sample. A non-finite sample is ignored and reported
    /// as not admitted, so the caller makes no transition from it.
    ///
    /// A sensor is proven by a non-zero pressure different from the contact's
    /// first one: platforms give sensor-less contacts a constant (0.5 on the
    /// W3C convention, 1.0 on Android). A mouse is never a sensor.
    #[must_use]
    fn record_pressure(&mut self, pressure: f64, kind: Option<PointerType>) -> bool {
        if !pressure.is_finite() {
            return false;
        }
        if kind != Some(PointerType::Mouse) && pressure != 0.0 {
            match self.first_reading {
                None => self.first_reading = Some(pressure),
                Some(first) if first != pressure => self.sensor = true,
                Some(_) => {}
            }
        }
        self.pressure = pressure;
        true
    }

    /// Enter `Started`, plus `Peaked` when the pressure is already there.
    fn start(&mut self, peak: f64, out: &mut Vec<Notice>) {
        self.phase = ForcePressPhase::Started;
        out.push(Notice::Start(self.details()));
        if self.pressure >= peak {
            self.phase = ForcePressPhase::Peaked;
            out.push(Notice::Peak(self.details()));
        }
    }
}

/// A user callback to deliver once the state lock is released.
enum Notice {
    Start(ForcePressDetails),
    Update(ForcePressDetails),
    Peak(ForcePressDetails),
    End(ForcePressDetails),
}

/// Arena work to run once the state lock is released, before notices.
enum ArenaStep {
    None,
    Claim(GestureArenaEntry),
    Withdraw(GestureArenaEntry),
}

impl ForcePressGestureRecognizer {
    /// Create a new force press recognizer with gesture arena
    pub fn new(arena: crate::arena::GestureArena) -> Arc<Self> {
        Self::with_settings(arena, GestureSettings::default())
    }

    /// Create a new force press recognizer with custom settings
    pub fn with_settings(
        arena: crate::arena::GestureArena,
        settings: GestureSettings,
    ) -> Arc<Self> {
        Arc::new(Self {
            state: RecognizerBase::new(arena),
            callbacks: Rc::new(RefCell::new(ForcePressCallbacks::default())),
            gesture_state: Arc::new(Mutex::new(ForcePressState::default())),
            settings: Arc::new(Mutex::new(settings)),
            thresholds: Arc::new(Mutex::new(Thresholds {
                start: FORCE_PRESS_START_PRESSURE,
                peak: FORCE_PRESS_PEAK_PRESSURE,
            })),
        })
    }

    /// Get the current gesture settings
    pub fn settings(&self) -> GestureSettings {
        self.settings.lock().clone()
    }

    /// Update gesture settings
    pub fn set_settings(&self, settings: GestureSettings) {
        *self.settings.lock() = settings;
    }

    /// Set the start pressure threshold, clamped to `0.0..=1.0`.
    ///
    /// Default is 0.4 (40% of max pressure). A non-finite value is ignored.
    /// The threshold is shared by every handle to this recognizer.
    pub fn with_start_pressure(self: Arc<Self>, pressure: f64) -> Arc<Self> {
        if pressure.is_finite() {
            self.thresholds.lock().start = pressure.clamp(0.0, 1.0);
        }
        self
    }

    /// Set the peak pressure threshold, clamped to `0.0..=1.0`.
    ///
    /// Default is 0.85 (85% of max pressure). A non-finite value is ignored.
    /// A peak at or below the start threshold fires `on_peak` together with
    /// `on_start`.
    pub fn with_peak_pressure(self: Arc<Self>, pressure: f64) -> Arc<Self> {
        if pressure.is_finite() {
            self.thresholds.lock().peak = pressure.clamp(0.0, 1.0);
        }
        self
    }

    /// Set the force press start callback
    ///
    /// Called once the pressure has crossed the start threshold and the arena
    /// accepted this recognizer.
    pub fn with_on_start(
        self: Arc<Self>,
        callback: impl Fn(ForcePressDetails) + 'static,
    ) -> Arc<Self> {
        let old = self
            .callbacks
            .borrow_mut()
            .on_start
            .replace(Rc::new(callback));
        drop(old);
        self
    }

    /// Set the force press update callback
    ///
    /// Called for each pressure sample while the force press is active.
    pub fn with_on_update(
        self: Arc<Self>,
        callback: impl Fn(ForcePressDetails) + 'static,
    ) -> Arc<Self> {
        let old = self
            .callbacks
            .borrow_mut()
            .on_update
            .replace(Rc::new(callback));
        drop(old);
        self
    }

    /// Set the force press peak callback
    ///
    /// Called once when pressure first reaches the peak threshold.
    pub fn with_on_peak(
        self: Arc<Self>,
        callback: impl Fn(ForcePressDetails) + 'static,
    ) -> Arc<Self> {
        let old = self
            .callbacks
            .borrow_mut()
            .on_peak
            .replace(Rc::new(callback));
        drop(old);
        self
    }

    /// Set the force press end callback
    ///
    /// Called when a started press ends: pressure falls below the start
    /// threshold, the contact drifts past slop, lifts, or is cancelled.
    pub fn with_on_end(
        self: Arc<Self>,
        callback: impl Fn(ForcePressDetails) + 'static,
    ) -> Arc<Self> {
        let old = self
            .callbacks
            .borrow_mut()
            .on_end
            .replace(Rc::new(callback));
        drop(old);
        self
    }

    /// Get the current start pressure threshold
    pub fn start_pressure(&self) -> f64 {
        self.thresholds.lock().start
    }

    /// Get the current peak pressure threshold
    pub fn peak_pressure(&self) -> f64 {
        self.thresholds.lock().peak
    }

    /// Run the arena step, then deliver each notice. The first panic is
    /// resumed after everything ran; the state was committed beforehand.
    fn finish(&self, step: ArenaStep, notices: Vec<Notice>) {
        let self_driven = self.state.arena().sweep_model() == SweepModel::SelfDriven;
        let mut first = None;
        match step {
            ArenaStep::None => {}
            ArenaStep::Claim(entry) => RoutePanic::preserve_first(
                &mut first,
                RoutePanic::capture(|| entry.resolve(GestureDisposition::Accepted)),
                "force press arena claim",
            ),
            ArenaStep::Withdraw(entry) => RoutePanic::preserve_first(
                &mut first,
                RoutePanic::capture(|| {
                    entry.resolve(GestureDisposition::Rejected);
                    if self_driven {
                        entry.sweep();
                    }
                }),
                "force press arena withdrawal",
            ),
        }
        // Every notice of a committed transition is delivered: a panic in
        // `on_start` must not leave the caller with a start and no end. The
        // first failure stays authoritative and resumes after the rest.
        for notice in notices {
            RoutePanic::preserve_first(
                &mut first,
                RoutePanic::capture(|| self.deliver(notice)),
                "force press callback",
            );
        }
        // Entered while the thread is already unwinding, the failure is retained
        // rather than resumed: a second unwind would abort.
        finish_containment(first, std::thread::panicking());
    }

    fn deliver(&self, notice: Notice) {
        let callbacks = self.callbacks.borrow();
        let (callback, details) = match notice {
            Notice::Start(d) => (callbacks.on_start.clone(), d),
            Notice::Update(d) => (callbacks.on_update.clone(), d),
            Notice::Peak(d) => (callbacks.on_peak.clone(), d),
            Notice::End(d) => (callbacks.on_end.clone(), d),
        };
        drop(callbacks);
        invoke_callback(callback, || {}, |cb| cb(details));
    }

    /// End the tracked sequence: reset the state, clear the base's tracking
    /// and return the entry to withdraw plus the end notice owed.
    fn retire_sequence(&self, state: &mut ForcePressState, out: &mut Vec<Notice>) -> ArenaStep {
        if state.is_active() {
            out.push(Notice::End(state.details()));
        }
        let entry = state.entry.take();
        *state = ForcePressState::default();
        self.state.set_primary_pointer(None);
        self.state.clear_initial_contact();
        entry.map_or(ArenaStep::None, ArenaStep::Withdraw)
    }

    /// One pressure (and position) sample for the tracked contact.
    fn handle_sample(
        &self,
        position: Offset<f64>,
        global_position: Offset<f64>,
        pressure: f64,
        kind: Option<PointerType>,
    ) {
        let thresholds = *self.thresholds.lock();
        let slop = kind.map(|kind| self.settings.lock().hit_slop(kind));
        let mut notices = Vec::new();
        let mut state = self.gesture_state.lock();
        if state.phase == ForcePressPhase::Ready {
            return;
        }
        // Drift past slop ends the press (or forfeits a press not started).
        if let (Some(slop), Some(initial)) = (slop, self.state.initial_position())
            && position.is_finite()
            && (position - initial).distance() > slop
        {
            let step = self.retire_sequence(&mut state, &mut notices);
            drop(state);
            self.finish(step, notices);
            return;
        }
        if position.is_finite() {
            state.position = position;
        }
        if global_position.is_finite() {
            state.global_position = global_position;
        }
        if !state.record_pressure(pressure, kind) {
            // An ignored sample makes no transition: no update or peak from a
            // stale pressure paired with the new position.
            return;
        }

        let mut step = ArenaStep::None;
        match state.phase {
            // A pressure fall before the claim is accepted withdraws it, as a
            // fall before the start does.
            ForcePressPhase::Claiming if state.pressure < thresholds.start => {
                step = self.retire_sequence(&mut state, &mut notices);
            }
            ForcePressPhase::Possible if state.sensor && state.pressure >= thresholds.start => {
                if state.won {
                    state.start(thresholds.peak, &mut notices);
                } else {
                    state.phase = ForcePressPhase::Claiming;
                    if let Some(entry) = state.entry.clone() {
                        step = ArenaStep::Claim(entry);
                    }
                }
            }
            ForcePressPhase::Started | ForcePressPhase::Peaked => {
                if state.pressure < thresholds.start {
                    step = self.retire_sequence(&mut state, &mut notices);
                } else {
                    if state.phase == ForcePressPhase::Started && state.pressure >= thresholds.peak
                    {
                        state.phase = ForcePressPhase::Peaked;
                        notices.push(Notice::Peak(state.details()));
                    }
                    notices.push(Notice::Update(state.details()));
                }
            }
            _ => {}
        }
        drop(state);
        self.finish(step, notices);
    }

    /// The tracked contact lifted or was cancelled.
    fn handle_release(&self, position: Option<Offset<f64>>, global_position: Option<Offset<f64>>) {
        let mut notices = Vec::new();
        let mut state = self.gesture_state.lock();
        if state.phase == ForcePressPhase::Ready {
            return;
        }
        if let Some(position) = position.filter(|p| p.is_finite()) {
            state.position = position;
        }
        if let Some(global) = global_position.filter(|p| p.is_finite()) {
            state.global_position = global;
        }
        state.pressure = 0.0;
        let step = self.retire_sequence(&mut state, &mut notices);
        drop(state);
        self.finish(step, notices);
    }
}

impl GestureRecognizer for ForcePressGestureRecognizer {
    fn add_pointer(
        self: &Arc<Self>,
        pointer: PointerId,
        position: Offset<f64>,
        global_position: Offset<f64>,
    ) {
        if !self.state.assert_not_disposed("add_pointer") || !position.is_finite() {
            return;
        }
        // A non-finite global position is never published: the local one
        // stands in until a finite sample arrives.
        let global_position = if global_position.is_finite() {
            global_position
        } else {
            position
        };
        // One contact at a time: a second contact while one is tracked does
        // not join. The same pointer going down again means its previous
        // sequence never saw its end, so that sequence is retired first.
        let tracked = self.gesture_state.lock().pointer;
        match tracked {
            Some(current) if current != pointer => return,
            Some(_) => self.handle_release(None, None),
            None => {}
        }
        // The retired sequence's `on_end` may have disposed this recognizer or
        // admitted a contact of its own; either way this admission is void.
        if self.state.is_disposed() || self.gesture_state.lock().pointer.is_some() {
            return;
        }
        self.state
            .start_tracking(pointer, position, global_position, self);
        let mut state = self.gesture_state.lock();
        *state = ForcePressState {
            phase: ForcePressPhase::Possible,
            pointer: Some(pointer),
            entry: self.state.tracked_entry(),
            position,
            global_position,
            ..ForcePressState::default()
        };
    }

    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        let event = dispatch.local;
        if !self.state.assert_not_disposed("handle_event") {
            return;
        }
        let tracked = self.gesture_state.lock().pointer;
        if tracked != Some(event.pointer_id()) {
            return;
        }
        let global_position = dispatch.global.position();
        match event {
            PointerEvent::Down(data) => {
                let pos = data.state.position;
                self.handle_sample(
                    Offset::new(pos.x, pos.y),
                    global_position,
                    f64::from(data.state.pressure),
                    Some(data.pointer.pointer_type),
                );
            }
            PointerEvent::Move(data) => {
                let pos = data.current.position;
                self.handle_sample(
                    Offset::new(pos.x, pos.y),
                    global_position,
                    f64::from(data.current.pressure),
                    Some(data.pointer.pointer_type),
                );
            }
            PointerEvent::Up(data) => {
                let pos = data.state.position;
                self.handle_release(Some(Offset::new(pos.x, pos.y)), Some(global_position));
            }
            PointerEvent::Cancel(_) => self.handle_release(None, None),
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
            *state = ForcePressState::default();
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

// =============================================================================
// Canonical trait hierarchy adoption
// =============================================================================
//
// A force press tracks a single pointer sequence.

impl crate::recognizers::OneSequenceGestureRecognizer for ForcePressGestureRecognizer {
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
            self.handle_release(None, None);
        }
    }
}

impl GestureArenaMember for ForcePressGestureRecognizer {
    fn accept_gesture(&self, pointer: PointerId) {
        let peak = self.thresholds.lock().peak;
        let mut notices = Vec::new();
        let mut state = self.gesture_state.lock();
        if state.pointer != Some(pointer) {
            return;
        }
        state.won = true;
        if state.phase == ForcePressPhase::Claiming {
            state.start(peak, &mut notices);
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
        let _resolved = self.retire_sequence(&mut state, &mut notices);
        drop(state);
        self.finish(ArenaStep::None, notices);
    }
}

impl std::fmt::Debug for ForcePressGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ForcePressGestureRecognizer")
            .field("state", &self.state)
            .field("gesture_state", &self.gesture_state.lock())
            .field("settings", &self.settings.lock())
            .field("thresholds", &self.thresholds.lock())
            .finish_non_exhaustive()
    }
}
