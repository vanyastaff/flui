//! Elastic, bounce and decelerate easing shapes.

use super::{Curve, CurveError, CurveParameter, settled, within};
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

/// Declares an elastic curve type: the period field, its validated
/// constructors, `Default` (period 0.4) and its serde form.
macro_rules! elastic_curve {
    ($(#[$doc:meta])* $name:ident = $name_str:literal, $wire:ident = $wire_str:literal) => {
        $(#[$doc])*
        ///
        /// Not monotone: it overshoots `[0, 1]`. Continuous at both ends.
        ///
        /// # Serde
        ///
        /// Serializes as `{ "period": … }`; decoding rejects what `try_new`
        /// rejects.
        #[derive(Debug, Clone, Copy, PartialEq)]
        #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
        #[cfg_attr(feature = "serde", serde(try_from = $wire_str, into = $wire_str))]
        pub struct $name {
            period: f64,
        }

        impl $name {
            /// Creates the curve with the given oscillation `period`, as a fraction
            /// of the animation.
            ///
            /// # Panics
            ///
            /// Panics when the period lies outside `[1e-6, 1e6]`. In a `const`
            /// item the panic is a compile error.
            #[must_use]
            pub const fn new(period: f64) -> Self {
                match validate_period(period) {
                    Ok(()) => Self { period },
                    Err(_) => panic!(concat!(
                        stringify!($name),
                        "::new: period must lie in [1e-6, 1e6]"
                    )),
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
            #[doc = concat!("use flui_animation::{CurveError, ", stringify!($name), "};")]
            ///
            #[doc = concat!("assert!(", stringify!($name), "::try_new(0.3).is_ok());")]
            #[doc = concat!("assert!(matches!(", stringify!($name), "::try_new(0.0), Err(CurveError::OutOfRange { .. })));")]
            /// ```
            pub fn try_new(period: f64) -> Result<Self, CurveError> {
                validate_period(period)?;
                Ok(Self { period })
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new(0.4)
            }
        }

        #[cfg(feature = "serde")]
        #[derive(serde::Serialize, serde::Deserialize)]
        #[serde(rename = $name_str)]
        struct $wire {
            period: f64,
        }

        #[cfg(feature = "serde")]
        impl From<$name> for $wire {
            fn from(curve: $name) -> Self {
                Self {
                    period: curve.period,
                }
            }
        }

        #[cfg(feature = "serde")]
        impl TryFrom<$wire> for $name {
            type Error = CurveError;

            fn try_from(wire: $wire) -> Result<Self, CurveError> {
                Self::try_new(wire.period)
            }
        }
    };
}

elastic_curve!(
    /// An oscillating curve that grows in magnitude while overshooting its
    /// bounds.
    ElasticInCurve = "ElasticInCurve",
    ElasticInWire = "ElasticInWire"
);

elastic_curve!(
    /// An oscillating curve that shrinks in magnitude while overshooting its
    /// bounds.
    ElasticOutCurve = "ElasticOutCurve",
    ElasticOutWire = "ElasticOutWire"
);

elastic_curve!(
    /// An oscillating curve that grows and then shrinks in magnitude while
    /// overshooting its bounds.
    ElasticInOutCurve = "ElasticInOutCurve",
    ElasticInOutWire = "ElasticInOutWire"
);

impl Curve for ElasticInCurve {
    builtin_value!(ElasticIn(self));

    fn transform(&self, t: f64) -> f64 {
        if let Some(settled) = settled(t) {
            return settled;
        }
        -elastic_envelope(1.0 - t) * elastic_wave(t - 1.0, self.period)
    }
}

impl Curve for ElasticOutCurve {
    builtin_value!(ElasticOut(self));

    fn transform(&self, t: f64) -> f64 {
        if let Some(settled) = settled(t) {
            return settled;
        }
        elastic_envelope(t) * elastic_wave(t, self.period) + 1.0
    }
}

impl Curve for ElasticInOutCurve {
    builtin_value!(ElasticInOut(self));

    fn transform(&self, t: f64) -> f64 {
        if let Some(settled) = settled(t) {
            return settled;
        }
        let u = 2.0 * t - 1.0;
        if u < 0.0 {
            -0.5 * elastic_envelope(-u) * elastic_wave(u, self.period)
        } else {
            0.5 * elastic_envelope(u) * elastic_wave(u, self.period) + 1.0
        }
    }
}

// ============================================================================
// Bounce curves
// ============================================================================

/// A bounce curve that bounces at the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BounceOutCurve;

impl Curve for BounceOutCurve {
    builtin_value!(BounceOut);

    fn transform(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        bounce_out(t)
    }
}

/// A bounce curve that bounces at the beginning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BounceInCurve;

impl Curve for BounceInCurve {
    builtin_value!(BounceIn);

    fn transform(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        1.0 - bounce_out(1.0 - t)
    }
}

/// A bounce curve that bounces at both ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BounceInOutCurve;

impl Curve for BounceInOutCurve {
    builtin_value!(BounceInOut);

    fn transform(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        if t < 0.5 {
            (1.0 - bounce_out(1.0 - t * 2.0)) * 0.5
        } else {
            bounce_out(t * 2.0 - 1.0) * 0.5 + 0.5
        }
    }
}

/// Helper function for bounce calculations.
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
// Decelerate Curve
// ============================================================================

/// A curve where the rate of change starts fast and then decelerates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DecelerateCurve;

impl Curve for DecelerateCurve {
    builtin_value!(Decelerate);

    #[inline]
    fn transform(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        1.0 - (1.0 - t) * (1.0 - t)
    }
}
