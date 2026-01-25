//! Headless platform implementation for testing
//!
//! This platform implementation runs without any actual windowing system,
//! making it ideal for unit tests and CI environments.

use crate::shared::PlatformHandlers;
use crate::traits::{
    Clipboard, DesktopCapabilities, Platform, PlatformCapabilities, PlatformDisplay,
    PlatformExecutor, PlatformTextSystem, PlatformWindow, WindowEvent, WindowId, WindowOptions,
};
use anyhow::Result;
use flui_types::geometry::{Bounds, DevicePixels, Pixels, Point, Size};
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;

/// Headless platform for testing
///
/// This platform implementation doesn't create any real windows or graphics contexts.
/// It's designed for:
/// - Unit tests that need a Platform implementation
/// - CI environments without display servers
/// - Benchmarking without rendering overhead
pub struct HeadlessPlatform {
    state: Arc<Mutex<HeadlessState>>,
}

struct HeadlessState {
    capabilities: DesktopCapabilities,
    handlers: PlatformHandlers,
    background_executor: Arc<TestExecutor>,
    foreground_executor: Arc<TestExecutor>,
    text_system: Arc<MockTextSystem>,
    clipboard: Arc<MockClipboard>,
    active_window: Option<WindowId>,
    is_running: bool,
    windows: Vec<MockWindow>,
}

impl HeadlessPlatform {
    /// Create a new headless platform
    pub fn new() -> Self {
        let state = HeadlessState {
            capabilities: DesktopCapabilities,
            handlers: PlatformHandlers::new(),
            background_executor: Arc::new(TestExecutor::new("background")),
            foreground_executor: Arc::new(TestExecutor::new("foreground")),
            text_system: Arc::new(MockTextSystem),
            clipboard: Arc::new(MockClipboard::new()),
            active_window: None,
            is_running: false,
            windows: Vec::new(),
        };

        Self {
            state: Arc::new(Mutex::new(state)),
        }
    }

    fn with_state<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut HeadlessState) -> R,
    {
        let mut state = self.state.lock();
        f(&mut state)
    }
}

impl Default for HeadlessPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl Platform for HeadlessPlatform {
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        self.with_state(|state| state.background_executor.clone())
    }

    fn foreground_executor(&self) -> Arc<dyn PlatformExecutor> {
        self.with_state(|state| state.foreground_executor.clone())
    }

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        self.with_state(|state| state.text_system.clone())
    }

    fn run(&self, on_ready: Box<dyn FnOnce()>) {
        tracing::info!("Starting headless platform (no event loop)");

        self.with_state(|state| {
            state.is_running = true;
        });

        // In headless mode, just call on_ready and return immediately
        on_ready();

        tracing::info!("Headless platform ready");
    }

    fn quit(&self) {
        tracing::info!("Quitting headless platform");

        self.with_state(|state| {
            state.is_running = false;
            state.handlers.invoke_quit();
        });
    }

    fn request_frame(&self) {
        // No-op in headless mode
    }

    fn open_window(&self, options: WindowOptions) -> Result<Box<dyn PlatformWindow>> {
        tracing::info!(?options, "Creating mock window");

        self.with_state(|state| {
            let window_id = WindowId(state.windows.len() as u64);
            let window = MockWindow::new(window_id, options.clone());

            state.windows.push(window.clone());
            state.active_window = Some(window_id);

            // Invoke window created event
            state
                .handlers
                .invoke_window_event(WindowEvent::Created(window_id));

            Ok(Box::new(window) as Box<dyn PlatformWindow>)
        })
    }

    fn active_window(&self) -> Option<WindowId> {
        self.with_state(|state| state.active_window)
    }

    fn window_stack(&self) -> Option<Vec<WindowId>> {
        Some(self.with_state(|state| state.windows.iter().map(|w| w.id).collect()))
    }

    fn displays(&self) -> Vec<Arc<dyn PlatformDisplay>> {
        // Return one mock display
        vec![Arc::new(MockDisplay::primary())]
    }

    fn primary_display(&self) -> Option<Arc<dyn PlatformDisplay>> {
        Some(Arc::new(MockDisplay::primary()))
    }

    fn clipboard(&self) -> Arc<dyn Clipboard> {
        self.with_state(|state| state.clipboard.clone())
    }

    fn capabilities(&self) -> &dyn PlatformCapabilities {
        unsafe { &*(&self.with_state(|state| state.capabilities) as *const _) }
    }

    fn name(&self) -> &'static str {
        "Headless"
    }

    fn on_quit(&self, callback: Box<dyn FnMut() + Send>) {
        self.with_state(|state| {
            state.handlers.quit = Some(callback);
        });
    }

    fn on_window_event(&self, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        self.with_state(|state| {
            state.handlers.window_event = Some(callback);
        });
    }

    fn app_path(&self) -> Result<PathBuf> {
        Ok(PathBuf::from("/mock/app/path"))
    }
}

