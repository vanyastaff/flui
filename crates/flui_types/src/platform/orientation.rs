//! Device orientation types

#[derive(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DeviceOrientation {
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
    #[must_use]
    #[inline]
    pub const fn is_portrait(&self) -> bool {
        matches!(self, Self::PortraitUp | Self::PortraitDown)
    }

    #[must_use]
    #[inline]
    pub const fn is_landscape(&self) -> bool {
        matches!(self, Self::LandscapeLeft | Self::LandscapeRight)
    }

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

    #[must_use]
    #[inline]
    pub fn rotation_radians(&self) -> f32 {
        self.rotation_degrees().to_radians()
    }

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

    #[must_use]
    #[inline]
    pub const fn is_upside_down(&self) -> bool {
        matches!(self, Self::PortraitDown)
    }

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
