//! Window test doubles for flui-app's test modules.
//!
//! The state-level [`TestWindow`] double is the frame runtime's
//! (`flui_runtime::testing`), shared with the UI runtime tests there. What stays
//! here is what names a `flui-native` type: [`HostedTestWindow`], a
//! `TestWindow` offered as a [`HostWindow`] the way an `open_window` reply
//! is, and the real headless windows for tests that exercise the platform's
//! own capabilities.

#[cfg(not(target_os = "android"))]
use crate::app::runner::window_routes::OpenRouted as _;
use std::sync::Arc;

#[cfg(not(target_os = "android"))]
use flui_native::HostWindow;
#[cfg(not(target_os = "android"))]
use flui_platform::PlatformWindow;
pub(crate) use flui_runtime::testing::TestWindow;

/// A REAL window from the headless platform, for tests that exercise the
/// platform's own capabilities (haptics, deferred opens, exit policy) rather
/// than a state-level double.
#[cfg(not(target_os = "android"))]
pub(crate) fn headless_test_window() -> Arc<dyn PlatformWindow> {
    headless_test_host_window()
}

/// [`headless_test_window`] as the runner receives it from `open_window`.
#[cfg(not(target_os = "android"))]
pub(crate) fn headless_test_host_window() -> Arc<dyn HostWindow> {
    flui_native::headless_platform()
        .open_routed(flui_platform::WindowOptions::default())
        .expect("headless platform should create a test window")
}
