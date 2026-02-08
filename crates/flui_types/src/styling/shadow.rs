//! Shadow types for styling

use crate::geometry::traits::{NumericUnit, Unit};
use crate::geometry::{Offset, Pixels};
use crate::styling::Color;

/// A single shadow cast by a shape.
///
/// Generic over unit type `T` for full type safety. Use `Shadow<Pixels>` for UI shadows.
///
/// # Examples
///
/// ```
/// use flui_types::styling::Shadow;
/// use flui_types::geometry::{Offset, px};
/// use flui_types::styling::Color;
///
/// let shadow = Shadow::new(Color::BLACK, Offset::new(px(2.0), px(2.0)), px(4.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Shadow<T: Unit> {
    /// The color of the shadow.
    pub color: Color,

    /// The displacement of the shadow from the casting element.
    pub offset: Offset<T>,

    /// The standard deviation of the Gaussian to convolve with the shadow's shape.
    ///
    /// A blur radius of 0.0 means the shadow has a sharp edge.
    pub blur_radius: T,
}

impl<T: Unit> Shadow<T> {
    /// Creates a new shadow.
    ///
    /// # Arguments
    ///
    /// * `color` - The color of the shadow
    /// * `offset` - The offset of the shadow from the element
    /// * `blur_radius` - The blur radius of the shadow (0.0 for sharp edges)
    #[inline]
    pub const fn new(color: Color, offset: Offset<T>, blur_radius: T) -> Self {
        Self {
            color,
            offset,
            blur_radius,
        }
    }

    /// Creates a copy of this shadow with the given color.
    #[inline]
    pub const fn with_color(self, color: Color) -> Self {
        Self { color, ..self }
    }

    /// Creates a copy of this shadow with the given offset.
    #[inline]
    pub const fn with_offset(self, offset: Offset<T>) -> Self {
        Self { offset, ..self }
    }

    /// Creates a copy of this shadow with the given blur radius.
    #[inline]
    pub const fn with_blur_radius(self, blur_radius: T) -> Self {
        Self {
            blur_radius,
            ..self
        }
    }
}

impl Shadow<Pixels> {
    /// Converts a blur radius in pixels to sigma units for use in Gaussian blur.
    ///
    /// This follows the same conversion that Flutter uses.
    #[inline]
    pub fn convert_radius_to_sigma(radius: Pixels) -> f32 {
        radius.0 * 0.57735 + 0.5
    }

    /// The standard deviation of the Gaussian blur to apply to the shadow.
    #[inline]
    pub fn blur_sigma(&self) -> f32 {
        Self::convert_radius_to_sigma(self.blur_radius)
    }
}

impl<T: NumericUnit> Shadow<T>
where
    T: std::ops::Mul<f32, Output = T>,
{
    /// Linearly interpolate between two shadows.
    #[inline]
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            color: Color::lerp(a.color, b.color, t),
            offset: crate::geometry::Offset::new(
                a.offset.dx * (1.0 - t) + b.offset.dx * t,
                a.offset.dy * (1.0 - t) + b.offset.dy * t,
            ),
            blur_radius: a.blur_radius * (1.0 - t) + b.blur_radius * t,
        }
    }

    /// Linearly interpolate between two lists of shadows.
    ///
    /// If the lists are different lengths, the shorter list is padded with
    /// transparent shadows at offset zero with zero blur.
    #[inline]
    pub fn lerp_list(a: &[Self], b: &[Self], t: f32) -> Vec<Self> {
        let t = t.clamp(0.0, 1.0);
        let max_len = a.len().max(b.len());
        let mut result = Vec::with_capacity(max_len);

        for i in 0..max_len {
            let a_shadow = a.get(i).copied().unwrap_or(Self {
                color: Color::TRANSPARENT,
                offset: Offset::new(T::zero(), T::zero()),
                blur_radius: T::zero(),
            });
            let b_shadow = b.get(i).copied().unwrap_or(Self {
                color: Color::TRANSPARENT,
                offset: Offset::new(T::zero(), T::zero()),
                blur_radius: T::zero(),
            });
            result.push(Self::lerp(a_shadow, b_shadow, t));
        }

        result
    }

    /// Scales the shadow's offset and blur radius by the given factor.
    #[inline]
    pub fn scale(&self, factor: f32) -> Self {
        Self {
            offset: self.offset * factor,
            blur_radius: self.blur_radius * factor,
            ..*self
        }
    }
}

