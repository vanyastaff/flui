//! Canonical stops for a normalized piecewise-linear easing.

use flui_foundation::geometry::Lerp;
use std::rc::Rc;

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
    knots: Rc<[Knot]>,
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
        if u32::try_from(stops.len()).is_err() {
            return Err(PiecewiseLinearError::TooManyStops);
        }
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
        // CSS `linear()` canonicalization: a missing first input is 0 and a
        // missing last one is 1, an explicit input is raised to the largest
        // before it, and a run of missing inputs is spread evenly between its
        // neighbours. The first input is always known, so every run has a
        // left neighbour, `known`.
        let last_index = stops.len() - 1;
        let mut knots: Vec<Knot> = Vec::with_capacity(stops.len());
        let mut known = 0.0_f64;
        let mut missing = 0_u32;
        for (index, stop) in stops.iter().enumerate() {
            let input = match stop.input {
                Some(input) => input,
                None if index == 0 => 0.0,
                None if index == last_index => 1.0,
                None => {
                    missing += 1;
                    continue;
                }
            };
            let input = input.max(known);
            // `missing < stops.len() ≤ u32::MAX`, so `missing + 1` does not overflow.
            for step in 1..=missing {
                let fraction = f64::from(step) / f64::from(missing + 1);
                let output = stops[knots.len()].output;
                knots.push(Knot {
                    input: known + (input - known) * fraction,
                    output,
                });
            }
            knots.push(Knot {
                input,
                output: stop.output,
            });
            known = input;
            missing = 0;
        }
        // The last stop is always known, so every stop has its knot.
        let (first, last) = (knots[0], knots[last_index]);
        if first.input != 0.0 || first.output != 0.0 || last.input != 1.0 || last.output != 1.0 {
            return Err(PiecewiseLinearError::InvalidEndpoints);
        }
        Ok(Self {
            knots: Rc::<[Knot]>::from(knots),
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
