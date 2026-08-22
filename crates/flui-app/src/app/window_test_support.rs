//! Shared [`PlatformWindow`] test double for flui-app's test modules.
//!
//! Before this module, six hand-rolled stubs implemented the same trait
//! surface across `presentation.rs`, `runtime.rs`, `ui_realm.rs`, and
//! `window_registry.rs`, differing only in a knob or two (window id, scale
//! factor, sizes, a redraw counter, an injected text-input or accessibility
//! capability). [`TestWindow`] carries every knob those stubs varied, with
//! the same defaults they shared.
//!
//! Deliberately NOT `flui_platform`'s `MockWindow`: that double is minted by
//! a live `HeadlessPlatform` and carries a back-reference into its
//! platform's window tracking and exit-policy machinery. State-level unit
//! tests here want a window value with zero platform ceremony; the tests
//! that DO want the real headless window (to exercise the platform's own
//! capabilities) use [`headless_test_window`].

use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use flui_platform::traits::{PlatformAccessibility, PlatformTextInput, PlatformWindow, WindowId};
use flui_types::geometry::{DevicePixels, Pixels, Size};

/// Configurable [`PlatformWindow`] double. Construct with [`TestWindow::new`],
/// adjust via the builder-style setters, then `Arc::new` it where a
/// `Arc<dyn PlatformWindow>` is needed.
pub(crate) struct TestWindow {
    id: WindowId,
    scale_factor: f64,
    physical_size: Size<DevicePixels>,
    logical_size: Size<Pixels>,
    focused: bool,
    /// Incremented by every [`PlatformWindow::request_redraw`]; hand the
    /// [`Self::redraw_calls_handle`] to the asserting side.
    redraw_calls: Arc<AtomicU32>,
    text_input: Option<Arc<dyn PlatformTextInput>>,
    accessibility: Option<Arc<dyn PlatformAccessibility>>,
    /// Last cursor set through [`PlatformWindow::set_cursor`]; read back via
    /// [`Self::cursor`].
    cursor: parking_lot::Mutex<flui_platform::CursorIcon>,
}

impl std::fmt::Debug for TestWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestWindow")
            .field("id", &self.id)
            .field("scale_factor", &self.scale_factor)
            .finish_non_exhaustive()
    }
}

impl TestWindow {
    pub(crate) fn new() -> Self {
        Self {
            id: WindowId(1),
            scale_factor: 1.0,
            physical_size: Size::default(),
            logical_size: Size::default(),
            focused: false,
            redraw_calls: Arc::new(AtomicU32::new(0)),
            text_input: None,
            accessibility: None,
            cursor: parking_lot::Mutex::new(flui_platform::CursorIcon::Default),
        }
    }

    pub(crate) fn with_id(mut self, id: u64) -> Self {
        self.id = WindowId(id);
        self
    }

    pub(crate) fn with_scale_factor(mut self, scale_factor: f64) -> Self {
        self.scale_factor = scale_factor;
        self
    }

    pub(crate) fn with_sizes(
        mut self,
        physical: Size<DevicePixels>,
        logical: Size<Pixels>,
    ) -> Self {
        self.physical_size = physical;
        self.logical_size = logical;
        self
    }

    pub(crate) fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub(crate) fn with_text_input(
        mut self,
        text_input: Option<Arc<dyn PlatformTextInput>>,
    ) -> Self {
        self.text_input = text_input;
        self
    }

    pub(crate) fn with_accessibility(
        mut self,
        accessibility: Arc<dyn PlatformAccessibility>,
    ) -> Self {
        self.accessibility = Some(accessibility);
        self
    }

    /// The counter [`PlatformWindow::request_redraw`] bumps — clone it out
    /// before `Arc`-ing the window.
    pub(crate) fn redraw_calls_handle(&self) -> Arc<AtomicU32> {
        Arc::clone(&self.redraw_calls)
    }

    /// The last cursor recorded by [`PlatformWindow::set_cursor`].
    pub(crate) fn cursor(&self) -> flui_platform::CursorIcon {
        *self.cursor.lock()
    }
}

impl PlatformWindow for TestWindow {
    fn id(&self) -> WindowId {
        self.id
    }

    fn physical_size(&self) -> Size<DevicePixels> {
        self.physical_size
    }

    fn logical_size(&self) -> Size<Pixels> {
        self.logical_size
    }

    fn scale_factor(&self) -> f64 {
        self.scale_factor
    }

    fn request_redraw(&self) {
        self.redraw_calls.fetch_add(1, Ordering::Relaxed);
    }

    fn is_focused(&self) -> bool {
        self.focused
    }

    fn is_visible(&self) -> bool {
        true
    }

    fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> {
        self.text_input.clone()
    }

    fn accessibility(&self) -> Option<Arc<dyn PlatformAccessibility>> {
        self.accessibility.clone()
    }

    fn set_cursor(
        &self,
        cursor: flui_platform::CursorIcon,
    ) -> Result<(), flui_platform::CursorError> {
        *self.cursor.lock() = cursor;
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// A REAL window from the headless platform, for tests that exercise the
/// platform's own capabilities (haptics, deferred opens, exit policy) rather
/// than a state-level double.
pub(crate) fn headless_test_window() -> Arc<dyn PlatformWindow> {
    flui_platform::headless_platform()
        .open_window(flui_platform::traits::WindowOptions::default())
        .expect("headless platform should create a test window")
}
