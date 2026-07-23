//! Platform window trait
//!
//! Provides a thin abstraction over platform windows for testability
//! and flexibility. Includes per-window callback registration for event
//! delivery.

use std::{any::Any, sync::Arc};

use cursor_icon::CursorIcon;
use flui_types::geometry::{Bounds, DevicePixels, Pixels, Point, Size};

use super::{
    display::PlatformDisplay,
    haptics::PlatformHaptics,
    input::{DispatchEventResult, Modifiers, PlatformInput},
    text_input::PlatformTextInput,
};

// ==================== Value Types ====================

/// Window appearance (light/dark theme)
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum WindowAppearance {
    /// Light appearance (default)
    #[default]
    Light,
    /// Dark appearance
    Dark,
    /// Vibrant light (macOS-style translucent light)
    VibrantLight,
    /// Vibrant dark (macOS-style translucent dark)
    VibrantDark,
}

/// Window background appearance (backdrop material)
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum WindowBackgroundAppearance {
    /// Opaque background (default)
    #[default]
    Opaque,
    /// Transparent background
    Transparent,
    /// Blurred background
    Blurred,
    /// Windows 11 Mica backdrop
    MicaBackdrop,
    /// Windows 11 Mica Alt backdrop
    MicaAltBackdrop,
}

/// Window bounds state (windowed, maximized, or fullscreen)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WindowBounds {
    /// Normal windowed mode with specific bounds
    Windowed(Bounds<Pixels>),
    /// Maximized with bounds
    Maximized(Bounds<Pixels>),
    /// Fullscreen with bounds
    Fullscreen(Bounds<Pixels>),
}

/// Failure to apply a cursor to one exact platform window.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CursorError {
    /// The backend has no pointer-cursor facility for this window.
    #[error("this platform window does not support pointer cursors")]
    Unsupported,
    /// The backend rejected a concrete cursor update.
    #[error("platform cursor update failed: {0}")]
    Backend(String),
}

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
#[cfg(feature = "winit-backend")]
use winit::window::Window;

/// Trait for platform window abstraction
///
/// Provides a minimal interface for window operations, enabling
/// testing and future flexibility (e.g., headless rendering).
///
/// # Callback Registration
///
/// Per-window callbacks use `&self` (not `&mut self`) with interior mutability.
/// This allows registering callbacks on shared references (`Arc<dyn
/// PlatformWindow>`). Callbacks are invoked by the platform's event loop when
/// native events arrive.
///
/// Callback storage locks are released before user code is invoked. Nested
/// notifications share one causal FIFO across event kinds; see
/// [`crate::WindowCallbacks`] for nested input return semantics.
pub trait PlatformWindow: Send + Sync {
    /// Get the window size in physical pixels (device pixels)
    fn physical_size(&self) -> Size<DevicePixels>;

    /// Get the window size in logical pixels
    fn logical_size(&self) -> Size<Pixels>;

    /// Get the scale factor (DPI scaling)
    fn scale_factor(&self) -> f64;

    /// Request a redraw
    fn request_redraw(&self);

    /// Check if window is focused
    fn is_focused(&self) -> bool;

    /// Check if window is visible
    fn is_visible(&self) -> bool;

    // ==================== Query Methods (US2) ====================

    /// Get the window bounds (position + size) in logical pixels
    fn bounds(&self) -> Bounds<Pixels> {
        Bounds::default()
    }

    /// Get the content (client area) size in logical pixels
    fn content_size(&self) -> Size<Pixels> {
        self.logical_size()
    }

    /// Get the window bounds state (windowed, maximized, or fullscreen)
    fn window_bounds(&self) -> WindowBounds {
        WindowBounds::Windowed(self.bounds())
    }

    /// Check if window is maximized
    fn is_maximized(&self) -> bool {
        false
    }

    /// Check if window is in fullscreen mode
    fn is_fullscreen(&self) -> bool {
        false
    }

    /// Check if window is the active (foreground) window
    fn is_active(&self) -> bool {
        self.is_focused()
    }

