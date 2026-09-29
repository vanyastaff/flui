//! [`Brightness`] — light/dark theme preference.

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
