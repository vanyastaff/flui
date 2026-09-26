//! Windows window implementation

use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use cursor_icon::CursorIcon;
use flui_types::geometry::{Bounds, DevicePixels, EdgeInsets, Pixels, Point, Size, device_px, px};
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
                GWLP_USERDATA, GetClientRect, GetCursorPos, GetForegroundWindow, GetWindowLongPtrW,
                GetWindowPlacement, IDC_APPSTARTING, IDC_ARROW, IDC_CROSS, IDC_HAND, IDC_IBEAM,
                IDC_NO, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE, IDC_WAIT,
                IsZoomed, LoadCursorW, PostMessageW, SW_HIDE, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE,
                SW_SHOW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
                SetClassLongPtrW, SetCursor, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos,
                SetWindowTextW, ShowWindow, WINDOWPLACEMENT, WM_CLOSE, WS_EX_APPWINDOW,
                WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_OVERLAPPEDWINDOW, WS_POPUP, WS_SYSMENU,
                WS_THICKFRAME, WS_VISIBLE,
            },
        },
    },
    core::HSTRING,
};

use super::util::{USER_DEFAULT_SCREEN_DPI, WINDOW_CLASS_NAME, logical_to_device};
use crate::{
    shared::{PlatformHandlers, WindowCallbacks, hwnd_affinity::UserDataRefusal},
    traits::{
        CursorError, DispatchEventResult, OpenWindowError, PlatformDisplay, PlatformInput,
        PlatformWindow, WindowAppearance, WindowBackgroundAppearance, WindowBounds,
        WindowExecutionState, WindowId, WindowMode, WindowOptions,
    },
};

/// Windows window wrapper
pub struct WindowsWindow {
    /// Native window handle
    hwnd: HWND,

    /// Window state
    state: Arc<Mutex<WindowState>>,

    /// Which native window this wrapper was built for. The callbacks
    /// themselves live in that window's owner-thread context, not here;
    /// see [`Self::register`].
    identity: super::platform::WindowIdentity,

    /// Reference to platform's window map (for cleanup)
    windows_map: Arc<Mutex<HashMap<isize, Arc<WindowsWindow>>>>,

    /// The context allocation this wrapper installed in `hwnd`'s
    /// `GWLP_USERDATA` slot at creation, kept ONLY as an identity token:
    /// teardown compares it (as a value, never dereferencing through this
    /// field) against the window's current slot so a wrapper that outlived
    /// its native window cannot destroy or close whatever unrelated window
    /// the OS recycled the handle value for — see
    /// [`teardown_route`](super::platform::teardown_route). Dangling as a
    /// VALUE once `WM_DESTROY` has run; that is fine for comparison and
    /// exactly why it must never be dereferenced here.
    context: *const super::platform::WindowContext,

    /// This window's UIA bridge, subclassed onto `hwnd` at construction so
    /// a UI Automation client that asks at any later point is answered.
    /// Inert until one does — see
    /// [`WindowsAccessibility::new`](super::accessibility::WindowsAccessibility::new).
    #[cfg(feature = "a11y")]
    accessibility: Arc<super::accessibility::WindowsAccessibility>,
}

