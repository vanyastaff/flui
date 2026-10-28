//! Test support for the native hosts (ADR-0243): the application handlers a
//! test runs a platform with, and what a test of a host's windows needs from
//! the host itself, such as turns of its event loop for a window fixture's
//! `settle`. Behind the `testing` feature; not part of the release API.

pub mod app;
#[cfg(target_os = "macos")]
pub mod appkit;
#[cfg(all(target_os = "linux", feature = "wayland"))]
pub mod wayland;
