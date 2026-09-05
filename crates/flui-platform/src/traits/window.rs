//! Platform window trait
//!
//! Provides a thin abstraction over platform windows for testability
//! and flexibility. Includes per-window callback registration for event
//! delivery.

use std::{any::Any, sync::Arc};

use cursor_icon::CursorIcon;
use flui_types::geometry::{Bounds, DevicePixels, Pixels, Point, Size};

use super::{
    accessibility::PlatformAccessibility,
    display::PlatformDisplay,
    haptics::PlatformHaptics,
    input::{DispatchEventResult, Modifiers, PlatformInput},
    platform::WindowId,
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
    /// This window's platform-internal identity.
    ///
    /// A window that cannot state its identity cannot be demultiplexed
    /// (ADR-0037 §2): the identity is what lets the demux boundary look up
    /// which `(RealmId, PresentationId)` a native event belongs to. Every
    /// implementor must return a real, stable-for-the-window's-lifetime
    /// value — never a shared sentinel that would make two different
    /// windows compare equal.
    fn id(&self) -> WindowId;

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

    /// Get this window's accessibility capability, if the backend exposes one.
    ///
    /// `None` for a backend with no accessibility integration — which is every
    /// backend until its per-OS adapter is wired, and permanently for one with
    /// no such platform API. A composition root that gets `None` simply never
    /// enables semantics assembly, so the cost is not paid either.
    fn accessibility(&self) -> Option<Arc<dyn PlatformAccessibility>> {
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

    /// Close and destroy the window — a decision already made, not a
    /// request.
    ///
    /// Bypasses the should-close veto ([`on_should_close`](Self::on_should_close))
    /// when called on the backend's owning thread — natively so: AppKit's
    /// `-close` never sends `windowShouldClose:`, and a same-thread Win32
    /// `DestroyWindow` never sends `WM_CLOSE`. Off the owning thread the
    /// veto's fate is backend-defined: Win32 posts `WM_CLOSE`, whose owner-side
    /// handler re-asks `on_should_close` before destroying, so a cross-thread
    /// `close()` there is a close *request* (see that impl); winit defers the
    /// whole teardown to the owner thread's next turn with no veto asked.
    ///
    /// Never bypasses the *bookkeeping* once the close proceeds: the backend
    /// runs the same teardown a user-initiated close takes — the
    /// [`on_close`](Self::on_close) callback, removal from the backend's window
    /// tracking, cleanup of per-window input state, and the exit-policy consult
    /// that ends the loop when this was the last window.
    ///
    /// Callable from any thread the native windowing API permits. On winit the
    /// teardown, and so `on_close`, runs on the owner thread's next turn —
    /// never synchronously within this call, whichever thread makes it. The
    /// headless test double, by contrast, runs `on_close` synchronously
    /// inside `close()`: a test that asserts state right after `close()`
    /// pins that double, not this contract. AppKit's `close()` has no thread
    /// marshaling today: call it from the main thread only.
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
    /// Delivery is backend-conditional, verified against winit 0.30's own
    /// `WindowEvent::Occluded` documentation and `platform_impl` source
    /// (not assumed): the emitting backends are **X11** (via Xlib's
    /// `VisibilityFullyObscured` — fires only on FULL obscuration, never
    /// partial), **macOS**, **iOS**, and **Web**. Winit's own doc states
    /// plainly: "Android / Wayland / Windows / Orbital: Unsupported." —
    /// there is no Wayland emitter anywhere in `platform_impl` at all, not
    /// merely a compositor-dependent one. On a Wayland compositor (this
    /// workspace's own primary desktop reference session), this callback
    /// never fires and the window is always treated as visible; on X11 it
    /// only fires when a window is fully covered, which a compositing
    /// window manager may rarely or never produce.
    ///
    /// The native backends derive their own signal (rules in
    /// `shared::visibility`, host-tested): **Win32** from
    /// `WM_SIZE`-minimize/restore plus `WM_SHOWWINDOW` hide/show (Windows
    /// has no occlusion events at all), **AppKit** from
    /// `windowDidChangeOcclusionState:`'s visible bit (fires on full
    /// occlusion, miniaturization, hide, and space switches). The headless
    /// backend's `simulate_visibility` drives the same wire for tests.
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

    /// Downcast to concrete type.
    ///
    /// No default body: a panicking default here would only be discovered
    /// the first time some caller downcasts a backend that forgot to
    /// override it — every implementor must supply its own (invariably
    /// `{ self }`), the same shape [`super::PlatformHaptics::as_any`]
    /// already requires.
    fn as_any(&self) -> &dyn Any;
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
        fn id(&self) -> WindowId {
            WindowId(1)
        }

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

        fn as_any(&self) -> &dyn Any {
            self
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
