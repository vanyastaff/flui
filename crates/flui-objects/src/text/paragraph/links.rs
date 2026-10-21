//! Which part of a paragraph a pointer is over, and the accessible nodes a
//! paragraph with links publishes.
//!
//! A paragraph's text is cut into runs wherever its link or its cursor
//! changes. A link and a cursor cover a span's children unless a child sets
//! its own, so a run takes them from the innermost span that sets each.
//! Links are numbered in document order, the order
//! [`InlineSpan::links`] lists them in: that index is the identity the
//! widget's [`TextLinkRoute`]s and the runs share.

use std::ops::Range;

use flui_foundation::geometry::{Offset, Rect};
use flui_interaction::CursorIcon;
use flui_painting::TextPainter;
use flui_painting::typography::{InlineSpan, TextAffinity, TextBox, TextSpan};
use flui_rendering::{
    hit_testing::PointerTarget,
    semantics::{
        SemanticsAction, SemanticsConfiguration, SemanticsRegion, SemanticsRegionKey,
        SemanticsRole, TextDirection,
    },
};

use crate::{FocusRingStyle, SemanticsActionRoute};

/// How the owner reaches one link of a paragraph: the pointer target its
/// contacts route to, and the route assistive technology activates it
/// through.
///
/// The widget that owns the link's tap recognizer and activation handler
/// registers both in the owner's interaction lane and hands the paragraph
/// one route per link, in [`InlineSpan::links`] order.
#[derive(Clone, Debug)]
pub struct TextLinkRoute {
    pointer: PointerTarget,
    activation: SemanticsActionRoute,
}

impl TextLinkRoute {
    /// Pair a link's pointer target with its activation route.
    #[must_use]
    pub fn new(pointer: PointerTarget, activation: SemanticsActionRoute) -> Self {
        Self {
            pointer,
            activation,
        }
    }

    /// The target a contact that lands on the link routes to.
    #[must_use]
    pub fn pointer_target(&self) -> PointerTarget {
        self.pointer
    }

    /// The route assistive technology activates the link through.
    #[must_use]
    pub fn activation(&self) -> &SemanticsActionRoute {
        &self.activation
    }

    /// Whether `other` reaches the same lane registrations, so the published
    /// semantics stay valid.
    pub(super) fn same_registration(&self, other: &Self) -> bool {
        self.pointer == other.pointer
            && self.activation.target() == other.activation.target()
            && std::sync::Arc::ptr_eq(self.activation.handler(), other.activation.handler())
    }
}

/// The link that holds the primary keyboard focus, and the ring it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextLinkFocus {
    /// The link, by its index in [`InlineSpan::links`].
    pub link: usize,
    /// The ring drawn just outside each line of the link's glyphs while the
    /// focus is visible; `None` while it is not (a pointer moved it).
    pub ring: Option<FocusRingStyle>,
}

/// A stretch of a paragraph's text whose spans share one link and one cursor.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SpanRun {
    /// Byte range in the paragraph's plain text.
    range: Range<usize>,
    /// The run's link, by its index in [`InlineSpan::links`].
    pub(super) link: Option<usize>,
    /// The cursor of the innermost span that sets one.
    pub(super) cursor: Option<CursorIcon>,
    /// What assistive technology reads for the run: each span's semantics
    /// label in place of its own text.
    label: String,
}

/// A paragraph's text, cut where its link or its cursor changes.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct SpanRuns {
    runs: Vec<SpanRun>,
    link_count: usize,
    has_cursor: bool,
}

impl SpanRuns {
    /// The runs of `text`, which a paragraph shapes as its plain text; none
    /// when no span sets a link or a cursor, as for most text.
    pub(super) fn of(text: &InlineSpan) -> Self {
        fn interactive(span: &TextSpan) -> bool {
            span.link.is_some()
                || span.mouse_cursor.is_some()
                || span.children.iter().any(interactive)
        }
        let mut runs = Self::default();
        if let InlineSpan::Text(root) = text
            && interactive(root)
        {
            let mut offset = 0;
            runs.walk(root, None, None, &mut offset);
        }
        runs
    }

    fn walk(
        &mut self,
        span: &TextSpan,
        link: Option<usize>,
        cursor: Option<CursorIcon>,
        offset: &mut usize,
    ) {
        // A link its children shadow everywhere covers no text: it takes no
        // index, as `InlineSpan::links` leaves it out.
        let link = if span.link_covers_text() {
            let index = self.link_count;
            self.link_count += 1;
            Some(index)
        } else {
            link
        };
        let cursor = span.mouse_cursor.or(cursor);
        self.has_cursor |= span.mouse_cursor.is_some();
        let text = span.text.as_deref().unwrap_or_default();
        let label = span
            .semantics_label
            .as_deref()
            .or(span.text.as_deref())
            .unwrap_or_default();
        let start = *offset;
        *offset += text.len();
        self.push(start..*offset, link, cursor, label);
        for child in &span.children {
            self.walk(child, link, cursor, offset);
        }
    }

    fn push(
        &mut self,
        range: Range<usize>,
        link: Option<usize>,
        cursor: Option<CursorIcon>,
        label: &str,
    ) {
        if range.is_empty() && label.is_empty() {
            return;
        }
        match self.runs.last_mut() {
            Some(last) if last.link == link && last.cursor == cursor => {
                last.range.end = range.end;
                last.label.push_str(label);
            }
            _ => self.runs.push(SpanRun {
                range,
                link,
                cursor,
                label: label.to_owned(),
            }),
        }
    }

