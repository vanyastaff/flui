//! [`CompositionLedger`]: what a document's composition stands for in its
//! committed text (ADR-0090 amendment item 1).
//!
//! The committed text is the document with the composing range replaced by
//! the text that occupied that range before the composition began: nothing
//! for a new preedit, the reconverted words when an input method marks text
//! the user already committed. A store keeps that text, the composition's
//! *origin*, beside its composing range, and records a session's edits here
//! so the origin follows them. Offsets are byte offsets into the store's
//! UTF-8 text, on `char` boundaries the store has already checked.
//!
//! The ledger follows the composition rules every store keeps (an edit
//! before the composition shifts it, one after it leaves it, one that
//! overlaps it or inserts strictly inside it clears it; ADR-0090
//! "Divergences"), so its composing range is the store's.

use std::ops::Range;

/// A region of the document whose committed text is `origin`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Standing {
    range: Range<usize>,
    origin: String,
}

/// One read-write session's account of the composition's origin.
///
/// Open it with the composition the store holds when the lock opens, call
/// [`Self::replace`] before applying each edit and
/// [`Self::set_composition`] for each change of the composing range, then
/// keep [`Self::origin`] with the store's composition. [`Self::committed`]
/// is the committed text at any point.
///
/// Text inserted during the session outside any composition is new: marking
/// it as the composition later in the same session (an input method inserts
/// its preedit, then marks it) gives an empty origin, while marking text the
/// session found in the document is a reconversion and keeps that text as
/// the origin. A composition an edit cleared keeps its origin for the rest
/// of the session, so an input method that rewrites its composition and
/// marks it again does not lose what it stands for. Narrowing a composition
/// commits the text it no longer covers, and the origin drops that text
/// where it begins or ends with it.
#[derive(Clone, Debug, Default)]
pub struct CompositionLedger {
    composition: Option<Standing>,
    /// A composition an edit cleared this session, waiting to be marked again.
    cleared: Option<Standing>,
    /// Text this session inserted outside any composition.
    inserted: Vec<Range<usize>>,
}

impl CompositionLedger {
    /// A session over a document whose composition is `composition`: its
    /// byte range and its origin.
    #[must_use]
    pub fn open(composition: Option<(Range<usize>, String)>) -> Self {
        Self {
            composition: composition.map(|(range, origin)| Standing { range, origin }),
            cleared: None,
            inserted: Vec::new(),
        }
    }

    /// Record that `edit` in `text` (the text before the edit) is about to
    /// be replaced by `inserted` bytes.
    pub fn replace(&mut self, text: &str, edit: Range<usize>, inserted: usize) {
        let composition = self.composition.take();
        let cleared = self.cleared.take();
        let hits = |standing: &Option<Standing>| {
            standing
                .as_ref()
                .is_some_and(|standing| clears(&edit, &standing.range))
        };
        let (hit_composition, hit_cleared) = (hits(&composition), hits(&cleared));
        if !hit_composition && !hit_cleared {
            self.composition = composition.map(|standing| shifted(standing, &edit, inserted));
            self.cleared = cleared.map(|standing| shifted(standing, &edit, inserted));
            self.record_insertion(&edit, inserted);
            return;
        }
        let hit: Vec<&Standing> = [
            composition.as_ref().filter(|_| hit_composition),
            cleared.as_ref().filter(|_| hit_cleared),
        ]
        .into_iter()
        .flatten()
        .collect();
        let start = hit
            .iter()
            .map(|s| s.range.start)
            .fold(edit.start, usize::min);
        let end = hit.iter().map(|s| s.range.end).fold(edit.end, usize::max);
        let origin = self.view(text, start..end, &hit);
        let new_end = end - edit.len() + inserted;
        self.inserted = outside(std::mem::take(&mut self.inserted), start..end)
            .map(|range| shift(range, &edit, inserted))
            .collect();
        self.cleared = Some(Standing {
            range: start..new_end,
            origin,
        });
        if !hit_composition {
            self.composition = composition.map(|standing| shifted(standing, &edit, inserted));
        }
    }

    /// Record that the composing range is now `range` in `text`, or that
    /// there is no composition.
    pub fn set_composition(&mut self, text: &str, range: Option<Range<usize>>) {
        let Some(range) = range else {
            self.composition = None;
            self.cleared = None;
            return;
        };
        let composition = self.composition.take();
        let cleared = self.cleared.take();
        // Two non-empty ranges share an origin only when they overlap: a
        // composition moved to the text beside it stands for that text
        // alone. An empty range has no text to overlap, so it joins one it
        // touches (an input method marking a caret inside or at the edge of
        // a composition it rewrites).
        let touches = |standing: &Option<Standing>| {
            standing.as_ref().is_some_and(|standing| {
                let near = &standing.range;
                if near.is_empty() || range.is_empty() {
                    near.start <= range.end && near.end >= range.start
                } else {
                    near.start < range.end && near.end > range.start
                }
            })
        };
        let (hit_composition, hit_cleared) = (touches(&composition), touches(&cleared));
        let hit: Vec<&Standing> = [
            composition.as_ref().filter(|_| hit_composition),
            cleared.as_ref().filter(|_| hit_cleared),
        ]
        .into_iter()
        .flatten()
        .collect();
        let start = hit
            .iter()
            .map(|s| s.range.start)
            .fold(range.start, usize::min);
        let end = hit.iter().map(|s| s.range.end).fold(range.end, usize::max);
        let origin = rebased(
            self.view(text, start..end, &hit),
            &text[start..range.start],
            &text[range.end..end],
        );
        self.inserted = outside(std::mem::take(&mut self.inserted), start..end).collect();
        if !hit_cleared {
            self.cleared = cleared;
        }
        self.composition = Some(Standing { range, origin });
    }

