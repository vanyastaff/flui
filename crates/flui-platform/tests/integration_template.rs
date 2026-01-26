//! Integration Test Template for flui-platform + Other Crates
//!
//! This file serves as a template for writing integration tests that verify
//! cross-crate interactions between flui-platform and other flui crates.
//!
//! # Purpose
//!
//! Integration tests ensure that:
//! - Platform APIs work correctly with rendering pipelines
//! - Window handles are compatible with graphics backends
//! - Events flow correctly through the system
//! - Multiple crates can work together without conflicts
//!
//! # How to Use This Template
//!
//! 1. Copy this file to a new test file (e.g., `integration_painting.rs`)
//! 2. Update the imports to include the crates you're testing
//! 3. Replace placeholder tests with actual integration scenarios
//! 4. Add the dependent crate to `[dev-dependencies]` in Cargo.toml
//! 5. Run with: `cargo test -p flui-platform --test integration_<name>`
//!
//! # Example Structure
//!
//! ```rust,ignore
//! // Import crates being integrated
//! use flui_platform::{current_platform, WindowOptions};
//! use flui_painting::Canvas; // Example dependent crate
//! use flui_types::geometry::{px, Size};
//!
//! #[test]
//! fn test_platform_with_canvas() {
//!     let platform = current_platform().unwrap();
//!     let window = platform.open_window(WindowOptions::default()).unwrap();
//!
//!     // Integration logic here
//!     let canvas = Canvas::from_window(&window);
//!     assert!(canvas.is_valid());
//! }
//! ```

use flui_platform::{current_platform, WindowOptions};
use flui_types::geometry::{px, Size};

// ═══════════════════════════════════════════════════════════════
// SECTION 1: Setup and Initialization
// ═══════════════════════════════════════════════════════════════

/// Initialize tracing for integration tests
fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();
}

/// Create a test platform instance
fn get_test_platform() -> std::sync::Arc<dyn flui_platform::Platform> {
    current_platform().expect("Failed to create platform")
}

/// Create a test window with default options
fn create_test_window() -> Result<Box<dyn flui_platform::PlatformWindow>, anyhow::Error> {
    let platform = get_test_platform();
    let options = WindowOptions {
        title: "Integration Test Window".to_string(),
        size: Size::new(px(800.0), px(600.0)),
        visible: false, // Hidden to avoid UI distraction
        ..Default::default()
    };
    platform.open_window(options)
}

// ═══════════════════════════════════════════════════════════════
// SECTION 2: Window Handle Integration
// ═══════════════════════════════════════════════════════════════

