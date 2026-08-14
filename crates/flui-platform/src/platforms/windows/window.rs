//! Windows window implementation

use std::{
    collections::HashMap,
    sync::Arc,
    thread::{self, ThreadId},
};

use anyhow::{Context, Result};
use cursor_icon::CursorIcon;
use flui_types::geometry::{Bounds, DevicePixels, Pixels, Point, Size, device_px, px};
use parking_lot::Mutex;
use raw_window_handle::{
    HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle, Win32WindowHandle,
    WindowsDisplayHandle,
};
use windows::{
    Win32::{
        Foundation::{FALSE, HWND, LPARAM, POINT, RECT, TRUE, WPARAM},
        Graphics::Gdi::{
            HRGN, InvalidateRect, MONITOR_DEFAULTTOPRIMARY, MonitorFromWindow, ScreenToClient,
            UpdateWindow,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::{GetDpiForSystem, GetDpiForWindow},
            WindowsAndMessaging::{
                CW_USEDEFAULT, CreateWindowExW, DestroyWindow, GCLP_HBRBACKGROUND, GWL_STYLE,
                GetClientRect, GetCursorPos, GetForegroundWindow, GetWindowLongPtrW,
                IDC_APPSTARTING, IDC_ARROW, IDC_CROSS, IDC_HAND, IDC_IBEAM, IDC_NO, IDC_SIZEALL,
                IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE, IDC_WAIT, LoadCursorW,
                PostMessageW, SW_HIDE, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, SW_SHOW,
                SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
                SetClassLongPtrW, SetCursor, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos,
                SetWindowTextW, ShowWindow, WS_EX_APPWINDOW, WS_MAXIMIZEBOX, WS_MINIMIZEBOX,
                WS_OVERLAPPEDWINDOW, WS_POPUP, WS_SYSMENU, WS_THICKFRAME, WS_VISIBLE,
            },
        },
    },
    core::HSTRING,
};

use super::util::{USER_DEFAULT_SCREEN_DPI, WINDOW_CLASS_NAME, logical_to_device};
use crate::{
    shared::{PlatformHandlers, WindowCallbacks},
    traits::{
        CursorError, DispatchEventResult, PlatformDisplay, PlatformInput, PlatformWindow,
        WindowAppearance, WindowBackgroundAppearance, WindowBounds, WindowId, WindowMode,
        WindowOptions,
    },
};

/// Windows window wrapper
pub struct WindowsWindow {
    /// Native window handle
    hwnd: HWND,

    /// Window state
    state: Arc<Mutex<WindowState>>,

    /// Context shared with the WNDPROC invocation pin.
    context: Arc<super::platform::WindowContext>,
}

// SAFETY, per field: `state` is synchronized and `context` is compile-time
// asserted `Send + Sync`. The only non-`Sync` member is `hwnd: HWND`, a bare
// address — sending or sharing the address itself aliases nothing, so `Send`
// and `Sync` are sound for the struct.
//
// NOT claimed — an HWND is thread-AFFINE, not "thread-safe by design": its
// message queue belongs to the thread that created it, and Win32 requires
// several mutations below to run on that owning thread. The public fullscreen,
// cursor, and close paths either execute on the recorded owner or post a
// private command to its WNDPROC; synchronized queries never dereference the
// raw userdata slot.
unsafe impl Send for WindowsWindow {}
unsafe impl Sync for WindowsWindow {}

/// Mutable window state
pub(super) struct WindowState {
    /// Current window bounds (logical pixels)
    pub(super) bounds: Bounds<Pixels>,

    /// Current scale factor (DPI / 96)
    pub(super) scale_factor: f32,

    /// Is window visible?
    pub(super) visible: bool,

    /// Is window focused?
    pub(super) focused: bool,

    /// Window title
    pub(super) title: String,

    pub(super) mode: WindowMode,
    pub(super) last_size: Size<DevicePixels>,
    pub(super) is_hovered: bool,
    pub(super) modifiers: keyboard_types::Modifiers,
    pub(super) cursor: CursorIcon,
    pub(super) restore_style: u32,
    pub(super) is_destroyed: bool,
    owner_thread: ThreadId,
}

const WM_FLUI_ENTER_FULLSCREEN: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 1;
const WM_FLUI_EXIT_FULLSCREEN: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 2;
const WM_FLUI_APPLY_CURSOR: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 3;
const WM_FLUI_CLOSE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WindowCommand {
    EnterFullscreen,
    ExitFullscreen,
    ApplyCursor,
    Close,
}

impl WindowCommand {
    pub(super) const fn from_message(message: u32) -> Option<Self> {
        match message {
            WM_FLUI_ENTER_FULLSCREEN => Some(Self::EnterFullscreen),
            WM_FLUI_EXIT_FULLSCREEN => Some(Self::ExitFullscreen),
            WM_FLUI_APPLY_CURSOR => Some(Self::ApplyCursor),
            WM_FLUI_CLOSE => Some(Self::Close),
            _ => None,
        }
    }

    const fn message(self) -> u32 {
        match self {
            Self::EnterFullscreen => WM_FLUI_ENTER_FULLSCREEN,
            Self::ExitFullscreen => WM_FLUI_EXIT_FULLSCREEN,
            Self::ApplyCursor => WM_FLUI_APPLY_CURSOR,
            Self::Close => WM_FLUI_CLOSE,
        }
    }
}

impl std::fmt::Debug for WindowsWindow {
    // Hand-written: `WindowCallbacks` is a callback payload with no meaningful
    // Debug representation.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowsWindow")
            .field("hwnd", &self.hwnd.0)
            .finish_non_exhaustive()
    }
}

