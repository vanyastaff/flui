//! The decidable half of the Android backend's input method (ADR-0247,
//! ADR-0090): how `FluiView`'s mirror and the field's `TextStore` stay one
//! document while two threads own the session.
//!
//! The input method reads and edits `FluiView`'s mirror on the Java main
//! thread and never waits for FLUI's owner thread. The owner publishes a
//! [`Snapshot`] of the focused store whenever the store changed outside the
//! input method's edits, tagged with a generation; the mirror becomes that
//! snapshot. After each input method edit (or batch of edits) the view
//! reports its state, and [`Mirror::report`] turns the report into one
//! [`ImeEdit`] against the snapshot it was made on. The owner replays the
//! edits in order ([`replay_current`]); an edit made on an older snapshot
//! than the one the owner published last is dropped, and the owner restarts
//! the view on the store's text, because a newer snapshot that only moved the
//! selection leaves the dropped edit's text in the input method. The owner
//! is the authority; the mirror never is.
//!
//! Cfg-free plain values over UTF-16, so Linux CI pins them;
//! `text::android` moves them across JNI.

#![forbid(unsafe_code)]

use flui_foundation::geometry::{Bounds, Point};
use flui_platform::keyboard::{ImeComposition, NamedKey};
use flui_platform::text_store::{
    Composition, Selection, TextInputPurpose, TextStoreEdit, TextStoreError, TextStoreRead,
    TextStoreStatus, Utf16Offset, Utf16Range,
};

use crate::text::ime::document_text;
use crate::text::web::ime::diff;

pub use crate::text::ime::PROTECTED_UNIT;

/// `InputConnection.CURSOR_UPDATE_IMMEDIATE`: send the cursor once, now.
pub const CURSOR_UPDATE_IMMEDIATE: i32 = 1;
/// `InputConnection.CURSOR_UPDATE_MONITOR`: send it whenever it changes.
pub const CURSOR_UPDATE_MONITOR: i32 = 2;

/// What `FluiView` reports after an input method edit: its mirror's text and
/// the raw offsets of `Selection` and the composing span (`-1` for none), as
/// Java counts them, in UTF-16 code units.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorState {
    /// The mirror's text.
    pub text: String,
    /// `Selection.getSelectionStart`: the selection's anchor.
    pub selection_start: i32,
    /// `Selection.getSelectionEnd`: the selection's moving end.
    pub selection_end: i32,
    /// `BaseInputConnection.getComposingSpanStart`.
    pub composition_start: i32,
    /// `BaseInputConnection.getComposingSpanEnd`.
    pub composition_end: i32,
}

/// The focused store as the owner last published it to the view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// Which publication this is; every edit names the one it was made on.
    pub generation: u32,
    /// The text, or [`PROTECTED_UNIT`]s for a protected field.
    pub text: String,
    /// The store's selection.
    pub selection: Selection,
    /// The store's composition.
    pub composition: Option<Utf16Range>,
}

impl Snapshot {
    /// The store as `session` reads it, published as `generation`.
    #[must_use]
    pub fn read(session: &dyn TextStoreRead, generation: u32) -> Self {
        Self {
            generation,
            text: document_text(session),
            selection: session.selection(),
            composition: session.composition().map(|composition| composition.range),
        }
    }

    /// The offsets `FluiView` takes: selection start and end, composition
    /// start and end, `-1` for no composition.
    #[must_use]
    pub fn java_offsets(&self) -> [i32; 4] {
        let java = |offset: Utf16Offset| i32::try_from(offset.get()).unwrap_or(i32::MAX);
        let (composition_start, composition_end) = self
            .composition
            .map_or((-1, -1), |range| (java(range.start()), java(range.end())));
        [
            java(self.selection.anchor),
            java(self.selection.active),
            composition_start,
            composition_end,
        ]
    }
}

