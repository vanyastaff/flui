//! macOS window (NSWindow) implementation

use super::view;
use crate::config::WindowConfiguration;
use crate::shared::PlatformHandlers;
use crate::traits::*;
use anyhow::{Context, Result};
use flui_types::geometry::{Bounds, DevicePixels, Pixels, Point, Size};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

use cocoa::appkit::{NSWindow, NSWindowStyleMask, NSBackingStoreType};
use cocoa::base::{id, nil, BOOL, YES, NO};
use cocoa::foundation::NSRect;
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel};
use raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, HasDisplayHandle, HasWindowHandle,
    RawDisplayHandle, RawWindowHandle,
};

/// macOS window wrapper around NSWindow
pub struct MacOSWindow {
    /// Native window handle (NSWindow*)
    ns_window: id,

    /// Window state
    state: Arc<Mutex<WindowState>>,

    /// Reference to all windows
    windows_map: Arc<Mutex<HashMap<u64, Arc<MacOSWindow>>>>,

    /// Platform handlers
    handlers: Arc<Mutex<PlatformHandlers>>,

    /// Window configuration
    _config: WindowConfiguration,
}

unsafe impl Send for MacOSWindow {}
unsafe impl Sync for MacOSWindow {}

/// Mutable window state
struct WindowState {
    /// Current window bounds (logical pixels)
    bounds: Bounds<Pixels>,

    /// Scale factor (1.0 for non-Retina, 2.0 for Retina)
    scale_factor: f64,
}

impl MacOSWindow {
    /// Create a new macOS window
    pub fn new(
        options: WindowOptions,
        windows_map: Arc<Mutex<HashMap<u64, Arc<MacOSWindow>>>>,
        handlers: Arc<Mutex<PlatformHandlers>>,
        config: WindowConfiguration,
    ) -> Result<Arc<Self>> {
        unsafe {
            // Convert logical size to NSRect
            let frame = NSRect::new(
                cocoa::foundation::NSPoint::new(0.0, 0.0),
                cocoa::foundation::NSSize::new(
                    options.size.width.0 as f64,
                    options.size.height.0 as f64,
                ),
            );

            // Build window style mask
            let mut style_mask = NSWindowStyleMask::NSClosableWindowMask
                | NSWindowStyleMask::NSMiniaturizableWindowMask;

            if options.decorated {
                style_mask |= NSWindowStyleMask::NSTitledWindowMask;
            }

            if options.resizable {
                style_mask |= NSWindowStyleMask::NSResizableWindowMask;
            }

            // Create NSWindow
            let ns_window: id = msg_send![class!(NSWindow), alloc];
            let ns_window: id = msg_send![ns_window,
                initWithContentRect: frame
                styleMask: style_mask
                backing: NSBackingStoreType::NSBackingStoreBuffered
                defer: false as i8
            ];

            if ns_window == nil {
                return Err(anyhow::anyhow!("Failed to create NSWindow"));
            }

            // Set window title
            let title = cocoa::foundation::NSString::alloc(nil);
            let title = cocoa::foundation::NSString::init_str(title, &options.title);
            let _: () = msg_send![ns_window, setTitle: title];

            // Get backing scale factor
            let scale: f64 = msg_send![ns_window, backingScaleFactor];

            // Make window visible if requested
            if options.visible {
                let _: () = msg_send![ns_window, makeKeyAndOrderFront: nil];
            }

            // Center window on screen
            let _: () = msg_send![ns_window, center];

            let window = Arc::new(Self {
                ns_window,
                state: Arc::new(Mutex::new(WindowState {
                    bounds: Bounds {
                        origin: Point::new(
                            flui_types::geometry::px(frame.origin.x as f32),
                            flui_types::geometry::px(frame.origin.y as f32),
                        ),
                        size: options.size,
                    },
                    scale_factor: scale,
                })),
                windows_map: Arc::clone(&windows_map),
                handlers: Arc::clone(&handlers),
                _config: config,
            });

            // Create content view for input events
            let content_view = view::create_content_view(
                frame,
                scale,
                Arc::downgrade(&handlers),
            );
            let _: () = msg_send![ns_window, setContentView: content_view];

            // Enable mouse tracking for mouse moved events
            view::enable_mouse_tracking(content_view);

            // Make content view first responder to receive keyboard events
            let _: () = msg_send![ns_window, makeFirstResponder: content_view];

            // Set window delegate for lifecycle events
            let delegate = create_window_delegate(Arc::downgrade(&window));
            let _: () = msg_send![ns_window, setDelegate: delegate];

            // Store in windows map
            let window_id = ns_window as u64;
            windows_map.lock().insert(window_id, Arc::clone(&window));

            tracing::info!(
                "Created NSWindow {:p} with size {}x{} (scale: {})",
                ns_window,
                options.size.width.0,
                options.size.height.0,
                scale
            );

            Ok(window)
        }
    }

