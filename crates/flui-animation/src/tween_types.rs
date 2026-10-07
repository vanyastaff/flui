//! Tween types for animating values.
//!
//! This module provides types for animating values between a beginning and ending state.
//! The core abstraction is the [`Animatable`] trait, which maps a progress value (0.0 to 1.0)
//! to an output value of any type. A value that moves through several states
//! over time is a [`Keyframes`](crate::Keyframes) track.
//!
//! # Examples
//!
//! ```
//! use flui_animation::{FloatTween, Animatable, ReverseTween};
//!
//! let tween = FloatTween::new(0.0, 100.0);
//! assert_eq!(tween.transform(0.5), 50.0);
//!
//! let reversed = ReverseTween::new(tween);
//! assert_eq!(reversed.transform(0.0), 100.0);
//! ```

use crate::curve::Curve;
use flui_foundation::geometry::{Edges, Lerp, Matrix4, Offset, Rect, Size};
use flui_painting::Alignment;
use flui_painting::styling::{BorderRadius, Color};

/// A value that can be animated.
pub trait Animatable<T> {
    /// Returns the value of this object at the given animation value.
    fn transform(&self, t: f64) -> T;
}

/// A tween that linearly interpolates between a `begin` and `end` value of any
/// [`Lerp`] type. One generic struct covers every per-type tween
/// (`ColorTween`, `SizeTween`, ...), which are aliases of it.
///
/// `transform` does **not** clamp `t`: bouncy/elastic/spring curves emit
/// `t > 1` (or `t < 0`) and the overshoot must reach the value. The exact
/// endpoints (`t == 0`, `t == 1`) are returned verbatim without interpolation.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Tween<V> {
    /// The value at the start of the animation.
    pub begin: V,
    /// The value at the end of the animation.
    pub end: V,
}

impl<V> Tween<V> {
    /// Creates a new tween between `begin` and `end`.
    #[must_use]
    pub const fn new(begin: V, end: V) -> Self {
        Self { begin, end }
    }
}

impl<V: Lerp> Animatable<V> for Tween<V> {
    fn transform(&self, t: f64) -> V {
        if t == 0.0 {
            return self.begin.clone();
        }
        if t == 1.0 {
            return self.end.clone();
        }
        self.begin.lerp_to(&self.end, t)
    }
}

// ============================================================================
// Concrete Tweens
// ============================================================================

/// A tween that linearly interpolates between two floats.
///
/// # Examples
///
/// ```
/// use flui_animation::{FloatTween, Animatable};
///
/// let tween = FloatTween::new(0.0, 100.0);
/// assert_eq!(tween.transform(0.0), 0.0);
/// assert_eq!(tween.transform(0.5), 50.0);
/// assert_eq!(tween.transform(1.0), 100.0);
/// ```
/// Tween between two floats. Alias for `Tween<f64>`.
pub type FloatTween = Tween<f64>;

/// A tween that linearly interpolates between two integers, rounding to the
/// nearest integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct IntTween {
    /// The beginning value.
    pub begin: i32,
    /// The ending value.
    pub end: i32,
}

impl IntTween {
    /// Creates a new integer tween.
    #[must_use]
    pub const fn new(begin: i32, end: i32) -> Self {
        Self { begin, end }
    }
}

impl Animatable<i32> for IntTween {
    #[expect(clippy::cast_possible_truncation)] // rounded f64->i32, saturating cast
    fn transform(&self, t: f64) -> i32 {
        let t = t.clamp(0.0, 1.0);
        (f64::from(self.begin) + (f64::from(self.end) - f64::from(self.begin)) * t).round() as i32
    }
}

/// A tween that linearly interpolates between two integers, flooring to the
/// nearest integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StepTween {
    /// The beginning value.
    pub begin: i32,
    /// The ending value.
    pub end: i32,
}

impl StepTween {
    /// Creates a new step tween.
    #[must_use]
    pub const fn new(begin: i32, end: i32) -> Self {
        Self { begin, end }
    }
}