    /// The origin of the current composition: what it stands for in the
    /// committed text. `None` without a composition.
    #[must_use]
    pub fn origin(&self) -> Option<&str> {
        self.composition
            .as_ref()
            .map(|standing| standing.origin.as_str())
    }

    /// `text` with the composing range replaced by its origin.
    #[must_use]
    pub fn committed(&self, text: &str) -> String {
        match &self.composition {
            Some(standing) => {
                committed_text(text, Some((standing.range.clone(), &standing.origin)))
            }
            None => text.to_owned(),
        }
    }

    /// An edit outside every composition: it is committed, and the text it
    /// inserts is new to this session.
    fn record_insertion(&mut self, edit: &Range<usize>, inserted: usize) {
        let mut start = edit.start;
        let mut end = edit.start + inserted;
        let mut kept = Vec::with_capacity(self.inserted.len() + 1);
        for range in std::mem::take(&mut self.inserted) {
            if range.end < edit.start || range.start > edit.end {
                kept.push(shift(range, edit, inserted));
            } else {
                start = start.min(range.start);
                let tail = if range.end > edit.end {
                    range.end - edit.len() + inserted
                } else {
                    edit.start + inserted
                };
                end = end.max(tail);
            }
        }
        if end > start {
            kept.push(start..end);
        }
        self.inserted = kept;
    }

    /// The committed text of `span` in `text`: each of `standing` (all
    /// inside `span`) replaced by its origin, and text this session inserted
    /// left out.
    fn view(&self, text: &str, span: Range<usize>, standing: &[&Standing]) -> String {
        let mut holes: Vec<(usize, usize, &str)> = standing
            .iter()
            .map(|s| (s.range.start, s.range.end, s.origin.as_str()))
            .collect();
        for range in &self.inserted {
            let (start, end) = (range.start.max(span.start), range.end.min(span.end));
            if start < end {
                holes.push((start, end, ""));
            }
        }
        holes.sort_unstable_by_key(|hole| hole.0);
        let mut view = String::new();
        let mut at = span.start;
        for (start, end, origin) in holes {
            if start < at {
                continue;
            }
            view.push_str(&text[at..start]);
            view.push_str(origin);
            at = end;
        }
        view.push_str(&text[at..span.end]);
        view
    }
}

/// `text` with `composing` — a byte range and its origin — replaced by the
/// origin: the committed text of a document a store holds.
#[must_use]
pub fn committed_text(text: &str, composing: Option<(Range<usize>, &str)>) -> String {
    match composing {
        Some((range, origin)) => {
            let mut committed = String::with_capacity(text.len() - range.len() + origin.len());
            committed.push_str(&text[..range.start]);
            committed.push_str(origin);
            committed.push_str(&text[range.end..]);
            committed
        }
        None => text.to_owned(),
    }
}

/// The origin of a composition that no longer covers `before` and `after`,
/// the text on either side of it that the composition it replaces spanned
/// (`origin` is the committed text of that whole span). That text leaves the
/// composition as committed text, so where `origin` begins or ends with it,
/// the composition no longer stands for it: narrowing a reconversion of
/// "abcdef" to "abc" leaves "abc". Text the origin does not account for (an
/// input method narrowing a conversion to commit part of its reading) stays
/// committed beside the whole origin.
fn rebased(origin: String, before: &str, after: &str) -> String {
    let inner = origin.strip_prefix(before).unwrap_or(&origin);
    let inner = inner.strip_suffix(after).unwrap_or(inner);
    if inner.len() == origin.len() {
        origin
    } else {
        inner.to_owned()
    }
}

/// The parts of `ranges` outside `span`: a range `span` cuts keeps what lies
/// before and after it, so text a session inserted and only partly marked
/// stays new where it was not marked.
fn outside(ranges: Vec<Range<usize>>, span: Range<usize>) -> impl Iterator<Item = Range<usize>> {
    ranges.into_iter().flat_map(move |range| {
        [
            range.start..range.end.min(span.start),
            range.start.max(span.end)..range.end,
        ]
        .into_iter()
        .filter(|part| part.start < part.end)
    })
}

/// Whether `edit` clears a composition over `range`: it neither ends at or
/// before the composition nor starts at or after it.
fn clears(edit: &Range<usize>, range: &Range<usize>) -> bool {
    !(edit.end <= range.start || edit.start >= range.end)
}

/// `range`, untouched by `edit`, after `edit` is replaced by `inserted` bytes.
fn shift(range: Range<usize>, edit: &Range<usize>, inserted: usize) -> Range<usize> {
    if range.start >= edit.end {
        range.start - edit.len() + inserted..range.end - edit.len() + inserted
    } else {
        range
    }
}

fn shifted(standing: Standing, edit: &Range<usize>, inserted: usize) -> Standing {
    Standing {
        range: shift(standing.range, edit, inserted),
        ..standing
    }
}
