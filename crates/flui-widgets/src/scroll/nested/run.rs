//! A frozen, weak chain and its exact per-link delivery obligations.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use flui_foundation::{
    InputSequenceId,
    geometry::{Point, Vec2},
    panic::PanicRecovery,
};
use smallvec::SmallVec;

use super::{
    ballistic::ScrollContinuation,
    connection::Connection,
    geometry::ViewportGeometry,
    link::{ConnectionLink, DispatchGate, ParticipantKind},
    types::{
        FlingHandoff, FlingStep, OverscrollBehavior, ScrollCause, ScrollDevice, ScrollEndReason,
        ScrollStep,
    },
};

struct Member {
    link: Weak<ConnectionLink>,
    started: Cell<bool>,
    ended: Cell<bool>,
    positive: Cell<Option<Vec2>>,
    negative: Cell<Option<Vec2>>,
}

/// One admitted source, including all later framework motion under its ID.
pub(super) struct SequenceRun {
    pub(super) sequence: InputSequenceId,
    pub(super) device: ScrollDevice,
    members: SmallVec<[Member; 4]>,
    ended: Cell<bool>,
    scratch: RefCell<SmallVec<[LiveMember; 4]>>,
    gate: Rc<DispatchGate>,
    released: Cell<bool>,
    continuation: RefCell<Option<ScrollContinuation>>,
    terminal_reason: Cell<ScrollEndReason>,
}

struct LiveMember {
    index: usize,
    link: Rc<ConnectionLink>,
    connection: Connection,
    geometry: ViewportGeometry,
    after_pre: Cell<Vec2>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ScrollOutcome {
    /// Actual participant consumption, in presentation coordinates.
    pub(super) consumed: Vec2,
    /// Motion still available after consumption and containment.
    pub(super) remaining: Vec2,
}

impl ScrollOutcome {
    /// A failed participant accounts for all motion it was offered, so no
    /// later participant receives it, and its link is contained.
    fn absorb_failure(&mut self, participant: &LiveMember) {
        self.consumed += self.remaining;
        self.remaining = Vec2::ZERO;
        participant.link.fail();
    }
}

impl SequenceRun {
    pub(super) fn new(
        leaf: &Rc<ConnectionLink>,
        sequence: InputSequenceId,
        device: ScrollDevice,
    ) -> Rc<Self> {
        let mut members = SmallVec::new();
        let mut link = Some(leaf.clone());
        while let Some(current) = link {
            if !current.alive.get() {
                break;
            }
            members.push(Member {
                link: Rc::downgrade(&current),
                started: Cell::new(false),
                ended: Cell::new(false),
                positive: Cell::new(None),
                negative: Cell::new(None),
            });
            link = current.parent.as_ref().and_then(Weak::upgrade);
        }
        members.reverse();
        let scratch = SmallVec::with_capacity(members.len());
        let run = Rc::new(Self {
            sequence,
            device,
            members,
            ended: Cell::new(false),
            scratch: RefCell::new(scratch),
            gate: leaf.gate.clone(),
            released: Cell::new(false),
            continuation: RefCell::new(None),
            terminal_reason: Cell::new(ScrollEndReason::Released),
        });
        for member in &run.members {
            if let Some(link) = member.link.upgrade() {
                link.register(&run);
            }
        }
        run
    }

    pub(super) fn has_ended(&self) -> bool {
        self.ended.get()
    }

    pub(super) fn end_link(&self, link: &Rc<ConnectionLink>, reason: ScrollEndReason) {
        for member in &self.members {
            if member.link.ptr_eq(&Rc::downgrade(link))
                && !member.ended.replace(true)
                && member.started.get()
            {
                link.gate
                    .end(link.connection.clone(), self.sequence, reason);
            }
        }
    }

    pub(super) fn end(&self, reason: ScrollEndReason) {
        if self.ended.replace(true) {
            return;
        }
        self.continuation.borrow_mut().take();
        for member in &self.members {
            if !member.ended.replace(true)
                && member.started.get()
                && let Some(link) = member.link.upgrade()
            {
                link.gate
                    .end(link.connection.clone(), self.sequence, reason);
            }
        }
    }

