//! Tween types for animating values.
//!
//! This module provides types for animating values between a beginning and ending state.
//! The core abstraction is the [`Animatable`] trait, which maps a progress value (0.0 to 1.0)
//! to an output value of any type.
//!
//! # Extension Traits
//!
//! This module provides extension traits for fluent API composition:
//!
//! - [`AnimatableExt`] - adds `.reversed()` and `.chain()` methods to any `Animatable`
//!
//! # Examples
//!
//! ```
//! use flui_animation::{FloatTween, Animatable, TweenAnimatableExt};
//!
//! let tween = FloatTween::new(0.0, 100.0);
//!
//! // Use the tween directly
//! assert_eq!(tween.transform(0.5), 50.0);
//!
//! // Or reverse it using extension trait
//! let reversed = tween.reversed();
//! assert_eq!(reversed.transform(0.0), 100.0);
//! ```

use crate::curve::Curve;
use flui_types::geometry::{Edges, Lerp, Matrix4, Offset, Pixels, Rect, Size};
use flui_types::layout::Alignment;
use flui_types::styling::{BorderRadius, Color};

/// A value that can be animated.
///
/// Similar to Flutter's `Animatable<T>`.
pub trait Animatable<T> {
    /// Returns the value of this object at the given animation value.
    fn transform(&self, t: f32) -> T;
}

/// A tween that linearly interpolates between a `begin` and `end` value of any
/// [`Lerp`] type. One generic struct replaces Flutter's per-type tween classes
/// (`ColorTween`, `SizeTween`, ...), which exist only because Dart dispatches
/// `begin + (end - begin) * t` dynamically.
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
    fn transform(&self, t: f32) -> V {
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
/// Similar to Flutter's `Tween<double>`.
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
/// Tween between two floats. Alias for `Tween<f32>`.
pub type FloatTween = Tween<f32>;

/// A tween that linearly interpolates between two integers, rounding to the
/// nearest integer.
///
/// Similar to Flutter's `IntTween`.
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
    #[allow(clippy::cast_possible_truncation)] // rounded f32->i32, saturating cast
    fn transform(&self, t: f32) -> i32 {
        let t = t.clamp(0.0, 1.0);
        (self.begin as f32 + (self.end - self.begin) as f32 * t).round() as i32
    }
}

/// A tween that linearly interpolates between two integers, flooring to the
/// nearest integer.
///
/// Similar to Flutter's `StepTween`.
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
    #[allow(clippy::cast_possible_truncation)] // floored f32->i32, saturating cast
    fn transform(&self, t: f32) -> i32 {
        let t = t.clamp(0.0, 1.0);
        (self.begin as f32 + (self.end - self.begin) as f32 * t).floor() as i32
    }
}

/// A tween that always returns the same value.
///
/// Similar to Flutter's `ConstantTween<T>`.
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
    fn transform(&self, _t: f32) -> T {
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
    fn transform(&self, t: f32) -> T {
        self.tween.transform(1.0 - t)
    }
}

// ============================================================================
// Geometric Tweens
// ============================================================================

/// A tween that linearly interpolates between two colors.
///
/// Similar to Flutter's `ColorTween`.
/// Tween between two colors. Alias for `Tween<Color>`.
pub type ColorTween = Tween<Color>;

/// A tween that linearly interpolates between two sizes.
///
/// Similar to Flutter's `SizeTween`.
/// Tween between two sizes. Alias for `Tween<Size<Pixels>>`.
pub type SizeTween = Tween<Size<Pixels>>;

/// A tween that linearly interpolates between two rectangles.
///
/// Similar to Flutter's `RectTween`.
/// Tween between two rectangles. Alias for `Tween<Rect<Pixels>>`.
pub type RectTween = Tween<Rect<Pixels>>;

/// A tween that linearly interpolates between two offsets.
///
/// Similar to Flutter's `OffsetTween` (but `Offset::lerp` is used directly in Flutter).
/// Tween between two offsets. Alias for `Tween<Offset<Pixels>>`.
pub type OffsetTween = Tween<Offset<Pixels>>;

