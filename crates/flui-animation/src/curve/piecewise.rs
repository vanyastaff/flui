//! Canonical stops for a normalized piecewise-linear easing.

use flui_foundation::geometry::Lerp;
use std::sync::Arc;

use super::{Builtin, BuiltinCurve, Curve};

/// An output value at an optional input progress on a [`PiecewiseLinear`] curve.
///
/// Missing inputs are distributed evenly between their explicit neighbours.
/// Build with [`Self::new`] and optionally [`Self::at`]; validation occurs when
/// the stops are admitted by [`PiecewiseLinear::try_new`]. Two stops with the same
/// output describe a plateau; equal inputs describe an instantaneous jump.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LinearStop {
    output: f64,
    input: Option<f64>,
}

impl LinearStop {
    /// An output whose input progress will be assigned during canonicalization.
    #[must_use]
    pub const fn new(output: f64) -> Self {
        Self {
            output,
            input: None,
        }
    }

    /// Place this stop at `input` progress, normalized to `[0, 1]`.
    #[must_use]
    pub const fn at(mut self, input: f64) -> Self {
        self.input = Some(input);
        self
    }
}

/// Why [`PiecewiseLinear::try_new`] refused its stops.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum PiecewiseLinearError {
    /// A normalized curve requires at least a start and an end.
    #[error("piecewise linear easing requires at least two stops")]
    TooFewStops,
    /// The stop count does not fit the exact integer spacing calculation.
    #[error("piecewise linear easing supports at most u32::MAX stops")]
    TooManyStops,
    /// The output of zero-based stop `index` is NaN or infinite.
    #[error("stop {index} has a non-finite output")]
    NonFiniteOutput {
        /// The refused stop's zero-based index.
        index: usize,
    },
    /// An explicit input of zero-based stop `index` is NaN or infinite.
    #[error("stop {index} has a non-finite input")]
    NonFiniteInput {
        /// The refused stop's zero-based index.
        index: usize,
    },
    /// An explicit input of zero-based stop `index` is finite but outside `[0, 1]`.
    #[error("stop {index} has input {value} outside [0, 1]")]
    InputOutOfRange {
        /// The refused stop's zero-based index.
        index: usize,
        /// The refused finite input progress.
        value: f64,
    },
    /// Canonical endpoints must be `(input, output) = (0, 0)` and `(1, 1)`.
    #[error("piecewise linear easing must start at (0, 0) and end at (1, 1)")]
    InvalidEndpoints,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Knot {
    input: f64,
    output: f64,
}

/// Immutable linear interpolation between canonicalized stops.
///
/// Inside `(0, 1)`, stop placement follows CSS Easing Level 2: missing first
/// and last inputs become 0 and 1, decreasing explicit inputs are raised to
/// the preceding maximum, and missing interior inputs are evenly distributed.
/// At an interior jump the last stop at that input wins (right continuity).
/// Outputs may overshoot or reverse direction; monotonicity depends on them.
///
/// FLUI's [`Curve`] contract additionally requires normalized endpoints,
/// clamps exterior inputs, and preserves NaN. This is not a CSS parser or an
/// extrapolating CSS timing function. In particular, endpoint outputs are
/// exactly 0 and 1 even when multiple stops occupy an endpoint.
///
/// Construction owns an immutable shared slice. Sampling allocates nothing.
/// Independently constructed equivalent curves compare by value in [`super::SharedCurve`].
///
/// ```
/// use flui_animation::{Curve, LinearStop, PiecewiseLinear};
/// let curve = PiecewiseLinear::try_new(&[
///     LinearStop::new(0.0),
///     LinearStop::new(0.25).at(0.25),
///     LinearStop::new(0.25).at(0.75),
///     LinearStop::new(1.0),
/// ])?;
/// assert_eq!(curve.transform(0.5), 0.25);
/// # Ok::<(), flui_animation::PiecewiseLinearError>(())
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "serde",
    serde(try_from = "Vec<LinearStop>", into = "Vec<LinearStop>")
)]
pub struct PiecewiseLinear {
    knots: Arc<[Knot]>,
}

