//! macOS window (NSWindow) implementation

use std::{collections::HashMap, sync::Arc};

use anyhow::Result;
use cocoa::{
    appkit::{NSBackingStoreType, NSWindowStyleMask},
    base::{BOOL, NO, YES, id, nil},
    foundation::NSRect,
};
use flui_types::geometry::{Bounds, DevicePixels, Pixels, Point, Size};
use objc::{
    class,
    declare::ClassDecl,
    msg_send,
    runtime::{Class, Object, Sel},
    sel, sel_impl,
};
use parking_lot::Mutex;
use raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle,
};

use super::view;
use crate::{
    config::WindowConfiguration,
    shared::WindowCallbacks,
    traits::{DispatchEventResult, PlatformInput, PlatformWindow, WindowOptions},
};

/// macOS window wrapper around NSWindow
pub struct MacOSWindow {
    /// Native window handle (NSWindow*)
    ns_window: id,

    /// Window state
    state: Arc<Mutex<MacOSWindowState>>,

    /// Reference to all windows
    windows_map: Arc<Mutex<HashMap<u64, Arc<MacOSWindow>>>>,

    /// Per-window callbacks (input, resize, close, ...)
    callbacks: Arc<WindowCallbacks>,

    /// Window configuration
    _config: WindowConfiguration,
}

// SAFETY: the NSWindow pointer is only messaged from the main thread (AppKit
// delivers all delegate/view callbacks there); the remaining fields are
// `Arc`/`Mutex`-protected. Sharing the wrapper across threads is required by
// the `PlatformWindow: Send + Sync` contract.
unsafe impl Send for MacOSWindow {}
// SAFETY: see `Send` above — interior mutability is Mutex-guarded and the raw
// pointer is main-thread-affine by AppKit convention.
unsafe impl Sync for MacOSWindow {}

