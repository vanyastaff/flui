//! [`Angle`] — a plane angle in radians that keeps whole turns.
//!
//! An angle is not reduced modulo a turn: `Angle::from_turns(3.25)` is three and a quarter
//! revolutions, and interpolating toward it spins three times. Interpolation is numeric (the
//! [`Lerp`] impl), so a multi-turn rotation is expressible. Taking the shorter arc instead is
//! the consumer's choice: it calls [`Angle::nearest_equivalent`] on its target before
//! animating.

use std::f64::consts::{PI, TAU};

use crate::geometry::{Lerp, QuarterTurns};

/// A plane angle, stored in radians as an `f64`.
///
/// Values are kept as given: no reduction to one turn, so `720°` and `0°` are different
/// angles (they rotate the same way, but animating between them spins twice). NaN and
/// infinities are carried through every operation unchanged, like the other `f64` geometry
/// values; a consumer that needs a finite angle checks [`f64::is_finite`] on
/// [`radians`](Self::radians).
///
/// # Examples
///
/// ```
/// use flui_foundation::geometry::Angle;
///
/// let quarter = Angle::from_degrees(90.0);
/// assert!((quarter.turns() - 0.25).abs() < 1e-15);
/// assert_eq!(Angle::from_turns(0.5).radians(), std::f64::consts::PI);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Angle {
    radians: f64,
}

impl Angle {
    /// No rotation.
    pub const ZERO: Self = Self { radians: 0.0 };

    /// An angle of `radians`.
    #[must_use]
    pub const fn from_radians(radians: f64) -> Self {
        Self { radians }
    }

    /// An angle of `degrees` (360° is one turn).
    #[must_use]
    pub fn from_degrees(degrees: f64) -> Self {
        Self::from_radians(degrees.to_radians())
    }

    /// An angle of `turns` full revolutions (`1.0` is 360°, `0.25` is 90°).
    #[must_use]
    pub fn from_turns(turns: f64) -> Self {
        Self::from_radians(turns * TAU)
    }

    /// The angle in radians.
    #[must_use]
    pub const fn radians(self) -> f64 {
        self.radians
    }

    /// The angle in degrees.
    #[must_use]
    pub fn degrees(self) -> f64 {
        self.radians.to_degrees()
    }

    /// The angle in full revolutions.
    #[must_use]
    pub fn turns(self) -> f64 {
        self.radians / TAU
    }

    /// The angle that points the same way as `self` and lies within half a turn of
    /// `reference`: `reference + d`, where `d` is `self - reference` reduced into
    /// `(-½ turn, ½ turn]`.
    ///
    /// Animating from `reference` to the result takes the shorter arc. An exact half turn
    /// resolves toward the increasing angle (`+½` turn, never `-½`). A NaN or infinite input
    /// yields NaN.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Angle;
    ///
    /// let target = Angle::from_degrees(270.0);
    /// let shorter = target.nearest_equivalent(Angle::ZERO);
    /// assert!((shorter.degrees() - -90.0).abs() < 1e-12);
    /// ```
    #[must_use]
    pub fn nearest_equivalent(self, reference: Angle) -> Angle {
        // `rem_euclid` lands in [0, TAU); folding (PI, TAU) down gives (-PI, PI].
        let delta = (self.radians - reference.radians).rem_euclid(TAU);
        let delta = if delta > PI { delta - TAU } else { delta };
        Self::from_radians(reference.radians + delta)
    }
}

impl From<QuarterTurns> for Angle {
    /// The quarter-turn rotation as an angle: `QuarterTurns::One` is 90°, exactly
    /// `TAU / 4` radians.
    fn from(turns: QuarterTurns) -> Self {
        Self::from_turns(f64::from(turns.as_int()) / 4.0)
    }
}

/// Numeric interpolation: `t` scales the signed difference, so a multi-turn span spins
/// every turn, and `t` outside `[0, 1]` extrapolates (the [`Lerp`] contract).
impl Lerp for Angle {
    #[inline]
    fn lerp_to(&self, other: &Self, t: f64) -> Self {
        Self::from_radians(self.radians.lerp_to(&other.radians, t))
    }
}
