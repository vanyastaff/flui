//! Web platform core implementation

use std::sync::Arc;

use anyhow::Result;
use parking_lot::Mutex;

use crate::{
    shared::PlatformHandlers,
    traits::*,
};

use super::{
    clipboard::WebClipboard,
    display::WebDisplay,
    executor::WebExecutor,
    text_system::WebTextSystem,
};

pub struct WebPlatform {
    state: Arc<Mutex<WebState>>,
}

struct WebState {
    handlers: PlatformHandlers,
    foreground_executor: Arc<WebExecutor>,
    background_executor: Arc<WebExecutor>,
    text_system: Arc<WebTextSystem>,
    clipboard: Arc<WebClipboard>,
    is_running: bool,
}

unsafe impl Send for WebPlatform {}
unsafe impl Sync for WebPlatform {}

impl WebPlatform {
    pub fn new() -> Result<Self> {
        console_error_panic_hook::set_once();
        let state = WebState {
            handlers: PlatformHandlers::new(),
            foreground_executor: Arc::new(WebExecutor::new()),
            background_executor: Arc::new(WebExecutor::new()),
            text_system: Arc::new(WebTextSystem::new()),
            clipboard: Arc::new(WebClipboard::new()),
            is_running: false,
        };
        Ok(Self {
            state: Arc::new(Mutex::new(state)),
        })
    }

    fn with_state<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut WebState) -> R,
    {
        f(&mut self.state.lock())
    }
}

impl Platform for WebPlatform {
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        self.with_state(|s| s.background_executor.clone())
    }

    fn foreground_executor(&self) -> Arc<dyn PlatformExecutor> {
        self.with_state(|s| s.foreground_executor.clone())
    }

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        self.with_state(|s| s.text_system.clone())
    }

    fn run(&self, on_ready: Box<dyn FnOnce()>) {
        tracing::info!("Starting web platform");
        self.with_state(|s| s.is_running = true);
        on_ready();
        tracing::info!("Web platform ready");
    }

    fn quit(&self) {
        tracing::info!("Web platform quit");
        self.with_state(|s| {
            s.is_running = false;
            s.handlers.invoke_quit();
        });
    }

    fn request_frame(&self) {}

    fn open_window(&self, _options: WindowOptions) -> Result<Box<dyn PlatformWindow>> {
        anyhow::bail!("WebWindow not yet implemented — see Task 7")
    }

    fn active_window(&self) -> Option<WindowId> {
        Some(WindowId(0))
    }

    fn window_stack(&self) -> Option<Vec<WindowId>> {
        Some(vec![WindowId(0)])
    }

    fn displays(&self) -> Vec<Arc<dyn PlatformDisplay>> {
        vec![Arc::new(WebDisplay::from_browser())]
    }

    fn primary_display(&self) -> Option<Arc<dyn PlatformDisplay>> {
        Some(Arc::new(WebDisplay::from_browser()))
    }

    fn clipboard(&self) -> Arc<dyn Clipboard> {
        self.with_state(|s| s.clipboard.clone())
    }

    fn capabilities(&self) -> &dyn PlatformCapabilities {
        &WebCapabilities
    }

    fn name(&self) -> &'static str {
        "Web (WASM)"
    }

    fn on_quit(&self, callback: Box<dyn FnMut() + Send>) {
        self.with_state(|s| s.handlers.quit = Some(callback));
    }

    fn on_window_event(&self, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        self.with_state(|s| s.handlers.window_event = Some(callback));
    }

    fn app_path(&self) -> Result<std::path::PathBuf> {
        Ok(std::path::PathBuf::from("/"))
    }
}
