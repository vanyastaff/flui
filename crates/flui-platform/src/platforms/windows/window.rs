//! Windows window implementation

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use raw_window_handle::{Win32WindowHandle, WindowsDisplayHandle};
use windows::core::HSTRING;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::util::{logical_to_device, USER_DEFAULT_SCREEN_DPI, WINDOW_CLASS_NAME};
use crate::shared::{PlatformHandlers, WindowCallbacks};
use crate::traits::*;
use flui_types::geometry::{device_px, px, Bounds, DevicePixels, Pixels, Point, Size};

/// Windows window wrapper
pub struct WindowsWindow {
    /// Native window handle
    hwnd: HWND,

    /// Window state
    state: Arc<Mutex<WindowState>>,

    /// Per-window callbacks for event delivery
    callbacks: Arc<WindowCallbacks>,

    /// Reference to platform's window map (for cleanup)
    windows_map: Arc<Mutex<HashMap<isize, Arc<WindowsWindow>>>>,
}

// SAFETY: HWND is just an integer handle and is safe to send/share between threads.
// Windows API handles are thread-safe by design.
unsafe impl Send for WindowsWindow {}
unsafe impl Sync for WindowsWindow {}

/// Mutable window state
struct WindowState {
    /// Current window bounds (logical pixels)
    bounds: Bounds<Pixels>,

    /// Current scale factor (DPI / 96)
    scale_factor: f32,

    /// Is window visible?
    visible: bool,

    /// Is window focused?
    focused: bool,

    /// Window title
    title: String,
}

