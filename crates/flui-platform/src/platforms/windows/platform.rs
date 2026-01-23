//! Windows platform implementation

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use parking_lot::Mutex;

use anyhow::{Context, Result};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

use crate::traits::*;
use super::window::WindowsWindow;
use super::util::*;

/// Windows platform window class name
const WINDOW_CLASS_NAME: PCWSTR = w!("FluiWindowClass");

/// Static flag to ensure window class is registered only once
static mut WINDOW_CLASS_REGISTERED: bool = false;

/// Windows platform state
pub struct WindowsPlatform {
    /// Message-only window for platform messages
    message_window: HWND,

    /// All created windows (keyed by HWND)
    windows: Rc<RefCell<HashMap<isize, Rc<WindowsWindow>>>>,

    /// Platform handlers (callbacks from platform to framework)
    handlers: Arc<Mutex<PlatformHandlers>>,

    /// Current DPI awareness
    dpi_awareness: DPI_AWARENESS_CONTEXT,
}

impl WindowsPlatform {
    /// Create a new Windows platform instance
    pub fn new() -> Result<Self> {
        // Initialize COM for drag-and-drop, clipboard, etc.
        unsafe {
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
            CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                .context("Failed to initialize COM")?;
        }

        // Set DPI awareness to per-monitor v2 (best quality)
        let dpi_awareness = unsafe {
            SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
                .context("Failed to set DPI awareness")?;
            GetThreadDpiAwarenessContext()
        };

        // Register window class
        unsafe {
            Self::register_window_class()?;
        }

        // Create message-only window for platform messages
        let message_window = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                WINDOW_CLASS_NAME,
                w!("Flui Platform Message Window"),
                WINDOW_STYLE(0),
                0, 0, 0, 0,
                HWND_MESSAGE, // Message-only window
                None,
                GetModuleHandleW(None).ok(),
                None,
            ).context("Failed to create message window")?
        };

        Ok(Self {
            message_window,
            windows: Rc::new(RefCell::new(HashMap::new())),
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

    /// Run the Windows message loop
    pub fn run(&self) -> Result<()> {
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
        let hwnd_value = window.hwnd().0;

        // Store window
        self.windows.borrow_mut().insert(hwnd_value, window.clone());

        Ok(Box::new(window))
    }

    fn clipboard(&self) -> &dyn crate::Clipboard {
        // TODO: Implement clipboard
        todo!("Windows clipboard not yet implemented")
    }

    fn on_window_event(&self, callback: Box<dyn FnMut(WindowEvent)>) {
        self.handlers.lock().window_event = Some(callback);
    }

    // Note: on_lifecycle_event removed from Platform trait
    // TODO: Add back if needed
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
    window_event: Option<Box<dyn FnMut(WindowEvent)>>,
    lifecycle_event: Option<Box<dyn FnMut(LifecycleEvent)>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_creation() {
        let result = WindowsPlatform::new();
        assert!(result.is_ok(), "Failed to create Windows platform: {:?}", result.err());
    }
}
