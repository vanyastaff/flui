//! Blend modes for compositing colors.

/// Blend modes for compositing colors.
///
/// Defines how colors should be blended when compositing images or drawing operations.
/// Similar to Flutter's `BlendMode` and CSS blend modes.
///
/// # Examples
///
/// ```
/// use flui_types::painting::BlendMode;
///
/// let mode = BlendMode::Multiply;
/// assert_eq!(mode, BlendMode::Multiply);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Default)]
pub enum BlendMode {
    /// Drop both the source and the destination, leaving nothing.
    ///
    /// This corresponds to the "clear" Porter-Duff operator.
    Clear,

    /// Drop the destination, only keep the source.
    ///
    /// This corresponds to the "src" Porter-Duff operator.
    Src,

    /// Drop the source, only keep the destination.
    ///
    /// This corresponds to the "dst" Porter-Duff operator.
    Dst,

    /// Composite the source over the destination.
    ///
    /// This is the default blend mode. This corresponds to the "src-over" Porter-Duff operator.
    #[default]
    SrcOver,

    /// Composite the destination over the source.
    ///
    /// This corresponds to the "dst-over" Porter-Duff operator.
    DstOver,

    /// Show the source image, but only where the two images overlap.
    ///
    /// This corresponds to the "src-in" Porter-Duff operator.
    SrcIn,

    /// Show the destination image, but only where the two images overlap.
    ///
    /// This corresponds to the "dst-in" Porter-Duff operator.
    DstIn,

    /// Show the source image, but only where the two images do not overlap.
    ///
    /// This corresponds to the "src-out" Porter-Duff operator.
    SrcOut,

    /// Show the destination image, but only where the two images do not overlap.
    ///
    /// This corresponds to the "dst-out" Porter-Duff operator.
    DstOut,

    /// Composite the source over the destination, but only where they overlap.
    ///
    /// This corresponds to the "src-atop" Porter-Duff operator.
    SrcATop,

    /// Composite the destination over the source, but only where they overlap.
    ///
    /// This corresponds to the "dst-atop" Porter-Duff operator.
    DstATop,

    /// Apply a bitwise XOR operator when compositing the source and destination.
    ///
    /// This corresponds to the "xor" Porter-Duff operator.
    Xor,

    /// Sum the components of the source and destination images.
    ///
    /// This corresponds to the "plus" Porter-Duff operator.
    Plus,

    /// Multiply the color components of the source and destination images.
    ///
    /// This can only darken the colors. This corresponds to the "modulate" Porter-Duff operator.
    Modulate,

    // Advanced blend modes (non-Porter-Duff)
    /// Multiply the inverse of the components of the source and destination images.
    ///
    /// This can only lighten the colors.
    Screen,

    /// Multiply or screen the components, depending on the destination.
    ///
    /// This corresponds to the CSS overlay mode.
    Overlay,

    /// The darker of the source and destination colors.
    ///
    /// This corresponds to the CSS darken mode.
    Darken,

    /// The lighter of the source and destination colors.
    ///
    /// This corresponds to the CSS lighten mode.
    Lighten,

    /// Brighten the destination color to reflect the source color.
    ///
    /// This corresponds to the CSS color-dodge mode.
    ColorDodge,

    /// Darken the destination color to reflect the source color.
    ///
    /// This corresponds to the CSS color-burn mode.
    ColorBurn,

    /// Multiply or screen the colors, depending on the source color.
    ///
    /// This corresponds to the CSS hard-light mode.
    HardLight,

    /// Lighten or darken the colors, depending on the source color.
    ///
    /// This corresponds to the CSS soft-light mode.
    SoftLight,

    /// Subtract the darker of the two colors from the lighter one.
    ///
    /// This corresponds to the CSS difference mode.
    Difference,

    /// Similar to difference, but with lower contrast.
    ///
    /// This corresponds to the CSS exclusion mode.
    Exclusion,

    /// Multiply the source and destination images.
    ///
    /// This corresponds to the CSS multiply mode.
    Multiply,

    /// Use the hue of the source, saturation and luminosity of the destination.
    ///
    /// This corresponds to the CSS hue mode.
    Hue,

    /// Use the saturation of the source, hue and luminosity of the destination.
    ///
    /// This corresponds to the CSS saturation mode.
    Saturation,

    /// Use the hue and saturation of the source, luminosity of the destination.
    ///
    /// This corresponds to the CSS color mode.
    Color,

    /// Use the luminosity of the source, hue and saturation of the destination.
    ///
    /// This corresponds to the CSS luminosity mode.
    Luminosity,
}

impl BlendMode {
    /// Returns true if this blend mode is a Porter-Duff mode.
    #[inline]
    #[must_use]
    pub const fn is_porter_duff(&self) -> bool {
        matches!(
            self,
            BlendMode::Clear
                | BlendMode::Src
                | BlendMode::Dst
                | BlendMode::SrcOver
                | BlendMode::DstOver
                | BlendMode::SrcIn
                | BlendMode::DstIn
                | BlendMode::SrcOut
                | BlendMode::DstOut
                | BlendMode::SrcATop
                | BlendMode::DstATop
                | BlendMode::Xor
                | BlendMode::Plus
                | BlendMode::Modulate
        )
    }

    /// Returns true if this blend mode requires the destination image.
    #[inline]
    #[must_use]
    pub const fn requires_destination(&self) -> bool {
        !matches!(self, BlendMode::Clear | BlendMode::Src)
    }

    /// Returns true if this is an advanced (non-Porter-Duff) blend mode.
    #[inline]
    #[must_use]
    pub const fn is_advanced(&self) -> bool {
        !self.is_porter_duff()
    }

    /// Returns true if this blend mode can lighten colors.
    #[inline]
    #[must_use]
    pub const fn can_lighten(&self) -> bool {
        matches!(
            self,
            BlendMode::Screen | BlendMode::Lighten | BlendMode::ColorDodge | BlendMode::Plus
        )
    }

    /// Returns true if this blend mode can darken colors.
    #[inline]
    #[must_use]
    pub const fn can_darken(&self) -> bool {
        matches!(
            self,
            BlendMode::Darken | BlendMode::ColorBurn | BlendMode::Multiply | BlendMode::Modulate
        )
    }

    /// Returns true if this blend mode is compositional (affects alpha).
    #[inline]
    #[must_use]
    pub const fn is_compositional(&self) -> bool {
        self.is_porter_duff()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blend_mode_default() {
        assert_eq!(BlendMode::default(), BlendMode::SrcOver);
    }

    #[test]
    fn test_blend_mode_porter_duff() {
        assert!(BlendMode::SrcOver.is_porter_duff());
        assert!(BlendMode::Clear.is_porter_duff());
        assert!(BlendMode::Xor.is_porter_duff());

        assert!(!BlendMode::Screen.is_porter_duff());
        assert!(!BlendMode::Multiply.is_porter_duff());
        assert!(!BlendMode::Overlay.is_porter_duff());
    }

    #[test]
    fn test_blend_mode_requires_destination() {
        assert!(!BlendMode::Clear.requires_destination());
        assert!(!BlendMode::Src.requires_destination());

        assert!(BlendMode::SrcOver.requires_destination());
        assert!(BlendMode::Multiply.requires_destination());
    }

    #[test]
    fn test_blend_mode_equality() {
        assert_eq!(BlendMode::Multiply, BlendMode::Multiply);
        assert_ne!(BlendMode::Multiply, BlendMode::Screen);
    }
}
