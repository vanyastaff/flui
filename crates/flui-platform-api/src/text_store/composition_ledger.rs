//! [`CompositionLedger`]: what a document's composition stands for in its
//! committed text (ADR-0142 item 1).
//!
//! The committed text is the document with the composing range replaced by
//! what that range stands for: nothing for a new preedit, the reconverted
//! words when an input method marks text the user already committed. A
//! store keeps that text, the composition's *origin*, beside its composing
//! range, and records a session's edits here so the origin follows them.
//! Offsets are byte offsets into the store's UTF-8 text, on `char`
//! boundaries the store has already checked; the ledger keeps lengths, and
//! reads the text the store passes when it needs characters.
//!
//! # The model
//!
//! The ledger keeps the session's document as runs, each knowing what it
//! stands for in the committed text:
//!
//! - **committed** text stands for itself: text the document held when the
//!   session opened outside any composition, or that a composition
//!   committed;
//! - **preedit** stands for nothing: text the session inserted;
//! - a **removal** shows nothing and stands for the committed text an edit
//!   removed from a composition (or from a composition an edit cleared);
//! - a **marker** shows and stands for nothing: it is where an empty
//!   composition lies.
//!
//! The committed text is the document with the composition replaced by what
//! its runs stand for. A composition an edit cleared keeps its runs as they
//! are, so an input method that rewrites its composition and marks it again
//! does not lose what it stood for. Text outside every composition is one
//! run per kind, whatever its length: a session over a long document costs
//! what its compositions do.
//!
//! The text one edit inserted, with the removal it left, is one
//! *replacement*: together they stand for the removed text, and no part of
//! the removed text belongs to any one inserted character. Replacements that
//! rewrote each other's text are one replacement.
//!
//! # Narrowing
//!
//! When a mark leaves part of a composition (or of a cleared one) outside the
//! new composing range, that part commits as the user sees it: its text
//! stands for itself and its removals go. What remains keeps what its runs
//! stand for. A removal goes with its own replacement's inserted text; a
//! removal whose replacement inserted nothing (a deletion) stays only
//! strictly inside the new range. A replacement whose inserted text lies
//! both inside and outside the new range is split; if it removed text, that
//! text cannot be divided between the two parts, and the region's remaining
//! text then stands for itself, so the committed text over it is what the
//! user sees. The removed text of one replacement is never counted beside
//! any of its inserted text.
//!
//! The ledger follows the composition rules every store keeps (an edit
//! before the composition shifts it, one after it leaves it, one that
//! overlaps it or inserts strictly inside it clears it; ADR-0090
//! "Divergences"), so its composing range is the store's.

use std::ops::Range;

/// A composition, or a composition an edit cleared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RegionId(usize);

/// One edit's inserted text and its removal (see the module doc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ReplacementId(usize);

/// What one run of the session's document is.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Content {
    /// This many bytes of text that stand for themselves.
    Committed(usize),
    /// This many bytes the session inserted, which stand for nothing.
    Preedit(usize),
    /// Committed text an edit removed: shows nothing, stands for the text.
    Removed(String),
    /// Where an empty region lies: shows and stands for nothing.
    Marker,
}

impl Content {
    /// How many bytes of the document the run shows.
    fn shown(&self) -> usize {
        match self {
            Self::Committed(len) | Self::Preedit(len) => *len,
            Self::Removed(_) | Self::Marker => 0,
        }
    }
}

/// One run of the document, and what it belongs to.
#[derive(Clone, Debug)]
struct Run {
    content: Content,
    region: Option<RegionId>,
    replacement: Option<ReplacementId>,
}

impl Run {
    fn plain(content: Content) -> Self {
        Self {
            content,
            region: None,
            replacement: None,
        }
    }

