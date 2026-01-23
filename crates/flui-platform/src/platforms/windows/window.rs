//! Windows window implementation

use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::NonNull;
use std::rc::Rc;

use anyhow::{Context, Result};
use raw_window_handle::{HasWindowHandle, HasDisplayHandle, RawWindowHandle, RawDisplayHandle};
use raw_window_handle::{Win32WindowHandle, WindowsDisplayHandle};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{HSTRING, PCWSTR};

use flui_types::geometry::{Bounds, DevicePixels, Pixels, Size, device_px};
use crate::traits::*;
use super::util::{logical_to_device, USER_DEFAULT_SCREEN_DPI};

const WINDOW_CLASS_NAME: PCWSTR = windows::core::w!("FluiWindowClass");

/// Windows window wrapper
pub struct WindowsWindow {
    /// Native window handle
    hwnd: HWND,

    /// Window state
    state: Rc<RefCell<WindowState>>,

    /// Reference to platform's window map (for cleanup)
    windows_map: Rc<RefCell<HashMap<isize, Rc<WindowsWindow>>>>,
}

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
        windows_map: Rc<RefCell<HashMap<isize, Rc<WindowsWindow>>>>,
    ) -> Result<Rc<Self>> {
        unsafe {
            let hinstance = GetModuleHandleW(None).context("Failed to get module handle")?;

            // Get DPI for initial size calculation
            let dpi = GetDpiForSystem();
            let scale_factor = dpi as f32 / USER_DEFAULT_SCREEN_DPI as f32;

            // Convert logical size to device pixels
            let width = logical_to_device(options.bounds.size.width.0, scale_factor);
            let height = logical_to_device(options.bounds.size.height.0, scale_factor);
            let x = logical_to_device(options.bounds.origin.x.0, scale_factor);
            let y = logical_to_device(options.bounds.origin.y.0, scale_factor);

            // Determine window style
            let style = if options.decorations == Decorations::Client {
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
            ).context("Failed to create window")?;

            if hwnd.is_invalid() {
                return Err(windows::core::Error::from_win32().into());
            }

            tracing::info!(
                "Created window HWND {:?} - {}x{} at ({}, {}) - scale: {}",
                hwnd, width, height, x, y, scale_factor
            );

            // Create window state
            let state = Rc::new(RefCell::new(WindowState {
                bounds: options.bounds,
                scale_factor,
                visible: false,
                focused: false,
                title: options.title.clone(),
            }));

            let window = Rc::new(Self {
                hwnd,
                state,
                windows_map,
            });

            // Show window if requested
            if options.visible {
                ShowWindow(hwnd, SW_SHOW);
                UpdateWindow(hwnd).ok();
                window.state.borrow_mut().visible = true;
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
        self.state.borrow().bounds
    }

    /// Get current scale factor
    pub fn scale_factor(&self) -> f32 {
        self.state.borrow().scale_factor
    }
}

impl PlatformWindow for WindowsWindow {
    fn physical_size(&self) -> Size<DevicePixels> {
        let logical = self.state.borrow().bounds.size;
        let scale = self.state.borrow().scale_factor;
        Size::new(
            device_px(logical_to_device(logical.width.0, scale)),
            device_px(logical_to_device(logical.height.0, scale)),
        )
    }

    fn logical_size(&self) -> Size<Pixels> {
        self.state.borrow().bounds.size
    }

    fn scale_factor(&self) -> f64 {
        self.state.borrow().scale_factor as f64
    }

    fn request_redraw(&self) {
        unsafe {
            InvalidateRect(self.hwnd, None, false).ok();
        }
    }

    fn is_focused(&self) -> bool {
        self.state.borrow().focused
    }

    fn is_visible(&self) -> bool {
        self.state.borrow().visible
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// Implement raw-window-handle for wgpu integration
impl HasWindowHandle for WindowsWindow {
    fn window_handle(&self) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let mut handle = Win32WindowHandle::new(
            NonNull::new(self.hwnd.0 as *mut std::ffi::c_void)
                .ok_or(raw_window_handle::HandleError::Unavailable)?
        );

        unsafe {
            let hinstance = GetModuleHandleW(None)
                .map_err(|_| raw_window_handle::HandleError::Unavailable)?;
            handle.hinstance = NonNull::new(hinstance.0 as *mut std::ffi::c_void);
        }

        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(RawWindowHandle::Win32(handle)) })
    }
}

impl HasDisplayHandle for WindowsWindow {
    fn display_handle(&self) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        let handle = WindowsDisplayHandle::new();
        Ok(unsafe { raw_window_handle::DisplayHandle::borrow_raw(RawDisplayHandle::Windows(handle)) })
    }
}

impl Clone for WindowsWindow {
    fn clone(&self) -> Self {
        Self {
            hwnd: self.hwnd,
            state: self.state.clone(),
            windows_map: self.windows_map.clone(),
        }
    }
}

impl Drop for WindowsWindow {
    fn drop(&mut self) {
        // Only destroy if this is the last reference
        if Rc::strong_count(&self.state) == 1 {
            tracing::debug!("Destroying window HWND {:?}", self.hwnd);

            unsafe {
                if !self.hwnd.is_invalid() {
                    DestroyWindow(self.hwnd).ok();
                }
            }

            // Remove from windows map
            self.windows_map.borrow_mut().remove(&self.hwnd.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_creation() {
        let options = WindowOptions {
            bounds: Bounds {
                origin: Point::new(px(100.0), px(100.0)),
                size: Size::new(px(800.0), px(600.0)),
            },
            title: "Test Window".to_string(),
            visible: false,
            decorations: Decorations::Client,
            ..Default::default()
        };

        let windows_map = Rc::new(RefCell::new(HashMap::new()));
        let result = WindowsWindow::new(options, windows_map);

        assert!(result.is_ok(), "Failed to create window: {:?}", result.err());

        let window = result.unwrap();
        assert!(!window.hwnd().is_invalid());
        assert_eq!(window.logical_size().width.0, 800.0);
    }
}
