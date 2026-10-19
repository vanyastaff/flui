//! The viewer's snap-back: how far a gesture shows the scene past its bounds,
//! with resistance, and the springs that bring it back.
//!
//! A gesture accumulates its *travel* past the bounds unresisted: viewport
//! pixels for a pan, the natural logarithm of the factor for a scale. What
//! the viewer shows is the *overlay*: the travel through a resistance curve
//! that never reaches its limit, so a contact can pull the scene at most one
//! viewport past its edge and zoom it at most [`SCALE_LIMIT`] past a bound.

use flui_animation::{SimulationError, SpringDescription, SpringSimulation, Tolerance};
use flui_foundation::geometry::{Matrix4, Offset};

/// How hard the rubber band pulls back: the iOS scroll view's coefficient.
const RESISTANCE: f64 = 0.55;
/// The furthest a pinch shows the scene past a scale bound, as the natural
/// logarithm of the factor: half an octave.
const SCALE_LIMIT: f64 = std::f64::consts::LN_2 / 2.0;

/// What a gesture accumulated past the bounds, unresisted.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(super) struct Travel {
    /// Viewport pixels the scene was pulled past its boundary, per axis.
    pub(super) pan: Offset<f64>,
    /// The logarithm of the factor a pinch went past a scale bound.
    pub(super) log_scale: f64,
}

/// What the viewer shows over its in-bounds transform.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(super) struct Overlay {
    /// The scene's displacement past its boundary, viewport pixels.
    pub(super) pan: Offset<f64>,
    /// The logarithm of the scale shown past a bound.
    pub(super) log_scale: f64,
    /// The viewport point the scale overlay is about.
    pub(super) focal: Offset<f64>,
}

impl Overlay {
    /// The overlay `travel` shows in a `width` × `height` viewport.
    pub(super) fn of(travel: Travel, focal: Offset<f64>, width: f64, height: f64) -> Self {
        Self {
            pan: Offset::new(resist(travel.pan.dx, width), resist(travel.pan.dy, height)),
            log_scale: resist(travel.log_scale, SCALE_LIMIT),
            focal,
        }
    }

    /// The travel that shows this overlay: a gesture that catches a
    /// snap-back continues from where the scene is.
    pub(super) fn travel(self, width: f64, height: f64) -> Travel {
        Travel {
            pan: Offset::new(unresist(self.pan.dx, width), unresist(self.pan.dy, height)),
            log_scale: unresist(self.log_scale, SCALE_LIMIT),
        }
    }

    pub(super) fn is_rest(self) -> bool {
        self.pan == Offset::ZERO && self.log_scale == 0.0
    }

    /// This overlay `fraction` of the way from rest.
    pub(super) fn scaled(self, fraction: f64) -> Self {
        Self {
            pan: self.pan * fraction,
            log_scale: self.log_scale * fraction,
            focal: self.focal,
        }
    }

    /// The viewport-space transform applied over the in-bounds one, or
    /// `None` when it is not finite.
    pub(super) fn matrix(self) -> Option<Matrix4> {
        let factor = self.log_scale.exp();
        let shift = self.pan + self.focal;
        if !factor.is_finite() || factor <= 0.0 || !shift.is_finite() || !self.focal.is_finite() {
            return None;
        }
        let matrix = Matrix4::translation(shift.dx, shift.dy, 0.0)
            * Matrix4::scaling(factor, factor, 1.0)
            * Matrix4::translation(-self.focal.dx, -self.focal.dy, 0.0);
        matrix
            .to_col_major_array()
            .iter()
            .all(|value| value.is_finite())
            .then_some(matrix)
    }
}

/// `travel` through the rubber band of an `extent`: the same sign, never as
/// far as `extent`. Zero for a degenerate extent.
pub(super) fn resist(travel: f64, extent: f64) -> f64 {
    if travel == 0.0 || !travel.is_finite() || !extent.is_finite() || extent <= 0.0 {
        return 0.0;
    }
    let shown = extent * (1.0 - 1.0 / (travel.abs() * RESISTANCE / extent + 1.0));
    shown.copysign(travel)
}

/// The travel [`resist`] shows as `shown`; a value at or past the band's
/// limit is the furthest finite travel.
pub(super) fn unresist(shown: f64, extent: f64) -> f64 {
    if shown == 0.0 || !shown.is_finite() || !extent.is_finite() || extent <= 0.0 {
        return 0.0;
    }
    let ratio = (shown.abs() / extent).min(1.0 - 1e-9);
    let travel = extent * (1.0 / (1.0 - ratio) - 1.0) / RESISTANCE;
    travel.copysign(shown)
}

/// The spring every snap-back runs: critically damped, so the scene does not
/// cross its bound on the way back.
fn spring() -> Result<SpringDescription, SimulationError> {
    SpringDescription::with_damping_ratio(1.0, 400.0, 1.0)
}

/// The run that brings a released overlay back: its fraction, from one to
/// rest.
pub(super) fn release_run() -> Result<SpringSimulation, SimulationError> {
    SpringSimulation::new(spring()?, 1.0, 0.0, 0.0, Tolerance::new(1e-3, 1e-2)?)
}

/// The run of a fling stopped at its boundary at `speed` (viewport pixels
/// per second): the overshoot past the bound in pixels, out and back.
pub(super) fn overshoot_run(speed: f64) -> Result<SpringSimulation, SimulationError> {
    SpringSimulation::new(spring()?, 0.0, 0.0, speed, Tolerance::new(0.25, 1.0)?)
}
