//! Shadow types for styling

use crate::geometry::Offset;
use crate::styling::Color;

/// A single shadow cast by a shape.
///
/// Similar to Flutter's `Shadow`.
///
/// # Examples
///
/// ```
/// use flui_types::styling::{Shadow, Color};
/// use flui_types::geometry::Offset;
///
/// // Simple shadow with default blur
/// let shadow = Shadow::new(Color::BLACK, Offset::new(2.0, 2.0), 4.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Shadow {
    /// The color of the shadow.
    pub color: Color,

    /// The displacement of the shadow from the casting element.
    pub offset: Offset,

    /// The standard deviation of the Gaussian to convolve with the shadow's shape.
    ///
    /// A blur radius of 0.0 means the shadow has a sharp edge.
    pub blur_radius: f32,
}

impl Shadow {
    /// Creates a new shadow.
    ///
    /// # Arguments
    ///
    /// * `color` - The color of the shadow
    /// * `offset` - The offset of the shadow from the element
    /// * `blur_radius` - The blur radius of the shadow (0.0 for sharp edges)
    pub const fn new(color: Color, offset: Offset, blur_radius: f32) -> Self {
        Self {
            color,
            offset,
            blur_radius,
        }
    }

    /// Converts a blur radius in pixels to sigma units for use in Gaussian blur.
    ///
    /// This follows the same conversion that Flutter uses.
    pub fn convert_radius_to_sigma(radius: f32) -> f32 {
        radius * 0.57735 + 0.5
    }

    /// The standard deviation of the Gaussian blur to apply to the shadow.
    pub fn blur_sigma(&self) -> f32 {
        Self::convert_radius_to_sigma(self.blur_radius)
    }

    /// Creates a copy of this shadow with the given color.
    pub const fn with_color(self, color: Color) -> Self {
        Self { color, ..self }
    }

    /// Creates a copy of this shadow with the given offset.
    pub const fn with_offset(self, offset: Offset) -> Self {
        Self { offset, ..self }
    }

    /// Creates a copy of this shadow with the given blur radius.
    pub const fn with_blur_radius(self, blur_radius: f32) -> Self {
        Self { blur_radius, ..self }
    }

    /// Linearly interpolate between two shadows.
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            color: Color::lerp(a.color, b.color, t),
            offset: Offset::lerp(a.offset, b.offset, t),
            blur_radius: a.blur_radius + (b.blur_radius - a.blur_radius) * t,
        }
    }

    /// Linearly interpolate between two lists of shadows.
    ///
    /// If the lists are different lengths, the shorter list is padded with
    /// transparent shadows at offset zero with zero blur.
    pub fn lerp_list(a: &[Self], b: &[Self], t: f32) -> Vec<Self> {
        let t = t.clamp(0.0, 1.0);
        let max_len = a.len().max(b.len());
        let mut result = Vec::with_capacity(max_len);

        for i in 0..max_len {
            let a_shadow = a.get(i).copied().unwrap_or(Self {
                color: Color::TRANSPARENT,
                offset: Offset::ZERO,
                blur_radius: 0.0,
            });
            let b_shadow = b.get(i).copied().unwrap_or(Self {
                color: Color::TRANSPARENT,
                offset: Offset::ZERO,
                blur_radius: 0.0,
            });
            result.push(Self::lerp(a_shadow, b_shadow, t));
        }

        result
    }

    /// Scales the shadow's offset and blur radius by the given factor.
    pub fn scale(&self, factor: f32) -> Self {
        Self {
            offset: self.offset * factor,
            blur_radius: self.blur_radius * factor,
            ..*self
        }
    }
}

impl Default for Shadow {
    fn default() -> Self {
        Self {
            color: Color::BLACK,
            offset: Offset::ZERO,
            blur_radius: 0.0,
        }
    }
}

/// A shadow cast by a box.
///
/// Similar to Flutter's `BoxShadow`.
///
/// BoxShadow extends Shadow with a spread radius, which causes the shadow to
/// expand or contract before being blurred. It also supports inner shadows.
///
/// # Examples
///
/// ```
/// use flui_types::styling::{BoxShadow, Color};
/// use flui_types::geometry::Offset;
///
/// // Box shadow with spread
/// let shadow = BoxShadow::new(
///     Color::BLACK.with_alpha(76), // 0.3 * 255 ≈ 76
///     Offset::new(0.0, 4.0),
///     8.0,
///     2.0,
/// );
///
/// // Inner shadow (like CSS inset)
/// let inner = BoxShadow::new(
///     Color::BLACK.with_alpha(51), // 0.2 * 255 ≈ 51
///     Offset::new(0.0, 2.0),
///     4.0,
///     0.0,
/// ).with_inset(true);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BoxShadow {
    /// The color of the shadow.
    pub color: Color,

    /// The displacement of the shadow from the casting element.
    pub offset: Offset,

    /// The standard deviation of the Gaussian to convolve with the shadow's shape.
    pub blur_radius: f32,

    /// The amount the box should be inflated prior to applying the blur.
    ///
    /// Positive values make the shadow larger, negative values make it smaller.
    pub spread_radius: f32,

    /// Whether this is an inner shadow (rendered inside the box).
    ///
    /// Similar to CSS `inset` keyword in `box-shadow`.
    pub inset: bool,
}

