//! Elastic, bounce and decelerate easing shapes.

use super::{Curve, CurveError, CurveParameter, settled, within};

/// Which end of an easing family's shape is eased.
///
/// `Out` settles into the end value (the shape most UI motion wants), `In`
/// leaves the start value the same way, and `InOut` does both over halves of
/// the animation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Ease {
    /// Eases away from the start.
    In,
    /// Eases into the end.
    #[default]
    Out,
    /// Eases away from the start and into the end.
    InOut,
}

// ============================================================================
// Elastic
// ============================================================================

/// `2^-10`: the Penner elastic envelope's value at the far end.
const ELASTIC_ENVELOPE_FLOOR: f64 = 1.0 / 1024.0;

/// The Penner elastic envelope `2^(-10 u)`, rescaled so it falls from exactly
/// 1 at `u = 0` to exactly 0 at `u = 1`.
///
/// The unscaled envelope ends at `2^-10`, so the curve jumped by up to
/// `2^-10` onto its end value on the last frame. Rescaling moves the shape by
/// at most `2^-10` and makes it continuous at both ends.
#[inline]
fn elastic_envelope(u: f64) -> f64 {
    (2.0_f64.powf(-10.0 * u) - ELASTIC_ENVELOPE_FLOOR) / (1.0 - ELASTIC_ENVELOPE_FLOOR)
}

/// The elastic oscillation at phase `u` for `period`.
#[inline]
fn elastic_wave(u: f64, period: f64) -> f64 {
    ((u - period / 4.0) * std::f64::consts::TAU / period).sin()
}

/// Admits an elastic period in `[1e-6, 1e6]`: covers every visible
/// oscillation and is narrow enough that the phase `(u − period / 4)·τ / period`
/// stays finite (an unbounded period overflows the product, a subnormal one
/// the quotient).
const fn validate_period(period: f64) -> Result<(), CurveError> {
    within(period, 1e-6, 1e6, CurveParameter::Period)
}

/// An oscillating curve that overshoots its bounds: growing away from the
/// start ([`Ease::In`]), shrinking into the end ([`Ease::Out`]), or both.
///
/// Not monotone. Continuous at both ends. The default is [`Ease::Out`] with a
/// period of 0.4.
///
/// # Serde
///
/// Serializes as `{ "ease": …, "period": … }`; a missing `ease` decodes as
/// [`Ease::Out`], and decoding rejects what [`try_new`](Self::try_new) rejects.
///
/// # Examples
///
/// ```
/// use flui_animation::{Curve, Ease, Elastic};
///
/// let wobble = Elastic::new(Ease::Out, 0.3);
/// assert_eq!(wobble.transform(1.0), 1.0);
/// assert!(wobble.transform(0.2) > 1.0, "overshoots the end");
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "serde",
    serde(try_from = "ElasticWire", into = "ElasticWire")
)]
pub struct Elastic {
    ease: Ease,
    period: f64,
}

impl Elastic {
    /// Creates the curve with the given oscillation `period`, as a fraction
    /// of the animation.
    ///
    /// # Panics
    ///
    /// Panics when the period lies outside `[1e-6, 1e6]`. In a `const` item
    /// the panic is a compile error.
    #[must_use]
    pub const fn new(ease: Ease, period: f64) -> Self {
        match validate_period(period) {
            Ok(()) => Self { ease, period },
            Err(_) => panic!("Elastic::new: period must lie in [1e-6, 1e6]"),
        }
    }

    /// Creates the curve, rejecting an invalid period.
    ///
    /// # Errors
    ///
    /// - [`CurveError::NonFinite`] when `period` is NaN or infinite;
    /// - [`CurveError::OutOfRange`] when `period` lies outside `[1e-6, 1e6]`.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::{CurveError, Ease, Elastic};
    ///
    /// assert!(Elastic::try_new(Ease::In, 0.3).is_ok());
    /// assert!(matches!(Elastic::try_new(Ease::In, 0.0), Err(CurveError::OutOfRange { .. })));
    /// ```
    pub fn try_new(ease: Ease, period: f64) -> Result<Self, CurveError> {
        validate_period(period)?;
        Ok(Self { ease, period })
    }

