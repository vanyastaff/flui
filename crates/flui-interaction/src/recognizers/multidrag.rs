//! Independent owner-local drags, one client per accepted contact.
use super::{
    callback_containment::{
        finish_containment, invoke_callback, retire_callback, retire_callbacks, withdraw_cancelled,
    },
    contact::{ArenaMembership, ContactId},
    recognizer::{CancelOutcome, EventTimeline, GestureRecognizer, event_time, is_primary_down},
};
use crate::{
    arena::{GestureArena, GestureArenaEntry, GestureArenaMember},
    events::{PointerEvent, PointerEventExt, PointerType},
    ids::PointerId,
    processing::VelocityTracker,
    routing::{PointerDispatch, RoutePanic},
    settings::GestureSettings,
};
use flui_foundation::geometry::Offset;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};
use web_time::Instant;

/// Owner-local client of one accepted drag.
/// `end` and `cancel` are terminal. The recognizer never delivers an update
/// after its contact has terminated. Dropping the recognizer retires clients
/// quietly; delivered cancellation is explicit through `cancel()`.
pub trait MultiDragHandle {
    /// Deliver movement on an accepted contact.
    fn update(&self, details: MultiDragUpdateDetails);
    /// Deliver the final contact and event-time velocity.
    fn end(&self, details: MultiDragEndDetails);
    /// Cancel an accepted contact.
    fn cancel(&self);
}
/// Axis used for each contact's movement threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiDragAxis {
    /// Admit movement in every direction.
    Free,
    /// Admit horizontal movement.
    Horizontal,
    /// Admit vertical movement.
    Vertical,
}
/// Factory called after acceptance. Returning `None` rejects that contact.
pub type MultiDragStartCallback =
    Rc<dyn Fn(PointerId, Offset<f64>) -> Option<Rc<dyn MultiDragHandle>>>;
/// Movement delivered to one drag client.
#[derive(Debug, Clone, PartialEq)]
pub struct MultiDragUpdateDetails {
    /// Contact pointer identity.
    pub pointer_id: PointerId,
    /// Observed position in root coordinates.
    pub global_position: Offset<f64>,
    /// Position in receiving-node coordinates.
    pub local_position: Offset<f64>,
    /// Movement since the prior delivered sample.
    pub delta: Offset<f64>,
    /// Device kind captured on Down.
    pub kind: PointerType,
    /// Event-clock timestamp.
    pub timestamp: Instant,
}
/// Terminal contact delivered to a drag client.
#[derive(Debug, Clone, PartialEq)]
pub struct MultiDragEndDetails {
    /// Contact pointer identity.
    pub pointer_id: PointerId,
    /// Final observed root-space position.
    pub global_position: Offset<f64>,
    /// Velocity on the contact's event clock.
    pub velocity: crate::processing::Velocity,
    /// Device kind captured on Down.
    pub kind: PointerType,
}

#[derive(Default)]
struct MultiDragCallbacks {
    on_start: Option<MultiDragStartCallback>,
}
impl Drop for MultiDragCallbacks {
    fn drop(&mut self) {
        retire_callbacks!(self; on_start);
    }
}
/// Collects immutable multi-drag policy and its client factory.
pub struct MultiDragGestureRecognizerBuilder {
    arena: GestureArena,
    axis: MultiDragAxis,
    settings: GestureSettings,
    callbacks: MultiDragCallbacks,
}
impl std::fmt::Debug for MultiDragGestureRecognizerBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiDragGestureRecognizerBuilder")
            .field("axis", &self.axis)
            .finish_non_exhaustive()
    }
}
impl MultiDragGestureRecognizerBuilder {
    /// Set settings captured separately for each Down.
    #[must_use]
    pub fn settings(mut self, settings: GestureSettings) -> Self {
        self.settings = settings;
        self
    }
    /// Set the accepted-contact factory.
    #[must_use]
    pub fn on_start(
        mut self,
        callback: impl Fn(PointerId, Offset<f64>) -> Option<Rc<dyn MultiDragHandle>> + 'static,
    ) -> Self {
        let outgoing = self.callbacks.on_start.replace(Rc::new(callback));
        let mut failure = None;
        retire_callback(outgoing, &mut failure);
        finish_containment(failure, std::thread::panicking());
        self
    }
    /// Build the recognizer on its UI owner.
    #[must_use]
    pub fn build(self) -> Rc<MultiDragGestureRecognizer> {
        Rc::<MultiDragGestureRecognizer>::new_cyclic(|this| MultiDragGestureRecognizer {
            membership: ArenaMembership::new(self.arena, this.clone()),
            axis: self.axis,
            settings: self.settings,
            callbacks: self.callbacks,
            pointers: RefCell::new(BTreeMap::new()),
            generation: Cell::new(0),
        })
    }
}

