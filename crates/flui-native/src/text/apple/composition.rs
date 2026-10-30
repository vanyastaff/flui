//! The document a UIKit text-input responder edits for the system keyboard:
//! what was typed since the field took focus, with the input method's marked
//! text and the selection in it, counted in UTF-16 units as UIKit counts.
//!
//! The field may hold text from before it took focus, which this document
//! does not see until the shared text store serves it (ADR-0255 decision
//! 11). So an edit is told to the field only in the vocabulary the field
//! follows at its caret: the composition as it now reads, the text it
//! commits, a delete before the caret, or a run of text before the caret
//! replaced by another. An edit that vocabulary cannot carry (a range away
//! from the caret) changes this document only. Pure Rust, so its rules run
//! on every host.

use flui_platform::ImeEvent;
use flui_platform::text_store::utf16::{Utf16Offset, Utf16Range};

use super::ime_cursor;

/// What an edit of the document means for the focused field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Edit {
    /// The composition now reads `text`, with the caret or selection at
    /// `cursor`, byte offsets in `text`; an empty `text` ended it uncommitted.
    Preedit {
        text: String,
        cursor: Option<(usize, usize)>,
    },
    /// `text` is committed at the caret.
    Commit(String),
    /// `deleted`, which ends at the caret, is replaced by `text`: an
    /// autocorrection, a text shortcut or a deleted selection.
    Replace { deleted: String, text: String },
    /// The character before the caret is deleted.
    DeleteBackward,
}

/// One thing the focused field is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Announcement {
    /// An input-method event.
    Ime(ImeEvent),
    /// A Backspace press, which deletes one grapheme cluster before the
    /// field's caret.
    Backspace,
}

impl Edit {
    /// What the field is told of this edit, in order.
    pub(crate) fn announcements(self) -> Vec<Announcement> {
        match self {
            Self::Preedit { text, cursor } => {
                vec![Announcement::Ime(ImeEvent::Preedit { text, cursor })]
            }
            Self::Commit(text) => vec![Announcement::Ime(ImeEvent::Commit(text))],
            Self::DeleteBackward => vec![Announcement::Backspace],
            Self::Replace { deleted, text } => {
                let mut told = vec![Announcement::Backspace; grapheme_count(&deleted)];
                if !text.is_empty() {
                    told.push(Announcement::Ime(ImeEvent::Commit(text)));
                }
                told
            }
        }
    }
}

/// How many Backspace presses take `text` away: one per extended grapheme
/// cluster, as the field deletes.
fn grapheme_count(text: &str) -> usize {
    const SEGMENTER: icu_segmenter::GraphemeClusterSegmenterBorrowed<'static> =
        icu_segmenter::GraphemeClusterSegmenter::new();
    // The boundaries include both ends of a non-empty text.
    SEGMENTER.segment_str(text).count().saturating_sub(1)
}

/// The text a responder holds for the keyboard, its marked range and its
/// selection.
#[derive(Debug)]
pub(crate) struct Composition {
    text: Vec<u16>,
    marked: Option<Utf16Range>,
    selection: Utf16Range,
}

impl Default for Composition {
    fn default() -> Self {
        Self {
            text: Vec::new(),
            marked: None,
            selection: collapsed(Utf16Offset::ZERO),
        }
    }
}

impl Composition {
    /// The document's end.
    pub(crate) fn end(&self) -> Utf16Offset {
        Utf16Offset::new(self.text.len())
    }

    /// `offset` within the document.
    fn clamp(&self, offset: Utf16Offset) -> Utf16Offset {
        offset.min(self.end())
    }

    /// The range between `a` and `b`, in either order, within the document.
    pub(crate) fn range(&self, a: Utf16Offset, b: Utf16Offset) -> Utf16Range {
        let (a, b) = (self.clamp(a), self.clamp(b));
        Utf16Range::try_new(a.min(b), a.max(b)).expect("BUG: the smaller end comes first")
    }

    /// The text of `range`.
    pub(crate) fn text_in(&self, range: Utf16Range) -> &[u16] {
        let range = self.range(range.start(), range.end());
        &self.text[range.start().get()..range.end().get()]
    }

    /// The marked range, while a composition is in progress.
    pub(crate) fn marked(&self) -> Option<Utf16Range> {
        self.marked
    }

    /// The selection; an empty one is the caret.
    pub(crate) fn selection(&self) -> Utf16Range {
        self.selection
    }

