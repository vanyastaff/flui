//! Platform callback handlers
//!
//! Registry pattern for platform callbacks, allowing the framework to register
//! event handlers without tight coupling to platform implementations.
//!
//! Two levels of callbacks:
//! - [`PlatformHandlers`]: Global platform-level callbacks (quit, reopen, etc.)
//! - [`WindowCallbacks`]: Per-window callbacks (input, resize, close, etc.)

use crate::traits::{DispatchEventResult, PlatformInput, WindowEvent};
use flui_types::geometry::{Pixels, Size};
use parking_lot::Mutex;

/// Platform callback handlers registry
///
/// This struct stores all registered callbacks from the framework.
/// Platform implementations invoke these callbacks when events occur.
///
/// # Design Pattern
///
/// This is the callback registry pattern from GPUI - it decouples the framework
/// from platform implementations. The framework registers handlers, and the
/// platform invokes them at appropriate times.
///
/// # Thread Safety
///
/// All callbacks are `Send` but not `Sync`, as they're typically invoked from
/// the main thread only.
pub struct PlatformHandlers {
    /// Called when the application should quit
    pub quit: Option<Box<dyn FnMut() + Send>>,

    /// Called when the application is reopened (macOS dock click)
    pub reopen: Option<Box<dyn FnMut() + Send>>,

    /// Called when a window event occurs
    pub window_event: Option<Box<dyn FnMut(WindowEvent) + Send>>,

    /// Called when URLs are opened (e.g., from file manager, browser)
    pub open_urls: Option<Box<dyn FnMut(Vec<String>) + Send>>,

    /// Called when keyboard layout changes
    pub keyboard_layout_changed: Option<Box<dyn FnMut() + Send>>,
}

impl PlatformHandlers {
    /// Create new empty handler registry
    pub fn new() -> Self {
        Self {
            quit: None,
            reopen: None,
            window_event: None,
            open_urls: None,
            keyboard_layout_changed: None,
        }
    }

    /// Invoke the quit callback if registered
    #[inline]
    pub fn invoke_quit(&mut self) {
        if let Some(ref mut handler) = self.quit {
            handler();
        }
    }

    /// Invoke the reopen callback if registered
    #[inline]
    pub fn invoke_reopen(&mut self) {
        if let Some(ref mut handler) = self.reopen {
            handler();
        }
    }

    /// Invoke the window event callback if registered
    #[inline]
    pub fn invoke_window_event(&mut self, event: WindowEvent) {
        if let Some(ref mut handler) = self.window_event {
            handler(event);
        }
    }

    /// Invoke the open URLs callback if registered
    #[inline]
    pub fn invoke_open_urls(&mut self, urls: Vec<String>) {
        if let Some(ref mut handler) = self.open_urls {
            handler(urls);
        }
    }

    /// Invoke the keyboard layout changed callback if registered
    #[inline]
    pub fn invoke_keyboard_layout_changed(&mut self) {
        if let Some(ref mut handler) = self.keyboard_layout_changed {
            handler();
        }
    }
}

impl Default for PlatformHandlers {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for PlatformHandlers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlatformHandlers")
            .field("quit", &self.quit.is_some())
            .field("reopen", &self.reopen.is_some())
            .field("window_event", &self.window_event.is_some())
            .field("open_urls", &self.open_urls.is_some())
            .field(
                "keyboard_layout_changed",
                &self.keyboard_layout_changed.is_some(),
            )
            .finish()
    }
}

// ============================================================================
// Per-Window Callbacks
// ============================================================================

/// Per-window callback storage using Mutex-based take/restore pattern
///
/// Each callback is stored in a `Mutex<Option<Box<dyn FnMut/FnOnce + Send>>>`.
/// The dispatch pattern ensures reentrancy safety:
/// 1. Lock → take callback out → unlock
/// 2. Call callback (lock is NOT held)
/// 3. Lock → restore callback → unlock
///
/// This prevents deadlocks when a callback tries to interact with the window
/// (which would require the same lock if stored differently).
pub struct WindowCallbacks {
    /// Called when an input event (pointer, keyboard) is delivered to this window.
    /// Returns `DispatchEventResult` indicating if the event was consumed.
    pub on_input: Mutex<Option<Box<dyn FnMut(PlatformInput) -> DispatchEventResult + Send>>>,

    /// Called when the platform requests a new frame to be rendered.
    pub on_request_frame: Mutex<Option<Box<dyn FnMut() + Send>>>,

    /// Called when the window is resized. Parameters: new size (logical), scale factor.
    pub on_resize: Mutex<Option<Box<dyn FnMut(Size<Pixels>, f32) + Send>>>,

    /// Called when the window is moved.
    pub on_moved: Mutex<Option<Box<dyn FnMut() + Send>>>,

    /// Called when the window is about to be destroyed. Only fires once (FnOnce).
    pub on_close: Mutex<Option<Box<dyn FnOnce() + Send>>>,

