//! [`TextEditingController`] — owns the text buffer and selection for an
//! [`EditableText`](super::editable_text::EditableText) field. The caret is
//! the collapsed case of that selection, not a separate value:
//! [`TextEditingController::caret_byte_offset`] reports the extent and
//! [`TextEditingController::selection`] the span.

use std::ops::Range;
use std::sync::{Arc, Mutex, PoisonError};

use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};

use flui_foundation::ListenerId;
use flui_foundation::notifier::{ChangeNotifier, Listenable, ListenerCallback};

// ============================================================================
// ControllerInner
// ============================================================================

/// The selection, as Flutter models it: the caret is a **collapsed
/// selection**, not a separate concept (`services/text_editing.dart`'s
/// `TextSelection`, whose `TextSelection.collapsed` sets `baseOffset ==
/// extentOffset`).
///
/// # Why one field, not `anchor` beside `caret`
///
/// The same reasoning [`ComposingState`] records for folding its two fields
/// into one option. Two independent `usize`s carry an invariant — "the anchor
/// equals the caret unless a selection is active" — that nothing enforces:
/// every mutator that collapses would have to remember to write BOTH, and one
/// that forgets leaves a phantom selection that paints a highlight the user
/// never made. Making the caret a projection of the selection means there is
/// no second field to forget.
///
/// Both offsets are byte offsets into [`ControllerInner::text`] and always sit
/// on UTF-8 char boundaries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Selection {
    /// Where the selection started — Flutter's `baseOffset`. Unmoved by
    /// extension; a drag moves the caret, not this.
    pub(super) anchor: usize,
    /// Where the caret is — Flutter's `extentOffset`. The end a further
    /// extension moves.
    pub(super) caret: usize,
}

impl ControllerInner {
    /// Remove the selected span from `text` and return the offset the caret
    /// belongs at afterwards — the span's start, which is where a replacement
    /// is inserted and where a deletion leaves the caret.
    ///
    /// A no-op for a collapsed selection, returning the caret unchanged, so
    /// every caller can call it unconditionally.
    fn delete_selected_range(&mut self) -> usize {
        let range = self.selection.range();
        if range.is_empty() {
            return self.selection.caret;
        }
        self.text.drain(range.clone());
        range.start
    }
}

impl Selection {
    /// A caret: a selection with nothing between its ends.
    pub(super) const fn collapsed(offset: usize) -> Self {
        Self {
            anchor: offset,
            caret: offset,
        }
    }

    /// The selected span in ascending order, which is what text operations
    /// need — the anchor may sit after the caret when the user dragged
    /// backwards.
    pub(super) const fn range(self) -> Range<usize> {
        if self.anchor <= self.caret {
            self.anchor..self.caret
        } else {
            self.caret..self.anchor
        }
    }

    const fn is_collapsed(self) -> bool {
        self.anchor == self.caret
    }

    /// The positive of [`Self::is_collapsed`]: something is actually
    /// selected. Named rather than negated at each site because every caller
    /// is asking "is there a selection to replace/delete/collapse", and that
    /// question reads forwards.
    const fn is_extended(self) -> bool {
        !self.is_collapsed()
    }
}

/// Mutable interior of a [`TextEditingController`].
///
/// Guarded by a `Mutex` inside `Arc` so any clone of the controller refers to
/// the same live text and caret state.
///
/// The field's text store (`super::text_store`) reads and writes these fields
/// directly through [`TextEditingController::with_inner`] and
/// [`TextEditingController::with_inner_silent`]: a platform session applies
/// its edits in one write and notifies once.
pub(super) struct ControllerInner {
    pub(super) text: String,
    /// The selection, of which the caret is the collapsed case — see
    /// [`Selection`].
    pub(super) selection: Selection,
    /// The in-progress IME composition, if any. `None` means no composition
    /// is active — see [`ComposingState`]'s doc for why its two fields are
    /// folded into one option rather than a sibling `caret_hidden: bool`
    /// field tracked independently.
    pub(super) composing: Option<ComposingState>,
}

/// The in-progress IME composition: its byte range into
/// [`ControllerInner::text`] plus whether the caret should stay hidden while
/// the IME owns its position.
///
/// # Why one option, not a sibling bool
///
/// An earlier shape tracked `composing: Option<Range<usize>>` and a
/// hypothetical `caret_hidden: bool` as two independent fields. That shape is
/// structurally leak-prone: nothing stops `caret_hidden` from staying `true`
/// after composition ends unless every single site that clears `composing`
/// remembers to *also* clear `caret_hidden` — a rule enforced by convention,
/// not the type system. Folding both into one `Option<ComposingState>` makes
/// the leak impossible instead of merely disciplined: every `composing =
/// None` site (the text store ending a composition, and every non-IME
/// mutator — [`TextEditingController::insert_str`]
/// /[`backspace`](TextEditingController::backspace)/
/// [`delete_forward`](TextEditingController::delete_forward)) drops
/// `caret_hidden` for free, along with the range it was never meaningful
/// without.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ComposingState {
    /// Byte range into `text`, on char boundaries: the text store converts
    /// and checks every platform offset before writing one here, and every
    /// non-IME edit clears the composition rather than leaving the range
    /// describing text that moved.
    pub(super) range: Range<usize>,
    /// Whether the caret should be hidden because the IME currently owns
    /// its position — winit's `ImeEvent::Preedit { cursor: None, .. }`
    /// signal. Cleared (implicitly, by this whole struct going away) on
    /// commit, on `Disabled`, and on any non-IME edit; explicitly reset to
    /// `false` by a direct caret-navigation call while composing stays
    /// active (see [`TextEditingController::move_caret_left`] and its
    /// siblings) — the user taking the caret back means the IME no longer
    /// owns its position, even though the composition itself continues.
    pub(super) caret_hidden: bool,
}

// ============================================================================
// TextEditingController
// ============================================================================