impl WindowsWindow {
    /// Create a new Windows window
    pub fn new(
        options: WindowOptions,
        windows_map: Arc<Mutex<HashMap<isize, Arc<WindowsWindow>>>>,
        handlers: Arc<Mutex<PlatformHandlers>>,
        config: crate::config::WindowConfiguration,
    ) -> Result<Arc<Self>> {
        // Ordinary scope only: construction stays one expression while each
        // Win32 operation receives its own narrow unsafe justification.
        {
            // SAFETY: `GetModuleHandleW(None)` queries the current process image
            // and takes no pointer arguments.
            let hinstance =
                unsafe { GetModuleHandleW(None) }.context("Failed to get module handle")?;

            // SAFETY: `GetDpiForSystem` takes no arguments and returns process DPI
            // state by value.
            let dpi = unsafe { GetDpiForSystem() };
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
            // SAFETY: the class is registered before `open_window`; `title` is a
            // live HSTRING for the call and no pointer argument is retained.
            let hwnd = unsafe {
                CreateWindowExW(
                    ex_style,
                    WINDOW_CLASS_NAME,
                    &title,
                    style,
                    x,
                    y,
                    width,
                    height,
                    None,
                    None,
                    Some(hinstance.into()),
                    None,
                )
            }
            .context("Failed to create window")?;

            if hwnd.is_invalid() {
                return Err(windows::core::Error::from_thread().into());
            }

            // Remove background brush to allow Mica backdrop
            // SAFETY: `hwnd` was just created successfully; this writes a plain
            // zero brush-handle value to the registered class slot.
            unsafe { SetClassLongPtrW(hwnd, GCLP_HBRBACKGROUND, 0) };

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

            // Create window state with default bounds (actual bounds will be set after
            // creation)
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
                mode: WindowMode::Normal,
                // `width`/`height` are ALREADY device pixels — they were
                // converted from `options.size` above. Converting again here
                // squared the scale factor, so a 2x display recorded a
                // `last_size` four times the logical size, which restore and
                // minimize sizing then read back.
                last_size: Size::new(DevicePixels(width), DevicePixels(height)),
                is_hovered: false,
                modifiers: keyboard_types::Modifiers::empty(),
                cursor: CursorIcon::default(),
                restore_style: 0,
                is_destroyed: false,
                owner_thread: thread::current().id(),
            }));

            // Create and store WindowContext for event dispatch
            use super::platform::WindowContext;

            let window_id = WindowId(hwnd.0 as u64);
            let context = Arc::new(WindowContext {
                window_id,
                handlers: handlers.clone(),
                callbacks,
                state: Arc::clone(&state),
                config,
                windows: Arc::downgrade(&windows_map),
            });
            let window = Arc::new(Self {
                hwnd,
                state,
                context: Arc::clone(&context),
            });
            if let Err(error) = super::platform::install_window_context(hwnd, context) {
                window.state.lock().is_destroyed = true;
                // SAFETY: creation succeeded on this owner thread; error
                // cleanup destroys the unpublished HWND here.
                if let Err(destroy_error) = unsafe { DestroyWindow(hwnd) } {
                    tracing::warn!(
                        ?hwnd,
                        ?destroy_error,
                        "DestroyWindow failed after userdata installation failure"
                    );
                }
                return Err(error);
            }

            // Show window if requested
            if options.visible {
                // ShowWindow's return value reports the window's PREVIOUS
                // visibility state (nonzero if it was already visible), not
                // success/failure -- a freshly created window is always
                // previously-hidden, so treating the BOOL as a Result would
                // warn on every visible window creation. Nothing meaningful
                // to check here; UpdateWindow below does return a genuine
                // success/failure BOOL.
                // SAFETY: `hwnd` is live and owner-affine here. ShowWindow's
                // result is prior visibility; UpdateWindow reports failure.
                let _ = unsafe { ShowWindow(hwnd, SW_SHOW) };
                if let Err(error) = unsafe { UpdateWindow(hwnd) }.ok() {
                    tracing::warn!(?hwnd, ?error, "UpdateWindow failed in WindowsWindow::new");
                }
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
        // SAFETY: every call here (`DwmExtendFrameIntoClientArea`,
        // `DwmSetWindowAttribute` x3) takes `hwnd` and a pointer to a
        // stack-local value whose `size_of` matches the `u32` byte-count
        // argument passed alongside it (`MARGINS` by-reference for the first
        // call; `&raw const <i32>` cast to `c_void` for the rest, each with
        // `size_of::<i32>()`). Callers of this function (`WindowsWindow::new`)
        // pass the `hwnd` just returned by `CreateWindowExW`, so it is valid
        // for the duration of this call. All three `DwmSetWindowAttribute`
        // results are discarded (`let _ =`) because every attribute here is a
        // Windows-11-only cosmetic feature — failure on older Windows is
        // expected and non-fatal, not evidence swallowed silently for an
        // operation the caller depends on.
        unsafe {
            use windows::Win32::{
                Graphics::Dwm::{
                    DWMWINDOWATTRIBUTE, DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
                },
                UI::Controls::MARGINS,
            };

            tracing::debug!("Applying Windows 11 features to HWND {:?}", hwnd);

            // 1. Extend frame into client area (required for Mica backdrop)
            let margins = MARGINS {
                cxLeftWidth: -1,
                cxRightWidth: -1,
                cyTopHeight: -1,
                cyBottomHeight: -1,
            };
            let _ = DwmExtendFrameIntoClientArea(hwnd, &raw const margins);

            // 2. Enable Mica backdrop (Windows 11+)
            let mica_value: i32 = 2; // DWMSBT_MAINWINDOW
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWINDOWATTRIBUTE(38), // DWMWA_SYSTEMBACKDROP_TYPE
                (&raw const mica_value).cast::<std::ffi::c_void>(),
                std::mem::size_of::<i32>() as u32,
            );

            // 3. Enable dark mode title bar
            let dark_mode_value: i32 = 1;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWINDOWATTRIBUTE(20), // DWMWA_USE_IMMERSIVE_DARK_MODE
                (&raw const dark_mode_value).cast::<std::ffi::c_void>(),
                std::mem::size_of::<i32>() as u32,
            );

            // 4. Set rounded corners
            let corner_value: i32 = 2; // DWMWCP_ROUND
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWINDOWATTRIBUTE(33), // DWMWA_WINDOW_CORNER_PREFERENCE
                (&raw const corner_value).cast::<std::ffi::c_void>(),
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

    /// Applies a fullscreen request on the HWND owner thread.
    ///
    /// This method implements borderless fullscreen by:
    /// 1. **Entering fullscreen**: Saves current window style and bounds,
    ///    removes window borders (WS_POPUP), and resizes to cover the entire
    ///    monitor
    /// 2. **Exiting fullscreen**: Restores saved window style and bounds
    ///
    /// # Implementation Details
    /// - Uses borderless fullscreen (WS_POPUP) rather than exclusive fullscreen
    ///   for better compatibility
    /// - Automatically detects the monitor containing the window and fills it
    ///   completely
    /// - Preserves window state (position, size, style) for proper restoration
    /// - Dispatches `WindowEvent::Fullscreen` and `WindowEvent::ExitFullscreen`
    ///   events
    ///
    pub(super) fn set_fullscreen_for_context(
        hwnd: HWND,
        context: &super::platform::WindowContext,
        fullscreen: bool,
    ) {
        use windows::Win32::{
            Graphics::Gdi::{
                GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromWindow,
            },
            UI::WindowsAndMessaging::{
                GWL_STYLE, GetWindowLongPtrW, GetWindowRect, HWND_TOP, SWP_FRAMECHANGED,
                SWP_NOACTIVATE, SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, WS_POPUP,
                WS_VISIBLE,
            },
        };

        // SAFETY: the caller is either the HWND's WNDPROC or a public method
        // after checking the owner thread recorded at creation. `context` is
        // already Arc-pinned by that caller, so no userdata lookup or raw
        // dereference occurs here. `GetWindowRect`/`GetWindowLongPtrW`/`SetWindowLongPtrW`/
        // `SetWindowPos`/`MonitorFromWindow`/`GetMonitorInfoW` below all take
        // `hwnd` or a `&raw mut` to a stack-local out-parameter whose size
        // matches what each call expects, and are otherwise ordinary Win32
        // calls with no additional invariant beyond `hwnd` naming a live
        // window, which the OS itself would report via a failed return
        // rather than UB if it did not.
        unsafe {
            let (current_mode, restore_style, is_destroyed) = {
                let state = context.state.lock();
                (state.mode, state.restore_style, state.is_destroyed)
            };
            if is_destroyed || current_mode.is_fullscreen() == fullscreen {
                return;
            }

            if let WindowMode::Fullscreen { restore_bounds } = current_mode {
                // Exit fullscreen - restore previous style and bounds
                tracing::info!("Exiting fullscreen mode");

                // Validate transition
                let candidate = WindowMode::Normal;
                if !current_mode.can_transition_to(&candidate) {
                    tracing::warn!("Cannot exit fullscreen: invalid state transition");
                    return;
                }

                // Restore window style from WindowContext
                SetWindowLongPtrW(hwnd, GWL_STYLE, restore_style as isize);

                // Restore window position and size
                if let Err(error) = SetWindowPos(
                    hwnd,
                    None,
                    restore_bounds.origin.x.0,
                    restore_bounds.origin.y.0,
                    restore_bounds.size.width.0,
                    restore_bounds.size.height.0,
                    SWP_FRAMECHANGED | SWP_NOZORDER | SWP_NOACTIVATE,
                ) {
                    tracing::warn!(
                        ?hwnd,
                        ?error,
                        "SetWindowPos (exit fullscreen restore) failed"
                    );
                }

                // Update state
                context.state.lock().mode = WindowMode::Normal;

                // Dispatch ExitFullscreen event
                context.dispatch_event(crate::traits::WindowEvent::ExitFullscreen {
                    window_id: context.window_id,
                    size: restore_bounds.size,
                });
            } else {
                // Enter fullscreen - save current state and go borderless on monitor
                tracing::info!("Entering fullscreen mode");

                // Get current window rect
                let mut rect = RECT::default();
                if let Err(error) = GetWindowRect(hwnd, &raw mut rect) {
                    tracing::warn!(
                        ?hwnd,
                        ?error,
                        "GetWindowRect failed entering fullscreen; restore bounds will be zeroed"
                    );
                }

                // Save current style to WindowContext
                let current_style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
                context.state.lock().restore_style = current_style;

                // Save current bounds
                let restore_bounds = Bounds {
                    origin: Point::new(DevicePixels(rect.left), DevicePixels(rect.top)),
                    size: Size::new(
                        DevicePixels(rect.right - rect.left),
                        DevicePixels(rect.bottom - rect.top),
                    ),
                };

                // Validate transition
                let candidate = WindowMode::Fullscreen { restore_bounds };
                if !current_mode.can_transition_to(&candidate) {
                    tracing::warn!(
                        "Cannot enter fullscreen: invalid state transition from {:?}",
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
                if let Err(error) = GetMonitorInfoW(monitor, &raw mut monitor_info).ok() {
                    tracing::warn!(
                        ?hwnd,
                        ?error,
                        "GetMonitorInfoW failed entering fullscreen; monitor rect will be zeroed"
                    );
                }

                let monitor_rect = monitor_info.rcMonitor;

                // Set borderless style
                let fullscreen_style = WS_POPUP | WS_VISIBLE;
                SetWindowLongPtrW(hwnd, GWL_STYLE, fullscreen_style.0 as isize);

                // Position window to cover entire monitor
                if let Err(error) = SetWindowPos(
                    hwnd,
                    Some(HWND_TOP),
                    monitor_rect.left,
                    monitor_rect.top,
                    monitor_rect.right - monitor_rect.left,
                    monitor_rect.bottom - monitor_rect.top,
                    SWP_FRAMECHANGED | SWP_NOACTIVATE,
                ) {
                    tracing::warn!(?hwnd, ?error, "SetWindowPos (enter fullscreen) failed");
                }

                // Update state
                context.state.lock().mode = candidate;

                // Dispatch Fullscreen event
                let size = Size::new(
                    flui_types::geometry::DevicePixels(monitor_rect.right - monitor_rect.left),
                    flui_types::geometry::DevicePixels(monitor_rect.bottom - monitor_rect.top),
                );
                context.dispatch_event(crate::traits::WindowEvent::Fullscreen {
                    window_id: context.window_id,
                    size,
                });
            }
        }
    }

    /// Toggle fullscreen mode for this window
    pub fn toggle_fullscreen(&self) {
        self.set_fullscreen(!self.is_fullscreen());
    }

    /// Check if the window is currently in fullscreen mode
    pub fn is_fullscreen(&self) -> bool {
        self.state.lock().mode.is_fullscreen()
    }

    /// Set fullscreen mode
    ///
    /// # Arguments
    /// * `fullscreen` - true to enter fullscreen, false to exit fullscreen
    pub fn set_fullscreen(&self, fullscreen: bool) {
        let command = if fullscreen {
            WindowCommand::EnterFullscreen
        } else {
            WindowCommand::ExitFullscreen
        };
        self.execute_or_post(command);
    }

    /// Check if rendering should be skipped for this window
    ///
    /// Returns true if the window is minimized, as rendering minimized windows
    /// wastes CPU/GPU resources without any visible output.
    pub(super) fn should_skip_render(context: &super::platform::WindowContext) -> bool {
        context.state.lock().mode.is_minimized()
    }

    fn execute_or_post(&self, command: WindowCommand) {
        let (is_owner, is_destroyed) = {
            let state = self.state.lock();
            (
                state.owner_thread == thread::current().id(),
                state.is_destroyed,
            )
        };
        if is_destroyed {
            return;
        }
        if is_owner {
            Self::execute_window_command(self.hwnd, &self.context, command);
        } else if let Err(error) = self.post_command(command) {
            tracing::warn!(
                hwnd = ?self.hwnd,
                ?command,
                ?error,
                "PostMessageW failed for owner-thread window command"
            );
        }
    }

    fn post_command(&self, command: WindowCommand) -> windows::core::Result<()> {
        // SAFETY: these private messages carry no pointer payload. The HWND
        // owner thread decodes the closed `WindowCommand` vocabulary in its
        // WNDPROC and uses the invocation-local Arc pin acquired there.
        unsafe { PostMessageW(Some(self.hwnd), command.message(), WPARAM(0), LPARAM(0)) }
    }

    pub(super) fn execute_window_command(
        hwnd: HWND,
        context: &super::platform::WindowContext,
        command: WindowCommand,
    ) {
        match command {
            WindowCommand::EnterFullscreen => {
                Self::set_fullscreen_for_context(hwnd, context, true);
            }
            WindowCommand::ExitFullscreen => {
                Self::set_fullscreen_for_context(hwnd, context, false);
            }
            WindowCommand::ApplyCursor => {
                let cursor = {
                    let state = context.state.lock();
                    if state.is_destroyed || !state.is_hovered {
                        return;
                    }
                    state.cursor
                };
                if let Err(error) = Self::apply_native_cursor(cursor) {
                    tracing::warn!(?hwnd, ?error, "failed to apply owner-thread cursor command");
                }
            }
            WindowCommand::Close => {
                if context.state.lock().is_destroyed {
                    return;
                }
                // SAFETY: this function is only invoked directly on the
                // recorded owner thread or from the HWND's WNDPROC. Win32
                // synchronously dispatches `WM_DESTROY` before returning.
                unsafe {
                    if let Err(error) = DestroyWindow(hwnd) {
                        tracing::warn!(?hwnd, ?error, "DestroyWindow failed for close command");
                    }
                }
            }
        }
    }

    pub(super) fn apply_native_cursor(cursor: CursorIcon) -> Result<(), CursorError> {
        let resource = match cursor {
            CursorIcon::Pointer | CursorIcon::Copy | CursorIcon::Grab | CursorIcon::Grabbing => {
                IDC_HAND
            }
            CursorIcon::Progress => IDC_APPSTARTING,
            CursorIcon::Wait => IDC_WAIT,
            CursorIcon::Cell | CursorIcon::Crosshair => IDC_CROSS,
            CursorIcon::Text | CursorIcon::VerticalText => IDC_IBEAM,
            CursorIcon::Move | CursorIcon::AllScroll => IDC_SIZEALL,
            CursorIcon::NoDrop | CursorIcon::NotAllowed => IDC_NO,
            CursorIcon::EResize
            | CursorIcon::WResize
            | CursorIcon::EwResize
            | CursorIcon::ColResize => IDC_SIZEWE,
            CursorIcon::NResize
            | CursorIcon::SResize
            | CursorIcon::NsResize
            | CursorIcon::RowResize => IDC_SIZENS,
            CursorIcon::NeResize | CursorIcon::SwResize | CursorIcon::NeswResize => IDC_SIZENESW,
            CursorIcon::NwResize | CursorIcon::SeResize | CursorIcon::NwseResize => IDC_SIZENWSE,
            // Default, ContextMenu, Help, Alias, ZoomIn, ZoomOut, DndAsk and
            // every future variant fall back to the arrow cursor.
            _ => IDC_ARROW,
        };

        // SAFETY: `LoadCursorW(None, resource)` loads a built-in system
        // cursor by atom/ordinal (`resource` is always one of the `IDC_*`
        // constants from the match above, never a caller-supplied pointer),
        // and returns a handle owned by the system — no allocation to free.
        // `SetCursor` takes that handle by value; passing `None` on error is
        // not reachable here since `?` returns before it.
        unsafe {
            let handle = LoadCursorW(None, resource)
                .map_err(|error| CursorError::Backend(error.to_string()))?;
            SetCursor(Some(handle));
        }
        Ok(())
    }
}

impl PlatformWindow for WindowsWindow {
    fn id(&self) -> WindowId {
        WindowId(self.hwnd.0 as u64)
    }

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
        // SAFETY: `InvalidateRect` takes `self.hwnd` and `None` for the
        // rect (invalidate the whole client area) — no pointer to validate.
        // Failure just means nothing was invalidated (e.g. the window is
        // already gone); requesting a redraw is inherently best-effort, so
        // discarding the result keeps this fire-and-forget by design.
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

    fn set_cursor(&self, cursor: CursorIcon) -> Result<(), CursorError> {
        let (is_owner, should_apply) = {
            let mut state = self.state.lock();
            if state.is_destroyed {
                return Err(CursorError::Backend(
                    "the native window is closed".to_string(),
                ));
            }
            state.cursor = cursor;
            (
                state.owner_thread == thread::current().id(),
                state.is_hovered,
            )
        };
        if should_apply {
            if is_owner {
                Self::apply_native_cursor(cursor)?;
            } else {
                self.post_command(WindowCommand::ApplyCursor)
                    .map_err(|error| CursorError::Backend(error.to_string()))?;
            }
        }
        Ok(())
    }

    // ==================== Query Methods ====================

    fn bounds(&self) -> Bounds<Pixels> {
        self.state.lock().bounds
    }

    fn content_size(&self) -> Size<Pixels> {
        // SAFETY: `rect` is a stack-local `RECT` and `&raw mut rect` gives
        // `GetClientRect` a valid, correctly-sized out-parameter; the `Err`
        // path (stale/destroyed `hwnd`) is handled by falling back to the
        // last-known cached bounds rather than reading `rect` uninitialized.
        unsafe {
            let mut rect = RECT::default();
            if GetClientRect(self.hwnd, &raw mut rect).is_ok() {
                let scale = self.state.lock().scale_factor;
                Size::new(
                    px((rect.right - rect.left) as f32 / scale),
                    px((rect.bottom - rect.top) as f32 / scale),
                )
            } else {
                self.state.lock().bounds.size
            }
        }
    }

    fn window_bounds(&self) -> WindowBounds {
        let bounds = self.bounds();
        if self.state.lock().mode.is_fullscreen() {
            return WindowBounds::Fullscreen(bounds);
        }
        if PlatformWindow::is_maximized(self) {
            WindowBounds::Maximized(bounds)
        } else {
            WindowBounds::Windowed(bounds)
        }
    }

    fn is_maximized(&self) -> bool {
        self.state.lock().mode.is_maximized()
    }

    fn is_fullscreen(&self) -> bool {
        // Delegate to the existing method on WindowsWindow
        WindowsWindow::is_fullscreen(self)
    }

    fn is_active(&self) -> bool {
        if self.state.lock().is_destroyed {
            return false;
        }
        // SAFETY: `GetForegroundWindow` takes no arguments; comparing its
        // result to `self.hwnd` is a plain integer/handle comparison.
        unsafe { GetForegroundWindow() == self.hwnd }
    }

    fn is_hovered(&self) -> bool {
        self.state.lock().is_hovered
    }

    fn mouse_position(&self) -> Point<Pixels> {
        if self.state.lock().is_destroyed {
            return Point::default();
        }
        // SAFETY: `cursor_pos` is a stack-local `POINT`; `&raw mut
        // cursor_pos` gives both `GetCursorPos` and `ScreenToClient` a
        // valid, correctly-sized out-parameter. The `is_ok()`/`as_bool()`
        // short-circuit means `cursor_pos` is only read after both calls
        // reported success, so it is never read uninitialized.
        unsafe {
            let mut cursor_pos = POINT::default();
            if GetCursorPos(&raw mut cursor_pos).is_ok()
                && ScreenToClient(self.hwnd, &raw mut cursor_pos).as_bool()
            {
                let scale = self.state.lock().scale_factor;
                Point::new(
                    px(cursor_pos.x as f32 / scale),
                    px(cursor_pos.y as f32 / scale),
                )
            } else {
                Point::default()
            }
        }
    }

    fn modifiers(&self) -> keyboard_types::Modifiers {
        self.state.lock().modifiers
    }

    fn appearance(&self) -> WindowAppearance {
        if self.state.lock().is_destroyed {
            return WindowAppearance::default();
        }
        // SAFETY: `DwmGetWindowAttribute` below
        // writes through `&raw mut dark_mode` cast to `c_void`, sized via
        // `size_of::<i32>()` to match the stack-local `i32` it points at;
        // `dark_mode` is only read after checking `result.is_ok()`.
        unsafe {
            // Check DWM dark mode attribute
            use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmGetWindowAttribute};
            let mut dark_mode: i32 = 0;
            let result = DwmGetWindowAttribute(
                self.hwnd,
                DWMWINDOWATTRIBUTE(20), // DWMWA_USE_IMMERSIVE_DARK_MODE
                (&raw mut dark_mode).cast::<std::ffi::c_void>(),
                std::mem::size_of::<i32>() as u32,
            );
            if result.is_ok() && dark_mode != 0 {
                WindowAppearance::Dark
            } else {
                WindowAppearance::Light
            }
        }
    }

    fn display(&self) -> Option<Arc<dyn PlatformDisplay>> {
        if self.state.lock().is_destroyed {
            return None;
        }
        // SAFETY: `MonitorFromWindow` takes `self.hwnd` by value and a flag;
        // `MONITOR_DEFAULTTOPRIMARY` guarantees a non-null `HMONITOR` even
        // for an invalid `hwnd`, so the `is_invalid()` check below is
        // defensive rather than load-bearing for memory safety — no pointer
        // is dereferenced here.
        unsafe {
            let monitor = MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTOPRIMARY);
            if monitor.is_invalid() {
                return None;
            }
            // Use the display enumeration to find matching monitor
            let displays = super::display::enumerate_displays();
            displays.into_iter().find(|d| {
                // Match by checking if this is the same monitor handle
                // The display enumeration uses HMONITOR internally
                d.is_primary() // Fallback: return primary
            })
        }
    }

    fn get_title(&self) -> String {
        self.state.lock().title.clone()
    }

    // ==================== Control Methods ====================

    fn set_title(&self, title: &str) {
        // SAFETY: `title_str` is a live, locally-owned `HSTRING` for the
        // duration of the call; `SetWindowTextW` reads it by reference and
        // does not retain the pointer past the call.
        unsafe {
            let title_str = HSTRING::from(title);
            if let Err(error) = SetWindowTextW(self.hwnd, &title_str) {
                tracing::warn!(hwnd = ?self.hwnd, ?error, "SetWindowTextW failed");
            }
            self.state.lock().title = title.to_string();
        }
    }

    fn activate(&self) {
        // SAFETY: `SetForegroundWindow` takes `self.hwnd` by value, no
        // pointer arguments.
        unsafe {
            if let Err(error) = SetForegroundWindow(self.hwnd).ok() {
                tracing::warn!(hwnd = ?self.hwnd, ?error, "SetForegroundWindow failed");
            }
        }
    }

    fn minimize(&self) {
        // SAFETY: `ShowWindow` takes `self.hwnd` and a command constant by
        // value, no pointer arguments.
        //
        // Return value is the window's PREVIOUS visibility state (nonzero
        // if it was already visible before this call), not success/failure
        // — nothing meaningful to check or warn on.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_MINIMIZE);
        }
    }

    fn maximize(&self) {
        // SAFETY: see `minimize` above — same call shape and return-value
        // semantics (previous visibility, not success/failure).
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_MAXIMIZE);
        }
    }

    fn restore(&self) {
        // SAFETY: see `minimize` above — same call shape and return-value
        // semantics (previous visibility, not success/failure).
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_RESTORE);
        }
    }

    fn toggle_fullscreen(&self) {
        WindowsWindow::toggle_fullscreen(self);
    }

    fn resize(&self, size: Size<Pixels>) {
        // SAFETY: `SetWindowPos` takes `self.hwnd` and plain integer/flag
        // arguments; `None` for the z-order handle is a documented no-op
        // value, not a null pointer.
        unsafe {
            let scale = self.state.lock().scale_factor;
            let width = logical_to_device(size.width.0, scale);
            let height = logical_to_device(size.height.0, scale);

            if let Err(error) = SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                width,
                height,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            ) {
                tracing::warn!(hwnd = ?self.hwnd, ?error, "SetWindowPos (PlatformWindow::resize) failed");
            }

            self.state.lock().bounds.size = size;
        }
    }

    fn close(&self) {
        self.execute_or_post(WindowCommand::Close);
    }

    fn set_background_appearance(&self, appearance: WindowBackgroundAppearance) {
        // SAFETY: `backdrop_value` is a stack-local `i32`; `DwmSetWindowAttribute`
        // receives it via `&raw const` cast to `c_void`, sized with
        // `size_of::<i32>()` matching the value pointed at.
        unsafe {
            use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmSetWindowAttribute};

            let backdrop_value: i32 = match appearance {
                // DWMSBT_NONE — Windows has no native transparent backdrop,
                // so Transparent also maps to NONE.
                WindowBackgroundAppearance::Opaque | WindowBackgroundAppearance::Transparent => 1,
                WindowBackgroundAppearance::Blurred => 3, // DWMSBT_TRANSIENTWINDOW (Acrylic)
                WindowBackgroundAppearance::MicaBackdrop => 2, // DWMSBT_MAINWINDOW (Mica)
                WindowBackgroundAppearance::MicaAltBackdrop => 4, // DWMSBT_TABBEDWINDOW (Mica Alt)
            };

            if let Err(error) = DwmSetWindowAttribute(
                self.hwnd,
                DWMWINDOWATTRIBUTE(38), // DWMWA_SYSTEMBACKDROP_TYPE
                (&raw const backdrop_value).cast::<std::ffi::c_void>(),
                std::mem::size_of::<i32>() as u32,
            ) {
                tracing::warn!(
                    hwnd = ?self.hwnd,
                    ?error,
                    ?appearance,
                    "DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE) failed"
                );
            }
        }
    }

    // ==================== Per-Window Callbacks ====================

    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult + Send>) {
        *self.context.callbacks.on_input.lock() = Some(callback);
    }

    fn on_request_frame(&self, callback: Box<dyn FnMut() + Send>) {
        *self.context.callbacks.on_request_frame.lock() = Some(callback);
    }

    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32) + Send>) {
        *self.context.callbacks.on_resize.lock() = Some(callback);
    }

    fn on_moved(&self, callback: Box<dyn FnMut() + Send>) {
        *self.context.callbacks.on_moved.lock() = Some(callback);
    }

    fn on_close(&self, callback: Box<dyn FnOnce() + Send>) {
        *self.context.callbacks.on_close.lock() = Some(callback);
    }

    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool + Send>) {
        *self.context.callbacks.on_should_close.lock() = Some(callback);
    }

    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        *self.context.callbacks.on_active_status_change.lock() = Some(callback);
    }

    fn on_visibility_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        *self.context.callbacks.on_visibility_status_change.lock() = Some(callback);
    }

    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        *self.context.callbacks.on_hover_status_change.lock() = Some(callback);
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut() + Send>) {
        *self.context.callbacks.on_appearance_changed.lock() = Some(callback);
    }

    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        HasWindowHandle::window_handle(self)
    }

    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        HasDisplayHandle::display_handle(self)
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

        // SAFETY: `GetModuleHandleW(None)` queries the current process
        // image, no pointer arguments — always sound.
        unsafe {
            let hinstance =
                GetModuleHandleW(None).map_err(|_| raw_window_handle::HandleError::Unavailable)?;
            let hinstance_value = hinstance.0 as isize;
            handle.hinstance = NonZeroIsize::new(hinstance_value);
        }

        // SAFETY: `raw_window_handle::WindowHandle::borrow_raw`'s contract
        // requires the wrapped handle to stay valid for the returned
        // `WindowHandle`'s lifetime. The `'_` this function returns only
        // ties to `&self` — i.e. to the `Arc<WindowsWindow>` staying alive —
        // not to the underlying native HWND staying valid. Those are NOT
        // the same lifetime: `PlatformWindow::close()` (or the user closing
        // the window, which reaches `DestroyWindow` through `window_proc`
        // regardless of which thread requested it) can destroy the native
        // window while an `Arc<WindowsWindow>`, and therefore a
        // `WindowHandle` borrowed from it, is still held and used elsewhere
        // (e.g. by wgpu to (re)create a surface). This is the same
        // undischarged-HWND-lifetime class as the documented cross-thread
        // userdata lifetime is independently pinned by WNDPROC now, but the
        // raw native handle's validity still is not tied to `&self`; this
        // remains a real gap, not a validity claim this code established.
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(RawWindowHandle::Win32(handle)) })
    }
}