struct MultiDragPointerState {
    id: ContactId,
    initial_position: Offset<f64>,
    initial_global_position: Offset<f64>,
    last_position: Offset<f64>,
    last_global_position: Offset<f64>,
    kind: PointerType,
    slop: f64,
    pending_delta: Offset<f64>,
    accepted: bool,
    client: Option<Rc<dyn MultiDragHandle>>,
    velocity_tracker: VelocityTracker,
    timeline: EventTimeline,
    last_time: Instant,
    arena_entry: GestureArenaEntry,
}

/// Recognizes independent drags for all admitted contacts.
pub struct MultiDragGestureRecognizer {
    membership: ArenaMembership,
    axis: MultiDragAxis,
    settings: GestureSettings,
    callbacks: MultiDragCallbacks,
    pointers: RefCell<BTreeMap<PointerId, MultiDragPointerState>>,
    generation: Cell<u64>,
}
impl std::fmt::Debug for MultiDragGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiDragGestureRecognizer")
            .field("axis", &self.axis)
            .finish_non_exhaustive()
    }
}
impl MultiDragGestureRecognizer {
    /// Configure the recognizer before shared ownership begins.
    #[must_use]
    pub fn builder(arena: GestureArena, axis: MultiDragAxis) -> MultiDragGestureRecognizerBuilder {
        MultiDragGestureRecognizerBuilder {
            arena,
            axis,
            settings: GestureSettings::default(),
            callbacks: MultiDragCallbacks::default(),
        }
    }
    fn is_current(&self, pointer: PointerId, id: ContactId) -> bool {
        self.pointers
            .borrow()
            .get(&pointer)
            .is_some_and(|state| state.id == id)
    }
    fn remove_current(&self, pointer: PointerId, id: ContactId) -> Option<MultiDragPointerState> {
        let mut pointers = self.pointers.borrow_mut();
        if pointers.get(&pointer).is_some_and(|state| state.id == id) {
            pointers.remove(&pointer)
        } else {
            None
        }
    }
    fn finish_pointer(
        &self,
        pointer: PointerId,
        mut state: MultiDragPointerState,
        dispatch: Option<PointerDispatch<'_>>,
        first: &mut Option<RoutePanic>,
    ) {
        let retirement = RoutePanic::capture(|| {
            if dispatch.is_some_and(|d| matches!(d.local, PointerEvent::Up(_))) {
                state.arena_entry.reject_without_self();
            } else {
                withdraw_cancelled(&state.arena_entry, self.membership.arena());
            }
        });
        RoutePanic::preserve_first(first, retirement, "multi-drag contact retirement");
        let client = state.client.take();
        if let Some(client) = client.as_ref() {
            let delivered = if let Some(dispatch) =
                dispatch.filter(|d| matches!(d.local, PointerEvent::Up(_)))
            {
                let clock = match RoutePanic::try_run(|| self.membership.now()) {
                    Ok(now) => now,
                    Err(panic) => {
                        RoutePanic::preserve_first(first, Some(panic), "multi-drag terminal clock");
                        state.last_time
                    }
                };
                let now = state.timeline.instant(event_time(dispatch.local), clock);
                let details = MultiDragEndDetails {
                    pointer_id: pointer,
                    global_position: dispatch.global.position(),
                    velocity: state.velocity_tracker.velocity_at(now),
                    kind: state.kind,
                };
                RoutePanic::capture(|| {
                    invoke_callback(Some(client.clone()), || {}, |client| client.end(details))
                })
            } else {
                RoutePanic::capture(|| {
                    invoke_callback(Some(client.clone()), || {}, |client| client.cancel())
                })
            };
            RoutePanic::preserve_first(first, delivered, "multi-drag terminal client");
        }
        retire_callback(client, first);
    }
    fn handle_move(&self, dispatch: PointerDispatch<'_>) {
        let pointer = dispatch.local.pointer_id();
        let position = dispatch.local.position();
        let global = dispatch.global.position();
        let Some(id) = self.pointers.borrow().get(&pointer).map(|state| state.id) else {
            return;
        };
        if !position.dx.is_finite()
            || !position.dy.is_finite()
            || !global.dx.is_finite()
            || !global.dy.is_finite()
        {
            let removed = self.pointers.borrow_mut().remove(&pointer);
            if let Some(state) = removed {
                let mut failure = None;
                self.finish_pointer(pointer, state, None, &mut failure);
                finish_containment(failure, std::thread::panicking());
            }
            return;
        }
        let clock = self.membership.now();
        let (client, update, claim) = {
            let mut pointers = self.pointers.borrow_mut();
            let Some(state) = pointers.get_mut(&pointer).filter(|state| state.id == id) else {
                return;
            };
            let timestamp = state.timeline.instant(event_time(dispatch.local), clock);
            let delta = (position - state.last_position).to_delta();
            let pending = state.pending_delta + delta;
            if !delta.dx.is_finite()
                || !delta.dy.is_finite()
                || !pending.dx.is_finite()
                || !pending.dy.is_finite()
            {
                drop(pointers);
                if let Some(state) = self.remove_current(pointer, id) {
                    let mut failure = None;
                    self.finish_pointer(pointer, state, None, &mut failure);
                    finish_containment(failure, std::thread::panicking());
                }
                return;
            }
            state.last_position = position;
            state.last_global_position = global;
            state.last_time = timestamp;
            state.velocity_tracker.add_position(timestamp, position);
            if let Some(client) = state.client.clone() {
                (
                    Some(client),
                    Some(MultiDragUpdateDetails {
                        pointer_id: pointer,
                        global_position: global,
                        local_position: position,
                        delta,
                        kind: state.kind,
                        timestamp,
                    }),
                    None,
                )
            } else {
                state.pending_delta = pending;
                let magnitude = match self.axis {
                    MultiDragAxis::Free => state.pending_delta.distance(),
                    MultiDragAxis::Horizontal => state.pending_delta.dx.abs(),
                    MultiDragAxis::Vertical => state.pending_delta.dy.abs(),
                };
                (
                    None,
                    None,
                    (!state.accepted && magnitude > state.slop).then(|| state.arena_entry.clone()),
                )
            }
        };
        if let (Some(client), Some(update)) = (client, update) {
            let mut failure = RoutePanic::capture(|| {
                invoke_callback(Some(client.clone()), || {}, |client| client.update(update))
            });
            retire_callback(Some(client), &mut failure);
            finish_containment(failure, std::thread::panicking());
        } else if let Some(entry) = claim {
            entry.resolve(crate::arena::GestureDisposition::Accepted);
        }
    }
    fn start_accepted_drag(&self, pointer: PointerId) {
        let (id, initial_position) = {
            let mut pointers = self.pointers.borrow_mut();
            let Some(state) = pointers.get_mut(&pointer) else {
                return;
            };
            if state.accepted {
                return;
            }
            state.accepted = true;
            (state.id, state.initial_position)
        };
        let callback = self.callbacks.on_start.clone();
        let mut client = None;
        let mut failure = RoutePanic::capture(|| {
            invoke_callback(
                callback,
                || {},
                |callback| {
                    client = callback(pointer, initial_position);
                },
            )
        });
        if failure.is_some() || client.is_none() {
            if let Some(state) = self.remove_current(pointer, id) {
                self.finish_pointer(pointer, state, None, &mut failure);
            }
            retire_callback(client, &mut failure);
            finish_containment(failure, std::thread::panicking());
            return;
        }
        let Some(client) = client else {
            return;
        };
        let update = {
            let mut pointers = self.pointers.borrow_mut();
            pointers
                .get_mut(&pointer)
                .filter(|state| state.id == id)
                .map(|state| {
                    let update = MultiDragUpdateDetails {
                        pointer_id: pointer,
                        global_position: state.initial_global_position,
                        local_position: state.initial_position,
                        delta: state.pending_delta,
                        kind: state.kind,
                        timestamp: state.last_time,
                    };
                    state.pending_delta = Offset::ZERO;
                    state.client = Some(client.clone());
                    update
                })
        };
        if self.is_current(pointer, id)
            && let Some(update) = update
        {
            failure = RoutePanic::capture(|| {
                invoke_callback(Some(client.clone()), || {}, |client| client.update(update))
            });
        } else {
            failure = RoutePanic::capture(|| {
                invoke_callback(Some(client.clone()), || {}, |client| client.cancel())
            });
        }
        retire_callback(Some(client), &mut failure);
        finish_containment(failure, std::thread::panicking());
    }
}
impl GestureRecognizer for MultiDragGestureRecognizer {
    fn add_pointer(&self, dispatch: PointerDispatch<'_>) {
        let pointer = dispatch.local.pointer_id();
        let _span = tracing::info_span!(
            "multidrag.add_pointer",
            pointer = ?pointer,
            event = %crate::observability::GestureEvent::RecognizerAdded,
        );
        if !is_primary_down(dispatch.local) {
            return;
        }
        let pointer = dispatch.local.pointer_id();
        let position = dispatch.local.position();
        let global = dispatch.global.position();
        if !position.dx.is_finite()
            || !position.dy.is_finite()
            || !global.dx.is_finite()
            || !global.dy.is_finite()
        {
            return;
        }
        let removed = self.pointers.borrow_mut().remove(&pointer);
        if let Some(state) = removed {
            let mut failure = None;
            self.finish_pointer(pointer, state, None, &mut failure);
            finish_containment(failure, std::thread::panicking());
            if self.pointers.borrow().contains_key(&pointer) {
                return;
            }
        }
        let Some(id) = ContactId::next(&self.generation) else {
            return;
        };
        let clock = self.membership.now();
        if self.pointers.borrow().contains_key(&pointer) {
            return;
        }
        let Some(entry) = self.membership.join(pointer) else {
            return;
        };
        let PointerEvent::Down(data) = dispatch.local else {
            return;
        };
        let kind = data.pointer.pointer_type;
        let mut timeline = EventTimeline::default();
        let now = timeline.instant(event_time(dispatch.local), clock);
        let mut velocity_tracker = VelocityTracker::new();
        velocity_tracker.add_position(now, position);
        self.pointers.borrow_mut().insert(
            pointer,
            MultiDragPointerState {
                id,
                initial_position: position,
                initial_global_position: global,
                last_position: position,
                last_global_position: global,
                kind,
                slop: self.settings.hit_slop(kind),
                pending_delta: Offset::ZERO,
                accepted: false,
                client: None,
                velocity_tracker,
                timeline,
                last_time: now,
                arena_entry: entry,
            },
        );
    }
    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        let _span = tracing::info_span!(
            "multidrag.handle_event",
            kind = %crate::observability::pointer_event_kind(dispatch.local),
            event = %crate::observability::GestureEvent::EventReceived,
        );
        let pointer = dispatch.local.pointer_id();
        match dispatch.local {
            PointerEvent::Move(_) => self.handle_move(dispatch),
            PointerEvent::Up(_) | PointerEvent::Cancel(_) => {
                let removed = self.pointers.borrow_mut().remove(&pointer);
                if let Some(state) = removed {
                    let mut failure = None;
                    self.finish_pointer(pointer, state, Some(dispatch), &mut failure);
                    finish_containment(failure, std::thread::panicking());
                }
            }
            _ => {}
        }
    }
    fn cancel(&self) -> CancelOutcome {
        let removed = std::mem::take(&mut *self.pointers.borrow_mut());
        if removed.is_empty() {
            return CancelOutcome::Idle;
        }
        let mut failure = None;
        for (pointer, state) in removed {
            self.finish_pointer(pointer, state, None, &mut failure);
        }
        finish_containment(failure, std::thread::panicking());
        CancelOutcome::Cancelled
    }
}
impl GestureArenaMember for MultiDragGestureRecognizer {
    fn accept_gesture(&self, pointer: PointerId) {
        self.start_accepted_drag(pointer);
    }
    fn reject_gesture(&self, pointer: PointerId) {
        let removed = self.pointers.borrow_mut().remove(&pointer);
        if let Some(state) = removed {
            let mut failure = None;
            self.finish_pointer(pointer, state, None, &mut failure);
            finish_containment(failure, std::thread::panicking());
        }
    }
}
impl Drop for MultiDragGestureRecognizer {
    fn drop(&mut self) {
        let removed = std::mem::take(self.pointers.get_mut());
        let mut failure = None;
        for (_, mut state) in removed {
            state.arena_entry.withdraw_deferred();
            retire_callback(state.client.take(), &mut failure);
        }
        finish_containment(failure, std::thread::panicking());
    }
}
