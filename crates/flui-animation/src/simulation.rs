//! Physics simulations for animation.
//!
//! A [`Simulation`] is a one-dimensional motion: a position [`x`](Simulation::x)
//! and velocity [`dx`](Simulation::dx) at a time `t` in seconds since the
//! simulation began, and a monotonic [`is_done`](Simulation::is_done).
//!
//! Every simulation here is an immutable value with no locks and no callbacks;
//! share one as `Arc<dyn Simulation>`. Constructors validate their input and
//! return [`SimulationError`]: a simulation that was built never publishes a
//! non-finite position or velocity.
//!
//! Rest is a moment, not a sample. Each simulation computes at construction
//! the time after which it is guaranteed to stay within its [`Tolerance`]; from
//! that time on `x` is exactly the resting position and `dx` is `0.0`, so
//! `is_done` never flickers and does not depend on how time is sampled.
//!
//! # Examples
//!
//! ```
//! use flui_animation::simulation::{Simulation, SpringDescription, SpringSimulation, Tolerance};
//! use std::time::Duration;
//!
//! # fn main() -> Result<(), flui_animation::simulation::SimulationError> {
//! let spring = SpringDescription::with_duration_and_bounce(Duration::from_millis(500), 0.2)?;
//! let sim = SpringSimulation::try_new(spring, 0.0, 1.0, 0.0, Tolerance::DEFAULT)?;
//!
//! assert_eq!(sim.x(0.0), 0.0);
//! assert!(sim.x(0.1) > 0.0);
//! assert!(sim.is_done(10.0));
//! assert_eq!(sim.x(10.0), 1.0); // exactly the target once at rest
//! # Ok(())
//! # }
//! ```

use std::f64::consts::TAU;
use std::fmt;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// The input a [`SimulationError::OutOfRange`] refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SimulationParameter {
    /// A spring's mass.
    Mass,
    /// A spring's stiffness.
    Stiffness,
    /// A spring's damping coefficient.
    Damping,
    /// A spring's damping ratio `ζ`.
    DampingRatio,
    /// A perceptual spring's duration or response, in seconds.
    Duration,
    /// A perceptual spring's bounce.
    Bounce,
    /// A perceptual spring's damping fraction.
    DampingFraction,
    /// A friction drag coefficient.
    Drag,
    /// A start or target position.
    Position,
    /// An initial velocity, or a tolerance's velocity limit.
    Velocity,
    /// A tolerance's distance.
    Distance,
    /// A device pixel ratio.
    DevicePixelRatio,
}

impl fmt::Display for SimulationParameter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Mass => "mass",
            Self::Stiffness => "stiffness",
            Self::Damping => "damping",
            Self::DampingRatio => "damping ratio",
            Self::Duration => "duration",
            Self::Bounce => "bounce",
            Self::DampingFraction => "damping fraction",
            Self::Drag => "drag",
            Self::Position => "position",
            Self::Velocity => "velocity",
            Self::Distance => "distance",
            Self::DevicePixelRatio => "device pixel ratio",
        })
    }
}

/// Why a simulation, spring, tolerance or bounds value could not be built.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum SimulationError {
    /// One input is NaN, infinite, or outside its documented range.
    #[error("{parameter} = {value} is outside its admitted range")]
    OutOfRange {
        /// The refused input.
        parameter: SimulationParameter,
        /// Its value (a duration in seconds).
        value: f64,
    },
    /// Every input is in range, but a constant derived from them is not a
    /// finite non-zero `f64` (for example `ω²` of an extremely stiff, light
    /// spring, or an initial displacement `start − end` that overflows).
    #[error("simulation constants are not representable as finite f64 values")]
    Overflow,
    /// Bounds that are not finite or whose `min > max`.
    #[error("bounds must be finite and ordered, got [{min}, {max}]")]
    InvalidBounds {
        /// The refused lower bound.
        min: f64,
        /// The refused upper bound.
        max: f64,
    },
}

fn out_of_range(parameter: SimulationParameter, value: f64) -> SimulationError {
    SimulationError::OutOfRange { parameter, value }
}

/// `Ok(value)` when `value` is finite and `> 0`.
fn positive(parameter: SimulationParameter, value: f64) -> Result<f64, SimulationError> {
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err(out_of_range(parameter, value))
    }
}

fn finite(parameter: SimulationParameter, value: f64) -> Result<f64, SimulationError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(out_of_range(parameter, value))
    }
}

/// `Ok(value)` when a derived constant is finite.
fn representable(value: f64) -> Result<f64, SimulationError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(SimulationError::Overflow)
    }
}

/// Before the simulation began: `t < 0` or `t` is NaN.
fn before_start(time: f64) -> bool {
    time.is_nan() || time < 0.0
}

// ---------------------------------------------------------------------------
// Tolerance
// ---------------------------------------------------------------------------

/// How close to rest a simulation must be before it reports done.
///
/// A simulation rests from the first moment after which it stays within
/// `distance` of its resting position and, unless the velocity limit is
/// infinite, below `velocity` in speed. With an infinite velocity limit the
/// simulation derives one from its own time scale: `distance · ω` for a
/// spring, and for friction the speed at which the remaining glide is
/// `distance`.
///
/// # Examples
///
/// ```
/// use flui_animation::simulation::Tolerance;
///
/// let precise = Tolerance::new(1e-4, 1e-4)?;
/// let screen = Tolerance::for_device_pixel_ratio(2.0)?; // a quarter logical pixel
/// assert_ne!(precise, screen);
/// assert!(Tolerance::new(f64::NAN, 1.0).is_err());
/// # Ok::<(), flui_animation::simulation::SimulationError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerance {
    pub(crate) distance: f64,
    pub(crate) velocity: f64,
    /// Not read by any simulation; the controller still builds a tolerance
    /// by struct literal and must name it.
    pub(crate) time: f64,
}

