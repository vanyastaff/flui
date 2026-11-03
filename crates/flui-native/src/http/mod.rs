//! The host's own HTTP stacks behind `flui_platform::http::HttpClient`
//! (ADR-0280).
//!
//! The web sends through the page's `fetch` (`FetchClient`, wasm32), and
//! Android through `HttpURLConnection` (`UrlConnectionClient`), each request
//! on a thread of its own (the `blocking` module's model). The native desktop
//! hosts use the composition root's `reqwest` client (`docs/gaps.md`, entry
//! 42).

#[cfg(target_os = "android")]
mod android;
#[cfg(any(target_os = "android", test))]
pub(crate) mod blocking;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(target_os = "android")]
pub use android::UrlConnectionClient;
#[cfg(target_arch = "wasm32")]
pub use web::FetchClient;
