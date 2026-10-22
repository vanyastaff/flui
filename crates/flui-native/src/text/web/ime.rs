//! The decidable half of the web backend's text input (ADR-0090): when a key
//! belongs to an input method, and which virtual keyboard a field asks for.
//! What a hidden `<textarea>`'s change was is the mirror's one edit
//! ([`crate::text::ime::diff`]). Cfg-free plain functions over integers, so
//! Linux CI pins them; `text::web::input` reads the DOM.

#![forbid(unsafe_code)]

use flui_platform::keyboard::ImeComposition;
use flui_platform::text_store::TextInputPurpose;

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

#[cfg(test)]
mod tests;
