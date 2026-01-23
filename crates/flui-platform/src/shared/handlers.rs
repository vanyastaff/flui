//! Platform callback handlers
//!
//! Registry pattern for platform callbacks, allowing the framework to register
//! event handlers without tight coupling to platform implementations.

use crate::traits::WindowEvent;

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
    pub fn invoke_quit(&mut self) {
        if let Some(ref mut handler) = self.quit {
            handler();
        }
    }

    /// Invoke the reopen callback if registered
    pub fn invoke_reopen(&mut self) {
        if let Some(ref mut handler) = self.reopen {
            handler();
        }
    }

    /// Invoke the window event callback if registered
    pub fn invoke_window_event(&mut self, event: WindowEvent) {
        if let Some(ref mut handler) = self.window_event {
            handler(event);
        }
    }

    /// Invoke the open URLs callback if registered
    pub fn invoke_open_urls(&mut self, urls: Vec<String>) {
        if let Some(ref mut handler) = self.open_urls {
            handler(urls);
        }
    }

    /// Invoke the keyboard layout changed callback if registered
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
