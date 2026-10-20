//! A precision touchpad's pan as a drag: the native pan/zoom stream of one
//! source, read as the motion of its focal point.
//!
//! The session is the recognizer's own: it never joins the arena, so it
//! neither delays nor loses to a contact. A route that claims native delivery
//! (`PanZoomDispatch::claim`) stops its ancestors from hearing the same
//! gesture, as a scale does. Zoom and rotation are not a drag's, and are
//! ignored.

use super::{
    DragAxis, DragCallbacks, DragDownDetails, DragEndDetails, DragGestureRecognizer,
    DragStartBehavior, DragStartDetails, DragUpdateDetails, GestureEndReason,
};
use crate::gesture::recognizers::{
    callback_containment::invoke_callback, contact::ContactId, recognizer::EventTimeline,
    scale::PanZoomDisposition,
};
use crate::{
    containment::RoutePanic,
    gesture::constraints::on_rail,
    gesture::settings::GestureSettings,
    gesture::{Velocity, VelocityTracker},
    pointer::PanZoomDispatch,
};
use flui_foundation::geometry::{Axis, Offset};
use flui_platform_api::pointer::{PanZoomPhase, PointerInfo};
use web_time::Instant;

/// One source's pan session.
pub(super) struct NativeDrag {
    id: ContactId,
    source: PointerInfo,
    settings: GestureSettings,
    started: bool,
    /// Where the session began: its motion is measured from here.
    origin: Offset<f64>,
    global_origin: Offset<f64>,
    last: Offset<f64>,
    global_last: Offset<f64>,
    /// The rail a railed free drag locked onto at its start.
    rail: Option<Axis>,
    timeline: EventTimeline,
    velocity: VelocityTracker,
    global_velocity: VelocityTracker,
}

impl NativeDrag {
    fn new(
        id: ContactId,
        source: PointerInfo,
        position: Offset<f64>,
        global: Offset<f64>,
        settings: GestureSettings,
    ) -> Self {
        let estimator = settings.velocity_estimator();
        Self {
            id,
            source,
            settings,
            started: false,
            origin: position,
            global_origin: global,
            last: position,
            global_last: global,
            rail: None,
            timeline: EventTimeline::default(),
            velocity: VelocityTracker::for_gesture(source.kind, estimator),
            global_velocity: VelocityTracker::for_gesture(source.kind, estimator),
        }
    }

    fn matches(&self, source: &PointerInfo) -> bool {
        self.source.id == source.id && self.source.device == source.device
    }
}

enum Step {
    Down(DragDownDetails),
    Start(DragStartDetails),
    Update(DragUpdateDetails),
    End(DragEndDetails),
    Cancel,
}