/// Owns the text buffer and caret position for a text input field.
///
/// Flutter parity: `widgets/editable_text.dart` `TextEditingController`.
///
/// # Sharing
///
/// `TextEditingController` is `Clone`: every clone shares the same underlying
/// buffer and listener list (both are `Arc`-backed internally).  The owning
/// widget state typically holds one clone; the key-event handler closure
/// captures a second.  `notify_listeners` propagates through every clone so
/// a listener added to any one clone fires regardless of which clone mutates
/// the buffer.
///
/// # Listening
///
/// Implement a reactive rebuild by registering via [`Listenable::add_listener`]
/// (returns a [`ListenerId`] for later removal in [`Listenable::remove_listener`])
/// or by passing [`TextEditingController::listenable`] to
/// [`AnimatedBuilder`](crate::AnimatedBuilder).
///
/// # IME composition
///
/// The controller holds Flutter's `TextEditingValue.composing` model, but
/// only a mounted [`EditableText`](super::EditableText)'s text store edits
/// it: the platform's input method reads and edits the field through that
/// store (ADR-0090), and a push-model `ImeEvent` is projected onto the same
/// store (`flui_platform_api::text_store::project_ime_event`), so there is
/// one editing path. The controller exposes what the rest of the widget
/// reads: [`Self::composing_range`], [`Self::is_composing`] and the **hidden
/// caret** case (`cursor: None` on a preedit event, winit's own semantics for
/// "the IME wants no caret drawn") through [`Self::caret_hidden_by_ime`] —
/// the owning `EditableTextState` consults it to suppress the painted caret
/// while composition still paints its own underline (see ADR-0030).
/// Composition end — a commit, `Disabled`, a non-IME edit, or `Preedit`
/// cancellation — always drops the whole internal composing state, so the
/// hidden-caret flag can never outlive the composition it describes.
///
/// # DEFERRED (v1)
///
/// The following behaviors are absent in v1 and must not be faked:
/// - **Multi-tap and shift-click selection**: a selection is tracked,
///   rendered, honoured by every edit, produced by a tap or a drag on the
///   field, and extended from the keyboard with Shift. Double-tap word
///   selection has since landed (`EditableTextState::wrap_double_tap_word_select`,
///   `flui-widgets`). Still absent: shift-click extension and triple-tap
///   line selection — plus the selection handles and toolbar.
/// - **Clipboard**: copy/paste/cut are not wired.
/// - **Input formatters**: no validation or transformation pipeline.
///
/// # Character unit
///
/// Caret movement ([`Self::move_caret_left`]/[`Self::move_caret_right`],
/// [`Self::extend_selection_left`]/[`Self::extend_selection_right`]) and
/// single-character deletion ([`Self::backspace`]/[`Self::delete_forward`])
/// step by **extended grapheme cluster** (UAX #29), the user-perceived
/// character Flutter's `characters` package / `CharacterRange` walks. A
/// Zero-Width-Joiner sequence (`'👨‍👩‍👦'`), a regional-indicator flag (`'🇺🇸'`)
/// or a base letter with combining marks (`"e\u{301}"`) is one step and one
/// deletion; stepping by Unicode scalar instead would leave a dangling joiner
/// rendered as a broken partial glyph. Offsets stay UTF-8 byte offsets — a
/// grapheme boundary is always a `char` boundary, so every slice below stays
/// valid.
///
/// # Word unit
///
/// Word-boundary movement ([`Self::move_caret_word_left`]/
/// [`Self::move_caret_word_right`], [`Self::extend_selection_word_left`]/
/// [`Self::extend_selection_word_right`]) and word deletion
/// ([`Self::delete_word_backward`]/[`Self::delete_word_forward`]) step by
/// UAX #29 **word** segments (`unicode-segmentation`'s
/// `split_word_bound_indices`), not ASCII whitespace runs: a
/// straight/curly apostrophe inside a word (`"don't"`) and a letter-digit
/// run (`"foo123"`) each stay one segment per the standard's own rules,
/// and a run of plain whitespace is skipped as a whole rather than
/// becoming its own stop. Clusters-only, not dictionary-based — CJK
/// (segments per character/script-run, not per linguistic word) and
/// Thai/Lao/Khmer/Myanmar (no boundary at all without a lexicon) are
/// known limitations; see `flui-widgets/ARCHITECTURE.md`'s Mapping
/// decision #19.
///
/// Forward and backward are deliberately NOT mirror images of each other,
/// matching the convention most editors already use: forward always
/// advances to the **next** word's start (finishing whichever segment the
/// caret currently touches, word or whitespace, then skipping any
/// whitespace beyond it); backward returns to the **current** word's own
/// start without skipping it — only a caret already sitting exactly at a
/// word's start, or inside/after pure whitespace, continues back to the
/// *previous* word's start. Word movement collapses an active selection
/// to its edge without a further jump, the same rule
/// [`Self::move_caret_left`] documents for character movement.
///
/// This is a SEPARATE implementation from `flui-painting`'s
/// `TextLayout::get_word_boundary` (neither is a doc link here:
/// `flui-painting` is a dev-dependency of this crate, not a regular one,
/// and `EditableTextState::wrap_double_tap_word_select`, which calls it,
/// is private to `editable_text.rs`), which backs double-tap word
/// selection one layer up — same underlying crate
/// (`unicode-segmentation`), different tie-break, by design, because the
/// two answer different questions: a directional jump here, a positional
/// lookup there, not expected to agree at every boundary.
#[derive(Clone)]
pub struct TextEditingController {
    /// Shared text buffer + caret state.
    inner: Arc<Mutex<ControllerInner>>,
    /// Listener list — `ChangeNotifier` is itself `Arc`-backed so clones share
    /// the same list.
    notifier: ChangeNotifier,
}

