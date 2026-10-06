//! [`CompositionLedger`]: what a document's composition stands for in its
//! committed text (ADR-0090 amendment item 1).
//!
//! The committed text is the document with the composing range replaced by
//! what that range stands for: nothing for a new preedit, the reconverted
//! words when an input method marks text the user already committed. A
//! store keeps that text, the composition's *origin*, beside its composing
//! range, and records a session's edits here so the origin follows them.
//! Offsets are byte offsets into the store's UTF-8 text, on `char`
//! boundaries the store has already checked.
//!
//! # The model
//!
//! The ledger keeps the session's document as a sequence of pieces, each
//! knowing what it stands for in the committed text:
//!
//! - a **committed** character stands for itself: text the document held
//!   when the session opened outside any composition, or that a composition
//!   committed;
//! - a **preedit** character stands for nothing: text the session inserted;
//! - a **removal** shows nothing and stands for the committed text an edit
//!   removed from a composition (or from a composition an edit cleared).
//!
//! The committed text is every character shown, except that the composition
//! shows what its pieces stand for. A composition an edit cleared keeps its
//! pieces as they are, so an input method that rewrites its composition and
//! marks it again does not lose what it stood for.
//!
//! The text one edit inserted, with the removal it left, is one
//! *replacement*: together they stand for the removed text, and no part of
//! the removed text belongs to any one inserted character. Replacements that
//! rewrote each other's text are one replacement.
//!
//! # Narrowing
//!
//! When a mark leaves part of a composition (or of a cleared one) outside the
//! new composing range, that part commits as the user sees it: its
//! characters stand for themselves and its removals go. What remains keeps
//! what its pieces stand for. If a replacement with removed text lies on
//! both sides of the new range, its removed text cannot be divided between
//! them: the remaining part then stands for its own visible text, so the
//! committed text over that region is exactly what the user sees. The
//! removed text of one replacement is never counted beside any of its
//! inserted text.
//!
//! The ledger follows the composition rules every store keeps (an edit
//! before the composition shifts it, one after it leaves it, one that
//! overlaps it or inserts strictly inside it clears it; ADR-0090
//! "Divergences"), so its composing range is the store's.

use std::ops::Range;

/// What one piece of the session's document is.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Content {
    /// A character that stands for itself.
    Committed(char),
    /// A character the session inserted, which stands for nothing.
    Preedit(char),
    /// Committed text an edit removed: shows nothing, stands for the text.
    Removed(String),
}

impl Content {
    fn shown(&self) -> Option<char> {
        match self {
            Self::Committed(ch) | Self::Preedit(ch) => Some(*ch),
            Self::Removed(_) => None,
        }
    }

    fn shown_len(&self) -> usize {
        self.shown().map_or(0, char::len_utf8)
    }

    /// What the piece stands for in the committed text.
    fn stands_for(&self, into: &mut String) {
        match self {
            Self::Committed(ch) => into.push(*ch),
            Self::Preedit(_) => {}
            Self::Removed(text) => into.push_str(text),
        }
    }
}

/// One piece of the document, and what it belongs to.
#[derive(Clone, Debug)]
struct Piece {
    content: Content,
    /// The composition, or a composition an edit cleared, it lies in.
    region: Option<usize>,
    /// The replacement it is part of.
    replacement: Option<usize>,
}

