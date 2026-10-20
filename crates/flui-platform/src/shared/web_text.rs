//! The decidable half of the web backend's text input (ADR-0090): when a key
//! belongs to an input method, and what a hidden `<textarea>`'s change was.
//! Cfg-free plain functions over integers and UTF-16, so Linux CI pins them;
//! `platforms::web::text_input` reads the DOM.

use flui_platform_api::keyboard::ImeComposition;
use flui_platform_api::text_store::TextInputPurpose;

/// `KeyboardEvent.keyCode` of a key an input method processes: every
/// browser reports 229 for it, including WebKit for the key that commits a
/// composition, which it dispatches after `compositionend` with
/// `isComposing` already false.
pub const IME_PROCESS_KEY_CODE: u32 = 229;

/// Whether a DOM key event belongs to the input method: the event says it
/// composes, carries the input method's key code, or arrives while the
/// window's own text input composes. Such a key reaches the application
/// marked [`ImeComposition::Active`], which no shortcut, focus traversal or
/// drag cancellation takes, so the input method sees it first: Escape
/// cancels the composition, not a dialog.
#[must_use]
pub const fn key_composition(
    is_composing: bool,
    key_code: u32,
    host_composing: bool,
) -> ImeComposition {
    if is_composing || key_code == IME_PROCESS_KEY_CODE || host_composing {
        ImeComposition::Active
    } else {
        ImeComposition::Inactive
    }
}

/// One edit between two versions of a text, in UTF-16 code units: the
/// range `start..old_end` of the old text became `start..new_end` of the
/// new one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextDiff {
    /// Where the texts first differ.
    pub start: usize,
    /// The end of the replaced range in the old text.
    pub old_end: usize,
    /// The end of the inserted range in the new text.
    pub new_end: usize,
}

/// The one edit that turns `old` into `new`: the longest common prefix and,
/// of what remains, the longest common suffix are kept. `None` when the
/// texts are equal. A surrogate pair is never split: an edit boundary inside
/// one moves outward.
#[must_use]
pub fn diff(old: &[u16], new: &[u16]) -> Option<TextDiff> {
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

/// The `inputmode` (and `EditContext.inputMode`) a field of `purpose` asks the
/// browser's virtual keyboard for.
#[must_use]
pub const fn input_mode(purpose: TextInputPurpose) -> &'static str {
    match purpose {
        TextInputPurpose::Email => "email",
        TextInputPurpose::Url => "url",
        TextInputPurpose::Phone => "tel",
        TextInputPurpose::Number => "numeric",
        TextInputPurpose::Search => "search",
        // Free text, and any purpose added later that has no keyboard here.
        _ => "text",
    }
}
