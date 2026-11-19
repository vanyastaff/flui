//! What a platform reads and edits inside a lock: the session traits and
//! the values they speak.

use flui_foundation::geometry::{Bounds, Point};

use super::input_options::{Autofill, TextInputOptions};
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
        Utf16Range::try_new(start, end).expect("BUG: an ordered pair is a valid range")
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
    pub bounds: Bounds<f64>,
    /// Whether part of the range lies outside the field's visible area.
    pub clipped: bool,
}

/// How [`TextStoreRead::index_at_point`] answers a point inside a
/// character's box, and one outside every character's box: two independent
/// choices, as TSF's two `GetACPFromPoint` flags are
/// (<https://learn.microsoft.com/en-us/windows/win32/api/textstor/nf-textstor-itextstoreacp-getacpfrompoint>).
///
/// With neither, a point inside a box answers that character's start, and a
/// point outside every box is [`TextStoreError::PointOutside`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[allow(
    clippy::exhaustive_structs,
    reason = "TSF's two `GetACPFromPoint` flags, each set or not"
)]
pub struct PointMode {
    /// A point inside a character's box answers the box's edge nearer the
    /// point, the character's start or end (`GXFPF_ROUND_NEAREST`).
    pub round_to_nearest_edge: bool,
    /// A point outside every character's box answers the position nearest
    /// it instead of [`TextStoreError::PointOutside`] (`GXFPF_NEAREST`).
    pub nearest_when_outside: bool,
}

impl PointMode {
    /// The character whose box contains the point; a point outside every
    /// box is [`TextStoreError::PointOutside`].
    pub const EXACT: Self = Self {
        round_to_nearest_edge: false,
        nearest_when_outside: false,
    };

    /// The character boundary nearest the point, inside the text or out.
    pub const NEAREST: Self = Self {
        round_to_nearest_edge: true,
        nearest_when_outside: true,
    };
}

/// The store's standing properties, TSF's `TS_STATUS`, and what the field
/// asks of the input method.
///
/// A host re-reads it on [`TextStoreObserver::status_changed`](super::TextStoreObserver::status_changed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct TextStoreStatus {
    /// The text must not be read out (a password field). Text reads return
    /// [`TextStoreError::Protected`]; edits, selection and geometry still
    /// work. Protection wins over [`Self::text_input`]: a protected field is
    /// never suggested, corrected or learnt from, whatever it asks
    /// ([`Self::holds_secret`]).
    pub protected: bool,
    /// The field holds one line: Enter does not break it.
    pub single_line: bool,
    /// What the field asks of the input method and the on-screen keyboard.
    pub text_input: TextInputOptions,
}

impl TextStoreStatus {
    /// An editable, unprotected, single-line field for free text.
    pub const EDITABLE_SINGLE_LINE: Self = Self {
        protected: false,
        single_line: true,
        text_input: TextInputOptions::TEXT,
    };

    /// This status with `protected` set.
    #[must_use]
    pub const fn with_protected(self, protected: bool) -> Self {
        Self { protected, ..self }
    }

    /// This status with `single_line` set.
    #[must_use]
    pub const fn with_single_line(self, single_line: bool) -> Self {
        Self {
            single_line,
            ..self
        }
    }

    /// This status asking the input method for `text_input`.
    #[must_use]
    pub const fn with_text_input(self, text_input: TextInputOptions) -> Self {
        Self { text_input, ..self }
    }

    /// Whether the field holds a secret: it is protected, or it asks to be
    /// filled with a password or a one-time code, whose text shows on screen
    /// yet must reach no dictionary, suggestion strip or recording.
    ///
    /// The one test every host and the journal make before they let an input
    /// method suggest, correct or learn, or record what was typed;
    /// [`Self::protected`] alone decides only what hides the text (secure
    /// entry, a password keyboard, `Protected` text reads).
    #[must_use]
    pub const fn holds_secret(&self) -> bool {
        self.protected
            || matches!(
                self.text_input.autofill,
                Some(Autofill::CurrentPassword | Autofill::NewPassword | Autofill::OneTimeCode)
            )
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
    /// Validate the offsets first: a split surrogate pair or an offset past
    /// the document is [`TextStoreError::Offset`] even on a protected store.
    /// A valid range in a protected store is [`TextStoreError::Protected`].
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
    fn document_bounds(&self) -> Result<Bounds<f64>, TextStoreError>;

    /// The offset at `point` (window-root logical pixels). The answer is
    /// always a scalar boundary: it never splits a surrogate pair.
    ///
    /// # Errors
    ///
    /// [`TextStoreError::PointOutside`] when the point misses every
    /// character's box and `mode` does not ask for the nearest position
    /// ([`PointMode::nearest_when_outside`]), or [`TextStoreError::NoLayout`].
    fn index_at_point(
        &self,
        point: Point<f64>,
        mode: PointMode,
    ) -> Result<Utf16Offset, TextStoreError>;

    /// The part of the field's box the window shows, in window-root logical
    /// pixels: [`Self::document_bounds`] cut by every clip of the field's
    /// ancestors (a scroll view, an app bar's clip) and by the window. `None`
    /// when none of it is shown. What a platform draws over the field (iOS's
    /// caret, selection highlight and handles) stays inside it.
    ///
    /// No default: a store that wraps another forwards it, and one that left
    /// it out would draw a platform's selection over whatever clips the
    /// field.
    ///
    /// # Errors
    ///
    /// [`TextStoreError::NoLayout`] when the field is not laid out, or the
    /// window's tree cannot be read now.
    fn visible_bounds(&self) -> Result<Option<Bounds<f64>>, TextStoreError>;

    /// What a press at `point` (window-root logical pixels) would land on, by
    /// the window's own hit test: the field, another target drawn there (a
    /// control over the field or beside it), or nothing that takes a press.
    /// A platform that recognizes gestures over the field itself (iOS's
    /// `UITextInteraction`) begins one only on the field, or where nothing
    /// else takes the press, so every control keeps its tap.
    ///
    /// No default: a store that wraps another forwards it, and one that left
    /// it out would hand a control's tap to the platform.
    ///
    /// # Errors
    ///
    /// [`TextStoreError::NoLayout`] when the field is not laid out, or the
    /// window's tree cannot be hit-tested now.
    fn press_target_at(&self, point: Point<f64>) -> Result<PressTarget, TextStoreError>;
}

/// What a press at a point would land on ([`TextStoreRead::press_target_at`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a press lands on the field, on another target or on none: there is no fourth place"
)]
pub enum PressTarget {
    /// The field.
    Field,
    /// Another target that takes a press, drawn over the field or beside it.
    Other,
    /// Nothing that takes a press.
    Nothing,
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