impl WindowsWindow {
    /// Create a new Windows window
    pub fn new(
        options: WindowOptions,
        windows_map: Arc<Mutex<HashMap<isize, Arc<WindowsWindow>>>>,
        handlers: Arc<Mutex<PlatformHandlers>>,
        config: crate::config::WindowConfiguration,
    ) -> Result<Arc<Self>> {
        unsafe {
            let hinstance = GetModuleHandleW(None).context("Failed to get module handle")?;

            // Get DPI for initial size calculation
            let dpi = GetDpiForSystem();
            let scale_factor = dpi as f32 / USER_DEFAULT_SCREEN_DPI as f32;

            // Convert logical size to device pixels
            let width = logical_to_device(options.size.width.0, scale_factor);
            let height = logical_to_device(options.size.height.0, scale_factor);

            // Default position (center on screen)
            let x = CW_USEDEFAULT;
            let y = CW_USEDEFAULT;

            // Determine window style
            let style = if options.decorated {
                WS_OVERLAPPEDWINDOW
            } else {
                WS_POPUP | WS_VISIBLE
            };

            let ex_style = WS_EX_APPWINDOW;

            // Create the window
            let title = HSTRING::from(&options.title);
            let hwnd = CreateWindowExW(
                ex_style,
                WINDOW_CLASS_NAME,
                &title,
                style,
                x,
                y,
                width,
                height,
                None, // parent
                None, // menu
                Some(hinstance.into()),
                None, // lpParam
            )
            .context("Failed to create window")?;

            if hwnd.is_invalid() {
                return Err(windows::core::Error::from_win32().into());
            }

            // Remove background brush to allow Mica backdrop
            SetClassLongPtrW(hwnd, GCLP_HBRBACKGROUND, 0);

            // Apply Windows 11 features automatically
            Self::apply_windows_features(hwnd);

            tracing::info!(
                "Created window HWND {:?} - {}x{} at ({}, {}) - scale: {}",
                hwnd,
                width,
                height,
                x,
                y,
                scale_factor
            );

            // Create window state with default bounds (actual bounds will be set after creation)
            let callbacks = Arc::new(WindowCallbacks::new());

            let state = Arc::new(Mutex::new(WindowState {
                bounds: Bounds {
                    origin: Point::new(px(0.0), px(0.0)),
                    size: options.size,
                },
                scale_factor,
                visible: false,
                focused: false,
                title: options.title.clone(),
            }));

            let window = Arc::new(Self {
                hwnd,
                state,
                callbacks: Arc::clone(&callbacks),
                windows_map,
            });

            // Create and store WindowContext for event dispatch
            use super::platform::WindowContext;
            use flui_types::geometry::{DevicePixels, Size};

            let window_id = WindowId(hwnd.0 as u64);
            let device_width = logical_to_device(width as f32, scale_factor);
            let device_height = logical_to_device(height as f32, scale_factor);
            let initial_size = Size::new(DevicePixels(device_width), DevicePixels(device_height));
            let context = Box::new(WindowContext {
                window_id,
                handlers: handlers.clone(),
                callbacks,
                scale_factor,
                mode: std::cell::Cell::new(WindowMode::Normal),
                last_size: std::cell::Cell::new(initial_size),
                config,
            });
            let context_ptr = Box::into_raw(context);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, context_ptr as isize);

            // Show window if requested
            if options.visible {
                let _ = ShowWindow(hwnd, SW_SHOW);
                let _ = UpdateWindow(hwnd);
                window.state.lock().visible = true;
            }

            Ok(window)
        }
    }

    /// Apply Windows 11 features automatically
    ///
    /// This applies modern Windows 11 visual features if running on Windows 11:
    /// - Mica backdrop for translucent background with blur
    /// - Dark mode title bar matching system theme
    /// - Rounded window corners
    /// - DWM frame extension for proper backdrop rendering
    fn apply_windows_features(hwnd: HWND) {
        unsafe {
            use windows::Win32::Graphics::Dwm::{
                DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWINDOWATTRIBUTE,
            };
            use windows::Win32::UI::Controls::MARGINS;

            tracing::debug!("Applying Windows 11 features to HWND {:?}", hwnd);

            // 1. Extend frame into client area (required for Mica backdrop)
            let margins = MARGINS {
                cxLeftWidth: -1,
                cxRightWidth: -1,
                cyTopHeight: -1,
                cyBottomHeight: -1,
            };
            let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);

            // 2. Enable Mica backdrop (Windows 11+)
            let mica_value: i32 = 2; // DWMSBT_MAINWINDOW
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWINDOWATTRIBUTE(38), // DWMWA_SYSTEMBACKDROP_TYPE
                &mica_value as *const i32 as *const std::ffi::c_void,
                std::mem::size_of::<i32>() as u32,
            );

            // 3. Enable dark mode title bar
            let dark_mode_value: i32 = 1;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWINDOWATTRIBUTE(20), // DWMWA_USE_IMMERSIVE_DARK_MODE
                &dark_mode_value as *const i32 as *const std::ffi::c_void,
                std::mem::size_of::<i32>() as u32,
            );

            // 4. Set rounded corners
            let corner_value: i32 = 2; // DWMWCP_ROUND
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWINDOWATTRIBUTE(33), // DWMWA_WINDOW_CORNER_PREFERENCE
                &corner_value as *const i32 as *const std::ffi::c_void,
                std::mem::size_of::<i32>() as u32,
            );

            tracing::debug!("Windows 11 features applied");
        }
    }

    /// Get the native HWND handle
    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    /// Get current window bounds
    pub fn bounds(&self) -> Bounds<Pixels> {
        let state = self.state.lock();
        Bounds {
            origin: state.bounds.origin,
            size: state.bounds.size,
        }
    }

    /// Get current scale factor
    pub fn scale_factor(&self) -> f32 {
        self.state.lock().scale_factor
    }

    /// Toggle fullscreen mode for a window by HWND (static method for use from window_proc)
    ///
    /// This method implements borderless fullscreen by:
    /// 1. **Entering fullscreen**: Saves current window style and bounds, removes window borders
    ///    (WS_POPUP), and resizes to cover the entire monitor
    /// 2. **Exiting fullscreen**: Restores saved window style and bounds
    ///
    /// # Implementation Details
    /// - Uses borderless fullscreen (WS_POPUP) rather than exclusive fullscreen for better compatibility
    /// - Automatically detects the monitor containing the window and fills it completely
    /// - Preserves window state (position, size, style) for proper restoration
    /// - Dispatches `WindowEvent::Fullscreen` and `WindowEvent::ExitFullscreen` events
    ///
    /// # Thread Safety
    /// This method is unsafe because it accesses raw window context via GWLP_USERDATA.
    /// It should only be called from the window's message loop thread or with proper synchronization.
    ///
    /// # Example
    /// ```ignore
    /// // Toggle fullscreen on F11 key press (from WM_KEYDOWN handler)
    /// WindowsWindow::toggle_fullscreen_for_hwnd(hwnd);
    /// ```
    pub fn toggle_fullscreen_for_hwnd(hwnd: HWND) {
        use windows::Win32::Graphics::Gdi::*;
        use windows::Win32::UI::WindowsAndMessaging::*;

        unsafe {
            // Get WindowContext from GWLP_USERDATA
            let ctx_ptr =
                GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut super::platform::WindowContext;
            if ctx_ptr.is_null() {
                tracing::warn!("Cannot toggle fullscreen: no WindowContext");
                return;
            }
            let ctx = &*ctx_ptr;

            let current_mode = ctx.mode.get();

            match current_mode {
                WindowMode::Fullscreen {
                    restore_style,
                    restore_bounds,
                } => {
                    // Exit fullscreen - restore previous style and bounds
                    tracing::info!("🪟 Exiting fullscreen mode");

                    // Validate transition
                    let candidate = WindowMode::Normal;
                    if !current_mode.can_transition_to(&candidate) {
                        tracing::warn!("⚠️  Cannot exit fullscreen: invalid state transition");
                        return;
                    }

                    // Restore window style
                    SetWindowLongPtrW(hwnd, GWL_STYLE, restore_style as isize);

                    // Restore window position and size
                    SetWindowPos(
                        hwnd,
                        None,
                        restore_bounds.origin.x.0,
                        restore_bounds.origin.y.0,
                        restore_bounds.size.width.0,
                        restore_bounds.size.height.0,
                        SWP_FRAMECHANGED | SWP_NOZORDER | SWP_NOACTIVATE,
                    )
                    .ok();

                    // Update state
                    ctx.mode.set(WindowMode::Normal);

                    // Dispatch ExitFullscreen event
                    ctx.dispatch_event(crate::traits::WindowEvent::ExitFullscreen {
                        window_id: ctx.window_id,
                        size: restore_bounds.size,
                    });
                }
                _ => {
                    // Enter fullscreen - save current state and go borderless on monitor
                    tracing::info!("🖥️  Entering fullscreen mode");

                    // Get current window rect
                    let mut rect = RECT::default();
                    GetWindowRect(hwnd, &mut rect).ok();

                    // Save current style
                    let current_style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;

                    // Save current bounds
                    let restore_bounds = Bounds {
                        origin: Point::new(DevicePixels(rect.left), DevicePixels(rect.top)),
                        size: Size::new(
                            DevicePixels(rect.right - rect.left),
                            DevicePixels(rect.bottom - rect.top),
                        ),
                    };

                    // Validate transition
                    let candidate = WindowMode::Fullscreen {
                        restore_style: current_style,
                        restore_bounds,
                    };
                    if !current_mode.can_transition_to(&candidate) {
                        tracing::warn!(
                            "⚠️  Cannot enter fullscreen: invalid state transition from {:?}",
                            current_mode
                        );
                        return;
                    }

                    // Get monitor containing this window
                    let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY);
                    let mut monitor_info = MONITORINFO {
                        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                        ..Default::default()
                    };
                    let _ = GetMonitorInfoW(monitor, &mut monitor_info);

                    let monitor_rect = monitor_info.rcMonitor;

                    // Set borderless style
                    let fullscreen_style = WS_POPUP | WS_VISIBLE;
                    SetWindowLongPtrW(hwnd, GWL_STYLE, fullscreen_style.0 as isize);

                    // Position window to cover entire monitor
                    SetWindowPos(
                        hwnd,
                        Some(HWND_TOP),
                        monitor_rect.left,
                        monitor_rect.top,
                        monitor_rect.right - monitor_rect.left,
                        monitor_rect.bottom - monitor_rect.top,
                        SWP_FRAMECHANGED | SWP_NOACTIVATE,
                    )
                    .ok();

                    // Update state
                    ctx.mode.set(candidate);

                    // Dispatch Fullscreen event
                    let size = Size::new(
                        flui_types::geometry::DevicePixels(monitor_rect.right - monitor_rect.left),
                        flui_types::geometry::DevicePixels(monitor_rect.bottom - monitor_rect.top),
                    );
                    ctx.dispatch_event(crate::traits::WindowEvent::Fullscreen {
                        window_id: ctx.window_id,
                        size,
                    });
                }
            }
        }
    }

    /// Toggle fullscreen mode for this window
    pub fn toggle_fullscreen(&self) {
        Self::toggle_fullscreen_for_hwnd(self.hwnd)
    }

    /// Check if the window is currently in fullscreen mode
    pub fn is_fullscreen(&self) -> bool {
        unsafe {
            let ctx_ptr =
                GetWindowLongPtrW(self.hwnd, GWLP_USERDATA) as *mut super::platform::WindowContext;
            if ctx_ptr.is_null() {
                return false;
            }
            let ctx = &*ctx_ptr;
            ctx.mode.get().is_fullscreen()
        }
    }

    /// Set fullscreen mode
    ///
    /// # Arguments
    /// * `fullscreen` - true to enter fullscreen, false to exit fullscreen
    pub fn set_fullscreen(&self, fullscreen: bool) {
        let is_fullscreen = self.is_fullscreen();

        // Only toggle if state needs to change
        if fullscreen != is_fullscreen {
            Self::toggle_fullscreen_for_hwnd(self.hwnd);
        }
    }

    /// Check if rendering should be skipped for this window
    ///
    /// Returns true if the window is minimized, as rendering minimized windows
    /// wastes CPU/GPU resources without any visible output.
    pub fn should_skip_render(hwnd: HWND) -> bool {
        unsafe {
            let ctx_ptr =
                GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut super::platform::WindowContext;
            if ctx_ptr.is_null() {
                return false;
            }
            let ctx = &*ctx_ptr;
            ctx.mode.get().is_minimized()
        }
    }
}

