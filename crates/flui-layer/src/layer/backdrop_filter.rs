//! BackdropFilterLayer - Applies image filter to backdrop content
//!
//! This layer type enables effects like frosted glass by capturing the content
//! behind a widget and applying blur or color filters before compositing the
//! child content on top.

use flui_types::{
    geometry::{Pixels, Rect},
    painting::{BlendMode, ImageFilter},
};

/// Layer that applies an image filter to backdrop content
///
/// # Architecture
///
/// ```text
/// Backdrop Content → Capture Texture → Apply Filter → Render Child → Composite
/// ```
///
/// # Rendering Process
///
/// 1. Capture current framebuffer content in specified bounds
/// 2. Apply image filter (blur, color adjustments, etc.) via GPU
/// 3. Render filtered backdrop to framebuffer
/// 4. Render child content on top (if present)
/// 5. Composite with blend mode
///
/// # Example
///
/// ```rust
/// use flui_layer::BackdropFilterLayer;
/// use flui_types::painting::{BlendMode, ImageFilter};
/// use flui_types::geometry::Rect;
///
/// // Create frosted glass effect
/// let frosted_glass = BackdropFilterLayer::new(
///     ImageFilter::blur(10.0),  // 10px gaussian blur
///     BlendMode::SrcOver,
///     Rect::from_xywh(0.0, 0.0, 400.0, 300.0),
/// );
/// ```
#[derive(Debug, Clone)]
pub struct BackdropFilterLayer {
    /// Image filter to apply to backdrop
    filter: ImageFilter,

    /// Blend mode for compositing
    blend_mode: BlendMode,

    /// Bounds for backdrop capture (pre-computed for performance)
    bounds: Rect<Pixels>,
}

impl BackdropFilterLayer {
    /// Create new backdrop filter layer
    ///
    /// # Arguments
    ///
    /// * `filter` - Image filter (blur, color adjustments, etc.)
    /// * `blend_mode` - Blend mode for compositing
    /// * `bounds` - Bounding rectangle for backdrop capture
    pub fn new(filter: ImageFilter, blend_mode: BlendMode, bounds: Rect<Pixels>) -> Self {
        Self {
            filter,
            blend_mode,
            bounds,
        }
    }

    /// Get the image filter.
    pub fn filter(&self) -> &ImageFilter {
        &self.filter
    }

    /// Get the blend mode.
    pub fn blend_mode(&self) -> BlendMode {
        self.blend_mode
    }

    /// Get the bounding rectangle of this layer.
    pub fn bounds(&self) -> Rect<Pixels> {
        self.bounds
    }

    /// Set new bounds for this layer.
    pub fn set_bounds(&mut self, bounds: Rect<Pixels>) {
        self.bounds = bounds;
    }
}

// Thread safety: BackdropFilterLayer contains only owned, Send types
unsafe impl Send for BackdropFilterLayer {}
unsafe impl Sync for BackdropFilterLayer {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backdrop_filter_layer_new() {
        let filter = ImageFilter::blur(5.0);
        let blend_mode = BlendMode::SrcOver;
        let bounds = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);

        let layer = BackdropFilterLayer::new(filter, blend_mode, bounds);

        assert_eq!(layer.bounds(), bounds);
        assert_eq!(layer.blend_mode(), BlendMode::SrcOver);
    }

    #[test]
    fn test_backdrop_filter_layer_bounds() {
        let filter = ImageFilter::blur(10.0);
        let bounds = Rect::from_xywh(10.0, 20.0, 200.0, 150.0);

        let layer = BackdropFilterLayer::new(filter, BlendMode::SrcOver, bounds);

        let retrieved_bounds = layer.bounds();
        assert_eq!(retrieved_bounds, bounds);
        assert_eq!(retrieved_bounds.width(), 200.0);
        assert_eq!(retrieved_bounds.height(), 150.0);
    }

    #[test]
    fn test_backdrop_filter_layer_blur() {
        let filter = ImageFilter::Blur {
            sigma_x: 5.0,
            sigma_y: 5.0,
        };
        let bounds = Rect::from_xywh(0.0, 0.0, 50.0, 50.0);

        let layer = BackdropFilterLayer::new(filter, BlendMode::Multiply, bounds);

        assert_eq!(layer.blend_mode(), BlendMode::Multiply);
        match layer.filter() {
            ImageFilter::Blur { sigma_x, sigma_y } => {
                assert_eq!(*sigma_x, 5.0);
                assert_eq!(*sigma_y, 5.0);
            }
            _ => panic!("Expected Blur filter"),
        }
    }

    #[test]
    fn test_backdrop_filter_layer_color_filter() {
        use flui_types::painting::effects::ColorAdjustment;

        let filter = ImageFilter::ColorAdjust(ColorAdjustment::Brightness(0.2));
        let bounds = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);

        let layer = BackdropFilterLayer::new(filter, BlendMode::Screen, bounds);

        assert_eq!(layer.blend_mode(), BlendMode::Screen);
        match layer.filter() {
            ImageFilter::ColorAdjust(ColorAdjustment::Brightness(brightness)) => {
                assert_eq!(*brightness, 0.2);
            }
            _ => panic!("Expected ColorAdjust filter with Brightness"),
        }
    }

    #[test]
    fn test_backdrop_filter_layer_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<BackdropFilterLayer>();
        assert_sync::<BackdropFilterLayer>();
    }

    #[test]
    fn test_backdrop_filter_layer_clone() {
        let filter = ImageFilter::blur(5.0);
        let bounds = Rect::from_xywh(0.0, 0.0, 50.0, 50.0);
        let layer = BackdropFilterLayer::new(filter, BlendMode::SrcOver, bounds);

        let cloned = layer.clone();
        assert_eq!(cloned.bounds(), layer.bounds());
        assert_eq!(cloned.blend_mode(), layer.blend_mode());
    }
}
