//! ImageFilterLayer - Image filter effects layer
//!
//! This layer applies image filters (blur, dilate, erode, etc.) to its children.
//! Corresponds to Flutter's `ImageFilterLayer`.

use flui_types::geometry::Pixels;
use flui_types::painting::effects::ImageFilter;
use flui_types::Offset;

/// Layer that applies an image filter to its children.
///
/// Image filters process the rendered content of children as an image,
/// applying effects like:
/// - Gaussian blur
/// - Dilate (expand bright areas)
/// - Erode (shrink bright areas)
/// - Composed filters
///
/// # Performance
///
/// Image filters require offscreen rendering and are computationally expensive.
/// Blur in particular can be slow for large sigma values.
///
/// # Difference from BackdropFilterLayer
///
/// - `ImageFilterLayer`: Applies filter to children's content
/// - `BackdropFilterLayer`: Applies filter to content *behind* the layer
///
/// # Architecture
///
/// ```text
/// ImageFilterLayer
///   │
///   │ Render children to offscreen buffer
///   │ Apply image filter (GPU compute/fragment shader)
///   ▼
/// Children rendered with filter effect
/// ```
///
/// # Example
///
/// ```rust
/// use flui_layer::ImageFilterLayer;
///
/// // Create blur filter
/// let layer = ImageFilterLayer::blur(5.0);
///
/// // Create blur with different x/y sigma
/// let directional_blur = ImageFilterLayer::blur_xy(10.0, 2.0);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ImageFilterLayer {
    /// The image filter to apply
    filter: ImageFilter,

    /// Optional offset (for optimization)
    offset: Offset<Pixels>,
}

impl ImageFilterLayer {
    /// Creates a new image filter layer.
    #[inline]
    pub fn new(filter: ImageFilter) -> Self {
        Self {
            filter,
            offset: Offset::ZERO,
        }
    }

    /// Creates an image filter layer with an offset.
    ///
    /// Combining offset with the filter avoids needing a separate OffsetLayer.
    #[inline]
    pub fn with_offset(filter: ImageFilter, offset: Offset<Pixels>) -> Self {
        Self { filter, offset }
    }

    /// Creates a Gaussian blur filter.
    ///
    /// # Arguments
    ///
    /// * `sigma` - Blur radius (standard deviation) for both axes
    #[inline]
    pub fn blur(sigma: f32) -> Self {
        Self::new(ImageFilter::blur(sigma))
    }

    /// Creates a directional Gaussian blur filter.
    ///
    /// # Arguments
    ///
    /// * `sigma_x` - Horizontal blur radius
    /// * `sigma_y` - Vertical blur radius
    #[inline]
    pub fn blur_xy(sigma_x: f32, sigma_y: f32) -> Self {
        Self::new(ImageFilter::blur_directional(sigma_x, sigma_y))
    }

    /// Creates a dilate filter.
    ///
    /// Dilation expands bright regions and shrinks dark regions.
    /// Useful for creating glow effects.
    ///
    /// # Arguments
    ///
    /// * `radius` - Dilation radius in pixels
    #[inline]
    pub fn dilate(radius: f32) -> Self {
        Self::new(ImageFilter::dilate(radius))
    }

    /// Creates an erode filter.
    ///
    /// Erosion shrinks bright regions and expands dark regions.
    /// Opposite of dilate.
    ///
    /// # Arguments
    ///
    /// * `radius` - Erosion radius in pixels
    #[inline]
    pub fn erode(radius: f32) -> Self {
        Self::new(ImageFilter::erode(radius))
    }

    /// Creates a filter from a color matrix.
    #[inline]
    pub fn matrix(matrix: flui_types::painting::effects::ColorMatrix) -> Self {
        Self::new(ImageFilter::matrix(matrix))
    }

    /// Returns a reference to the image filter.
    #[inline]
    pub fn filter(&self) -> &ImageFilter {
        &self.filter
    }

    /// Sets the image filter.
    #[inline]
    pub fn set_filter(&mut self, filter: ImageFilter) {
        self.filter = filter;
    }

