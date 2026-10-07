//! Pressure-sensitive contact recognition with immutable owner-local configuration.
//!
//! A mouse never starts a force press. Other devices must report two different
//! nonzero pressures before crossing the start threshold can claim the arena.
//! Cancellation ends an active press and permits reuse; dropping the owner is silent.

use std::{cell::RefCell, rc::Rc};

use flui_foundation::geometry::Offset;

use super::{
    callback_containment::{finish_containment, invoke_callback, retire_callback},
    contact::{ArenaMembership, PrimaryContact},
    recognizer::{CancelOutcome, GestureRecognizer, is_primary_down},
};
use crate::{
    ForcePressDetails,
    arena::{GestureArena, GestureArenaEntry, GestureArenaMember, GestureDisposition},
    events::{PointerEvent, PointerType},
    ids::PointerId,
    routing::{PointerDispatch, RoutePanic},
    settings::GestureSettings,
    traits::PointerEventExtTrait,
};

/// Default pressure needed to start a force press.
pub const FORCE_PRESS_START_PRESSURE: f64 = 0.4;
/// Default pressure needed to report peak force.
pub const FORCE_PRESS_PEAK_PRESSURE: f64 = 0.85;
/// Callback for a force press starting.
pub type ForcePressStartCallback = Rc<dyn Fn(ForcePressDetails)>;
/// Callback for a pressure update.
pub type ForcePressUpdateCallback = Rc<dyn Fn(ForcePressDetails)>;
/// Callback for reaching peak force.
pub type ForcePressPeakCallback = Rc<dyn Fn(ForcePressDetails)>;
/// Callback for an active force press ending.
pub type ForcePressEndCallback = Rc<dyn Fn(ForcePressDetails)>;

/// Recognizes sensor-backed pressure gestures after winning their contact arena.
///
/// Configure callbacks before `build`. A callback referring to its own owner
/// should capture `Weak` to avoid an ownership cycle. Callbacks run without state
/// borrows, and can cancel or begin another contact through the same handle.
pub struct ForcePressGestureRecognizer {
    contact: PrimaryContact,
    gesture_state: RefCell<ForcePressState>,
    settings: GestureSettings,
    thresholds: Thresholds,
    callbacks: ForcePressCallbacks,
}

#[derive(Debug, Clone, Copy)]
struct Thresholds {
    start: f64,
    peak: f64,
}

#[expect(clippy::struct_field_names)]
#[derive(Default)]
struct ForcePressCallbacks {
    on_start: Option<ForcePressStartCallback>,
    on_update: Option<ForcePressUpdateCallback>,
    on_peak: Option<ForcePressPeakCallback>,
    on_end: Option<ForcePressEndCallback>,
}

impl Drop for ForcePressCallbacks {
    fn drop(&mut self) {
        let mut first = None;
        retire_callback(self.on_start.take(), &mut first);
        retire_callback(self.on_update.take(), &mut first);
        retire_callback(self.on_peak.take(), &mut first);
        retire_callback(self.on_end.take(), &mut first);
        finish_containment(first, std::thread::panicking());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ForcePressPhase {
    #[default]
    Ready,
    Possible,
    Claiming,
    Started,
    Peaked,
}

#[derive(Debug, Default)]
struct ForcePressState {
    phase: ForcePressPhase,
    won: bool,
    sensor: bool,
    first_reading: Option<f64>,
    position: Offset<f64>,
    global_position: Offset<f64>,
    pressure: f64,
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

    fn record_pressure(&mut self, pressure: f64, kind: PointerType) -> bool {
        if !pressure.is_finite() {
            return false;
        }
        if kind != PointerType::Mouse && pressure != 0.0 {
            match self.first_reading {
                None => self.first_reading = Some(pressure),
                Some(first) if first != pressure => self.sensor = true,
                Some(_) => {}
            }
        }
        self.pressure = pressure;
        true
    }

    fn start(&mut self, peak: f64, notices: &mut Vec<Notice>) {
        self.phase = ForcePressPhase::Started;
        notices.push(Notice::Start(self.details()));
        if self.pressure >= peak {
            self.phase = ForcePressPhase::Peaked;
            notices.push(Notice::Peak(self.details()));
        }
    }

    fn retire(&mut self, notices: &mut Vec<Notice>) {
        if self.is_active() {
            notices.push(Notice::End(self.details()));
        }
        *self = Self::default();
    }
}

enum Notice {
    Start(ForcePressDetails),
    Update(ForcePressDetails),
    Peak(ForcePressDetails),
    End(ForcePressDetails),
}

enum ArenaStep {
    None,
    Claim(GestureArenaEntry),
    Withdraw,
    Abandon,
    Finish,
}

/// Immutable configuration for a shared force-press recognizer.
#[must_use]
pub struct ForcePressGestureRecognizerBuilder {
    arena: GestureArena,
    settings: GestureSettings,
    thresholds: Thresholds,
    callbacks: ForcePressCallbacks,
}

impl std::fmt::Debug for ForcePressGestureRecognizerBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ForcePressGestureRecognizerBuilder")
            .field("settings", &self.settings)
            .field("thresholds", &self.thresholds)
            .finish_non_exhaustive()
    }
}

