//! Owner-local drag recognition with one admitted contact.
use super::{
    callback_containment::{
        finish_containment, invoke_callback, retire_callback, retire_callbacks,
    },
    contact::{ArenaMembership, ContactId, PrimaryContact},
    recognizer::{CancelOutcome, EventTimeline, GestureRecognizer, event_time, is_primary_down},
};
use crate::{
    arena::{GestureArena, GestureArenaMember},
    events::{PointerEvent, PointerType},
    ids::PointerId,
    processing::VelocityTracker,
    routing::PointerDispatch,
    settings::GestureSettings,
    traits::{DragAxis, PointerEventExtTrait},
};
use flui_foundation::geometry::Offset;
use std::{cell::RefCell, rc::Rc};
use web_time::Instant;

/// Position used for the initial drag notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum DragStartBehavior {
    /// Report the contact's Down position and accumulated initial movement.
    Down,
    /// Report the position at acceptance.
    #[default]
    Start,
}
/// Details about a newly admitted drag contact.
#[derive(Debug, Clone, PartialEq)]
pub struct DragDownDetails {
    /// Root-space contact position.
    pub global_position: Offset<f64>,
    /// Receiving node's contact position.
    pub local_position: Offset<f64>,
    /// Device kind captured at admission.
    pub kind: PointerType,
}
/// Details about an accepted drag.
#[derive(Debug, Clone)]
pub struct DragStartDetails {
    /// Root-space initial position.
    pub global_position: Offset<f64>,
    /// Receiving node's initial position.
    pub local_position: Offset<f64>,
    /// Admitted device kind.
    pub kind: PointerType,
    /// Event-clock instant at acceptance.
    pub timestamp: Instant,
}
/// Details about movement during an accepted drag.
#[derive(Debug, Clone, PartialEq)]
pub struct DragUpdateDetails {
    /// Root-space observed position.
    pub global_position: Offset<f64>,
    /// Receiving node's position.
    pub local_position: Offset<f64>,
    /// Movement since the previous update, projected onto the axis.
    pub delta: Offset<f64>,
    /// Axis component of this update's movement.
    pub primary_delta: f64,
    /// Admitted device kind.
    pub kind: PointerType,
}
/// Why an accepted drag ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureEndReason {
    /// Pointer release completed the drag.
    Completed,
    /// Cancellation interrupted the drag.
    Cancelled,
}
/// Details about an accepted drag's terminal notification.
#[derive(Debug, Clone, PartialEq)]
pub struct DragEndDetails {
    /// Completion or cancellation.
    pub reason: GestureEndReason,
    /// Velocity measured on the event clock.
    pub velocity: Velocity,
    /// Final observed root-space position.
    pub global_position: Offset<f64>,
    /// Final receiving-node position.
    pub local_position: Offset<f64>,
    /// Axis component of the velocity.
    pub primary_velocity: f64,
}
pub use crate::processing::Velocity;
/// Callback for an admitted contact.
pub type DragDownCallback = Rc<dyn Fn(DragDownDetails)>;
/// Callback for arena acceptance.
pub type DragStartCallback = Rc<dyn Fn(DragStartDetails)>;
/// Callback for accepted movement.
pub type DragUpdateCallback = Rc<dyn Fn(DragUpdateDetails)>;
/// Callback for an accepted terminal contact.
pub type DragEndCallback = Rc<dyn Fn(DragEndDetails)>;
/// Callback for a contact cancelled before acceptance.
pub type DragCancelCallback = Rc<dyn Fn()>;

#[derive(Default)]
#[expect(
    clippy::struct_field_names,
    reason = "callback fields name their public notifications"
)]
struct DragCallbacks {
    on_down: Option<DragDownCallback>,
    on_start: Option<DragStartCallback>,
    on_update: Option<DragUpdateCallback>,
    on_end: Option<DragEndCallback>,
    on_cancel: Option<DragCancelCallback>,
}
impl Drop for DragCallbacks {
    fn drop(&mut self) {
        retire_callbacks!(self; on_down, on_start, on_update, on_end, on_cancel);
    }
}
fn replace_callback<T: ?Sized>(slot: &mut Option<Rc<T>>, incoming: Rc<T>) {
    let outgoing = slot.replace(incoming);
    let mut failure = None;
    retire_callback(outgoing, &mut failure);
    finish_containment(failure, std::thread::panicking());
}