impl PlatformWindow for Arc<WindowsWindow> {
    fn physical_size(&self) -> Size<DevicePixels> {
        self.as_ref().physical_size()
    }

    fn logical_size(&self) -> Size<Pixels> {
        self.as_ref().logical_size()
    }

    fn scale_factor(&self) -> f64 {
        self.as_ref().scale_factor() as f64
    }

    fn request_redraw(&self) {
        PlatformWindow::request_redraw(self.as_ref())
    }

    fn is_focused(&self) -> bool {
        PlatformWindow::is_focused(self.as_ref())
    }

    fn is_visible(&self) -> bool {
        PlatformWindow::is_visible(self.as_ref())
    }

    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult + Send>) {
        PlatformWindow::on_input(self.as_ref(), callback)
    }

    fn on_request_frame(&self, callback: Box<dyn FnMut() + Send>) {
        PlatformWindow::on_request_frame(self.as_ref(), callback)
    }

    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32) + Send>) {
        PlatformWindow::on_resize(self.as_ref(), callback)
    }

    fn on_moved(&self, callback: Box<dyn FnMut() + Send>) {
        PlatformWindow::on_moved(self.as_ref(), callback)
    }

    fn on_close(&self, callback: Box<dyn FnOnce() + Send>) {
        PlatformWindow::on_close(self.as_ref(), callback)
    }

    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool + Send>) {
        PlatformWindow::on_should_close(self.as_ref(), callback)
    }

    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        PlatformWindow::on_active_status_change(self.as_ref(), callback)
    }

    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        PlatformWindow::on_hover_status_change(self.as_ref(), callback)
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut() + Send>) {
        PlatformWindow::on_appearance_changed(self.as_ref(), callback)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self.as_ref()
    }
}

