//! ColorFilterLayer - Color transformation layer
//!
//! This layer applies a color filter (5x4 color matrix) to its children.
//! Corresponds to Flutter's `ColorFilterLayer`.

use flui_types::painting::effects::ColorMatrix;

/// Layer that applies a color filter to its children.
///
/// Color filters transform the color of every pixel rendered by children
/// using a 5x4 color matrix. This enables effects like:
/// - Grayscale conversion
/// - Sepia tone
/// - Brightness/contrast adjustment
/// - Saturation adjustment
/// - Hue rotation
/// - Color inversion
///
/// # Performance
///
/// Color filter layers require offscreen rendering and per-pixel computation.
/// For simple color changes, consider using Paint colors directly.
///
/// # Architecture
///
/// ```text
/// ColorFilterLayer
///   │
///   │ Render children to offscreen buffer
///   │ Apply 5x4 color matrix to each pixel
///   ▼
/// Children rendered with color transformation
/// ```
///
/// # Example
///
/// ```rust
/// use flui_layer::ColorFilterLayer;
///
/// // Create grayscale filter
/// let layer = ColorFilterLayer::grayscale();
///
/// // Create custom brightness adjustment
/// let bright = ColorFilterLayer::brightness(0.2);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ColorFilterLayer {
    /// The color transformation matrix
    color_filter: ColorMatrix,
}

impl ColorFilterLayer {
    /// Creates a new color filter layer with a custom matrix.
    #[inline]
    pub fn new(color_filter: ColorMatrix) -> Self {
        Self { color_filter }
    }

    /// Creates an identity color filter (no transformation).
    #[inline]
    pub fn identity() -> Self {
        Self::new(ColorMatrix::identity())
    }

    /// Creates a grayscale color filter.
    ///
    /// Converts colors to grayscale using luminance weights.
    #[inline]
    pub fn grayscale() -> Self {
        Self::new(ColorMatrix::grayscale())
    }

    /// Creates a sepia tone color filter.
    ///
    /// Gives a warm, vintage look to images.
    #[inline]
    pub fn sepia() -> Self {
        Self::new(ColorMatrix::sepia())
    }

    /// Creates a brightness adjustment filter.
    ///
    /// # Arguments
    ///
    /// * `amount` - Brightness adjustment (-1.0 to 1.0, 0.0 = no change)
    #[inline]
    pub fn brightness(amount: f32) -> Self {
        Self::new(ColorMatrix::brightness(amount))
    }

    /// Creates a contrast adjustment filter.
    ///
    /// # Arguments
    ///
    /// * `amount` - Contrast multiplier (0.0 to 2.0, 1.0 = no change)
    #[inline]
    pub fn contrast(amount: f32) -> Self {
        Self::new(ColorMatrix::contrast(amount))
    }

    /// Creates a saturation adjustment filter.
    ///
    /// # Arguments
    ///
    /// * `amount` - Saturation multiplier (0.0 = grayscale, 1.0 = no change, 2.0 = double saturation)
    #[inline]
    pub fn saturation(amount: f32) -> Self {
        Self::new(ColorMatrix::saturation(amount))
    }

    /// Creates a hue rotation filter.
    ///
    /// # Arguments
    ///
    /// * `degrees` - Hue rotation in degrees (0.0 to 360.0)
    #[inline]
    pub fn hue_rotate(degrees: f32) -> Self {
        Self::new(ColorMatrix::hue_rotate(degrees))
    }

    /// Creates a color inversion filter.
    ///
    /// Inverts all color channels (like a photo negative).
    #[inline]
    pub fn invert() -> Self {
        Self::new(ColorMatrix::invert())
    }

    /// Returns a reference to the color matrix.
    #[inline]
    pub fn color_filter(&self) -> &ColorMatrix {
        &self.color_filter
    }

    /// Sets the color matrix.
    #[inline]
    pub fn set_color_filter(&mut self, color_filter: ColorMatrix) {
        self.color_filter = color_filter;
    }

    /// Returns true if this is an identity matrix (no transformation).
    #[inline]
    pub fn is_identity(&self) -> bool {
        self.color_filter == ColorMatrix::identity()
    }

    /// Applies the color filter to a color.
    ///
    /// # Arguments
    ///
    /// * `color` - Input color as [r, g, b, a] where each component is 0.0-1.0
    ///
    /// # Returns
    ///
    /// Transformed color as [r, g, b, a]
    #[inline]
    pub fn apply(&self, color: [f32; 4]) -> [f32; 4] {
        self.color_filter.apply(color)
    }

