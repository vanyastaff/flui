//! Exact simulation samples and owner-local continuation custody.

use std::{
    cell::Cell,
    rc::{Rc, Weak},
};

use flui_animation::{Animation, AnimationController, Simulation};
use flui_foundation::{
    InputSequenceId,
    geometry::{Axis, Point, Vec2},
    panic::PanicRecovery,
};

use super::run::SequenceRun;

/// This private capability cannot be manufactured from a repeated sequence ID.
/// A replacement run withdraws its exact ticket before stopping old animation.
#[derive(Clone)]
pub(crate) struct ScrollContinuation {
    pub(super) run: Weak<SequenceRun>,
    pub(super) ticket: Rc<()>,
    pub(super) member: usize,
    pub(super) focal: Point,
    pub(super) device_pixel_ratio: f64,
}

impl ScrollContinuation {
    fn fail(&self, recovery: &mut PanicRecovery) {
        if let Some(run) = self.run.upgrade() {
            run.fail_continuation(self, recovery);
        }
    }
    fn preempt(&self, recovery: &mut PanicRecovery) {
        if let Some(run) = self.run.upgrade() {
            run.preempt_continuation(self, recovery);
        }
    }
    pub(crate) fn is_current(&self) -> bool {
        self.run
            .upgrade()
            .is_some_and(|run| run.owns_continuation(&self.ticket))
    }

    fn advance(
        &self,
        value: f64,
        previous: &Cell<f64>,
        delta: Vec2,
        velocity: Vec2,
        recovery: &mut PanicRecovery,
    ) {
        if let Some(run) = self.run.upgrade() {
            run.advance_continuation(self, value, previous, delta, velocity, recovery);
        }
    }

    fn advance_held(
        &self,
        value: f64,
        previous: &Cell<f64>,
        delta: Vec2,
        velocity: Vec2,
        recovery: &mut PanicRecovery,
    ) {
        if let Some(run) = self.run.upgrade() {
            run.advance_continuation_held(self, value, previous, delta, velocity, recovery);
        }
    }
}

struct SampledSimulation {
    source: Box<dyn Simulation>,
    time: Cell<f64>,
}

/// Records the exact local time the animation controller sampled, including its
/// final sample after the controller has already withdrawn its running state.
#[derive(Clone)]
pub(crate) struct BallisticSimulation(Rc<SampledSimulation>);

impl Simulation for BallisticSimulation {
    fn x(&self, time: f64) -> f64 {
        let value = self.0.source.x(time);
        self.0.time.set(time);
        value
    }
    fn dx(&self, time: f64) -> f64 {
        self.0.source.dx(time)
    }
    fn is_done(&self, time: f64) -> bool {
        self.0.source.is_done(time)
    }
}

pub(crate) struct BallisticMotion {
    continuation: ScrollContinuation,
    controller: AnimationController,
    simulation: BallisticSimulation,
    previous: Cell<f64>,
    generation: Cell<Option<u64>>,
    axis: Axis,
    physical_sign: f64,
}

impl std::fmt::Debug for BallisticMotion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BallisticMotion")
            .field("axis", &self.axis)
            .field("previous", &self.previous.get())
            .field("generation", &self.generation.get())
            .finish_non_exhaustive()
    }
}

impl BallisticMotion {
    pub(crate) fn preempt(&self, recovery: &mut PanicRecovery) {
        self.continuation.preempt(recovery);
    }
    pub(crate) fn new(
        continuation: ScrollContinuation,
        controller: AnimationController,
        simulation: Box<dyn Simulation>,
        initial: f64,
        axis: Axis,
        reversed: bool,
    ) -> Rc<Self> {
        Rc::new(Self {
            continuation,
            controller,
            simulation: BallisticSimulation(Rc::new(SampledSimulation {
                source: simulation,
                time: Cell::new(0.0),
            })),
            previous: Cell::new(initial),
            generation: Cell::new(None),
            axis,
            physical_sign: if reversed { 1.0 } else { -1.0 },
        })
    }

    pub(crate) fn simulation(&self) -> BallisticSimulation {
        self.simulation.clone()
    }

    pub(crate) fn admitted(&self) {
        self.generation.set(Some(self.controller.run_generation()));
    }

    pub(crate) fn belongs_to(&self, sequence: InputSequenceId) -> bool {
        self.continuation
            .run
            .upgrade()
            .is_some_and(|run| run.sequence == sequence)
    }

    pub(crate) fn stop(&self, recovery: &mut PanicRecovery) {
        if self.generation.get() == Some(self.controller.run_generation()) {
            recovery.run(|| {
                let _ = self.controller.stop();
            });
        }
    }

    fn sample(&self) -> Option<(f64, Vec2, Vec2)> {
        if self.generation.get() != Some(self.controller.run_generation())
            || !self.continuation.is_current()
        {
            return None;
        }
        let value = self.controller.value();
        let delta = (value - self.previous.get()) * self.physical_sign;
        let velocity = self.simulation.dx(self.simulation.0.time.get()) * self.physical_sign;
        if !value.is_finite()
            || !delta.is_finite()
            || !velocity.is_finite()
            || self.generation.get() != Some(self.controller.run_generation())
            || !self.continuation.is_current()
        {
            return None;
        }
        let vector = |component| match self.axis {
            Axis::Horizontal => Vec2::new(component, 0.0),
            Axis::Vertical => Vec2::new(0.0, component),
        };
        Some((value, vector(delta), vector(velocity)))
    }

    pub(crate) fn tick(&self) {
        let mut recovery = PanicRecovery::new();
        if let Some((value, delta, velocity)) = self.sample_with_recovery(&mut recovery) {
            self.continuation
                .advance(value, &self.previous, delta, velocity, &mut recovery);
        }
        recovery.finish();
    }

    /// Completion may precede value listeners. The chain's drain flushes that
    /// committed final value before deciding whether this ticket may end.
    pub(crate) fn flush_held(&self, recovery: &mut PanicRecovery) {
        if let Some((value, delta, velocity)) = self.sample_with_recovery(recovery) {
            self.continuation
                .advance_held(value, &self.previous, delta, velocity, recovery);
        }
    }

    fn sample_with_recovery(&self, recovery: &mut PanicRecovery) -> Option<(f64, Vec2, Vec2)> {
        let mut sampled = None;
        let succeeded = recovery
            .scope()
            .run_classified(flui_foundation::panic::FailureClass::Computation, |_| {
                sampled = self.sample()
            });
        if !succeeded
            || (sampled.is_none()
                && self.generation.get() == Some(self.controller.run_generation())
                && self.continuation.is_current())
        {
            self.continuation.fail(recovery);
        }
        sampled
    }

    pub(crate) fn owns(&self, continuation: &ScrollContinuation) -> bool {
        Rc::ptr_eq(&self.continuation.ticket, &continuation.ticket)
    }
}
