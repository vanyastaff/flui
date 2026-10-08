//! Owner-local drag recognition with explicit touch continuation policy.
use super::{
    callback_containment::{
        finish_containment, invoke_callback, retire_callback, retire_callbacks,
    },
    contact::{ArenaMembership, ContactId, PrimaryContact},
    recognizer::{
        CancelOutcome, EventTimeline, GestureRecognizer, event_time, is_primary_down,
        motion_history,
    },
};
use crate::{
    arena::{GestureArena, GestureArenaMember},
    events::{PointerEvent, PointerEventExt, PointerKind},
    ids::PointerId,
    processing::VelocityTracker,
    routing::PointerDispatch,
    settings::{GestureSettings, GestureSettingsProvider},
    traits::DragAxis,
};
use flui_foundation::geometry::Offset;
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};
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

/// Which contacts may continue one drag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum DragPointerStrategy {
    /// The first admitted contact alone produces the drag.
    #[default]
    PrimaryOnly,
    /// Keep the active touch until release, then continue with the earliest
    /// admitted surviving touch from the same device, without a position jump.
    ContinueWithRemaining,
}
/// Details about a newly admitted drag contact.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct DragDownDetails {
    /// Root-space contact position.
    pub global_position: Offset<f64>,
    /// Receiving node's contact position.
    pub local_position: Offset<f64>,
    /// Device kind captured at admission.
    pub kind: PointerKind,
}
/// Details about an accepted drag.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DragStartDetails {
    /// Root-space initial position.
    pub global_position: Offset<f64>,
    /// Receiving node's initial position.
    pub local_position: Offset<f64>,
    /// Admitted device kind.
    pub kind: PointerKind,
    /// Event-clock instant at acceptance.
    pub timestamp: Instant,
}
/// Details about movement during an accepted drag.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
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
    pub kind: PointerKind,
}

impl DragUpdateDetails {
    /// Create an update from observed positions and axis movement.
    #[must_use]
    pub const fn new(
        global_position: Offset<f64>,
        local_position: Offset<f64>,
        delta: Offset<f64>,
        primary_delta: f64,
        kind: PointerKind,
    ) -> Self {
        Self {
            global_position,
            local_position,
            delta,
            primary_delta,
            kind,
        }
    }
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
#[non_exhaustive]
pub struct DragEndDetails {
    /// Completion or cancellation.
    pub reason: GestureEndReason,
    /// Velocity measured on the event clock.
    pub velocity: Velocity,
    /// Final observed root-space position.
    pub global_position: Offset<f64>,
    /// Final receiving-node position.
    pub local_position: Offset<f64>,
    /// Exact axis component, or the vector magnitude for a free drag.
    /// An unrepresentable free-drag magnitude uses the finite scalar ceiling
    /// `f64::MAX`; the raw vector components remain available in [`Self::velocity`].
    pub primary_velocity: f64,
    fling_velocity: Velocity,
}
impl DragEndDetails {
    /// Release velocity resolved with the gesture's admitted minimum and maximum.
    ///
    /// Cancellation and releases below the minimum give zero. The maximum caps
    /// vector magnitude while preserving direction. [`Self::velocity`] remains
    /// the raw finite-component measurement, independent of fling policy. A
    /// component estimate outside the representable range is refused as zero.
    #[must_use]
    pub const fn fling_velocity(&self) -> Velocity {
        self.fling_velocity
    }
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
    pointer_strategy: DragPointerStrategy,
    settings: GestureSettingsProvider,
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
    /// Configure how another touch may continue the same drag.
    #[must_use]
    pub fn pointer_strategy(mut self, strategy: DragPointerStrategy) -> Self {
        self.pointer_strategy = strategy;
        self
    }
    /// Choose settings captured by the first contact and retained by its drag group.
    #[must_use]
    pub fn settings(mut self, settings: impl Into<GestureSettingsProvider>) -> Self {
        self.settings = settings.into();
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
            arena: self.arena,
            this: this.clone(),
            axis: self.axis,
            start_behavior: self.start_behavior,
            pointer_strategy: self.pointer_strategy,
            settings: self.settings,
            callbacks: self.callbacks,
            contacts: RefCell::new(Vec::new()),
            active: Cell::new(None),
            started: Cell::new(false),
            last_contact: Cell::new(0),
        })
    }
}