impl DragGestureRecognizer {
    /// Read one native pan/zoom event of a precision touchpad as drag motion.
    ///
    /// `Start` stages a session and reports it admitted without callbacks or a
    /// claim. The first `Update` that carries the focal point past this
    /// drag's slop on its axis claims native delivery and starts the drag; the
    /// rest follow it and report handled. An `Update` with no `Start` is one
    /// independent step. A terminal event ends a started drag with its
    /// release velocity, or retires a staged session.
    ///
    /// A source the recognizer's devices do not admit, or any event while a
    /// contact is tracked, is ignored.
    pub fn handle_pan_zoom(&self, dispatch: PanZoomDispatch<'_>) -> PanZoomDisposition {
        let (local, global) = (dispatch.local, dispatch.global);
        let source = *local.pointer();
        if source.id != global.pointer().id
            || source.device != global.pointer().device
            || !self.allowed_devices.contains_kind(source.kind)
            || !self.contacts.borrow().is_empty()
        {
            return PanZoomDisposition::Ignored;
        }
        let position = {
            let point = local.position.get();
            Offset::new(point.x, point.y)
        };
        let global_position = {
            let point = global.position.get();
            Offset::new(point.x, point.y)
        };
        if !position.is_finite() || !global_position.is_finite() {
            return PanZoomDisposition::Ignored;
        }
        let clock = self.arena.sample_now();
        match local.phase {
            PanZoomPhase::Start => {
                let Some(id) = ContactId::next(&self.last_contact) else {
                    return PanZoomDisposition::Ignored;
                };
                let settings = self.settings.snapshot_for(source.kind);
                let mut session = NativeDrag::new(id, source, position, global_position, settings);
                let time = session.timeline.instant(Some(local.time.as_nanos()), clock);
                session.velocity.add_position(time, position);
                session.global_velocity.add_position(time, global_position);
                let retired = self.native.replace(Some(session));
                let owner = self.this.clone();
                let current_owner = self.this.clone();
                dispatch.on_generation_retirement(
                    move || {
                        current_owner.upgrade().is_some_and(|owner| {
                            owner
                                .native
                                .borrow()
                                .as_ref()
                                .is_some_and(|session| session.id == id)
                        })
                    },
                    move || {
                        if let Some(owner) = owner.upgrade() {
                            let mut native = owner.native.borrow_mut();
                            if native.as_ref().is_some_and(|session| session.id == id) {
                                *native = None;
                            }
                        }
                    },
                );
                if retired.is_some_and(|session| session.started) {
                    self.deliver_native(Step::Cancel);
                }
                PanZoomDisposition::Admitted
            }
            PanZoomPhase::Update(_) => {
                if !matches!(global.phase, PanZoomPhase::Update(_)) {
                    return PanZoomDisposition::Ignored;
                }
                let independent = self.native.borrow().is_none();
                if independent {
                    let Some(id) = ContactId::next(&self.last_contact) else {
                        return PanZoomDisposition::Ignored;
                    };
                    let settings = self.settings.snapshot_for(source.kind);
                    let mut session =
                        NativeDrag::new(id, source, position, global_position, settings);
                    let time = session.timeline.instant(Some(local.time.as_nanos()), clock);
                    session.velocity.add_position(time, position);
                    session.global_velocity.add_position(time, global_position);
                    *self.native.borrow_mut() = Some(session);
                }
                let steps = {
                    let mut native = self.native.borrow_mut();
                    let Some(session) = native.as_mut() else {
                        return PanZoomDisposition::Ignored;
                    };
                    if !session.matches(&source) {
                        return PanZoomDisposition::Ignored;
                    }
                    let time = session.timeline.instant(Some(local.time.as_nanos()), clock);
                    let travel = position - session.origin;
                    let mut steps = Vec::new();
                    if session.started {
                        let delta = self.project(position - session.last, session.rail);
                        steps.push(Step::Update(DragUpdateDetails {
                            global_position,
                            local_position: position,
                            delta,
                            global_delta: global_position - session.global_last,
                            primary_delta: self.primary_delta(delta),
                            kind: source.kind,
                        }));
                    } else {
                        // An independent step is a whole gesture: it needs no
                        // slop to count as one.
                        if !independent
                            && !self.exceeds_slop(source.kind, &session.settings, travel)
                        {
                            session.last = position;
                            session.global_last = global_position;
                            session.velocity.add_position(time, position);
                            session.global_velocity.add_position(time, global_position);
                            return PanZoomDisposition::Admitted;
                        }
                        session.started = true;
                        if self.axis == DragAxis::Free {
                            session.rail = self.axis_lock.rail(source.kind, travel);
                        }
                        let behavior = self.start_behavior;
                        let (start_local, start_global) = match behavior {
                            DragStartBehavior::Down => (session.origin, session.global_origin),
                            DragStartBehavior::Start => (position, global_position),
                        };
                        steps.push(Step::Down(DragDownDetails {
                            global_position: session.global_origin,
                            local_position: session.origin,
                            kind: source.kind,
                            device: source.device,
                            sequence: None,
                        }));
                        steps.push(Step::Start(DragStartDetails {
                            global_position: start_global,
                            local_position: start_local,
                            kind: source.kind,
                            device: source.device,
                            sequence: None,
                            timestamp: time,
                        }));
                        if behavior == DragStartBehavior::Down {
                            let delta = self.project(travel, session.rail);
                            if delta != Offset::ZERO {
                                steps.push(Step::Update(DragUpdateDetails {
                                    global_position,
                                    local_position: session.origin + delta,
                                    delta,
                                    global_delta: global_position - session.global_origin,
                                    primary_delta: self.primary_delta(delta),
                                    kind: source.kind,
                                }));
                            }
                        }
                    }
                    session.last = position;
                    session.global_last = global_position;
                    session.velocity.add_position(time, position);
                    session.global_velocity.add_position(time, global_position);
                    steps
                };
                let id = self.native.borrow().as_ref().map(|session| session.id);
                let mut first = RoutePanic::capture(|| dispatch.claim());
                let live = || {
                    self.native
                        .borrow()
                        .as_ref()
                        .is_some_and(|session| Some(session.id) == id)
                };
                for step in steps {
                    if !live() {
                        break;
                    }
                    RoutePanic::preserve_first(
                        &mut first,
                        RoutePanic::capture(|| self.deliver_native(step)),
                        "native drag step",
                    );
                }
                if independent && live() {
                    let session = self.native.borrow_mut().take();
                    if let Some(session) = session {
                        let end = self.end_native(session, GestureEndReason::Completed, clock);
                        RoutePanic::preserve_first(
                            &mut first,
                            RoutePanic::capture(|| self.deliver_native(Step::End(end))),
                            "native drag end",
                        );
                    }
                }
                super::finish_containment(first, std::thread::panicking());
                PanZoomDisposition::Handled
            }
            PanZoomPhase::End | PanZoomPhase::Cancelled => {
                if !dispatch.native_terminal_is_current() {
                    return PanZoomDisposition::Ignored;
                }
                let mut native = self.native.borrow_mut();
                if native
                    .as_ref()
                    .is_none_or(|session| !session.matches(&source))
                {
                    return PanZoomDisposition::Ignored;
                }
                let session = native
                    .take()
                    .expect("BUG: matched native terminal retains its session");
                drop(native);
                if !session.started {
                    return PanZoomDisposition::Admitted;
                }
                let reason = if matches!(local.phase, PanZoomPhase::Cancelled) {
                    GestureEndReason::Cancelled
                } else {
                    GestureEndReason::Completed
                };
                let end = self.end_native(session, reason, clock);
                self.deliver_native(Step::End(end));
                PanZoomDisposition::Handled
            }
            _ => PanZoomDisposition::Ignored,
        }
    }