// SAFETY, per field: `state` is an `Arc<Mutex<..>>` with its own
// synchronization; `windows_map` is likewise an `Arc<Mutex<..>>`;
// `identity` is a plain integer. The struct carries no callback storage:
// callbacks live in the owner-thread `WindowContext` and are installed only
// through the context gate below, so a foreign thread can neither run nor
// drop one. The non-`Sync`/non-`Send` members are `hwnd: HWND` and
// `context: *const WindowContext`, both bare addresses that this type never
// dereferences through these fields (`context` is an identity token for
// value comparison only — see its field doc) — sending or sharing an
// address itself aliases nothing, so `Send` and `Sync` are sound for the
// struct.
//
// The HWND behind that address is thread-AFFINE, not "thread-safe by
// design": its message queue belongs to the thread that created it, Win32
// dispatches `window_proc` (including the `WM_DESTROY` arm that retires the
// `WindowContext` stored in `GWLP_USERDATA`) on that thread, and
// `DestroyWindow` refuses to run anywhere else. That obligation is
// DISCHARGED — not merely documented — by routing every affine touch on
// this type through two gates in `platform.rs`:
//
// - every dereference of the `GWLP_USERDATA` context goes through
//   `with_window_context`, which refuses (safe fallback, traced) unless the
//   calling thread is the window's owning thread and the window is live, of
//   our class, with a non-null slot — on the owning thread the deref cannot
//   race the free: the only retiring path (`WM_DESTROY`) is dispatched on
//   that same thread, and the borrow-ledger (`ContextGuard`/`ContextLedger`)
//   defers the actual free past every live borrow, including across
//   reentrant dispatch;
// - every teardown (`close()`, the last wrapper `Drop`) goes through
//   `teardown_route`, which verifies the handle still names THIS wrapper's
//   window (class + context identity, guarding against OS handle
//   recycling), calls `DestroyWindow` only on the owning thread, and
//   otherwise posts `WM_CLOSE` to the owner's queue (`PostMessageW`, the
//   documented cross-thread mechanism).
//
// The decision rules are pure and Linux-tested (`shared::hwnd_affinity`);
// the remaining Win32 calls on `&self` (`ShowWindow`, `SetWindowPos`,
// `SetWindowTextW`, DWM attributes, ...) are documented by Win32 as legal
// cross-thread and dereference nothing.
unsafe impl Send for WindowsWindow {}
// SAFETY: see `Send` above — shared access adds nothing beyond the same
// gated paths.
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
    ///
    /// # Errors
    /// [`OpenWindowError::Backend`] when Win32 window creation fails.
    pub fn new(
        options: WindowOptions,
        windows_map: Arc<Mutex<HashMap<isize, Arc<WindowsWindow>>>>,
        handlers: Rc<RefCell<PlatformHandlers>>,
        config: crate::config::WindowConfiguration,
    ) -> Result<Arc<Self>, OpenWindowError> {
        // SAFETY: `GetModuleHandleW(None)` queries the current process image
        // and takes no pointer arguments — always sound. `GetDpiForSystem`
        // reads global state, no preconditions. `CreateWindowExW` requires
        // `WINDOW_CLASS_NAME` to already be registered, which
        // `WindowsPlatform::with_config` guarantees before any `open_window`
        // call can reach here; `&title` is a live `HSTRING` owned by this
        // frame. `hwnd.is_invalid()` is checked immediately after, so every
        // Win32 call below it only runs against a handle the OS just
        // returned as valid. `SetClassLongPtrW` and `apply_windows_features`
        // operate on that same freshly created, still-valid `hwnd`.
        // `Box::into_raw(context)` intentionally leaks the allocation into
        // the `GWLP_USERDATA` slot — ownership transfers to the window and is
        // reclaimed by `WindowsPlatform::window_proc`'s `WM_DESTROY` arm
        // (`platform.rs`, `Box::from_raw`); a window destroyed by any path
        // that skips `WM_DESTROY` (process-exit teardown) leaks the
        // allocation rather than double-frees or dangles it. `ShowWindow`/
        // `UpdateWindow` again only need a valid `hwnd`, which holds here.
        unsafe {
            let hinstance = GetModuleHandleW(None).map_err(|e| OpenWindowError::Backend {
                message: format!("Failed to get module handle: {e}"),
            })?;

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
            .map_err(|e| OpenWindowError::Backend {
                message: format!("Failed to create window: {e}"),
            })?;

            if hwnd.is_invalid() {
                return Err(OpenWindowError::Backend {
                    message: windows::core::Error::from_thread().to_string(),
                });
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

            // Create window state with default bounds (actual bounds will be set after
            // creation)
            let identity = super::platform::WindowIdentity::mint();

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

            // Create and install the WindowContext for event dispatch
            // BEFORE building the wrapper: the wrapper keeps the pointer as
            // its teardown identity token (see the `context` field doc).
            use flui_types::geometry::{DevicePixels, Size};

            use super::platform::WindowContext;

            let window_id = WindowId(hwnd.0 as u64);
            let device_width = logical_to_device(width as f32, scale_factor);
            let device_height = logical_to_device(height as f32, scale_factor);
            let initial_size = Size::new(DevicePixels(device_width), DevicePixels(device_height));
            // Seed the visibility edge filter from the window's ACTUAL
            // style, not a default: an undecorated window is created
            // `WS_POPUP | WS_VISIBLE` (already visible before this context
            // installs), so the creation-time `ShowWindow(SW_SHOW)` below
            // never delivers a `WM_SHOWWINDOW` edge for it — a `false`
            // seed would swallow that window's first minimize. See
            // `shared::visibility::win32_initial_visibility`'s doc.
            let created_style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            let context = Box::new(WindowContext {
                window_id,
                identity,
                handlers,
                callbacks: WindowCallbacks::new(),
                scale_factor: std::cell::Cell::new(scale_factor),
                mode: std::cell::Cell::new(WindowMode::Normal),
                last_size: std::cell::Cell::new(initial_size),
                config,
                is_hovered: std::cell::Cell::new(false),
                modifiers: std::cell::Cell::new(keyboard_types::Modifiers::empty()),
                cursor: std::cell::Cell::new(CursorIcon::default()),
                restore_style: std::cell::Cell::new(0),
                last_visibility_dispatched: std::cell::Cell::new(
                    crate::shared::visibility::win32_initial_visibility(created_style),
                ),
                pending_high_surrogate: std::cell::Cell::new(None),
                ledger: std::cell::RefCell::new(crate::shared::hwnd_affinity::ContextLedger::new()),
            });
            let context_ptr = Box::into_raw(context);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, context_ptr as isize);

            let window = Arc::new(Self {
                hwnd,
                state,
                identity,
                windows_map,
                context: context_ptr,
                // On the creating (owning) thread, as Win32 subclassing
                // requires; `hwnd` was validated just above.
                #[cfg(feature = "a11y")]
                accessibility: Arc::new(super::accessibility::WindowsAccessibility::new(hwnd)),
            });

            // Show window if requested
            if options.visible {
                // ShowWindow's return value reports the window's PREVIOUS
                // visibility state (nonzero if it was already visible), not
                // success/failure -- a freshly created window is always
                // previously-hidden, so treating the BOOL as a Result would
                // warn on every visible window creation. Nothing meaningful
                // to check here; UpdateWindow below does return a genuine
                // success/failure BOOL.
                let _ = ShowWindow(hwnd, SW_SHOW);
                if let Err(error) = UpdateWindow(hwnd).ok() {
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

    /// Installs a callback into this window's owner-thread context.
    ///
    /// `install` runs only on the window's owner thread, against the context
    /// of the very window this wrapper was built for. Otherwise the
    /// registration is refused and `install`, with the callback it carries,
    /// is dropped right here — on the calling thread for a foreign-thread
    /// registration, which is logged as an error because the caller has
    /// just lost a callback; on the owner for a window that is already gone,
    /// which is routine teardown ordering.
    fn register(&self, op: &'static str, install: impl FnOnce(&WindowCallbacks)) {
        let outcome = super::platform::with_window_context_checked(self.hwnd, op, |context| {
            (context.identity == self.identity).then(|| install(&context.callbacks))
        });
        match outcome {
            Ok(Some(())) => {}
            Ok(None) => tracing::debug!(
                hwnd = ?self.hwnd,
                op,
                "callback registration refused: the handle now names a different window"
            ),
            Err(UserDataRefusal::ForeignThread) => tracing::error!(
                hwnd = ?self.hwnd,
                op,
                "callback registration refused: window callbacks are registered on the \
                 window's owner thread; the callback was dropped on the calling thread"
            ),
            Err(reason) => tracing::debug!(
                hwnd = ?self.hwnd,
                op,
                ?reason,
                "callback registration refused: the native window is gone"
            ),
        }
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

    /// Toggle fullscreen mode for a window by HWND (static method for use from
    /// window_proc)
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
    /// # Thread Safety
    /// Callable from any thread, but only effective on the window's owning
    /// thread (where `window_proc`'s hotkey arm calls it): the context gate
    /// (`with_window_context`, see `platform.rs`) refuses the toggle with a
    /// warning on any other thread instead of racing the owner's
    /// `WM_DESTROY` free of the window context.
    ///
    /// # Example
    /// ```ignore
    /// // Toggle fullscreen on F11 key press (from WM_KEYDOWN handler)
    /// WindowsWindow::toggle_fullscreen_for_hwnd(hwnd);
    /// ```
    pub fn toggle_fullscreen_for_hwnd(hwnd: HWND) {
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

        let toggled = super::platform::with_window_context(hwnd, "toggle_fullscreen", |ctx| {
            let current_mode = ctx.mode.get();

            // SAFETY: `GetWindowRect`/`GetWindowLongPtrW`/`SetWindowLongPtrW`/
            // `SetWindowPos`/`MonitorFromWindow`/`GetMonitorInfoW` below all
            // take `hwnd` or a `&raw mut` to a stack-local out-parameter
            // whose size matches what each call expects, and are otherwise
            // ordinary Win32 calls whose failure mode against a dead `hwnd`
            // is an error return, not UB. The window CAN die mid-closure:
            // `SetWindowPos` synchronously re-enters `window_proc`
            // (`WM_SIZE`/`WM_MOVE`), whose arms dispatch framework
            // callbacks, and `ctx.dispatch_event` below does so directly —
            // a callback may close this window, running a nested
            // `WM_DESTROY` right here. What stays valid regardless is
            // `ctx` itself: the gate's `ContextGuard` defers the context
            // free past this closure (`WM_DESTROY` only retires it — see
            // `ContextLedger`), so post-destruction the remaining `Cell`
            // reads/writes and dead-`hwnd` Win32 calls are sound no-ops,
            // never dangling.
            unsafe {
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
                    let restore_style = ctx.restore_style.get();
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
                    ctx.mode.set(WindowMode::Normal);

                    // Dispatch ExitFullscreen event
                    ctx.dispatch_event(crate::traits::WindowEvent::ExitFullscreen {
                        window_id: ctx.window_id,
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
                    ctx.restore_style.set(current_style);

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
        });
        if toggled.is_none() {
            tracing::warn!(?hwnd, "cannot toggle fullscreen: no usable window context");
        }
    }

    /// Toggle fullscreen mode for this window
    pub fn toggle_fullscreen(&self) {
        Self::toggle_fullscreen_for_hwnd(self.hwnd);
    }

    /// Check if the window is currently in fullscreen mode
    ///
    /// `false` off the owning thread or once the window is gone — the
    /// context gate refuses the read instead of racing `WM_DESTROY`'s free.
    pub fn is_fullscreen(&self) -> bool {
        super::platform::with_window_context(self.hwnd, "is_fullscreen", |ctx| {
            ctx.mode.get().is_fullscreen()
        })
        .unwrap_or(false)
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
        // Every current call site (`platform.rs`'s `WM_PAINT` arm) runs on
        // the owning thread; the context gate enforces that for any other
        // caller of this bare-`HWND` function instead of trusting it.
        super::platform::with_window_context(hwnd, "should_skip_render", |ctx| {
            ctx.mode.get().is_minimized()
        })
        .unwrap_or(false)
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

    /// The window's own UIA bridge — the capability the composition root's
    /// accessibility wire discovers. Without this override the trait
    /// default (`None`) leaves every real Windows window silently invisible
    /// to UI Automation clients.
    #[cfg(feature = "a11y")]
    fn accessibility(&self) -> Option<Arc<dyn crate::traits::PlatformAccessibility>> {
        Some(Arc::clone(&self.accessibility) as _)
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
        super::platform::with_window_context(self.hwnd, "set_cursor", |ctx| {
            ctx.cursor.set(cursor);
            if ctx.is_hovered.get() {
                Self::apply_native_cursor(cursor)?;
            }
            Ok(())
        })
        .ok_or_else(|| {
            CursorError::Backend(
                "the native window is closed, or set_cursor was called off its owning thread"
                    .to_string(),
            )
        })?
    }

    // ==================== Query Methods (US2) ====================

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
        let fullscreen = super::platform::with_window_context(self.hwnd, "window_bounds", |ctx| {
            ctx.mode.get().is_fullscreen()
        })
        .unwrap_or(false);
        if fullscreen {
            return WindowBounds::Fullscreen(bounds);
        }
        if PlatformWindow::is_maximized(self) {
            WindowBounds::Maximized(bounds)
        } else {
            WindowBounds::Windowed(bounds)
        }
    }

    fn is_maximized(&self) -> bool {
        // SAFETY: `IsZoomed` takes `self.hwnd` by value and returns a
        // `BOOL` — no pointer arguments, no invariant beyond an ordinary
        // FFI call.
        unsafe { IsZoomed(self.hwnd).as_bool() }
    }

    fn is_fullscreen(&self) -> bool {
        // Delegate to the existing method on WindowsWindow
        WindowsWindow::is_fullscreen(self)
    }

    fn is_active(&self) -> bool {
        // SAFETY: `GetForegroundWindow` takes no arguments; comparing its
        // result to `self.hwnd` is a plain integer/handle comparison.
        unsafe { GetForegroundWindow() == self.hwnd }
    }

    fn is_hovered(&self) -> bool {
        super::platform::with_window_context(self.hwnd, "is_hovered", |ctx| ctx.is_hovered.get())
            .unwrap_or(false)
    }

    fn mouse_position(&self) -> Point<Pixels> {
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
        super::platform::with_window_context(self.hwnd, "modifiers", |ctx| ctx.modifiers.get())
            .unwrap_or_else(keyboard_types::Modifiers::empty)
    }

    fn appearance(&self) -> WindowAppearance {
        // No window-context read: this query touches no framework state, and
        // DWM answers (or fails safely to the `Light` default, which is also
        // what a dead handle produces) from any thread.
        //
        // SAFETY: `DwmGetWindowAttribute` writes through `&raw mut
        // dark_mode` cast to `c_void`, sized via `size_of::<i32>()` to match
        // the stack-local `i32` it points at; `dark_mode` is only read after
        // checking `result.is_ok()`.
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

    // ==================== Control Methods (US2) ====================

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

    fn show(&self) -> Result<(), crate::WindowShowError> {
        use windows::Win32::UI::WindowsAndMessaging::{
            SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED, WPF_RESTORETOMAXIMIZED,
        };
        crate::shared::hwnd_affinity::show_owned_window(
            || {
                use crate::shared::hwnd_affinity::TeardownRoute;
                let route = super::platform::teardown_route(self.hwnd, self.context);
                if route != TeardownRoute::DestroyDirect {
                    return route;
                }
                // The wrapper and its live context carry the same identity,
                // minted once and never reused, so unlike the raw context
                // address it cannot be recycled. Only inspect it under the
                // owner guard.
                if super::platform::with_window_context(self.hwnd, "show identity", |context| {
                    context.identity == self.identity
                }) == Some(true)
                {
                    route
                } else {
                    TeardownRoute::StaleHandle
                }
            },
            || {
                super::platform::with_window_context(self.hwnd, "show", |_| {
                    // SAFETY: the identity check proves this wrapper's live HWND
                    // on its owner thread. The context guard pins userdata through
                    // ShowWindow reentry; placement is an initialized out pointer.
                    unsafe {
                        let mut placement = WINDOWPLACEMENT {
                            length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                            ..Default::default()
                        };
                        GetWindowPlacement(self.hwnd, &raw mut placement).map_err(|error| {
                            crate::WindowShowError::Native {
                                message: error.to_string(),
                            }
                        })?;
                        let command = if placement.showCmd == SW_SHOWMINIMIZED.0 as u32 {
                            if placement.flags.0 & WPF_RESTORETOMAXIMIZED.0 != 0 {
                                SW_SHOWMAXIMIZED
                            } else {
                                SW_RESTORE
                            }
                        } else {
                            SW_SHOW
                        };
                        // Return value describes previous visibility, not success.
                        let _ = ShowWindow(self.hwnd, command);
                        Ok(())
                    }
                })
                .unwrap_or(Err(crate::WindowShowError::Closed))
            },
            || {
                // SAFETY: the adapter rechecked wrapper identity after ShowWindow,
                // on the same owner thread, before this callback-capable call.
                // Foreground policy may deny the request without failing show.
                unsafe {
                    let _ = SetForegroundWindow(self.hwnd);
                }
            },
        )
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
        use crate::shared::hwnd_affinity::TeardownRoute;
        match super::platform::teardown_route(self.hwnd, self.context) {
            TeardownRoute::DestroyDirect => {
                // SAFETY: `DestroyWindow` takes `self.hwnd` by value; the
                // route just established this is the owning thread, the only
                // one Win32 permits the call from, and where `WM_DESTROY`
                // (handled in `platform.rs`, reclaiming the `GWLP_USERDATA`
                // allocation) is dispatched synchronously before the call
                // returns.
                unsafe {
                    if let Err(error) = DestroyWindow(self.hwnd) {
                        tracing::warn!(hwnd = ?self.hwnd, ?error, "DestroyWindow (PlatformWindow::close) failed");
                    }
                }
            }
            TeardownRoute::PostClose => {
                // Cross-thread close cannot call `DestroyWindow` (Win32
                // forbids it off the creating thread), so it goes through
                // the documented mechanism instead: post `WM_CLOSE` to the
                // owner thread's queue. Note the behavioral nuance: the
                // owner-side `WM_CLOSE` arm consults the window's
                // should-close veto before destroying, so a vetoing window
                // treats a cross-thread `close()` as a close *request*.
                //
                // SAFETY: `PostMessageW` takes the handle and plain
                // integers by value — nothing dereferenced, delivery
                // marshalled by the OS to the owning thread.
                unsafe {
                    if let Err(error) =
                        PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0))
                    {
                        tracing::warn!(hwnd = ?self.hwnd, ?error, "PostMessageW(WM_CLOSE) (cross-thread PlatformWindow::close) failed");
                    }
                }
            }
            TeardownRoute::AlreadyGone => {
                tracing::debug!(hwnd = ?self.hwnd, "close: the native window is already gone");
            }
            TeardownRoute::StaleHandle => {
                tracing::debug!(
                    hwnd = ?self.hwnd,
                    "close: the handle no longer names this wrapper's window \
                     (recycled by the OS); leaving its new owner untouched"
                );
            }
        }
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
    //
    // Every setter goes through `Self::register`: it installs into the
    // owner-thread context, or refuses and drops the callback on the calling
    // thread. A replaced callback is dropped after its slot's lock is
    // released, since its destructor may re-enter this window.

    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult + Send>) {
        self.register("on_input", move |callbacks| {
            let previous = callbacks.on_input.lock().replace(callback);
            drop(previous);
        });
    }

    fn on_request_frame(&self, callback: Box<dyn FnMut() + Send>) {
        self.register("on_request_frame", move |callbacks| {
            let previous = callbacks.on_request_frame.lock().replace(callback);
            drop(previous);
        });
    }

    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32) + Send>) {
        self.register("on_resize", move |callbacks| {
            let previous = callbacks.on_resize.lock().replace(callback);
            drop(previous);
        });
    }

    fn on_moved(&self, callback: Box<dyn FnMut() + Send>) {
        self.register("on_moved", move |callbacks| {
            let previous = callbacks.on_moved.lock().replace(callback);
            drop(previous);
        });
    }

    fn on_close(&self, callback: Box<dyn FnOnce() + Send>) {
        self.register("on_close", move |callbacks| {
            let previous = callbacks.on_close.lock().replace(callback);
            drop(previous);
        });
    }

    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool + Send>) {
        self.register("on_should_close", move |callbacks| {
            let previous = callbacks.on_should_close.lock().replace(callback);
            drop(previous);
        });
    }

    fn on_safe_area_change(&self, callback: Box<dyn FnMut(EdgeInsets) + Send>) {
        self.register("on_safe_area_change", move |callbacks| {
            callbacks.set_safe_area_callback(callback);
        });
    }

    fn on_execution_state_change(&self, callback: Box<dyn FnMut(WindowExecutionState) + Send>) {
        self.register("on_execution_state_change", move |callbacks| {
            callbacks.set_execution_state_callback(callback);
        });
    }

    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        self.register("on_active_status_change", move |callbacks| {
            let previous = callbacks.on_active_status_change.lock().replace(callback);
            drop(previous);
        });
    }

    fn on_visibility_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        self.register("on_visibility_status_change", move |callbacks| {
            let previous = callbacks
                .on_visibility_status_change
                .lock()
                .replace(callback);
            drop(previous);
        });
    }

    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        self.register("on_hover_status_change", move |callbacks| {
            let previous = callbacks.on_hover_status_change.lock().replace(callback);
            drop(previous);
        });
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut() + Send>) {
        self.register("on_appearance_changed", move |callbacks| {
            let previous = callbacks.on_appearance_changed.lock().replace(callback);
            drop(previous);
        });
    }

    fn on_surface_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        self.register("on_surface_status_change", move |callbacks| {
            let previous = callbacks.on_surface_status_change.lock().replace(callback);
            drop(previous);
        });
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

/// Whether [`WindowsWindow`]'s [`HasWindowHandle::window_handle`] may hand
/// out a handle for a window whose teardown routes to `route`.
///
/// Pure so it is testable without a live `HWND` (see
/// [`crate::shared::hwnd_affinity::TeardownRoute`]): `DestroyDirect` and
/// `PostClose` both describe a live window this wrapper still owns (they
/// differ only in which thread may call `DestroyWindow` directly);
/// `AlreadyGone` and `StaleHandle` both describe a window with nothing left
/// to hand a caller — destroyed, or the OS recycled the `HWND` value for an
/// unrelated window.
#[must_use]
fn handle_available(route: crate::shared::hwnd_affinity::TeardownRoute) -> bool {
    use crate::shared::hwnd_affinity::TeardownRoute;
    matches!(
        route,
        TeardownRoute::DestroyDirect | TeardownRoute::PostClose
    )
}

// Implement raw-window-handle for wgpu integration
impl HasWindowHandle for WindowsWindow {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        use std::num::NonZeroIsize;

        // Refuse a destroyed or recycled `HWND` up front — the same
        // identity probe `close()` uses to route teardown, reused here so a
        // caller that re-queries this method (issue #1043's recovery path,
        // which holds an `Arc<dyn PlatformWindow>` rather than a saved
        // handle) never receives a handle whose pointee no longer exists.
        if !handle_available(super::platform::teardown_route(self.hwnd, self.context)) {
            return Err(raw_window_handle::HandleError::Unavailable);
        }

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
        // `WindowHandle`'s lifetime, which this function ties only to
        // `&self` — i.e. to the `Arc<WindowsWindow>` staying alive — not to
        // the underlying native HWND staying valid for that whole span.
        //
        // What the `handle_available` check above closes: a caller that
        // re-acquires a handle from a retained `Arc<dyn PlatformWindow>`
        // (issue #1043's recovery path) never receives one for a window
        // that is already destroyed or whose `HWND` value the OS already
        // recycled for someone else's window — those routes are refused
        // before `Win32WindowHandle::new` runs. And `WM_DESTROY` now calls
        // `WindowCallbacks::clear()` (see `platform.rs`), which breaks the
        // frame-callback → renderer → surface → `Arc<WindowsWindow>` cycle
        // that used to orphan the whole chain forever after a native close
        // — a surface is no longer pinned alive past the window it was
        // created from.
        //
        // What stays open: the identity probe is a point-in-time check.
        // `close()` on the owning thread (`TeardownRoute::DestroyDirect`)
        // calls `DestroyWindow` synchronously, and Win32 dispatches
        // `WM_DESTROY` — with it, `callbacks.clear()` — before that call
        // returns. If that `close()` is itself invoked from inside a
        // callback currently leased out of `WindowCallbacks` (the
        // `CallbackLease` restore hazard #919 identified), a surface built
        // from a handle this method returned earlier in the same call chain
        // can still be alive while `DestroyWindow` runs beneath it — rwh's
        // own contract puts the burden of not outliving the native window
        // on the caller holding the handle, not on this method. That is a
        // logic-level ordering gap this comment records, not a memory-safety
        // one: `WindowHandle::borrow_raw`'s own doc
        // (raw-window-handle-0.6.2/src/borrowed.rs) states its non-null
        // guarantee "only applies to *pointers*, and not any window ID
        // types in the handle", explaining that "it is possible for safe
        // code in the same process to delete the window" — `hwnd` here is
        // exactly such an ID (`Win32WindowHandle::hwnd: NonZeroIsize`, not a
        // pointer type), so a caller is required to handle `HandleError` on
        // every subsequent use rather than assume a handle it already holds
        // stays good.
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(RawWindowHandle::Win32(handle)) })
    }
}