/// Mutable window state
struct MacOSWindowState {
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
        config: WindowConfiguration,
    ) -> Result<Arc<Self>> {
        // SAFETY: must run on the main thread (enforced by the platform's
        // event-loop ownership); all messaged objects are alive: the freshly
        // allocated NSWindow is checked for nil before further use.
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
                defer: NO
            ];

            if ns_window == nil {
                return Err(anyhow::anyhow!("Failed to create NSWindow"));
            }

            // Set window title
            let title = cocoa::foundation::NSString::alloc(nil);
            let title = cocoa::foundation::NSString::init_str(title, &options.title);
            let _: () = msg_send![ns_window, setTitle: title];

            // Apply size constraints
            if let Some(min) = options.min_size {
                let ns_size =
                    cocoa::foundation::NSSize::new(min.width.0 as f64, min.height.0 as f64);
                let _: () = msg_send![ns_window, setMinSize: ns_size];
            }
            if let Some(max) = options.max_size {
                let ns_size =
                    cocoa::foundation::NSSize::new(max.width.0 as f64, max.height.0 as f64);
                let _: () = msg_send![ns_window, setMaxSize: ns_size];
            }

            // Get backing scale factor
            let scale: f64 = msg_send![ns_window, backingScaleFactor];

            // Make window visible if requested
            if options.visible {
                let _: () = msg_send![ns_window, makeKeyAndOrderFront: nil];
            }

            // Center window on screen
            let _: () = msg_send![ns_window, center];

            let callbacks = Arc::new(WindowCallbacks::new());

            let window = Arc::new(Self {
                ns_window,
                state: Arc::new(Mutex::new(MacOSWindowState {
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
                callbacks,
                _config: config,
            });

            // Create content view for input events
            let content_view =
                view::create_content_view(frame, scale, Arc::downgrade(&window.callbacks));
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

    /// Get the per-window callbacks registry
    pub fn callbacks(&self) -> &Arc<WindowCallbacks> {
        &self.callbacks
    }
}

impl PlatformWindow for MacOSWindow {
    fn physical_size(&self) -> Size<DevicePixels> {
        let state = self.state.lock();
        let logical = state.bounds.size;
        let scale = state.scale_factor as f32;
        Size::new(
            flui_types::geometry::device_px((logical.width.0 * scale).round() as i32),
            flui_types::geometry::device_px((logical.height.0 * scale).round() as i32),
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
        // SAFETY: `ns_window` is alive for the lifetime of `self`; the
        // content view is nil-checked before messaging.
        unsafe {
            // Tell the window's content view to redraw
            let content_view: id = msg_send![self.ns_window, contentView];
            if content_view != nil {
                let _: () = msg_send![content_view, setNeedsDisplay: YES];
            }
        }
    }

    fn is_focused(&self) -> bool {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let is_key: bool = msg_send![self.ns_window, isKeyWindow];
            is_key
        }
    }

    fn is_visible(&self) -> bool {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let is_visible: bool = msg_send![self.ns_window, isVisible];
            is_visible
        }
    }

    fn bounds(&self) -> Bounds<Pixels> {
        self.state.lock().bounds
    }

    fn get_title(&self) -> String {
        // SAFETY: `ns_window` is alive; `title` returns an autoreleased
        // NSString whose UTF8String buffer is copied before returning.
        unsafe {
            let ns_title: id = msg_send![self.ns_window, title];
            if ns_title == nil {
                return String::new();
            }
            let c_str: *const i8 = msg_send![ns_title, UTF8String];
            if c_str.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(c_str)
                    .to_string_lossy()
                    .into_owned()
            }
        }
    }

    fn set_title(&self, title: &str) {
        // SAFETY: `ns_window` is alive; the NSString is created from a valid
        // Rust string and ownership passes to the window.
        unsafe {
            let ns_title = cocoa::foundation::NSString::alloc(nil);
            let ns_title = cocoa::foundation::NSString::init_str(ns_title, title);
            let _: () = msg_send![self.ns_window, setTitle: ns_title];
        }
    }

    fn activate(&self) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let _: () = msg_send![self.ns_window, makeKeyAndOrderFront: nil];
        }
    }

    fn minimize(&self) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let _: () = msg_send![self.ns_window, miniaturize: nil];
        }
    }

    fn maximize(&self) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let is_zoomed: bool = msg_send![self.ns_window, isZoomed];
            if !is_zoomed {
                let _: () = msg_send![self.ns_window, zoom: nil];
            }
        }
    }

    fn restore(&self) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let is_minimized: bool = msg_send![self.ns_window, isMiniaturized];
            if is_minimized {
                let _: () = msg_send![self.ns_window, deminiaturize: nil];
            }
            let is_zoomed: bool = msg_send![self.ns_window, isZoomed];
            if is_zoomed {
                let _: () = msg_send![self.ns_window, zoom: nil];
            }
        }
    }

    fn toggle_fullscreen(&self) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let _: () = msg_send![self.ns_window, toggleFullScreen: nil];
        }
    }

    fn resize(&self, size: Size<Pixels>) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let frame: NSRect = msg_send![self.ns_window, frame];
            let new_frame = NSRect::new(
                frame.origin,
                cocoa::foundation::NSSize::new(size.width.0 as f64, size.height.0 as f64),
            );
            let _: () = msg_send![self.ns_window, setFrame: new_frame display: YES];

            // Update state
            let mut state = self.state.lock();
            state.bounds.size = size;
        }
    }

    fn close(&self) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let _: () = msg_send![self.ns_window, close];
        }
    }

    // ==================== Callback Registration ====================

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

/// Delegating impl so `Arc<MacOSWindow>` can be boxed as a `PlatformWindow`
/// (mirrors the Windows backend).
impl PlatformWindow for Arc<MacOSWindow> {
    fn physical_size(&self) -> Size<DevicePixels> {
        self.as_ref().physical_size()
    }

    fn logical_size(&self) -> Size<Pixels> {
        self.as_ref().logical_size()
    }

    fn scale_factor(&self) -> f64 {
        PlatformWindow::scale_factor(self.as_ref())
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

    fn bounds(&self) -> Bounds<Pixels> {
        PlatformWindow::bounds(self.as_ref())
    }

    fn get_title(&self) -> String {
        PlatformWindow::get_title(self.as_ref())
    }

    fn set_title(&self, title: &str) {
        PlatformWindow::set_title(self.as_ref(), title)
    }

    fn activate(&self) {
        PlatformWindow::activate(self.as_ref())
    }

    fn minimize(&self) {
        PlatformWindow::minimize(self.as_ref())
    }

    fn maximize(&self) {
        PlatformWindow::maximize(self.as_ref())
    }

    fn restore(&self) {
        PlatformWindow::restore(self.as_ref())
    }

    fn toggle_fullscreen(&self) {
        PlatformWindow::toggle_fullscreen(self.as_ref())
    }

    fn resize(&self, size: Size<Pixels>) {
        PlatformWindow::resize(self.as_ref(), size)
    }

    fn close(&self) {
        PlatformWindow::close(self.as_ref())
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

    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        PlatformWindow::window_handle(self.as_ref())
    }

    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        PlatformWindow::display_handle(self.as_ref())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self.as_ref()
    }
}

