//! Tween types for animating values.
//!
//! This module provides types for animating values between a beginning and ending state.
//! The core abstraction is the [`Animatable`] trait, which maps a progress value (0.0 to 1.0)
//! to an output value of any type.
//!
//! # Extension Traits
//!
//! This module provides [`CurveExt`] for converting curves into animatables.
//! The fluent composition methods on animatables themselves (`.reversed()`,
//! `.chain()`, `.with_curve()`, `.animate()`) live on
//! [`AnimatableExt`](crate::ext::AnimatableExt).
//!
//! # Examples
//!
//! ```
//! use flui_animation::{FloatTween, Animatable, AnimatableExt};
//!
//! let tween = FloatTween::new(0.0, 100.0);
//!
//! // Use the tween directly
//! assert_eq!(tween.transform(0.5), 50.0);
//!
//! // Or reverse it using the extension trait
//! let reversed = tween.reversed();
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
/// nearest integer (half away from zero). `t` is clamped to `[0, 1]`; a NaN `t`
/// returns `begin`.
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
    #[expect(
        clippy::cast_possible_truncation,
        reason = "rounded f64 to i32; the cast saturates"
    )]
    fn transform(&self, t: f64) -> i32 {
        if t.is_nan() {
            return self.begin;
        }
        let t = t.clamp(0.0, 1.0);
        (f64::from(self.begin) + (f64::from(self.end) - f64::from(self.begin)) * t).round() as i32
    }
}

/// A tween that linearly interpolates between two integers, flooring to the
/// integer below (toward negative infinity). `t` is clamped to `[0, 1]`; a NaN `t`
/// returns `begin`.
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
    #[expect(
        clippy::cast_possible_truncation,
        reason = "floored f64 to i32; the cast saturates"
    )]
    fn transform(&self, t: f64) -> i32 {
        if t.is_nan() {
            return self.begin;
        }
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

/// Tween between two colors. Alias for `Tween<Color>`: interpolates in Oklab with
/// premultiplied alpha (`Color`'s `Lerp`, ADR-0149).
pub type ColorTween = Tween<Color>;

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
// Complex Tweens
// ============================================================================

/// A tween that chains together multiple tweens in sequence.
///
/// Each item in the sequence has a weight that determines what portion of the
/// animation duration it occupies.
///
/// # Type Parameters
///
/// - `T`: The output type of the animation.
/// - `A`: The animatable type that produces `T` values.
///
/// # Examples
///
/// ```
/// use flui_animation::{TweenSequence, TweenSequenceItem, FloatTween, Animatable};
///
/// let items = vec![
///     TweenSequenceItem::new(FloatTween::new(0.0, 50.0), 1.0),
///     TweenSequenceItem::new(FloatTween::new(50.0, 100.0), 1.0),
/// ];
/// let sequence = TweenSequence::new(items);
///
/// assert_eq!(sequence.transform(0.0), 0.0);
/// assert_eq!(sequence.transform(0.5), 50.0);
/// assert_eq!(sequence.transform(1.0), 100.0);
/// ```
///
/// # Example with Colors
///
/// ```
/// use flui_animation::{TweenSequence, TweenSequenceItem, Animatable, ColorTween};
/// use flui_painting::styling::Color;
///
/// let items = vec![
///     TweenSequenceItem::new(ColorTween::new(Color::RED, Color::GREEN), 1.0),
///     TweenSequenceItem::new(ColorTween::new(Color::GREEN, Color::BLUE), 1.0),
/// ];
/// let sequence = TweenSequence::new(items);
///
/// // At t=0, we get RED
/// let start = sequence.transform(0.0);
/// assert_eq!(start, Color::RED);
///
/// // At t=0.5, we get GREEN (transition point)
/// let mid = sequence.transform(0.5);
/// assert_eq!(mid, Color::GREEN);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct TweenSequence<T, A: Animatable<T>> {
    /// The items in the sequence.
    items: Vec<TweenSequenceItem<T, A>>,
    /// Sum in the caller's original units; it can overflow for finite weights.
    total_weight: f64,
    /// Evaluation uses scaled weights so their sum cannot overflow.
    weight_scale: f64,
    normalized_total: f64,
}

impl<T, A: Animatable<T>> TweenSequence<T, A> {
    /// Creates a new tween sequence.
    ///
    /// # Panics
    ///
    /// Panics if `items` is empty or an item's weight is not finite and positive.
    #[must_use]
    pub fn new(items: Vec<TweenSequenceItem<T, A>>) -> Self {
        assert!(
            !items.is_empty(),
            "TweenSequence must have at least one item"
        );

        // Item fields are public, so validate again after any caller edits.
        let mut weight_scale = 0.0_f64;
        for item in &items {
            assert!(
                item.weight.is_finite() && item.weight > 0.0,
                "TweenSequence item weights must be finite and positive"
            );
            weight_scale = weight_scale.max(item.weight);
        }
        let total_weight = items.iter().map(|item| item.weight).sum();
        let normalized_total = items.iter().map(|item| item.weight / weight_scale).sum();

        Self {
            items,
            total_weight,
            weight_scale,
            normalized_total,
        }
    }

    /// Returns the items in the sequence.
    #[inline]
    #[must_use]
    pub fn items(&self) -> &[TweenSequenceItem<T, A>] {
        &self.items
    }

