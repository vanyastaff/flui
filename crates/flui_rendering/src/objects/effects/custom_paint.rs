//! RenderCustomPaint - custom painting with user-defined painters

use flui_types::Size;
use flui_core::render::{RenderObject, SingleArity, LayoutCx, PaintCx, SingleChild, SingleChildPaint};
use flui_engine::{BoxedLayer, PictureLayer};

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
/// use flui_rendering::RenderCustomPaint;
///
/// // Custom paint with preferred size
/// let custom = RenderCustomPaint::new(Size::new(100.0, 100.0));
/// ```
#[derive(Debug)]
pub struct RenderCustomPaint {
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

impl RenderObject for RenderCustomPaint {
    type Arity = SingleArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        // SingleArity always has exactly one child
        // Layout child with our constraints
        let child = cx.child();
        let size = cx.layout_child(child, cx.constraints());

        // Store the laid out size for use during paint
        self.laid_out_size = size;
        size
    }

    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        use flui_engine::ContainerLayer;

        // Use the size from layout phase
        let size = self.laid_out_size;
        let mut layers: Vec<BoxedLayer> = Vec::new();

        // Paint background painter
        if let Some(bg_painter) = &self.painter {
            let mut picture = PictureLayer::new();
            bg_painter.paint(&mut picture, size);
            layers.push(Box::new(picture));
        }

        // Paint child - SingleArity always has exactly one child
        let child = cx.child();
        layers.push(cx.capture_child_layer(child));

        // Paint foreground painter (on top of child)
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
        let custom = RenderCustomPaint::with_painter(
            Box::new(MockPainter),
            Size::new(50.0, 75.0),
        );
        assert_eq!(custom.size(), Size::new(50.0, 75.0));
        assert!(custom.painter.is_some());
        assert!(custom.foreground_painter.is_none());
    }

    #[test]
    fn test_render_custom_paint_with_foreground() {
        let custom = RenderCustomPaint::with_foreground(
            Box::new(MockPainter),
            Size::new(50.0, 75.0),
        );
        assert_eq!(custom.size(), Size::new(50.0, 75.0));
        assert!(custom.painter.is_none());
        assert!(custom.foreground_painter.is_some());
    }
}