/// Template: Test window handle compatibility with raw-window-handle
#[test]
fn test_window_handle_compatibility() {
    init_tracing();

    tracing::info!("Testing window handle compatibility with raw-window-handle");

    let platform = get_test_platform();

    // Skip if platform doesn't support windows
    if platform.name() == "Headless" {
        tracing::info!("⊘ SKIP: Headless platform doesn't support real windows");
        return;
    }

    match create_test_window() {
        Ok(window) => {
            // TODO: Replace this with actual window handle verification
            // Example:
            // use raw_window_handle::{HasRawWindowHandle, HasRawDisplayHandle};
            // let _window_handle = window.raw_window_handle();
            // let _display_handle = window.raw_display_handle();

            tracing::info!("✓ Window created successfully");

            let logical_size = window.logical_size();
            tracing::info!(
                "Window size: {}x{}",
                logical_size.width.0,
                logical_size.height.0
            );

            assert!(
                logical_size.width.0 > 0.0 && logical_size.height.0 > 0.0,
                "Window must have valid size"
            );
        }
        Err(e) => {
            panic!("Failed to create window: {}", e);
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// SECTION 3: Graphics Backend Integration
// ═══════════════════════════════════════════════════════════════

/// Template: Test platform window + wgpu surface creation
#[test]
#[ignore] // Remove #[ignore] when implementing
fn test_wgpu_surface_creation() {
    init_tracing();

    tracing::info!("Testing wgpu surface creation from platform window");

    // TODO: Add wgpu to dev-dependencies in Cargo.toml
    // TODO: Implement wgpu surface creation
    //
    // Example implementation:
    // ```rust
    // let window = create_test_window().unwrap();
    // let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
    // let surface = unsafe {
    //     instance.create_surface_from_raw_window_handle(
    //         window.raw_window_handle(),
    //         window.raw_display_handle()
    //     )
    // };
    // assert!(surface.is_ok());
    // ```

    tracing::warn!("TODO: Implement wgpu surface creation test");
}

/// Template: Test platform window + Canvas API integration
#[test]
#[ignore] // Remove #[ignore] when flui_painting is ready
fn test_canvas_integration() {
    init_tracing();

    tracing::info!("Testing Canvas API integration with platform window");

    // TODO: Implement when flui_painting Canvas API is available
    //
    // Example implementation:
    // ```rust
    // use flui_painting::Canvas;
    //
    // let window = create_test_window().unwrap();
    // let canvas = Canvas::from_window(&window).expect("Failed to create canvas");
    //
    // // Test basic canvas operations
    // canvas.clear(Color::WHITE);
    // canvas.draw_rect(Rect::new(px(10.0), px(10.0), px(100.0), px(100.0)), Color::RED);
    // canvas.present();
    // ```

    tracing::warn!("TODO: Implement Canvas integration test");
}

// ═══════════════════════════════════════════════════════════════
// SECTION 4: Event Flow Integration
// ═══════════════════════════════════════════════════════════════

/// Template: Test event propagation through platform → rendering pipeline
#[test]
#[ignore] // Remove #[ignore] when implementing
fn test_event_propagation() {
    init_tracing();

    tracing::info!("Testing event propagation through platform layers");

    // TODO: Implement event flow testing
    //
    // Example implementation:
    // ```rust
    // use std::sync::{Arc, Mutex};
    //
    // let platform = get_test_platform();
    // let events_received = Arc::new(Mutex::new(Vec::new()));
    // let events_clone = Arc::clone(&events_received);
    //
    // platform.on_window_event(Box::new(move |event| {
    //     events_clone.lock().unwrap().push(event.clone());
    // }));
    //
    // let window = create_test_window().unwrap();
    // window.request_redraw();
    //
    // // Verify redraw event was captured
    // let events = events_received.lock().unwrap();
    // assert!(events.iter().any(|e| matches!(e, WindowEvent::RedrawRequested)));
    // ```

    tracing::warn!("TODO: Implement event propagation test");
}

// ═══════════════════════════════════════════════════════════════
// SECTION 5: Text System Integration
// ═══════════════════════════════════════════════════════════════

/// Template: Test text system + rendering integration
#[test]
fn test_text_system_integration() {
    init_tracing();

    tracing::info!("Testing text system integration");

    let platform = get_test_platform();
    let text_system = platform.text_system();

    // Basic text measurement
    let text = "Integration Test";
    let font_family = text_system.default_font_family();
    let font_size = 16.0;

    tracing::info!(
        "Measuring text '{}' with font '{}' at {}pt",
        text,
        font_family,
        font_size
    );

    let bounds = text_system.measure_text(text, &font_family, font_size);

    tracing::info!("Text bounds: {}x{}", bounds.width().0, bounds.height().0);

    // Text bounds should be reasonable
    assert!(
        bounds.width().0 > 0.0 && bounds.width().0 < 1000.0,
        "Text width should be reasonable"
    );

    assert!(
        bounds.height().0 > 0.0 && bounds.height().0 < 100.0,
        "Text height should be reasonable"
    );

    // TODO: Add actual rendering integration when flui_painting is ready
    // Example:
    // ```rust
    // let canvas = Canvas::from_window(&window)?;
    // canvas.draw_text(text, Point::ORIGIN, &font_family, font_size, Color::BLACK);
    // canvas.present();
    // ```

    tracing::info!("✓ Text system integration validated");
}

// ═══════════════════════════════════════════════════════════════
// SECTION 6: Clipboard Integration
// ═══════════════════════════════════════════════════════════════

/// Template: Test clipboard + UI component integration
#[test]
fn test_clipboard_integration() {
    init_tracing();

    tracing::info!("Testing clipboard integration");

    let platform = get_test_platform();
    let clipboard = platform.clipboard();

    // Basic clipboard operations
    let test_text = "Integration test clipboard content";

    clipboard.write_text(test_text.to_string());

    if let Some(read_text) = clipboard.read_text() {
        assert_eq!(read_text, test_text, "Clipboard roundtrip failed");
        tracing::info!("✓ Clipboard roundtrip successful");
    } else {
        tracing::warn!("Clipboard read returned None (may be expected in CI)");
    }

    // TODO: Add UI component clipboard integration when widgets are ready
    // Example:
    // ```rust
    // let text_input = TextInput::new();
    // text_input.paste_from_clipboard(&clipboard);
    // assert_eq!(text_input.value(), test_text);
    // ```
}

// ═══════════════════════════════════════════════════════════════
// SECTION 7: Executor Integration
// ═══════════════════════════════════════════════════════════════

/// Template: Test executor + async rendering pipeline
#[test]
fn test_executor_integration() {
    init_tracing();

    tracing::info!("Testing executor integration");

    let platform = get_test_platform();
    let bg_executor = platform.background_executor();

    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    let task_executed = Arc::new(AtomicBool::new(false));
    let task_executed_clone = Arc::clone(&task_executed);

    // Spawn background task
    bg_executor.spawn(Box::new(move || {
        tracing::info!("Background task executing");
        task_executed_clone.store(true, Ordering::SeqCst);
    }));

    // Give task time to execute
    std::thread::sleep(std::time::Duration::from_millis(100));

    assert!(
        task_executed.load(Ordering::SeqCst),
        "Background task should have executed"
    );

    tracing::info!("✓ Executor integration validated");

    // TODO: Add foreground executor + UI update integration
    // Example:
    // ```rust
    // let fg_executor = platform.foreground_executor();
    // fg_executor.spawn(Box::new(move || {
    //     // Update UI state on main thread
    //     window.request_redraw();
    // }));
    // fg_executor.drain_tasks();
    // ```
}

// ═══════════════════════════════════════════════════════════════
// SECTION 8: Multi-Crate Integration Examples
// ═══════════════════════════════════════════════════════════════

/// Template: Full integration test with multiple crates
#[test]
#[ignore] // Remove when implementing full integration
fn test_full_integration() {
    init_tracing();

    tracing::info!("Running full integration test across multiple crates");

    // TODO: Implement comprehensive integration test
    //
    // Example workflow:
    // 1. Create platform and window (flui-platform)
    // 2. Initialize graphics surface (flui_painting + wgpu)
    // 3. Create UI components (flui_widgets)
    // 4. Render frame (flui_rendering + flui_engine)
    // 5. Process events (flui_interaction)
    // 6. Verify everything works together

    tracing::warn!("TODO: Implement full integration test");
}

// ═══════════════════════════════════════════════════════════════
// SECTION 9: Helper Utilities
// ═══════════════════════════════════════════════════════════════

/// Helper: Run a test with platform cleanup
#[allow(dead_code)]
fn with_platform<F>(test_fn: F)
where
    F: FnOnce(std::sync::Arc<dyn flui_platform::Platform>),
{
    init_tracing();
    let platform = get_test_platform();
    test_fn(platform);
    // Platform cleanup happens automatically on drop
}

/// Helper: Run a test with window cleanup
#[allow(dead_code)]
fn with_window<F>(test_fn: F) -> Result<(), anyhow::Error>
where
    F: FnOnce(Box<dyn flui_platform::PlatformWindow>) -> Result<(), anyhow::Error>,
{
    init_tracing();
    let window = create_test_window()?;
    test_fn(window)?;
    // Window cleanup happens automatically on drop
    Ok(())
}

// ═══════════════════════════════════════════════════════════════
// SECTION 10: Documentation and Notes
// ═══════════════════════════════════════════════════════════════

/*
# Integration Testing Best Practices

1. **Isolation**: Each test should be independent and not rely on global state
2. **Cleanup**: Use RAII patterns to ensure resources are cleaned up
3. **Timing**: Be careful with timing-dependent assertions in async code
4. **Platform Differences**: Use platform detection to skip unsupported features
5. **Logging**: Add tracing for debugging integration issues

# Adding New Integration Tests

To add a new integration test:

1. Add the dependent crate to [dev-dependencies] in Cargo.toml:
   ```toml
   [dev-dependencies]
   flui_painting = { path = "../flui_painting" }
   ```

2. Import the necessary types:
   ```rust
   use flui_painting::{Canvas, Color};
   ```

3. Write the test using the templates above as a guide

4. Run the test:
   ```bash
   cargo test -p flui-platform --test integration_template test_name
   ```

# Common Integration Patterns

## Pattern 1: Window + Graphics Surface
```rust
let window = create_test_window()?;
let surface = create_wgpu_surface(&window)?;
// Render to surface
```

## Pattern 2: Platform Events → UI Updates
```rust
platform.on_window_event(Box::new(|event| {
    match event {
        WindowEvent::Resized(size) => {
            // Update UI layout
        }
        _ => {}
    }
}));
```

## Pattern 3: Background Task → Main Thread Update
```rust
bg_executor.spawn(Box::new(move || {
    let result = expensive_computation();
    fg_executor.spawn(Box::new(move || {
        update_ui(result);
    }));
}));
```

*/
