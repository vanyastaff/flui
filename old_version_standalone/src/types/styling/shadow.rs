//! Shadow types for widget effects
//!
//! This module contains types for representing shadows,
//! similar to Flutter's Shadow and BoxShadow system.

use crate::types::core::{Color, Offset};

/// The style of blur to use for shadows.
///
/// Similar to Flutter's `BlurStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlurStyle {
    /// Fuzzy inside and outside. This is the default.
    #[default]
    Normal,

    /// Solid inside, fuzzy outside.
    Solid,

    /// Nothing inside, fuzzy outside.
    Outer,

    /// Fuzzy inside, nothing outside.
    Inner,
}

/// A single shadow cast by a box.
///
/// Similar to Flutter's `Shadow`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadow {
    /// The color of the shadow.
    pub color: Color,

    /// The offset of the shadow from the casting element.
    ///
    /// Positive x offset moves the shadow to the right.
    /// Positive y offset moves the shadow down.
    pub offset: Offset,

    /// The standard deviation of the shadow's blur.
    ///
    /// A blur radius of 0.0 means a hard shadow with no blur.
    pub blur_radius: f32,
}

impl Shadow {
    /// Create a new shadow with the given properties.
    pub fn new(color: impl Into<Color>, offset: impl Into<Offset>, blur_radius: f32) -> Self {
        Self {
            color: color.into(),
            offset: offset.into(),
            blur_radius,
        }
    }

    /// Create a shadow with no offset or blur.
    pub fn simple(color: impl Into<Color>) -> Self {
        Self::new(color, Offset::ZERO, 0.0)
    }

    /// Create a default shadow (no shadow).
    pub const NONE: Self = Self {
        color: Color::TRANSPARENT,
        offset: Offset::ZERO,
        blur_radius: 0.0,
    };

    /// Check if this shadow is effectively invisible.
    pub fn is_none(&self) -> bool {
        self.color.is_transparent() || (self.blur_radius == 0.0 && self.offset == Offset::ZERO)
    }

    /// Scale the shadow's offset and blur radius.
    pub fn scale(&self, factor: f32) -> Self {
        Self {
            color: self.color,
            offset: self.offset * factor,
            blur_radius: self.blur_radius * factor,
        }
    }

    /// Create a copy with a different color.
    pub fn with_color(&self, color: impl Into<Color>) -> Self {
        Self {
            color: color.into(),
            offset: self.offset,
            blur_radius: self.blur_radius,
        }
    }

    /// Create a copy with a different offset.
    pub fn with_offset(&self, offset: impl Into<Offset>) -> Self {
        Self {
            color: self.color,
            offset: offset.into(),
            blur_radius: self.blur_radius,
        }
    }

    /// Create a copy with a different blur radius.
    pub fn with_blur_radius(&self, blur_radius: f32) -> Self {
        Self {
            color: self.color,
            offset: self.offset,
            blur_radius,
        }
    }

}

impl Default for Shadow {
    fn default() -> Self {
        Self::NONE
    }
}

impl From<Color> for Shadow {
    fn from(color: Color) -> Self {
        Self::simple(color)
    }
}


impl std::ops::Mul<f32> for Shadow {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        self.scale(rhs)
    }
}

/// A shadow cast by a box.
///
/// Similar to Flutter's `BoxShadow`, with additional spread radius.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxShadow {
    /// The color of the shadow.
    pub color: Color,

    /// The offset of the shadow from the casting box.
    pub offset: Offset,

    /// The standard deviation of the shadow's blur.
    pub blur_radius: f32,

    /// The amount the box should be inflated before applying the blur.
    ///
    /// Positive values make the shadow larger and lighter.
    /// Negative values make the shadow smaller and darker.
    pub spread_radius: f32,

    /// The blur style to use for this shadow.
    pub blur_style: BlurStyle,
}

impl BoxShadow {
    /// Create a new box shadow with all properties.
    pub fn new(
        color: impl Into<Color>,
        offset: impl Into<Offset>,
        blur_radius: f32,
        spread_radius: f32,
        blur_style: BlurStyle,
    ) -> Self {
        Self {
            color: color.into(),
            offset: offset.into(),
            blur_radius,
            spread_radius,
            blur_style,
        }
    }

    /// Create a simple box shadow without spread or special blur style.
    pub fn simple(color: impl Into<Color>, offset: impl Into<Offset>, blur_radius: f32) -> Self {
        Self::new(color, offset, blur_radius, 0.0, BlurStyle::Normal)
    }

