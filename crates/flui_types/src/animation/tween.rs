//! Tween types for animating values.

use crate::geometry::{Offset, Rect, Size};
use crate::layout::{Alignment, EdgeInsets};
use crate::styling::{BorderRadius, Color};

/// A value that can be animated.
///
/// Similar to Flutter's `Animatable<T>`.
pub trait Animatable<T> {
    /// Returns the value of this object at the given animation value.
    fn transform(&self, t: f32) -> T;
}

/// A tween that linearly interpolates between a beginning and ending value.
///
/// Similar to Flutter's `Tween<T>`.
pub trait Tween<T>: Animatable<T> {
    /// The value this tween has at the beginning of the animation.
    fn begin(&self) -> &T;

    /// The value this tween has at the end of the animation.
    fn end(&self) -> &T;

    /// Returns the interpolated value for the current value of the given animation.
    fn lerp(&self, t: f32) -> T;
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
/// use flui_types::animation::{FloatTween, Animatable};
///
/// let tween = FloatTween::new(0.0, 100.0);
/// assert_eq!(tween.transform(0.0), 0.0);
/// assert_eq!(tween.transform(0.5), 50.0);
/// assert_eq!(tween.transform(1.0), 100.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FloatTween {
    /// The beginning value.
    pub begin: f32,
    /// The ending value.
    pub end: f32,
}

impl FloatTween {
    /// Creates a new float tween.
    pub const fn new(begin: f32, end: f32) -> Self {
        Self { begin, end }
    }
}

impl Animatable<f32> for FloatTween {
    fn transform(&self, t: f32) -> f32 {
        self.lerp(t)
    }
}

impl Tween<f32> for FloatTween {
    fn begin(&self) -> &f32 {
        &self.begin
    }

    fn end(&self) -> &f32 {
        &self.end
    }

    fn lerp(&self, t: f32) -> f32 {
        self.begin + (self.end - self.begin) * t.clamp(0.0, 1.0)
    }
}

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
    pub const fn new(begin: i32, end: i32) -> Self {
        Self { begin, end }
    }
}

impl Animatable<i32> for IntTween {
    fn transform(&self, t: f32) -> i32 {
        self.lerp(t)
    }
}

impl Tween<i32> for IntTween {
    fn begin(&self) -> &i32 {
        &self.begin
    }

    fn end(&self) -> &i32 {
        &self.end
    }

    fn lerp(&self, t: f32) -> i32 {
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
    pub const fn new(begin: i32, end: i32) -> Self {
        Self { begin, end }
    }
}

impl Animatable<i32> for StepTween {
    fn transform(&self, t: f32) -> i32 {
        self.lerp(t)
    }
}

impl Tween<i32> for StepTween {
    fn begin(&self) -> &i32 {
        &self.begin
    }

    fn end(&self) -> &i32 {
        &self.end
    }

    fn lerp(&self, t: f32) -> i32 {
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
    pub const fn new(value: T) -> Self {
        Self { value }
    }
}

impl<T: Clone> Animatable<T> for ConstantTween<T> {
    fn transform(&self, _t: f32) -> T {
        self.value.clone()
    }
}

impl<T: Clone> Tween<T> for ConstantTween<T> {
    fn begin(&self) -> &T {
        &self.value
    }

    fn end(&self) -> &T {
        &self.value
    }

    fn lerp(&self, _t: f32) -> T {
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
    pub fn new(tween: A) -> Self {
        Self {
            tween,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T, A: Animatable<T>> Animatable<T> for ReverseTween<T, A> {
    fn transform(&self, t: f32) -> T {
        self.tween.transform(1.0 - t.clamp(0.0, 1.0))
    }
}

// ============================================================================
// Geometric Tweens
// ============================================================================

/// A tween that linearly interpolates between two colors.
///
/// Similar to Flutter's `ColorTween`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ColorTween {
    /// The beginning color.
    pub begin: Color,
    /// The ending color.
    pub end: Color,
}

impl ColorTween {
    /// Creates a new color tween.
    pub const fn new(begin: Color, end: Color) -> Self {
        Self { begin, end }
    }
}

impl Animatable<Color> for ColorTween {
    fn transform(&self, t: f32) -> Color {
        self.lerp(t)
    }
}

impl Tween<Color> for ColorTween {
    fn begin(&self) -> &Color {
        &self.begin
    }

    fn end(&self) -> &Color {
        &self.end
    }

    fn lerp(&self, t: f32) -> Color {
        Color::lerp(self.begin, self.end, t.clamp(0.0, 1.0))
    }
}

/// A tween that linearly interpolates between two sizes.
///
/// Similar to Flutter's `SizeTween`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SizeTween {
    /// The beginning size.
    pub begin: Size,
    /// The ending size.
    pub end: Size,
}

impl SizeTween {
    /// Creates a new size tween.
    pub const fn new(begin: Size, end: Size) -> Self {
        Self { begin, end }
    }
}

impl Animatable<Size> for SizeTween {
    fn transform(&self, t: f32) -> Size {
        self.lerp(t)
    }
}

impl Tween<Size> for SizeTween {
    fn begin(&self) -> &Size {
        &self.begin
    }

    fn end(&self) -> &Size {
        &self.end
    }

    fn lerp(&self, t: f32) -> Size {
        let t = t.clamp(0.0, 1.0);
        Size::new(
            self.begin.width + (self.end.width - self.begin.width) * t,
            self.begin.height + (self.end.height - self.begin.height) * t,
        )
    }
}

/// A tween that linearly interpolates between two rectangles.
///
/// Similar to Flutter's `RectTween`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RectTween {
    /// The beginning rectangle.
    pub begin: Rect,
    /// The ending rectangle.
    pub end: Rect,
}

impl RectTween {
    /// Creates a new rectangle tween.
    pub const fn new(begin: Rect, end: Rect) -> Self {
        Self { begin, end }
    }
}

impl Animatable<Rect> for RectTween {
    fn transform(&self, t: f32) -> Rect {
        self.lerp(t)
    }
}

impl Tween<Rect> for RectTween {
    fn begin(&self) -> &Rect {
        &self.begin
    }

