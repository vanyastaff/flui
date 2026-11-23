//! The real headless windows flui-app's test modules open, for tests that exercise the
//! platform's own capabilities. A test that needs only a window uses
//! `flui_platform::testing::MockWindow`.

#[cfg(not(target_os = "android"))]
use crate::app::runner::window_routes::OpenRouted as _;
#[cfg(not(target_os = "android"))]
use std::rc::Rc;

#[cfg(not(target_os = "android"))]
use flui_native::HostWindow;
#[cfg(not(target_os = "android"))]
use flui_platform::PlatformWindow;

/// A REAL window from the headless platform, for tests that exercise the
/// platform's own capabilities (haptics, deferred opens, exit policy) rather
/// than a state-level double.
#[cfg(not(target_os = "android"))]
pub(crate) fn headless_test_window() -> Rc<dyn PlatformWindow> {
    headless_test_host_window()
}

/// [`headless_test_window`] as the runner receives it from `open_window`.
#[cfg(not(target_os = "android"))]
pub(crate) fn headless_test_host_window() -> Rc<dyn HostWindow> {
    flui_native::headless_platform()
        .open_routed(flui_platform::WindowOptions::default())
        .expect("headless platform should create a test window")
}
