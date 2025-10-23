//! RenderLimitedBox - limits max width/height

use flui_types::{Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Data for RenderLimitedBox
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LimitedBoxData {
    /// Maximum width when unconstrained
    pub max_width: f32,
    /// Maximum height when unconstrained
    pub max_height: f32,
}

impl LimitedBoxData {
    /// Create new limited box data
    pub fn new(max_width: f32, max_height: f32) -> Self {
        Self { max_width, max_height }
    }
}

impl Default for LimitedBoxData {
    fn default() -> Self {
        Self {
            max_width: f32::INFINITY,
            max_height: f32::INFINITY,
        }
    }
}

/// RenderObject that limits maximum size when unconstrained
///
/// This is useful to prevent a child from becoming infinitely large when
/// placed in an unbounded context. Only applies limits when the incoming
/// constraints are infinite.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::layout::LimitedBoxData};
///
/// let mut limited = SingleRenderBox::new(LimitedBoxData::new(100.0, 100.0));
/// ```
pub type RenderLimitedBox = SingleRenderBox<LimitedBoxData>;

// ===== Public API =====

impl RenderLimitedBox {
    /// Get the max width
    pub fn max_width(&self) -> f32 {
        self.data().max_width
    }

    /// Get the max height
    pub fn max_height(&self) -> f32 {
        self.data().max_height
    }

    /// Set new max width
    pub fn set_max_width(&mut self, max_width: f32) {
        if (self.data().max_width - max_width).abs() > f32::EPSILON {
            self.data_mut().max_width = max_width;
            RenderBoxMixin::mark_needs_layout(self);
        }
    }

    /// Set new max height
    pub fn set_max_height(&mut self, max_height: f32) {
        if (self.data().max_height - max_height).abs() > f32::EPSILON {
            self.data_mut().max_height = max_height;
            RenderBoxMixin::mark_needs_layout(self);
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderLimitedBox {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Store constraints
        self.state_mut().constraints = Some(constraints);

        let max_width = self.data().max_width;
        let max_height = self.data().max_height;

        // Apply limits only if constraints are infinite
        let limited_constraints = BoxConstraints::new(
            constraints.min_width,
            if constraints.max_width.is_infinite() { max_width } else { constraints.max_width },
            constraints.min_height,
            if constraints.max_height.is_infinite() { max_height } else { constraints.max_height },
        );

        // Layout child with limited constraints
        let size = if let Some(child) = self.child_mut() {
            child.layout(limited_constraints)
        } else {
            // No child - use smallest size
            limited_constraints.smallest()
        };

        // Store size and clear needs_layout flag
        self.state_mut().size = Some(size);
        self.clear_needs_layout();

        size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        // Simply paint child at offset
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
    fn test_limited_box_data_new() {
        let data = LimitedBoxData::new(100.0, 200.0);
        assert_eq!(data.max_width, 100.0);
        assert_eq!(data.max_height, 200.0);
    }

    #[test]
    fn test_limited_box_data_default() {
        let data = LimitedBoxData::default();
        assert!(data.max_width.is_infinite());
        assert!(data.max_height.is_infinite());
    }

    #[test]
    fn test_render_limited_box_new() {
        let limited = SingleRenderBox::new(LimitedBoxData::new(100.0, 200.0));
        assert_eq!(limited.max_width(), 100.0);
        assert_eq!(limited.max_height(), 200.0);
    }

    #[test]
    fn test_render_limited_box_set_max_width() {
        let mut limited = SingleRenderBox::new(LimitedBoxData::new(100.0, 200.0));

        limited.set_max_width(150.0);
        assert_eq!(limited.max_width(), 150.0);
        assert!(RenderBoxMixin::needs_layout(&limited));
    }

    #[test]
    fn test_render_limited_box_layout_unconstrained() {
        let mut limited = SingleRenderBox::new(LimitedBoxData::new(100.0, 200.0));
        let constraints = BoxConstraints::new(0.0, f32::INFINITY, 0.0, f32::INFINITY);

        let size = limited.layout(constraints);

        // Should apply limits
        assert_eq!(size, Size::new(0.0, 0.0)); // Smallest size within limits
    }

    #[test]
    fn test_render_limited_box_layout_constrained() {
        let mut limited = SingleRenderBox::new(LimitedBoxData::new(100.0, 200.0));
        let constraints = BoxConstraints::new(0.0, 50.0, 0.0, 50.0);

        let size = limited.layout(constraints);

        // Limits don't apply when already constrained
        assert_eq!(size, Size::new(0.0, 0.0)); // Smallest size within incoming constraints
    }

    #[test]
    fn test_render_limited_box_layout_partially_unconstrained() {
        let mut limited = SingleRenderBox::new(LimitedBoxData::new(100.0, 200.0));
        // Width constrained, height unconstrained
        let constraints = BoxConstraints::new(0.0, 150.0, 0.0, f32::INFINITY);

        let size = limited.layout(constraints);

        // Should limit height only
        assert_eq!(size.width, 0.0); // Uses incoming constraint
        assert_eq!(size.height, 0.0); // Uses limit (smallest within 0..200)
    }
}