    /// Combines this filter with another.
    ///
    /// The result applies `other` first, then `self`.
    #[inline]
    pub fn then(&self, other: &ColorFilterLayer) -> ColorFilterLayer {
        ColorFilterLayer::new(self.color_filter.multiply(&other.color_filter))
    }
}

impl Default for ColorFilterLayer {
    fn default() -> Self {
        Self::identity()
    }
}

// Thread safety
unsafe impl Send for ColorFilterLayer {}
unsafe impl Sync for ColorFilterLayer {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_filter_layer_new() {
        let matrix = ColorMatrix::grayscale();
        let layer = ColorFilterLayer::new(matrix.clone());

        assert_eq!(layer.color_filter(), &matrix);
    }

    #[test]
    fn test_color_filter_layer_identity() {
        let layer = ColorFilterLayer::identity();

        assert!(layer.is_identity());

        // Identity should not change colors
        let color = [0.5, 0.6, 0.7, 0.8];
        let result = layer.apply(color);
        for i in 0..4 {
            assert!((result[i] - color[i]).abs() < 0.001);
        }
    }

    #[test]
    fn test_color_filter_layer_grayscale() {
        let layer = ColorFilterLayer::grayscale();

        // Red should become gray
        let red = [1.0, 0.0, 0.0, 1.0];
        let result = layer.apply(red);

        // All RGB channels should be equal (grayscale)
        assert!((result[0] - result[1]).abs() < 0.001);
        assert!((result[1] - result[2]).abs() < 0.001);
        assert_eq!(result[3], 1.0); // Alpha unchanged
    }

    #[test]
    fn test_color_filter_layer_sepia() {
        let layer = ColorFilterLayer::sepia();

        assert!(!layer.is_identity());
    }

    #[test]
    fn test_color_filter_layer_brightness() {
        let layer = ColorFilterLayer::brightness(0.2);

        let gray = [0.5, 0.5, 0.5, 1.0];
        let result = layer.apply(gray);

        // Should be brighter
        assert!(result[0] > gray[0]);
        assert!(result[1] > gray[1]);
        assert!(result[2] > gray[2]);
    }

    #[test]
    fn test_color_filter_layer_contrast() {
        let layer = ColorFilterLayer::contrast(1.5);

        assert!(!layer.is_identity());
    }

    #[test]
    fn test_color_filter_layer_saturation() {
        // Zero saturation should give grayscale
        let layer = ColorFilterLayer::saturation(0.0);

        let red = [1.0, 0.0, 0.0, 1.0];
        let result = layer.apply(red);

        // Should be grayscale
        assert!((result[0] - result[1]).abs() < 0.001);
        assert!((result[1] - result[2]).abs() < 0.001);
    }

    #[test]
    fn test_color_filter_layer_hue_rotate() {
        let layer = ColorFilterLayer::hue_rotate(180.0);

        assert!(!layer.is_identity());
    }

    #[test]
    fn test_color_filter_layer_invert() {
        let layer = ColorFilterLayer::invert();

        let black = [0.0, 0.0, 0.0, 1.0];
        let result = layer.apply(black);

        // Black should become white
        assert!((result[0] - 1.0).abs() < 0.001);
        assert!((result[1] - 1.0).abs() < 0.001);
        assert!((result[2] - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_color_filter_layer_set_color_filter() {
        let mut layer = ColorFilterLayer::identity();

        layer.set_color_filter(ColorMatrix::grayscale());
        assert!(!layer.is_identity());
    }

    #[test]
    fn test_color_filter_layer_then() {
        let brightness = ColorFilterLayer::brightness(0.1);
        let grayscale = ColorFilterLayer::grayscale();

        let combined = grayscale.then(&brightness);

        // Should not be identity
        assert!(!combined.is_identity());
    }

    #[test]
    fn test_color_filter_layer_default() {
        let layer = ColorFilterLayer::default();

        assert!(layer.is_identity());
    }

    #[test]
    fn test_color_filter_layer_clone() {
        let layer = ColorFilterLayer::grayscale();
        let cloned = layer.clone();

        assert_eq!(layer, cloned);
    }

    #[test]
    fn test_color_filter_layer_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<ColorFilterLayer>();
        assert_sync::<ColorFilterLayer>();
    }
}
