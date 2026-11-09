//! Test support for the native hosts (ADR-0243): the application handlers a
//! test runs a platform with, and what a test of a host's windows needs from
//! the host itself, such as turns of its event loop for a window fixture's
//! `settle`, and the scenarios every host runs over its own clipboard. Behind
//! the `testing` feature; not part of the release API.

pub mod app;
#[cfg(target_os = "macos")]
pub mod appkit;
pub mod clipboard;
/// The `docs/gaps.md` entry a scenario fails as, and the known-failure assertion.
pub mod gap;
/// The request-per-thread HTTP client's scripted stack.
#[cfg(test)]
pub(crate) mod http;
/// The parity table rendered from `platform-parity.toml`.
pub mod parity;
#[cfg(all(target_os = "linux", feature = "wayland"))]
pub mod wayland;
#[cfg(all(target_os = "linux", feature = "x11"))]
pub mod x11;
