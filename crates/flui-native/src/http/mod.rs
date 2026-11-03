//! The host's own HTTP stacks behind `flui_platform::http::HttpClient`
//! (ADR-0280).
//!
//! The web sends through the page's `fetch` (`FetchClient`, wasm32), Android
//! through `HttpURLConnection` (`UrlConnectionClient`) and Windows through
//! WinHTTP (`WinHttpClient`), each request on a thread of its own (the
//! `blocking` module's model). The other native desktop hosts use the
//! composition root's `reqwest` client (`docs/gaps.md`, entry 42).

#[cfg(target_os = "android")]
mod android;
#[cfg(any(target_os = "android", target_os = "windows", test))]
pub(crate) mod blocking;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(any(target_os = "windows", test))]
mod winhttp;

#[cfg(target_os = "android")]
pub use android::UrlConnectionClient;
#[cfg(target_arch = "wasm32")]
pub use web::FetchClient;
#[cfg(target_os = "windows")]
pub use winhttp::WinHttpClient;
