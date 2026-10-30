//! The touch handles of a [`SelectionArea`](super::SelectionArea): what they look like.
//!
//! The area places each handle, follows its end through scrolls and drags it; a
//! [`SelectionHandles`] only draws the two glyphs.

use std::any::Any;
use std::fmt;

use flui_foundation::geometry::{Point, Size};
use flui_painting::styling::Color;
use flui_painting::{Canvas, Paint};
use flui_rendering::delegates::CustomPainter;
use flui_view::BoxedView;
use flui_view::prelude::*;

use crate::CustomPaint;

/// Which end of the selection a handle moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HandleSide {
    /// The end earlier in reading order.
    Start,
    /// The end later in reading order.
    End,
}

/// Draws the two touch handles of a [`SelectionArea`](super::SelectionArea).
///
/// A handle's box hangs below its end's line: the top centre of the box sits on the bottom of
/// the line at the end, so the glyph points up at the text from there. The whole box takes the
/// drag, so a larger box is an easier target.
pub trait SelectionHandles {
    /// The size of each handle's box in logical pixels. A size that is not finite and positive
    /// is replaced by the default handles' size, with a warning.
    fn size(&self) -> Size;

    /// The glyph of the `side` handle, filling its box.
    fn build(&self, side: HandleSide, ctx: &dyn BuildContext) -> BoxedView;
}

/// The handles a [`SelectionArea`](super::SelectionArea) draws when it is given none: a dot on a
/// short stem, in the selection's colour made opaque. Theme-free; a design system passes its own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DotHandles {
    color: Color,
}

impl DotHandles {
    /// The box of each handle.
    pub const SIZE: Size = Size::new(22.0, 28.0);

    /// Handles drawn in `color`.
    #[must_use]
    pub fn new(color: Color) -> Self {
        Self { color }
    }
}

impl SelectionHandles for DotHandles {
    fn size(&self) -> Size {
        Self::SIZE
    }

    fn build(&self, _side: HandleSide, _ctx: &dyn BuildContext) -> BoxedView {
        CustomPaint::new()
            .painter(Dot { color: self.color })
            .size(Self::SIZE)
            .boxed()
    }
}

#[derive(Debug)]
struct Dot {
    color: Color,
}

impl CustomPainter for Dot {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        if !(size.width.is_finite() && size.height.is_finite())
            || size.width <= 0.0
            || size.height <= 0.0
        {
            return;
        }
        let radius = (size.width / 2.0).min(size.height / 3.0);
        let middle = size.width / 2.0;
        let stem = (size.height - 2.0 * radius).max(0.0);
        let paint = Paint::fill(self.color);
        canvas.draw_rect(
            flui_foundation::geometry::Rect::from_ltwh(middle - 1.0, 0.0, 2.0, stem + radius),
            &paint,
        );
        canvas.draw_circle(Point::new(middle, stem + radius), radius, &paint);
    }

    fn should_repaint(&self, old: &dyn CustomPainter) -> bool {
        old.as_any()
            .downcast_ref::<Self>()
            .is_none_or(|old| old.color != self.color)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A shared [`SelectionHandles`], so the area's view stays `Clone`.
#[derive(Clone)]
pub(super) struct SharedHandles(pub(super) std::rc::Rc<dyn SelectionHandles>);

impl fmt::Debug for SharedHandles {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SharedHandles")
            .field("size", &self.0.size())
            .finish()
    }
}

impl SharedHandles {
    /// The handles' size, or the default's when theirs is not finite and positive.
    pub(super) fn checked_size(&self) -> Size {
        let size = self.0.size();
        if size.width.is_finite()
            && size.height.is_finite()
            && size.width > 0.0
            && size.height > 0.0
        {
            size
        } else {
            tracing::warn!(
                ?size,
                "selection handles reported a size that is not finite and positive; using the default"
            );
            DotHandles::SIZE
        }
    }
}
