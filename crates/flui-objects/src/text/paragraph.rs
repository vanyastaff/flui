//! RenderParagraph — lays out and paints styled text.
//!
//! Wraps [`flui_painting::TextPainter`] (the shaping and metrics authority)
//! the way [`RenderImage`](crate::RenderImage) wraps a leaf: the render
//! object owns a painter, drives its layout from box constraints, and
//! forwards intrinsics / baseline / paint to it. Every measurement goes
//! through the UI runtime's text context, lent by the layout, intrinsics and dry
//! contexts as `ctx.text()` (ADR-0092 §10 step 3). It covers
//! the renderable core of a paragraph: layout, dry layout, the four
//! intrinsics, baseline, and paint — with soft wrap, `max_lines`, and
//! ellipsis truncation.
//!
//! Its spans' links and cursors are answered from the same committed layout
//! that paints the text, so they follow the text scale (ADR-0183): a hit on
//! a link routes to that link's [`TextLinkRoute`] and asks for its cursor,
//! and a paragraph with routed links publishes one accessible node per link
//! and per stretch of text between them (`links` module).
//!
//! Out of scope for this object (separable): inline `WidgetSpan` children, text selection, and the
//! clip/fade `TextOverflow` policies (only `ellipsis` is wired here).

mod links;

pub use links::{ShownLinksListener, TextLinkFocus, TextLinkKey, TextLinkRoute};

use flui_foundation::Diagnosticable;
use flui_foundation::Leaf;
use flui_foundation::geometry::{Matrix4, Offset, Rect, Size};
use flui_interaction::CursorIcon;
use flui_painting::styling::Color;
use flui_painting::text_boundaries;
use flui_painting::typography::{
    InlineSpan, LineMetrics, TextAffinity, TextAlign, TextDirection, TextPosition,
};
use flui_painting::{Invalidation, Paint, TextBaseline as PainterBaseline, TextPainter};

use flui_rendering::{
    RenderResult,
    constraints::BoxConstraints,
    context::{
        BoxDryBaselineCtx, BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext,
        PaintCx,
    },
    hit_testing::CursorRequest,
    layer::LayerLink,
    parent_data::BoxParentData,
    semantics::{SemanticsAnchor, SemanticsConfiguration, SemanticsRegions, TextContent},
    traits::{RenderBox, TextBaseline},
};

use links::SpanRuns;

/// Render object that lays out and paints a styled text span.
#[derive(Debug)]
pub struct RenderParagraph {
    /// The shaping + metrics authority. Owns the text, alignment, scale,
    /// max-lines/ellipsis, and the cached layout.
    painter: TextPainter,
    /// Whether the text wraps at the box's max width. When `false` the text
    /// lays out at unbounded width (single logical line per hard break) and
    /// can overflow.
    soft_wrap: bool,
    /// The text cut where its link or cursor changes.
    runs: SpanRuns,
    /// How the owner reaches each link, in [`InlineSpan::links`] order;
    /// empty while nothing handles them.
    link_routes: Vec<TextLinkRoute>,
    /// The link holding the primary keyboard focus.
    link_focus: Option<TextLinkFocus>,
    /// Told which links have glyphs after a layout that changed it.
    shown_links_listener: Option<ShownLinksListener>,
    /// What the listener was last told.
    shown_links: Vec<bool>,
    /// The anchor a selection area registered this text under, while it is
    /// selectable: it then publishes its runs (ADR-0214).
    selectable: Option<SemanticsAnchor>,
    /// The bytes of the text to highlight as selected, if any.
    selection: Option<std::ops::Range<usize>>,
    /// The highlight behind the selected glyphs.
    selection_color: Color,
    /// Where the selection's start is published for a handle, if anywhere.
    start_handle: Option<LayerLink>,
    /// Where the selection's end is published for a handle, if anywhere.
    end_handle: Option<LayerLink>,
}

