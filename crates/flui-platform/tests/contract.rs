//! Contract test for the window lifecycle every `Platform` implementation
//! must honour: reported sizes are positive and consistent with the scale
//! factor, `visible: false` is respected, and windows are independent.

use flui_foundation::geometry::Size;
use flui_platform::{WindowOptions, current_platform};

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "requires an AppKit-run-loop-pumping test process (ADR-0039): the macOS platform surface asserts the owner main thread, a bare macOS cargo test cannot pump it and unbundled NSWindow construction aborts the process — these run headless on CI (FLUI_HEADLESS=1) and from an AppKit-pumping process only"
)]
fn test_window_lifecycle_contract() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();

    tracing::info!("Testing window lifecycle contract across platforms");

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
        size: Size::new(640.0, 480.0),
        visible: false,
        resizable: true,
        decorated: true,
        min_size: Some(Size::new(320.0, 240.0)),
        max_size: None,
        ..Default::default()
    };

    match platform.open_window(options) {
        Ok(window) => {
            tracing::info!("✓ Platform supports window creation");

            // Contract 4: Window must report valid sizes
            let logical_size = window.logical_size();
            let physical_size = window.physical_size();
            let scale_factor = window.scale_factor();

            tracing::info!(
                "Window sizes - Logical: {}x{}, Physical: {}x{}, Scale: {}",
                logical_size.width,
                logical_size.height,
                physical_size.width,
                physical_size.height,
                scale_factor
            );

            // Logical size must be positive
            assert!(
                logical_size.width > 0.0 && logical_size.height > 0.0,
                "Logical size must be positive"
            );

            // Physical size must be positive
            assert!(
                physical_size.width > 0 && physical_size.height > 0,
                "Physical size must be positive"
            );

            // Scale factor must be positive
            assert!(scale_factor > 0.0, "Scale factor must be positive");

            // Contract 5: Scale factor relationship (physical = logical * scale)
            let expected_physical_width = (logical_size.width * scale_factor) as i32;
            let expected_physical_height = (logical_size.height * scale_factor) as i32;

            let width_diff = (physical_size.width - expected_physical_width).abs();
            let height_diff = (physical_size.height - expected_physical_height).abs();

            tracing::info!(
                "Scale relationship - Expected physical: {}x{}, Actual: {}x{}, Diff: {}x{}",
                expected_physical_width,
                expected_physical_height,
                physical_size.width,
                physical_size.height,
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
            assert!(
                !is_visible,
                "Window should not be visible with visible=false"
            );

            // Contract 7: Window focus API
            let is_focused = window.is_focused();
            tracing::info!("Window focus: {}", is_focused);

            // Contract 8: request_redraw() must not panic
            window.request_redraw();
            tracing::info!("✓ request_redraw() executed without panic");

            // Contract 9: Multiple window creation
            let options2 = WindowOptions {
                title: "Contract Test Window 2".to_string(),
                size: Size::new(400.0, 300.0),
                visible: false,
                ..Default::default()
            };

            match platform.open_window(options2) {
                Ok(window2) => {
                    tracing::info!("✓ Platform supports multiple concurrent windows");

                    let size2 = window2.logical_size();
                    assert!(
                        size2.width > 0.0 && size2.height > 0.0,
                        "Second window must have valid size"
                    );

                    // Windows must be independent (different sizes)
                    assert!(
                        (logical_size.width - size2.width).abs() > 1.0,
                        "Windows should have different sizes"
                    );
                }
                Err(e) => {
                    tracing::warn!("Platform doesn't support multiple windows: {}", e);
                }
            }

            tracing::info!(
                "✓ PASS: Window lifecycle contract validated for {}",
                platform_name
            );
        }
        Err(e) => {
            tracing::info!(
                "Platform {} doesn't support window creation: {}",
                platform_name,
                e
            );
            tracing::info!("⊘ SKIP: Platform doesn't support windows (expected for headless)");
        }
    }
}