impl BoxShadow {
    /// Creates a new box shadow.
    ///
    /// # Arguments
    ///
    /// * `color` - The color of the shadow
    /// * `offset` - The offset of the shadow from the element
    /// * `blur_radius` - The blur radius of the shadow
    /// * `spread_radius` - The spread radius of the shadow
    pub const fn new(color: Color, offset: Offset, blur_radius: f32, spread_radius: f32) -> Self {
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
    pub const fn inner(color: Color, offset: Offset, blur_radius: f32, spread_radius: f32) -> Self {
        Self {
            color,
            offset,
            blur_radius,
            spread_radius,
            inset: true,
        }
    }

    /// Converts a blur radius in pixels to sigma units for use in Gaussian blur.
    pub fn convert_radius_to_sigma(radius: f32) -> f32 {
        Shadow::convert_radius_to_sigma(radius)
    }

    /// The standard deviation of the Gaussian blur to apply to the shadow.
    pub fn blur_sigma(&self) -> f32 {
        Self::convert_radius_to_sigma(self.blur_radius)
    }

    /// Creates a copy of this box shadow with the given color.
    pub const fn with_color(self, color: Color) -> Self {
        Self { color, ..self }
    }

    /// Creates a copy of this box shadow with the given offset.
    pub const fn with_offset(self, offset: Offset) -> Self {
        Self { offset, ..self }
    }

    /// Creates a copy of this box shadow with the given blur radius.
    pub const fn with_blur_radius(self, blur_radius: f32) -> Self {
        Self { blur_radius, ..self }
    }

    /// Creates a copy of this box shadow with the given spread radius.
    pub const fn with_spread_radius(self, spread_radius: f32) -> Self {
        Self {
            spread_radius,
            ..self
        }
    }

    /// Creates a copy of this box shadow with the given inset value.
    pub const fn with_inset(self, inset: bool) -> Self {
        Self { inset, ..self }
    }

    /// Linearly interpolate between two box shadows.
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            color: Color::lerp(a.color, b.color, t),
            offset: Offset::lerp(a.offset, b.offset, t),
            blur_radius: a.blur_radius + (b.blur_radius - a.blur_radius) * t,
            spread_radius: a.spread_radius + (b.spread_radius - a.spread_radius) * t,
            inset: if t < 0.5 { a.inset } else { b.inset },
        }
    }

    /// Linearly interpolate between two lists of box shadows.
    pub fn lerp_list(a: &[Self], b: &[Self], t: f32) -> Vec<Self> {
        let t = t.clamp(0.0, 1.0);
        let max_len = a.len().max(b.len());
        let mut result = Vec::with_capacity(max_len);

        for i in 0..max_len {
            let a_shadow = a.get(i).copied().unwrap_or(Self {
                color: Color::TRANSPARENT,
                offset: Offset::ZERO,
                blur_radius: 0.0,
                spread_radius: 0.0,
                inset: false,
            });
            let b_shadow = b.get(i).copied().unwrap_or(Self {
                color: Color::TRANSPARENT,
                offset: Offset::ZERO,
                blur_radius: 0.0,
                spread_radius: 0.0,
                inset: false,
            });
            result.push(Self::lerp(a_shadow, b_shadow, t));
        }

        result
    }

    /// Scales the shadow's offset, blur radius, and spread radius by the given factor.
    pub fn scale(&self, factor: f32) -> Self {
        Self {
            offset: self.offset * factor,
            blur_radius: self.blur_radius * factor,
            spread_radius: self.spread_radius * factor,
            ..*self
        }
    }

    /// Converts this BoxShadow to a Shadow (losing the spread radius).
    pub const fn to_shadow(self) -> Shadow {
        Shadow {
            color: self.color,
            offset: self.offset,
            blur_radius: self.blur_radius,
        }
    }
}

impl From<Shadow> for BoxShadow {
    fn from(shadow: Shadow) -> Self {
        Self {
            color: shadow.color,
            offset: shadow.offset,
            blur_radius: shadow.blur_radius,
            spread_radius: 0.0,
            inset: false,
        }
    }
}

impl Default for BoxShadow {
    fn default() -> Self {
        Self {
            color: Color::BLACK,
            offset: Offset::ZERO,
            blur_radius: 0.0,
            spread_radius: 0.0,
            inset: false,
        }
    }
}

/// Shadow rendering quality level.
///
/// Used by shadow rendering systems to control blur quality and performance.
/// Higher quality levels produce smoother shadows but require more computation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ShadowQuality {
    /// Low quality - single pass blur approximation
    Low,

    /// Medium quality - multi-pass blur (3 passes)
    Medium,

    /// High quality - high-quality gaussian blur (5+ passes)
    High,
}

