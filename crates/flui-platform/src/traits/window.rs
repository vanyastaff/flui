//! Platform window trait
//!
//! Provides a thin abstraction over platform windows for testability
//! and flexibility. Includes per-window callback registration for event delivery.

use super::input::{DispatchEventResult, Modifiers, PlatformInput};
use flui_types::geometry::{Bounds, DevicePixels, Pixels, Point, Size};
use std::any::Any;
use std::sync::Arc;

use super::display::PlatformDisplay;

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
/// This allows registering callbacks on shared references (`Arc<dyn PlatformWindow>`).
/// Callbacks are invoked by the platform's event loop when native events arrive.
///
/// The take/restore dispatch pattern ensures reentrancy safety:
/// the callback storage lock is released before the callback is invoked.
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

    // ==================== Callback Registration ====================

    /// Register a callback for input events (pointer, keyboard)
    ///
    /// The callback receives a `PlatformInput` and returns a `DispatchEventResult`
    /// indicating whether the event was consumed.
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
    /// Called with `true` when the window gains focus, `false` when it loses focus.
    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        let _ = callback;
    }

    /// Register a callback for mouse hover changes
    ///
    /// Called with `true` when the mouse enters the window, `false` when it leaves.
    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        let _ = callback;
    }

    /// Register a callback for system appearance changes (light/dark theme)
    fn on_appearance_changed(&self, callback: Box<dyn FnMut() + Send>) {
        let _ = callback;
    }

    // ==================== Window Handles (for GPU integration) ====================

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

#[cfg(feature = "winit-backend")]
/// Concrete winit window wrapper
///
/// Wraps `winit::window::Window` to implement `PlatformWindow`.
pub struct WinitWindow {
    window: Arc<Window>,
    is_focused: bool,
    is_visible: bool,
}

#[cfg(feature = "winit-backend")]
impl WinitWindow {
    /// Create a new WinitWindow wrapper
    pub fn new(window: Arc<Window>) -> Self {
        Self {
            window,
            is_focused: true,
            is_visible: true,
        }
    }

    /// Get the underlying Arc<Window>
    pub fn inner(&self) -> &Arc<Window> {
        &self.window
    }

    /// Update focus state
    pub fn set_focused(&mut self, focused: bool) {
        self.is_focused = focused;
    }

    /// Update visibility state
    pub fn set_visible(&mut self, visible: bool) {
        self.is_visible = visible;
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
        self.is_focused
    }

    fn is_visible(&self) -> bool {
        self.is_visible
    }

    fn as_winit(&self) -> Option<&Arc<Window>> {
        Some(&self.window)
    }
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