// Implement raw-window-handle for wgpu integration
impl HasWindowHandle for MacOSWindow {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        use std::ptr::NonNull;

        // raw-window-handle 0.6 AppKitWindowHandle expects the NSView, not
        // the NSWindow.
        // SAFETY: `ns_window` is alive for the lifetime of `self`; the
        // returned handle borrows `self`, so the view outlives it.
        let content_view: id = unsafe { msg_send![self.ns_window, contentView] };
        let ns_view = NonNull::new(content_view as *mut std::ffi::c_void)
            .ok_or(raw_window_handle::HandleError::Unavailable)?;
        let handle = AppKitWindowHandle::new(ns_view);

        // SAFETY: the handle is valid for the lifetime of `&self` (the view
        // is retained by the window, which `self` keeps alive).
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(RawWindowHandle::AppKit(handle)) })
    }
}

impl HasDisplayHandle for MacOSWindow {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        let handle = AppKitDisplayHandle::new();
        // SAFETY: AppKit display handles carry no pointer; always valid.
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
            callbacks: Arc::clone(&self.callbacks),
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

            // SAFETY: this is the last wrapper referencing the NSWindow we
            // alloc-init'ed in `new`, so releasing our +1 retain is balanced.
            unsafe {
                let _: () = msg_send![self.ns_window, release];
            }
        }
    }
}

// ============================================================================
// Cross-Platform Window Trait Implementation
// ============================================================================

use crate::window::{
    RawWindowHandle as CrossRawWindowHandle, Window as WindowTrait, WindowId as CrossWindowId,
    WindowState,
};

impl WindowTrait for MacOSWindow {
    fn id(&self) -> CrossWindowId {
        CrossWindowId::new(self.ns_window as u64)
    }

    fn title(&self) -> String {
        PlatformWindow::get_title(self)
    }

    fn set_title(&mut self, title: &str) {
        PlatformWindow::set_title(self, title)
    }

    fn position(&self) -> Point<Pixels> {
        let state = self.state.lock();
        state.bounds.origin
    }

    fn set_position(&mut self, position: Point<Pixels>) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let frame: NSRect = msg_send![self.ns_window, frame];
            let new_frame = NSRect::new(
                cocoa::foundation::NSPoint::new(position.x.0 as f64, position.y.0 as f64),
                frame.size,
            );
            let _: () = msg_send![self.ns_window, setFrame: new_frame display: YES];

