//! Headless platform implementation for testing
//!
//! This platform implementation runs without any actual windowing system,
//! making it ideal for unit tests and CI environments.

use crate::cursor::CursorStyle;
use crate::shared::{PlatformHandlers, WindowCallbacks};
use crate::traits::{
    Clipboard, ClipboardItem, DesktopCapabilities, DispatchEventResult, Platform,
    PlatformCapabilities, PlatformDisplay, PlatformExecutor, PlatformInput, PlatformTextSystem,
    PlatformWindow, WindowAppearance, WindowBackgroundAppearance, WindowBounds, WindowEvent,
    WindowId, WindowOptions,
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
    // US3 state
    cursor_style: CursorStyle,
    appearance: WindowAppearance,
    keyboard_layout: String,
    opened_urls: Vec<String>,
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
            cursor_style: CursorStyle::default(),
            appearance: WindowAppearance::default(),
            keyboard_layout: "en-US".to_string(),
            opened_urls: Vec::new(),
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

    // ==================== US3 Methods ====================

    fn activate(&self, _ignoring_other_apps: bool) {
        // No-op in headless mode
    }

    fn window_appearance(&self) -> WindowAppearance {
        self.with_state(|state| state.appearance)
    }

    fn set_cursor_style(&self, style: CursorStyle) {
        self.with_state(|state| {
            state.cursor_style = style;
        });
    }

    fn write_to_clipboard(&self, item: ClipboardItem) {
        if let Some(text) = item.text_content() {
            self.with_state(|state| {
                state.clipboard.write_text(text.to_string());
            });
        }
    }

    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        self.with_state(|state| state.clipboard.read_text().map(ClipboardItem::text))
    }

    fn open_url(&self, url: &str) {
        self.with_state(|state| {
            state.opened_urls.push(url.to_string());
        });
    }

    fn keyboard_layout(&self) -> String {
        self.with_state(|state| state.keyboard_layout.clone())
    }

    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut() + Send>) {
        self.with_state(|state| {
            state.handlers.keyboard_layout_changed = Some(callback);
        });
    }

    fn app_path(&self) -> Result<PathBuf> {
        Ok(PathBuf::from("/mock/app/path"))
    }
}

// ==================== Mock Implementations ====================

/// Mock window for headless testing
///
/// Supports per-window callback registration and programmatic event injection
/// for testing without a display server.
#[derive(Clone)]
struct MockWindow {
    id: WindowId,
    state: Arc<Mutex<MockWindowState>>,
    callbacks: Arc<WindowCallbacks>,
}

/// Mutable state for headless MockWindow
struct MockWindowState {
    title: String,
    bounds: Bounds<Pixels>,
    scale_factor: f64,
    focused: bool,
    visible: bool,
    maximized: bool,
    fullscreen: bool,
    hovered: bool,
    modifiers: keyboard_types::Modifiers,
    appearance: WindowAppearance,
}

impl Clone for MockWindowState {
    fn clone(&self) -> Self {
        Self {
            title: self.title.clone(),
            bounds: self.bounds,
            scale_factor: self.scale_factor,
            focused: self.focused,
            visible: self.visible,
            maximized: self.maximized,
            fullscreen: self.fullscreen,
            hovered: self.hovered,
            modifiers: self.modifiers,
            appearance: self.appearance,
        }
    }
}

impl MockWindow {
    fn new(id: WindowId, options: WindowOptions) -> Self {
        Self {
            id,
            state: Arc::new(Mutex::new(MockWindowState {
                title: options.title.clone(),
                bounds: Bounds {
                    origin: Point::default(),
                    size: options.size,
                },
                scale_factor: 1.0,
                focused: true,
                visible: options.visible,
                maximized: false,
                fullscreen: false,
                hovered: false,
                modifiers: keyboard_types::Modifiers::empty(),
                appearance: WindowAppearance::default(),
            })),
            callbacks: Arc::new(WindowCallbacks::new()),
        }
    }