#[cfg(test)]
mod window_handle_availability_tests {
    use super::handle_available;
    use crate::shared::hwnd_affinity::TeardownRoute;

    // This module compiles and runs only on Windows (`WindowsWindow` lives
    // under `#[cfg(windows)]`), so on every other host it is proven sound
    // only by `cross-typecheck`'s clippy pass — never linked, never
    // executed there. `handle_available` itself is a pure function over an
    // already-Linux-tested enum (`hwnd_affinity::route_teardown`'s own
    // tests), so these four cases are the entire behavior this file adds.

    #[test]
    fn a_live_window_this_wrapper_owns_may_hand_out_a_handle() {
        assert!(handle_available(TeardownRoute::DestroyDirect));
        assert!(handle_available(TeardownRoute::PostClose));
    }

    #[test]
    fn a_destroyed_or_recycled_window_refuses_a_handle() {
        assert!(!handle_available(TeardownRoute::AlreadyGone));
        assert!(!handle_available(TeardownRoute::StaleHandle));
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

impl Clone for WindowsWindow {
    fn clone(&self) -> Self {
        Self {
            hwnd: self.hwnd,
            state: Arc::clone(&self.state),
            identity: self.identity,
            windows_map: Arc::clone(&self.windows_map),
            // Same window, same identity token.
            context: self.context,
            // The clone shares the same window, so it shares the same UIA
            // bridge — one subclass hook per HWND, never two.
            #[cfg(feature = "a11y")]
            accessibility: Arc::clone(&self.accessibility),
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
        // SAFETY: see `PlatformWindow::close` above — same call shape.
        unsafe {
            if let Err(error) = DestroyWindow(self.hwnd) {
                tracing::warn!(hwnd = ?self.hwnd, ?error, "DestroyWindow failed");
            }
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
    /// Helper to get window placement
    fn get_window_placement(&self) -> WINDOWPLACEMENT {
        // SAFETY: `placement` is a stack-local `WINDOWPLACEMENT` with
        // `length` pre-filled to its own `size_of` as the API requires;
        // `&raw mut placement` gives `GetWindowPlacement` a valid,
        // correctly-sized out-parameter regardless of whether the call
        // succeeds, so returning `placement` on `Err` is safe (it holds the
        // `Default` values this function seeded, not uninitialized memory) —
        // callers just get a placement that will not match SW_MINIMIZE or
        // SW_MAXIMIZE, i.e. a normal-state placement.
        unsafe {
            let mut placement = WINDOWPLACEMENT {
                length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                ..Default::default()
            };
            if let Err(error) = GetWindowPlacement(self.hwnd, &raw mut placement) {
                tracing::warn!(hwnd = ?self.hwnd, ?error, "GetWindowPlacement failed");
            }
            placement
        }
    }

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
        // Only destroy if this is the last reference
        if Arc::strong_count(&self.state) == 1 {
            tracing::debug!("Destroying window HWND {:?}", self.hwnd);

            use crate::shared::hwnd_affinity::TeardownRoute;
            match super::platform::teardown_route(self.hwnd, self.context) {
                TeardownRoute::DestroyDirect => {
                    // Unhook the UIA subclass BEFORE destroying the window —
                    // Win32 wants subclasses removed while the window still
                    // exists, and this is the owner-thread teardown path
                    // subclassing requires. Any capability `Arc` still held
                    // elsewhere degrades to a no-op.
                    #[cfg(feature = "a11y")]
                    self.accessibility.shutdown();

                    // SAFETY: `DestroyWindow` takes `self.hwnd` by value;
                    // the route just established this is the owning thread
                    // and that the handle still names THIS wrapper's window
                    // (a never-created or already-destroyed handle routes
                    // `AlreadyGone`, a recycled one `StaleHandle`), where
                    // `WM_DESTROY` (handled in `platform.rs`) retires the
                    // `GWLP_USERDATA` allocation synchronously within this
                    // call, the ledger freeing it at the outermost borrow
                    // release.
                    unsafe {
                        if let Err(error) = DestroyWindow(self.hwnd) {
                            tracing::warn!(hwnd = ?self.hwnd, ?error, "DestroyWindow failed in Drop");
                        }
                    }
                }
                TeardownRoute::PostClose => {
                    // Off the owning thread `DestroyWindow` is forbidden and
                    // unhooking the UIA subclass would race the owner's
                    // message dispatch, so neither runs here: the close is
                    // posted to the owner's queue, and the subclass adapter
                    // is left for `WindowsAccessibility`'s own drop guard
                    // (which leaks it, with a warning, rather than unhook
                    // cross-thread).
                    //
                    // SAFETY: `PostMessageW` takes the handle and plain
                    // integers by value — nothing dereferenced.
                    unsafe {
                        if let Err(error) =
                            PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0))
                        {
                            tracing::warn!(hwnd = ?self.hwnd, ?error, "PostMessageW(WM_CLOSE) (cross-thread Drop) failed");
                        }
                    }
                }
                TeardownRoute::AlreadyGone => {
                    tracing::debug!(hwnd = ?self.hwnd, "Drop: the native window is already gone");
                }
                TeardownRoute::StaleHandle => {
                    tracing::debug!(
                        hwnd = ?self.hwnd,
                        "Drop: the handle no longer names this wrapper's window \
                         (recycled by the OS); leaving its new owner untouched"
                    );
                }
            }

            // Remove from windows map
            let hwnd_key = self.hwnd.0 as isize;
            let _prev = self.windows_map.lock().remove(&hwnd_key);
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
            ..Default::default()
        };

        let windows_map = Arc::new(Mutex::new(HashMap::new()));
        let handlers = Rc::new(RefCell::new(PlatformHandlers::default()));
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

/// Window callbacks live in the owner-thread context: a registration from
/// any other thread is refused, and the refused callback is released on the
/// thread that offered it.
#[cfg(test)]
mod callback_affinity_tests {
    use std::sync::Arc;

    use windows::Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        Graphics::Gdi::InvalidateRect,
        UI::WindowsAndMessaging::{DestroyWindow, SendMessageW, WM_PAINT},
    };

    use super::super::{
        WindowsPlatform,
        test_probe::{Probe, ProbeLog, hwnd_of, open_hidden},
    };

    /// Invalidates the whole client area and handles `WM_PAINT` now. The
    /// window is hidden, so the system would not paint it by itself;
    /// `SendMessageW` runs `window_proc` synchronously, so the frame
    /// callback has run (or not) by the time this returns.
    fn paint_now(hwnd: HWND) {
        // SAFETY: both calls take the handle and plain values; `hwnd` names a
        // live window this thread created, so the send is handled here.
        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, false);
            SendMessageW(hwnd, WM_PAINT, Some(WPARAM(0)), Some(LPARAM(0)));
        }
    }

    #[test]
    fn off_owner_registration_is_refused_and_dropped_on_the_registering_thread() {
        let platform = WindowsPlatform::new().expect("platform");
        let window = open_hidden(&platform);
        let log = Arc::new(ProbeLog::default());

        let worker = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let probe = Probe::new(&log);
                    window.on_request_frame(Box::new(move || probe.hit()));
                    std::thread::current().id()
                })
                .join()
                .expect("registering thread")
        });
        paint_now(hwnd_of(&window));

        assert_eq!(log.runs(), 0, "an off-owner frame callback must not run");
        assert_eq!(
            log.dropped_on(),
            Some(worker),
            "the refused callback is released on the thread that offered it"
        );
    }

    #[test]
    fn registration_after_destroy_is_refused_not_parked_on_the_wrapper() {
        let platform = WindowsPlatform::new().expect("platform");
        let window = open_hidden(&platform);
        // SAFETY: the handle names a live window this thread created.
        unsafe { DestroyWindow(hwnd_of(&window)) }.expect("destroy on the owner");

        let log = Arc::new(ProbeLog::default());
        let probe = Probe::new(&log);
        window.on_resize(Box::new(move |_, _| probe.hit()));

        assert_eq!(
            log.dropped_on(),
            Some(std::thread::current().id()),
            "a registration on a destroyed window is released at once, on the registering owner"
        );
        std::thread::spawn(move || drop(window))
            .join()
            .expect("worker drop");
        assert_eq!(log.runs(), 0);
    }

    #[test]
    fn owner_registration_runs_and_is_released_on_the_owner() {
        let platform = WindowsPlatform::new().expect("platform");
        let window = open_hidden(&platform);
        let log = Arc::new(ProbeLog::default());
        let probe = Probe::new(&log);
        window.on_request_frame(Box::new(move || probe.hit()));

        paint_now(hwnd_of(&window));
        assert_eq!(log.runs(), 1, "the owner's frame callback runs on WM_PAINT");
        assert_eq!(log.ran_on(), Some(std::thread::current().id()));
        assert_eq!(
            log.dropped_on(),
            None,
            "still registered while the window lives"
        );

        window.close();
        assert_eq!(
            log.dropped_on(),
            Some(std::thread::current().id()),
            "WM_DESTROY releases the callback on the owner"
        );
    }
}