impl HasDisplayHandle for WindowsWindow {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        // wgpu 29.x fix: For multi-monitor support, ensure we return a valid display handle.
        // WindowsDisplayHandle::new() creates a valid default for Windows Display enumeration.
        // This helps wgpu locate the correct adapter/surface for the window's monitor.
        let handle = WindowsDisplayHandle::new();
        // SAFETY: `WindowsDisplayHandle` carries no fields (Windows has no
        // per-display native handle in this API) — there is nothing for
        // `borrow_raw` to invalidate; the call only exists to satisfy the
        // `raw-window-handle` trait's `unsafe fn` signature.
        Ok(unsafe {
            raw_window_handle::DisplayHandle::borrow_raw(RawDisplayHandle::Windows(handle))
        })
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
        // SAFETY: see `PlatformWindow::set_title` above — same
        // locally-owned `HSTRING` and call shape.
        unsafe {
            let title_str = HSTRING::from(title);
            if let Err(error) = SetWindowTextW(self.hwnd, &title_str) {
                tracing::warn!(hwnd = ?self.hwnd, ?error, "SetWindowTextW failed");
            }
            self.state.lock().title = title.to_string();
        }
    }

    fn position(&self) -> Point<Pixels> {
        self.state.lock().bounds.origin
    }

    fn set_position(&mut self, position: Point<Pixels>) {
        // SAFETY: `SetWindowPos` takes `self.hwnd` and plain integer/flag
        // arguments; `None` for the z-order handle is a documented no-op
        // value, not a null pointer.
        unsafe {
            let scale = self.state.lock().scale_factor;
            let x = logical_to_device(position.x.0, scale);
            let y = logical_to_device(position.y.0, scale);

            if let Err(error) = SetWindowPos(
                self.hwnd,
                None,
                x,
                y,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            ) {
                tracing::warn!(hwnd = ?self.hwnd, ?error, "SetWindowPos (move) failed");
            }

            self.state.lock().bounds.origin = position;
        }
    }

    fn size(&self) -> Size<Pixels> {
        self.state.lock().bounds.size
    }

    fn set_size(&mut self, size: Size<Pixels>) {
        // SAFETY: see `set_position` above — same call shape.
        unsafe {
            let scale = self.state.lock().scale_factor;
            let width = logical_to_device(size.width.0, scale);
            let height = logical_to_device(size.height.0, scale);

            if let Err(error) = SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                width,
                height,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            ) {
                tracing::warn!(hwnd = ?self.hwnd, ?error, "SetWindowPos (resize) failed");
            }

            self.state.lock().bounds.size = size;
        }
    }

    fn state(&self) -> CrossWindowState {
        match self.state.lock().mode {
            WindowMode::Minimized { .. } => CrossWindowState::Minimized,
            WindowMode::Maximized { .. } => CrossWindowState::Maximized,
            WindowMode::Fullscreen { .. } => CrossWindowState::Fullscreen,
            WindowMode::Normal => CrossWindowState::Normal,
        }
    }

    fn set_state(&mut self, state: CrossWindowState) {
        // SAFETY: `ShowWindow` takes `self.hwnd` and a command constant by
        // value, no pointer arguments; `self.set_fullscreen`/`self.is_fullscreen`
        // carry their own SAFETY contracts documented where they're defined.
        //
        // Every `ShowWindow` return value below is the window's PREVIOUS
        // visibility state (nonzero if it was already visible before this
        // call), not success/failure — nothing meaningful to check or warn
        // on.
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
        // SAFETY: `ShowWindow` takes `self.hwnd` and a command constant by
        // value, no pointer arguments. Return value is the window's
        // PREVIOUS visibility state (nonzero if it was already visible
        // before this call), not success/failure — nothing meaningful to
        // check or warn on.
        unsafe {
            let cmd = if visible { SW_SHOW } else { SW_HIDE };
            let _ = ShowWindow(self.hwnd, cmd);
            self.state.lock().visible = visible;
        }
    }

    fn is_resizable(&self) -> bool {
        // SAFETY: `GetWindowLongPtrW` takes `self.hwnd` and an index
        // constant, returning a plain `isize` bit pattern — no pointer
        // arguments, no invariant beyond the ordinary FFI call.
        unsafe {
            let style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
            (style & WS_THICKFRAME.0) != 0
        }
    }

    fn set_resizable(&mut self, resizable: bool) {
        // SAFETY: see `is_resizable` above for `GetWindowLongPtrW`;
        // `SetWindowLongPtrW` is the same shape in reverse (writes a plain
        // bit pattern, not a pointer); `SetWindowPos` here only re-applies
        // frame metrics (`SWP_FRAMECHANGED`) with no move/resize, same call
        // shape as `set_position` above.
        unsafe {
            let mut style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
            if resizable {
                style |= WS_THICKFRAME.0;
            } else {
                style &= !WS_THICKFRAME.0;
            }
            SetWindowLongPtrW(self.hwnd, GWL_STYLE, style as isize);
            if let Err(error) = SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            ) {
                tracing::warn!(hwnd = ?self.hwnd, ?error, "SetWindowPos (resizable style refresh) failed");
            }
        }
    }

    fn is_minimizable(&self) -> bool {
        // SAFETY: see `is_resizable` above — same call shape.
        unsafe {
            let style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
            (style & WS_MINIMIZEBOX.0) != 0
        }
    }

    fn set_minimizable(&mut self, minimizable: bool) {
        // SAFETY: see `set_resizable` above — same call shape.
        unsafe {
            let mut style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
            if minimizable {
                style |= WS_MINIMIZEBOX.0;
            } else {
                style &= !WS_MINIMIZEBOX.0;
            }
            SetWindowLongPtrW(self.hwnd, GWL_STYLE, style as isize);
            if let Err(error) = SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            ) {
                tracing::warn!(hwnd = ?self.hwnd, ?error, "SetWindowPos (minimizable style refresh) failed");
            }
        }
    }

    fn is_closable(&self) -> bool {
        // SAFETY: see `is_resizable` above — same call shape.
        unsafe {
            let style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
            (style & WS_SYSMENU.0) != 0
        }
    }

    fn set_closable(&mut self, closable: bool) {
        // SAFETY: see `set_resizable` above — same call shape.
        unsafe {
            let mut style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
            if closable {
                style |= WS_SYSMENU.0;
            } else {
                style &= !WS_SYSMENU.0;
            }
            SetWindowLongPtrW(self.hwnd, GWL_STYLE, style as isize);
            if let Err(error) = SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            ) {
                tracing::warn!(hwnd = ?self.hwnd, ?error, "SetWindowPos (closable style refresh) failed");
            }
        }
    }

    fn focus(&mut self) {
        // SAFETY: `SetForegroundWindow` takes `self.hwnd` by value, no
        // pointer arguments.
        unsafe {
            if let Err(error) = SetForegroundWindow(self.hwnd).ok() {
                tracing::warn!(hwnd = ?self.hwnd, ?error, "SetForegroundWindow failed in focus");
            }
        }
    }

    fn is_focused(&self) -> bool {
        self.state.lock().focused
    }

    fn close(&mut self) {
        self.execute_or_post(WindowCommand::Close);
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
        // SAFETY: `GetModuleHandleW(None)` queries the current process
        // image, no pointer arguments — always sound.
        unsafe {
            let hinstance = GetModuleHandleW(None)
                .expect("BUG: GetModuleHandleW(None) cannot fail for the current process image");
            CrossRawWindowHandle::Windows {
                hwnd: self.hwnd.0,
                hinstance: hinstance.0,
            }
        }
    }
}

