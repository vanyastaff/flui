//! Brightness settings for theming

use crate::styling::Color;

#[derive(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Brightness {
    #[default]
    Light,

    /// Dark theme (light text on dark background)
    Dark,
}

impl Brightness {
    #[must_use]
    #[inline]
    pub const fn is_light(&self) -> bool {
        matches!(self, Self::Light)
    }

    #[must_use]
    #[inline]
    pub const fn is_dark(&self) -> bool {
        matches!(self, Self::Dark)
    }

    #[must_use]
    #[inline]
    pub const fn invert(&self) -> Self {
        match self {
            Self::Light => Self::Dark,
            Self::Dark => Self::Light,
        }
    }

    #[must_use]
    #[inline]
    pub const fn background_color(&self) -> Color {
        match self {
            Self::Light => Color::rgba(255, 255, 255, 255), // White
            Self::Dark => Color::rgba(18, 18, 18, 255),     // Near black
        }
    }

    #[must_use]
    #[inline]
    pub const fn foreground_color(&self) -> Color {
        match self {
            Self::Light => Color::rgba(0, 0, 0, 255),      // Black
            Self::Dark => Color::rgba(255, 255, 255, 255), // White
        }
    }

    #[must_use]
    #[inline]
    pub const fn surface_color(&self) -> Color {
        match self {
            Self::Light => Color::rgba(255, 255, 255, 255), // White
            Self::Dark => Color::rgba(30, 30, 30, 255),     // Dark gray
        }
    }

    #[must_use]
    #[inline]
    pub const fn shadow_opacity(&self) -> f32 {
        match self {
            Self::Light => 0.2,
            Self::Dark => 0.4,
        }
    }

    #[must_use]
    #[inline]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }

    #[must_use]
    #[inline]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}
