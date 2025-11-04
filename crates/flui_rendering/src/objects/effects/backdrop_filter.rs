//! RenderBackdropFilter - Applies a filter to the content behind the widget
//!
//! This render object applies image filters (like blur) to the content that lies
//! behind it in the paint order. Common use case is frosted glass effect.

use flui_core::element::{ElementId, ElementTree};
use flui_core::render::SingleRender;
use flui_engine::BoxedLayer;
use flui_types::{Offset, Size, constraints::BoxConstraints, painting::BlendMode};

// ===== Data Structure =====

/// Image filter specification
#[derive(Debug, Clone, PartialEq)]
pub enum ImageFilter {
    /// Gaussian blur filter
    Blur {
        /// Blur radius in logical pixels (sigma)
        radius: f32,
    },
    /// Brightness adjustment
    Brightness {
        /// Brightness factor (1.0 = no change, >1.0 = brighter, <1.0 = darker)
        factor: f32,
    },
    /// Saturation adjustment
    Saturation {
        /// Saturation factor (1.0 = no change, 0.0 = grayscale, >1.0 = more saturated)
        factor: f32,
    },
    /// Invert colors
    Invert,
}

impl ImageFilter {
    /// Create blur filter with given radius
    pub fn blur(radius: f32) -> Self {
        Self::Blur { radius }
    }

    /// Create brightness filter
    pub fn brightness(factor: f32) -> Self {
        Self::Brightness { factor }
    }

    /// Create saturation filter
    pub fn saturation(factor: f32) -> Self {
        Self::Saturation { factor }
    }

    /// Create invert filter
    pub fn invert() -> Self {
        Self::Invert
    }
}

/// Data for RenderBackdropFilter
#[derive(Debug, Clone)]
pub struct BackdropFilterData {
    /// Image filter to apply to backdrop
    pub filter: ImageFilter,
    /// Blend mode for compositing filtered result
    pub blend_mode: BlendMode,
}

impl BackdropFilterData {
    /// Create new backdrop filter with blur
    pub fn blur(radius: f32) -> Self {
        Self {
            filter: ImageFilter::blur(radius),
            blend_mode: BlendMode::default(),
        }
    }

    /// Create with custom filter
    pub fn new(filter: ImageFilter) -> Self {
        Self {
            filter,
            blend_mode: BlendMode::default(),
        }
    }

    /// Set blend mode
    pub fn with_blend_mode(mut self, blend_mode: BlendMode) -> Self {
        self.blend_mode = blend_mode;
        self
    }
}

// ===== RenderObject =====

/// RenderBackdropFilter - Applies a filter to content behind the widget
///
/// This applies image filters (most commonly blur) to the content that was painted
/// before this widget in the paint order. This creates effects like frosted glass.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderBackdropFilter;
///
/// // Create frosted glass effect
/// let filter = RenderBackdropFilter::blur(10.0);
/// ```
///
/// # Notes
///
/// - This is an expensive operation (requires copying and filtering the backdrop)
/// - Consider using RepaintBoundary around filtered areas for better performance
/// - The filter is applied to the rectangular region covered by this widget
#[derive(Debug)]
pub struct RenderBackdropFilter {
    /// Image filter to apply to backdrop
    pub filter: ImageFilter,
    /// Blend mode for compositing filtered result
    pub blend_mode: BlendMode,
}

// ===== Methods =====

impl RenderBackdropFilter {
    /// Create new backdrop filter with blur
    pub fn blur(radius: f32) -> Self {
        Self {
            filter: ImageFilter::blur(radius),
            blend_mode: BlendMode::default(),
        }
    }

    /// Create with custom filter
    pub fn new(filter: ImageFilter) -> Self {
        Self {
            filter,
            blend_mode: BlendMode::default(),
        }
    }

    /// Set blend mode
    pub fn with_blend_mode(mut self, blend_mode: BlendMode) -> Self {
        self.blend_mode = blend_mode;
        self
    }

    /// Get the image filter
    pub fn filter(&self) -> &ImageFilter {
        &self.filter
    }

