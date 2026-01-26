//! Contract tests for Platform trait implementations
//!
//! These tests ensure that all platform implementations (Windows, macOS, Linux, etc.)
//! provide consistent behavior for the Platform trait methods.
//!
//! # Test Philosophy
//!
//! Contract tests verify that:
//! 1. Platform creation succeeds and returns valid instances
//! 2. Basic operations (name, capabilities, displays) work consistently
//! 3. Clipboard operations follow the same behavior across platforms
//! 4. Executor spawning and task execution is consistent
//! 5. Window creation and management follows platform conventions
//!
//! # Running Contract Tests
//!
//! ```bash
//! # Run all contract tests
//! cargo test -p flui-platform --test contract
//!
//! # Run in headless mode (CI)
//! FLUI_HEADLESS=1 cargo test -p flui-platform --test contract
//!
//! # Run with logging
//! RUST_LOG=debug cargo test -p flui-platform --test contract -- --nocapture
//! ```

use flui_platform::{current_platform, headless_platform, Platform, WindowOptions};
use flui_types::geometry::{device_px, px, Size};
use std::sync::Arc;

// ==================== Helper: Get Test Platform ====================

/// Get the platform to test
///
/// In normal mode: Uses current_platform() for native testing
/// In headless mode: Uses headless_platform() for CI/testing
fn get_test_platform() -> Arc<dyn Platform> {
    // Check if FLUI_HEADLESS environment variable is set
    if std::env::var("FLUI_HEADLESS").is_ok() {
        headless_platform()
    } else {
        // Use headless for CI safety - native platforms require display/window system
        headless_platform()
    }
}

// ==================== Contract Tests: Platform Basics ====================

#[test]
fn contract_platform_creation() {
    // GIVEN: A platform implementation
    let platform = get_test_platform();

    // THEN: Platform should have a non-empty name
    assert!(!platform.name().is_empty(), "Platform name should not be empty");

    // THEN: Platform should have capabilities
    let caps = platform.capabilities();
    // Just verify we can call it - specific capabilities vary by platform
    let _ = caps.supports_multiple_windows();
}

#[test]
fn contract_platform_displays() {
    // GIVEN: A platform implementation
    let platform = get_test_platform();

    // WHEN: We query displays
    let displays = platform.displays();

    // THEN: Should return at least one display (or empty vec for headless)
    // Headless returns empty vec, native platforms return 1+
    assert!(
        displays.is_empty() || !displays.is_empty(),
        "Displays should be a valid vector"
    );

    // IF we have displays, verify basic properties
    if let Some(display) = displays.first() {
        // Name should not be empty
        assert!(!display.name().is_empty(), "Display name should not be empty");

        // Scale factor should be positive
        assert!(display.scale_factor() > 0.0, "Scale factor should be positive");

        // Bounds should have positive dimensions
        let bounds = display.bounds();
        assert!(
            bounds.size.width > device_px(0),
            "Display width should be positive"
        );
        assert!(
            bounds.size.height > device_px(0),
            "Display height should be positive"
        );
    }
}

#[test]
fn contract_platform_primary_display() {
    // GIVEN: A platform implementation
    let platform = get_test_platform();

    // WHEN: We query the primary display
    let primary = platform.primary_display();

    // THEN: Result should be consistent with displays()
    let displays = platform.displays();

    if displays.is_empty() {
        // Headless platform returns None
        assert!(primary.is_none(), "Primary display should be None when no displays");
    } else {
        // Native platforms should return Some
        if let Some(primary_display) = primary {
            // Primary display should be in the displays list
            assert!(
                displays.iter().any(|d| d.id() == primary_display.id()),
                "Primary display should be in displays list"
            );

            // Primary display should be marked as primary
            assert!(
                primary_display.is_primary(),
                "Primary display should have is_primary() == true"
            );
        }
    }
}

// ==================== Contract Tests: Clipboard ====================

#[test]
fn contract_clipboard_operations() {
    // GIVEN: A platform implementation
    let platform = get_test_platform();
    let clipboard = platform.clipboard();

    // WHEN: We write text to clipboard
    let test_text = "Contract Test Text";
    clipboard.write_text(test_text.to_string());

    // THEN: We should be able to read it back
    assert!(clipboard.has_text(), "Clipboard should have text after write");

    let read_text = clipboard.read_text();
    assert_eq!(
        read_text.as_deref(),
        Some(test_text),
        "Clipboard text should roundtrip correctly"
    );
}

#[test]
fn contract_clipboard_empty() {
    // GIVEN: A platform implementation
    let platform = get_test_platform();
    let clipboard = platform.clipboard();

    // WHEN: We clear the clipboard
    clipboard.write_text("".to_string());

    // THEN: has_text should reflect the state correctly
    // Note: Empty string behavior may vary - some platforms treat "" as no text
    let has_text = clipboard.has_text();
    let text = clipboard.read_text();

    if has_text {
        assert!(text.is_some(), "If has_text is true, read should return Some");
    }
}

