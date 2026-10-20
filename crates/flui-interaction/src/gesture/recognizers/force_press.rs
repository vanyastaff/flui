//! Pressure-sensitive contact recognition with immutable owner-local configuration.
//!
//! Recognition requires declared pressure support and an actual measured reading.
//! Device kind alone does not imply a pressure sensor; constant readings are valid.
//! Cancellation ends an active press and permits reuse; dropping the owner is silent.
//! Measured pressure history is processed in packet order before the current
//! reading. Predictions never trigger recognition. Reentrant contact replacement
//! revokes the remainder of the old packet; observer failures preserve the first
//! panic while allowing the still-current measured tail to finish.

use std::{cell::RefCell, rc::Rc};

use smallvec::SmallVec;

use web_time::Instant;

use flui_foundation::geometry::Offset;

use super::{
    callback_containment::{finish_containment, invoke_callback, retire_callback},
    contact::{ArenaMembership, ContactEnd, PrimaryContact},
    recognizer::{
        CancelOutcome, EventTimeline, GestureRecognizer, event_time, is_primary_down, slop_travel,
    },
};
use crate::gesture::observer::GestureContest;
use crate::{
    ForcePressDetails,
    containment::RoutePanic,
    events::{PointerEvent, PointerEventExt},
    gesture::PositionSmoother,
    gesture::arena::{GestureArena, GestureArenaEntry, GestureArenaMember, GestureDisposition},
    gesture::settings::GestureSettingsProvider,
    pointer::PointerDispatch,
};
use flui_platform_api::input_capabilities::PointerKindSet;

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
    settings: GestureSettingsProvider,
    allowed_devices: PointerKindSet,
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
    position: Offset<f64>,
    global_position: Offset<f64>,
    pressure: f64,
    timeline: EventTimeline,
    /// Event-clock time of the latest sample.
    sample_time: Option<Instant>,
    /// Post-recognition smoothing of updates, captured with the settings.
    smoother: Option<PositionSmoother>,
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

    /// Details for an update or peak after the start: the position is
    /// smoothed when the settings select it; pressure never is.
    fn published_details(&mut self) -> ForcePressDetails {
        let (position, global) = match (self.smoother.as_mut(), self.sample_time) {
            (Some(smoother), Some(time)) => {
                smoother.smooth(time, self.position, self.global_position)
            }
            _ => (self.position, self.global_position),
        };
        ForcePressDetails::new(global, position, self.pressure, 1.0)
    }

    fn start(&mut self, peak: f64, notices: &mut Notices) {
        self.phase = ForcePressPhase::Started;
        // Updates smooth from the position the press started at.
        if let (Some(smoother), Some(time)) = (self.smoother.as_mut(), self.sample_time) {
            smoother.smooth(time, self.position, self.global_position);
        }
        notices.push(Notice::Start(self.details()));
        if self.pressure >= peak {
            self.phase = ForcePressPhase::Peaked;
            notices.push(Notice::Peak(self.details()));
        }
    }

    fn retire(&mut self, notices: &mut Notices) {
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

/// One sample's notices: at most a peak and its update, or an end. Inline, so
/// a pen's coalesced pressure history allocates nothing per sample.
type Notices = SmallVec<[Notice; 2]>;

enum ArenaStep {
    None,
    Claim(Box<GestureArenaEntry>),
    Withdraw,
    Abandon,
    Finish(ContactEnd),
}

/// Immutable configuration for a shared force-press recognizer.
#[must_use]
pub struct ForcePressGestureRecognizerBuilder {
    arena: GestureArena,
    settings: GestureSettingsProvider,
    allowed_devices: PointerKindSet,
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
    pub fn settings(mut self, settings: impl Into<GestureSettingsProvider>) -> Self {
        self.settings = settings.into();
        self
    }

    /// Admit only contacts of these device kinds, before any arena rule runs
    /// (ADR-0208 rule 1): a pen that draws beside a finger that scrolls. The
    /// full set, the default, admits every device.
    pub fn allowed_devices(mut self, devices: PointerKindSet) -> Self {
        self.allowed_devices = devices;
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
                allowed_devices: self.allowed_devices,
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
            settings: GestureSettingsProvider::default(),
            allowed_devices: PointerKindSet::all(),
            thresholds: Thresholds {
                start: FORCE_PRESS_START_PRESSURE,
                peak: FORCE_PRESS_PEAK_PRESSURE,
            },
            callbacks: ForcePressCallbacks::default(),
        }
    }

    fn finish(&self, step: ArenaStep, notices: Notices) {
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
            ArenaStep::Finish(end) => {
                self.contact.finish(end);
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

    fn handle_sample(
        &self,
        position: Offset<f64>,
        global: Offset<f64>,
        pressure: Option<f64>,
        stamp: Option<u64>,
    ) {
        let Some(contact) = self.contact.current() else {
            return;
        };
        let entry = self.contact.entry();
        let clock = self.contact.arena().sample_now();
        let mut notices = Notices::new();
        let mut state = self.gesture_state.borrow_mut();
        if state.phase == ForcePressPhase::Ready {
            return;
        }
        let time = state.timeline.instant(stamp, clock);
        state.sample_time = Some(time);
        if position.is_finite()
            && contact.settings.exceeds_hit_slop(
                contact.kind,
                slop_travel(position - contact.local, global - contact.global),
            )
        {
            state.retire(&mut notices);
            drop(state);
            self.finish(ArenaStep::Withdraw, notices);
            return;
        }
        // Missing sensor data can still end a contact through motion, but it
        // cannot move the coordinates attached to the last measured pressure.
        let Some(pressure) = pressure else {
            return;
        };
        if position.is_finite() {
            state.position = position;
        }
        if global.is_finite() {
            state.global_position = global;
        }
        state.pressure = pressure;
        let mut step = ArenaStep::None;
        match state.phase {
            ForcePressPhase::Claiming if state.pressure < self.thresholds.start => {
                state.retire(&mut notices);
                step = ArenaStep::Withdraw;
            }
            ForcePressPhase::Possible if state.pressure >= self.thresholds.start => {
                if state.won {
                    state.start(self.thresholds.peak, &mut notices);
                } else {
                    state.phase = ForcePressPhase::Claiming;
                    if let Some(entry) = entry {
                        step = ArenaStep::Claim(Box::new(entry));
                    }
                }
            }
            ForcePressPhase::Started | ForcePressPhase::Peaked => {
                if state.pressure < self.thresholds.start {
                    state.retire(&mut notices);
                    step = ArenaStep::Withdraw;
                } else if state.phase == ForcePressPhase::Started
                    && state.pressure >= self.thresholds.peak
                {
                    // One smoothed sample: the peak and its update agree.
                    state.phase = ForcePressPhase::Peaked;
                    let details = state.published_details();
                    notices.push(Notice::Peak(details.clone()));
                    notices.push(Notice::Update(details));
                } else {
                    notices.push(Notice::Update(state.published_details()));
                }
            }
            _ => {}
        }
        drop(state);
        self.finish(step, notices);
    }

    fn end(&self, dispatch: Option<PointerDispatch<'_>>, step: ArenaStep) {
        let mut notices = Notices::new();
        let mut state = self.gesture_state.borrow_mut();
        // A press that never reached its start pressure, or a device without
        // pressure, recognized nothing: it leaves the contest at release.
        let step = match step {
            ArenaStep::Finish(_)
                if !state.won
                    && !matches!(
                        state.phase,
                        ForcePressPhase::Claiming
                            | ForcePressPhase::Started
                            | ForcePressPhase::Peaked
                    ) =>
            {
                ArenaStep::Finish(ContactEnd::Withdrawn)
            }
            step => step,
        };
        if let Some(dispatch) = dispatch {
            if let Some(position) = dispatch.local.position() {
                state.position = position;
            }
            if let Some(global) = dispatch.global.position() {
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
    fn allowed_devices(&self) -> PointerKindSet {
        self.allowed_devices
    }
    fn add_pointer(&self, down: PointerDispatch<'_>) {
        if !is_primary_down(down.local) {
            return;
        }
        let settings = self
            .settings
            .snapshot_for(super::recognizer::admission_kind(down.local));
        if self.contact.begin(down, &settings).is_err() {
            return;
        }
        let Some(contact) = self.contact.current() else {
            return;
        };
        *self.gesture_state.borrow_mut() = ForcePressState {
            phase: ForcePressPhase::Possible,
            position: contact.local,
            global_position: contact.global,
            smoother: contact
                .settings
                .position_smoothing()
                .map(PositionSmoother::new),
            ..ForcePressState::default()
        };
        if let PointerEvent::Down(data) = down.local
            && let Some(pressure) = data.sample.pressure
        {
            self.handle_sample(
                contact.local,
                contact.global,
                Some(f64::from(pressure.get())),
                event_time(down.local),
            );
        }
    }

    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        if !dispatch
            .local
            .pointer_id()
            .is_some_and(|pointer| self.contact.tracks(pointer))
        {
            return;
        }
        match dispatch.local {
            PointerEvent::Down(data) => {
                if let (Some(pressure), Some(local), Some(global)) = (
                    data.sample.pressure,
                    dispatch.local.position(),
                    dispatch.global.position(),
                ) {
                    self.handle_sample(
                        local,
                        global,
                        Some(f64::from(pressure.get())),
                        event_time(dispatch.local),
                    );
                }
            }
            PointerEvent::Move(data) => {
                let Some(contact) = self.contact.current() else {
                    return;
                };
                let PointerEvent::Move(global) = dispatch.global else {
                    return;
                };
                let samples = data
                    .coalesced()
                    .iter()
                    .zip(global.coalesced())
                    .chain(std::iter::once((data.current(), global.current())));
                let mut first = None;
                for (local, root) in samples {
                    if !self.contact.is_current(contact.id) {
                        break;
                    }
                    let local_position = local.position.get();
                    let root_position = root.position.get();
                    let candidate = RoutePanic::capture(|| {
                        self.handle_sample(
                            Offset::new(local_position.x, local_position.y),
                            Offset::new(root_position.x, root_position.y),
                            local.pressure.map(|pressure| f64::from(pressure.get())),
                            Some(local.time.as_nanos()),
                        );
                    });
                    RoutePanic::preserve_first(&mut first, candidate, "measured pressure history");
                }
                finish_containment(first, std::thread::panicking());
            }
            PointerEvent::Up(_) => {
                self.end(Some(dispatch), ArenaStep::Finish(ContactEnd::Contending))
            }
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
    fn accept_gesture(&self, contest: GestureContest) {
        if !self.contact.tracks_contest(contest) {
            return;
        }
        let mut notices = Notices::new();
        let mut state = self.gesture_state.borrow_mut();
        state.won = true;
        if state.phase == ForcePressPhase::Claiming {
            state.start(self.thresholds.peak, &mut notices);
        }
        drop(state);
        self.finish(ArenaStep::None, notices);
    }

    fn reject_gesture(&self, contest: GestureContest) {
        if !self.contact.tracks_contest(contest) {
            return;
        }
        let mut notices = Notices::new();
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
