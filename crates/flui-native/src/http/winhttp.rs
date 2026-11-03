//! [`WinHttpClient`]: HTTP through Windows' WinHTTP (ADR-0280).
//!
//! Each request runs on a thread of its own (the `blocking` module's
//! model), over WinHTTP in asynchronous mode: the thread starts each
//! operation and waits for the status callback to complete it. So an abort
//! (a dropped request or body, a passed deadline) closes the request handle
//! while no call is inside WinHTTP on it, and the pending operation ends
//! with the close; a handle is never used once closed. The session brings
//! the system's proxy settings (WPAD and the user's settings) and its trust
//! store.
//!
//! Every failure becomes an [`HttpError`](flui_platform::http::HttpError):
//! WinHTTP's error code by its meaning, a failed secure check naming what
//! failed, and anything else by the stage that failed. Nothing panics on
//! the request's thread, and the status callback runs inside the crate's
//! panic boundary.

mod gate;
#[cfg(target_os = "windows")]
mod stack;
mod wire;

#[cfg(target_os = "windows")]
pub use client::WinHttpClient;

#[cfg(target_os = "windows")]
mod client {
    use flui_platform::http::{HttpAuthority, HttpClient, HttpFuture, HttpRequest, HttpResponse};

    use super::stack::Sessions;
    use crate::http::blocking::BlockingClient;

    /// An HTTP client over Windows' WinHTTP (ADR-0280).
    ///
    /// Each request runs on a thread of its own and answers through a
    /// channel, so the future [`send`](HttpClient::send) returns, and the
    /// response's body, are polled on any thread with no runtime of the
    /// caller's. The body is read a chunk at a time, only as fast as it is
    /// read. Dropping the request or its body aborts it, and the request's
    /// deadline aborts it with
    /// [`HttpError::Timeout`](flui_platform::http::HttpError::Timeout). One
    /// WinHTTP session per client keeps its connections.
    pub struct WinHttpClient(BlockingClient<Sessions>);

    impl WinHttpClient {
        /// A client over the system's WinHTTP; its session opens on the
        /// first request.
        #[must_use]
        pub fn new() -> Self {
            Self(BlockingClient::new(Sessions::new()))
        }
    }

    impl Default for WinHttpClient {
        fn default() -> Self {
            Self::new()
        }
    }

    impl std::fmt::Debug for WinHttpClient {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("WinHttpClient").finish_non_exhaustive()
        }
    }

    impl HttpClient for WinHttpClient {
        fn authority(&self) -> HttpAuthority {
            self.0.authority()
        }

        fn send(&self, request: HttpRequest) -> HttpFuture<HttpResponse> {
            self.0.send(request)
        }
    }
}
