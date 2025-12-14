//! RenderDecoratedBox - paints decoration around its child.
//!
//! This render object paints backgrounds, borders, and shadows.

use flui_types::{BoxConstraints, Color, Offset, Point, Rect, Size};

use crate::containers::ProxyBox;
use crate::objects::r#box::effects::clip_rrect::BorderRadius;
use crate::pipeline::PaintingContext;
use crate::traits::TextBaseline;

/// Position of the decoration relative to the child.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DecorationPosition {
    /// Decoration paints behind the child.
    #[default]
    Background,
    /// Decoration paints in front of the child.
    Foreground,
}

/// Box shadow configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxShadow {
    /// Shadow color.
    pub color: Color,
    /// Horizontal offset.
    pub offset_x: f32,
    /// Vertical offset.
    pub offset_y: f32,
    /// Blur radius.
    pub blur_radius: f32,
    /// Spread radius.
    pub spread_radius: f32,
}

impl BoxShadow {
    /// Creates a new box shadow.
    pub fn new(
        color: Color,
        offset_x: f32,
        offset_y: f32,
        blur_radius: f32,
        spread_radius: f32,
    ) -> Self {
        Self {
            color,
            offset_x,
            offset_y,
            blur_radius,
            spread_radius,
        }
    }

    /// Creates a shadow with default black color.
    pub fn simple(offset_x: f32, offset_y: f32, blur_radius: f32) -> Self {
        Self::new(
            Color::rgba(0, 0, 0, 64),
            offset_x,
            offset_y,
            blur_radius,
            0.0,
        )
    }
}

impl Default for BoxShadow {
    fn default() -> Self {
        Self::new(Color::TRANSPARENT, 0.0, 0.0, 0.0, 0.0)
    }
}

/// Border side configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BorderSide {
    /// Border color.
    pub color: Color,
    /// Border width.
    pub width: f32,
}

impl BorderSide {
    /// No border.
    pub const NONE: Self = Self {
        color: Color::TRANSPARENT,
        width: 0.0,
    };

    /// Creates a new border side.
    pub fn new(color: Color, width: f32) -> Self {
        Self { color, width }
    }
}

impl Default for BorderSide {
    fn default() -> Self {
        Self::NONE
    }
}

/// Box border configuration.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BoxBorder {
    /// Top border.
    pub top: BorderSide,
    /// Right border.
    pub right: BorderSide,
    /// Bottom border.
    pub bottom: BorderSide,
    /// Left border.
    pub left: BorderSide,
}

impl BoxBorder {
    /// No border.
    pub const NONE: Self = Self {
        top: BorderSide::NONE,
        right: BorderSide::NONE,
        bottom: BorderSide::NONE,
        left: BorderSide::NONE,
    };

    /// Creates a uniform border.
    pub fn all(side: BorderSide) -> Self {
        Self {
            top: side,
            right: side,
            bottom: side,
            left: side,
        }
    }

    /// Creates a border with only some sides.
    pub fn only(top: BorderSide, right: BorderSide, bottom: BorderSide, left: BorderSide) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    /// Returns true if all sides have zero width.
    pub fn is_none(&self) -> bool {
        self.top.width == 0.0
            && self.right.width == 0.0
            && self.bottom.width == 0.0
            && self.left.width == 0.0
    }
}

/// Box decoration configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct BoxDecoration {
    /// Background color.
    pub color: Option<Color>,
    /// Border.
    pub border: BoxBorder,
    /// Border radius.
    pub border_radius: BorderRadius,
    /// Box shadows.
    pub box_shadow: Vec<BoxShadow>,
}

impl BoxDecoration {
    /// Creates a decoration with just a color.
    pub fn color(color: Color) -> Self {
        Self {
            color: Some(color),
            border: BoxBorder::NONE,
            border_radius: BorderRadius::ZERO,
            box_shadow: Vec::new(),
        }
    }

