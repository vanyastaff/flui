//! [`Text`] — displays a run of styled text.

use flui_types::typography::{TextAlign, TextDirection, TextSpan, TextStyle};
use flui_view::element::ElementKind;
use flui_view::prelude::*;

use super::default_text_style::DefaultTextStyle;
use super::rich_text::RichText;

/// Displays a string of text with a single style.
///
/// Flutter parity: `widgets/text.dart` `Text` — a `StatelessWidget` that merges
/// the ambient [`DefaultTextStyle`] with its own and builds a
/// [`RichText`] (`text.dart:716-765`), as Flutter's does. For
/// multi-style runs, use `RichText` with a [`TextSpan`] tree directly — it reads
/// no ambient style, also as in Flutter.
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
    align: Option<TextAlign>,
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
            align: None,
            direction: TextDirection::Ltr,
            max_lines: None,
        }
    }

    /// Apply a [`TextStyle`] to the whole run. Merged **over** the ambient
    /// [`DefaultTextStyle`], field by field (`text.dart:718-720`).
    #[must_use]
    pub fn style(mut self, style: TextStyle) -> Self {
        self.style = Some(style);
        self
    }

    /// Set the horizontal alignment of the text within its bounds. Unset, the
    /// ambient [`DefaultTextStyle`]'s alignment applies, then start (`:757`).
    #[must_use]
    pub fn align(mut self, align: TextAlign) -> Self {
        self.align = Some(align);
        self
    }

    /// Set the reading direction (default left-to-right).
    #[must_use]
    pub fn direction(mut self, direction: TextDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Cap the number of lines before truncating. Unset, the ambient
    /// [`DefaultTextStyle`]'s cap applies (`:765`).
    #[must_use]
    pub fn max_lines(mut self, max_lines: u32) -> Self {
        self.max_lines = Some(max_lines);
        self
    }
}

impl View for Text {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

impl StatelessView for Text {
    /// `Text.build` (`text.dart:716-765`), reduced to the properties FLUI's text
    /// stack carries: the ambient style merges under this run's own, and
    /// alignment / line cap fall back to the ambient values when unset here.
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        // No scope above behaves as an empty style with no alignment or cap —
        // `DefaultTextStyle.fallback` (`text.dart:81-88`).
        let (ambient_style, ambient_align, ambient_max_lines) = ctx
            .depend_on::<DefaultTextStyle, _>(DefaultTextStyle::ambient)
            .unwrap_or_default();

        // `defaultTextStyle.style.merge(style)` (`:720`); FLUI's `TextStyle` has
        // no `inherit` flag, so the merge is unconditional (all fields optional).
        let effective_style = match &self.style {
            Some(own) => ambient_style.merge(own),
            None => ambient_style,
        };
        // An all-unset style is byte-for-byte the unstyled span.
        let span = if effective_style == TextStyle::default() {
            TextSpan::new(self.data.clone())
        } else {
            TextSpan::styled(self.data.clone(), effective_style)
        };

        let align = self
            .align
            .or(ambient_align)
            // `textAlign ?? defaultTextStyle.textAlign ?? TextAlign.start` (`:757`).
            .unwrap_or(TextAlign::Start);

        let mut rich = RichText::new(span).align(align).direction(self.direction);
        // `maxLines ?? defaultTextStyle.maxLines` (`:765`).
        if let Some(max_lines) = self.max_lines.or(ambient_max_lines) {
            rich = rich.max_lines(max_lines);
        }
        rich
    }
}
