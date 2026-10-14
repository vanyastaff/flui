//! Successful completion at the admitted run's declared endpoint.

use std::cell::RefMut;

use flui_foundation::panic::RecoveryScope;

use super::run::{ActiveRun, RunMode};
use super::{
    AnimationController, AnimationControllerInner, AnimationDirection, AnimationError,
    RetiredSources, Retirement, RunCompleter, SimulationRun, ValueChange,
};

impl AnimationController {
    /// Finish the current finite run at its declared endpoint.
    ///
    /// Resolves the original run future successfully, after committing the
    /// terminal value and directional settled phase. Value listeners, status
    /// listeners and completion continuations use the normal delivery order.
    /// No replacement run is started and no curve or custom simulation is sampled.
    ///
    /// Finite repeats land at the final leg's endpoint. Native property motion
    /// uses its target and native flings use their bound. A paused finite run can
    /// finish; this explicit command does not advance the presentation clock or
    /// change the configured playback rate. Finishing an idle controller is silent.
    ///
    /// Reentrant listeners may start another run; that replacement remains
    /// authoritative when this call returns. Callback failures follow the existing
    /// containment contract and do not turn accepted completion into cancellation.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] after owner disposal, or
    /// [`AnimationError::NoFiniteEndpoint`] for an infinite repeat or an arbitrary
    /// custom simulation. Refusal leaves the original run and future unchanged.
    pub fn finish(&self) -> Result<(), AnimationError> {
        self.reported("AnimationController::finish", self.admit_finish())
    }

    fn admit_finish(&self) -> Result<(), AnimationError> {
        let mut inner = self.inner.borrow_mut();
        Self::check_disposed(&inner)?;
        if inner.run.is_none() {
            return Ok(());
        }
        let endpoint = inner.completion_endpoint()?;
        inner.note_settle_reason("stopped");
        let mut retirement = Retirement::new();
        self.complete_finite_run(inner, endpoint, &mut retirement.scope());
        retirement.finish();
        Ok(())
    }

    /// No author callouts occur until the endpoint and original outcome commit.
    /// Policy settlement supplies its enclosing recovery custody to this same path.
    pub(super) fn complete_finite_run(
        &self,
        mut inner: RefMut<'_, AnimationControllerInner>,
        (value, direction): (f64, AnimationDirection),
        recovery: &mut RecoveryScope<'_>,
    ) {
        if let Some(ActiveRun {
            mode: RunMode::Simulation(SimulationRun::Value(source)),
            ..
        }) = &inner.run
        {
            // Generated value motion is sealed; settlement only publishes Copy
            // arrays and its retained exact-target representation.
            source.settle();
        }
        let mut retired = RetiredSources::new();
        let completer = inner.take_run(&mut retired);
        inner.value = value.clamp(inner.lower_bound, inner.upper_bound);
        inner.start_value = inner.value;
        inner.target_value = inner.value;
        inner.direction = direction;
        let status = direction.settled_status();
        inner.status = status;
        let delivery = completer.map(RunCompleter::complete);
        self.deliver_transition_with_recovery(
            status,
            ValueChange::Notify,
            delivery,
            retired,
            inner,
            recovery,
        );
    }
}

impl AnimationControllerInner {
    pub(super) fn completion_endpoint(&self) -> Result<(f64, AnimationDirection), AnimationError> {
        match self.run.as_ref().map(|run| &run.mode) {
            Some(RunMode::Repeat(run)) => run.landing().ok_or(AnimationError::NoFiniteEndpoint),
            Some(RunMode::Simulation(SimulationRun::Custom(_))) => {
                Err(AnimationError::NoFiniteEndpoint)
            }
            Some(RunMode::Simulation(SimulationRun::Fling {
                bound, direction, ..
            })) => Ok((*bound, *direction)),
            Some(RunMode::Simulation(SimulationRun::Value(_))) => Ok((0.0, self.direction)),
            Some(RunMode::Simulation(SimulationRun::Motion { .. }) | RunMode::Timed { .. })
            | None => Ok((self.target_value, self.direction)),
        }
    }
}