impl Piece {
    fn new(content: Content) -> Self {
        Self {
            content,
            region: None,
            replacement: None,
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
    pieces: Vec<Piece>,
    /// Regions that keep what they stand for: the composition and those an
    /// edit cleared.
    regions: Vec<usize>,
    composition: Option<usize>,
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
            ledger.pieces = text
                .chars()
                .map(|ch| Piece::new(Content::Committed(ch)))
                .collect();
            return ledger;
        };
        let region = ledger.new_region();
        let shown = &text[range.clone()];
        let replacement = (!origin.is_empty() && origin != shown).then(|| ledger.new_replacement());
        let member = |content| Piece {
            content,
            region: Some(region),
            replacement,
        };
        ledger.pieces.extend(
            text[..range.start]
                .chars()
                .map(|ch| Piece::new(Content::Committed(ch))),
        );
        if replacement.is_some() || shown.is_empty() {
            let removed = if replacement.is_some() {
                origin.clone()
            } else {
                String::new()
            };
            ledger.pieces.push(member(Content::Removed(removed)));
        }
        let stands_for_itself = !origin.is_empty() && origin == shown;
        ledger.pieces.extend(shown.chars().map(|ch| {
            member(if stands_for_itself {
                Content::Committed(ch)
            } else {
                Content::Preedit(ch)
            })
        }));
        ledger.pieces.extend(
            text[range.end..]
                .chars()
                .map(|ch| Piece::new(Content::Committed(ch))),
        );
        ledger.regions.push(region);
        ledger.composition = Some(region);
        ledger
    }

    /// Record that `edit` (a byte range of the text before it) is about to
    /// be replaced by `inserted`.
    pub fn replace(&mut self, edit: Range<usize>, inserted: &str) {
        let at = self.positions();
        let hit: Vec<usize> = self
            .regions
            .iter()
            .copied()
            .filter(|&region| clears(&edit, &self.extent(region, &at)))
            .collect();
        let fresh = inserted.chars().map(Content::Preedit);
        if hit.is_empty() {
            self.replace_committed(&edit, fresh.map(Piece::new).collect(), &at);
            return;
        }
        let mut span = edit.clone();
        for &region in &hit {
            let extent = self.extent(region, &at);
            span.start = span.start.min(extent.start);
            span.end = span.end.max(extent.end);
        }
        let in_span = |index: usize, piece: &Piece| match piece.content.shown() {
            Some(_) => span.contains(&at[index]),
            None => {
                piece.region.is_some_and(|region| hit.contains(&region))
                    || (at[index] > span.start && at[index] < span.end)
            }
        };
        let removed_by_edit = |index: usize, piece: &Piece| match piece.content.shown() {
            Some(_) => edit.contains(&at[index]),
            None => at[index] > edit.start && at[index] < edit.end,
        };
        let region = self.new_region();
        let replacement = self.new_replacement();
        let mut removed = String::new();
        let mut kept = Vec::with_capacity(self.pieces.len() + inserted.len());
        let mut placed = false;
        let mut first_in_span = None;
        for (index, piece) in std::mem::take(&mut self.pieces).into_iter().enumerate() {
            if !in_span(index, &piece) {
                kept.push(piece);
                continue;
            }
            first_in_span.get_or_insert(kept.len());
            if removed_by_edit(index, &piece) {
                piece.content.stands_for(&mut removed);
                if let Some(other) = piece.replacement {
                    self.join(replacement, other);
                }
                continue;
            }
            let after_edit = match piece.content.shown() {
                Some(_) => at[index] >= edit.start,
                None => at[index] >= edit.end,
            };
            if !placed && after_edit {
                kept.push(Piece {
                    content: Content::Removed(String::new()),
                    region: Some(region),
                    replacement: Some(replacement),
                });
                kept.extend(fresh.clone().map(|content| Piece {
                    content,
                    region: Some(region),
                    replacement: Some(replacement),
                }));
                placed = true;
            }
            kept.push(Piece {
                region: Some(region),
                ..piece
            });
        }
        let first_in_span = first_in_span.unwrap_or(kept.len());
        if !placed {
            // After every piece of the span: find where the span ends.
            let end = kept[first_in_span..]
                .iter()
                .position(|piece| piece.region != Some(region))
                .map_or(kept.len(), |offset| first_in_span + offset);
            let insertion: Vec<Piece> = std::iter::once(Content::Removed(String::new()))
                .chain(fresh)
                .map(|content| Piece {
                    content,
                    region: Some(region),
                    replacement: Some(replacement),
                })
                .collect();
            kept.splice(end..end, insertion);
        }
        // The removal carries what the edit removed.
        if let Some(removal) = kept.iter_mut().find(|piece| {
            piece.replacement == Some(replacement)
                && piece.region == Some(region)
                && matches!(piece.content, Content::Removed(ref text) if text.is_empty())
        }) {
            removal.content = Content::Removed(removed);
        }
        self.pieces = kept;
        self.regions.retain(|live| !hit.contains(live));
        if self
            .composition
            .is_some_and(|composition| hit.contains(&composition))
        {
            self.composition = None;
        }
        self.regions.push(region);
    }

