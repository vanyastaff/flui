//! Unit conversion example for flui_types
//!
//! This example demonstrates the layout-to-render pipeline with unit conversions:
//! - Pixels: Logical units used in layout (density-independent)
//! - DevicePixels: Physical pixels on the screen (GPU coordinates)
//! - ScaledPixels: Intermediate representation for scaling operations
//!
//! This showcases how FLUI handles different display densities (1x, 2x retina, 1.5x, etc.)

use flui_types::geometry::{device_px, px, Point, Rect, Size};

fn main() {
    println!("=== FLUI Unit Conversions: Layout to Render Pipeline ===\n");

    // 1. Standard Display (1x scale factor)
    println!("1. Standard Display (1x scale, 96 DPI):");
    demonstrate_conversion(1.0, "Standard");

    // 2. Retina Display (2x scale factor)
    println!("\n2. Retina Display (2x scale, 192 DPI):");
    demonstrate_conversion(2.0, "Retina");

    // 3. High DPI Display (1.5x scale factor)
    println!("\n3. High DPI Display (1.5x scale, 144 DPI):");
    demonstrate_conversion(1.5, "High DPI");

    // 4. Practical Example: Responsive UI Element
    println!("\n4. Practical Example - Responsive Button:");
    responsive_button_example();

    // 5. GPU Rendering Pipeline
    println!("\n5. GPU Rendering Pipeline:");
    gpu_pipeline_example();

    // 6. Pixel Perfect Alignment
    println!("\n6. Pixel Perfect Alignment:");
    pixel_perfect_example();

    println!("\n=== Example Complete ===");
}

fn demonstrate_conversion(scale_factor: f32, display_name: &str) {
    // Layout: Designer specifies 100x100 logical pixels
    let logical_size = Size::new(px(100.0), px(100.0));

    println!("   Layout (logical pixels): {:?}", logical_size);

    // Convert to device pixels for GPU rendering
    let device_width = logical_size.width.to_device_pixels(scale_factor);
    let device_height = logical_size.height.to_device_pixels(scale_factor);
    let device_size = Size::new(device_width, device_height);

    println!("   Render (device pixels): {:?}", device_size);
    println!(
        "   Physical pixels on {}: {}x{}",
        display_name,
        device_width.get(),
        device_height.get()
    );

    // Round trip conversion
    let back_to_logical_width = device_width.to_pixels(scale_factor);
    let back_to_logical_height = device_height.to_pixels(scale_factor);

    println!(
        "   Round-trip check: {:?} x {:?}",
        back_to_logical_width, back_to_logical_height
    );
}

fn responsive_button_example() {
    // Button defined in logical pixels (44px touch target per iOS HIG)
    let button_rect = Rect::from_xywh(px(20.0), px(20.0), px(44.0), px(44.0));

    println!("   Logical button: {:?}", button_rect);

    // Render on different displays
    for (scale, name) in [(1.0, "1x"), (1.5, "1.5x"), (2.0, "2x"), (3.0, "3x")] {
        let device_rect = Rect::from_xywh(
            button_rect.left().to_device_pixels(scale),
            button_rect.top().to_device_pixels(scale),
            button_rect.width().to_device_pixels(scale),
            button_rect.height().to_device_pixels(scale),
        );

        println!(
            "   {} display: {:?} ({}x{} physical pixels)",
            name,
            device_rect,
            device_rect.width().get(),
            device_rect.height().get()
        );
    }
}

fn gpu_pipeline_example() {
    println!("   Simulating GPU rendering pipeline:");

    // Step 1: Layout phase (logical pixels)
    let viewport = Rect::from_xywh(px(0.0), px(0.0), px(800.0), px(600.0));
    println!("   1. Layout: Viewport = {:?}", viewport);

    // Step 2: Convert to device pixels for GPU
    let scale = 2.0; // Retina display
    let gpu_viewport = Rect::from_xywh(
        viewport.left().to_device_pixels(scale),
        viewport.top().to_device_pixels(scale),
        viewport.width().to_device_pixels(scale),
        viewport.height().to_device_pixels(scale),
    );
    println!("   2. GPU: Framebuffer = {:?}", gpu_viewport);
    println!(
        "      Physical resolution: {}x{}",
        gpu_viewport.width().get(),
        gpu_viewport.height().get()
    );

    // Step 3: Scissor rect for clipping (must be in device pixels)
    let clip_rect_logical = Rect::from_xywh(px(100.0), px(100.0), px(200.0), px(150.0));
    let scissor_rect = Rect::from_xywh(
        clip_rect_logical.left().to_device_pixels(scale),
        clip_rect_logical.top().to_device_pixels(scale),
        clip_rect_logical.width().to_device_pixels(scale),
        clip_rect_logical.height().to_device_pixels(scale),
    );
    println!("   3. Scissor: Clip region = {:?}", scissor_rect);

    // Step 4: Texture atlas coordinates (device pixels)
    let sprite_pos = Point::new(device_px(256), device_px(128));
    let sprite_size = Size::new(device_px(64), device_px(64));
    println!(
        "   4. Texture: Atlas sprite at {:?}, size {:?}",
        sprite_pos, sprite_size
    );
}

fn pixel_perfect_example() {
    println!("   Ensuring pixel-perfect rendering:");

    // Anti-pattern: Fractional logical pixels on retina
    let misaligned = px(100.5); // Will cause blur on 2x displays
    let device = misaligned.to_device_pixels(2.0);
    println!(
        "   ❌ Misaligned: {} logical -> {} device (fractional!)",
        misaligned.get(),
        device.get()
    );

    // Best practice: Use integer logical pixels or round
    let aligned = px(100.0);
    let device_aligned = aligned.to_device_pixels(2.0);
    println!(
        "   ✅ Aligned: {} logical -> {} device (sharp!)",
        aligned.get(),
        device_aligned.get()
    );

    // Alternatively: Round to device pixel boundaries
    let rounded = px(100.5).round();
    let device_rounded = rounded.to_device_pixels(2.0);
    println!(
        "   ✅ Rounded: {} logical -> {} device (sharp!)",
        rounded.get(),
        device_rounded.get()
    );
}