/// One input method edit, against the snapshot it was made on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImeEdit {
    /// The snapshot generation the view held when it made the edit.
    pub base: u32,
    /// The span of that text the edit replaced, and what replaced it; `None`
    /// when only the selection or the composition moved.
    pub replaced: Option<(Utf16Range, String)>,
    /// The selection after the edit.
    pub selection: Selection,
    /// The composition after the edit.
    pub composition: Option<Utf16Range>,
}

impl ImeEdit {
    /// Whether the input method composes after this edit.
    #[must_use]
    pub const fn composes(&self) -> bool {
        self.composition.is_some()
    }

    /// Apply the edit to `session`: the replacement, then the selection and
    /// the composition the view ended with.
    ///
    /// # Errors
    ///
    /// The store's refusal of a step; the steps before it stay applied.
    pub fn replay(&self, session: &mut dyn TextStoreEdit) -> Result<(), TextStoreError> {
        if let Some((range, text)) = &self.replaced {
            session.replace(*range, text)?;
        }
        session.set_selection(self.selection)?;
        session.set_composition(self.composition.map(|range| Composition {
            range,
            hides_caret: false,
        }))
    }
}

/// What [`replay_current`] did with an edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Replayed {
    /// The store took it.
    Applied,
    /// It was made on an older snapshot than the owner's latest, which resets
    /// the view: dropped.
    Stale,
    /// The store refused a step: the view no longer mirrors the store, and
    /// the owner publishes a new snapshot.
    Refused,
}

impl Replayed {
    /// Whether the view must restart on a fresh snapshot of the store. A
    /// stale edit was dropped after the owner's newer snapshot reached the
    /// view as a selection-only update, which only tells the input method:
    /// the character it committed would otherwise stay in its cached text
    /// while the field never received it.
    #[must_use]
    pub const fn restarts_the_view(self) -> bool {
        !matches!(self, Self::Applied)
    }
}

/// Replay `edit` into `session` if it was made on the snapshot the owner
/// published last (`published`).
pub fn replay_current(edit: &ImeEdit, published: u32, session: &mut dyn TextStoreEdit) -> Replayed {
    if edit.base != published {
        return Replayed::Stale;
    }
    match edit.replay(session) {
        Ok(()) => Replayed::Applied,
        Err(_) => Replayed::Refused,
    }
}

/// The Java-thread half: the text of the snapshot the view holds now, which
/// each report is diffed against.
#[derive(Debug, Default)]
pub struct Mirror {
    generation: u32,
    units: Vec<u16>,
}

impl Mirror {
    /// The view now holds `snapshot`.
    pub fn install(&mut self, snapshot: &Snapshot) {
        self.hold(snapshot.generation, &snapshot.text);
    }

    /// The view now holds `text`, published as `generation`: a snapshot's
    /// text, which is all of it the reports are diffed against.
    pub fn hold(&mut self, generation: u32, text: &str) {
        self.generation = generation;
        self.units = text.encode_utf16().collect();
    }

    /// The generation of the snapshot the view holds.
    #[must_use]
    pub const fn generation(&self) -> u32 {
        self.generation
    }

    /// The edit that turned the mirror into `state`, which the view reported
    /// holding snapshot `generation`; `None` when the report is about an older
    /// snapshot than the one published since (the owner drops such edits
    /// anyway) or changes nothing.
    ///
    /// Offsets are clamped into the new text and moved off the middle of a
    /// surrogate pair, which a store refuses.
    pub fn report(&mut self, generation: u32, state: &EditorState) -> Option<ImeEdit> {
        if generation != self.generation {
            return None;
        }
        let units: Vec<u16> = state.text.encode_utf16().collect();
        let replaced = diff(&self.units, &units).map(|change| {
            let range = Utf16Range::new(
                Utf16Offset::new(change.start),
                Utf16Offset::new(change.old_end),
            )
            .expect("BUG: a diff is ordered");
            (
                range,
                String::from_utf16_lossy(&units[change.start..change.new_end]),
            )
        });
        let at = |offset: i32| Utf16Offset::new(scalar_boundary(&units, offset));
        let end = units.len();
        let selection = if state.selection_start < 0 || state.selection_end < 0 {
            Selection::collapsed(Utf16Offset::new(end))
        } else {
            Selection {
                anchor: at(state.selection_start),
                active: at(state.selection_end),
            }
        };
        let composition = (state.composition_start >= 0 && state.composition_end >= 0)
            .then(|| {
                let (start, end) = (
                    at(state.composition_start.min(state.composition_end)),
                    at(state.composition_start.max(state.composition_end)),
                );
                Utf16Range::new(start, end).expect("BUG: ordered offsets make a range")
            })
            .filter(|range| !range.is_empty());
        self.units = units;
        Some(ImeEdit {
            base: generation,
            replaced,
            selection,
            composition,
        })
    }
}

