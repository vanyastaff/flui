//! A discrete swipe along one axis (ADR-0208 §4).
//!
//! The swipe is an axial member of the arena: it claims its contact when the
//! contact crosses the pan slop on its axis, so the ownership table decides
//! between it and an orthogonal drag the way it decides any two axial
//! members. Once it owns the contact, a release fast enough to fling fires
//! [`on_swipe`](SwipeGestureRecognizerBuilder::on_swipe) with the edge the
//! swipe heads towards; a slow release fires nothing.

use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

use flui_foundation::geometry::{Axis, Offset};
use flui_platform_api::input_capabilities::PointerKindSet;

use super::{
    ArenaMembership, CancelOutcome, GestureRecognizer, PrimaryContact,
    callback_containment::{invoke_callback, retire_callbacks},
    recognizer::{EventTimeline, admission_kind, event_time, is_primary_down, motion_history},
};
use crate::{
    events::{PointerEvent, PointerEventExt, PointerKind},
    gesture::arena::{GestureArena, GestureArenaMember, MotionRole},
    gesture::settings::GestureSettingsProvider,
    gesture::{Velocity, VelocityTracker, on_rail},
    ids::PointerId,
};

/// An edge of the screen or of a region in physical terms: left is left in
/// every reading direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PhysicalEdge {
    /// The left edge.
    Left,
    /// The top edge.
    Top,
    /// The right edge.
    Right,
    /// The bottom edge.
    Bottom,
}

impl PhysicalEdge {
    /// The axis across this edge: a swipe from it or towards it runs along
    /// this axis.
    #[must_use]
    pub const fn axis(self) -> Axis {
        match self {
            Self::Left | Self::Right => Axis::Horizontal,
            Self::Top | Self::Bottom => Axis::Vertical,
        }
    }

    /// The edge a motion of `component` along `axis` heads towards.
    fn towards(axis: Axis, component: f64) -> Self {
        match (axis, component >= 0.0) {
            (Axis::Horizontal, true) => Self::Right,
            (Axis::Horizontal, false) => Self::Left,
            (Axis::Vertical, true) => Self::Bottom,
            (Axis::Vertical, false) => Self::Top,
        }
    }

    const fn opposite(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Top => Self::Bottom,
            Self::Right => Self::Left,
            Self::Bottom => Self::Top,
        }
    }
}

/// Where a swipe may start and which way it may go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SwipeStart {
    /// Anywhere on the recognizer's region, either way along the axis.
    Anywhere(Axis),
    /// Away from this edge, across it. Attach the recognizer to a region
    /// along the edge: the region decides where the swipe may start, the
    /// recognizer which way it goes.
    FromEdge(PhysicalEdge),
}

impl SwipeStart {
    const fn axis(self) -> Axis {
        match self {
            Self::Anywhere(axis) => axis,
            Self::FromEdge(edge) => edge.axis(),
        }
    }

    /// Whether a swipe heading towards `edge` is one this start admits.
    fn admits(self, edge: PhysicalEdge) -> bool {
        match self {
            Self::Anywhere(_) => true,
            Self::FromEdge(from) => from.opposite() == edge,
        }
    }
}

/// A recognized swipe.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SwipeDetails {
    /// The edge the swipe heads towards.
    pub towards: PhysicalEdge,
    /// Release velocity along the swipe's axis, within the fling limits.
    pub velocity: Velocity,
    /// Device kind of the contact.
    pub kind: PointerKind,
}

type SwipeCallback = Rc<dyn Fn(SwipeDetails)>;

struct SwipeCallbacks {
    on_swipe: Option<SwipeCallback>,
}

impl Drop for SwipeCallbacks {
    fn drop(&mut self) {
        retire_callbacks!(self; on_swipe);
    }
}

struct SwipeState {
    accepted: bool,
    /// The contact crossed the slop on the swipe's axis, the admitted way.
    crossed: bool,
    timeline: EventTimeline,
    velocity: VelocityTracker,
}

