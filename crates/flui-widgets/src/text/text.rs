//! [`Text`] — displays a run of text in the style around it.

use std::sync::Arc;

use flui_foundation::{CreationSite, TextDirection};
use flui_painting::typography::{InlineSpan, TextAlign, TextLink, TextSpan, TextStyle};
use flui_view::element::ElementKind;
use flui_view::environment::MediaQuery;
use flui_view::prelude::*;
use flui_view::{EventContext, EventOutcome};

use super::default_text_style::DefaultTextStyle;
use super::rich_text::RichText;
use super::selection_area::{SelectableParagraph, SelectionScope};
use crate::support::{RefCallback, ref_callback};

/// Displays a string, or a tree of styled spans, in the style around it.
///
/// The widget an application writes for text, as Flutter's `Text` is
/// (`flutter/flutter@3.50.1`, `packages/flutter/lib/src/widgets/text.dart:723`).
/// It reads what is around it and builds a [`RichText`] from it:
///
/// - the ambient [`DefaultTextStyle`], merged under its own style, and that
///   style's alignment and line cap where it sets none;
/// - the ambient [`MediaQuery`]'s text sizing and its bold-text weight
///   adjustment (ADR-0174), as Flutter's reads `MediaQuery.textScalerOf` and
///   `boldTextOf` (`text.dart:757` and `:729`);
/// - a [`SelectionArea`](crate::SelectionArea) above it, which it joins
///   (`text.dart:752`).
///
/// [`Text::rich`] displays a span tree, each span with its own style, under
/// the ambient one.
///
/// # Links
///
/// A span made a link with
/// [`TextSpan::with_link`](flui_painting::typography::TextSpan::with_link)
/// does something only through [`on_link`](Self::on_link): a tap on the
/// link's glyphs, or assistive technology activating the link, calls it with
/// the span's [`TextLink`]. While it is set, the pointer over a link is a
/// pointing hand, each link is an accessible link node, and each link is a
/// focus stop: Tab reaches it in reading order, Enter or Select opens it
/// ([`ActivateLinkIntent`](crate::ActivateLinkIntent)), and a
/// [`FocusRing`](crate::FocusRing)-styled ring outlines each line of its
/// glyphs while the focus is visible. Without it, links are plain text.
///
/// # Examples
///
/// ```rust
/// # use flui_widgets::prelude::*;
/// let _ = Text::new("Hello, world");
/// ```
///
/// A link:
///
/// ```rust
/// # use flui_widgets::prelude::*;
/// # use flui_painting::typography::TextSpan;
/// let _ = Text::rich(
///     TextSpan::new("Read the ").with_child(TextSpan::new("terms").with_link("app://terms")),
/// )
/// .on_link(|_ctx, link| println!("open {link}"));
/// ```
#[derive(Clone)]
pub struct Text {
    content: Content,
    style: Option<TextStyle>,
    align: Option<TextAlign>,
    direction: Option<TextDirection>,
    max_lines: Option<u32>,
    on_link: Option<RefCallback<TextLink>>,
    site: CreationSite,
}

/// What a [`Text`] displays.
#[derive(Clone, Debug)]
enum Content {
    /// A string, in the run's style.
    Plain(String),
    /// A span tree, beneath the run's style.
    Rich(Arc<TextSpan>),
}

impl std::fmt::Debug for Text {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Text")
            .field("content", &self.content)
            .field("style", &self.style)
            .field("align", &self.align)
            .field("direction", &self.direction)
            .field("max_lines", &self.max_lines)
            .field("on_link", &self.on_link.is_some())
            .finish_non_exhaustive()
    }
}

impl Text {
    /// Display `data` in the ambient style, with its alignment, in the
    /// ambient [`Directionality`](crate::Directionality)'s reading direction.
    #[track_caller]
    pub fn new(data: impl Into<String>) -> Self {
        Self::with_content(Content::Plain(data.into()))
    }

    /// Display the span tree `text` beneath the ambient style, each span
    /// merging its own style over it, as Flutter's `Text.rich` does
    /// (`text.dart:545`).
    #[track_caller]
    pub fn rich(text: TextSpan) -> Self {
        Self::with_content(Content::Rich(Arc::new(text)))
    }

