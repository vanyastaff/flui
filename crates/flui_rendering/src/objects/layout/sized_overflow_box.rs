//! RenderSizedOverflowBox - fixed size with child overflow

use flui_types::{Offset, Size, constraints::BoxConstraints, Alignment};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Data for RenderSizedOverflowBox
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SizedOverflowBoxData {
    /// Explicit width for this widget
    pub width: Option<f32>,
    /// Explicit height for this widget
    pub height: Option<f32>,
    /// Minimum width for child (overrides parent constraints)
    pub child_min_width: Option<f32>,
    /// Maximum width for child (overrides parent constraints)
    pub child_max_width: Option<f32>,
    /// Minimum height for child (overrides parent constraints)
    pub child_min_height: Option<f32>,
    /// Maximum height for child (overrides parent constraints)
    pub child_max_height: Option<f32>,
    /// How to align the child
    pub alignment: Alignment,
}

impl SizedOverflowBoxData {
    /// Create new sized overflow box data
    pub fn new(width: Option<f32>, height: Option<f32>) -> Self {
        Self {
            width,
            height,
            child_min_width: None,
            child_max_width: None,
            child_min_height: None,
            child_max_height: None,
            alignment: Alignment::CENTER,
        }
    }

    /// Create with explicit size and child constraints
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
        }
    }

    /// Create with specific alignment
    pub fn with_alignment(width: Option<f32>, height: Option<f32>, alignment: Alignment) -> Self {
        Self {
            width,
            height,
            alignment,
            ..Self::new(width, height)
        }
    }
}

impl Default for SizedOverflowBoxData {
    fn default() -> Self {
        Self::new(None, None)
    }
}

/// RenderObject with fixed size that allows child to overflow
///
/// This is a combination of SizedBox and OverflowBox:
/// - The widget itself has a specific size (width/height)
/// - The child can have different constraints, allowing it to overflow
/// - The child is aligned within this widget
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::layout::SizedOverflowBoxData};
///
/// // Create 100x100 box, but allow child to be 200x200
/// let mut sized_overflow = SingleRenderBox::new(
///     SizedOverflowBoxData::with_child_constraints(
///         Some(100.0), Some(100.0),
///         None, Some(200.0),
///         None, Some(200.0),
///     )
/// );
/// ```
pub type RenderSizedOverflowBox = SingleRenderBox<SizedOverflowBoxData>;

// ===== Public API =====

impl RenderSizedOverflowBox {
    /// Get width
    pub fn width(&self) -> Option<f32> {
        self.data().width
    }

    /// Get height
    pub fn height(&self) -> Option<f32> {
        self.data().height
    }

    /// Get alignment
    pub fn alignment(&self) -> Alignment {
        self.data().alignment
    }

    /// Set width
    pub fn set_width(&mut self, width: Option<f32>) {
        if self.data().width != width {
            self.data_mut().width = width;
            self.mark_needs_layout();
        }
    }

    /// Set height
    pub fn set_height(&mut self, height: Option<f32>) {
        if self.data().height != height {
            self.data_mut().height = height;
            self.mark_needs_layout();
        }
    }