impl RenderParagraph {
    /// Creates a paragraph for `text` laid out in `direction`.
    pub fn new(text: impl Into<InlineSpan>, direction: TextDirection) -> Self {
        let text = text.into();
        Self {
            runs: SpanRuns::of(&text),
            painter: TextPainter::new()
                .with_text(text)
                .with_text_direction(direction),
            soft_wrap: true,
            link_routes: Vec::new(),
            link_focus: None,
            shown_links_listener: None,
            shown_links: Vec::new(),
            selectable: None,
            selection: None,
            selection_color: Color::TRANSPARENT,
            start_handle: None,
            end_handle: None,
        }
    }

    /// Routes the text's links (builder form): one route per link, in
    /// [`InlineSpan::links`] order.
    #[must_use]
    pub fn with_link_routes(mut self, routes: Vec<TextLinkRoute>) -> Self {
        self.link_routes = routes;
        self
    }

    /// Replaces the link routes. Hit testing reads them live; the
    /// accessible link nodes change only when a route reaches a different
    /// registration.
    pub fn set_link_routes(
        &mut self,
        routes: Vec<TextLinkRoute>,
    ) -> flui_rendering::RenderUpdateImpact {
        let unchanged = routes.len() == self.link_routes.len()
            && routes
                .iter()
                .zip(&self.link_routes)
                .all(|(new, old)| new.same_registration(old));
        self.link_routes = routes;
        if unchanged {
            flui_rendering::RenderUpdateImpact::NONE
        } else {
            flui_rendering::RenderUpdateImpact::SEMANTICS
        }
    }

    /// The routes the text's links are reached through.
    pub fn link_routes(&self) -> &[TextLinkRoute] {
        &self.link_routes
    }