impl WindowsWindow {
    /// Set DWM window attribute
    ///
    /// # Safety
    ///
    /// `value` must be a valid, initialized `T` whose size and layout match
    /// what `attribute` (a `DWMWINDOWATTRIBUTE` ordinal) expects DWM to read
    /// — the byte count passed to `DwmSetWindowAttribute` is derived from
    /// `size_of::<T>()`, so calling this with the wrong `T` for a given
    /// `attribute` has DWM read past `value`'s bytes.
    unsafe fn set_dwm_attribute<T>(&self, attribute: i32, value: &T) -> windows::core::Result<()> {
        // SAFETY: per the `# Safety` contract above, the caller guarantees
        // `T` matches `attribute`'s expected layout; `value` is a live `&T`
        // for the duration of this call, and `size_of::<T>()` is the
        // correct byte count for the pointer `std::ptr::from_ref` produces
        // from it.
        unsafe {
            use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmSetWindowAttribute};

            DwmSetWindowAttribute(
                self.hwnd,
                DWMWINDOWATTRIBUTE(attribute),
                std::ptr::from_ref::<T>(value).cast::<std::ffi::c_void>(),
                std::mem::size_of::<T>() as u32,
            )
        }
    }

    /// Get DWM window attribute
    ///
    /// # Safety
    ///
    /// `T` must match the layout DWM writes for `attribute` (a
    /// `DWMWINDOWATTRIBUTE` ordinal) — `size_of::<T>()` is the byte count
    /// passed to `DwmGetWindowAttribute`, so the wrong `T` for a given
    /// `attribute` has DWM write past `value`'s bytes.
    unsafe fn get_dwm_attribute<T: Default>(&self, attribute: i32) -> windows::core::Result<T> {
        // SAFETY: per the `# Safety` contract above, `T::default()` seeds a
        // valid, fully-initialized `value` before `&raw mut value` is handed
        // to DWM, so even if the call fails without writing anything, `value`
        // is never read uninitialized; the caller guarantees `T` matches
        // `attribute`'s expected layout and `size_of::<T>()`.
        unsafe {
            use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmGetWindowAttribute};

            let mut value = T::default();
            DwmGetWindowAttribute(
                self.hwnd,
                DWMWINDOWATTRIBUTE(attribute),
                (&raw mut value).cast::<std::ffi::c_void>(),
                std::mem::size_of::<T>() as u32,
            )?;
            Ok(value)
        }
    }
}

