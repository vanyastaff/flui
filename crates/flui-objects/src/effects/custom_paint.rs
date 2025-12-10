//! RenderCustomPaint - custom painting with user-defined painters
//!
//! Implements Flutter's custom painting container that allows arbitrary Canvas drawing
//! before and/or after the child widget.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderCustomPaint` | `RenderCustomPaint` from `package:flutter/src/rendering/proxy_box.dart` |
//! | `CustomPainter` trait | `CustomPainter` abstract class |
//! | `painter` | `painter` property (background) |
//! | `foreground_painter` | `foregroundPainter` property |
//! | `should_repaint()` | `shouldRepaint()` method |
//! | `is_complex` | `isComplex` property |
//! | `will_change` | `willChange` property |
//!
//! # Layout Protocol
//!
//! 1. **Layout child with constraints**
//!    - Child receives same constraints (proxy behavior for layout)
//!
//! 2. **Return child size**
//!    - Container size = child size
//!
//! 3. **Cache size**
//!    - Store size for painters to use during paint
//!
//! # Paint Protocol
//!
//! 1. **Paint background painter** (if present)
//!    - Call `painter.paint(canvas, size)` before child
//!    - Draws behind child content
//!
//! 2. **Paint child**
//!    - Child painted in middle layer
//!
//! 3. **Paint foreground painter** (if present)
//!    - Call `foreground_painter.paint(canvas, size)` after child
//!    - Draws on top of child content
//!
//! # Performance
//!
//! - **Layout**: O(1) - pass-through to child
//! - **Paint**: O(p) where p = painter complexity + child paint
//! - **Memory**: ~64 bytes (2 trait objects + Size + flags + cached Size)
//!
//! # Use Cases
//!
//! - **Custom shapes**: Draw complex paths, bezier curves, polygons
//! - **Decorations**: Custom borders, backgrounds, overlays
//! - **Charts**: Line charts, bar charts, pie charts, gauges
//! - **Visualizations**: Data visualizations, infographics
//! - **Effects**: Particle systems, custom shadows, glows
//! - **Animations**: Frame-by-frame custom drawing
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::{RenderCustomPaint, CustomPainter};
//! use flui_painting::{Canvas, Paint};
//! use flui_types::{Color, Point, Size};
//!
//! // Define a custom painter
//! #[derive(Debug)]
//! struct CirclePainter {
//!     color: Color,
//! }
//!
//! impl CustomPainter for CirclePainter {
//!     fn paint(&self, canvas: &mut Canvas, size: Size) {
//!         let center = Point::new(size.width / 2.0, size.height / 2.0);
//!         let radius = size.width.min(size.height) / 2.0;
//!         let paint = Paint::fill(self.color);
//!         canvas.draw_circle(center, radius, &paint);
//!     }
//! }
//!
//! // Use as background
//! let custom = RenderCustomPaint::with_painter(
//!     Box::new(CirclePainter { color: Color::BLUE }),
//!     Size::new(100.0, 100.0)
//! );
//!
//! // Use as foreground overlay
//! let overlay = RenderCustomPaint::with_foreground(
//!     Box::new(CirclePainter { color: Color::rgba(255, 0, 0, 128) }),
//!     Size::new(100.0, 100.0)
//! );
//! ```

use flui_painting::Canvas;
use flui_rendering::{BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use flui_rendering::{RenderObject, RenderResult};
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

/// RenderObject that allows custom painting before and/or after a child.
///
/// Provides arbitrary Canvas drawing capabilities through the CustomPainter trait.
/// Background painter draws behind child, foreground painter draws on top.
///
/// # Arity
///
/// `Single` - Must have exactly 1 child.
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Use Cases
///
/// - **Custom graphics**: Shapes, paths, bezier curves beyond standard widgets
/// - **Chart rendering**: Line/bar/pie charts, gauges, data visualizations
/// - **Visual effects**: Particle systems, custom shadows, glows, gradients
/// - **Decorative overlays**: Custom borders, watermarks, badges
/// - **Dynamic drawing**: Frame-by-frame animations, interactive drawings
/// - **Complex layouts**: Custom grid lines, rulers, graph paper backgrounds
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderCustomPaint behavior:
/// - Passes constraints unchanged to child (proxy for layout)
/// - Size determined by child
/// - Background painter draws before child
/// - Foreground painter draws after child
/// - `should_repaint()` optimization for painter changes
/// - `is_complex` and `will_change` hints for rendering optimization
/// - Uses CustomPainter trait (Flutter: CustomPainter abstract class)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderCustomPaint, CustomPainter};
/// use flui_painting::{Canvas, Paint};
/// use flui_types::{Color, Size};
///
/// #[derive(Debug)]
/// struct MyPainter;
///
/// impl CustomPainter for MyPainter {
///     fn paint(&self, canvas: &mut Canvas, size: Size) {
///         // Custom drawing code
///     }
/// }
///
/// let custom = RenderCustomPaint::with_painter(
///     Box::new(MyPainter),
///     Size::new(100.0, 100.0)
/// );
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

impl RenderObject for RenderCustomPaint {}

impl RenderBox<Single> for RenderCustomPaint {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Single arity always has exactly one child
        // Layout child with our constraints
        let size = ctx.layout_child(child_id, ctx.constraints, true)?;

        // Store the laid out size for use during paint
        self.laid_out_size = size;
        Ok(size)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Use the size from layout phase
        let size = self.laid_out_size;

        // Paint background painter (if any)
        if let Some(bg_painter) = &self.painter {
            bg_painter.paint(ctx.canvas_mut(), size);
        }

        // Paint child
        ctx.paint_child(child_id, ctx.offset);

        // Paint foreground painter on top (if any)
        if let Some(fg_painter) = &self.foreground_painter {
            fg_painter.paint(ctx.canvas_mut(), size);
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
}
