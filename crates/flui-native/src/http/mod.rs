//! The host's own HTTP stacks behind `flui_platform::http::HttpClient`
//! (ADR-0280).
//!
//! Only the web has one here: `FetchClient` (wasm32) sends through the page's
//! `fetch`. The native desktop hosts use the composition root's `reqwest`
//! client, and Android has none yet (`docs/gaps.md`, entry 42).

#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(target_arch = "wasm32")]
pub use web::FetchClient;