/// `offset` clamped into `units` and moved back off the low half of a
/// surrogate pair.
fn scalar_boundary(units: &[u16], offset: i32) -> usize {
    let offset = usize::try_from(offset).unwrap_or(0).min(units.len());
    let splits_pair = offset > 0
        && offset < units.len()
        && matches!(units[offset - 1], 0xD800..=0xDBFF)
        && matches!(units[offset], 0xDC00..=0xDFFF);
    if splits_pair { offset - 1 } else { offset }
}

/// A key the native queue delivers while the input method composes belongs
/// to the input method. Android hands every key to the input method before
/// native code (`NativePreImeInputStage` pre-dispatches it), so a key that
/// reaches FLUI during a composition is one the input method passed on; it
/// arrives marked [`ImeComposition::Active`] so no shortcut, focus traversal
/// or drag cancellation takes it from the composition.
#[must_use]
pub const fn key_composition(composing: bool) -> ImeComposition {
    if composing {
        ImeComposition::Active
    } else {
        ImeComposition::Inactive
    }
}

/// `EditorInfo.TYPE_CLASS_TEXT`.
pub const TYPE_CLASS_TEXT: i32 = 0x1;
/// `EditorInfo.TYPE_TEXT_VARIATION_PASSWORD`.
pub const TYPE_TEXT_VARIATION_PASSWORD: i32 = 0x80;
/// `EditorInfo.TYPE_TEXT_FLAG_MULTI_LINE`.
pub const TYPE_TEXT_FLAG_MULTI_LINE: i32 = 0x2_0000;
/// `EditorInfo.IME_ACTION_DONE`.
pub const IME_ACTION_DONE: i32 = 6;
/// `EditorInfo.IME_ACTION_NEXT`.
pub const IME_ACTION_NEXT: i32 = 5;
/// `EditorInfo.IME_ACTION_PREVIOUS`.
pub const IME_ACTION_PREVIOUS: i32 = 7;
/// `EditorInfo.IME_FLAG_NO_ENTER_ACTION`: Enter inserts a line break.
pub const IME_FLAG_NO_ENTER_ACTION: i32 = 0x4000_0000;
/// `EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING`.
pub const IME_FLAG_NO_PERSONALIZED_LEARNING: i32 = 0x0100_0000;

/// `EditorInfo.TYPE_CLASS_NUMBER`.
pub const TYPE_CLASS_NUMBER: i32 = 0x2;
/// `EditorInfo.TYPE_CLASS_PHONE`.
pub const TYPE_CLASS_PHONE: i32 = 0x3;
/// `EditorInfo.TYPE_TEXT_VARIATION_URI`.
pub const TYPE_TEXT_VARIATION_URI: i32 = 0x10;
/// `EditorInfo.TYPE_TEXT_VARIATION_EMAIL_ADDRESS`.
pub const TYPE_TEXT_VARIATION_EMAIL_ADDRESS: i32 = 0x20;
/// `EditorInfo.IME_ACTION_SEARCH`.
pub const IME_ACTION_SEARCH: i32 = 3;

