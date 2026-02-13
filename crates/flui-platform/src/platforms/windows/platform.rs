//! Windows platform implementation

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::display::enumerate_displays;
use super::util::*;
use super::window::WindowsWindow;
use crate::config::WindowConfiguration;
use crate::executor::{BackgroundExecutor, ForegroundExecutor};
use crate::shared::{PlatformHandlers, WindowCallbacks};
use crate::traits::*;
use flui_types::geometry::{Bounds, DevicePixels, Point, Size};

/// Ensures window class is registered exactly once (sound replacement for `static mut bool`).
static REGISTER_WINDOW_CLASS: std::sync::Once = std::sync::Once::new();

/// Context data stored per window for event dispatch
pub(super) struct WindowContext {
    /// Window ID for event dispatch
    pub window_id: WindowId,
    /// Reference to platform handlers (global)
    pub handlers: Arc<Mutex<PlatformHandlers>>,
    /// Per-window callbacks for event delivery
    pub callbacks: Arc<WindowCallbacks>,
    /// Scale factor for coordinate conversion
    pub scale_factor: f32,
    /// Current window mode (replaces display_state + saved bounds)
    pub mode: std::cell::Cell<WindowMode>,
    /// Last known size (before minimization) for restore detection
    pub last_size: std::cell::Cell<Size<DevicePixels>>,
    /// Window configuration (hotkeys, debouncing, etc.)
    pub config: WindowConfiguration,
    /// Is mouse hovering over this window? (T034)
    pub is_hovered: std::cell::Cell<bool>,
    /// Current keyboard modifiers (T035)
    pub modifiers: std::cell::Cell<keyboard_types::Modifiers>,
}

impl WindowContext {
    /// Dispatch a window event safely without holding locks
    ///
    /// This method extracts the handler, releases the lock, calls the handler,
    /// then re-acquires the lock to restore it. This prevents deadlocks when
    /// the handler tries to acquire the same lock.
    #[inline]
    pub(super) fn dispatch_event(&self, event: WindowEvent) {
        // Take the handler out of the lock
        let handler = self.handlers.lock().window_event.take();

        // Release the lock before calling the handler
        if let Some(mut handler) = handler {
            handler(event);

            // Restore the handler after the call
            self.handlers.lock().window_event = Some(handler);
        }
    }
}

/// Windows platform state
pub struct WindowsPlatform {
    /// Message-only window for platform messages
    message_window: HWND,

    /// All created windows (keyed by HWND)
    windows: Arc<Mutex<HashMap<isize, Arc<WindowsWindow>>>>,

    /// Platform handlers (callbacks from platform to framework)
    handlers: Arc<Mutex<PlatformHandlers>>,

    /// Background executor for async tasks
    background_executor: Arc<BackgroundExecutor>,

    /// Foreground executor for UI thread tasks
    foreground_executor: Arc<ForegroundExecutor>,

    /// Window configuration (shared across all windows)
    config: WindowConfiguration,

    /// DirectWrite text system
    text_system: Arc<dyn PlatformTextSystem>,
}

// SAFETY: HWND is just an integer handle and is safe to send/share between threads.
// Windows API handles are thread-safe by design.
unsafe impl Send for WindowsPlatform {}
unsafe impl Sync for WindowsPlatform {}

impl WindowsPlatform {
    /// Create a new Windows platform instance with default configuration
    pub fn new() -> Result<Self> {
        Self::with_config(WindowConfiguration::default())
    }

    /// Create a new Windows platform instance with custom configuration
    ///
    /// # Arguments
    ///
    /// * `config` - Window configuration (hotkeys, debouncing, fullscreen behavior)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_platform::{WindowsPlatform, WindowConfiguration, FullscreenMonitor};
    ///
    /// // Disable F11 hotkey
    /// let config = WindowConfiguration::no_hotkey();
    /// let platform = WindowsPlatform::with_config(config)?;
    ///
    /// // Use primary monitor for fullscreen
    /// let config = WindowConfiguration {
    ///     fullscreen_monitor: FullscreenMonitor::Primary,
    ///     ..Default::default()
    /// };
    /// let platform = WindowsPlatform::with_config(config)?;
    /// ```
    pub fn with_config(config: WindowConfiguration) -> Result<Self> {
        // Initialize COM for drag-and-drop, clipboard, etc.
        unsafe {
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
            let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            if hr.is_err() {
                return Err(anyhow::anyhow!("Failed to initialize COM: {:?}", hr));
            }
        }

        // Set DPI awareness to per-monitor v2 (best quality)
        // Ignore errors - this can fail if already set or on older Windows
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }

        // Register window class
        unsafe {
            Self::register_window_class()?;
        }