impl ForcePressGestureRecognizerBuilder {
    /// Freeze the gesture policy used by each admitted contact.
    pub fn settings(mut self, settings: GestureSettings) -> Self {
        self.settings = settings;
        self
    }
    /// Set the start threshold, clamped to `0..=1`; ignore nonfinite values.
    pub fn start_pressure(mut self, pressure: f64) -> Self {
        if pressure.is_finite() {
            self.thresholds.start = pressure.clamp(0.0, 1.0);
        }
        self
    }
    /// Set the peak threshold, clamped to `0..=1`; ignore nonfinite values.
    pub fn peak_pressure(mut self, pressure: f64) -> Self {
        if pressure.is_finite() {
            self.thresholds.peak = pressure.clamp(0.0, 1.0);
        }
        self
    }
    /// Configure the callback delivered after winning the arena.
    pub fn on_start(mut self, callback: impl Fn(ForcePressDetails) + 'static) -> Self {
        self.callbacks.on_start = Some(Rc::new(callback));
        self
    }
    /// Configure the callback delivered for active pressure samples.
    pub fn on_update(mut self, callback: impl Fn(ForcePressDetails) + 'static) -> Self {
        self.callbacks.on_update = Some(Rc::new(callback));
        self
    }
    /// Configure the callback delivered upon first reaching peak pressure.
    pub fn on_peak(mut self, callback: impl Fn(ForcePressDetails) + 'static) -> Self {
        self.callbacks.on_peak = Some(Rc::new(callback));
        self
    }
    /// Configure the callback delivered when an active press ends.
    pub fn on_end(mut self, callback: impl Fn(ForcePressDetails) + 'static) -> Self {
        self.callbacks.on_end = Some(Rc::new(callback));
        self
    }
    /// Build the owner; arena membership retains only a weak reference to it.
    pub fn build(self) -> Rc<ForcePressGestureRecognizer> {
        Rc::new_cyclic(|this: &std::rc::Weak<ForcePressGestureRecognizer>| {
            let member: std::rc::Weak<dyn GestureArenaMember> = this.clone();
            ForcePressGestureRecognizer {
                contact: PrimaryContact::new(ArenaMembership::new(self.arena, member)),
                gesture_state: RefCell::new(ForcePressState::default()),
                settings: self.settings,
                thresholds: self.thresholds,
                callbacks: self.callbacks,
            }
        })
    }
}

impl ForcePressGestureRecognizer {
    /// Assemble immutable pressure policy and callbacks before sharing the owner.
    pub fn builder(arena: GestureArena) -> ForcePressGestureRecognizerBuilder {
        ForcePressGestureRecognizerBuilder {
            arena,
            settings: GestureSettings::default(),
            thresholds: Thresholds {
                start: FORCE_PRESS_START_PRESSURE,
                peak: FORCE_PRESS_PEAK_PRESSURE,
            },
            callbacks: ForcePressCallbacks::default(),
        }
    }

    fn finish(&self, step: ArenaStep, notices: Vec<Notice>) {
        let id = self.contact.current().map(|snapshot| snapshot.id);
        let ends_sequence = notices
            .iter()
            .any(|notice| matches!(notice, Notice::End(_)));
        let mut first = RoutePanic::capture(|| match step {
            ArenaStep::None => {}
            ArenaStep::Claim(entry) => entry.resolve(GestureDisposition::Accepted),
            ArenaStep::Withdraw => {
                self.contact.withdraw();
            }
            ArenaStep::Abandon => {
                self.contact.cancel();
            }
            ArenaStep::Finish => {
                self.contact.finish();
            }
        });
        for notice in notices {
            if !ends_sequence && !id.is_some_and(|id| self.contact.is_current(id)) {
                break;
            }
            RoutePanic::preserve_first(
                &mut first,
                RoutePanic::capture(|| self.deliver(notice)),
                "force press callback",
            );
        }
        finish_containment(first, std::thread::panicking());
    }

    fn deliver(&self, notice: Notice) {
        let (callback, details) = match notice {
            Notice::Start(details) => (self.callbacks.on_start.clone(), details),
            Notice::Update(details) => (self.callbacks.on_update.clone(), details),
            Notice::Peak(details) => (self.callbacks.on_peak.clone(), details),
            Notice::End(details) => (self.callbacks.on_end.clone(), details),
        };
        invoke_callback(callback, || {}, |callback| callback(details));
    }

