//! The web host's text input (ADR-0090, ADR-0135): the focused field's store
//! mirrored into an `EditContext` on the canvas, or a hidden textarea beside
//! it where the browser has none, and served to the browser's input method.
//!
//! - [`ime`]: the decidable half — when a key belongs to the input method,
//!   and which virtual keyboard a field asks for — built on every target (see
//!   the feature's doc);
//! - `input`: `WebTextInput`, the window's `TextStoreHost` over the DOM.

pub mod ime;
#[cfg(target_arch = "wasm32")]
mod input;

#[cfg(target_arch = "wasm32")]
pub(crate) use input::WebTextInput;