    pub(super) fn dispatch(
        &self,
        focal: Point,
        available: Vec2,
        cause: ScrollCause,
        recovery: &mut PanicRecovery,
    ) -> ScrollOutcome {
        let mut outcome = ScrollOutcome {
            consumed: Vec2::ZERO,
            remaining: available,
        };
        if self.ended.get() || !available.is_finite() {
            return outcome;
        }
        // The chain-wide gate is already held. Move the reusable buffer out:
        // no participant executes under a RefCell borrow, even for deep chains.
        let mut live = std::mem::take(&mut *self.scratch.borrow_mut());
        for (index, member) in self.members.iter().enumerate() {
            if cause == ScrollCause::SideEffect
                && self
                    .continuation
                    .borrow()
                    .as_ref()
                    .is_some_and(|motion| index > motion.member)
            {
                break;
            }
            if member.ended.get() {
                continue;
            }
            let Some(link) = member.link.upgrade().filter(|link| link.alive.get()) else {
                continue;
            };
            let Some(connection) = link.connection.upgrade() else {
                continue;
            };
            let geometry = link
                .pipeline
                .as_ref()
                .zip(link.node.get())
                .and_then(|(pipeline, node)| ViewportGeometry::sample(pipeline, node, focal));
            if let Some(geometry) = geometry {
                live.push(LiveMember {
                    index,
                    link,
                    connection,
                    geometry,
                    after_pre: Cell::new(Vec2::ZERO),
                });
            } else {
                recovery.retire(connection);
            }
        }
        for participant in &live {
            self.invoke(participant, available, cause, true, &mut outcome, recovery);
            participant.after_pre.set(outcome.consumed);
        }
        for participant in live.iter().rev() {
            self.invoke(participant, available, cause, false, &mut outcome, recovery);
        }
        // Retire each authored owner independently. An aggregate Drop could
        // otherwise double-panic before the containment boundary regains control.
        while let Some(participant) = live.pop() {
            recovery.retire(participant.connection);
            recovery.retire(participant.link);
        }
        *self.scratch.borrow_mut() = live;
        outcome
    }