    /// Inject a platform input event for testing.
    /// Fires the registered `on_input` callback.
    #[allow(dead_code)]
    pub fn inject_event(&self, event: PlatformInput) -> DispatchEventResult {
        self.callbacks.dispatch_input(event)
    }

    /// Simulate a resize for testing.
    /// Fires the registered `on_resize` callback.
    #[allow(dead_code)]
    pub fn simulate_resize(&self, width: f32, height: f32) {
        use flui_types::geometry::px;
        let size = Size::new(px(width), px(height));
        let scale = self.state.lock().scale_factor as f32;
        self.state.lock().bounds.size = size;
        self.callbacks.dispatch_resize(size, scale);
    }

    /// Simulate focus change for testing.
    /// Fires the registered `on_active_status_change` callback.
    #[allow(dead_code)]
    pub fn simulate_focus(&self, focused: bool) {
        self.state.lock().focused = focused;
        self.callbacks.dispatch_active_status_change(focused);
    }

    /// Simulate close request for testing.
    /// Fires `on_should_close`, then `on_close` if allowed.
    #[allow(dead_code)]
    pub fn simulate_close(&self) -> bool {
        let should = self.callbacks.dispatch_should_close();
        if should {
            self.callbacks.dispatch_close();
        }
        should
    }
}

impl crate::traits::PlatformWindow for MockWindow {
    fn physical_size(&self) -> Size<DevicePixels> {
        use flui_types::geometry::device_px;

        let state = self.state.lock();
        Size::new(
            device_px((state.bounds.size.width.0 * state.scale_factor as f32) as i32),
            device_px((state.bounds.size.height.0 * state.scale_factor as f32) as i32),
        )
    }

    fn logical_size(&self) -> Size<Pixels> {
        self.state.lock().bounds.size
    }

    fn scale_factor(&self) -> f64 {
        self.state.lock().scale_factor
    }

    fn request_redraw(&self) {
        self.callbacks.dispatch_request_frame();
    }

    fn is_focused(&self) -> bool {
        self.state.lock().focused
    }

    fn is_visible(&self) -> bool {
        self.state.lock().visible
    }

    // ==================== Query Methods (US2) ====================

    fn bounds(&self) -> Bounds<Pixels> {
        self.state.lock().bounds
    }

    fn content_size(&self) -> Size<Pixels> {
        self.state.lock().bounds.size
    }

    fn window_bounds(&self) -> WindowBounds {
        let state = self.state.lock();
        if state.fullscreen {
            WindowBounds::Fullscreen(state.bounds)
        } else if state.maximized {
            WindowBounds::Maximized(state.bounds)
        } else {
            WindowBounds::Windowed(state.bounds)
        }
    }

    fn is_maximized(&self) -> bool {
        self.state.lock().maximized
    }

    fn is_fullscreen(&self) -> bool {
        self.state.lock().fullscreen
    }

    fn is_active(&self) -> bool {
        self.state.lock().focused
    }

    fn is_hovered(&self) -> bool {
        self.state.lock().hovered
    }

    fn mouse_position(&self) -> Point<Pixels> {
        Point::default()
    }

    fn modifiers(&self) -> keyboard_types::Modifiers {
        self.state.lock().modifiers
    }

    fn appearance(&self) -> WindowAppearance {
        self.state.lock().appearance
    }

    fn display(&self) -> Option<Arc<dyn PlatformDisplay>> {
        Some(Arc::new(MockDisplay::primary()))
    }

    fn get_title(&self) -> String {
        self.state.lock().title.clone()
    }

    // ==================== Control Methods (US2) ====================

    fn set_title(&self, title: &str) {
        self.state.lock().title = title.to_string();
    }

    fn activate(&self) {
        self.state.lock().focused = true;
    }

    fn minimize(&self) {
        let mut state = self.state.lock();
        state.maximized = false;
        state.fullscreen = false;
    }

