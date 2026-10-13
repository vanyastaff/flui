//! `Split`: one curve before a progress point, another after it.

use super::{
    Builtin, BuiltinCurve, Cubic, Curve, CurveError, CurveParameter, Linear, settled, within,
};
use crate::animation::{Retirement, Terminal};
use crate::curves;
/// A curve that progresses according to `begin_curve` until `split`, then
/// according to `end_curve`.
///
/// Useful when a widget must track the user's finger (linear) and then be
/// flung with an easing curve after release: `split` is the animation
/// progress at the moment of release.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Split<B: Curve = Linear, E: Curve = Cubic> {
    #[cfg_attr(feature = "serde", serde(rename = "split"))]
    boundary: f64,
    begin_curve: Terminal<B>,
    end_curve: Terminal<E>,
}

/// Decode owns each completed field until the complete split is admitted.
/// Failed/partial decoding retains opaque curves so its error stays authoritative.
#[cfg(feature = "serde")]
struct DecodedCurve<T>(Option<T>);

#[cfg(feature = "serde")]
impl<T> DecodedCurve<T> {
    fn commit(mut self) -> Terminal<T> {
        Terminal::new(self.0.take().expect("BUG: decoded curve is committed once"))
    }
}

#[cfg(feature = "serde")]
impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for DecodedCurve<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(deserializer).map(|value| Self(Some(value)))
    }
}

#[cfg(feature = "serde")]
impl<T> Drop for DecodedCurve<T> {
    fn drop(&mut self) {
        std::mem::forget(self.0.take());
    }
}

#[cfg(feature = "serde")]
impl<'de, B, E> serde::Deserialize<'de> for Split<B, E>
where
    B: Curve + serde::Deserialize<'de>,
    E: Curve + serde::Deserialize<'de>,
{
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename = "Split")]
        struct Fields<B, E> {
            split: f64,
            begin_curve: DecodedCurve<B>,
            end_curve: DecodedCurve<E>,
        }
        let fields = <Fields<B, E> as serde::Deserialize>::deserialize(deserializer)?;
        if let Err(error) = validate_split(fields.split) {
            // Preserve the validation error without invoking opaque curve
            // destruction on the rejected aggregate.
            return Err(serde::de::Error::custom(error));
        }
        Ok(Self {
            boundary: fields.split,
            begin_curve: fields.begin_curve.commit(),
            end_curve: fields.end_curve.commit(),
        })
    }
}

/// Admits a finite split point in `[0, 1]`.
const fn validate_split(split: f64) -> Result<(), CurveError> {
    within(split, 0.0, 1.0, CurveParameter::Split)
}

impl<B: Curve, E: Curve> Drop for Split<B, E> {
    fn drop(&mut self) {
        let begin = self.begin_curve.withdraw();
        let end = self.end_curve.withdraw();
        let mut retirement = Retirement::new();
        retirement.retire(begin);
        retirement.retire(end);
        retirement.finish();
    }
}

impl Split<Linear, Cubic> {
    /// Creates a split curve with the default halves: linear before `split`,
    /// `curves::EASE_OUT_CUBIC` after.
    #[must_use]
    pub fn new(split: f64) -> Self {
        Self::with_curves(split, Linear, curves::EASE_OUT_CUBIC)
    }
}

impl<B: Curve, E: Curve> Split<B, E> {
    /// The checked progress value separating the curves, in `[0, 1]`.
    #[must_use]
    pub fn split(&self) -> f64 {
        self.boundary
    }

    /// The curve sampled before the split.
    #[must_use]
    pub fn begin_curve(&self) -> &B {
        self.begin_curve.get()
    }

    /// The curve sampled at and after the split.
    #[must_use]
    pub fn end_curve(&self) -> &E {
        self.end_curve.get()
    }

    /// Creates a split curve with explicit segment curves.
    ///
    /// # Panics
    ///
    /// Panics with the [`CurveError`] message when `split` is not finite or
    /// lies outside `[0, 1]`; [`try_with_curves`](Self::try_with_curves)
    /// returns that error instead.
    #[must_use]
    pub fn with_curves(split: f64, begin_curve: B, end_curve: E) -> Self {
        let begin_curve = Terminal::new(begin_curve);
        let end_curve = Terminal::new(end_curve);
        if let Err(error) = validate_split(split) {
            panic!("{error}");
        }
        Self {
            boundary: split,
            begin_curve,
            end_curve,
        }
    }

    /// Creates a split curve with explicit segment curves, rejecting an
    /// invalid split point.
    ///
    /// # Errors
    ///
    /// - [`CurveError::NonFinite`] when `split` is NaN or infinite;
    /// - [`CurveError::OutOfRange`] when `split` lies outside `[0, 1]`.
    ///
    /// The rejected curves are dropped before the error returns.
    pub fn try_with_curves(split: f64, begin_curve: B, end_curve: E) -> Result<Self, CurveError> {
        if let Err(error) = validate_split(split) {
            let mut retirement = Retirement::new();
            retirement.retire(begin_curve);
            retirement.retire(end_curve);
            retirement.finish();
            return Err(error);
        }
        Ok(Self::with_curves(split, begin_curve, end_curve))
    }
}

impl<B: Curve, E: Curve> Curve for Split<B, E> {
    fn transform(&self, t: f64) -> f64 {
        split_transform(
            self.boundary,
            t,
            |progress| self.begin_curve.transform(progress),
            |progress| self.end_curve.transform(progress),
        )
    }

    /// A split of two built-in halves is a value; with a user half it keeps
    /// identity equality.
    fn builtin(&self) -> Option<BuiltinCurve> {
        Some(BuiltinCurve::new(Builtin::Split {
            split: self.boundary,
            begin: self.begin_curve.builtin()?,
            end: self.end_curve.builtin()?,
        }))
    }
}

/// `begin` over `[0, boundary)` and `end` over `(boundary, 1]`, each rescaled
/// to its share of the progress; the boundary maps to itself exactly.
#[expect(
    clippy::float_cmp,
    reason = "the boundary itself maps to itself exactly"
)]
pub(super) fn split_transform(
    boundary: f64,
    t: f64,
    begin: impl FnOnce(f64) -> f64,
    end: impl FnOnce(f64) -> f64,
) -> f64 {
    if let Some(settled) = settled(t) {
        return settled;
    }
    if t == boundary {
        return boundary;
    }
    if t < boundary {
        // `t < split` implies `split > 0`, so the division is safe.
        boundary * begin(t / boundary)
    } else {
        // `t > split` implies `split < 1`, so the division is safe.
        boundary + (1.0 - boundary) * end((t - boundary) / (1.0 - boundary))
    }
}