    fn invoke(
        &self,
        participant: &LiveMember,
        total: Vec2,
        cause: ScrollCause,
        pre: bool,
        outcome: &mut ScrollOutcome,
        recovery: &mut PanicRecovery,
    ) {
        let member = &self.members[participant.index];
        if self.ended.get() || member.ended.get() || !participant.link.alive.get() {
            return;
        }
        let mut admitted = true;
        if cause == ScrollCause::Programmatic {
            let succeeded = recovery.scope().run_classified(
                flui_foundation::panic::FailureClass::Computation,
                |_| {
                    admitted = participant.connection.callbacks().accepts_programmatic();
                },
            );
            if !succeeded {
                outcome.absorb_failure(participant);
                return;
            }
        }
        if !admitted {
            return;
        }
        if !member.started.replace(true) {
            let succeeded = recovery.scope().run_classified(
                flui_foundation::panic::FailureClass::Delivery,
                |_| {
                    participant
                        .connection
                        .callbacks()
                        .sequence_started(self.sequence);
                },
            );
            if !succeeded {
                outcome.absorb_failure(participant);
                return;
            }
        }
        if self.ended.get() || member.ended.get() || !participant.link.alive.get() {
            return;
        }
        let Some(available) = participant.geometry.offer(outcome.remaining, total) else {
            return;
        };
        let direction = match participant.link.connection.kind() {
            ParticipantKind::Connection => None,
            ParticipantKind::Owner { axis, .. } => {
                let Some(original) = participant.geometry.local_delta(total) else {
                    return;
                };
                let component = match axis {
                    flui_foundation::geometry::Axis::Horizontal => original.x,
                    flui_foundation::geometry::Axis::Vertical => original.y,
                };
                if component == 0.0 {
                    return;
                }
                let Some(unit) = participant.geometry.root_axis(axis) else {
                    return;
                };
                let direction = unit * component.signum();
                let active_motion = cause == ScrollCause::SideEffect
                    && self
                        .continuation
                        .borrow()
                        .as_ref()
                        .is_some_and(|motion| motion.member == participant.index);
                if !active_motion
                    && self.members.iter().enumerate().any(|(index, other)| {
                        index != participant.index
                            && !other.ended.get()
                            && [other.positive.get(), other.negative.get()]
                                .into_iter()
                                .flatten()
                                .any(|latched| {
                                    latched.x.mul_add(direction.x, latched.y * direction.y)
                                        > 1.0 - 64.0 * f64::EPSILON
                                })
                    })
                {
                    return;
                }
                Some((component > 0.0, direction))
            }
        };
        let step = ScrollStep {
            sequence: self.sequence,
            device: self.device,
            cause,
            available,
            focal_point: participant.geometry.local_focal,
            device_pixel_ratio: if cause == ScrollCause::SideEffect {
                self.continuation
                    .borrow()
                    .as_ref()
                    .map_or(participant.geometry.device_pixel_ratio, |motion| {
                        motion.device_pixel_ratio
                    })
            } else {
                participant.geometry.device_pixel_ratio
            },
        };
        let Some(child) = participant
            .geometry
            .local_delta(outcome.consumed - participant.after_pre.get())
        else {
            return;
        };
        let mut consumed = Vec2::ZERO;
        let succeeded = recovery.scope().run_classified(
            flui_foundation::panic::FailureClass::Computation,
            |_| {
                consumed = if pre {
                    participant.connection.callbacks().pre_scroll(&step)
                } else {
                    match &participant.connection {
                        Connection::Authored(connection) => connection.post_scroll(&step, child),
                        Connection::Owner { connection, .. } => {
                            connection.consume_nested(&step, &participant.link.alive)
                        }
                    }
                };
            },
        );
        if !succeeded {
            outcome.absorb_failure(participant);
            return;
        }
        let checked = Vec2::new(
            component_consumption(consumed.x, available.x),
            component_consumption(consumed.y, available.y),
        );
        if consumed != checked {
            tracing::warn!(
                ?consumed,
                ?available,
                "nested scroll participant returned invalid consumption"
            );
        }
        let Some(root) = participant.geometry.root_delta(checked) else {
            return;
        };
        outcome.remaining -= root;
        outcome.consumed += root;
        if checked != Vec2::ZERO
            && let Some((positive, direction)) = direction
        {
            if positive {
                member.positive.set(Some(direction));
            } else {
                member.negative.set(Some(direction));
            }
        }
        if !pre
            && let ParticipantKind::Owner {
                axis,
                overscroll: OverscrollBehavior::Contain,
            } = participant.link.connection.kind()
        {
            let remaining = participant
                .geometry
                .local_delta(outcome.remaining)
                .unwrap_or(Vec2::ZERO);
            let blocked = match axis {
                flui_foundation::geometry::Axis::Horizontal => Vec2::new(remaining.x, 0.0),
                flui_foundation::geometry::Axis::Vertical => Vec2::new(0.0, remaining.y),
            };
            if let Some(blocked) = participant.geometry.root_delta(blocked) {
                outcome.remaining -= blocked;
            }
        }
    }

    pub(super) fn owns_continuation(&self, ticket: &Rc<()>) -> bool {
        !self.ended.get()
            && self
                .continuation
                .borrow()
                .as_ref()
                .is_some_and(|current| Rc::ptr_eq(&current.ticket, ticket))
    }

    pub(super) fn preempt_continuation(
        self: &Rc<Self>,
        continuation: &ScrollContinuation,
        recovery: &mut PanicRecovery,
    ) {
        if self.owns_continuation(&continuation.ticket) {
            self.end(ScrollEndReason::Preempted);
        }
        if let Some(_guard) = self.gate.enter() {
            self.gate.drain(recovery);
        }
    }

    pub(super) fn fail_continuation(
        self: &Rc<Self>,
        continuation: &ScrollContinuation,
        recovery: &mut PanicRecovery,
    ) {
        if self.owns_continuation(&continuation.ticket) {
            self.end(ScrollEndReason::Cancelled);
        }
        if let Some(_guard) = self.gate.enter() {
            self.gate.drain(recovery);
        }
    }

    pub(super) fn recover_cancelled(
        self: &Rc<Self>,
        focal: Point,
        device_pixel_ratio: f64,
        recovery: &mut PanicRecovery,
    ) {
        if self.ended.get() || self.released.replace(true) {
            return;
        }
        self.terminal_reason.set(ScrollEndReason::Cancelled);
        if !self.offer_fling(
            self.members.len(),
            focal,
            Vec2::ZERO,
            device_pixel_ratio,
            recovery,
        ) {
            self.end(ScrollEndReason::Cancelled);
        }
    }

