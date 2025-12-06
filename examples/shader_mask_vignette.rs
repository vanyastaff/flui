//! Shader Mask Example: Vignette Effect
//!
//! Demonstrates RenderShaderMask with a radial gradient vignette.
//! This example shows how to create classic vignette effects.

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

    tracing::info!("=== Shader Mask: Vignette Effect Example ===");

    // Example 1: Classic vignette (bright center → dark edges)
    let classic_vignette = RenderShaderMask {
        shader: ShaderSpec::RadialGradient {
            center: (0.5, 0.5), // Center of viewport (normalized 0-1)
            radius: 0.7,        // 70% of viewport size
            colors: vec![
                Color32::WHITE,                                // Bright center (fully opaque)
                Color32::from_rgba_unmultiplied(0, 0, 0, 200), // Dark edges (alpha = 200/255)
            ],
        },
        blend_mode: BlendMode::Multiply, // Multiply darkens the content
    };

    tracing::info!("Created classic vignette:");
    tracing::info!("  Center: (0.5, 0.5) - viewport center");
    tracing::info!("  Radius: 0.7 (70% of viewport)");
    tracing::info!("  Colors: White (center) → Semi-transparent Black (edges)");
    tracing::info!("  Blend mode: {:?}", classic_vignette.blend_mode);

    // Example 2: Soft vignette (subtle effect)
    let soft_vignette = RenderShaderMask::radial_gradient(
        (0.5, 0.5),
        1.0, // Full viewport radius
        vec![
            Color32::WHITE,
            Color32::from_rgba_unmultiplied(0, 0, 0, 100), // Very subtle darkening
        ],
    )
    .with_blend_mode(BlendMode::Multiply);

    tracing::info!("\nCreated soft vignette:");
    tracing::info!("  Center: (0.5, 0.5)");
    tracing::info!("  Radius: 1.0 (full viewport)");
    tracing::info!("  Effect: Very subtle edge darkening (alpha = 100/255)");

    // Example 3: Spotlight effect (inverted vignette)
    let spotlight = RenderShaderMask::radial_gradient(
        (0.5, 0.5),
        0.5, // Smaller radius for focused spotlight
        vec![
            Color32::WHITE, // Bright spotlight
            Color32::BLACK, // Completely dark outside
        ],
    )
    .with_blend_mode(BlendMode::SrcOver);

    tracing::info!("\nCreated spotlight effect:");
    tracing::info!("  Center: (0.5, 0.5)");
    tracing::info!("  Radius: 0.5 (tight spotlight)");
    tracing::info!("  Colors: White → Black");
    tracing::info!(
        "  Blend mode: {:?} (standard compositing)",
        spotlight.blend_mode
    );

    // Example 4: Colored vignette (creative effect)
    let colored_vignette = RenderShaderMask::radial_gradient(
        (0.5, 0.5),
        0.8,
        vec![
            Color32::WHITE,
            Color32::from_rgb(150, 100, 200), // Purple tint on edges
        ],
    )
    .with_blend_mode(BlendMode::Multiply);

    tracing::info!("\nCreated colored vignette:");
    tracing::info!("  Center: (0.5, 0.5)");
    tracing::info!("  Radius: 0.8");
    tracing::info!("  Colors: White → Purple");
    tracing::info!("  Effect: Purple-tinted edges");

    tracing::info!("\n✅ Example complete! These vignettes can be used with RenderBox::paint()");
    tracing::info!("💡 Vignettes are perfect for:");
    tracing::info!("   • Focus attention on center content");
    tracing::info!("   • Add cinematic effects");
    tracing::info!("   • Create depth and atmosphere");
}