impl Tolerance {
    /// `distance` and `velocity` of `0.001` in the simulation's own units.
    pub const DEFAULT: Self = Self {
        distance: 1e-3,
        velocity: 1e-3,
        time: 1e-3,
    };

    /// A tolerance of `distance` (finite, `> 0`) in position units and
    /// `velocity` (`> 0`, `f64::INFINITY` for a velocity limit derived from
    /// the simulation's own time scale) in position units per second.
    ///
    /// # Errors
    ///
    /// [`SimulationError::OutOfRange`] naming [`SimulationParameter::Distance`]
    /// or [`SimulationParameter::Velocity`] for a NaN, non-positive or (for
    /// `distance`) infinite value.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::simulation::Tolerance;
    ///
    /// assert!(Tolerance::new(0.5, f64::INFINITY).is_ok());
    /// assert!(Tolerance::new(0.0, 1.0).is_err());
    /// ```
    pub fn new(distance: f64, velocity: f64) -> Result<Self, SimulationError> {
        let distance = positive(SimulationParameter::Distance, distance)?;
        if velocity.is_nan() || velocity <= 0.0 {
            return Err(out_of_range(SimulationParameter::Velocity, velocity));
        }
        Ok(Self {
            distance,
            velocity,
            time: 1e-3,
        })
    }

    /// Half a device pixel at `device_pixel_ratio` device pixels per logical
    /// pixel, with a derived velocity limit: the tolerance for a motion in
    /// logical pixels whose rest must not be visible.
    ///
    /// # Errors
    ///
    /// [`SimulationError::OutOfRange`] naming
    /// [`SimulationParameter::DevicePixelRatio`] when the ratio is NaN,
    /// infinite, non-positive, or so small that half a device pixel overflows.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::simulation::Tolerance;
    ///
    /// assert_eq!(
    ///     Tolerance::for_device_pixel_ratio(2.0)?,
    ///     Tolerance::new(0.25, f64::INFINITY)?,
    /// );
    /// # Ok::<(), flui_animation::simulation::SimulationError>(())
    /// ```
    pub fn for_device_pixel_ratio(device_pixel_ratio: f64) -> Result<Self, SimulationError> {
        let refused = || out_of_range(SimulationParameter::DevicePixelRatio, device_pixel_ratio);
        let ratio = positive(SimulationParameter::DevicePixelRatio, device_pixel_ratio)?;
        Self::new(0.5 / ratio, f64::INFINITY).map_err(|_| refused())
    }

    /// The speed limit for a motion whose natural rate is `rate` per second.
    fn velocity_limit(self, rate: f64) -> f64 {
        if self.velocity.is_infinite() {
            self.distance * rate
        } else {
            self.velocity
        }
    }
}

impl Default for Tolerance {
    fn default() -> Self {
        Self::DEFAULT
    }
}

// ---------------------------------------------------------------------------
// Simulation
// ---------------------------------------------------------------------------

/// A one-dimensional motion sampled at `time` seconds since it began.
///
/// Implementations used by the framework guarantee: `x` and `dx` are finite
/// for every `time`; a `time` before the start (negative or NaN) yields the
/// initial state; `is_done` is monotonic in `time`.
pub trait Simulation: Send + Sync {
    /// The position at `time` seconds.
    fn x(&self, time: f64) -> f64;

    /// The velocity, in position units per second, at `time` seconds.
    fn dx(&self, time: f64) -> f64;

    /// Whether the motion has come to rest at `time` seconds.
    fn is_done(&self, time: f64) -> bool;

    /// The tolerance this simulation rests within. Drivers do not read it;
    /// the default returns [`Tolerance::DEFAULT`].
    fn tolerance(&self) -> Tolerance {
        Tolerance::DEFAULT
    }
}

/// Lets a `Box<dyn Simulation>` (as `ScrollPhysics` returns) be passed where
/// `S: Simulation` is expected.
impl<S: Simulation + ?Sized> Simulation for Box<S> {
    #[inline]
    fn x(&self, time: f64) -> f64 {
        (**self).x(time)
    }

    #[inline]
    fn dx(&self, time: f64) -> f64 {
        (**self).dx(time)
    }

    #[inline]
    fn is_done(&self, time: f64) -> bool {
        (**self).is_done(time)
    }

    #[inline]
    fn tolerance(&self) -> Tolerance {
        (**self).tolerance()
    }
}

// ---------------------------------------------------------------------------
// SpringDescription
// ---------------------------------------------------------------------------

/// A damped spring, stored as its natural angular frequency `ω` (radians per
/// second) and damping ratio `ζ`.
///
/// The fields are private, so a spring outside the admitted domain — `ω` and
/// `ζ` finite and positive, `ω²` and `ζω` finite — cannot be written down.
/// Mass does not appear: motion depends on `ω = √(k/m)` and
/// `ζ = c / (2√(km))` only. An undamped spring (`ζ = 0`) is refused because
/// it never comes to rest.
///
/// ```compile_fail
/// use flui_animation::simulation::SpringDescription;
///
/// let spring = SpringDescription { omega: 10.0, zeta: -1.0 }; // private fields
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringDescription {
    omega: f64,
    zeta: f64,
}

impl SpringDescription {
    /// A spring of `mass`, `stiffness` and `damping` coefficient, each finite
    /// and `> 0`.
    ///
    /// # Errors
    ///
    /// [`SimulationError::OutOfRange`] naming the first input that is NaN,
    /// infinite or not positive; [`SimulationError::Overflow`] when `ω` or `ζ`
    /// is not representable.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::simulation::{SpringDescription, SpringType};
    ///
    /// let spring = SpringDescription::new(1.0, 100.0, 20.0)?;
    /// assert_eq!(spring.spring_type(), SpringType::CriticallyDamped);
    /// assert!(SpringDescription::new(1.0, 100.0, 0.0).is_err());
    /// # Ok::<(), flui_animation::simulation::SimulationError>(())
    /// ```
    pub fn new(mass: f64, stiffness: f64, damping: f64) -> Result<Self, SimulationError> {
        let mass = positive(SimulationParameter::Mass, mass)?.sqrt();
        let stiffness = positive(SimulationParameter::Stiffness, stiffness)?.sqrt();
        let damping = positive(SimulationParameter::Damping, damping)?;
        Self::from_omega_zeta(stiffness / mass, damping / 2.0 / stiffness / mass)
    }

