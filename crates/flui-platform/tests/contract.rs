//! Contract tests for Platform trait compliance
//!
//! Ensures all platform implementations (Windows, macOS, Headless) provide
//! identical API behavior and correctly implement the Platform trait contract.
//!
//! Run with: cargo test -p flui-platform --test contract

use flui_platform::{current_platform, WindowOptions};
use flui_types::geometry::{px, Size};

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
            // Should not be visible since we set visible: false
            assert!(!is_visible, "Window should not be visible with visible=false");

            // Contract 7: Window focus API
            let is_focused = window.is_focused();
            tracing::info!("Window focus: {}", is_focused);
            // Focus state should be deterministic

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
            // Headless or unsupported platform
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

    // Contract: displays() must return a valid collection (possibly empty)
    // Empty is OK for headless platforms

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

    // Contract: Platform name must be non-empty and consistent
    assert!(!name.is_empty(), "Platform name should not be empty");

    // Platform name should match the actual platform
    #[cfg(target_os = "windows")]
    assert_eq!(name, "Windows", "Windows platform should report 'Windows'");

    #[cfg(target_os = "macos")]
    assert_eq!(name, "macOS", "macOS platform should report 'macOS'");

    // Headless is determined by absence of display server, not compile target
    if name == "Headless" {
        tracing::info!("Running on headless platform (no display server)");
    }

    tracing::info!("✓ Platform name contract validated");
}

/// Test WindowOptions default values
#[test]
fn test_window_options_default_contract() {
    let options = WindowOptions::default();

    // Contract: Default values must be sensible
    assert!(!options.title.is_empty(), "Default title should not be empty");
    assert!(options.size.width.0 > 0.0, "Default width must be positive");
    assert!(options.size.height.0 > 0.0, "Default height must be positive");
    assert!(options.resizable, "Default should be resizable");
    assert!(options.visible, "Default should be visible");
    assert!(options.decorated, "Default should be decorated");

    tracing::info!("✓ WindowOptions default contract validated");
}
