//! RenderCustomPaint - custom painting with user-defined painters

use crate::core::{
    FullRenderTree,
    FullRenderTree, RenderBox, Optional, {BoxProtocol, LayoutContext, PaintContext},
};
use flui_painting::Canvas;
use flui_types::Size;

/// Custom painter trait
///
/// Implement this trait to define custom painting logic.
pub trait CustomPainter: std::fmt::Debug + Send + Sync {
    /// Paint custom content into a Canvas
    fn paint(&self, canvas: &mut Canvas, size: Size);

    /// Whether this painter should repaint when something changes
    fn should_repaint(&self, _old: &dyn CustomPainter) -> bool {
        true
    }
}

/// RenderObject that allows custom painting
///
/// This widget allows you to paint custom graphics before and/or after
/// the child widget. Useful for drawing custom shapes, decorations, etc.
///
/// # Without Child
///
/// When no child is present, CustomPaint uses its `size` field to determine
/// layout size (decorative painting only).
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderCustomPaint;
///
/// // Custom paint with preferred size (can work without child)
/// let custom = RenderCustomPaint::new(Size::new(100.0, 100.0));
/// ```
#[derive(Debug)]
pub struct RenderCustomPaint {
    /// Foreground painter (painted on top of child_id)
    pub foreground_painter: Option<Box<dyn CustomPainter>>,
    /// Background painter (painted behind child_id)
    pub painter: Option<Box<dyn CustomPainter>>,
    /// Size to use when child_id is not present
    pub size: Size,
    /// Whether child_id is interactive (if false, hit tests go through)
    pub is_complex: bool,
    /// Whether foreground paints on top of child_id
    pub will_change: bool,
    /// Laid out size (set during layout, used during paint)
    laid_out_size: Size,
}

// ===== Public API =====

impl RenderCustomPaint {
    /// Create new custom paint
    pub fn new(size: Size) -> Self {
        Self {
            foreground_painter: None,
            painter: None,
            size,
            is_complex: false,
            will_change: false,
            laid_out_size: Size::ZERO,
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
            laid_out_size: Size::ZERO,
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
            laid_out_size: Size::ZERO,
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
            laid_out_size: Size::ZERO,
        }
    }

    /// Get size
    pub fn size(&self) -> Size {
        self.size
    }

    /// Get is_complex flag
    pub fn is_complex(&self) -> bool {
        self.is_complex
    }

    /// Get will_change flag
    pub fn will_change(&self) -> bool {
        self.will_change
    }

    /// Set size
    pub fn set_size(&mut self, size: Size) {
        self.size = size;
    }

    /// Set is_complex flag
    pub fn set_is_complex(&mut self, is_complex: bool) {
        self.is_complex = is_complex;
    }

    /// Set will_change flag
    pub fn set_will_change(&mut self, will_change: bool) {
        self.will_change = will_change;
    }
}

impl Default for RenderCustomPaint {
    fn default() -> Self {
        Self::new(Size::ZERO)
    }
}

// ===== RenderObject Implementation =====

impl<T: FullRenderTree> RenderBox<T, Optional> for RenderCustomPaint {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Optional, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let constraints = ctx.constraints;

        let size = if let Some(child_id) = ctx.children.get() {
            // Layout child and use its size
            ctx.layout_child(child_id, constraints)
        } else {
            // No child - use preferred size constrained by layout constraints
            // This matches Flutter's behavior: constraints.constrain(preferredSize ?? Size.zero)
            constraints.constrain(self.size)
        };

        // Store the laid out size for use during paint
        self.laid_out_size = size;
        size
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Optional>)
    where
        T: crate::core::PaintTree,
    {
        // Use the size from layout phase
        let size = self.laid_out_size;
        let offset = ctx.offset;

        // Paint background painter (if any)
        if let Some(bg_painter) = &self.painter {
            bg_painter.paint(ctx.canvas(), size);
        }

        // Paint child if present
        if let Some(child_id) = ctx.children.get() {
            ctx.paint_child(child_id, offset);
        }

        // Paint foreground painter on top (if any)
        if let Some(fg_painter) = &self.foreground_painter {
            fg_painter.paint(ctx.canvas(), size);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock painter for testing
    #[derive(Debug)]
    struct MockPainter;

    impl CustomPainter for MockPainter {
        fn paint(&self, _canvas: &mut Canvas, _size: Size) {
            // Do nothing
        }
    }

    #[test]
    fn test_render_custom_paint_new() {
        let custom = RenderCustomPaint::new(Size::new(100.0, 100.0));
        assert_eq!(custom.size(), Size::new(100.0, 100.0));
        assert!(!custom.is_complex());
        assert!(!custom.will_change());
    }

    #[test]
    fn test_render_custom_paint_set_size() {
        let mut custom = RenderCustomPaint::default();

        custom.set_size(Size::new(200.0, 300.0));
        assert_eq!(custom.size(), Size::new(200.0, 300.0));
    }

    #[test]
    fn test_render_custom_paint_set_is_complex() {
        let mut custom = RenderCustomPaint::default();

        custom.set_is_complex(true);
        assert!(custom.is_complex());
    }

    #[test]
    fn test_render_custom_paint_set_will_change() {
        let mut custom = RenderCustomPaint::default();

        custom.set_will_change(true);
        assert!(custom.will_change());
    }

    #[test]
    fn test_render_custom_paint_with_painter() {
        let custom = RenderCustomPaint::with_painter(Box::new(MockPainter), Size::new(50.0, 75.0));
        assert_eq!(custom.size(), Size::new(50.0, 75.0));
        assert!(custom.painter.is_some());
        assert!(custom.foreground_painter.is_none());
    }

    #[test]
    fn test_render_custom_paint_with_foreground() {
        let custom =
            RenderCustomPaint::with_foreground(Box::new(MockPainter), Size::new(50.0, 75.0));
        assert_eq!(custom.size(), Size::new(50.0, 75.0));
        assert!(custom.painter.is_none());
        assert!(custom.foreground_painter.is_some());
    }

    #[test]
    fn test_render_custom_paint_with_both() {
        let custom = RenderCustomPaint::with_both(
            Box::new(MockPainter),
            Box::new(MockPainter),
            Size::new(100.0, 100.0),
        );
        assert_eq!(custom.size(), Size::new(100.0, 100.0));
        assert!(custom.painter.is_some());
        assert!(custom.foreground_painter.is_some());
    }

    #[test]
    fn test_render_custom_paint_optional_arity_supports_no_child() {
        // This test documents that RenderCustomPaint now uses Optional arity
        // which allows it to work without a child (decorative use case)
        let custom = RenderCustomPaint::with_painter(Box::new(MockPainter), Size::new(200.0, 150.0));
        assert_eq!(custom.size(), Size::new(200.0, 150.0));

        // The key improvement: CustomPaint can now be used for decorative painting
        // without requiring a child widget, matching Flutter's behavior
    }

    #[test]
    fn test_render_custom_paint_default_creates_zero_size() {
        let custom = RenderCustomPaint::default();
        assert_eq!(custom.size(), Size::ZERO);
        assert!(custom.painter.is_none());
        assert!(custom.foreground_painter.is_none());
    }
}
