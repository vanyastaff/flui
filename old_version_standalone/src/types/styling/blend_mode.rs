//! Blend modes for compositing visual elements.
//!
//! Similar to Flutter's `BlendMode` and CSS `mix-blend-mode`.

/// Algorithms for blending colors together.
///
/// Used when compositing layers or applying visual effects.
/// Similar to Flutter's `BlendMode` and CSS `mix-blend-mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlendMode {
    // ========== Porter-Duff Modes ==========
    /// Drop the source, show the destination.
    Clear,

    /// Show the source.
    Source,

    /// Show the destination.
    Destination,

    /// Show the source over the destination.
    SourceOver,

    /// Show the destination over the source.
    DestinationOver,

    /// Show the source in the destination.
    SourceIn,

    /// Show the destination in the source.
    DestinationIn,

    /// Show the source outside the destination.
    SourceOut,

    /// Show the destination outside the source.
    DestinationOut,

    /// Show the source on top of the destination.
    SourceAtop,

    /// Show the destination on top of the source.
    DestinationAtop,

    /// Show the exclusive OR of the source and destination.
    Xor,

    /// Sum the source and destination.
    Plus,

    /// Modulate the source and destination.
    Modulate,

    // ========== Separable Blend Modes ==========
    /// Screen blend mode - inverts, multiplies, and inverts again.
    ///
    /// Result is always at least as light as either input.
    Screen,

    /// Overlay blend mode - multiplies or screens, depending on destination.
    Overlay,

    /// Darken blend mode - keeps the darker of source and destination.
    Darken,

    /// Lighten blend mode - keeps the lighter of source and destination.
    Lighten,

    /// Color dodge - brightens the destination to reflect the source.
    ColorDodge,

    /// Color burn - darkens the destination to reflect the source.
    ColorBurn,

    /// Hard light - multiplies or screens, depending on source.
    HardLight,

    /// Soft light - darkens or lightens, depending on source.
    SoftLight,

    /// Difference - subtracts darker from lighter.
    Difference,

    /// Exclusion - similar to difference but lower contrast.
    Exclusion,

    /// Multiply - multiplies the source and destination.
    ///
    /// Result is always at least as dark as either input.
    Multiply,

    // ========== Non-Separable Blend Modes ==========
    /// Hue - uses the hue of the source with saturation/luminosity of destination.
    Hue,

    /// Saturation - uses the saturation of the source with hue/luminosity of destination.
    Saturation,

    /// Color - uses the hue and saturation of the source with luminosity of destination.
    Color,

    /// Luminosity - uses the luminosity of the source with hue/saturation of destination.
    Luminosity,
}

impl BlendMode {
    /// Check if this blend mode requires destination pixels.
    ///
    /// Some blend modes (like `Source`) don't need to read the destination.
    pub fn requires_destination(&self) -> bool {
        !matches!(self, BlendMode::Source | BlendMode::Clear)
    }

    /// Check if this blend mode preserves opacity.
    ///
    /// Some blend modes (like `SourceOver`) preserve the alpha channel.
    pub fn preserves_opacity(&self) -> bool {
        matches!(
            self,
            BlendMode::SourceOver
                | BlendMode::DestinationOver
                | BlendMode::SourceIn
                | BlendMode::DestinationIn
                | BlendMode::SourceOut
                | BlendMode::DestinationOut
                | BlendMode::SourceAtop
                | BlendMode::DestinationAtop
                | BlendMode::Xor
        )
    }

    /// Check if this is a Porter-Duff compositing mode.
    pub fn is_porter_duff(&self) -> bool {
        matches!(
            self,
            BlendMode::Clear
                | BlendMode::Source
                | BlendMode::Destination
                | BlendMode::SourceOver
                | BlendMode::DestinationOver
                | BlendMode::SourceIn
                | BlendMode::DestinationIn
                | BlendMode::SourceOut
                | BlendMode::DestinationOut
                | BlendMode::SourceAtop
                | BlendMode::DestinationAtop
                | BlendMode::Xor
                | BlendMode::Plus
                | BlendMode::Modulate
        )
    }