    /// What the run stands for, read from `text` where it shows at `at`.
    fn stands_for(&self, text: &str, at: usize, into: &mut String) {
        match &self.content {
            Content::Committed(len) => into.push_str(&text[at..at + len]),
            Content::Removed(removed) => into.push_str(removed),
            Content::Preedit(_) | Content::Marker => {}
        }
    }
}

/// One read-write session's account of the composition's origin.
///
/// Open it with the document and the composition the store holds when the
/// lock opens, call [`Self::replace`] before applying each edit and
/// [`Self::set_composition`] for each change of the composing range, then
/// keep [`Self::origin`] with the store's composition. [`Self::committed`]
/// is the committed text at any point. See the module doc for the model.
#[derive(Clone, Debug, Default)]
pub struct CompositionLedger {
    runs: Vec<Run>,
    /// Regions that keep what they stand for: the composition and those an
    /// edit cleared.
    regions: Vec<RegionId>,
    composition: Option<RegionId>,
    next_region: usize,
    /// Each replacement's parent in the sets of replacements that rewrote
    /// each other's text.
    replacements: Vec<usize>,
}

impl CompositionLedger {
    /// A session over `text`, whose composition is `composition`: its byte
    /// range and its origin.
    ///
    /// A composition whose origin is its visible text stands for itself, one
    /// with an empty origin is preedit, and any other is one replacement of
    /// its origin by its visible text.
    #[must_use]
    pub fn open(text: &str, composition: Option<(Range<usize>, String)>) -> Self {
        let mut ledger = Self::default();
        let Some((range, origin)) = composition else {
            ledger.push_plain(Content::Committed(text.len()));
            return ledger;
        };
        ledger.push_plain(Content::Committed(range.start));
        let region = ledger.new_region();
        let shown = &text[range.clone()];
        let member = |content, replacement| Run {
            content,
            region: Some(region),
            replacement,
        };
        if origin.is_empty() {
            if shown.is_empty() {
                ledger.runs.push(member(Content::Marker, None));
            } else {
                ledger
                    .runs
                    .push(member(Content::Preedit(shown.len()), None));
            }
        } else if origin == shown {
            ledger
                .runs
                .push(member(Content::Committed(shown.len()), None));
        } else {
            let replacement = Some(ledger.new_replacement());
            ledger
                .runs
                .push(member(Content::Removed(origin), replacement));
            ledger
                .runs
                .push(member(Content::Preedit(shown.len()), replacement));
        }
        ledger.push_plain(Content::Committed(text.len() - range.end));
        ledger.normalize();
        ledger.regions.push(region);
        ledger.composition = Some(region);
        ledger
    }

