//! Blend modes for compositing colors.

/// Algorithms for blending a source (the pixels being drawn) with a
/// destination (the pixels already on the canvas).
///
/// The first fourteen variants are Porter-Duff compositing operators; the
/// rest are "advanced" (separable and non-separable) blend modes matching
/// CSS/Skia semantics. Mirrors Flutter's `BlendMode`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    /// This is the default blend mode for ordinary painting and corresponds
    /// to the "src-over" Porter-Duff operator.
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

    /// Show the destination image, but only where the two images do not
    /// overlap.
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

    /// Apply a bitwise XOR operator when compositing the source and
    /// destination.
    ///
    /// This corresponds to the "xor" Porter-Duff operator.
    Xor,

    /// Sum the components of the source and destination images.
    ///
    /// This corresponds to the "plus" Porter-Duff operator.
    Plus,

    /// Multiply the color components of the source and destination images.
    ///
    /// This can only darken the colors. This corresponds to the "modulate"
    /// Porter-Duff operator.
    Modulate,

    // Advanced blend modes (non-Porter-Duff)
    /// Multiply the inverse of the components of the source and destination
    /// images.
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
    /// Returns `true` if this is one of the fourteen Porter-Duff compositing
    /// operators (`Clear` through `Modulate`).
    #[must_use]
    #[inline]
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

    /// Returns `true` if the result depends on the destination pixels.
    ///
    /// Only `Clear` and `Src` ignore the destination entirely; every other
    /// mode must read the existing pixels to compute its output.
    #[must_use]
    #[inline]
    pub const fn requires_destination(&self) -> bool {
        !matches!(self, BlendMode::Clear | BlendMode::Src)
    }

    /// Returns `true` for the advanced (non-Porter-Duff) blend modes, i.e.
    /// `Screen` and beyond.
    #[must_use]
    #[inline]
    pub const fn is_advanced(&self) -> bool {
        !self.is_porter_duff()
    }

    /// Returns `true` for modes that can only lighten colors
    /// (`Screen`, `Lighten`, `ColorDodge`, `Plus`).
    #[must_use]
    #[inline]
    pub const fn can_lighten(&self) -> bool {
        matches!(
            self,
            BlendMode::Screen | BlendMode::Lighten | BlendMode::ColorDodge | BlendMode::Plus
        )
    }

    /// Returns `true` for modes that can only darken colors
    /// (`Darken`, `ColorBurn`, `Multiply`, `Modulate`).
    #[must_use]
    #[inline]
    pub const fn can_darken(&self) -> bool {
        matches!(
            self,
            BlendMode::Darken | BlendMode::ColorBurn | BlendMode::Multiply | BlendMode::Modulate
        )
    }

    /// Returns `true` if this mode is purely compositional (alpha-driven)
    /// rather than a color blend; currently an alias for `is_porter_duff`.
    #[must_use]
    #[inline]
    pub const fn is_compositional(&self) -> bool {
        self.is_porter_duff()
    }
}
