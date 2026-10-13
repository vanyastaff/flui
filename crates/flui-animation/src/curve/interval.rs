//! `Interval`: one curve played over a slice of the parent's progress.

use super::{
    Builtin, BuiltinCurve, Curve, CurveError, CurveParameter, Linear, difference_slope, settled,
    settled_slope, within,
};
/// A curve that is 0 until `begin`, follows `curve` rescaled to
/// `[begin, end]`, and is 1 after `end`.
///
/// Lets several animations share one controller, each running in its own
/// slice of the controller's progress. An interval narrower than `1e-6` is a
/// step at `end`. Monotone when `curve` is.
///
/// # Examples
///
/// ```
/// use flui_animation::{Curve, curves, Interval};
///
/// let late = Interval::new(0.5, 1.0, curves::LINEAR);
/// assert_eq!(late.transform(0.25), 0.0);
/// assert_eq!(late.transform(0.75), 0.5);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "serde",
    serde(
        try_from = "IntervalWire<C>",
        into = "IntervalWire<C>",
        bound(
            serialize = "C: serde::Serialize",
            deserialize = "C: serde::Deserialize<'de>"
        )
    )
)]
pub struct Interval<C: Curve + Copy = Linear> {
    begin: f64,
    end: f64,
    curve: C,
}

impl<C: Curve + Copy> Interval<C> {
    /// Admits `0 <= begin <= end <= 1`, both finite.
    const fn validate(begin: f64, end: f64) -> Result<(), CurveError> {
        check!(within(begin, 0.0, 1.0, CurveParameter::Begin));
        check!(within(end, 0.0, 1.0, CurveParameter::End));
        if begin > end {
            return Err(CurveError::IntervalReversed { begin, end });
        }
        Ok(())
    }

    /// Creates an interval curve running `curve` over `[begin, end]`.
    ///
    /// # Panics
    ///
    /// Panics when [`Interval::try_new`] would return an error. In a `const`
    /// item the panic is a compile error.
    #[must_use]
    pub const fn new(begin: f64, end: f64, curve: C) -> Self {
        let interval = Self { begin, end, curve };
        match Self::validate(begin, end) {
            Ok(()) => interval,
            Err(_) => panic!(
                "Interval::new: begin and end must be finite with 0 <= begin <= end <= 1 \
                 (Interval::try_new reports which)"
            ),
        }
    }

    /// Creates an interval curve, rejecting invalid bounds.
    ///
    /// # Errors
    ///
    /// - [`CurveError::NonFinite`] when `begin` or `end` is NaN or infinite;
    /// - [`CurveError::OutOfRange`] when either lies outside `[0, 1]`;
    /// - [`CurveError::IntervalReversed`] when `begin > end`.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::{CurveError, Interval, Linear};
    ///
    /// assert!(Interval::try_new(0.2, 0.8, Linear).is_ok());
    /// assert!(matches!(
    ///     Interval::try_new(0.8, 0.2, Linear),
    ///     Err(CurveError::IntervalReversed { .. })
    /// ));
    /// ```
    pub fn try_new(begin: f64, end: f64, curve: C) -> Result<Self, CurveError> {
        Self::validate(begin, end)?;
        Ok(Self { begin, end, curve })
    }
}

impl Interval<Linear> {
    /// Creates a linear interval over `[begin, end]`.
    ///
    /// # Panics
    ///
    /// Panics on the bounds [`Interval::try_new`] rejects.
    #[must_use]
    pub const fn linear(begin: f64, end: f64) -> Self {
        Self::new(begin, end, Linear)
    }
}

impl<C: Curve + Copy> Curve for Interval<C> {
    fn transform(&self, t: f64) -> f64 {
        interval_transform(self.begin, self.end, t, |local| self.curve.transform(local))
    }

    fn slope(&self, t: f64) -> f64 {
        interval_slope(
            self.begin,
            self.end,
            t,
            |local| self.curve.slope(local),
            |local| self.curve.transform(local),
        )
    }

    fn builtin(&self) -> Option<BuiltinCurve> {
        let curve = self.curve.builtin()?;
        Some(BuiltinCurve::new(Builtin::Interval {
            begin: self.begin,
            end: self.end,
            curve,
        }))
    }
}

/// [`Interval`]'s derivative by the chain rule: 0 outside `[begin, end]`
/// and for a step, `slope(local) / (end − begin)` inside. Where that quotient
/// overflows (a steep inner curve in a narrow interval) the finite
/// difference of the interval's own shape stands in.
pub(super) fn interval_slope(
    begin: f64,
    end: f64,
    t: f64,
    slope: impl FnOnce(f64) -> f64,
    transform: impl Fn(f64) -> f64,
) -> f64 {
    if let Some(settled) = settled_slope(t) {
        return settled;
    }
    if t < begin || t > end || end - begin < 1e-6 {
        return 0.0;
    }
    let chained = slope((t - begin) / (end - begin)) / (end - begin);
    if chained.is_finite() {
        chained
    } else {
        difference_slope(|t| interval_transform(begin, end, t, &transform), t)
    }
}

/// [`Interval`]'s shape over validated bounds, with `inner` the curve
/// evaluated on the local progress.
pub(super) fn interval_transform(
    begin: f64,
    end: f64,
    t: f64,
    inner: impl FnOnce(f64) -> f64,
) -> f64 {
    if let Some(settled) = settled(t) {
        return settled;
    }
    if t < begin {
        0.0
    } else if t > end {
        1.0
    } else if end - begin < 1e-6 {
        if t < end { 0.0 } else { 1.0 }
    } else {
        inner((t - begin) / (end - begin))
    }
}

/// The serialized form of [`Interval`]: the field names it always had.
#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename = "Interval")]
struct IntervalWire<C> {
    begin: f64,
    end: f64,
    curve: C,
}

#[cfg(feature = "serde")]
impl<C: Curve + Copy> From<Interval<C>> for IntervalWire<C> {
    fn from(interval: Interval<C>) -> Self {
        Self {
            begin: interval.begin,
            end: interval.end,
            curve: interval.curve,
        }
    }
}

#[cfg(feature = "serde")]
impl<C: Curve + Copy> TryFrom<IntervalWire<C>> for Interval<C> {
    type Error = CurveError;

    fn try_from(wire: IntervalWire<C>) -> Result<Self, CurveError> {
        Self::try_new(wire.begin, wire.end, wire.curve)
    }
}