impl PlatformWindow for WindowsWindow {
    fn physical_size(&self) -> Size<DevicePixels> {
        let state = self.state.lock();
        let logical = state.bounds.size;
        let scale = state.scale_factor;
        Size::new(
            device_px(logical_to_device(logical.width.0, scale)),
            device_px(logical_to_device(logical.height.0, scale)),
        )
    }

    fn logical_size(&self) -> Size<Pixels> {
        self.state.lock().bounds.size
    }

    fn scale_factor(&self) -> f64 {
        self.state.lock().scale_factor as f64
    }

    fn request_redraw(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    fn is_focused(&self) -> bool {
        self.state.lock().focused
    }

    fn is_visible(&self) -> bool {
        self.state.lock().visible
    }

    // ==================== Per-Window Callbacks ====================

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

    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        *self.callbacks.on_hover_status_change.lock() = Some(callback);
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut() + Send>) {
        *self.callbacks.on_appearance_changed.lock() = Some(callback);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// Implement raw-window-handle for wgpu integration
impl HasWindowHandle for WindowsWindow {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        use std::num::NonZeroIsize;

        let hwnd_value = self.hwnd.0 as isize;
        let mut handle = Win32WindowHandle::new(
            NonZeroIsize::new(hwnd_value).ok_or(raw_window_handle::HandleError::Unavailable)?,
        );

        unsafe {
            let hinstance =
                GetModuleHandleW(None).map_err(|_| raw_window_handle::HandleError::Unavailable)?;
            let hinstance_value = hinstance.0 as isize;
            handle.hinstance = NonZeroIsize::new(hinstance_value);
        }

        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(RawWindowHandle::Win32(handle)) })
    }
}

