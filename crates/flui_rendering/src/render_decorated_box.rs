//! RenderDecoratedBox - paints BoxDecoration around child
//!
//! This is the render object for DecoratedBox widget.
//! Similar to Flutter's RenderDecoratedBox.
//!
//! It paints a BoxDecoration (color, border, border radius, shadows, gradient)
//! before or after painting the child.

use crate::{BoxConstraints, Offset, RenderObject, Size};
use crate::decoration_painter::BoxDecorationPainter;
use flui_types::styling::BoxDecoration;

/// Position for painting decoration relative to child
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecorationPosition {
    /// Paint decoration behind the child
    Background,

    /// Paint decoration in front of the child
    Foreground,
}

/// RenderDecoratedBox - paints BoxDecoration around child
///
/// Decorates a child with a BoxDecoration. The decoration is painted
/// either before or after the child depending on the decoration position.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderDecoratedBox;
/// use flui_types::styling::{BoxDecoration, Color};
///
/// let decoration = BoxDecoration::default()
///     .with_color(Color::from_rgba(255, 0, 0, 255));
///
/// let mut decorated = RenderDecoratedBox::new(decoration);
/// ```
#[derive(Debug)]
pub struct RenderDecoratedBox {
    /// The painter for the decoration
    painter: Option<BoxDecorationPainter>,

    /// Where to paint the decoration (background or foreground)
    position: DecorationPosition,

    /// The single child
    child: Option<Box<dyn RenderObject>>,

    /// Current size after layout
    size: Size,

    /// Current constraints
    constraints: Option<BoxConstraints>,

    /// Layout dirty flag
    needs_layout_flag: bool,

    /// Paint dirty flag
    needs_paint_flag: bool,
}

impl RenderDecoratedBox {
    /// Create a new RenderDecoratedBox with decoration
    pub fn new(decoration: BoxDecoration) -> Self {
        let painter = Some(BoxDecorationPainter::new(decoration));
        Self {
            painter,
            position: DecorationPosition::Background,
            child: None,
            size: Size::zero(),
            constraints: None,
            needs_layout_flag: true,
            needs_paint_flag: true,
        }
    }

    /// Create RenderDecoratedBox with foreground decoration
    pub fn foreground(decoration: BoxDecoration) -> Self {
        let painter = Some(BoxDecorationPainter::new(decoration));
        Self {
            painter,
            position: DecorationPosition::Foreground,
            child: None,
            size: Size::zero(),
            constraints: None,
            needs_layout_flag: true,
            needs_paint_flag: true,
        }
    }

    /// Set the decoration
    pub fn set_decoration(&mut self, decoration: BoxDecoration) {
        // Check if decoration changed
        let needs_update = self.painter.as_ref()
            .is_none_or(|p| p.decoration() != &decoration);

        if needs_update {
            self.painter = Some(BoxDecorationPainter::new(decoration));
            self.mark_needs_paint();
        }
    }

    /// Get the decoration
    pub fn decoration(&self) -> Option<&BoxDecoration> {
        self.painter.as_ref().map(|p| p.decoration())
    }

    /// Set decoration position
    pub fn set_position(&mut self, position: DecorationPosition) {
        if self.position != position {
            self.position = position;
            self.mark_needs_paint();
        }
    }

    /// Get decoration position
    pub fn position(&self) -> DecorationPosition {
        self.position
    }

    /// Set the child
    pub fn set_child(&mut self, child: Box<dyn RenderObject>) {
        self.child = Some(child);
        self.mark_needs_layout();
    }

    /// Remove the child
    pub fn remove_child(&mut self) {
        self.child = None;
        self.mark_needs_layout();
    }

    /// Get reference to child
    pub fn child(&self) -> Option<&dyn RenderObject> {
        self.child.as_deref()
    }

    /// Perform layout
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        self.constraints = Some(constraints);

        if let Some(child) = &mut self.child {
            // Child gets same constraints
            self.size = child.layout(constraints);
        } else {
            // No child - use smallest size
            self.size = constraints.smallest();
        }

        self.size
    }
}

impl Default for RenderDecoratedBox {
    fn default() -> Self {
        Self::new(BoxDecoration::default())
    }
}

impl RenderObject for RenderDecoratedBox {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.needs_layout_flag = false;
        self.perform_layout(constraints)
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        use flui_types::Rect;

        let rect = Rect::from_min_size(offset, self.size);