    /// Returns the sum of the original item weights.
    ///
    /// This sum can be infinite when finite weights overflow. Evaluation uses
    /// normalized weights and remains defined in that case.
    #[inline]
    #[must_use]
    pub fn total_weight(&self) -> f64 {
        self.total_weight
    }
}

impl<T, A: Animatable<T>> Animatable<T> for TweenSequence<T, A> {
    /// Unlike a plain [`Tween`], a sequence **clamps** `t` to `[0, 1]`:
    /// overshoot (elastic/spring `t` outside the unit range) saturates at the
    /// first/last item's endpoint rather than extrapolating, because there is
    /// no meaningful item to attribute out-of-range progress to.
    fn transform(&self, t: f64) -> T {
        let t = t.clamp(0.0, 1.0);

        // Exact endpoints remain reachable even if an item's relative weight
        // is too small to represent as a distinct progress interval in f64.
        if t == 0.0 {
            return self.items[0].tween.transform(0.0);
        }
        if t == 1.0 {
            return self.items[self.items.len() - 1].tween.transform(1.0);
        }

        let progress = t * self.normalized_total;
        let mut accumulated_weight = 0.0;
        for (i, item) in self.items.iter().enumerate() {
            let weight = item.weight / self.weight_scale;
            let item_end = accumulated_weight + weight;
            if progress <= item_end || i == self.items.len() - 1 {
                let local_t = if weight == 0.0 {
                    0.0
                } else {
                    ((progress - accumulated_weight) / weight).clamp(0.0, 1.0)
                };
                return item.tween.transform(local_t);
            }
            accumulated_weight = item_end;
        }

        // Unreachable: `new()` (the only constructor) asserts `items` is
        // non-empty, and the loop's `i == len - 1` arm returns on the final
        // iteration. Kept as a typed fallthrough for the compiler.
        self.items
            .last()
            .expect("BUG: TweenSequence items are non-empty (asserted in new), so the loop above returns on its final iteration")
            .tween
            .transform(1.0)
    }
}

/// An item in a [`TweenSequence`].
///
/// # Type Parameters
///
/// - `T`: The output type of the animation.
/// - `A`: The animatable type that produces `T` values.
#[derive(Debug, Clone, PartialEq)]
pub struct TweenSequenceItem<T, A: Animatable<T>> {
    /// The tween to use for this item.
    pub tween: A,

    /// The weight of this item in the sequence.
    ///
    /// The time spent in this item is proportional to its weight.
    pub weight: f64,

    _phantom: std::marker::PhantomData<T>,
}

impl<T, A: Animatable<T>> TweenSequenceItem<T, A> {
    /// Creates a new tween sequence item.
    ///
    /// # Panics
    ///
    /// Panics if `weight` is not positive (must be > 0).
    #[must_use]
    pub fn new(tween: A, weight: f64) -> Self {
        assert!(weight > 0.0, "Weight must be positive");
        assert!(weight.is_finite(), "Weight must be finite");
        Self {
            tween,
            weight,
            _phantom: std::marker::PhantomData,
        }
    }
}

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

// ============================================================================
// Extension Traits
// ============================================================================

/// Extension trait for [`Curve`] types.
///
/// Provides fluent methods for converting curves to animatables.
///
/// # Examples
///
/// ```
/// use flui_animation::{Curves, CurveExt, FloatTween, Animatable};
///
/// // Convert curve to tween
/// let tween = Curves::EaseIn.into_tween();
/// assert!(tween.transform(0.5) < 0.5);
///
/// // Chain curve with value tween
/// let value_tween = Curves::EaseIn.then(FloatTween::new(0.0, 100.0));
/// assert!(value_tween.transform(0.5) < 50.0);
/// ```
pub trait CurveExt: Curve + Sized {
    /// Converts this curve into a [`CurveTween`].
    #[inline]
    #[must_use]
    fn into_tween(self) -> CurveTween<Self> {
        CurveTween::new(self)
    }

    /// Chains this curve with an animatable.
    ///
    /// The curve is applied first, then its output is passed to the animatable.
    #[inline]
    #[must_use]
    fn then<T, A: Animatable<T>>(self, animatable: A) -> ChainedTween<CurveTween<Self>, A> {
        ChainedTween::new(CurveTween::new(self), animatable)
    }
}

// Blanket implementation for all Curve types
impl<C: Curve> CurveExt for C {}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_tween_sequence_weighted() {
        let items = vec![
            TweenSequenceItem::new(FloatTween::new(0.0, 50.0), 1.0),
            TweenSequenceItem::new(FloatTween::new(50.0, 100.0), 3.0),
        ];
        let sequence = TweenSequence::new(items);

        assert_eq!(sequence.transform(0.0), 0.0);
        // 25% through total = end of first item
        assert_eq!(sequence.transform(0.25), 50.0);
        // 62.5% through total = 50% through second item
        assert!((sequence.transform(0.625) - 75.0).abs() < 1e-5);
        assert_eq!(sequence.transform(1.0), 100.0);
    }

    // ========================================================================
    // Tests for new types: CurveTween, ChainedTween, extension traits
    // ========================================================================

    #[test]
    fn tween_types_contract() {
        crate::test_cases::run_cases(&[(
            "test tween sequence weighted",
            test_tween_sequence_weighted,
        )]);
    }
}
