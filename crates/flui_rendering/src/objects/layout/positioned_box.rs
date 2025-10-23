//! RenderPositionedBox - positions child with explicit coordinates

use flui_types::{Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Data for RenderPositionedBox
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositionedBoxData {
    /// Distance from left edge
    pub left: Option<f32>,
    /// Distance from top edge
    pub top: Option<f32>,
    /// Distance from right edge
    pub right: Option<f32>,
    /// Distance from bottom edge
    pub bottom: Option<f32>,
    /// Explicit width
    pub width: Option<f32>,
    /// Explicit height
    pub height: Option<f32>,
}

impl PositionedBoxData {
    /// Create new positioned box data
    pub fn new() -> Self {
        Self {
            left: None,
            top: None,
            right: None,
            bottom: None,
            width: None,
            height: None,
        }
    }

    /// Create with left and top
    pub fn at(left: f32, top: f32) -> Self {
        Self {
            left: Some(left),
            top: Some(top),
            ..Self::new()
        }
    }

    /// Create with all edges
    pub fn fill(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left: Some(left),
            top: Some(top),
            right: Some(right),
            bottom: Some(bottom),
            width: None,
            height: None,
        }
    }
}

impl Default for PositionedBoxData {
    fn default() -> Self {
        Self::new()
    }
}

/// RenderObject that positions child with explicit coordinates
///
/// This is typically used inside a Stack to position a child at specific
/// coordinates. The coordinates can be absolute (left, top, right, bottom)
/// or combined with explicit width/height.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::layout::PositionedBoxData};
///
/// let mut positioned = SingleRenderBox::new(PositionedBoxData::at(10.0, 20.0));
/// ```
pub type RenderPositionedBox = SingleRenderBox<PositionedBoxData>;

// ===== Public API =====

impl RenderPositionedBox {
    /// Get left position
    pub fn left(&self) -> Option<f32> {
        self.data().left
    }

    /// Get top position
    pub fn top(&self) -> Option<f32> {
        self.data().top
    }

    /// Get right position
    pub fn right(&self) -> Option<f32> {
        self.data().right
    }

    /// Get bottom position
    pub fn bottom(&self) -> Option<f32> {
        self.data().bottom
    }

    /// Set left position
    pub fn set_left(&mut self, left: Option<f32>) {
        if self.data().left != left {
            self.data_mut().left = left;
            self.mark_needs_layout();
        }
    }

    /// Set top position
    pub fn set_top(&mut self, top: Option<f32>) {
        if self.data().top != top {
            self.data_mut().top = top;
            self.mark_needs_layout();
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderPositionedBox {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Store constraints
        self.state_mut().constraints = Some(constraints);

        let data = self.data();

        // Calculate child constraints based on positioning
        let child_constraints = if let (Some(left), Some(right)) = (data.left, data.right) {
            // Width determined by left and right
            let width = (constraints.max_width - left - right).max(0.0);
            BoxConstraints::new(width, width, 0.0, constraints.max_height)
        } else if let Some(width) = data.width {
            // Explicit width
            BoxConstraints::new(width, width, 0.0, constraints.max_height)
        } else {
            // Unconstrained width
            BoxConstraints::new(0.0, constraints.max_width, 0.0, constraints.max_height)
        };

        // Layout child
        let size = if let Some(child) = self.child_mut() {
            child.layout(child_constraints)
        } else {
            // No child - use the calculated constraints
            child_constraints.smallest()
        };

        // Store size and clear needs_layout flag
        self.state_mut().size = Some(size);
        self.clear_needs_layout();

        size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        // Paint child at calculated position
        if let Some(child) = self.child() {
            let data = self.data();

            // Calculate paint offset based on positioning
            let paint_offset = Offset::new(
                offset.dx + data.left.unwrap_or(0.0),
                offset.dy + data.top.unwrap_or(0.0),
            );

            child.paint(painter, paint_offset);
        }
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_positioned_box_data_new() {
        let data = PositionedBoxData::new();
        assert_eq!(data.left, None);
        assert_eq!(data.top, None);
        assert_eq!(data.right, None);
        assert_eq!(data.bottom, None);
    }

    #[test]
    fn test_positioned_box_data_at() {
        let data = PositionedBoxData::at(10.0, 20.0);
        assert_eq!(data.left, Some(10.0));
        assert_eq!(data.top, Some(20.0));
    }

    #[test]
    fn test_positioned_box_data_fill() {
        let data = PositionedBoxData::fill(10.0, 20.0, 30.0, 40.0);
        assert_eq!(data.left, Some(10.0));
        assert_eq!(data.top, Some(20.0));
        assert_eq!(data.right, Some(30.0));
        assert_eq!(data.bottom, Some(40.0));
    }

    #[test]
    fn test_render_positioned_box_new() {
        let positioned = SingleRenderBox::new(PositionedBoxData::at(10.0, 20.0));
        assert_eq!(positioned.left(), Some(10.0));
        assert_eq!(positioned.top(), Some(20.0));
    }

    #[test]
    fn test_render_positioned_box_set_left() {
        let mut positioned = SingleRenderBox::new(PositionedBoxData::new());

        positioned.set_left(Some(15.0));
        assert_eq!(positioned.left(), Some(15.0));
        assert!(positioned.needs_layout());
    }

    #[test]
    fn test_render_positioned_box_layout() {
        let mut positioned = SingleRenderBox::new(PositionedBoxData::at(10.0, 20.0));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let size = positioned.layout(constraints);

        // Should use smallest size (no child, no explicit size)
        assert_eq!(size, Size::new(0.0, 0.0));
    }

    #[test]
    fn test_render_positioned_box_layout_with_left_right() {
        let mut positioned = SingleRenderBox::new(PositionedBoxData::fill(10.0, 0.0, 10.0, 0.0));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let size = positioned.layout(constraints);

        // Width should be constrained by left and right
        // 100 - 10 - 10 = 80
        assert_eq!(size.width, 80.0);
    }
}
