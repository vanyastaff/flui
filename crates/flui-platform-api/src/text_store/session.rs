//! What a platform reads and edits inside a lock: the session traits and
//! the values they speak.

use flui_types::geometry::{Bounds, Pixels, Point};

use super::lock::TextStoreError;
use super::utf16::{Utf16Offset, Utf16Range};

/// A selection: where it started and where its moving end is.
///
/// `active` may precede `anchor` (a backward selection); [`Self::range`]
/// orders them. A caret is the collapsed case.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Selection {
    /// The fixed end.
    pub anchor: Utf16Offset,
    /// The moving end, where the caret is drawn.
    pub active: Utf16Offset,
}

impl Selection {
    /// A caret at `at`.
    #[must_use]
    pub const fn collapsed(at: Utf16Offset) -> Self {
        Self {
            anchor: at,
            active: at,
        }
    }

    /// The selected span in ascending order.
    #[must_use]
    pub fn range(self) -> Utf16Range {
        let (start, end) = if self.anchor <= self.active {
            (self.anchor, self.active)
        } else {
            (self.active, self.anchor)
        };
        Utf16Range::new(start, end).expect("BUG: an ordered pair is a valid range")
    }

    /// Whether nothing is selected.
    #[must_use]
    pub const fn is_collapsed(self) -> bool {
        self.anchor.get() == self.active.get()
    }
}

/// The in-progress composition.
///
/// One value, not a range beside a flag (ADR-0030 §5): the caret-hiding
/// signal cannot outlive the composition it belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Composition {
    /// The composed text's span.
    pub range: Utf16Range,
    /// Whether the input method owns the caret's position and wants no caret
    /// drawn (winit's `Preedit { cursor: None }`).
    pub hides_caret: bool,
}

/// What one edit did, in TSF's `TS_TEXTCHANGE` shape: the span
/// `start..old_end` became `start..new_end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextChange {
    /// Where the edit began.
    pub start: Utf16Offset,
    /// Where the replaced span ended, before the edit.
    pub old_end: Utf16Offset,
    /// Where the inserted text ends, after the edit.
    pub new_end: Utf16Offset,
}

/// A range's bounding rect, in window-root logical pixels (ADR-0030 §7).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RangeRect {
    /// The union of the range's boxes; for an empty range, the caret.
    pub bounds: Bounds<Pixels>,
    /// Whether part of the range lies outside the field's visible area.
    pub clipped: bool,
}

/// How [`TextStoreRead::index_at_point`] treats a point that falls between
/// characters or outside the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointMode {
    /// The character whose box contains the point; a point outside the text
    /// is [`TextStoreError::PointOutside`].
    Exact,
    /// The character boundary nearest the point, clamping one outside the
    /// text to the nearer end.
    Nearest,
}

/// The store's standing properties, TSF's `TS_STATUS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct TextStoreStatus {
    /// The text must not be read out (a password field). Text reads return
    /// [`TextStoreError::Protected`]; edits, selection and geometry still work.
    pub protected: bool,
    /// The field holds one line.
    pub single_line: bool,
}

impl TextStoreStatus {
    /// An editable, unprotected, single-line field.
    pub const EDITABLE_SINGLE_LINE: Self = Self {
        protected: false,
        single_line: true,
    };

    /// This status with `protected` set.
    #[must_use]
    pub const fn with_protected(self, protected: bool) -> Self {
        Self { protected, ..self }
    }
}

/// The reads a platform makes under any lock.
///
/// Every offset is a [`Utf16Offset`]; one that splits a surrogate pair or
/// lies past the end is an [`OffsetError`](super::utf16::OffsetError) wrapped
/// in [`TextStoreError::Offset`], never a clamp.
pub trait TextStoreRead {
    /// The document's length in UTF-16 code units.
    fn document_len(&self) -> Utf16Offset;

    /// The text in `range`.
    ///
    /// # Errors
    ///
    /// [`TextStoreError::Protected`] on a protected store, or an offset error.
    fn text(&self, range: Utf16Range) -> Result<String, TextStoreError>;

    /// The current selection.
    fn selection(&self) -> Selection;

    /// The current composition, if one is in progress.
    fn composition(&self) -> Option<Composition>;

    /// The bounding rect of `range` in window-root logical pixels; for an
    /// empty range, the caret at that offset.
    ///
    /// # Errors
    ///
    /// [`TextStoreError::NoLayout`] when the field has no layout for its
    /// current text, or an offset error.
    fn rect_for_range(&self, range: Utf16Range) -> Result<RangeRect, TextStoreError>;

    /// The field's whole bounds in window-root logical pixels.
    ///
    /// # Errors
    ///
    /// [`TextStoreError::NoLayout`] when the field is not laid out.
    fn document_bounds(&self) -> Result<Bounds<Pixels>, TextStoreError>;

    /// The offset at `point` (window-root logical pixels). The answer is
    /// always a scalar boundary: it never splits a surrogate pair.
    ///
    /// # Errors
    ///
    /// [`TextStoreError::PointOutside`] for [`PointMode::Exact`] when the
    /// point misses the text, or [`TextStoreError::NoLayout`].
    fn index_at_point(
        &self,
        point: Point<Pixels>,
        mode: PointMode,
    ) -> Result<Utf16Offset, TextStoreError>;
}

/// The edits a platform makes under a read-write lock.
///
/// Every edit follows the same rules, which the conformance kit checks:
///
/// - [`Self::replace`] and [`Self::insert_at_selection`] leave a caret after
///   the inserted text.
/// - An edit that does not touch the composition shifts it by the edit's
///   length change; one that overlaps it (or inserts strictly inside it)
///   clears it. An edit never creates or extends a composition: only
///   [`Self::set_composition`] does.
/// - Nothing reaches the store's observer from inside the session: the
///   platform made the edit and already knows. The field sees the whole
///   session as one change, when the lock is released.
pub trait TextStoreEdit: TextStoreRead {
    /// Replace `range` with `text`.
    ///
    /// # Errors
    ///
    /// An offset error.
    fn replace(&mut self, range: Utf16Range, text: &str) -> Result<TextChange, TextStoreError>;

    /// Replace the selection with `text`.
    ///
    /// # Errors
    ///
    /// None from the built-in stores; the `Result` leaves room for a store
    /// that refuses edits.
    fn insert_at_selection(&mut self, text: &str) -> Result<TextChange, TextStoreError>;

    /// Set the selection, exactly: a platform selection is kept at any scalar
    /// boundary, including inside a grapheme cluster.
    ///
    /// # Errors
    ///
    /// An offset error.
    fn set_selection(&mut self, selection: Selection) -> Result<(), TextStoreError>;

    /// Start, move or end the composition.
    ///
    /// # Errors
    ///
    /// An offset error.
    fn set_composition(&mut self, composition: Option<Composition>) -> Result<(), TextStoreError>;
}
