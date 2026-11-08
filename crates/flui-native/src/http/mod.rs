//! The host's own HTTP stacks behind `flui_platform::http::HttpClient`
//! (ADR-0280).
//!
//! The web sends through the page's `fetch` (`FetchClient`, wasm32), Android
//! through `HttpURLConnection` (`UrlConnectionClient`), Windows through
//! WinHTTP (`WinHttpClient`), macOS and iOS through `NSURLSession`
//! (`UrlSessionClient`) and the native hosts without a system stack (Linux
//! and the BSDs) through `ureq` (`UreqClient`, the `http` feature), the
//! native ones each request on a thread of its own (the `blocking` module's
//! model).

#[cfg(target_os = "android")]
mod android;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod apple;
#[cfg(any(
    target_os = "android",
    target_os = "windows",
    target_os = "macos",
    target_os = "ios",
    ureq_host,
    test
))]
pub(crate) mod blocking;
#[cfg(any(
    target_os = "android",
    target_os = "windows",
    target_os = "macos",
    target_os = "ios",
    target_arch = "wasm32",
    ureq_host,
    test
))]
mod head;
#[cfg(ureq_host)]
mod ureq_stack;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(any(target_os = "windows", test))]
mod winhttp;

#[cfg(target_os = "android")]
pub use android::UrlConnectionClient;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use apple::UrlSessionClient;
#[cfg(ureq_host)]
pub use ureq_stack::UreqClient;
#[cfg(target_arch = "wasm32")]
pub use web::FetchClient;
#[cfg(target_os = "windows")]
pub use winhttp::WinHttpClient;