    fn maximize(&self) {
        let mut state = self.state.lock();
        state.maximized = true;
        state.fullscreen = false;
    }

    fn restore(&self) {
        let mut state = self.state.lock();
        state.maximized = false;
        state.fullscreen = false;
    }

    fn toggle_fullscreen(&self) {
        let mut state = self.state.lock();
        state.fullscreen = !state.fullscreen;
        if state.fullscreen {
            state.maximized = false;
        }
    }

    fn resize(&self, size: Size<Pixels>) {
        self.state.lock().bounds.size = size;
    }

    fn close(&self) {
        self.callbacks.dispatch_close();
    }

    fn set_background_appearance(&self, _appearance: WindowBackgroundAppearance) {
        // No-op in headless mode
    }

    // ==================== Callbacks (US1) ====================

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

    #[test]
    fn test_on_input_callback() {
        use crate::traits::{DispatchEventResult, PlatformInput};
        use std::sync::atomic::{AtomicBool, Ordering};

        let window = MockWindow::new(WindowId(0), WindowOptions::default());

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        window.on_input(Box::new(move |_event| {
            called_clone.store(true, Ordering::SeqCst);
            DispatchEventResult::default()
        }));

        // Inject a keyboard event
        let event = PlatformInput::Keyboard(crate::traits::KeyboardEvent {
            key: crate::traits::Key::Named(keyboard_types::NamedKey::Enter),
            modifiers: keyboard_types::Modifiers::empty(),
            is_down: true,
            is_repeat: false,
        });

        window.inject_event(event);
        assert!(called.load(Ordering::SeqCst));
    }

    #[test]
    fn test_on_resize_callback() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let window = MockWindow::new(WindowId(0), WindowOptions::default());

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        window.on_resize(Box::new(move |size, _scale| {
            assert_eq!(size.width.0, 1024.0);
            assert_eq!(size.height.0, 768.0);
            called_clone.store(true, Ordering::SeqCst);
        }));

