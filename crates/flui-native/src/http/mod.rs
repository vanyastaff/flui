//! The host's own HTTP stacks behind `flui_platform::http::HttpClient`
//! (ADR-0280).
//!
//! The web sends through the page's `fetch` (`FetchClient`, wasm32), Android
//! through `HttpURLConnection` (`UrlConnectionClient`), Windows through
//! WinHTTP (`WinHttpClient`) and macOS and iOS through `NSURLSession`
//! (`UrlSessionClient`), the native ones each request on a thread of its own
//! (the `blocking` module's model). Linux uses the composition root's
//! `reqwest` client (`docs/gaps.md`, entry 42).

#[cfg(target_os = "android")]
mod android;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod apple;
#[cfg(any(
    target_os = "android",
    target_os = "windows",
    target_os = "macos",
    target_os = "ios",
    test
))]
pub(crate) mod blocking;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(any(target_os = "windows", test))]
mod winhttp;

#[cfg(target_os = "android")]
pub use android::UrlConnectionClient;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use apple::UrlSessionClient;
#[cfg(target_arch = "wasm32")]
pub use web::FetchClient;
#[cfg(target_os = "windows")]
pub use winhttp::WinHttpClient;
