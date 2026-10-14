//! Owner-local held-press recognition, driven by the arena's frame clock.

use super::{
    ArenaMembership, CancelOutcome, ContactId, PrimaryContact,
    callback_containment::{CallbackSequence, finish_containment, retire_callbacks},
    recognizer::{
        EventTimeline, GestureRecognizer, event_time, is_primary_down, measured_positions,
        motion_history,
    },
};
use crate::gesture::details::{LongPressEndDetails, LongPressMoveUpdateDetails};
use crate::{
    containment::RoutePanic,
    events::{PointerEvent, PointerEventExt, PointerKind},
    gesture::arena::{GestureArena, GestureArenaMember},
    gesture::settings::GestureSettingsProvider,
    gesture::{PositionSmoother, VelocityTracker},
    ids::PointerId,
    pointer::PointerDispatch,
};
use flui_foundation::geometry::Offset;
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};
use flui_platform_api::EventTime;
use web_time::Instant;

/// Callback for initial press contact.
pub type LongPressDownCallback = Rc<dyn Fn(LongPressDownDetails)>;
/// Callback for recognition without contact details.
pub type LongPressSimpleCallback = Rc<dyn Fn()>;
/// Callback for recognition with contact details.
pub type LongPressStartCallback = Rc<dyn Fn(LongPressStartDetails)>;
/// Callback for release-position notification or cancellation.
pub type LongPressCallback = Rc<dyn Fn(LongPressDetails)>;
/// Callback for measured movement after recognition.
pub type LongPressMoveUpdateCallback = Rc<dyn Fn(LongPressMoveUpdateDetails)>;
/// Callback for completed release and its measured terminal velocity.
pub type LongPressEndCallback = Rc<dyn Fn(LongPressEndDetails)>;

/// Initial press contact in root and recognizer coordinates.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LongPressDownDetails {
    /// Root-space position.
    pub global_position: Offset<f64>,
    /// Recognizer-local position.
    pub local_position: Offset<f64>,
    /// Admitted device kind.
    pub kind: PointerKind,
}
/// Contact at recognition time.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LongPressStartDetails {
    /// Root-space position.
    pub global_position: Offset<f64>,
    /// Recognizer-local position.
    pub local_position: Offset<f64>,
    /// Admitted device kind.
    pub kind: PointerKind,
}
/// Latest contact position at release or cancellation.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LongPressDetails {
    /// Root-space position.
    pub global_position: Offset<f64>,
    /// Recognizer-local position.
    pub local_position: Offset<f64>,
    /// Admitted device kind.
    pub kind: PointerKind,
}