    /// Which end is eased.
    #[must_use]
    pub const fn ease(&self) -> Ease {
        self.ease
    }

    /// The oscillation period, as a fraction of the animation.
    #[must_use]
    pub const fn period(&self) -> f64 {
        self.period
    }
}

impl Default for Elastic {
    fn default() -> Self {
        Self::new(Ease::Out, 0.4)
    }
}

impl Curve for Elastic {
    builtin_value!(Elastic(self));

    fn transform(&self, t: f64) -> f64 {
        if let Some(settled) = settled(t) {
            return settled;
        }
        let period = self.period;
        match self.ease {
            Ease::In => -elastic_envelope(1.0 - t) * elastic_wave(t - 1.0, period),
            Ease::Out => elastic_envelope(t) * elastic_wave(t, period) + 1.0,
            Ease::InOut => {
                let u = 2.0 * t - 1.0;
                if u < 0.0 {
                    -0.5 * elastic_envelope(-u) * elastic_wave(u, period)
                } else {
                    0.5 * elastic_envelope(u) * elastic_wave(u, period) + 1.0
                }
            }
        }
    }
}

#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename = "Elastic")]
struct ElasticWire {
    #[serde(default)]
    ease: Ease,
    period: f64,
}

#[cfg(feature = "serde")]
impl From<Elastic> for ElasticWire {
    fn from(curve: Elastic) -> Self {
        Self {
            ease: curve.ease,
            period: curve.period,
        }
    }
}

#[cfg(feature = "serde")]
impl TryFrom<ElasticWire> for Elastic {
    type Error = CurveError;

    fn try_from(wire: ElasticWire) -> Result<Self, CurveError> {
        Self::try_new(wire.ease, wire.period)
    }
}

// ============================================================================
// Bounce
// ============================================================================

/// A curve that bounces like a dropped ball, staying within `[0, 1]`: at the
/// start ([`Ease::In`]), at the end ([`Ease::Out`]), or at both.
///
/// ```
/// use flui_animation::{Bounce, Curve, Ease};
///
/// let landing = Bounce(Ease::Out);
/// assert_eq!(landing.transform(1.0), 1.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Bounce(pub Ease);

impl Curve for Bounce {
    builtin_value!(Bounce(self));

    fn transform(&self, t: f64) -> f64 {
        if let Some(settled) = settled(t) {
            return settled;
        }
        match self.0 {
            Ease::In => 1.0 - bounce_out(1.0 - t),
            Ease::Out => bounce_out(t),
            Ease::InOut => {
                if t < 0.5 {
                    (1.0 - bounce_out(1.0 - t * 2.0)) * 0.5
                } else {
                    bounce_out(t * 2.0 - 1.0) * 0.5 + 0.5
                }
            }
        }
    }
}

/// The Penner bounce-out shape: four parabolic arcs landing exactly on 1.
#[inline]
fn bounce_out(t: f64) -> f64 {
    const N1: f64 = 7.5625;
    const D1: f64 = 2.75;

    if t < 1.0 / D1 {
        N1 * t * t
    } else if t < 2.0 / D1 {
        let t = t - 1.5 / D1;
        N1 * t * t + 0.75
    } else if t < 2.5 / D1 {
        let t = t - 2.25 / D1;
        N1 * t * t + 0.9375
    } else {
        let t = t - 2.625 / D1;
        N1 * t * t + 0.984_375
    }
}

// ============================================================================
// Decelerate
// ============================================================================

/// A curve whose rate of change starts fast and then decelerates: the
/// quadratic ease-out `1 − (1 − t)²`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Decelerate;

impl Curve for Decelerate {
    builtin_value!(Decelerate);

    #[inline]
    fn transform(&self, t: f64) -> f64 {
        if let Some(settled) = settled(t) {
            return settled;
        }
        1.0 - (1.0 - t) * (1.0 - t)
    }
}