/// A tween that linearly interpolates between two alignments.
///
/// Similar to Flutter's `AlignmentTween`.
/// Tween between two alignments. Alias for `Tween<Alignment>`.
pub type AlignmentTween = Tween<Alignment>;

/// A tween that linearly interpolates between two edge insets.
///
/// Similar to Flutter's `EdgeInsetsTween`.
/// Tween between two edge insets. Alias for `Tween<Edges<Pixels>>`.
pub type EdgeInsetsTween = Tween<Edges<Pixels>>;

/// Tween between two border radii. Alias for `Tween<BorderRadius>` (now that
/// `Lerp for Corners<T>` lives in flui-geometry).
pub type BorderRadiusTween = Tween<BorderRadius>;

/// Tween between two affine transforms. Alias for `Tween<Matrix4>`; interpolates
/// by decompose -> slerp (see `Matrix4::lerp`), so rotation animates correctly.
pub type Matrix4Tween = Tween<Matrix4>;

// ============================================================================
// Complex Tweens
// ============================================================================

/// A tween that chains together multiple tweens in sequence.
///
/// Similar to Flutter's `TweenSequence<T>`. Each item in the sequence has a
/// weight that determines what portion of the animation duration it occupies.
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
/// use flui_types::styling::Color;
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
    /// Cached total weight for performance.
    total_weight: f32,
}

impl<T, A: Animatable<T>> TweenSequence<T, A> {
    /// Creates a new tween sequence.
    ///
    /// # Panics
    ///
    /// Panics if `items` is empty or if total weight is not positive.
    #[must_use]
    pub fn new(items: Vec<TweenSequenceItem<T, A>>) -> Self {
        assert!(
            !items.is_empty(),
            "TweenSequence must have at least one item"
        );

        // Validate that weights sum to a positive number
        let total_weight: f32 = items.iter().map(|item| item.weight).sum();
        assert!(total_weight > 0.0, "Total weight must be positive");

        Self {
            items,
            total_weight,
        }
    }

    /// Returns the items in the sequence.
    #[inline]
    #[must_use]
    pub fn items(&self) -> &[TweenSequenceItem<T, A>] {
        &self.items
    }

    /// Returns the total weight of all items.
    #[inline]
    #[must_use]
    pub fn total_weight(&self) -> f32 {
        self.total_weight
    }
}

impl<T, A: Animatable<T>> Animatable<T> for TweenSequence<T, A> {
    fn transform(&self, t: f32) -> T {
        let t = t.clamp(0.0, 1.0);

        // Find which item we're in
        let mut accumulated_weight = 0.0;
        for (i, item) in self.items.iter().enumerate() {
            let item_end = (accumulated_weight + item.weight) / self.total_weight;

            if t <= item_end || i == self.items.len() - 1 {
                // Calculate local t within this item
                let item_start = accumulated_weight / self.total_weight;
                let local_t = if (item_end - item_start).abs() < 1e-6 {
                    0.0
                } else {
                    ((t - item_start) / (item_end - item_start)).clamp(0.0, 1.0)
                };

                return item.tween.transform(local_t);
            }

            accumulated_weight += item.weight;
        }

        // Should never reach here, but return last item's end value just in case
        self.items.last().unwrap().tween.transform(1.0)
    }
}

/// An item in a [`TweenSequence`].
///
/// Similar to Flutter's `TweenSequenceItem<T>`.
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
    pub weight: f32,

    _phantom: std::marker::PhantomData<T>,
}

