//! `RenderDecoratedBox` — paints a [`BoxDecoration`] around its child.
//!
//! Flutter parity: proxy_box.dart `RenderDecoratedBox`. The decoration
//! paints either BEHIND the child (`DecorationPosition::Background`,
//! the default) or IN FRONT of it (`Foreground`) — in the fragment
//! paint model that is simply the order of canvas ops around the
//! `paint_child` marker. Hit testing delegates to the decoration's
//! geometry (rounded corners exclude the rect's corners), then to the
//! child.

use flui_painting::{box_decoration_hit_test, paint_box_decoration};
use flui_tree::Single;
use flui_types::{Offset, Pixels, Point, Rect, Size, styling::BoxDecoration};

use crate::{
    context::{BoxHitTestContext, BoxLayoutContext, PaintCx},
    parent_data::BoxParentData,
    traits::{HotReloadCapability, PaintEffectsCapability, RenderBox, SemanticsCapability},
};

/// Where the decoration paints relative to the child.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DecorationPosition {
    /// Behind the child (the common case).
    #[default]
    Background,
    /// In front of the child (e.g. a vignette or focus ring).
    Foreground,
}

/// A render object that paints a [`BoxDecoration`] before or after its
/// child.
#[derive(Debug, Clone)]
pub struct RenderDecoratedBox {
    /// What to paint.
    decoration: BoxDecoration<Pixels>,
    /// Behind or in front of the child.
    position: DecorationPosition,
    /// Whether we have a child.
    has_child: bool,
}

impl RenderDecoratedBox {
    /// Creates a decorated box painting `decoration` behind the child.
    pub fn new(decoration: BoxDecoration<Pixels>) -> Self {
        Self {
            decoration,
            position: DecorationPosition::Background,
            has_child: false,
        }
    }

    /// Sets where the decoration paints relative to the child.
    #[must_use]
    pub fn with_position(mut self, position: DecorationPosition) -> Self {
        self.position = position;
        self
    }

    /// The current decoration.
    pub fn decoration(&self) -> &BoxDecoration<Pixels> {
        &self.decoration
    }

    /// Replaces the decoration. Paint-only state: the caller is
    /// responsible for the repaint mark.
    pub fn set_decoration(&mut self, decoration: BoxDecoration<Pixels>) {
        self.decoration = decoration;
    }

    /// The decoration's position relative to the child.
    pub fn position(&self) -> DecorationPosition {
        self.position
    }

    /// Sets the decoration's position. Paint-only state: the caller is
    /// responsible for the repaint mark.
    pub fn set_position(&mut self, position: DecorationPosition) {
        self.position = position;
    }

    fn paint_rect(&self, size: Size) -> Rect {
        Rect::from_origin_size(Point::ZERO, size)
    }
}

impl flui_foundation::Diagnosticable for RenderDecoratedBox {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add_enum("decoration", self.decoration.clone());
        properties.add_default_enum("position", self.position, DecorationPosition::Background);
    }
}

impl RenderBox for RenderDecoratedBox {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, Self::ParentData>) -> Size {
        let constraints = *ctx.constraints();
        if ctx.child_count() > 0 {
            self.has_child = true;
            let child_size = ctx.layout_child(0, constraints);
            ctx.position_child(0, Offset::ZERO);
            child_size
        } else {
            self.has_child = false;
            constraints.smallest()
        }
    }

    crate::forward_single_child_box_queries!();

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        let rect = self.paint_rect(ctx.size());
        if self.position == DecorationPosition::Background {
            paint_box_decoration(ctx.canvas(), rect, &self.decoration);
        }
        if self.has_child {
            ctx.paint_child();
        }
        if self.position == DecorationPosition::Foreground {
            paint_box_decoration(ctx.canvas(), rect, &self.decoration);
        }
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, Self::ParentData>) -> bool {
        // The decoration's geometry decides (rounded corners exclude
        // the bounding rect's corners — Flutter `BoxDecoration.hitTest`
        // via `RenderDecoratedBox.hitTestSelf`).
        let position = *ctx.position();
        if !box_decoration_hit_test(self.paint_rect(ctx.own_size()), &self.decoration, position) {
            return false;
        }
        if self.has_child && ctx.hit_test_child_at_offset(0, Offset::ZERO) {
            return true;
        }
        // The decorated area itself is hit-opaque (a Container with a
        // color absorbs taps).
        true
    }
}

// Capability opt-outs: the decoration is plain canvas content, no
// layer-level effects.
impl PaintEffectsCapability for RenderDecoratedBox {}
impl SemanticsCapability for RenderDecoratedBox {}
impl HotReloadCapability for RenderDecoratedBox {}

#[cfg(test)]
mod tests {
    use flui_types::styling::Color;

    use super::*;

    #[test]
    fn defaults_to_background_position() {
        let node = RenderDecoratedBox::new(BoxDecoration::with_color(Color::RED));
        assert_eq!(node.position(), DecorationPosition::Background);
    }

    #[test]
    fn builder_sets_foreground() {
        let node = RenderDecoratedBox::new(BoxDecoration::with_color(Color::RED))
            .with_position(DecorationPosition::Foreground);
        assert_eq!(node.position(), DecorationPosition::Foreground);
    }
}