/// Collects drag policy and callbacks before shared ownership begins.
/// Callbacks cannot be replaced after [`build`](Self::build).
pub struct DragGestureRecognizerBuilder {
    arena: GestureArena,
    axis: DragAxis,
    start_behavior: DragStartBehavior,
    settings: GestureSettings,
    callbacks: DragCallbacks,
}
impl std::fmt::Debug for DragGestureRecognizerBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragGestureRecognizerBuilder")
            .field("axis", &self.axis)
            .field("start_behavior", &self.start_behavior)
            .finish_non_exhaustive()
    }
}
impl DragGestureRecognizerBuilder {
    /// Configure admission settings, captured independently for each contact.
    #[must_use]
    pub fn settings(mut self, settings: GestureSettings) -> Self {
        self.settings = settings;
        self
    }
    /// Configure the initial-position policy.
    #[must_use]
    pub fn drag_start_behavior(mut self, behavior: DragStartBehavior) -> Self {
        self.start_behavior = behavior;
        self
    }
    /// Set the contact callback.
    #[must_use]
    pub fn on_down(mut self, callback: impl Fn(DragDownDetails) + 'static) -> Self {
        replace_callback(&mut self.callbacks.on_down, Rc::new(callback));
        self
    }
    /// Set the acceptance callback.
    #[must_use]
    pub fn on_start(mut self, callback: impl Fn(DragStartDetails) + 'static) -> Self {
        replace_callback(&mut self.callbacks.on_start, Rc::new(callback));
        self
    }
    /// Set the movement callback.
    #[must_use]
    pub fn on_update(mut self, callback: impl Fn(DragUpdateDetails) + 'static) -> Self {
        replace_callback(&mut self.callbacks.on_update, Rc::new(callback));
        self
    }
    /// Set the accepted terminal callback.
    #[must_use]
    pub fn on_end(mut self, callback: impl Fn(DragEndDetails) + 'static) -> Self {
        replace_callback(&mut self.callbacks.on_end, Rc::new(callback));
        self
    }
    /// Set the unaccepted cancellation callback.
    #[must_use]
    pub fn on_cancel(mut self, callback: impl Fn() + 'static) -> Self {
        replace_callback(&mut self.callbacks.on_cancel, Rc::new(callback));
        self
    }
    /// Build a recognizer belonging to the current UI owner.
    #[must_use]
    pub fn build(self) -> Rc<DragGestureRecognizer> {
        Rc::<DragGestureRecognizer>::new_cyclic(|this| DragGestureRecognizer {
            contact: PrimaryContact::new(ArenaMembership::new(self.arena, this.clone())),
            axis: self.axis,
            start_behavior: self.start_behavior,
            settings: self.settings,
            callbacks: self.callbacks,
            drag_state: RefCell::new(None),
        })
    }
}

struct DragState {
    id: ContactId,
    accepted: bool,
    last_position: Offset<f64>,
    last_global_position: Offset<f64>,
    last_time: Instant,
    timeline: EventTimeline,
    velocity_tracker: VelocityTracker,
}

