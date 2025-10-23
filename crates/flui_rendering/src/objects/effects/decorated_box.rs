//! RenderDecoratedBox - paints decoration around a child

use flui_types::{Offset, Size, Rect, constraints::BoxConstraints, styling::BoxDecoration};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};
use flui_painting::BoxDecorationPainter;

/// Position of the decoration relative to the child
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecorationPosition {
    /// Paint decoration behind the child
    Background,
    /// Paint decoration in front of the child
    Foreground,
}

/// Data for RenderDecoratedBox
#[derive(Debug, Clone, PartialEq)]
pub struct DecoratedBoxData {
    /// The decoration to paint
    pub decoration: BoxDecoration,
    /// Position of the decoration
    pub position: DecorationPosition,
}

impl DecoratedBoxData {
    /// Create new decorated box data
    pub fn new(decoration: BoxDecoration) -> Self {
        Self {
            decoration,
            position: DecorationPosition::Background,
        }
    }

    /// Create with specific position
    pub fn with_position(decoration: BoxDecoration, position: DecorationPosition) -> Self {
        Self {
            decoration,
            position,
        }
    }
}

/// RenderObject that paints a decoration around its child
///
/// This uses the BoxDecorationPainter from flui_painting to render
/// backgrounds, borders, shadows, and gradients.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::effects::DecoratedBoxData};
/// use flui_types::styling::{BoxDecoration, Color};
///
/// let decoration = BoxDecoration {
///     color: Some(Color::WHITE),
///     border: None,
///     border_radius: None,
///     box_shadow: None,
///     gradient: None,
/// };
/// let mut decorated = SingleRenderBox::new(DecoratedBoxData::new(decoration));
/// ```
pub type RenderDecoratedBox = SingleRenderBox<DecoratedBoxData>;

// ===== Public API =====

impl RenderDecoratedBox {
    /// Get the decoration
    pub fn decoration(&self) -> &BoxDecoration {
        &self.data().decoration
    }

    /// Get the decoration position
    pub fn position(&self) -> DecorationPosition {
        self.data().position
    }

    /// Set new decoration
    ///
    /// If decoration changes, marks as needing paint (not layout).
    pub fn set_decoration(&mut self, decoration: BoxDecoration) {
        if self.data().decoration != decoration {
            self.data_mut().decoration = decoration;
            self.mark_needs_paint();
        }
    }

    /// Set decoration position
    ///
    /// If position changes, marks as needing paint (not layout).
    pub fn set_position(&mut self, position: DecorationPosition) {
        if self.data().position != position {
            self.data_mut().position = position;
            self.mark_needs_paint();
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderDecoratedBox {
    fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
        // Store constraints
        *state.constraints.lock() = Some(constraints);

        // Get children from ElementTree via RenderContext
        let children_ids = ctx.children();

        // Layout child with same constraints
        let size = if let Some(&child_id) = children_ids.first() {
            ctx.layout_child(child_id, constraints)
        } else {
            // No child - use smallest size
            constraints.smallest()
        };

        // Store size and clear needs_layout flag
        *state.size.lock() = Some(size);
        state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);

        size
    }

    fn paint(&self, state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) {
        let size = state.size.lock().unwrap_or(Size::ZERO);
        let rect = Rect::from_xywh(offset.dx, offset.dy, size.width, size.height);

        let decoration = &self.data().decoration;
        let position = self.data().position;

        // Paint decoration in background position
        if position == DecorationPosition::Background {
            BoxDecorationPainter::paint(painter, rect, decoration);
        }

        // Get children from ElementTree via RenderContext
        let children_ids = ctx.children();

        // Paint child
        if let Some(&child_id) = children_ids.first() {
            ctx.paint_child(child_id, painter, offset);
        }

        // Paint decoration in foreground position
        if position == DecorationPosition::Foreground {
            BoxDecorationPainter::paint(painter, rect, decoration);
        }
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::styling::Color;

    #[test]
    fn test_render_decorated_box_new() {
        let decoration = BoxDecoration {
            color: Some(Color::WHITE),
            border: None,
            border_radius: None,
            box_shadow: None,
            gradient: None,
        };
        let mut decorated = SingleRenderBox::new(DecoratedBoxData::new(decoration.clone()));
        assert_eq!(decorated.decoration(), &decoration);
        assert_eq!(decorated.position(), DecorationPosition::Background);
    }

    #[test]
    fn test_render_decorated_box_set_decoration() {
        use flui_core::testing::mock_render_context;

        let decoration1 = BoxDecoration {
            color: Some(Color::WHITE),
            border: None,
            border_radius: None,
            box_shadow: None,
            gradient: None,
        };
        let mut decorated = SingleRenderBox::new(DecoratedBoxData::new(decoration1));

        // Clear initial needs_layout flag by doing a layout
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
        let (_tree, ctx) = mock_render_context();
        let _ = decorated.layout(constraints, &ctx);

        // Now set decoration - should only mark needs_paint, not needs_layout
        let decoration2 = BoxDecoration {
            color: Some(Color::BLACK),
            border: None,
            border_radius: None,
            box_shadow: None,
            gradient: None,
        };
        decorated.set_decoration(decoration2.clone());
        assert_eq!(decorated.decoration(), &decoration2);
        assert!(decorated.needs_paint());
        assert!(!decorated.needs_layout());
    }

    #[test]
    fn test_render_decorated_box_set_position() {
        use flui_core::testing::mock_render_context;

        let decoration = BoxDecoration {
            color: Some(Color::WHITE),
            border: None,
            border_radius: None,
            box_shadow: None,
            gradient: None,
        };
        let mut decorated = SingleRenderBox::new(DecoratedBoxData::new(decoration));

        // Clear initial needs_layout flag by doing a layout
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
        let (_tree, ctx) = mock_render_context();
        let _ = decorated.layout(constraints, &ctx);

        // Now set position - should only mark needs_paint, not needs_layout
        decorated.set_position(DecorationPosition::Foreground);
        assert_eq!(decorated.position(), DecorationPosition::Foreground);
        assert!(decorated.needs_paint());
        assert!(!decorated.needs_layout());
    }

    #[test]
    fn test_render_decorated_box_layout_no_child() {
        use flui_core::testing::mock_render_context;

        let decoration = BoxDecoration {
            color: Some(Color::WHITE),
            border: None,
            border_radius: None,
            box_shadow: None,
            gradient: None,
        };
        let mut decorated = SingleRenderBox::new(DecoratedBoxData::new(decoration));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let (_tree, ctx) = mock_render_context();
        let size = decorated.layout(constraints, &ctx);

        // Should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
    }

    #[test]
    fn test_decoration_position_variants() {
        // Test enum variants
        assert_eq!(DecorationPosition::Background, DecorationPosition::Background);
        assert_eq!(DecorationPosition::Foreground, DecorationPosition::Foreground);
        assert_ne!(DecorationPosition::Background, DecorationPosition::Foreground);
    }

    #[test]
    fn test_decorated_box_data_with_position() {
        let decoration = BoxDecoration {
            color: Some(Color::WHITE),
            border: None,
            border_radius: None,
            box_shadow: None,
            gradient: None,
        };
        let data = DecoratedBoxData::with_position(decoration.clone(), DecorationPosition::Foreground);
        assert_eq!(data.decoration, decoration);
        assert_eq!(data.position, DecorationPosition::Foreground);
    }
}
