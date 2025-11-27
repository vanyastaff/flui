//! Shader Mask Example: Gradient Fade Effect
//!
//! Demonstrates RenderShaderMask with a linear gradient fade-out.
//! This example shows how to create shader mask effects programmatically.

use flui_rendering::prelude::RenderShaderMask;
use flui_types::{
    painting::{BlendMode, ShaderSpec},
    styling::Color32,
};

fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tracing::info!("=== Shader Mask: Linear Gradient Fade Example ===");

    // Example 1: Horizontal fade (left opaque → right transparent)
    let horizontal_fade = RenderShaderMask {
        shader: ShaderSpec::LinearGradient {
            start: (0.0, 0.5),  // Left center (normalized 0-1)
            end: (1.0, 0.5),    // Right center
            colors: vec![
                Color32::WHITE,        // Fully opaque (left)
                Color32::TRANSPARENT,  // Fully transparent (right)
            ],
        },
        blend_mode: BlendMode::SrcOver,
    };

    tracing::info!("Created horizontal fade gradient:");
    tracing::info!("  Start: (0.0, 0.5) → End: (1.0, 0.5)");
    tracing::info!("  Colors: White → Transparent");
    tracing::info!("  Blend mode: {:?}", horizontal_fade.blend_mode);

    // Example 2: Vertical fade (top opaque → bottom transparent)
    let vertical_fade = RenderShaderMask::linear_gradient(
        (0.5, 0.0),  // Top center
        (0.5, 1.0),  // Bottom center
        vec![
            Color32::from_rgba_unmultiplied(255, 255, 255, 255),  // Opaque white
            Color32::from_rgba_unmultiplied(255, 255, 255, 0),    // Transparent white
        ],
    );

    tracing::info!("\nCreated vertical fade gradient:");
    tracing::info!("  Start: (0.5, 0.0) → End: (0.5, 1.0)");
    tracing::info!("  Colors: White → Transparent");

    // Example 3: Diagonal fade with custom blend mode
    let diagonal_fade = RenderShaderMask::linear_gradient(
        (0.0, 0.0),  // Top-left
        (1.0, 1.0),  // Bottom-right
        vec![
            Color32::RED,
            Color32::BLUE,
        ],
    )
    .with_blend_mode(BlendMode::Multiply);

    tracing::info!("\nCreated diagonal fade gradient:");
    tracing::info!("  Start: (0.0, 0.0) → End: (1.0, 1.0)");
    tracing::info!("  Colors: Red → Blue");
    tracing::info!("  Blend mode: {:?}", diagonal_fade.blend_mode);

    tracing::info!("\n✅ Example complete! These shader masks can be used with RenderBox::paint()");
    tracing::info!("💡 The paint() method will apply these gradients as masks to child content");
}
