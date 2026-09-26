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

use crate::{
    shared::WindowCallbacks,
    traits::{CursorError, PlatformWindow, WindowId},
};

/// Android window wrapping the native ANativeWindow via `AndroidApp`
///
/// On Android there is only one window (the Activity surface). This struct
/// provides the `PlatformWindow` interface over that surface.
///
/// # Raw Window Handle
///
/// `AndroidApp` implements `HasWindowHandle` and `HasDisplayHandle`, so this
/// window can be used directly with wgpu for Vulkan surface creation.
#[derive(Debug, Clone)]
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

impl crate::traits::HostWindow for AndroidWindow {}

impl PlatformWindow for AndroidWindow {
    // Android hosts exactly one `AndroidApp` surface for the process's
    // lifetime (see the struct docs above) — there is no second native
    // window this identity could ever collide with, so a fixed constant is
    // this backend's honest identity, not a stand-in for missing
    // information.
    fn id(&self) -> WindowId {
        WindowId(1)
    }

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

    crate::shared::impl_window_callback_setters!(callbacks);

    // ==================== Window Handles (GPU integration) ====================

    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        // Get the ANativeWindow pointer from the AndroidApp and construct the handle
        // manually. We can't delegate to NativeWindow::window_handle() because
        // that borrows a temporary.
        //
        // This already satisfies `PlatformWindow::window_handle`'s MUST
        // (see that trait method's doc): `native_window()` answers `None`
        // exactly when no ANativeWindow is live, and the `ok_or` below maps
        // that straight to `Unavailable` — no separate flag needed. The span
        // where it is `None` runs from `MainEvent::TerminateWindow` to the
        // next `MainEvent::InitWindow` (`platforms/android/mod.rs`'s event
        // loop, module doc's "Surface Lifecycle" section), and that span is
        // open at both ends *inside* those callbacks: the field is still
        // `Some` while the `TerminateWindow` callback runs (the applier clears
        // it in `post_exec_cmd` once the callback has returned) and already
        // `Some` while the `InitWindow` callback runs (it is set in
        // `pre_exec_cmd` before the callback). Note what that span is
        // NOT: an ordinary `MainEvent::Pause` leaves `native_window()`
        // `Some`, because `AppCmd::TermWindow` is what clears it and a pause
        // does not apply that command.
        let native_window = self
            .app
            .native_window()
            .ok_or(raw_window_handle::HandleError::Unavailable)?;
        // NativeWindow::ptr() returns NonNull<ANativeWindow>; cast to NonNull<c_void>
        // for rwh
        let ptr = native_window.ptr().cast();
        let handle = raw_window_handle::AndroidNdkWindowHandle::new(ptr);
        let raw = raw_window_handle::RawWindowHandle::AndroidNdk(handle);
        // SAFETY: `ptr` is the ANativeWindow `AndroidApp` currently holds,
        // re-queried immediately above. The bound on that pointer is the
        // ANativeWindow refcount, and it is NOT this borrow. `ndk::NativeWindow`
        // is a refcounted wrapper (`Clone` calls `ANativeWindow_acquire`,
        // `Drop` calls `ANativeWindow_release`); `AndroidApp::native_window()`
        // returns a clone of the glue guard's field
        // (`android-activity` 0.6.1, `native_activity/mod.rs`); and the guard
        // holds the last strong clone the Rust side keeps, dropping it in
        // `post_exec_cmd(AppCmd::TermWindow)` — `guard.window = None`,
        // `native_activity/glue.rs` — which the main loop applies *after* the
        // `MainEvent::TerminateWindow` callback returns (`pre_exec_cmd` →
        // callback → `post_exec_cmd`). The `'_` on the returned handle is the
        // borrow of `self` and does not encode that bound: `AndroidPlatform`
        // keeps holding its `AndroidWindow` across a pause, and the only
        // writes to that field (`platforms/android/mod.rs`) are `open_window`,
        // which fills it, and `AndroidPlatform::run`'s exit path, which takes
        // it exactly once, after the last dispatch, so for every handle this
        // method ever returns, `self` and the borrow outlive the pointer.
        //
        // The obligation that carries the weight is therefore
        // consumer-enforced, not type-enforced: nothing may dereference the
        // handle once the window has been terminated, and this type cannot
        // express that, because `native_window()` keeps answering `Some`
        // through the very callback that precedes the release. `flui-app`'s
        // Android runner discharges it by releasing the wgpu surface built
        // from this handle inside `on_surface_status_change(false)`, which
        // `MainEvent::Pause` and `MainEvent::TerminateWindow` both deliver —
        // the second one still inside the callback, before the release above.
        #[expect(unsafe_code)]
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(raw) })
    }

    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        // Android always uses the default Android display
        let handle = raw_window_handle::AndroidDisplayHandle::new();
        let raw = raw_window_handle::RawDisplayHandle::Android(handle);
        // SAFETY: The Android display handle is always valid while the app is running
        #[expect(unsafe_code)]
        Ok(unsafe { raw_window_handle::DisplayHandle::borrow_raw(raw) })
    }

    // ==================== Additional query methods ====================

    fn get_title(&self) -> String {
        "FLUI Android".to_string()
    }

    fn mouse_position(&self) -> Point<Pixels> {
        Point::default()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
