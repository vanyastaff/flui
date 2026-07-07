//! Device orientation types

/// The physical orientation of the device screen.
///
/// Mirrors Flutter's `DeviceOrientation` enum. The four variants describe
/// where the top of the device is pointing relative to its natural
/// portrait position.
#[derive(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DeviceOrientation {
    /// Portrait orientation with the top of the device up (the default).
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
    /// Returns `true` if this is a portrait orientation
    /// (`PortraitUp` or `PortraitDown`).
    #[must_use]
    #[inline]
    pub const fn is_portrait(&self) -> bool {
        matches!(self, Self::PortraitUp | Self::PortraitDown)
    }

    /// Returns `true` if this is a landscape orientation
    /// (`LandscapeLeft` or `LandscapeRight`).
    #[must_use]
    #[inline]
    pub const fn is_landscape(&self) -> bool {
        matches!(self, Self::LandscapeLeft | Self::LandscapeRight)
    }

    /// Returns the rotation angle in degrees relative to `PortraitUp`.
    ///
    /// `PortraitUp` is 0°, `LandscapeLeft` 90°, `PortraitDown` 180°,
    /// and `LandscapeRight` 270°.
    #[must_use]
    #[inline]
    pub const fn rotation_degrees(&self) -> f32 {
        match self {
            Self::PortraitUp => 0.0,
            Self::LandscapeLeft => 90.0,
            Self::PortraitDown => 180.0,
            Self::LandscapeRight => 270.0,
        }
    }

    /// Returns the rotation angle in radians relative to `PortraitUp`.
    ///
    /// Same as [`rotation_degrees`](Self::rotation_degrees) converted
    /// to radians.
    #[must_use]
    #[inline]
    pub fn rotation_radians(&self) -> f32 {
        self.rotation_degrees().to_radians()
    }

    /// Returns the orientation reached by rotating the device 90°
    /// clockwise from this one.
    #[must_use]
    #[inline]
    pub const fn rotate_clockwise(&self) -> Self {
        match self {
            Self::PortraitUp => Self::LandscapeRight,
            Self::LandscapeRight => Self::PortraitDown,
            Self::PortraitDown => Self::LandscapeLeft,
            Self::LandscapeLeft => Self::PortraitUp,
        }
    }

    /// Returns the orientation reached by rotating the device 90°
    /// counter-clockwise from this one.
    #[must_use]
    #[inline]
    pub const fn rotate_counter_clockwise(&self) -> Self {
        match self {
            Self::PortraitUp => Self::LandscapeLeft,
            Self::LandscapeLeft => Self::PortraitDown,
            Self::PortraitDown => Self::LandscapeRight,
            Self::LandscapeRight => Self::PortraitUp,
        }
    }

    /// Returns the orientation rotated 180° from this one
    /// (e.g. `PortraitUp` ↔ `PortraitDown`).
    #[must_use]
    #[inline]
    pub const fn opposite(&self) -> Self {
        match self {
            Self::PortraitUp => Self::PortraitDown,
            Self::PortraitDown => Self::PortraitUp,
            Self::LandscapeLeft => Self::LandscapeRight,
            Self::LandscapeRight => Self::LandscapeLeft,
        }
    }

    /// Returns `true` if the device is upside down (`PortraitDown`).
    #[must_use]
    #[inline]
    pub const fn is_upside_down(&self) -> bool {
        matches!(self, Self::PortraitDown)
    }

    /// Parses an orientation from its string name, case-insensitively.
    ///
    /// Accepts snake_case (`"portrait_up"`) and concatenated
    /// (`"portraitup"`) forms; returns `None` for unrecognized input.
    #[must_use]
    #[inline]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "portrait_up" | "portraitup" => Some(Self::PortraitUp),
            "portrait_down" | "portraitdown" => Some(Self::PortraitDown),
            "landscape_left" | "landscapeleft" => Some(Self::LandscapeLeft),
            "landscape_right" | "landscaperight" => Some(Self::LandscapeRight),
            _ => None,
        }
    }

    /// Returns the canonical snake_case name of this orientation
    /// (e.g. `"portrait_up"`), the inverse of [`parse`](Self::parse).
    #[must_use]
    #[inline]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::PortraitUp => "portrait_up",
            Self::PortraitDown => "portrait_down",
            Self::LandscapeLeft => "landscape_left",
            Self::LandscapeRight => "landscape_right",
        }
    }
}