    fn end(&self) -> &Rect {
        &self.end
    }

    fn lerp(&self, t: f32) -> Rect {
        let t = t.clamp(0.0, 1.0);
        let min_x = self.begin.left() + (self.end.left() - self.begin.left()) * t;
        let min_y = self.begin.top() + (self.end.top() - self.begin.top()) * t;
        let width = self.begin.width() + (self.end.width() - self.begin.width()) * t;
        let height = self.begin.height() + (self.end.height() - self.begin.height()) * t;
        Rect::from_xywh(min_x, min_y, width, height)
    }
}

/// A tween that linearly interpolates between two offsets.
///
/// Similar to Flutter's `OffsetTween` (but Offset::lerp is used directly in Flutter).
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OffsetTween {
    /// The beginning offset.
    pub begin: Offset,
    /// The ending offset.
    pub end: Offset,
}

impl OffsetTween {
    /// Creates a new offset tween.
    pub const fn new(begin: Offset, end: Offset) -> Self {
        Self { begin, end }
    }
}

impl Animatable<Offset> for OffsetTween {
    fn transform(&self, t: f32) -> Offset {
        self.lerp(t)
    }
}

impl Tween<Offset> for OffsetTween {
    fn begin(&self) -> &Offset {
        &self.begin
    }

    fn end(&self) -> &Offset {
        &self.end
    }

    fn lerp(&self, t: f32) -> Offset {
        Offset::lerp(self.begin, self.end, t.clamp(0.0, 1.0))
    }
}

/// A tween that linearly interpolates between two alignments.
///
/// Similar to Flutter's `AlignmentTween`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AlignmentTween {
    /// The beginning alignment.
    pub begin: Alignment,
    /// The ending alignment.
    pub end: Alignment,
}

impl AlignmentTween {
    /// Creates a new alignment tween.
    pub const fn new(begin: Alignment, end: Alignment) -> Self {
        Self { begin, end }
    }
}

impl Animatable<Alignment> for AlignmentTween {
    fn transform(&self, t: f32) -> Alignment {
        self.lerp(t)
    }
}

impl Tween<Alignment> for AlignmentTween {
    fn begin(&self) -> &Alignment {
        &self.begin
    }

    fn end(&self) -> &Alignment {
        &self.end
    }

    fn lerp(&self, t: f32) -> Alignment {
        Alignment::lerp(self.begin, self.end, t.clamp(0.0, 1.0))
    }
}

/// A tween that linearly interpolates between two edge insets.
///
/// Similar to Flutter's `EdgeInsetsTween`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EdgeInsetsTween {
    /// The beginning edge insets.
    pub begin: EdgeInsets,
    /// The ending edge insets.
    pub end: EdgeInsets,
}

impl EdgeInsetsTween {
    /// Creates a new edge insets tween.
    pub const fn new(begin: EdgeInsets, end: EdgeInsets) -> Self {
        Self { begin, end }
    }
}

impl Animatable<EdgeInsets> for EdgeInsetsTween {
    fn transform(&self, t: f32) -> EdgeInsets {
        self.lerp(t)
    }
}

impl Tween<EdgeInsets> for EdgeInsetsTween {
    fn begin(&self) -> &EdgeInsets {
        &self.begin
    }

    fn end(&self) -> &EdgeInsets {
        &self.end
    }