    /// Check if the mouse cursor is hovering over this window
    fn is_hovered(&self) -> bool {
        false
    }

    /// Get the current mouse position in logical pixels (relative to window)
    fn mouse_position(&self) -> Point<Pixels> {
        Point::default()
    }

    /// Get the currently pressed keyboard modifiers
    fn modifiers(&self) -> Modifiers {
        Modifiers::empty()
    }

    /// Get the window's current appearance (light/dark)
    fn appearance(&self) -> WindowAppearance {
        WindowAppearance::default()
    }

    /// Get the display this window is currently on
    fn display(&self) -> Option<Arc<dyn PlatformDisplay>> {
        None
    }

    /// Get this window's IME text-input capability, if the backend supports
    /// it. `None` for backends that cannot honor IME composition (returned
    /// by this trait's default so every non-desktop/no-IME backend does not
    /// have to inherit unusable `set_ime_allowed`/`set_ime_cursor_area`
    /// methods directly on `PlatformWindow`).
    fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> {
        None
    }

    /// Get this window's haptic feedback capability, if the backend
    /// supports it. `None` for backends with no haptic hardware (desktop
    /// winit targets; a minimal future embedder) — see
    /// [`PlatformHaptics`]'s module doc for the full per-window-not-global
    /// rationale.
    fn haptics(&self) -> Option<Arc<dyn PlatformHaptics>> {
        None
    }

    /// Get the window title
    fn get_title(&self) -> String {
        String::new()
    }

    // ==================== Control Methods (US2) ====================

    /// Set the window title
    fn set_title(&self, title: &str) {
        let _ = title;
    }

    /// Activate (bring to front / focus) the window
    fn activate(&self) {}

    /// Minimize the window
    fn minimize(&self) {}

    /// Maximize the window
    fn maximize(&self) {}

    /// Restore the window from minimized or maximized state
    fn restore(&self) {}

    /// Toggle fullscreen mode
    fn toggle_fullscreen(&self) {}

    /// Resize the window to the given logical size
    fn resize(&self, size: Size<Pixels>) {
        let _ = size;
    }

    /// Close and destroy the window
    fn close(&self) {}

    /// Set the window's background appearance (backdrop material)
    fn set_background_appearance(&self, appearance: WindowBackgroundAppearance) {
        let _ = appearance;
    }

    /// Apply the cursor selected by this window's presentation.
    ///
    /// This is deliberately window-scoped: a process-global cursor setter
    /// cannot identify which of several presentations owns the hovered region.
    ///
    /// # Errors
    ///
    /// Returns [`CursorError::Unsupported`] when this window backend has no
    /// pointer-cursor facility, or [`CursorError::Backend`] when the native
    /// update fails.
    fn set_cursor(&self, cursor: CursorIcon) -> Result<(), CursorError>;

    // ==================== Callback Registration ====================