impl TextEditingController {
    /// Whether `self` and `other` are clones of the SAME controller.
    ///
    /// Identity, not value: two controllers holding the same text are
    /// different controllers, and a caller swapping one for another expects
    /// the swap to be noticed even when the text happens to match. Clones
    /// share the buffer, so `Clone` preserves identity — which is what makes
    /// this the right question for "did the parent hand me a different
    /// controller?".
    ///
    /// Deliberately a named method rather than `PartialEq`: `==` on a value
    /// type reads as value equality, and answering `false` for two controllers
    /// with identical text under that spelling would be a trap. The reference
    /// compares Dart object identity for the same purpose
    /// (`text_field.dart`'s `didUpdateWidget`).
    #[must_use]
    pub fn is_same_controller(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// How many change listeners are registered.
    ///
    /// Crate-internal, and it exists for one reason: a test that claims a
    /// mounted field registers its listener exactly once, and moves it rather
    /// than duplicating it on a controller swap, has to be able to SEE that.
    /// Asserting the visible text instead would pass just as well against a
    /// field that had accumulated a listener per rebuild.
    ///
    /// A shipped build has no caller: the integration tests read it through
    /// `crate::__test_access::TextEditingControllerProbe` (ADR-0083 §4), and it
    /// leaves with that module.
    #[must_use]
    pub(crate) fn listener_count(&self) -> usize {
        self.notifier.len()
    }
}

impl std::fmt::Debug for TextEditingController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        f.debug_struct("TextEditingController")
            .field("text", &guard.text)
            .field("selection", &guard.selection)
            .field("composing", &guard.composing)
            // `notifier` is intentionally omitted: its Arc-backed listener list
            // is noise in debug output and has no stable representation.
            .finish_non_exhaustive()
    }
}

impl Default for TextEditingController {
    fn default() -> Self {
        Self::new()
    }
}