struct DragState {
    accepted: bool,
    last_position: Offset<f64>,
    last_global_position: Offset<f64>,
    last_time: Instant,
    timeline: EventTimeline,
    velocity_tracker: VelocityTracker,
}

// A contact is the arena member, just as Tap's per-sequence participant is.
// Its exact admission identity prevents old verdicts reaching a reused pointer.
struct DragContact {
    owner: Weak<DragGestureRecognizer>,
    id: ContactId,
    pointer: PointerId,
    device: Option<flui_platform_api::pointer::DeviceId>,
    contact: PrimaryContact,
    state: RefCell<DragState>,
}
impl GestureArenaMember for DragContact {
    fn accept_gesture(&self, pointer: PointerId) {
        if pointer == self.pointer
            && let Some(owner) = self.owner.upgrade()
        {
            owner.accept_contact(pointer, self.id);
        }
    }
    fn reject_gesture(&self, pointer: PointerId) {
        if pointer == self.pointer
            && let Some(owner) = self.owner.upgrade()
        {
            owner.reject_contact(pointer, self.id);
        }
    }
}

/// Recognizes one drag, using immutable callbacks and policy.
/// An accepted cancellation delivers on_end(Cancelled); an unaccepted
/// cancellation delivers on_cancel. Both leave the recognizer reusable.
pub struct DragGestureRecognizer {
    contacts: RefCell<Vec<Rc<DragContact>>>,
    arena: GestureArena,
    this: Weak<DragGestureRecognizer>,
    active: Cell<Option<ContactId>>,
    started: Cell<bool>,
    last_contact: Cell<u64>,
    axis: DragAxis,
    start_behavior: DragStartBehavior,
    pointer_strategy: DragPointerStrategy,
    settings: GestureSettingsProvider,
    callbacks: DragCallbacks,
}
impl std::fmt::Debug for DragGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragGestureRecognizer")
            .field("axis", &self.axis)
            .field("start_behavior", &self.start_behavior)
            .field("pointer_strategy", &self.pointer_strategy)
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
            pointer_strategy: DragPointerStrategy::default(),
            settings: GestureSettingsProvider::default(),
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
    fn current(&self, pointer: PointerId) -> Option<Rc<DragContact>> {
        self.contacts
            .borrow()
            .iter()
            .find(|contact| contact.pointer == pointer)
            .cloned()
    }
    fn active_contact(&self) -> Option<Rc<DragContact>> {
        let id = self.active.get()?;
        self.contacts
            .borrow()
            .iter()
            .find(|contact| contact.id == id)
            .cloned()
    }
    fn is_current(&self, contact: &DragContact) -> bool {
        self.contacts
            .borrow()
            .iter()
            .any(|current| current.id == contact.id && current.pointer == contact.pointer)
    }
    fn remove(&self, pointer: PointerId, id: ContactId) -> Option<Rc<DragContact>> {
        let mut contacts = self.contacts.borrow_mut();
        let index = contacts
            .iter()
            .position(|contact| contact.pointer == pointer && contact.id == id)?;
        Some(contacts.remove(index))
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
    fn exceeds_slop(
        &self,
        kind: PointerKind,
        settings: &GestureSettings,
        delta: Offset<f64>,
    ) -> bool {
        match self.axis {
            DragAxis::Free => settings.exceeds_pan_slop_for(kind, delta),
            DragAxis::Vertical => delta.dy.abs() > settings.pan_slop_vertical_for(kind),
            DragAxis::Horizontal => delta.dx.abs() > settings.pan_slop_horizontal_for(kind),
        }
    }
    fn handle_move(&self, dispatch: PointerDispatch<'_>) {
        let Some(tracked) = dispatch
            .local
            .pointer_id()
            .and_then(|pointer| self.current(pointer))
        else {
            return;
        };
        let Some(snapshot) = tracked.contact.current() else {
            return;
        };
        let (Some(position), Some(global)) =
            (dispatch.local.position(), dispatch.global.position())
        else {
            return;
        };
        if !position.dx.is_finite()
            || !position.dy.is_finite()
            || !global.dx.is_finite()
            || !global.dy.is_finite()
        {
            self.cancel();
            return;
        }
        let history = motion_history(dispatch.local);
        let clock = self.arena.now();
        if !self.is_current(&tracked) {
            return;
        }
        let active = self.active.get() == Some(tracked.id);
        let (update, claim) = {
            let mut state = tracked.state.borrow_mut();
            for (stamp, position) in history {
                let timestamp = state.timeline.instant(stamp, clock);
                state.velocity_tracker.add_position(timestamp, position);
            }
            let now = state.timeline.instant(event_time(dispatch.local), clock);
            let delta = self.project_delta(position - state.last_position);
            if !delta.dx.is_finite()
                || !delta.dy.is_finite()
                || !self.primary_delta(delta).is_finite()
            {
                drop(state);
                self.cancel();
                return;
            }
            state.last_position = position;
            state.last_global_position = global;
            state.last_time = now;
            state.velocity_tracker.add_position(now, position);
            let update = (active && self.started.get()).then_some(DragUpdateDetails {
                global_position: global,
                local_position: position,
                delta,
                primary_delta: self.primary_delta(delta),
                kind: snapshot.kind,
            });
            let claim = active
                && !state.accepted
                && super::recognizer::measured_positions(dispatch.local).any(|position| {
                    self.exceeds_slop(snapshot.kind, &snapshot.settings, position - snapshot.local)
                });
            (update, claim)
        };
        if let Some(details) = update {
            invoke_callback(
                self.callbacks.on_update.clone(),
                || {},
                |callback| callback(details),
            );
        } else if claim {
            tracked.contact.accept();
        }
    }
    fn accept_contact(&self, pointer: PointerId, id: ContactId) {
        let Some(tracked) = self.current(pointer).filter(|contact| contact.id == id) else {
            return;
        };
        tracked.state.borrow_mut().accepted = true;
        if self.active.get() == Some(id) {
            self.begin_accepted_drag(&tracked);
        }
    }
    fn begin_accepted_drag(&self, tracked: &Rc<DragContact>) {
        let Some(snapshot) = tracked.contact.current() else {
            return;
        };
        if !self.is_current(tracked) || self.started.replace(true) {
            return;
        }
        let (start, update) = {
            let state = tracked.state.borrow();
            let (local, global) = match self.start_behavior {
                DragStartBehavior::Down => (snapshot.local, snapshot.global),
                DragStartBehavior::Start => (state.last_position, state.last_global_position),
            };
            let start = DragStartDetails {
                global_position: global,
                local_position: local,
                kind: snapshot.kind,
                timestamp: state.last_time,
            };
            let delta = self.project_delta(state.last_position - snapshot.local);
            let update = (self.start_behavior == DragStartBehavior::Down && delta != Offset::ZERO)
                .then_some(DragUpdateDetails {
                    global_position: state.last_global_position,
                    local_position: snapshot.local + delta,
                    delta,
                    primary_delta: self.primary_delta(delta),
                    kind: snapshot.kind,
                });
            (start, update)
        };
        let mut first = crate::routing::RoutePanic::capture(|| {
            invoke_callback(
                self.callbacks.on_start.clone(),
                || {},
                |callback| callback(start),
            );
        });
        if first.is_none()
            && self.is_current(tracked)
            && self.active.get() == Some(tracked.id)
            && let Some(details) = update
        {
            let candidate = crate::routing::RoutePanic::capture(|| {
                invoke_callback(
                    self.callbacks.on_update.clone(),
                    || {},
                    |callback| callback(details),
                );
            });
            crate::routing::RoutePanic::preserve_first(
                &mut first,
                candidate,
                "drag initial update",
            );
        }
        // Claim remaining contacts only after the active acceptance callback.
        // That callback can cancel the entire sequence or admit a replacement.
        if self.is_current(tracked) {
            let pending: Vec<_> = self
                .contacts
                .borrow()
                .iter()
                .filter(|contact| contact.id != tracked.id && !contact.state.borrow().accepted)
                .cloned()
                .collect();
            for contact in pending {
                if self.is_current(&contact) {
                    let candidate =
                        crate::routing::RoutePanic::capture(|| contact.contact.accept());
                    crate::routing::RoutePanic::preserve_first(
                        &mut first,
                        candidate,
                        "drag remaining contact claim",
                    );
                }
                retire_callback(Some(contact), &mut first);
            }
        }
        finish_containment(first, std::thread::panicking());
    }
    fn release_contact(&self, tracked: Rc<DragContact>) {
        let was_active = self.active.get() == Some(tracked.id);
        let outgoing = self.remove(tracked.pointer, tracked.id);
        let next = was_active
            .then(|| {
                self.contacts
                    .borrow()
                    .iter()
                    .find(|contact| contact.state.borrow().accepted)
                    .cloned()
            })
            .flatten();
        if was_active {
            self.active.set(next.as_ref().map(|contact| contact.id));
        }
        // The successor's own latest position and tracker already establish the
        // baseline. No synthetic move or inter-finger velocity sample is added.
        tracked.contact.finish();
        drop(outgoing);
        if let Some(next) = next
            && !self.started.get()
            && self.is_current(&next)
        {
            self.begin_accepted_drag(&next);
        }
    }
    fn reject_contact(&self, pointer: PointerId, id: ContactId) {
        let Some(tracked) = self.current(pointer).filter(|contact| contact.id == id) else {
            return;
        };
        if self.active.get() == Some(tracked.id) {
            self.terminate(GestureEndReason::Cancelled, None);
        } else if let Some(outgoing) = self.remove(tracked.pointer, tracked.id) {
            outgoing.contact.withdraw();
        }
    }
    fn terminate(
        &self,
        reason: GestureEndReason,
        dispatch: Option<PointerDispatch<'_>>,
    ) -> CancelOutcome {
        let Some(active) = self.active_contact() else {
            return CancelOutcome::Idle;
        };
        let Some(snapshot) = active.contact.current() else {
            return CancelOutcome::Idle;
        };
        // Detach the complete outgoing sequence before clocks, diagnostics,
        // arena verdicts or callbacks can admit a replacement.
        let outgoing = std::mem::take(&mut *self.contacts.borrow_mut());
        self.active.set(None);
        let accepted = self.started.replace(false);
        let (clock, clock_failure) = match crate::routing::RoutePanic::try_run(|| self.arena.now())
        {
            Ok(now) => (now, None),
            Err(failure) => (active.state.borrow().last_time, Some(failure)),
        };
        let (velocity, position, global) = {
            let mut state = active.state.borrow_mut();
            let now = state
                .timeline
                .instant(dispatch.and_then(|d| event_time(d.local)), clock);
            let velocity = state.velocity_tracker.velocity_at(now);
            let (position, global) = dispatch
                .filter(|d| matches!(d.local, PointerEvent::Up(_)))
                .map_or((state.last_position, state.last_global_position), |d| {
                    (
                        d.local.position().unwrap_or(state.last_position),
                        d.global.position().unwrap_or(state.last_global_position),
                    )
                });
            (velocity, position, global)
        };
        let fling_velocity = if reason == GestureEndReason::Completed {
            snapshot
                .settings
                .resolve_fling_velocity(snapshot.kind, velocity)
        } else {
            Velocity::ZERO
        };
        let retire = || {
            let mut first = clock_failure;
            for contact in outgoing {
                let candidate = crate::routing::RoutePanic::capture(|| {
                    // An unaccepted drag bows out before pointer-up can sweep
                    // the remaining competition; it must not win by order.
                    if accepted && reason == GestureEndReason::Completed && contact.id == active.id
                    {
                        contact.contact.finish();
                    } else if accepted {
                        contact.contact.cancel();
                    } else {
                        contact.contact.withdraw();
                    }
                });
                crate::routing::RoutePanic::preserve_first(
                    &mut first,
                    candidate,
                    "drag contact retirement",
                );
                retire_callback(Some(contact), &mut first);
            }
            finish_containment(first, std::thread::panicking());
        };
        if accepted {
            invoke_callback(self.callbacks.on_end.clone(), retire, |callback| {
                callback(DragEndDetails {
                    reason,
                    velocity,
                    local_position: position,
                    global_position: global,
                    primary_velocity: self
                        .primary_delta(velocity.pixels_per_second)
                        .clamp(f64::MIN, f64::MAX),
                    fling_velocity,
                });
            });
        } else {
            invoke_callback(self.callbacks.on_cancel.clone(), retire, |callback| {
                callback();
            });
        }
        CancelOutcome::Cancelled
    }
}
impl GestureRecognizer for DragGestureRecognizer {
    fn add_pointer(&self, dispatch: PointerDispatch<'_>) {
        if !is_primary_down(dispatch.local) {
            return;
        }
        let PointerEvent::Down(down) = dispatch.local else {
            return;
        };
        if let Some(existing) = self.current(down.pointer.id) {
            self.cancel();
            if self.active.get().is_some() || self.is_current(&existing) {
                return;
            }
        }
        let settings = if let Some(active) = self.active_contact() {
            let Some(snapshot) = active.contact.current() else {
                return;
            };
            if self.pointer_strategy == DragPointerStrategy::PrimaryOnly
                || snapshot.kind != PointerKind::Touch
                || down.pointer.kind != snapshot.kind
                || down.pointer.device != active.device
            {
                return;
            }
            snapshot.settings
        } else {
            self.settings.snapshot()
        };
        let previous = self.active.get();
        let Some(id) = ContactId::next(&self.last_contact) else {
            return;
        };
        let clock = self.arena.now();
        if self.active.get() != previous || self.current(down.pointer.id).is_some() {
            return;
        }
        let mut timeline = EventTimeline::default();
        let now = timeline.instant(event_time(dispatch.local), clock);
        let mut velocity_tracker =
            VelocityTracker::for_gesture(down.pointer.kind, settings.velocity_estimator());
        let Some(position) = dispatch.local.position() else {
            return;
        };
        let Some(global) = dispatch.global.position() else {
            return;
        };
        velocity_tracker.add_position(now, position);
        let tracked = Rc::<DragContact>::new_cyclic(|this| DragContact {
            owner: self.this.clone(),
            id,
            pointer: down.pointer.id,
            device: down.pointer.device,
            contact: PrimaryContact::new(ArenaMembership::new(self.arena.clone(), this.clone())),
            state: RefCell::new(DragState {
                accepted: false,
                last_position: position,
                last_global_position: global,
                last_time: now,
                timeline,
                velocity_tracker,
            }),
        });
        if tracked.contact.begin(dispatch, &settings).is_err() {
            return;
        }
        if self.active.get() != previous || self.current(down.pointer.id).is_some() {
            return;
        }
        self.contacts.borrow_mut().push(tracked.clone());
        if previous.is_none() {
            self.active.set(Some(id));
        }
        invoke_callback(
            self.callbacks.on_down.clone(),
            || {},
            |callback| {
                callback(DragDownDetails {
                    global_position: global,
                    local_position: position,
                    kind: down.pointer.kind,
                });
            },
        );
        if self.started.get() && self.is_current(&tracked) {
            tracked.contact.accept();
        }
    }
    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        let Some(tracked) = dispatch
            .local
            .pointer_id()
            .and_then(|pointer| self.current(pointer))
        else {
            return;
        };
        let active = self.active.get() == Some(tracked.id);
        let has_remaining = self
            .contacts
            .borrow()
            .iter()
            .any(|contact| contact.id != tracked.id && contact.state.borrow().accepted);
        match dispatch.local {
            PointerEvent::Move(_) => self.handle_move(dispatch),
            PointerEvent::Up(_) if active && !has_remaining => {
                self.terminate(GestureEndReason::Completed, Some(dispatch));
            }
            PointerEvent::Up(_) => self.release_contact(tracked),
            PointerEvent::Cancel(_) if active => {
                self.terminate(GestureEndReason::Cancelled, Some(dispatch));
            }
            PointerEvent::Cancel(_) => {
                if let Some(outgoing) = self.remove(tracked.pointer, tracked.id) {
                    outgoing.contact.cancel();
                }
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
        if let Some(contact) = self.current(pointer) {
            self.accept_contact(pointer, contact.id);
        }
    }
    fn reject_gesture(&self, pointer: PointerId) {
        if let Some(contact) = self.current(pointer) {
            self.reject_contact(pointer, contact.id);
        }
    }
}