/// Recognizes a swipe; see the `swipe` module documentation.
pub struct SwipeGestureRecognizer {
    contact: PrimaryContact,
    start: SwipeStart,
    settings: GestureSettingsProvider,
    allowed_devices: PointerKindSet,
    state: RefCell<Option<SwipeState>>,
    callbacks: SwipeCallbacks,
}

impl std::fmt::Debug for SwipeGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SwipeGestureRecognizer")
            .field("start", &self.start)
            .finish_non_exhaustive()
    }
}

/// Construction policy, frozen before the recognizer gains shared ownership.
#[must_use]
pub struct SwipeGestureRecognizerBuilder {
    arena: GestureArena,
    start: SwipeStart,
    settings: GestureSettingsProvider,
    allowed_devices: PointerKindSet,
    on_swipe: Option<SwipeCallback>,
}

impl std::fmt::Debug for SwipeGestureRecognizerBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SwipeGestureRecognizerBuilder")
            .field("start", &self.start)
            .finish_non_exhaustive()
    }
}

impl SwipeGestureRecognizerBuilder {
    /// Slops and fling limits, captured by each admitted contact.
    pub fn settings(mut self, settings: impl Into<GestureSettingsProvider>) -> Self {
        self.settings = settings.into();
        self
    }

    /// Admit only contacts of these device kinds (ADR-0208 rule 1).
    pub fn allowed_devices(mut self, devices: PointerKindSet) -> Self {
        self.allowed_devices = devices;
        self
    }

    /// Called once per recognized swipe, at its release.
    pub fn on_swipe(mut self, callback: impl Fn(SwipeDetails) + 'static) -> Self {
        self.on_swipe = Some(Rc::new(callback));
        self
    }

    /// Build an owner-local recognizer.
    pub fn build(self) -> Rc<SwipeGestureRecognizer> {
        Rc::new_cyclic(|this: &Weak<SwipeGestureRecognizer>| {
            let member: Weak<dyn GestureArenaMember> = this.clone();
            SwipeGestureRecognizer {
                contact: PrimaryContact::new(
                    ArenaMembership::new(self.arena, member)
                        .with_motion_role(MotionRole::Axial(self.start.axis())),
                ),
                start: self.start,
                settings: self.settings,
                allowed_devices: self.allowed_devices,
                state: RefCell::new(None),
                callbacks: SwipeCallbacks {
                    on_swipe: self.on_swipe,
                },
            }
        })
    }
}

fn component(offset: Offset<f64>, axis: Axis) -> f64 {
    match axis {
        Axis::Horizontal => offset.dx,
        Axis::Vertical => offset.dy,
    }
}

impl SwipeGestureRecognizer {
    /// Begin construction for swipes that `start` admits.
    pub fn builder(arena: GestureArena, start: SwipeStart) -> SwipeGestureRecognizerBuilder {
        SwipeGestureRecognizerBuilder {
            arena,
            start,
            settings: GestureSettingsProvider::default(),
            allowed_devices: PointerKindSet::all(),
            on_swipe: None,
        }
    }

    /// Where swipes may start and which way they may go.
    #[must_use]
    pub fn start(&self) -> SwipeStart {
        self.start
    }

    fn handle_move(&self, event: &PointerEvent, global: &PointerEvent) {
        let (Some(snapshot), Some(position), Some(global)) =
            (self.contact.current(), event.position(), global.position())
        else {
            return;
        };
        let axis = self.start.axis();
        let now = self.contact.arena().sample_now();
        let claim = {
            let mut state = self.state.borrow_mut();
            let Some(state) = state.as_mut() else {
                return;
            };
            for (stamp, sample) in motion_history(event) {
                let time = state.timeline.instant(stamp, now);
                state.velocity.add_position(time, sample);
            }
            let time = state.timeline.instant(event_time(event), now);
            state.velocity.add_position(time, position);
            let travel = component(position - snapshot.local, axis);
            let slop = match axis {
                Axis::Horizontal => snapshot.settings.pan_slop_horizontal_for(snapshot.kind),
                Axis::Vertical => snapshot.settings.pan_slop_vertical_for(snapshot.kind),
            };
            if state.crossed || travel.abs() <= slop {
                None
            } else if self.start.admits(PhysicalEdge::towards(axis, travel)) {
                state.crossed = true;
                Some(true)
            } else {
                Some(false)
            }
        };
        match claim {
            // The ownership table decides on the displacement since Down.
            Some(true) => self.contact.claim_motion(global - snapshot.global),
            // Heading back into its edge: not this swipe.
            Some(false) => {
                self.state.replace(None);
                self.contact.withdraw();
            }
            None => {}
        }
    }