    /// A spring of `mass` and `stiffness` damped at `ratio` of critical
    /// damping: `1.0` settles fastest without overshoot, `< 1.0` bounces,
    /// `> 1.0` creeps.
    ///
    /// Meant for constant springs; [`new`](Self::new) is the fallible form.
    ///
    /// # Panics
    ///
    /// When an input is NaN, infinite or not positive, or `ω` is not
    /// representable.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::simulation::{SpringDescription, SpringType};
    ///
    /// let spring = SpringDescription::with_damping_ratio(1.0, 500.0, 1.0);
    /// assert_eq!(spring.spring_type(), SpringType::CriticallyDamped);
    /// ```
    #[must_use]
    pub fn with_damping_ratio(mass: f64, stiffness: f64, ratio: f64) -> Self {
        let spring = positive(SimulationParameter::Mass, mass).and_then(|mass| {
            let stiffness = positive(SimulationParameter::Stiffness, stiffness)?;
            let ratio = positive(SimulationParameter::DampingRatio, ratio)?;
            Self::from_omega_zeta(stiffness.sqrt() / mass.sqrt(), ratio)
        });
        match spring {
            Ok(spring) => spring,
            Err(error) => panic!("invalid spring: {error}"),
        }
    }

    /// A spring that completes a perceptual `duration` with `bounce`, the
    /// parameterization of SwiftUI's `Spring(duration:bounce:)`:
    /// `ω = 2π / duration`; `ζ = 1 − bounce` for `bounce ≥ 0`.
    ///
    /// `bounce` is in the open range `(−1, 1)`: `0` is critically damped,
    /// positive values overshoot. For a negative bounce, `ζ = 1 / (1 + bounce)`
    /// (an overdamped spring) — a mapping that is not confirmed against
    /// Apple's documentation.
    ///
    /// # Errors
    ///
    /// [`SimulationError::OutOfRange`] for a zero `duration` or a `bounce`
    /// outside `(−1, 1)`; [`SimulationError::Overflow`] for a `duration` so
    /// short that `ω²` overflows.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::simulation::{SpringDescription, SpringType};
    /// use std::time::Duration;
    ///
    /// let spring = SpringDescription::with_duration_and_bounce(Duration::from_millis(500), 0.3)?;
    /// assert_eq!(spring.spring_type(), SpringType::Underdamped);
    /// # Ok::<(), flui_animation::simulation::SimulationError>(())
    /// ```
    pub fn with_duration_and_bounce(
        duration: Duration,
        bounce: f64,
    ) -> Result<Self, SimulationError> {
        let omega = angular_frequency(duration)?;
        if bounce.is_nan() || bounce <= -1.0 || bounce >= 1.0 {
            return Err(out_of_range(SimulationParameter::Bounce, bounce));
        }
        let zeta = if bounce >= 0.0 {
            1.0 - bounce
        } else {
            1.0 / (1.0 + bounce)
        };
        Self::from_omega_zeta(omega, zeta)
    }

    /// A spring with a natural period of `response` and a damping ratio of
    /// `damping_fraction` (`> 0`), the parameterization of SwiftUI's
    /// `spring(response:dampingFraction:)`: `ω = 2π / response`,
    /// `ζ = damping_fraction`. The `response` mapping is not confirmed
    /// against Apple's documentation.
    ///
    /// # Errors
    ///
    /// [`SimulationError::OutOfRange`] for a zero `response` or a NaN,
    /// infinite or non-positive `damping_fraction`;
    /// [`SimulationError::Overflow`] when `ω²` or `ζω` overflows.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::simulation::SpringDescription;
    /// use std::time::Duration;
    ///
    /// let spring = SpringDescription::with_response_and_damping(Duration::from_millis(300), 0.8)?;
    /// assert!(SpringDescription::with_response_and_damping(Duration::ZERO, 0.8).is_err());
    /// # let _ = spring;
    /// # Ok::<(), flui_animation::simulation::SimulationError>(())
    /// ```
    pub fn with_response_and_damping(
        response: Duration,
        damping_fraction: f64,
    ) -> Result<Self, SimulationError> {
        let omega = angular_frequency(response)?;
        let zeta = positive(SimulationParameter::DampingFraction, damping_fraction)?;
        Self::from_omega_zeta(omega, zeta)
    }

    /// The spring's motion regime, from its damping ratio.
    #[must_use]
    pub fn spring_type(&self) -> SpringType {
        match self.zeta.total_cmp(&1.0) {
            std::cmp::Ordering::Less => SpringType::Underdamped,
            std::cmp::Ordering::Equal => SpringType::CriticallyDamped,
            std::cmp::Ordering::Greater => SpringType::Overdamped,
        }
    }

    fn from_omega_zeta(omega: f64, zeta: f64) -> Result<Self, SimulationError> {
        let admitted = |value: f64| value.is_finite() && value > 0.0;
        if admitted(omega)
            && admitted(zeta)
            && admitted(omega * omega)
            && admitted(omega * zeta)
            && (omega * omega * ((1.0 - zeta) * (1.0 + zeta))).is_finite()
        {
            Ok(Self { omega, zeta })
        } else {
            Err(SimulationError::Overflow)
        }
    }
}

/// `2π / period`, refusing a zero period.
fn angular_frequency(period: Duration) -> Result<f64, SimulationError> {
    let seconds = period.as_secs_f64();
    if seconds > 0.0 {
        Ok(TAU / seconds)
    } else {
        Err(out_of_range(SimulationParameter::Duration, seconds))
    }
}