    /// Get the NSWindow handle
    pub fn ns_window(&self) -> id {
        self.ns_window
    }
}

impl PlatformWindow for MacOSWindow {
    fn physical_size(&self) -> Size<DevicePixels> {
        let state = self.state.lock();
        let logical = state.bounds.size;
        let scale = state.scale_factor;
        Size::new(
            flui_types::geometry::device_px((logical.width.0 * scale as f32).round()),
            flui_types::geometry::device_px((logical.height.0 * scale as f32).round()),
        )
    }

    fn logical_size(&self) -> Size<Pixels> {
        let state = self.state.lock();
        state.bounds.size
    }

    fn scale_factor(&self) -> f64 {
        let state = self.state.lock();
        state.scale_factor
    }

    fn request_redraw(&self) {
        unsafe {
            // Tell the window's content view to redraw
            let content_view: id = msg_send![self.ns_window, contentView];
            if content_view != nil {
                let _: () = msg_send![content_view, setNeedsDisplay: true];
            }
        }
    }

    fn is_focused(&self) -> bool {
        unsafe {
            let is_key: bool = msg_send![self.ns_window, isKeyWindow];
            is_key
        }
    }

    fn is_visible(&self) -> bool {
        unsafe {
            let is_visible: bool = msg_send![self.ns_window, isVisible];
            is_visible
        }
    }

    fn set_title(&self, title: &str) {
        unsafe {
            let ns_title = cocoa::foundation::NSString::alloc(nil);
            let ns_title = cocoa::foundation::NSString::init_str(ns_title, title);
            let _: () = msg_send![self.ns_window, setTitle: ns_title];
        }
    }

    fn set_size(&self, size: Size<Pixels>) {
        unsafe {
            let frame: NSRect = msg_send![self.ns_window, frame];
            let new_frame = NSRect::new(
                frame.origin,
                cocoa::foundation::NSSize::new(
                    size.width.0 as f64,
                    size.height.0 as f64,
                ),
            );
            let _: () = msg_send![self.ns_window, setFrame: new_frame display: true];

            // Update state
            let mut state = self.state.lock();
            state.bounds.size = size;
        }
    }

    fn close(&self) {
        unsafe {
            let _: () = msg_send![self.ns_window, close];
        }
    }
}

// Implement raw-window-handle for wgpu integration
impl HasWindowHandle for MacOSWindow {
    fn window_handle(&self) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        use std::ptr::NonNull;

        let ns_window = self.ns_window as *mut std::ffi::c_void;
        let handle = AppKitWindowHandle::new(NonNull::new(ns_window).unwrap());

        Ok(unsafe {
            raw_window_handle::WindowHandle::borrow_raw(RawWindowHandle::AppKit(handle))
        })
    }
}

impl HasDisplayHandle for MacOSWindow {
    fn display_handle(&self) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        let handle = AppKitDisplayHandle::new();
        Ok(unsafe {
            raw_window_handle::DisplayHandle::borrow_raw(RawDisplayHandle::AppKit(handle))
        })
    }
}

impl Clone for MacOSWindow {
    fn clone(&self) -> Self {
        Self {
            ns_window: self.ns_window,
            state: Arc::clone(&self.state),
            windows_map: Arc::clone(&self.windows_map),
            handlers: Arc::clone(&self.handlers),
            _config: self._config.clone(),
        }
    }
}