    fn handle_up(&self, event: &PointerEvent) {
        let Some(snapshot) = self.contact.current() else {
            return;
        };
        let now = self.contact.arena().sample_now();
        let swipe = self.state.take().and_then(|mut state| {
            if !(state.accepted && state.crossed) {
                return None;
            }
            let time = state.timeline.instant(event_time(event), now);
            let axis = self.start.axis();
            let rail = Some(axis);
            let raw = Velocity::new(on_rail(
                state.velocity.velocity_at(time).pixels_per_second,
                rail,
            ));
            let velocity = snapshot.settings.resolve_fling_velocity(snapshot.kind, raw);
            let along = component(velocity.pixels_per_second, axis);
            let towards = PhysicalEdge::towards(axis, along);
            (along != 0.0 && self.start.admits(towards)).then_some(SwipeDetails {
                towards,
                velocity,
                kind: snapshot.kind,
            })
        });
        self.contact.finish();
        if let Some(details) = swipe {
            invoke_callback(
                self.callbacks.on_swipe.clone(),
                || {},
                |callback| {
                    callback(details);
                },
            );
        }
    }
}

impl GestureRecognizer for SwipeGestureRecognizer {
    fn allowed_devices(&self) -> PointerKindSet {
        self.allowed_devices
    }

    fn add_pointer(&self, down: crate::pointer::PointerDispatch<'_>) {
        if !is_primary_down(down.local) {
            return;
        }
        let Some(position) = down.local.position() else {
            return;
        };
        let kind = admission_kind(down.local);
        let settings = self.settings.snapshot_for(kind);
        let estimator = settings.velocity_estimator();
        let now = self.contact.arena().sample_now();
        if self.contact.begin(down, &settings).is_err() {
            return;
        }
        let mut timeline = EventTimeline::default();
        let mut velocity = VelocityTracker::for_gesture(kind, estimator);
        velocity.add_position(timeline.instant(event_time(down.local), now), position);
        self.state.replace(Some(SwipeState {
            accepted: false,
            crossed: false,
            timeline,
            velocity,
        }));
    }

    fn handle_event(&self, dispatch: crate::pointer::PointerDispatch<'_>) {
        if !dispatch
            .local
            .pointer_id()
            .is_some_and(|pointer| self.contact.tracks(pointer))
        {
            return;
        }
        match dispatch.local {
            PointerEvent::Move(_) => self.handle_move(dispatch.local, dispatch.global),
            PointerEvent::Up(_) => self.handle_up(dispatch.local),
            PointerEvent::Cancel(_) => {
                let _ = self.cancel();
            }
            _ => {}
        }
    }

    fn cancel(&self) -> CancelOutcome {
        self.state.replace(None);
        if self.contact.cancel().is_some() {
            CancelOutcome::Cancelled
        } else {
            CancelOutcome::Idle
        }
    }
}

impl GestureArenaMember for SwipeGestureRecognizer {
    fn accept_gesture(&self, pointer: PointerId) {
        if self.contact.tracks(pointer)
            && let Some(state) = self.state.borrow_mut().as_mut()
        {
            state.accepted = true;
        }
    }

    fn reject_gesture(&self, pointer: PointerId) {
        if self.contact.tracks(pointer) {
            self.state.replace(None);
            self.contact.withdraw();
        }
    }
}