    /// All callbacks registered on a window must be invoked on the same
    /// platform/event-loop thread that registered them. `Send` permits backend
    /// storage and wake plumbing; it is not permission to execute a UI callback
    /// on an arbitrary worker thread. Backends must marshal first or reject the
    /// dispatch when they cannot uphold this contract.
    ///
    /// Register a callback for input events (pointer, keyboard)
    ///
    /// The callback receives a `PlatformInput` and returns a
    /// `DispatchEventResult` indicating whether the event was consumed.
    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult + Send>) {
        let _ = callback;
    }

    /// Register a callback for frame rendering requests
    ///
    /// Called by the platform when a new frame should be rendered (e.g., after
    /// `request_redraw()` or when the compositor needs content).
    fn on_request_frame(&self, callback: Box<dyn FnMut() + Send>) {
        let _ = callback;
    }

    /// Register a callback for window resize events
    ///
    /// Called with the new logical size and current scale factor.
    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32) + Send>) {
        let _ = callback;
    }

    /// Register a callback for window move events
    fn on_moved(&self, callback: Box<dyn FnMut() + Send>) {
        let _ = callback;
    }

    /// Register a callback for when the window is destroyed
    ///
    /// This fires once when the window is actually closed/destroyed.
    /// Uses `FnOnce` since it can only fire once.
    fn on_close(&self, callback: Box<dyn FnOnce() + Send>) {
        let _ = callback;
    }

    /// Register a callback to query whether the window should close
    ///
    /// Return `false` to veto the close request (e.g., unsaved changes dialog).
    /// If no callback is registered, close is always allowed.
    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool + Send>) {
        let _ = callback;
    }

    /// Register a callback for focus changes
    ///
    /// Called with `true` when the window gains focus, `false` when it loses
    /// focus.
    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        let _ = callback;
    }

    /// Register a callback for visibility (occlusion) changes.
    ///
    /// Called with `true` when the window becomes visible/unoccluded,
    /// `false` when it becomes fully occluded (or minimized, on backends
    /// that report that through the same signal). Distinct from
    /// [`on_active_status_change`](Self::on_active_status_change): a window
    /// can be visible but unfocused, or occluded while still nominally
    /// focused.
    ///
    /// Delivery is compositor/backend-conditional — on Wayland this rides
    /// the xdg-shell v6 `suspended` state, which not every compositor
    /// sends; where it is never delivered, this callback simply never
    /// fires (the window is treated as always visible, today's behavior).
    fn on_visibility_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        let _ = callback;
    }

    /// Register a callback for mouse hover changes
    ///
    /// Called with `true` when the mouse enters the window, `false` when it
    /// leaves.
    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        let _ = callback;
    }

    /// Register a callback for system appearance changes (light/dark theme)
    fn on_appearance_changed(&self, callback: Box<dyn FnMut() + Send>) {
        let _ = callback;
    }

    // ==================== Window Handles (for GPU integration)
    // ====================

    /// Get a window handle for creating GPU surfaces (wgpu, etc.)
    ///
    /// Concrete platform windows (WindowsWindow, MacOSWindow) implement
    /// `raw_window_handle::HasWindowHandle` and delegate through this method.
    /// Headless windows return `HandleError::Unavailable`.
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        Err(raw_window_handle::HandleError::Unavailable)
    }

    /// Get a display handle for creating GPU surfaces (wgpu, etc.)
    ///
    /// Concrete platform windows (WindowsWindow, MacOSWindow) implement
    /// `raw_window_handle::HasDisplayHandle` and delegate through this method.
    /// Headless windows return `HandleError::Unavailable`.
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        Err(raw_window_handle::HandleError::Unavailable)
    }

    // ==================== Utility ====================

    /// Get the underlying winit window (if available)
    ///
    /// Returns `None` for non-winit platforms (e.g., headless testing).
    #[cfg(feature = "winit-backend")]
    fn as_winit(&self) -> Option<&Arc<Window>> {
        None
    }

    /// Downcast to concrete type
    fn as_any(&self) -> &dyn Any {
        panic!("as_any not implemented")
    }
}

impl HasWindowHandle for dyn PlatformWindow + '_ {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        PlatformWindow::window_handle(self)
    }
}

impl HasDisplayHandle for dyn PlatformWindow + '_ {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        PlatformWindow::display_handle(self)
    }
}

#[cfg(feature = "winit-backend")]
/// Concrete winit window wrapper
///
/// Wraps `winit::window::Window` to implement `PlatformWindow`.
/// Includes per-window callbacks for event delivery using the causal FIFO
/// dispatch pattern for reentrancy safety.
pub struct WinitWindow {
    window: Arc<Window>,
    is_focused: parking_lot::Mutex<bool>,
    is_visible: parking_lot::Mutex<bool>,
    callbacks: crate::shared::WindowCallbacks,
}

/// [`PlatformTextInput`] for a winit window.
///
/// A thin wrapper around `Arc<winit::window::Window>` rather than an impl
/// directly on `WinitWindow`: `PlatformWindow::text_input` hands back an
/// `Arc<dyn PlatformTextInput>` from `&self`. Cloning the exact inner
/// `Arc<Window>` gives the capability independent ownership without cloning
/// or forwarding the platform-window object itself.
#[cfg(feature = "winit-backend")]
pub struct WinitTextInput {
    window: Arc<Window>,
}

