//! Test support for the native hosts (ADR-0243): what a test of a host's
//! windows needs from the host itself, such as turns of its event loop for a
//! window fixture's `settle`. Behind the `testing` feature; not part of the
//! release API.

#[cfg(target_os = "macos")]
pub mod appkit;