            // Update state
            let mut state = self.state.lock();
            state.bounds.origin = position;
        }
    }

    fn size(&self) -> Size<Pixels> {
        let state = self.state.lock();
        state.bounds.size
    }

    fn set_size(&mut self, size: Size<Pixels>) {
        PlatformWindow::resize(self, size)
    }

    fn state(&self) -> WindowState {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let is_minimized: bool = msg_send![self.ns_window, isMiniaturized];
            let is_zoomed: bool = msg_send![self.ns_window, isZoomed];
            let style_mask: cocoa::appkit::NSWindowStyleMask = msg_send![self.ns_window, styleMask];

            if is_minimized {
                WindowState::Minimized
            } else if style_mask.contains(NSWindowStyleMask::NSFullScreenWindowMask) {
                WindowState::Fullscreen
            } else if is_zoomed {
                WindowState::Maximized
            } else {
                WindowState::Normal
            }
        }
    }

    fn set_state(&mut self, state: WindowState) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            match state {
                WindowState::Normal => {
                    // Restore from minimized
                    let is_minimized: bool = msg_send![self.ns_window, isMiniaturized];
                    if is_minimized {
                        let _: () = msg_send![self.ns_window, deminiaturize: nil];
                    }

                    // Restore from maximized
                    let is_zoomed: bool = msg_send![self.ns_window, isZoomed];
                    if is_zoomed {
                        let _: () = msg_send![self.ns_window, zoom: nil];
                    }

                    // Exit fullscreen
                    let style_mask: cocoa::appkit::NSWindowStyleMask =
                        msg_send![self.ns_window, styleMask];
                    if style_mask.contains(NSWindowStyleMask::NSFullScreenWindowMask) {
                        let _: () = msg_send![self.ns_window, toggleFullScreen: nil];
                    }
                }
                WindowState::Minimized => {
                    let _: () = msg_send![self.ns_window, miniaturize: nil];
                }
                WindowState::Maximized => {
                    // First restore from minimized if needed
                    let is_minimized: bool = msg_send![self.ns_window, isMiniaturized];
                    if is_minimized {
                        let _: () = msg_send![self.ns_window, deminiaturize: nil];
                    }

                    // Then zoom (maximize)
                    let is_zoomed: bool = msg_send![self.ns_window, isZoomed];
                    if !is_zoomed {
                        let _: () = msg_send![self.ns_window, zoom: nil];
                    }
                }
                WindowState::Fullscreen => {
                    let style_mask: cocoa::appkit::NSWindowStyleMask =
                        msg_send![self.ns_window, styleMask];
                    if !style_mask.contains(NSWindowStyleMask::NSFullScreenWindowMask) {
                        let _: () = msg_send![self.ns_window, toggleFullScreen: nil];
                    }
                }
            }
        }
    }

    fn is_visible(&self) -> bool {
        PlatformWindow::is_visible(self)
    }

    fn set_visible(&mut self, visible: bool) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            if visible {
                let _: () = msg_send![self.ns_window, makeKeyAndOrderFront: nil];
            } else {
                let _: () = msg_send![self.ns_window, orderOut: nil];
            }
        }
    }

    fn is_resizable(&self) -> bool {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let style_mask: cocoa::appkit::NSWindowStyleMask = msg_send![self.ns_window, styleMask];
            style_mask.contains(NSWindowStyleMask::NSResizableWindowMask)
        }
    }

    fn set_resizable(&mut self, resizable: bool) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let mut style_mask: cocoa::appkit::NSWindowStyleMask =
                msg_send![self.ns_window, styleMask];
            if resizable {
                style_mask |= NSWindowStyleMask::NSResizableWindowMask;
            } else {
                style_mask &= !NSWindowStyleMask::NSResizableWindowMask;
            }
            let _: () = msg_send![self.ns_window, setStyleMask: style_mask];
        }
    }

    fn is_minimizable(&self) -> bool {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let style_mask: cocoa::appkit::NSWindowStyleMask = msg_send![self.ns_window, styleMask];
            style_mask.contains(NSWindowStyleMask::NSMiniaturizableWindowMask)
        }
    }

    fn set_minimizable(&mut self, minimizable: bool) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let mut style_mask: cocoa::appkit::NSWindowStyleMask =
                msg_send![self.ns_window, styleMask];
            if minimizable {
                style_mask |= NSWindowStyleMask::NSMiniaturizableWindowMask;
            } else {
                style_mask &= !NSWindowStyleMask::NSMiniaturizableWindowMask;
            }
            let _: () = msg_send![self.ns_window, setStyleMask: style_mask];
        }
    }

    fn is_closable(&self) -> bool {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let style_mask: cocoa::appkit::NSWindowStyleMask = msg_send![self.ns_window, styleMask];
            style_mask.contains(NSWindowStyleMask::NSClosableWindowMask)
        }
    }

    fn set_closable(&mut self, closable: bool) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let mut style_mask: cocoa::appkit::NSWindowStyleMask =
                msg_send![self.ns_window, styleMask];
            if closable {
                style_mask |= NSWindowStyleMask::NSClosableWindowMask;
            } else {
                style_mask &= !NSWindowStyleMask::NSClosableWindowMask;
            }
            let _: () = msg_send![self.ns_window, setStyleMask: style_mask];
        }
    }

    fn focus(&mut self) {
        PlatformWindow::activate(self)
    }

    fn is_focused(&self) -> bool {
        PlatformWindow::is_focused(self)
    }

    fn close(&mut self) {
        PlatformWindow::close(self)
    }

    fn request_redraw(&mut self) {
        PlatformWindow::request_redraw(self)
    }

    fn set_min_size(&mut self, size: Option<Size<Pixels>>) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            if let Some(size) = size {
                let ns_size =
                    cocoa::foundation::NSSize::new(size.width.0 as f64, size.height.0 as f64);
                let _: () = msg_send![self.ns_window, setMinSize: ns_size];
            } else {
                // Set to zero to remove constraint
                let ns_size = cocoa::foundation::NSSize::new(0.0, 0.0);
                let _: () = msg_send![self.ns_window, setMinSize: ns_size];
            }
        }
    }

    fn set_max_size(&mut self, size: Option<Size<Pixels>>) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            if let Some(size) = size {
                let ns_size =
                    cocoa::foundation::NSSize::new(size.width.0 as f64, size.height.0 as f64);
                let _: () = msg_send![self.ns_window, setMaxSize: ns_size];
            } else {
                // Set to max to remove constraint
                let ns_size = cocoa::foundation::NSSize::new(f64::MAX, f64::MAX);
                let _: () = msg_send![self.ns_window, setMaxSize: ns_size];
            }
        }
    }

    fn scale_factor(&self) -> f32 {
        let state = self.state.lock();
        state.scale_factor as f32
    }

    fn raw_window_handle(&self) -> CrossRawWindowHandle {
        // SAFETY: `ns_window` is alive for the lifetime of `self`; the
        // returned raw pointers are opaque handles for GPU integration.
        unsafe {
            let content_view: id = msg_send![self.ns_window, contentView];
            CrossRawWindowHandle::MacOS {
                ns_view: content_view as *mut std::ffi::c_void,
                ns_window: self.ns_window as *mut std::ffi::c_void,
            }
        }
    }
}

