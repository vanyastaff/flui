//! RenderSizedBox - enforces exact size

use flui_types::{Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Data for RenderSizedBox
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SizedBoxData {
    /// Explicit width (None = unconstrained)
    pub width: Option<f32>,
    /// Explicit height (None = unconstrained)
    pub height: Option<f32>,
}

impl SizedBoxData {
    /// Create new sized box data
    pub fn new(width: Option<f32>, height: Option<f32>) -> Self {
        Self { width, height }
    }

    /// Create with specific width and height
    pub fn exact(width: f32, height: f32) -> Self {
        Self {
            width: Some(width),
            height: Some(height),
        }
    }

    /// Create with only width specified
    pub fn width(width: f32) -> Self {
        Self {
            width: Some(width),
            height: None,
        }
    }

    /// Create with only height specified
    pub fn height(height: f32) -> Self {
        Self {
            width: None,
            height: Some(height),
        }
    }

    /// Create empty (no child, just size)
    pub fn empty(width: f32, height: f32) -> Self {
        Self::exact(width, height)
    }
}

impl Default for SizedBoxData {
    fn default() -> Self {
        Self::new(None, None)
    }
}

/// RenderObject that enforces exact size constraints
///
/// This widget forces its child to have a specific width and/or height,
/// or acts as an invisible spacer if no child is present.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::layout::SizedBoxData};
///
/// // Force child to be exactly 100x100
/// let mut sized = SingleRenderBox::new(SizedBoxData::exact(100.0, 100.0));
///
/// // Create a 50 pixel wide spacer
/// let spacer = SingleRenderBox::new(SizedBoxData::width(50.0));
/// ```
pub type RenderSizedBox = SingleRenderBox<SizedBoxData>;

// ===== Public API =====

impl RenderSizedBox {
    /// Get width
    pub fn width(&self) -> Option<f32> {
        self.data().width
    }

    /// Get height
    pub fn height(&self) -> Option<f32> {
        self.data().height
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
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderSizedBox {
    fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
        // Store constraints
        *state.constraints.lock() = Some(constraints);

        let data = self.data();

        // Calculate our size based on explicit width/height
        let width = data.width.unwrap_or(constraints.max_width);
        let height = data.height.unwrap_or(constraints.max_height);

        // Create tight constraints for child
        let child_constraints = BoxConstraints::tight(Size::new(width, height));

        // Get children from ElementTree via RenderContext
        let children_ids = ctx.children();

        // Layout child with tight constraints
        if let Some(&child_id) = children_ids.first() {
            let _ = ctx.layout_child(child_id, child_constraints);
        }

        // Our size is the specified size, constrained by parent
        let size = constraints.constrain(Size::new(width, height));

        // Store size and clear needs_layout flag
        *state.size.lock() = Some(size);
        state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);

        size
    }

    fn paint(&self, state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) {
        // Paint child at our position
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
    fn test_sized_box_data_new() {
        let data = SizedBoxData::new(Some(100.0), Some(200.0));
        assert_eq!(data.width, Some(100.0));
        assert_eq!(data.height, Some(200.0));
    }

    #[test]
    fn test_sized_box_data_exact() {
        let data = SizedBoxData::exact(50.0, 75.0);
        assert_eq!(data.width, Some(50.0));
        assert_eq!(data.height, Some(75.0));
    }

    #[test]
    fn test_sized_box_data_width() {
        let data = SizedBoxData::width(100.0);
        assert_eq!(data.width, Some(100.0));
        assert_eq!(data.height, None);
    }

    #[test]
    fn test_sized_box_data_height() {
        let data = SizedBoxData::height(200.0);
        assert_eq!(data.width, None);
        assert_eq!(data.height, Some(200.0));
    }

    #[test]
    fn test_sized_box_data_default() {
        let data = SizedBoxData::default();
        assert_eq!(data.width, None);
        assert_eq!(data.height, None);
    }

    #[test]
    fn test_render_sized_box_new() {
        let sized = SingleRenderBox::new(SizedBoxData::exact(100.0, 200.0));
        assert_eq!(sized.width(), Some(100.0));
        assert_eq!(sized.height(), Some(200.0));
    }

    #[test]
    fn test_render_sized_box_set_width() {
        let mut sized = SingleRenderBox::new(SizedBoxData::default());

        sized.set_width(Some(150.0));
        assert_eq!(sized.width(), Some(150.0));
        assert!(sized.needs_layout());
    }

    #[test]
    fn test_render_sized_box_set_height() {
        let mut sized = SingleRenderBox::new(SizedBoxData::default());

        sized.set_height(Some(250.0));
        assert_eq!(sized.height(), Some(250.0));
        assert!(sized.needs_layout());
    }

    #[test]
    fn test_render_sized_box_layout_exact() {
        use flui_core::testing::mock_render_context;

        let sized = SingleRenderBox::new(SizedBoxData::exact(100.0, 200.0));
        let constraints = BoxConstraints::new(0.0, 500.0, 0.0, 500.0);

        let (_tree, ctx) = mock_render_context();
        let size = sized.layout(constraints, &ctx);

        // Should use exact size
        assert_eq!(size, Size::new(100.0, 200.0));
    }

    #[test]
    fn test_render_sized_box_layout_width_only() {
        use flui_core::testing::mock_render_context;

        let sized = SingleRenderBox::new(SizedBoxData::width(100.0));
        let constraints = BoxConstraints::new(0.0, 500.0, 0.0, 300.0);

        let (_tree, ctx) = mock_render_context();
        let size = sized.layout(constraints, &ctx);

        // Width is exact, height uses max constraint
        assert_eq!(size, Size::new(100.0, 300.0));
    }

    #[test]
    fn test_render_sized_box_layout_constrained() {
        use flui_core::testing::mock_render_context;

        let sized = SingleRenderBox::new(SizedBoxData::exact(1000.0, 2000.0));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let (_tree, ctx) = mock_render_context();
        let size = sized.layout(constraints, &ctx);

        // Should be constrained by parent constraints
        assert_eq!(size, Size::new(100.0, 100.0));
    }
}
