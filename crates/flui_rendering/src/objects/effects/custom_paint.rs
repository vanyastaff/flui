//! RenderCustomPaint - custom painting with user-defined painters

use flui_types::{Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Custom painter trait
///
/// Implement this trait to define custom painting logic.
pub trait CustomPainter: std::fmt::Debug + Send + Sync {
    /// Paint custom content
    fn paint(&self, painter: &egui::Painter, size: Size, offset: Offset);

    /// Whether this painter should repaint when something changes
    fn should_repaint(&self, _old: &dyn CustomPainter) -> bool {
        true
    }
}

/// Data for RenderCustomPaint
#[derive(Debug)]
pub struct CustomPaintData {
    /// Foreground painter (painted on top of child)
    pub foreground_painter: Option<Box<dyn CustomPainter>>,
    /// Background painter (painted behind child)
    pub painter: Option<Box<dyn CustomPainter>>,
    /// Size to use when child is not present
    pub size: Size,
    /// Whether child is interactive (if false, hit tests go through)
    pub is_complex: bool,
    /// Whether foreground paints on top of child
    pub will_change: bool,
}

impl CustomPaintData {
    /// Create new custom paint data
    pub fn new(size: Size) -> Self {
        Self {
            foreground_painter: None,
            painter: None,
            size,
            is_complex: false,
            will_change: false,
        }
    }

    /// Create with background painter
    pub fn with_painter(painter: Box<dyn CustomPainter>, size: Size) -> Self {
        Self {
            foreground_painter: None,
            painter: Some(painter),
            size,
            is_complex: false,
            will_change: false,
        }
    }

    /// Create with foreground painter
    pub fn with_foreground(foreground: Box<dyn CustomPainter>, size: Size) -> Self {
        Self {
            foreground_painter: Some(foreground),
            painter: None,
            size,
            is_complex: false,
            will_change: false,
        }
    }

    /// Create with both painters
    pub fn with_both(
        painter: Box<dyn CustomPainter>,
        foreground: Box<dyn CustomPainter>,
        size: Size,
    ) -> Self {
        Self {
            foreground_painter: Some(foreground),
            painter: Some(painter),
            size,
            is_complex: false,
            will_change: false,
        }
    }
}

impl Default for CustomPaintData {
    fn default() -> Self {
        Self::new(Size::ZERO)
    }
}

/// RenderObject that allows custom painting
///
/// This widget allows you to paint custom graphics before and/or after
/// the child widget. Useful for drawing custom shapes, decorations, etc.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::effects::CustomPaintData};
///
/// // Custom paint with background and foreground
/// let mut custom = SingleRenderBox::new(
///     CustomPaintData::new(Size::new(100.0, 100.0))
/// );
/// ```
pub type RenderCustomPaint = SingleRenderBox<CustomPaintData>;

// ===== Public API =====

impl RenderCustomPaint {
    /// Get size
    pub fn size(&self) -> Size {
        self.data().size
    }

    /// Get is_complex flag
    pub fn is_complex(&self) -> bool {
        self.data().is_complex
    }

    /// Get will_change flag
    pub fn will_change(&self) -> bool {
        self.data().will_change
    }

    /// Set size
    pub fn set_size(&mut self, size: Size) {
        if self.data().size != size {
            self.data_mut().size = size;
            self.mark_needs_layout();
        }
    }

    /// Set is_complex flag
    pub fn set_is_complex(&mut self, is_complex: bool) {
        if self.data().is_complex != is_complex {
            self.data_mut().is_complex = is_complex;
        }
    }

    /// Set will_change flag
    pub fn set_will_change(&mut self, will_change: bool) {
        if self.data().will_change != will_change {
            self.data_mut().will_change = will_change;
            self.mark_needs_paint();
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderCustomPaint {
    fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
        // Store constraints
        *state.constraints.lock() = Some(constraints);

        // Layout child if present
        let children_ids = ctx.children();
        let size =
        if let Some(&child_id) = children_ids.first() {
            ctx.layout_child_cached(child_id, constraints, None)
        } else {
            // No child - use our preferred size
            let preferred_size = self.data().size;
            constraints.constrain(preferred_size)
        };

        // Store size and clear needs_layout flag
        *state.size.lock() = Some(size);
        state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);

        size
    }

    fn paint(&self, state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) {
        let size = state.size.lock().unwrap_or(Size::ZERO);

        // Paint background painter
        if let Some(bg_painter) = &self.data().painter {
            bg_painter.paint(painter, size, offset);
        }

        // Paint child
        let children_ids = ctx.children();
        if let Some(&child_id) = children_ids.first() {
            ctx.paint_child(child_id, painter, offset);
        }

        // Paint foreground painter (on top of child)
        if let Some(fg_painter) = &self.data().foreground_painter {
            fg_painter.paint(painter, size, offset);
        }
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock painter for testing
    #[derive(Debug)]
    struct MockPainter;

    impl CustomPainter for MockPainter {
        fn paint(&self, _painter: &egui::Painter, _size: Size, _offset: Offset) {
            // Do nothing
        }
    }

    #[test]
    fn test_custom_paint_data_new() {
        let data = CustomPaintData::new(Size::new(100.0, 200.0));
        assert_eq!(data.size, Size::new(100.0, 200.0));
        assert!(data.painter.is_none());
        assert!(data.foreground_painter.is_none());
    }

    #[test]
    fn test_custom_paint_data_with_painter() {
        let data = CustomPaintData::with_painter(
            Box::new(MockPainter),
            Size::new(50.0, 75.0),
        );
        assert_eq!(data.size, Size::new(50.0, 75.0));
        assert!(data.painter.is_some());
        assert!(data.foreground_painter.is_none());
    }

    #[test]
    fn test_custom_paint_data_with_foreground() {
        let data = CustomPaintData::with_foreground(
            Box::new(MockPainter),
            Size::new(50.0, 75.0),
        );
        assert_eq!(data.size, Size::new(50.0, 75.0));
        assert!(data.painter.is_none());
        assert!(data.foreground_painter.is_some());
    }

    #[test]
    fn test_custom_paint_data_default() {
        let data = CustomPaintData::default();
        assert_eq!(data.size, Size::ZERO);
    }

    #[test]
    fn test_render_custom_paint_new() {
        let custom = SingleRenderBox::new(CustomPaintData::new(Size::new(100.0, 100.0)));
        assert_eq!(custom.size(), Size::new(100.0, 100.0));
        assert!(!custom.is_complex());
        assert!(!custom.will_change());
    }

    #[test]
    fn test_render_custom_paint_set_size() {
        let mut custom = SingleRenderBox::new(CustomPaintData::default());

        custom.set_size(Size::new(200.0, 300.0));
        assert_eq!(custom.size(), Size::new(200.0, 300.0));
        assert!(custom.needs_layout());
    }

    #[test]
    fn test_render_custom_paint_set_is_complex() {
        let mut custom = SingleRenderBox::new(CustomPaintData::default());

        custom.set_is_complex(true);
        assert!(custom.is_complex());
    }

    #[test]
    fn test_render_custom_paint_set_will_change() {
        let mut custom = SingleRenderBox::new(CustomPaintData::default());

        custom.set_will_change(true);
        assert!(custom.will_change());
        assert!(custom.needs_paint());
    }

    #[test]
    fn test_render_custom_paint_layout() {
        use flui_core::testing::mock_render_context;

        let custom = SingleRenderBox::new(CustomPaintData::new(Size::new(150.0, 250.0)));
        let constraints = BoxConstraints::new(0.0, 300.0, 0.0, 400.0);

        let (_tree, ctx) = mock_render_context();
        let size = custom.layout(constraints, &ctx);

        // No child, should use preferred size
        assert_eq!(size, Size::new(150.0, 250.0));
    }
}
