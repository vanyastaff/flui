//! `ColorFilterLayer` — applies a [`ColorFilter`] to its children.
//!
//! Corresponds to Flutter's `ColorFilterLayer`.

use flui_types::painting::{effects::ColorMatrix, ColorFilter};

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
    /// Creates a new color filter layer with the given [`ColorFilter`].
    #[inline]
    #[must_use]
    pub const fn new(color_filter: ColorFilter) -> Self {
        Self { color_filter }
    }

    /// Creates an identity filter layer (no color transformation).
    ///
    /// The render impl short-circuits identity layers to a no-op.
    #[inline]
    #[must_use]
    pub fn identity() -> Self {
        Self::new(ColorFilter::Matrix(ColorMatrix::identity()))
    }

    /// Returns the [`ColorFilter`] this layer applies.
    ///
    /// `ColorFilter` is `Copy`, so the value is returned by value at no cost.
    #[inline]
    #[must_use]
    pub const fn color_filter(&self) -> ColorFilter {
        self.color_filter
    }

    /// Replaces the active [`ColorFilter`].
    #[inline]
    pub fn set_color_filter(&mut self, color_filter: ColorFilter) {
        self.color_filter = color_filter;
    }

    /// Returns `true` if this layer applies no transformation.
    ///
    /// Only a `Matrix`-variant that equals the identity matrix is considered
    /// identity.  `Mode` and `Gamma` variants are never identity — they always
    /// affect pixel values.
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
        painting::{effects::ColorMatrix, BlendMode, ColorFilter},
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

    #[test]
    fn layer_is_copy() {
        let a = ColorFilterLayer::new(ColorFilter::grayscale());
        let b = a; // Copy
        assert_eq!(a, b);
    }

    #[test]
    fn layer_is_clone() {
        // `ColorFilterLayer: Copy`; route through a generic `&T` call so
        // clippy's `clone_on_copy` lint doesn't fire.
        fn clone_it<T: Clone>(v: &T) -> T {
            v.clone()
        }
        let a = ColorFilterLayer::new(ColorFilter::grayscale());
        let b = clone_it(&a);
        assert_eq!(a, b);
    }

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

    #[test]
    fn set_color_filter_updates_field() {
        let mut layer = ColorFilterLayer::identity();
        assert!(layer.is_identity());

        layer.set_color_filter(ColorFilter::grayscale());
        assert!(!layer.is_identity());
        assert_eq!(layer.color_filter(), ColorFilter::grayscale());
    }

    #[test]
    fn set_mode_filter() {
        let mut layer = ColorFilterLayer::identity();
        let mode_filter = ColorFilter::mode(Color::RED, BlendMode::Multiply);
        layer.set_color_filter(mode_filter);
        assert_eq!(layer.color_filter(), mode_filter);
        assert!(!layer.is_identity());
    }

    // ── Send + Sync ───────────────────────────────────────────────────────────

    #[test]
    fn layer_is_send_and_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<ColorFilterLayer>();
        assert_sync::<ColorFilterLayer>();
    }
}
