//! Device orientation types

/// The physical orientation of the device screen.
///
/// Mirrors Flutter's `DeviceOrientation` enum. The four variants describe
/// where the top of the device is pointing relative to its natural
/// portrait position.
#[derive(Debug, Default)]
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

#[cfg(test)]
mod tests {
    use super::DeviceOrientation::{self, *};

    const ALL: [DeviceOrientation; 4] = [PortraitUp, LandscapeLeft, PortraitDown, LandscapeRight];

    /// `ALL` is in counter-clockwise order, 90° apart: `rotation_degrees`
    /// counts counter-clockwise from `PortraitUp` (`LandscapeLeft` is the
    /// device turned with its top to the left), so a clockwise turn subtracts
    /// 90° and a counter-clockwise one adds it.
    #[test]
    fn rotations_follow_the_degree_table() {
        for (i, o) in ALL.iter().enumerate() {
            assert_eq!(o.rotation_degrees(), i as f32 * 90.0, "{o:?}");
            assert!((o.rotation_radians() - (i as f32 * 90.0).to_radians()).abs() < 1e-6);
            assert_eq!(
                o.rotate_counter_clockwise().as_str(),
                ALL[(i + 1) % 4].as_str()
            );
            assert_eq!(o.rotate_clockwise().as_str(), ALL[(i + 3) % 4].as_str());
            assert_eq!(o.opposite().as_str(), ALL[(i + 2) % 4].as_str());
        }
    }

    #[test]
    fn predicates() {
        for (o, (portrait, landscape, upside_down)) in [
            (PortraitUp, (true, false, false)),
            (PortraitDown, (true, false, true)),
            (LandscapeLeft, (false, true, false)),
            (LandscapeRight, (false, true, false)),
        ] {
            assert_eq!(
                (o.is_portrait(), o.is_landscape(), o.is_upside_down()),
                (portrait, landscape, upside_down),
                "{o:?}"
            );
        }
    }

    /// Accepts the `as_str` spelling, the same without underscores, and any
    /// case.
    #[test]
    fn parse_and_as_str() {
        for o in ALL {
            let name = o.as_str();
            let parsed = |s: &str| DeviceOrientation::parse(s).map(|p| p.as_str());
            assert_eq!(parsed(name), Some(name));
            assert_eq!(parsed(&name.replace('_', "")), Some(name));
            assert_eq!(parsed(&name.to_uppercase()), Some(name));
        }
        assert!(DeviceOrientation::parse("sideways").is_none());
    }
}
