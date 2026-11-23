//! Test support for the native hosts (ADR-0243); not part of the release API.
//!
//! With the `scripted` feature, the headless host's doubles and controls: the
//! [`HeadlessPlatform`] a test runs an application on, the windows it opens
//! ([`HeadlessHostWindow`]), the handles that drive it from outside
//! ([`HeadlessOwnerTurns`], [`HeadlessHostEvents`], [`HeadlessDisplayControls`], ...)
//! and the capability doubles ([`FakeTextInput`], [`FakeAccessibility`]). They are
//! named here and nowhere else: the crate root names no double.
//!
//! With the `testing` feature too, the submodules: the application handlers a
//! test runs a platform with, what a test of a host's windows needs from the
//! host itself, such as turns of its event loop for a window fixture's `settle`,
//! and the scenarios every host runs over its own clipboard.

#[cfg(feature = "scripted")]
pub use crate::accessibility::headless::FakeAccessibility;
#[cfg(feature = "scripted")]
pub use crate::display::headless::{
    DisplayChange, DisplayChangeError, DisplaySpec, HeadlessDisplayControls,
};
#[cfg(feature = "scripted")]
pub use crate::drag::headless::HeadlessDragAndDrop;
#[cfg(feature = "scripted")]
pub use crate::hosts::HeadlessPlatform;
#[cfg(feature = "scripted")]
pub use crate::input::headless::HeadlessInputDevices;
#[cfg(feature = "scripted")]
pub use crate::lifecycle::headless::HeadlessHostEvents;
#[cfg(feature = "scripted")]
pub use crate::owner::headless::{
    HeadlessDeferredWindowOpens, HeadlessExitReevaluation, HeadlessOwnerTurns,
};
#[cfg(feature = "scripted")]
pub use crate::preferences::headless::HeadlessPreferences;
#[cfg(feature = "scripted")]
pub use crate::text::headless::FakeTextInput;
#[cfg(feature = "scripted")]
pub use crate::window::headless::{
    HeadlessHostWindow, HeadlessOpenWindows, HeadlessWindowPolicy, HeadlessWindowSystem,
    UnknownDisplay,
};

// The submodules are the heavy half: the Wayland server and the native
// recording codec come with the `testing` feature, which this crate's own
// tests turn on through its dev-dependency.
#[cfg(any(test, feature = "testing"))]
pub mod app;
#[cfg(all(target_os = "macos", any(test, feature = "testing")))]
pub mod appkit;
#[cfg(any(test, feature = "testing"))]
pub mod clipboard;
/// The `docs/gaps.md` entry a scenario fails as, and the known-failure assertion.
#[cfg(any(test, feature = "testing"))]
pub mod gap;
/// The request-per-thread HTTP client's scripted stack.
#[cfg(test)]
pub(crate) mod http;
#[cfg(any(test, feature = "testing"))]
pub mod keyboard_layout;
/// How a unit test counts the log lines a host writes.
#[cfg(test)]
pub(crate) mod log;
/// `docs/platform-parity.md`, rendered from `platform-parity.toml`.
#[cfg(test)]
pub(crate) mod parity;
#[cfg(all(
    target_os = "linux",
    feature = "wayland",
    any(test, feature = "testing")
))]
pub mod wayland;
// A server alone, so it needs only the `testing` feature. Its own `//!` is its
// doc: an outer doc here would resolve that one's links in this scope.
#[cfg(all(target_os = "linux", feature = "testing"))]
pub mod wayland_compositor;
#[cfg(all(target_os = "linux", feature = "x11", any(test, feature = "testing")))]
pub mod x11;
#[cfg(all(target_os = "linux", any(test, feature = "testing")))]
pub mod xvfb;
