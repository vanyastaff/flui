//! [`TextEditingController`] — owns the text buffer and caret position for an
//! [`EditableText`](super::editable_text::EditableText) field.

use std::ops::Range;
use std::sync::{Arc, Mutex, PoisonError};

use flui_foundation::ListenerId;
use flui_foundation::notifier::{ChangeNotifier, Listenable, ListenerCallback};

// ============================================================================
// ControllerInner
// ============================================================================

/// Mutable interior of a [`TextEditingController`].
///
/// Guarded by a `Mutex` inside `Arc` so any clone of the controller refers to
/// the same live text and caret state.
struct ControllerInner {
    text: String,
    /// Byte offset of the caret into `text`.  Always a valid UTF-8 char boundary.
    caret_byte_offset: usize,
    /// The in-progress IME composition, if any. `None` means no composition
    /// is active — see [`ComposingState`]'s doc for why its two fields are
    /// folded into one option rather than a sibling `caret_hidden: bool`
    /// field tracked independently.
    composing: Option<ComposingState>,
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
/// None` site (both `set_composing_text`'s empty-preedit-cancel path and
/// every existing mutator-clears site — [`TextEditingController::insert_str`]
/// /[`backspace`](TextEditingController::backspace)/
/// [`delete_forward`](TextEditingController::delete_forward)/
/// [`commit_text`](TextEditingController::commit_text)/
/// [`clear_composing`](TextEditingController::clear_composing)) drops
/// `caret_hidden` for free, along with the range it was never meaningful
/// without.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ComposingState {
    /// Byte range into `text`. Always char-boundary-clamped — see
    /// [`TextEditingController::set_composing_text`]'s "Malformed input"
    /// doc. Every read site additionally re-clamps against the CURRENT
    /// `text` before use (`clamp_range_to_text`), so even a stored range
    /// that somehow outlived a mutation degrades to a wrong-but-in-bounds
    /// slice, never a `replace_range` panic.
    range: Range<usize>,
    /// Whether the caret should be hidden because the IME currently owns
    /// its position — winit's `ImeEvent::Preedit { cursor: None, .. }`
    /// signal. Cleared (implicitly, by this whole struct going away) on
    /// commit, on `Disabled`, and on any non-IME edit; explicitly reset to
    /// `false` by a direct caret-navigation call while composing stays
    /// active (see [`TextEditingController::move_caret_left`] and its
    /// siblings) — the user taking the caret back means the IME no longer
    /// owns its position, even though the composition itself continues.
    caret_hidden: bool,
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
/// [`Self::set_composing_text`]/[`Self::commit_text`]/[`Self::clear_composing`]
/// implement Flutter's `TextEditingValue.composing` model — see each method's
/// doc for the exact replace-vs-insert and clamping rules. The **hidden
/// caret** case (`cursor: None` on a preedit event, winit's own semantics for
/// "the IME wants no caret drawn") is tracked internally and exposed through
/// [`Self::caret_hidden_by_ime`] — the owning `EditableTextState` consults it
/// to suppress the painted caret while composition still paints its own
/// underline (see ADR-0033). Composition end — a commit, `Disabled`, a
/// non-IME edit, or `Preedit` cancellation (see below) — always drops the
/// whole internal composing state, so the hidden-caret flag can never
/// outlive the composition it describes.
///
/// **`Preedit` with empty text is composition cancellation, not an
/// empty-but-active composition.** Winit signals a cancelled composition as
/// `Preedit { text: "", cursor: None }` with no following `Commit`/`Disabled`
/// — see [`Self::set_composing_text`]'s doc for the exact handling.
///
/// # DEFERRED (v1)
///
/// The following behaviors are absent in v1 and must not be faked:
/// - **Text selection**: only a collapsed caret (anchor == focus) is tracked.
///   Drag-to-select and selection rendering are not implemented.
/// - **Clipboard**: copy/paste/cut are not wired.
/// - **Input formatters**: no validation or transformation pipeline.
/// - **Grapheme-cluster-aware deletion**: [`Self::backspace`]/[`Self::delete_forward`]
///   walk back or forward exactly one Unicode scalar value (`char`), not one
///   extended grapheme cluster. Flutter deletes by grapheme cluster (the
///   `characters` package / `CharacterRange`, matching `TextEditingValue`'s
///   own delete-by-character semantics) — for a plain multi-byte character
///   (e.g. `'€'`) the two happen to coincide, but for a Zero-Width-Joiner
///   sequence (a family/flag emoji: `'👨‍👩‍👦'`) a single Backspace removes only
///   the trailing scalar, leaving a dangling joiner rendered as a broken
///   partial glyph on screen instead of deleting the whole visual character.
///   Tracked as ROADMAP Cross.H's "no grapheme-cluster segmentation" known
///   gap; the fix needs a grapheme-segmentation dependency
///   (`unicode-segmentation`) threaded through this module's mutators and is
///   its own future unit, not attempted here.
#[derive(Clone)]
pub struct TextEditingController {
    /// Shared text buffer + caret state.
    inner: Arc<Mutex<ControllerInner>>,
    /// Listener list — `ChangeNotifier` is itself `Arc`-backed so clones share
    /// the same list.
    notifier: ChangeNotifier,
}

impl std::fmt::Debug for TextEditingController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        f.debug_struct("TextEditingController")
            .field("text", &guard.text)
            .field("caret_byte_offset", &guard.caret_byte_offset)
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
                caret_byte_offset: 0,
                composing: None,
            })),
            notifier: ChangeNotifier::new(),
        }
    }

    /// Create a controller pre-populated with `initial_text`, caret at the end.
    #[must_use]
    pub fn with_text(initial_text: impl Into<String>) -> Self {
        let text = initial_text.into();
        let caret_byte_offset = text.len();
        Self {
            inner: Arc::new(Mutex::new(ControllerInner {
                text,
                caret_byte_offset,
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
            .caret_byte_offset
    }

    // =========================================================================
    // Mutation — each method notifies listeners after the change
    // =========================================================================

    /// Insert `text` at the current caret position and advance the caret past it.
    ///
    /// Clears any active composing region — Flutter parity:
    /// `TextEditingController`'s `text` setter resets `composing` to empty
    /// on every programmatic change (`editable_text.dart`, tag `3.44.0`).
    /// A stale composing region left pointing at a now-shifted buffer is
    /// exactly the "stored range no longer describes the current text" bug
    /// class this controller must not reintroduce (see
    /// [`Self::set_composing_text`]'s "Malformed input" doc) — this is a
    /// non-IME edit, so IME composition state does not survive it.
    ///
    /// Notifies listeners after the insertion.
    pub fn insert_str(&self, text: &str) {
        {
            let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            let caret = guard.caret_byte_offset;
            guard.text.insert_str(caret, text);
            guard.caret_byte_offset = caret + text.len();
            guard.composing = None;
        }
        self.notifier.notify_listeners();
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
            let caret = guard.caret_byte_offset;
            if caret == 0 {
                false
            } else {
                // Walk back to the previous char boundary.
                let prev_boundary = guard.text[..caret]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(idx, _)| idx);
                guard.text.drain(prev_boundary..caret);
                guard.caret_byte_offset = prev_boundary;
                guard.composing = None;
                true
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
            let caret = guard.caret_byte_offset;
            if caret == guard.text.len() {
                false
            } else {
                // Width of the char starting at `caret`.
                let char_width = guard.text[caret..].chars().next().map_or(0, char::len_utf8);
                guard.text.drain(caret..caret + char_width);
                guard.composing = None;
                true
            }
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
            let caret = guard.caret_byte_offset;
            let moved = if caret == 0 {
                false
            } else {
                let prev_boundary = guard.text[..caret]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(idx, _)| idx);
                guard.caret_byte_offset = prev_boundary;
                true
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
            let caret = guard.caret_byte_offset;
            let moved = if caret == guard.text.len() {
                false
            } else {
                let char_width = guard.text[caret..].chars().next().map_or(0, char::len_utf8);
                guard.caret_byte_offset = caret + char_width;
                true
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
            let moved = if guard.caret_byte_offset == 0 {
                false
            } else {
                guard.caret_byte_offset = 0;
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
            let moved = if guard.caret_byte_offset == end {
                false
            } else {
                guard.caret_byte_offset = end;
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
    /// ([`flui_types::ImeEvent`]'s doc): suppress `Key::Character` insertion
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
    /// Reflects the most recent [`Self::set_composing_text`]'s `cursor`
    /// argument: `cursor: None` (winit's "hide the caret" signal) sets this
    /// `true`; a caret-navigation call (
    /// [`move_caret_left`](Self::move_caret_left)/
    /// [`move_caret_right`](Self::move_caret_right)/
    /// [`move_caret_home`](Self::move_caret_home)/
    /// [`move_caret_end`](Self::move_caret_end)) clears it back to `false`
    /// without ending the composition — the user took the caret back, so the
    /// IME no longer owns its position even though composing text is still
    /// present. [`EditableTextState`](super::EditableTextState) consults this
    /// to suppress the painted caret while the composing-region underline
    /// keeps painting (ADR-0033).
    #[must_use]
    pub fn caret_hidden_by_ime(&self) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .composing
            .as_ref()
            .is_some_and(|state| state.caret_hidden)
    }

    /// Apply an IME preedit update.
    ///
    /// `text` is the full current composition string; `cursor` is a byte
    /// offset range **into `text`** (not into [`Self::text`]) — matching
    /// [`flui_types::ImeEvent::Preedit`]'s own convention.
    ///
    /// Replaces the existing composing region if one is already active,
    /// else inserts `text` at the current caret and starts a new composing
    /// region there. The caret is repositioned to `cursor`'s end, translated
    /// into the outer buffer; `cursor: None` (the platform wants no caret
    /// drawn) sets [`Self::caret_hidden_by_ime`] and additionally collapses
    /// the caret to the end of the composing region (both — hiding it is
    /// not a substitute for tracking where it logically sits).
    ///
    /// # `text.is_empty()` is composition cancellation
    ///
    /// Winit signals a cancelled composition as `Preedit { text: "", cursor:
    /// None }`, with **no** following `Commit`/`Disabled` event. Treating
    /// this the same as any other (non-empty) preedit update would strip the
    /// composing slice — correct — but then set `composing = Some(empty
    /// range)` — wrong: [`Self::is_composing`] would report `true` forever
    /// after, permanently suppressing `Key::Character` insertion for the
    /// rest of the focus session (the exact failure mode
    /// [`flui_types::ImeEvent`]'s suppression contract warns against). So an
    /// empty `text` strips the existing composing slice (if any) the same
    /// way [`Self::clear_composing`] does, and ends composition — `composing`
    /// becomes `None`, not `Some` of an empty range.
    ///
    /// # Malformed input
    ///
    /// `cursor` offsets that land mid-character or past `text`'s end are
    /// clamped to the nearest valid char boundary — a byte offset from an
    /// IME is untrusted platform input, not an internal invariant, so this
    /// never panics (`docs/PANIC-POLICY.md`).
    pub fn set_composing_text(&self, text: &str, cursor: Option<(usize, usize)>) {
        {
            let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            let region = guard
                .composing
                .as_ref()
                .map(|state| state.range.clone())
                .unwrap_or(guard.caret_byte_offset..guard.caret_byte_offset);
            // Defense in depth: every non-IME mutator already clears
            // `composing` on a text edit (see `Self::insert_str`'s doc), so
            // `region` should always already describe `guard.text` — this
            // re-clamp is what makes a future mutator that forgets that rule
            // degrade to wrong text instead of a `replace_range` panic.
            let region = clamp_range_to_text(&region, &guard.text);
            if text.is_empty() {
                // Composition cancel (see this method's doc) — strip the
                // slice and END composition, never leave a `Some(empty
                // range)` marker behind.
                guard.text.replace_range(region.clone(), "");
                guard.composing = None;
                if guard.caret_byte_offset > region.start {
                    guard.caret_byte_offset = region.start;
                }
            } else {
                guard.text.replace_range(region.clone(), text);
                guard.composing = Some(ComposingState {
                    range: region.start..region.start + text.len(),
                    caret_hidden: cursor.is_none(),
                });
                let caret_in_preedit = match cursor {
                    Some((_, end)) => clamp_to_char_boundary(text, end),
                    None => text.len(),
                };
                guard.caret_byte_offset = region.start + caret_in_preedit;
            }
        }
        self.notifier.notify_listeners();
    }

    /// Apply an IME commit.
    ///
    /// Replaces the composing region with `text` if one is active, else
    /// inserts `text` at the current caret (a direct commit with no
    /// preceding preedit — winit delivers these too, not every commit is
    /// composition-terminated). Clears the composing region and positions
    /// the caret immediately after the committed text.
    pub fn commit_text(&self, text: &str) {
        {
            let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            let insert_at = if let Some(state) = guard.composing.clone() {
                // Defense in depth — see `set_composing_text`'s matching comment.
                let range = clamp_range_to_text(&state.range, &guard.text);
                guard.text.replace_range(range.clone(), text);
                range.start
            } else {
                let caret = guard.caret_byte_offset;
                guard.text.insert_str(caret, text);
                caret
            };
            guard.composing = None;
            guard.caret_byte_offset = insert_at + text.len();
        }
        self.notifier.notify_listeners();
    }

    /// Apply an IME `Disabled` notification.
    ///
    /// Strips the in-progress composing **slice** from the buffer, not just
    /// the region marker — winit's own semantics, a documented divergence
    /// from Flutter's `TextInputConnection.connectionClosed`, which instead
    /// keeps the uncommitted text (see [`flui_types::ImeEvent`]'s doc).
    /// No-op (and no listener notification) when no composition is active.
    ///
    /// The caret clamps to the stripped region's start when it sat inside
    /// or past it; a caret positioned strictly before the composing region
    /// is left untouched.
    pub fn clear_composing(&self) {
        let changed = {
            let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            match guard.composing.take() {
                Some(state) => {
                    // Defense in depth — see `set_composing_text`'s matching comment.
                    let range = clamp_range_to_text(&state.range, &guard.text);
                    guard.text.replace_range(range.clone(), "");
                    if guard.caret_byte_offset > range.start {
                        guard.caret_byte_offset = range.start;
                    }
                    true
                }
                None => false,
            }
        };
        if changed {
            self.notifier.notify_listeners();
        }
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

/// Clamp a stored composing [`Range`] to `text`'s current bounds and char
/// boundaries, degrading a stale range (one that no longer describes `text`
/// — e.g. a non-IME edit that should have cleared it but didn't) to a
/// sane, in-bounds slice instead of a `replace_range` panic. `start > end`
/// after clamping (the range's start itself outlived the text) collapses to
/// a zero-width range at the clamped start, matching an empty composing
/// region rather than reordering the bounds.
fn clamp_range_to_text(range: &Range<usize>, text: &str) -> Range<usize> {
    let start = clamp_to_char_boundary(text, range.start);
    let end = clamp_to_char_boundary(text, range.end);
    if start > end {
        start..start
    } else {
        start..end
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // Basic buffer operations
    // ------------------------------------------------------------------

    #[test]
    fn empty_controller_starts_with_empty_text_and_caret_at_zero() {
        let controller = TextEditingController::new();
        assert_eq!(controller.text(), "");
        assert_eq!(controller.caret_byte_offset(), 0);
    }

    #[test]
    fn with_text_positions_caret_at_end() {
        let controller = TextEditingController::with_text("hello");
        assert_eq!(controller.text(), "hello");
        assert_eq!(controller.caret_byte_offset(), 5);
    }

    #[test]
    fn insert_str_appends_when_caret_at_end() {
        let controller = TextEditingController::new();
        controller.insert_str("hello");
        assert_eq!(controller.text(), "hello");
        assert_eq!(controller.caret_byte_offset(), 5);
    }

    #[test]
    fn insert_str_inserts_in_the_middle() {
        let controller = TextEditingController::with_text("helo");
        // Manually place caret before 'o'.
        controller.inner.lock().unwrap().caret_byte_offset = 3;
        controller.insert_str("l");
        assert_eq!(controller.text(), "hello");
        assert_eq!(controller.caret_byte_offset(), 4);
    }

    #[test]
    fn backspace_removes_char_left_of_caret() {
        let controller = TextEditingController::with_text("hello");
        controller.backspace();
        assert_eq!(controller.text(), "hell");
        assert_eq!(controller.caret_byte_offset(), 4);
    }

    #[test]
    fn backspace_at_start_is_noop() {
        let controller = TextEditingController::new();
        controller.backspace(); // Must not panic.
        assert_eq!(controller.text(), "");
        assert_eq!(controller.caret_byte_offset(), 0);
    }

    #[test]
    fn delete_forward_removes_char_right_of_caret() {
        let controller = TextEditingController::with_text("hello");
        controller.move_caret_home();
        controller.delete_forward();
        assert_eq!(controller.text(), "ello");
        assert_eq!(controller.caret_byte_offset(), 0);
    }

    #[test]
    fn delete_forward_at_end_is_noop() {
        let controller = TextEditingController::with_text("hi");
        controller.delete_forward(); // Caret already at end.
        assert_eq!(controller.text(), "hi");
        assert_eq!(controller.caret_byte_offset(), 2);
    }

    // ------------------------------------------------------------------
    // Caret navigation
    // ------------------------------------------------------------------

    #[test]
    fn move_caret_left_moves_one_char() {
        let controller = TextEditingController::with_text("abc");
        controller.move_caret_left();
        assert_eq!(controller.caret_byte_offset(), 2);
        controller.move_caret_left();
        assert_eq!(controller.caret_byte_offset(), 1);
    }

    #[test]
    fn move_caret_left_at_start_is_noop() {
        let controller = TextEditingController::with_text("a");
        controller.move_caret_home();
        controller.move_caret_left(); // Must not underflow.
        assert_eq!(controller.caret_byte_offset(), 0);
    }

    #[test]
    fn move_caret_right_moves_one_char() {
        let controller = TextEditingController::with_text("abc");
        controller.move_caret_home();
        controller.move_caret_right();
        assert_eq!(controller.caret_byte_offset(), 1);
    }

    #[test]
    fn move_caret_right_at_end_is_noop() {
        let controller = TextEditingController::with_text("a");
        controller.move_caret_right(); // Already at end.
        assert_eq!(controller.caret_byte_offset(), 1);
    }

    #[test]
    fn move_caret_home_resets_to_zero() {
        let controller = TextEditingController::with_text("hello");
        controller.move_caret_home();
        assert_eq!(controller.caret_byte_offset(), 0);
    }

    #[test]
    fn move_caret_end_moves_to_last_byte() {
        let controller = TextEditingController::new();
        controller.insert_str("hello");
        controller.move_caret_home();
        controller.move_caret_end();
        assert_eq!(controller.caret_byte_offset(), 5);
    }

    // ------------------------------------------------------------------
    // Multi-byte (UTF-8) correctness
    // ------------------------------------------------------------------

    #[test]
    fn backspace_removes_full_multibyte_char() {
        // '€' is 3 bytes in UTF-8.
        let controller = TextEditingController::with_text("a€b");
        // Caret at end (5 bytes: 'a'=1 + '€'=3 + 'b'=1)
        controller.backspace(); // Should remove 'b' (1 byte).
        assert_eq!(controller.text(), "a€");
        assert_eq!(controller.caret_byte_offset(), 4);
        controller.backspace(); // Should remove '€' (3 bytes).
        assert_eq!(controller.text(), "a");
        assert_eq!(controller.caret_byte_offset(), 1);
    }

    #[test]
    fn delete_forward_removes_full_multibyte_char() {
        let controller = TextEditingController::with_text("€b");
        controller.move_caret_home();
        controller.delete_forward(); // Should remove '€' (3 bytes).
        assert_eq!(controller.text(), "b");
        assert_eq!(controller.caret_byte_offset(), 0);
    }

    // ------------------------------------------------------------------
    // Change notification
    // ------------------------------------------------------------------

    #[test]
    fn listeners_fire_on_insert() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let controller = TextEditingController::new();
        let call_count = Arc::new(AtomicUsize::new(0));
        let count_clone = Arc::clone(&call_count);

        controller.add_listener(Arc::new(move || {
            count_clone.fetch_add(1, Ordering::Relaxed);
        }));

        controller.insert_str("a");
        assert_eq!(
            call_count.load(Ordering::Relaxed),
            1,
            "listener must fire on insert"
        );
    }

    #[test]
    fn listeners_do_not_fire_when_backspace_at_start() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let controller = TextEditingController::new();
        let call_count = Arc::new(AtomicUsize::new(0));
        let count_clone = Arc::clone(&call_count);

        controller.add_listener(Arc::new(move || {
            count_clone.fetch_add(1, Ordering::Relaxed);
        }));

        controller.backspace(); // No-op — must not notify.
        assert_eq!(call_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn remove_listener_stops_notifications() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let controller = TextEditingController::new();
        let call_count = Arc::new(AtomicUsize::new(0));
        let count_clone = Arc::clone(&call_count);

        let id = controller.add_listener(Arc::new(move || {
            count_clone.fetch_add(1, Ordering::Relaxed);
        }));
        controller.remove_listener(id);
        controller.insert_str("x");
        assert_eq!(call_count.load(Ordering::Relaxed), 0);
    }

    // ------------------------------------------------------------------
    // IME composing region
    // ------------------------------------------------------------------

    #[test]
    fn set_composing_text_inserts_at_the_caret_when_no_composition_is_active() {
        let controller = TextEditingController::with_text("hello");
        controller.set_composing_text("ni", Some((0, 2)));
        assert_eq!(controller.text(), "helloni");
        assert_eq!(controller.composing_range(), Some(5..7));
        assert_eq!(controller.caret_byte_offset(), 7);
        assert!(controller.is_composing());
    }

    #[test]
    fn set_composing_text_replaces_the_existing_composing_region_as_preedit_grows() {
        let controller = TextEditingController::with_text("Hello ");
        controller.set_composing_text("n", Some((1, 1)));
        assert_eq!(controller.text(), "Hello n");
        assert_eq!(controller.composing_range(), Some(6..7));

        controller.set_composing_text("ni", Some((2, 2)));
        assert_eq!(controller.text(), "Hello ni");
        assert_eq!(controller.composing_range(), Some(6..8));
        assert_eq!(controller.caret_byte_offset(), 8);

        controller.set_composing_text("nihao", Some((5, 5)));
        assert_eq!(controller.text(), "Hello nihao");
        assert_eq!(controller.composing_range(), Some(6..11));
    }

    #[test]
    fn set_composing_text_replaces_the_existing_composing_region_as_preedit_shrinks() {
        let controller = TextEditingController::with_text("Hello ");
        controller.set_composing_text("nihao", Some((5, 5)));
        assert_eq!(controller.text(), "Hello nihao");

        // The user backspaced inside the IME candidate window.
        controller.set_composing_text("niha", Some((4, 4)));
        assert_eq!(controller.text(), "Hello niha");
        assert_eq!(controller.composing_range(), Some(6..10));
        assert_eq!(controller.caret_byte_offset(), 10);
    }

    /// The full pinyin-style composition lifecycle: preedit grows, shrinks,
    /// then a multi-byte CJK commit replaces the whole composing region.
    #[test]
    fn cjk_composition_grows_shrinks_then_commits() {
        let controller = TextEditingController::with_text("Hello ");
        controller.set_composing_text("n", Some((1, 1)));
        controller.set_composing_text("ni", Some((2, 2)));
        controller.set_composing_text("nihao", Some((5, 5)));
        controller.set_composing_text("niha", Some((4, 4)));
        assert_eq!(controller.text(), "Hello niha");

        controller.commit_text("你好");
        assert_eq!(controller.text(), "Hello 你好");
        assert_eq!(controller.composing_range(), None);
        assert!(!controller.is_composing());
        assert_eq!(controller.caret_byte_offset(), "Hello 你好".len());
    }

    #[test]
    fn composing_region_growth_with_multibyte_preedit_content_tracks_byte_length() {
        let controller = TextEditingController::new();
        // "に" is a 3-byte character; cursor.1 indexes bytes within the
        // preedit string, not chars.
        controller.set_composing_text("に", Some((3, 3)));
        assert_eq!(controller.composing_range(), Some(0..3));

        controller.set_composing_text("にほ", Some((6, 6)));
        assert_eq!(controller.text(), "にほ");
        assert_eq!(controller.composing_range(), Some(0..6));
        assert_eq!(controller.caret_byte_offset(), 6);
    }

    #[test]
    fn commit_text_with_no_active_composing_inserts_at_the_caret() {
        let controller = TextEditingController::with_text("ab");
        controller.commit_text("X");
        assert_eq!(controller.text(), "abX");
        assert_eq!(controller.caret_byte_offset(), 3);
        assert!(!controller.is_composing());
    }

    /// Oracle analog: `'connection is closed when TextInputClient
    /// .onConnectionClosed message received'` (`editable_text_test.dart`,
    /// tag `3.44.0`) — **adapted, not a direct port**: Flutter's
    /// `connectionClosed` leaves the buffer untouched; `clear_composing`
    /// (FLUI's `ImeEvent::Disabled` handler) strips the composing slice
    /// instead — see this method's own doc comment for the documented
    /// divergence, and
    /// `flui_widgets::text::editable_text::tests::disabled_removes_the_underline_and_restores_the_caret`
    /// for the widget-level counterpart.
    ///
    /// Red-check: change `clear_composing`'s `replace_range(range, "")` to
    /// only clear the `composing` marker (`guard.composing = None`) without
    /// touching `guard.text` — this test's text assertion fails because the
    /// composing slice would still be present.
    #[test]
    fn clear_composing_strips_exactly_the_composing_slice() {
        let controller = TextEditingController::with_text("Hello ");
        controller.set_composing_text("wor", Some((3, 3)));
        assert_eq!(controller.text(), "Hello wor");

        controller.clear_composing();
        assert_eq!(
            controller.text(),
            "Hello ",
            "a mid-composition Disabled must strip the composing slice, not \
             keep it — winit semantics, a documented divergence from \
             Flutter's TextInputConnection.connectionClosed"
        );
        assert!(!controller.is_composing());
        assert_eq!(controller.caret_byte_offset(), "Hello ".len());
    }

    #[test]
    fn clear_composing_with_no_active_composition_is_a_noop() {
        let controller = TextEditingController::with_text("abc");
        controller.clear_composing(); // Must not panic or change the buffer.
        assert_eq!(controller.text(), "abc");
        assert_eq!(controller.caret_byte_offset(), 3);
    }

    /// Oracle analog: `'Clears composing range if cursor moves outside that
    /// range'` (`editable_text_test.dart`, tag `3.44.0`) — see
    /// `caret_navigation_restores_the_caret_while_composing`'s doc comment
    /// for the divergence this contrasts with (Flutter would clear the
    /// composing range here; FLUI's `clear_composing` is a distinct,
    /// explicit call — direct caret navigation alone never triggers it).
    #[test]
    fn clear_composing_leaves_a_caret_before_the_region_untouched() {
        let controller = TextEditingController::with_text("Hello ");
        controller.set_composing_text("wor", Some((3, 3)));
        // Simulate Home pressed mid-composition: the caret moves out of the
        // composing region while the region itself stays active.
        controller.move_caret_home();
        assert_eq!(controller.caret_byte_offset(), 0);

        controller.clear_composing();
        assert_eq!(controller.text(), "Hello ");
        assert_eq!(
            controller.caret_byte_offset(),
            0,
            "a caret strictly before the composing region must not be pulled forward"
        );
    }

    #[test]
    fn cursor_none_collapses_the_caret_to_the_end_of_the_composing_region() {
        let controller = TextEditingController::with_text("Hi ");
        controller.set_composing_text("wor", None);
        assert_eq!(controller.text(), "Hi wor");
        assert_eq!(controller.composing_range(), Some(3..6));
        assert_eq!(
            controller.caret_byte_offset(),
            6,
            "cursor: None (the platform's hidden-caret signal) collapses the \
             caret to the end of the composing region in v1"
        );
        assert!(
            controller.caret_hidden_by_ime(),
            "cursor: None must also mark the caret hidden, not just collapse \
             its position"
        );
    }

    #[test]
    fn cursor_some_leaves_the_caret_visible() {
        let controller = TextEditingController::with_text("Hi ");
        controller.set_composing_text("wor", Some((3, 3)));
        assert!(
            !controller.caret_hidden_by_ime(),
            "a cursor: Some preedit must not hide the caret"
        );
    }

    #[test]
    fn caret_hidden_by_ime_is_false_when_no_composition_is_active() {
        let controller = TextEditingController::with_text("Hi");
        assert!(!controller.caret_hidden_by_ime());
    }

    /// The bug this reshape fixes: winit signals a cancelled composition as
    /// `Preedit { text: "", cursor: None }` with no following
    /// `Commit`/`Disabled`. Before the fix, `set_composing_text("", _)` left
    /// `composing = Some(n..n)` (an empty-but-active region), so
    /// `is_composing()` stayed `true` forever and `Key::Character` insertion
    /// was permanently suppressed for the rest of the focus session.
    ///
    /// Red-check: revert `set_composing_text` to always take the
    /// `Some(region.start..region.start + text.len())` branch regardless of
    /// whether `text` is empty — `is_composing()` after the cancel returns
    /// `true` instead of `false`, and the typed-character assertion below
    /// fails (the character never reaches the buffer).
    #[test]
    fn empty_preedit_ends_composition_instead_of_leaving_an_empty_active_region() {
        let controller = TextEditingController::new();
        controller.set_composing_text("nihao", Some((5, 5)));
        assert_eq!(controller.text(), "nihao");
        assert!(controller.is_composing());

        // Winit's composition-cancel signal: empty text, no cursor.
        controller.set_composing_text("", None);

        assert_eq!(
            controller.text(),
            "",
            "the cancelled preedit's slice must be stripped from the buffer"
        );
        assert!(
            !controller.is_composing(),
            "Preedit(\"\") must end composition, not leave an empty-but-active \
             region behind"
        );
        assert!(!controller.caret_hidden_by_ime());

        // Plain typing must work immediately after the cancel — the exact
        // suppression-forever failure mode this fix closes.
        controller.insert_str("x");
        assert_eq!(controller.text(), "x");
    }

    /// Direct caret navigation takes the caret back from the IME: the
    /// composition itself keeps running (the underline still paints), but
    /// the caret is no longer hidden.
    ///
    /// Oracle analog: `'Preserves composing range if cursor moves within
    /// that range'`, `'Clears composing range if cursor moves outside that
    /// range'`, and its `'case two'` variant (`editable_text_test.dart`, tag
    /// `3.44.0`) — **divergent, not a port**: Flutter clears the composing
    /// range whenever the selection moves outside it; FLUI's direct caret
    /// navigation never touches the composing range at all (only the
    /// caret-hidden flag this test pins) — see
    /// `tests/parity/text_editing_controller_test.rs`'s
    /// `direct_caret_navigation_leaves_the_composing_range_untouched_unlike_flutter`
    /// for the divergence pinned again from outside the crate, and
    /// `clear_composing_leaves_a_caret_before_the_region_untouched` below for
    /// the composing-range-survives half of this same contrast.
    ///
    /// Red-check: remove the `clear_caret_hidden` call from
    /// `move_caret_home` — `caret_hidden_by_ime()` stays `true` after this
    /// test's `move_caret_home()` call.
    #[test]
    fn caret_navigation_restores_the_caret_while_composing() {
        let controller = TextEditingController::with_text("abc");
        controller.set_composing_text("def", None);
        assert!(controller.caret_hidden_by_ime());
        let composing_before = controller.composing_range();

        controller.move_caret_home();

        assert!(
            !controller.caret_hidden_by_ime(),
            "moving the caret directly must restore its visibility"
        );
        assert!(
            controller.is_composing(),
            "caret navigation must not end the composition"
        );
        assert_eq!(
            controller.composing_range(),
            composing_before,
            "the composing range itself must stay untouched by caret navigation"
        );
    }

    /// Oracle analog: `'Asserts if composing text is not valid'` (`test`,
    /// `editable_text_test.dart`, tag `3.44.0`) — **divergent, not a port**:
    /// Flutter throws constructing/assigning a `TextEditingValue` whose
    /// composing range is out of bounds; FLUI's `set_composing_text` takes
    /// text + cursor directly and clamps instead of asserting
    /// (`docs/PANIC-POLICY.md`: untrusted platform input must not panic) —
    /// see `tests/parity/text_editing_controller_test.rs`'s
    /// `malformed_composing_cursor_clamps_through_the_public_api_instead_of_asserting`
    /// for the same contract proven from outside the crate.
    ///
    /// Red-check: drop the `clamp_to_char_boundary` call in
    /// `set_composing_text` (use `cursor.1` raw) — this test panics instead
    /// of asserting the clamped value.
    #[test]
    fn malformed_cursor_offset_past_the_preedit_end_clamps_without_panicking() {
        let controller = TextEditingController::new();
        controller.set_composing_text("ni", Some((0, 100)));
        assert_eq!(controller.composing_range(), Some(0..2));
        assert_eq!(
            controller.caret_byte_offset(),
            2,
            "an out-of-range cursor offset clamps to the preedit's own length"
        );
    }

    #[test]
    fn malformed_cursor_offset_mid_multibyte_char_clamps_forward_without_panicking() {
        let controller = TextEditingController::new();
        // '€' is 3 bytes; a cursor end of 1 lands mid-character.
        controller.set_composing_text("€", Some((0, 1)));
        assert_eq!(
            controller.caret_byte_offset(),
            3,
            "a cursor offset landing mid-character rounds forward to the next \
             boundary rather than panicking"
        );
    }

    #[test]
    fn is_composing_reflects_active_composition_state() {
        let controller = TextEditingController::new();
        assert!(!controller.is_composing());

        controller.set_composing_text("a", Some((1, 1)));
        assert!(controller.is_composing());

        controller.commit_text("a");
        assert!(!controller.is_composing());
    }

    #[test]
    fn listeners_fire_on_composing_updates_and_commit() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let controller = TextEditingController::new();
        let call_count = Arc::new(AtomicUsize::new(0));
        let count_clone = Arc::clone(&call_count);
        controller.add_listener(Arc::new(move || {
            count_clone.fetch_add(1, Ordering::Relaxed);
        }));

        controller.set_composing_text("a", Some((1, 1)));
        assert_eq!(call_count.load(Ordering::Relaxed), 1);

        controller.commit_text("a");
        assert_eq!(call_count.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn clear_composing_notifies_only_when_it_actually_strips_something() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let controller = TextEditingController::new();
        let call_count = Arc::new(AtomicUsize::new(0));
        let count_clone = Arc::clone(&call_count);
        controller.add_listener(Arc::new(move || {
            count_clone.fetch_add(1, Ordering::Relaxed);
        }));

        controller.clear_composing(); // No active composition — no notify.
        assert_eq!(call_count.load(Ordering::Relaxed), 0);

        controller.set_composing_text("a", Some((1, 1)));
        assert_eq!(call_count.load(Ordering::Relaxed), 1);

        controller.clear_composing();
        assert_eq!(call_count.load(Ordering::Relaxed), 2);
    }

    // ------------------------------------------------------------------
    // Interleaving a non-IME edit with an active composition
    //
    // A real field can receive a plain edit (Backspace/Delete — the
    // suppression contract only gates `Key::Character`, per ADR-0030) while
    // an IME composition is in progress. The composing region must not
    // survive that edit stale: a later `commit_text`/`clear_composing`
    // trusting the old range against the now-shifted `text` is exactly the
    // `replace_range` panic class this controller must not reintroduce.
    // ------------------------------------------------------------------

    /// Red-check: comment out the `guard.composing = None;` line in
    /// `backspace` — this test panics (`replace_range` end index out of
    /// bounds) instead of reaching its assertions. Verified by hand before
    /// this test was written: reverting the fix reproduces exactly this
    /// panic on `commit_text`.
    #[test]
    fn backspace_during_active_composition_clears_it_so_a_later_commit_does_not_panic() {
        let controller = TextEditingController::with_text("Hello ");
        controller.set_composing_text("nihao", Some((5, 5)));
        assert_eq!(controller.text(), "Hello nihao");
        assert_eq!(controller.composing_range(), Some(6..11));

        // A non-IME edit while composing is active: Backspace is never
        // suppressed (only `Key::Character` is, per ADR-0030).
        controller.backspace();
        assert_eq!(controller.text(), "Hello niha");
        assert!(
            !controller.is_composing(),
            "a non-IME text edit must clear the composing region, not leave \
             it pointing at a range the backspace already invalidated"
        );

        // Must not panic: before the fix, `commit_text` trusted the stale
        // `6..11` range against an 10-byte buffer.
        controller.commit_text("X");
        assert_eq!(controller.text(), "Hello nihaX");
    }

    /// The `insert_str` counterpart of the above, and `clear_composing` as
    /// the second composing-region consumer (not just `commit_text`).
    ///
    /// Red-check: comment out the `guard.composing = None;` line in
    /// `insert_str` — this test panics on `clear_composing`'s
    /// `replace_range` instead of reaching its assertions.
    #[test]
    fn insert_str_during_active_composition_clears_it_so_a_later_clear_composing_does_not_panic() {
        let controller = TextEditingController::with_text("Hello ");
        controller.set_composing_text("nihao", Some((5, 5)));
        assert_eq!(controller.text(), "Hello nihao");

        // `insert_str` is what the suppression contract exists to prevent
        // for `Key::Character` specifically, but nothing stops another
        // caller (a paste, a programmatic edit) from calling it directly
        // while composing is active.
        controller.insert_str("Z");
        assert_eq!(controller.text(), "Hello nihaoZ");
        assert!(
            !controller.is_composing(),
            "a non-IME insert must clear the composing region"
        );

        // Must not panic: before the fix, `clear_composing` trusted the
        // stale `6..11` range against a 12-byte buffer that had already
        // grown past it in the wrong place.
        controller.clear_composing();
        assert_eq!(
            controller.text(),
            "Hello nihaoZ",
            "a no-op clear changes nothing"
        );
    }

    /// Defense in depth, exercised directly: even if a stale composing range
    /// somehow survived to reach a use site (bypassing the mutator-clears
    /// rule the two tests above verify), `clamp_range_to_text` must degrade
    /// it to an in-bounds slice rather than let `replace_range` panic. This
    /// reaches into `ControllerInner` directly (test-only) to fabricate
    /// exactly that otherwise-unreachable state.
    ///
    /// Red-check: remove the `clamp_range_to_text` call in `commit_text` —
    /// this test panics instead of asserting the degraded (wrong but
    /// in-bounds) outcome.
    /// Oracle analog: `'Asserts if composing text is not valid'` (`test`,
    /// `editable_text_test.dart`, tag `3.44.0`) — see
    /// `malformed_cursor_offset_past_the_preedit_end_clamps_without_panicking`'s
    /// doc comment for the divergence (Flutter asserts; FLUI clamps).
    #[test]
    fn a_stale_composing_range_that_bypasses_the_mutator_guard_still_cannot_panic_commit() {
        let controller = TextEditingController::with_text("Hello nihao");
        {
            let mut guard = controller.inner.lock().unwrap();
            // Fabricate exactly the otherwise-unreachable state a future
            // mutator that forgets to clear `composing` could produce: a
            // region that described the text BEFORE it shrank.
            guard.text = "Hello niha".to_string(); // shrank by one byte
            guard.caret_byte_offset = guard.text.len();
            guard.composing = Some(ComposingState {
                range: 6..11, // now out of bounds
                caret_hidden: false,
            });
        }

        // Must not panic.
        controller.commit_text("X");
        assert!(!controller.is_composing());
    }
}
