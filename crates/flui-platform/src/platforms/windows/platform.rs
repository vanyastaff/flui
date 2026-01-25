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

use super::util::*;
use super::window::WindowsWindow;
use crate::traits::*;

/// Windows platform window class name
const WINDOW_CLASS_NAME: PCWSTR = w!("FluiWindowClass");

/// Static flag to ensure window class is registered only once
static mut WINDOW_CLASS_REGISTERED: bool = false;

/// Windows platform state
pub struct WindowsPlatform {
    /// Message-only window for platform messages
    message_window: HWND,

    /// All created windows (keyed by HWND)
    windows: Arc<Mutex<HashMap<isize, Arc<WindowsWindow>>>>,

    /// Platform handlers (callbacks from platform to framework)
    handlers: Arc<Mutex<PlatformHandlers>>,

    /// Current DPI awareness
    dpi_awareness: DPI_AWARENESS_CONTEXT,
}

// SAFETY: HWND and DPI_AWARENESS_CONTEXT are just integer handles and are safe to send/share between threads.
// Windows API handles are thread-safe by design.
unsafe impl Send for WindowsPlatform {}
unsafe impl Sync for WindowsPlatform {}

impl WindowsPlatform {
    /// Create a new Windows platform instance
    pub fn new() -> Result<Self> {
        // Initialize COM for drag-and-drop, clipboard, etc.
        unsafe {
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
            let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            if hr.is_err() {
                return Err(anyhow::anyhow!("Failed to initialize COM: {:?}", hr));
            }
        }

        // Set DPI awareness to per-monitor v2 (best quality)
        let dpi_awareness = unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            // Ignore errors - this can fail if already set or on older Windows
            GetThreadDpiAwarenessContext()
        };

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
                HWND_MESSAGE, // Message-only window
                None,
                hinstance,
                None,
            )
            .map_err(|e| anyhow::anyhow!("Failed to create message window: {:?}", e))?
        };

        Ok(Self {
            message_window,
            windows: Arc::new(Mutex::new(HashMap::new())),
            handlers: Arc::new(Mutex::new(PlatformHandlers::default())),
            dpi_awareness,
        })
    }

    /// Register the window class for all FLUI windows
    unsafe fn register_window_class() -> Result<()> {
        if WINDOW_CLASS_REGISTERED {
            return Ok(());
        }

        let hinstance = GetModuleHandleW(None).context("Failed to get module handle")?;

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW | CS_OWNDC,
            lpfnWndProc: Some(Self::window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance.into(),
            hIcon: HICON::default(),
            hCursor: load_cursor_style(IDC_ARROW)?,
            hbrBackground: HBRUSH::default(),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: WINDOW_CLASS_NAME,
        };

        let atom = RegisterClassW(&wc);
        if atom == 0 {
            return Err(windows::core::Error::from_win32().into());
        }

        WINDOW_CLASS_REGISTERED = true;
        tracing::info!("Registered Windows window class");

        Ok(())
    }

    /// Main window procedure for all FLUI windows
    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_CREATE => {
                tracing::debug!("WM_CREATE for HWND {:?}", hwnd);
                LRESULT(0)
            }

            WM_CLOSE => {
                tracing::debug!("WM_CLOSE for HWND {:?}", hwnd);
                // Let the window handle it
                DestroyWindow(hwnd).ok();
                LRESULT(0)
            }

            WM_DESTROY => {
                tracing::debug!("WM_DESTROY for HWND {:?}", hwnd);
                // TODO: Remove window from windows map
                LRESULT(0)
            }

            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                if !hdc.is_invalid() {
                    // TODO: Trigger paint callback
                    EndPaint(hwnd, &ps);
                }
                LRESULT(0)
            }

            WM_SIZE => {
                let width = get_x_lparam(lparam);
                let height = get_y_lparam(lparam);
                tracing::trace!("WM_SIZE: {}x{}", width, height);
                // TODO: Handle resize
                LRESULT(0)
            }

            WM_MOVE => {
                let x = get_x_lparam(lparam);
                let y = get_y_lparam(lparam);
                tracing::trace!("WM_MOVE: ({}, {})", x, y);
                // TODO: Handle move
                LRESULT(0)
            }

            WM_DPICHANGED => {
                tracing::debug!("WM_DPICHANGED");
                // TODO: Handle DPI change
                LRESULT(0)
            }

            WM_MOUSEMOVE => {
                // TODO: Handle mouse move
                LRESULT(0)
            }

            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN => {
                // TODO: Handle mouse button down
                LRESULT(0)
            }

            WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP => {
                // TODO: Handle mouse button up
                LRESULT(0)
            }

            WM_MOUSEWHEEL => {
                // TODO: Handle mouse wheel
                LRESULT(0)
            }

            WM_KEYDOWN | WM_SYSKEYDOWN => {
                // TODO: Handle key down
                LRESULT(0)
            }

            WM_KEYUP | WM_SYSKEYUP => {
                // TODO: Handle key up
                LRESULT(0)
            }

            WM_CHAR => {
                // TODO: Handle character input
                LRESULT(0)
            }

            WM_SETFOCUS => {
                tracing::debug!("WM_SETFOCUS");
                // TODO: Handle focus gained
                LRESULT(0)
            }

            WM_KILLFOCUS => {
                tracing::debug!("WM_KILLFOCUS");
                // TODO: Handle focus lost
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
                TranslateMessage(&msg);
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
        // TODO: Implement proper executor
        Arc::new(DummyExecutor)
    }

    fn foreground_executor(&self) -> Arc<dyn PlatformExecutor> {
        // TODO: Implement proper executor
        Arc::new(DummyExecutor)
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
        // TODO: Enumerate monitors
        vec![]
    }

    fn primary_display(&self) -> Option<Arc<dyn PlatformDisplay>> {
        // TODO: Get primary monitor
        None
    }

    fn open_window(&self, options: WindowOptions) -> Result<Box<dyn PlatformWindow>> {
        tracing::info!("Opening window: {:?}", options.title);

        let window = WindowsWindow::new(options, self.windows.clone())?;
        let hwnd_value = window.hwnd().0 as isize;

        // Store window
        self.windows.lock().insert(hwnd_value, window.clone());

        Ok(Box::new(window))
    }

    // ==================== Input & Clipboard ====================

    fn clipboard(&self) -> Arc<dyn Clipboard> {
        // TODO: Implement Windows clipboard
        Arc::new(DummyClipboard)
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
            let mut buffer = vec![0u16; 512];
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

/// Platform-specific handlers
#[derive(Default)]
struct PlatformHandlers {
    window_event: Option<Box<dyn FnMut(WindowEvent) + Send>>,
    quit: Option<Box<dyn FnMut() + Send>>,
}

// ==================== Dummy Implementations ====================

struct DummyExecutor;

impl PlatformExecutor for DummyExecutor {
    fn spawn(&self, task: Box<dyn FnOnce() + Send>) {
        // TODO: Use proper thread pool
        std::thread::spawn(task);
    }
}

struct DummyTextSystem;

impl PlatformTextSystem for DummyTextSystem {}

struct DummyClipboard;

impl Clipboard for DummyClipboard {
    fn read_text(&self) -> Option<String> {
        None
    }

    fn write_text(&self, _text: String) {
        // No-op
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
