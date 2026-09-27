//! Window test doubles for flui-app's test modules.
//!
//! The state-level [`TestWindow`] double is the frame runtime's
//! (`flui_runtime::testing`), shared with the realm tests there. What stays
//! here is what names a `flui-platform` type: [`HostedTestWindow`], a
//! `TestWindow` offered as a [`HostWindow`] the way an `open_window` reply
//! is, and the real headless windows for tests that exercise the platform's
//! own capabilities.

use std::sync::Arc;

use flui_platform::traits::{HostWindow, PlatformAccessibility, PlatformWindow};
pub(crate) use flui_runtime::testing::TestWindow;

/// A [`TestWindow`] as the runner receives a window from `open_window`: a
/// [`HostWindow`] whose accessibility bridge is the one the double carries.
///
/// A wrapper because neither `HostWindow` nor `TestWindow` is this crate's.
/// It delegates every method `TestWindow` overrides, and `as_any` exposes
/// the inner `TestWindow`, so a downcast sees the same double.
pub(crate) struct HostedTestWindow(TestWindow);

impl HostedTestWindow {
    pub(crate) fn new(window: TestWindow) -> Self {
        Self(window)
    }
}

impl std::fmt::Debug for HostedTestWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("HostedTestWindow").field(&self.0).finish()
    }
}

impl PlatformWindow for HostedTestWindow {
    fn show(&self) -> Result<(), flui_platform::WindowShowError> {
        self.0.show()
    }

    fn id(&self) -> flui_platform::traits::WindowId {
        self.0.id()
    }

    fn physical_size(&self) -> flui_types::geometry::Size<flui_types::geometry::DevicePixels> {
        self.0.physical_size()
    }

    fn logical_size(&self) -> flui_types::geometry::Size<flui_types::geometry::Pixels> {
        self.0.logical_size()
    }

    fn scale_factor(&self) -> f64 {
        self.0.scale_factor()
    }

    fn pre_present_notify(&self) {
        self.0.pre_present_notify();
    }

    fn request_redraw(&self) {
        self.0.request_redraw();
    }

    fn is_focused(&self) -> bool {
        self.0.is_focused()
    }

    fn is_visible(&self) -> bool {
        self.0.is_visible()
    }

    fn text_input(&self) -> Option<Arc<dyn flui_platform::traits::PlatformTextInput>> {
        self.0.text_input()
    }

    fn set_cursor(
        &self,
        cursor: flui_platform::CursorIcon,
    ) -> Result<(), flui_platform::CursorError> {
        self.0.set_cursor(cursor)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self.0.as_any()
    }
}

impl HostWindow for HostedTestWindow {
    fn accessibility(&self) -> Option<Arc<dyn PlatformAccessibility>> {
        self.0.accessibility()
    }
}

/// A REAL window from the headless platform, for tests that exercise the
/// platform's own capabilities (haptics, deferred opens, exit policy) rather
/// than a state-level double.
pub(crate) fn headless_test_window() -> Arc<dyn PlatformWindow> {
    headless_test_host_window()
}

/// [`headless_test_window`] as the runner receives it from `open_window`.
pub(crate) fn headless_test_host_window() -> Arc<dyn HostWindow> {
    flui_platform::headless_platform()
        .open_window(flui_platform::traits::WindowOptions::default())
        .expect("headless platform should create a test window")
}
