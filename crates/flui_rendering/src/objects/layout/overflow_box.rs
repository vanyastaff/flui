//! RenderOverflowBox - allows child to overflow constraints

use flui_types::{Offset, Size, constraints::BoxConstraints, Alignment};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Data for RenderOverflowBox
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverflowBoxData {
    /// Minimum width for child (overrides parent constraints)
    pub min_width: Option<f32>,
    /// Maximum width for child (overrides parent constraints)
    pub max_width: Option<f32>,
    /// Minimum height for child (overrides parent constraints)
    pub min_height: Option<f32>,
    /// Maximum height for child (overrides parent constraints)
    pub max_height: Option<f32>,
    /// How to align the overflowing child
    pub alignment: Alignment,
}

impl OverflowBoxData {
    /// Create new overflow box data
    pub fn new() -> Self {
        Self {
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            alignment: Alignment::CENTER,
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
        }
    }

    /// Create with specific alignment
    pub fn with_alignment(alignment: Alignment) -> Self {
        Self {
            alignment,
            ..Self::new()
        }
    }
}

impl Default for OverflowBoxData {
    fn default() -> Self {
        Self::new()
    }
}

/// RenderObject that allows child to overflow parent constraints
///
/// This widget imposes different constraints on its child than it gets from
/// its parent, allowing the child to overflow. The child is then aligned
/// within this RenderObject using the alignment property.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::layout::OverflowBoxData};
///
/// // Allow child to be wider than parent
/// let mut overflow = SingleRenderBox::new(
///     OverflowBoxData::with_constraints(None, Some(200.0), None, None)
/// );
/// ```
pub type RenderOverflowBox = SingleRenderBox<OverflowBoxData>;

// ===== Public API =====

impl RenderOverflowBox {
    /// Get minimum width
    pub fn min_width(&self) -> Option<f32> {
        self.data().min_width
    }

    /// Get maximum width
    pub fn max_width(&self) -> Option<f32> {
        self.data().max_width
    }

    /// Get minimum height
    pub fn min_height(&self) -> Option<f32> {
        self.data().min_height
    }

    /// Get maximum height
    pub fn max_height(&self) -> Option<f32> {
        self.data().max_height
    }

    /// Get alignment
    pub fn alignment(&self) -> Alignment {
        self.data().alignment
    }

    /// Set minimum width
    pub fn set_min_width(&mut self, min_width: Option<f32>) {
        if self.data().min_width != min_width {
            self.data_mut().min_width = min_width;
            RenderBoxMixin::mark_needs_layout(self);
        }
    }

    /// Set maximum width
    pub fn set_max_width(&mut self, max_width: Option<f32>) {
        if self.data().max_width != max_width {
            self.data_mut().max_width = max_width;
            RenderBoxMixin::mark_needs_layout(self);
        }
    }

    /// Set alignment
    pub fn set_alignment(&mut self, alignment: Alignment) {
        if self.data().alignment != alignment {
            self.data_mut().alignment = alignment;
            RenderBoxMixin::mark_needs_layout(self);
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderOverflowBox {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Store constraints
        self.state_mut().constraints = Some(constraints);

        let data = self.data();

        // Calculate child constraints by overriding parent constraints
        let child_constraints = BoxConstraints::new(
            data.min_width.unwrap_or(0.0),
            data.max_width.unwrap_or(f32::INFINITY),
            data.min_height.unwrap_or(0.0),
            data.max_height.unwrap_or(f32::INFINITY),
        );

        // Layout child with overridden constraints
        if let Some(child) = self.child_mut() {
            let _ = child.layout(child_constraints);
        }

        // Our size is determined by parent constraints
        // We constrain ourselves, but let child overflow
        let size = constraints.constrain(Size::new(
            constraints.max_width,
            constraints.max_height,
        ));

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
    fn test_overflow_box_data_new() {
        let data = OverflowBoxData::new();
        assert_eq!(data.min_width, None);
        assert_eq!(data.max_width, None);
        assert_eq!(data.min_height, None);
        assert_eq!(data.max_height, None);
        assert_eq!(data.alignment, Alignment::CENTER);
    }

    #[test]
    fn test_overflow_box_data_with_constraints() {
        let data = OverflowBoxData::with_constraints(
            Some(10.0),
            Some(100.0),
            Some(20.0),
            Some(200.0),
        );
        assert_eq!(data.min_width, Some(10.0));
        assert_eq!(data.max_width, Some(100.0));
        assert_eq!(data.min_height, Some(20.0));
        assert_eq!(data.max_height, Some(200.0));
    }

    #[test]
    fn test_overflow_box_data_with_alignment() {
        let data = OverflowBoxData::with_alignment(Alignment::TOP_LEFT);
        assert_eq!(data.alignment, Alignment::TOP_LEFT);
    }

    #[test]
    fn test_render_overflow_box_new() {
        let overflow = SingleRenderBox::new(OverflowBoxData::new());
        assert_eq!(overflow.min_width(), None);
        assert_eq!(overflow.alignment(), Alignment::CENTER);
    }

    #[test]
    fn test_render_overflow_box_set_min_width() {
        let mut overflow = SingleRenderBox::new(OverflowBoxData::new());

        overflow.set_min_width(Some(50.0));
        assert_eq!(overflow.min_width(), Some(50.0));
        assert!(RenderBoxMixin::needs_layout(&overflow));
    }

    #[test]
    fn test_render_overflow_box_set_alignment() {
        let mut overflow = SingleRenderBox::new(OverflowBoxData::new());

        overflow.set_alignment(Alignment::BOTTOM_RIGHT);
        assert_eq!(overflow.alignment(), Alignment::BOTTOM_RIGHT);
        assert!(RenderBoxMixin::needs_layout(&overflow));
    }

    #[test]
    fn test_render_overflow_box_layout() {
        let mut overflow = SingleRenderBox::new(OverflowBoxData::new());
        let constraints = BoxConstraints::new(50.0, 100.0, 50.0, 100.0);

        let size = overflow.layout(constraints);

        // Should use parent constraints (max values)
        assert_eq!(size, Size::new(100.0, 100.0));
    }

    #[test]
    fn test_render_overflow_box_layout_with_child_overflow() {
        let mut overflow = SingleRenderBox::new(
            OverflowBoxData::with_constraints(None, Some(200.0), None, None)
        );
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let size = overflow.layout(constraints);

        // Parent size stays at 100, child can be up to 200
        assert_eq!(size, Size::new(100.0, 100.0));
    }
}