    /// Record that `edit`, a byte range of `text` (the text before the
    /// edit), is about to be replaced by `inserted`.
    pub fn replace(&mut self, text: &str, edit: Range<usize>, inserted: &str) {
        self.cut(edit.start);
        self.cut(edit.end);
        let at = self.positions();
        let hit: Vec<RegionId> = self
            .regions
            .iter()
            .copied()
            .filter(|&region| clears(&edit, &self.extent(region, &at)))
            .collect();
        if hit.is_empty() {
            self.replace_outside(&edit, inserted.len(), &at);
            return;
        }
        let mut span = edit.clone();
        for &region in &hit {
            let extent = self.extent(region, &at);
            span.start = span.start.min(extent.start);
            span.end = span.end.max(extent.end);
        }
        let member = |index: usize, run: &Run| {
            if run.content.shown() > 0 {
                span.contains(&at[index])
            } else {
                run.region.is_some_and(|region| hit.contains(&region))
                    || (at[index] > span.start && at[index] < span.end)
            }
        };
        let removed_by_edit = |index: usize, run: &Run| {
            if run.content.shown() > 0 {
                edit.contains(&at[index])
            } else {
                at[index] > edit.start && at[index] < edit.end
            }
        };
        // First pass: what the edit removes, the replacements it rewrites,
        // and where its own replacement goes.
        let mut removed = String::new();
        let mut rewritten = Vec::new();
        let mut kept_member = false;
        let mut insert_at = None;
        let mut after_members = 0;
        for (index, run) in self.runs.iter().enumerate() {
            if !member(index, run) {
                continue;
            }
            after_members = index + 1;
            if removed_by_edit(index, run) {
                run.stands_for(text, at[index], &mut removed);
                rewritten.extend(run.replacement);
                continue;
            }
            kept_member |= run.content != Content::Marker;
            let past_edit = if run.content.shown() > 0 {
                at[index] >= edit.start
            } else {
                at[index] >= edit.end
            };
            if past_edit && insert_at.is_none() {
                insert_at = Some(index);
            }
        }
        let insert_at = insert_at.unwrap_or(after_members);
        let region = self.new_region();
        let replacement = self.new_replacement();
        for other in rewritten {
            self.join(replacement, other);
        }
        let part = |content| Run {
            content,
            region: Some(region),
            replacement: Some(replacement),
        };
        let mut insertion = Vec::with_capacity(2);
        if !removed.is_empty() {
            insertion.push(part(Content::Removed(removed)));
        }
        if !inserted.is_empty() {
            insertion.push(part(Content::Preedit(inserted.len())));
        }
        if insertion.is_empty() && !kept_member {
            insertion.push(Run {
                content: Content::Marker,
                region: Some(region),
                replacement: None,
            });
        }
        let mut runs = Vec::with_capacity(self.runs.len() + insertion.len());
        let mut insertion = Some(insertion);
        for (index, run) in std::mem::take(&mut self.runs).into_iter().enumerate() {
            if index == insert_at {
                runs.extend(insertion.take().into_iter().flatten());
            }
            if !member(index, &run) {
                runs.push(run);
            } else if !removed_by_edit(index, &run) && run.content != Content::Marker {
                runs.push(Run {
                    region: Some(region),
                    ..run
                });
            }
        }
        runs.extend(insertion.into_iter().flatten());
        self.runs = runs;
        self.regions.retain(|live| !hit.contains(live));
        if self
            .composition
            .is_some_and(|composition| hit.contains(&composition))
        {
            self.composition = None;
        }
        self.regions.push(region);
        self.normalize();
    }

    /// Record that the composing range is now `range`, or that there is no
    /// composition.
    pub fn set_composition(&mut self, range: Option<Range<usize>>) {
        let Some(range) = range else {
            for region in std::mem::take(&mut self.regions) {
                self.commit(region);
            }
            self.composition = None;
            self.normalize();
            return;
        };
        let at = self.positions();
        let touched: Vec<RegionId> = self
            .regions
            .iter()
            .copied()
            .filter(|&region| touches(&self.extent(region, &at), &range))
            .collect();
        if let Some(composition) = self.composition.take()
            && !touched.contains(&composition)
        {
            self.commit(composition);
            self.regions.retain(|&live| live != composition);
        }
        self.cut(range.start);
        self.cut(range.end);
        for &region in &touched {
            let extent = self.extent(region, &self.positions());
            if extent.start < range.start || extent.end > range.end {
                self.narrow(region, &range);
            }
        }
        self.gather(&touched, &range);
        let at = self.positions();
        let composition = self.new_region();
        let mut inside = false;
        for (index, run) in self.runs.iter_mut().enumerate() {
            let joins = if run.content.shown() > 0 {
                range.contains(&at[index])
            } else {
                run.region.is_some_and(|region| touched.contains(&region))
            };
            if joins {
                run.region = Some(composition);
                inside = true;
            }
        }
        if !inside {
            let index = (0..self.runs.len())
                .find(|&index| at[index] >= range.start)
                .unwrap_or(self.runs.len());
            self.runs.insert(
                index,
                Run {
                    content: Content::Marker,
                    region: Some(composition),
                    replacement: None,
                },
            );
        }
        self.regions.retain(|live| !touched.contains(live));
        self.regions.push(composition);
        self.composition = Some(composition);
        self.normalize();
    }