// ============================================================================
// Windows Window Extension Trait Implementation
// ============================================================================

use super::window_ext::{
    TaskbarProgressState, WindowCornerPreference, WindowsBackdrop, WindowsTheme,
    WindowsWindowExt as WindowsWindowExtTrait, dwm_attributes,
};

impl WindowsWindowExtTrait for WindowsWindow {
    fn set_backdrop(&mut self, backdrop: WindowsBackdrop) {
        // SAFETY: `backdrop_value` is `i32`, matching `DWMWA_SYSTEMBACKDROP_TYPE`'s
        // expected layout, satisfying `set_dwm_attribute`'s `# Safety` contract.
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
        // SAFETY: `i32` matches `DWMWA_SYSTEMBACKDROP_TYPE`'s expected
        // layout, satisfying `get_dwm_attribute`'s `# Safety` contract.
        unsafe {
            match self.get_dwm_attribute::<i32>(dwm_attributes::DWMWA_SYSTEMBACKDROP_TYPE) {
                // Ok(1) is DWMSBT_NONE — covered by the fallback arm.
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
        //
        // SAFETY: see `PlatformWindow::is_resizable`/`set_resizable` above —
        // same `GetWindowLongPtrW`/`SetWindowLongPtrW`/`SetWindowPos` shape.
        unsafe {
            let mut style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
            style |= WS_MAXIMIZEBOX.0;
            SetWindowLongPtrW(self.hwnd, GWL_STYLE, style as isize);
            if let Err(error) = SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            ) {
                tracing::warn!(hwnd = ?self.hwnd, ?error, "SetWindowPos (enable snap layouts style refresh) failed");
            }

            tracing::debug!("Snap Layouts enabled (via WS_MAXIMIZEBOX)");
        }
    }

    fn disable_snap_layouts(&mut self) {
        // Disable by removing WS_MAXIMIZEBOX
        //
        // SAFETY: see `enable_snap_layouts` above — same call shape.
        unsafe {
            let mut style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
            style &= !WS_MAXIMIZEBOX.0;
            SetWindowLongPtrW(self.hwnd, GWL_STYLE, style as isize);
            if let Err(error) = SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            ) {
                tracing::warn!(hwnd = ?self.hwnd, ?error, "SetWindowPos (disable snap layouts style refresh) failed");
            }

            tracing::debug!("Snap Layouts disabled");
        }
    }

