//! [`RichText`] — displays a tree of styled inline spans in one paragraph.

use flui_objects::RenderParagraph;
use flui_painting::typography::{InlineSpan, TextAlign, TextDirection, TextLink};
use flui_rendering::protocol::BoxProtocol;
use flui_view::element::ElementKind;
use flui_view::{
    BuildContext, EventCx, EventOutcome, IntoView, RenderView, StatelessView, View, ViewExt,
    impl_render_view,
};

use super::linked_paragraph::{LinkRouting, LinkedParagraph};
use crate::MediaQuery;
use crate::support::{RefCallback, ref_callback};

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
/// # Links
///
/// A span made a link with
/// [`TextSpan::with_link`](flui_painting::typography::TextSpan::with_link)
/// does something only through [`on_link`](Self::on_link): a tap on the
/// link's glyphs, or assistive technology activating the link, calls it with
/// the span's [`TextLink`]. While it is set, the pointer over a link is a
/// pointing hand and each link is an accessible link node. Without it, links
/// are plain text.
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
///
/// A link:
///
/// ```rust
/// # use flui_widgets::prelude::*;
/// # use flui_painting::typography::TextSpan;
/// let _ = RichText::new(
///     TextSpan::new("Read the ").with_child(TextSpan::new("terms").with_link("app://terms")),
/// )
/// .on_link(|_cx, link| println!("open {link}"));
/// ```
#[derive(Clone)]
pub struct RichText {
    pub(super) text: InlineSpan,
    align: TextAlign,
    direction: TextDirection,
    max_lines: Option<u32>,
    on_link: Option<RefCallback<TextLink>>,
}

impl std::fmt::Debug for RichText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RichText")
            .field("text", &self.text)
            .field("align", &self.align)
            .field("direction", &self.direction)
            .field("max_lines", &self.max_lines)
            .field("on_link", &self.on_link.is_some())
            .finish()
    }
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
            on_link: None,
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

    /// Call `handler` with a link's [`TextLink`] when a tap or assistive
    /// technology activates that link. It receives the activation's
    /// [`EventCx`], so it writes signals as any event callback does.
    #[must_use]
    pub fn on_link<F, R>(mut self, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, &TextLink) -> R + 'static,
        R: EventOutcome,
    {
        self.on_link = Some(ref_callback(handler));
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
        let paragraph = ResolvedParagraph {
            authored: self.clone(),
            text_sizing: MediaQuery::text_sizing_of(ctx),
            font_weight_adjustment: MediaQuery::font_weight_adjustment_of(ctx).unwrap_or(0),
            links: None,
        };
        match &self.on_link {
            // Links need recognizers built against the arena scope, which
            // only a mounted state can acquire.
            Some(on_link) => LinkedParagraph::new(paragraph, on_link.clone()).boxed(),
            None => paragraph.boxed(),
        }
    }
}

/// The inherited dependency belongs to the widget build; the render object
/// receives values and keeps the authored span tree unchanged.
#[derive(Clone, Debug)]
pub(super) struct ResolvedParagraph {
    pub(super) authored: RichText,
    pub(super) text_sizing: Option<flui_painting::TextSizing>,
    pub(super) font_weight_adjustment: i32,
    /// What reaches the links, when a handler activates them.
    pub(super) links: Option<LinkRouting>,
}

impl RenderView for ResolvedParagraph {
    type Protocol = BoxProtocol;
    type RenderObject = RenderParagraph;

    fn create_render_object(&self, ctx: &flui_view::RenderObjectContext<'_>) -> Self::RenderObject {
        RenderParagraph::new(self.authored.text.clone(), self.authored.direction)
            .with_text_align(self.authored.align)
            .with_max_lines(self.authored.max_lines)
            .with_text_sizing(self.text_sizing.clone())
            .with_font_weight_adjustment(self.font_weight_adjustment)
            .with_link_routes(LinkRouting::sync(self.links.as_ref(), ctx, &[]))
    }

    fn update_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let existing = render_object.link_routes().to_vec();
        let routes = LinkRouting::sync(self.links.as_ref(), ctx, &existing);
        render_object.set_text(self.authored.text.clone())
            | render_object.set_text_align(self.authored.align)
            | render_object.set_text_direction(self.authored.direction)
            | render_object.set_max_lines(self.authored.max_lines)
            | render_object.set_text_sizing(self.text_sizing.clone())
            | render_object.set_font_weight_adjustment(self.font_weight_adjustment)
            | render_object.set_link_routes(routes)
    }

    fn did_unmount_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        LinkRouting::release(ctx, render_object.link_routes());
        let _ = render_object.set_link_routes(Vec::new());
    }
}

impl_render_view!(ResolvedParagraph);