impl PiecewiseLinear {
    /// Canonicalize and validate the stops once, before publishing a curve.
    ///
    /// # Errors
    ///
    /// Returns [`PiecewiseLinearError`] for fewer than two or more than
    /// `u32::MAX` stops, non-finite outputs, explicit inputs outside `[0, 1]`,
    /// or canonical endpoints other than `(0, 0)` and `(1, 1)`.
    pub fn try_new(stops: &[LinearStop]) -> Result<Self, PiecewiseLinearError> {
        if stops.len() < 2 {
            return Err(PiecewiseLinearError::TooFewStops);
        }
        let count = u32::try_from(stops.len()).map_err(|_| PiecewiseLinearError::TooManyStops)?;
        let mut stops = stops.to_vec();
        for (index, stop) in stops.iter().enumerate() {
            if !stop.output.is_finite() {
                return Err(PiecewiseLinearError::NonFiniteOutput { index });
            }
            if let Some(input) = stop.input {
                if !input.is_finite() {
                    return Err(PiecewiseLinearError::NonFiniteInput { index });
                }
                if !(0.0..=1.0).contains(&input) {
                    return Err(PiecewiseLinearError::InputOutOfRange {
                        index,
                        value: input,
                    });
                }
            }
        }
        stops[0].input.get_or_insert(0.0);
        stops
            .last_mut()
            .expect("BUG: two stops were admitted")
            .input
            .get_or_insert(1.0);
        let mut maximum: f64 = 0.0;
        for stop in &mut stops {
            if let Some(input) = &mut stop.input {
                *input = input.max(maximum);
                maximum = *input;
            }
        }
        let mut left = 0_u32;
        while left < count - 1 {
            let right = (left + 1..count)
                .find(|&index| stops[index as usize].input.is_some())
                .expect("BUG: the last stop has an input");
            let begin = stops[left as usize]
                .input
                .expect("BUG: the preceding input is known");
            let end = stops[right as usize]
                .input
                .expect("BUG: the following input is known");
            for index in left + 1..right {
                let fraction = f64::from(index - left) / f64::from(right - left);
                stops[index as usize].input = Some(begin + (end - begin) * fraction);
            }
            left = right;
        }
        let first = stops[0];
        let last = *stops.last().expect("BUG: two stops were admitted");
        if first.input != Some(0.0)
            || first.output != 0.0
            || last.input != Some(1.0)
            || last.output != 1.0
        {
            return Err(PiecewiseLinearError::InvalidEndpoints);
        }
        Ok(Self {
            knots: stops
                .into_iter()
                .map(|stop| Knot {
                    input: stop.input.expect("BUG: every input was canonicalized"),
                    output: stop.output,
                })
                .collect(),
        })
    }
}

impl Curve for PiecewiseLinear {
    fn transform(&self, t: f64) -> f64 {
        if let Some(endpoint) = super::settled(t) {
            return endpoint;
        }
        let right = self.knots.partition_point(|knot| knot.input <= t);
        let from = self.knots[right - 1];
        if from.input == t {
            return from.output;
        }
        let to = self.knots[right];
        let fraction = (t - from.input) / (to.input - from.input);
        // Stable Lerp preserves opposite extreme endpoints whose difference
        // overflows. Rounding a convex combination at MAX may saturate.
        from.output
            .lerp_to(&to.output, fraction)
            .clamp(-f64::MAX, f64::MAX)
    }

    /// The outgoing segment's derivative at an interior knot, the incoming
    /// derivative at 1, and zero at a repeated-input jump. A mathematically
    /// unrepresentable finite-segment rate saturates to signed `f64::MAX`.
    fn slope(&self, t: f64) -> f64 {
        if let Some(slope) = super::settled_slope(t) {
            return slope;
        }
        let right = self.knots.partition_point(|knot| knot.input <= t);
        let left = self.knots.partition_point(|knot| knot.input < t);
        if right - left > 1 {
            return 0.0;
        }
        let next = right.min(self.knots.len() - 1);
        let from = self.knots[next - 1];
        let to = self.knots[next];
        ((to.output - from.output) / (to.input - from.input)).clamp(-f64::MAX, f64::MAX)
    }
    fn builtin(&self) -> Option<BuiltinCurve> {
        Some(BuiltinCurve::new(Builtin::PiecewiseLinear(self.clone())))
    }
}

#[cfg(feature = "serde")]
impl TryFrom<Vec<LinearStop>> for PiecewiseLinear {
    type Error = PiecewiseLinearError;
    fn try_from(stops: Vec<LinearStop>) -> Result<Self, Self::Error> {
        Self::try_new(&stops)
    }
}

#[cfg(feature = "serde")]
impl From<PiecewiseLinear> for Vec<LinearStop> {
    fn from(curve: PiecewiseLinear) -> Self {
        curve
            .knots
            .iter()
            .map(|knot| LinearStop::new(knot.output).at(knot.input))
            .collect()
    }
}
