//! UTF-16 offsets, the unit every text-store call counts in.
//!
//! TSF's ACP offsets, AppKit's `NSRange` and Android's `InputConnection`
//! all count UTF-16 code units, so the store surface does too (ADR-0090 §1).
//! A field keeps its own representation (UTF-8 bytes in Rust) and converts
//! at the store boundary through these functions, once, rather than each
//! backend carrying its own converter.
//!
//! [`Utf16Offset`] is a newtype so a byte offset cannot be passed where a
//! UTF-16 offset is expected. Every conversion is checked: an offset past
//! the end, one between the two halves of a surrogate pair, or a byte offset
//! off a `char` boundary is an [`OffsetError`], never a clamp. Clamping is a
//! policy decision a field makes for untrusted input; the conversion itself
//! answers exactly or refuses.
//!
//! Each conversion walks the text from the start, O(n) in its length. That is
//! the right cost for single-line fields; a multiline field (ADR-0030 §6)
//! needs a cached index behind the same functions.

use std::ops::Range;

/// An offset into a text, counted in UTF-16 code units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Utf16Offset(usize);

impl Utf16Offset {
    /// The start of every text.
    pub const ZERO: Self = Self(0);

    /// The offset `units` code units from the start.
    #[must_use]
    pub const fn new(units: usize) -> Self {
        Self(units)
    }

    /// The number of code units from the start.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// A span of UTF-16 code units whose start never exceeds its end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Utf16Range {
    start: Utf16Offset,
    end: Utf16Offset,
}

impl Utf16Range {
    /// The span from `start` to `end`, or `None` when `start > end`.
    #[must_use]
    pub const fn new(start: Utf16Offset, end: Utf16Offset) -> Option<Self> {
        if start.0 > end.0 {
            None
        } else {
            Some(Self { start, end })
        }
    }

    /// The empty span at `at`.
    #[must_use]
    pub const fn collapsed(at: Utf16Offset) -> Self {
        Self { start: at, end: at }
    }

    /// Where the span begins.
    #[must_use]
    pub const fn start(self) -> Utf16Offset {
        self.start
    }

    /// Where the span ends, exclusive.
    #[must_use]
    pub const fn end(self) -> Utf16Offset {
        self.end
    }

    /// The number of code units the span covers.
    #[must_use]
    pub const fn len(self) -> usize {
        self.end.0 - self.start.0
    }

    /// Whether the span covers nothing.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start.0 == self.end.0
    }
}

/// Why an offset does not name a position in a text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum OffsetError {
    /// The offset lies past the end. `len` is the text's length in the same
    /// unit as `offset`: code units for a UTF-16 offset, bytes for a byte
    /// offset.
    #[error("offset {offset} is past the end of a text of length {len}")]
    PastEnd {
        /// The offset that was asked for.
        offset: usize,
        /// The text's length, in the offset's unit.
        len: usize,
    },
    /// The UTF-16 offset falls between the two halves of a surrogate pair.
    #[error("UTF-16 offset {0} splits a surrogate pair")]
    SplitsSurrogatePair(usize),
    /// The byte offset is not on a `char` boundary.
    #[error("byte offset {0} is not on a char boundary")]
    NotCharBoundary(usize),
    /// A byte range whose start lies after its end.
    #[error("byte range {start}..{end} is inverted")]
    Inverted {
        /// The range's start.
        start: usize,
        /// The range's end.
        end: usize,
    },
}

/// The length of `text` in UTF-16 code units.
#[must_use]
pub fn utf16_len(text: &str) -> Utf16Offset {
    Utf16Offset(text.chars().map(char::len_utf16).sum())
}

/// The UTF-16 offset of the byte offset `byte` in `text`.
///
/// # Errors
///
/// [`OffsetError::PastEnd`] when `byte > text.len()`, and
/// [`OffsetError::NotCharBoundary`] when `byte` is inside a `char`.
pub fn utf16_offset(text: &str, byte: usize) -> Result<Utf16Offset, OffsetError> {
    if byte > text.len() {
        return Err(OffsetError::PastEnd {
            offset: byte,
            len: text.len(),
        });
    }
    if !text.is_char_boundary(byte) {
        return Err(OffsetError::NotCharBoundary(byte));
    }
    Ok(utf16_len(&text[..byte]))
}

/// The byte offset of the UTF-16 offset `offset` in `text`.
///
/// # Errors
///
/// [`OffsetError::PastEnd`] when `offset` is past the text's UTF-16 length,
/// and [`OffsetError::SplitsSurrogatePair`] when it falls inside a
/// supplementary-plane scalar.
pub fn byte_offset(text: &str, offset: Utf16Offset) -> Result<usize, OffsetError> {
    let target = offset.0;
    let mut units = 0;
    for (byte, scalar) in text.char_indices() {
        if units == target {
            return Ok(byte);
        }
        units += scalar.len_utf16();
        if units > target {
            return Err(OffsetError::SplitsSurrogatePair(target));
        }
    }
    if units == target {
        Ok(text.len())
    } else {
        Err(OffsetError::PastEnd {
            offset: target,
            len: units,
        })
    }
}

/// The byte range `range` covers in `text`.
///
/// # Errors
///
/// Either end's [`byte_offset`] error.
pub fn byte_range(text: &str, range: Utf16Range) -> Result<Range<usize>, OffsetError> {
    Ok(byte_offset(text, range.start)?..byte_offset(text, range.end)?)
}

/// The UTF-16 range the byte range `bytes` covers in `text`.
///
/// # Errors
///
/// Either end's [`utf16_offset`] error, or [`OffsetError::Inverted`] when
/// `bytes.start > bytes.end`.
pub fn utf16_range(text: &str, bytes: Range<usize>) -> Result<Utf16Range, OffsetError> {
    if bytes.start > bytes.end {
        return Err(OffsetError::Inverted {
            start: bytes.start,
            end: bytes.end,
        });
    }
    let start = utf16_offset(text, bytes.start)?;
    let end = utf16_offset(text, bytes.end)?;
    Ok(Utf16Range { start, end })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "a", a grinning face (U+1F600, two units, four bytes), "e" with a
    /// combining acute (one unit and two bytes for the mark), a ZWJ family
    /// (man, ZWJ, woman, ZWJ, girl: eight units) and a regional-indicator
    /// flag (two supplementary scalars: four units).
    const CORPUS: &str = "a😀e\u{301}👨‍👩‍👧🇯🇵";

    fn at(units: usize) -> Utf16Offset {
        Utf16Offset::new(units)
    }

    #[test]
    fn an_offset_inside_a_surrogate_pair_is_refused() {
        assert_eq!(
            byte_offset(CORPUS, at(2)),
            Err(OffsetError::SplitsSurrogatePair(2))
        );
        let range = Utf16Range::new(at(0), at(2)).expect("ordered");
        assert_eq!(
            byte_range(CORPUS, range),
            Err(OffsetError::SplitsSurrogatePair(2))
        );
    }

    #[test]
    fn every_char_boundary_round_trips() {
        for text in [CORPUS, "مرحبا بالعالم", "東京タワー", ""] {
            for (byte, _) in text.char_indices().chain([(text.len(), ' ')]) {
                let offset = utf16_offset(text, byte).expect("a char boundary");
                assert_eq!(byte_offset(text, offset), Ok(byte), "{text:?} at {byte}");
            }
            let whole = utf16_range(text, 0..text.len()).expect("the whole text");
            assert_eq!(whole.len(), utf16_len(text).get());
            assert_eq!(byte_range(text, whole), Ok(0..text.len()));
        }
    }
}
