//! RenderOpacity - applies opacity to a child

use flui_types::{Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Data for RenderOpacity
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpacityData {
    /// Opacity value (0.0 = fully transparent, 1.0 = fully opaque)
    pub opacity: f32,
}

impl OpacityData {
    /// Create new opacity data
    pub fn new(opacity: f32) -> Self {
        Self {
            opacity: opacity.clamp(0.0, 1.0)
        }
    }
}

/// RenderObject that applies opacity to its child
///
/// The opacity value ranges from 0.0 (fully transparent) to 1.0 (fully opaque).
/// Changing opacity only affects painting, not layout.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::effects::OpacityData};
///
/// let mut opacity = SingleRenderBox::new(OpacityData::new(0.5));
/// ```
pub type RenderOpacity = SingleRenderBox<OpacityData>;

// ===== Public API =====

impl RenderOpacity {
    /// Get the opacity
    pub fn opacity(&self) -> f32 {
        self.data().opacity
    }

    /// Set new opacity
    ///
    /// If opacity changes, marks as needing paint (not layout).
    pub fn set_opacity(&mut self, opacity: f32) {
        let clamped = opacity.clamp(0.0, 1.0);
        if self.data().opacity != clamped {
            self.data_mut().opacity = clamped;
            self.mark_needs_paint();
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderOpacity {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Store constraints
        self.state_mut().constraints = Some(constraints);

        // Layout child with same constraints
        let size = if let Some(child) = self.child_mut() {
            child.layout(constraints)
        } else {
            // No child - use smallest size
            constraints.smallest()
        };

        // Store size and clear needs_layout flag
        self.state_mut().size = Some(size);
        self.clear_needs_layout();

        size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        // Paint child with opacity
        if let Some(child) = self.child() {
            let opacity = self.data().opacity;

            // If fully transparent, skip painting
            if opacity <= 0.0 {
                return;
            }

            // If fully opaque, paint normally
            if opacity >= 1.0 {
                child.paint(painter, offset);
                return;
            }

            // TODO: When egui supports opacity layers, apply opacity here
            // For now, just paint the child normally
            // In a real implementation, we would:
            // 1. Create an offscreen layer
            // 2. Paint child to that layer
            // 3. Composite the layer with opacity
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
    fn test_render_opacity_new() {
        let opacity = SingleRenderBox::new(OpacityData::new(0.5));
        assert_eq!(opacity.opacity(), 0.5);
    }

    #[test]
    fn test_render_opacity_clamping() {
        let opacity1 = SingleRenderBox::new(OpacityData::new(-0.5));
        assert_eq!(opacity1.opacity(), 0.0);

        let opacity2 = SingleRenderBox::new(OpacityData::new(1.5));
        assert_eq!(opacity2.opacity(), 1.0);
    }

    #[test]
    fn test_render_opacity_set_opacity() {
        let mut opacity = SingleRenderBox::new(OpacityData::new(0.5));

        // Clear initial needs_layout flag by doing a layout
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
        let _ = opacity.layout(constraints);

        // Now set opacity - should only mark needs_paint, not needs_layout
        opacity.set_opacity(0.8);
        assert_eq!(opacity.opacity(), 0.8);
        assert!(opacity.needs_paint());
        assert!(!opacity.needs_layout());
    }

    #[test]
    fn test_render_opacity_layout_no_child() {
        let mut opacity = SingleRenderBox::new(OpacityData::new(0.5));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let size = opacity.layout(constraints);

        // Should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
    }

    #[test]
    fn test_opacity_data_debug() {
        let data = OpacityData::new(0.75);
        let debug_str = format!("{:?}", data);
        assert!(debug_str.contains("OpacityData"));
    }
}