impl HasDisplayHandle for WindowsWindow {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        let handle = WindowsDisplayHandle::new();
        Ok(unsafe {
            raw_window_handle::DisplayHandle::borrow_raw(RawDisplayHandle::Windows(handle))
        })
    }
}

impl Clone for WindowsWindow {
    fn clone(&self) -> Self {
        Self {
            hwnd: self.hwnd,
            state: Arc::clone(&self.state),
            callbacks: Arc::clone(&self.callbacks),
            windows_map: Arc::clone(&self.windows_map),
        }
    }
}

// ============================================================================
// Cross-Platform Window Trait Implementation
// ============================================================================

use crate::window::{
    RawWindowHandle as CrossRawWindowHandle, Window as WindowTrait, WindowId as CrossWindowId,
    WindowState as CrossWindowState,
};

impl WindowTrait for WindowsWindow {
    fn id(&self) -> CrossWindowId {
        CrossWindowId::new(self.hwnd.0 as u64)
    }

    fn title(&self) -> String {
        self.state.lock().title.clone()
    }

    fn set_title(&mut self, title: &str) {
        unsafe {
            let title_str = HSTRING::from(title);
            SetWindowTextW(self.hwnd, &title_str).ok();
            self.state.lock().title = title.to_string();
        }
    }

    fn position(&self) -> Point<Pixels> {
        self.state.lock().bounds.origin
    }

    fn set_position(&mut self, position: Point<Pixels>) {
        unsafe {
            let scale = self.state.lock().scale_factor;
            let x = logical_to_device(position.x.0, scale);
            let y = logical_to_device(position.y.0, scale);

            SetWindowPos(
                self.hwnd,
                None,
                x,
                y,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .ok();

            self.state.lock().bounds.origin = position;
        }
    }

    fn size(&self) -> Size<Pixels> {
        self.state.lock().bounds.size
    }

    fn set_size(&mut self, size: Size<Pixels>) {
        unsafe {
            let scale = self.state.lock().scale_factor;
            let width = logical_to_device(size.width.0, scale);
            let height = logical_to_device(size.height.0, scale);

            SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                width,
                height,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .ok();

            self.state.lock().bounds.size = size;
        }
    }

    fn state(&self) -> CrossWindowState {
        let placement = self.get_window_placement();

        if placement.showCmd == SW_MINIMIZE.0 as u32 {
            CrossWindowState::Minimized
        } else if placement.showCmd == SW_MAXIMIZE.0 as u32 {
            CrossWindowState::Maximized
        } else if self.is_fullscreen() {
            CrossWindowState::Fullscreen
        } else {
            CrossWindowState::Normal
        }
    }

    fn set_state(&mut self, state: CrossWindowState) {
        unsafe {
            match state {
                CrossWindowState::Normal => {
                    if self.is_fullscreen() {
                        self.set_fullscreen(false);
                    }
                    let _ = ShowWindow(self.hwnd, SW_RESTORE);
                }
                CrossWindowState::Minimized => {
                    let _ = ShowWindow(self.hwnd, SW_MINIMIZE);
                }
                CrossWindowState::Maximized => {
                    if self.is_fullscreen() {
                        self.set_fullscreen(false);
                    }
                    let _ = ShowWindow(self.hwnd, SW_MAXIMIZE);
                }
                CrossWindowState::Fullscreen => {
                    self.set_fullscreen(true);
                }
            }
        }
    }

    fn is_visible(&self) -> bool {
        self.state.lock().visible
    }

    fn set_visible(&mut self, visible: bool) {
        unsafe {
            let cmd = if visible { SW_SHOW } else { SW_HIDE };
            let _ = ShowWindow(self.hwnd, cmd);
            self.state.lock().visible = visible;
        }
    }

    fn is_resizable(&self) -> bool {
        unsafe {
            let style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
            (style & WS_THICKFRAME.0) != 0
        }
    }

    fn set_resizable(&mut self, resizable: bool) {
        unsafe {
            let mut style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
            if resizable {
                style |= WS_THICKFRAME.0;
            } else {
                style &= !WS_THICKFRAME.0;
            }
            SetWindowLongPtrW(self.hwnd, GWL_STYLE, style as isize);
            SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .ok();
        }
    }

    fn is_minimizable(&self) -> bool {
        unsafe {
            let style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
            (style & WS_MINIMIZEBOX.0) != 0
        }
    }

    fn set_minimizable(&mut self, minimizable: bool) {
        unsafe {
            let mut style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
            if minimizable {
                style |= WS_MINIMIZEBOX.0;
            } else {
                style &= !WS_MINIMIZEBOX.0;
            }
            SetWindowLongPtrW(self.hwnd, GWL_STYLE, style as isize);
            SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .ok();
        }
    }

    fn is_closable(&self) -> bool {
        unsafe {
            let style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
            (style & WS_SYSMENU.0) != 0
        }
    }

    fn set_closable(&mut self, closable: bool) {
        unsafe {
            let mut style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
            if closable {
                style |= WS_SYSMENU.0;
            } else {
                style &= !WS_SYSMENU.0;
            }
            SetWindowLongPtrW(self.hwnd, GWL_STYLE, style as isize);
            SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .ok();
        }
    }

    fn focus(&mut self) {
        unsafe {
            let _ = SetForegroundWindow(self.hwnd);
        }
    }

    fn is_focused(&self) -> bool {
        self.state.lock().focused
    }

    fn close(&mut self) {
        unsafe {
            DestroyWindow(self.hwnd).ok();
        }
    }

    fn request_redraw(&mut self) {
        PlatformWindow::request_redraw(self);
    }

    fn set_min_size(&mut self, size: Option<Size<Pixels>>) {
        // Windows doesn't have a direct API for min/max size
        // This would need to be handled in WM_GETMINMAXINFO message
        // For now, store in WindowState for future use
        tracing::debug!("set_min_size: {:?} (not yet implemented)", size);
    }

    fn set_max_size(&mut self, size: Option<Size<Pixels>>) {
        // Windows doesn't have a direct API for min/max size
        // This would need to be handled in WM_GETMINMAXINFO message
        // For now, store in WindowState for future use
        tracing::debug!("set_max_size: {:?} (not yet implemented)", size);
    }

    fn scale_factor(&self) -> f32 {
        self.state.lock().scale_factor
    }

    fn raw_window_handle(&self) -> CrossRawWindowHandle {
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            CrossRawWindowHandle::Windows {
                hwnd: self.hwnd.0,
                hinstance: hinstance.0,
            }
        }
    }
}

