//! The Web's accessibility: a parallel ARIA DOM over the canvas (ADR-0255
//! decision 18), since AccessKit has no adapter for the browser.
//!
//! - `aria`: what one AccessKit node becomes, as pure Rust tested on the
//!   Linux host;
//! - `mirror`: the mirrored tree and the DOM operations each update needs;
//! - `selection`: offsets in a paragraph's text against its runs' positions;
//! - `dom`: the adapter a canvas window hands out, which runs those
//!   operations on the semantics host and turns DOM events into actions.

pub(crate) mod aria;
#[cfg(target_arch = "wasm32")]
mod dom;
pub(crate) mod mirror;
pub(crate) mod selection;

#[cfg(target_arch = "wasm32")]
pub(crate) use dom::WebAccessibility;
