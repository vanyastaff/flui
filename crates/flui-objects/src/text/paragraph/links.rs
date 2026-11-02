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
use std::rc::Rc;

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
    key: TextLinkKey,
    pointer: PointerTarget,
    activation: SemanticsActionRoute,
}

/// A link's identity across rebuilds, chosen by whoever routes it.
///
/// It keys the link's accessible node, so assistive technology keeps its
/// place on a link when links are added or removed before it. The router
/// gives each link it keeps across a rebuild the same key, and no two links
/// of one paragraph the same key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextLinkKey(pub u64);

impl TextLinkRoute {
    /// Pair a link's identity with its pointer target and activation route.
    #[must_use]
    pub fn new(key: TextLinkKey, pointer: PointerTarget, activation: SemanticsActionRoute) -> Self {
        Self {
            key,
            pointer,
            activation,
        }
    }

    /// The link's identity across rebuilds.
    #[must_use]
    pub fn key(&self) -> TextLinkKey {
        self.key
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
        self.key == other.key
            && self.pointer == other.pointer
            && self.activation.target() == other.activation.target()
            && std::rc::Rc::ptr_eq(self.activation.handler(), other.activation.handler())
    }
}

/// Told, after a layout that changed it, which of a paragraph's links have
/// glyphs: one flag per link, in [`InlineSpan::links`] order. A link that
/// `max_lines` or an ellipsis cut has none, so nothing should focus it.
///
/// It runs inside layout, so it records the flags and schedules work; it
/// does not change the tree.
#[derive(Clone)]
pub struct ShownLinksListener(ShownLinksCallback);

/// What a [`ShownLinksListener`] runs: the flags, one per link.
type ShownLinksCallback = Rc<dyn Fn(&[bool])>;

impl ShownLinksListener {
    /// A listener that runs `listener` with the flags.
    #[must_use]
    pub fn new(listener: impl Fn(&[bool]) + 'static) -> Self {
        Self(Rc::new(listener))
    }

    pub(super) fn notify(&self, shown: &[bool]) {
        (self.0)(shown);
    }

    /// Whether `other` is this same listener.
    pub(super) fn same(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for ShownLinksListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShownLinksListener").finish_non_exhaustive()
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
    /// The span whose accessibility override replaces the run's text, by
    /// its index among the spans that set one, in document order; `None`
    /// for text read as written.
    overridden: Option<usize>,
    /// What assistive technology reads for the run: each span's semantics
    /// label in place of its own text.
    label: String,
}

/// A paragraph's text, cut where its link or its cursor changes.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct SpanRuns {
    runs: Vec<SpanRun>,
    link_count: usize,
    override_count: usize,
    has_cursor: bool,
}

