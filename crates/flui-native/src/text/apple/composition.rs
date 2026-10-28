//! The document a UIKit text-input responder edits for the system keyboard:
//! what was typed since the field took focus, with the input method's marked
//! text and the selection in it, counted in UTF-16 units as UIKit counts.
//!
//! Each edit answers what the focused field is told: the composition as it
//! now reads, the text it commits, or a delete before the caret. Pure Rust,
//! so its rules run on every host.

use std::ops::Range;

/// What an edit of the document tells the focused field.
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
    /// The character before the caret is deleted.
    DeleteBackward,
}

/// The text a responder holds for the keyboard, its marked range and its
/// selection.
#[derive(Debug, Default)]
pub(crate) struct Composition {
    text: Vec<u16>,
    marked: Option<Range<usize>>,
    selection: Range<usize>,
}

impl Composition {
    /// The document's length, in UTF-16 units.
    pub(crate) fn len(&self) -> usize {
        self.text.len()
    }

    /// Whether the document holds any text.
    pub(crate) fn has_text(&self) -> bool {
        !self.text.is_empty()
    }

    /// `offset` within the document.
    pub(crate) fn clamp(&self, offset: usize) -> usize {
        offset.min(self.text.len())
    }

    /// `range` within the document, start first.
    pub(crate) fn clamp_range(&self, range: Range<usize>) -> Range<usize> {
        let (start, end) = (self.clamp(range.start), self.clamp(range.end));
        start.min(end)..start.max(end)
    }

    /// The text of `range`.
    pub(crate) fn text_in(&self, range: Range<usize>) -> &[u16] {
        &self.text[self.clamp_range(range)]
    }

    /// The marked range, while a composition is in progress.
    pub(crate) fn marked(&self) -> Option<Range<usize>> {
        self.marked.clone()
    }

    /// The selection; an empty one is the caret.
    pub(crate) fn selection(&self) -> Range<usize> {
        self.selection.clone()
    }

    /// Select `range`, as the keyboard moves the caret.
    pub(crate) fn select(&mut self, range: Range<usize>) {
        self.selection = self.clamp_range(range);
    }

    /// Where the next edit lands: the composition, or else the selection.
    fn target(&self) -> Range<usize> {
        self.marked
            .clone()
            .unwrap_or_else(|| self.selection.clone())
    }

    /// Put `text` in place of `range`, returning where it now lies.
    fn splice(&mut self, range: Range<usize>, text: &[u16]) -> Range<usize> {
        let range = self.clamp_range(range);
        let start = range.start;
        self.text.splice(range, text.iter().copied());
        start..start + text.len()
    }

    /// The input method's composition becomes `marked`, with `selected` its
    /// selection within it. An empty `marked` ends the composition
    /// uncommitted.
    pub(crate) fn set_marked(&mut self, marked: &[u16], selected: Range<usize>) -> Edit {
        let placed = self.splice(self.target(), marked);
        if marked.is_empty() {
            self.marked = None;
            self.selection = placed.start..placed.start;
            return Edit::Preedit {
                text: String::new(),
                cursor: None,
            };
        }
        let selected = selected.start.min(marked.len())..selected.end.min(marked.len());
        self.selection = placed.start + selected.start..placed.start + selected.end;
        self.marked = Some(placed);
        let text = String::from_utf16_lossy(marked);
        let cursor = byte_range(marked, selected);
        Edit::Preedit { text, cursor }
    }

    /// `text` replaces the composition, or else the selection, and is
    /// committed.
    pub(crate) fn insert(&mut self, text: &[u16]) -> Edit {
        let placed = self.splice(self.target(), text);
        self.marked = None;
        self.selection = placed.end..placed.end;
        Edit::Commit(String::from_utf16_lossy(text))
    }

    /// The composition is accepted as it reads; nothing when none is in
    /// progress.
    pub(crate) fn unmark(&mut self) -> Option<Edit> {
        let marked = self.marked.take()?;
        self.selection = marked.end..marked.end;
        Some(Edit::Commit(String::from_utf16_lossy(&self.text[marked])))
    }

    /// The keyboard's delete key: the selection, or else the character
    /// before the caret, goes. Inside a composition it shortens the
    /// composition instead.
    pub(crate) fn delete_backward(&mut self) -> Option<Edit> {
        let selection = self.selection.clone();
        let doomed = if selection.is_empty() {
            let end = selection.start;
            if end == 0 {
                return None;
            }
            let before = end - 1;
            let start = if before > 0 && is_low_surrogate(self.text[before]) {
                before - 1
            } else {
                before
            };
            start..end
        } else {
            selection
        };
        self.splice(doomed.clone(), &[]);
        self.selection = doomed.start..doomed.start;
        match self.marked.take() {
            Some(marked) => {
                let kept = shrink(marked, &doomed);
                let text = self.text[kept.clone()].to_vec();
                let caret = doomed.start.clamp(kept.start, kept.end) - kept.start;
                self.marked = (!kept.is_empty()).then_some(kept);
                Some(preedit_after_delete(&text, caret))
            }
            None => Some(Edit::DeleteBackward),
        }
    }

