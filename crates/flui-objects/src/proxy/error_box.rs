//! `RenderErrorBox` — the render object behind `ErrorView`.
//!
//! A filled box that stands in for a subtree whose build panicked. In a debug
//! build it is dark red and paints the caught message; in release it is a
//! neutral grey box and the message reaches diagnostics only. Flutter parity:
//! `RenderErrorBox` (`rendering/error.dart`) — same two background colours,
//! same yellow monospace text in debug, message withheld in release.
//!
//! # Sizing
//!
//! Flutter's box asks for `100000 × 100000` and lets the constraints clamp
//! it, which fills any bounded parent and explodes an unbounded one. FLUI
//! fills a bounded axis the same way and falls back to a fixed extent on an
//! unbounded axis ([`ERROR_BOX_FALLBACK_EXTENT`]): a panicking item inside a
//! lazy list — whose main axis is unbounded — must occupy a visible, finite
//! row, not the whole scroll extent.

use flui_foundation::Diagnosticable;
use flui_painting::Paint;
use flui_tree::Leaf;
use flui_types::typography::TextStyle;
use flui_types::{Color, Offset, Point, Rect, Size, geometry::px};

use flui_rendering::{
    constraints::BoxConstraints, context::BoxLayoutContext, parent_data::BoxParentData,
    traits::RenderBox,
};

/// The extent the box takes on an axis its constraints leave unbounded.
pub const ERROR_BOX_FALLBACK_EXTENT: f32 = 48.0;

/// Debug background — Flutter's `RenderErrorBox.backgroundColor` in debug.
const DEBUG_BACKGROUND: Color = Color::from_argb(0xF090_0000);
/// Release background — Flutter's `RenderErrorBox.backgroundColor` in release.
const RELEASE_BACKGROUND: Color = Color::from_argb(0xF0C0_C0C0);
/// Debug text colour — Flutter's `RenderErrorBox.textStyle`.
const DEBUG_TEXT: Color = Color::from_argb(0xFFFF_FF66);
const DEBUG_FONT_SIZE: f64 = 14.0;

/// A filled box standing in for a subtree whose build panicked.
#[derive(Debug, Clone)]
pub struct RenderErrorBox {
    message: String,
    details: Option<String>,
}

impl RenderErrorBox {
    /// A box for `message`, with optional `details` (a stack trace, a cause).
    #[must_use]
    pub fn new(message: impl Into<String>, details: Option<String>) -> Self {
        Self {
            message: message.into(),
            details,
        }
    }

    /// The caught error's message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The caught error's details, if any.
    #[must_use]
    pub fn details(&self) -> Option<&str> {
        self.details.as_deref()
    }

    /// Replace the message and details; a change repaints (the size does not
    /// depend on the text).
    pub fn set_error(
        &mut self,
        message: impl Into<String>,
        details: Option<String>,
    ) -> flui_rendering::RenderUpdateImpact {
        let message = message.into();
        if self.message == message && self.details == details {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.message = message;
        self.details = details;
        flui_rendering::RenderUpdateImpact::PAINT
    }

    fn size_for(constraints: &BoxConstraints) -> Size {
        let axis = |max: f32| {
            if max.is_finite() {
                max
            } else {
                ERROR_BOX_FALLBACK_EXTENT
            }
        };
        constraints.constrain(Size::new(
            px(axis(constraints.max_width.get())),
            px(axis(constraints.max_height.get())),
        ))
    }
}

impl Diagnosticable for RenderErrorBox {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add("message", &self.message);
        properties.add_optional("details", self.details.as_deref());
    }
}

impl RenderBox for RenderErrorBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
        Self::size_for(ctx.constraints())
    }

    fn compute_min_intrinsic_width(
        &self,
        _height: f32,
        _ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        0.0
    }

    fn compute_max_intrinsic_width(
        &self,
        _height: f32,
        _ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        ERROR_BOX_FALLBACK_EXTENT
    }

    fn compute_min_intrinsic_height(
        &self,
        _width: f32,
        _ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        0.0
    }

    fn compute_max_intrinsic_height(
        &self,
        _width: f32,
        _ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        ERROR_BOX_FALLBACK_EXTENT
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        _ctx: &mut flui_rendering::context::BoxDryLayoutCtx<'_>,
    ) -> Size {
        Self::size_for(&constraints)
    }

    fn paint(&self, ctx: &mut flui_rendering::context::PaintCx<'_, Leaf>) {
        let size = ctx.size();
        let rect = Rect::from_origin_size(Point::ZERO, size);
        let background = if cfg!(debug_assertions) {
            DEBUG_BACKGROUND
        } else {
            RELEASE_BACKGROUND
        };
        ctx.canvas().draw_rect(rect, &Paint::fill(background));
        if cfg!(debug_assertions) {
            // The message is developer-facing and may name private state; it
            // is painted in debug builds only, exactly as Flutter withholds
            // the text from release `ErrorWidget`s.
            let style = TextStyle::new()
                .with_color(DEBUG_TEXT)
                .with_font_size(DEBUG_FONT_SIZE)
                .with_font_family("monospace");
            ctx.canvas().draw_text(
                &self.message,
                Offset::ZERO,
                size,
                &style,
                &Paint::fill(DEBUG_TEXT),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tight(w: f32, h: f32) -> BoxConstraints {
        BoxConstraints::tight(Size::new(px(w), px(h)))
    }

    #[test]
    fn fills_a_bounded_axis_and_falls_back_on_an_unbounded_one() {
        let bounded = tight(120.0, 30.0);
        assert_eq!(
            RenderErrorBox::size_for(&bounded),
            Size::new(px(120.0), px(30.0))
        );
        let unbounded_height = BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(f32::INFINITY));
        assert_eq!(
            RenderErrorBox::size_for(&unbounded_height),
            Size::new(px(200.0), px(ERROR_BOX_FALLBACK_EXTENT))
        );
    }

    #[test]
    fn set_error_reports_paint_only_on_change() {
        let mut b = RenderErrorBox::new("boom", None);
        assert_eq!(
            b.set_error("boom", None),
            flui_rendering::RenderUpdateImpact::NONE
        );
        assert_eq!(
            b.set_error("bang", Some("trace".into())),
            flui_rendering::RenderUpdateImpact::PAINT
        );
        assert_eq!(b.message(), "bang");
        assert_eq!(b.details(), Some("trace"));
    }
}