    /// Record that the composing range is now `range` in the text, or that
    /// there is no composition.
    pub fn set_composition(&mut self, range: Option<Range<usize>>) {
        let Some(range) = range else {
            for region in std::mem::take(&mut self.regions) {
                self.commit(region);
            }
            self.composition = None;
            return;
        };
        let at = self.positions();
        let touched: Vec<usize> = self
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
        for &region in &touched {
            let extent = self.extent(region, &self.positions());
            if extent.start < range.start || extent.end > range.end {
                self.narrow(region, &range);
            }
        }
        let at = self.positions();
        let composition = self.new_region();
        let mut inside = false;
        for (index, piece) in self.pieces.iter_mut().enumerate() {
            let joins = match piece.content.shown() {
                Some(_) => range.contains(&at[index]),
                None => piece.region.is_some_and(|region| touched.contains(&region)),
            };
            if joins {
                piece.region = Some(composition);
                inside = true;
            }
        }
        if !inside {
            let index = (0..self.pieces.len())
                .find(|&index| at[index] >= range.start)
                .unwrap_or(self.pieces.len());
            self.pieces.insert(
                index,
                Piece {
                    content: Content::Removed(String::new()),
                    region: Some(composition),
                    replacement: None,
                },
            );
        }
        self.regions.retain(|live| !touched.contains(live));
        self.regions.push(composition);
        self.composition = Some(composition);
    }

    /// The origin of the current composition: what it stands for in the
    /// committed text. `None` without a composition.
    #[must_use]
    pub fn origin(&self) -> Option<String> {
        let composition = self.composition?;
        let mut origin = String::new();
        for piece in &self.pieces {
            if piece.region == Some(composition) {
                piece.content.stands_for(&mut origin);
            }
        }
        Some(origin)
    }

    /// The committed text: the text, with the composition replaced by what
    /// it stands for.
    #[must_use]
    pub fn committed(&self) -> String {
        let mut committed = String::new();
        for piece in &self.pieces {
            if piece.region.is_some() && piece.region == self.composition {
                piece.content.stands_for(&mut committed);
            } else if let Some(ch) = piece.content.shown() {
                committed.push(ch);
            }
        }
        committed
    }

    /// The text the ledger follows.
    #[must_use]
    pub fn text(&self) -> String {
        self.pieces
            .iter()
            .filter_map(|piece| piece.content.shown())
            .collect()
    }

    /// An edit that touches no region: what it removes leaves the committed
    /// text, and what it inserts is preedit, placed so every region stays
    /// where a store keeps its composition.
    fn replace_committed(&mut self, edit: &Range<usize>, fresh: Vec<Piece>, at: &[usize]) {
        let starts: Vec<(usize, usize)> = self
            .regions
            .iter()
            .map(|&region| (region, self.extent(region, at).start))
            .collect();
        let region_start = |region: Option<usize>| {
            starts
                .iter()
                .find(|(live, _)| Some(*live) == region)
                .map_or(0, |&(_, start)| start)
        };
        let mut kept = Vec::with_capacity(self.pieces.len() + fresh.len());
        let mut fresh = Some(fresh);
        for (index, piece) in std::mem::take(&mut self.pieces).into_iter().enumerate() {
            let before = match piece.content.shown() {
                Some(_) => at[index] >= edit.start,
                // The insertion goes before a removal only when the removal's
                // region lies wholly at or after the edit, as a store shifts
                // a composition the edit ends at.
                None => at[index] >= edit.end && region_start(piece.region) >= edit.end,
            };
            if before && let Some(fresh) = fresh.take() {
                kept.extend(fresh);
            }
            if piece.content.shown().is_some() && edit.contains(&at[index]) {
                continue;
            }
            kept.push(piece);
        }
        if let Some(fresh) = fresh {
            kept.extend(fresh);
        }
        self.pieces = kept;
    }