impl TextEditingController {
    /// Create a controller with an empty text buffer and caret at position 0.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ControllerInner {
                text: String::new(),
                selection: Selection::collapsed(0),
                composing: None,
            })),
            notifier: ChangeNotifier::new(),
        }
    }

    /// Create a controller pre-populated with `initial_text`, caret at the end.
    #[must_use]
    pub fn with_text(initial_text: impl Into<String>) -> Self {
        let text = initial_text.into();
        let selection = Selection::collapsed(text.len());
        Self {
            inner: Arc::new(Mutex::new(ControllerInner {
                text,
                selection,
                composing: None,
            })),
            notifier: ChangeNotifier::new(),
        }
    }

    // =========================================================================
    // Read accessors
    // =========================================================================

    /// A snapshot of the current text buffer.
    pub fn text(&self) -> String {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .text
            .clone()
    }

    /// The current caret position as a byte offset into [`Self::text`].
    ///
    /// Always points to a valid UTF-8 char boundary (including one past the
    /// last byte when the caret is at the end).
    pub fn caret_byte_offset(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .selection
            .caret
    }

    /// The selected span as a byte range into [`Self::text`], in ascending
    /// order.
    ///
    /// Empty when nothing is selected — the caret is a collapsed selection
    /// (Flutter's `TextSelection.collapsed`), so
    /// `selection().is_empty() == true` and `selection().start ==
    /// caret_byte_offset()` is the resting state of a focused field.
    ///
    /// Ascending order, not anchor-then-caret: the anchor sits *after* the
    /// caret whenever the user dragged backwards, and every consumer of this
    /// — painting a highlight, deleting a range — needs the span, not the
    /// direction. [`Self::caret_byte_offset`] is where the direction lives.
    #[must_use]
    pub fn selection(&self) -> Range<usize> {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .selection
            .range()
    }

    /// Whether anything is selected, i.e. the selection is not collapsed.
    #[must_use]
    pub fn has_selection(&self) -> bool {
        !self
            .inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .selection
            .is_collapsed()
    }

    /// The selected text, empty when the selection is collapsed — Flutter's
    /// `TextSelection.textInside(text)`, what a copy writes to the clipboard.
    #[must_use]
    pub fn selected_text(&self) -> String {
        let guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let range = guard.selection.range();
        guard.text[range].to_owned()
    }

    // =========================================================================
    // Mutation — each method notifies listeners after the change
    // =========================================================================

    /// Place the caret at `offset`, discarding any selection.
    ///
    /// `offset` is clamped to the buffer and snapped forward to an
    /// extended-grapheme-cluster boundary (see the type doc's "Character
    /// unit"), so a caller working from a hit test can neither slice a
    /// codepoint nor park the caret inside one user-perceived character.
    /// Notifies only on an actual change.
    ///
    /// Does **not** clear the composing region: moving the caret is not a text
    /// edit, and the IME keeps owning its composition. It does clear
    /// [`Self::caret_hidden_by_ime`], for the reason
    /// [`Self::move_caret_left`] documents — the user reaching for the caret
    /// directly means the IME no longer owns its position.
    pub fn set_caret_byte_offset(&self, offset: usize) {
        self.set_selection(offset, offset);
    }

    /// Select from `anchor` to `extent`, leaving the caret at `extent`.
    ///
    /// The two are given in the order the user made them — a backwards drag
    /// passes an `anchor` greater than the `extent` — so that a later
    /// extension moves the right end. [`Self::selection`] normalises.
    ///
    /// Both offsets are clamped to the buffer and snapped forward to
    /// extended-grapheme-cluster boundaries (see the type doc's "Character
    /// unit"): an offset inside a ZWJ sequence or a base-plus-combining-mark
    /// lands after that cluster. Passing the same value twice is a collapse,
    /// which is what [`Self::set_caret_byte_offset`] is.
    ///
    /// Notifies only on an actual change, so a drag that re-reports the same
    /// offset — which a pointer-move stream does constantly — does not
    /// rebuild the field on every event.
    pub fn set_selection(&self, anchor: usize, extent: usize) {
        let changed = {
            let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            let next = Selection {
                anchor: clamp_to_grapheme_boundary(&guard.text, anchor),
                caret: clamp_to_grapheme_boundary(&guard.text, extent),
            };
            let moved = guard.selection != next;
            guard.selection = next;
            let unhid = clear_caret_hidden(&mut guard);
            moved || unhid
        };
        if changed {
            self.notifier.notify_listeners();
        }
    }

    /// Insert `text` at the current caret position and advance the caret past it.
    ///
    /// Clears any active composing region — Flutter parity:
    /// `TextEditingController`'s `text` setter resets `composing` to empty
    /// on every programmatic change (`editable_text.dart`, tag `3.44.0`).
    /// A stale composing region left pointing at a now-shifted buffer is
    /// exactly the "stored range no longer describes the current text" bug
    /// class this controller must not reintroduce — this is a non-IME edit,
    /// so IME composition state does not survive it.
    ///
    /// Notifies listeners after the insertion.
    pub fn insert_str(&self, text: &str) {
        {
            let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            // A non-collapsed selection is REPLACED, which is what every text
            // editor does and what `TextEditingController.text`'s setter
            // amounts to in Flutter. Deleting first and inserting at the
            // range's start keeps this one notification, not two.
            let at = guard.delete_selected_range();
            guard.text.insert_str(at, text);
            guard.selection = Selection::collapsed(at + text.len());
            guard.composing = None;
        }
        self.notifier.notify_listeners();
    }

    /// Replace the whole buffer with `text`, ignoring the current selection —
    /// the programmatic counterpart to typing. Clears any active composing
    /// region, the same non-IME-edit rule [`Self::insert_str`] documents.
    ///
    /// **Divergence from Flutter, deliberate:** `TextEditingController.text`'s
    /// setter (`editable_text.dart`) also replaces the value wholesale, but
    /// collapses the selection to `TextSelection.collapsed(offset: -1)` — an
    /// off-the-end sentinel that does not paint a caret at all until
    /// something else moves it. That is a common source of "my caret
    /// disappeared after I set `.text`" surprise in Flutter itself. This
    /// method collapses the caret to `text.len()` instead — the visible,
    /// unsurprising place to leave it after a programmatic replacement — and
    /// that choice is the whole point of diverging here, not an oversight.
    ///
    /// A no-op (no notification) when `text` already equals the current
    /// buffer — the same "notify only on a real change" rule most mutators
    /// here follow ([`Self::insert_str`] is the exception: it notifies
    /// unconditionally on every call) — so a
    /// caller that calls this unconditionally on every build does not force
    /// a rebuild loop.
    pub fn set_text(&self, text: impl Into<String>) {
        let text = text.into();
        let changed = {
            let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            if guard.text == text {
                false
            } else {
                guard.selection = Selection::collapsed(text.len());
                guard.text = text;
                guard.composing = None;
                true
            }
        };
        if changed {
            self.notifier.notify_listeners();
        }
    }

    /// Empty the buffer and collapse the caret to `0`.
    ///
    /// Flutter parity: `TextEditingController.clear()` (`editable_text.dart`).
    /// Defined in terms of [`Self::set_text`] so the two cannot drift — same
    /// no-op-when-already-empty rule, same composing-region reset.
    pub fn clear(&self) {
        self.set_text(String::new());
    }

    /// Delete the character immediately to the **left** of the caret (Backspace).
    ///
    /// No-op when the caret is at the beginning of the buffer. Clears any
    /// active composing region on an actual deletion — see
    /// [`Self::insert_str`]'s doc for why a non-IME text edit must not
    /// leave a stale composing range behind.
    pub fn backspace(&self) {
        let changed = {
            let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            // With a selection, Backspace deletes the selection rather than
            // one character — the character before its start is not part of
            // what the user asked to remove.
            if guard.selection.is_extended() {
                let at = guard.delete_selected_range();
                guard.selection = Selection::collapsed(at);
                guard.composing = None;
                true
            } else {
                let caret = guard.selection.caret;
                if caret == 0 {
                    false
                } else {
                    // Walk back to the previous grapheme boundary.
                    let prev_boundary = prev_grapheme_boundary(&guard.text, caret);
                    guard.text.drain(prev_boundary..caret);
                    guard.selection = Selection::collapsed(prev_boundary);
                    guard.composing = None;
                    true
                }
            }
        };
        if changed {
            self.notifier.notify_listeners();
        }
    }

    /// Delete the character immediately to the **right** of the caret (Delete key).
    ///
    /// No-op when the caret is at the end of the buffer. Clears any active
    /// composing region on an actual deletion — see [`Self::insert_str`]'s
    /// doc for why a non-IME text edit must not leave a stale composing
    /// range behind.
    pub fn delete_forward(&self) {
        let changed = {
            let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            // Same rule as Backspace: a selection is what gets deleted.
            if guard.selection.is_extended() {
                let at = guard.delete_selected_range();
                guard.selection = Selection::collapsed(at);
                guard.composing = None;
                true
            } else {
                let caret = guard.selection.caret;
                if caret == guard.text.len() {
                    false
                } else {
                    // Width of the grapheme starting at `caret`.
                    let next_boundary = next_grapheme_boundary(&guard.text, caret);
                    guard.text.drain(caret..next_boundary);
                    guard.composing = None;
                    true
                }
            }
        };
        if changed {
            self.notifier.notify_listeners();
        }
    }

    /// Move the selection's EXTENT one character left, leaving the anchor —
    /// Shift+Left.
    ///
    /// Flutter's `ExtendSelectionByCharacterIntent(collapseSelection: false)`:
    /// *"Moves the selection's [TextSelection.extent] past the user-perceived
    /// character before/after it"* (`widgets/editable_text.dart:697`). The
    /// contrast with [`Self::move_caret_left`] is the whole point of the pair:
    /// unmodified, an arrow COLLAPSES a selection to its edge and stops;
    /// modified, it steps the caret from wherever it is and grows or shrinks
    /// the span. A selection dragged rightwards then shrunk with Shift+Left
    /// therefore narrows rather than jumping.
    ///
    /// Also clears [`Self::caret_hidden_by_ime`], for the reason
    /// [`Self::move_caret_left`] documents.
    pub fn extend_selection_left(&self) {
        self.extend_to(|guard| {
            let caret = guard.selection.caret;
            (caret != 0).then(|| prev_grapheme_boundary(&guard.text, caret))
        });
    }

    /// Move the selection's extent one character right — Shift+Right. The
    /// mirror of [`Self::extend_selection_left`].
    pub fn extend_selection_right(&self) {
        self.extend_to(|guard| {
            let caret = guard.selection.caret;
            (caret != guard.text.len()).then(|| next_grapheme_boundary(&guard.text, caret))
        });
    }

    /// Move the selection's extent to the start of the buffer —
    /// Shift+Home.
    pub fn extend_selection_home(&self) {
        self.extend_to(|_| Some(0));
    }

    /// Move the selection's extent to the end of the buffer — Shift+End.
    pub fn extend_selection_end(&self) {
        self.extend_to(|guard| Some(guard.text.len()));
    }

    /// Shared body of the four extend operations: move the caret to whatever
    /// `next` computes, leave the anchor, notify only on a real change.
    ///
    /// One function rather than four copies because "leave the anchor" is the
    /// property that distinguishes these from the plain moves, and a copy that
    /// forgot it would silently collapse — the failure this whole shape exists
    /// to make impossible.
    fn extend_to(&self, next: impl FnOnce(&ControllerInner) -> Option<usize>) {
        let changed = {
            let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            let moved = match next(&guard) {
                Some(caret) if caret != guard.selection.caret => {
                    guard.selection.caret = caret;
                    true
                }
                _ => false,
            };
            let unhid = clear_caret_hidden(&mut guard);
            moved || unhid
        };
        if changed {
            self.notifier.notify_listeners();
        }
    }

    /// Move the caret one character to the left.
    ///
    /// No-op when the caret is at the beginning. Also clears
    /// [`Self::caret_hidden_by_ime`] when a composition is active — the user
    /// taking the caret back means the IME no longer owns its position, even
    /// though the composition itself keeps running.
    pub fn move_caret_left(&self) {
        let changed = {
            let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            // With a selection, Left COLLAPSES to its logical start and moves
            // no further — `widgets/editable_text.dart:685`,
            // `ExtendSelectionByCharacterIntent(collapseSelection: true)`:
            // "Collapses the selection to the logical start/end of the
            // selection". Collapsing *and* stepping would skip a character
            // the user can see.
            let moved = if guard.selection.is_extended() {
                guard.selection = Selection::collapsed(guard.selection.range().start);
                true
            } else {
                let caret = guard.selection.caret;
                if caret == 0 {
                    false
                } else {
                    let prev_boundary = prev_grapheme_boundary(&guard.text, caret);
                    guard.selection = Selection::collapsed(prev_boundary);
                    true
                }
            };
            // Always invoked (not short-circuited by `moved`): the flag must
            // clear even when the caret was already at the boundary — a
            // no-op move at the buffer's edge still means the user reached
            // for the caret directly.
            let unhid = clear_caret_hidden(&mut guard);
            moved || unhid
        };
        if changed {
            self.notifier.notify_listeners();
        }
    }

    /// Move the caret one character to the right.
    ///
    /// No-op when the caret is at the end. Also clears
    /// [`Self::caret_hidden_by_ime`] when a composition is active — see
    /// [`Self::move_caret_left`]'s doc.
    pub fn move_caret_right(&self) {
        let changed = {
            let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            // The mirror of Left — see its comment for the reference.
            let moved = if guard.selection.is_extended() {
                guard.selection = Selection::collapsed(guard.selection.range().end);
                true
            } else {
                let caret = guard.selection.caret;
                if caret == guard.text.len() {
                    false
                } else {
                    let next_boundary = next_grapheme_boundary(&guard.text, caret);
                    guard.selection = Selection::collapsed(next_boundary);
                    true
                }
            };
            let unhid = clear_caret_hidden(&mut guard);
            moved || unhid
        };
        if changed {
            self.notifier.notify_listeners();
        }
    }

    /// Move the caret to the beginning of the buffer (Home).
    ///
    /// No-op when the caret is already at position 0. Also clears
    /// [`Self::caret_hidden_by_ime`] when a composition is active — see
    /// [`Self::move_caret_left`]'s doc.
    pub fn move_caret_home(&self) {
        let changed = {
            let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            let moved = if guard.selection == Selection::collapsed(0) {
                false
            } else {
                guard.selection = Selection::collapsed(0);
                true
            };
            let unhid = clear_caret_hidden(&mut guard);
            moved || unhid
        };
        if changed {
            self.notifier.notify_listeners();
        }
    }

    /// Move the caret to the end of the buffer (End).
    ///
    /// No-op when the caret is already at the end. Also clears
    /// [`Self::caret_hidden_by_ime`] when a composition is active — see
    /// [`Self::move_caret_left`]'s doc.
    pub fn move_caret_end(&self) {
        let changed = {
            let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            let end = guard.text.len();
            let moved = if guard.selection == Selection::collapsed(end) {
                false
            } else {
                guard.selection = Selection::collapsed(end);
                true
            };
            let unhid = clear_caret_hidden(&mut guard);
            moved || unhid
        };
        if changed {
            self.notifier.notify_listeners();
        }
    }

    // =========================================================================
    // Word movement
    // =========================================================================

    /// Move the caret one WORD to the left — Ctrl/Alt+Left.
    ///
    /// With an active selection, collapses to its logical start without a
    /// further jump — the same rule [`Self::move_caret_left`] documents for
    /// character movement, kept here for consistency within this
    /// controller's own API rather than a verified port of Flutter's
    /// widgets-level `Action` plumbing for the analogous intent (see the
    /// type doc's `# Word unit` section). Also clears
    /// [`Self::caret_hidden_by_ime`], for the reason
    /// [`Self::move_caret_left`] documents.
    pub fn move_caret_word_left(&self) {
        let changed = {
            let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            let moved = if guard.selection.is_extended() {
                guard.selection = Selection::collapsed(guard.selection.range().start);
                true
            } else {
                let caret = guard.selection.caret;
                if caret == 0 {
                    false
                } else {
                    let prev = prev_word_boundary(&guard.text, caret);
                    guard.selection = Selection::collapsed(prev);
                    true
                }
            };
            let unhid = clear_caret_hidden(&mut guard);
            moved || unhid
        };
        if changed {
            self.notifier.notify_listeners();
        }
    }

    /// Move the caret one WORD to the right — Ctrl/Alt+Right. The mirror
    /// of [`Self::move_caret_word_left`] — see its doc.
    pub fn move_caret_word_right(&self) {
        let changed = {
            let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            let moved = if guard.selection.is_extended() {
                guard.selection = Selection::collapsed(guard.selection.range().end);
                true
            } else {
                let caret = guard.selection.caret;
                if caret == guard.text.len() {
                    false
                } else {
                    let next = next_word_boundary(&guard.text, caret);
                    guard.selection = Selection::collapsed(next);
                    true
                }
            };
            let unhid = clear_caret_hidden(&mut guard);
            moved || unhid
        };
        if changed {
            self.notifier.notify_listeners();
        }
    }

    /// Move the selection's EXTENT one WORD left, leaving the anchor —
    /// Shift+Ctrl/Alt+Left. The word-granularity counterpart of
    /// [`Self::extend_selection_left`] — see its doc for why this shares
    /// the same private `extend_to` helper rather than duplicating the
    /// anchor-preserving logic.
    pub fn extend_selection_word_left(&self) {
        self.extend_to(|guard| {
            let caret = guard.selection.caret;
            (caret != 0).then(|| prev_word_boundary(&guard.text, caret))
        });
    }

    /// Move the selection's EXTENT one WORD right, leaving the anchor —
    /// Shift+Ctrl/Alt+Right. Mirror of [`Self::extend_selection_word_left`].
    pub fn extend_selection_word_right(&self) {
        self.extend_to(|guard| {
            let caret = guard.selection.caret;
            (caret != guard.text.len()).then(|| next_word_boundary(&guard.text, caret))
        });
    }

    /// Delete the WORD immediately to the left of the caret — Ctrl+Backspace.
    ///
    /// With an active selection, deletes the selection rather than a word —
    /// same rule [`Self::backspace`] documents. No-op at the start of the
    /// buffer. Clears any active composing region on an actual deletion,
    /// same reason as [`Self::backspace`].
    pub fn delete_word_backward(&self) {
        let changed = {
            let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            if guard.selection.is_extended() {
                let at = guard.delete_selected_range();
                guard.selection = Selection::collapsed(at);
                guard.composing = None;
                true
            } else {
                let caret = guard.selection.caret;
                if caret == 0 {
                    false
                } else {
                    let prev = prev_word_boundary(&guard.text, caret);
                    guard.text.drain(prev..caret);
                    guard.selection = Selection::collapsed(prev);
                    guard.composing = None;
                    true
                }
            }
        };
        if changed {
            self.notifier.notify_listeners();
        }
    }

    /// Delete the WORD immediately to the right of the caret — Ctrl+Delete.
    /// Mirror of [`Self::delete_word_backward`], for the same reason
    /// [`Self::delete_forward`] exists beside [`Self::backspace`].
    pub fn delete_word_forward(&self) {
        let changed = {
            let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            if guard.selection.is_extended() {
                let at = guard.delete_selected_range();
                guard.selection = Selection::collapsed(at);
                guard.composing = None;
                true
            } else {
                let caret = guard.selection.caret;
                if caret == guard.text.len() {
                    false
                } else {
                    let next = next_word_boundary(&guard.text, caret);
                    guard.text.drain(caret..next);
                    guard.composing = None;
                    true
                }
            }
        };
        if changed {
            self.notifier.notify_listeners();
        }
    }

    // =========================================================================
    // IME composing region
    // =========================================================================

    /// The current composing region, if a composition is active.
    ///
    /// A byte range into [`Self::text`], always char-boundary-clamped.
    #[must_use]
    pub fn composing_range(&self) -> Option<Range<usize>> {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .composing
            .as_ref()
            .map(|state| state.range.clone())
    }

    /// Whether an IME composition is currently in progress.
    ///
    /// [`EditableText`](super::EditableText)'s key handler consults this to
    /// implement the suppression contract
    /// ([`flui_platform_api::ImeEvent`]'s doc): suppress `Key::Character` insertion
    /// **only** while this is `true` — a field must not swallow plain
    /// typing for the rest of a focus session just because IME composition
    /// happened once.
    #[must_use]
    pub fn is_composing(&self) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .composing
            .is_some()
    }

    /// Whether the caret should currently be hidden because the IME owns its
    /// position — `false` whenever no composition is active, so a caller
    /// never needs to separately check [`Self::is_composing`] first.
    ///
    /// Reflects the composition's `hides_caret` as the input method last set
    /// it (winit's `Preedit { cursor: None }` sets it `true`); a
    /// caret-navigation call (
    /// [`move_caret_left`](Self::move_caret_left)/
    /// [`move_caret_right`](Self::move_caret_right)/
    /// [`move_caret_home`](Self::move_caret_home)/
    /// [`move_caret_end`](Self::move_caret_end)) clears it back to `false`
    /// without ending the composition — the user took the caret back, so the
    /// IME no longer owns its position even though composing text is still
    /// present. [`EditableTextState`](super::EditableTextState) consults this
    /// to suppress the painted caret while the composing-region underline
    /// keeps painting (ADR-0030).
    #[must_use]
    pub fn caret_hidden_by_ime(&self) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .composing
            .as_ref()
            .is_some_and(|state| state.caret_hidden)
    }

    // =========================================================================
    // Text-store access
    // =========================================================================

    /// Read the controller's state under its lock.
    ///
    /// For the field's text store (`super::text_store`), which answers a
    /// platform's reads from these fields directly.
    pub(super) fn with_inner<R>(&self, f: impl FnOnce(&ControllerInner) -> R) -> R {
        f(&self.inner.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Change the controller's state under its lock WITHOUT notifying
    /// listeners — the caller notifies once through [`Self::notify_changed`]
    /// when it is done. A platform session writes all of its edits back this
    /// way, so the session is one change to the field, not one per edit.
    pub(super) fn with_inner_silent<R>(&self, f: impl FnOnce(&mut ControllerInner) -> R) -> R {
        f(&mut self.inner.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Notify listeners of a change made through [`Self::with_inner_silent`].
    pub(super) fn notify_changed(&self) {
        self.notifier.notify_listeners();
    }

    // =========================================================================
    // Reactive integration
    // =========================================================================

    /// Return a listenable that fires whenever the controller's text or caret
    /// changes.  Pass it to [`AnimatedBuilder`](crate::AnimatedBuilder) to
    /// rebuild a widget subtree on every edit.
    ///
    /// The returned `Arc` wraps a clone of the internal `ChangeNotifier`, which
    /// is itself `Arc`-backed — both the widget build and the key handler share
    /// the same live listener list through their respective clones.
    pub fn listenable(&self) -> Arc<dyn Listenable> {
        Arc::new(self.notifier.clone())
    }
}

// Delegate `Listenable` to the shared notifier so external code can subscribe
// directly on the controller rather than going through `controller.listenable()`.
impl Listenable for TextEditingController {
    fn add_listener(&self, listener: ListenerCallback) -> ListenerId {
        self.notifier.add_listener(listener)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.notifier.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.notifier.remove_all_listeners();
    }
}

/// Clears the active composition's `caret_hidden` flag, if one is set.
/// Returns whether it actually changed (`true` → `false`), for callers that
/// only want to notify listeners on a real change — see
/// [`TextEditingController::move_caret_left`]'s doc for why direct caret
/// navigation takes the caret back from the IME without ending composition.
fn clear_caret_hidden(guard: &mut ControllerInner) -> bool {
    match guard.composing.as_mut() {
        Some(state) if state.caret_hidden => {
            state.caret_hidden = false;
            true
        }
        _ => false,
    }
}

/// The byte offset where the extended grapheme cluster ending at `caret`
/// begins — one user-perceived character to the left. `0` at the start.
///
/// `caret` must be a char boundary of `text` (every caller holds one: the
/// controller clamps every offset it stores). It need not be a grapheme
/// boundary: the cursor walks the WHOLE string, so a caret that landed
/// strictly inside a cluster — a platform-supplied IME offset, say — is
/// resolved with the cluster's full context (UAX #29 rules such as the
/// ZWJ-sequence and regional-indicator-pair rules look at what precedes the
/// caret) and steps to that cluster's start, the nearest boundary a user can
/// see. Segmenting only the slice on one side of the caret would lose that
/// context and could answer a boundary that is not one.
fn prev_grapheme_boundary(text: &str, caret: usize) -> usize {
    let mut cursor = GraphemeCursor::new(caret, text.len(), true);
    // The whole string is the one chunk, starting at 0, so the cursor never
    // needs more context and the `Err` arms (`PreContext`/`NextChunk`, asked
    // for only when a chunk is partial) are unreachable.
    cursor.prev_boundary(text, 0).ok().flatten().unwrap_or(0)
}

/// The byte offset where the extended grapheme cluster starting at (or
/// containing) `caret` ends — one user-perceived character to the right.
/// `caret` itself at the end.
///
/// Same precondition and full-context walk as [`prev_grapheme_boundary`].
fn next_grapheme_boundary(text: &str, caret: usize) -> usize {
    let mut cursor = GraphemeCursor::new(caret, text.len(), true);
    cursor
        .next_boundary(text, 0)
        .ok()
        .flatten()
        .unwrap_or(text.len())
}

/// Whether a UAX #29 word segment contains nothing but whitespace —
/// Flutter's `RenderEditable._onlyWhitespace` (`rendering/editable.dart`),
/// reused here to decide which segments a word jump skips over versus
/// stops on.
fn is_whitespace_only_word(segment: &str) -> bool {
    segment.chars().all(char::is_whitespace)
}

/// The byte offset of the next word-jump stop forward from `offset` — see
/// [`TextEditingController`]'s `# Word unit` doc section for the full
/// forward/backward contract this implements.
///
/// Finds the segment `offset` currently touches (the first one whose end
/// is past `offset`) and returns the start of the first non-whitespace
/// segment strictly after it, or `text.len()` if none remains. Streamed,
/// not collected: `split_word_bound_indices` is walked once, forward,
/// with no intermediate `Vec` — this runs on every Ctrl/Alt+Right and
/// must not allocate a segment list for the whole buffer on every
/// keystroke.
fn next_word_boundary(text: &str, offset: usize) -> usize {
    let total = text.len();
    if text.is_empty() || offset >= total {
        return total;
    }
    let offset = clamp_to_char_boundary(text, offset);
    let mut segments = text
        .split_word_bound_indices()
        .map(|(idx, word)| (idx, idx + word.len(), is_whitespace_only_word(word)))
        .skip_while(|&(_, end, _)| end <= offset);
    // Consume the segment `offset` touches (already skipped-to by the
    // `skip_while` above) without inspecting it — a word-jump always
    // clears whatever segment it started in/on.
    segments.next();
    segments
        .find(|&(_, _, whitespace_only)| !whitespace_only)
        .map_or(total, |(start, _, _)| start)
}

/// The byte offset of the previous word-jump stop backward from `offset`
/// — see [`TextEditingController`]'s `# Word unit` doc section for the
/// full forward/backward contract this implements.
///
/// Finds the segment `offset` currently touches (the last one whose start
/// is before `offset`). If that segment is a word, returns ITS OWN start
/// without skipping it; if it is whitespace (or `offset` already sits at
/// a word's start), continues back to the start of the previous
/// non-whitespace segment, or `0` if none remains. Streamed backward via
/// `unicode_segmentation`'s `DoubleEndedIterator` support
/// (`SplitWordBoundIndices::rev`) — same no-`Vec` reasoning as
/// [`next_word_boundary`].
fn prev_word_boundary(text: &str, offset: usize) -> usize {
    if text.is_empty() || offset == 0 {
        return 0;
    }
    let offset = clamp_to_char_boundary(text, offset);
    if offset == 0 {
        return 0;
    }
    let mut segments = text
        .split_word_bound_indices()
        .map(|(idx, word)| (idx, idx + word.len(), is_whitespace_only_word(word)))
        .rev()
        .skip_while(|&(start, _, _)| start >= offset);
    let Some(touching) = segments.next() else {
        return 0;
    };
    if !touching.2 {
        return touching.0;
    }
    segments
        .find(|&(_, _, whitespace_only)| !whitespace_only)
        .map_or(0, |(start, _, _)| start)
}

/// Clamp `offset` to the nearest extended-grapheme-cluster boundary of `s`,
/// rounding forward like [`clamp_to_char_boundary`] (which it applies
/// first, so an offset off a char boundary is safe too).
///
/// The controller's user-facing selection setters
/// ([`TextEditingController::set_selection`],
/// [`TextEditingController::set_caret_byte_offset`]) snap through this: a
/// selection edge strictly inside a cluster — from a hit test that resolved
/// to a scalar, or a caller's arithmetic — would render as a caret in the
/// middle of one glyph and make the next `move_caret_*` step look like it
/// skipped. The IME preedit path is the deliberate exception: a composition
/// cursor inside a still-forming cluster is the input method's own state and
/// stays where it was reported.
fn clamp_to_grapheme_boundary(s: &str, offset: usize) -> usize {
    let offset = clamp_to_char_boundary(s, offset);
    let mut cursor = GraphemeCursor::new(offset, s.len(), true);
    // `Err` is unreachable with the whole string as the one chunk; a char
    // boundary is the safe answer if it ever were.
    if cursor.is_boundary(s, 0) == Ok(false) {
        // Not a boundary: the next one forward is the cluster's end.
        next_grapheme_boundary(s, offset)
    } else {
        offset
    }
}

/// Clamp `offset` to the nearest valid UTF-8 char boundary in `s`, rounding
/// forward. Mirrors
/// [`RenderEditable`](flui_objects::RenderEditable)'s own
/// `safe_caret_offset` — an untrusted, platform-supplied byte offset (an IME
/// preedit cursor) must never panic a `str` slice operation.
fn clamp_to_char_boundary(s: &str, offset: usize) -> usize {
    if offset >= s.len() {
        return s.len();
    }
    if s.is_char_boundary(offset) {
        return offset;
    }
    s.char_indices()
        .map(|(idx, _)| idx)
        .chain(std::iter::once(s.len()))
        .find(|idx| *idx >= offset)
        .unwrap_or(s.len())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // Selection
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // Basic buffer operations
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // Caret navigation
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // Multi-byte (UTF-8) correctness
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // Grapheme-cluster correctness: the unit is the user-perceived
    // character, not the Unicode scalar.
    //
    // Oracle: `'Can access characters on editing string'`
    // (`editable_text_test.dart`, tag `3.44.0`) — Flutter's
    // `TextEditingValue` deletes and steps by `characters`/`CharacterRange`.
    // Red-check: swap either helper back to `char_indices`/`chars` and the
    // ZWJ cases below leave a dangling joiner.
    // ------------------------------------------------------------------

    /// A family emoji is one grapheme made of five scalars (three people
    /// joined by two Zero-Width-Joiners); one Backspace removes all of it.
    const FAMILY: &str = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F466}";

    #[test]
    fn backspace_removes_a_whole_zwj_sequence_not_one_scalar() {
        let controller = TextEditingController::with_text(format!("a{FAMILY}"));
        controller.backspace();
        assert_eq!(
            controller.text(),
            "a",
            "one Backspace must remove the whole family emoji, not leave a \
             dangling joiner behind"
        );
        assert_eq!(controller.caret_byte_offset(), 1);
    }

    // ------------------------------------------------------------------
    // Word-boundary correctness: UAX #29 word segmentation, not ASCII
    // whitespace runs.
    //
    // Oracle (shape, not byte-for-byte tie-break — see
    // `TextEditingController`'s `# Word unit` doc section for why):
    // `RenderEditable._handleMoveCursorForwardByWord`/
    // `_handleMoveCursorBackwardByWord` (`rendering/editable.dart`, tag
    // `3.44.0`).
    // Red-check: swap `next_word_boundary`/`prev_word_boundary` back to an
    // ASCII-whitespace scan and the CJK/Arabic/emoji cases below jump by
    // scalar or byte instead of by word.
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // Change notification
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // The composing region, as the field's text store leaves it
    //
    // Only the text store edits the composition (the IME rules themselves
    // are pinned by `flui_platform_api::text_store`'s projection tests);
    // these pin what the controller does with one while it is there.
    // ------------------------------------------------------------------

    /// Mark `range` as composed, silently, as a platform session's
    /// write-back does.
    fn compose(controller: &TextEditingController, range: Range<usize>, caret_hidden: bool) {
        controller.with_inner_silent(|inner| {
            inner.composing = Some(ComposingState {
                range,
                caret_hidden,
            });
        });
    }

    /// A non-IME edit while composing ends the composition rather than
    /// leaving its range describing text that moved: Backspace is never
    /// suppressed while composing (only `Key::Character` is, per ADR-0030).
    ///
    /// Red-check: comment out the `guard.composing = None;` line in
    /// `backspace` — the composition survives with a stale range.
    #[test]
    fn backspace_during_active_composition_clears_it() {
        let controller = TextEditingController::with_text("Hello nihao");
        compose(&controller, 6..11, false);

        controller.backspace();

        assert_eq!(controller.text(), "Hello niha");
        assert!(!controller.is_composing());
    }
}