/// Recognizes one drag, using immutable callbacks and policy.
/// An accepted cancellation delivers `on_end(Cancelled)`; an unaccepted
/// cancellation delivers `on_cancel`. Both leave the recognizer reusable.
pub struct DragGestureRecognizer {
    contact: PrimaryContact,
    axis: DragAxis,
    start_behavior: DragStartBehavior,
    settings: GestureSettings,
    callbacks: DragCallbacks,
    drag_state: RefCell<Option<DragState>>,
}
impl std::fmt::Debug for DragGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragGestureRecognizer")
            .field("axis", &self.axis)
            .field("start_behavior", &self.start_behavior)
            .finish_non_exhaustive()
    }
}
impl DragGestureRecognizer {
    /// Configure a drag before allocating its shared owner.
    #[must_use]
    pub fn builder(arena: GestureArena, axis: DragAxis) -> DragGestureRecognizerBuilder {
        DragGestureRecognizerBuilder {
            arena,
            axis,
            start_behavior: DragStartBehavior::default(),
            settings: GestureSettings::default(),
            callbacks: DragCallbacks::default(),
        }
    }
    /// Configured axis.
    #[must_use]
    pub fn axis(&self) -> DragAxis {
        self.axis
    }
    /// Configured initial-position policy.
    #[must_use]
    pub fn drag_start_behavior(&self) -> DragStartBehavior {
        self.start_behavior
    }
    fn project_delta(&self, delta: Offset<f64>) -> Offset<f64> {
        match self.axis {
            DragAxis::Vertical => Offset::new(0.0, delta.dy),
            DragAxis::Horizontal => Offset::new(delta.dx, 0.0),
            DragAxis::Free => delta.to_delta(),
        }
    }
    fn primary_delta(&self, delta: Offset<f64>) -> f64 {
        match self.axis {
            DragAxis::Vertical => delta.dy,
            DragAxis::Horizontal => delta.dx,
            DragAxis::Free => delta.dx.hypot(delta.dy),
        }
    }
    fn slop(&self, kind: PointerType, settings: &GestureSettings) -> f64 {
        match self.axis {
            DragAxis::Free => settings.pan_slop_for(kind),
            DragAxis::Vertical | DragAxis::Horizontal if kind == PointerType::Mouse => {
                settings.hit_slop(kind)
            }
            DragAxis::Vertical => settings.pan_slop_vertical(),
            DragAxis::Horizontal => settings.pan_slop_horizontal(),
        }
    }
    fn handle_move(&self, dispatch: PointerDispatch<'_>) {
        let Some(contact) = self.contact.current() else {
            return;
        };
        let position = dispatch.local.position();
        let global = dispatch.global.position();
        if !position.dx.is_finite()
            || !position.dy.is_finite()
            || !global.dx.is_finite()
            || !global.dy.is_finite()
        {
            self.cancel();
            return;
        }
        let clock = self.contact.now();
        let (update, claim) = {
            let mut slot = self.drag_state.borrow_mut();
            let Some(state) = slot.as_mut() else {
                return;
            };
            if state.id != contact.id {
                return;
            }
            let now = state.timeline.instant(event_time(dispatch.local), clock);
            let delta = self.project_delta(position - state.last_position);
            if !delta.dx.is_finite()
                || !delta.dy.is_finite()
                || !self.primary_delta(delta).is_finite()
            {
                drop(slot);
                self.cancel();
                return;
            }
            state.last_position = position;
            state.last_global_position = global;
            state.last_time = now;
            state.velocity_tracker.add_position(now, position);
            let update = state.accepted.then_some(DragUpdateDetails {
                global_position: global,
                local_position: position,
                delta,
                primary_delta: self.primary_delta(delta),
                kind: contact.kind,
            });
            let claim = !state.accepted
                && self.primary_delta(position - contact.local).abs()
                    > self.slop(contact.kind, &contact.settings);
            (update, claim)
        };
        if let Some(details) = update {
            invoke_callback(
                self.callbacks.on_update.clone(),
                || {},
                |callback| callback(details),
            );
        } else if claim {
            self.contact.accept();
        }
    }
    fn begin_accepted_drag(&self) {
        let Some(contact) = self.contact.current() else {
            return;
        };
        let (start, update) = {
            let mut state = self.drag_state.borrow_mut();
            let Some(state) = state.as_mut() else {
                return;
            };
            if state.accepted || state.id != contact.id {
                return;
            }
            state.accepted = true;
            let (local, global) = match self.start_behavior {
                DragStartBehavior::Down => (contact.local, contact.global),
                DragStartBehavior::Start => (state.last_position, state.last_global_position),
            };
            let start = DragStartDetails {
                global_position: global,
                local_position: local,
                kind: contact.kind,
                timestamp: state.last_time,
            };
            let delta = self.project_delta(state.last_position - contact.local);
            let update = (self.start_behavior == DragStartBehavior::Down && delta != Offset::ZERO)
                .then_some(DragUpdateDetails {
                    global_position: state.last_global_position,
                    local_position: contact.local + delta,
                    delta,
                    primary_delta: self.primary_delta(delta),
                    kind: contact.kind,
                });
            (start, update)
        };
        invoke_callback(
            self.callbacks.on_start.clone(),
            || {},
            |callback| callback(start),
        );
        if self.contact.is_current(contact.id)
            && let Some(details) = update
        {
            invoke_callback(
                self.callbacks.on_update.clone(),
                || {},
                |callback| callback(details),
            );
        }
    }
    fn terminate(
        &self,
        reason: GestureEndReason,
        dispatch: Option<PointerDispatch<'_>>,
    ) -> CancelOutcome {
        let Some(contact) = self.contact.current() else {
            return CancelOutcome::Idle;
        };
        let clock = self.contact.now();
        if !self.contact.is_current(contact.id) {
            return CancelOutcome::Idle;
        }
        let state = self.drag_state.borrow_mut().take();
        let Some(mut state) = state else {
            self.contact.cancel();
            return CancelOutcome::Cancelled;
        };
        let now = state
            .timeline
            .instant(dispatch.and_then(|d| event_time(d.local)), clock);
        let velocity = state.velocity_tracker.velocity_at(now);
        let (position, global) = dispatch
            .filter(|d| matches!(d.local, PointerEvent::Up(_)))
            .map_or((state.last_position, state.last_global_position), |d| {
                (d.local.position(), d.global.position())
            });
        if state.accepted {
            let before = || {
                if reason == GestureEndReason::Completed {
                    self.contact.finish();
                } else {
                    self.contact.cancel();
                }
            };
            invoke_callback(self.callbacks.on_end.clone(), before, |callback| {
                callback(DragEndDetails {
                    reason,
                    velocity,
                    local_position: position,
                    global_position: global,
                    primary_velocity: self.primary_delta(velocity.pixels_per_second),
                })
            });
        } else {
            invoke_callback(
                self.callbacks.on_cancel.clone(),
                || {
                    if reason == GestureEndReason::Completed {
                        self.contact.withdraw();
                    } else {
                        self.contact.cancel();
                    }
                },
                |callback| callback(),
            );
        }
        CancelOutcome::Cancelled
    }
}
impl GestureRecognizer for DragGestureRecognizer {
    fn add_pointer(&self, dispatch: PointerDispatch<'_>) {
        if !is_primary_down(dispatch.local) {
            return;
        }
        if let Some(current) = self.contact.current() {
            if current.pointer != dispatch.local.pointer_id() {
                return;
            }
            self.cancel();
            if self.contact.current().is_some() {
                return;
            }
        }
        let Ok(id) = self.contact.begin(dispatch, &self.settings) else {
            return;
        };
        let Some(contact) = self.contact.current() else {
            return;
        };
        let mut timeline = EventTimeline::default();
        let now = timeline.instant(event_time(dispatch.local), self.contact.now());
        if !self.contact.is_current(id) {
            return;
        }
        let mut velocity_tracker = VelocityTracker::new();
        velocity_tracker.add_position(now, contact.local);
        *self.drag_state.borrow_mut() = Some(DragState {
            id,
            accepted: false,
            last_position: contact.local,
            last_global_position: contact.global,
            last_time: now,
            timeline,
            velocity_tracker,
        });
        invoke_callback(
            self.callbacks.on_down.clone(),
            || {},
            |callback| {
                callback(DragDownDetails {
                    global_position: contact.global,
                    local_position: contact.local,
                    kind: contact.kind,
                })
            },
        );
    }
    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        if !self.contact.tracks(dispatch.local.pointer_id()) {
            return;
        }
        match dispatch.local {
            PointerEvent::Move(_) => self.handle_move(dispatch),
            PointerEvent::Up(_) => {
                self.terminate(GestureEndReason::Completed, Some(dispatch));
            }
            PointerEvent::Cancel(_) => {
                self.terminate(GestureEndReason::Cancelled, Some(dispatch));
            }
            _ => {}
        }
    }
    fn cancel(&self) -> CancelOutcome {
        self.terminate(GestureEndReason::Cancelled, None)
    }
}
impl GestureArenaMember for DragGestureRecognizer {
    fn accept_gesture(&self, pointer: PointerId) {
        if self.contact.tracks(pointer) {
            self.begin_accepted_drag();
        }
    }
    fn reject_gesture(&self, pointer: PointerId) {
        if !self.contact.tracks(pointer) {
            return;
        }
        let active = self.drag_state.borrow_mut().take().is_some();
        if active {
            invoke_callback(
                self.callbacks.on_cancel.clone(),
                || {
                    self.contact.withdraw();
                },
                |callback| callback(),
            );
        } else {
            self.contact.withdraw();
        }
    }
}