    fn end_native(
        &self,
        mut session: NativeDrag,
        reason: GestureEndReason,
        clock: Instant,
    ) -> DragEndDetails {
        let now = session.timeline.instant(None, clock);
        let measured = session.velocity.velocity_at(now).pixels_per_second;
        let velocity = Velocity::new(on_rail(measured, session.rail));
        let global_velocity = session.global_velocity.velocity_at(now);
        let along = self.project(velocity.pixels_per_second, session.rail);
        let fling_velocity = if reason == GestureEndReason::Completed {
            session
                .settings
                .resolve_fling_velocity(session.source.kind, Velocity::new(along))
        } else {
            Velocity::ZERO
        };
        let global_fling_velocity = Velocity::new(super::to_root(
            fling_velocity.pixels_per_second,
            measured,
            global_velocity.pixels_per_second,
        ));
        DragEndDetails {
            reason,
            velocity,
            global_velocity,
            local_position: session.last,
            global_position: session.global_last,
            primary_velocity: self
                .primary_delta(velocity.pixels_per_second)
                .clamp(f64::MIN, f64::MAX),
            fling_velocity,
            global_fling_velocity,
        }
    }

    fn deliver_native(&self, step: Step) {
        let callbacks: &DragCallbacks = &self.callbacks;
        match step {
            Step::Down(details) => {
                invoke_callback(
                    callbacks.on_down.clone(),
                    || {},
                    |callback| callback(details),
                );
            }
            Step::Start(details) => {
                invoke_callback(
                    callbacks.on_start.clone(),
                    || {},
                    |callback| callback(details),
                );
            }
            Step::Update(details) => {
                invoke_callback(
                    callbacks.on_update.clone(),
                    || {},
                    |callback| callback(details),
                );
            }
            Step::End(details) => {
                invoke_callback(
                    callbacks.on_end.clone(),
                    || {},
                    |callback| callback(details),
                );
            }
            Step::Cancel => {
                invoke_callback(callbacks.on_cancel.clone(), || {}, |callback| callback());
            }
        }
    }
}