    /// The origin of the current composition in `text`: what it stands for
    /// in the committed text. `None` without a composition.
    #[must_use]
    pub fn origin(&self, text: &str) -> Option<String> {
        let composition = self.composition?;
        let mut origin = String::new();
        let mut at = 0;
        for run in &self.runs {
            if run.region == Some(composition) {
                run.stands_for(text, at, &mut origin);
            }
            at += run.content.shown();
        }
        Some(origin)
    }

    /// The committed text of `text`: the text with the composition replaced
    /// by what it stands for.
    #[must_use]
    pub fn committed(&self, text: &str) -> String {
        let composing = self.composition.map(|composition| {
            (
                self.extent(composition, &self.positions()),
                self.origin(text).unwrap_or_default(),
            )
        });
        committed_text(
            text,
            composing
                .as_ref()
                .map(|(range, origin)| (range.clone(), origin.as_str())),
        )
    }

    /// An edit that touches no region: what it removes leaves the committed
    /// text, and what it inserts is preedit, placed so every region stays
    /// where a store keeps its composition.
    fn replace_outside(&mut self, edit: &Range<usize>, inserted: usize, at: &[usize]) {
        let starts: Vec<(RegionId, usize)> = self
            .regions
            .iter()
            .map(|&region| (region, self.extent(region, at).start))
            .collect();
        let region_start = |region: Option<RegionId>| {
            starts
                .iter()
                .find(|(live, _)| Some(*live) == region)
                .map_or(0, |&(_, start)| start)
        };
        let mut runs = Vec::with_capacity(self.runs.len() + 1);
        let mut insertion = (inserted > 0).then(|| Run::plain(Content::Preedit(inserted)));
        for (index, run) in std::mem::take(&mut self.runs).into_iter().enumerate() {
            let shown = run.content.shown();
            // The insertion goes before a zero-width run only when the run's
            // region lies wholly at or after the edit, as a store shifts a
            // composition the edit ends at.
            let past_edit = if shown > 0 {
                at[index] >= edit.start
            } else {
                at[index] >= edit.end && region_start(run.region) >= edit.end
            };
            if past_edit && let Some(insertion) = insertion.take() {
                runs.push(insertion);
            }
            if shown > 0 && edit.contains(&at[index]) {
                continue;
            }
            runs.push(run);
        }
        runs.extend(insertion);
        self.runs = runs;
        self.normalize();
    }

    /// A mark leaves the part of `region` outside `range`: it commits as
    /// shown (module doc, "Narrowing"). `range`'s ends are run boundaries.
    fn narrow(&mut self, region: RegionId, range: &Range<usize>) {
        let at = self.positions();
        // Each replacement's shown text, inside and outside the range.
        let mut sides: Vec<(usize, bool, bool)> = Vec::new();
        for (index, run) in self.runs.iter().enumerate() {
            if run.region != Some(region) || run.content.shown() == 0 {
                continue;
            }
            if let Some(replacement) = run.replacement {
                let root = self.root(replacement);
                let inside = range.contains(&at[index]);
                match sides.iter_mut().find(|(seen, ..)| *seen == root) {
                    Some((_, ins, outs)) => {
                        *ins |= inside;
                        *outs |= !inside;
                    }
                    None => sides.push((root, inside, !inside)),
                }
            }
        }
        let side_of = |replacement: Option<ReplacementId>| {
            replacement.and_then(|replacement| {
                let root = self.root(replacement);
                sides
                    .iter()
                    .find(|(seen, ..)| *seen == root)
                    .map(|&(_, ins, outs)| (ins, outs))
            })
        };
        let split = self.runs.iter().any(|run| {
            run.region == Some(region)
                && matches!(&run.content, Content::Removed(text) if !text.is_empty())
                && side_of(run.replacement).is_some_and(|(ins, outs)| ins && outs)
        });
        let leaves = |index: usize, run: &Run| {
            if run.content.shown() > 0 {
                return !range.contains(&at[index]);
            }
            match side_of(run.replacement) {
                // A removal goes with its replacement's text.
                Some((_, outs)) => outs,
                // A deletion's removal, or a marker, stays only strictly
                // inside the range.
                None => !(at[index] > range.start && at[index] < range.end),
            }
        };
        let leaving: Vec<bool> = self
            .runs
            .iter()
            .enumerate()
            .map(|(index, run)| leaves(index, run))
            .collect();
        let mut runs = Vec::with_capacity(self.runs.len());
        for (run, leaving) in std::mem::take(&mut self.runs).into_iter().zip(leaving) {
            if run.region != Some(region) {
                runs.push(run);
                continue;
            }
            if !split && !leaving {
                runs.push(run);
                continue;
            }
            let shown = run.content.shown();
            if shown > 0 {
                runs.push(Run {
                    content: Content::Committed(shown),
                    region: (!leaving).then_some(region),
                    replacement: None,
                });
            }
        }
        self.runs = runs;
    }