impl Default for ShadowQuality {
    fn default() -> Self {
        Self::Medium
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shadow_new() {
        let shadow = Shadow::new(Color::BLACK, Offset::new(2.0, 3.0), 4.0);
        assert_eq!(shadow.color, Color::BLACK);
        assert_eq!(shadow.offset, Offset::new(2.0, 3.0));
        assert_eq!(shadow.blur_radius, 4.0);
    }

    #[test]
    fn test_shadow_blur_sigma() {
        let shadow = Shadow::new(Color::BLACK, Offset::ZERO, 10.0);
        let sigma = shadow.blur_sigma();
        // sigma = radius * 0.57735 + 0.5
        assert!((sigma - 6.2735).abs() < 0.001);
    }

    #[test]
    fn test_shadow_with_methods() {
        let shadow = Shadow::default();

        let colored = shadow.with_color(Color::RED);
        assert_eq!(colored.color, Color::RED);

        let offset = shadow.with_offset(Offset::new(5.0, 5.0));
        assert_eq!(offset.offset, Offset::new(5.0, 5.0));

        let blurred = shadow.with_blur_radius(10.0);
        assert_eq!(blurred.blur_radius, 10.0);
    }

    #[test]
    fn test_shadow_lerp() {
        let a = Shadow::new(Color::BLACK, Offset::new(0.0, 0.0), 0.0);
        let b = Shadow::new(Color::WHITE, Offset::new(10.0, 10.0), 10.0);

        let mid = Shadow::lerp(a, b, 0.5);
        assert_eq!(mid.offset, Offset::new(5.0, 5.0));
        assert_eq!(mid.blur_radius, 5.0);
    }

    #[test]
    fn test_shadow_lerp_list() {
        let a = vec![
            Shadow::new(Color::BLACK, Offset::ZERO, 0.0),
        ];
        let b = vec![
            Shadow::new(Color::WHITE, Offset::new(10.0, 10.0), 10.0),
            Shadow::new(Color::RED, Offset::new(5.0, 5.0), 5.0),
        ];

        let result = Shadow::lerp_list(&a, &b, 0.5);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].offset, Offset::new(5.0, 5.0));
    }

    #[test]
    fn test_shadow_scale() {
        let shadow = Shadow::new(Color::BLACK, Offset::new(2.0, 2.0), 4.0);
        let scaled = shadow.scale(2.0);
        assert_eq!(scaled.offset, Offset::new(4.0, 4.0));
        assert_eq!(scaled.blur_radius, 8.0);
    }

    #[test]
    fn test_box_shadow_new() {
        let shadow = BoxShadow::new(Color::BLACK, Offset::new(2.0, 3.0), 4.0, 1.0);
        assert_eq!(shadow.color, Color::BLACK);
        assert_eq!(shadow.offset, Offset::new(2.0, 3.0));
        assert_eq!(shadow.blur_radius, 4.0);
        assert_eq!(shadow.spread_radius, 1.0);
    }

    #[test]
    fn test_box_shadow_with_methods() {
        let shadow = BoxShadow::default();

        let spread = shadow.with_spread_radius(5.0);
        assert_eq!(spread.spread_radius, 5.0);
    }

    #[test]
    fn test_box_shadow_lerp() {
        let a = BoxShadow::new(Color::BLACK, Offset::ZERO, 0.0, 0.0);
        let b = BoxShadow::new(Color::WHITE, Offset::new(10.0, 10.0), 10.0, 5.0);

        let mid = BoxShadow::lerp(a, b, 0.5);
        assert_eq!(mid.offset, Offset::new(5.0, 5.0));
        assert_eq!(mid.blur_radius, 5.0);
        assert_eq!(mid.spread_radius, 2.5);
    }

    #[test]
    fn test_box_shadow_scale() {
        let shadow = BoxShadow::new(Color::BLACK, Offset::new(2.0, 2.0), 4.0, 2.0);
        let scaled = shadow.scale(2.0);
        assert_eq!(scaled.offset, Offset::new(4.0, 4.0));
        assert_eq!(scaled.blur_radius, 8.0);
        assert_eq!(scaled.spread_radius, 4.0);
    }

    #[test]
    fn test_box_shadow_to_shadow() {
        let box_shadow = BoxShadow::new(Color::RED, Offset::new(1.0, 2.0), 3.0, 4.0);
        let shadow = box_shadow.to_shadow();
        assert_eq!(shadow.color, Color::RED);
        assert_eq!(shadow.offset, Offset::new(1.0, 2.0));
        assert_eq!(shadow.blur_radius, 3.0);
    }

    #[test]
    fn test_shadow_to_box_shadow() {
        let shadow = Shadow::new(Color::BLUE, Offset::new(5.0, 6.0), 7.0);
        let box_shadow: BoxShadow = shadow.into();
        assert_eq!(box_shadow.color, Color::BLUE);
        assert_eq!(box_shadow.offset, Offset::new(5.0, 6.0));
        assert_eq!(box_shadow.blur_radius, 7.0);
        assert_eq!(box_shadow.spread_radius, 0.0);
    }
}
