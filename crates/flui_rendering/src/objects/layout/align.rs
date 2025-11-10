//! RenderAlign - aligns child within available space

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};

use flui_engine::BoxedLayer;
use flui_types::{Alignment, Offset, Size};

/// RenderObject that aligns its child within the available space
///
/// This render object positions its child according to the alignment parameter.
/// If width_factor or height_factor are specified, the RenderAlign will
/// size itself to be that factor times the child's size in that dimension.
///
/// # Layout Behavior
///
/// - **With factors**: Size is `child_size * factor` (clamped to constraints)
/// - **Without factors**: Expands to fill available space (takes max constraints)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderAlign;
/// use flui_types::Alignment;
///
/// // Center align with natural sizing
/// let align = RenderAlign::new(Alignment::CENTER);
///
/// // Top-left align with size factors
/// let align = RenderAlign::with_factors(
///     Alignment::TOP_LEFT,
///     Some(2.0),   // Width = child_width * 2.0
///     Some(1.5),   // Height = child_height * 1.5
/// );
/// ```
#[derive(Debug)]
pub struct RenderAlign {
    /// The alignment within the available space
    pub alignment: Alignment,
    /// Width factor - if Some, the width is child_width * width_factor
    /// Otherwise, expands to fill available space
    pub width_factor: Option<f32>,
    /// Height factor - if Some, the height is child_height * height_factor
    /// Otherwise, expands to fill available space
    pub height_factor: Option<f32>,

    // Cached values from layout for paint phase
    child_size: Size,
    size: Size,
}

impl RenderAlign {
    /// Create new RenderAlign with specified alignment
    pub fn new(alignment: Alignment) -> Self {
        Self {
            alignment,
            width_factor: None,
            height_factor: None,
            child_size: Size::ZERO,
            size: Size::ZERO,
        }
    }

    /// Create with alignment and size factors
    pub fn with_factors(
        alignment: Alignment,
        width_factor: Option<f32>,
        height_factor: Option<f32>,
    ) -> Self {
        Self {
            alignment,
            width_factor,
            height_factor,
            child_size: Size::ZERO,
            size: Size::ZERO,
        }
    }

    /// Set new alignment
    pub fn set_alignment(&mut self, alignment: Alignment) {
        self.alignment = alignment;
    }

    /// Set width factor
    pub fn set_width_factor(&mut self, width_factor: Option<f32>) {
        self.width_factor = width_factor;
    }

    /// Set height factor
    pub fn set_height_factor(&mut self, height_factor: Option<f32>) {
        self.height_factor = height_factor;
    }
}

impl Default for RenderAlign {
    fn default() -> Self {
        Self::new(Alignment::CENTER)
    }
}

impl Render for RenderAlign {

    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let constraints = ctx.constraints;
        // Layout child with loose constraints to get its natural size
        // Loose constraints allow the child to be smaller than max constraints
        let child_size = tree.layout_child(child_id, constraints.loosen());

        // Store child size for paint phase
        self.child_size = child_size;

        // Calculate our size based on factors
        // Flutter-compatible behavior:
        // - If factor is set: size = child_size * factor (clamped to constraints)
        // - If no factor: expand to fill max constraints
        let width = if let Some(factor) = self.width_factor {
            (child_size.width * factor).clamp(constraints.min_width, constraints.max_width)
        } else {
            // No factor: expand to fill available width
            constraints.max_width
        };

        let height = if let Some(factor) = self.height_factor {
            (child_size.height * factor).clamp(constraints.min_height, constraints.max_height)
        } else {
            // No factor: expand to fill available height
            constraints.max_height
        };

        let size = Size::new(width, height);
        // Store our size for paint phase
        self.size = size;
        size
    }

    fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let offset = ctx.offset;
        // Use cached values from layout phase
        let size = self.size;
        let child_size = self.child_size;

        // Calculate aligned offset in local coordinates
        // Alignment uses normalized coordinates: -1.0 = left/top, 0.0 = center, 1.0 = right/bottom
        let available_width = size.width - child_size.width;
        let available_height = size.height - child_size.height;

        // Convert normalized alignment to pixel offset
        // Formula: offset = (available_space * (alignment + 1.0)) / 2.0
        // Examples:
        //   alignment = -1.0 → offset = 0.0 (left/top)
        //   alignment =  0.0 → offset = available_space / 2.0 (center)
        //   alignment =  1.0 → offset = available_space (right/bottom)
        let aligned_x = (available_width * (self.alignment.x + 1.0)) / 2.0;
        let aligned_y = (available_height * (self.alignment.y + 1.0)) / 2.0;

        let local_child_offset = Offset::new(aligned_x, aligned_y);

        // Paint child at aligned position
        let child_layer = tree.paint_child(child_id, local_child_offset);

        // Apply parent offset if non-zero
        if offset != Offset::ZERO {
            Box::new(flui_engine::TransformLayer::translate(child_layer, offset))
        } else {
            child_layer
        }
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Variable  // Default - update if needed
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_align_new() {
        let align = RenderAlign::new(Alignment::TOP_LEFT);
        assert_eq!(align.alignment, Alignment::TOP_LEFT);
        assert_eq!(align.width_factor, None);
        assert_eq!(align.height_factor, None);
    }

    #[test]
    fn test_render_align_default() {
        let align = RenderAlign::default();
        assert_eq!(align.alignment, Alignment::CENTER);
    }

    #[test]
    fn test_render_align_with_factors() {
        let align = RenderAlign::with_factors(Alignment::CENTER, Some(2.0), Some(1.5));
        assert_eq!(align.alignment, Alignment::CENTER);
        assert_eq!(align.width_factor, Some(2.0));
        assert_eq!(align.height_factor, Some(1.5));
    }

    #[test]
    fn test_render_align_set_alignment() {
        let mut align = RenderAlign::new(Alignment::TOP_LEFT);
        align.set_alignment(Alignment::BOTTOM_RIGHT);
        assert_eq!(align.alignment, Alignment::BOTTOM_RIGHT);
    }

    #[test]
    fn test_render_align_set_factors() {
        let mut align = RenderAlign::new(Alignment::CENTER);
        align.set_width_factor(Some(2.0));
        align.set_height_factor(Some(1.5));
        assert_eq!(align.width_factor, Some(2.0));
        assert_eq!(align.height_factor, Some(1.5));

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Exact(1)
    }
    }
}
