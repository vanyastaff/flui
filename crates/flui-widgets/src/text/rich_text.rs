//! [`RichText`] — lays out and paints a tree of styled inline spans as one
//! paragraph.

use flui_foundation::{CreationSite, TextDirection};
use flui_layer::LayerLink;
use flui_objects::{RenderParagraph, TextLinkFocus};
use flui_painting::TextSizing;
use flui_painting::typography::{InlineSpan, TextAlign, TextLink};
use flui_rendering::protocol::BoxProtocol;
use flui_semantics::SemanticsAnchor;
use flui_view::{BoxedView, RenderView, ViewExt, impl_render_view};

use super::linked_paragraph::{LinkRouting, LinkedParagraph};
use crate::Directionality;
use crate::support::RefCallback;

/// Lays out and paints a tree of styled [`InlineSpan`]s (most commonly a
/// [`TextSpan`](flui_painting::typography::TextSpan)) as one paragraph.
///
/// The render widget [`Text`](crate::Text) builds, as Flutter's `RichText`
/// is the one its `Text` builds (`flutter/flutter@3.50.1`,
/// `packages/flutter/lib/src/widgets/basic.dart:6514`): each span carries its
/// own style, so a sentence mixes bold and coloured words in one paragraph.
/// It reads the ambient [`Directionality`] when it names no direction of its
/// own, as Flutter's does, while it configures its render object
/// (`basic.dart:6665` and `:6687`), so a direction change reconfigures the
/// same render object.
/// The ambient [`DefaultTextStyle`](crate::DefaultTextStyle), the
/// [`MediaQuery`](flui_view::environment::MediaQuery) text sizing and weight
/// adjustment, a [`SelectionArea`](crate::SelectionArea) and link handling
/// are [`Text`](crate::Text)'s: a `RichText` paints its spans at their
/// authored sizes unless it is given a [`text_sizing`](Self::text_sizing), as
/// Flutter's defaults to `TextScaler.noScaling` (`basic.dart:6535`), and its
/// links are plain text.
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
    pub(super) text: InlineSpan,
    pub(super) align: TextAlign,
    /// The authored direction; unset, the ambient one.
    pub(super) direction: Option<TextDirection>,
    pub(super) max_lines: Option<u32>,
    /// The sizing its spans are laid out at; `None` inherits the
    /// presentation's, which only [`Text`](crate::Text) asks for.
    pub(super) text_sizing: Option<TextSizing>,
    pub(super) font_weight_adjustment: i32,
    /// What reaches the links, when a handler activates them.
    pub(super) links: Option<LinkRouting>,
    /// The link holding the primary keyboard focus.
    pub(super) link_focus: Option<TextLinkFocus>,
    /// The anchor a selection area registered the text under, while it is
    /// selectable (ADR-0214).
    pub(super) selectable: Option<SemanticsAnchor>,
    /// The selected bytes and the colour that highlights them.
    pub(super) highlight: Option<super::selection_area::Highlight>,
    pub(super) site: CreationSite,
}

impl RichText {
    /// Display `text` (any type convertible into an [`InlineSpan`] — most
    /// commonly a [`TextSpan`](flui_painting::typography::TextSpan)) with start
    /// alignment, in the ambient [`Directionality`]'s reading direction, at
    /// its spans' authored sizes.
    #[track_caller]
    pub fn new(text: impl Into<InlineSpan>) -> Self {
        Self {
            text: text.into(),
            align: TextAlign::Start,
            direction: None,
            max_lines: None,
            text_sizing: Some(TextSizing::fixed()),
            font_weight_adjustment: 0,
            links: None,
            link_focus: None,
            selectable: None,
            highlight: None,
            site: CreationSite::here(),
        }
    }

