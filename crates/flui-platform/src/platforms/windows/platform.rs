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
use windows::Win32::UI::WindowsAndMessaging::*;

use super::display::enumerate_displays;
use super::util::*;
use super::window::WindowsWindow;
use crate::config::WindowConfiguration;
use crate::executor::{BackgroundExecutor, ForegroundExecutor};
use crate::shared::PlatformHandlers;
use crate::traits::*;
use flui_types::geometry::{Bounds, DevicePixels, Point, Size};

/// Ensures window class is registered exactly once (sound replacement for `static mut bool`).
static REGISTER_WINDOW_CLASS: std::sync::Once = std::sync::Once::new();

/// Context data stored per window for event dispatch
pub(super) struct WindowContext {
    /// Window ID for event dispatch
    pub window_id: WindowId,
    /// Reference to platform handlers
    pub handlers: Arc<Mutex<PlatformHandlers>>,
    /// Scale factor for coordinate conversion
    pub scale_factor: f32,
    /// Current window mode (replaces display_state + saved bounds)
    pub mode: std::cell::Cell<WindowMode>,
    /// Last known size (before minimization) for restore detection
    pub last_size: std::cell::Cell<Size<DevicePixels>>,
    /// Window configuration (hotkeys, debouncing, etc.)
    pub config: WindowConfiguration,
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

        tracing::info!("Windows platform initialized with Tokio executors");

        Ok(Self {
            message_window,
            windows: Arc::new(Mutex::new(HashMap::new())),
            handlers: Arc::new(Mutex::new(PlatformHandlers::default())),
            background_executor,
            foreground_executor,
            config,
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

                // Dispatch CloseRequested event
                if let Some(ctx) = ctx {
                    ctx.dispatch_event(WindowEvent::CloseRequested {
                        window_id: ctx.window_id,
                    });
                }

                // Let the window handle it
                DestroyWindow(hwnd).ok();
                LRESULT(0)
            }

            WM_DESTROY => {
                tracing::debug!("WM_DESTROY for HWND {:?}", hwnd);

                // Dispatch Closed event
                if let Some(ctx) = ctx {
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

                        // Dispatch RedrawRequested event
                        if let Some(ctx) = ctx {
                            ctx.dispatch_event(WindowEvent::RedrawRequested {
                                window_id: ctx.window_id,
                            });
                        }
                    } else {
                        tracing::trace!("⏭️  Skipping render for minimized window");
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

                    // Dispatch event if any
                    if let Some(event) = event {
                        ctx.dispatch_event(event);
                    }
                }

                LRESULT(0)
            }

            WM_MOVE => {
                let x = get_x_lparam(lparam);
                let y = get_y_lparam(lparam);
                tracing::info!("📍 Window Moved: ({}, {})", x, y);

                // Dispatch Moved event
                if let Some(ctx) = ctx {
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
                let x = get_x_lparam(lparam);
                let y = get_y_lparam(lparam);
                tracing::debug!("🖱️  Mouse Move: ({}, {})", x, y);
                LRESULT(0)
            }

            WM_LBUTTONDOWN => {
                let x = get_x_lparam(lparam);
                let y = get_y_lparam(lparam);
                tracing::info!("🖱️  Left Mouse Button Down at ({}, {})", x, y);
                LRESULT(0)
            }

            WM_RBUTTONDOWN => {
                let x = get_x_lparam(lparam);
                let y = get_y_lparam(lparam);
                tracing::info!("🖱️  Right Mouse Button Down at ({}, {})", x, y);
                LRESULT(0)
            }

            WM_MBUTTONDOWN => {
                let x = get_x_lparam(lparam);
                let y = get_y_lparam(lparam);
                tracing::info!("🖱️  Middle Mouse Button Down at ({}, {})", x, y);
                LRESULT(0)
            }

            WM_LBUTTONUP => {
                let x = get_x_lparam(lparam);
                let y = get_y_lparam(lparam);
                tracing::info!("🖱️  Left Mouse Button Up at ({}, {})", x, y);
                LRESULT(0)
            }

            WM_RBUTTONUP => {
                let x = get_x_lparam(lparam);
                let y = get_y_lparam(lparam);
                tracing::info!("🖱️  Right Mouse Button Up at ({}, {})", x, y);
                LRESULT(0)
            }

            WM_MBUTTONUP => {
                let x = get_x_lparam(lparam);
                let y = get_y_lparam(lparam);
                tracing::info!("🖱️  Middle Mouse Button Up at ({}, {})", x, y);
                LRESULT(0)
            }

            WM_MOUSEWHEEL => {
                let delta = ((wparam.0 as i32) >> 16) as i16;
                let x = get_x_lparam(lparam);
                let y = get_y_lparam(lparam);
                tracing::info!("🖱️  Mouse Wheel: delta={} at ({}, {})", delta, x, y);
                LRESULT(0)
            }

            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let vk = wparam.0 as u16;
                let is_repeat = (lparam.0 & (1 << 30)) != 0;
                tracing::info!("⌨️  Key Down: VK={:#04x} (repeat={})", vk, is_repeat);

                // Check if fullscreen hotkey is pressed (configurable, default F11)
                if let Some(ctx) = ctx {
                    if let Some(hotkey) = ctx.config.fullscreen_hotkey {
                        if vk == hotkey && !is_repeat {
                            tracing::info!(
                                "🔄 Fullscreen hotkey (VK={:#04x}) pressed - toggling fullscreen",
                                hotkey
                            );
                            WindowsWindow::toggle_fullscreen_for_hwnd(hwnd);
                        }
                    }
                }

                LRESULT(0)
            }

            WM_KEYUP | WM_SYSKEYUP => {
                let vk = wparam.0 as u16;
                tracing::info!("⌨️  Key Up: VK={:#04x}", vk);
                LRESULT(0)
            }

            WM_CHAR => {
                let ch = wparam.0 as u32;
                if let Some(c) = char::from_u32(ch) {
                    tracing::info!("⌨️  Char: '{}'", c);
                }
                LRESULT(0)
            }

            WM_SETFOCUS => {
                tracing::info!("🎯 Window Focused");

                // Dispatch FocusChanged event
                if let Some(ctx) = ctx {
                    ctx.dispatch_event(WindowEvent::FocusChanged {
                        window_id: ctx.window_id,
                        focused: true,
                    });
                }

                LRESULT(0)
            }

            WM_KILLFOCUS => {
                tracing::info!("💤 Window Unfocused");

                // Dispatch FocusChanged event
                if let Some(ctx) = ctx {
                    ctx.dispatch_event(WindowEvent::FocusChanged {
                        window_id: ctx.window_id,
                        focused: false,
                    });
                }

                LRESULT(0)
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
        // TODO: Implement DirectWrite text system
        Arc::new(DummyTextSystem)
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

// ==================== Dummy Implementations ====================

struct DummyTextSystem;

impl PlatformTextSystem for DummyTextSystem {}

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
