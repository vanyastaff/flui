//! Independent owner-local drags, one client per accepted contact.
use super::{
    callback_containment::{
        finish_containment, invoke_callback, retire_callback, retire_callbacks, withdraw_cancelled,
    },
    contact::{ArenaMembership, ContactId},
    recognizer::{
        CancelOutcome, EventTimeline, GestureRecognizer, event_time, is_primary_down,
        motion_history,
    },
};
use crate::gesture::observer::GestureContest;
use crate::{
    containment::RoutePanic,
    events::{PointerEvent, PointerEventExt, PointerKind},
    gesture::arena::{GestureArena, GestureArenaEntry, GestureArenaMember, MotionRole},
    gesture::settings::{GestureSettings, GestureSettingsProvider},
    gesture::{PublishedPosition, VelocityTracker},
    ids::PointerId,
    pointer::PointerDispatch,
};
use flui_foundation::geometry::Offset;
use flui_platform_api::input_capabilities::PointerKindSet;
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
impl MultiDragAxis {
    /// The part each contact plays in the ownership table (ADR-0208).
    const fn motion_role(self) -> MotionRole {
        match self {
            Self::Free => MotionRole::Free,
            Self::Horizontal => MotionRole::Axial(flui_foundation::geometry::Axis::Horizontal),
            Self::Vertical => MotionRole::Axial(flui_foundation::geometry::Axis::Vertical),
        }
    }
}
/// Factory called after acceptance with the admitted Down position in the
/// receiving node's local coordinates. Updates carry the latest observed local
/// and root positions. Returning `None` rejects that contact.
pub type MultiDragStartCallback =
    Rc<dyn Fn(PointerId, Offset<f64>) -> Option<Rc<dyn MultiDragHandle>>>;
/// Movement delivered to one drag client.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct MultiDragUpdateDetails {
    /// Contact pointer identity.
    pub pointer_id: PointerId,
    /// Observed position in root coordinates.
    pub global_position: Offset<f64>,
    /// Position in receiving-node coordinates.
    pub local_position: Offset<f64>,
    /// Movement since the prior delivered sample.
    pub delta: Offset<f64>,
    /// Movement of this same sample in presentation coordinates.
    pub global_delta: Offset<f64>,
    /// Device kind captured on Down.
    pub kind: PointerKind,
    /// Event-clock timestamp.
    pub timestamp: Instant,
}
/// Terminal contact delivered to a drag client.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct MultiDragEndDetails {
    /// Contact pointer identity.
    pub pointer_id: PointerId,
    /// Final observed root-space position.
    pub global_position: Offset<f64>,
    /// Measured velocity in receiving-node local logical coordinates, using the
    /// contact's event clock. The terminal position above is in root space.
    pub velocity: crate::gesture::Velocity,
    /// Device kind captured on Down.
    pub kind: PointerKind,
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
    settings: GestureSettingsProvider,
    allowed_devices: PointerKindSet,
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
    pub fn settings(mut self, settings: impl Into<GestureSettingsProvider>) -> Self {
        self.settings = settings.into();
        self
    }

    /// Admit only contacts of these device kinds, before any arena rule runs
    /// (ADR-0208 rule 1): a pen that draws beside a finger that scrolls. The
    /// full set, the default, admits every device.
    #[must_use]
    pub fn allowed_devices(mut self, devices: PointerKindSet) -> Self {
        self.allowed_devices = devices;
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
            membership: ArenaMembership::new(self.arena, this.clone())
                .with_motion_role(self.axis.motion_role()),
            axis: self.axis,
            settings: self.settings,
            allowed_devices: self.allowed_devices,
            callbacks: self.callbacks,
            pointers: RefCell::new(BTreeMap::new()),
            generation: Cell::new(0),
        })
    }
}

struct MultiDragPointerState {
    id: ContactId,
    initial_position: Offset<f64>,
    /// Root-space Down position, from which the ownership table measures.
    initial_global_position: Offset<f64>,
    last_position: Offset<f64>,
    last_global_position: Offset<f64>,
    kind: PointerKind,
    settings: GestureSettings,
    pending_delta: Offset<f64>,
    pending_global_delta: Offset<f64>,
    accepted: bool,
    client: Option<Rc<dyn MultiDragHandle>>,
    velocity_tracker: VelocityTracker,
    timeline: EventTimeline,
    last_time: Instant,
    arena_entry: GestureArenaEntry,
    /// Published position, smoothed after acceptance when the contact's
    /// settings select it.
    published: PublishedPosition,
}