impl Drop for MacOSWindow {
    fn drop(&mut self) {
        // Only cleanup if this is the last reference
        if Arc::strong_count(&self.state) == 1 {
            tracing::debug!("Closing NSWindow {:p}", self.ns_window);

            // Remove from windows map
            let window_id = self.ns_window as u64;
            self.windows_map.lock().remove(&window_id);

            unsafe {
                // Release NSWindow (will be deallocated by Cocoa)
                let _: () = msg_send![self.ns_window, release];
            }
        }
    }
}

// ============================================================================
// NSWindowDelegate Implementation
// ============================================================================

use std::sync::Weak;

/// Create a window delegate for lifecycle events
fn create_window_delegate(window: Weak<MacOSWindow>) -> id {
    unsafe {
        // Get or create delegate class
        let class = get_or_create_delegate_class();
        let delegate: id = msg_send![class, alloc];
        let delegate: id = msg_send![delegate, init];

        // Store weak pointer to window
        let window_ptr = Box::into_raw(Box::new(window)) as *mut std::ffi::c_void;
        (*delegate).set_ivar("window_ptr", window_ptr);

        delegate
    }
}

/// Get or create the NSWindowDelegate class
fn get_or_create_delegate_class() -> &'static Class {
    use std::sync::Once;
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        let superclass = class!(NSObject);
        let mut decl = ClassDecl::new("FLUIWindowDelegate", superclass).unwrap();

        // Add ivar to store window pointer
        decl.add_ivar::<*mut std::ffi::c_void>("window_ptr");

        // windowDidResize:
        unsafe extern "C" fn window_did_resize(this: &Object, _sel: Sel, _notification: id) {
            if let Some(window) = get_window_from_delegate(this) {
                window.handle_resize();
            }
        }

        // windowDidMove:
        unsafe extern "C" fn window_did_move(this: &Object, _sel: Sel, _notification: id) {
            if let Some(window) = get_window_from_delegate(this) {
                window.handle_move();
            }
        }

        // windowDidBecomeKey:
        unsafe extern "C" fn window_did_become_key(this: &Object, _sel: Sel, _notification: id) {
            if let Some(window) = get_window_from_delegate(this) {
                window.handle_focus_gained();
            }
        }

        // windowDidResignKey:
        unsafe extern "C" fn window_did_resign_key(this: &Object, _sel: Sel, _notification: id) {
            if let Some(window) = get_window_from_delegate(this) {
                window.handle_focus_lost();
            }
        }

        // windowShouldClose:
        unsafe extern "C" fn window_should_close(this: &Object, _sel: Sel, _sender: id) -> BOOL {
            if let Some(window) = get_window_from_delegate(this) {
                if window.handle_close_request() {
                    YES
                } else {
                    NO
                }
            } else {
                YES
            }
        }

        // windowWillClose:
        unsafe extern "C" fn window_will_close(this: &Object, _sel: Sel, _notification: id) {
            if let Some(window) = get_window_from_delegate(this) {
                window.handle_close();
            }
        }

        // windowDidChangeBackingProperties: (Retina/DPI change)
        unsafe extern "C" fn window_did_change_backing_properties(
            this: &Object,
            _sel: Sel,
            _notification: id,
        ) {
            if let Some(window) = get_window_from_delegate(this) {
                window.handle_backing_properties_changed();
            }
        }

        // windowDidChangeScreen: (moved to different monitor)
        unsafe extern "C" fn window_did_change_screen(this: &Object, _sel: Sel, _notification: id) {
            if let Some(window) = get_window_from_delegate(this) {
                window.handle_screen_changed();
            }
        }

        // Add methods
        unsafe {
            decl.add_method(
                sel!(windowDidResize:),
                window_did_resize as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(windowDidMove:),
                window_did_move as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(windowDidBecomeKey:),
                window_did_become_key as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(windowDidResignKey:),
                window_did_resign_key as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(windowShouldClose:),
                window_should_close as extern "C" fn(&Object, Sel, id) -> BOOL,
            );
            decl.add_method(
                sel!(windowWillClose:),
                window_will_close as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(windowDidChangeBackingProperties:),
                window_did_change_backing_properties as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(windowDidChangeScreen:),
                window_did_change_screen as extern "C" fn(&Object, Sel, id),
            );
        }

        decl.register();
    });

    Class::get("FLUIWindowDelegate").unwrap()
}