// ============================================================================
// macOS Window Extension Trait Implementation
// ============================================================================

use super::{
    liquid_glass::{LiquidGlassConfig, LiquidGlassMaterial},
    window_ext::{
        MacOSCollectionBehavior, MacOSWindowExt as MacOSWindowExtTrait, MacOSWindowLevel,
    },
    window_tiling::TilingConfiguration,
};

impl MacOSWindowExtTrait for MacOSWindow {
    fn set_liquid_glass(&mut self, material: LiquidGlassMaterial) {
        // Create default config from material
        let config = LiquidGlassConfig::from_material(material);
        self.set_liquid_glass_config(config);
    }

    fn set_liquid_glass_config(&mut self, config: LiquidGlassConfig) {
        // SAFETY: `ns_window` is alive; NSVisualEffectView is alloc-init'ed
        // and ownership passes to the window via `setContentView:`.
        unsafe {
            // Apply vibrancy effect to window content view
            let content_view: id = msg_send![self.ns_window, contentView];
            if content_view == nil {
                tracing::warn!("Cannot apply Liquid Glass: content view is nil");
                return;
            }

            // Create NSVisualEffectView
            let effect_view_class = class!(NSVisualEffectView);
            let effect_view: id = msg_send![effect_view_class, alloc];
            let effect_view: id = msg_send![effect_view, init];

            // Set frame to match content view
            let frame: NSRect = msg_send![content_view, frame];
            let _: () = msg_send![effect_view, setFrame: frame];

            // Set material (NSVisualEffectMaterial)
            let material_value: usize = config.material.to_ns_visual_effect_material();
            let _: () = msg_send![effect_view, setMaterial: material_value];

            // Set blending mode (NSVisualEffectBlendingMode)
            let blending_mode: usize = config.blending_mode.to_ns_blending_mode();
            let _: () = msg_send![effect_view, setBlendingMode: blending_mode];

            // Set state (NSVisualEffectState)
            let state: usize = 1; // NSVisualEffectStateActive
            let _: () = msg_send![effect_view, setState: state];

            // Enable autoresizing
            let autoresizing_mask: usize = (1 << 1) | (1 << 4); // NSViewWidthSizable | NSViewHeightSizable
            let _: () = msg_send![effect_view, setAutoresizingMask: autoresizing_mask];

            // Set as window content view
            let _: () = msg_send![self.ns_window, setContentView: effect_view];

            // Make window titlebar transparent if requested
            if config.transparent_titlebar {
                let style_mask: cocoa::appkit::NSWindowStyleMask =
                    msg_send![self.ns_window, styleMask];
                let new_style_mask =
                    style_mask | NSWindowStyleMask::NSFullSizeContentViewWindowMask;
                let _: () = msg_send![self.ns_window, setStyleMask: new_style_mask];
                let _: () = msg_send![self.ns_window, setTitlebarAppearsTransparent: YES];
            }

            tracing::debug!("Applied Liquid Glass material: {:?}", config.material);
        }
    }