    /// Check if this is a separable blend mode.
    pub fn is_separable(&self) -> bool {
        matches!(
            self,
            BlendMode::Screen
                | BlendMode::Overlay
                | BlendMode::Darken
                | BlendMode::Lighten
                | BlendMode::ColorDodge
                | BlendMode::ColorBurn
                | BlendMode::HardLight
                | BlendMode::SoftLight
                | BlendMode::Difference
                | BlendMode::Exclusion
                | BlendMode::Multiply
        )
    }

    /// Check if this is a non-separable blend mode.
    pub fn is_non_separable(&self) -> bool {
        matches!(
            self,
            BlendMode::Hue
                | BlendMode::Saturation
                | BlendMode::Color
                | BlendMode::Luminosity
        )
    }

    /// Get the CSS `mix-blend-mode` equivalent name.
    pub fn css_name(&self) -> &'static str {
        match self {
            BlendMode::Clear => "clear",
            BlendMode::Source => "copy",
            BlendMode::Destination => "destination",
            BlendMode::SourceOver => "source-over",
            BlendMode::DestinationOver => "destination-over",
            BlendMode::SourceIn => "source-in",
            BlendMode::DestinationIn => "destination-in",
            BlendMode::SourceOut => "source-out",
            BlendMode::DestinationOut => "destination-out",
            BlendMode::SourceAtop => "source-atop",
            BlendMode::DestinationAtop => "destination-atop",
            BlendMode::Xor => "xor",
            BlendMode::Plus => "plus",
            BlendMode::Modulate => "modulate",
            BlendMode::Screen => "screen",
            BlendMode::Overlay => "overlay",
            BlendMode::Darken => "darken",
            BlendMode::Lighten => "lighten",
            BlendMode::ColorDodge => "color-dodge",
            BlendMode::ColorBurn => "color-burn",
            BlendMode::HardLight => "hard-light",
            BlendMode::SoftLight => "soft-light",
            BlendMode::Difference => "difference",
            BlendMode::Exclusion => "exclusion",
            BlendMode::Multiply => "multiply",
            BlendMode::Hue => "hue",
            BlendMode::Saturation => "saturation",
            BlendMode::Color => "color",
            BlendMode::Luminosity => "luminosity",
        }
    }
}

// Default to normal blending (source over destination)
impl Default for BlendMode {
    fn default() -> Self {
        BlendMode::SourceOver
    }
}

impl std::fmt::Display for BlendMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blend_mode_requires_destination() {
        assert!(!BlendMode::Source.requires_destination());
        assert!(!BlendMode::Clear.requires_destination());
        assert!(BlendMode::SourceOver.requires_destination());
        assert!(BlendMode::Multiply.requires_destination());
    }

    #[test]
    fn test_blend_mode_preserves_opacity() {
        assert!(BlendMode::SourceOver.preserves_opacity());
        assert!(BlendMode::SourceAtop.preserves_opacity());
        assert!(!BlendMode::Multiply.preserves_opacity());
        assert!(!BlendMode::Screen.preserves_opacity());
    }

    #[test]
    fn test_blend_mode_categories() {
        // Porter-Duff
        assert!(BlendMode::SourceOver.is_porter_duff());
        assert!(BlendMode::Xor.is_porter_duff());
        assert!(!BlendMode::Multiply.is_porter_duff());

        // Separable
        assert!(BlendMode::Multiply.is_separable());
        assert!(BlendMode::Screen.is_separable());
        assert!(!BlendMode::Hue.is_separable());

        // Non-separable
        assert!(BlendMode::Hue.is_non_separable());
        assert!(BlendMode::Saturation.is_non_separable());
        assert!(!BlendMode::Multiply.is_non_separable());
    }

    #[test]
    fn test_blend_mode_css_names() {
        assert_eq!(BlendMode::Multiply.css_name(), "multiply");
        assert_eq!(BlendMode::Screen.css_name(), "screen");
        assert_eq!(BlendMode::Overlay.css_name(), "overlay");
        assert_eq!(BlendMode::SourceOver.css_name(), "source-over");
    }

    #[test]
    fn test_blend_mode_default() {
        assert_eq!(BlendMode::default(), BlendMode::SourceOver);
    }

    #[test]
    fn test_blend_mode_display() {
        assert_eq!(format!("{}", BlendMode::Multiply), "Multiply");
        assert_eq!(format!("{}", BlendMode::Screen), "Screen");
    }
}
