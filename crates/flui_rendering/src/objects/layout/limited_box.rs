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
            self.mark_needs_layout();
        }
    }

    /// Set new max height
    pub fn set_max_height(&mut self, max_height: f32) {
        if (self.data().max_height - max_height).abs() > f32::EPSILON {
            self.data_mut().max_height = max_height;
            self.mark_needs_layout();
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderLimitedBox {
    fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
        // Store constraints
        *state.constraints.lock() = Some(constraints);

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
        let children_ids = ctx.children();
        let size =
        if let Some(&child_id) = children_ids.first() {
            ctx.layout_child_cached(child_id, limited_constraints, None)
        } else {
            // No child - use smallest size
            limited_constraints.smallest()
        };

        // Store size and clear needs_layout flag
        *state.size.lock() = Some(size);
        state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);

        size
    }

    fn paint(&self, state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) {
        // Simply paint child at offset
        // Get children from ElementTree via RenderContext
        let children_ids = ctx.children();

        if let Some(&child_id) = children_ids.first() {
            ctx.paint_child(child_id, painter, offset);
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
        assert!(limited.needs_layout());
    }

    #[test]
    fn test_render_limited_box_layout_unconstrained() {
        use flui_core::testing::mock_render_context;

        let limited = SingleRenderBox::new(LimitedBoxData::new(100.0, 200.0));
        let constraints = BoxConstraints::new(0.0, f32::INFINITY, 0.0, f32::INFINITY);

        let (_tree, ctx) = mock_render_context();
        let size = limited.layout(constraints, &ctx);

        // Should apply limits
        assert_eq!(size, Size::new(0.0, 0.0)); // Smallest size within limits
    }

    #[test]
    fn test_render_limited_box_layout_constrained() {
        use flui_core::testing::mock_render_context;

        let limited = SingleRenderBox::new(LimitedBoxData::new(100.0, 200.0));
        let constraints = BoxConstraints::new(0.0, 50.0, 0.0, 50.0);

        let (_tree, ctx) = mock_render_context();
        let size = limited.layout(constraints, &ctx);

        // Limits don't apply when already constrained
        assert_eq!(size, Size::new(0.0, 0.0)); // Smallest size within incoming constraints
    }

    #[test]
    fn test_render_limited_box_layout_partially_unconstrained() {
        use flui_core::testing::mock_render_context;

        let limited = SingleRenderBox::new(LimitedBoxData::new(100.0, 200.0));
        // Width constrained, height unconstrained
        let constraints = BoxConstraints::new(0.0, 150.0, 0.0, f32::INFINITY);

        let (_tree, ctx) = mock_render_context();
        let size = limited.layout(constraints, &ctx);

        // Should limit height only
        assert_eq!(size.width, 0.0); // Uses incoming constraint
        assert_eq!(size.height, 0.0); // Uses limit (smallest within 0..200)
    }
}
