//! BlurLayer - applies image filters to child layers
//!
//! This module provides compositor-level image filtering effects including blur,
//! dilate, erode, and color matrix transformations. Supports CSS filter and
//! backdrop-filter style effects.

use crate::layer::{BoxedLayer, Layer};
use crate::painter::Painter;
use flui_types::painting::effects::{BlurMode, BlurQuality, ImageFilter};
use flui_types::{Offset, Rect};
use flui_types::events::{Event, HitTestResult};

/// A layer that applies image filters to its child content or backdrop.
///
/// Supports various filter types from ImageFilter enum:
/// - Blur (gaussian)
/// - Dilate (morphological dilation)
/// - Erode (morphological erosion)
/// - Matrix (color transformation)
/// - Color (brightness, contrast, etc.)
/// - Compose (chain multiple filters)
///
/// # Examples
///
/// ```rust,ignore
/// use flui_engine::layer::BlurLayer;
/// use flui_types::painting::effects::ImageFilter;
///
/// // Simple blur
/// let blur = BlurLayer::new(child)
///     .with_filter(ImageFilter::blur(5.0));
///
/// // Dilate effect
/// let dilate = BlurLayer::new(child)
///     .with_filter(ImageFilter::dilate(2.0));
///
/// // Combined filters
/// let combined = BlurLayer::new(child)
///     .with_filter(ImageFilter::Compose(vec![
///         ImageFilter::blur(3.0),
///         ImageFilter::color(ColorFilter::Brightness(0.1)),
///     ]));
/// ```
pub struct BlurLayer {
    /// Child layer
    child: Option<BoxedLayer>,

    /// Image filter to apply
    filter: ImageFilter,

    /// Quality level for rendering
    quality: BlurQuality,

    /// Filter mode (content or backdrop)
    mode: BlurMode,

    /// Tile mode for edges (true = clamp, false = transparent)
    tile_mode_clamp: bool,

    /// Cached bounds including filter extent
    cached_bounds: Option<Rect>,

    /// Whether this layer has been disposed
    disposed: bool,
}

impl BlurLayer {
    /// Create a new image filter layer with a child
    ///
    /// Defaults to a blur filter with sigma=5.0
    ///
    /// # Arguments
    ///
    /// * `child` - Child layer to apply filter to
    #[must_use]
    pub fn new(child: BoxedLayer) -> Self {
        Self {
            child: Some(child),
            filter: ImageFilter::blur(5.0),
            quality: BlurQuality::default(),
            mode: BlurMode::default(),
            tile_mode_clamp: true,
            cached_bounds: None,
            disposed: false,
        }
    }

    /// Set the image filter
    #[must_use]
    pub fn with_filter(mut self, filter: ImageFilter) -> Self {
        self.filter = filter;
        self.cached_bounds = None;
        self
    }

    /// Set blur quality
    #[must_use]
    pub fn with_quality(mut self, quality: BlurQuality) -> Self {
        self.quality = quality;
        self
    }

    /// Set filter mode (content or backdrop)
    #[must_use]
    pub fn with_mode(mut self, mode: BlurMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set tile mode for edges
    #[must_use]
    pub fn with_tile_mode_clamp(mut self, clamp: bool) -> Self {
        self.tile_mode_clamp = clamp;
        self
    }

    /// Get the child layer
    pub fn child(&self) -> Option<&BoxedLayer> {
        self.child.as_ref()
    }

    /// Set the child layer
    pub fn set_child(&mut self, child: BoxedLayer) {
        self.child = Some(child);
        self.cached_bounds = None;
        self.mark_needs_paint();
    }

    /// Update the filter
    pub fn set_filter(&mut self, filter: ImageFilter) {
        self.filter = filter;
        self.cached_bounds = None;
        self.mark_needs_paint();
    }

    /// Calculate filter extent (how far filter effects extend beyond content)
    fn calculate_filter_extent(&self) -> f32 {
        match &self.filter {
            ImageFilter::Blur { sigma_x, sigma_y } => {
                // Gaussian blur typically extends about 3 sigma
                sigma_x.max(*sigma_y) * 3.0
            }
            ImageFilter::Dilate { radius } | ImageFilter::Erode { radius } => {
                // Morphological operations extend by their radius
                *radius
            }
            ImageFilter::Matrix(_) | ImageFilter::Color(_) => {
                // Color transformations don't extend bounds
                0.0
            }
            ImageFilter::Compose(filters) => {
                // Maximum extent of all composed filters
                filters
                    .iter()
                    .map(|f| {
                        // Recursively calculate extent for each filter
                        match f {
                            ImageFilter::Blur { sigma_x, sigma_y } => sigma_x.max(*sigma_y) * 3.0,
                            ImageFilter::Dilate { radius } | ImageFilter::Erode { radius } => *radius,
                            _ => 0.0,
                        }
                    })
                    .fold(0.0f32, f32::max)
            }
        }
    }

    /// Apply image filter by rendering (placeholder implementation)
    fn apply_filter(&self, painter: &mut dyn Painter, _bounds: Rect) {
        // Note: Proper image filter implementation requires offscreen rendering
        // with GPU shaders or CPU convolution. This is a placeholder that just
        // renders the child content.
        //
        // Production implementation would:
        // 1. Render child to offscreen texture
        // 2. Apply filter shader/convolution
        // 3. Render result to screen
        //
        // For now, we just render the child normally
        if let Some(child) = &self.child {
            painter.save();
            child.paint(painter);
            painter.restore();
        }
    }
}

impl Layer for BlurLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        if self.disposed {
            panic!("Cannot paint disposed BlurLayer");
        }