    /// Set the horizontal alignment of the text within its bounds.
    #[must_use]
    pub fn align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }

    /// Set the reading direction. Unset, the ambient [`Directionality`] applies,
    /// then left-to-right.
    #[must_use]
    pub fn direction(mut self, direction: TextDirection) -> Self {
        self.direction = Some(direction);
        self
    }

    /// Cap the number of lines before truncating.
    #[must_use]
    pub fn max_lines(mut self, max_lines: u32) -> Self {
        self.max_lines = Some(max_lines);
        self
    }

    /// Size each span by `sizing`, as Flutter's `RichText` takes a
    /// `textScaler` (`basic.dart:6611`). Unset, each span keeps its authored
    /// size; [`Text`](crate::Text) passes the ambient
    /// [`MediaQuery`](flui_view::environment::MediaQuery)'s.
    #[must_use]
    pub fn text_sizing(mut self, sizing: TextSizing) -> Self {
        self.text_sizing = Some(sizing);
        self
    }

    /// Shift every span's weight by `adjustment` before its face is matched
    /// (ADR-0174), as the platform's bold-text setting does through
    /// [`Text`](crate::Text). Zero keeps the authored weights.
    #[must_use]
    pub fn font_weight_adjustment(mut self, adjustment: i32) -> Self {
        self.font_weight_adjustment = adjustment;
        self
    }

    /// The text sizing `Text` resolved: the ambient `MediaQuery`'s, or, with
    /// none above it, the presentation's own (`None`).
    pub(super) fn with_text_sizing(mut self, sizing: Option<TextSizing>) -> Self {
        self.text_sizing = sizing;
        self
    }

    /// This paragraph, with its links activated by `on_link` when set.
    pub(super) fn linked(self, on_link: Option<&RefCallback<TextLink>>) -> BoxedView {
        match on_link {
            // Links need recognizers built against the arena scope, which
            // only a mounted state can acquire.
            Some(on_link) => LinkedParagraph::new(self, on_link.clone()).boxed(),
            None => self.boxed(),
        }
    }

    /// The authored direction, else the ambient one, read with a dependency
    /// only when none is authored.
    fn resolved_direction(&self, ctx: &flui_view::RenderObjectContext<'_>) -> TextDirection {
        self.direction
            .unwrap_or_else(|| Directionality::of_render_object(ctx))
    }

    /// The links the selection's start and end are published under, for a
    /// selection area's handles.
    fn handles(&self) -> (Option<LayerLink>, Option<LayerLink>) {
        self.highlight
            .as_ref()
            .map_or((None, None), |highlight| highlight.handles)
    }

    fn shown_links_listener(&self) -> Option<flui_objects::ShownLinksListener> {
        self.links
            .as_ref()
            .map(|links| links.shown_links_listener().clone())
    }
}

impl RenderView for RichText {
    type Protocol = BoxProtocol;
    type RenderObject = RenderParagraph;

    fn create_render_object(&self, ctx: &flui_view::RenderObjectContext<'_>) -> Self::RenderObject {
        let mut paragraph = RenderParagraph::new(self.text.clone(), self.resolved_direction(ctx))
            .with_text_align(self.align)
            .with_max_lines(self.max_lines)
            .with_text_sizing(self.text_sizing.clone())
            .with_font_weight_adjustment(self.font_weight_adjustment)
            .with_link_routes(LinkRouting::sync(self.links.as_ref(), ctx, &[]));
        let _ = paragraph.set_link_focus(self.link_focus);
        let _ = paragraph.set_selectable(self.selectable.clone());
        let _ = paragraph.set_selection(
            self.highlight
                .as_ref()
                .map(|highlight| highlight.range.clone()),
        );
        if let Some(highlight) = &self.highlight {
            let _ = paragraph.set_selection_color(highlight.color);
        }
        let (start, end) = self.handles();
        let _ = paragraph.set_selection_handles(start, end);
        paragraph.set_shown_links_listener(self.shown_links_listener());
        paragraph
    }

    fn update_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let existing = render_object.link_routes().to_vec();
        let routes = LinkRouting::sync(self.links.as_ref(), ctx, &existing);
        render_object.set_shown_links_listener(self.shown_links_listener());
        let (start, end) = self.handles();
        render_object.set_text(self.text.clone())
            | render_object.set_text_align(self.align)
            | render_object.set_text_direction(self.resolved_direction(ctx))
            | render_object.set_max_lines(self.max_lines)
            | render_object.set_text_sizing(self.text_sizing.clone())
            | render_object.set_font_weight_adjustment(self.font_weight_adjustment)
            | render_object.set_link_routes(routes)
            | render_object.set_link_focus(self.link_focus)
            | render_object.set_selectable(self.selectable.clone())
            | render_object.set_selection(
                self.highlight
                    .as_ref()
                    .map(|highlight| highlight.range.clone()),
            )
            | self
                .highlight
                .as_ref()
                .map_or(flui_rendering::RenderUpdateImpact::NONE, |highlight| {
                    render_object.set_selection_color(highlight.color)
                })
            | render_object.set_selection_handles(start, end)
    }

    fn did_unmount_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        LinkRouting::release(self.links.as_ref(), ctx, render_object.link_routes());
        let _ = render_object.set_link_routes(Vec::new());
    }
}

impl_render_view!(RichText, site = site);
