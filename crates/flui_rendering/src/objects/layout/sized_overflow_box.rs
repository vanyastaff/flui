//! RenderSizedOverflowBox - fixed size with child_id overflow

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};

use flui_engine::{BoxedLayer, TransformLayer};
use flui_types::constraints::BoxConstraints;
use flui_types::{Alignment, Offset, Size};

/// RenderObject with fixed size that allows child_id to overflow
///
/// This is a combination of SizedBox and OverflowBox:
/// - The widget itself has a specific size (width/height)
/// - The child_id can have different constraints, allowing it to overflow
/// - The child_id is aligned within this widget
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSizedOverflowBox;
///
/// // Create 100x100 box, but allow child_id to be 200x200
/// let mut sized_overflow = RenderSizedOverflowBox::with_child_constraints(
///     Some(100.0), Some(100.0),
///     None, Some(200.0),
///     None, Some(200.0),
/// );
/// ```
#[derive(Debug)]
pub struct RenderSizedOverflowBox {
    /// Explicit width for this widget
    pub width: Option<f32>,
    /// Explicit height for this widget
    pub height: Option<f32>,
    /// Minimum width for child_id (overrides parent constraints)
    pub child_min_width: Option<f32>,
    /// Maximum width for child_id (overrides parent constraints)
    pub child_max_width: Option<f32>,
    /// Minimum height for child_id (overrides parent constraints)
    pub child_min_height: Option<f32>,
    /// Maximum height for child_id (overrides parent constraints)
    pub child_max_height: Option<f32>,
    /// How to align the child_id
    pub alignment: Alignment,

    // Cache for paint
    size: Size,
    child_size: Size,
}

// ===== Public API =====

impl RenderSizedOverflowBox {
    /// Create new sized overflow box
    pub fn new(width: Option<f32>, height: Option<f32>) -> Self {
        Self {
            width,
            height,
            child_min_width: None,
            child_max_width: None,
            child_min_height: None,
            child_max_height: None,
            alignment: Alignment::CENTER,
            size: Size::ZERO,
            child_size: Size::ZERO,
        }
    }

    /// Create with explicit size and child_id constraints
    pub fn with_child_constraints(
        width: Option<f32>,
        height: Option<f32>,
        child_min_width: Option<f32>,
        child_max_width: Option<f32>,
        child_min_height: Option<f32>,
        child_max_height: Option<f32>,
    ) -> Self {
        Self {
            width,
            height,
            child_min_width,
            child_max_width,
            child_min_height,
            child_max_height,
            alignment: Alignment::CENTER,
            size: Size::ZERO,
            child_size: Size::ZERO,
        }
    }

    /// Create with specific alignment
    pub fn with_alignment(width: Option<f32>, height: Option<f32>, alignment: Alignment) -> Self {
        Self {
            width,
            height,
            alignment,
            child_min_width: None,
            child_max_width: None,
            child_min_height: None,
            child_max_height: None,
            size: Size::ZERO,
            child_size: Size::ZERO,
        }
    }

    /// Set width
    pub fn set_width(&mut self, width: Option<f32>) {
        self.width = width;
    }

    /// Set height
    pub fn set_height(&mut self, height: Option<f32>) {
        self.height = height;
    }

    /// Set alignment
    pub fn set_alignment(&mut self, alignment: Alignment) {
        self.alignment = alignment;
    }
}

// ===== RenderObject Implementation =====

impl Render for RenderSizedOverflowBox {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let constraints = ctx.constraints;
        // Build child_id constraints from override values
        let child_min_width = self.child_min_width.unwrap_or(constraints.min_width);
        let child_max_width = self.child_max_width.unwrap_or(constraints.max_width);
        let child_min_height = self.child_min_height.unwrap_or(constraints.min_height);
        let child_max_height = self.child_max_height.unwrap_or(constraints.max_height);

        let child_constraints = BoxConstraints::new(
            child_min_width,
            child_max_width,
            child_min_height,
            child_max_height,
        );

        // Layout child_id with override constraints
        self.child_size = tree.layout_child(child_id, child_constraints);

        // Our size is the specified size (or constrained by parent)
        let width = self.width.unwrap_or(constraints.max_width);
        let height = self.height.unwrap_or(constraints.max_height);

        self.size = constraints.constrain(Size::new(width, height));
        self.size
    }

    fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let offset = ctx.offset;
        // Calculate aligned position
        let child_offset = self.alignment.calculate_offset(self.child_size, self.size);

        // Capture child_id layer
        let child_layer = tree.paint_child(child_id, offset);

        // Apply offset if needed
        if child_offset != Offset::ZERO {
            Box::new(TransformLayer::translate(child_layer, child_offset))
        } else {
            child_layer
        }
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Exact(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_sized_overflow_box_new() {
        let sized_overflow = RenderSizedOverflowBox::new(Some(100.0), Some(200.0));
        assert_eq!(sized_overflow.width, Some(100.0));
        assert_eq!(sized_overflow.height, Some(200.0));
        assert_eq!(sized_overflow.alignment, Alignment::CENTER);
    }

    #[test]
    fn test_render_sized_overflow_box_set_width() {
        let mut sized_overflow = RenderSizedOverflowBox::new(None, None);

        sized_overflow.set_width(Some(150.0));
        assert_eq!(sized_overflow.width, Some(150.0));
    }

    #[test]
    fn test_render_sized_overflow_box_set_alignment() {
        let mut sized_overflow = RenderSizedOverflowBox::new(None, None);

        sized_overflow.set_alignment(Alignment::BOTTOM_RIGHT);
        assert_eq!(sized_overflow.alignment, Alignment::BOTTOM_RIGHT);
    }

    #[test]
    fn test_render_sized_overflow_box_with_child_constraints() {
        let sized_overflow = RenderSizedOverflowBox::with_child_constraints(
            Some(100.0),
            Some(100.0),
            None,
            Some(200.0),
            None,
            Some(200.0),
        );
        assert_eq!(sized_overflow.width, Some(100.0));
        assert_eq!(sized_overflow.height, Some(100.0));
        assert_eq!(sized_overflow.child_max_width, Some(200.0));
        assert_eq!(sized_overflow.child_max_height, Some(200.0));
    }

    #[test]
    fn test_render_sized_overflow_box_with_alignment() {
        let sized_overflow =
            RenderSizedOverflowBox::with_alignment(Some(50.0), Some(75.0), Alignment::TOP_LEFT);
        assert_eq!(sized_overflow.alignment, Alignment::TOP_LEFT);

        
    }
}