impl Animatable<i32> for StepTween {
    #[expect(clippy::cast_possible_truncation)] // floored f64->i32, saturating cast
    fn transform(&self, t: f64) -> i32 {
        let t = t.clamp(0.0, 1.0);
        (f64::from(self.begin) + (f64::from(self.end) - f64::from(self.begin)) * t).floor() as i32
    }
}

/// A tween that always returns the same value.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ConstantTween<T: Clone> {
    /// The constant value.
    pub value: T,
}

impl<T: Clone> ConstantTween<T> {
    /// Creates a new constant tween.
    #[must_use]
    pub const fn new(value: T) -> Self {
        Self { value }
    }
}

impl<T: Clone> Animatable<T> for ConstantTween<T> {
    fn transform(&self, _t: f64) -> T {
        self.value.clone()
    }
}

/// A tween that reverses another tween.
///
/// The reversed tween starts at the end value and goes to the begin value.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ReverseTween<T, A: Animatable<T>> {
    /// The tween to reverse.
    pub tween: A,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, A: Animatable<T>> ReverseTween<T, A> {
    /// Creates a new reversed tween.
    #[must_use]
    pub fn new(tween: A) -> Self {
        Self {
            tween,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T, A: Animatable<T>> Animatable<T> for ReverseTween<T, A> {
    fn transform(&self, t: f64) -> T {
        self.tween.transform(1.0 - t)
    }
}

// ============================================================================
// Geometric Tweens
// ============================================================================

/// A tween that linearly interpolates between two colors.
/// Tween between two colors. Alias for `Tween<Color>`.
pub type ColorTween = Tween<Color>;

/// A color tween that interpolates through Oklab space (perceptually
/// uniform) instead of componentwise sRGB.
///
/// `Color::lerp` averages gamma-encoded channels,
/// so cross-hue transitions pass through dark, gray midpoints (blue→yellow
/// goes through mud). Interpolating in Oklab keeps perceived lightness and
/// chroma steady across the whole transition.
///
/// Costs two color-space conversions per `transform` (`powf`/`cbrt` per
/// channel); prefer [`ColorTween`] for near-identical endpoints or very hot
/// paths. `t` is clamped to `[0, 1]` — extrapolating outside the segment in
/// Oklab leaves the sRGB gamut almost immediately.
///
/// # Examples
///
/// ```
/// use flui_animation::{Animatable, OklabColorTween};
/// use flui_painting::styling::Color;
///
/// let tween = OklabColorTween::new(Color::rgb(0, 0, 255), Color::rgb(255, 255, 0));
/// let perceptual_mid = tween.transform(0.5);
/// let srgb_mid = Color::lerp(Color::rgb(0, 0, 255), Color::rgb(255, 255, 0), 0.5);
/// // The perceptual path differs from the muddy componentwise-sRGB midpoint.
/// assert_ne!(perceptual_mid, srgb_mid);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OklabColorTween {
    /// The color at the start of the animation.
    pub begin: Color,
    /// The color at the end of the animation.
    pub end: Color,
}

impl OklabColorTween {
    /// Creates a new perceptual color tween between `begin` and `end`.
    #[must_use]
    pub const fn new(begin: Color, end: Color) -> Self {
        Self { begin, end }
    }
}

impl Animatable<Color> for OklabColorTween {
    fn transform(&self, t: f64) -> Color {
        if t == 0.0 {
            return self.begin;
        }
        if t == 1.0 {
            return self.end;
        }
        Color::lerp_oklab(self.begin, self.end, t)
    }
}

/// A tween that linearly interpolates between two sizes.
/// Tween between two sizes. Alias for `Tween<Size>`.
pub type SizeTween = Tween<Size<f64>>;

/// A tween that linearly interpolates between two rectangles.
/// Tween between two rectangles. Alias for `Tween<Rect>`.
pub type RectTween = Tween<Rect<f64>>;

/// A tween that linearly interpolates between two offsets.
/// Tween between two offsets. Alias for `Tween<Offset>`.
pub type OffsetTween = Tween<Offset<f64>>;

/// A tween that linearly interpolates between two alignments.
/// Tween between two alignments. Alias for `Tween<Alignment>`.
pub type AlignmentTween = Tween<Alignment>;

/// A tween that linearly interpolates between two edge insets.
/// Tween between two edge insets. Alias for `Tween<EdgeInsets>`.
pub type EdgeInsetsTween = Tween<Edges<f64>>;

/// Tween between two border radii. Alias for `Tween<BorderRadius>` (now that
/// `Lerp for Corners<T>` lives in `flui_foundation::geometry`).
pub type BorderRadiusTween = Tween<BorderRadius>;

/// Tween between two affine transforms. Alias for `Tween<Matrix4>`; interpolates
/// by decompose -> slerp (see `Matrix4::lerp`), so rotation animates correctly.
pub type Matrix4Tween = Tween<Matrix4>;

// ============================================================================
// Curve-based Tweens
// ============================================================================

/// A tween that applies a curve to the animation progress.
///
/// Unlike [`CurvedAnimation`] in `flui_animation` which wraps an `Animation`,
/// `CurveTween` implements [`Animatable`] and can be chained with other tweens.
///
/// # Examples
///
/// ```
/// use flui_animation::{CurveTween, Animatable, Curves};
///
/// // Apply ease-in curve to progress
/// let curve_tween = CurveTween::new(Curves::EaseIn);
/// assert!(curve_tween.transform(0.0).abs() < 0.001); // ~0
/// assert!(curve_tween.transform(0.5) < 0.5); // ease-in is slower at start
/// assert!((curve_tween.transform(1.0) - 1.0).abs() < 0.001); // ~1
/// ```
///
/// [`CurvedAnimation`]: crate::curved::CurvedAnimation
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveTween<C: Curve> {
    /// The curve to apply.
    pub curve: C,
}

impl<C: Curve> CurveTween<C> {
    /// Creates a new curve tween.
    #[inline]
    #[must_use]
    pub const fn new(curve: C) -> Self {
        Self { curve }
    }
}

impl<C: Curve> Animatable<f64> for CurveTween<C> {
    #[inline]
    fn transform(&self, t: f64) -> f64 {
        self.curve.transform(t.clamp(0.0, 1.0))
    }
}

// ============================================================================
// Chained Tweens
// ============================================================================

/// A tween that chains two animatables together.
///
/// The first animatable transforms the input `t`, and its output is passed
/// to the second animatable.
///
/// This is useful for applying a curve before a value tween:
///
/// # Examples
///
/// ```
/// use flui_animation::{ChainedTween, CurveTween, FloatTween, Animatable, Curves};
///
/// // Chain a curve with a float tween
/// let chained = ChainedTween::new(
///     CurveTween::new(Curves::EaseIn),
///     FloatTween::new(0.0, 100.0),
/// );
///
/// assert!(chained.transform(0.0).abs() < 0.1); // ~0
/// assert!(chained.transform(0.5) < 50.0); // ease-in effect
/// assert!((chained.transform(1.0) - 100.0).abs() < 0.1); // ~100
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChainedTween<A, B> {
    /// The first animatable (transforms t).
    pub first: A,
    /// The second animatable (transforms the output of first).
    pub second: B,
}

impl<A, B> ChainedTween<A, B> {
    /// Creates a new chained tween.
    #[inline]
    #[must_use]
    pub const fn new(first: A, second: B) -> Self {
        Self { first, second }
    }
}

impl<T, A, B> Animatable<T> for ChainedTween<A, B>
where
    A: Animatable<f64>,
    B: Animatable<T>,
{
    #[inline]
    fn transform(&self, t: f64) -> T {
        let curved_t = self.first.transform(t);
        self.second.transform(curved_t)
    }
}
