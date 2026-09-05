//! The winit backend's [`PlatformWindow`]: a thin wrapper over
//! `Arc<winit::window::Window>` carrying the platform-minted [`WindowId`],
//! the per-window callback table, and the route its programmatic `close()`
//! takes back to the event-loop owner (`ControlSender`, backend-private —
//! which is why this type lives here, in the backend, and not beside the
//! trait it implements).

use std::{any::Any, sync::Arc};

use cursor_icon::CursorIcon;
use flui_types::geometry::{Bounds, DevicePixels, Pixels, Size};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::Window;

use super::control::ControlSender;
#[cfg(all(target_os = "linux", feature = "a11y"))]
use crate::traits::PlatformAccessibility;
use crate::traits::{CursorError, PlatformTextInput, PlatformWindow, WindowAppearance, WindowId};

/// Concrete winit window wrapper
///
/// Wraps `winit::window::Window` to implement `PlatformWindow`.
/// Includes per-window callbacks for event delivery using the causal FIFO
/// dispatch pattern for reentrancy safety.
pub struct WinitWindow {
    id: WindowId,
    window: Arc<Window>,
    is_focused: parking_lot::Mutex<bool>,
    is_visible: parking_lot::Mutex<bool>,
    callbacks: crate::shared::WindowCallbacks,
    /// This window's AT-SPI announcement, created with the window so a
    /// screen reader that attaches at any later point finds it on the bus.
    /// Construction succeeds (and stays inert) with no session bus at all —
    /// see [`UnixAccessibility::new`](crate::platforms::linux::UnixAccessibility::new).
    #[cfg(all(target_os = "linux", feature = "a11y"))]
    accessibility: Arc<crate::platforms::linux::UnixAccessibility>,
    /// The route a programmatic [`PlatformWindow::close`] takes to the
    /// event-loop owner (issue #919). The close teardown needs the live
    /// `ActiveEventLoop` (to end the loop when this was the last window),
    /// which only an owner-turn callback holds — so `close()` cannot run it
    /// in place; it posts a non-droppable per-window request and the owner
    /// runs the exact teardown the compositor's `CloseRequested` arm runs.
    close_lane: ControlSender,
}

/// [`PlatformTextInput`] for a winit window.
///
/// A thin wrapper around `Arc<winit::window::Window>` rather than an impl
/// directly on `WinitWindow`: `PlatformWindow::text_input` hands back an
/// `Arc<dyn PlatformTextInput>` from `&self`. Cloning the exact inner
/// `Arc<Window>` gives the capability independent ownership without cloning
/// or forwarding the platform-window object itself.
struct WinitTextInput {
    window: Arc<Window>,
}

impl PlatformTextInput for WinitTextInput {
    fn set_ime_allowed(&self, allowed: bool) {
        self.window.set_ime_allowed(allowed);
    }

    fn set_ime_cursor_area(&self, area: Bounds<Pixels>) {
        use winit::dpi::{LogicalPosition, LogicalSize};

        self.window.set_ime_cursor_area(
            LogicalPosition::new(f64::from(area.origin.x.0), f64::from(area.origin.y.0)),
            LogicalSize::new(f64::from(area.size.width.0), f64::from(area.size.height.0)),
        );
    }
}

impl std::fmt::Debug for WinitWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `WindowCallbacks` holds boxed closures that don't implement
        // `Debug`; print the focus/visibility flags only.
        f.debug_struct("WinitWindow")
            .field("is_focused", &*self.is_focused.lock())
            .field("is_visible", &*self.is_visible.lock())
            .finish_non_exhaustive()
    }
}

impl WinitWindow {
    /// Create a new WinitWindow wrapper, addressed by the platform-minted
    /// `id` the caller already allocated for it, with the owner lane its
    /// programmatic `close()` reports to. Backend-private: only this
    /// backend's own window creation can supply that lane, so nothing
    /// outside it can construct a window whose close would go nowhere.
    pub(super) fn new(id: WindowId, window: Arc<Window>, close_lane: ControlSender) -> Self {
        Self {
            id,
            window,
            is_focused: parking_lot::Mutex::new(true),
            is_visible: parking_lot::Mutex::new(true),
            callbacks: crate::shared::WindowCallbacks::new(),
            #[cfg(all(target_os = "linux", feature = "a11y"))]
            accessibility: Arc::new(crate::platforms::linux::UnixAccessibility::new()),
            close_lane,
        }
    }

    /// Get the underlying `Arc<Window>`
    pub fn inner(&self) -> &Arc<Window> {
        &self.window
    }

    /// Get a reference to the per-window callbacks
    pub fn callbacks(&self) -> &crate::shared::WindowCallbacks {
        &self.callbacks
    }

    /// Update focus state
    pub fn set_focused(&self, focused: bool) {
        *self.is_focused.lock() = focused;
    }

    /// Update visibility state
    pub fn set_visible(&self, visible: bool) {
        *self.is_visible.lock() = visible;
    }
}