    fn is_snap_layouts_enabled(&self) -> bool {
        // SAFETY: see `PlatformWindow::is_resizable` above — same call shape.
        unsafe {
            let style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
            (style & WS_MAXIMIZEBOX.0) != 0
        }
    }

    fn set_corner_preference(&mut self, preference: WindowCornerPreference) {
        // SAFETY: `corner_value` is `i32`, matching
        // `DWMWA_WINDOW_CORNER_PREFERENCE`'s expected layout, satisfying
        // `set_dwm_attribute`'s `# Safety` contract.
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
        // SAFETY: `i32` matches `DWMWA_WINDOW_CORNER_PREFERENCE`'s expected
        // layout, satisfying `get_dwm_attribute`'s `# Safety` contract.
        unsafe {
            match self.get_dwm_attribute::<i32>(dwm_attributes::DWMWA_WINDOW_CORNER_PREFERENCE) {
                // Ok(0) is DWMCP_DEFAULT — covered by the fallback arm.
                Ok(1) => WindowCornerPreference::DoNotRound,
                Ok(2) => WindowCornerPreference::Round,
                Ok(3) => WindowCornerPreference::RoundSmall,
                _ => WindowCornerPreference::Default,
            }
        }
    }

    fn enable_blur_behind(&mut self, enable: bool) {
        use windows::Win32::Graphics::Dwm::{
            DWM_BB_ENABLE, DWM_BLURBEHIND, DwmEnableBlurBehindWindow,
        };

        // SAFETY: `bb` is a stack-local `DWM_BLURBEHIND`, fully initialized
        // above; `&raw const bb` gives `DwmEnableBlurBehindWindow` a valid
        // pointer to it — the API takes a typed struct pointer, not a
        // `(pointer, size)` pair, so there is no separate size argument to
        // get wrong.
        unsafe {
            let bb = DWM_BLURBEHIND {
                dwFlags: DWM_BB_ENABLE,
                fEnable: if enable { TRUE } else { FALSE },
                hRgnBlur: HRGN::default(),
                fTransitionOnMaximized: FALSE,
            };

            if let Err(e) = DwmEnableBlurBehindWindow(self.hwnd, &raw const bb) {
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
        // SAFETY: `dark_mode_value` is `i32`, matching
        // `DWMWA_USE_IMMERSIVE_DARK_MODE`'s expected layout, satisfying
        // `set_dwm_attribute`'s `# Safety` contract.
        unsafe {
            let dark_mode_value: i32 = i32::from(dark_mode);
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
        // SAFETY: `i32` matches `DWMWA_USE_IMMERSIVE_DARK_MODE`'s expected
        // layout, satisfying `get_dwm_attribute`'s `# Safety` contract.
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
        // SAFETY: both `colorref` and `default_color` are `u32`, matching
        // `DWMWA_CAPTION_COLOR`'s expected `COLORREF` layout, satisfying
        // `set_dwm_attribute`'s `# Safety` contract.
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
                let default_color: u32 = 0xFFFF_FFFF;
                if let Err(error) =
                    self.set_dwm_attribute(dwm_attributes::DWMWA_CAPTION_COLOR, &default_color)
                {
                    tracing::warn!(
                        hwnd = ?self.hwnd,
                        ?error,
                        "failed to reset title bar color to default"
                    );
                }
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
        // SAFETY: `GetDpiForWindow` takes `self.hwnd` by value, no pointer
        // arguments — a stale or invalid `hwnd` cannot cause UB here, it is
        // just an ordinary FFI call either way. NOT a "safe fallback",
        // though: per its documented contract, an invalid `hwnd` makes this
        // return a literal `0`, not some usable default DPI — a caller that
        // divides by this result (e.g. computing a scale factor) would get
        // infinity or NaN, not graceful degradation. No current caller in
        // this crate divides by `dpi()`'s result, but a future one should
        // not assume `0` means "use 96 instead".
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
        let (is_owner, is_destroyed) = {
            let state = self.state.lock();
            (
                state.owner_thread == thread::current().id(),
                state.is_destroyed,
            )
        };
        if is_destroyed || self.hwnd.is_invalid() {
            return;
        }

        tracing::debug!("Closing live window from WindowsWindow::drop");
        if is_owner {
            Self::execute_window_command(self.hwnd, &self.context, WindowCommand::Close);
        } else if let Err(error) = self.post_command(WindowCommand::Close) {
            tracing::warn!(
                hwnd = ?self.hwnd,
                ?error,
                "PostMessageW failed while closing window from foreign-thread Drop"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires WindowsPlatform to register the window class"]
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
