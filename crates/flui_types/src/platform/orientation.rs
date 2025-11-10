//! Device orientation types

/// The orientation of the device
///
/// Similar to Flutter's `DeviceOrientation`. Describes the current
/// orientation of the device screen.
///
/// # Examples
///
/// ```
/// use flui_types::platform::DeviceOrientation;
///
/// let orientation = DeviceOrientation::PortraitUp;
/// assert!(orientation.is_portrait());
/// assert!(!orientation.is_landscape());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DeviceOrientation {
    /// Portrait orientation with the top of the device up
    #[default]
    PortraitUp,

    /// Portrait orientation with the top of the device down (upside down)
    PortraitDown,

    /// Landscape orientation with the top of the device to the left
    LandscapeLeft,

    /// Landscape orientation with the top of the device to the right
    LandscapeRight,
}

impl DeviceOrientation {
    /// Returns true if this is a portrait orientation
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::DeviceOrientation;
    ///
    /// assert!(DeviceOrientation::PortraitUp.is_portrait());
    /// assert!(DeviceOrientation::PortraitDown.is_portrait());
    /// assert!(!DeviceOrientation::LandscapeLeft.is_portrait());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_portrait(&self) -> bool {
        matches!(self, Self::PortraitUp | Self::PortraitDown)
    }

    /// Returns true if this is a landscape orientation
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::DeviceOrientation;
    ///
    /// assert!(DeviceOrientation::LandscapeLeft.is_landscape());
    /// assert!(DeviceOrientation::LandscapeRight.is_landscape());
    /// assert!(!DeviceOrientation::PortraitUp.is_landscape());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_landscape(&self) -> bool {
        matches!(self, Self::LandscapeLeft | Self::LandscapeRight)
    }

    /// Returns the rotation angle in degrees from PortraitUp
    ///
    /// Useful for rendering and transform calculations.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::DeviceOrientation;
    ///
    /// assert_eq!(DeviceOrientation::PortraitUp.rotation_degrees(), 0.0);
    /// assert_eq!(DeviceOrientation::LandscapeLeft.rotation_degrees(), 90.0);
    /// assert_eq!(DeviceOrientation::PortraitDown.rotation_degrees(), 180.0);
    /// assert_eq!(DeviceOrientation::LandscapeRight.rotation_degrees(), 270.0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn rotation_degrees(&self) -> f32 {
        match self {
            Self::PortraitUp => 0.0,
            Self::LandscapeLeft => 90.0,
            Self::PortraitDown => 180.0,
            Self::LandscapeRight => 270.0,
        }
    }

    /// Returns the rotation angle in radians from PortraitUp
    ///
    /// Useful for rendering and transform calculations.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::DeviceOrientation;
    /// use std::f32::consts::PI;
    ///
    /// assert!((DeviceOrientation::PortraitUp.rotation_radians() - 0.0).abs() < 0.001);
    /// assert!((DeviceOrientation::LandscapeLeft.rotation_radians() - PI / 2.0).abs() < 0.001);
    /// assert!((DeviceOrientation::PortraitDown.rotation_radians() - PI).abs() < 0.001);
    /// ```
    #[inline]
    #[must_use]
    pub fn rotation_radians(&self) -> f32 {
        self.rotation_degrees().to_radians()
    }

    /// Rotates the orientation 90 degrees clockwise
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::DeviceOrientation;
    ///
    /// assert_eq!(
    ///     DeviceOrientation::PortraitUp.rotate_clockwise(),
    ///     DeviceOrientation::LandscapeRight
    /// );
    /// assert_eq!(
    ///     DeviceOrientation::LandscapeRight.rotate_clockwise(),
    ///     DeviceOrientation::PortraitDown
    /// );
    /// ```
    #[inline]
    #[must_use]
    pub const fn rotate_clockwise(&self) -> Self {
        match self {
            Self::PortraitUp => Self::LandscapeRight,
            Self::LandscapeRight => Self::PortraitDown,
            Self::PortraitDown => Self::LandscapeLeft,
            Self::LandscapeLeft => Self::PortraitUp,
        }
    }

    /// Rotates the orientation 90 degrees counter-clockwise
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::DeviceOrientation;
    ///
    /// assert_eq!(
    ///     DeviceOrientation::PortraitUp.rotate_counter_clockwise(),
    ///     DeviceOrientation::LandscapeLeft
    /// );
    /// assert_eq!(
    ///     DeviceOrientation::LandscapeLeft.rotate_counter_clockwise(),
    ///     DeviceOrientation::PortraitDown
    /// );
    /// ```
    #[inline]
    #[must_use]
    pub const fn rotate_counter_clockwise(&self) -> Self {
        match self {
            Self::PortraitUp => Self::LandscapeLeft,
            Self::LandscapeLeft => Self::PortraitDown,
            Self::PortraitDown => Self::LandscapeRight,
            Self::LandscapeRight => Self::PortraitUp,
        }
    }

    /// Returns the opposite orientation (180 degree rotation)
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::DeviceOrientation;
    ///
    /// assert_eq!(
    ///     DeviceOrientation::PortraitUp.opposite(),
    ///     DeviceOrientation::PortraitDown
    /// );
    /// assert_eq!(
    ///     DeviceOrientation::LandscapeLeft.opposite(),
    ///     DeviceOrientation::LandscapeRight
    /// );
    /// ```
    #[inline]
    #[must_use]
    pub const fn opposite(&self) -> Self {
        match self {
            Self::PortraitUp => Self::PortraitDown,
            Self::PortraitDown => Self::PortraitUp,
            Self::LandscapeLeft => Self::LandscapeRight,
            Self::LandscapeRight => Self::LandscapeLeft,
        }
    }

    /// Returns whether this orientation is upside down
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::DeviceOrientation;
    ///
    /// assert!(!DeviceOrientation::PortraitUp.is_upside_down());
    /// assert!(DeviceOrientation::PortraitDown.is_upside_down());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_upside_down(&self) -> bool {
        matches!(self, Self::PortraitDown)
    }

    /// Parses an orientation from a string
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::DeviceOrientation;
    ///
    /// assert_eq!(
    ///     DeviceOrientation::parse("portrait_up"),
    ///     Some(DeviceOrientation::PortraitUp)
    /// );
    /// assert_eq!(
    ///     DeviceOrientation::parse("landscape_left"),
    ///     Some(DeviceOrientation::LandscapeLeft)
    /// );
    /// assert_eq!(DeviceOrientation::parse("invalid"), None);
    /// ```
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "portrait_up" | "portraitup" => Some(Self::PortraitUp),
            "portrait_down" | "portraitdown" => Some(Self::PortraitDown),
            "landscape_left" | "landscapeleft" => Some(Self::LandscapeLeft),
            "landscape_right" | "landscaperight" => Some(Self::LandscapeRight),
            _ => None,
        }
    }

    /// Returns a string representation
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::DeviceOrientation;
    ///
    /// assert_eq!(DeviceOrientation::PortraitUp.as_str(), "portrait_up");
    /// assert_eq!(DeviceOrientation::LandscapeLeft.as_str(), "landscape_left");
    /// ```
    #[inline]
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::PortraitUp => "portrait_up",
            Self::PortraitDown => "portrait_down",
            Self::LandscapeLeft => "landscape_left",
            Self::LandscapeRight => "landscape_right",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_orientation_default() {
        assert_eq!(DeviceOrientation::default(), DeviceOrientation::PortraitUp);
    }

    #[test]
    fn test_device_orientation_is_portrait() {
        assert!(DeviceOrientation::PortraitUp.is_portrait());
        assert!(DeviceOrientation::PortraitDown.is_portrait());
        assert!(!DeviceOrientation::LandscapeLeft.is_portrait());
        assert!(!DeviceOrientation::LandscapeRight.is_portrait());
    }

    #[test]
    fn test_device_orientation_is_landscape() {
        assert!(DeviceOrientation::LandscapeLeft.is_landscape());
        assert!(DeviceOrientation::LandscapeRight.is_landscape());
        assert!(!DeviceOrientation::PortraitUp.is_landscape());
        assert!(!DeviceOrientation::PortraitDown.is_landscape());
    }
}
