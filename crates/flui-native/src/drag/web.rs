//! HTML drag and drop, as target and as source (ADR-0038, ADR-0211 §11).
//!
//! - [`html_model`]: how `DataTransfer` types and effect names map to transfer
//!   formats and actions, and what string a payload is stored as, decided
//!   without the DOM; every host checks it;
//! - `html`: the DOM side in the browser — the transport, a canvas's drop
//!   target and a window's drag source.

mod html_model;

#[cfg(target_arch = "wasm32")]
mod html;

#[cfg(target_arch = "wasm32")]
pub(crate) use html::{CanvasDrag, WebTransfer, register};
