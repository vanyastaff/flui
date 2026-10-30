//! Common easing presets, usable directly or wrapped in [`crate::SharedCurve`].
//!
//! [`EASE`], [`EASE_IN`], [`EASE_OUT`] and [`EASE_IN_OUT`] use the CSS control
//! points. Other Bézier presets approximate their named motion styles; a
//! polynomial or trigonometric family name does not promise that exact formula.
//! Back, elastic and bounce curves are not monotone. Back and elastic curves
//! overshoot `[0, 1]`; bounce curves stay within it but reverse direction.

use crate::curve::{Bounce, Cubic, Decelerate, Ease, Elastic, Linear, ThreePointCubic};

/// Linear progress, without easing.
#[doc(alias = "Linear")]
pub const LINEAR: Linear = Linear;

/// The CSS ease-in curve: a slow start.
#[doc(alias = "EaseIn")]
pub const EASE_IN: Cubic = Cubic::new(0.42, 0.0, 1.0, 1.0);

/// The CSS ease-out curve: a slow finish.
#[doc(alias = "EaseOut")]
pub const EASE_OUT: Cubic = Cubic::new(0.0, 0.0, 0.58, 1.0);

/// The CSS ease-in-out curve: a slow start and finish.
#[doc(alias = "EaseInOut")]
pub const EASE_IN_OUT: Cubic = Cubic::new(0.42, 0.0, 0.58, 1.0);

/// Accelerates into the middle, then decelerates to rest.
#[doc(alias = "FastOutSlowIn")]
pub const FAST_OUT_SLOW_IN: Cubic = Cubic::new(0.4, 0.0, 0.2, 1.0);

/// Fast initial progress, decelerating to rest.
#[doc(alias = "SlowOutFastIn")]
pub const LINEAR_OUT_SLOW_IN: Cubic = Cubic::new(0.0, 0.0, 0.2, 1.0);

/// A cubic-style ease-in-out approximation.
#[doc(alias = "EaseInOutCubic")]
pub const EASE_IN_OUT_CUBIC: Cubic = Cubic::new(0.645, 0.045, 0.355, 1.0);

/// A sine-style ease-in approximation.
#[doc(alias = "EaseInSine")]
pub const EASE_IN_SINE: Cubic = Cubic::new(0.47, 0.0, 0.745, 0.715);

/// A sine-style ease-out approximation.
#[doc(alias = "EaseOutSine")]
pub const EASE_OUT_SINE: Cubic = Cubic::new(0.39, 0.575, 0.565, 1.0);

/// A sine-style ease-in-out approximation.
#[doc(alias = "EaseInOutSine")]
pub const EASE_IN_OUT_SINE: Cubic = Cubic::new(0.445, 0.05, 0.55, 0.95);

/// An exponential-style ease-in approximation.
#[doc(alias = "EaseInExpo")]
pub const EASE_IN_EXPO: Cubic = Cubic::new(0.95, 0.05, 0.795, 0.035);

/// An exponential-style ease-out approximation.
#[doc(alias = "EaseOutExpo")]
pub const EASE_OUT_EXPO: Cubic = Cubic::new(0.19, 1.0, 0.22, 1.0);

/// An exponential-style ease-in-out approximation.
#[doc(alias = "EaseInOutExpo")]
pub const EASE_IN_OUT_EXPO: Cubic = Cubic::new(1.0, 0.0, 0.0, 1.0);

/// A circular-style ease-in approximation.
#[doc(alias = "EaseInCirc")]
pub const EASE_IN_CIRC: Cubic = Cubic::new(0.6, 0.04, 0.98, 0.335);

/// A circular-style ease-out approximation.
#[doc(alias = "EaseOutCirc")]
pub const EASE_OUT_CIRC: Cubic = Cubic::new(0.075, 0.82, 0.165, 1.0);

/// A circular-style ease-in-out approximation.
#[doc(alias = "EaseInOutCirc")]
pub const EASE_IN_OUT_CIRC: Cubic = Cubic::new(0.785, 0.135, 0.15, 0.86);

/// Dips below zero before accelerating into the target. Not monotone.
#[doc(alias = "EaseInBack")]
pub const EASE_IN_BACK: Cubic = Cubic::new(0.6, -0.28, 0.735, 0.045);

/// Overshoots the target, then settles back. Not monotone.
#[doc(alias = "EaseOutBack")]
pub const EASE_OUT_BACK: Cubic = Cubic::new(0.175, 0.885, 0.32, 1.275);

/// Anticipates below zero and overshoots the target. Not monotone.
#[doc(alias = "EaseInOutBack")]
pub const EASE_IN_OUT_BACK: Cubic = Cubic::new(0.68, -0.55, 0.265, 1.55);

/// An elastic ease-in with a period of 0.4.
#[doc(alias = "ElasticIn")]
pub const ELASTIC_IN: Elastic = Elastic::new(Ease::In, 0.4);

/// An elastic ease-out with a period of 0.4.
#[doc(alias = "ElasticOut")]
pub const ELASTIC_OUT: Elastic = Elastic::new(Ease::Out, 0.4);