        window.simulate_resize(1024.0, 768.0);
        assert!(called.load(Ordering::SeqCst));
    }

    #[test]
    fn test_on_should_close_veto() {
        let window = MockWindow::new(WindowId(0), WindowOptions::default());

        // Register a callback that vetoes close
        window.on_should_close(Box::new(|| false));

        // Close should be vetoed
        assert!(!window.simulate_close());
    }

    #[test]
    fn test_on_close_callback() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let window = MockWindow::new(WindowId(0), WindowOptions::default());

        let closed = Arc::new(AtomicBool::new(false));
        let closed_clone = closed.clone();

        window.on_close(Box::new(move || {
            closed_clone.store(true, Ordering::SeqCst);
        }));

        // No on_should_close registered → defaults to allow
        assert!(window.simulate_close());
        assert!(closed.load(Ordering::SeqCst));
    }

    #[test]
    fn test_on_active_status_change() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let window = MockWindow::new(WindowId(0), WindowOptions::default());

        let focused = Arc::new(AtomicBool::new(false));
        let focused_clone = focused.clone();

        window.on_active_status_change(Box::new(move |is_active| {
            focused_clone.store(is_active, Ordering::SeqCst);
        }));

        window.simulate_focus(true);
        assert!(focused.load(Ordering::SeqCst));

        window.simulate_focus(false);
        assert!(!focused.load(Ordering::SeqCst));
    }

    // ==================== US2 Tests ====================

    #[test]
    fn test_set_title_get_title() {
        let window = MockWindow::new(
            WindowId(0),
            WindowOptions {
                title: "Original".to_string(),
                ..Default::default()
            },
        );

        assert_eq!(window.get_title(), "Original");
        window.set_title("Updated");
        assert_eq!(window.get_title(), "Updated");
    }

    #[test]
    fn test_window_bounds_query() {
        use flui_types::geometry::px;

        let window = MockWindow::new(
            WindowId(0),
            WindowOptions {
                size: Size::new(px(800.0), px(600.0)),
                ..Default::default()
            },
        );

        let bounds = window.bounds();
        assert_eq!(bounds.size.width.0, 800.0);
        assert_eq!(bounds.size.height.0, 600.0);

        assert_eq!(window.content_size(), Size::new(px(800.0), px(600.0)));

        match window.window_bounds() {
            WindowBounds::Windowed(b) => assert_eq!(b.size.width.0, 800.0),
            _ => panic!("Expected Windowed"),
        }
    }

    #[test]
    fn test_maximize_restore_fullscreen() {
        let window = MockWindow::new(WindowId(0), WindowOptions::default());

        assert!(!window.is_maximized());
        assert!(!window.is_fullscreen());

        window.maximize();
        assert!(window.is_maximized());
        assert!(!window.is_fullscreen());
        assert!(matches!(window.window_bounds(), WindowBounds::Maximized(_)));

        window.restore();
        assert!(!window.is_maximized());
        assert!(matches!(window.window_bounds(), WindowBounds::Windowed(_)));

        window.toggle_fullscreen();
        assert!(window.is_fullscreen());
        assert!(!window.is_maximized());
        assert!(matches!(
            window.window_bounds(),
            WindowBounds::Fullscreen(_)
        ));

        window.toggle_fullscreen();
        assert!(!window.is_fullscreen());
    }

    #[test]
    fn test_resize() {
        use flui_types::geometry::px;

        let window = MockWindow::new(WindowId(0), WindowOptions::default());
        window.resize(Size::new(px(1920.0), px(1080.0)));
        assert_eq!(window.logical_size(), Size::new(px(1920.0), px(1080.0)));
    }

    #[test]
    fn test_display_query() {
        let window = MockWindow::new(WindowId(0), WindowOptions::default());
        let display = window.display();
        assert!(display.is_some());
        assert!(display.unwrap().is_primary());
    }

    // ==================== US3 Tests ====================

    #[test]
    fn test_window_appearance() {
        let platform = HeadlessPlatform::new();
        assert_eq!(platform.window_appearance(), WindowAppearance::Light);
    }

    #[test]
    fn test_set_cursor_style() {
        let platform = HeadlessPlatform::new();
        platform.set_cursor_style(CursorStyle::IBeam);
        platform.with_state(|state| {
            assert_eq!(state.cursor_style, CursorStyle::IBeam);
        });
    }

    #[test]
    fn test_clipboard_item_roundtrip() {
        let platform = HeadlessPlatform::new();
        let item = ClipboardItem::text("hello world".to_string());
        platform.write_to_clipboard(item);
        let read = platform.read_from_clipboard();
        assert!(read.is_some());
        assert_eq!(read.unwrap().text_content(), Some("hello world"));
    }

    #[test]
    fn test_open_url_tracking() {
        let platform = HeadlessPlatform::new();
        platform.open_url("https://example.com");
        platform.open_url("https://rust-lang.org");
        platform.with_state(|state| {
            assert_eq!(state.opened_urls.len(), 2);
            assert_eq!(state.opened_urls[0], "https://example.com");
            assert_eq!(state.opened_urls[1], "https://rust-lang.org");
        });
    }

    #[test]
    fn test_keyboard_layout() {
        let platform = HeadlessPlatform::new();
        assert_eq!(platform.keyboard_layout(), "en-US");
    }

    #[test]
    fn test_keyboard_layout_change_callback() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let platform = HeadlessPlatform::new();
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        platform.on_keyboard_layout_change(Box::new(move || {
            called_clone.store(true, Ordering::SeqCst);
        }));

        // Simulate a keyboard layout change by invoking the handler
        platform.with_state(|state| {
            state.handlers.invoke_keyboard_layout_changed();
        });

        assert!(called.load(Ordering::SeqCst));
    }
}
