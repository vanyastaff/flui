//! [`RichText`] — displays a tree of styled inline spans in one paragraph.

use flui_objects::RenderParagraph;
use flui_rendering::protocol::BoxProtocol;
use flui_types::typography::{InlineSpan, TextAlign, TextDirection};
use flui_view::{RenderView, impl_render_view};

/// Displays a tree of styled [`InlineSpan`]s (most commonly a
/// [`TextSpan`](flui_types::typography::TextSpan)) in a single paragraph.
///
/// Flutter parity: `widgets/basic.dart` `RichText` over `RenderParagraph` —
/// the same render object [`Text`](crate::Text) uses. Unlike `Text`, which
/// applies one style to a flat string, `RichText` accepts a span tree where
/// each node carries its own style, letting a sentence mix e.g. bold and
/// colored words without splitting it across multiple widgets.
///
/// # Examples
///
/// ```rust
/// # use flui_widgets::prelude::*;
/// # use flui_types::typography::{FontWeight, TextSpan, TextStyle};
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
    /// commonly a [`TextSpan`](flui_types::typography::TextSpan)) with start
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

    fn create_render_object(&self) -> Self::RenderObject {
        self.build_render_object()
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        *render_object = self.build_render_object();
    }
}

impl_render_view!(RichText);

#[cfg(test)]
mod tests {
    use flui_types::typography::{FontWeight, TextSpan, TextStyle};
    use flui_view::RenderView;

    use super::*;

    fn two_word_span() -> TextSpan {
        TextSpan::new("Hello, ").with_child(TextSpan::styled(
            "world",
            TextStyle {
                font_weight: Some(FontWeight::BOLD),
                ..Default::default()
            },
        ))
    }

    #[test]
    fn new_defaults_to_start_alignment_ltr_no_max_lines() {
        let rich_text = RichText::new(two_word_span());
        assert_eq!(rich_text.align, TextAlign::Start);
        assert_eq!(rich_text.direction, TextDirection::Ltr);
        assert_eq!(rich_text.max_lines, None);
    }

    #[test]
    fn builder_methods_override_align_direction_and_max_lines() {
        let rich_text = RichText::new(two_word_span())
            .align(TextAlign::Center)
            .direction(TextDirection::Rtl)
            .max_lines(2);
        assert_eq!(rich_text.align, TextAlign::Center);
        assert_eq!(rich_text.direction, TextDirection::Rtl);
        assert_eq!(rich_text.max_lines, Some(2));
    }

    fn plain_text(render_object: &RenderParagraph) -> String {
        render_object
            .painter()
            .text()
            .expect("create_render_object/update_render_object must always install a text span")
            .to_plain_text()
    }

    #[test]
    fn create_render_object_preserves_the_full_span_tree() {
        let render_object = RichText::new(two_word_span()).create_render_object();
        // `RenderParagraph` exposes its text only through `painter().text()`;
        // the plain-text projection is the public surface available to prove
        // the whole tree (both the parent span's own text and the bolded
        // child's) reached the render object, not just the top-level string.
        assert_eq!(plain_text(&render_object), "Hello, world");
    }

    #[test]
    fn update_render_object_replaces_the_span_tree() {
        let mut render_object = RichText::new(TextSpan::new("first")).create_render_object();
        assert_eq!(plain_text(&render_object), "first");

        RichText::new(TextSpan::new("second")).update_render_object(&mut render_object);

        assert_eq!(plain_text(&render_object), "second");
    }

    #[test]
    fn has_children_is_always_false() {
        assert!(!RichText::new(two_word_span()).has_children());
    }
}
