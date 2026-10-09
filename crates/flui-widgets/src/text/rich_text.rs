//! [`RichText`] — displays a tree of styled inline spans in one paragraph.

use flui_objects::RenderParagraph;
use flui_painting::typography::{InlineSpan, TextAlign, TextDirection};
use flui_rendering::protocol::BoxProtocol;
use flui_view::element::ElementKind;
use flui_view::{BuildContext, IntoView, RenderView, StatelessView, View, impl_render_view};

use crate::MediaQuery;

/// Displays a tree of styled [`InlineSpan`]s (most commonly a
/// [`TextSpan`](flui_painting::typography::TextSpan)) in a single paragraph.
///
/// Backed by `RenderParagraph`, the same render object [`Text`](crate::Text)
/// uses. Unlike `Text`, which
/// applies one style to a flat string, `RichText` accepts a span tree where
/// each node carries its own style, letting a sentence mix e.g. bold and
/// colored words without splitting it across multiple widgets.
/// Text sizing follows the nearest [`MediaQuery`]; changes invalidate paragraph
/// layout without modifying the authored span styles.
///
/// # Examples
///
/// ```rust
/// # use flui_widgets::prelude::*;
/// # use flui_painting::typography::{FontWeight, TextSpan, TextStyle};
/// let _ = RichText::new(
///     TextSpan::new("Hello, ").with_child(TextSpan::new("world").with_style(TextStyle {
///         font_weight: Some(FontWeight::BOLD),
///         ..Default::default()
///     })),
/// );
/// ```
#[derive(Clone, Debug)]
pub struct RichText {
    text: InlineSpan,
    align: TextAlign,
    direction: TextDirection,
    max_lines: Option<u32>,
    scaling: TextScaling,
}

#[derive(Clone, Copy, Debug)]
enum TextScaling {
    Inherited,
    Fixed,
}

impl RichText {
    /// Display `text` (any type convertible into an [`InlineSpan`] — most
    /// commonly a [`TextSpan`](flui_painting::typography::TextSpan)) with start
    /// alignment and left-to-right direction.
    pub fn new(text: impl Into<InlineSpan>) -> Self {
        Self {
            text: text.into(),
            align: TextAlign::Start,
            direction: TextDirection::Ltr,
            max_lines: None,
            scaling: TextScaling::Inherited,
        }
    }

    /// Set the horizontal alignment of the text within its bounds.
    #[must_use]
    pub fn align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }

    /// Set the reading direction (default left-to-right).
    #[must_use]
    pub fn direction(mut self, direction: TextDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Cap the number of lines before truncating.
    #[must_use]
    pub fn max_lines(mut self, max_lines: u32) -> Self {
        self.max_lines = Some(max_lines);
        self
    }

    /// The composing widget has already resolved its logical glyph size.
    /// Weight preferences still apply; sizing must not be inherited a second time.
    pub(crate) fn unscaled(mut self) -> Self {
        self.scaling = TextScaling::Fixed;
        self
    }
}

impl View for RichText {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

impl StatelessView for RichText {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        ResolvedParagraph {
            authored: self.clone(),
            text_scale_factor: match self.scaling {
                TextScaling::Inherited => MediaQuery::text_scale_factor_of(ctx).unwrap_or(1.0),
                TextScaling::Fixed => 1.0,
            },
            font_weight_adjustment: MediaQuery::font_weight_adjustment_of(ctx).unwrap_or(0),
        }
    }
}

/// The inherited dependency belongs to the widget build; the render object
/// receives values and keeps the authored span tree unchanged.
#[derive(Clone, Debug)]
struct ResolvedParagraph {
    authored: RichText,
    text_scale_factor: f64,
    font_weight_adjustment: i32,
}

impl RenderView for ResolvedParagraph {
    type Protocol = BoxProtocol;
    type RenderObject = RenderParagraph;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderParagraph::new(self.authored.text.clone(), self.authored.direction)
            .with_text_align(self.authored.align)
            .with_max_lines(self.authored.max_lines)
            .with_text_scale_factor(self.text_scale_factor)
            .with_font_weight_adjustment(self.font_weight_adjustment)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_text(self.authored.text.clone())
            | render_object.set_text_align(self.authored.align)
            | render_object.set_text_direction(self.authored.direction)
            | render_object.set_max_lines(self.authored.max_lines)
            | render_object.set_text_scale_factor(self.text_scale_factor)
            | render_object.set_font_weight_adjustment(self.font_weight_adjustment)
    }
}

impl_render_view!(ResolvedParagraph);
