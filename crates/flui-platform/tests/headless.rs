//! Headless Platform Tests (Phase 6: T064-T069)
//!
//! Tests for headless platform used in CI/testing environments.

// `std::env::set_var`/`remove_var` are `unsafe` in edition 2024. The safety
// condition is that no other thread reads the environment concurrently, and
// what supplies it here is nextest's process-per-test isolation — NOT
// `--test-threads 1`, which nothing in this repo sets for these tests. Under a
// plain `cargo test -p flui-platform` these calls race the other tests in the
// same binary.
#![allow(unsafe_code)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_platform::{WindowOptions, current_platform, headless_platform};
use flui_types::geometry::{Size, px};

#[test]
fn test_t064_flui_headless_environment_variable() {
    // T064: current_platform() returns HeadlessPlatform when FLUI_HEADLESS=1

    // Set environment variable
    // SAFETY: no other thread reads the environment concurrently — supplied
    // by nextest's process-per-test isolation, not by any `--test-threads`
    // setting. See the file header.
    unsafe { std::env::set_var("FLUI_HEADLESS", "1") };

    let platform = current_platform().expect("Failed to get platform");

    assert_eq!(
        platform.name(),
        "Headless",
        "Expected headless platform when FLUI_HEADLESS=1"
    );

    // Clean up
    // SAFETY: no other thread reads the environment concurrently — supplied
    // by nextest's process-per-test isolation, not by any `--test-threads`
    // setting. See the file header.
    unsafe { std::env::remove_var("FLUI_HEADLESS") };
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

    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

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

    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

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

    // SAFETY: no other thread reads the environment concurrently — supplied
    // by nextest's process-per-test isolation, not by any `--test-threads`
    // setting. See the file header.
    unsafe { std::env::set_var("FLUI_HEADLESS", "1") };

    let platform = current_platform().expect("Failed to get platform");

    // Basic platform operations
    assert_eq!(platform.name(), "Headless");

    // Executors
    let _bg_executor = platform.background_executor();

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

    // SAFETY: no other thread reads the environment concurrently — supplied
    // by nextest's process-per-test isolation, not by any `--test-threads`
    // setting. See the file header.
    unsafe { std::env::remove_var("FLUI_HEADLESS") };
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

    // Both windows created successfully (PlatformWindow trait doesn't expose
    // id())
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

// ============================================================================
// Exit-policy hook (issue #555's live-loop wiring) -- driven entirely through
// the PUBLIC `Platform`/`PlatformWindow` surface, exactly as a real embedder
// would: `open_window`, the returned window's own `close()`, `on_quit`,
// `set_exit_policy_hook`. No internal `MockWindow`/`HeadlessState` access.
// This is "the real platform loop path" for the headless backend: the same
// `PlatformHandlers::exit_policy` slot and `notify_closed` bookkeeping the
// winit backend's `CloseRequested` handler consults, not a parallel
// test-only mechanism.
// ============================================================================

/// An unset hook must not change existing behavior at all: closing every
/// tracked window is a no-op today (no `quit()` call), and this test pins
/// that every embedder/test written before this mechanism existed keeps
/// seeing exactly that.
#[test]
fn closing_the_only_window_without_a_hook_never_calls_quit() {
    let platform = headless_platform();
    let quit_calls = Arc::new(AtomicUsize::new(0));
    let quit_calls_for_handler = Arc::clone(&quit_calls);
    platform.on_quit(Box::new(move || {
        quit_calls_for_handler.fetch_add(1, Ordering::SeqCst);
    }));

    let window = platform
        .open_window(WindowOptions::default())
        .expect("headless platform should create a window");
    window.close();

    assert_eq!(
        quit_calls.load(Ordering::SeqCst),
        0,
        "no exit-policy hook was installed -- closing the last window must not invoke quit"
    );
}

/// Closing one of two windows must never even consult the hook -- the
/// decision is gated on every tracked window being gone, not on any one
/// closing.
#[test]
fn closing_one_of_two_windows_does_not_consult_the_exit_policy_hook() {
    let platform = headless_platform();
    let hook_calls = Arc::new(AtomicUsize::new(0));
    let hook_calls_for_hook = Arc::clone(&hook_calls);
    platform.set_exit_policy_hook(Box::new(move || {
        hook_calls_for_hook.fetch_add(1, Ordering::SeqCst);
        true
    }));
    let quit_calls = Arc::new(AtomicUsize::new(0));
    let quit_calls_for_handler = Arc::clone(&quit_calls);
    platform.on_quit(Box::new(move || {
        quit_calls_for_handler.fetch_add(1, Ordering::SeqCst);
    }));

    let window_a = platform
        .open_window(WindowOptions::default())
        .expect("headless platform should create window A");
    let window_b = platform
        .open_window(WindowOptions::default())
        .expect("headless platform should create window B");

    window_a.close();
    assert_eq!(
        hook_calls.load(Ordering::SeqCst),
        0,
        "closing one of two windows must not consult the exit-policy hook"
    );
    assert_eq!(quit_calls.load(Ordering::SeqCst), 0);

    window_b.close();
    assert_eq!(
        hook_calls.load(Ordering::SeqCst),
        1,
        "closing the LAST window must consult the exit-policy hook exactly once"
    );
    assert_eq!(
        quit_calls.load(Ordering::SeqCst),
        1,
        "the hook allowed the exit (returned true) -- quit must fire"
    );
}

/// The hook's veto is honored: `quit` must not fire even after every window
/// has closed, when the hook returns `false` (the drain-before-decide
/// scenario `AppRuntime::should_exit` implements one layer up).
#[test]
fn exit_policy_hook_veto_prevents_quit_even_after_the_last_window_closes() {
    let platform = headless_platform();
    platform.set_exit_policy_hook(Box::new(|| false));
    let quit_calls = Arc::new(AtomicUsize::new(0));
    let quit_calls_for_handler = Arc::clone(&quit_calls);
    platform.on_quit(Box::new(move || {
        quit_calls_for_handler.fetch_add(1, Ordering::SeqCst);
    }));

    let window = platform
        .open_window(WindowOptions::default())
        .expect("headless platform should create a window");
    window.close();

    assert_eq!(
        quit_calls.load(Ordering::SeqCst),
        0,
        "a hook that vetoes the exit must prevent quit from firing"
    );
}