    /// Create a box shadow with no offset or blur.
    pub const NONE: Self = Self {
        color: Color::TRANSPARENT,
        offset: Offset::ZERO,
        blur_radius: 0.0,
        spread_radius: 0.0,
        blur_style: BlurStyle::Normal,
    };

    /// Check if this shadow is effectively invisible.
    pub fn is_none(&self) -> bool {
        self.color.is_transparent() ||
        (self.blur_radius == 0.0 && self.spread_radius == 0.0 && self.offset == Offset::ZERO)
    }

    /// Scale the shadow's offset, blur radius, and spread radius.
    pub fn scale(&self, factor: f32) -> Self {
        Self {
            color: self.color,
            offset: self.offset * factor,
            blur_radius: self.blur_radius * factor,
            spread_radius: self.spread_radius * factor,
            blur_style: self.blur_style,
        }
    }

    /// Create a copy with a different color.
    pub fn with_color(&self, color: impl Into<Color>) -> Self {
        Self {
            color: color.into(),
            offset: self.offset,
            blur_radius: self.blur_radius,
            spread_radius: self.spread_radius,
            blur_style: self.blur_style,
        }
    }

    /// Create a copy with a different offset.
    pub fn with_offset(&self, offset: impl Into<Offset>) -> Self {
        Self {
            color: self.color,
            offset: offset.into(),
            blur_radius: self.blur_radius,
            spread_radius: self.spread_radius,
            blur_style: self.blur_style,
        }
    }

    /// Create a copy with a different blur radius.
    pub fn with_blur_radius(&self, blur_radius: f32) -> Self {
        Self {
            color: self.color,
            offset: self.offset,
            blur_radius,
            spread_radius: self.spread_radius,
            blur_style: self.blur_style,
        }
    }

    /// Create a copy with a different spread radius.
    pub fn with_spread_radius(&self, spread_radius: f32) -> Self {
        Self {
            color: self.color,
            offset: self.offset,
            blur_radius: self.blur_radius,
            spread_radius,
            blur_style: self.blur_style,
        }
    }

    /// Create a copy with a different blur style.
    pub fn with_blur_style(&self, blur_style: BlurStyle) -> Self {
        Self {
            color: self.color,
            offset: self.offset,
            blur_radius: self.blur_radius,
            spread_radius: self.spread_radius,
            blur_style,
        }
    }

    /// Convert to a basic Shadow (losing spread and blur style information).
    pub fn to_shadow(&self) -> Shadow {
        Shadow::new(self.color, self.offset, self.blur_radius)
    }


    /// Create a typical elevation shadow (like Material Design).
    ///
    /// Higher elevations create larger, softer shadows.
    pub fn elevation(elevation: f32, color: impl Into<Color>) -> Self {
        let blur = elevation.max(0.0);
        let offset_y = elevation.max(0.0) * 0.5;

        Self::simple(
            color,
            Offset::new(0.0, offset_y),
            blur,
        )
    }

    /// Create a set of layered shadows for elevation effect.
    ///
    /// Returns (key_shadow, ambient_shadow) like Material Design.
    pub fn elevation_shadows(elevation: f32) -> (Self, Self) {
        let key_color = Color::from_rgba(0, 0, 0, (0.14 * 255.0) as u8);
        let ambient_color = Color::from_rgba(0, 0, 0, (0.12 * 255.0) as u8);

        let key_blur = elevation;
        let key_offset = elevation * 0.5;
        let ambient_blur = elevation * 2.0;

        let key_shadow = Self::simple(
            key_color,
            Offset::new(0.0, key_offset),
            key_blur,
        );

        let ambient_shadow = Self::simple(
            ambient_color,
            Offset::ZERO,
            ambient_blur,
        );

        (key_shadow, ambient_shadow)
    }
}

impl Default for BoxShadow {
    fn default() -> Self {
        Self::NONE
    }
}

impl From<Shadow> for BoxShadow {
    fn from(shadow: Shadow) -> Self {
        Self::simple(shadow.color, shadow.offset, shadow.blur_radius)
    }
}

impl From<Color> for BoxShadow {
    fn from(color: Color) -> Self {
        Self::simple(color, Offset::ZERO, 0.0)
    }
}