/// Recognizes independent drags for all admitted contacts.
pub struct MultiDragGestureRecognizer {
    membership: ArenaMembership,
    axis: MultiDragAxis,
    settings: GestureSettingsProvider,
    allowed_devices: PointerKindSet,
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
            settings: GestureSettingsProvider::default(),
            allowed_devices: PointerKindSet::all(),
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
                let clock = match RoutePanic::try_run(|| self.membership.arena().sample_now()) {
                    Ok(now) => now,
                    Err(panic) => {
                        RoutePanic::preserve_first(first, Some(panic), "multi-drag terminal clock");
                        state.last_time
                    }
                };
                let now = state.timeline.instant(event_time(dispatch.local), clock);
                let details = MultiDragEndDetails {
                    pointer_id: pointer,
                    global_position: dispatch
                        .global
                        .position()
                        .unwrap_or(state.last_global_position),
                    velocity: state.velocity_tracker.velocity_at(now),
                    kind: state.kind,
                };
                RoutePanic::capture(|| {
                    invoke_callback(Some(client.clone()), || {}, |client| client.end(details));
                })
            } else {
                RoutePanic::capture(|| {
                    invoke_callback(Some(client.clone()), || {}, MultiDragHandle::cancel);
                })
            };
            RoutePanic::preserve_first(first, delivered, "multi-drag terminal client");
        }
        retire_callback(client, first);
    }
    fn handle_move(&self, dispatch: PointerDispatch<'_>) {
        let Some(pointer) = dispatch.local.pointer_id() else {
            return;
        };
        let (Some(position), Some(global)) =
            (dispatch.local.position(), dispatch.global.position())
        else {
            return;
        };
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
        let history = motion_history(dispatch.local);
        let clock = self.membership.arena().sample_now();
        let (client, update, claim) = {
            let mut pointers = self.pointers.borrow_mut();
            let Some(state) = pointers.get_mut(&pointer).filter(|state| state.id == id) else {
                return;
            };
            for (stamp, position) in history {
                let timestamp = state.timeline.instant(stamp, clock);
                state.velocity_tracker.add_position(timestamp, position);
            }
            let timestamp = state.timeline.instant(event_time(dispatch.local), clock);
            let (published, published_global, delta) = if state.client.is_some() {
                state.published.publish(timestamp, position, global)
            } else {
                state.published.follow(position);
                (position, global, position - state.last_position)
            };
            let delta = delta.to_delta();
            let global_delta = global - state.last_global_position;
            let pending = state.pending_delta + delta;
            let pending_global = state.pending_global_delta + global_delta;
            if !delta.dx.is_finite()
                || !delta.dy.is_finite()
                || !pending.dx.is_finite()
                || !pending.dy.is_finite()
                || !global_delta.dx.is_finite()
                || !global_delta.dy.is_finite()
                || !pending_global.dx.is_finite()
                || !pending_global.dy.is_finite()
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
                        global_position: published_global,
                        local_position: published,
                        delta,
                        global_delta,
                        kind: state.kind,
                        timestamp,
                    }),
                    None,
                )
            } else {
                state.pending_delta = pending;
                state.pending_global_delta = pending_global;
                let crossed = super::recognizer::measured_travel(
                    dispatch,
                    state.initial_position,
                    state.initial_global_position,
                )
                .any(|delta| {
                    let movement = match self.axis {
                        MultiDragAxis::Free => delta,
                        MultiDragAxis::Horizontal => Offset::new(delta.dx, 0.0),
                        MultiDragAxis::Vertical => Offset::new(0.0, delta.dy),
                    };
                    state.settings.exceeds_hit_slop(state.kind, movement)
                });
                (
                    None,
                    None,
                    (!state.accepted && crossed).then(|| {
                        (
                            state.arena_entry.clone(),
                            global - state.initial_global_position,
                        )
                    }),
                )
            }
        };
        if let (Some(client), Some(update)) = (client, update) {
            let mut failure = RoutePanic::capture(|| {
                invoke_callback(Some(client.clone()), || {}, |client| client.update(update));
            });
            retire_callback(Some(client), &mut failure);
            finish_containment(failure, std::thread::panicking());
        } else if let Some((entry, displacement)) = claim {
            entry.claim_motion(displacement);
        }
    }
    /// Whether `contest` is the one its pointer's tracked contact joined.
    fn tracks_contest(&self, contest: GestureContest) -> bool {
        self.pointers
            .borrow()
            .get(&contest.pointer())
            .is_some_and(|state| state.arena_entry.contest() == contest)
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
            );
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
                        global_position: state.last_global_position,
                        local_position: state.last_position,
                        delta: state.pending_delta,
                        global_delta: state.pending_global_delta,
                        kind: state.kind,
                        timestamp: state.last_time,
                    };
                    state.pending_delta = Offset::ZERO;
                    state.pending_global_delta = Offset::ZERO;
                    state.published.begin(
                        state.last_time,
                        state.last_position,
                        state.last_global_position,
                    );
                    state.client = Some(client.clone());
                    update
                })
        };
        if self.is_current(pointer, id)
            && let Some(update) = update
        {
            failure = RoutePanic::capture(|| {
                invoke_callback(Some(client.clone()), || {}, |client| client.update(update));
            });
        } else {
            failure = RoutePanic::capture(|| {
                invoke_callback(Some(client.clone()), || {}, MultiDragHandle::cancel);
            });
        }
        retire_callback(Some(client), &mut failure);
        finish_containment(failure, std::thread::panicking());
    }
}
impl GestureRecognizer for MultiDragGestureRecognizer {
    fn allowed_devices(&self) -> PointerKindSet {
        self.allowed_devices
    }
    fn add_pointer(&self, dispatch: PointerDispatch<'_>) {
        let Some(pointer) = dispatch.local.pointer_id() else {
            return;
        };
        let _span = tracing::info_span!(
            "multidrag.add_pointer",
            pointer = ?pointer,
            event = %crate::gesture::observability::GestureEvent::RecognizerAdded,
        );
        if !is_primary_down(dispatch.local) {
            return;
        }
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
        let settings = self
            .settings
            .snapshot_for(super::recognizer::admission_kind(dispatch.local));
        let clock = self.membership.arena().sample_now();
        if self.pointers.borrow().contains_key(&pointer) {
            return;
        }
        let Some(entry) = self.membership.join_from(pointer, dispatch.feeder()) else {
            return;
        };
        let PointerEvent::Down(data) = dispatch.local else {
            return;
        };
        let kind = data.pointer.kind;
        let mut timeline = EventTimeline::default();
        let now = timeline.instant(event_time(dispatch.local), clock);
        let mut velocity_tracker =
            VelocityTracker::for_gesture(kind, settings.velocity_estimator());
        velocity_tracker.add_position(now, position);
        let published = PublishedPosition::new(settings.position_smoothing(), position);
        self.pointers.borrow_mut().insert(
            pointer,
            MultiDragPointerState {
                id,
                initial_position: position,
                initial_global_position: global,
                last_position: position,
                last_global_position: global,
                kind,
                settings,
                pending_delta: Offset::ZERO,
                pending_global_delta: Offset::ZERO,
                accepted: false,
                client: None,
                velocity_tracker,
                timeline,
                last_time: now,
                arena_entry: entry,
                published,
            },
        );
    }
    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        let _span = tracing::info_span!(
            "multidrag.handle_event",
            kind = %crate::gesture::observability::pointer_event_kind(dispatch.local),
            event = %crate::gesture::observability::GestureEvent::EventReceived,
        );
        let Some(pointer) = dispatch.local.pointer_id() else {
            return;
        };
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
    fn accept_gesture(&self, contest: GestureContest) {
        if self.tracks_contest(contest) {
            self.start_accepted_drag(contest.pointer());
        }
    }
    fn reject_gesture(&self, contest: GestureContest) {
        if !self.tracks_contest(contest) {
            return;
        }
        let pointer = contest.pointer();
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
