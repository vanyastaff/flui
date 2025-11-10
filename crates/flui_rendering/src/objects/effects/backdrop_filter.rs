//! RenderBackdropFilter - Applies a filter to the content behind the widget
//!
//! This render object applies image filters (like blur) to the content that lies
//! behind it in the paint order. Common use case is frosted glass effect.

use flui_core::element::ElementId;
use flui_core::render::{Arity, LayoutContext, PaintContext, Render};

use flui_engine::BoxedLayer;
use flui_types::{
    constraints::BoxConstraints, painting::BlendMode, painting::ImageFilter, Offset, Size,
};

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

impl Render for RenderBackdropFilter {

    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let constraints = ctx.constraints;
        // Layout child_id with same constraints
        tree.layout_child(child_id, constraints)
    }

    fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let offset = ctx.offset;
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
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Variable  // Default - update if needed
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

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Exact(1)
    }
    }
}
