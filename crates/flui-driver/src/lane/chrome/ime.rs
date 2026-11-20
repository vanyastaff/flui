//! An input method's composition as the Chrome DevTools commands that play it.
//!
//! WebDriver BiDi has no input method, so the lane reaches Chrome's DevTools input domain through
//! ChromeDriver's `goog:cdp.sendCommand`: `Input.imeSetComposition` shows a composition and
//! `Input.insertText` commits one. Chrome delivers both through the path an OS input method's
//! text takes (an `EditContext`'s `textupdate`, or a hidden text area's `compositionupdate` and
//! `input`), so the page reads them as a person's composition, not as script.
//!
//! A cursor is counted in characters (Unicode scalar values) on the wire and in UTF-16 units by
//! Chrome; a caret the input method hides sits at the composition's end. Cancelling is an empty
//! composition, which is how Chrome itself cancels one.

use flui_protocol::input::ImeAction;
use serde_json::{Value, json};

use crate::error::{Error, Result};

/// The DevTools method and parameters that play `action`.
///
/// # Errors
///
/// [`Error::InvalidArgument`] for a cursor past the composition's end; [`Error::NotSupported`] for
/// a step this lane does not know.
pub fn devtools_command(action: &ImeAction) -> Result<(&'static str, Value)> {
    match action {
        ImeAction::SetComposition {
            text,
            cursor,
            caret_hidden,
        } => {
            let characters = text.chars().count();
            let at = match cursor {
                Some(cursor) if !caret_hidden => usize::try_from(*cursor).unwrap_or(usize::MAX),
                _ => characters,
            };
            if at > characters {
                return Err(Error::invalid_argument(format!(
                    "a caret at character {at} is past the end of the {characters}-character \
                     composition {text:?}"
                )));
            }
            let units: usize = text.chars().take(at).map(char::len_utf16).sum();
            Ok((
                "Input.imeSetComposition",
                json!({ "text": text, "selectionStart": units, "selectionEnd": units }),
            ))
        }
        ImeAction::Commit { text } => Ok(("Input.insertText", json!({ "text": text }))),
        ImeAction::Cancel => Ok((
            "Input.imeSetComposition",
            json!({ "text": "", "selectionStart": 0, "selectionEnd": 0 }),
        )),
        _ => Err(Error::not_supported(
            "os_ime_step",
            "the Chrome lane plays no such input-method step",
        )),
    }
}

/// Whether a composition is open once `action` was played: shown by a set, ended by a commit or a
/// cancel.
#[must_use]
pub fn composing_after(action: &ImeAction) -> bool {
    matches!(action, ImeAction::SetComposition { text, .. } if !text.is_empty())
}