        let Some(child) = &self.child else {
            return;
        };

        painter.save();

        match self.mode {
            BlurMode::Content => {
                // Apply filter to child content
                self.apply_filter(painter, child.bounds());
            }
            BlurMode::Backdrop => {
                // Backdrop filter: filter what's behind the child
                // Note: This requires rendering the backdrop first,
                // applying filter, then rendering the child on top
                // (proper implementation needs compositor support)

                painter.save();
                // TODO: Implement proper backdrop filtering with offscreen rendering
                child.paint(painter);
                painter.restore();
            }
        }

        painter.restore();
    }

    fn bounds(&self) -> Rect {
        if let Some(bounds) = self.cached_bounds {
            return bounds;
        }

        let child_bounds = self.child.as_ref().map_or(Rect::ZERO, |c| c.bounds());

        // Expand bounds by filter extent
        let extent = self.calculate_filter_extent();
        Rect::from_xywh(
            child_bounds.left() - extent,
            child_bounds.top() - extent,
            child_bounds.width() + extent * 2.0,
            child_bounds.height() + extent * 2.0,
        )
    }

    fn is_visible(&self) -> bool {
        !self.disposed && self.child.as_ref().is_some_and(|c| c.is_visible())
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        if self.disposed {
            return false;
        }

        // Hit testing considers child, blur doesn't affect hit testing
        self.child
            .as_ref()
            .is_some_and(|c| c.hit_test(position, result))
    }

    fn handle_event(&mut self, event: &Event) -> bool {
        if self.disposed {
            return false;
        }

        self.child.as_mut().is_some_and(|c| c.handle_event(event))
    }

    fn dispose(&mut self) {
        if let Some(mut child) = self.child.take() {
            child.dispose();
        }
        self.disposed = true;
    }

    fn is_disposed(&self) -> bool {
        self.disposed
    }

    fn mark_needs_paint(&mut self) {
        if let Some(child) = &mut self.child {
            child.mark_needs_paint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::painting::effects::ColorFilter;

    #[test]
    fn test_blur_quality() {
        assert_eq!(BlurQuality::default(), BlurQuality::Medium);
    }

    #[test]
    fn test_blur_mode() {
        assert_eq!(BlurMode::default(), BlurMode::Content);
    }

    #[test]
    fn test_blur_filter_extent() {
        let child = Box::new(crate::layer::picture::PictureLayer::new()) as BoxedLayer;
        let blur = BlurLayer::new(child).with_filter(ImageFilter::blur(10.0));

        // Blur extent should be ~3 sigma
        let extent = blur.calculate_filter_extent();
        assert!((extent - 30.0).abs() < 0.1);
    }

    #[test]
    fn test_dilate_extent() {
        let child = Box::new(crate::layer::picture::PictureLayer::new()) as BoxedLayer;
        let dilate = BlurLayer::new(child).with_filter(ImageFilter::dilate(5.0));

        let extent = dilate.calculate_filter_extent();
        assert_eq!(extent, 5.0);
    }

    #[test]
    fn test_color_filter_no_extent() {
        let child = Box::new(crate::layer::picture::PictureLayer::new()) as BoxedLayer;
        let color = BlurLayer::new(child).with_filter(ImageFilter::color(ColorFilter::Brightness(0.5)));

        let extent = color.calculate_filter_extent();
        assert_eq!(extent, 0.0);
    }

    #[test]
    fn test_compose_filter_extent() {
        let child = Box::new(crate::layer::picture::PictureLayer::new()) as BoxedLayer;
        let compose = BlurLayer::new(child).with_filter(ImageFilter::Compose(vec![
            ImageFilter::blur(5.0),
            ImageFilter::dilate(10.0),
        ]));

        // Should use max extent (blur(5.0)*3 = 15.0)
        let extent = compose.calculate_filter_extent();
        assert_eq!(extent, 15.0);
    }
}
