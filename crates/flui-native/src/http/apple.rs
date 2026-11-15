//! [`UrlSessionClient`]: HTTP through Apple's `NSURLSession`.
//!
//! Each request runs on a thread of its own (the `blocking` module's model)
//! and waits there for what the session's delegate hears on the session's own
//! serial queue: the response, the body's data and the end. The system stack
//! brings the user's proxy settings and the keychain's trust, the certificates
//! a user or a configuration profile installed included. The session is
//! ephemeral: like the desktop client it keeps no cache or cookies on disk.
//!
//! `session` is the client, its `NSURLSession` and the delegate; `transfer`
//! is what the delegate and a request's thread share, which is pure and runs
//! on every host. The body waits there in a buffer between the two: a buffer
//! at its limit suspends the task, so the network stops delivering, and the
//! reader draining it resumes it. Every `NSError` becomes an
//! [`HttpError`](flui_platform::http::HttpError) by its code in
//! `NSURLErrorDomain` (a TLS failure is `Tls`, a timeout `Timeout`, a failed
//! lookup or connection `Connect`). The delegate's methods are OS entries,
//! each one call of `os_entry` (ADR-0151 §16), and a step of one that panics
//! ends its transfer with `Host`.

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod session;
mod transfer;

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use session::UrlSessionClient;