    fn clear_liquid_glass(&mut self) {
        // SAFETY: `ns_window` is alive; the freshly created content view's
        // ownership passes to the window via `setContentView:`.
        unsafe {
            let frame: NSRect = msg_send![self.ns_window, frame];

            // Remove visual effect view and restore normal content view
            let content_view = view::create_content_view(
                NSRect::new(cocoa::foundation::NSPoint::new(0.0, 0.0), frame.size),
                PlatformWindow::scale_factor(self),
                Arc::downgrade(&self.callbacks),
            );

            let _: () = msg_send![self.ns_window, setContentView: content_view];

            // Restore titlebar appearance
            let _: () = msg_send![self.ns_window, setTitlebarAppearsTransparent: NO];

            tracing::debug!("Cleared Liquid Glass effect");
        }
    }

    fn enable_tiling(&mut self, config: TilingConfiguration) {
        // Native tiling API requires macOS 15+; until adopted, the
        // configuration is recorded for observability only.
        tracing::info!(
            "Window tiling enabled: position={:?}, ratio={}, layout={:?}",
            config.primary_position,
            config.split_ratio,
            config.layout
        );
    }

    fn disable_tiling(&mut self) {
        tracing::info!("Window tiling disabled");
    }

    fn is_tiling_enabled(&self) -> bool {
        // Native tiling API requires macOS 15+; not yet adopted.
        false
    }

    fn enable_tabbing(&mut self) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            // Enable automatic tabbing (macOS 10.12+)
            let tabbing_mode: isize = 1; // NSWindowTabbingModeAutomatic
            let _: () = msg_send![self.ns_window, setTabbingMode: tabbing_mode];

            tracing::debug!("Window tabbing enabled");
        }
    }

    fn disable_tabbing(&mut self) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let tabbing_mode: isize = 2; // NSWindowTabbingModeDisallowed
            let _: () = msg_send![self.ns_window, setTabbingMode: tabbing_mode];

            tracing::debug!("Window tabbing disabled");
        }
    }

    fn add_tab_to_window(&mut self, other_window_id: u64) {
        // SAFETY: `other_window_id` round-trips an NSWindow pointer that was
        // handed out as a window id; nil is rejected before messaging.
        unsafe {
            // Window ids are NSWindow pointers (see `WindowTrait::id`)
            let other_ns_window = other_window_id as *mut Object;

            if other_ns_window != nil {
                let _: () = msg_send![self.ns_window, addTabbedWindow:other_ns_window ordered:0]; // NSWindowAbove
                tracing::debug!("Added tab to window {:p}", other_ns_window);
            } else {
                tracing::warn!("Cannot add tab: window {:?} not found", other_window_id);
            }
        }
    }

    fn toggle_native_fullscreen(&mut self) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let _: () = msg_send![self.ns_window, toggleFullScreen: nil];
            tracing::debug!("Toggled native fullscreen");
        }
    }

    fn set_window_level(&mut self, level: MacOSWindowLevel) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let level_value = level.to_ns_value();
            let _: () = msg_send![self.ns_window, setLevel: level_value];
            tracing::debug!("Set window level to {:?} ({})", level, level_value);
        }
    }

    fn window_level(&self) -> MacOSWindowLevel {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let level_value: isize = msg_send![self.ns_window, level];
            match level_value {
                0 => MacOSWindowLevel::Normal,
                3 => MacOSWindowLevel::Floating,
                8 => MacOSWindowLevel::ModalPanel,
                24 => MacOSWindowLevel::MainMenu,
                25 => MacOSWindowLevel::Status,
                101 => MacOSWindowLevel::PopUpMenu,
                1000 => MacOSWindowLevel::ScreenSaver,
                _ if level_value == isize::MAX - 1 => MacOSWindowLevel::FloatingPanel,
                _ => MacOSWindowLevel::Normal, // Default to normal for unknown values
            }
        }
    }

    fn set_collection_behavior(&mut self, behavior: MacOSCollectionBehavior) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let _: () = msg_send![self.ns_window, setCollectionBehavior: behavior.bits() as usize];
            tracing::debug!("Set collection behavior: {:?}", behavior);
        }
    }

    fn set_has_shadow(&mut self, has_shadow: bool) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let value: BOOL = if has_shadow { YES } else { NO };
            let _: () = msg_send![self.ns_window, setHasShadow: value];
            tracing::debug!("Set window shadow: {}", has_shadow);
        }
    }

    fn set_alpha(&mut self, alpha: f32) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let clamped_alpha = alpha.clamp(0.0, 1.0);
            let _: () = msg_send![self.ns_window, setAlphaValue: clamped_alpha as f64];
            tracing::debug!("Set window alpha: {}", clamped_alpha);
        }
    }

    fn backing_scale_factor(&self) -> f32 {
        let state = self.state.lock();
        state.scale_factor as f32
    }

    fn convert_point_from_backing(&self, point: Point<Pixels>) -> Point<Pixels> {
        let scale = self.backing_scale_factor();
        Point::new(Pixels(point.x.0 / scale), Pixels(point.y.0 / scale))
    }

    fn convert_point_to_backing(&self, point: Point<Pixels>) -> Point<Pixels> {
        let scale = self.backing_scale_factor();
        Point::new(Pixels(point.x.0 * scale), Pixels(point.y.0 * scale))
    }
}

