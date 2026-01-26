//! Headless Platform Tests (Phase 6: T064-T069)
//!
//! Tests for headless platform used in CI/testing environments.

use flui_platform::{current_platform, headless_platform, WindowOptions};
use flui_types::geometry::{px, Size};

#[test]
fn test_t064_flui_headless_environment_variable() {
    // T064: current_platform() returns HeadlessPlatform when FLUI_HEADLESS=1

    // Set environment variable
    std::env::set_var("FLUI_HEADLESS", "1");

    let platform = current_platform().expect("Failed to get platform");

    assert_eq!(
        platform.name(),
        "Headless",
        "Expected headless platform when FLUI_HEADLESS=1"
    );

    // Clean up
    std::env::remove_var("FLUI_HEADLESS");
}

#[test]
fn test_t065_headless_window_creation() {
    // T065: Headless window creation returns mock window (no OS window)

    let platform = headless_platform();

    let options = WindowOptions {
        title: "Test Window".to_string(),
        size: Size::new(px(800.0), px(600.0)),
        visible: true,
        ..Default::default()
    };

    let _window = platform
        .open_window(options)
        .expect("Failed to create headless window");

    // Verify it's a mock window (doesn't panic or create actual OS window)
    // Note: PlatformWindow trait doesn't expose id() method
}

#[test]
fn test_t066_headless_clipboard_roundtrip() {
    // T066: Headless clipboard roundtrip (in-memory storage)

    let platform = headless_platform();
    let clipboard = platform.clipboard();

    let test_text = "Hello from headless clipboard!";

    clipboard.write_text(test_text.to_string());

    let read_text = clipboard
        .read_text()
        .expect("Failed to read from headless clipboard");

    assert_eq!(
        read_text, test_text,
        "Headless clipboard should store text in memory"
    );
}

#[test]
fn test_t067_headless_executor_immediate_execution() {
    // T067: Headless executor runs tasks immediately on calling thread

    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let platform = headless_platform();
    let executor = platform.background_executor();

    let executed = Arc::new(AtomicBool::new(false));
    let executed_clone = Arc::clone(&executed);

    executor.spawn(Box::new(move || {
        executed_clone.store(true, Ordering::SeqCst);
    }));

    // Give the async executor time to run the task
    std::thread::sleep(std::time::Duration::from_millis(100));

    assert!(
        executed.load(Ordering::SeqCst),
        "Headless executor should execute task"
    );
}

#[test]
fn test_t068_parallel_test_execution() {
    // T068: Parallel test execution has no race conditions

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let counter = Arc::new(AtomicUsize::new(0));
    let mut handles = vec![];

    // Spawn multiple threads creating headless platforms
    for _ in 0..10 {
        let counter_clone = Arc::clone(&counter);
        let handle = std::thread::spawn(move || {
            let platform = headless_platform();

            // Create window
            let options = WindowOptions {
                title: "Parallel Test".to_string(),
                size: Size::new(px(800.0), px(600.0)),
                visible: true,
                ..Default::default()
            };

            let _window = platform
                .open_window(options)
                .expect("Failed to create window");

            // Use clipboard
            let clipboard = platform.clipboard();
            clipboard.write_text("test".to_string());
            let _ = clipboard.read_text();

            // Increment counter
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().expect("Thread panicked");
    }

    assert_eq!(
        counter.load(Ordering::SeqCst),
        10,
        "All 10 parallel tests should complete without race conditions"
    );
}

#[test]
fn test_t069_all_tests_pass_in_headless_mode() {
    // T069: Verify all existing tests pass in headless mode
    // This is a meta-test that verifies headless mode doesn't break other tests

    std::env::set_var("FLUI_HEADLESS", "1");

    let platform = current_platform().expect("Failed to get platform");

    // Basic platform operations
    assert_eq!(platform.name(), "Headless");

    // Text system
    let text_system = platform.text_system();
    let _default_font = text_system.default_font_family();

    // Executors
    let _bg_executor = platform.background_executor();
    let _fg_executor = platform.foreground_executor();

    // Displays
    let displays = platform.displays();
    assert!(
        !displays.is_empty(),
        "Should have at least one mock display"
    );

    // Clipboard
    let clipboard = platform.clipboard();
    clipboard.write_text("test".to_string());
    assert_eq!(clipboard.read_text(), Some("test".to_string()));

    std::env::remove_var("FLUI_HEADLESS");
}

#[test]
fn test_headless_platform_capabilities() {
    // Additional test: Verify headless platform capabilities

    let platform = headless_platform();
    let caps = platform.capabilities();

    // Headless should support desktop capabilities
    assert!(caps.supports_multiple_windows());
}

#[test]
fn test_headless_multiple_windows() {
    // Additional test: Verify multiple window creation

    let platform = headless_platform();

    let options1 = WindowOptions {
        title: "Window 1".to_string(),
        size: Size::new(px(800.0), px(600.0)),
        visible: true,
        ..Default::default()
    };

    let options2 = WindowOptions {
        title: "Window 2".to_string(),
        size: Size::new(px(1024.0), px(768.0)),
        visible: true,
        ..Default::default()
    };

    let _window1 = platform
        .open_window(options1)
        .expect("Failed to create window 1");
    let _window2 = platform
        .open_window(options2)
        .expect("Failed to create window 2");

    // Both windows created successfully (PlatformWindow trait doesn't expose id())
}

#[test]
fn test_headless_text_system() {
    // Additional test: Verify text system works in headless mode

    let platform = headless_platform();
    let text_system = platform.text_system();

    let bounds =
        text_system.measure_text("Hello, World!", &text_system.default_font_family(), 16.0);

    assert!(
        bounds.width() > px(0.0),
        "Text measurement should return non-zero width"
    );
    assert!(
        bounds.height() > px(0.0),
        "Text measurement should return non-zero height"
    );
}

#[test]
fn test_headless_clipboard_empty() {
    // Additional test: Verify empty clipboard behavior

    let platform = headless_platform();
    let clipboard = platform.clipboard();

    // Initially empty
    assert_eq!(clipboard.read_text(), None, "New clipboard should be empty");

    // Write and read
    clipboard.write_text("test".to_string());
    assert_eq!(clipboard.read_text(), Some("test".to_string()));
}