impl std::ops::Mul<f32> for BoxShadow {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        self.scale(rhs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shadow_creation() {
        let shadow = Shadow::new(Color::BLACK, Offset::new(2.0, 2.0), 4.0);
        assert_eq!(shadow.color, Color::BLACK);
        assert_eq!(shadow.offset, Offset::new(2.0, 2.0));
        assert_eq!(shadow.blur_radius, 4.0);
        assert!(!shadow.is_none());

        let simple = Shadow::simple(Color::RED);
        assert_eq!(simple.color, Color::RED);
        assert_eq!(simple.offset, Offset::ZERO);
        assert_eq!(simple.blur_radius, 0.0);

        let none = Shadow::NONE;
        assert!(none.is_none());
    }

    #[test]
    fn test_shadow_modifications() {
        let shadow = Shadow::new(Color::BLACK, Offset::new(2.0, 2.0), 4.0);

        let with_color = shadow.with_color(Color::RED);
        assert_eq!(with_color.color, Color::RED);
        assert_eq!(with_color.offset, shadow.offset);

        let with_offset = shadow.with_offset(Offset::new(5.0, 5.0));
        assert_eq!(with_offset.offset, Offset::new(5.0, 5.0));

        let with_blur = shadow.with_blur_radius(8.0);
        assert_eq!(with_blur.blur_radius, 8.0);

        let scaled = shadow.scale(2.0);
        assert_eq!(scaled.offset, Offset::new(4.0, 4.0));
        assert_eq!(scaled.blur_radius, 8.0);
    }

    #[test]
    fn test_shadow_conversions() {
        let from_color: Shadow = Color::RED.into();
        assert_eq!(from_color.color, Color::RED);
    }

    #[test]
    fn test_box_shadow_creation() {
        let shadow = BoxShadow::new(
            Color::BLACK,
            Offset::new(2.0, 2.0),
            4.0,
            1.0,
            BlurStyle::Normal,
        );
        assert_eq!(shadow.color, Color::BLACK);
        assert_eq!(shadow.offset, Offset::new(2.0, 2.0));
        assert_eq!(shadow.blur_radius, 4.0);
        assert_eq!(shadow.spread_radius, 1.0);
        assert_eq!(shadow.blur_style, BlurStyle::Normal);
        assert!(!shadow.is_none());

        let simple = BoxShadow::simple(Color::RED, Offset::new(1.0, 1.0), 2.0);
        assert_eq!(simple.color, Color::RED);
        assert_eq!(simple.spread_radius, 0.0);

        let none = BoxShadow::NONE;
        assert!(none.is_none());
    }

    #[test]
    fn test_box_shadow_modifications() {
        let shadow = BoxShadow::simple(Color::BLACK, Offset::new(2.0, 2.0), 4.0);

        let with_color = shadow.with_color(Color::RED);
        assert_eq!(with_color.color, Color::RED);

        let with_offset = shadow.with_offset(Offset::new(5.0, 5.0));
        assert_eq!(with_offset.offset, Offset::new(5.0, 5.0));

        let with_blur = shadow.with_blur_radius(8.0);
        assert_eq!(with_blur.blur_radius, 8.0);

        let with_spread = shadow.with_spread_radius(2.0);
        assert_eq!(with_spread.spread_radius, 2.0);

        let with_style = shadow.with_blur_style(BlurStyle::Outer);
        assert_eq!(with_style.blur_style, BlurStyle::Outer);

        let scaled = shadow.scale(2.0);
        assert_eq!(scaled.offset, Offset::new(4.0, 4.0));
        assert_eq!(scaled.blur_radius, 8.0);
    }

    #[test]
    fn test_box_shadow_conversions() {
        let from_color: BoxShadow = Color::RED.into();
        assert_eq!(from_color.color, Color::RED);

        let shadow = Shadow::new(Color::BLACK, Offset::new(2.0, 2.0), 4.0);
        let box_shadow: BoxShadow = shadow.into();
        assert_eq!(box_shadow.color, shadow.color);
        assert_eq!(box_shadow.offset, shadow.offset);
        assert_eq!(box_shadow.blur_radius, shadow.blur_radius);
        assert_eq!(box_shadow.spread_radius, 0.0);

        let back = box_shadow.to_shadow();
        assert_eq!(back.color, shadow.color);
        assert_eq!(back.offset, shadow.offset);
    }

    #[test]
    fn test_box_shadow_elevation() {
        let elevation = BoxShadow::elevation(4.0, Color::from_rgba(0, 0, 0, 50));
        assert_eq!(elevation.blur_radius, 4.0);
        assert_eq!(elevation.offset.dy, 2.0); // elevation * 0.5
        assert_eq!(elevation.offset.dx, 0.0);

        let (key, ambient) = BoxShadow::elevation_shadows(8.0);
        assert!(key.blur_radius > 0.0);
        assert!(ambient.blur_radius > key.blur_radius);
        assert_eq!(ambient.offset, Offset::ZERO);
    }

    #[test]
    fn test_blur_style() {
        assert_eq!(BlurStyle::default(), BlurStyle::Normal);
    }

}