// ============================================================================
// NSWindowDelegate Implementation
// ============================================================================

use std::sync::Weak;

/// Create a window delegate for lifecycle events
fn create_window_delegate(window: Weak<MacOSWindow>) -> id {
    // SAFETY: the delegate class is registered before alloc/init; the boxed
    // Weak pointer stored in the ivar is reclaimed in the delegate's
    // `dealloc`, so it lives exactly as long as the delegate.
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
        let mut decl = ClassDecl::new("FLUIWindowDelegate", superclass)
            .expect("FLUIWindowDelegate must be registered exactly once (guarded by Once)");

        // Add ivar to store window pointer
        decl.add_ivar::<*mut std::ffi::c_void>("window_ptr");

        // windowDidResize:
        extern "C" fn window_did_resize(this: &Object, _sel: Sel, _notification: id) {
            // SAFETY: AppKit invokes delegate methods on live delegate objects.
            if let Some(window) = unsafe { get_window_from_delegate(this) } {
                window.handle_resize();
            }
        }

        // windowDidMove:
        extern "C" fn window_did_move(this: &Object, _sel: Sel, _notification: id) {
            // SAFETY: AppKit invokes delegate methods on live delegate objects.
            if let Some(window) = unsafe { get_window_from_delegate(this) } {
                window.handle_move();
            }
        }

        // windowDidBecomeKey:
        extern "C" fn window_did_become_key(this: &Object, _sel: Sel, _notification: id) {
            // SAFETY: AppKit invokes delegate methods on live delegate objects.
            if let Some(window) = unsafe { get_window_from_delegate(this) } {
                window.handle_focus_gained();
            }
        }

        // windowDidResignKey:
        extern "C" fn window_did_resign_key(this: &Object, _sel: Sel, _notification: id) {
            // SAFETY: AppKit invokes delegate methods on live delegate objects.
            if let Some(window) = unsafe { get_window_from_delegate(this) } {
                window.handle_focus_lost();
            }
        }

        // windowShouldClose:
        extern "C" fn window_should_close(this: &Object, _sel: Sel, _sender: id) -> BOOL {
            // SAFETY: AppKit invokes delegate methods on live delegate objects.
            if let Some(window) = unsafe { get_window_from_delegate(this) } {
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
        extern "C" fn window_will_close(this: &Object, _sel: Sel, _notification: id) {
            // SAFETY: AppKit invokes delegate methods on live delegate objects.
            if let Some(window) = unsafe { get_window_from_delegate(this) } {
                window.handle_close();
            }
        }

        // windowDidChangeBackingProperties: (Retina/DPI change)
        extern "C" fn window_did_change_backing_properties(
            this: &Object,
            _sel: Sel,
            _notification: id,
        ) {
            // SAFETY: AppKit invokes delegate methods on live delegate objects.
            if let Some(window) = unsafe { get_window_from_delegate(this) } {
                window.handle_backing_properties_changed();
            }
        }

        // windowDidChangeScreen: (moved to different monitor)
        extern "C" fn window_did_change_screen(this: &Object, _sel: Sel, _notification: id) {
            // SAFETY: AppKit invokes delegate methods on live delegate objects.
            if let Some(window) = unsafe { get_window_from_delegate(this) } {
                window.handle_screen_changed();
            }
        }

        // dealloc — reclaim the boxed Weak<MacOSWindow>
        extern "C" fn delegate_dealloc(this: &Object, _sel: Sel) {
            // SAFETY: the ivar holds either null or a Box<Weak<MacOSWindow>>
            // leaked in `create_window_delegate`; reclaiming it exactly once
            // on dealloc is the matching release. The super dealloc message
            // is the mandatory NSObject teardown.
            unsafe {
                let window_ptr: *mut std::ffi::c_void = *this.get_ivar("window_ptr");
                if !window_ptr.is_null() {
                    drop(Box::from_raw(window_ptr as *mut Weak<MacOSWindow>));
                }
                let superclass = class!(NSObject);
                let _: () = msg_send![super(this, superclass), dealloc];
            }
        }

        // Add methods
        // SAFETY: every registered function pointer matches the Objective-C
        // method signature of its selector, as required by
        // `ClassDecl::add_method`.
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
            decl.add_method(
                sel!(dealloc),
                delegate_dealloc as extern "C" fn(&Object, Sel),
            );
        }

        decl.register();
    });

    Class::get("FLUIWindowDelegate")
        .expect("FLUIWindowDelegate was registered by the Once block above")
}

