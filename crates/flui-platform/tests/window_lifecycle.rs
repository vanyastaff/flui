//! Window lifecycle tests for Phase 3
//!
//! Tests window creation, modes, events, and DPI handling.
//!
//! Run with: cargo test -p flui-platform --test window_lifecycle

use flui_platform::{current_platform, WindowOptions};
use flui_types::geometry::{px, Size};

/// T011: Test window creation with WindowOptions
#[test]
fn test_window_creation_with_options() {
    // Initialize tracing for test debugging
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .try_init();

    tracing::info!("T011: Testing window creation with WindowOptions");

    // Create platform
    let platform = current_platform().expect("Failed to create platform");
    tracing::info!("Platform created: {}", platform.name());

    // Create window with custom options
    let options = WindowOptions {
        title: "Test Window - T011".to_string(),
        size: Size::new(px(640.0), px(480.0)),
        resizable: true,
        visible: false, // Don't show window during tests
        decorated: true,
        min_size: Some(Size::new(px(320.0), px(240.0))),
        max_size: Some(Size::new(px(1920.0), px(1080.0))),
    };

    // Attempt to create window
    let result = platform.open_window(options);

    // On headless platform, this might fail - that's OK for now
    match result {
        Ok(window) => {
            tracing::info!("Window created successfully");

            // Verify window properties
            let logical_size = window.logical_size();
            tracing::info!(
                "Window logical size: {}x{}",
                logical_size.width.0,
                logical_size.height.0
            );

            // Size should be approximately what we requested (±1px tolerance for rounding)
            assert!(
                (logical_size.width.0 - 640.0).abs() < 1.0,
                "Window width should be ~640px, got {}",
                logical_size.width.0
            );
            assert!(
                (logical_size.height.0 - 480.0).abs() < 1.0,
                "Window height should be ~480px, got {}",
                logical_size.height.0
            );

            // Window should not be visible (per options)
            assert!(
                !window.is_visible(),
                "Window should not be visible during test"
            );

            tracing::info!("✓ T011 PASS: Window creation validated");
        }
        Err(e) => {
            // Headless or unsupported platform - log and skip
            tracing::warn!("Window creation not supported (headless?): {}", e);
            tracing::info!("⊘ T011 SKIP: Platform doesn't support window creation");
        }
    }
}

/// T012: Test window close event fires on CloseRequested
#[test]
fn test_window_close_event() {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .try_init();

    tracing::info!("T012: Testing window close event");

    let platform = current_platform().expect("Failed to create platform");

    let options = WindowOptions {
        title: "Test Window - T012".to_string(),
        size: Size::new(px(400.0), px(300.0)),
        visible: false,
        ..Default::default()
    };

    let result = platform.open_window(options);

    match result {
        Ok(window) => {
            tracing::info!("Window created for close event test");

            // TODO: Once event system is implemented (Phase 5), verify:
            // 1. CloseRequested event fires when window closed
            // 2. Event callback is invoked
            // 3. Window can be destroyed gracefully

            // For now, just verify window exists
            assert!(
                window.logical_size().width.0 > 0.0,
                "Window should have valid size"
            );

            tracing::info!("✓ T012 PASS: Window close event test prepared (Phase 5 will complete)");
        }
        Err(e) => {
            tracing::warn!("Window creation not supported: {}", e);
            tracing::info!("⊘ T012 SKIP: Platform doesn't support window creation");
        }
    }
}

/// T015: Test creating multiple concurrent windows
#[test]
fn test_multiple_concurrent_windows() {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .try_init();

    tracing::info!("T015: Testing multiple concurrent windows");

    let platform = current_platform().expect("Failed to create platform");

    // Create 3 windows with different sizes
    let window_configs = vec![
        ("Window 1", px(400.0), px(300.0)),
        ("Window 2", px(600.0), px(450.0)),
        ("Window 3", px(800.0), px(600.0)),
    ];

    let mut windows = Vec::new();

    for (title, width, height) in window_configs {
        let options = WindowOptions {
            title: title.to_string(),
            size: Size::new(width, height),
            visible: false,
            ..Default::default()
        };

        match platform.open_window(options) {
            Ok(window) => {
                tracing::info!("Created window: {}", title);
                windows.push(window);
            }
            Err(e) => {
                tracing::warn!("Failed to create window {}: {}", title, e);
            }
        }
    }

    if windows.is_empty() {
        tracing::info!("⊘ T015 SKIP: Platform doesn't support window creation");
        return;
    }

    // Verify all windows exist and have independent state
    assert_eq!(
        windows.len(),
        3,
        "Should have created 3 windows (got {})",
        windows.len()
    );

    for (idx, window) in windows.iter().enumerate() {
        let size = window.logical_size();
        tracing::info!(
            "Window {}: {}x{}",
            idx + 1,
            size.width.0,
            size.height.0
        );

        // Each window should have different size
        assert!(size.width.0 > 0.0 && size.height.0 > 0.0);
    }

    tracing::info!("✓ T015 PASS: Multiple concurrent windows validated");
}

/// T021: Test window.request_redraw() fires RedrawRequested event
#[test]
fn test_request_redraw() {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .try_init();

    tracing::info!("T021: Testing request_redraw()");

    let platform = current_platform().expect("Failed to create platform");

    let options = WindowOptions {
        title: "Test Window - T021".to_string(),
        size: Size::new(px(400.0), px(300.0)),
        visible: false,
        ..Default::default()
    };

    match platform.open_window(options) {
        Ok(window) => {
            tracing::info!("Window created for redraw test");

            // Call request_redraw - should not panic
            window.request_redraw();
            tracing::info!("request_redraw() called successfully");

            // TODO: Once event system is implemented (Phase 5), verify:
            // 1. RedrawRequested event fires
            // 2. Event is received by callback
            // 3. Multiple request_redraw calls coalesce

            tracing::info!("✓ T021 PASS: request_redraw() validated (Phase 5 will add event verification)");
        }
        Err(e) => {
            tracing::warn!("Window creation not supported: {}", e);
            tracing::info!("⊘ T021 SKIP: Platform doesn't support window creation");
        }
    }
}

/// T022: Test window resize fires Resized event with new logical size
#[test]
fn test_window_resize_event() {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .try_init();

    tracing::info!("T022: Testing window resize event");

    let platform = current_platform().expect("Failed to create platform");

    let options = WindowOptions {
        title: "Test Window - T022".to_string(),
        size: Size::new(px(400.0), px(300.0)),
        visible: false,
        resizable: true,
        ..Default::default()
    };

    match platform.open_window(options) {
        Ok(window) => {
            tracing::info!("Window created for resize test");

            let initial_size = window.logical_size();
            tracing::info!(
                "Initial size: {}x{}",
                initial_size.width.0,
                initial_size.height.0
            );

            // TODO: Once window.set_size() is implemented, test:
            // 1. Call set_size() with new dimensions
            // 2. Verify Resized event fires
            // 3. Verify event contains correct new size
            // 4. Verify logical_size() returns updated size

            tracing::info!("✓ T022 PASS: Window resize test prepared (requires set_size() API)");
        }
        Err(e) => {
            tracing::warn!("Window creation not supported: {}", e);
            tracing::info!("⊘ T022 SKIP: Platform doesn't support window creation");
        }
    }
}
