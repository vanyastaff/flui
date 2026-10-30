//! The winit host's text input: winit's IME enablement and cursor area.

use std::sync::Arc;

use flui_foundation::geometry::Bounds;
use flui_platform::PlatformTextInput;
use winit::window::Window;

use super::WindowTextInput;

/// [`PlatformTextInput`] for a winit window.
///
/// A thin wrapper around `Arc<winit::window::Window>` rather than an impl
/// directly on `WinitWindow`: `PlatformWindow::text_input` hands back an
/// `Arc<dyn PlatformTextInput>` from `&self`. Cloning the exact inner
/// `Arc<Window>` gives the capability independent ownership without cloning
/// or forwarding the platform-window object itself.
pub struct WinitTextInput {
    window: Arc<Window>,
}

impl WinitTextInput {
    /// The text input of `window`.
    pub(crate) fn new(window: Arc<Window>) -> Self {
        Self { window }
    }
}

impl std::fmt::Debug for WinitTextInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WinitTextInput").finish_non_exhaustive()
    }
}

impl super::TextInput for Arc<WinitTextInput> {
    fn window_text_input(&self) -> Option<WindowTextInput> {
        Some(WindowTextInput::Ime(
            Arc::clone(self) as Arc<dyn PlatformTextInput>
        ))
    }

    /// winit reports no system text size beyond the scale factor.
    fn capture_sizing(
        &self,
    ) -> Result<crate::TextSizingCaptureState, crate::TextSizingCaptureError> {
        Ok(crate::TextSizingCaptureState::Unsupported)
    }

    fn invalidate_sizing(&self) {}

    fn fence_sizing(&self) {}

    /// winit's IME state goes with its window.
    fn detach(&self) {}
}

/// Each winit window's text input drives that window's IME.
impl crate::host::Text<Arc<Window>> for WinitTextInput {
    fn for_window(window: &Arc<Window>) -> Arc<dyn super::TextInput> {
        Arc::new(Arc::new(Self::new(Arc::clone(window))))
    }
}

impl PlatformTextInput for WinitTextInput {
    fn set_ime_allowed(&self, allowed: bool) {
        self.window.set_ime_allowed(allowed);
    }

    fn set_ime_cursor_area(&self, area: Bounds<f64>) {
        use winit::dpi::{LogicalPosition, LogicalSize};

        self.window.set_ime_cursor_area(
            LogicalPosition::new(area.origin.x, area.origin.y),
            LogicalSize::new(area.size.width, area.size.height),
        );
    }

    /// A workaround: winit 0.30 has no call that ends a preedit without
    /// committing it, and disabling IME is the one that drops it
    /// ([`Window::set_ime_allowed`](https://docs.rs/winit/0.30/winit/window/struct.Window.html#method.set_ime_allowed)).
    /// Turning it back on keeps the field composing; the `Ime::Disabled` the
    /// toggle posts ends a composition the store no longer has.
    fn discard_composition(&self) {
        self.window.set_ime_allowed(false);
        self.window.set_ime_allowed(true);
    }
}
