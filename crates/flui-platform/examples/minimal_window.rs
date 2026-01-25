//! Minimal window example - basic platform test without complex event handling
//!
//! Usage:
//! ```bash
//! cargo run --example minimal_window -p flui-platform --features=winit-backend
//! ```

use std::sync::Arc;

fn main() {
    // Note: Logging requires tracing-subscriber dependency

    println!("===========================================");
    println!("  FLUI Platform Minimal Window Test");
    println!("===========================================\n");

    // Try to get platform
    let platform = flui_platform::current_platform();

    println!("Platform initialized: {}", platform.name());
    println!("Platform capabilities available: {:?}\n",
             std::any::type_name_of_val(&platform.capabilities()));

    // Get displays
    let displays = platform.displays();
    println!("Found {} display(s):\n", displays.len());

    for (idx, display) in displays.iter().enumerate() {
        let logical = display.logical_size();
        let bounds = display.bounds();
        println!("  Display {}:", idx + 1);
        println!("    Name: {}", display.name());
        println!("    Physical: {}x{} @ ({}, {})",
                 bounds.size.width, bounds.size.height,
                 bounds.origin.x, bounds.origin.y);
        println!("    Logical: {}x{}", logical.width, logical.height);
        println!("    Scale: {:.1}x", display.scale_factor());
        println!("    Primary: {}", if display.is_primary() { "Yes" } else { "No" });
        println!();
    }

    if let Some(primary) = platform.primary_display() {
        let logical = primary.logical_size();
        println!("Primary Display: {} ({}x{})\n",
                 primary.name(), logical.width, logical.height);
    }

    println!("✓ Platform test completed successfully!");
}
