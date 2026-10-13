//! A physical spring sampled as a normalized, duration-based easing.

use super::{Builtin, BuiltinCurve, Curve};
use crate::simulation::{
    Simulation, SimulationError, SimulationParameter, SpringDescription, SpringSimulation,
    Tolerance,
};
use std::time::Duration;

/// A zero-velocity spring from 0 to 1, sampled by normalized progress.
///
/// The physical time is `progress * duration()`. The duration is the spring's
/// conservative rest boundary, rounded upward to a representable nanosecond
/// (at least one nanosecond). Interior samples retain physical overshoot.
/// The endpoints and exterior inputs follow [`Curve`]'s exact clamp policy.
///
/// This immutable easing has no clock, run, listener or accumulated frame state.
/// It fits keyframe segments and authored Hero mappings. For motion that inherits
/// velocity on interruption, use [`crate::MotionSpec::Spring`] instead.
/// Independently constructed equivalent curves compare by value in [`super::ArcCurve`].
///
/// ```
/// use flui_animation::{Curve, SpringCurve, SpringDescription, Tolerance};
/// let spring = SpringDescription::new(1.0, 100.0, 10.0)?;
/// let curve = SpringCurve::new(spring, Tolerance::DEFAULT)?;
/// assert!(!curve.duration().is_zero());
/// assert_eq!(curve.transform(1.0), 1.0);
/// # Ok::<(), flui_animation::SimulationError>(())
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct SpringCurve {
    simulation: SpringSimulation,
    duration: Duration,
}

impl SpringCurve {
    /// Admit a spring and a tolerance measured in normalized output units.
    ///
    /// # Errors
    ///
    /// Returns [`SimulationError::OutOfRange`] naming
    /// [`SimulationParameter::UnitDistance`] for a distance tolerance at least
    /// 1, which would consider the initial displacement already at rest.
    /// Returns [`SimulationError::Overflow`] for unrepresentable physical
    /// coefficients or a rest duration outside [`Duration`]'s range.
    pub fn new(spring: SpringDescription, tolerance: Tolerance) -> Result<Self, SimulationError> {
        if tolerance.distance >= 1.0 {
            return Err(SimulationError::OutOfRange {
                parameter: SimulationParameter::UnitDistance,
                value: tolerance.distance,
            });
        }
        let simulation = SpringSimulation::new(spring, 0.0, 1.0, 0.0, tolerance)?;
        let rest = simulation.rest_transition().0;
        let mut duration =
            Duration::try_from_secs_f64(rest).map_err(|_| SimulationError::Overflow)?;
        if duration.is_zero() || !simulation.is_done(duration.as_secs_f64()) {
            duration = duration
                .checked_add(Duration::from_nanos(1))
                .ok_or(SimulationError::Overflow)?;
        }
        Ok(Self {
            simulation,
            duration,
        })
    }

    /// The natural settling duration; use it to preserve the authored spring's
    /// time scale when placing an eased keyframe segment.
    #[must_use]
    pub fn duration(&self) -> Duration {
        self.duration
    }
}

impl Curve for SpringCurve {
    fn transform(&self, t: f64) -> f64 {
        super::settled(t).unwrap_or_else(|| self.simulation.x(t * self.duration.as_secs_f64()))
    }
    /// Analytic physical velocity multiplied by the duration, in output
    /// units per normalized input unit. At completed progress the slope is
    /// exactly zero; an unrepresentable rate saturates to signed `f64::MAX`.
    fn slope(&self, t: f64) -> f64 {
        if let Some(slope) = super::settled_slope(t) {
            return slope;
        }
        if t == 1.0 {
            return 0.0;
        }
        let duration = self.duration.as_secs_f64();
        (self.simulation.dx(t * duration) * duration).clamp(-f64::MAX, f64::MAX)
    }
    fn builtin(&self) -> Option<BuiltinCurve> {
        Some(BuiltinCurve::new(Builtin::Spring(self.clone())))
    }
}
