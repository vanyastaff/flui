//! macOS platform implementation using AppKit/Cocoa

use std::{collections::HashMap, sync::Arc};

use anyhow::{Context, Result};
use cocoa::{
    appkit::{NSApp, NSApplication, NSApplicationActivationPolicyRegular},
    base::{YES, id, nil},
};
use objc::{class, msg_send, sel, sel_impl};
use parking_lot::Mutex;

use super::{display, window::MacOSWindow};
use crate::{
    config::WindowConfiguration,
    executor::{BackgroundExecutor, ForegroundExecutor},
    shared::PlatformHandlers,
    traits::{
        Clipboard, DesktopCapabilities, Platform, PlatformCapabilities, PlatformDisplay,
        PlatformExecutor, PlatformWindow, WindowEvent, WindowId, WindowOptions,
    },
};

/// Capabilities descriptor shared by all macOS platform instances.
static MACOS_CAPABILITIES: DesktopCapabilities = DesktopCapabilities;

/// macOS platform state
pub struct MacOSPlatform {
    /// NSApplication instance (retained)
    app: id,

    /// Open windows (keyed by NSWindow pointer as u64)
    windows: Arc<Mutex<HashMap<u64, Arc<MacOSWindow>>>>,

    /// Platform event handlers
    handlers: Arc<Mutex<PlatformHandlers>>,

    /// Background executor (GCD-based)
    background_executor: Arc<BackgroundExecutor>,

    /// Foreground executor (NSRunLoop-based)
    foreground_executor: Arc<ForegroundExecutor>,

    /// Window configuration
    config: WindowConfiguration,
}

// SAFETY: the NSApplication pointer is a process-wide singleton only messaged
// from the main thread (AppKit convention); all other fields are
// `Arc`/`Mutex`-protected. `Platform: Send + Sync` requires the wrapper to be
// shareable.
unsafe impl Send for MacOSPlatform {}
// SAFETY: see `Send` above — interior mutability is Mutex-guarded and the raw
// pointer is main-thread-affine by AppKit convention.
unsafe impl Sync for MacOSPlatform {}

impl MacOSPlatform {
    /// Create a new macOS platform with default configuration
    pub fn new() -> Result<Self> {
        Self::with_config(WindowConfiguration::default())
    }

    /// Create a new macOS platform with custom configuration
    pub fn with_config(config: WindowConfiguration) -> Result<Self> {
        // SAFETY: must run on the main thread (the platform owns the event
        // loop); `NSApp()` returns the shared application singleton which is
        // nil-checked before use.
        unsafe {
            // Initialize NSApplication
            let app = NSApp();
            if app == nil {
                return Err(anyhow::anyhow!("Failed to get NSApplication"));
            }

            // Set activation policy to regular app (shows in Dock)
            app.setActivationPolicy_(NSApplicationActivationPolicyRegular);

            // Create executors
            let background_executor = Arc::new(BackgroundExecutor::new());
            let foreground_executor = Arc::new(ForegroundExecutor::new());

            tracing::info!("macOS platform initialized with AppKit");

            Ok(Self {
                app,
                windows: Arc::new(Mutex::new(HashMap::new())),
                handlers: Arc::new(Mutex::new(PlatformHandlers::default())),
                background_executor,
                foreground_executor,
                config,
            })
        }
    }

    /// Get the NSApplication instance
    pub fn app(&self) -> id {
        self.app
    }
}

impl Platform for MacOSPlatform {
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        Arc::clone(&self.background_executor) as Arc<dyn PlatformExecutor>
    }

    fn foreground_executor(&self) -> Arc<dyn PlatformExecutor> {
        Arc::clone(&self.foreground_executor) as Arc<dyn PlatformExecutor>
    }

    fn run(self: Box<Self>, on_finish_launching: Box<dyn FnOnce()>) {
        // SAFETY: runs on the main thread; `self.app` is the live
        // NSApplication singleton.
        unsafe {
            // Call the launch callback
            on_finish_launching();

            // Activate the app (bring to foreground)
            self.app.activateIgnoringOtherApps_(YES);

            // Run the NSApplication event loop. Window lifecycle events are
            // delivered via NSWindowDelegate; input events via the content
            // view's NSResponder chain.
            tracing::info!("Starting NSApplication event loop");
            self.app.run();
        }
    }

    fn quit(&self) {
        // SAFETY: `self.app` is the live NSApplication singleton.
        unsafe {
            tracing::info!("Requesting application quit");
            let _: () = msg_send![self.app, terminate: nil];
        }
    }

    fn open_window(&self, options: WindowOptions) -> Result<Box<dyn PlatformWindow>> {
        let window = MacOSWindow::new(options, Arc::clone(&self.windows), self.config.clone())?;

        Ok(Box::new(window))
    }

    fn active_window(&self) -> Option<WindowId> {
        // SAFETY: `self.app` is the live NSApplication singleton; `keyWindow`
        // returns nil or a live NSWindow whose pointer value is used as an id.
        unsafe {
            let key_window: id = msg_send![self.app, keyWindow];
            if key_window != nil {
                let ptr = key_window as u64;
                Some(WindowId(ptr))
            } else {
                None
            }
        }
    }

    fn displays(&self) -> Vec<Arc<dyn PlatformDisplay>> {
        display::enumerate_displays()
    }

    fn primary_display(&self) -> Option<Arc<dyn PlatformDisplay>> {
        display::enumerate_displays()
            .into_iter()
            .find(|d| d.is_primary())
    }

    fn clipboard(&self) -> Arc<dyn Clipboard> {
        Arc::new(super::MacOSClipboard::new())
    }

    fn capabilities(&self) -> &dyn PlatformCapabilities {
        &MACOS_CAPABILITIES
    }

    fn name(&self) -> &'static str {
        "macOS (AppKit)"
    }

    fn on_quit(&self, callback: Box<dyn FnMut() + Send>) {
        let mut handlers = self.handlers.lock();
        handlers.quit = Some(callback);
    }

    fn on_window_event(&self, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        let mut handlers = self.handlers.lock();
        handlers.window_event = Some(callback);
    }

    fn app_path(&self) -> Result<std::path::PathBuf> {
        // SAFETY: `mainBundle` returns the shared NSBundle singleton; both it
        // and `bundlePath` are nil-checked, and the UTF8String buffer is
        // copied into an owned PathBuf before the autorelease pool drains.
        unsafe {
            let bundle: id = msg_send![class!(NSBundle), mainBundle];
            if bundle == nil {
                return Err(anyhow::anyhow!("Failed to get main bundle"));
            }

            let path: id = msg_send![bundle, bundlePath];
            if path == nil {
                return Err(anyhow::anyhow!("Failed to get bundle path"));
            }

            let c_str: *const i8 = msg_send![path, UTF8String];
            if c_str.is_null() {
                return Err(anyhow::anyhow!("Bundle path has no UTF-8 representation"));
            }
            let rust_str = std::ffi::CStr::from_ptr(c_str)
                .to_str()
                .context("Invalid UTF-8 in bundle path")?;

            Ok(std::path::PathBuf::from(rust_str))
        }
    }
}

impl Drop for MacOSPlatform {
    fn drop(&mut self) {
        tracing::debug!("Dropping MacOSPlatform");
        // NSApplication is a singleton, no need to release
    }
}