impl Drop for WinitWindow {
    fn drop(&mut self) {
        // Teardown-order invariant: a registered callback may own this
        // window's GPU renderer, whose `wgpu::Surface` was created from
        // this window's raw handles — that surface must be destroyed while
        // the native window objects behind those handles are still alive
        // (on Wayland, destroying the swapchain after the `wl_surface` is a
        // use-after-free on the surface's `wl_proxy`; observed as the
        // post-quit SIGSEGV of issue #713). `Drop::drop` runs before any
        // field is dropped, so clearing the callbacks here guarantees the
        // renderer dies before `self.window` regardless of field order.
        // The winit `CloseRequested` arm also clears eagerly at close (the
        // primary, in-loop path); this is the last-resort guarantee for a
        // window whose final `Arc` unwinds anywhere else.
        self.callbacks.clear();
    }
}

impl PlatformWindow for WinitWindow {
    fn id(&self) -> WindowId {
        self.id
    }

    fn physical_size(&self) -> Size<DevicePixels> {
        use flui_types::geometry::device_px;

        let size = self.window.inner_size();
        Size::new(device_px(size.width as i32), device_px(size.height as i32))
    }

    fn logical_size(&self) -> Size<Pixels> {
        use flui_types::geometry::px;

        let size = self.window.inner_size();
        let scale = self.window.scale_factor() as f32;
        Size::new(
            px(size.width as f32 / scale),
            px(size.height as f32 / scale),
        )
    }

    fn appearance(&self) -> WindowAppearance {
        // The live winit theme, so an appearance-change consumer querying at
        // dispatch time (and the bootstrap's initial seed) reads the REAL
        // value — the trait default is a permanent `Light` that silently
        // killed the whole dark-mode wire. `None` (winit cannot determine
        // the theme on this platform) keeps the light default.
        match self.window.theme() {
            Some(winit::window::Theme::Dark) => WindowAppearance::Dark,
            Some(winit::window::Theme::Light) | None => WindowAppearance::Light,
        }
    }

    fn scale_factor(&self) -> f64 {
        self.window.scale_factor()
    }

    fn request_redraw(&self) {
        self.window.request_redraw();
    }

    fn refresh_period(&self) -> Option<std::time::Duration> {
        self.window
            .current_monitor()
            .and_then(|monitor| monitor.refresh_rate_millihertz())
            .filter(|&millihertz| millihertz > 0)
            .map(|millihertz| std::time::Duration::from_secs_f64(1000.0 / f64::from(millihertz)))
    }

    fn pre_present_notify(&self) {
        // The Wayland frame-callback arm (see the trait doc); harmless
        // elsewhere. winit documents this as "call before presenting".
        self.window.pre_present_notify();
    }

    fn is_focused(&self) -> bool {
        *self.is_focused.lock()
    }

    fn is_visible(&self) -> bool {
        *self.is_visible.lock()
    }

    fn set_title(&self, title: &str) {
        self.window.set_title(title);
    }

    fn minimize(&self) {
        self.window.set_minimized(true);
    }

    fn maximize(&self) {
        self.window.set_maximized(true);
    }

    fn restore(&self) {
        self.window.set_minimized(false);
        self.window.set_maximized(false);
    }

    fn toggle_fullscreen(&self) {
        use winit::window::Fullscreen;
        let current = self.window.fullscreen();
        if current.is_some() {
            self.window.set_fullscreen(None);
        } else {
            self.window
                .set_fullscreen(Some(Fullscreen::Borderless(None)));
        }
    }

    fn close(&self) {
        // Hide now, from whichever thread this is called on: the visible
        // effect of a close should not wait for the owner's next turn. The
        // close itself — `on_close`, map removal, callback clear, exit
        // policy — is the owner's (see `close_lane`): running `on_close`
        // here would fire it on the caller's thread, where an embedder's
        // owner-affine close handling (`flui-app` rejects realm dispatch
        // off its owner thread) would silently refuse it, and skipping the
        // map removal is precisely issue #919 — a hidden window that the
        // exit policy still counts, so the process never exits.
        self.window.set_visible(false);
        *self.is_visible.lock() = false;
        self.close_lane.request_close_window(self.id);
    }

    fn set_cursor(&self, cursor: CursorIcon) -> Result<(), CursorError> {
        self.window.set_cursor(cursor);
        Ok(())
    }

    crate::shared::impl_window_callback_setters!(callbacks);

    // GPU integration: `winit::window::Window` implements `HasWindowHandle`/
    // `HasDisplayHandle` directly — without these overrides both fall through
    // to the trait defaults (`Err(HandleError::Unavailable)`), which is what
    // made every wgpu surface creation on this backend fail regardless of
    // which GPU backend was compiled in.
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        self.window.window_handle()
    }

    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        self.window.display_handle()
    }

    fn as_winit(&self) -> Option<&Arc<Window>> {
        Some(&self.window)
    }

    fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> {
        Some(Arc::new(WinitTextInput {
            window: Arc::clone(&self.window),
        }))
    }

    /// The window's own AT-SPI bridge — the capability the composition
    /// root's accessibility wire discovers. Without this override the trait
    /// default (`None`) makes every real Linux window silently
    /// screen-reader-invisible while the headless fake works, which is
    /// exactly backwards.
    #[cfg(all(target_os = "linux", feature = "a11y"))]
    fn accessibility(&self) -> Option<Arc<dyn PlatformAccessibility>> {
        Some(Arc::clone(&self.accessibility) as _)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    // No `haptics()` override: desktop winit targets have no haptic
    // hardware to drive, so the `PlatformWindow` trait default (`None`) is
    // the permanent correct answer here, not a stub awaiting a backend.
}