    /// How many spans set a link.
    pub(super) fn link_count(&self) -> usize {
        self.link_count
    }

    /// Whether any span sets a cursor.
    pub(super) fn has_cursor(&self) -> bool {
        self.has_cursor
    }

    /// The run whose glyphs are under `position`, in the painter's committed
    /// layout.
    ///
    /// The painter answers the caret nearest the position; the character the
    /// pointer touches is the one after a downstream caret and the one before
    /// an upstream caret. A position past a line's end or between lines has
    /// a nearest caret too, so the run answers only when one of its boxes
    /// holds the position.
    pub(super) fn run_at(&self, painter: &TextPainter, position: Offset) -> Option<&SpanRun> {
        let caret = painter.get_position_for_offset(position);
        let byte = match caret.affinity {
            TextAffinity::Downstream => caret.offset,
            TextAffinity::Upstream => caret.offset.checked_sub(1)?,
        };
        let run = self
            .runs
            .get(self.runs.partition_point(|run| run.range.end <= byte))?;
        if !run.range.contains(&byte) {
            return None;
        }
        painter
            .get_boxes_for_selection(run.range.start, run.range.end)
            .iter()
            .any(|text_box| text_box.rect.contains(position.to_point()))
            .then_some(run)
    }

    /// The boxes of link `link`'s glyphs in the committed layout, one per
    /// stretch of one direction on one line, in reading order.
    pub(super) fn link_boxes(&self, painter: &TextPainter, link: usize) -> Vec<TextBox> {
        let mut boxes = Vec::new();
        let mut runs = self.runs.iter().peekable();
        while let Some(first) = runs.next() {
            if first.link != Some(link) {
                continue;
            }
            let mut end = first.range.end;
            while let Some(next) = runs.next_if(|next| next.link == Some(link)) {
                end = next.range.end;
            }
            boxes.extend(painter.get_boxes_for_selection(first.range.start, end));
        }
        boxes
    }

    /// The bounds of link `link`'s glyphs in the committed layout; `None`
    /// when it has none (cut by `max_lines` or an ellipsis).
    pub(super) fn link_bounds(&self, painter: &TextPainter, link: usize) -> Option<Rect<f64>> {
        self.link_boxes(painter, link)
            .into_iter()
            .map(|text_box| text_box.rect)
            .reduce(|bounds, rect| bounds.union(&rect))
    }

    /// One accessible node per stretch of text that is a link or is not, in
    /// reading order, bounded by the stretch's glyphs in the committed
    /// layout.
    ///
    /// A link with a route is a focusable [`SemanticsRole::Link`] that
    /// assistive technology clicks and focuses through the route's handler,
    /// focused while it is `focused`. A link without one is read as plain
    /// text: nothing could run it. A stretch whose glyphs `max_lines` or an
    /// ellipsis cut publishes nothing, as it paints nothing. A stretch that
    /// is only a semantics label (a textless span's) has no glyphs of its
    /// own: it is bounded by the paragraph, `bounds`, so it is still read in
    /// its place.
    pub(super) fn semantics_regions(
        &self,
        painter: &TextPainter,
        routes: &[TextLinkRoute],
        focused: Option<usize>,
        direction: Option<TextDirection>,
        bounds: Rect<f64>,
    ) -> Vec<SemanticsRegion> {
        let mut regions = Vec::new();
        // A link that encloses another one is cut in two by it, so link
        // regions are counted like the text between them, not keyed by link.
        let (mut text_ordinal, mut link_ordinal) = (0, 0);
        let mut runs = self.runs.iter().peekable();
        while let Some(first) = runs.next() {
            let mut range = first.range.clone();
            let mut label = first.label.clone();
            while let Some(next) = runs.next_if(|next| next.link == first.link) {
                range.end = next.range.end;
                label.push_str(&next.label);
            }
            let routed = first.link.and_then(|link| Some((link, routes.get(link)?)));
            let ordinal = if routed.is_some() {
                &mut link_ordinal
            } else {
                &mut text_ordinal
            };
            let key = region_key(*ordinal, routed.is_some());
            *ordinal += 1;
            let glyphs = painter
                .get_boxes_for_selection(range.start, range.end)
                .iter()
                .map(|text_box| text_box.rect)
                .reduce(|glyphs, rect| glyphs.union(&rect));
            let rect = match glyphs {
                Some(glyphs) => glyphs,
                None if range.is_empty() => bounds,
                None => continue,
            };
            if label.is_empty() && routed.is_none() {
                continue;
            }
            let mut config = SemanticsConfiguration::new();
            if !label.is_empty() {
                config.set_label(label);
            }
            if let Some(direction) = direction {
                config.set_text_direction(direction);
            }
            if let Some((link, route)) = routed {
                let handler = route.activation().handler();
                config.set_role(SemanticsRole::Link);
                config.set_focusable(true);
                config.set_focused(focused == Some(link));
                config.add_action(SemanticsAction::Click, std::sync::Arc::clone(handler));
                config.add_action(SemanticsAction::Focus, std::sync::Arc::clone(handler));
            }
            regions.push(SemanticsRegion::new(key, rect, config));
        }
        regions
    }
}

/// A region's identity: its order among the paragraph's link regions or
/// among the text regions between them, in two key spaces.
fn region_key(ordinal: usize, link: bool) -> SemanticsRegionKey {
    let ordinal =
        u64::try_from(ordinal).map_or(u64::MAX >> 1, |ordinal| ordinal.min(u64::MAX >> 1));
    SemanticsRegionKey(ordinal << 1 | u64::from(link))
}