#[derive(Default)]
#[expect(clippy::struct_field_names)]
struct LongPressCallbacks {
    on_long_press_down: Option<LongPressDownCallback>,
    on_long_press: Option<LongPressSimpleCallback>,
    on_long_press_start: Option<LongPressStartCallback>,
    on_long_press_move_update: Option<LongPressMoveUpdateCallback>,
    on_long_press_up: Option<LongPressCallback>,
    on_long_press_end: Option<LongPressEndCallback>,
    on_long_press_cancel: Option<LongPressCallback>,
}
impl Drop for LongPressCallbacks {
    fn drop(&mut self) {
        retire_callbacks!(self; on_long_press_down, on_long_press, on_long_press_start,
            on_long_press_move_update, on_long_press_up, on_long_press_end, on_long_press_cancel);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum LongPressPhase {
    #[default]
    Ready,
    Possible,
    Started,
}
#[derive(Debug, Default)]
struct LongPressState {
    phase: LongPressPhase,
    local: Offset<f64>,
    global: Offset<f64>,
    timeline: EventTimeline,
    velocity_tracker: VelocityTracker,
    /// Post-recognition smoothing of move updates, captured with the settings.
    smoother: Option<PositionSmoother>,
    /// Event-clock time of the latest sample.
    sample_time: Option<Instant>,
}

/// Recognizes a primary contact held within its frozen admission tolerance.
///
/// Callbacks are configured before creating the owner. For reentry, keep a
/// `Weak` in an external slot; a strong owner in the slot can create a cycle.
pub struct LongPressGestureRecognizer {
    // Contact destruction withdraws silently before callback captures retire.
    contact: PrimaryContact,
    callbacks: LongPressCallbacks,
    state: RefCell<LongPressState>,
    settings: GestureSettingsProvider,
}
/// Immutable press policy and callbacks, consumed to create one owner.
#[must_use]
pub struct LongPressGestureRecognizerBuilder {
    arena: GestureArena,
    callbacks: LongPressCallbacks,
    settings: GestureSettingsProvider,
}
impl LongPressGestureRecognizerBuilder {
    /// Freeze device-specific gesture policy for contact admission.
    pub fn settings(mut self, settings: impl Into<GestureSettingsProvider>) -> Self {
        self.settings = settings.into();
        self
    }
    /// Called immediately after contact admission.
    pub fn on_long_press_down(mut self, callback: impl Fn(LongPressDownDetails) + 'static) -> Self {
        self.callbacks.on_long_press_down = Some(Rc::new(callback));
        self
    }
    /// Called when the press is recognized.
    pub fn on_long_press(mut self, callback: impl Fn() + 'static) -> Self {
        self.callbacks.on_long_press = Some(Rc::new(callback));
        self
    }
    /// Called after the simple recognition callback while the contact remains current.
    pub fn on_long_press_start(
        mut self,
        callback: impl Fn(LongPressStartDetails) + 'static,
    ) -> Self {
        self.callbacks.on_long_press_start = Some(Rc::new(callback));
        self
    }
    /// Called for movement after recognition, with displacement from Down in
    /// each coordinate space. Unrepresentable displacement cancels the contact.
    pub fn on_long_press_move_update(
        mut self,
        callback: impl Fn(LongPressMoveUpdateDetails) + 'static,
    ) -> Self {
        self.callbacks.on_long_press_move_update = Some(Rc::new(callback));
        self
    }
    /// Called for release after recognition.
    pub fn on_long_press_up(mut self, callback: impl Fn(LongPressDetails) + 'static) -> Self {
        self.callbacks.on_long_press_up = Some(Rc::new(callback));
        self
    }
    /// Called after release while its contact is still current. Velocity uses
    /// measured local samples, the admitted estimator and release event time;
    /// predictions do not contribute and a stationary terminal tail yields zero.
    pub fn on_long_press_end(mut self, callback: impl Fn(LongPressEndDetails) + 'static) -> Self {
        self.callbacks.on_long_press_end = Some(Rc::new(callback));
        self
    }
    /// Called for explicit cancellation or a lost arena competition.
    pub fn on_long_press_cancel(mut self, callback: impl Fn(LongPressDetails) + 'static) -> Self {
        self.callbacks.on_long_press_cancel = Some(Rc::new(callback));
        self
    }
    /// Create the allocation used for dispatch and arena competition.
    #[must_use]
    pub fn build(self) -> Rc<LongPressGestureRecognizer> {
        Rc::new_cyclic(|this: &Weak<LongPressGestureRecognizer>| {
            let member: Weak<dyn GestureArenaMember> = this.clone();
            LongPressGestureRecognizer {
                contact: PrimaryContact::new(ArenaMembership::new(self.arena, member)),
                callbacks: self.callbacks,
                state: RefCell::default(),
                settings: self.settings,
            }
        })
    }
}
impl LongPressGestureRecognizer {
    /// Start immutable owner configuration.
    pub fn builder(arena: GestureArena) -> LongPressGestureRecognizerBuilder {
        LongPressGestureRecognizerBuilder {
            arena,
            callbacks: LongPressCallbacks::default(),
            settings: GestureSettingsProvider::default(),
        }
    }
    fn details(&self, kind: PointerKind) -> LongPressDetails {
        let state = self.state.borrow();
        LongPressDetails {
            global_position: state.global,
            local_position: state.local,
            kind,
        }
    }
    fn clock_for(&self, id: ContactId) -> Option<EventTime> {
        match RoutePanic::try_run(|| self.contact.now()) {
            Ok(now) => self.contact.is_current(id).then_some(now),
            Err(failure) => {
                self.cancel_failed_contact(id, failure);
                None
            }
        }
    }
    fn cancel_failed_contact(&self, id: ContactId, failure: RoutePanic) {
        let mut first = Some(failure);
        if self.contact.is_current(id) {
            RoutePanic::preserve_first(
                &mut first,
                RoutePanic::capture(|| {
                    self.cancel();
                }),
                "long press clock failure cancellation",
            );
        }
        finish_containment(first, std::thread::panicking());
    }
    fn finish_contact(&self, id: ContactId, first: &mut Option<RoutePanic>) {
        if self.contact.is_current(id) {
            RoutePanic::preserve_first(
                first,
                RoutePanic::capture(|| {
                    let _ = self.contact.finish();
                }),
                "long press contact completion",
            );
        }
    }
    fn fire_deadline(&self, now: EventTime) {
        self.contact.arena().run_owned_recovery(|recovery| {
            self.fire_deadline_with_recovery(now, recovery);
        });
    }

    fn fire_deadline_with_recovery(
        &self,
        now: EventTime,
        recovery: &mut flui_foundation::panic::RecoveryScope<'_>,
    ) -> bool {
        use flui_foundation::panic::FailureClass;
        let arena = self.contact.arena();
        let mut prepared = None;
        let completed = arena.invoke_with_recovery(FailureClass::Unknown, recovery, |_| {
            let Some(deadline) = self.contact.deadline() else {
                return;
            };
            if now < deadline {
                return;
            }
            let Some(contact) = self.contact.current() else {
                return;
            };
            let details = self.details(contact.kind);
            let mut state = self.state.borrow_mut();
            if state.phase != LongPressPhase::Possible {
                return;
            }
            state.phase = LongPressPhase::Started;
            // Move updates smooth from the position the press started at.
            let state = &mut *state;
            if let (Some(smoother), Some(time)) = (state.smoother.as_mut(), state.sample_time) {
                smoother.smooth(time, state.local, state.global);
            }
            prepared = Some((contact, details));
        });
        if !completed {
            return false;
        }
        let Some((contact, details)) = prepared else {
            return true;
        };
        arena.invoke_with_recovery(FailureClass::Delivery, recovery, |_| {
            self.contact.disarm_deadline();
        });
        if self.contact.is_current(contact.id) {
            arena.invoke_with_recovery(FailureClass::Unknown, recovery, |scope| {
                self.contact.accept_with_recovery(scope);
            });
        }
        let preserving = recovery.has_failure();
        arena.invoke_with_recovery(FailureClass::Delivery, recovery, |_| {
            let mut notices = CallbackSequence::preserving_failure(preserving);
            if self.contact.is_current(contact.id) {
                notices.call(self.callbacks.on_long_press.clone(), |callback| callback());
            }
            if self.contact.is_current(contact.id) {
                notices.call(self.callbacks.on_long_press_start.clone(), |callback| {
                    callback(LongPressStartDetails {
                        global_position: details.global_position,
                        local_position: details.local_position,
                        kind: details.kind,
                    });
                });
            }
            notices.finish();
        });
        true
    }
}
impl GestureRecognizer for LongPressGestureRecognizer {
    fn add_pointer(&self, down: PointerDispatch<'_>) {
        let pointer = down.local.pointer_id();
        let _span = tracing::info_span!(
            "long_press.add_pointer",
            pointer = ?pointer,
            event = %crate::gesture::observability::GestureEvent::RecognizerAdded,
        );
        if !is_primary_down(down.local) {
            return;
        }
        let settings = self.settings.snapshot();
        let Ok(id) = self.contact.begin(down, &settings) else {
            return;
        };
        let Some(contact) = self.contact.current().filter(|contact| contact.id == id) else {
            return;
        };
        *self.state.borrow_mut() = LongPressState {
            phase: LongPressPhase::Possible,
            local: contact.local,
            global: contact.global,
            timeline: EventTimeline::default(),
            velocity_tracker: VelocityTracker::for_gesture(
                contact.kind,
                contact.settings.velocity_estimator(),
            ),
            smoother: contact
                .settings
                .position_smoothing()
                .map(PositionSmoother::new),
            sample_time: None,
        };
        let Some(clock) = self.clock_for(id) else {
            return;
        };
        {
            let mut state = self.state.borrow_mut();
            let now = state
                .timeline
                .instant(event_time(down.local), self.contact.instant_at(clock));
            state.velocity_tracker.add_position(now, contact.local);
            state.sample_time = Some(now);
        }
        if let Err(failure) = RoutePanic::try_run(|| {
            self.contact
                .arm_deadline(contact.settings.long_press_timeout())
        }) {
            self.cancel_failed_contact(id, failure);
        }
        if !self.contact.is_current(id) {
            return;
        }
        let mut notices = CallbackSequence::new();
        notices.call(self.callbacks.on_long_press_down.clone(), |callback| {
            callback(LongPressDownDetails {
                global_position: contact.global,
                local_position: contact.local,
                kind: contact.kind,
            });
        });
        notices.finish();
    }
    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        let _span = tracing::info_span!(
            "long_press.handle_event",
            kind = %crate::gesture::observability::pointer_event_kind(dispatch.local),
            event = %crate::gesture::observability::GestureEvent::EventReceived,
        );
        let Some(contact) = self.contact.current() else {
            return;
        };
        if dispatch.local.pointer_id() != Some(contact.pointer) {
            return;
        }
        if matches!(dispatch.local, PointerEvent::Move(_) | PointerEvent::Up(_)) {
            let local = dispatch.local.position().unwrap_or(contact.local);
            let global = dispatch.global.position().unwrap_or(contact.global);
            if !local.dx.is_finite()
                || !local.dy.is_finite()
                || !global.dx.is_finite()
                || !global.dy.is_finite()
            {
                self.cancel();
                return;
            }
        }
        let clock = if matches!(dispatch.local, PointerEvent::Move(_) | PointerEvent::Up(_)) {
            let Some(clock) = self.clock_for(contact.id) else {
                return;
            };
            Some(clock)
        } else {
            None
        };
        match dispatch.local {
            PointerEvent::Move(_) => {
                let local = dispatch.local.position().unwrap_or(contact.local);
                let global = dispatch.global.position().unwrap_or(contact.global);
                let phase = self.state.borrow().phase;
                let offset = global - contact.global;
                let local_offset = local - contact.local;
                if !offset.dx.is_finite()
                    || !offset.dy.is_finite()
                    || !local_offset.dx.is_finite()
                    || !local_offset.dy.is_finite()
                    || measured_positions(dispatch.local).any(|position| {
                        let delta = position - contact.local;
                        !delta.dx.is_finite() || !delta.dy.is_finite()
                    })
                {
                    self.cancel();
                    return;
                }
                if phase == LongPressPhase::Possible
                    && measured_positions(dispatch.local).any(|position| {
                        let delta = position - contact.local;
                        contact.settings.exceeds_hit_slop(contact.kind, delta)
                    })
                {
                    self.cancel();
                    return;
                }
                // Smoothing shapes only the published move update; the slop
                // check above and the velocity tracker read the raw sample.
                let (published, published_global) = {
                    let mut state = self.state.borrow_mut();
                    let state = &mut *state;
                    state.local = local;
                    state.global = global;
                    let clock = self
                        .contact
                        .instant_at(clock.expect("BUG: Move reads the owner clock"));
                    for (stamp, position) in motion_history(dispatch.local) {
                        let time = state.timeline.instant(stamp, clock);
                        state.velocity_tracker.add_position(time, position);
                    }
                    let time = state.timeline.instant(event_time(dispatch.local), clock);
                    state.velocity_tracker.add_position(time, local);
                    state.sample_time = Some(time);
                    match state.smoother.as_mut() {
                        Some(smoother) if phase == LongPressPhase::Started => {
                            smoother.smooth(time, local, global)
                        }
                        _ => (local, global),
                    }
                };
                if phase == LongPressPhase::Possible {
                    if self.contact.is_current(contact.id) {
                        self.fire_deadline(clock.expect("BUG: Move reads the owner clock"));
                    }
                } else if phase == LongPressPhase::Started {
                    let details = LongPressMoveUpdateDetails {
                        global_position: published_global,
                        local_position: published,
                        offset_from_origin: published_global - contact.global,
                        local_offset_from_origin: published - contact.local,
                        kind: contact.kind,
                    };
                    let mut notices = CallbackSequence::new();
                    notices.call(
                        self.callbacks.on_long_press_move_update.clone(),
                        |callback| callback(details),
                    );
                    notices.finish();
                }
            }
            PointerEvent::Up(_) => {
                self.contact.disarm_deadline();
                if !self.contact.is_current(contact.id) {
                    return;
                }
                let (started, velocity) = {
                    let mut state = self.state.borrow_mut();
                    let started = state.phase == LongPressPhase::Started;
                    state.phase = LongPressPhase::Ready;
                    state.local = dispatch.local.position().unwrap_or(contact.local);
                    state.global = dispatch.global.position().unwrap_or(contact.global);
                    let time = state.timeline.instant(
                        event_time(dispatch.local),
                        self.contact
                            .instant_at(clock.expect("BUG: Up reads the owner clock")),
                    );
                    (started, state.velocity_tracker.velocity_at(time))
                };
                let details = self.details(contact.kind);
                let end = LongPressEndDetails {
                    global_position: details.global_position,
                    local_position: details.local_position,
                    velocity,
                    kind: contact.kind,
                };
                let mut notices = CallbackSequence::new();
                if started {
                    notices.call(self.callbacks.on_long_press_up.clone(), |callback| {
                        callback(details.clone());
                    });
                    if self.contact.is_current(contact.id) {
                        notices.call(self.callbacks.on_long_press_end.clone(), |callback| {
                            callback(end);
                        });
                    }
                }
                let mut first = RoutePanic::capture(|| notices.finish());
                self.finish_contact(contact.id, &mut first);
                finish_containment(first, std::thread::panicking());
            }
            PointerEvent::Cancel(_) => {
                self.cancel();
            }
            _ => {}
        }
    }
    fn cancel(&self) -> CancelOutcome {
        let Some(contact) = self.contact.current() else {
            return CancelOutcome::Idle;
        };
        let details = self.details(contact.kind);
        let notify = {
            let mut state = self.state.borrow_mut();
            let notify = state.phase != LongPressPhase::Ready;
            state.phase = LongPressPhase::Ready;
            notify
        };
        let mut first = RoutePanic::capture(|| {
            let _ = self.contact.cancel();
        });
        let mut notices = CallbackSequence::new();
        if notify {
            notices.call(self.callbacks.on_long_press_cancel.clone(), |callback| {
                callback(details);
            });
        }
        RoutePanic::preserve_first(
            &mut first,
            RoutePanic::capture(|| notices.finish()),
            "long press cancellation",
        );
        finish_containment(first, std::thread::panicking());
        CancelOutcome::Cancelled
    }
}
impl GestureArenaMember for LongPressGestureRecognizer {
    fn accept_gesture(&self, _: PointerId) {}
    fn reject_gesture(&self, pointer: PointerId) {
        if self.contact.tracks(pointer) {
            self.cancel();
        }
    }
    fn deadline(&self) -> Option<EventTime> {
        self.contact.deadline()
    }
    fn poll_deadline(&self, now: EventTime) {
        self.fire_deadline(now);
    }
    fn poll_deadline_with_recovery(
        &self,
        now: EventTime,
        recovery: &mut flui_foundation::panic::RecoveryScope<'_>,
    ) -> bool {
        self.fire_deadline_with_recovery(now, recovery)
    }
}
impl std::fmt::Debug for LongPressGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LongPressGestureRecognizer")
            .field("state", &self.state.borrow())
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}
impl std::fmt::Debug for LongPressGestureRecognizerBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LongPressGestureRecognizerBuilder")
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}
