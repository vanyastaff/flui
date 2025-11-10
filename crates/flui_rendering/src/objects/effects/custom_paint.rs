//! RenderCustomPaint - custom painting with user-defined painters

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};

use flui_engine::{BoxedLayer, PictureLayer};
use flui_types::Size;

/// Custom painter trait
///
/// Implement this trait to define custom painting logic.
pub trait CustomPainter: std::fmt::Debug + Send + Sync {
    /// Paint custom content into a PictureLayer
    fn paint(&self, picture: &mut PictureLayer, size: Size);

    /// Whether this painter should repaint when something changes
    fn should_repaint(&self, _old: &dyn CustomPainter) -> bool {
        true
    }
}

/// RenderObject that allows custom painting
///
/// This widget allows you to paint custom graphics before and/or after
/// the child_id widget. Useful for drawing custom shapes, decorations, etc.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderCustomPaint;
///
/// // Custom paint with preferred size
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

impl Render for RenderCustomPaint {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let constraints = ctx.constraints;
        // SingleArity always has exactly one child_id
        // Layout child_id with our constraints
        let size = tree.layout_child(child_id, constraints);

        // Store the laid out size for use during paint
        self.laid_out_size = size;
        size
    }

    fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let offset = ctx.offset;
        // Use the size from layout phase
        let size = self.laid_out_size;
        let mut layers: Vec<BoxedLayer> = Vec::new();

        // Paint background painter
        if let Some(bg_painter) = &self.painter {
            let mut picture = PictureLayer::new();
            bg_painter.paint(&mut picture, size);
            layers.push(Box::new(picture));
        }

        // Paint child_id - SingleArity always has exactly one child_id
        layers.push(tree.paint_child(child_id, offset));

        // Paint foreground painter (on top of child_id)
        if let Some(fg_painter) = &self.foreground_painter {
            let mut picture = PictureLayer::new();
            fg_painter.paint(&mut picture, size);
            layers.push(Box::new(picture));
        }

        // Wrap all layers in a container - use pool for efficiency
        let mut container = flui_engine::layer::pool::acquire_container();
        for layer in layers {
            container.add_child(layer);
        }
        Box::new(container)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Variable // Default - update if needed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock painter for testing
    #[derive(Debug)]
    struct MockPainter;

    impl CustomPainter for MockPainter {
        fn paint(&self, _picture: &mut PictureLayer, _size: Size) {
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
}
