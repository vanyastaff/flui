//! RenderBackdropFilter - Applies a filter to the content behind the widget
//!
//! This render object applies image filters (like blur) to the content that lies
//! behind it in the paint order. Common use case is frosted glass effect.

use crate::core::{
    FullRenderTree,
    FullRenderTree, RenderBox, Single, {BoxProtocol, LayoutContext, PaintContext},
};
use flui_foundation::ElementId;
use flui_types::{geometry::Rect, painting::BlendMode, painting::ImageFilter, Size};

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
    pub filter: flui_types::painting::ImageFilter,
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

impl<T: FullRenderTree> RenderBox<T, Single> for RenderBackdropFilter {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let child_id = ctx.children.single();
        // Layout child with same constraints
        ctx.layout_child(child_id, ctx.constraints)
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: crate::core::PaintTree,
    {
        let child_id = ctx.children.single();
        let child_element_id = ElementId::new(child_id.get());

        // Get the child's size to calculate backdrop bounds
        let child_size_tuple = ctx
            .tree()
            .get_size(child_element_id)
            .expect("Child should have a size after layout");
        let child_size = Size::new(child_size_tuple.0, child_size_tuple.1);

        // Create bounds for the backdrop filter region
        let bounds = Rect::from_min_size(ctx.offset, child_size);

        // Save offset before mutably borrowing ctx
        let offset = ctx.offset;

        // Paint child content FIRST (to be rendered on top of filtered backdrop)
        let child_canvas = ctx
            .tree_mut()
            .perform_paint(child_element_id, offset)
            .expect("Child paint should succeed");

        // Use Canvas::draw_backdrop_filter() to apply backdrop filter
        ctx.canvas().draw_backdrop_filter(
            bounds,
            self.filter.clone(),
            self.blend_mode,
            Some(|backdrop_canvas: &mut flui_painting::Canvas| {
                // Append the pre-rendered child content on top of filtered backdrop
                backdrop_canvas.append_canvas(child_canvas);
            }),
        );
    }
}

// ===== Tests =====

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_backdrop_filter_set_filter() {
        let mut filter = RenderBackdropFilter::blur(5.0);

        let new_filter = ImageFilter::Blur {
            sigma_x: 10.0,
            sigma_y: 10.0,
        };
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