    /// Creates a decoration with all options.
    pub fn new(
        color: Option<Color>,
        border: BoxBorder,
        border_radius: BorderRadius,
        box_shadow: Vec<BoxShadow>,
    ) -> Self {
        Self {
            color,
            border,
            border_radius,
            box_shadow,
        }
    }

    /// Returns whether the decoration has rounded corners.
    pub fn has_rounded_corners(&self) -> bool {
        self.border_radius != BorderRadius::ZERO
    }
}

impl Default for BoxDecoration {
    fn default() -> Self {
        Self {
            color: None,
            border: BoxBorder::NONE,
            border_radius: BorderRadius::ZERO,
            box_shadow: Vec::new(),
        }
    }
}

/// A render object that paints decoration around its child.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::r#box::effects::{RenderDecoratedBox, BoxDecoration};
/// use flui_types::Color;
///
/// let decoration = BoxDecoration::color(Color::rgb(200, 200, 200));
/// let decorated = RenderDecoratedBox::new(decoration);
/// ```
#[derive(Debug)]
pub struct RenderDecoratedBox {
    /// Container holding the child and geometry.
    proxy: ProxyBox,

    /// The decoration to paint.
    decoration: BoxDecoration,

    /// Position of decoration relative to child.
    position: DecorationPosition,
}

impl RenderDecoratedBox {
    /// Creates a new decorated box.
    pub fn new(decoration: BoxDecoration) -> Self {
        Self {
            proxy: ProxyBox::new(),
            decoration,
            position: DecorationPosition::Background,
        }
    }

    /// Creates with position specified.
    pub fn with_position(decoration: BoxDecoration, position: DecorationPosition) -> Self {
        Self {
            proxy: ProxyBox::new(),
            decoration,
            position,
        }
    }

    /// Returns the decoration.
    pub fn decoration(&self) -> &BoxDecoration {
        &self.decoration
    }

    /// Sets the decoration.
    pub fn set_decoration(&mut self, decoration: BoxDecoration) {
        if self.decoration != decoration {
            self.decoration = decoration;
        }
    }

    /// Returns the position.
    pub fn position(&self) -> DecorationPosition {
        self.position
    }

    /// Sets the position.
    pub fn set_position(&mut self, position: DecorationPosition) {
        if self.position != position {
            self.position = position;
        }
    }

    /// Returns the current size.
    pub fn size(&self) -> Size {
        *self.proxy.geometry()
    }