/// An elastic ease-in-out with a period of 0.4.
#[doc(alias = "ElasticInOut")]
pub const ELASTIC_IN_OUT: Elastic = Elastic::new(Ease::InOut, 0.4);

/// Bounces at the beginning while staying within `[0, 1]`.
#[doc(alias = "BounceIn")]
pub const BOUNCE_IN: Bounce = Bounce(Ease::In);

/// Bounces at the end while staying within `[0, 1]`.
#[doc(alias = "BounceOut")]
pub const BOUNCE_OUT: Bounce = Bounce(Ease::Out);

/// Bounces at both ends while staying within `[0, 1]`.
#[doc(alias = "BounceInOut")]
pub const BOUNCE_IN_OUT: Bounce = Bounce(Ease::InOut);

/// Quadratic deceleration from a fast start to rest.
#[doc(alias = "Decelerate")]
pub const DECELERATE: Decelerate = Decelerate;

/// The CSS ease curve: speeds up quickly, then ends slowly.
#[doc(alias = "Ease")]
pub const EASE: Cubic = Cubic::new(0.25, 0.1, 0.25, 1.0);

/// A quadratic-style ease-in approximation.
#[doc(alias = "EaseInQuad")]
pub const EASE_IN_QUAD: Cubic = Cubic::new(0.55, 0.085, 0.68, 0.53);

/// A cubic-style ease-in approximation.
#[doc(alias = "EaseInCubic")]
pub const EASE_IN_CUBIC: Cubic = Cubic::new(0.55, 0.055, 0.675, 0.19);

/// A quartic-style ease-in approximation.
#[doc(alias = "EaseInQuart")]
pub const EASE_IN_QUART: Cubic = Cubic::new(0.895, 0.03, 0.685, 0.22);

/// A quintic-style ease-in approximation.
#[doc(alias = "EaseInQuint")]
pub const EASE_IN_QUINT: Cubic = Cubic::new(0.755, 0.05, 0.855, 0.06);

/// A quadratic-style ease-out approximation.
#[doc(alias = "EaseOutQuad")]
pub const EASE_OUT_QUAD: Cubic = Cubic::new(0.25, 0.46, 0.45, 0.94);

/// A cubic-style ease-out approximation.
#[doc(alias = "EaseOutCubic")]
pub const EASE_OUT_CUBIC: Cubic = Cubic::new(0.215, 0.61, 0.355, 1.0);

/// A quartic-style ease-out approximation.
#[doc(alias = "EaseOutQuart")]
pub const EASE_OUT_QUART: Cubic = Cubic::new(0.165, 0.84, 0.44, 1.0);

/// A quintic-style ease-out approximation.
#[doc(alias = "EaseOutQuint")]
pub const EASE_OUT_QUINT: Cubic = Cubic::new(0.23, 1.0, 0.32, 1.0);

/// A quadratic-style ease-in-out approximation.
#[doc(alias = "EaseInOutQuad")]
pub const EASE_IN_OUT_QUAD: Cubic = Cubic::new(0.455, 0.03, 0.515, 0.955);

/// A quartic-style ease-in-out approximation.
#[doc(alias = "EaseInOutQuart")]
pub const EASE_IN_OUT_QUART: Cubic = Cubic::new(0.77, 0.0, 0.175, 1.0);

/// A quintic-style ease-in-out approximation.
#[doc(alias = "EaseInOutQuint")]
pub const EASE_IN_OUT_QUINT: Cubic = Cubic::new(0.86, 0.0, 0.07, 1.0);

/// Fast initial progress followed by a long deceleration into rest.
#[doc(alias = "FastLinearToSlowEaseIn")]
pub const FAST_LINEAR_TO_SLOW_EASE_IN: Cubic = Cubic::new(0.18, 1.0, 0.04, 1.0);

/// A fast start and gentle finish, used for an outgoing transition.
#[doc(alias = "LinearToEaseOut")]
pub const LINEAR_TO_EASE_OUT: Cubic = Cubic::new(0.35, 0.91, 0.33, 0.97);

/// A slow start followed by an approximately linear finish.
#[doc(alias = "EaseInToLinear")]
pub const EASE_IN_TO_LINEAR: Cubic = Cubic::new(0.67, 0.03, 0.65, 0.09);

/// Fast at the edges and slow through the middle.
#[doc(alias = "SlowMiddle")]
pub const SLOW_MIDDLE: Cubic = Cubic::new(0.15, 0.85, 0.85, 0.15);

/// A three-point emphasized ease-in-out.
#[doc(alias = "EaseInOutCubicEmphasized")]
pub const EASE_IN_OUT_CUBIC_EMPHASIZED: ThreePointCubic = ThreePointCubic::new(
    (0.05, 0.0),
    (0.133_333, 0.06),
    (0.166_666, 0.4),
    (0.208_333, 0.82),
    (0.25, 1.0),
);

/// A strong ease-in followed by a long, gentle ease-out.
#[doc(alias = "FastEaseInToSlowEaseOut")]
pub const FAST_EASE_IN_TO_SLOW_EASE_OUT: ThreePointCubic = ThreePointCubic::new(
    (0.056, 0.024),
    (0.108, 0.308_5),
    (0.198, 0.541),
    (0.365_5, 1.0),
    (0.546_5, 0.989),
);