impl WindowsWindow {
    /// Helper to get window placement
    fn get_window_placement(&self) -> WINDOWPLACEMENT {
        unsafe {
            let mut placement = WINDOWPLACEMENT {
                length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                ..Default::default()
            };
            GetWindowPlacement(self.hwnd, &mut placement).ok();
            placement
        }
    }

    /// Set DWM window attribute
    unsafe fn set_dwm_attribute<T>(&self, attribute: i32, value: &T) -> windows::core::Result<()> {
        use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};

        DwmSetWindowAttribute(
            self.hwnd,
            DWMWINDOWATTRIBUTE(attribute),
            value as *const T as *const std::ffi::c_void,
            std::mem::size_of::<T>() as u32,
        )
    }

    /// Get DWM window attribute
    unsafe fn get_dwm_attribute<T: Default>(&self, attribute: i32) -> windows::core::Result<T> {
        use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWINDOWATTRIBUTE};

        let mut value = T::default();
        DwmGetWindowAttribute(
            self.hwnd,
            DWMWINDOWATTRIBUTE(attribute),
            &mut value as *mut T as *mut std::ffi::c_void,
            std::mem::size_of::<T>() as u32,
        )?;
        Ok(value)
    }
}

// ============================================================================
// Windows Window Extension Trait Implementation
// ============================================================================

use super::window_ext::{
    dwm_attributes, TaskbarProgressState, WindowCornerPreference, WindowsBackdrop, WindowsTheme,
    WindowsWindowExt as WindowsWindowExtTrait,
};

impl WindowsWindowExtTrait for WindowsWindow {
    fn set_backdrop(&mut self, backdrop: WindowsBackdrop) {
        unsafe {
            let backdrop_value = backdrop.to_dwm_value();
            if let Err(e) =
                self.set_dwm_attribute(dwm_attributes::DWMWA_SYSTEMBACKDROP_TYPE, &backdrop_value)
            {
                tracing::warn!("Failed to set backdrop material: {:?}", e);
            } else {
                tracing::debug!("Set window backdrop to {:?}", backdrop);
            }
        }
    }

    fn clear_backdrop(&mut self) {
        self.set_backdrop(WindowsBackdrop::None);
    }

