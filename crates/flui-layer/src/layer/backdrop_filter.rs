//! `BackdropFilterLayer` — filters what is already painted behind it: frosted glass.

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
/// use flui_types::geometry::px;
/// use flui_layer::BackdropFilterLayer;
/// use flui_types::{
///     geometry::Rect,
///     painting::{BlendMode, ImageFilter},
/// };
///
/// // Create frosted glass effect
/// let frosted_glass = BackdropFilterLayer::new(
///     ImageFilter::blur(10.0), // 10px gaussian blur
///     BlendMode::SrcOver,
///     Rect::from_xywh(px(0.0), px(0.0), px(400.0), px(300.0)),
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
    /// Filters what is already painted behind `bounds` with `filter`, compositing with `blend_mode`.
    pub fn new(filter: ImageFilter, blend_mode: BlendMode, bounds: Rect<Pixels>) -> Self {
        Self {
            filter,
            blend_mode,
            bounds,
        }
    }

    /// The filter applied to the backdrop.
    pub fn filter(&self) -> &ImageFilter {
        &self.filter
    }

    /// How the filtered backdrop composites.
    pub fn blend_mode(&self) -> BlendMode {
        self.blend_mode
    }

    /// The rectangle of backdrop captured and filtered.
    pub fn bounds(&self) -> Rect<Pixels> {
        self.bounds
    }
}

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    #[test]
    fn test_backdrop_filter_layer_new() {
        let filter = ImageFilter::blur(5.0);
        let blend_mode = BlendMode::SrcOver;
        let bounds = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));

        let layer = BackdropFilterLayer::new(filter, blend_mode, bounds);

        assert_eq!(layer.bounds(), bounds);
        assert_eq!(layer.blend_mode(), BlendMode::SrcOver);
    }

    #[test]
    fn test_backdrop_filter_layer_bounds() {
        let filter = ImageFilter::blur(10.0);
        let bounds = Rect::from_xywh(px(10.0), px(20.0), px(200.0), px(150.0));

        let layer = BackdropFilterLayer::new(filter, BlendMode::SrcOver, bounds);

        let retrieved_bounds = layer.bounds();
        assert_eq!(retrieved_bounds, bounds);
        assert_eq!(retrieved_bounds.width(), px(200.0));
        assert_eq!(retrieved_bounds.height(), px(150.0));
    }

    #[test]
    fn test_backdrop_filter_layer_blur() {
        let filter = ImageFilter::Blur {
            sigma_x: 5.0,
            sigma_y: 5.0,
        };
        let bounds = Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0));

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
        let bounds = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));

        let layer = BackdropFilterLayer::new(filter, BlendMode::Screen, bounds);

        assert_eq!(layer.blend_mode(), BlendMode::Screen);
        match layer.filter() {
            ImageFilter::ColorAdjust(ColorAdjustment::Brightness(brightness)) => {
                assert_eq!(*brightness, 0.2);
            }
            _ => panic!("Expected ColorAdjust filter with Brightness"),
        }
    }
}