    /// Select `range`, as the keyboard moves the caret.
    pub(crate) fn select(&mut self, range: Utf16Range) {
        self.selection = self.range(range.start(), range.end());
    }

    /// The character next to `at`, before it when `backward`: a surrogate
    /// pair whole, never half of one; empty at the document's edge.
    pub(crate) fn extend(&self, at: Utf16Offset, backward: bool) -> Utf16Range {
        let at = self.boundary(self.clamp(at).get());
        let units = &self.text;
        let (start, end) = if backward {
            let start = at.saturating_sub(1);
            let start = if start > 0 && is_low_surrogate(units[start]) {
                start - 1
            } else {
                start
            };
            (start, at)
        } else if at == units.len() {
            (at, at)
        } else {
            let end = at + 1;
            let end = if end < units.len() && is_low_surrogate(units[end]) {
                end + 1
            } else {
                end
            };
            (at, end)
        };
        offsets(start, end)
    }

    /// `at`, moved off the low half of a surrogate pair to the pair's start.
    fn boundary(&self, at: usize) -> usize {
        if at > 0 && at < self.text.len() && is_low_surrogate(self.text[at]) {
            at - 1
        } else {
            at
        }
    }

    /// Where the next edit lands: the composition, or else the selection.
    fn target(&self) -> Utf16Range {
        self.marked.unwrap_or(self.selection)
    }

    /// Put `text` in place of `range`, returning where it now lies.
    fn splice(&mut self, range: Utf16Range, text: &[u16]) -> Utf16Range {
        let range = self.range(range.start(), range.end());
        let start = range.start().get();
        self.text
            .splice(start..range.end().get(), text.iter().copied());
        offsets(start, start + text.len())
    }

    /// The input method's composition becomes `marked`, with `selected` its
    /// selection within it. An empty `marked` ends the composition
    /// uncommitted.
    pub(crate) fn set_marked(&mut self, marked: &[u16], selected: Utf16Range) -> Edit {
        let placed = self.splice(self.target(), marked);
        if marked.is_empty() {
            self.marked = None;
            self.selection = collapsed(placed.start());
            return Edit::Preedit {
                text: String::new(),
                cursor: None,
            };
        }
        let within = |offset: Utf16Offset| offset.get().min(marked.len());
        let (from, to) = (within(selected.start()), within(selected.end()));
        let start = placed.start().get();
        self.selection = offsets(start + from, start + to);
        self.marked = Some(placed);
        let text = String::from_utf16_lossy(marked);
        let cursor = ime_cursor(&text, Utf16Offset::new(from), Utf16Offset::new(to));
        Edit::Preedit { text, cursor }
    }

    /// `text` replaces the composition, or else the selection, and is
    /// committed.
    pub(crate) fn insert(&mut self, text: &[u16]) -> Edit {
        let placed = self.splice(self.target(), text);
        self.marked = None;
        self.selection = collapsed(placed.end());
        Edit::Commit(String::from_utf16_lossy(text))
    }

    /// The composition is accepted as it reads; nothing when none is in
    /// progress.
    pub(crate) fn unmark(&mut self) -> Option<Edit> {
        let marked = self.marked.take()?;
        self.selection = collapsed(marked.end());
        Some(Edit::Commit(String::from_utf16_lossy(self.text_in(marked))))
    }

    /// `range` becomes `text`: an autocorrection or a text shortcut. With
    /// no composition and the caret at the range's end, the field is told
    /// the run before its caret is replaced; the composition itself is
    /// replaced as a commit. Any other range changes this document only (a
    /// composition elsewhere keeps its text and moves with it), since the
    /// field is told edits at its caret alone.
    pub(crate) fn replace(&mut self, range: Utf16Range, text: &[u16]) -> Option<Edit> {
        let range = self.range(range.start(), range.end());
        if self.marked == Some(range) {
            return Some(self.insert(text));
        }
        let at_caret = self.marked.is_none()
            && self.selection.is_empty()
            && self.selection.end() == range.end();
        let deleted = String::from_utf16_lossy(self.text_in(range));
        let caret = self.selection.end();
        let placed = self.splice(range, text);
        self.marked = self
            .marked
            .and_then(|marked| shifted(marked, range, placed.end()));
        if at_caret {
            self.selection = collapsed(placed.end());
            return Some(Edit::Replace {
                deleted,
                text: String::from_utf16_lossy(text),
            });
        }
        self.selection = collapsed(shifted_offset(caret, range, placed.end()));
        None
    }

