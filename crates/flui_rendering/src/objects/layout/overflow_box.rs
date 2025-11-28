//! RenderOverflowBox - allows child to overflow constraints
//!
//! Flutter equivalent: `RenderConstrainedOverflowBox`
//! Source: https://api.flutter.dev/flutter/rendering/RenderConstrainedOverflowBox-class.html

use crate::core::{BoxProtocol, LayoutContext, PaintContext, FullRenderTree, RenderBox, Single};
use flui_types::constraints::BoxConstraints;
use flui_types::{Alignment, Offset, Size};

/// RenderObject that allows child_id to overflow parent constraints
///
/// This widget imposes different constraints on its child_id than it gets from
/// its parent, allowing the child_id to overflow. The child_id is then aligned
/// within this RenderObject using the alignment property.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderOverflowBox;
///
/// // Allow child_id to be wider than parent
/// let overflow = RenderOverflowBox::new();
/// ```
#[derive(Debug)]
pub struct RenderOverflowBox {
    /// Minimum width for child_id (overrides parent constraints)
    pub min_width: Option<f32>,
    /// Maximum width for child_id (overrides parent constraints)
    pub max_width: Option<f32>,
    /// Minimum height for child_id (overrides parent constraints)
    pub min_height: Option<f32>,
    /// Maximum height for child_id (overrides parent constraints)
    pub max_height: Option<f32>,
    /// How to align the overflowing child_id
    pub alignment: Alignment,

    // Cache for paint
    child_size: Size,
    container_size: Size,
}

impl RenderOverflowBox {
    /// Create new RenderOverflowBox
    pub fn new() -> Self {
        Self {
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            alignment: Alignment::CENTER,
            child_size: Size::ZERO,
            container_size: Size::ZERO,
        }
    }

    /// Create with specific constraints
    pub fn with_constraints(
        min_width: Option<f32>,
        max_width: Option<f32>,
        min_height: Option<f32>,
        max_height: Option<f32>,
    ) -> Self {
        Self {
            min_width,
            max_width,
            min_height,
            max_height,
            alignment: Alignment::CENTER,
            child_size: Size::ZERO,
            container_size: Size::ZERO,
        }
    }

    /// Create with specific alignment
    pub fn with_alignment(alignment: Alignment) -> Self {
        Self {
            alignment,
            ..Self::new()
        }
    }

    /// Set minimum width
    pub fn set_min_width(&mut self, min_width: Option<f32>) {
        self.min_width = min_width;
    }

    /// Set maximum width
    pub fn set_max_width(&mut self, max_width: Option<f32>) {
        self.max_width = max_width;
    }

    /// Set alignment
    pub fn set_alignment(&mut self, alignment: Alignment) {
        self.alignment = alignment;
    }
}

impl Default for RenderOverflowBox {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: FullRenderTree> RenderBox<T, Single> for RenderOverflowBox {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let child_id = ctx.children.single();
        let constraints = ctx.constraints;

        // Calculate child_id constraints by overriding parent constraints
        let child_min_width = self.min_width.unwrap_or(constraints.min_width);
        let child_max_width = self.max_width.unwrap_or(constraints.max_width);
        let child_min_height = self.min_height.unwrap_or(constraints.min_height);
        let child_max_height = self.max_height.unwrap_or(constraints.max_height);

        let child_constraints = BoxConstraints::new(
            child_min_width,
            child_max_width,
            child_min_height,
            child_max_height,
        );

        // Layout child_id with overridden constraints
        let child_size = ctx.layout_child(child_id, child_constraints);

        // Our size is determined by parent constraints
        // We constrain ourselves, but let child_id overflow
        let size = constraints.constrain(Size::new(constraints.max_width, constraints.max_height));

        // Store sizes for paint
        self.child_size = child_size;
        self.container_size = size;

        size
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: crate::core::PaintTree,
    {
        let child_id = ctx.children.single();
        let offset = ctx.offset;

        // Calculate aligned offset
        let available_width = self.container_size.width - self.child_size.width;
        let available_height = self.container_size.height - self.child_size.height;

        let aligned_x = (available_width * (self.alignment.x + 1.0)) / 2.0;
        let aligned_y = (available_height * (self.alignment.y + 1.0)) / 2.0;

        let child_offset = offset + Offset::new(aligned_x, aligned_y);

        // Paint child at aligned offset
        ctx.paint_child(child_id, child_offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_overflow_box_new() {
        let overflow = RenderOverflowBox::new();
        assert_eq!(overflow.min_width, None);
        assert_eq!(overflow.max_width, None);
        assert_eq!(overflow.alignment, Alignment::CENTER);
    }

    #[test]
    fn test_render_overflow_box_with_constraints() {
        let overflow =
            RenderOverflowBox::with_constraints(Some(10.0), Some(100.0), Some(20.0), Some(200.0));
        assert_eq!(overflow.min_width, Some(10.0));
        assert_eq!(overflow.max_width, Some(100.0));
        assert_eq!(overflow.min_height, Some(20.0));
        assert_eq!(overflow.max_height, Some(200.0));
    }

    #[test]
    fn test_render_overflow_box_with_alignment() {
        let overflow = RenderOverflowBox::with_alignment(Alignment::TOP_LEFT);
        assert_eq!(overflow.alignment, Alignment::TOP_LEFT);
    }

    #[test]
    fn test_render_overflow_box_set_min_width() {
        let mut overflow = RenderOverflowBox::new();
        overflow.set_min_width(Some(50.0));
        assert_eq!(overflow.min_width, Some(50.0));
    }

    #[test]
    fn test_render_overflow_box_set_alignment() {
        let mut overflow = RenderOverflowBox::new();
        overflow.set_alignment(Alignment::BOTTOM_RIGHT);
        assert_eq!(overflow.alignment, Alignment::BOTTOM_RIGHT);
    }
}