/// Get window from delegate
unsafe fn get_window_from_delegate(delegate: &Object) -> Option<Arc<MacOSWindow>> {
    let window_ptr: *mut std::ffi::c_void = *delegate.get_ivar("window_ptr");
    let weak_ptr = window_ptr as *mut Weak<MacOSWindow>;
    if weak_ptr.is_null() {
        return None;
    }
    (*weak_ptr).upgrade()
}

// ============================================================================
// Window Event Handlers
// ============================================================================

impl MacOSWindow {
    /// Handle window resize event
    fn handle_resize(&self) {
        unsafe {
            let frame: NSRect = msg_send![self.ns_window, frame];
            let content_rect: NSRect = msg_send![self.ns_window, contentRectForFrameRect: frame];

            let new_size = Size::new(
                flui_types::geometry::px(content_rect.size.width as f32),
                flui_types::geometry::px(content_rect.size.height as f32),
            );

            // Update state
            {
                let mut state = self.state.lock();
                state.bounds.size = new_size;
            }

            // Notify handlers
            let handlers = self.handlers.lock();
            if let Some(handler) = &handlers.on_resize {
                handler(new_size);
            }

            tracing::debug!(
                "Window resized to {}x{}",
                new_size.width.0,
                new_size.height.0
            );
        }
    }

    /// Handle window move event
    fn handle_move(&self) {
        unsafe {
            let frame: NSRect = msg_send![self.ns_window, frame];

            let new_origin = Point::new(
                flui_types::geometry::px(frame.origin.x as f32),
                flui_types::geometry::px(frame.origin.y as f32),
            );

            // Update state
            {
                let mut state = self.state.lock();
                state.bounds.origin = new_origin;
            }

            tracing::debug!(
                "Window moved to ({}, {})",
                new_origin.x.0,
                new_origin.y.0
            );
        }
    }

    /// Handle focus gained event
    fn handle_focus_gained(&self) {
        let handlers = self.handlers.lock();
        if let Some(handler) = &handlers.on_active {
            handler();
        }
        tracing::debug!("Window gained focus");
    }

    /// Handle focus lost event
    fn handle_focus_lost(&self) {
        let handlers = self.handlers.lock();
        if let Some(handler) = &handlers.on_inactive {
            handler();
        }
        tracing::debug!("Window lost focus");
    }

    /// Handle close request event
    ///
    /// Returns true to allow close, false to prevent
    fn handle_close_request(&self) -> bool {
        let handlers = self.handlers.lock();
        if let Some(handler) = &handlers.should_close {
            let should_close = handler();
            tracing::debug!("Window close requested: {}", should_close);
            should_close
        } else {
            true // Allow close by default
        }
    }

    /// Handle window close event
    fn handle_close(&self) {
        let handlers = self.handlers.lock();
        if let Some(handler) = &handlers.on_close {
            handler();
        }
        tracing::debug!("Window closed");
    }

    /// Handle backing properties changed (Retina/DPI change)
    fn handle_backing_properties_changed(&self) {
        unsafe {
            let new_scale: f64 = msg_send![self.ns_window, backingScaleFactor];

            // Update window state
            {
                let mut state = self.state.lock();
                if (state.scale_factor - new_scale).abs() > 0.01 {
                    state.scale_factor = new_scale;
                    tracing::info!("Window scale factor changed to {}", new_scale);
                }
            }

            // Update content view scale factor
            let content_view: id = msg_send![self.ns_window, contentView];
            if content_view != nil {
                view::update_view_scale_factor(content_view, new_scale);
            }
        }
    }

    /// Handle screen changed (moved to different monitor)
    fn handle_screen_changed(&self) {
        unsafe {
            let screen: id = msg_send![self.ns_window, screen];
            if screen != nil {
                let scale: f64 = msg_send![screen, backingScaleFactor];
                tracing::debug!("Window moved to screen with scale factor {}", scale);

                // Update scale factor (will trigger backing properties changed)
                self.handle_backing_properties_changed();
            }
        }
    }
}
