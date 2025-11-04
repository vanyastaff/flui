//! BlurLayer - applies image filters to child layers
//!
//! This module provides compositor-level image filtering effects including blur,
//! dilate, erode, and color matrix transformations. Supports CSS filter and
//! backdrop-filter style effects.

use crate::layer::{base_single_child::SingleChildLayerBase, BoxedLayer, Layer};
use crate::painter::Painter;
use flui_types::events::{Event, HitTestResult};
use flui_types::painting::effects::{BlurMode, BlurQuality, ImageFilter};
use flui_types::{Offset, Rect};

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
    /// Base single-child layer functionality
    base: SingleChildLayerBase,

    /// Image filter to apply
    filter: ImageFilter,

    /// Quality level for rendering
    quality: BlurQuality,

    /// Filter mode (content or backdrop)
    mode: BlurMode,

    /// Tile mode for edges (true = clamp, false = transparent)
    tile_mode_clamp: bool,
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
            base: SingleChildLayerBase::new(child),
            filter: ImageFilter::blur(5.0),
            quality: BlurQuality::default(),
            mode: BlurMode::default(),
            tile_mode_clamp: true,
        }
    }

    /// Set the image filter
    #[must_use]
    pub fn with_filter(mut self, filter: ImageFilter) -> Self {
        self.filter = filter;
        self.base.invalidate_cache();
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
        self.base.child()
    }

    /// Set the child layer
    pub fn set_child(&mut self, child: BoxedLayer) {
        self.base.set_child(child);
        self.mark_needs_paint();
    }

    /// Update the filter
    pub fn set_filter(&mut self, filter: ImageFilter) {
        self.filter = filter;
        self.base.invalidate_cache();
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
                            ImageFilter::Dilate { radius } | ImageFilter::Erode { radius } => {
                                *radius
                            }
                            _ => 0.0,
                        }
                    })
                    .fold(0.0f32, f32::max)
            }
            #[cfg(debug_assertions)]
            ImageFilter::OverflowIndicator { .. } => {
                // Overflow indicators don't extend bounds
                0.0
            }
        }
    }

    /// Apply image filter by rendering with layer composition
    fn apply_filter(&self, painter: &mut dyn Painter, bounds: Rect) {
        // Use save_layer to create an offscreen context,
        // then apply the filter using apply_image_filter()
        //
        // This approach allows backends that support it to:
        // 1. Render child to offscreen texture (via save_layer)
        // 2. Apply filter shader/convolution (via apply_image_filter)
        // 3. Render result to screen (via restore)
        //
        // Backends without full support will fallback gracefully

        if let Some(child) = self.base.child() {
            painter.save();

            // Create a layer for filtered rendering
            painter.save_layer(bounds, &crate::painter::Paint::default());

            // Apply the image filter to the layer
            painter.apply_image_filter(&self.filter, bounds);

            // Render child content into the filtered layer
            child.paint(painter);

            // Restore the layer (composites with filter applied)
            painter.restore();

            painter.restore();
        }
    }
}

impl Layer for BlurLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        let Some(child) = self.base.child() else {
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
                // Uses save_layer_backdrop() to capture the backdrop for filtering
                // Note: Full effect requires GPU backend with framebuffer capture

                let child_bounds = child.bounds();
                painter.save_layer_backdrop(child_bounds);

                // In a full implementation with GPU backend:
                // 1. save_layer_backdrop() captures what's already painted
                // 2. Apply blur filter to the captured backdrop
                // 3. Paint child on top of blurred backdrop
                // 4. restore() composites everything back

                // For now, just paint the child (no blur applied in non-GPU backends)
                child.paint(painter);
                painter.restore();
            }
        }

        painter.restore();
    }

    fn bounds(&self) -> Rect {
        if let Some(bounds) = self.base.cached_bounds() {
            return bounds;
        }

        let child_bounds = self.base.child_bounds();

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
        self.base.is_child_visible()
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        // Hit testing considers child, blur doesn't affect hit testing
        self.base.child_hit_test(position, result)
    }

    fn handle_event(&mut self, event: &Event) -> bool {
        self.base.child_handle_event(event)
    }

    fn dispose(&mut self) {
        self.base.dispose_child();
    }

    fn is_disposed(&self) -> bool {
        self.base.is_disposed()
    }

    fn mark_needs_paint(&mut self) {
        self.base.invalidate_cache();
        if let Some(child) = self.base.child_mut() {
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
        let color =
            BlurLayer::new(child).with_filter(ImageFilter::color(ColorFilter::Brightness(0.5)));

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