/// The motion regime of a spring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SpringType {
    /// `ζ = 1`: no overshoot, the fastest approach without one.
    CriticallyDamped,
    /// `ζ < 1`: oscillates while it settles.
    Underdamped,
    /// `ζ > 1`: no overshoot, slower than critical damping.
    Overdamped,
}

// ---------------------------------------------------------------------------
// SpringSimulation
// ---------------------------------------------------------------------------

/// Below this `|q t²|`, `cos`/`sin` and `cosh`/`sinh` are summed as series so
/// the motion is continuous through `ζ = 1` and exact at `t → 0`.
const SERIES_LIMIT: f64 = 1e-2;

/// A spring moving from `start` toward `end`.
///
/// The motion is the closed-form solution of `x'' + 2ζω x' + ω² x = 0` in one
/// form for every damping ratio, finite for every `t`. The spring rests at a
/// time computed at construction from a conservative envelope of the motion:
/// from then on it stays within its [`Tolerance`], [`x`](Simulation::x) is
/// exactly `end` and [`dx`](Simulation::dx) is `0.0`.
#[derive(Debug, Clone, PartialEq)]
pub struct SpringSimulation {
    start: f64,
    end: f64,
    /// Initial displacement `start − end` and velocity.
    x0: f64,
    v0: f64,
    /// `a = ζω` and `q = ω²(1 − ζ²)`.
    a: f64,
    q: f64,
    /// `a·x0 + v0` and `ω²·x0 + a·v0`: the coefficients of `e^{−at}·S`.
    position_s: f64,
    velocity_s: f64,
    omega: f64,
    zeta: f64,
    roots: Roots,
    rest_secs: f64,
    tolerance: Tolerance,
}

/// The decay rates the propagator and the rest envelopes need.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Roots {
    /// `ζ < 1`: damped angular frequency `ω_d = √q`.
    Oscillating { damped_omega: f64 },
    /// `ζ = 1`.
    Critical,
    /// `ζ > 1`: `s = √(−q)` and the slow and fast roots `λs = −a + s`,
    /// `λf = −a − s`, each computed without cancellation.
    Decaying { s: f64, slow: f64, fast: f64 },
}

impl SpringSimulation {
    /// A spring from `start` to `end` with initial `velocity`, resting within
    /// [`Tolerance::DEFAULT`].
    ///
    /// Meant for inputs already known to be finite; [`try_new`](Self::try_new)
    /// is the fallible form.
    ///
    /// # Panics
    ///
    /// When `start`, `end` or `velocity` is not finite, or the motion's
    /// constants overflow.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::simulation::{Simulation, SpringDescription, SpringSimulation};
    ///
    /// let spring = SpringDescription::with_damping_ratio(1.0, 300.0, 0.5);
    /// let sim = SpringSimulation::new(spring, 0.0, 1.0, 0.0);
    /// assert_eq!(sim.x(0.0), 0.0);
    /// ```
    #[must_use]
    pub fn new(spring: SpringDescription, start: f64, end: f64, velocity: f64) -> Self {
        match Self::try_new(spring, start, end, velocity, Tolerance::DEFAULT) {
            Ok(simulation) => simulation,
            Err(error) => panic!("invalid spring simulation: {error}"),
        }
    }

    /// A spring from `start` to `end` with initial `velocity` (position units
    /// per second), resting within `tolerance`.
    ///
    /// # Errors
    ///
    /// [`SimulationError::OutOfRange`] naming [`SimulationParameter::Position`]
    /// or [`SimulationParameter::Velocity`] for a non-finite input;
    /// [`SimulationError::Overflow`] when `start − end` or a product of it
    /// with the spring's rates is not finite.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::simulation::{Simulation, SpringDescription, SpringSimulation, Tolerance};
    ///
    /// let spring = SpringDescription::new(1.0, 100.0, 10.0)?;
    /// let sim = SpringSimulation::try_new(spring, 0.0, 100.0, 0.0, Tolerance::new(0.5, f64::INFINITY)?)?;
    /// assert!(!sim.is_done(0.1));
    /// assert!(SpringSimulation::try_new(spring, f64::NAN, 1.0, 0.0, Tolerance::DEFAULT).is_err());
    /// # Ok::<(), flui_animation::simulation::SimulationError>(())
    /// ```
    pub fn try_new(
        spring: SpringDescription,
        start: f64,
        end: f64,
        velocity: f64,
        tolerance: Tolerance,
    ) -> Result<Self, SimulationError> {
        let start = finite(SimulationParameter::Position, start)?;
        let end = finite(SimulationParameter::Position, end)?;
        let v0 = finite(SimulationParameter::Velocity, velocity)?;
        let SpringDescription { omega, zeta } = spring;
        let x0 = representable(start - end)?;
        let a = omega * zeta;
        let q = omega * omega * ((1.0 - zeta) * (1.0 + zeta));
        let roots = match spring.spring_type() {
            SpringType::Underdamped => Roots::Oscillating {
                damped_omega: q.sqrt(),
            },
            SpringType::CriticallyDamped => Roots::Critical,
            SpringType::Overdamped => {
                // √(ζ² − 1): factored near one (no cancellation), scaled
                // for a huge ζ (no overflow of ζ²).
                let root = if zeta < 1e100 {
                    ((zeta - 1.0) * (zeta + 1.0)).sqrt()
                } else {
                    zeta * (1.0 - zeta.recip().powi(2)).sqrt()
                };
                let spread = zeta + root;
                Roots::Decaying {
                    s: omega * root,
                    slow: -omega / spread,
                    fast: -omega * spread,
                }
            }
        };
        let position_s = representable(a * x0 + v0)?;
        let velocity_s = representable(omega * omega * x0 + a * v0)?;
        if let Roots::Decaying { fast, .. } = roots {
            representable(fast * x0)?;
            representable(fast * v0)?;
        }
        let mut simulation = Self {
            start,
            end,
            x0,
            v0,
            a,
            q,
            position_s,
            velocity_s,
            omega,
            zeta,
            roots,
            rest_secs: 0.0,
            tolerance,
        };
        simulation.rest_secs = simulation.settle_time(tolerance);
        Ok(simulation)
    }

