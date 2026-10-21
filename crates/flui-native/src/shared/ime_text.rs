//! What the Android and web backends mirror of a focused text store: its
//! text with a protected field's characters masked, and the scalar each code
//! unit of a character-bounds request belongs to. Cfg-free plain values over
//! UTF-16, so Linux CI pins them.

use flui_platform_api::text_store::{TextStoreRead, Utf16Offset, Utf16Range};

/// What a protected field's text reads as in a mirror: one bullet per code
/// unit, so the input method's offsets still land where they do in the field.
pub const PROTECTED_UNIT: char = '\u{2022}';

/// The whole text of `session`, or [`PROTECTED_UNIT`]s for a protected field.
#[must_use]
pub fn document_text(session: &dyn TextStoreRead) -> String {
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
/// the pair is listed once.
#[must_use]
pub fn scalar_spans(text: &str, start: usize, end: usize) -> Vec<Utf16Range> {
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