    /// The keyboard's delete key: the selection, or else the character
    /// before the caret, goes. With nothing known before the caret the
    /// field still deletes, from text it held before it took focus. Inside
    /// a composition it shortens the composition; before it, the field
    /// deletes and the composition keeps its text.
    pub(crate) fn delete_backward(&mut self) -> Edit {
        let selection = self.selection;
        let doomed = if selection.is_empty() {
            if selection.end() == Utf16Offset::ZERO {
                return Edit::DeleteBackward;
            }
            self.extend(selection.end(), true)
        } else {
            selection
        };
        let deleted = String::from_utf16_lossy(self.text_in(doomed));
        self.splice(doomed, &[]);
        self.selection = collapsed(doomed.start());
        let Some(marked) = self.marked.take() else {
            return if selection.is_empty() {
                Edit::DeleteBackward
            } else {
                Edit::Replace {
                    deleted,
                    text: String::new(),
                }
            };
        };
        if doomed.end() <= marked.start() {
            self.marked = shifted(marked, doomed, doomed.start());
            return Edit::DeleteBackward;
        }
        let kept = shrink(marked, doomed);
        let text = self.text_in(kept).to_vec();
        let caret = doomed.start().clamp(kept.start(), kept.end()).get() - kept.start().get();
        self.marked = (!kept.is_empty()).then_some(kept);
        preedit_after_delete(&text, caret)
    }

    /// The field lost focus, composition was turned off, or another field
    /// took the keyboard: the document empties, and a composition in
    /// progress ends uncommitted.
    pub(crate) fn reset(&mut self) -> Option<Edit> {
        let composing = self.marked.is_some();
        *self = Self::default();
        composing.then_some(Edit::Preedit {
            text: String::new(),
            cursor: None,
        })
    }
}

/// The empty range at `at`.
fn collapsed(at: Utf16Offset) -> Utf16Range {
    Utf16Range::collapsed(at)
}

/// The range from `start` to `end` units, start first.
fn offsets(start: usize, end: usize) -> Utf16Range {
    Utf16Range::try_new(Utf16Offset::new(start), Utf16Offset::new(end))
        .expect("BUG: a range in the document starts first")
}

/// Where `offset` lies once `replaced` became text ending at `new_end`.
fn shifted_offset(offset: Utf16Offset, replaced: Utf16Range, new_end: Utf16Offset) -> Utf16Offset {
    if offset < replaced.end() {
        return offset.min(new_end);
    }
    Utf16Offset::new(offset.get() - replaced.end().get() + new_end.get())
}

/// `range` once `replaced` became text ending at `new_end`; `None` when the
/// two overlap, which ends a composition.
fn shifted(range: Utf16Range, replaced: Utf16Range, new_end: Utf16Offset) -> Option<Utf16Range> {
    if replaced.end() <= range.start() {
        let start = shifted_offset(range.start(), replaced, new_end);
        let end = shifted_offset(range.end(), replaced, new_end);
        return Utf16Range::try_new(start, end);
    }
    (range.end() <= replaced.start()).then_some(range)
}

/// The composition as it reads after a delete inside it left `text`, with
/// the caret `caret` units in.
fn preedit_after_delete(text: &[u16], caret: usize) -> Edit {
    if text.is_empty() {
        return Edit::Preedit {
            text: String::new(),
            cursor: None,
        };
    }
    let text = String::from_utf16_lossy(text);
    let cursor = ime_cursor(&text, Utf16Offset::new(caret), Utf16Offset::new(caret));
    Edit::Preedit { text, cursor }
}

/// `range` once `doomed`, which overlaps or follows it, is removed.
fn shrink(range: Utf16Range, doomed: Utf16Range) -> Utf16Range {
    let removed = |offset: Utf16Offset| {
        offset
            .get()
            .saturating_sub(doomed.start().get())
            .min(doomed.len())
    };
    offsets(
        range.start().get() - removed(range.start()),
        range.end().get() - removed(range.end()),
    )
}

