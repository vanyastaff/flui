//! Android window implementation
//!
//! Wraps `AndroidApp` to provide the `PlatformWindow` trait, delegating
//! to the native ANativeWindow for size queries and raw-window-handle for GPU
//! surface creation.

use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};

use android_activity::AndroidApp;
use cursor_icon::CursorIcon;
use flui_foundation::geometry::{Point, Size};

use crate::{
    shared::WindowCallbacks,
    traits::{CursorError, PlatformWindow, WindowExecutionState, WindowId},
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
#[derive(Clone)]
pub struct AndroidWindow {
    app: AndroidApp,
    callbacks: Arc<WindowCallbacks>,
    redraw_requested: Arc<AtomicBool>,
    execution_resumed: Arc<AtomicBool>,
    owner: std::thread::ThreadId,
    owner_signal: Weak<crate::shared::owner_signal::OwnerSignal>,
    geometry_live: Arc<AtomicBool>,
    text_sizing: Arc<super::text_sizing::Acquisition>,
}

impl std::fmt::Debug for AndroidWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidWindow")
            .field("owner", &self.owner)
            .finish_non_exhaustive()
    }
}

impl AndroidWindow {
    /// Create a new Android window wrapping the given `AndroidApp`
    pub(crate) fn new(
        app: AndroidApp,
        execution_resumed: Arc<AtomicBool>,
        owner_signal: Weak<crate::shared::owner_signal::OwnerSignal>,
    ) -> Self {
        Self {
            app,
            callbacks: Arc::new(WindowCallbacks::new()),
            redraw_requested: Arc::new(AtomicBool::new(true)),
            execution_resumed,
            owner: std::thread::current().id(),
            owner_signal,
            geometry_live: Arc::new(AtomicBool::new(true)),
            text_sizing: super::text_sizing::Acquisition::new(),
        }
    }

    /// Access the callback storage (used by `AndroidPlatform` to dispatch
    /// events)
    pub fn callbacks(&self) -> &WindowCallbacks {
        &self.callbacks
    }

    /// Replacing the Activity presentation retires this projection capability,
    /// even if an old handle still keeps the shared AndroidApp alive.
    pub(crate) fn revoke_geometry(&self) {
        self.geometry_live.store(false, Ordering::Release);
        self.text_sizing.invalidate();
    }

    pub(crate) fn invalidate_text_sizing(&self) {
        self.text_sizing.invalidate();
        if let Some(signal) = self.owner_signal.upgrade() {
            let _ = signal.wake();
        }
    }

    pub(crate) fn fence_text_sizing(&self) {
        self.text_sizing.fence();
    }

    /// Read whether a redraw is pending without consuming it. The event loop
    /// uses this only to choose its poll timeout; native lifecycle events may
    /// still suspend execution before delivery becomes possible.
    pub(crate) fn has_redraw_request(&self) -> bool {
        self.redraw_requested.load(Ordering::SeqCst)
    }

    /// Consume a redraw only when the same loop turn can deliver its frame.
    pub(crate) fn take_deliverable_redraw_request(&self) -> bool {
        take_deliverable_redraw_request(
            &self.redraw_requested,
            self.execution_resumed.load(Ordering::SeqCst),
        )
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

fn take_deliverable_redraw_request(redraw_requested: &AtomicBool, execution_running: bool) -> bool {
    execution_running && redraw_requested.swap(false, Ordering::SeqCst)
}

impl crate::traits::HostWindow for AndroidWindow {
    fn capture_text_sizing(
        &self,
        _owner: crate::traits::OwnerThreadToken,
    ) -> Result<crate::TextSizingCaptureState, crate::TextSizingCaptureError> {
        if self.owner != std::thread::current().id() {
            return Err(crate::TextSizingCaptureError::WrongThread);
        }
        if !self.geometry_live.load(Ordering::Acquire)
            || self.app.native_window().is_none()
            || !self
                .owner_signal
                .upgrade()
                .is_some_and(|signal| signal.accepting())
        {
            return Err(crate::TextSizingCaptureError::Unavailable);
        }
        self.text_sizing
            .poll(&self.app, &self.owner_signal, self.owner)
    }
}

impl PlatformWindow for AndroidWindow {
    // Android hosts exactly one `AndroidApp` surface for the process's
    // lifetime (see the struct docs above) — there is no second native
    // window this identity could ever collide with, so a fixed constant is
    // this backend's honest identity, not a stand-in for missing
    // information.
    fn id(&self) -> WindowId {
        WindowId(1)
    }

    fn physical_size(&self) -> Size<i32> {
        let (w, h) = self.native_size();
        Size::new(w, h)
    }

    fn logical_size(&self) -> Size<f64> {
        let (w, h) = self.native_size();
        let scale = self.scale_factor();
        if scale > 0.0 {
            Size::new(w as f64 / scale, h as f64 / scale)
        } else {
            Size::new(w as f64, h as f64)
        }
    }

    fn scale_factor(&self) -> f64 {
        if let Ok(ratio) = super::preferences::pixel_ratio(&self.app) {
            return ratio.get();
        }
        // android-activity config returns density as DPI / 160
        // Default to 2.0 if config is unavailable
        let config = self.app.config();
        let density = config.density().unwrap_or(320);
        density as f64 / 160.0
    }

    fn gesture_geometry(
        &self,
    ) -> Result<Option<crate::GestureGeometry>, crate::PreferenceQueryError> {
        if self.owner != std::thread::current().id() {
            return Err(crate::PreferenceQueryError::WrongThread);
        }
        if !self.geometry_live.load(Ordering::Acquire)
            || self.app.native_window().is_none()
            || !self
                .owner_signal
                .upgrade()
                .is_some_and(|signal| signal.accepting())
        {
            return Err(crate::PreferenceQueryError::Unavailable);
        }
        super::preferences::geometry(&self.app).map(Some)
    }

    fn execution_state(&self) -> WindowExecutionState {
        if self.execution_resumed.load(Ordering::SeqCst) {
            WindowExecutionState::Running
        } else {
            WindowExecutionState::Suspended
        }
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

    fn mouse_position(&self) -> Point<f64> {
        Point::default()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
