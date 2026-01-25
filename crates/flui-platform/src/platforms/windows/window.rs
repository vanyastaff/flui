//! Windows window implementation

use parking_lot::Mutex;
use std::collections::HashMap;
use std::ptr::NonNull;
use std::sync::Arc;

use anyhow::{Context, Result};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use raw_window_handle::{Win32WindowHandle, WindowsDisplayHandle};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::util::{logical_to_device, USER_DEFAULT_SCREEN_DPI};
use crate::traits::*;
use flui_types::geometry::{device_px, px, Bounds, DevicePixels, Pixels, Point, Size};

const WINDOW_CLASS_NAME: PCWSTR = windows::core::w!("FluiWindowClass");

/// Windows window wrapper
pub struct WindowsWindow {
    /// Native window handle
    hwnd: HWND,

    /// Window state
    state: Arc<Mutex<WindowState>>,

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
                hinstance,
                None, // lpParam
            )
            .context("Failed to create window")?;

            if hwnd.is_invalid() {
                return Err(windows::core::Error::from_win32().into());
            }

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
                windows_map,
            });

            // Show window if requested
            if options.visible {
                ShowWindow(hwnd, SW_SHOW);
                UpdateWindow(hwnd).ok();
                window.state.lock().visible = true;
            }

            Ok(window)
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
        self.as_ref().request_redraw()
    }

    fn is_focused(&self) -> bool {
        self.as_ref().is_focused()
    }

    fn is_visible(&self) -> bool {
        self.as_ref().is_visible()
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
            InvalidateRect(self.hwnd, None, false).ok();
        }
    }

    fn is_focused(&self) -> bool {
        self.state.lock().focused
    }

    fn is_visible(&self) -> bool {
        self.state.lock().visible
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
            windows_map: Arc::clone(&self.windows_map),
        }
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
        let result = WindowsWindow::new(options, windows_map);

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
