//! `Steps`: discrete jumps, with CSS jump positions.

use super::{Curve, CurveError, CurveParameter, settled, settled_slope};
/// Where a [`Steps`] curve places its jumps (CSS Easing 1 §2.3.1
/// `<step-position>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum JumpAt {
    /// The first jump happens as the curve starts (`jump-start`).
    Start,
    /// The last jump happens as the curve ends (`jump-end`, the CSS default).
    #[default]
    End,
    /// No jump at either end (`jump-none`): the first and last steps each
    /// hold for one interval.
    None,
    /// Jumps at both ends (`jump-both`).
    Both,
}

/// A staircase curve: `count` equal intervals, each holding one value
/// (CSS Easing 1 §2.3.1 `steps()`).
///
/// For `t` strictly inside `(0, 1)` the output is `step / jumps`, where
/// `step = ⌊t · count⌋` (plus one for [`JumpAt::Start`] and
/// [`JumpAt::Both`]) capped at `jumps`, and `jumps` is `count` for
/// [`JumpAt::Start`]/[`JumpAt::End`], `count − 1` for [`JumpAt::None`] and
/// `count + 1` for [`JumpAt::Both`]. Each interval is closed on the left:
/// the jump belongs to the later step.
///
/// The ends follow the [`Curve`] contract — `0 → 0`, `1 → 1` — which is the
/// CSS value with the before flag set at 0. FLUI has no before/after phases,
/// so the before flag is not modelled. Monotone (non-decreasing).
///
/// # Serde
///
/// Serializes as `{ "count": n, "jump": "End" }`; decoding rejects what
/// [`Steps::try_new`] rejects.
///
/// # Examples
///
/// ```
/// use flui_animation::{Curve, JumpAt, Steps};
///
/// const FOUR: Steps = Steps::new(4, JumpAt::End);
/// assert_eq!(FOUR.transform(0.24), 0.0);
/// assert_eq!(FOUR.transform(0.25), 0.25);
/// assert_eq!(Steps::new(4, JumpAt::Start).transform(0.1), 0.25);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(try_from = "StepsWire", into = "StepsWire"))]
pub struct Steps {
    count: u32,
    jump: JumpAt,
}

impl Steps {
    /// Admits `count ≥ 1`, and `count ≥ 2` for [`JumpAt::None`].
    const fn validate(count: u32, jump: JumpAt) -> Result<Self, CurveError> {
        if count == 0 {
            return Err(CurveError::OutOfRange {
                parameter: CurveParameter::StepCount,
                value: 0.0,
            });
        }
        if count < 2 && matches!(jump, JumpAt::None) {
            return Err(CurveError::TooFewSteps);
        }
        Ok(Self { count, jump })
    }

    /// Creates a `steps(count, jump)` curve.
    ///
    /// # Panics
    ///
    /// Panics when [`Steps::try_new`] would return an error. In a `const`
    /// item the panic is a compile error.
    #[must_use]
    pub const fn new(count: u32, jump: JumpAt) -> Self {
        match Self::validate(count, jump) {
            Ok(steps) => steps,
            Err(_) => panic!(
                "Steps::new: count must be at least 1, and at least 2 for JumpAt::None \
                 (Steps::try_new reports which)"
            ),
        }
    }

    /// Creates a `steps(count, jump)` curve, rejecting an invalid count.
    ///
    /// # Errors
    ///
    /// - [`CurveError::OutOfRange`] when `count` is 0;
    /// - [`CurveError::TooFewSteps`] when `jump` is [`JumpAt::None`] and
    ///   `count` is 1.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::{CurveError, JumpAt, Steps};
    ///
    /// assert!(Steps::try_new(3, JumpAt::Both).is_ok());
    /// assert_eq!(Steps::try_new(1, JumpAt::None), Err(CurveError::TooFewSteps));
    /// ```
    pub fn try_new(count: u32, jump: JumpAt) -> Result<Self, CurveError> {
        Self::validate(count, jump)
    }
}

/// The serialized form of [`Steps`].
#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename = "Steps")]
struct StepsWire {
    count: u32,
    jump: JumpAt,
}

#[cfg(feature = "serde")]
impl From<Steps> for StepsWire {
    fn from(steps: Steps) -> Self {
        Self {
            count: steps.count,
            jump: steps.jump,
        }
    }
}

#[cfg(feature = "serde")]
impl TryFrom<StepsWire> for Steps {
    type Error = CurveError;

    fn try_from(wire: StepsWire) -> Result<Self, CurveError> {
        Self::try_new(wire.count, wire.jump)
    }
}

impl Curve for Steps {
    builtin_value!(Steps(self));

    /// 0 wherever it is defined: the curve is flat between jumps, and a jump
    /// has no finite derivative, so a neighbouring keyframe segment inherits
    /// no velocity from it. NaN gives NaN, as for every curve.
    fn slope(&self, t: f64) -> f64 {
        settled_slope(t).unwrap_or(0.0)
    }

    fn transform(&self, t: f64) -> f64 {
        if let Some(end) = settled(t) {
            return end;
        }
        let count = f64::from(self.count);
        let jumps = match self.jump {
            JumpAt::Start | JumpAt::End => count,
            JumpAt::None => count - 1.0,
            JumpAt::Both => count + 1.0,
        };
        let mut step = (t * count).floor();
        if matches!(self.jump, JumpAt::Start | JumpAt::Both) {
            step += 1.0;
        }
        step.min(jumps) / jumps
    }
}