    /// The spring's motion regime.
    #[must_use]
    pub fn spring_type(&self) -> SpringType {
        SpringDescription {
            omega: self.omega,
            zeta: self.zeta,
        }
        .spring_type()
    }

    /// Accepted for source compatibility: a spring always settles exactly at
    /// `end` once at rest.
    #[must_use]
    pub(crate) fn with_snap_to_end(self, _snap: bool) -> Self {
        self
    }

    /// `(e^{−at}·C(t), e^{−at}·S(t))`, where `C = cos(ω_d t)`,
    /// `S = sin(ω_d t)/ω_d` (and their hyperbolic and `ζ = 1` limits).
    fn propagator(&self, t: f64) -> (f64, f64) {
        let qt2 = self.q * t * t;
        if qt2.abs() < SERIES_LIMIT {
            // C = Σ (−q t²)ⁿ/(2n)!, S = t·Σ (−q t²)ⁿ/(2n+1)!.
            let ratio = -qt2;
            let mut cosine = 1.0;
            let mut sine_over_t = 1.0;
            let mut cosine_term = 1.0;
            let mut sine_term = 1.0;
            for order in 1..=6_u32 {
                let order = f64::from(order);
                cosine_term *= ratio / ((2.0 * order - 1.0) * (2.0 * order));
                sine_term *= ratio / ((2.0 * order) * (2.0 * order + 1.0));
                cosine += cosine_term;
                sine_over_t += sine_term;
            }
            let decay = (-self.a * t).exp();
            return (decay * cosine, decay * sine_over_t * t);
        }
        match self.roots {
            Roots::Oscillating { damped_omega } => {
                let decay = (-self.a * t).exp();
                let (sin, cos) = (damped_omega * t).sin_cos();
                (decay * cos, decay * sin / damped_omega)
            }
            // q = 0 always takes the series branch.
            Roots::Critical => {
                let decay = (-self.a * t).exp();
                (decay, decay * t)
            }
            Roots::Decaying { s, slow, .. } => {
                // e^{−at}cosh(st) = ½e^{λs t}(1 + e^{−2st}),
                // e^{−at}sinh(st)/s = e^{λs t}(1 − e^{−2st})/(2s).
                let slow_decay = (slow * t).exp();
                let gap = (-2.0 * s * t).exp_m1();
                (0.5 * slow_decay * (2.0 + gap), slow_decay * -gap / (2.0 * s))
            }
        }
    }

    /// The analytic displacement from `end` at `t ≥ 0`, ignoring rest.
    fn displacement(&self, t: f64) -> f64 {
        let (c, s) = self.propagator(t);
        c * self.x0 + s * self.position_s
    }

    /// The analytic velocity at `t ≥ 0`, ignoring rest.
    fn velocity(&self, t: f64) -> f64 {
        let (c, s) = self.propagator(t);
        c * self.v0 - s * self.velocity_s
    }

    /// The first time after which the displacement stays within
    /// `tolerance.distance` and the speed within the velocity limit.
    fn settle_time(&self, tolerance: Tolerance) -> f64 {
        let distance = tolerance.distance;
        let speed = tolerance.velocity_limit(self.omega);
        let (x0, v0) = (self.x0.abs(), self.v0.abs());
        // e^{−at}|C + aS| ≤ (1 + at)e^{−rt} and e^{−at}|S| ≤ t·e^{−rt}, with
        // r = a (ζ ≤ 1) or −λs (ζ > 1).
        let rate = match self.roots {
            Roots::Decaying { slow, .. } => -slow,
            _ => self.a,
        };
        // A bound specific to the regime first; the polynomial bound is then
        // searched only below it.
        let (position, velocity) = match self.roots {
            Roots::Oscillating { damped_omega } => {
                // |x| ≤ A·e^{−at}, |v| ≤ B·e^{−at}.
                let amplitude_x = self.x0.hypot(self.position_s / damped_omega);
                let amplitude_v = self.v0.hypot(self.velocity_s / damped_omega);
                (
                    settle_exponential(amplitude_x, self.a, distance),
                    settle_exponential(amplitude_v, self.a, speed),
                )
            }
            Roots::Critical => (f64::INFINITY, f64::INFINITY),
            Roots::Decaying { s, slow, fast } => {
                // x = c_s·e^{λs t} + c_f·e^{λf t}.
                let slow_coefficient = (self.v0 - fast * self.x0) / (2.0 * s);
                let fast_coefficient = self.x0 - slow_coefficient;
                (
                    settle_two_exponentials(
                        (slow_coefficient.abs(), slow),
                        (fast_coefficient.abs(), fast),
                        distance,
                    ),
                    settle_two_exponentials(
                        ((slow_coefficient * slow).abs(), slow),
                        ((fast_coefficient * fast).abs(), fast),
                        speed,
                    ),
                )
            }
        };
        let position = settle_polynomial(x0, self.a * x0 + v0, rate, distance, position);
        let velocity = settle_polynomial(
            v0,
            self.omega * self.omega * x0 + self.a * v0,
            rate,
            speed,
            velocity,
        );
        position.max(velocity)
    }
}

impl Simulation for SpringSimulation {
    fn x(&self, time: f64) -> f64 {
        if before_start(time) {
            self.start
        } else if time >= self.rest_secs {
            self.end
        } else {
            self.end + self.displacement(time)
        }
    }

    fn dx(&self, time: f64) -> f64 {
        if before_start(time) {
            self.v0
        } else if time >= self.rest_secs {
            0.0
        } else {
            self.velocity(time)
        }
    }

    fn is_done(&self, time: f64) -> bool {
        time >= self.rest_secs
    }

    fn tolerance(&self) -> Tolerance {
        self.tolerance
    }
}

// ---------------------------------------------------------------------------
// Rest-time envelopes
// ---------------------------------------------------------------------------

/// The bisection budget; each envelope is monotone on the searched interval.
const BISECTION_STEPS: u32 = 128;

