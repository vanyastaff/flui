//! [`Brightness`] — light/dark theme preference.

use crate::styling::Color;

/// Whether the ambient theme is visually light or dark.
///
/// Mirrors Flutter's `Brightness` enum. Used by `MediaQueryData`
/// (platform OS preference) and `ThemeData` (app-level override).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Brightness {
    /// Light theme: dark text on a light background.
    #[default]
    Light,
    /// Dark theme: light text on a dark background.
    Dark,
}

impl Brightness {
    /// Returns `true` if this is `Brightness::Light`.
    #[must_use]
    #[inline]
    pub const fn is_light(&self) -> bool {
        matches!(self, Self::Light)
    }

    /// Returns `true` if this is `Brightness::Dark`.
    #[must_use]
    #[inline]
    pub const fn is_dark(&self) -> bool {
        matches!(self, Self::Dark)
    }

    /// Returns the opposite brightness (`Light` ↔ `Dark`).
    #[must_use]
    #[inline]
    pub const fn invert(&self) -> Self {
        match self {
            Self::Light => Self::Dark,
            Self::Dark => Self::Light,
        }
    }

    /// Returns the default background color for this brightness:
    /// white for `Light`, near-black for `Dark`.
    #[must_use]
    #[inline]
    pub const fn background_color(&self) -> Color {
        match self {
            Self::Light => Color::rgba(255, 255, 255, 255), // White
            Self::Dark => Color::rgba(18, 18, 18, 255),     // Near black
        }
    }

    /// Returns the default foreground (text) color for this brightness:
    /// black for `Light`, white for `Dark`.
    #[must_use]
    #[inline]
    pub const fn foreground_color(&self) -> Color {
        match self {
            Self::Light => Color::rgba(0, 0, 0, 255),      // Black
            Self::Dark => Color::rgba(255, 255, 255, 255), // White
        }
    }

    /// Returns the default surface color (cards, sheets) for this
    /// brightness: white for `Light`, dark gray for `Dark`.
    #[must_use]
    #[inline]
    pub const fn surface_color(&self) -> Color {
        match self {
            Self::Light => Color::rgba(255, 255, 255, 255), // White
            Self::Dark => Color::rgba(30, 30, 30, 255),     // Dark gray
        }
    }

    /// Returns the default shadow opacity for this brightness.
    ///
    /// Dark themes use a stronger shadow (0.4 vs 0.2) so elevation
    /// stays legible against dark surfaces.
    #[must_use]
    #[inline]
    pub const fn shadow_opacity(&self) -> f32 {
        match self {
            Self::Light => 0.2,
            Self::Dark => 0.4,
        }
    }

    /// Parses a brightness from `"light"` or `"dark"`, case-insensitively.
    ///
    /// Returns `None` for unrecognized input.
    #[must_use]
    #[inline]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }

    /// Returns the canonical lowercase name (`"light"` or `"dark"`),
    /// the inverse of [`parse`](Self::parse).
    #[must_use]
    #[inline]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}
