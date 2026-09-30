//! [`RichText`] — displays a tree of styled inline spans in one paragraph.

use flui_objects::RenderParagraph;
use flui_painting::typography::{InlineSpan, TextAlign, TextDirection};
use flui_rendering::protocol::BoxProtocol;
use flui_view::{RenderView, impl_render_view};

/// Displays a tree of styled [`InlineSpan`]s (most commonly a
/// [`TextSpan`](flui_painting::typography::TextSpan)) in a single paragraph.
///
/// Backed by `RenderParagraph`, the same render object [`Text`](crate::Text)
/// uses. Unlike `Text`, which
/// applies one style to a flat string, `RichText` accepts a span tree where
/// each node carries its own style, letting a sentence mix e.g. bold and
/// colored words without splitting it across multiple widgets.
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

    fn build_render_object(&self) -> RenderParagraph {
        RenderParagraph::new(self.text.clone(), self.direction)
            .with_text_align(self.align)
            .with_max_lines(self.max_lines)
    }
}

impl RenderView for RichText {
    type Protocol = BoxProtocol;
    type RenderObject = RenderParagraph;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        self.build_render_object()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_text(self.text.clone())
            | render_object.set_text_align(self.align)
            | render_object.set_text_direction(self.direction)
            | render_object.set_max_lines(self.max_lines)
    }
}

impl_render_view!(RichText);