    /// A removal that stays with the new composition lies where the
    /// composition does: one left before `range` (its replacement's text was
    /// narrowed to the range) moves to the range's start, one left after it
    /// to its end, which keeps the order of what the composition stands for.
    fn gather(&mut self, touched: &[RegionId], range: &Range<usize>) {
        let at = self.positions();
        let (mut before, mut after) = (Vec::new(), Vec::new());
        let mut runs = Vec::with_capacity(self.runs.len());
        for (index, run) in std::mem::take(&mut self.runs).into_iter().enumerate() {
            let stays = run.content.shown() == 0
                && run.region.is_some_and(|region| touched.contains(&region));
            if stays && at[index] < range.start {
                before.push(run);
            } else if stays && at[index] > range.end {
                after.push(run);
            } else {
                runs.push(run);
            }
        }
        self.runs = runs;
        if !before.is_empty() {
            let at = self.positions();
            let index = (0..self.runs.len())
                .find(|&index| at[index] >= range.start)
                .unwrap_or(self.runs.len());
            self.runs.splice(index..index, before);
        }
        if !after.is_empty() {
            let at = self.positions();
            let index = (0..self.runs.len())
                .find(|&index| {
                    at[index] > range.end
                        || (at[index] == range.end && self.runs[index].content.shown() > 0)
                })
                .unwrap_or(self.runs.len());
            self.runs.splice(index..index, after);
        }
    }

    /// Commit `region`: its text stands for itself and its removals go.
    fn commit(&mut self, region: RegionId) {
        self.runs
            .retain(|run| run.region != Some(region) || run.content.shown() > 0);
        for run in &mut self.runs {
            if run.region == Some(region) {
                run.content = Content::Committed(run.content.shown());
                run.region = None;
                run.replacement = None;
            }
        }
    }

    fn push_plain(&mut self, content: Content) {
        self.runs.push(Run::plain(content));
    }

    /// Split the run that shows the byte at `at` and some before it, so a
    /// run starts at `at`.
    fn cut(&mut self, at: usize) {
        let mut start = 0;
        for index in 0..self.runs.len() {
            let shown = self.runs[index].content.shown();
            if at > start && at < start + shown {
                let (head, tail) = (at - start, start + shown - at);
                let run = &mut self.runs[index];
                let tail_content = match run.content {
                    Content::Committed(_) => {
                        run.content = Content::Committed(head);
                        Content::Committed(tail)
                    }
                    Content::Preedit(_) => {
                        run.content = Content::Preedit(head);
                        Content::Preedit(tail)
                    }
                    Content::Removed(_) | Content::Marker => {
                        unreachable!("BUG: only a run that shows text is cut")
                    }
                };
                let tail = Run {
                    content: tail_content,
                    ..run.clone()
                };
                self.runs.insert(index + 1, tail);
                return;
            }
            start += shown;
        }
    }