    fn lerp(&self, t: f32) -> EdgeInsets {
        let t = t.clamp(0.0, 1.0);
        EdgeInsets::new(
            self.begin.left + (self.end.left - self.begin.left) * t,
            self.begin.top + (self.end.top - self.begin.top) * t,
            self.begin.right + (self.end.right - self.begin.right) * t,
            self.begin.bottom + (self.end.bottom - self.begin.bottom) * t,
        )
    }
}

/// A tween that linearly interpolates between two border radii.
///
/// Similar to Flutter's `BorderRadiusTween`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BorderRadiusTween {
    /// The beginning border radius.
    pub begin: BorderRadius,
    /// The ending border radius.
    pub end: BorderRadius,
}

impl BorderRadiusTween {
    /// Creates a new border radius tween.
    pub const fn new(begin: BorderRadius, end: BorderRadius) -> Self {
        Self { begin, end }
    }
}

impl Animatable<BorderRadius> for BorderRadiusTween {
    fn transform(&self, t: f32) -> BorderRadius {
        self.lerp(t)
    }
}

impl Tween<BorderRadius> for BorderRadiusTween {
    fn begin(&self) -> &BorderRadius {
        &self.begin
    }

    fn end(&self) -> &BorderRadius {
        &self.end
    }

    fn lerp(&self, t: f32) -> BorderRadius {
        BorderRadius::lerp(self.begin, self.end, t.clamp(0.0, 1.0))
    }
}

// ============================================================================
// Complex Tweens
// ============================================================================

/// A tween that chains together multiple float tweens in sequence.
///
/// Similar to Flutter's `TweenSequence<T>`. This is a simplified version
/// that works with f32 values only for now.
///
/// # Examples
///
/// ```
/// use flui_types::animation::{TweenSequence, TweenSequenceItem, FloatTween, Animatable};
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
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TweenSequence {
    /// The items in the sequence.
    pub items: Vec<TweenSequenceItem>,
}

impl TweenSequence {
    /// Creates a new tween sequence.
    pub fn new(items: Vec<TweenSequenceItem>) -> Self {
        assert!(
            !items.is_empty(),
            "TweenSequence must have at least one item"
        );

        // Validate that weights sum to a positive number
        let total_weight: f32 = items.iter().map(|item| item.weight).sum();
        assert!(total_weight > 0.0, "Total weight must be positive");

        Self { items }
    }
}

impl Animatable<f32> for TweenSequence {
    fn transform(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);

        // Calculate total weight
        let total_weight: f32 = self.items.iter().map(|item| item.weight).sum();

        // Find which item we're in
        let mut accumulated_weight = 0.0;
        for (i, item) in self.items.iter().enumerate() {
            let item_end = (accumulated_weight + item.weight) / total_weight;

            if t <= item_end || i == self.items.len() - 1 {
                // Calculate local t within this item
                let item_start = accumulated_weight / total_weight;
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

/// An item in a [TweenSequence].
///
/// Similar to Flutter's `TweenSequenceItem<T>`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TweenSequenceItem {
    /// The tween to use for this item.
    pub tween: FloatTween,

    /// The weight of this item in the sequence.
    ///
    /// The time spent in this item is proportional to its weight.
    pub weight: f32,
}

impl TweenSequenceItem {
    /// Creates a new tween sequence item.
    pub fn new(tween: FloatTween, weight: f32) -> Self {
        assert!(weight >= 0.0, "Weight must be non-negative");
        Self { tween, weight }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(mid.red(), 127);
        assert_eq!(mid.blue(), 127);
    }

    #[test]
    fn test_size_tween() {
        let tween = SizeTween::new(Size::new(0.0, 0.0), Size::new(100.0, 200.0));
        let mid = tween.transform(0.5);
        assert_eq!(mid.width, 50.0);
        assert_eq!(mid.height, 100.0);
    }

    #[test]
    fn test_rect_tween() {
        let begin = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let end = Rect::from_xywh(100.0, 100.0, 200.0, 200.0);
        let tween = RectTween::new(begin, end);

        let mid = tween.transform(0.5);
        assert_eq!(mid.left(), 50.0);
        assert_eq!(mid.top(), 50.0);
    }

    #[test]
    fn test_offset_tween() {
        let tween = OffsetTween::new(Offset::ZERO, Offset::new(100.0, 200.0));
        let mid = tween.transform(0.5);
        assert_eq!(mid.dx, 50.0);
        assert_eq!(mid.dy, 100.0);
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
        let begin = EdgeInsets::all(0.0);
        let end = EdgeInsets::all(20.0);
        let tween = EdgeInsetsTween::new(begin, end);

        let mid = tween.transform(0.5);
        assert_eq!(mid.left, 10.0);
        assert_eq!(mid.top, 10.0);
    }

    #[test]
    fn test_border_radius_tween() {
        let begin = BorderRadius::circular(0.0);
        let end = BorderRadius::circular(20.0);
        let tween = BorderRadiusTween::new(begin, end);

        let mid = tween.transform(0.5);
        assert_eq!(mid.top_left.x, 10.0);
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
}
