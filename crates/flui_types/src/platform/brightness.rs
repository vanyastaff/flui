//! Brightness settings for theming

use crate::styling::Color;

/// The brightness of the overall theme
///
/// Similar to Flutter's `Brightness`. Used to determine whether to use
/// light or dark colors.
///
/// # Memory Safety
/// - Zero-sized enum with no allocations
/// - Const-evaluable methods
///
/// # Type Safety
/// - `#[must_use]` on all pure methods
/// - Strongly typed brightness states
///
/// # Examples
///
/// ```
/// use flui_types::platform::Brightness;
///
/// let brightness = Brightness::Dark;
/// assert!(brightness.is_dark());
/// assert!(!brightness.is_light());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Brightness {
    /// Light theme (dark text on light background)
    Light,

    /// Dark theme (light text on dark background)
    Dark,
}

impl Brightness {
    /// Returns true if this is light brightness
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::Brightness;
    ///
    /// assert!(Brightness::Light.is_light());
    /// assert!(!Brightness::Dark.is_light());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_light(&self) -> bool {
        matches!(self, Self::Light)
    }

    /// Returns true if this is dark brightness
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::Brightness;
    ///
    /// assert!(Brightness::Dark.is_dark());
    /// assert!(!Brightness::Light.is_dark());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_dark(&self) -> bool {
        matches!(self, Self::Dark)
    }

    /// Returns the opposite brightness
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::Brightness;
    ///
    /// assert_eq!(Brightness::Light.invert(), Brightness::Dark);
    /// assert_eq!(Brightness::Dark.invert(), Brightness::Light);
    /// ```
    #[inline]
    #[must_use]
    pub const fn invert(&self) -> Self {
        match self {
            Self::Light => Self::Dark,
            Self::Dark => Self::Light,
        }
    }

    /// Returns a suggested background color for this brightness
    ///
    /// Useful for theming and rendering.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::Brightness;
    ///
    /// let light_bg = Brightness::Light.background_color();
    /// assert_eq!(light_bg.red(), 255);
    ///
    /// let dark_bg = Brightness::Dark.background_color();
    /// assert_eq!(dark_bg.red(), 18);
    /// ```
    #[inline]
    #[must_use]
    pub const fn background_color(&self) -> Color {
        match self {
            Self::Light => Color::rgba(255, 255, 255, 255), // White
            Self::Dark => Color::rgba(18, 18, 18, 255),     // Near black
        }
    }

    /// Returns a suggested foreground/text color for this brightness
    ///
    /// Useful for theming and rendering.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::Brightness;
    ///
    /// let light_fg = Brightness::Light.foreground_color();
    /// assert_eq!(light_fg.red(), 0);
    ///
    /// let dark_fg = Brightness::Dark.foreground_color();
    /// assert_eq!(dark_fg.red(), 255);
    /// ```
    #[inline]
    #[must_use]
    pub const fn foreground_color(&self) -> Color {
        match self {
            Self::Light => Color::rgba(0, 0, 0, 255),       // Black
            Self::Dark => Color::rgba(255, 255, 255, 255),  // White
        }
    }

    /// Returns a suggested surface color for this brightness
    ///
    /// Useful for cards, dialogs, and elevated surfaces.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::Brightness;
    ///
    /// let light_surface = Brightness::Light.surface_color();
    /// assert_eq!(light_surface.red(), 255);
    ///
    /// let dark_surface = Brightness::Dark.surface_color();
    /// assert_eq!(dark_surface.red(), 30);
    /// ```
    #[inline]
    #[must_use]
    pub const fn surface_color(&self) -> Color {
        match self {
            Self::Light => Color::rgba(255, 255, 255, 255), // White
            Self::Dark => Color::rgba(30, 30, 30, 255),     // Dark gray
        }
    }

    /// Returns the opacity factor for elevation shadows
    ///
    /// Dark themes typically need stronger shadows.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::Brightness;
    ///
    /// assert_eq!(Brightness::Light.shadow_opacity(), 0.2);
    /// assert_eq!(Brightness::Dark.shadow_opacity(), 0.4);
    /// ```
    #[inline]
    #[must_use]
    pub const fn shadow_opacity(&self) -> f32 {
        match self {
            Self::Light => 0.2,
            Self::Dark => 0.4,
        }
    }

    /// Parses a brightness from a string
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::Brightness;
    ///
    /// assert_eq!(Brightness::parse("light"), Some(Brightness::Light));
    /// assert_eq!(Brightness::parse("dark"), Some(Brightness::Dark));
    /// assert_eq!(Brightness::parse("LIGHT"), Some(Brightness::Light));
    /// assert_eq!(Brightness::parse("invalid"), None);
    /// ```
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }

    /// Returns a string representation
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::Brightness;
    ///
    /// assert_eq!(Brightness::Light.as_str(), "light");
    /// assert_eq!(Brightness::Dark.as_str(), "dark");
    /// ```
    #[inline]
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

impl Default for Brightness {
    fn default() -> Self {
        Self::Light
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_brightness_default() {
        assert_eq!(Brightness::default(), Brightness::Light);
    }

    #[test]
    fn test_brightness_is_light() {
        assert!(Brightness::Light.is_light());
        assert!(!Brightness::Dark.is_light());
    }

    #[test]
    fn test_brightness_is_dark() {
        assert!(Brightness::Dark.is_dark());
        assert!(!Brightness::Light.is_dark());
    }

    #[test]
    fn test_brightness_invert() {
        assert_eq!(Brightness::Light.invert(), Brightness::Dark);
        assert_eq!(Brightness::Dark.invert(), Brightness::Light);
    }
}