    pub(super) fn release(
        self: &Rc<Self>,
        focal: Point,
        velocity: Vec2,
        device_pixel_ratio: f64,
        recovery: &mut PanicRecovery,
    ) {
        if self.ended.get() || self.released.replace(true) {
            return;
        }
        for member in &self.members {
            if !member.started.get() || member.ended.get() {
                continue;
            }
            let Some(link) = member.link.upgrade().filter(|link| link.alive.get()) else {
                continue;
            };
            let Some(connection) = link.connection.upgrade() else {
                continue;
            };
            if let Some(step) = self.fling_step(&link, focal, velocity, device_pixel_ratio) {
                let succeeded = recovery.scope().run_classified(
                    flui_foundation::panic::FailureClass::Delivery,
                    |_| {
                        connection.callbacks().released(&step);
                    },
                );
                if !succeeded {
                    link.fail();
                }
            }
            recovery.retire(connection);
        }
        if !self.offer_fling(
            self.members.len(),
            focal,
            velocity,
            device_pixel_ratio,
            recovery,
        ) {
            self.end(ScrollEndReason::Released);
        }
    }

    fn fling_step(
        &self,
        link: &ConnectionLink,
        focal: Point,
        velocity: Vec2,
        device_pixel_ratio: f64,
    ) -> Option<FlingStep> {
        let geometry = ViewportGeometry::sample(link.pipeline.as_ref()?, link.node.get()?, focal)?;
        let velocity = geometry.local_velocity(velocity)?;
        Some(FlingStep {
            sequence: self.sequence,
            device: self.device,
            available: flui_interaction::Velocity {
                pixels_per_second: flui_foundation::geometry::Offset::new(velocity.x, velocity.y),
            },
            focal_point: geometry.local_focal,
            device_pixel_ratio,
        })
    }

    fn offer_fling(
        self: &Rc<Self>,
        before: usize,
        focal: Point,
        velocity: Vec2,
        device_pixel_ratio: f64,
        recovery: &mut PanicRecovery,
    ) -> bool {
        for (index, member) in self.members[..before].iter().enumerate().rev() {
            if self.ended.get() {
                return false;
            }
            if member.ended.get() || !member.started.get() {
                continue;
            }
            let Some(link) = member.link.upgrade().filter(|link| link.alive.get()) else {
                continue;
            };
            let Some(connection) = link.connection.upgrade() else {
                continue;
            };
            let Some(step) = self.fling_step(&link, focal, velocity, device_pixel_ratio) else {
                recovery.retire(connection);
                continue;
            };
            if self.terminal_reason.get() == ScrollEndReason::Cancelled
                && matches!(&connection, Connection::Authored(_))
            {
                recovery.retire(connection);
                continue;
            }
            let candidate = ScrollContinuation {
                run: Rc::downgrade(self),
                ticket: Rc::new(()),
                member: index,
                focal,
                device_pixel_ratio,
            };
            let outgoing = self.continuation.replace(Some(candidate.clone()));
            let mut handoff = FlingHandoff::Declined;
            let succeeded = recovery.scope().run_classified(
                flui_foundation::panic::FailureClass::Computation,
                |_| {
                    handoff = match &connection {
                        Connection::Authored(connection) => connection.post_fling(&step),
                        Connection::Owner { connection, .. } => {
                            connection.start_nested(&step, &link.alive, candidate.clone())
                        }
                    };
                },
            );
            if !succeeded {
                link.fail();
            }
            if let FlingHandoff::Accepted(future) = handoff {
                if self.owns_continuation(&candidate.ticket) && link.alive.get() {
                    if let Connection::Owner { connection, .. } = &connection {
                        connection.flush_nested(&candidate, recovery);
                    }
                    let completion = candidate.clone();
                    let run = Rc::downgrade(self);
                    recovery.run(|| {
                        future.when_complete_or_cancel(move |result| {
                            if let Some(run) = run.upgrade() {
                                run.complete_continuation(&completion, result.is_ok());
                            }
                        });
                    });
                }
                recovery.retire(future);
                recovery.retire(connection);
                return true;
            }
            if self.owns_continuation(&candidate.ticket) {
                self.continuation.replace(outgoing);
            }
            recovery.retire(connection);
            if let ParticipantKind::Owner {
                overscroll: OverscrollBehavior::Contain,
                ..
            } = link.connection.kind()
            {
                return false;
            }
        }
        false
    }

