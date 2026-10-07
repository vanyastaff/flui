//! Two primary taps within one frozen inter-tap window.

use super::{
    callback_containment::{finish_containment, invoke_callback},
    contact::{ArenaMembership, PrimaryContact},
    recognizer::{CancelOutcome, GestureRecognizer, is_primary_down},
};
use crate::{
    arena::{GestureArena, GestureArenaEntry, GestureArenaMember, GestureDisposition},
    events::{PointerEvent, PointerEventExt, PointerType},
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

/// Callback carrying the contact in local and root coordinates.
pub type DoubleTapCallback = Rc<dyn Fn(DoubleTapDetails)>;
/// Position and device kind of a double tap contact.
#[derive(Debug, Clone, PartialEq)]
pub struct DoubleTapDetails {
    /// Position in the root coordinate space.
    pub global_position: Offset<f64>,
    /// Position in the recognizer coordinate space.
    pub local_position: Offset<f64>,
    /// Device kind frozen at admission.
    pub kind: PointerType,
}

#[derive(Default)]
#[expect(clippy::struct_field_names)]
struct DoubleTapCallbacks {
    on_double_tap: Option<DoubleTapCallback>,
    on_double_tap_down: Option<DoubleTapCallback>,
    on_double_tap_cancel: Option<DoubleTapCallback>,
}
impl Drop for DoubleTapCallbacks {
    fn drop(&mut self) {
        super::callback_containment::retire_callbacks!(self; on_double_tap, on_double_tap_down, on_double_tap_cancel);
    }
}
/// Configure the recognizer before its callbacks become immutable. Capture a
/// `Weak` through an external slot for callbacks that reach the recognizer.
#[must_use]
pub struct DoubleTapGestureRecognizerBuilder {
    arena: GestureArena,
    settings: GestureSettings,
    callbacks: DoubleTapCallbacks,
}
impl std::fmt::Debug for DoubleTapGestureRecognizerBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DoubleTapGestureRecognizerBuilder")
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}
impl DoubleTapGestureRecognizerBuilder {
    /// Freeze gesture settings for contacts admitted by this recognizer.
    pub fn settings(mut self, settings: GestureSettings) -> Self {
        self.settings = settings;
        self
    }
    /// Register a callback before building the recognizer.
    pub fn on_double_tap(mut self, callback: impl Fn(DoubleTapDetails) + 'static) -> Self {
        self.callbacks.on_double_tap = Some(Rc::new(callback));
        self
    }
    /// Register a callback before building the recognizer.
    pub fn on_double_tap_down(mut self, callback: impl Fn(DoubleTapDetails) + 'static) -> Self {
        self.callbacks.on_double_tap_down = Some(Rc::new(callback));
        self
    }
    /// Register a callback before building the recognizer.
    pub fn on_double_tap_cancel(mut self, callback: impl Fn(DoubleTapDetails) + 'static) -> Self {
        self.callbacks.on_double_tap_cancel = Some(Rc::new(callback));
        self
    }
    /// Allocate the owner-local recognizer with immutable callbacks.
    #[must_use]
    pub fn build(self) -> Rc<DoubleTapGestureRecognizer> {
        Rc::new_cyclic(|this: &Weak<DoubleTapGestureRecognizer>| {
            let member: Weak<dyn GestureArenaMember> = this.clone();
            DoubleTapGestureRecognizer {
                contact: PrimaryContact::new(ArenaMembership::new(self.arena, member)),
                gesture: RefCell::new(DoubleTapState::Ready),
                first_entry: RefCell::new(None),
                settings: self.settings,
                callbacks: self.callbacks,
            }
        })
    }
}