    fn handle_sample(&self, position: Offset<f64>, global: Offset<f64>, pressure: f64) {
        let Some(contact) = self.contact.current() else {
            return;
        };
        let entry = self.contact.entry();
        let mut notices = Vec::new();
        let mut state = self.gesture_state.borrow_mut();
        if state.phase == ForcePressPhase::Ready {
            return;
        }
        if position.is_finite()
            && (position - contact.local).distance() > contact.settings.hit_slop(contact.kind)
        {
            state.retire(&mut notices);
            drop(state);
            self.finish(ArenaStep::Withdraw, notices);
            return;
        }
        if position.is_finite() {
            state.position = position;
        }
        if global.is_finite() {
            state.global_position = global;
        }
        if !state.record_pressure(pressure, contact.kind) {
            return;
        }
        let mut step = ArenaStep::None;
        match state.phase {
            ForcePressPhase::Claiming if state.pressure < self.thresholds.start => {
                state.retire(&mut notices);
                step = ArenaStep::Withdraw;
            }
            ForcePressPhase::Possible
                if state.sensor && state.pressure >= self.thresholds.start =>
            {
                if state.won {
                    state.start(self.thresholds.peak, &mut notices);
                } else {
                    state.phase = ForcePressPhase::Claiming;
                    if let Some(entry) = entry {
                        step = ArenaStep::Claim(entry);
                    }
                }
            }
            ForcePressPhase::Started | ForcePressPhase::Peaked => {
                if state.pressure < self.thresholds.start {
                    state.retire(&mut notices);
                    step = ArenaStep::Withdraw;
                } else {
                    if state.phase == ForcePressPhase::Started
                        && state.pressure >= self.thresholds.peak
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

    fn end(&self, dispatch: Option<PointerDispatch<'_>>, step: ArenaStep) {
        let mut notices = Vec::new();
        let mut state = self.gesture_state.borrow_mut();
        if let Some(dispatch) = dispatch {
            let position = dispatch.local.position();
            let global = dispatch.global.position();
            if position.is_finite() {
                state.position = position;
            }
            if global.is_finite() {
                state.global_position = global;
            }
        }
        state.pressure = 0.0;
        state.retire(&mut notices);
        drop(state);
        self.finish(step, notices);
    }
}

impl GestureRecognizer for ForcePressGestureRecognizer {
    fn add_pointer(&self, down: PointerDispatch<'_>) {
        if !is_primary_down(down.local) {
            return;
        }
        if self.contact.begin(down, &self.settings).is_err() {
            return;
        }
        let Some(contact) = self.contact.current() else {
            return;
        };
        *self.gesture_state.borrow_mut() = ForcePressState {
            phase: ForcePressPhase::Possible,
            position: contact.local,
            global_position: contact.global,
            ..ForcePressState::default()
        };
        if let PointerEvent::Down(data) = down.local {
            self.handle_sample(
                contact.local,
                contact.global,
                f64::from(data.state.pressure),
            );
        }
    }

    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        if !self.contact.tracks(dispatch.local.pointer_id()) {
            return;
        }
        match dispatch.local {
            PointerEvent::Down(data) => self.handle_sample(
                dispatch.local.position(),
                dispatch.global.position(),
                f64::from(data.state.pressure),
            ),
            PointerEvent::Move(data) => self.handle_sample(
                dispatch.local.position(),
                dispatch.global.position(),
                f64::from(data.current.pressure),
            ),
            PointerEvent::Up(_) => self.end(Some(dispatch), ArenaStep::Finish),
            PointerEvent::Cancel(_) => {
                self.cancel();
            }
            _ => {}
        }
    }

    fn cancel(&self) -> CancelOutcome {
        if self.contact.current().is_none() {
            return CancelOutcome::Idle;
        }
        self.end(None, ArenaStep::Abandon);
        CancelOutcome::Cancelled
    }
}

impl GestureArenaMember for ForcePressGestureRecognizer {
    fn accept_gesture(&self, pointer: PointerId) {
        if !self.contact.tracks(pointer) {
            return;
        }
        let mut notices = Vec::new();
        let mut state = self.gesture_state.borrow_mut();
        state.won = true;
        if state.phase == ForcePressPhase::Claiming {
            state.start(self.thresholds.peak, &mut notices);
        }
        drop(state);
        self.finish(ArenaStep::None, notices);
    }

    fn reject_gesture(&self, pointer: PointerId) {
        if !self.contact.tracks(pointer) {
            return;
        }
        let mut notices = Vec::new();
        self.gesture_state.borrow_mut().retire(&mut notices);
        self.finish(ArenaStep::Withdraw, notices);
    }
}

impl std::fmt::Debug for ForcePressGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ForcePressGestureRecognizer")
            .field("gesture_state", &self.gesture_state.borrow())
            .field("settings", &self.settings)
            .field("thresholds", &self.thresholds)
            .finish_non_exhaustive()
    }
}