impl<T: Unit> Default for Shadow<T> {
    #[inline]
    fn default() -> Self {
        Self {
            color: Color::BLACK,
            offset: Offset::new(T::zero(), T::zero()),
            blur_radius: T::zero(),
        }
    }
}

/// A shadow cast by a box.
///
/// Generic over unit type `T` for full type safety. Use `BoxShadow<Pixels>` for UI shadows.
///
/// BoxShadow extends Shadow with a spread radius, which causes the shadow to
/// expand or contract before being blurred. It also supports inner shadows.
///
/// # Examples
///
/// ```
/// use flui_types::styling::BoxShadow;
/// use flui_types::geometry::{Offset, px};
/// use flui_types::styling::Color;
///
/// // Standard drop shadow
/// let shadow = BoxShadow::new(
///     Color::BLACK.with_alpha(76),
///     Offset::new(px(0.0), px(4.0)),
///     px(8.0),
///     px(2.0),
/// );
///
/// // Inner shadow (like CSS inset)
/// let inner = BoxShadow::inner(
///     Color::BLACK.with_alpha(51),
///     Offset::new(px(0.0), px(2.0)),
///     px(4.0),
///     px(0.0),
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BoxShadow<T: Unit> {
    /// The color of the shadow.
    pub color: Color,

    /// The displacement of the shadow from the casting element.
    pub offset: Offset<T>,

    /// The standard deviation of the Gaussian to convolve with the shadow's shape.
    pub blur_radius: T,

    /// The amount the box should be inflated prior to applying the blur.
    ///
    /// Positive values make the shadow larger, negative values make it smaller.
    pub spread_radius: T,

    /// Whether this is an inner shadow (rendered inside the box).
    ///
    /// Similar to CSS `inset` keyword in `box-shadow`.
    pub inset: bool,
}

impl<T: Unit> BoxShadow<T> {
    /// Creates a new box shadow.
    ///
    /// # Arguments
    ///
    /// * `color` - The color of the shadow
    /// * `offset` - The offset of the shadow from the element
    /// * `blur_radius` - The blur radius of the shadow
    /// * `spread_radius` - The spread radius of the shadow
    #[inline]
    pub const fn new(color: Color, offset: Offset<T>, blur_radius: T, spread_radius: T) -> Self {
        Self {
            color,
            offset,
            blur_radius,
            spread_radius,
            inset: false,
        }
    }

    /// Creates a new inner (inset) shadow.
    ///
    /// # Arguments
    ///
    /// * `color` - The color of the shadow
    /// * `offset` - The offset of the shadow from the element
    /// * `blur_radius` - The blur radius of the shadow
    /// * `spread_radius` - The spread radius of the shadow
    #[inline]
    pub const fn inner(color: Color, offset: Offset<T>, blur_radius: T, spread_radius: T) -> Self {
        Self {
            color,
            offset,
            blur_radius,
            spread_radius,
            inset: true,
        }
    }

    /// Creates a copy of this box shadow with the given color.
    #[inline]
    pub const fn with_color(self, color: Color) -> Self {
        Self { color, ..self }
    }

    /// Creates a copy of this box shadow with the given offset.
    #[inline]
    pub const fn with_offset(self, offset: Offset<T>) -> Self {
        Self { offset, ..self }
    }

    /// Creates a copy of this box shadow with the given blur radius.
    #[inline]
    pub const fn with_blur_radius(self, blur_radius: T) -> Self {
        Self {
            blur_radius,
            ..self
        }
    }

    /// Creates a copy of this box shadow with the given spread radius.
    #[inline]
    pub const fn with_spread_radius(self, spread_radius: T) -> Self {
        Self {
            spread_radius,
            ..self
        }
    }

    /// Creates a copy of this box shadow with the given inset value.
    #[inline]
    pub const fn with_inset(self, inset: bool) -> Self {
        Self { inset, ..self }
    }

    /// Converts this BoxShadow to a Shadow (losing the spread radius).
    #[inline]
    pub const fn to_shadow(self) -> Shadow<T> {
        Shadow {
            color: self.color,
            offset: self.offset,
            blur_radius: self.blur_radius,
        }
    }
}

impl BoxShadow<Pixels> {
    /// Converts a blur radius in pixels to sigma units for use in Gaussian blur.
    #[inline]
    pub fn convert_radius_to_sigma(radius: Pixels) -> f32 {
        Shadow::<Pixels>::convert_radius_to_sigma(radius)
    }

