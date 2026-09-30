//! Shadow types for styling

use crate::styling::Color;
use flui_foundation::geometry::{
    Offset,
    traits::{NumericUnit, Unit},
};

/// A single shadow cast by a shape.
///
/// Generic over unit type `T` for full type safety. Use `Shadow<f64>` for UI
/// shadows.
///
/// # Examples
///
/// ```
/// use flui_foundation::geometry::Offset;
/// use flui_painting::styling::{Color, Shadow};
///
/// let shadow = Shadow::new(Color::BLACK, Offset::new(2.0, 2.0), 4.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Shadow<T: Unit> {
    /// The color of the shadow.
    pub color: Color,

    /// The displacement of the shadow from the casting element.
    pub offset: Offset<T>,

    /// The standard deviation of the Gaussian to convolve with the shadow's
    /// shape.
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

impl Shadow<f64> {
    /// Converts a blur radius in pixels to sigma units for use in Gaussian
    /// blur.
    ///
    /// Uses the Skia blur-radius to sigma conversion (`radius * 0.57735 + 0.5`).
    #[inline]
    pub fn convert_radius_to_sigma(radius: f64) -> f64 {
        radius * 0.57735 + 0.5
    }

    /// The standard deviation of the Gaussian blur to apply to the shadow.
    #[inline]
    pub fn blur_sigma(&self) -> f64 {
        Self::convert_radius_to_sigma(self.blur_radius)
    }
}

impl<T: NumericUnit> Shadow<T>
where
    T: std::ops::Mul<f64, Output = T>,
{
    /// Linearly interpolate between two shadows.
    #[inline]
    pub fn lerp(a: Self, b: Self, t: f64) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            color: Color::lerp(a.color, b.color, t),
            offset: flui_foundation::geometry::Offset::new(
                a.offset.dx * (1.0 - t) + b.offset.dx * t,
                a.offset.dy * (1.0 - t) + b.offset.dy * t,
            ),
            blur_radius: a.blur_radius * (1.0 - t) + b.blur_radius * t,
        }
    }

    /// Linearly interpolate between two lists of shadows: pairs lerp; a
    /// shadow only one list has keeps its color and has its geometry scaled
    /// by `1 - t` (from `a`) or `t` (from `b`).
    #[inline]
    pub fn lerp_list(a: &[Self], b: &[Self], t: f64) -> Vec<Self> {
        let t = t.clamp(0.0, 1.0);
        let common = a.len().min(b.len());
        a.iter()
            .zip(b)
            .map(|(a_shadow, b_shadow)| Self::lerp(*a_shadow, *b_shadow, t))
            .chain(a[common..].iter().map(|extra| extra.scale(1.0 - t)))
            .chain(b[common..].iter().map(|extra| extra.scale(t)))
            .collect()
    }

    /// Scales the shadow's offset and blur radius by the given factor.
    #[inline]
    pub fn scale(&self, factor: f64) -> Self {
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
/// Generic over unit type `T` for full type safety. Use `BoxShadow<f64>` for
/// UI shadows.
///
/// BoxShadow extends Shadow with a spread radius, which causes the shadow to
/// expand or contract before being blurred. It also supports inner shadows.
///
/// # Examples
///
/// ```
/// use flui_foundation::geometry::Offset;
/// use flui_painting::styling::{BoxShadow, Color};
///
/// // Standard drop shadow
/// let shadow = BoxShadow::new(
///     Color::BLACK.with_alpha(76),
///     Offset::new(0.0, 4.0),
///     8.0,
///     2.0,
/// );
///
/// // Inner shadow (like CSS inset)
/// let inner = BoxShadow::inner(
///     Color::BLACK.with_alpha(51),
///     Offset::new(0.0, 2.0),
///     4.0,
///     0.0,
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BoxShadow<T: Unit> {
    /// The color of the shadow.
    pub color: Color,

    /// The displacement of the shadow from the casting element.
    pub offset: Offset<T>,

    /// The standard deviation of the Gaussian to convolve with the shadow's
    /// shape.
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

impl BoxShadow<f64> {
    /// Converts a blur radius in pixels to sigma units for use in Gaussian
    /// blur.
    #[inline]
    pub fn convert_radius_to_sigma(radius: f64) -> f64 {
        Shadow::<f64>::convert_radius_to_sigma(radius)
    }

    /// The standard deviation of the Gaussian blur to apply to the shadow.
    #[inline]
    pub fn blur_sigma(&self) -> f64 {
        Self::convert_radius_to_sigma(self.blur_radius)
    }
}

impl<T: NumericUnit> BoxShadow<T>
where
    T: std::ops::Mul<f64, Output = T>,
{
    /// Linearly interpolate between two box shadows.
    #[inline]
    pub fn lerp(a: Self, b: Self, t: f64) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            color: Color::lerp(a.color, b.color, t),
            offset: flui_foundation::geometry::Offset::new(
                a.offset.dx * (1.0 - t) + b.offset.dx * t,
                a.offset.dy * (1.0 - t) + b.offset.dy * t,
            ),
            blur_radius: a.blur_radius * (1.0 - t) + b.blur_radius * t,
            spread_radius: a.spread_radius * (1.0 - t) + b.spread_radius * t,
            inset: if t < 0.5 { a.inset } else { b.inset },
        }
    }

    /// Linearly interpolate between two lists of box shadows: pairs lerp; a
    /// shadow only one list has keeps its color and has its geometry scaled
    /// by `1 - t` (from `a`) or `t` (from `b`).
    #[inline]
    pub fn lerp_list(a: &[Self], b: &[Self], t: f64) -> Vec<Self> {
        let t = t.clamp(0.0, 1.0);
        let common = a.len().min(b.len());
        a.iter()
            .zip(b)
            .map(|(a_shadow, b_shadow)| Self::lerp(*a_shadow, *b_shadow, t))
            .chain(a[common..].iter().map(|extra| extra.scale(1.0 - t)))
            .chain(b[common..].iter().map(|extra| extra.scale(t)))
            .collect()
    }

    /// Scales the shadow's offset, blur radius, and spread radius by the given
    /// factor.
    #[inline]
    pub fn scale(&self, factor: f64) -> Self {
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