    /// Set alignment
    pub fn set_alignment(&mut self, alignment: Alignment) {
        if self.data().alignment != alignment {
            self.data_mut().alignment = alignment;
            self.mark_needs_layout();
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderSizedOverflowBox {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Store constraints
        self.state_mut().constraints = Some(constraints);

        // Extract values before mutable borrow
        let (child_constraints, width, height) = {
            let data = self.data();
            let child_constraints = BoxConstraints::new(
                data.child_min_width.unwrap_or(0.0),
                data.child_max_width.unwrap_or(f32::INFINITY),
                data.child_min_height.unwrap_or(0.0),
                data.child_max_height.unwrap_or(f32::INFINITY),
            );
            let width = data.width.unwrap_or(constraints.max_width);
            let height = data.height.unwrap_or(constraints.max_height);
            (child_constraints, width, height)
        };

        // Layout child with override constraints
        if let Some(child) = self.child_mut() {
            let _ = child.layout(child_constraints);
        }

        // Our size is the specified size (or constrained by parent)
        let size = constraints.constrain(Size::new(width, height));

        // Store size and clear needs_layout flag
        self.state_mut().size = Some(size);
        self.clear_needs_layout();

        size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        // Paint child with alignment offset
        if let Some(child) = self.child() {
            let size = self.state().size.unwrap_or(Size::ZERO);
            let child_size = child.size();
            let alignment = self.data().alignment;

            // Calculate aligned position
            let child_offset = alignment.calculate_offset(child_size, size);

            let paint_offset = Offset::new(
                offset.dx + child_offset.dx,
                offset.dy + child_offset.dy,
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
    fn test_sized_overflow_box_data_new() {
        let data = SizedOverflowBoxData::new(Some(100.0), Some(200.0));
        assert_eq!(data.width, Some(100.0));
        assert_eq!(data.height, Some(200.0));
        assert_eq!(data.alignment, Alignment::CENTER);
    }

    #[test]
    fn test_sized_overflow_box_data_with_child_constraints() {
        let data = SizedOverflowBoxData::with_child_constraints(
            Some(100.0),
            Some(100.0),
            None,
            Some(200.0),
            None,
            Some(200.0),
        );
        assert_eq!(data.width, Some(100.0));
        assert_eq!(data.height, Some(100.0));
        assert_eq!(data.child_max_width, Some(200.0));
        assert_eq!(data.child_max_height, Some(200.0));
    }

    #[test]
    fn test_sized_overflow_box_data_with_alignment() {
        let data = SizedOverflowBoxData::with_alignment(
            Some(50.0),
            Some(75.0),
            Alignment::TOP_LEFT,
        );
        assert_eq!(data.alignment, Alignment::TOP_LEFT);
    }

    #[test]
    fn test_render_sized_overflow_box_new() {
        let sized_overflow = SingleRenderBox::new(SizedOverflowBoxData::new(Some(100.0), Some(200.0)));
        assert_eq!(sized_overflow.width(), Some(100.0));
        assert_eq!(sized_overflow.height(), Some(200.0));
        assert_eq!(sized_overflow.alignment(), Alignment::CENTER);
    }

    #[test]
    fn test_render_sized_overflow_box_set_width() {
        let mut sized_overflow = SingleRenderBox::new(SizedOverflowBoxData::default());

        sized_overflow.set_width(Some(150.0));
        assert_eq!(sized_overflow.width(), Some(150.0));
        assert!(sized_overflow.needs_layout());
    }

    #[test]
    fn test_render_sized_overflow_box_set_alignment() {
        let mut sized_overflow = SingleRenderBox::new(SizedOverflowBoxData::default());

        sized_overflow.set_alignment(Alignment::BOTTOM_RIGHT);
        assert_eq!(sized_overflow.alignment(), Alignment::BOTTOM_RIGHT);
        assert!(sized_overflow.needs_layout());
    }

    #[test]
    fn test_render_sized_overflow_box_layout() {
        let mut sized_overflow = SingleRenderBox::new(
            SizedOverflowBoxData::new(Some(100.0), Some(200.0))
        );
        let constraints = BoxConstraints::new(0.0, 500.0, 0.0, 500.0);

        let size = sized_overflow.layout(constraints);

        // Should use specified size
        assert_eq!(size, Size::new(100.0, 200.0));
    }

    #[test]
    fn test_render_sized_overflow_box_layout_constrained() {
        let mut sized_overflow = SingleRenderBox::new(
            SizedOverflowBoxData::new(Some(1000.0), Some(2000.0))
        );
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let size = sized_overflow.layout(constraints);

        // Should be constrained by parent
        assert_eq!(size, Size::new(100.0, 100.0));
    }
}