/// Get window from delegate
///
/// # Safety
///
/// `delegate` must be a live FLUIWindowDelegate whose `window_ptr` ivar is
/// either null or points to a `Weak<MacOSWindow>` owned by that delegate.
unsafe fn get_window_from_delegate(delegate: &Object) -> Option<Arc<MacOSWindow>> {
    // SAFETY: per the function contract the ivar is null or a valid
    // Box<Weak<MacOSWindow>> pointer owned by the delegate.
    unsafe {
        let window_ptr: *mut std::ffi::c_void = *delegate.get_ivar("window_ptr");
        let weak_ptr = window_ptr as *mut Weak<MacOSWindow>;
        if weak_ptr.is_null() {
            return None;
        }
        (*weak_ptr).upgrade()
    }
}

// ============================================================================
// Window Event Handlers
// ============================================================================

impl MacOSWindow {
    /// Handle window resize event
    fn handle_resize(&self) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let frame: NSRect = msg_send![self.ns_window, frame];
            let content_rect: NSRect = msg_send![self.ns_window, contentRectForFrameRect: frame];

            let new_size = Size::new(
                flui_types::geometry::px(content_rect.size.width as f32),
                flui_types::geometry::px(content_rect.size.height as f32),
            );

            // Update state
            let scale = {
                let mut state = self.state.lock();
                state.bounds.size = new_size;
                state.scale_factor
            };

            // Notify per-window callbacks
            self.callbacks.dispatch_resize(new_size, scale as f32);

            tracing::debug!(
                "Window resized to {}x{}",
                new_size.width.0,
                new_size.height.0
            );
        }
    }

    /// Handle window move event
    fn handle_move(&self) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
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

            self.callbacks.dispatch_moved();

            tracing::debug!("Window moved to ({}, {})", new_origin.x.0, new_origin.y.0);
        }
    }

    /// Handle focus gained event
    fn handle_focus_gained(&self) {
        self.callbacks.dispatch_active_status_change(true);
        tracing::debug!("Window gained focus");
    }

    /// Handle focus lost event
    fn handle_focus_lost(&self) {
        self.callbacks.dispatch_active_status_change(false);
        tracing::debug!("Window lost focus");
    }

    /// Handle close request event
    ///
    /// Returns true to allow close, false to prevent
    fn handle_close_request(&self) -> bool {
        let should_close = self.callbacks.dispatch_should_close();
        tracing::debug!("Window close requested: {}", should_close);
        should_close
    }

    /// Handle window close event
    fn handle_close(&self) {
        self.callbacks.dispatch_close();
        tracing::debug!("Window closed");
    }

    /// Handle backing properties changed (Retina/DPI change)
    fn handle_backing_properties_changed(&self) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`; the
        // content view is nil-checked before use.
        unsafe {
            let new_scale: f64 = msg_send![self.ns_window, backingScaleFactor];

            // Update window state
            let (changed, size) = {
                let mut state = self.state.lock();
                let changed = (state.scale_factor - new_scale).abs() > 0.01;
                if changed {
                    state.scale_factor = new_scale;
                    tracing::info!("Window scale factor changed to {}", new_scale);
                }
                (changed, state.bounds.size)
            };

            // Update content view scale factor
            let content_view: id = msg_send![self.ns_window, contentView];
            if content_view != nil {
                view::update_view_scale_factor(content_view, new_scale);
            }

            // A scale change invalidates layout: notify as a resize
            if changed {
                self.callbacks.dispatch_resize(size, new_scale as f32);
            }
        }
    }

    /// Handle screen changed (moved to different monitor)
    fn handle_screen_changed(&self) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`; the
        // screen object is nil-checked before messaging.
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
