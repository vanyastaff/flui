//! `ColorFilterLayer` — recolours its subtree with a [`ColorFilter`].

use flui_types::painting::{ColorFilter, effects::ColorMatrix};

/// Layer that applies a [`ColorFilter`] to its children.
///
/// Color filters transform the color of every pixel rendered by children.
/// The full [`ColorFilter`] enum is supported:
///
/// | Variant | Effect |
/// |---|---|
/// | [`ColorFilter::Matrix`] | 5×4 matrix in un-premultiplied RGBA |
/// | [`ColorFilter::Mode`] | Porter-Duff / W3C blend of a solid color |
/// | [`ColorFilter::LinearToSrgbGamma`] | linear → sRGB transfer per RGB channel |
/// | [`ColorFilter::SrgbToLinearGamma`] | sRGB → linear transfer per RGB channel |
///
/// Use the constructors on [`ColorFilter`] directly to build the filter value:
///
/// ```rust
/// use flui_layer::ColorFilterLayer;
/// use flui_types::painting::{ColorFilter, BlendMode};
/// use flui_types::styling::Color;
/// use flui_types::painting::effects::ColorMatrix;
///
/// // Matrix-based filter (e.g. grayscale).
/// let layer = ColorFilterLayer::new(ColorFilter::grayscale());
///
/// // Mode-based filter: tint with 50% opacity blue.
/// let tint = ColorFilterLayer::new(
///     ColorFilter::mode(Color::BLUE, BlendMode::SrcOver),
/// );
///
/// // Identity (no transformation): equivalent to no filter layer.
/// let identity = ColorFilterLayer::identity();
/// ```
///
/// # Performance
///
/// Color filter layers require offscreen rendering and per-pixel computation.
/// `Matrix`-identity layers are short-circuited in the render impl — they
/// emit a no-op `save_layer` rather than a full GPU pass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorFilterLayer {
    /// The color filter to apply to the layer's children.
    color_filter: ColorFilter,
}

impl ColorFilterLayer {
    /// Recolours the subtree with `color_filter`.
    #[inline]
    #[must_use]
    pub const fn new(color_filter: ColorFilter) -> Self {
        Self { color_filter }
    }

    /// A filter that changes nothing; the engine lowers it to a no-op.
    #[inline]
    #[must_use]
    pub fn identity() -> Self {
        Self::new(ColorFilter::Matrix(ColorMatrix::identity()))
    }

    /// The filter applied to the subtree (`ColorFilter` is `Copy`).
    #[inline]
    #[must_use]
    pub const fn color_filter(&self) -> ColorFilter {
        self.color_filter
    }

    /// Whether the filter changes nothing: only a `Matrix` equal to the identity qualifies;
    /// `Mode` and `Gamma` always affect pixels.
    #[inline]
    #[must_use]
    pub fn is_identity(&self) -> bool {
        match self.color_filter {
            ColorFilter::Matrix(m) => m == ColorMatrix::identity(),
            _ => false,
        }
    }
}

impl Default for ColorFilterLayer {
    #[inline]
    fn default() -> Self {
        Self::identity()
    }
}

#[cfg(test)]
mod tests {
    use flui_types::{
        painting::{BlendMode, ColorFilter, effects::ColorMatrix},
        styling::Color,
    };

    use super::*;

    // ── Construction ──────────────────────────────────────────────────────────

    #[test]
    fn new_stores_filter() {
        let filter = ColorFilter::grayscale();
        let layer = ColorFilterLayer::new(filter);
        assert_eq!(layer.color_filter(), filter);
    }

    #[test]
    fn identity_is_matrix_identity() {
        let layer = ColorFilterLayer::identity();
        assert!(layer.is_identity());
        assert!(matches!(layer.color_filter(), ColorFilter::Matrix(_)));
    }

    #[test]
    fn default_is_identity() {
        let layer = ColorFilterLayer::default();
        assert!(layer.is_identity());
    }

    // ── Copy + Clone ──────────────────────────────────────────────────────────

    // ── is_identity semantics ─────────────────────────────────────────────────

    #[test]
    fn matrix_identity_is_identity() {
        let layer = ColorFilterLayer::new(ColorFilter::Matrix(ColorMatrix::identity()));
        assert!(layer.is_identity());
    }

    #[test]
    fn non_identity_matrix_is_not_identity() {
        let layer = ColorFilterLayer::new(ColorFilter::grayscale());
        assert!(!layer.is_identity());
    }

    #[test]
    fn mode_filter_is_never_identity() {
        let layer = ColorFilterLayer::new(ColorFilter::mode(Color::WHITE, BlendMode::SrcOver));
        assert!(
            !layer.is_identity(),
            "Mode filter must not be classified as identity even with white+SrcOver"
        );
    }

    #[test]
    fn linear_to_srgb_gamma_is_never_identity() {
        let layer = ColorFilterLayer::new(ColorFilter::LinearToSrgbGamma);
        assert!(!layer.is_identity());
    }

    #[test]
    fn srgb_to_linear_gamma_is_never_identity() {
        let layer = ColorFilterLayer::new(ColorFilter::SrgbToLinearGamma);
        assert!(!layer.is_identity());
    }

    // ── set_color_filter ──────────────────────────────────────────────────────

    // ── Send + Sync ───────────────────────────────────────────────────────────
}