#[cfg(feature = "winit-backend")]
impl super::text_input::PlatformTextInput for WinitTextInput {
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

#[cfg(feature = "winit-backend")]
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

#[cfg(feature = "winit-backend")]
impl WinitWindow {
    /// Create a new WinitWindow wrapper
    pub fn new(window: Arc<Window>) -> Self {
        Self {
            window,
            is_focused: parking_lot::Mutex::new(true),
            is_visible: parking_lot::Mutex::new(true),
            callbacks: crate::shared::WindowCallbacks::new(),
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

#[cfg(feature = "winit-backend")]
impl PlatformWindow for WinitWindow {
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

    fn scale_factor(&self) -> f64 {
        self.window.scale_factor()
    }

    fn request_redraw(&self) {
        self.window.request_redraw();
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
        self.callbacks.dispatch_close();
        self.window.set_visible(false);
    }

    fn set_cursor(&self, cursor: CursorIcon) -> Result<(), CursorError> {
        self.window.set_cursor(cursor);
        Ok(())
    }

    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult + Send>) {
        *self.callbacks.on_input.lock() = Some(callback);
    }

    fn on_request_frame(&self, callback: Box<dyn FnMut() + Send>) {
        *self.callbacks.on_request_frame.lock() = Some(callback);
    }

    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32) + Send>) {
        *self.callbacks.on_resize.lock() = Some(callback);
    }

    fn on_moved(&self, callback: Box<dyn FnMut() + Send>) {
        *self.callbacks.on_moved.lock() = Some(callback);
    }

    fn on_close(&self, callback: Box<dyn FnOnce() + Send>) {
        *self.callbacks.on_close.lock() = Some(callback);
    }

    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool + Send>) {
        *self.callbacks.on_should_close.lock() = Some(callback);
    }

    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        *self.callbacks.on_active_status_change.lock() = Some(callback);
    }

    fn on_visibility_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        *self.callbacks.on_visibility_status_change.lock() = Some(callback);
    }

    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        *self.callbacks.on_hover_status_change.lock() = Some(callback);
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut() + Send>) {
        *self.callbacks.on_appearance_changed.lock() = Some(callback);
    }

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

    // No `haptics()` override: desktop winit targets have no haptic
    // hardware to drive, so the `PlatformWindow` trait default (`None`) is
    // the permanent correct answer here, not a stub awaiting a backend.
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock window for testing
    struct MockWindow {
        size: Size<Pixels>,
        scale_factor: f64,
        focused: bool,
        visible: bool,
    }

    impl PlatformWindow for MockWindow {
        fn physical_size(&self) -> Size<DevicePixels> {
            use flui_types::geometry::device_px;

            Size::new(
                device_px((self.size.width.0 * self.scale_factor as f32) as i32),
                device_px((self.size.height.0 * self.scale_factor as f32) as i32),
            )
        }

        fn logical_size(&self) -> Size<Pixels> {
            self.size
        }

        fn scale_factor(&self) -> f64 {
            self.scale_factor
        }

        fn request_redraw(&self) {
            // No-op for mock
        }

        fn is_focused(&self) -> bool {
            self.focused
        }

        fn is_visible(&self) -> bool {
            self.visible
        }

        fn set_cursor(&self, _cursor: CursorIcon) -> Result<(), CursorError> {
            Ok(())
        }
    }

    #[test]
    fn test_mock_window() {
        use flui_types::geometry::{device_px, px};

        let window = MockWindow {
            size: Size::new(px(800.0), px(600.0)),
            scale_factor: 2.0,
            focused: true,
            visible: true,
        };

        assert_eq!(
            window.physical_size(),
            Size::new(device_px(1600), device_px(1200))
        );
        assert_eq!(window.logical_size(), Size::new(px(800.0), px(600.0)));
        assert_eq!(window.scale_factor(), 2.0);
        assert!(window.is_focused());
        assert!(window.is_visible());
    }
}
