//! RenderBackdropFilter - Applies a filter to the content behind the widget
//!
//! This render object applies image filters (like blur) to the content that lies
//! behind it in the paint order. Common use case is frosted glass effect.

use flui_core::DynRenderObject;
use flui_types::{Offset, Size, constraints::BoxConstraints, painting::BlendMode};

use crate::core::{RenderBoxMixin, SingleRenderBox};
use crate::delegate_to_mixin;

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

// ===== Type Alias =====

/// RenderBackdropFilter - Applies a filter to content behind the widget
///
/// This applies image filters (most commonly blur) to the content that was painted
/// before this widget in the paint order. This creates effects like frosted glass.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderBackdropFilter, BackdropFilterData};
///
/// // Create frosted glass effect
/// let data = BackdropFilterData::blur(10.0);
/// let mut filter = RenderBackdropFilter::new(data);
/// ```
///
/// # Notes
///
/// - This is an expensive operation (requires copying and filtering the backdrop)
/// - Consider using RepaintBoundary around filtered areas for better performance
/// - The filter is applied to the rectangular region covered by this widget
pub type RenderBackdropFilter = SingleRenderBox<BackdropFilterData>;

// ===== Methods =====

impl RenderBackdropFilter {
    /// Get the image filter
    pub fn filter(&self) -> &ImageFilter {
        &self.data().filter
    }

    /// Set the image filter
    pub fn set_filter(&mut self, filter: ImageFilter) {
        if &self.data().filter != &filter {
            self.data_mut().filter = filter;
            self.mark_needs_paint();
        }
    }

    /// Get the blend mode
    pub fn blend_mode(&self) -> BlendMode {
        self.data().blend_mode
    }

    /// Set the blend mode
    pub fn set_blend_mode(&mut self, blend_mode: BlendMode) {
        if self.data().blend_mode != blend_mode {
            self.data_mut().blend_mode = blend_mode;
            self.mark_needs_paint();
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderBackdropFilter {
    fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
        *state.constraints.lock() = Some(constraints);

        let children_ids = ctx.children();
        let size =
        if let Some(&child_id) = children_ids.first() {
            // Layout child with same constraints
            ctx.layout_child(child_id, constraints)
        } else {
            // No child - use smallest size
            constraints.smallest()
        };

        *state.size.lock() = Some(size);
        state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);
        size
    }

    fn paint(&self, state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) {
        if let Some(size) = *state.size.lock() {
            // Note: Full backdrop filtering requires compositor support
            // In production, this would:
            // 1. Capture the current paint layer content
            // 2. Apply the image filter to that content
            // 3. Paint the filtered result
            // 4. Paint the child on top

            // For now, we'll just paint the child
            // In a real implementation with egui, we'd use layers and effects

            // Visual debug indicator (in production, this would show filtered backdrop)
            if matches!(&self.data().filter, ImageFilter::Blur { .. }) {
                let rect = egui::Rect::from_min_size(
                    egui::pos2(offset.dx, offset.dy),
                    egui::vec2(size.width, size.height),
                );
                // Draw a semi-transparent overlay to indicate backdrop filter region
                painter.rect_filled(
                    rect,
                    4.0,
                    egui::Color32::from_rgba_unmultiplied(200, 200, 255, 30),
                );
            }

            let children_ids = ctx.children();
        if let Some(&child_id) = children_ids.first() {
            ctx.paint_child(child_id, painter, offset);
            }
        }
    }

    // Delegate all other methods to the mixin
    delegate_to_mixin!();
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
        let data = BackdropFilterData::blur(10.0);
        let mut filter = SingleRenderBox::new(data);

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
        use flui_core::testing::mock_render_context;

        let data = BackdropFilterData::blur(5.0);
        let mut filter = SingleRenderBox::new(data);

        // Do layout first to clear initial needs_paint
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let (_tree, ctx) = mock_render_context();
        filter.layout(constraints, &ctx);

        let new_filter = ImageFilter::brightness(1.5);
        filter.set_filter(new_filter.clone());

        assert_eq!(*filter.filter(), new_filter);
        assert!(filter.needs_paint());
    }

    #[test]
    fn test_render_backdrop_filter_set_blend_mode() {
        use flui_core::testing::mock_render_context;

        let data = BackdropFilterData::blur(10.0);
        let mut filter = SingleRenderBox::new(data);

        // Do layout first to clear initial needs_paint
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let (_tree, ctx) = mock_render_context();
        filter.layout(constraints, &ctx);

        filter.set_blend_mode(BlendMode::Screen);
        assert_eq!(filter.blend_mode(), BlendMode::Screen);
        assert!(filter.needs_paint());
    }

    #[test]
    fn test_render_backdrop_filter_layout() {
        use flui_core::testing::mock_render_context;

        let data = BackdropFilterData::blur(10.0);
        let mut filter = SingleRenderBox::new(data);

        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
        let (_tree, ctx) = mock_render_context();
        let size = filter.layout(constraints, &ctx);

        // Without child, should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
        assert_eq!(filter.size(), Size::new(0.0, 0.0));
    }
}