fn is_low_surrogate(unit: u16) -> bool {
    (0xDC00..=0xDFFF).contains(&unit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn at(units: usize) -> Utf16Offset {
        Utf16Offset::new(units)
    }

    fn span(start: usize, end: usize) -> Utf16Range {
        offsets(start, end)
    }

    fn all(document: &Composition) -> String {
        String::from_utf16_lossy(document.text_in(span(0, document.end().get())))
    }

    fn preedit(text: &str, cursor: Option<(usize, usize)>) -> Edit {
        Edit::Preedit {
            text: text.to_owned(),
            cursor,
        }
    }

    /// The Japanese Romaji keyboard spells a word in kana, then a candidate
    /// replaces it: each step is the composition as it reads, and the pick is
    /// committed in its place.
    #[test]
    fn kana_composed_then_converted_is_committed() {
        let mut document = Composition::default();
        let steps = [
            ("に", 1, 3),
            ("にほ", 2, 6),
            ("にほん", 3, 9),
            ("にほんご", 4, 12),
        ];
        for (kana, caret, bytes) in steps {
            let edit = document.set_marked(&utf16(kana), span(caret, caret));
            assert_eq!(edit, preedit(kana, Some((bytes, bytes))), "marking {kana}");
        }
        assert_eq!(document.marked(), Some(span(0, 4)));
        assert_eq!(
            document.insert(&utf16("日本語")),
            Edit::Commit("日本語".to_owned())
        );
        assert_eq!(document.marked(), None);
        assert_eq!(all(&document), "日本語");
        assert_eq!(
            document.selection(),
            span(3, 3),
            "the caret follows the commit"
        );
    }

    /// Pinyin composes after text already committed; the pick replaces only
    /// the composition.
    #[test]
    fn pinyin_composes_after_committed_text() {
        let mut document = Composition::default();
        document.insert(&utf16("说"));
        document.set_marked(&utf16("ni"), span(2, 2));
        document.set_marked(&utf16("ni h"), span(4, 4));
        assert_eq!(document.marked(), Some(span(1, 5)));
        assert_eq!(
            document.insert(&utf16("你好")),
            Edit::Commit("你好".to_owned())
        );
        assert_eq!(all(&document), "说你好");
    }

    #[test]
    fn unmarking_commits_the_composition_as_it_reads() {
        let mut document = Composition::default();
        assert_eq!(document.unmark(), None, "nothing to accept");
        document.set_marked(&utf16("かな"), span(2, 2));
        assert_eq!(document.unmark(), Some(Edit::Commit("かな".to_owned())));
        assert_eq!(document.marked(), None);
        assert_eq!(document.selection(), span(2, 2));
    }

    #[test]
    fn an_empty_composition_ends_it_uncommitted() {
        let mut document = Composition::default();
        document.set_marked(&utf16("あ"), span(1, 1));
        assert_eq!(document.set_marked(&[], span(0, 0)), preedit("", None));
        assert_eq!(document.marked(), None);
        assert_eq!(document.end(), at(0));
    }

    #[test]
    fn delete_backward_takes_a_whole_surrogate_pair() {
        let mut document = Composition::default();
        document.insert(&utf16("a😀"));
        assert_eq!(document.end(), at(3));
        assert_eq!(document.delete_backward(), Edit::DeleteBackward);
        assert_eq!(all(&document), "a");
    }

    /// The field may hold text from before it took focus, which the
    /// document never saw: the delete still reaches it.
    #[test]
    fn a_delete_with_nothing_known_before_the_caret_still_reaches_the_field() {
        let mut document = Composition::default();
        assert_eq!(document.delete_backward(), Edit::DeleteBackward);
        document.insert(&utf16("x"));
        document.delete_backward();
        assert_eq!(document.delete_backward(), Edit::DeleteBackward);
    }

    #[test]
    fn delete_backward_inside_a_composition_shortens_it() {
        let mut document = Composition::default();
        document.set_marked(&utf16("にほ"), span(2, 2));
        assert_eq!(document.delete_backward(), preedit("に", Some((3, 3))));
        assert_eq!(document.marked(), Some(span(0, 1)));
        assert_eq!(document.delete_backward(), preedit("", None));
        assert_eq!(document.marked(), None);
    }

    /// The caret at the composition's start: the delete takes the character
    /// before it, which the field holds, and the composition keeps its text.
    #[test]
    fn a_delete_before_the_composition_reaches_the_field() {
        let mut document = Composition::default();
        document.insert(&utf16("a"));
        document.set_marked(&utf16("か"), span(1, 1));
        document.select(span(1, 1));
        assert_eq!(document.delete_backward(), Edit::DeleteBackward);
        assert_eq!(document.marked(), Some(span(0, 1)));
        assert_eq!(all(&document), "か");
    }

    /// An autocorrection replaces the word before the caret: the field
    /// deletes it and commits the correction, not the correction on top.
    #[test]
    fn a_replaced_word_before_the_caret_is_deleted_then_committed() {
        let mut document = Composition::default();
        document.insert(&utf16("cafe"));
        let edit = document.replace(span(0, 4), &utf16("café"));
        assert_eq!(
            edit,
            Some(Edit::Replace {
                deleted: "cafe".to_owned(),
                text: "café".to_owned(),
            })
        );
        assert_eq!(all(&document), "café");
        assert_eq!(document.selection(), span(4, 4));
    }

    /// Replacing text away from a composition leaves the composition where
    /// its text now is, instead of replacing it.
    #[test]
    fn a_replacement_during_a_composition_edits_the_range_asked_for() {
        let mut document = Composition::default();
        document.insert(&utf16("ab"));
        document.set_marked(&utf16("か"), span(1, 1));
        assert_eq!(document.replace(span(0, 1), &utf16("XY")), None);
        assert_eq!(all(&document), "XYbか");
        assert_eq!(document.marked(), Some(span(3, 4)));
        let composition = document.marked().expect("still composing");
        assert_eq!(
            document.replace(composition, &utf16("家")),
            Some(Edit::Commit("家".to_owned()))
        );
    }

    #[test]
    fn resetting_ends_a_composition_uncommitted() {
        let mut document = Composition::default();
        document.insert(&utf16("x"));
        assert_eq!(document.reset(), None, "no composition to end");
        document.set_marked(&utf16("ご"), span(1, 1));
        assert_eq!(document.reset(), Some(preedit("", None)));
        assert_eq!(document.end(), at(0));
    }

    #[test]
    fn ranges_from_the_keyboard_are_clamped_to_the_document() {
        let mut document = Composition::default();
        document.insert(&utf16("ab"));
        // UIKit can hand a range end first.
        document.select(document.range(at(5), at(1)));
        assert_eq!(document.selection(), span(1, 2));
        assert_eq!(document.text_in(span(1, 9)), utf16("b"));
        let edit = document.set_marked(&utf16("ä"), span(0, 7));
        assert_eq!(
            edit,
            preedit("ä", Some((0, 2))),
            "a selection past the end is clamped"
        );
    }

    /// A character next to a position is a surrogate pair whole, from
    /// either side and from inside the pair.
    #[test]
    fn a_character_next_to_a_position_never_splits_a_surrogate_pair() {
        let mut document = Composition::default();
        document.insert(&utf16("a😀b"));
        assert_eq!(document.extend(at(1), false), span(1, 3), "forward over it");
        assert_eq!(document.extend(at(3), true), span(1, 3), "backward over it");
        assert_eq!(document.extend(at(2), false), span(1, 3), "from inside it");
        assert_eq!(document.extend(at(0), true), span(0, 0), "before the start");
        assert_eq!(document.extend(at(4), false), span(4, 4), "after the end");
    }

    /// Each edit becomes what the field hears: input-method events, and
    /// Backspace presses, one per grapheme cluster a replacement takes.
    #[test]
    fn an_edit_is_announced_as_the_field_hears_it() {
        let ime = Announcement::Ime;
        let rows = [
            (
                preedit("にほ", Some((6, 6))),
                vec![ime(ImeEvent::Preedit {
                    text: "にほ".to_owned(),
                    cursor: Some((6, 6)),
                })],
            ),
            (
                Edit::Commit("日本語".to_owned()),
                vec![ime(ImeEvent::Commit("日本語".to_owned()))],
            ),
            (Edit::DeleteBackward, vec![Announcement::Backspace]),
            (
                Edit::Replace {
                    deleted: "cafe".to_owned(),
                    text: "café".to_owned(),
                },
                vec![
                    Announcement::Backspace,
                    Announcement::Backspace,
                    Announcement::Backspace,
                    Announcement::Backspace,
                    ime(ImeEvent::Commit("café".to_owned())),
                ],
            ),
            (
                // A family emoji is one cluster of five scalars.
                Edit::Replace {
                    deleted: "👨\u{200D}👩\u{200D}👧é".to_owned(),
                    text: String::new(),
                },
                vec![Announcement::Backspace, Announcement::Backspace],
            ),
        ];
        for (edit, expected) in rows {
            assert_eq!(edit.clone().announcements(), expected, "{edit:?}");
        }
    }
}