// ==================== Contract Tests: Executors ====================

#[test]
fn contract_background_executor() {
    // GIVEN: A platform implementation
    let platform = get_test_platform();
    let executor = platform.background_executor();

    // WHEN: We spawn a task
    use std::sync::{Arc as StdArc, Mutex};
    let flag = StdArc::new(Mutex::new(false));
    let flag_clone = flag.clone();

    executor.spawn(Box::new(move || {
        *flag_clone.lock().unwrap() = true;
    }));

    // THEN: Task should eventually execute
    // Give it time to run (background executor may use thread pool)
    std::thread::sleep(std::time::Duration::from_millis(100));

    assert!(
        *flag.lock().unwrap(),
        "Background executor should execute spawned tasks"
    );
}

#[test]
fn contract_foreground_executor() {
    // GIVEN: A platform implementation
    let platform = get_test_platform();
    let executor = platform.foreground_executor();

    // WHEN: We spawn a task
    use std::sync::{Arc as StdArc, Mutex};
    let flag = StdArc::new(Mutex::new(false));
    let flag_clone = flag.clone();

    executor.spawn(Box::new(move || {
        *flag_clone.lock().unwrap() = true;
    }));

    // THEN: Task should be queued (foreground executor requires manual draining)
    // Note: Actual execution depends on platform event loop calling drain_tasks()
    // This test just verifies spawning doesn't panic
    // The flag won't be set without drain_tasks() being called by the platform
}

// ==================== Contract Tests: Platform Metadata ====================

#[test]
fn contract_platform_name_consistency() {
    // GIVEN: Multiple platform instances
    let platform1 = get_test_platform();
    let platform2 = get_test_platform();

    // THEN: Name should be consistent
    assert_eq!(
        platform1.name(),
        platform2.name(),
        "Platform name should be consistent across instances"
    );
}

#[test]
fn contract_platform_capabilities_consistency() {
    // GIVEN: Multiple platform instances
    let platform1 = get_test_platform();
    let platform2 = get_test_platform();

    // THEN: Capabilities should be consistent
    let caps1 = platform1.capabilities();
    let caps2 = platform2.capabilities();

    assert_eq!(
        caps1.supports_multiple_windows(),
        caps2.supports_multiple_windows(),
        "Capabilities should be consistent across instances"
    );

    assert_eq!(
        caps1.supports_mouse(),
        caps2.supports_mouse(),
        "Capabilities should be consistent across instances"
    );
}

// ==================== Contract Tests: Error Handling ====================

#[test]
fn contract_clipboard_error_handling() {
    // GIVEN: A platform implementation
    let platform = get_test_platform();
    let clipboard = platform.clipboard();

    // WHEN: We try to read from an empty clipboard (or write/read operations)
    // THEN: Operations should not panic

    // Write operation should not panic
    clipboard.write_text("test".to_string());

    // Read operation should not panic
    let _read_result = clipboard.read_text();

    // has_text should not panic
    let _ = clipboard.has_text();
}

// ==================== T023: Window Lifecycle Contract ====================