    /// Returns the offset.
    #[inline]
    pub fn offset(&self) -> Offset<Pixels> {
        self.offset
    }

    /// Sets the offset.
    #[inline]
    pub fn set_offset(&mut self, offset: Offset<Pixels>) {
        self.offset = offset;
    }

    /// Returns true if this layer has a non-zero offset.
    #[inline]
    pub fn has_offset(&self) -> bool {
        use flui_types::geometry::px;
        self.offset.dx != px(0.0) || self.offset.dy != px(0.0)
    }

    /// Returns the blur sigma values if this is a blur filter.
    ///
    /// Returns `None` for non-blur filters.
    pub fn blur_sigma(&self) -> Option<(f32, f32)> {
        match &self.filter {
            ImageFilter::Blur { sigma_x, sigma_y } => Some((*sigma_x, *sigma_y)),
            _ => None,
        }
    }

    /// Returns true if this is a blur filter.
    #[inline]
    pub fn is_blur(&self) -> bool {
        matches!(self.filter, ImageFilter::Blur { .. })
    }

    /// Returns true if this is a dilate filter.
    #[inline]
    pub fn is_dilate(&self) -> bool {
        matches!(self.filter, ImageFilter::Dilate { .. })
    }

    /// Returns true if this is an erode filter.
    #[inline]
    pub fn is_erode(&self) -> bool {
        matches!(self.filter, ImageFilter::Erode { .. })
    }
}

// Thread safety
unsafe impl Send for ImageFilterLayer {}
unsafe impl Sync for ImageFilterLayer {}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::px;

    #[test]
    fn test_image_filter_layer_new() {
        let filter = ImageFilter::blur(5.0);
        let layer = ImageFilterLayer::new(filter.clone());

        assert_eq!(layer.filter(), &filter);
        assert_eq!(layer.offset(), Offset::ZERO);
    }

    #[test]
    fn test_image_filter_layer_with_offset() {
        let filter = ImageFilter::blur(5.0);
        let offset = Offset::new(px(10.0), px(20.0));
        let layer = ImageFilterLayer::with_offset(filter, offset);

        assert!(layer.has_offset());
        assert_eq!(layer.offset().dx, px(10.0));
        assert_eq!(layer.offset().dy, px(20.0));
    }

    #[test]
    fn test_image_filter_layer_blur() {
        let layer = ImageFilterLayer::blur(10.0);

        assert!(layer.is_blur());
        assert!(!layer.is_dilate());
        assert!(!layer.is_erode());

        let sigma = layer.blur_sigma().unwrap();
        assert_eq!(sigma, (10.0, 10.0));
    }

    #[test]
    fn test_image_filter_layer_blur_xy() {
        let layer = ImageFilterLayer::blur_xy(5.0, 15.0);

        assert!(layer.is_blur());

        let sigma = layer.blur_sigma().unwrap();
        assert_eq!(sigma, (5.0, 15.0));
    }

    #[test]
    fn test_image_filter_layer_dilate() {
        let layer = ImageFilterLayer::dilate(3.0);

        assert!(layer.is_dilate());
        assert!(!layer.is_blur());
        assert!(layer.blur_sigma().is_none());
    }

    #[test]
    fn test_image_filter_layer_erode() {
        let layer = ImageFilterLayer::erode(2.0);

        assert!(layer.is_erode());
        assert!(!layer.is_blur());
    }

    #[test]
    fn test_image_filter_layer_setters() {
        let mut layer = ImageFilterLayer::blur(5.0);

        layer.set_filter(ImageFilter::dilate(3.0));
        assert!(layer.is_dilate());

        layer.set_offset(Offset::new(px(5.0), px(10.0)));
        assert!(layer.has_offset());
    }

    #[test]
    fn test_image_filter_layer_clone() {
        let layer = ImageFilterLayer::blur(5.0);
        let cloned = layer.clone();

        assert_eq!(layer, cloned);
    }

    #[test]
    fn test_image_filter_layer_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<ImageFilterLayer>();
        assert_sync::<ImageFilterLayer>();
    }
}
