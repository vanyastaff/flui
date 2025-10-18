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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DeviceOrientation {
    /// Portrait orientation with the top of the device up
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
    pub const fn is_landscape(&self) -> bool {
        matches!(self, Self::LandscapeLeft | Self::LandscapeRight)
    }
}

impl Default for DeviceOrientation {
    fn default() -> Self {
        Self::PortraitUp
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
