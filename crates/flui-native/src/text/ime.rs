//! What the Android and web backends mirror of a focused text store: its
//! text with a protected field's characters masked, the scalar each code
//! unit of a character-bounds request belongs to, and the one edit between
//! two versions of the mirrored text. Plain values over UTF-16, built for the
//! two backends and for tests, so Linux CI pins them.

#![forbid(unsafe_code)]

use flui_platform::text_store::{TextStoreRead, Utf16Offset, Utf16Range};

/// What a protected field's text reads as in a mirror: one bullet per code
/// unit, so the input method's offsets still land where they do in the field.
pub(crate) const PROTECTED_UNIT: char = '\u{2022}';

/// The whole text of `session`, or [`PROTECTED_UNIT`]s for a protected field.
#[must_use]
pub(crate) fn document_text(session: &dyn TextStoreRead) -> String {
    let len = session.document_len();
    let whole =
        Utf16Range::new(Utf16Offset::ZERO, len).expect("BUG: the whole document is a range");
    session
        .text(whole)
        .unwrap_or_else(|_| std::iter::repeat_n(PROTECTED_UNIT, len.get()).collect())
}

/// The scalars that cover the code units `start..end` of `text`, in order,
/// each as the UTF-16 range it occupies. A scalar outside the text is not
/// listed; a unit that is half of a surrogate pair names the whole pair, and
/// the pair is listed once. The web backend answers character bounds with
/// it; Android's input method asks none.
#[cfg(any(target_arch = "wasm32", test))]
#[must_use]
pub(crate) fn scalar_spans(text: &str, start: usize, end: usize) -> Vec<Utf16Range> {
    let mut spans = Vec::new();
    let mut at = 0;
    for scalar in text.chars() {
        let next = at + scalar.len_utf16();
        if next > start && at < end {
            spans.push(
                Utf16Range::new(Utf16Offset::new(at), Utf16Offset::new(next))
                    .expect("BUG: a scalar's span is ordered"),
            );
        }
        if next >= end {
            break;
        }
        at = next;
    }
    spans
}

/// One edit between two versions of a text, in UTF-16 code units: the
/// range `start..old_end` of the old text became `start..new_end` of the
/// new one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextDiff {
    /// Where the texts first differ.
    pub(crate) start: usize,
    /// The end of the replaced range in the old text.
    pub(crate) old_end: usize,
    /// The end of the inserted range in the new text.
    pub(crate) new_end: usize,
}

/// The one edit that turns `old` into `new`: the longest common prefix and,
/// of what remains, the longest common suffix are kept. `None` when the
/// texts are equal. A surrogate pair is never split: an edit boundary inside
/// one moves outward.
#[must_use]
pub(crate) fn diff(old: &[u16], new: &[u16]) -> Option<TextDiff> {
    if old == new {
        return None;
    }
    let mut start = old
        .iter()
        .zip(new)
        .take_while(|(old, new)| old == new)
        .count();
    // A shared high surrogate whose low half differs is part of the edit.
    if start > 0 && is_high_surrogate(old[start - 1]) {
        start -= 1;
    }
    let room = old.len().min(new.len()) - start;
    let mut suffix = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(room)
        .take_while(|(old, new)| old == new)
        .count();
    // A shared low surrogate whose high half differs is part of the edit.
    if suffix > 0 && is_low_surrogate(old[old.len() - suffix]) {
        suffix -= 1;
    }
    Some(TextDiff {
        start,
        old_end: old.len() - suffix,
        new_end: new.len() - suffix,
    })
}

const fn is_high_surrogate(unit: u16) -> bool {
    matches!(unit, 0xD800..=0xDBFF)
}

const fn is_low_surrogate(unit: u16) -> bool {
    matches!(unit, 0xDC00..=0xDFFF)
}

#[cfg(test)]
mod tests;
