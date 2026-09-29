//! `ImageFilterLayer` — filters its subtree's pixels: blur, dilate, erode, colour matrix.

use flui_foundation::geometry::Offset;
use flui_painting::paint::effects::ImageFilter;

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
    offset: Offset<f64>,
}

impl ImageFilterLayer {
    /// Filters the subtree's pixels with `filter`.
    #[inline]
    pub fn new(filter: ImageFilter) -> Self {
        Self {
            filter,
            offset: Offset::ZERO,
        }
    }

    /// Like [`Self::new`], also translating the subtree by `offset`.
    #[inline]
    pub fn with_offset(filter: ImageFilter, offset: Offset<f64>) -> Self {
        Self { filter, offset }
    }

    /// A Gaussian blur with standard deviation `sigma` on both axes.
    #[inline]
    pub fn blur(sigma: f64) -> Self {
        Self::new(ImageFilter::blur(sigma))
    }

    /// A Gaussian blur with per-axis standard deviations.
    #[inline]
    pub fn blur_xy(sigma_x: f64, sigma_y: f64) -> Self {
        Self::new(ImageFilter::blur_directional(sigma_x, sigma_y))
    }

    /// A morphological dilation by `radius` pixels: bright regions grow (glow).
    #[inline]
    pub fn dilate(radius: f64) -> Self {
        Self::new(ImageFilter::dilate(radius))
    }

    /// A morphological erosion by `radius` pixels: bright regions shrink.
    #[inline]
    pub fn erode(radius: f64) -> Self {
        Self::new(ImageFilter::erode(radius))
    }

    /// A colour-matrix filter.
    #[inline]
    pub fn matrix(matrix: flui_painting::paint::effects::ColorMatrix) -> Self {
        Self::new(ImageFilter::matrix(matrix))
    }

    /// The filter applied to the subtree.
    #[inline]
    pub fn filter(&self) -> &ImageFilter {
        &self.filter
    }

    /// The translation applied to the subtree (see [`Self::with_offset`]).
    #[inline]
    pub fn offset(&self) -> Offset<f64> {
        self.offset
    }

    /// Whether the layer translates its subtree.
    #[inline]
    pub fn has_offset(&self) -> bool {
        !self.offset.is_zero()
    }
}