    /// The standard deviation of the Gaussian blur to apply to the shadow.
    #[inline]
    pub fn blur_sigma(&self) -> f32 {
        Self::convert_radius_to_sigma(self.blur_radius)
    }
}

impl<T: NumericUnit> BoxShadow<T>
where
    T: std::ops::Mul<f32, Output = T>,
{
    /// Linearly interpolate between two box shadows.
    #[inline]
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            color: Color::lerp(a.color, b.color, t),
            offset: crate::geometry::Offset::new(
                a.offset.dx * (1.0 - t) + b.offset.dx * t,
                a.offset.dy * (1.0 - t) + b.offset.dy * t,
            ),
            blur_radius: a.blur_radius * (1.0 - t) + b.blur_radius * t,
            spread_radius: a.spread_radius * (1.0 - t) + b.spread_radius * t,
            inset: if t < 0.5 { a.inset } else { b.inset },
        }
    }

    /// Linearly interpolate between two lists of box shadows.
    #[inline]
    pub fn lerp_list(a: &[Self], b: &[Self], t: f32) -> Vec<Self> {
        let t = t.clamp(0.0, 1.0);
        let max_len = a.len().max(b.len());
        let mut result = Vec::with_capacity(max_len);

        for i in 0..max_len {
            let a_shadow = a.get(i).copied().unwrap_or(Self {
                color: Color::TRANSPARENT,
                offset: Offset::new(T::zero(), T::zero()),
                blur_radius: T::zero(),
                spread_radius: T::zero(),
                inset: false,
            });
            let b_shadow = b.get(i).copied().unwrap_or(Self {
                color: Color::TRANSPARENT,
                offset: Offset::new(T::zero(), T::zero()),
                blur_radius: T::zero(),
                spread_radius: T::zero(),
                inset: false,
            });
            result.push(Self::lerp(a_shadow, b_shadow, t));
        }

        result
    }

    /// Scales the shadow's offset, blur radius, and spread radius by the given factor.
    #[inline]
    pub fn scale(&self, factor: f32) -> Self {
        Self {
            offset: self.offset * factor,
            blur_radius: self.blur_radius * factor,
            spread_radius: self.spread_radius * factor,
            ..*self
        }
    }
}

impl<T: Unit> From<Shadow<T>> for BoxShadow<T> {
    #[inline]
    fn from(shadow: Shadow<T>) -> Self {
        Self {
            color: shadow.color,
            offset: shadow.offset,
            blur_radius: shadow.blur_radius,
            spread_radius: T::zero(),
            inset: false,
        }
    }
}

impl<T: Unit> Default for BoxShadow<T> {
    #[inline]
    fn default() -> Self {
        Self {
            color: Color::BLACK,
            offset: Offset::new(T::zero(), T::zero()),
            blur_radius: T::zero(),
            spread_radius: T::zero(),
            inset: false,
        }
    }
}