    /// Called to ask if the window should close. Return `false` to veto.
    pub on_should_close: Mutex<Option<Box<dyn FnMut() -> bool + Send>>>,

    /// Called when the window gains or loses focus. Parameter: is_active.
    pub on_active_status_change: Mutex<Option<Box<dyn FnMut(bool) + Send>>>,

    /// Called when the mouse enters or leaves the window. Parameter: is_hovered.
    pub on_hover_status_change: Mutex<Option<Box<dyn FnMut(bool) + Send>>>,

    /// Called when the system appearance (light/dark) changes.
    pub on_appearance_changed: Mutex<Option<Box<dyn FnMut() + Send>>>,
}

impl WindowCallbacks {
    /// Create a new empty callback set
    pub fn new() -> Self {
        Self {
            on_input: Mutex::new(None),
            on_request_frame: Mutex::new(None),
            on_resize: Mutex::new(None),
            on_moved: Mutex::new(None),
            on_close: Mutex::new(None),
            on_should_close: Mutex::new(None),
            on_active_status_change: Mutex::new(None),
            on_hover_status_change: Mutex::new(None),
            on_appearance_changed: Mutex::new(None),
        }
    }

    /// Dispatch an input event. Returns `DispatchEventResult::default()` if no callback.
    pub fn dispatch_input(&self, event: PlatformInput) -> DispatchEventResult {
        let cb = self.on_input.lock().take();
        if let Some(mut cb) = cb {
            let result = cb(event);
            *self.on_input.lock() = Some(cb);
            result
        } else {
            DispatchEventResult::default()
        }
    }

    /// Dispatch a frame request.
    pub fn dispatch_request_frame(&self) {
        let cb = self.on_request_frame.lock().take();
        if let Some(mut cb) = cb {
            cb();
            *self.on_request_frame.lock() = Some(cb);
        }
    }

    /// Dispatch a resize event with new logical size and scale factor.
    pub fn dispatch_resize(&self, size: Size<Pixels>, scale_factor: f32) {
        let cb = self.on_resize.lock().take();
        if let Some(mut cb) = cb {
            cb(size, scale_factor);
            *self.on_resize.lock() = Some(cb);
        }
    }

    /// Dispatch a window moved event.
    pub fn dispatch_moved(&self) {
        let cb = self.on_moved.lock().take();
        if let Some(mut cb) = cb {
            cb();
            *self.on_moved.lock() = Some(cb);
        }
    }

    /// Dispatch close event. Consumes the callback (FnOnce).
    pub fn dispatch_close(&self) {
        let cb = self.on_close.lock().take();
        if let Some(cb) = cb {
            cb();
            // FnOnce — not restored
        }
    }

    /// Query whether the window should close. Returns `true` if no callback registered.
    pub fn dispatch_should_close(&self) -> bool {
        let cb = self.on_should_close.lock().take();
        if let Some(mut cb) = cb {
            let result = cb();
            *self.on_should_close.lock() = Some(cb);
            result
        } else {
            true // Default: allow close
        }
    }

    /// Dispatch active status change (focus gained/lost).
    pub fn dispatch_active_status_change(&self, is_active: bool) {
        let cb = self.on_active_status_change.lock().take();
        if let Some(mut cb) = cb {
            cb(is_active);
            *self.on_active_status_change.lock() = Some(cb);
        }
    }

    /// Dispatch hover status change (mouse enter/leave).
    pub fn dispatch_hover_status_change(&self, is_hovered: bool) {
        let cb = self.on_hover_status_change.lock().take();
        if let Some(mut cb) = cb {
            cb(is_hovered);
            *self.on_hover_status_change.lock() = Some(cb);
        }
    }

    /// Dispatch appearance change (system theme changed).
    pub fn dispatch_appearance_changed(&self) {
        let cb = self.on_appearance_changed.lock().take();
        if let Some(mut cb) = cb {
            cb();
            *self.on_appearance_changed.lock() = Some(cb);
        }
    }
}

impl Default for WindowCallbacks {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for WindowCallbacks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowCallbacks")
            .field("on_input", &self.on_input.lock().is_some())
            .field("on_request_frame", &self.on_request_frame.lock().is_some())
            .field("on_resize", &self.on_resize.lock().is_some())
            .field("on_moved", &self.on_moved.lock().is_some())
            .field("on_close", &self.on_close.lock().is_some())
            .field("on_should_close", &self.on_should_close.lock().is_some())
            .field(
                "on_active_status_change",
                &self.on_active_status_change.lock().is_some(),
            )
            .field(
                "on_hover_status_change",
                &self.on_hover_status_change.lock().is_some(),
            )
            .field(
                "on_appearance_changed",
                &self.on_appearance_changed.lock().is_some(),
            )
            .finish()
    }
}