impl<T, A: Animatable<T>> TweenSequenceItem<T, A> {
    /// Creates a new tween sequence item.
    ///
    /// # Panics
    ///
    /// Panics if `weight` is not positive (must be > 0).
    #[must_use]
    pub fn new(tween: A, weight: f32) -> Self {
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
/// Similar to Flutter's `CurveTween`.
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

impl<C: Curve> Animatable<f32> for CurveTween<C> {
    #[inline]
    fn transform(&self, t: f32) -> f32 {
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
    A: Animatable<f32>,
    B: Animatable<T>,
{
    #[inline]
    fn transform(&self, t: f32) -> T {
        let curved_t = self.first.transform(t);
        self.second.transform(curved_t)
    }
}

// ============================================================================
// Extension Traits
// ============================================================================

/// Extension trait for [`Animatable`] types.
///
/// Provides fluent methods for composing and transforming animatables.
///
/// # Examples
///
/// ```
/// use flui_animation::{FloatTween, Animatable, TweenAnimatableExt, Curves};
///
/// let tween = FloatTween::new(0.0, 100.0);
///
/// // Reverse the tween
/// let reversed = tween.reversed();
/// assert_eq!(reversed.transform(0.0), 100.0);
///
/// // Chain with a curve
/// let curved = tween.with_curve(Curves::EaseIn);
/// assert!(curved.transform(0.5) < 50.0);
/// ```
pub trait AnimatableExt<T>: Animatable<T> + Sized {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// Returns a reversed version of this animatable.
    ///
    /// The reversed animatable transforms `t` to `1.0 - t` before passing
    /// to the original animatable.
    #[inline]
    #[must_use]
    fn reversed(self) -> ReverseTween<T, Self> {
        ReverseTween::new(self)
    }

    /// Chains this animatable with another.
    ///
    /// The output of `self` is passed as input to `other`.
    /// This is useful when `self` outputs `f32` (like a curve) and `other`
    /// transforms that to the final type.
    #[inline]
    #[must_use]
    fn chain<B>(self, other: B) -> ChainedTween<Self, B>
    where
        Self: Animatable<f32>,
    {
        ChainedTween::new(self, other)
    }

    /// Applies a curve to this animatable.
    ///
    /// This is a convenience method that chains a `CurveTween` before this animatable.
    #[inline]
    #[must_use]
    fn with_curve<C: Curve>(self, curve: C) -> ChainedTween<CurveTween<C>, Self>
    where
        Self: Animatable<T>,
    {
        ChainedTween::new(CurveTween::new(curve), self)
    }
}

// Blanket implementation for all Animatable types
impl<T, A: Animatable<T>> AnimatableExt<T> for A {}

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
    // `BorderRadius::circular` is a `BorderRadiusExt` method; the production code
    // no longer needs the trait (Lerp handles interpolation), only the tests do.
    use flui_types::styling::BorderRadiusExt;
    use crate::curve::Curves;

    #[test]
    fn test_float_tween() {
        let tween = FloatTween::new(0.0, 100.0);
        assert_eq!(tween.transform(0.0), 0.0);
        assert_eq!(tween.transform(0.5), 50.0);
        assert_eq!(tween.transform(1.0), 100.0);
    }

    #[test]
    fn test_int_tween() {
        let tween = IntTween::new(0, 10);
        assert_eq!(tween.transform(0.0), 0);
        assert_eq!(tween.transform(0.5), 5);
        assert_eq!(tween.transform(1.0), 10);
    }

    #[test]
    fn test_step_tween() {
        let tween = StepTween::new(0, 10);
        assert_eq!(tween.transform(0.0), 0);
        assert_eq!(tween.transform(0.49), 4); // floors
        assert_eq!(tween.transform(1.0), 10);
    }

    #[test]
    fn test_constant_tween() {
        let tween = ConstantTween::new(42);
        assert_eq!(tween.transform(0.0), 42);
        assert_eq!(tween.transform(0.5), 42);
        assert_eq!(tween.transform(1.0), 42);
    }

    #[test]
    fn test_reverse_tween() {
        let tween = FloatTween::new(0.0, 100.0);
        let reversed = ReverseTween::new(tween);
        assert_eq!(reversed.transform(0.0), 100.0);
        assert_eq!(reversed.transform(0.5), 50.0);
        assert_eq!(reversed.transform(1.0), 0.0);
    }

    #[test]
    fn test_color_tween() {
        let tween = ColorTween::new(Color::RED, Color::BLUE);
        let mid = tween.transform(0.5);
        // 255 * 0.5 = 127.5 -> rounds to 128 (the old code truncated to 127).
        assert_eq!(mid.r, 128);
        assert_eq!(mid.b, 128);
    }

    #[test]
    fn tween_extrapolates_overshoot() {
        // B3 regression: the generic Tween must NOT clamp t, so spring/elastic
        // overshoot (t > 1, t < 0) reaches the value.
        let tween = FloatTween::new(0.0, 10.0);
        assert_eq!(tween.transform(1.5), 15.0, "overshoot above end");
        assert_eq!(tween.transform(-0.5), -5.0, "overshoot below begin");
        // Exact endpoints are returned verbatim.
        assert_eq!(tween.transform(0.0), 0.0);
        assert_eq!(tween.transform(1.0), 10.0);
    }

    #[test]
    fn test_size_tween() {
        use flui_types::geometry::px;
        let tween = SizeTween::new(Size::new(px(0.0), px(0.0)), Size::new(px(100.0), px(200.0)));
        let mid = tween.transform(0.5);
        assert_eq!(mid.width, px(50.0));
        assert_eq!(mid.height, px(100.0));
    }

    #[test]
    fn test_rect_tween() {
        use flui_types::geometry::px;
        let begin = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));
        let end = Rect::from_xywh(px(100.0), px(100.0), px(200.0), px(200.0));
        let tween = RectTween::new(begin, end);

        let mid = tween.transform(0.5);
        assert_eq!(mid.left(), px(50.0));
        assert_eq!(mid.top(), px(50.0));
    }

    #[test]
    fn test_offset_tween() {
        use flui_types::geometry::px;
        let tween = OffsetTween::new(Offset::ZERO, Offset::new(px(100.0), px(200.0)));
        let mid = tween.transform(0.5);
        assert_eq!(mid.dx, px(50.0));
        assert_eq!(mid.dy, px(100.0));
    }

    #[test]
    fn test_alignment_tween() {
        let tween = AlignmentTween::new(Alignment::TOP_LEFT, Alignment::BOTTOM_RIGHT);
        let mid = tween.transform(0.5);
        assert_eq!(mid.x, 0.0);
        assert_eq!(mid.y, 0.0);
    }

    #[test]
    fn test_edge_insets_tween() {
        use flui_types::geometry::px;
        let begin = Edges::all(px(0.0));
        let end = Edges::all(px(20.0));
        let tween = EdgeInsetsTween::new(begin, end);

        let mid = tween.transform(0.5);
        assert_eq!(mid.left, px(10.0));
        assert_eq!(mid.top, px(10.0));
    }

    #[test]
    fn test_border_radius_tween() {
        use flui_types::geometry::px;
        let begin = BorderRadius::circular(px(0.0));
        let end = BorderRadius::circular(px(20.0));
        let tween = BorderRadiusTween::new(begin, end);

        let mid = tween.transform(0.5);
        assert_eq!(mid.top_left.x, px(10.0));
    }

    #[test]
    fn test_tween_sequence() {
        let items = vec![
            TweenSequenceItem::new(FloatTween::new(0.0, 50.0), 1.0),
            TweenSequenceItem::new(FloatTween::new(50.0, 100.0), 1.0),
        ];
        let sequence = TweenSequence::new(items);

        assert_eq!(sequence.transform(0.0), 0.0);
        assert_eq!(sequence.transform(0.5), 50.0);
        assert_eq!(sequence.transform(1.0), 100.0);
    }

    #[test]
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
    fn test_curve_tween() {
        let tween = CurveTween::new(Curves::Linear);
        assert_eq!(tween.transform(0.0), 0.0);
        assert_eq!(tween.transform(0.5), 0.5);
        assert_eq!(tween.transform(1.0), 1.0);
    }

    #[test]
    fn test_curve_tween_ease_in() {
        let tween = CurveTween::new(Curves::EaseIn);
        assert!(tween.transform(0.0).abs() < 0.001);
        assert!(tween.transform(0.5) < 0.5); // ease-in is slower at start
        assert!((tween.transform(1.0) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_curve_tween_clamps_input() {
        let tween = CurveTween::new(Curves::Linear);
        assert_eq!(tween.transform(-0.5), 0.0);
        assert_eq!(tween.transform(1.5), 1.0);
    }

    #[test]
    fn test_chained_tween() {
        let chained =
            ChainedTween::new(CurveTween::new(Curves::Linear), FloatTween::new(0.0, 100.0));
        assert_eq!(chained.transform(0.0), 0.0);
        assert_eq!(chained.transform(0.5), 50.0);
        assert_eq!(chained.transform(1.0), 100.0);
    }

    #[test]
    fn test_chained_tween_with_ease_in() {
        let chained =
            ChainedTween::new(CurveTween::new(Curves::EaseIn), FloatTween::new(0.0, 100.0));
        assert!(chained.transform(0.0).abs() < 0.1);
        assert!(chained.transform(0.5) < 50.0); // ease-in effect
        assert!((chained.transform(1.0) - 100.0).abs() < 0.1);
    }

    #[test]
    fn test_animatable_ext_reversed() {
        let tween = FloatTween::new(0.0, 100.0);
        let reversed = tween.reversed();
        assert_eq!(reversed.transform(0.0), 100.0);
        assert_eq!(reversed.transform(0.5), 50.0);
        assert_eq!(reversed.transform(1.0), 0.0);
    }

    #[test]
    fn test_animatable_ext_chain() {
        let curve = CurveTween::new(Curves::Linear);
        let chained = curve.chain(FloatTween::new(0.0, 100.0));
        assert_eq!(chained.transform(0.5), 50.0);
    }

    #[test]
    fn test_animatable_ext_with_curve() {
        let tween = FloatTween::new(0.0, 100.0);
        let curved = tween.with_curve(Curves::Linear);
        assert_eq!(curved.transform(0.5), 50.0);
    }

    #[test]
    fn test_curve_ext_into_tween() {
        let tween = Curves::Linear.into_tween();
        assert_eq!(tween.transform(0.5), 0.5);
    }

    #[test]
    fn test_curve_ext_then() {
        let chained = Curves::Linear.then(FloatTween::new(0.0, 100.0));
        assert_eq!(chained.transform(0.5), 50.0);
    }

    #[test]
    fn test_double_reverse() {
        let tween = FloatTween::new(0.0, 100.0);
        let double_reversed = tween.reversed().reversed();
        assert_eq!(double_reversed.transform(0.0), 0.0);
        assert_eq!(double_reversed.transform(1.0), 100.0);
    }

    #[test]
    fn test_tween_sequence_generic_with_color() {
        use flui_types::styling::Color;

        let items = vec![
            TweenSequenceItem::new(ColorTween::new(Color::RED, Color::GREEN), 1.0),
            TweenSequenceItem::new(ColorTween::new(Color::GREEN, Color::BLUE), 1.0),
        ];
        let sequence = TweenSequence::new(items);

        // At t=0, we get RED
        assert_eq!(sequence.transform(0.0), Color::RED);

        // At t=0.5, we get GREEN (transition point)
        assert_eq!(sequence.transform(0.5), Color::GREEN);

        // At t=1.0, we get BLUE
        assert_eq!(sequence.transform(1.0), Color::BLUE);
    }

    #[test]
    fn test_tween_sequence_items_accessor() {
        let items = vec![
            TweenSequenceItem::new(FloatTween::new(0.0, 50.0), 1.0),
            TweenSequenceItem::new(FloatTween::new(50.0, 100.0), 2.0),
        ];
        let sequence = TweenSequence::new(items);

        assert_eq!(sequence.items().len(), 2);
        assert_eq!(sequence.total_weight(), 3.0);
    }
}