        // Create message-only window for platform messages
        let message_window = unsafe {
            let hinstance = GetModuleHandleW(None)
                .map_err(|e| anyhow::anyhow!("Failed to get module handle: {:?}", e))?;

            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                WINDOW_CLASS_NAME,
                w!("Flui Platform Message Window"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE), // Message-only window
                None,
                Some(hinstance.into()),
                None,
            )
            .map_err(|e| anyhow::anyhow!("Failed to create message window: {:?}", e))?
        };

        // Create executors
        let background_executor = Arc::new(BackgroundExecutor::new());
        let foreground_executor = Arc::new(ForegroundExecutor::new());

        // Create DirectWrite text system (fall back to dummy if DirectWrite fails)
        let text_system: Arc<dyn PlatformTextSystem> =
            match super::text_system::DirectWriteTextSystem::new() {
                Ok(ts) => {
                    tracing::info!("DirectWrite text system initialized");
                    Arc::new(ts)
                }
                Err(e) => {
                    tracing::warn!("DirectWrite init failed, using fallback: {:?}", e);
                    Arc::new(DummyTextSystem)
                }
            };

        tracing::info!("Windows platform initialized with Tokio executors");

        Ok(Self {
            message_window,
            windows: Arc::new(Mutex::new(HashMap::new())),
            handlers: Arc::new(Mutex::new(PlatformHandlers::default())),
            background_executor,
            foreground_executor,
            config,
            text_system,
        })
    }

    /// Register the window class for all FLUI windows (idempotent via `Once`).
    unsafe fn register_window_class() -> Result<()> {
        let mut result: Result<()> = Ok(());

        REGISTER_WINDOW_CLASS.call_once(|| {
            let reg = (|| -> Result<()> {
                let hinstance = GetModuleHandleW(None).context("Failed to get module handle")?;

                let wc = WNDCLASSW {
                    style: CS_HREDRAW | CS_VREDRAW | CS_OWNDC,
                    lpfnWndProc: Some(Self::window_proc),
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: hinstance.into(),
                    hIcon: HICON::default(),
                    hCursor: load_cursor_style(IDC_ARROW)?,
                    hbrBackground: HBRUSH(std::ptr::null_mut()),
                    lpszMenuName: PCWSTR::null(),
                    lpszClassName: WINDOW_CLASS_NAME,
                };

                let atom = RegisterClassW(&wc);
                if atom == 0 {
                    return Err(windows::core::Error::from_win32().into());
                }

                tracing::info!("Registered Windows window class");
                Ok(())
            })();

            if let Err(e) = reg {
                result = Err(e);
            }
        });

        result
    }

    /// Main window procedure for all FLUI windows
    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // Get window context from GWLP_USERDATA
        let ctx_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowContext;
        let ctx = if !ctx_ptr.is_null() {
            Some(&*ctx_ptr)
        } else {
            None
        };

        match msg {
            WM_CREATE => {
                tracing::debug!("WM_CREATE for HWND {:?}", hwnd);
                LRESULT(0)
            }

            WM_CLOSE => {
                tracing::debug!("WM_CLOSE for HWND {:?}", hwnd);

                if let Some(ctx) = ctx {
                    // Ask per-window callback if close should proceed
                    let should_close = ctx.callbacks.dispatch_should_close();

                    if should_close {
                        // Dispatch CloseRequested to global handlers
                        ctx.dispatch_event(WindowEvent::CloseRequested {
                            window_id: ctx.window_id,
                        });
                        DestroyWindow(hwnd).ok();
                    }
                    // If !should_close, the close is vetoed
                } else {
                    DestroyWindow(hwnd).ok();
                }

                LRESULT(0)
            }

            WM_DESTROY => {
                tracing::debug!("WM_DESTROY for HWND {:?}", hwnd);

                if let Some(ctx) = ctx {
                    // Fire per-window on_close callback (FnOnce)
                    ctx.callbacks.dispatch_close();

                    // Dispatch Closed event to global handlers
                    ctx.dispatch_event(WindowEvent::Closed(ctx.window_id));

                    // Clean up context - IMPORTANT: Clear pointer BEFORE dropping to avoid dangling pointer
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    drop(Box::from_raw(ctx_ptr));
                }

                LRESULT(0)
            }

            WM_ERASEBKGND => {
                // Return 1 to prevent Windows from erasing background
                // This allows Mica backdrop and other DWM effects to show through
                tracing::debug!("WM_ERASEBKGND - preventing background erase");
                LRESULT(1)
            }

            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                if !hdc.is_invalid() {
                    // Skip rendering for minimized windows to save CPU/GPU resources
                    let should_skip = WindowsWindow::should_skip_render(hwnd);
                    if !should_skip {
                        // Fill with solid black - required for Mica backdrop transparency
                        // When app draws, this will be replaced with actual content
                        let mut rect = RECT::default();
                        if GetClientRect(hwnd, &mut rect).is_ok() {
                            let black_brush = GetStockObject(BLACK_BRUSH);
                            FillRect(hdc, &rect, HBRUSH(black_brush.0));
                        }

                        if let Some(ctx) = ctx {
                            // Fire per-window on_request_frame callback
                            ctx.callbacks.dispatch_request_frame();

                            // Also dispatch RedrawRequested to global handlers
                            ctx.dispatch_event(WindowEvent::RedrawRequested {
                                window_id: ctx.window_id,
                            });
                        }
                    } else {
                        tracing::trace!("Skipping render for minimized window");
                    }
                    let _ = EndPaint(hwnd, &ps);
                }
                LRESULT(0)
            }

            WM_SIZE => {
                use super::util::{SIZE_MAXIMIZED, SIZE_MINIMIZED, SIZE_RESTORED};

                let width = get_x_lparam(lparam).max(1);
                let height = get_y_lparam(lparam).max(1);
                let size_type = wparam.0 as u32;

                if let Some(ctx) = ctx {
                    use flui_types::geometry::DevicePixels;
                    let size = Size::new(DevicePixels(width), DevicePixels(height));
                    let prev_mode = ctx.mode.get();

                    // Handle state transition and dispatch appropriate event
                    let (new_mode, event) = match size_type {
                        SIZE_MINIMIZED => {
                            tracing::info!("📦 Window Minimized");
                            // Validate transition
                            let candidate = WindowMode::Minimized {
                                previous: Bounds {
                                    origin: Point::new(DevicePixels(0), DevicePixels(0)),
                                    size: ctx.last_size.get(),
                                },
                            };
                            if !prev_mode.can_transition_to(&candidate) {
                                tracing::warn!("⚠️  Invalid state transition: {:?} -> Minimized (transition ignored)", prev_mode);
                                (prev_mode, None)
                            } else {
                                // Save current size before minimizing
                                if !prev_mode.is_minimized() {
                                    ctx.last_size.set(size);
                                }
                                (
                                    candidate,
                                    Some(WindowEvent::Minimized {
                                        window_id: ctx.window_id,
                                    }),
                                )
                            }
                        }
                        SIZE_MAXIMIZED => {
                            tracing::info!("📏 Window Maximized: {}x{}", width, height);
                            // Validate transition
                            let candidate = WindowMode::Maximized {
                                previous: Bounds {
                                    origin: Point::new(DevicePixels(0), DevicePixels(0)),
                                    size: ctx.last_size.get(),
                                },
                            };
                            if !prev_mode.can_transition_to(&candidate) {
                                tracing::warn!("⚠️  Invalid state transition: {:?} -> Maximized (transition ignored)", prev_mode);
                                (prev_mode, None)
                            } else {
                                ctx.last_size.set(size);
                                (
                                    candidate,
                                    Some(WindowEvent::Maximized {
                                        window_id: ctx.window_id,
                                        size,
                                    }),
                                )
                            }
                        }
                        SIZE_RESTORED => {
                            tracing::info!("📐 Window Restored: {}x{}", width, height);
                            // Validate transition
                            let candidate = WindowMode::Normal;
                            if !prev_mode.can_transition_to(&candidate) {
                                tracing::warn!("⚠️  Invalid state transition: {:?} -> Normal (transition ignored)", prev_mode);
                                (prev_mode, None)
                            } else {
                                ctx.last_size.set(size);

                                // Dispatch Restored event only when transitioning FROM minimized or maximized
                                let event = if prev_mode.is_minimized() || prev_mode.is_maximized()
                                {
                                    Some(WindowEvent::Restored {
                                        window_id: ctx.window_id,
                                        size,
                                    })
                                } else {
                                    // Normal resize within normal state
                                    Some(WindowEvent::Resized {
                                        window_id: ctx.window_id,
                                        size,
                                    })
                                };
                                (candidate, event)
                            }
                        }
                        _ => {
                            // Regular resize while in current state
                            tracing::info!("📐 Window Resized: {}x{}", width, height);
                            if prev_mode.is_normal() {
                                ctx.last_size.set(size);
                            }
                            (
                                prev_mode,
                                Some(WindowEvent::Resized {
                                    window_id: ctx.window_id,
                                    size,
                                }),
                            )
                        }
                    };

                    // Update state
                    ctx.mode.set(new_mode);

                    // Fire per-window on_resize callback (for all size changes except minimize)
                    if size_type != SIZE_MINIMIZED {
                        let logical_size = Size::new(
                            flui_types::geometry::px(super::util::device_to_logical(
                                width,
                                ctx.scale_factor,
                            )),
                            flui_types::geometry::px(super::util::device_to_logical(
                                height,
                                ctx.scale_factor,
                            )),
                        );
                        ctx.callbacks
                            .dispatch_resize(logical_size, ctx.scale_factor);
                    }

                    // Dispatch event to global handlers if any
                    if let Some(event) = event {
                        ctx.dispatch_event(event);
                    }
                }

                LRESULT(0)
            }

            WM_MOVE => {
                let x = get_x_lparam(lparam);
                let y = get_y_lparam(lparam);
                tracing::debug!("Window Moved: ({}, {})", x, y);

                if let Some(ctx) = ctx {
                    // Fire per-window on_moved callback
                    ctx.callbacks.dispatch_moved();

                    // Dispatch Moved event to global handlers
                    use flui_types::geometry::{px, Point};
                    let position = Point::new(
                        px(x as f32 / ctx.scale_factor),
                        px(y as f32 / ctx.scale_factor),
                    );
                    ctx.dispatch_event(WindowEvent::Moved {
                        id: ctx.window_id,
                        position,
                    });
                }

                LRESULT(0)
            }

            WM_DPICHANGED => {
                // Extract new DPI from wparam
                let new_dpi = hiword(wparam.0 as u32) as f32;
                let new_scale = new_dpi / 96.0; // 96 DPI = 1.0 scale
                tracing::info!("🔍 DPI Changed: {} (scale: {:.2}x)", new_dpi, new_scale);

                // Dispatch ScaleFactorChanged event
                if let Some(ctx) = ctx {
                    ctx.dispatch_event(WindowEvent::ScaleFactorChanged {
                        window_id: ctx.window_id,
                        scale_factor: new_scale as f64,
                    });

                    // Update context scale factor
                    let ctx_mut = &mut *(ctx_ptr);
                    ctx_mut.scale_factor = new_scale;

                    // Suggested rect for new DPI
                    let suggested_rect = lparam.0 as *const RECT;
                    if !suggested_rect.is_null() {
                        let rect = *suggested_rect;
                        SetWindowPos(
                            hwnd,
                            None,
                            rect.left,
                            rect.top,
                            rect.right - rect.left,
                            rect.bottom - rect.top,
                            SWP_NOZORDER | SWP_NOACTIVATE,
                        )
                        .ok();
                    }
                }

                LRESULT(0)
            }

            WM_MOUSEMOVE => {
                if let Some(ctx) = ctx {
                    // Request WM_MOUSELEAVE notification for hover tracking
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    let _ = TrackMouseEvent(&mut tme);

                    // Track hover state (T034)
                    ctx.is_hovered.set(true);

                    // Dispatch hover enter (will be cleared on WM_MOUSELEAVE)
                    ctx.callbacks.dispatch_hover_status_change(true);

                    use super::events::mouse_move_event;
                    let event = mouse_move_event(lparam, ctx.scale_factor);
                    ctx.callbacks.dispatch_input(event);
                }
                LRESULT(0)
            }

            WM_LBUTTONDOWN => {
                if let Some(ctx) = ctx {
                    use super::events::mouse_button_event;
                    use ui_events::pointer::PointerButton;
                    let event =
                        mouse_button_event(PointerButton::Primary, true, lparam, ctx.scale_factor);
                    ctx.callbacks.dispatch_input(event);
                }
                LRESULT(0)
            }

            WM_RBUTTONDOWN => {
                if let Some(ctx) = ctx {
                    use super::events::mouse_button_event;
                    use ui_events::pointer::PointerButton;
                    let event = mouse_button_event(
                        PointerButton::Secondary,
                        true,
                        lparam,
                        ctx.scale_factor,
                    );
                    ctx.callbacks.dispatch_input(event);
                }
                LRESULT(0)
            }

            WM_MBUTTONDOWN => {
                if let Some(ctx) = ctx {
                    use super::events::mouse_button_event;
                    use ui_events::pointer::PointerButton;
                    let event = mouse_button_event(
                        PointerButton::Auxiliary,
                        true,
                        lparam,
                        ctx.scale_factor,
                    );
                    ctx.callbacks.dispatch_input(event);
                }
                LRESULT(0)
            }

            WM_LBUTTONUP => {
                if let Some(ctx) = ctx {
                    use super::events::mouse_button_event;
                    use ui_events::pointer::PointerButton;
                    let event =
                        mouse_button_event(PointerButton::Primary, false, lparam, ctx.scale_factor);
                    ctx.callbacks.dispatch_input(event);
                }
                LRESULT(0)
            }

            WM_RBUTTONUP => {
                if let Some(ctx) = ctx {
                    use super::events::mouse_button_event;
                    use ui_events::pointer::PointerButton;
                    let event = mouse_button_event(
                        PointerButton::Secondary,
                        false,
                        lparam,
                        ctx.scale_factor,
                    );
                    ctx.callbacks.dispatch_input(event);
                }
                LRESULT(0)
            }

            WM_MBUTTONUP => {
                if let Some(ctx) = ctx {
                    use super::events::mouse_button_event;
                    use ui_events::pointer::PointerButton;
                    let event = mouse_button_event(
                        PointerButton::Auxiliary,
                        false,
                        lparam,
                        ctx.scale_factor,
                    );
                    ctx.callbacks.dispatch_input(event);
                }
                LRESULT(0)
            }

            WM_MOUSEWHEEL => {
                if let Some(ctx) = ctx {
                    use super::events::mouse_wheel_event;
                    let event = mouse_wheel_event(wparam, lparam, ctx.scale_factor);
                    ctx.callbacks.dispatch_input(event);
                }
                LRESULT(0)
            }

            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let vk = wparam.0 as u16;
                let is_repeat = (lparam.0 & (1 << 30)) != 0;

                // Check if fullscreen hotkey is pressed (configurable, default F11)
                if let Some(ctx) = ctx {
                    if let Some(hotkey) = ctx.config.fullscreen_hotkey {
                        if vk == hotkey && !is_repeat {
                            tracing::info!(
                                "Fullscreen hotkey (VK={:#04x}) pressed - toggling fullscreen",
                                hotkey
                            );
                            WindowsWindow::toggle_fullscreen_for_hwnd(hwnd);
                        }
                    }

                    // Track modifiers (T035)
                    ctx.modifiers.set(current_modifiers());

                    // Dispatch keyboard event via per-window callback
                    use super::events::key_down_event;
                    let event = key_down_event(wparam, lparam);
                    ctx.callbacks.dispatch_input(event);
                }

                LRESULT(0)
            }

            WM_KEYUP | WM_SYSKEYUP => {
                if let Some(ctx) = ctx {
                    // Track modifiers (T035)
                    ctx.modifiers.set(current_modifiers());

                    use super::events::key_up_event;
                    let event = key_up_event(wparam, lparam);
                    ctx.callbacks.dispatch_input(event);
                }
                LRESULT(0)
            }

            WM_CHAR => {
                // WM_CHAR is handled by the framework via KeyboardEvent
                // No per-window callback dispatch needed here
                LRESULT(0)
            }

            WM_SETFOCUS => {
                tracing::debug!("Window Focused");

                if let Some(ctx) = ctx {
                    // Fire per-window on_active_status_change callback
                    ctx.callbacks.dispatch_active_status_change(true);

                    // Dispatch FocusChanged to global handlers
                    ctx.dispatch_event(WindowEvent::FocusChanged {
                        window_id: ctx.window_id,
                        focused: true,
                    });
                }

                LRESULT(0)
            }

            WM_KILLFOCUS => {
                tracing::debug!("Window Unfocused");

                if let Some(ctx) = ctx {
                    // Fire per-window on_active_status_change callback
                    ctx.callbacks.dispatch_active_status_change(false);

                    // Dispatch FocusChanged to global handlers
                    ctx.dispatch_event(WindowEvent::FocusChanged {
                        window_id: ctx.window_id,
                        focused: false,
                    });
                }

                LRESULT(0)
            }

            // T025: Mouse hover tracking — WM_MOUSELEAVE (0x02A3)
            0x02A3 => {
                if let Some(ctx) = ctx {
                    // Track hover state (T034)
                    ctx.is_hovered.set(false);

                    ctx.callbacks.dispatch_hover_status_change(false);
                }
                LRESULT(0)
            }

            // T026: System theme/appearance change
            WM_SETTINGCHANGE => {
                if let Some(ctx) = ctx {
                    ctx.callbacks.dispatch_appearance_changed();
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }

            // T046: Keyboard layout change
            WM_INPUTLANGCHANGE => {
                if let Some(ctx) = ctx {
                    // Dispatch keyboard layout change via take/restore pattern
                    let handler = ctx.handlers.lock().keyboard_layout_changed.take();
                    if let Some(mut handler) = handler {
                        handler();
                        ctx.handlers.lock().keyboard_layout_changed = Some(handler);
                    }
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }

            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    /// Run the Windows message loop (internal implementation)
    fn run_message_loop(&self) -> Result<()> {
        tracing::info!("Starting Windows message loop");

        unsafe {
            let mut msg = MSG::default();

            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                // Drain foreground executor tasks before processing Windows messages
                self.foreground_executor.drain_tasks();

                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            tracing::info!("Message loop exited with code: {}", msg.wParam.0);
        }

        Ok(())
    }
}

impl Platform for WindowsPlatform {
    // ==================== Core System ====================

    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        Arc::clone(&self.background_executor) as Arc<dyn PlatformExecutor>
    }

    fn foreground_executor(&self) -> Arc<dyn PlatformExecutor> {
        Arc::clone(&self.foreground_executor) as Arc<dyn PlatformExecutor>
    }

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        self.text_system.clone()
    }

    // ==================== Lifecycle ====================

    fn run(&self, on_ready: Box<dyn FnOnce()>) {
        tracing::info!("Running Windows platform");

        // Call ready callback
        on_ready();

        // Run message loop
        if let Err(e) = self.run_message_loop() {
            tracing::error!("Message loop error: {:?}", e);
        }
    }

    fn quit(&self) {
        tracing::info!("Quitting Windows platform");
        unsafe {
            PostQuitMessage(0);
        }
    }

    fn request_frame(&self) {
        // TODO: Implement frame requests
        tracing::trace!("Frame requested");
    }

    // ==================== Window Management ====================

    fn active_window(&self) -> Option<WindowId> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_invalid() {
                None
            } else {
                Some(WindowId(hwnd.0 as u64))
            }
        }
    }

    // ==================== Display Management ====================

    fn displays(&self) -> Vec<Arc<dyn PlatformDisplay>> {
        enumerate_displays()
    }

    fn primary_display(&self) -> Option<Arc<dyn PlatformDisplay>> {
        enumerate_displays().into_iter().find(|d| d.is_primary())
    }

    fn open_window(&self, options: WindowOptions) -> Result<Box<dyn PlatformWindow>> {
        tracing::info!("Opening window: {:?}", options.title);

        let window = WindowsWindow::new(
            options,
            self.windows.clone(),
            self.handlers.clone(),
            self.config.clone(),
        )?;
        let hwnd_value = window.hwnd().0 as isize;

        // Store window
        self.windows.lock().insert(hwnd_value, window.clone());

        Ok(Box::new(window))
    }

    // ==================== Input & Clipboard ====================

    fn clipboard(&self) -> Arc<dyn Clipboard> {
        Arc::new(super::WindowsClipboard::new())
    }

    // ==================== Platform Capabilities ====================

    fn capabilities(&self) -> &dyn PlatformCapabilities {
        // TODO: Return actual capabilities
        &WINDOWS_CAPABILITIES
    }

    fn name(&self) -> &'static str {
        "Windows"
    }

    // ==================== Callbacks ====================

    fn on_quit(&self, callback: Box<dyn FnMut() + Send>) {
        self.handlers.lock().quit = Some(callback);
    }

    fn on_window_event(&self, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        self.handlers.lock().window_event = Some(callback);
    }

    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut() + Send>) {
        self.handlers.lock().keyboard_layout_changed = Some(callback);
    }

    // ==================== App Activation (US3 T038) ====================

    fn activate(&self, _ignoring_other_apps: bool) {
        unsafe {
            // Bring the foreground window to front
            let hwnd = GetForegroundWindow();
            if !hwnd.is_invalid() {
                let _ = SetForegroundWindow(hwnd);
            }
        }
    }

    // ==================== Appearance (US3 T040) ====================

    fn window_appearance(&self) -> WindowAppearance {
        // Read system theme from registry: AppsUseLightTheme
        use windows::Win32::System::Registry::*;
        unsafe {
            let mut hkey = HKEY::default();
            let subkey: Vec<u16> =
                "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0"
                    .encode_utf16()
                    .collect();
            let value_name: Vec<u16> = "AppsUseLightTheme\0".encode_utf16().collect();

            let status = RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                Some(0),
                KEY_READ,
                &mut hkey,
            );
            if status.is_err() {
                return WindowAppearance::Light;
            }

            let mut data: u32 = 1;
            let mut data_size = std::mem::size_of::<u32>() as u32;
            let status = RegQueryValueExW(
                hkey,
                PCWSTR(value_name.as_ptr()),
                None,
                None,
                Some(&mut data as *mut u32 as *mut u8),
                Some(&mut data_size),
            );
            let _ = RegCloseKey(hkey);

            if status.is_err() {
                return WindowAppearance::Light;
            }

            if data == 0 {
                WindowAppearance::Dark
            } else {
                WindowAppearance::Light
            }
        }
    }

    // ==================== Cursor (US3 T039) ====================

    fn set_cursor_style(&self, style: crate::cursor::CursorStyle) {
        use crate::cursor::CursorStyle;
        let cursor_id = match style {
            CursorStyle::Arrow => IDC_ARROW,
            CursorStyle::IBeam => IDC_IBEAM,
            CursorStyle::Crosshair => IDC_CROSS,
            CursorStyle::ClosedHand | CursorStyle::OpenHand => IDC_HAND,
            CursorStyle::PointingHand => IDC_HAND,
            CursorStyle::ResizeLeft | CursorStyle::ResizeRight | CursorStyle::ResizeLeftRight => {
                IDC_SIZEWE
            }
            CursorStyle::ResizeUp | CursorStyle::ResizeDown | CursorStyle::ResizeUpDown => {
                IDC_SIZENS
            }
            CursorStyle::ResizeUpLeftDownRight => IDC_SIZENWSE,
            CursorStyle::ResizeUpRightDownLeft => IDC_SIZENESW,
            CursorStyle::ResizeColumn => IDC_SIZEWE,
            CursorStyle::ResizeRow => IDC_SIZENS,
            CursorStyle::OperationNotAllowed => IDC_NO,
            CursorStyle::DragLink | CursorStyle::DragCopy => IDC_HAND,
            CursorStyle::ContextualMenu => IDC_ARROW,
            CursorStyle::None => {
                // Hide cursor
                unsafe {
                    SetCursor(None);
                }
                return;
            }
        };
        unsafe {
            if let Ok(cursor) = LoadCursorW(None, cursor_id) {
                SetCursor(Some(cursor));
            }
        }
    }

    // ==================== File Operations (US3 T041) ====================

    fn open_url(&self, url: &str) {
        use windows::Win32::UI::Shell::ShellExecuteW;
        let wide_url: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                PCWSTR(wide_url.as_ptr()),
                None,
                None,
                SW_SHOWNORMAL,
            );
        }
    }

    fn reveal_path(&self, path: &std::path::Path) {
        use windows::Win32::UI::Shell::ShellExecuteW;
        // Use "explorer /select,<path>" to reveal in Explorer
        let path_str = path.to_string_lossy();
        let arg = format!("/select,{}", path_str);
        let wide_arg: Vec<u16> = arg.encode_utf16().chain(std::iter::once(0)).collect();
        let explorer: Vec<u16> = "explorer\0".encode_utf16().collect();
        unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                PCWSTR(explorer.as_ptr()),
                PCWSTR(wide_arg.as_ptr()),
                None,
                SW_SHOWNORMAL,
            );
        }
    }

    fn open_path(&self, path: &std::path::Path) {
        use windows::Win32::UI::Shell::ShellExecuteW;
        let wide_path: Vec<u16> = path
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                PCWSTR(wide_path.as_ptr()),
                None,
                None,
                SW_SHOWNORMAL,
            );
        }
    }

    // ==================== File Dialogs (US3 T042-T043) ====================

    fn prompt_for_paths(
        &self,
        options: crate::traits::PathPromptOptions,
    ) -> crate::task::Task<Result<Option<Vec<std::path::PathBuf>>>> {
        let executor = self.background_executor.clone();
        executor.spawn(async move {
            // COM file dialogs must run on an STA thread
            let result = std::thread::spawn(move || -> Result<Option<Vec<std::path::PathBuf>>> {
                unsafe {
                    use windows::Win32::System::Com::*;
                    use windows::Win32::UI::Shell::*;

                    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

                    let dialog: IFileOpenDialog =
                        CoCreateInstance(&FileOpenDialog, None, CLSCTX_ALL)?;

                    let mut flags = FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST;
                    if options.multiple {
                        flags |= FOS_ALLOWMULTISELECT;
                    }
                    if options.directories {
                        flags |= FOS_PICKFOLDERS;
                    }
                    dialog.SetOptions(flags)?;

                    match dialog.Show(None) {
                        Ok(()) => {}
                        Err(e)
                            if e.code()
                                == windows::core::HRESULT::from_win32(ERROR_CANCELLED.0) =>
                        {
                            return Ok(None);
                        }
                        Err(e) => return Err(e.into()),
                    }

                    let results = dialog.GetResults()?;
                    let count = results.GetCount()?;
                    let mut paths = Vec::with_capacity(count as usize);
                    for i in 0..count {
                        let item = results.GetItemAt(i)?;
                        let name = item.GetDisplayName(SIGDN_FILESYSPATH)?;
                        let path_str = name.to_string()?;
                        paths.push(std::path::PathBuf::from(path_str));
                        CoTaskMemFree(Some(name.as_ptr() as *const _));
                    }
                    Ok(Some(paths))
                }
            })
            .join()
            .map_err(|_| anyhow::anyhow!("File dialog thread panicked"))??;
            Ok(result)
        })
    }

    fn prompt_for_new_path(
        &self,
        directory: &std::path::Path,
        suggested_name: Option<&str>,
    ) -> crate::task::Task<Result<Option<std::path::PathBuf>>> {
        let dir = directory.to_path_buf();
        let name = suggested_name.map(|s| s.to_string());
        let executor = self.background_executor.clone();
        executor.spawn(async move {
            let result = std::thread::spawn(move || -> Result<Option<std::path::PathBuf>> {
                unsafe {
                    use windows::Win32::System::Com::*;
                    use windows::Win32::UI::Shell::*;

                    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

                    let dialog: IFileSaveDialog =
                        CoCreateInstance(&FileSaveDialog, None, CLSCTX_ALL)?;

                    dialog.SetOptions(
                        FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_OVERWRITEPROMPT,
                    )?;

                    // Set initial directory
                    let dir_wide: Vec<u16> = dir
                        .to_string_lossy()
                        .encode_utf16()
                        .chain(std::iter::once(0))
                        .collect();
                    if let Ok(folder) = SHCreateItemFromParsingName::<PCWSTR, _, IShellItem>(
                        PCWSTR(dir_wide.as_ptr()),
                        None,
                    ) {
                        let _ = dialog.SetFolder(&folder);
                    }

                    // Set suggested file name
                    if let Some(ref name) = name {
                        let name_hstring = windows::core::HSTRING::from(name.as_str());
                        let _ = dialog.SetFileName(&name_hstring);
                    }

                    match dialog.Show(None) {
                        Ok(()) => {}
                        Err(e)
                            if e.code()
                                == windows::core::HRESULT::from_win32(ERROR_CANCELLED.0) =>
                        {
                            return Ok(None);
                        }
                        Err(e) => return Err(e.into()),
                    }

                    let result = dialog.GetResult()?;
                    let name = result.GetDisplayName(SIGDN_FILESYSPATH)?;
                    let path_str = name.to_string()?;
                    let path = std::path::PathBuf::from(path_str);
                    CoTaskMemFree(Some(name.as_ptr() as *const _));
                    Ok(Some(path))
                }
            })
            .join()
            .map_err(|_| anyhow::anyhow!("File dialog thread panicked"))??;
            Ok(result)
        })
    }

    // ==================== Keyboard (US3 T045) ====================

    fn keyboard_layout(&self) -> String {
        use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayoutNameW;
        unsafe {
            let mut buffer = [0u16; 9]; // KL_NAMELENGTH = 9
            if GetKeyboardLayoutNameW(&mut buffer).is_ok() {
                String::from_utf16_lossy(
                    &buffer[..buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len())],
                )
            } else {
                String::new()
            }
        }
    }

    // ==================== File System Integration ====================

    fn app_path(&self) -> Result<std::path::PathBuf> {
        unsafe {
            let mut buffer = [0u16; 260]; // MAX_PATH, stack-allocated
            let len = GetModuleFileNameW(None, &mut buffer);
            if len == 0 {
                return Err(windows::core::Error::from_win32().into());
            }
            Ok(std::path::PathBuf::from(String::from_utf16_lossy(
                &buffer[..len as usize],
            )))
        }
    }
}