/// Shadow rendering quality level.
///
/// Used by shadow rendering systems to control blur quality and performance.
/// Higher quality levels produce smoother shadows but require more computation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ShadowQuality {
    /// Low quality - single pass blur approximation
    Low,

    /// Medium quality - multi-pass blur (3 passes)
    #[default]
    Medium,

    /// High quality - high-quality gaussian blur (5+ passes)
    High,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::px;

    #[test]
    fn test_shadow_new() {
        let shadow = Shadow::new(Color::BLACK, Offset::new(px(2.0), px(3.0)), px(4.0));
        assert_eq!(shadow.color, Color::BLACK);
        assert_eq!(shadow.offset, Offset::new(px(2.0), px(3.0)));
        assert_eq!(shadow.blur_radius, px(4.0));
    }

    #[test]
    fn test_shadow_blur_sigma() {
        let shadow = Shadow::new(Color::BLACK, Offset::new(px(0.0), px(0.0)), px(10.0));
        let sigma = shadow.blur_sigma();
        assert!((sigma - 6.2735).abs() < 0.001);
    }

    #[test]
    fn test_shadow_with_methods() {
        let shadow = Shadow::<Pixels>::default();

        let colored = shadow.with_color(Color::RED);
        assert_eq!(colored.color, Color::RED);

        let offset = shadow.with_offset(Offset::new(px(5.0), px(5.0)));
        assert_eq!(offset.offset, Offset::new(px(5.0), px(5.0)));

        let blurred = shadow.with_blur_radius(px(10.0));
        assert_eq!(blurred.blur_radius, px(10.0));
    }

    #[test]
    fn test_shadow_lerp() {
        let a = Shadow::new(Color::BLACK, Offset::new(px(0.0), px(0.0)), px(0.0));
        let b = Shadow::new(Color::WHITE, Offset::new(px(10.0), px(10.0)), px(10.0));

        let mid = Shadow::lerp(a, b, 0.5);
        assert_eq!(mid.offset, Offset::new(px(5.0), px(5.0)));
        assert_eq!(mid.blur_radius, px(5.0));
    }

    #[test]
    fn test_shadow_lerp_list() {
        let a = vec![Shadow::new(
            Color::BLACK,
            Offset::new(px(0.0), px(0.0)),
            px(0.0),
        )];
        let b = vec![
            Shadow::new(Color::WHITE, Offset::new(px(10.0), px(10.0)), px(10.0)),
            Shadow::new(Color::RED, Offset::new(px(5.0), px(5.0)), px(5.0)),
        ];

        let result = Shadow::lerp_list(&a, &b, 0.5);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].offset, Offset::new(px(5.0), px(5.0)));
    }

    #[test]
    fn test_shadow_scale() {
        let shadow = Shadow::new(Color::BLACK, Offset::new(px(2.0), px(2.0)), px(4.0));
        let scaled = shadow.scale(2.0);
        assert_eq!(scaled.offset, Offset::new(px(4.0), px(4.0)));
        assert_eq!(scaled.blur_radius, px(8.0));
    }

    #[test]
    fn test_box_shadow_new() {
        let shadow = BoxShadow::new(
            Color::BLACK,
            Offset::new(px(2.0), px(3.0)),
            px(4.0),
            px(1.0),
        );
        assert_eq!(shadow.color, Color::BLACK);
        assert_eq!(shadow.offset, Offset::new(px(2.0), px(3.0)));
        assert_eq!(shadow.blur_radius, px(4.0));
        assert_eq!(shadow.spread_radius, px(1.0));
    }

    #[test]
    fn test_box_shadow_with_methods() {
        let shadow = BoxShadow::<Pixels>::default();

        let spread = shadow.with_spread_radius(px(5.0));
        assert_eq!(spread.spread_radius, px(5.0));
    }

    #[test]
    fn test_box_shadow_lerp() {
        let a = BoxShadow::new(
            Color::BLACK,
            Offset::new(px(0.0), px(0.0)),
            px(0.0),
            px(0.0),
        );
        let b = BoxShadow::new(
            Color::WHITE,
            Offset::new(px(10.0), px(10.0)),
            px(10.0),
            px(5.0),
        );

        let mid = BoxShadow::lerp(a, b, 0.5);
        assert_eq!(mid.offset, Offset::new(px(5.0), px(5.0)));
        assert_eq!(mid.blur_radius, px(5.0));
        assert_eq!(mid.spread_radius, px(2.5));
    }

    #[test]
    fn test_box_shadow_scale() {
        let shadow = BoxShadow::new(
            Color::BLACK,
            Offset::new(px(2.0), px(2.0)),
            px(4.0),
            px(2.0),
        );
        let scaled = shadow.scale(2.0);
        assert_eq!(scaled.offset, Offset::new(px(4.0), px(4.0)));
        assert_eq!(scaled.blur_radius, px(8.0));
        assert_eq!(scaled.spread_radius, px(4.0));
    }

    #[test]
    fn test_box_shadow_to_shadow() {
        let box_shadow =
            BoxShadow::new(Color::RED, Offset::new(px(1.0), px(2.0)), px(3.0), px(4.0));
        let shadow = box_shadow.to_shadow();
        assert_eq!(shadow.color, Color::RED);
        assert_eq!(shadow.offset, Offset::new(px(1.0), px(2.0)));
        assert_eq!(shadow.blur_radius, px(3.0));
    }

    #[test]
    fn test_shadow_to_box_shadow() {
        let shadow = Shadow::new(Color::BLUE, Offset::new(px(5.0), px(6.0)), px(7.0));
        let box_shadow: BoxShadow<Pixels> = shadow.into();
        assert_eq!(box_shadow.color, Color::BLUE);
        assert_eq!(box_shadow.offset, Offset::new(px(5.0), px(6.0)));
        assert_eq!(box_shadow.blur_radius, px(7.0));
        assert_eq!(box_shadow.spread_radius, px(0.0));
    }
}