    #[track_caller]
    fn with_content(content: Content) -> Self {
        Self {
            content,
            style: None,
            align: None,
            direction: None,
            max_lines: None,
            on_link: None,
            site: CreationSite::here(),
        }
    }

    /// Apply a [`TextStyle`] to the whole run. Merged **over** the ambient
    /// [`DefaultTextStyle`], field by field.
    #[must_use]
    pub fn style(mut self, style: TextStyle) -> Self {
        self.style = Some(style);
        self
    }

    /// Set the horizontal alignment of the text within its bounds. Unset, the
    /// ambient [`DefaultTextStyle`]'s alignment applies, then start.
    #[must_use]
    pub fn align(mut self, align: TextAlign) -> Self {
        self.align = Some(align);
        self
    }

    /// Set the reading direction. Unset, the ambient
    /// [`Directionality`](crate::Directionality) applies, then left-to-right.
    #[must_use]
    pub fn direction(mut self, direction: TextDirection) -> Self {
        self.direction = Some(direction);
        self
    }

    /// Cap the number of lines before truncating. Unset, the ambient
    /// [`DefaultTextStyle`]'s cap applies.
    #[must_use]
    pub fn max_lines(mut self, max_lines: u32) -> Self {
        self.max_lines = Some(max_lines);
        self
    }

    /// Call `handler` with a link's [`TextLink`] when a tap or assistive
    /// technology activates that link. It receives the activation's
    /// [`EventContext`], so it writes signals as any event callback does.
    #[must_use]
    pub fn on_link<F, R>(mut self, handler: F) -> Self
    where
        F: Fn(&mut EventContext<'_>, &TextLink) -> R + 'static,
        R: EventOutcome,
    {
        self.on_link = Some(ref_callback(handler));
        self
    }

    /// The span the paragraph displays: the content under `style`. An
    /// all-unset style leaves it as authored.
    fn span(&self, style: TextStyle) -> InlineSpan {
        let unstyled = style == TextStyle::default();
        match &self.content {
            Content::Plain(data) if unstyled => TextSpan::new(data.clone()).into(),
            Content::Plain(data) => TextSpan::styled(data.clone(), style).into(),
            Content::Rich(span) if unstyled => InlineSpan::Text(Arc::clone(span)),
            Content::Rich(span) => TextSpan::styled("", style)
                .with_child(TextSpan::clone(span))
                .into(),
        }
    }
}

impl View for Text {
    fn create_element(self: std::rc::Rc<Self>) -> ElementKind {
        ElementKind::stateless(self)
    }

    fn creation_site(&self) -> CreationSite {
        self.site
    }
}

impl StatelessView for Text {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        // No scope above behaves as an empty style with no alignment or cap.
        let (ambient_style, ambient_align, ambient_max_lines) = ctx
            .depend_on::<DefaultTextStyle, _>(DefaultTextStyle::ambient)
            .unwrap_or_default();

        // `TextStyle` has no `inherit` flag, so the merge is unconditional (all
        // fields optional).
        let style = match &self.style {
            Some(own) => ambient_style.merge(own),
            None => ambient_style,
        };
        let mut paragraph = RichText::new(self.span(style))
            .align(self.align.or(ambient_align).unwrap_or(TextAlign::Start))
            .with_text_sizing(MediaQuery::text_sizing_of(ctx))
            .font_weight_adjustment(MediaQuery::font_weight_adjustment_of(ctx).unwrap_or(0));
        if let Some(direction) = self.direction {
            paragraph = paragraph.direction(direction);
        }
        if let Some(max_lines) = self.max_lines.or(ambient_max_lines) {
            paragraph = paragraph.max_lines(max_lines);
        }
        // Inside a selection area the text registers itself, which takes a
        // mounted state of its own.
        if let Some((registry, color)) = SelectionScope::registry_of(ctx) {
            return SelectableParagraph::new(paragraph, self.on_link.clone(), registry, color)
                .boxed();
        }
        paragraph.linked(self.on_link.as_ref())
    }
}