/// The `EditorInfo` input type and options a field of `status` asks for: the
/// keyboard its purpose names (email, web address, number, phone, search), a
/// password variation that the keyboard does not learn from for a protected
/// field (which wins over the purpose), a multi-line field whose Enter breaks
/// lines, and Done otherwise.
#[must_use]
pub const fn editor_info(status: TextStoreStatus) -> (i32, i32) {
    let mut input_type = TYPE_CLASS_TEXT;
    let mut options = IME_ACTION_DONE;
    match status.purpose {
        TextInputPurpose::Email => input_type |= TYPE_TEXT_VARIATION_EMAIL_ADDRESS,
        TextInputPurpose::Url => input_type |= TYPE_TEXT_VARIATION_URI,
        TextInputPurpose::Number => input_type = TYPE_CLASS_NUMBER,
        TextInputPurpose::Phone => input_type = TYPE_CLASS_PHONE,
        TextInputPurpose::Search => options = IME_ACTION_SEARCH,
        // Free text, and any purpose added later that has no keyboard here.
        _ => {}
    }
    if status.protected {
        input_type = TYPE_CLASS_TEXT | TYPE_TEXT_VARIATION_PASSWORD;
        options |= IME_FLAG_NO_PERSONALIZED_LEARNING;
    }
    if !status.single_line && input_type & 0xf == TYPE_CLASS_TEXT {
        input_type |= TYPE_TEXT_FLAG_MULTI_LINE;
        options |= IME_FLAG_NO_ENTER_ACTION;
    }
    (input_type, options)
}

/// The key an editor action (`performEditorAction`) arrives as, and whether
/// Shift is held: Next is Tab, Previous is Shift+Tab, and every other action
/// (Done, Go, Search, Send) is Enter, which a field answers as it answers a
/// keyboard's.
#[must_use]
pub const fn editor_action_key(action: i32) -> (NamedKey, bool) {
    match action {
        IME_ACTION_NEXT => (NamedKey::Tab, false),
        IME_ACTION_PREVIOUS => (NamedKey::Tab, true),
        _ => (NamedKey::Enter, false),
    }
}

/// Where the caret and the field are for `CursorAnchorInfo`, in the view's
/// physical pixels: caret left, top and bottom, then the field's left, top,
/// right and bottom (a negative width when the field has no layout). `None`
/// when a coordinate would not be finite.
#[must_use]
pub fn cursor_anchor(
    caret: Bounds<f64>,
    field: Option<Bounds<f64>>,
    scale: f64,
) -> Option<[f32; 7]> {
    let pixel = |value: f64| {
        let scaled = value * scale;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "CursorAnchorInfo takes floats; finite pixels fit"
        )]
        let narrowed = scaled as f32;
        narrowed.is_finite().then_some(narrowed)
    };
    let (field_left, field_top, field_right, field_bottom) = match field {
        Some(field) => (
            pixel(field.origin.x)?,
            pixel(field.origin.y)?,
            pixel(field.origin.x + field.size.width)?,
            pixel(field.origin.y + field.size.height)?,
        ),
        None => (0.0, 0.0, -1.0, 0.0),
    };
    Some([
        pixel(caret.origin.x)?,
        pixel(caret.origin.y)?,
        pixel(caret.origin.y + caret.size.height)?,
        field_left,
        field_top,
        field_right,
        field_bottom,
    ])
}

/// Whether a stylus press at `point` starts the input method's handwriting, on
/// a device that has it (API 33): a pen tip down inside the focused field's
/// bounds. Android's automatic initiation sees only the stylus events native
/// code leaves unhandled, so FLUI, which hit-tests on its owner, asks for it
/// explicitly.
#[must_use]
pub fn starts_handwriting(
    pen_tip_down: bool,
    field: Option<Bounds<f64>>,
    point: Point<f64>,
) -> bool {
    pen_tip_down
        && field.is_some_and(|field| {
            point.x >= field.origin.x
                && point.y >= field.origin.y
                && point.x <= field.origin.x + field.size.width
                && point.y <= field.origin.y + field.size.height
        })
}

#[cfg(test)]
mod tests;