    /// Set the image filter
    pub fn set_filter(&mut self, filter: ImageFilter) {
        self.filter = filter;
    }

    /// Get the blend mode
    pub fn blend_mode(&self) -> BlendMode {
        self.blend_mode
    }

    /// Set the blend mode
    pub fn set_blend_mode(&mut self, blend_mode: BlendMode) {
        self.blend_mode = blend_mode;
    }
}

// ===== RenderObject Implementation =====

impl SingleRender for RenderBackdropFilter {
    /// No metadata needed
    type Metadata = ();

    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        // Layout child_id with same constraints
        tree.layout_child(child_id, constraints)
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
        // Capture child_id layer
        // Note: Full backdrop filtering requires compositor support
        // In production, this would:
        // 1. Capture the current paint layer content
        // 2. Apply the image filter to that content
        // 3. Paint the filtered result
        // 4. Paint the child_id on top
        //
        // For now, we just return the child_id layer
        // TODO: Implement BackdropFilterLayer when compositor supports it

        (tree.paint_child(child_id, offset)) as _
    }
}

// ===== Tests =====

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_filter_blur() {
        let filter = ImageFilter::blur(10.0);
        match filter {
            ImageFilter::Blur { radius } => {
                assert_eq!(radius, 10.0);
            }
            _ => panic!("Expected blur filter"),
        }
    }

    #[test]
    fn test_image_filter_brightness() {
        let filter = ImageFilter::brightness(1.5);
        match filter {
            ImageFilter::Brightness { factor } => {
                assert_eq!(factor, 1.5);
            }
            _ => panic!("Expected brightness filter"),
        }
    }

    #[test]
    fn test_image_filter_saturation() {
        let filter = ImageFilter::saturation(0.5);
        match filter {
            ImageFilter::Saturation { factor } => {
                assert_eq!(factor, 0.5);
            }
            _ => panic!("Expected saturation filter"),
        }
    }

    #[test]
    fn test_image_filter_invert() {
        let filter = ImageFilter::invert();
        assert_eq!(filter, ImageFilter::Invert);
    }

    #[test]
    fn test_backdrop_filter_data_blur() {
        let data = BackdropFilterData::blur(5.0);
        match data.filter {
            ImageFilter::Blur { radius } => {
                assert_eq!(radius, 5.0);
            }
            _ => panic!("Expected blur filter"),
        }
        assert_eq!(data.blend_mode, BlendMode::default());
    }

    #[test]
    fn test_backdrop_filter_data_new() {
        let filter = ImageFilter::brightness(2.0);
        let data = BackdropFilterData::new(filter.clone());
        assert_eq!(data.filter, filter);
    }

    #[test]
    fn test_backdrop_filter_data_with_blend_mode() {
        let data = BackdropFilterData::blur(10.0).with_blend_mode(BlendMode::Multiply);
        assert_eq!(data.blend_mode, BlendMode::Multiply);
    }

    #[test]
    fn test_render_backdrop_filter_new() {
        let filter = RenderBackdropFilter::blur(10.0);

        match filter.filter() {
            ImageFilter::Blur { radius } => {
                assert_eq!(*radius, 10.0);
            }
            _ => panic!("Expected blur filter"),
        }
        assert_eq!(filter.blend_mode(), BlendMode::default());
    }

    #[test]
    fn test_render_backdrop_filter_set_filter() {
        let mut filter = RenderBackdropFilter::blur(5.0);

        let new_filter = ImageFilter::brightness(1.5);
        filter.set_filter(new_filter.clone());

        assert_eq!(*filter.filter(), new_filter);
    }

    #[test]
    fn test_render_backdrop_filter_set_blend_mode() {
        let mut filter = RenderBackdropFilter::blur(10.0);

        filter.set_blend_mode(BlendMode::Screen);
        assert_eq!(filter.blend_mode(), BlendMode::Screen);
    }

    #[test]
    fn test_render_backdrop_filter_with_blend_mode() {
        let filter = RenderBackdropFilter::blur(10.0).with_blend_mode(BlendMode::Multiply);
        assert_eq!(filter.blend_mode(), BlendMode::Multiply);
    }
}