    fn backdrop(&self) -> WindowsBackdrop {
        unsafe {
            match self.get_dwm_attribute::<i32>(dwm_attributes::DWMWA_SYSTEMBACKDROP_TYPE) {
                Ok(1) => WindowsBackdrop::None,
                Ok(2) => WindowsBackdrop::Mica,
                Ok(3) => WindowsBackdrop::Acrylic,
                Ok(4) => WindowsBackdrop::MicaAlt,
                _ => WindowsBackdrop::None,
            }
        }
    }

    fn enable_snap_layouts(&mut self) {
        // Snap Layouts are automatically enabled on Windows 11 if the window has
        // a standard maximize button. No explicit API call needed.
        // We just need to ensure WS_MAXIMIZEBOX is set
        unsafe {
            let mut style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
            style |= WS_MAXIMIZEBOX.0;
            SetWindowLongPtrW(self.hwnd, GWL_STYLE, style as isize);
            SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .ok();

            tracing::debug!("Snap Layouts enabled (via WS_MAXIMIZEBOX)");
        }
    }

    fn disable_snap_layouts(&mut self) {
        // Disable by removing WS_MAXIMIZEBOX
        unsafe {
            let mut style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
            style &= !WS_MAXIMIZEBOX.0;
            SetWindowLongPtrW(self.hwnd, GWL_STYLE, style as isize);
            SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .ok();

            tracing::debug!("Snap Layouts disabled");
        }
    }

    fn is_snap_layouts_enabled(&self) -> bool {
        unsafe {
            let style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
            (style & WS_MAXIMIZEBOX.0) != 0
        }
    }

    fn set_corner_preference(&mut self, preference: WindowCornerPreference) {
        unsafe {
            let corner_value = preference.to_dwm_value();
            if let Err(e) = self.set_dwm_attribute(
                dwm_attributes::DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner_value,
            ) {
                tracing::warn!("Failed to set corner preference: {:?}", e);
            } else {
                tracing::debug!("Set corner preference to {:?}", preference);
            }
        }
    }

    fn corner_preference(&self) -> WindowCornerPreference {
        unsafe {
            match self.get_dwm_attribute::<i32>(dwm_attributes::DWMWA_WINDOW_CORNER_PREFERENCE) {
                Ok(0) => WindowCornerPreference::Default,
                Ok(1) => WindowCornerPreference::DoNotRound,
                Ok(2) => WindowCornerPreference::Round,
                Ok(3) => WindowCornerPreference::RoundSmall,
                _ => WindowCornerPreference::Default,
            }
        }
    }

    fn enable_blur_behind(&mut self, enable: bool) {
        use windows::Win32::Graphics::Dwm::{
            DwmEnableBlurBehindWindow, DWM_BB_ENABLE, DWM_BLURBEHIND,
        };

        unsafe {
            let bb = DWM_BLURBEHIND {
                dwFlags: DWM_BB_ENABLE,
                fEnable: if enable { TRUE } else { FALSE },
                hRgnBlur: HRGN::default(),
                fTransitionOnMaximized: FALSE,
            };

            if let Err(e) = DwmEnableBlurBehindWindow(self.hwnd, &bb) {
                tracing::warn!("Failed to enable blur behind: {:?}", e);
            } else {
                tracing::debug!("Blur behind: {}", enable);
            }
        }
    }

    fn set_taskbar_progress(&mut self, state: TaskbarProgressState, progress: u32) {
        // This requires ITaskbarList3 COM interface
        // For now, just log - full implementation would need COM integration
        tracing::debug!("Set taskbar progress: {:?} {}%", state, progress);

        // TODO: Implement ITaskbarList3::SetProgressState and SetProgressValue
        // This requires:
        // 1. CoCreateInstance for ITaskbarList3
        // 2. Call SetProgressState(hwnd, state)
        // 3. Call SetProgressValue(hwnd, progress, 100)
    }

    fn clear_taskbar_progress(&mut self) {
        self.set_taskbar_progress(TaskbarProgressState::NoProgress, 0);
    }

    fn set_dark_mode(&mut self, dark_mode: bool) {
        unsafe {
            let dark_mode_value: i32 = if dark_mode { 1 } else { 0 };
            if let Err(e) = self.set_dwm_attribute(
                dwm_attributes::DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark_mode_value,
            ) {
                tracing::warn!("Failed to set dark mode: {:?}", e);
            } else {
                tracing::debug!("Set dark mode: {}", dark_mode);
            }
        }
    }