/// A rest time is resolved to this many seconds (rounded up, so it stays
/// conservative); a frame is millions of times longer.
const REST_RESOLUTION_SECS: f64 = 1e-7;

/// The smallest `T` with `envelope(t) ≤ limit` for all `t ≥ T`, given that
/// `envelope` (supplied as its natural logarithm) is non-increasing from
/// `from` on and already within `limit` at `known` (pass `+inf` when no
/// such time is known). Returns `from` when the limit holds there and
/// `+inf` when no representable time satisfies it.
fn settle_monotone(
    from: f64,
    scale: f64,
    ln_limit: f64,
    known: f64,
    ln_envelope: impl Fn(f64) -> f64,
) -> f64 {
    if ln_envelope(from) <= ln_limit {
        return from;
    }
    let mut low = from;
    let mut high = if known.is_finite() {
        known
    } else {
        let mut high = from + scale;
        while ln_envelope(high) > ln_limit {
            low = high;
            high = from + 2.0 * (high - from);
            if !high.is_finite() {
                return f64::INFINITY;
            }
        }
        high
    };
    for _ in 0..BISECTION_STEPS {
        if high - low <= REST_RESOLUTION_SECS.max(high * f64::EPSILON) {
            break;
        }
        let middle = low + 0.5 * (high - low);
        if ln_envelope(middle) > ln_limit {
            low = middle;
        } else {
            high = middle;
        }
    }
    high
}

/// Rest time of `e^{−rt}(p + q·t) ≤ limit`, `p, q ≥ 0`, `r > 0`, or `known`
/// when that is earlier (another valid bound of the same motion).
fn settle_polynomial(p: f64, q: f64, rate: f64, limit: f64, known: f64) -> f64 {
    if p == 0.0 && q == 0.0 {
        return 0.0;
    }
    let ln_limit = limit.ln();
    let ln_envelope = |t: f64| {
        let linear = if q == 0.0 {
            p.ln()
        } else {
            q.ln() + (t + p / q).ln()
        };
        linear - rate * t
    };
    // The envelope peaks at 1/r − p/q and decreases after it.
    let peak = if q == 0.0 {
        0.0
    } else {
        (rate.recip() - p / q).max(0.0)
    };
    if ln_envelope(peak) <= ln_limit {
        return 0.0;
    }
    // This bound rests after its peak; `known` is earlier unless it is past
    // the peak and the envelope is already within the limit there.
    if known <= peak || ln_envelope(known) > ln_limit {
        return known;
    }
    settle_monotone(peak, rate.recip(), ln_limit, known, ln_envelope)
}

/// Rest time of `amplitude·e^{−rt} ≤ limit`.
fn settle_exponential(amplitude: f64, rate: f64, limit: f64) -> f64 {
    if amplitude <= limit {
        0.0
    } else {
        // A non-finite amplitude leaves the polynomial bound in charge.
        let time = (amplitude.ln() - limit.ln()) / rate;
        if time.is_nan() { f64::INFINITY } else { time }
    }
}

/// Rest time of `c_s·e^{λs t} + c_f·e^{λf t} ≤ limit`, each term given as
/// `(cᵢ ≥ 0, λᵢ < 0)`.
fn settle_two_exponentials(slow: (f64, f64), fast: (f64, f64), limit: f64) -> f64 {
    let ((slow_amplitude, slow_rate), (fast_amplitude, fast_rate)) = (slow, fast);
    if !(slow_amplitude.is_finite() && fast_amplitude.is_finite()) {
        return f64::INFINITY;
    }
    if slow_amplitude == 0.0 && fast_amplitude == 0.0 {
        return 0.0;
    }
    let ln_envelope = |t: f64| {
        let first = slow_amplitude.ln() + slow_rate * t;
        let second = fast_amplitude.ln() + fast_rate * t;
        let (high, low) = if first >= second {
            (first, second)
        } else {
            (second, first)
        };
        high + (low - high).exp().ln_1p()
    };
    settle_monotone(
        0.0,
        (-slow_rate).recip(),
        limit.ln(),
        f64::INFINITY,
        ln_envelope,
    )
}

// ---------------------------------------------------------------------------
// Friction
// ---------------------------------------------------------------------------

/// Motion decelerating by drag: velocity `v₀·dragᵗ`, coasting toward
/// [`final_x`](Self::final_x).
///
/// It rests once the remaining glide is within the tolerance's distance (and
/// the speed under its velocity limit); from then on `x` is exactly
/// `final_x` and `dx` is `0.0`.
#[derive(Debug, Clone, PartialEq)]
pub struct FrictionSimulation {
    drag: f64,
    drag_log: f64,
    position: f64,
    velocity: f64,
    final_x: f64,
    rest_secs: f64,
    tolerance: Tolerance,
}

impl FrictionSimulation {
    /// Motion from `position` at `velocity` (units per second), losing all
    /// but `drag` (in the open range `(0, 1)`) of its velocity each second.
    ///
    /// # Errors
    ///
    /// [`SimulationError::OutOfRange`] for a `drag` outside `(0, 1)` or a
    /// non-finite `position` or `velocity`; [`SimulationError::Overflow`]
    /// when the resting position is not finite.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::simulation::{FrictionSimulation, Simulation, Tolerance};
    ///
    /// let sim = FrictionSimulation::new(0.135, 0.0, 1000.0, Tolerance::new(0.5, f64::INFINITY)?)?;
    /// assert!(sim.final_x() > 0.0);
    /// assert!(sim.is_done(10.0));
    /// assert_eq!(sim.x(10.0), sim.final_x());
    /// assert!(FrictionSimulation::new(1.0, 0.0, 1.0, Tolerance::DEFAULT).is_err());
    /// # Ok::<(), flui_animation::simulation::SimulationError>(())
    /// ```
    pub fn new(
        drag: f64,
        position: f64,
        velocity: f64,
        tolerance: Tolerance,
    ) -> Result<Self, SimulationError> {
        if drag.is_nan() || drag <= 0.0 || drag >= 1.0 {
            return Err(out_of_range(SimulationParameter::Drag, drag));
        }
        let position = finite(SimulationParameter::Position, position)?;
        let velocity = finite(SimulationParameter::Velocity, velocity)?;
        let drag_log = drag.ln();
        let final_x = representable(position - velocity / drag_log)?;
        // Remaining glide |v(t)/ln d| ≤ distance, and |v(t)| ≤ the limit.
        let speed_limit = tolerance
            .velocity_limit(-drag_log)
            .min(tolerance.distance * -drag_log);
        let rest_secs = if velocity.abs() <= speed_limit {
            0.0
        } else {
            (speed_limit.ln() - velocity.abs().ln()) / drag_log
        };
        Ok(Self {
            drag,
            drag_log,
            position,
            velocity,
            final_x,
            rest_secs,
            tolerance,
        })
    }