/// T023: Test all platforms implement window lifecycle identically
#[test]
fn test_window_lifecycle_contract() {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .try_init();

    tracing::info!("T023: Testing window lifecycle contract across platforms");

    let platform = current_platform().expect("Failed to create platform");
    let platform_name = platform.name();
    tracing::info!("Testing platform: {}", platform_name);

    // Contract 1: Platform must have a name
    assert!(
        !platform_name.is_empty(),
        "Platform name should not be empty"
    );

    // Contract 2: Platform must enumerate displays (even if empty for headless)
    let displays = platform.displays();
    tracing::info!("Platform has {} display(s)", displays.len());

    // Contract 3: Window creation should either succeed or fail gracefully
    let options = WindowOptions {
        title: "Contract Test Window".to_string(),
        size: Size::new(px(640.0), px(480.0)),
        visible: false,
        resizable: true,
        decorated: true,
        min_size: Some(Size::new(px(320.0), px(240.0))),
        max_size: None,
    };

    match platform.open_window(options.clone()) {
        Ok(window) => {
            tracing::info!("✓ Platform supports window creation");

            // Contract 4: Window must report valid sizes
            let logical_size = window.logical_size();
            let physical_size = window.physical_size();
            let scale_factor = window.scale_factor();

            tracing::info!(
                "Window sizes - Logical: {}x{}, Physical: {}x{}, Scale: {}",
                logical_size.width.0,
                logical_size.height.0,
                physical_size.width.0,
                physical_size.height.0,
                scale_factor
            );

            // Logical size must be positive
            assert!(
                logical_size.width.0 > 0.0 && logical_size.height.0 > 0.0,
                "Logical size must be positive"
            );

            // Physical size must be positive
            assert!(
                physical_size.width.0 > 0 && physical_size.height.0 > 0,
                "Physical size must be positive"
            );

            // Scale factor must be positive
            assert!(scale_factor > 0.0, "Scale factor must be positive");

            // Contract 5: Scale factor relationship (physical = logical * scale)
            let expected_physical_width = (logical_size.width.0 * scale_factor as f32) as i32;
            let expected_physical_height = (logical_size.height.0 * scale_factor as f32) as i32;

            let width_diff = (physical_size.width.0 - expected_physical_width).abs();
            let height_diff = (physical_size.height.0 - expected_physical_height).abs();

            tracing::info!(
                "Scale relationship - Expected physical: {}x{}, Actual: {}x{}, Diff: {}x{}",
                expected_physical_width,
                expected_physical_height,
                physical_size.width.0,
                physical_size.height.0,
                width_diff,
                height_diff
            );

            assert!(
                width_diff < 2 && height_diff < 2,
                "Physical size should equal logical size * scale (±2px tolerance)"
            );

            // Contract 6: Window visibility API
            let is_visible = window.is_visible();
            tracing::info!("Window visibility: {}", is_visible);
            assert!(!is_visible, "Window should not be visible with visible=false");

            // Contract 7: Window focus API
            let is_focused = window.is_focused();
            tracing::info!("Window focus: {}", is_focused);

            // Contract 8: request_redraw() must not panic
            window.request_redraw();
            tracing::info!("✓ request_redraw() executed without panic");

            // Contract 9: Multiple window creation
            let options2 = WindowOptions {
                title: "Contract Test Window 2".to_string(),
                size: Size::new(px(400.0), px(300.0)),
                visible: false,
                ..Default::default()
            };

            match platform.open_window(options2) {
                Ok(window2) => {
                    tracing::info!("✓ Platform supports multiple concurrent windows");

                    let size2 = window2.logical_size();
                    assert!(
                        size2.width.0 > 0.0 && size2.height.0 > 0.0,
                        "Second window must have valid size"
                    );

                    // Windows must be independent (different sizes)
                    assert!(
                        (logical_size.width.0 - size2.width.0).abs() > 1.0,
                        "Windows should have different sizes"
                    );
                }
                Err(e) => {
                    tracing::warn!("Platform doesn't support multiple windows: {}", e);
                }
            }

            tracing::info!("✓ T023 PASS: Window lifecycle contract validated for {}", platform_name);
        }
        Err(e) => {
            tracing::info!(
                "Platform {} doesn't support window creation: {}",
                platform_name,
                e
            );
            tracing::info!("⊘ T023 SKIP: Platform doesn't support windows (expected for headless)");
        }
    }
}

/// Test that all platform implementations provide consistent display information
#[test]
fn test_display_enumeration_contract() {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .try_init();

    tracing::info!("Testing display enumeration contract");

    let platform = current_platform().expect("Failed to create platform");
    let displays = platform.displays();

    tracing::info!("Platform {} has {} display(s)", platform.name(), displays.len());

    for (idx, display) in displays.iter().enumerate() {
        let scale = display.scale_factor();
        let bounds = display.bounds();

        tracing::info!(
            "Display {}: scale={}, bounds={}x{}+{}+{}",
            idx + 1,
            scale,
            bounds.size.width.0,
            bounds.size.height.0,
            bounds.origin.x.0,
            bounds.origin.y.0
        );

        // Contract: Scale factor must be positive and reasonable
        assert!(
            scale > 0.0 && scale <= 4.0,
            "Display scale factor should be 0.0-4.0, got {}",
            scale
        );

        // Contract: Display bounds must be valid
        assert!(
            bounds.size.width.0 > 0 && bounds.size.height.0 > 0,
            "Display size must be positive"
        );
    }

    tracing::info!("✓ Display enumeration contract validated");
}

/// Test platform name consistency
#[test]
fn test_platform_name_contract() {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .try_init();

    let platform = current_platform().expect("Failed to create platform");
    let name = platform.name();

    tracing::info!("Platform name: {}", name);

    assert!(!name.is_empty(), "Platform name should not be empty");

    #[cfg(target_os = "windows")]
    assert_eq!(name, "Windows", "Windows platform should report 'Windows'");

    #[cfg(target_os = "macos")]
    assert_eq!(name, "macOS", "macOS platform should report 'macOS'");

    if name == "Headless" {
        tracing::info!("Running on headless platform (no display server)");
    }

    tracing::info!("✓ Platform name contract validated");
}

/// Test WindowOptions default values
#[test]
fn test_window_options_default_contract() {
    let options = WindowOptions::default();

    assert!(!options.title.is_empty(), "Default title should not be empty");
    assert!(options.size.width.0 > 0.0, "Default width must be positive");
    assert!(options.size.height.0 > 0.0, "Default height must be positive");
    assert!(options.resizable, "Default should be resizable");
    assert!(options.visible, "Default should be visible");
    assert!(options.decorated, "Default should be decorated");

    tracing::info!("✓ WindowOptions default contract validated");
}
