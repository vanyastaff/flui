//! Android window implementation
//!
//! Wraps `AndroidApp` to provide the `PlatformWindow` trait, delegating
//! to the native ANativeWindow for size queries and raw-window-handle for GPU
//! surface creation.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use android_activity::AndroidApp;
use cursor_icon::CursorIcon;
use flui_types::geometry::{DevicePixels, Pixels, Point, Size, device_px, px};

use crate::{shared::WindowCallbacks, traits::*};

/// Android window wrapping the native ANativeWindow via `AndroidApp`
///
/// On Android there is only one window (the Activity surface). This struct
/// provides the `PlatformWindow` interface over that surface.
///
/// # Raw Window Handle
///
/// `AndroidApp` implements `HasWindowHandle` and `HasDisplayHandle`, so this
/// window can be used directly with wgpu for Vulkan surface creation.
#[derive(Clone)]
pub struct AndroidWindow {
    app: AndroidApp,
    callbacks: Arc<WindowCallbacks>,
    redraw_requested: Arc<AtomicBool>,
}

impl AndroidWindow {
    /// Create a new Android window wrapping the given `AndroidApp`
    pub fn new(app: AndroidApp) -> Self {
        Self {
            app,
            callbacks: Arc::new(WindowCallbacks::new()),
            redraw_requested: Arc::new(AtomicBool::new(true)),
        }
    }

    /// Access the callback storage (used by `AndroidPlatform` to dispatch
    /// events)
    pub fn callbacks(&self) -> &WindowCallbacks {
        &self.callbacks
    }

    /// Check and clear the redraw request flag
    pub fn take_redraw_request(&self) -> bool {
        self.redraw_requested.swap(false, Ordering::SeqCst)
    }

    /// Get native window dimensions, returning (0, 0) if window is not
    /// available
    fn native_size(&self) -> (i32, i32) {
        if let Some(native_window) = self.app.native_window() {
            (native_window.width(), native_window.height())
        } else {
            (0, 0)
        }
    }
}

impl PlatformWindow for AndroidWindow {
    fn physical_size(&self) -> Size<DevicePixels> {
        let (w, h) = self.native_size();
        Size::new(device_px(w), device_px(h))
    }

    fn logical_size(&self) -> Size<Pixels> {
        let (w, h) = self.native_size();
        let scale = self.scale_factor() as f32;
        if scale > 0.0 {
            Size::new(px(w as f32 / scale), px(h as f32 / scale))
        } else {
            Size::new(px(w as f32), px(h as f32))
        }
    }

    fn scale_factor(&self) -> f64 {
        // android-activity config returns density as DPI / 160
        // Default to 2.0 if config is unavailable
        let config = self.app.config();
        let density = config.density().unwrap_or(320);
        density as f64 / 160.0
    }

    fn request_redraw(&self) {
        self.redraw_requested.store(true, Ordering::SeqCst);
    }

    fn is_focused(&self) -> bool {
        // On Android, the Activity surface is always focused when resumed
        true
    }

    fn is_visible(&self) -> bool {
        self.app.native_window().is_some()
    }

    fn set_cursor(&self, _cursor: CursorIcon) -> Result<(), CursorError> {
        Err(CursorError::Unsupported)
    }

    // ==================== Callback Registration ====================

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

    // ==================== Window Handles (GPU integration) ====================

    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        // Get the ANativeWindow pointer from the AndroidApp and construct the handle
        // manually. We can't delegate to NativeWindow::window_handle() because
        // that borrows a temporary.
        let native_window = self
            .app
            .native_window()
            .ok_or(raw_window_handle::HandleError::Unavailable)?;
        // NativeWindow::ptr() returns NonNull<ANativeWindow>; cast to NonNull<c_void>
        // for rwh
        let ptr = native_window.ptr().cast();
        let handle = raw_window_handle::AndroidNdkWindowHandle::new(ptr);
        let raw = raw_window_handle::RawWindowHandle::AndroidNdk(handle);
        // SAFETY: The ANativeWindow pointer is valid as long as we are between Resume
        // and Pause. AndroidWindow is only used within that lifecycle window.
        #[allow(unsafe_code)]
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(raw) })
    }

    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        // Android always uses the default Android display
        let handle = raw_window_handle::AndroidDisplayHandle::new();
        let raw = raw_window_handle::RawDisplayHandle::Android(handle);
        // SAFETY: The Android display handle is always valid while the app is running
        #[allow(unsafe_code)]
        Ok(unsafe { raw_window_handle::DisplayHandle::borrow_raw(raw) })
    }

    // ==================== Additional query methods ====================

    fn get_title(&self) -> String {
        "FLUI Android".to_string()
    }

    fn mouse_position(&self) -> Point<Pixels> {
        Point::default()
    }
}