    /// The position the motion coasts toward.
    #[must_use]
    pub fn final_x(&self) -> f64 {
        self.final_x
    }

    /// The time at which the unrested motion passes `x`: `0.0` at the start,
    /// `+inf` for a position it never reaches (behind the start, at or past
    /// [`final_x`](Self::final_x), or any position other than the start when
    /// the velocity is zero), NaN for a NaN `x`.
    #[must_use]
    pub fn time_at_x(&self, x: f64) -> f64 {
        if x.is_nan() {
            return f64::NAN;
        }
        if x == self.position {
            return 0.0;
        }
        // x(t) = x₀ + v·(dᵗ − 1)/ln d  ⇒  t = ln(1 + ln d·(x − x₀)/v)/ln d.
        let time = (self.drag_log * (x - self.position) / self.velocity).ln_1p() / self.drag_log;
        if time > 0.0 { time } else { f64::INFINITY }
    }

    fn glide(&self, time: f64) -> f64 {
        // exp_m1 keeps small displacements for drag near one or time near zero.
        self.position + self.velocity * (self.drag_log * time).exp_m1() / self.drag_log
    }

    fn speed(&self, time: f64) -> f64 {
        self.velocity * self.drag.powf(time)
    }
}

impl Simulation for FrictionSimulation {
    fn x(&self, time: f64) -> f64 {
        if before_start(time) {
            self.position
        } else if time >= self.rest_secs {
            self.final_x
        } else {
            self.glide(time)
        }
    }

    fn dx(&self, time: f64) -> f64 {
        if before_start(time) {
            self.velocity
        } else if time >= self.rest_secs {
            0.0
        } else {
            self.speed(time)
        }
    }

    fn is_done(&self, time: f64) -> bool {
        time >= self.rest_secs
    }

    fn tolerance(&self) -> Tolerance {
        self.tolerance
    }
}

// ---------------------------------------------------------------------------
// Bounds
// ---------------------------------------------------------------------------

/// A finite, ordered position range `[min, max]`.
///
/// # Examples
///
/// ```
/// use flui_animation::simulation::SimulationBounds;
///
/// assert!(SimulationBounds::new(0.0, 100.0).is_ok());
/// assert!(SimulationBounds::new(100.0, 0.0).is_err());
/// assert!(SimulationBounds::new(0.0, f64::NAN).is_err());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimulationBounds {
    min: f64,
    max: f64,
}

impl SimulationBounds {
    /// The range `[min, max]`.
    ///
    /// # Errors
    ///
    /// [`SimulationError::InvalidBounds`] when either end is not finite or
    /// `min > max`.
    pub fn new(min: f64, max: f64) -> Result<Self, SimulationError> {
        if min.is_finite() && max.is_finite() && min <= max {
            Ok(Self { min, max })
        } else {
            Err(SimulationError::InvalidBounds { min, max })
        }
    }

    fn contains(self, x: f64) -> bool {
        self.min <= x && x <= self.max
    }
}

/// Friction confined to bounds: it stops at the bound it travels toward, or
/// where the friction rests if that comes first.
///
/// A clamping scroll uses it so a fling that would overshoot the content
/// stops at the edge.
#[derive(Debug, Clone, PartialEq)]
pub struct BoundedFrictionSimulation {
    friction: FrictionSimulation,
    bounds: SimulationBounds,
    rest_x: f64,
    rest_secs: f64,
}

impl BoundedFrictionSimulation {
    /// Friction from `position` at `velocity` with `drag`, confined to
    /// `bounds`.
    ///
    /// # Errors
    ///
    /// As [`FrictionSimulation::new`].
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::simulation::{BoundedFrictionSimulation, Simulation, SimulationBounds, Tolerance};
    ///
    /// let bounds = SimulationBounds::new(0.0, 100.0)?;
    /// let sim = BoundedFrictionSimulation::new(0.135, 50.0, 8000.0, bounds, Tolerance::DEFAULT)?;
    /// assert!(sim.is_done(1.0));
    /// assert_eq!(sim.x(1.0), 100.0);
    /// # Ok::<(), flui_animation::simulation::SimulationError>(())
    /// ```
    pub fn new(
        drag: f64,
        position: f64,
        velocity: f64,
        bounds: SimulationBounds,
        tolerance: Tolerance,
    ) -> Result<Self, SimulationError> {
        let friction = FrictionSimulation::new(drag, position, velocity, tolerance)?;
        let bound = if velocity > 0.0 {
            bounds.max
        } else {
            bounds.min
        };
        let reach = if velocity == 0.0 {
            f64::INFINITY
        } else if (velocity > 0.0 && position >= bound) || (velocity < 0.0 && position <= bound)
        {
            0.0
        } else {
            friction.time_at_x(bound)
        };
        let (rest_x, rest_secs) = if reach < friction.rest_secs {
            (bound, reach)
        } else {
            (
                friction.final_x.clamp(bounds.min, bounds.max),
                friction.rest_secs,
            )
        };
        Ok(Self {
            friction,
            bounds,
            rest_x,
            rest_secs,
        })
    }
}

