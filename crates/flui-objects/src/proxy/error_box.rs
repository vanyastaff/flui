//! `RenderErrorBox` — the render object behind `ErrorView`.
//!
//! A filled box that stands in for a subtree whose build panicked. In a debug
//! build it is dark red and paints the caught message; in release it is a
//! neutral grey box and the message reaches diagnostics only. The debug text is
//! yellow monospace; in release the message is withheld.
//!
//! # Sizing
//!
//! Asking for a huge size and letting the constraints clamp it would fill any
//! bounded parent and explode an unbounded one. Instead a bounded axis is
//! filled and an unbounded axis falls back to a fixed extent on an
//! unbounded axis ([`ERROR_BOX_FALLBACK_EXTENT`]): a panicking item inside a
//! lazy list — whose main axis is unbounded — must occupy a visible, finite
//! row, not the whole scroll extent.

use std::sync::Arc;

use flui_foundation::Diagnosticable;
use flui_foundation::Leaf;
use flui_foundation::geometry::{Offset, Point, Rect, Size};
use flui_painting::parley_text::ParagraphSpec;
use flui_painting::styling::Color;
use flui_painting::typography::{TextDirection, TextStyle};
use flui_painting::{Paint, ShapedParagraph};

use flui_rendering::{
    constraints::BoxConstraints, context::BoxLayoutContext, parent_data::BoxParentData,
    traits::RenderBox,
};

/// The extent the box takes on an axis its constraints leave unbounded.
pub const ERROR_BOX_FALLBACK_EXTENT: f64 = 48.0;

/// Debug background.
const DEBUG_BACKGROUND: Color = Color::from_argb(0xF090_0000);
/// Release background.
const RELEASE_BACKGROUND: Color = Color::from_argb(0xF0C0_C0C0);
/// Debug text colour.
const DEBUG_TEXT: Color = Color::from_argb(0xFFFF_FF66);
const DEBUG_FONT_SIZE: f64 = 14.0;

/// A filled box standing in for a subtree whose build panicked.
#[derive(Debug, Clone)]
pub struct RenderErrorBox {
    message: String,
    details: Option<String>,
    /// The message shaped at the committed width, in debug builds; `None`
    /// before the first layout and in release.
    painted: Option<Arc<ShapedParagraph>>,
}

impl RenderErrorBox {
    /// A box for `message`, with optional `details` (a stack trace, a cause).
    #[must_use]
    pub fn new(message: impl Into<String>, details: Option<String>) -> Self {
        Self {
            message: message.into(),
            details,
            painted: None,
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

    /// Replace the message and details. A new message is shaped again at the
    /// next layout, in debug builds, where it is painted; the size does not
    /// depend on it. New details only reach diagnostics.
    pub fn set_error(
        &mut self,
        message: impl Into<String>,
        details: Option<String>,
    ) -> flui_rendering::RenderUpdateImpact {
        let message = message.into();
        if self.message == message && self.details == details {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        let impact = if self.message != message && cfg!(debug_assertions) {
            flui_rendering::RenderUpdateImpact::LAYOUT
        } else {
            flui_rendering::RenderUpdateImpact::PAINT
        };
        self.message = message;
        self.details = details;
        impact
    }

    fn size_for(constraints: &BoxConstraints) -> Size {
        let axis = |max: f64| {
            if max.is_finite() {
                max
            } else {
                ERROR_BOX_FALLBACK_EXTENT
            }
        };
        constraints.constrain(Size::new(
            axis(constraints.max_width),
            axis(constraints.max_height),
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
        let size = Self::size_for(ctx.constraints());
        // The message is developer-facing and may name private state; it is
        // shaped and painted in debug builds only.
        self.painted = cfg!(debug_assertions).then(|| {
            let style = TextStyle::new()
                .with_color(DEBUG_TEXT)
                .with_font_size(DEBUG_FONT_SIZE)
                .with_font_family("monospace");
            let spans = [(self.message.clone(), Some(style.clone()))];
            #[expect(
                clippy::cast_possible_truncation,
                reason = "logical sizes narrow to the shaper's f32 layout space"
            )]
            let paragraph = ctx.text().shape(&ParagraphSpec {
                spans: &spans,
                default_style: Some(&style),
                font_size: DEBUG_FONT_SIZE as f32,
                max_width: Some(size.width as f32),
                line_height: None,
                direction: TextDirection::Ltr,
                max_lines: None,
                ellipsis: None,
            });
            Arc::new(paragraph.to_shaped(Some(DEBUG_TEXT)))
        });
        size
    }

    fn compute_min_intrinsic_width(
        &self,
        _height: f64,
        _ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f64 {
        0.0
    }

    fn compute_max_intrinsic_width(
        &self,
        _height: f64,
        _ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f64 {
        ERROR_BOX_FALLBACK_EXTENT
    }

    fn compute_min_intrinsic_height(
        &self,
        _width: f64,
        _ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f64 {
        0.0
    }

    fn compute_max_intrinsic_height(
        &self,
        _width: f64,
        _ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f64 {
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
        if let Some(paragraph) = &self.painted {
            ctx.canvas()
                .draw_paragraph(paragraph, Offset::ZERO, DEBUG_TEXT);
        }
    }
}