        // Paint decoration behind child (background)
        if self.position == DecorationPosition::Background {
            if let Some(box_painter) = &self.painter {
                box_painter.paint(painter, rect);
            }
        }

        // Paint child
        if let Some(child) = &self.child {
            child.paint(painter, offset);
        }

        // Paint decoration in front of child (foreground)
        if self.position == DecorationPosition::Foreground {
            if let Some(box_painter) = &self.painter {
                box_painter.paint(painter, rect);
            }
        }
    }

    fn size(&self) -> Size {
        self.size
    }

    fn constraints(&self) -> Option<BoxConstraints> {
        self.constraints
    }

    fn needs_layout(&self) -> bool {
        self.needs_layout_flag
    }

    fn mark_needs_layout(&mut self) {
        self.needs_layout_flag = true;
    }

    fn needs_paint(&self) -> bool {
        self.needs_paint_flag
    }

    fn mark_needs_paint(&mut self) {
        self.needs_paint_flag = true;
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn RenderObject)) {
        if let Some(child) = &self.child {
            visitor(&**child);
        }
    }

    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn RenderObject)) {
        if let Some(child) = &mut self.child {
            visitor(&mut **child);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RenderBox;
    use flui_types::styling::Color;

    #[test]
    fn test_render_decorated_box_new() {
        let decoration = BoxDecoration::default();
        let decorated = RenderDecoratedBox::new(decoration.clone());
        assert!(decorated.needs_layout());
        assert_eq!(decorated.decoration(), Some(&decoration));
        assert_eq!(decorated.position(), DecorationPosition::Background);
    }

    #[test]
    fn test_render_decorated_box_foreground() {
        let decoration = BoxDecoration::default();
        let decorated = RenderDecoratedBox::foreground(decoration);
        assert_eq!(decorated.position(), DecorationPosition::Foreground);
    }

    #[test]
    fn test_render_decorated_box_no_child() {
        let mut decorated = RenderDecoratedBox::new(BoxDecoration::default());
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let size = decorated.layout(constraints);

        // No child - should use smallest size
        assert_eq!(size, Size::zero());
    }

    #[test]
    fn test_render_decorated_box_with_child() {
        let mut decorated = RenderDecoratedBox::new(BoxDecoration::default());
        decorated.set_child(Box::new(RenderBox::new()));

        let constraints = BoxConstraints::tight(Size::new(100.0, 50.0));
        let size = decorated.layout(constraints);

        // Should match child size
        assert_eq!(size, Size::new(100.0, 50.0));
        assert_eq!(decorated.child().unwrap().size(), Size::new(100.0, 50.0));
    }

    #[test]
    fn test_render_decorated_box_set_decoration() {
        let mut decorated = RenderDecoratedBox::new(BoxDecoration::default());

        let new_decoration = BoxDecoration::with_color(Color::rgba(255, 0, 0, 255));
        decorated.set_decoration(new_decoration.clone());

        assert_eq!(decorated.decoration(), Some(&new_decoration));
        assert!(decorated.needs_paint());
    }

    #[test]
    fn test_render_decorated_box_set_position() {
        let mut decorated = RenderDecoratedBox::new(BoxDecoration::default());
        assert_eq!(decorated.position(), DecorationPosition::Background);

        decorated.set_position(DecorationPosition::Foreground);
        assert_eq!(decorated.position(), DecorationPosition::Foreground);
        assert!(decorated.needs_paint());
    }

    #[test]
    fn test_render_decorated_box_remove_child() {
        let mut decorated = RenderDecoratedBox::new(BoxDecoration::default());
        decorated.set_child(Box::new(RenderBox::new()));

        assert!(decorated.child().is_some());

        decorated.remove_child();
        assert!(decorated.child().is_none());
        assert!(decorated.needs_layout());
    }

    #[test]
    fn test_render_decorated_box_visit_children() {
        let mut decorated = RenderDecoratedBox::new(BoxDecoration::default());
        decorated.set_child(Box::new(RenderBox::new()));

        let mut count = 0;
        decorated.visit_children(&mut |_| count += 1);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_render_decorated_box_visit_children_no_child() {
        let decorated = RenderDecoratedBox::new(BoxDecoration::default());

        let mut count = 0;
        decorated.visit_children(&mut |_| count += 1);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_decoration_position_variants() {
        let bg = DecorationPosition::Background;
        let fg = DecorationPosition::Foreground;

        assert_ne!(bg, fg);
    }
}