#[derive(Debug, Clone)]
enum DoubleTapState {
    Ready,
    FirstDown,
    Waiting {
        details: DoubleTapDetails,
        deadline: Option<Instant>,
        settings: GestureSettings,
    },
    SecondDown,
}
/// Recognizes two primary taps, keeping the first exact arena generation held.
pub struct DoubleTapGestureRecognizer {
    contact: PrimaryContact,
    gesture: RefCell<DoubleTapState>,
    first_entry: RefCell<Option<GestureArenaEntry>>,
    settings: GestureSettings,
    callbacks: DoubleTapCallbacks,
}
impl std::fmt::Debug for DoubleTapGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DoubleTapGestureRecognizer")
            .field("gesture", &self.gesture.try_borrow().ok())
            .finish_non_exhaustive()
    }
}
impl DoubleTapGestureRecognizer {
    /// Start configuring an owner-local recognizer.
    #[must_use]
    pub fn builder(arena: GestureArena) -> DoubleTapGestureRecognizerBuilder {
        DoubleTapGestureRecognizerBuilder {
            arena,
            settings: GestureSettings::default(),
            callbacks: DoubleTapCallbacks::default(),
        }
    }
    fn details(&self) -> DoubleTapDetails {
        self.contact.current().map_or(
            DoubleTapDetails {
                local_position: Offset::ZERO,
                global_position: Offset::ZERO,
                kind: PointerType::Touch,
            },
            |contact| DoubleTapDetails {
                local_position: contact.local,
                global_position: contact.global,
                kind: contact.kind,
            },
        )
    }
    fn retire_attempt(&self, details: DoubleTapDetails, cancelled: bool) {
        *self.gesture.borrow_mut() = DoubleTapState::Ready;
        let first_entry = self.first_entry.borrow_mut().take();
        let incoming = std::thread::panicking();
        let mut first = RoutePanic::capture(|| {
            if cancelled {
                self.contact.cancel();
            } else {
                self.contact.withdraw();
            }
        });
        if let Some(entry) = first_entry {
            let candidate = RoutePanic::capture(|| {
                entry.resolve(GestureDisposition::Rejected);
            });
            RoutePanic::preserve_first(&mut first, candidate, "double tap first rejection");
            let candidate = RoutePanic::capture(|| entry.release());
            RoutePanic::preserve_first(&mut first, candidate, "double tap hold release");
        }
        let candidate = RoutePanic::capture(|| {
            invoke_callback(
                self.callbacks.on_double_tap_cancel.clone(),
                || {},
                |callback| callback(details),
            )
        });
        RoutePanic::preserve_first(&mut first, candidate, "double tap cancellation");
        finish_containment(first, incoming);
    }
    fn expire(&self, now: Instant) -> bool {
        let waiting = self.gesture.borrow().clone();
        if let DoubleTapState::Waiting {
            details,
            deadline: Some(deadline),
            ..
        } = waiting
            && now >= deadline
        {
            self.retire_attempt(details, false);
            true
        } else {
            false
        }
    }
    fn complete(&self, details: DoubleTapDetails) {
        let Some(snapshot) = self.contact.current() else {
            return;
        };
        *self.gesture.borrow_mut() = DoubleTapState::Ready;
        let first_entry = self.first_entry.borrow_mut().take();
        let incoming = std::thread::panicking();
        let mut first = None;
        if let Some(entry) = first_entry {
            let candidate = RoutePanic::capture(|| entry.resolve(GestureDisposition::Accepted));
            RoutePanic::preserve_first(&mut first, candidate, "double tap first acceptance");
            let candidate = RoutePanic::capture(|| entry.release());
            RoutePanic::preserve_first(&mut first, candidate, "double tap hold release");
        }
        if self.contact.is_current(snapshot.id) {
            let candidate = RoutePanic::capture(|| self.contact.accept());
            RoutePanic::preserve_first(&mut first, candidate, "double tap acceptance");
        }
        if self.contact.is_current(snapshot.id) {
            let candidate = RoutePanic::capture(|| {
                invoke_callback(
                    self.callbacks.on_double_tap.clone(),
                    || {},
                    |callback| callback(details),
                )
            });
            RoutePanic::preserve_first(&mut first, candidate, "double tap completion");
        }
        if self.contact.is_current(snapshot.id) {
            let candidate = RoutePanic::capture(|| {
                self.contact.finish();
            });
            RoutePanic::preserve_first(&mut first, candidate, "double tap contact retirement");
        }
        finish_containment(first, incoming);
    }
}
impl GestureRecognizer for DoubleTapGestureRecognizer {
    fn add_pointer(&self, dispatch: PointerDispatch<'_>) {
        if !is_primary_down(dispatch.local) {
            return;
        }
        let PointerEvent::Down(_) = dispatch.local else {
            return;
        };
        let state = self.gesture.borrow().clone();
        let mut failure = None;
        let mut second = false;
        match state {
            DoubleTapState::FirstDown | DoubleTapState::SecondDown => return,
            DoubleTapState::Waiting {
                details,
                deadline,
                settings,
            } => {
                let Some(waiting_contact) = self.contact.current() else {
                    return;
                };
                let now = self.contact.now();
                if !self.contact.is_current(waiting_contact.id) {
                    return;
                }
                let distance = (dispatch.local.position() - details.local_position).distance();
                if deadline.is_some_and(|deadline| now >= deadline)
                    || !distance.is_finite()
                    || distance > settings.double_tap_slop()
                {
                    failure = RoutePanic::capture(|| self.retire_attempt(details, false));
                } else {
                    // The held first generation survives retiring its contact.
                    failure = RoutePanic::capture(|| {
                        self.contact.finish();
                    });
                    second = true;
                }
            }
            DoubleTapState::Ready => {}
        }
        // Cleanup callbacks may have admitted a replacement contact.
        if self.contact.current().is_none() && self.contact.begin(dispatch, &self.settings).is_ok()
        {
            *self.gesture.borrow_mut() = if second {
                DoubleTapState::SecondDown
            } else {
                DoubleTapState::FirstDown
            };
            if second {
                let candidate = RoutePanic::capture(|| {
                    invoke_callback(
                        self.callbacks.on_double_tap_down.clone(),
                        || {},
                        |callback| callback(self.details()),
                    )
                });
                RoutePanic::preserve_first(&mut failure, candidate, "double tap second down");
            }
        }
        finish_containment(failure, std::thread::panicking());
    }
    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        let Some(contact) = self.contact.current() else {
            return;
        };
        if !self.contact.tracks(dispatch.local.pointer_id()) {
            return;
        }
        let details = DoubleTapDetails {
            local_position: dispatch.local.position(),
            global_position: dispatch.global.position(),
            kind: contact.kind,
        };
        let state = self.gesture.borrow().clone();
        match dispatch.local {
            PointerEvent::Move(_) => {
                if matches!(
                    state,
                    DoubleTapState::FirstDown | DoubleTapState::SecondDown
                ) && self.contact.moved_beyond(
                    details.local_position,
                    contact.settings.hit_slop(contact.kind),
                ) {
                    self.retire_attempt(details, true);
                }
            }
            PointerEvent::Up(_) => match state {
                DoubleTapState::FirstDown => {
                    let entry = self.contact.entry();
                    if let Some(entry) = &entry {
                        entry.hold();
                    }
                    *self.first_entry.borrow_mut() = entry;
                    let deadline = self
                        .contact
                        .arm_deadline(contact.settings.double_tap_timeout());
                    if self.contact.is_current(contact.id) {
                        *self.gesture.borrow_mut() = DoubleTapState::Waiting {
                            details,
                            deadline,
                            settings: contact.settings,
                        };
                    }
                }
                DoubleTapState::SecondDown => self.complete(details),
                _ => {}
            },
            PointerEvent::Cancel(_) => self.retire_attempt(self.details(), true),
            _ => {}
        }
    }
    fn cancel(&self) -> CancelOutcome {
        if self.contact.current().is_none()
            && matches!(*self.gesture.borrow(), DoubleTapState::Ready)
        {
            return CancelOutcome::Idle;
        }
        self.retire_attempt(self.details(), true);
        CancelOutcome::Cancelled
    }
}
impl GestureArenaMember for DoubleTapGestureRecognizer {
    fn accept_gesture(&self, _: PointerId) {}
    fn reject_gesture(&self, _: PointerId) {
        if self.contact.current().is_some() {
            self.retire_attempt(self.details(), false);
        }
    }
    fn deadline(&self) -> Option<Instant> {
        self.contact.deadline()
    }
    fn poll_deadline(&self, now: Instant) {
        self.expire(now);
    }
}
impl Drop for DoubleTapGestureRecognizer {
    fn drop(&mut self) {
        if let Some(entry) = self.contact.entry() {
            entry.withdraw_deferred();
        }
        if let Some(entry) = self.first_entry.get_mut().take() {
            entry.release_deferred();
            entry.withdraw_deferred();
        }
        // Fields then drop in declaration order: contact first, callbacks last.
    }
}