    /// Makes the text selectable under `anchor`, the identity a selection
    /// area registered it with, or plain static text again with `None`.
    ///
    /// Selectable text publishes its laid-out text as runs, declares
    /// `anchor` on its node and merges its own descendants, so it keeps a
    /// node of its own and its runs even inside an outer merge (ADR-0214
    /// §1-§2). Text outside a selection area publishes no runs.
    pub fn set_selectable(
        &mut self,
        anchor: Option<SemanticsAnchor>,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.selectable == anchor {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.selectable = anchor;
        flui_rendering::RenderUpdateImpact::SEMANTICS
    }

    /// The anchor the text is selectable under, if it is.
    pub fn selectable(&self) -> Option<&SemanticsAnchor> {
        self.selectable.as_ref()
    }

    /// Highlights `selection`, bytes of the text, behind its glyphs, or no
    /// part of it with `None`. The range is clamped to the laid-out text and
    /// its ends moved to grapheme boundaries when painted; an empty range
    /// paints nothing.
    pub fn set_selection(
        &mut self,
        selection: Option<std::ops::Range<usize>>,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.selection == selection {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.selection = selection;
        flui_rendering::RenderUpdateImpact::PAINT
    }

    /// The highlighted bytes, if any.
    pub fn selection(&self) -> Option<std::ops::Range<usize>> {
        self.selection.clone()
    }

    /// Sets the colour of the selection highlight; a transparent colour
    /// paints none.
    pub fn set_selection_color(&mut self, color: Color) -> flui_rendering::RenderUpdateImpact {
        if self.selection_color == color {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.selection_color = color;
        if self
            .selection
            .as_ref()
            .is_some_and(|range| !range.is_empty())
        {
            flui_rendering::RenderUpdateImpact::PAINT
        } else {
            flui_rendering::RenderUpdateImpact::NONE
        }
    }

    /// The colour of the selection highlight.
    pub fn selection_color(&self) -> Color {
        self.selection_color
    }

    /// Publishes where the selection's ends sit, for handles that follow
    /// them: a zero-width leader one line tall at the caret before the
    /// first selected byte under `start`, and after the last under `end`.
    /// The links come from whatever owns the handles; `None` publishes
    /// nothing for that end, and a paragraph without a selection publishes
    /// neither.
    pub fn set_selection_handles(
        &mut self,
        start: Option<LayerLink>,
        end: Option<LayerLink>,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.start_handle == start && self.end_handle == end {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        let was_compositing = self.publishes_handles();
        self.start_handle = start;
        self.end_handle = end;
        if was_compositing == self.publishes_handles() {
            flui_rendering::RenderUpdateImpact::PAINT
        } else {
            flui_rendering::RenderUpdateImpact::COMPOSITING_BITS
        }
    }

    /// The links the selection's start and end are published under.
    pub fn selection_handles(&self) -> (Option<LayerLink>, Option<LayerLink>) {
        (self.start_handle, self.end_handle)
    }

    fn publishes_handles(&self) -> bool {
        self.start_handle.is_some() || self.end_handle.is_some()
    }

    /// Pushes a leader at each selection end that has a link. The start
    /// sits on the line after it (downstream) and the end on the line it
    /// ends (upstream), so an end at a soft wrap stays with its glyphs.
    fn paint_handle_leaders(&self, ctx: &mut PaintCx<'_, Leaf>) {
        let Some(range) = self.selection.as_ref() else {
            return;
        };
        let length = self
            .painter
            .text()
            .map_or(0, |span| span.to_plain_text().len());
        let ends = [
            (
                self.start_handle,
                TextPosition::downstream(range.start.min(length)),
            ),
            (
                self.end_handle,
                TextPosition::upstream(range.end.min(length)),
            ),
        ];
        let lines = self.painter.get_line_metrics();
        for (link, position) in ends {
            let Some(link) = link else {
                continue;
            };
            let caret = self.painter.get_offset_for_caret(position);
            let height = line_of(&lines, position).map_or(0.0, |line| line.height);
            ctx.with_transform(Matrix4::translation(caret.dx, caret.dy, 0.0), |ctx| {
                ctx.with_leader(link, Size::new(0.0, height), |_| {});
            });
        }
    }

    /// Paints the selection's boxes behind the glyphs, each once: with a
    /// translucent colour an overlap would read darker.
    fn paint_selection(&self, ctx: &mut PaintCx<'_, Leaf>) {
        let Some(range) = self.selection.as_ref() else {
            return;
        };
        if range.is_empty() || self.selection_color.is_transparent() {
            return;
        }
        let Some(span) = self.painter.text() else {
            return;
        };
        let text = span.to_plain_text();
        let snap = |byte: usize| {
            let byte = byte.min(text.len());
            if text_boundaries::is_grapheme_boundary(&text, byte) {
                byte
            } else {
                text_boundaries::previous_grapheme_boundary(&text, byte)
            }
        };
        let (start, end) = (snap(range.start), snap(range.end));
        if start >= end {
            return;
        }
        let paint = Paint::fill(self.selection_color);
        let mut drawn: Vec<Rect> = Vec::new();
        for text_box in self.painter.get_boxes_for_selection(start, end) {
            if text_box.rect.is_empty() || drawn.contains(&text_box.rect) {
                continue;
            }
            drawn.push(text_box.rect);
            ctx.canvas().draw_rect(text_box.rect, &paint);
        }
    }

    /// The laid-out text as runs, or `None` before layout. A run the
    /// semantics model refuses is a bug in this description: it is logged
    /// and the text publishes its label alone.
    fn semantics_text_content(&self, text: &str) -> Option<TextContent> {
        if !self.painter.has_layout() {
            return None;
        }
        let runs = crate::text::runs::text_runs(
            text,
            self.painter.line_layouts(),
            self.painter.text_direction().unwrap_or(TextDirection::Ltr),
            0.0,
            |start, line| {
                let caret = self
                    .painter
                    .get_offset_for_caret(TextPosition::new(start, TextAffinity::Downstream));
                flui_foundation::geometry::Rect::from_ltrb(
                    caret.dx,
                    line.top,
                    caret.dx,
                    line.bottom,
                )
            },
        );
        match runs.and_then(TextContent::try_new) {
            Ok(content) => Some(content),
            Err(error) => {
                tracing::error!(%error, "BUG: a paragraph's layout described a run the semantics model refuses");
                None
            }
        }
    }

    /// Names the link that holds the primary keyboard focus and the ring it
    /// shows, or none.
    pub fn set_link_focus(
        &mut self,
        focus: Option<TextLinkFocus>,
    ) -> flui_rendering::RenderUpdateImpact {
        let previous = std::mem::replace(&mut self.link_focus, focus);
        if previous.map(|focus| focus.link) != focus.map(|focus| focus.link) {
            flui_rendering::RenderUpdateImpact::PAINT
                | flui_rendering::RenderUpdateImpact::SEMANTICS
        } else if previous != focus {
            flui_rendering::RenderUpdateImpact::PAINT
        } else {
            flui_rendering::RenderUpdateImpact::NONE
        }
    }

    /// Tells `listener`, after each layout that changes it, which links have
    /// glyphs (builder form).
    #[must_use]
    pub fn with_shown_links_listener(mut self, listener: ShownLinksListener) -> Self {
        self.shown_links_listener = Some(listener);
        self
    }

    /// Replaces the shown-links listener. A new listener is told at the next
    /// layout, whatever an earlier one was told.
    pub fn set_shown_links_listener(&mut self, listener: Option<ShownLinksListener>) {
        let same = match (&self.shown_links_listener, &listener) {
            (Some(current), Some(new)) => current.same(new),
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.shown_links_listener = listener;
            self.shown_links.clear();
        }
    }

    /// Tells the listener which links the committed layout shows, when that
    /// changed. Only a layout that truncated the text can hide a link.
    fn publish_shown_links(&mut self) {
        let Some(listener) = &self.shown_links_listener else {
            return;
        };
        let truncated = self.painter.did_exceed_max_lines() || self.painter.ellipsis().is_some();
        let shown = self.runs.shown_links(&self.painter, truncated);
        if shown != self.shown_links {
            listener.notify(&shown);
            self.shown_links = shown;
        }
    }

    /// The bounds of link `link`'s glyphs, in this paragraph's coordinates,
    /// from the layout it committed: what keyboard focus orders and reveals
    /// the link by. `None` before layout, for an index past the text's links,
    /// or for a link `max_lines` or an ellipsis cut.
    #[must_use]
    pub fn link_bounds(&self, link: usize) -> Option<flui_foundation::geometry::Rect<f64>> {
        if !self.painter.has_layout() {
            return None;
        }
        self.runs.link_bounds(&self.painter, link)
    }

    /// Whether the paragraph publishes its links as accessible nodes of
    /// their own: some link has a route.
    fn routes_links(&self) -> bool {
        self.runs.link_count() > 0 && !self.link_routes.is_empty()
    }

    /// Sets the text alignment (builder form).
    #[must_use]
    pub fn with_text_align(mut self, align: TextAlign) -> Self {
        self.painter.set_text_align(align);
        self
    }

    /// Sets the maximum number of lines before truncation (builder form).
    #[must_use]
    pub fn with_max_lines(mut self, max_lines: Option<u32>) -> Self {
        self.painter.set_max_lines(max_lines);
        self
    }

    /// Sets the ellipsis string shown when text is truncated (builder form).
    #[must_use]
    pub fn with_ellipsis(mut self, ellipsis: Option<String>) -> Self {
        self.painter.set_ellipsis(ellipsis);
        self
    }

    /// Sets the accessibility text scale factor (builder form).
    #[must_use]
    pub fn with_text_scale_factor(mut self, factor: f64) -> Self {
        self.painter.set_text_scale_factor(factor);
        self
    }

    /// Updates accessibility sizing and invalidates paragraph geometry.
    pub fn set_text_scale_factor(&mut self, factor: f64) -> flui_rendering::RenderUpdateImpact {
        let previous = self.painter.text_sizing().cloned();
        self.painter.set_text_scale_factor(factor);
        if self.painter.text_sizing() == previous.as_ref() {
            flui_rendering::RenderUpdateImpact::NONE
        } else {
            flui_rendering::RenderUpdateImpact::LAYOUT
                | flui_rendering::RenderUpdateImpact::SEMANTICS
        }
    }

    /// Replace the inherited sizing authority, or restore it with `None`.
    #[must_use]
    pub fn with_text_sizing(mut self, sizing: Option<flui_painting::TextSizing>) -> Self {
        self.painter.set_text_sizing(sizing);
        self
    }

    /// Update glyph geometry and accessibility with the selected authority.
    pub fn set_text_sizing(
        &mut self,
        sizing: Option<flui_painting::TextSizing>,
    ) -> flui_rendering::RenderUpdateImpact {
        let previous = self.painter.text_sizing().cloned();
        self.painter.set_text_sizing(sizing);
        if self.painter.text_sizing() == previous.as_ref() {
            flui_rendering::RenderUpdateImpact::NONE
        } else {
            flui_rendering::RenderUpdateImpact::LAYOUT
                | flui_rendering::RenderUpdateImpact::SEMANTICS
        }
    }

    /// Applies a signed weight adjustment without modifying authored spans.
    #[must_use]
    pub fn with_font_weight_adjustment(mut self, adjustment: i32) -> Self {
        self.painter.set_font_weight_adjustment(adjustment);
        self
    }

    /// Updates weight resolution and invalidates paragraph geometry.
    pub fn set_font_weight_adjustment(
        &mut self,
        adjustment: i32,
    ) -> flui_rendering::RenderUpdateImpact {
        let previous = self.painter.font_weight_adjustment();
        self.painter.set_font_weight_adjustment(adjustment);
        if previous == adjustment {
            flui_rendering::RenderUpdateImpact::NONE
        } else {
            flui_rendering::RenderUpdateImpact::LAYOUT
                | flui_rendering::RenderUpdateImpact::SEMANTICS
        }
    }

    /// Disables line wrapping (builder form) — the text lays out at unbounded
    /// width and may overflow the box.
    #[must_use]
    pub fn without_soft_wrap(mut self) -> Self {
        self.soft_wrap = false;
        self
    }

    /// Replaces the text span and returns the invalidation level.
    ///
    /// - [`Invalidation::Layout`] — text content changed; caller must
    ///   mark the node layout-dirty.
    /// - [`Invalidation::Paint`] — only paint attributes changed (color,
    ///   shadow); caller can mark paint-dirty only (cheaper).
    /// - [`Invalidation::None`] — no observable change.
    pub fn set_text(&mut self, text: impl Into<InlineSpan>) -> flui_rendering::RenderUpdateImpact {
        let text = text.into();
        let runs = SpanRuns::of(&text);
        // The runs decide the link nodes' boundaries and labels.
        let semantics_changed = runs != self.runs
            || self.painter.text().map(InlineSpan::to_semantics_text)
                != Some(text.to_semantics_text());
        self.runs = runs;
        let impact = text_invalidation_impact(self.painter.set_text(Some(text)), true);
        if semantics_changed {
            impact | flui_rendering::RenderUpdateImpact::SEMANTICS
        } else {
            impact
        }
    }

    /// Sets the text alignment. The caller is responsible for marking the node
    /// layout-dirty.
    pub fn set_text_align(&mut self, align: TextAlign) -> flui_rendering::RenderUpdateImpact {
        text_invalidation_impact(self.painter.set_text_align(align), false)
    }

    /// Updates the resolved text direction.
    pub fn set_text_direction(
        &mut self,
        direction: TextDirection,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.painter.text_direction() == Some(direction) {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.painter.set_text_direction(Some(direction));
        flui_rendering::RenderUpdateImpact::LAYOUT | flui_rendering::RenderUpdateImpact::SEMANTICS
    }

    /// Updates the maximum line count.
    pub fn set_max_lines(&mut self, max_lines: Option<u32>) -> flui_rendering::RenderUpdateImpact {
        if self.painter.max_lines() == max_lines {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.painter.set_max_lines(max_lines);
        flui_rendering::RenderUpdateImpact::LAYOUT | flui_rendering::RenderUpdateImpact::SEMANTICS
    }

    /// Read access to the underlying painter (cursor / selection geometry).
    pub fn painter(&self) -> &TextPainter {
        &self.painter
    }

    /// The width to lay out at for the given constraints. The box width
    /// matters — and the finite max is used — when the text wraps OR an
    /// ellipsis is configured. A no-wrap label
    /// still needs the finite width so its ellipsis truncation can trigger;
    /// only a no-wrap, no-ellipsis paragraph lays out at unbounded width.
    fn layout_max_width(&self, constraints: &BoxConstraints) -> f64 {
        let width_matters = self.soft_wrap || self.painter.ellipsis().is_some();
        let max = constraints.max_width;
        if width_matters && max.is_finite() {
            max
        } else {
            f64::INFINITY
        }
    }
}

fn text_invalidation_impact(
    invalidation: Invalidation,
    semantics_may_change: bool,
) -> flui_rendering::RenderUpdateImpact {
    match invalidation {
        Invalidation::None => flui_rendering::RenderUpdateImpact::NONE,
        Invalidation::Paint => flui_rendering::RenderUpdateImpact::PAINT,
        Invalidation::Layout => {
            let impact = flui_rendering::RenderUpdateImpact::LAYOUT;
            if semantics_may_change {
                impact | flui_rendering::RenderUpdateImpact::SEMANTICS
            } else {
                impact
            }
        }
    }
}

/// The line `position` caret sits on: with upstream affinity, the line a
/// soft wrap at it ends.
fn line_of(lines: &[LineMetrics], position: TextPosition) -> Option<&LineMetrics> {
    let byte = position.offset;
    let ending = || {
        lines
            .iter()
            .find(|line| line.start_index < byte && line.end_index == byte)
    };
    (position.affinity == TextAffinity::Upstream)
        .then(ending)
        .flatten()
        .or_else(|| {
            lines
                .iter()
                .take_while(|line| line.start_index <= byte)
                .last()
                .or_else(|| lines.first())
        })
}

impl Diagnosticable for RenderParagraph {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add_enum("text_align", self.painter.text_align());
        properties.add(
            "text_direction",
            self.painter
                .text_direction()
                .map_or_else(|| "unset".to_string(), |d| format!("{d:?}")),
        );
        properties.add_flag("soft_wrap", self.soft_wrap, "soft wrap");
        properties.add("selection", format!("{:?}", self.selection));
        properties.add("selection_color", format!("{:?}", self.selection_color));
        properties.add(
            "selection_handles",
            format!(
                "{:?}",
                (
                    self.start_handle.map(|link| link.id()),
                    self.end_handle.map(|link| link.id()),
                )
            ),
        );
        properties.add(
            "max_lines",
            self.painter
                .max_lines()
                .map_or_else(|| "unlimited".to_string(), |n| n.to_string()),
        );
        properties.add(
            "ellipsis",
            self.painter
                .ellipsis()
                .map_or_else(|| "none".to_string(), ToString::to_string),
        );
        // Emit the plain text so diagnostics dumps and test finders can match on
        // content without inspecting the full `InlineSpan` tree.
        if let Some(span) = self.painter.text() {
            properties.add("text", span.to_plain_text());
            // Lets a mounted-widget test read a resolved text color back out
            // (e.g. a themed/cascaded color reaching the real paragraph a
            // build produced) without downcasting or inspecting paint
            // output — `TextStyle: Debug` is already required for the type
            // to be usable at all, so this is a pure diagnostics read, not
            // a new capability.
            properties.add("style", format!("{:?}", span.style()));
        }
    }
}

impl RenderBox for RenderParagraph {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    /// A paragraph that publishes its selection's ends pushes leader layers.
    fn always_needs_compositing(&self) -> bool {
        self.publishes_handles()
    }

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
    ) -> RenderResult<Size> {
        let constraints = *ctx.constraints();
        let max_width = self.layout_max_width(&constraints);
        ctx.text()?
            .measurement()
            .layout(&mut self.painter, constraints.min_width, max_width)?;
        self.publish_shown_links();
        // The text's own size, then clamped into the box constraints.
        Ok(constraints.constrain(self.painter.size()))
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> RenderResult<Size> {
        let max_width = self.layout_max_width(&constraints);
        let text_size =
            ctx.text()?
                .measurement()
                .dry_size(&self.painter, constraints.min_width, max_width)?;
        Ok(constraints.constrain(text_size))
    }

    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: TextBaseline,
        ctx: &mut BoxDryBaselineCtx<'_>,
    ) -> RenderResult<Option<f64>> {
        let max_width = self.layout_max_width(&constraints);
        let painter_baseline = match baseline {
            TextBaseline::Alphabetic => PainterBaseline::Alphabetic,
            TextBaseline::Ideographic => PainterBaseline::Ideographic,
        };
        Ok(ctx.text()?.measurement().dry_baseline(
            &self.painter,
            constraints.min_width,
            max_width,
            painter_baseline,
        )?)
    }

    // Width intrinsics ignore the height extent (text width does not depend on
    // available height); height intrinsics lay the text out at the given width.

    fn compute_min_intrinsic_width(
        &self,
        _height: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> RenderResult<f64> {
        Ok(ctx
            .text()?
            .measurement()
            .min_intrinsic_width(&self.painter)?)
    }

    fn compute_max_intrinsic_width(
        &self,
        _height: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> RenderResult<f64> {
        Ok(ctx
            .text()?
            .measurement()
            .max_intrinsic_width(&self.painter)?)
    }

    fn compute_min_intrinsic_height(
        &self,
        width: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> RenderResult<f64> {
        Ok(ctx
            .text()?
            .measurement()
            .intrinsic_height(&self.painter, width)?)
    }

    fn compute_max_intrinsic_height(
        &self,
        width: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> RenderResult<f64> {
        Ok(ctx
            .text()?
            .measurement()
            .intrinsic_height(&self.painter, width)?)
    }

    fn compute_distance_to_actual_baseline(
        &self,
        baseline: TextBaseline,
    ) -> RenderResult<Option<f64>> {
        // Map the render-side baseline enum onto the painting-side one (two
        // parallel definitions, consolidation tracked). Valid only after
        // `perform_layout` populated the painter's cache; the baseline phase
        // always runs after layout, but guard so a stray pre-layout query
        // returns None instead of panicking.
        let painter_baseline = match baseline {
            TextBaseline::Alphabetic => PainterBaseline::Alphabetic,
            TextBaseline::Ideographic => PainterBaseline::Ideographic,
        };
        Ok(self.painter.has_layout().then(|| {
            self.painter
                .compute_distance_to_actual_baseline(painter_baseline)
        }))
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Leaf>) {
        // The recorder pre-translates the canvas to this node's origin, so the
        // text paints in local coordinates. Skipped before layout (no cache).
        if !self.painter.has_layout() {
            return;
        }
        self.paint_selection(ctx);
        self.painter.paint(ctx.canvas(), Offset::ZERO);
        self.paint_handle_leaders(ctx);
        // The ring follows each line of the focused link, just outside its
        // glyphs, so it covers none of them.
        if let Some(TextLinkFocus {
            link,
            ring: Some(style),
        }) = self.link_focus
        {
            for text_box in self.runs.link_boxes(&self.painter, link) {
                style.paint_around(ctx.canvas(), text_box.rect);
            }
        }
    }

    /// The whole box is hit, as for any leaf. Over a routed link the hit
    /// routes to that link and asks for its cursor (a pointing hand unless
    /// the span sets another); over a span that sets a cursor it asks for
    /// that cursor. Elsewhere the paragraph routes nowhere and defers the
    /// cursor.
    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Leaf, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        if (self.link_routes.is_empty() && !self.runs.has_cursor()) || !self.painter.has_layout() {
            return true;
        }
        let Some(run) = self.runs.run_at(&self.painter, ctx.offset()) else {
            return true;
        };
        let route = run.link.and_then(|link| self.link_routes.get(link));
        if let Some(route) = route {
            ctx.set_self_hit_pointer_target(route.pointer_target());
        }
        if let Some(cursor) = run.cursor.or(route.map(|_| CursorIcon::Pointer)) {
            ctx.set_self_hit_cursor(CursorRequest::Icon(cursor));
        }
        true
    }

    /// A paragraph with routed links publishes one region per link and per
    /// stretch of text between them, bounded by their glyphs.
    fn semantics_regions(&self, size: Size) -> SemanticsRegions {
        if !self.routes_links() || !self.painter.has_layout() {
            return SemanticsRegions::default();
        }
        SemanticsRegions {
            before_children: self.runs.semantics_regions(
                &self.painter,
                &self.link_routes,
                self.link_focus.map(|focus| focus.link),
                self.painter.text_direction().map(semantics_text_direction),
                flui_foundation::geometry::Rect::from_ltwh(0.0, 0.0, size.width, size.height),
            ),
            after_children: Vec::new(),
        }
    }

    /// Covers the plain-text case this object supports (no `WidgetSpan`
    /// children — see the module doc's "Out of scope" list): the label uses
    /// each span's accessibility override in place of its own text,
    /// preserving children. The text direction is the paragraph's.
    ///
    /// A paragraph with routed links is instead a boundary whose children
    /// are its [`semantics_regions`](Self::semantics_regions): its links and
    /// the text between them, read in order, so no text is read twice.
    ///
    /// Mapping decision (see `crates/flui-objects/ARCHITECTURE.md` "Mapping
    /// decisions"): an EMPTY plain-text span sets neither `label` nor
    /// `text_direction`, so the paragraph stays un-annotated and contributes
    /// no semantics node of its own. `SemanticsConfiguration::set_text_direction`
    /// alone marks the configuration annotated (see that
    /// setter's doc comment), which would otherwise publish an empty,
    /// unlabelled node for every text-less paragraph in the tree.
    fn describe_semantics_configuration(&self, config: &mut SemanticsConfiguration) {
        let Some(span) = self.painter.text() else {
            return;
        };
        let text = span.to_semantics_text();
        if text.is_empty() {
            return;
        }
        let plain = span.to_plain_text();
        // A span's accessibility override reads differently from its glyphs.
        let overridden = text != plain;
        if self.routes_links() {
            config.set_semantics_boundary(true);
            config.set_explicit_child_nodes(true);
        } else {
            config.set_label(text);
        }
        // Selectable text is a nested merger with its runs: a node of its own
        // inside any merge, read and selected by character (ADR-0214). Its
        // runs are its authored text, so text a span's accessibility
        // override replaces or silences keeps its label and publishes no
        // runs until runs can carry overrides. With routed links it stays
        // the boundary of its link regions, so each link keeps its own node
        // and handler, and publishes no runs until links carry runs of their
        // own. Either way it names its anchor.
        if let Some(anchor) = &self.selectable {
            config.add_anchor(anchor.clone());
            config.set_semantics_boundary(true);
            if !self.routes_links() && !overridden {
                config.set_merging_semantics_of_descendants(true);
                if let Some(content) = self.semantics_text_content(&plain) {
                    config.set_text_content(content);
                }
            }
        }
        if let Some(direction) = self.painter.text_direction() {
            config.set_text_direction(semantics_text_direction(direction));
        }
    }
}

/// Maps the painting-side [`TextDirection`] onto `flui-semantics`'s own
/// parallel enum of the same name (two definitions of the same concept).
pub(super) fn semantics_text_direction(
    direction: TextDirection,
) -> flui_rendering::semantics::TextDirection {
    match direction {
        TextDirection::Ltr => flui_rendering::semantics::TextDirection::Ltr,
        TextDirection::Rtl => flui_rendering::semantics::TextDirection::Rtl,
    }
}