    fn is_dark_mode(&self) -> bool {
        unsafe {
            self.get_dwm_attribute::<i32>(dwm_attributes::DWMWA_USE_IMMERSIVE_DARK_MODE)
                .unwrap_or(0)
                != 0
        }
    }

    fn set_theme(&mut self, theme: WindowsTheme) {
        if let Some(dark_mode) = theme.to_dark_mode_value() {
            self.set_dark_mode(dark_mode);
        } else {
            // System theme - try to detect system preference
            // For now, just log
            tracing::debug!("Using system theme");
        }
    }

    fn theme(&self) -> WindowsTheme {
        if self.is_dark_mode() {
            WindowsTheme::Dark
        } else {
            WindowsTheme::Light
        }
    }

    fn set_has_shadow(&mut self, has_shadow: bool) {
        // Windows doesn't have a direct API to disable shadows
        // Shadows are controlled by DWM composition
        // We can try extended window styles, but this is limited
        tracing::debug!("set_has_shadow: {} (limited support)", has_shadow);
    }

    fn set_title_bar_color(&mut self, color: Option<(u8, u8, u8)>) {
        unsafe {
            if let Some((r, g, b)) = color {
                // Windows expects COLORREF format: 0x00BBGGRR
                let colorref: u32 = ((b as u32) << 16) | ((g as u32) << 8) | (r as u32);

                if let Err(e) =
                    self.set_dwm_attribute(dwm_attributes::DWMWA_CAPTION_COLOR, &colorref)
                {
                    tracing::warn!("Failed to set title bar color: {:?}", e);
                } else {
                    tracing::debug!("Set title bar color: RGB({}, {}, {})", r, g, b);
                }
            } else {
                // Reset to default (0xFFFFFFFF means use default)
                let default_color: u32 = 0xFFFFFFFF;
                self.set_dwm_attribute(dwm_attributes::DWMWA_CAPTION_COLOR, &default_color)
                    .ok();
            }
        }
    }

    fn set_caption_color(&mut self, color: Option<(u8, u8, u8)>) {
        // Caption color is the same as title bar color in Windows 11
        self.set_title_bar_color(color);
    }

    fn set_animations_enabled(&mut self, enabled: bool) {
        // Windows animations are typically controlled system-wide
        // Per-window animation control is limited
        tracing::debug!("set_animations_enabled: {} (system-wide setting)", enabled);
    }

    fn dpi(&self) -> u32 {
        unsafe { GetDpiForWindow(self.hwnd) }
    }

    fn convert_point_from_device(&self, point: Point<DevicePixels>) -> Point<Pixels> {
        let scale = self.scale_factor();
        Point::new(px(point.x.0 as f32 / scale), px(point.y.0 as f32 / scale))
    }

    fn convert_point_to_device(&self, point: Point<Pixels>) -> Point<DevicePixels> {
        let scale = self.scale_factor();
        Point::new(
            device_px((point.x.0 * scale).round() as i32),
            device_px((point.y.0 * scale).round() as i32),
        )
    }
}

impl Drop for WindowsWindow {
    fn drop(&mut self) {
        // Only destroy if this is the last reference
        if Arc::strong_count(&self.state) == 1 {
            tracing::debug!("Destroying window HWND {:?}", self.hwnd);

            unsafe {
                if !self.hwnd.is_invalid() {
                    DestroyWindow(self.hwnd).ok();
                }
            }

            // Remove from windows map
            let hwnd_key = self.hwnd.0 as isize;
            self.windows_map.lock().remove(&hwnd_key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // Requires WindowsPlatform to register window class
    fn test_window_creation() {
        let options = WindowOptions {
            title: "Test Window".to_string(),
            size: Size::new(px(800.0), px(600.0)),
            resizable: true,
            visible: false,
            decorated: true,
            min_size: None,
            max_size: None,
        };

        let windows_map = Arc::new(Mutex::new(HashMap::new()));
        let handlers = Arc::new(Mutex::new(PlatformHandlers::default()));
        let config = crate::config::WindowConfiguration::default();
        let result = WindowsWindow::new(options, windows_map, handlers, config);

        assert!(
            result.is_ok(),
            "Failed to create window: {:?}",
            result.err()
        );

        let window = result.unwrap();
        assert!(!window.hwnd().is_invalid());
        assert_eq!(window.logical_size().width.0, 800.0);
    }
}
