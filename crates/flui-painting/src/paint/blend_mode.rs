//! Blend modes for compositing colors.

/// Algorithms for blending a source (the pixels being drawn) with a
/// destination (the pixels already on the canvas).
///
/// The first fourteen variants are Porter-Duff compositing operators; the
/// rest are "advanced" (separable and non-separable) blend modes matching
/// CSS/Skia semantics.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(flui_foundation::inspect::Inspect)]
#[expect(
    clippy::exhaustive_enums,
    reason = "the Porter-Duff and W3C compositing modes are a fixed list, and each is classified exhaustively"
)]
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

/// How a blend mode treats the destination where its source is partly or
/// wholly transparent: the one classification that coverage, compositing and
/// damage decisions route on.
///
/// A renderer reports partial coverage `c` either by folding it into the
/// source (scaling the premultiplied source by `c`) or by interpolating
/// `mix(destination, blend(source, destination), c)`. The class says whether
/// the two agree, and whether a transparent source keeps the destination.
///
/// Every variant of [`BlendMode`] is in exactly one class, by an exhaustive
/// match in [`BlendMode::coverage_class`], so a new mode is classified before
/// any of these decisions can see it.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[expect(
    clippy::exhaustive_enums,
    reason = "each decision that folds coverage handles every class, and a `_` arm would fold a new one wrongly"
)]
pub enum CoverageClass {
    /// The destination factor is `One` or `1 - srcAlpha`: a transparent
    /// source keeps the destination, and folding coverage is exact.
    /// `Dst`, `SrcOver`, `DstOver`, `DstOut`, `SrcATop`, `Xor`.
    Absorbing,
    /// `Plus`: a transparent source keeps the destination, but the sum
    /// saturates, so coverage folded in before the clamp differs from
    /// coverage interpolated after it.
    Saturating,
    /// The destination factor is `Zero`: a transparent source clears the
    /// destination, and folding coverage is wrong.
    /// `Clear`, `Src`, `SrcIn`, `SrcOut`, `Modulate`.
    Replacing,
    /// The destination factor is `srcAlpha`: a transparent source clears the
    /// destination, and folding coverage is wrong. `DstIn`, `DstATop`.
    Masking,
    /// The fifteen separable and non-separable modes, `Screen` through
    /// `Luminosity`: composited source-over with a blend function of the
    /// destination colour, which no fixed-function blend state expresses. A
    /// transparent source keeps the destination, and folding coverage is
    /// exact, because the composite is linear in source alpha.
    Advanced,
}

impl BlendMode {
    /// The mode's [`CoverageClass`].
    #[must_use]
    #[inline]
    pub const fn coverage_class(self) -> CoverageClass {
        match self {
            BlendMode::Dst
            | BlendMode::SrcOver
            | BlendMode::DstOver
            | BlendMode::DstOut
            | BlendMode::SrcATop
            | BlendMode::Xor => CoverageClass::Absorbing,
            BlendMode::Plus => CoverageClass::Saturating,
            BlendMode::Clear
            | BlendMode::Src
            | BlendMode::SrcIn
            | BlendMode::SrcOut
            | BlendMode::Modulate => CoverageClass::Replacing,
            BlendMode::DstIn | BlendMode::DstATop => CoverageClass::Masking,
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
            | BlendMode::Hue
            | BlendMode::Saturation
            | BlendMode::Color
            | BlendMode::Luminosity => CoverageClass::Advanced,
        }
    }

    /// Returns `true` if this is one of the fourteen Porter-Duff compositing
    /// operators (`Clear` through `Modulate`): every mode whose
    /// [`coverage_class`](Self::coverage_class) is not `Advanced`.
    #[must_use]
    #[inline]
    pub const fn is_porter_duff(&self) -> bool {
        match self.coverage_class() {
            CoverageClass::Absorbing
            | CoverageClass::Saturating
            | CoverageClass::Replacing
            | CoverageClass::Masking => true,
            CoverageClass::Advanced => false,
        }
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

    /// Returns `true` if a fully transparent source leaves the destination
    /// as it was.
    ///
    /// The `Replacing` and `Masking` classes (`Clear`, `Src`, `SrcIn`,
    /// `SrcOut`, `Modulate`, `DstIn`, `DstATop`) change pixels the source
    /// never covers; every other mode, the advanced ones included, reduces to
    /// the destination there. A layer composited with a mode for which this is
    /// `false` changes every pixel of its composite area, not only the ones its
    /// content inks.
    #[must_use]
    #[inline]
    pub const fn keeps_destination_under_transparent_source(&self) -> bool {
        match self.coverage_class() {
            CoverageClass::Absorbing | CoverageClass::Saturating | CoverageClass::Advanced => true,
            CoverageClass::Replacing | CoverageClass::Masking => false,
        }
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