    /// Join neighbouring runs of one kind that belong to the same region
    /// and replacement, and drop empty text runs.
    fn normalize(&mut self) {
        let mut runs: Vec<Run> = Vec::with_capacity(self.runs.len());
        for run in std::mem::take(&mut self.runs) {
            if matches!(run.content, Content::Committed(0) | Content::Preedit(0)) {
                continue;
            }
            if let Some(last) = runs.last_mut()
                && last.region == run.region
                && last.replacement == run.replacement
            {
                match (&mut last.content, &run.content) {
                    (Content::Committed(len), Content::Committed(more))
                    | (Content::Preedit(len), Content::Preedit(more)) => {
                        *len += more;
                        continue;
                    }
                    _ => {}
                }
            }
            runs.push(run);
        }
        self.runs = runs;
    }

    /// The byte position of each run: where it starts, or where it sits
    /// for one that shows nothing.
    fn positions(&self) -> Vec<usize> {
        let mut at = 0;
        self.runs
            .iter()
            .map(|run| {
                let start = at;
                at += run.content.shown();
                start
            })
            .collect()
    }

    /// The byte range `region` shows.
    fn extent(&self, region: RegionId, at: &[usize]) -> Range<usize> {
        let mut extent: Option<Range<usize>> = None;
        for (index, run) in self.runs.iter().enumerate() {
            if run.region == Some(region) {
                let end = at[index] + run.content.shown();
                extent = Some(match extent {
                    Some(extent) => extent.start..end,
                    None => at[index]..end,
                });
            }
        }
        extent.expect("BUG: a live region keeps at least one run")
    }

    /// A region id no other region of this session had. Ids are never
    /// reissued: a session that ran through them all fails the edit that
    /// asked for one more (the grant's panic, contained by the arbiter, and
    /// nothing written back) instead of wrapping onto a live region.
    fn new_region(&mut self) -> RegionId {
        self.next_region = self
            .next_region
            .checked_add(1)
            .expect("BUG: one session makes fewer than usize::MAX compositions");
        RegionId(self.next_region)
    }

    fn new_replacement(&mut self) -> ReplacementId {
        self.replacements.push(self.replacements.len());
        ReplacementId(self.replacements.len() - 1)
    }

    fn root(&self, replacement: ReplacementId) -> usize {
        let mut at = replacement.0;
        while self.replacements[at] != at {
            at = self.replacements[at];
        }
        at
    }

    fn join(&mut self, one: ReplacementId, other: ReplacementId) {
        let (one, other) = (self.root(one), self.root(other));
        if one != other {
            self.replacements[other] = one;
        }
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

/// Whether `edit` clears a composition over `range`: it neither ends at or
/// before the composition nor starts at or after it.
fn clears(edit: &Range<usize>, range: &Range<usize>) -> bool {
    !(edit.end <= range.start || edit.start >= range.end)
}

/// Whether a mark of `range` takes in a region over `extent`: two non-empty
/// ranges when they overlap, an empty one when it touches the other.
fn touches(extent: &Range<usize>, range: &Range<usize>) -> bool {
    if extent.is_empty() || range.is_empty() {
        extent.start <= range.end && extent.end >= range.start
    } else {
        extent.start < range.end && extent.end > range.start
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A session that ran through its region ids fails the next mark rather
    /// than reissue an id a live region holds. The counter starts at its
    /// last value here, since no public path makes that many marks.
    #[test]
    #[should_panic(expected = "fewer than usize::MAX compositions")]
    fn exhausted_region_ids_are_never_reissued() {
        let mut ledger = CompositionLedger::open("ab", Some((0..1, "a".to_owned())));
        ledger.next_region = usize::MAX;
        ledger.set_composition(Some(1..2));
    }
}
