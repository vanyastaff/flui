//! Brightness settings for theming

/// The brightness of the overall theme
///
/// Similar to Flutter's `Brightness`. Used to determine whether to use
/// light or dark colors.
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
    pub const fn is_light(&self) -> bool {
        matches!(self, Self::Light)
    }

    /// Returns true if this is dark brightness
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
    pub const fn invert(&self) -> Self {
        match self {
            Self::Light => Self::Dark,
            Self::Dark => Self::Light,
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