impl Simulation for BoundedFrictionSimulation {
    fn x(&self, time: f64) -> f64 {
        if before_start(time) {
            self.friction.position
        } else if time >= self.rest_secs {
            self.rest_x
        } else {
            self.friction
                .glide(time)
                .clamp(self.bounds.min, self.bounds.max)
        }
    }

    fn dx(&self, time: f64) -> f64 {
        if before_start(time) {
            self.friction.velocity
        } else if time >= self.rest_secs {
            0.0
        } else {
            self.friction.speed(time)
        }
    }

    fn is_done(&self, time: f64) -> bool {
        time >= self.rest_secs
    }

    fn tolerance(&self) -> Tolerance {
        self.friction.tolerance
    }
}

// ---------------------------------------------------------------------------
// BouncingScrollSimulation
// ---------------------------------------------------------------------------

/// A scroll fling with overscroll: friction inside the bounds; where the
/// friction would carry past an edge, a spring takes over at the edge with
/// the friction's velocity, overshoots and returns to rest exactly on the
/// edge. A start outside the bounds springs back to the nearest edge.
///
/// Position and velocity are continuous where the spring takes over.
#[derive(Debug, Clone, PartialEq)]
pub struct BouncingScrollSimulation {
    phase: BouncingPhase,
    rest_secs: f64,
}

#[derive(Debug, Clone, PartialEq)]
enum BouncingPhase {
    /// The friction rests inside the bounds.
    Friction(FrictionSimulation),
    /// The start is outside the bounds.
    Spring(SpringSimulation),
    /// Friction until `edge_secs`, then the edge spring.
    Handoff {
        friction: FrictionSimulation,
        spring: SpringSimulation,
        edge_secs: f64,
    },
}

impl BouncingScrollSimulation {
    /// A fling from `position` at `velocity` with friction `drag`, bouncing
    /// off `bounds` on `spring`, resting within `tolerance`.
    ///
    /// # Errors
    ///
    /// As [`FrictionSimulation::new`] and [`SpringSimulation::try_new`].
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::simulation::{
    ///     BouncingScrollSimulation, Simulation, SimulationBounds, SpringDescription, Tolerance,
    /// };
    ///
    /// let spring = SpringDescription::with_damping_ratio(1.0, 500.0, 0.75);
    /// let bounds = SimulationBounds::new(0.0, 100.0)?;
    /// let sim = BouncingScrollSimulation::new(spring, 0.135, 90.0, 2000.0, bounds, Tolerance::DEFAULT)?;
    /// let overscroll = (1..200).map(|i| sim.x(f64::from(i) * 0.01)).fold(f64::MIN, f64::max);
    /// assert!(overscroll > 100.0);
    /// assert!(sim.is_done(10.0));
    /// assert_eq!(sim.x(10.0), 100.0);
    /// # Ok::<(), flui_animation::simulation::SimulationError>(())
    /// ```
    pub fn new(
        spring: SpringDescription,
        drag: f64,
        position: f64,
        velocity: f64,
        bounds: SimulationBounds,
        tolerance: Tolerance,
    ) -> Result<Self, SimulationError> {
        let edge_spring =
            |edge, from, v| SpringSimulation::try_new(spring, from, edge, v, tolerance);
        let position = finite(SimulationParameter::Position, position)?;
        let phase = if position < bounds.min {
            BouncingPhase::Spring(edge_spring(bounds.min, position, velocity)?)
        } else if position > bounds.max {
            BouncingPhase::Spring(edge_spring(bounds.max, position, velocity)?)
        } else {
            let friction = FrictionSimulation::new(drag, position, velocity, tolerance)?;
            if bounds.contains(friction.final_x) {
                BouncingPhase::Friction(friction)
            } else {
                let edge = if friction.final_x > bounds.max {
                    bounds.max
                } else {
                    bounds.min
                };
                let edge_secs = friction.time_at_x(edge);
                let edge_secs = if edge_secs.is_finite() { edge_secs } else { 0.0 };
                let spring = edge_spring(edge, edge, friction.speed(edge_secs))?;
                BouncingPhase::Handoff {
                    friction,
                    spring,
                    edge_secs,
                }
            }
        };
        let rest_secs = match &phase {
            BouncingPhase::Friction(friction) => friction.rest_secs,
            BouncingPhase::Spring(spring) => spring.rest_secs,
            BouncingPhase::Handoff {
                spring, edge_secs, ..
            } => edge_secs + spring.rest_secs,
        };
        Ok(Self { phase, rest_secs })
    }
}

impl Simulation for BouncingScrollSimulation {
    fn x(&self, time: f64) -> f64 {
        match &self.phase {
            BouncingPhase::Friction(friction) => friction.x(time),
            BouncingPhase::Spring(spring) => spring.x(time),
            BouncingPhase::Handoff {
                friction,
                spring,
                edge_secs,
            } => {
                if before_start(time) {
                    friction.position
                } else if time < *edge_secs {
                    friction.glide(time)
                } else {
                    spring.x(time - edge_secs)
                }
            }
        }
    }

    fn dx(&self, time: f64) -> f64 {
        match &self.phase {
            BouncingPhase::Friction(friction) => friction.dx(time),
            BouncingPhase::Spring(spring) => spring.dx(time),
            BouncingPhase::Handoff {
                friction,
                spring,
                edge_secs,
            } => {
                if before_start(time) {
                    friction.velocity
                } else if time < *edge_secs {
                    friction.speed(time)
                } else {
                    spring.dx(time - edge_secs)
                }
            }
        }
    }

    fn is_done(&self, time: f64) -> bool {
        time >= self.rest_secs
    }

    fn tolerance(&self) -> Tolerance {
        match &self.phase {
            BouncingPhase::Friction(friction) => friction.tolerance,
            BouncingPhase::Spring(spring) | BouncingPhase::Handoff { spring, .. } => {
                spring.tolerance
            }
        }
    }
}