    /// A mark leaves the part of `region` outside `range`: it commits as
    /// shown. A replacement with removed text on both sides commits the
    /// whole region as shown (module doc, "Narrowing").
    fn narrow(&mut self, region: usize, range: &Range<usize>) {
        let at = self.positions();
        let leaves = |index: usize, piece: &Piece| match piece.content.shown() {
            Some(_) => !range.contains(&at[index]),
            None => at[index] < range.start || at[index] > range.end,
        };
        let mut sides: Vec<(usize, bool, bool)> = Vec::new();
        for (index, piece) in self.pieces.iter().enumerate() {
            if piece.region != Some(region) {
                continue;
            }
            if let Some(replacement) = piece.replacement {
                let root = self.root(replacement);
                let leaving = leaves(index, piece);
                match sides.iter_mut().find(|(seen, ..)| *seen == root) {
                    Some((_, left, stayed)) => {
                        *left |= leaving;
                        *stayed |= !leaving;
                    }
                    None => sides.push((root, leaving, !leaving)),
                }
            }
        }
        let split = sides.iter().any(|&(root, left, stayed)| {
            left && stayed
                && self.pieces.iter().any(|piece| {
                    piece.region == Some(region)
                        && piece
                            .replacement
                            .is_some_and(|other| self.root(other) == root)
                        && matches!(&piece.content, Content::Removed(text) if !text.is_empty())
                })
        });
        let mut kept = Vec::with_capacity(self.pieces.len());
        for (index, piece) in std::mem::take(&mut self.pieces).into_iter().enumerate() {
            if piece.region != Some(region) {
                kept.push(piece);
                continue;
            }
            let leaving = leaves(index, &piece);
            if !split && !leaving {
                kept.push(piece);
                continue;
            }
            if let Some(ch) = piece.content.shown() {
                kept.push(Piece {
                    content: Content::Committed(ch),
                    region: (!leaving).then_some(region),
                    replacement: None,
                });
            } else if !leaving {
                // A removal that stays keeps the region's place.
                kept.push(Piece {
                    content: Content::Removed(String::new()),
                    region: Some(region),
                    replacement: None,
                });
            }
        }
        self.pieces = kept;
    }

    /// Commit `region`: its characters stand for themselves and its
    /// removals go.
    fn commit(&mut self, region: usize) {
        self.pieces
            .retain(|piece| piece.region != Some(region) || piece.content.shown().is_some());
        for piece in &mut self.pieces {
            if piece.region == Some(region) {
                if let Some(ch) = piece.content.shown() {
                    piece.content = Content::Committed(ch);
                }
                piece.region = None;
                piece.replacement = None;
            }
        }
    }

    /// The byte position of each piece: where it starts, or where it sits
    /// for a removal.
    fn positions(&self) -> Vec<usize> {
        let mut at = 0;
        self.pieces
            .iter()
            .map(|piece| {
                let start = at;
                at += piece.content.shown_len();
                start
            })
            .collect()
    }

    /// The byte range `region` shows.
    fn extent(&self, region: usize, at: &[usize]) -> Range<usize> {
        let mut extent: Option<Range<usize>> = None;
        for (index, piece) in self.pieces.iter().enumerate() {
            if piece.region == Some(region) {
                let end = at[index] + piece.content.shown_len();
                extent = Some(match extent {
                    Some(extent) => extent.start..end,
                    None => at[index]..end,
                });
            }
        }
        extent.expect("BUG: a live region keeps at least one piece")
    }

    fn new_region(&mut self) -> usize {
        self.next_region += 1;
        self.next_region
    }

    fn new_replacement(&mut self) -> usize {
        self.replacements.push(self.replacements.len());
        self.replacements.len() - 1
    }

    fn root(&self, mut replacement: usize) -> usize {
        while self.replacements[replacement] != replacement {
            replacement = self.replacements[replacement];
        }
        replacement
    }

    fn join(&mut self, one: usize, other: usize) {
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
