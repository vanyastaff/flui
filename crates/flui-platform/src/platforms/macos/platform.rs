//! macOS platform implementation using AppKit/Cocoa

use crate::config::WindowConfiguration;
use crate::executor::{BackgroundExecutor, ForegroundExecutor};
use crate::shared::PlatformHandlers;
use crate::traits::*;
use anyhow::{Context, Result};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

use cocoa::appkit::{NSApp, NSApplication, NSApplicationActivationPolicyRegular};
use cocoa::base::{id, nil};
use objc::runtime::Object;

mod display;
mod window;

pub use display::MacOSDisplay;
pub use window::MacOSWindow;

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

unsafe impl Send for MacOSPlatform {}
unsafe impl Sync for MacOSPlatform {}

impl MacOSPlatform {
    /// Create a new macOS platform with default configuration
    pub fn new() -> Result<Self> {
        Self::with_config(WindowConfiguration::default())
    }

    /// Create a new macOS platform with custom configuration
    pub fn with_config(config: WindowConfiguration) -> Result<Self> {
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

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        // TODO: Implement Core Text system (Phase 2)
        // For now, uses trait default methods (stub implementation)
        Arc::new(DummyTextSystem)
    }

    fn run(&self, on_finish_launching: Box<dyn FnOnce()>) {
        unsafe {
            // Call the launch callback
            on_finish_launching();

            // Activate the app (bring to foreground)
            self.app.activateIgnoringOtherApps_(true);

            // Set up event handling (custom sendEvent to intercept input)
            // Note: For now, we rely on NSWindowDelegate for window events
            // Input events can be handled via NSResponder chain or custom NSApplication subclass

            // Run the NSApplication event loop
            tracing::info!("Starting NSApplication event loop");
            self.app.run();
        }
    }

    fn quit(&self) {
        unsafe {
            tracing::info!("Requesting application quit");
            let _: () = msg_send![self.app, terminate: nil];
        }
    }

    fn request_frame(&self) {
        // TODO: Post event to trigger frame rendering
        tracing::trace!("Frame requested (not yet implemented)");
    }

    fn open_window(&self, options: WindowOptions) -> Result<Box<dyn PlatformWindow>> {
        let window = MacOSWindow::new(
            options,
            Arc::clone(&self.windows),
            Arc::clone(&self.handlers),
            self.config.clone(),
        )?;

        Ok(Box::new(window))
    }

    fn active_window(&self) -> Option<WindowId> {
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
        Arc::new(crate::platforms::macos::MacOSClipboard::new())
    }

    fn capabilities(&self) -> &dyn PlatformCapabilities {
        // TODO: Return macOS capabilities
        unimplemented!("macOS capabilities not yet implemented")
    }

    fn name(&self) -> &'static str {
        "macOS (AppKit)"
    }

    fn on_quit(&self, callback: Box<dyn FnMut() + Send>) {
        let mut handlers = self.handlers.lock();
        handlers.on_quit = Some(callback);
    }

    fn on_window_event(&self, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        let mut handlers = self.handlers.lock();
        handlers.on_window_event = Some(callback);
    }

    fn app_path(&self) -> Result<std::path::PathBuf> {
        unsafe {
            use cocoa::foundation::NSBundle;

            let bundle: id = msg_send![class!(NSBundle), mainBundle];
            if bundle == nil {
                return Err(anyhow::anyhow!("Failed to get main bundle"));
            }

            let path: id = msg_send![bundle, bundlePath];
            if path == nil {
                return Err(anyhow::anyhow!("Failed to get bundle path"));
            }

            use cocoa::foundation::NSString;
            let c_str = NSString::UTF8String(path);
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

// ==================== Dummy Implementations ====================

struct DummyTextSystem;

impl PlatformTextSystem for DummyTextSystem {}