impl SpanRuns {
    /// The runs of `text`, which a paragraph shapes as its plain text; none
    /// when no span sets a link, a cursor or an accessibility override, as
    /// for most text.
    pub(super) fn of(text: &InlineSpan) -> Self {
        fn interactive(span: &TextSpan) -> bool {
            span.link.is_some()
                || span.mouse_cursor.is_some()
                || span.semantics_label.is_some()
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
        // An override replaces the span's own text only: its children keep
        // their own reading.
        let overridden = span.semantics_label.is_some().then(|| {
            self.override_count += 1;
            self.override_count - 1
        });
        let start = *offset;
        *offset += text.len();
        self.push(start..*offset, link, cursor, overridden, label);
        for child in &span.children {
            self.walk(child, link, cursor, offset);
        }
    }

    fn push(
        &mut self,
        range: Range<usize>,
        link: Option<usize>,
        cursor: Option<CursorIcon>,
        overridden: Option<usize>,
        label: &str,
    ) {
        if range.is_empty() && label.is_empty() {
            return;
        }
        match self.runs.last_mut() {
            Some(last)
                if last.link == link && last.cursor == cursor && last.overridden == overridden =>
            {
                last.range.end = range.end;
                last.label.push_str(label);
            }
            _ => self.runs.push(SpanRun {
                range,
                link,
                cursor,
                overridden,
                label: label.to_owned(),
            }),
        }
    }

    /// How many spans set an accessibility override.
    pub(super) fn override_count(&self) -> usize {
        self.override_count
    }

    /// How many spans set a link.
    pub(super) fn link_count(&self) -> usize {
        self.link_count
    }

    /// Whether any span sets a cursor.
    pub(super) fn has_cursor(&self) -> bool {
        self.has_cursor
    }

    /// The bytes where the text enters or leaves a link or an overridden
    /// span, in order: where a selectable paragraph's runs are cut, so each
    /// region holds its own.
    pub(super) fn region_edges(&self) -> Vec<usize> {
        self.runs
            .windows(2)
            .filter(|pair| pair[0].link != pair[1].link || pair[0].overridden != pair[1].overridden)
            .map(|pair| pair[1].range.start)
            .collect()
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

    /// Whether each link has glyphs in the committed layout, in link order.
    /// Only a layout that `truncated` its text can have cut one.
    pub(super) fn shown_links(&self, painter: &TextPainter, truncated: bool) -> Vec<bool> {
        (0..self.link_count)
            .map(|link| !truncated || !self.link_boxes(painter, link).is_empty())
            .collect()
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
    /// its place. A selectable paragraph passes its runs' bytes as
    /// `run_ranges`, and each region holds the runs in its bytes ([`host_runs`]);
    /// each span whose accessibility override replaces its text is then a
    /// region of its own outside a link, hidden when the override silences it.
    pub(super) fn semantics_regions(
        &self,
        painter: &TextPainter,
        routes: &[TextLinkRoute],
        focused: Option<usize>,
        direction: Option<TextDirection>,
        bounds: Rect<f64>,
        run_ranges: &[Range<usize>],
    ) -> Vec<SemanticsRegion> {
        let mut regions = Vec::new();
        let mut spans = Vec::new();
        let (mut text_ordinal, mut continued_ordinal) = (0, 0);
        let mut keyed = vec![false; routes.len()];
        // Hosting runs, each overridden span outside a link is a region of
        // its own: its override is its reading, its runs its authored text.
        let hosting = !run_ranges.is_empty();
        let same_region = |first: &SpanRun, next: &SpanRun| {
            next.link == first.link
                && (!hosting || first.link.is_some() || next.overridden == first.overridden)
        };
        let mut runs = self.runs.iter().peekable();
        while let Some(first) = runs.next() {
            let mut range = first.range.clone();
            let mut label = first.label.clone();
            while let Some(next) = runs.next_if(|next| same_region(first, next)) {
                range.end = next.range.end;
                label.push_str(&next.label);
            }
            let routed = first.link.and_then(|link| Some((link, routes.get(link)?)));
            // A link keeps its router's key; a link that encloses another one
            // is cut in two by it, and its later part is counted instead.
            let key = match routed {
                Some((link, route)) if !std::mem::replace(&mut keyed[link], true) => {
                    RegionIdentity::Link(route.key())
                }
                Some(_) => {
                    continued_ordinal += 1;
                    RegionIdentity::LinkContinued(continued_ordinal - 1)
                }
                None => {
                    text_ordinal += 1;
                    RegionIdentity::Text(text_ordinal - 1)
                }
            }
            .key();
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
            // A silenced span reads as nothing; hosting runs, it still holds
            // its characters, hidden, so a selection across it covers them.
            let silenced = label.is_empty() && routed.is_none();
            if silenced && !(hosting && first.overridden.is_some()) {
                continue;
            }
            let mut config = SemanticsConfiguration::new();
            if silenced {
                config.set_hidden(true);
            } else if !label.is_empty() {
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
                config.add_action(SemanticsAction::Click, std::rc::Rc::clone(handler));
                config.add_action(SemanticsAction::Focus, std::rc::Rc::clone(handler));
            }
            spans.push(range);
            regions.push(SemanticsRegion::new(key, rect, config));
        }
        if !run_ranges.is_empty() {
            host_runs(&mut regions, &spans, run_ranges);
        }
        regions
    }
}

/// What identifies an accessible region of a paragraph, in three key spaces.
enum RegionIdentity {
    /// The text between links, by its order.
    Text(usize),
    /// A link, by its router's key.
    Link(TextLinkKey),
    /// The part of a link after a link it encloses, by its order.
    LinkContinued(usize),
}

impl RegionIdentity {
    fn key(self) -> SemanticsRegionKey {
        const VALUE_LIMIT: u64 = u64::MAX >> 2;
        let ordinal = |ordinal: usize| u64::try_from(ordinal).unwrap_or(VALUE_LIMIT);
        let (value, space) = match self {
            Self::Text(ordinal_in_text) => (ordinal(ordinal_in_text), 0),
            Self::Link(key) => (key.0, 1),
            Self::LinkContinued(ordinal_in_links) => (ordinal(ordinal_in_links), 2),
        };
        SemanticsRegionKey(value.min(VALUE_LIMIT) << 2 | space)
    }
}

/// Gives each region the paragraph's runs that lie in its bytes, when every
/// run lies whole in one and each region's are consecutive in run order:
/// `run_ranges` holds each run's bytes, `spans` each region's, in order.
/// Otherwise the regions hold none and the paragraph keeps its runs: in
/// bidirectional text whose runs leave byte order, where a run crosses a
/// link edge (a grapheme cluster a link starts inside), or where a region
/// publishes nothing.
fn host_runs(regions: &mut [SemanticsRegion], spans: &[Range<usize>], run_ranges: &[Range<usize>]) {
    let Some(last) = spans.len().checked_sub(1) else {
        return;
    };
    let region_of = |run: &Range<usize>| {
        spans
            .iter()
            .position(|span| span.contains(&run.start) && run.end <= span.end)
            // An empty run at the text's end belongs to the last region.
            .or_else(|| (run.is_empty() && run.start >= spans[last].end).then_some(last))
    };
    let owners: Option<Vec<usize>> = run_ranges.iter().map(region_of).collect();
    let Some(owners) = owners else {
        return;
    };
    // Consecutive in run order, region by region, so each holds a range.
    if owners.windows(2).any(|pair| pair[1] < pair[0]) {
        return;
    }
    for (index, region) in regions.iter_mut().enumerate() {
        let first = owners.partition_point(|&owner| owner < index);
        let end = owners.partition_point(|&owner| owner <= index);
        region.config.set_hosted_runs(Some(first..end));
    }
}