    /// The field lost focus or composition was turned off: the document
    /// empties, and a composition in progress ends uncommitted.
    pub(crate) fn reset(&mut self) -> Option<Edit> {
        let composing = self.marked.is_some();
        *self = Self::default();
        composing.then_some(Edit::Preedit {
            text: String::new(),
            cursor: None,
        })
    }
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
    Edit::Preedit {
        text: String::from_utf16_lossy(text),
        cursor: byte_range(text, caret..caret),
    }
}

/// `range` once `doomed`, which overlaps or follows it, is removed.
fn shrink(range: Range<usize>, doomed: &Range<usize>) -> Range<usize> {
    let removed = |offset: usize| offset.saturating_sub(doomed.start).min(doomed.len());
    range.start - removed(range.start)..range.end - removed(range.end)
}

fn is_low_surrogate(unit: u16) -> bool {
    (0xDC00..=0xDFFF).contains(&unit)
}

/// The byte range of `units`' UTF-16 `range` in its UTF-8 text, or `None`
/// when an end falls inside a surrogate pair.
fn byte_range(units: &[u16], range: Range<usize>) -> Option<(usize, usize)> {
    let byte_offset = |end: usize| -> Option<usize> {
        let prefix = units.get(..end)?;
        String::from_utf16(prefix).ok().map(|text| text.len())
    };
    Some((byte_offset(range.start)?, byte_offset(range.end)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
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
        let steps = [("に", 1), ("にほ", 2), ("にほん", 3), ("にほんご", 4)];
        for (kana, caret) in steps {
            let edit = document.set_marked(&utf16(kana), caret..caret);
            let bytes = kana[..kana
                .char_indices()
                .nth(caret)
                .map_or(kana.len(), |(at, _)| at)]
                .len();
            assert_eq!(edit, preedit(kana, Some((bytes, bytes))), "marking {kana}");
        }
        assert_eq!(document.marked(), Some(0..4));
        assert_eq!(
            document.insert(&utf16("日本語")),
            Edit::Commit("日本語".to_owned())
        );
        assert_eq!(document.marked(), None);
        assert_eq!(document.text_in(0..usize::MAX), utf16("日本語"));
        assert_eq!(document.selection(), 3..3, "the caret follows the commit");
    }

    /// Pinyin composes after text already committed; the pick replaces only
    /// the composition.
    #[test]
    fn pinyin_composes_after_committed_text() {
        let mut document = Composition::default();
        document.insert(&utf16("说"));
        document.set_marked(&utf16("ni"), 2..2);
        document.set_marked(&utf16("ni h"), 4..4);
        assert_eq!(document.marked(), Some(1..5));
        assert_eq!(
            document.insert(&utf16("你好")),
            Edit::Commit("你好".to_owned())
        );
        assert_eq!(document.text_in(0..usize::MAX), utf16("说你好"));
    }

    #[test]
    fn unmarking_commits_the_composition_as_it_reads() {
        let mut document = Composition::default();
        assert_eq!(document.unmark(), None, "nothing to accept");
        document.set_marked(&utf16("かな"), 2..2);
        assert_eq!(document.unmark(), Some(Edit::Commit("かな".to_owned())));
        assert_eq!(document.marked(), None);
        assert_eq!(document.selection(), 2..2);
    }

    #[test]
    fn an_empty_composition_ends_it_uncommitted() {
        let mut document = Composition::default();
        document.set_marked(&utf16("あ"), 1..1);
        assert_eq!(document.set_marked(&[], 0..0), preedit("", None));
        assert_eq!(document.marked(), None);
        assert!(!document.has_text());
    }

    #[test]
    fn delete_backward_takes_a_whole_surrogate_pair() {
        let mut document = Composition::default();
        document.insert(&utf16("a😀"));
        assert_eq!(document.len(), 3);
        assert_eq!(document.delete_backward(), Some(Edit::DeleteBackward));
        assert_eq!(document.text_in(0..usize::MAX), utf16("a"));
        document.delete_backward();
        assert_eq!(document.delete_backward(), None, "nothing before the caret");
    }

    #[test]
    fn delete_backward_inside_a_composition_shortens_it() {
        let mut document = Composition::default();
        document.set_marked(&utf16("にほ"), 2..2);
        assert_eq!(
            document.delete_backward(),
            Some(preedit("に", Some((3, 3))))
        );
        assert_eq!(document.marked(), Some(0..1));
        assert_eq!(document.delete_backward(), Some(preedit("", None)));
        assert_eq!(document.marked(), None);
    }

    #[test]
    fn resetting_ends_a_composition_uncommitted() {
        let mut document = Composition::default();
        document.insert(&utf16("x"));
        assert_eq!(document.reset(), None, "no composition to end");
        document.set_marked(&utf16("ご"), 1..1);
        assert_eq!(document.reset(), Some(preedit("", None)));
        assert!(!document.has_text());
    }

    #[test]
    fn ranges_from_the_keyboard_are_clamped_to_the_document() {
        let mut document = Composition::default();
        document.insert(&utf16("ab"));
        // UIKit can hand a range end first.
        document.select(Range { start: 5, end: 1 });
        assert_eq!(document.selection(), 1..2);
        assert_eq!(document.text_in(1..9), utf16("b"));
        let edit = document.set_marked(&utf16("ä"), 0..7);
        assert_eq!(
            edit,
            preedit("ä", Some((0, 2))),
            "a selection past the end is clamped"
        );
    }
}
