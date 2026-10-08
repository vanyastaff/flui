//! Owner-local held-press recognition, driven by the arena's frame clock.

use super::{
    ArenaMembership, CancelOutcome, ContactId, PrimaryContact,
    callback_containment::{CallbackSequence, finish_containment, retire_callbacks},
    recognizer::{GestureRecognizer, is_primary_down, measured_positions},
};
use crate::{
    arena::{GestureArena, GestureArenaMember},
    events::{PointerEvent, PointerEventExt, PointerKind},
    ids::PointerId,
    routing::{PointerDispatch, RoutePanic},
    settings::GestureSettings,
};
use flui_foundation::geometry::Offset;
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};
use web_time::Instant;

/// Callback for initial press contact.
pub type LongPressDownCallback = Rc<dyn Fn(LongPressDownDetails)>;
/// Callback for recognition without contact details.
pub type LongPressSimpleCallback = Rc<dyn Fn()>;
/// Callback for recognition with contact details.
pub type LongPressStartCallback = Rc<dyn Fn(LongPressStartDetails)>;
/// Callback for movement, release, or cancellation.
pub type LongPressCallback = Rc<dyn Fn(LongPressDetails)>;

/// Initial press contact in root and recognizer coordinates.
#[derive(Debug, Clone, PartialEq)]
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
pub struct LongPressStartDetails {
    /// Root-space position.
    pub global_position: Offset<f64>,
    /// Recognizer-local position.
    pub local_position: Offset<f64>,
    /// Admitted device kind.
    pub kind: PointerKind,
}
/// Contact at movement, release, or cancellation.
#[derive(Debug, Clone, PartialEq)]
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
    on_long_press_move_update: Option<LongPressCallback>,
    on_long_press_up: Option<LongPressCallback>,
    on_long_press_end: Option<LongPressCallback>,
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
    settings: GestureSettings,
}
/// Immutable press policy and callbacks, consumed to create one owner.
#[must_use]
pub struct LongPressGestureRecognizerBuilder {
    arena: GestureArena,
    callbacks: LongPressCallbacks,
    settings: GestureSettings,
}
impl LongPressGestureRecognizerBuilder {
    /// Freeze device-specific gesture policy for contact admission.
    #[must_use]
    pub fn settings(mut self, settings: GestureSettings) -> Self {
        self.settings = settings;
        self
    }
    /// Called immediately after contact admission.
    #[must_use]
    pub fn on_long_press_down(mut self, callback: impl Fn(LongPressDownDetails) + 'static) -> Self {
        self.callbacks.on_long_press_down = Some(Rc::new(callback));
        self
    }
    /// Called when the press is recognized.
    #[must_use]
    pub fn on_long_press(mut self, callback: impl Fn() + 'static) -> Self {
        self.callbacks.on_long_press = Some(Rc::new(callback));
        self
    }
    /// Called after the simple recognition callback while the contact remains current.
    #[must_use]
    pub fn on_long_press_start(
        mut self,
        callback: impl Fn(LongPressStartDetails) + 'static,
    ) -> Self {
        self.callbacks.on_long_press_start = Some(Rc::new(callback));
        self
    }
    /// Called for movement after recognition.
    #[must_use]
    pub fn on_long_press_move_update(
        mut self,
        callback: impl Fn(LongPressDetails) + 'static,
    ) -> Self {
        self.callbacks.on_long_press_move_update = Some(Rc::new(callback));
        self
    }
    /// Called for release after recognition.
    #[must_use]
    pub fn on_long_press_up(mut self, callback: impl Fn(LongPressDetails) + 'static) -> Self {
        self.callbacks.on_long_press_up = Some(Rc::new(callback));
        self
    }
    /// Called after release while its contact is still current.
    #[must_use]
    pub fn on_long_press_end(mut self, callback: impl Fn(LongPressDetails) + 'static) -> Self {
        self.callbacks.on_long_press_end = Some(Rc::new(callback));
        self
    }
    /// Called for explicit cancellation or a lost arena competition.
    #[must_use]
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
    #[must_use]
    pub fn builder(arena: GestureArena) -> LongPressGestureRecognizerBuilder {
        LongPressGestureRecognizerBuilder {
            arena,
            callbacks: LongPressCallbacks::default(),
            settings: GestureSettings::default(),
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
    fn fire_deadline(&self, now: Instant) {
        let Some(deadline) = self.contact.deadline() else {
            return;
        };
        if now < deadline {
            return;
        }
        let Some(contact) = self.contact.current() else {
            return;
        };
        {
            let mut state = self.state.borrow_mut();
            if state.phase != LongPressPhase::Possible {
                return;
            }
            state.phase = LongPressPhase::Started;
        }
        self.contact.disarm_deadline();
        let details = self.details(contact.kind);
        let mut first = RoutePanic::capture(|| self.contact.accept());
        let mut notices = CallbackSequence::new();
        if self.contact.is_current(contact.id) {
            notices.call(self.callbacks.on_long_press.clone(), |callback| callback());
        }
        if self.contact.is_current(contact.id) {
            notices.call(self.callbacks.on_long_press_start.clone(), |callback| {
                callback(LongPressStartDetails {
                    global_position: details.global_position,
                    local_position: details.local_position,
                    kind: details.kind,
                })
            });
        }
        RoutePanic::preserve_first(
            &mut first,
            RoutePanic::capture(|| notices.finish()),
            "long press recognition",
        );
        finish_containment(first, std::thread::panicking());
    }
}
impl GestureRecognizer for LongPressGestureRecognizer {
    fn add_pointer(&self, down: PointerDispatch<'_>) {
        let pointer = down.local.pointer_id();
        let _span = tracing::info_span!(
            "long_press.add_pointer",
            pointer = ?pointer,
            event = %crate::observability::GestureEvent::RecognizerAdded,
        );
        if !is_primary_down(down.local) {
            return;
        }
        let Ok(id) = self.contact.begin(down, &self.settings) else {
            return;
        };
        let Some(contact) = self.contact.current().filter(|contact| contact.id == id) else {
            return;
        };
        *self.state.borrow_mut() = LongPressState {
            phase: LongPressPhase::Possible,
            local: contact.local,
            global: contact.global,
        };
        let _ = self
            .contact
            .arm_deadline(contact.settings.long_press_timeout());
        if !self.contact.is_current(id) {
            return;
        }
        let mut notices = CallbackSequence::new();
        notices.call(self.callbacks.on_long_press_down.clone(), |callback| {
            callback(LongPressDownDetails {
                global_position: contact.global,
                local_position: contact.local,
                kind: contact.kind,
            })
        });
        notices.finish();
    }
    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        let _span = tracing::info_span!(
            "long_press.handle_event",
            kind = %crate::observability::pointer_event_kind(dispatch.local),
            event = %crate::observability::GestureEvent::EventReceived,
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
        match dispatch.local {
            PointerEvent::Move(_) => {
                let local = dispatch.local.position().unwrap_or(contact.local);
                let global = dispatch.global.position().unwrap_or(contact.global);
                let phase = self.state.borrow().phase;
                if phase == LongPressPhase::Possible
                    && measured_positions(dispatch.local).any(|position| {
                        let delta = position - contact.local;
                        delta.dx.hypot(delta.dy) > contact.settings.hit_slop(contact.kind)
                    })
                {
                    self.cancel();
                    return;
                }
                {
                    let mut state = self.state.borrow_mut();
                    state.local = local;
                    state.global = global;
                }
                if phase == LongPressPhase::Possible {
                    let now = self.contact.now();
                    if self.contact.is_current(contact.id) {
                        self.fire_deadline(now);
                    }
                } else if phase == LongPressPhase::Started {
                    let details = self.details(contact.kind);
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
                let started = {
                    let mut state = self.state.borrow_mut();
                    let started = state.phase == LongPressPhase::Started;
                    state.phase = LongPressPhase::Ready;
                    state.local = dispatch.local.position().unwrap_or(contact.local);
                    state.global = dispatch.global.position().unwrap_or(contact.global);
                    started
                };
                let details = self.details(contact.kind);
                let mut notices = CallbackSequence::new();
                if started {
                    notices.call(self.callbacks.on_long_press_up.clone(), |callback| {
                        callback(details.clone())
                    });
                    if self.contact.is_current(contact.id) {
                        notices.call(self.callbacks.on_long_press_end.clone(), |callback| {
                            callback(details)
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
                callback(details)
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
    fn deadline(&self) -> Option<Instant> {
        self.contact.deadline()
    }
    fn poll_deadline(&self, now: Instant) {
        self.fire_deadline(now);
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
