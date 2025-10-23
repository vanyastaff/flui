//! RenderIntrinsicWidth - sizes child to its intrinsic width

use flui_types::{Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Data for RenderIntrinsicWidth
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IntrinsicWidthData {
    /// Step width (rounds intrinsic width to nearest multiple)
    pub step_width: Option<f32>,
    /// Step height (rounds intrinsic height to nearest multiple)
    pub step_height: Option<f32>,
}

impl IntrinsicWidthData {
    /// Create new intrinsic width data
    pub fn new() -> Self {
        Self {
            step_width: None,
            step_height: None,
        }
    }

    /// Create with step width
    pub fn with_step_width(step_width: f32) -> Self {
        Self {
            step_width: Some(step_width),
            step_height: None,
        }
    }

    /// Create with step height
    pub fn with_step_height(step_height: f32) -> Self {
        Self {
            step_width: None,
            step_height: Some(step_height),
        }
    }

    /// Create with both step dimensions
    pub fn with_steps(step_width: f32, step_height: f32) -> Self {
        Self {
            step_width: Some(step_width),
            step_height: Some(step_height),
        }
    }
}

impl Default for IntrinsicWidthData {
    fn default() -> Self {
        Self::new()
    }
}

/// RenderObject that sizes child to its intrinsic width
///
/// This forces the child to be as wide as it "naturally" wants to be,
/// ignoring the parent's width constraints. Useful for making text
/// widgets take up only as much space as needed.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::layout::IntrinsicWidthData};
///
/// // Child will be sized to its intrinsic width
/// let mut intrinsic = SingleRenderBox::new(IntrinsicWidthData::new());
/// ```
pub type RenderIntrinsicWidth = SingleRenderBox<IntrinsicWidthData>;

// ===== Public API =====

impl RenderIntrinsicWidth {
    /// Get step width
    pub fn step_width(&self) -> Option<f32> {
        self.data().step_width
    }

    /// Get step height
    pub fn step_height(&self) -> Option<f32> {
        self.data().step_height
    }

    /// Set step width
    pub fn set_step_width(&mut self, step_width: Option<f32>) {
        if self.data().step_width != step_width {
            self.data_mut().step_width = step_width;
            RenderBoxMixin::mark_needs_layout(self);
        }
    }

    /// Set step height
    pub fn set_step_height(&mut self, step_height: Option<f32>) {
        if self.data().step_height != step_height {
            self.data_mut().step_height = step_height;
            RenderBoxMixin::mark_needs_layout(self);
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderIntrinsicWidth {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Store constraints
        self.state_mut().constraints = Some(constraints);

        let step_width = self.data().step_width;
        let step_height = self.data().step_height;

        // Layout child with infinite width to get intrinsic width
        let size = if let Some(child) = self.child_mut() {
            // Get child's intrinsic width by giving it infinite width
            let intrinsic_constraints = BoxConstraints::new(
                0.0,
                f32::INFINITY,
                constraints.min_height,
                constraints.max_height,
            );

            let child_size = child.layout(intrinsic_constraints);

            // Apply step width/height if specified
            let width = if let Some(step) = step_width {
                (child_size.width / step).ceil() * step
            } else {
                child_size.width
            };

            let height = if let Some(step) = step_height {
                (child_size.height / step).ceil() * step
            } else {
                child_size.height
            };

            // Constrain to parent constraints
            constraints.constrain(Size::new(width, height))
        } else {
            constraints.smallest()
        };

        // Store size and clear needs_layout flag
        self.state_mut().size = Some(size);
        self.clear_needs_layout();

        size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        // Paint child at our position
        if let Some(child) = self.child() {
            child.paint(painter, offset);
        }
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intrinsic_width_data_new() {
        let data = IntrinsicWidthData::new();
        assert_eq!(data.step_width, None);
        assert_eq!(data.step_height, None);
    }

    #[test]
    fn test_intrinsic_width_data_with_step_width() {
        let data = IntrinsicWidthData::with_step_width(10.0);
        assert_eq!(data.step_width, Some(10.0));
        assert_eq!(data.step_height, None);
    }

    #[test]
    fn test_intrinsic_width_data_with_step_height() {
        let data = IntrinsicWidthData::with_step_height(5.0);
        assert_eq!(data.step_width, None);
        assert_eq!(data.step_height, Some(5.0));
    }

    #[test]
    fn test_intrinsic_width_data_with_steps() {
        let data = IntrinsicWidthData::with_steps(10.0, 5.0);
        assert_eq!(data.step_width, Some(10.0));
        assert_eq!(data.step_height, Some(5.0));
    }

    #[test]
    fn test_intrinsic_width_data_default() {
        let data = IntrinsicWidthData::default();
        assert_eq!(data.step_width, None);
        assert_eq!(data.step_height, None);
    }

    #[test]
    fn test_render_intrinsic_width_new() {
        let intrinsic = SingleRenderBox::new(IntrinsicWidthData::new());
        assert_eq!(intrinsic.step_width(), None);
        assert_eq!(intrinsic.step_height(), None);
    }

    #[test]
    fn test_render_intrinsic_width_set_step_width() {
        let mut intrinsic = SingleRenderBox::new(IntrinsicWidthData::new());

        intrinsic.set_step_width(Some(8.0));
        assert_eq!(intrinsic.step_width(), Some(8.0));
        assert!(RenderBoxMixin::needs_layout(&intrinsic));
    }

    #[test]
    fn test_render_intrinsic_width_set_step_height() {
        let mut intrinsic = SingleRenderBox::new(IntrinsicWidthData::new());

        intrinsic.set_step_height(Some(4.0));
        assert_eq!(intrinsic.step_height(), Some(4.0));
        assert!(RenderBoxMixin::needs_layout(&intrinsic));
    }

    #[test]
    fn test_render_intrinsic_width_layout() {
        let mut intrinsic = SingleRenderBox::new(IntrinsicWidthData::new());
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let size = intrinsic.layout(constraints);

        // No child, should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
    }
}