    pub(super) fn advance_continuation(
        self: &Rc<Self>,
        continuation: &ScrollContinuation,
        value: f64,
        previous: &Cell<f64>,
        delta: Vec2,
        velocity: Vec2,
        recovery: &mut PanicRecovery,
    ) {
        if !self.owns_continuation(&continuation.ticket) {
            return;
        }
        let Some(_guard) = self.gate.enter() else {
            return;
        };
        self.advance_continuation_held(continuation, value, previous, delta, velocity, recovery);
        self.gate.drain(recovery);
    }

    pub(super) fn advance_continuation_held(
        self: &Rc<Self>,
        continuation: &ScrollContinuation,
        value: f64,
        previous: &Cell<f64>,
        delta: Vec2,
        velocity: Vec2,
        recovery: &mut PanicRecovery,
    ) {
        if !self.owns_continuation(&continuation.ticket) {
            return;
        }
        let Some(link) = self.members[continuation.member]
            .link
            .upgrade()
            .filter(|link| link.alive.get())
        else {
            self.end(ScrollEndReason::Cancelled);
            return;
        };
        let Some(geometry) =
            link.pipeline
                .as_ref()
                .zip(link.node.get())
                .and_then(|(pipeline, node)| {
                    ViewportGeometry::sample(pipeline, node, continuation.focal)
                })
        else {
            self.end(ScrollEndReason::Cancelled);
            return;
        };
        let Some((delta, velocity)) = geometry
            .root_delta(delta)
            .zip(geometry.root_velocity(velocity))
        else {
            self.end(ScrollEndReason::Cancelled);
            return;
        };
        previous.set(value);
        if delta == Vec2::ZERO {
            return;
        }
        let outcome = self.dispatch(continuation.focal, delta, ScrollCause::SideEffect, recovery);
        if !self.owns_continuation(&continuation.ticket) || outcome.remaining == Vec2::ZERO {
            return;
        }
        let Some(connection) = link.connection.upgrade() else {
            self.end(ScrollEndReason::Cancelled);
            return;
        };
        let mut permits = false;
        if let Connection::Owner {
            connection,
            overscroll,
            ..
        } = &connection
            && *overscroll != OverscrollBehavior::Contain
        {
            let local = geometry.local_velocity(velocity);
            if let Some(local) = local {
                let succeeded = recovery.scope().run_classified(
                    flui_foundation::panic::FailureClass::Computation,
                    |_| {
                        permits = connection
                            .permits_nested_transfer(local, continuation.device_pixel_ratio);
                    },
                );
                if !succeeded {
                    link.fail();
                }
            }
        }
        if self.owns_continuation(&continuation.ticket) {
            if !permits
                || !self.offer_fling(
                    continuation.member,
                    continuation.focal,
                    velocity,
                    continuation.device_pixel_ratio,
                    recovery,
                )
            {
                self.end(self.terminal_reason.get());
            }
            if let Connection::Owner { connection, .. } = &connection {
                connection.stop_nested(continuation, recovery);
            }
        }
        recovery.retire(connection);
    }

    pub(super) fn complete_continuation(
        self: &Rc<Self>,
        continuation: &ScrollContinuation,
        success: bool,
    ) {
        if !self.owns_continuation(&continuation.ticket) {
            return;
        }
        self.gate.complete(continuation.clone(), success);
        if let Some(_guard) = self.gate.enter() {
            let mut recovery = PanicRecovery::new();
            self.gate.drain(&mut recovery);
            recovery.finish();
        }
    }

    pub(super) fn complete_continuation_held(
        self: &Rc<Self>,
        continuation: &ScrollContinuation,
        success: bool,
        recovery: &mut PanicRecovery,
    ) {
        if !self.owns_continuation(&continuation.ticket) {
            return;
        }
        if success
            && let Some(link) = self.members[continuation.member].link.upgrade()
            && let Some(connection) = link.connection.upgrade()
        {
            if let Connection::Owner { connection, .. } = &connection {
                connection.flush_nested(continuation, recovery);
            }
            recovery.retire(connection);
        }
        if self.owns_continuation(&continuation.ticket) {
            self.end(if success {
                self.terminal_reason.get()
            } else {
                ScrollEndReason::Cancelled
            });
        }
    }
}

fn component_consumption(consumed: f64, available: f64) -> f64 {
    if !consumed.is_finite() {
        return 0.0;
    }
    if available >= 0.0 {
        consumed.clamp(0.0, available)
    } else {
        consumed.clamp(available, 0.0)
    }
}
