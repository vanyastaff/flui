//! Spacing constants and types for consistent spacing
//!
//! This module provides a standardized spacing scale,
//! similar to design systems like Material Design or Tailwind CSS.

/// A standardized spacing scale.
///
/// Provides predefined spacing values for consistent layouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Spacing {
    /// No spacing (0px)
    None,
    /// Extra extra small spacing (2px)
    XXS,
    /// Extra small spacing (4px)
    XS,
    /// Small spacing (8px)
    SM,
    /// Medium spacing (12px)
    MD,
    /// Large spacing (16px)
    LG,
    /// Extra large spacing (24px)
    XL,
    /// Extra extra large spacing (32px)
    XXL,
    /// Extra extra extra large spacing (48px)
    XXXL,
    /// Custom spacing value
    Custom(u16),
}

impl Spacing {
    /// Convert spacing to pixels as f32.
    pub fn to_pixels(&self) -> f32 {
        match self {
            Spacing::None => 0.0,
            Spacing::XXS => 2.0,
            Spacing::XS => 4.0,
            Spacing::SM => 8.0,
            Spacing::MD => 12.0,
            Spacing::LG => 16.0,
            Spacing::XL => 24.0,
            Spacing::XXL => 32.0,
            Spacing::XXXL => 48.0,
            Spacing::Custom(value) => *value as f32,
        }
    }

    /// Create spacing from pixels (rounds to nearest spacing value).
    pub fn from_pixels(pixels: f32) -> Self {
        match pixels as u16 {
            0 => Spacing::None,
            1..=3 => Spacing::XXS,
            4..=6 => Spacing::XS,
            7..=10 => Spacing::SM,
            11..=14 => Spacing::MD,
            15..=20 => Spacing::LG,
            21..=28 => Spacing::XL,
            29..=40 => Spacing::XXL,
            41..=55 => Spacing::XXXL,
            value => Spacing::Custom(value),
        }
    }

    /// Get the next larger spacing value.
    pub fn larger(&self) -> Self {
        match self {
            Spacing::None => Spacing::XXS,
            Spacing::XXS => Spacing::XS,
            Spacing::XS => Spacing::SM,
            Spacing::SM => Spacing::MD,
            Spacing::MD => Spacing::LG,
            Spacing::LG => Spacing::XL,
            Spacing::XL => Spacing::XXL,
            Spacing::XXL => Spacing::XXXL,
            Spacing::XXXL => Spacing::XXXL,
            Spacing::Custom(v) => Spacing::Custom(*v + 8),
        }
    }

    /// Get the next smaller spacing value.
    pub fn smaller(&self) -> Self {
        match self {
            Spacing::None => Spacing::None,
            Spacing::XXS => Spacing::None,
            Spacing::XS => Spacing::XXS,
            Spacing::SM => Spacing::XS,
            Spacing::MD => Spacing::SM,
            Spacing::LG => Spacing::MD,
            Spacing::XL => Spacing::LG,
            Spacing::XXL => Spacing::XL,
            Spacing::XXXL => Spacing::XXL,
            Spacing::Custom(v) => Spacing::Custom(v.saturating_sub(8)),
        }
    }

    /// Scale the spacing by a factor.
    pub fn scale(&self, factor: f32) -> Self {
        let pixels = self.to_pixels() * factor;
        Self::from_pixels(pixels)
    }
}

impl Default for Spacing {
    fn default() -> Self {
        Spacing::MD
    }
}

impl From<Spacing> for f32 {
    fn from(spacing: Spacing) -> Self {
        spacing.to_pixels()
    }
}

impl From<f32> for Spacing {
    fn from(pixels: f32) -> Self {
        Spacing::from_pixels(pixels)
    }
}

impl std::fmt::Display for Spacing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Spacing::None => write!(f, "none (0px)"),
            Spacing::XXS => write!(f, "xxs (2px)"),
            Spacing::XS => write!(f, "xs (4px)"),
            Spacing::SM => write!(f, "sm (8px)"),
            Spacing::MD => write!(f, "md (12px)"),
            Spacing::LG => write!(f, "lg (16px)"),
            Spacing::XL => write!(f, "xl (24px)"),
            Spacing::XXL => write!(f, "xxl (32px)"),
            Spacing::XXXL => write!(f, "xxxl (48px)"),
            Spacing::Custom(v) => write!(f, "custom ({}px)", v),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spacing_to_pixels() {
        assert_eq!(Spacing::None.to_pixels(), 0.0);
        assert_eq!(Spacing::XXS.to_pixels(), 2.0);
        assert_eq!(Spacing::XS.to_pixels(), 4.0);
        assert_eq!(Spacing::SM.to_pixels(), 8.0);
        assert_eq!(Spacing::MD.to_pixels(), 12.0);
        assert_eq!(Spacing::LG.to_pixels(), 16.0);
        assert_eq!(Spacing::XL.to_pixels(), 24.0);
        assert_eq!(Spacing::XXL.to_pixels(), 32.0);
        assert_eq!(Spacing::XXXL.to_pixels(), 48.0);
        assert_eq!(Spacing::Custom(100).to_pixels(), 100.0);
    }

    #[test]
    fn test_spacing_from_pixels() {
        assert_eq!(Spacing::from_pixels(0.0), Spacing::None);
        assert_eq!(Spacing::from_pixels(2.0), Spacing::XXS);
        assert_eq!(Spacing::from_pixels(4.0), Spacing::XS);
        assert_eq!(Spacing::from_pixels(8.0), Spacing::SM);
        assert_eq!(Spacing::from_pixels(12.0), Spacing::MD);
        assert_eq!(Spacing::from_pixels(16.0), Spacing::LG);
        assert_eq!(Spacing::from_pixels(24.0), Spacing::XL);
        assert_eq!(Spacing::from_pixels(32.0), Spacing::XXL);
        assert_eq!(Spacing::from_pixels(48.0), Spacing::XXXL);
        assert_eq!(Spacing::from_pixels(100.0), Spacing::Custom(100));
    }

    #[test]
    fn test_spacing_larger_smaller() {
        assert_eq!(Spacing::None.larger(), Spacing::XXS);
        assert_eq!(Spacing::XS.larger(), Spacing::SM);
        assert_eq!(Spacing::XXL.larger(), Spacing::XXXL);
        assert_eq!(Spacing::XXXL.larger(), Spacing::XXXL);

        assert_eq!(Spacing::None.smaller(), Spacing::None);
        assert_eq!(Spacing::XXS.smaller(), Spacing::None);
        assert_eq!(Spacing::SM.smaller(), Spacing::XS);
    }

    #[test]
    fn test_spacing_scale() {
        let spacing = Spacing::MD; // 12px
        let scaled = spacing.scale(2.0); // 24px -> XL
        assert_eq!(scaled.to_pixels(), 24.0);

        let scaled_down = spacing.scale(0.5); // 6px -> rounds to XS (4px)
        assert_eq!(scaled_down.to_pixels(), 4.0);
    }

    #[test]
    fn test_spacing_conversions() {
        let from_f32: Spacing = 12.0.into();
        assert_eq!(from_f32, Spacing::MD);

        let to_f32: f32 = Spacing::LG.into();
        assert_eq!(to_f32, 16.0);
    }

    #[test]
    fn test_spacing_display() {
        assert_eq!(format!("{}", Spacing::MD), "md (12px)");
        assert_eq!(format!("{}", Spacing::Custom(50)), "custom (50px)");
    }
}