impl Drop for WindowsPlatform {
    fn drop(&mut self) {
        tracing::debug!("Dropping WindowsPlatform");

        // Destroy message window
        if !self.message_window.is_invalid() {
            unsafe {
                DestroyWindow(self.message_window).ok();
            }
        }

        // Uninitialize COM
        unsafe {
            use windows::Win32::System::Com::CoUninitialize;
            CoUninitialize();
        }
    }
}

// PlatformHandlers is imported from crate::shared

// ==================== Helper Functions ====================

/// Read current keyboard modifier state from Win32 (T035)
fn current_modifiers() -> keyboard_types::Modifiers {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };

    unsafe {
        let mut mods = keyboard_types::Modifiers::empty();
        if (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 {
            mods |= keyboard_types::Modifiers::SHIFT;
        }
        if (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 {
            mods |= keyboard_types::Modifiers::CONTROL;
        }
        if (GetKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0 {
            mods |= keyboard_types::Modifiers::ALT;
        }
        if (GetKeyState(VK_LWIN.0 as i32) as u16 & 0x8000) != 0
            || (GetKeyState(VK_RWIN.0 as i32) as u16 & 0x8000) != 0
        {
            mods |= keyboard_types::Modifiers::META;
        }
        mods
    }
}

// ==================== Dummy Implementations ====================

struct DummyTextSystem;

impl PlatformTextSystem for DummyTextSystem {
    fn add_fonts(&self, _fonts: Vec<std::borrow::Cow<'static, [u8]>>) -> anyhow::Result<()> {
        Ok(())
    }

    fn all_font_names(&self) -> Vec<String> {
        vec!["Segoe UI".to_string()]
    }

    fn font_id(&self, _descriptor: &Font) -> anyhow::Result<FontId> {
        Ok(FontId(0))
    }

    fn font_metrics(&self, _font_id: FontId) -> FontMetrics {
        FontMetrics {
            units_per_em: 2048,
            ascent: 1854.0,
            descent: 434.0,
            line_gap: 0.0,
            underline_position: -130.0,
            underline_thickness: 90.0,
            cap_height: 1434.0,
            x_height: 1024.0,
        }
    }

    fn glyph_for_char(&self, _font_id: FontId, _ch: char) -> Option<GlyphId> {
        None
    }

    fn layout_line(&self, text: &str, font_size: f32, _runs: &[FontRun]) -> LineLayout {
        let char_count = text.chars().count() as f32;
        LineLayout {
            font_size,
            width: char_count * font_size * 0.6,
            ascent: font_size * 0.8,
            descent: font_size * 0.2,
            runs: Vec::new(),
            len: text.len(),
        }
    }
}

// Windows platform capabilities
static WINDOWS_CAPABILITIES: DesktopCapabilities = DesktopCapabilities;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_creation() {
        let result = WindowsPlatform::new();
        assert!(
            result.is_ok(),
            "Failed to create Windows platform: {:?}",
            result.err()
        );
    }
}
