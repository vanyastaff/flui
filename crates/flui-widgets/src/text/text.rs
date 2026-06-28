//! [`Text`] — displays a run of styled text.

use flui_objects::RenderParagraph;
use flui_rendering::protocol::BoxProtocol;
use flui_types::typography::{TextAlign, TextDirection, TextSpan, TextStyle};
use flui_view::{RenderView, impl_render_view};

/// Displays a string of text with a single style.
///
/// Flutter parity: `widgets/text.dart` `Text` over `RenderParagraph`. This is a
/// leaf widget — it measures and paints the text but has no child. For
/// multi-style runs, build a [`TextSpan`] tree directly (a richer `RichText`
/// widget lands with the inline-span catalog).
///
/// # Examples
///
/// ```rust
/// # use flui_widgets::prelude::*;
/// let _ = Text::new("Hello, world");
/// ```
#[derive(Clone, Debug)]
pub struct Text {
    data: String,
    style: Option<TextStyle>,
    align: TextAlign,
    direction: TextDirection,
    max_lines: Option<u32>,
}

impl Text {
    /// Create text displaying `data` with default style, start alignment, and
    /// left-to-right direction.
    pub fn new(data: impl Into<String>) -> Self {
        Self {
            data: data.into(),
            style: None,
            align: TextAlign::Start,
            direction: TextDirection::Ltr,
            max_lines: None,
        }
    }

    /// Apply a [`TextStyle`] to the whole run.
    #[must_use]
    pub fn style(mut self, style: TextStyle) -> Self {
        self.style = Some(style);
        self
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

    fn span(&self) -> TextSpan {
        match &self.style {
            Some(style) => TextSpan::styled(self.data.clone(), style.clone()),
            None => TextSpan::new(self.data.clone()),
        }
    }

    fn build_render_object(&self) -> RenderParagraph {
        RenderParagraph::new(self.span(), self.direction)
            .with_text_align(self.align)
            .with_max_lines(self.max_lines)
    }
}

impl RenderView for Text {
    type Protocol = BoxProtocol;
    type RenderObject = RenderParagraph;

    fn create_render_object(&self) -> Self::RenderObject {
        self.build_render_object()
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        *render_object = self.build_render_object();
    }
}

impl_render_view!(Text);