// ==================== Mock Implementations ====================

/// Mock window for headless testing
#[derive(Clone)]
struct MockWindow {
    id: WindowId,
    size: Size<Pixels>,
    scale_factor: f64,
    focused: bool,
    visible: bool,
}

impl MockWindow {
    fn new(id: WindowId, options: WindowOptions) -> Self {
        Self {
            id,
            size: options.size,
            scale_factor: 1.0,
            focused: true,
            visible: options.visible,
        }
    }
}

impl crate::traits::PlatformWindow for MockWindow {
    fn physical_size(&self) -> Size<DevicePixels> {
        use flui_types::geometry::device_px;

        Size::new(
            device_px((self.size.width.0 * self.scale_factor as f32) as i32),
            device_px((self.size.height.0 * self.scale_factor as f32) as i32),
        )
    }

    fn logical_size(&self) -> Size<Pixels> {
        self.size
    }

    fn scale_factor(&self) -> f64 {
        self.scale_factor
    }

    fn request_redraw(&self) {
        // No-op
    }

    fn is_focused(&self) -> bool {
        self.focused
    }

    fn is_visible(&self) -> bool {
        self.visible
    }
}

/// Mock display for headless testing
struct MockDisplay {
    is_primary: bool,
}

impl MockDisplay {
    fn primary() -> Self {
        Self { is_primary: true }
    }
}

impl crate::traits::PlatformDisplay for MockDisplay {
    fn id(&self) -> crate::traits::DisplayId {
        crate::traits::DisplayId(0)
    }

    fn name(&self) -> String {
        "Mock Display".to_string()
    }

    fn bounds(&self) -> Bounds<DevicePixels> {
        use flui_types::geometry::device_px;

        // Mock display: 1920x1080 at origin (0, 0)
        Bounds::new(
            Point::new(device_px(0), device_px(0)),
            Size::new(device_px(1920), device_px(1080)),
        )
    }

    fn scale_factor(&self) -> f64 {
        1.0
    }

    fn is_primary(&self) -> bool {
        self.is_primary
    }
}

/// Test executor that runs tasks immediately
struct TestExecutor {
    name: String,
}

impl TestExecutor {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }
}

impl PlatformExecutor for TestExecutor {
    fn spawn(&self, task: Box<dyn FnOnce() + Send>) {
        tracing::trace!(executor = %self.name, "Running task immediately");
        task();
    }

    fn is_on_executor(&self) -> bool {
        true // Always on executor in test mode
    }
}

/// Mock text system
struct MockTextSystem;

impl PlatformTextSystem for MockTextSystem {
    fn default_font_family(&self) -> String {
        "Mock Font".to_string()
    }
}

/// Mock clipboard with in-memory storage
struct MockClipboard {
    content: Mutex<Option<String>>,
}

impl MockClipboard {
    fn new() -> Self {
        Self {
            content: Mutex::new(None),
        }
    }
}

impl Clipboard for MockClipboard {
    fn read_text(&self) -> Option<String> {
        self.content.lock().clone()
    }

    fn write_text(&self, text: String) {
        *self.content.lock() = Some(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_headless_platform_creation() {
        let platform = HeadlessPlatform::new();
        assert_eq!(platform.name(), "Headless");
        assert!(platform.active_window().is_none());
    }

    #[test]
    fn test_mock_clipboard() {
        let clipboard = MockClipboard::new();
        assert_eq!(clipboard.read_text(), None);

        clipboard.write_text("test".to_string());
        assert_eq!(clipboard.read_text(), Some("test".to_string()));
    }

    #[test]
    fn test_mock_window_creation() {
        let platform = HeadlessPlatform::new();

        let options = WindowOptions {
            title: "Test".to_string(),
            size: Size::new(
                flui_types::geometry::px(800.0),
                flui_types::geometry::px(600.0),
            ),
            ..Default::default()
        };

        let window = platform.open_window(options).unwrap();
        assert_eq!(
            window.logical_size(),
            Size::new(
                flui_types::geometry::px(800.0),
                flui_types::geometry::px(600.0)
            )
        );
        assert!(window.is_focused());
        assert!(window.is_visible());
    }
}