    /// Performs layout without a child.
    pub fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        let size = constraints.smallest();
        self.proxy.set_geometry(size);
        size
    }

    /// Performs layout with a child size.
    pub fn perform_layout_with_child(
        &mut self,
        _constraints: BoxConstraints,
        child_size: Size,
    ) -> Size {
        self.proxy.set_geometry(child_size);
        child_size
    }

    /// Returns constraints for the child.
    pub fn constraints_for_child(&self, constraints: BoxConstraints) -> BoxConstraints {
        constraints
    }

    /// Paints the decoration.
    fn paint_decoration(&self, context: &mut PaintingContext, offset: Offset) {
        let size = self.size();
        let rect = Rect::from_origin_size(Point::new(offset.dx, offset.dy), size);

        // Paint shadows first (behind everything)
        for shadow in &self.decoration.box_shadow {
            // In real implementation: paint shadow with offset and blur
            let _ = shadow;
        }

        // Paint background color
        if let Some(color) = self.decoration.color {
            if self.decoration.has_rounded_corners() {
                let rrect = self.decoration.border_radius.to_rrect(rect);
                // In real implementation: context.canvas.draw_rrect(rrect, color);
                let _ = (&*context, rrect, color);
            } else {
                // In real implementation: context.canvas.draw_rect(rect, color);
                let _ = (&*context, rect, color);
            }
        }

        // Paint border
        if !self.decoration.border.is_none() {
            // In real implementation: paint border with proper stroke
            let _ = (&*context, rect);
        }
    }

    /// Paints this render object.
    pub fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        match self.position {
            DecorationPosition::Background => {
                self.paint_decoration(context, offset);
                // In real implementation: context.paint_child(child, offset);
            }
            DecorationPosition::Foreground => {
                // In real implementation: context.paint_child(child, offset);
                self.paint_decoration(context, offset);
            }
        }
    }

    /// Hit test - delegates to child.
    pub fn hit_test(&self, position: Offset) -> bool {
        let size = self.size();
        let rect = Rect::from_origin_size(Point::ZERO, size);

        if self.decoration.has_rounded_corners() {
            let rrect = self.decoration.border_radius.to_rrect(rect);
            rrect.contains(Point::new(position.dx, position.dy))
        } else {
            rect.contains(Point::new(position.dx, position.dy))
        }
    }

    /// Computes minimum intrinsic width.
    pub fn compute_min_intrinsic_width(&self, _height: f32, child_width: Option<f32>) -> f32 {
        child_width.unwrap_or(0.0)
    }

    /// Computes maximum intrinsic width.
    pub fn compute_max_intrinsic_width(&self, _height: f32, child_width: Option<f32>) -> f32 {
        child_width.unwrap_or(0.0)
    }

    /// Computes minimum intrinsic height.
    pub fn compute_min_intrinsic_height(&self, _width: f32, child_height: Option<f32>) -> f32 {
        child_height.unwrap_or(0.0)
    }

    /// Computes maximum intrinsic height.
    pub fn compute_max_intrinsic_height(&self, _width: f32, child_height: Option<f32>) -> f32 {
        child_height.unwrap_or(0.0)
    }

    /// Computes distance to baseline.
    pub fn compute_distance_to_baseline(
        &self,
        _baseline: TextBaseline,
        child_baseline: Option<f32>,
    ) -> Option<f32> {
        child_baseline
    }
}

impl Default for RenderDecoratedBox {
    fn default() -> Self {
        Self::new(BoxDecoration::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decorated_box_new() {
        let decoration = BoxDecoration::color(Color::rgb(255, 0, 0));
        let decorated = RenderDecoratedBox::new(decoration);
        assert_eq!(decorated.position(), DecorationPosition::Background);
    }

    #[test]
    fn test_decoration_position() {
        let decoration = BoxDecoration::default();
        let mut decorated = RenderDecoratedBox::new(decoration);
        decorated.set_position(DecorationPosition::Foreground);
        assert_eq!(decorated.position(), DecorationPosition::Foreground);
    }

    #[test]
    fn test_box_decoration_color() {
        let decoration = BoxDecoration::color(Color::rgb(100, 150, 200));
        assert_eq!(decoration.color, Some(Color::rgb(100, 150, 200)));
        assert!(!decoration.has_rounded_corners());
    }

    #[test]
    fn test_box_border_all() {
        let side = BorderSide::new(Color::BLACK, 2.0);
        let border = BoxBorder::all(side);
        assert!(!border.is_none());
        assert_eq!(border.top.width, 2.0);
    }

    #[test]
    fn test_box_shadow() {
        let shadow = BoxShadow::simple(2.0, 2.0, 4.0);
        assert_eq!(shadow.offset_x, 2.0);
        assert_eq!(shadow.offset_y, 2.0);
        assert_eq!(shadow.blur_radius, 4.0);
    }

    #[test]
    fn test_hit_test_rect() {
        let mut decorated = RenderDecoratedBox::default();
        decorated.perform_layout_with_child(
            BoxConstraints::tight(Size::new(100.0, 100.0)),
            Size::new(100.0, 100.0),
        );

        assert!(decorated.hit_test(Offset::new(50.0, 50.0)));
        assert!(!decorated.hit_test(Offset::new(150.0, 50.0)));
    }

    #[test]
    fn test_layout() {
        let mut decorated = RenderDecoratedBox::default();
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 150.0);
        let child_size = Size::new(100.0, 75.0);

        let size = decorated.perform_layout_with_child(constraints, child_size);

        assert_eq!(size, child_size);
    }
}
