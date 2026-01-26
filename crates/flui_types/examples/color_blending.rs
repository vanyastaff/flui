//! Color blending example for flui_types
//!
//! This example demonstrates color manipulation and blending operations:
//! - Color creation (RGB, RGBA, hex)
//! - Color mixing (linear interpolation)
//! - Alpha blending (Porter-Duff over operator)
//! - HSL operations (lighten, darken)
//! - Practical UI scenarios

use flui_types::styling::Color;

fn main() {
    println!("=== FLUI Color Blending & Manipulation ===\n");

    // 1. Creating Colors
    println!("1. Creating Colors:");
    create_colors_example();

    // 2. Color Mixing (Linear Interpolation)
    println!("\n2. Color Mixing (Linear Interpolation):");
    color_mixing_example();

    // 3. Alpha Blending
    println!("\n3. Alpha Blending (Porter-Duff Over):");
    alpha_blending_example();

    // 4. HSL Operations
    println!("\n4. HSL Operations (Lighten/Darken):");
    hsl_operations_example();

    // 5. Practical UI Examples
    println!("\n5. Practical UI Examples:");
    practical_examples();

    // 6. Color Gradients
    println!("\n6. Color Gradients:");
    gradient_example();

    // 7. Accessibility
    println!("\n7. Accessibility Considerations:");
    accessibility_example();

    println!("\n=== Example Complete ===");
}

fn create_colors_example() {
    // RGB colors
    let red = Color::rgb(255, 0, 0);
    let green = Color::rgb(0, 255, 0);
    let blue = Color::rgb(0, 0, 255);

    println!("   RGB colors:");
    println!("   Red: {:?} -> {}", red, red.to_hex());
    println!("   Green: {:?} -> {}", green, green.to_hex());
    println!("   Blue: {:?} -> {}", blue, blue.to_hex());

    // RGBA colors (with transparency)
    let semi_red = Color::rgba(255, 0, 0, 128);
    println!("\n   RGBA (semi-transparent):");
    println!("   Semi-transparent red: {:?}", semi_red);

    // From hex strings
    println!("\n   From hex strings:");
    let material_green = Color::from_hex("#4CAF50").unwrap();
    let material_blue = Color::from_hex("#2196F3").unwrap();
    println!("   Material Green: {}", material_green.to_hex());
    println!("   Material Blue: {}", material_blue.to_hex());

    // Predefined colors
    println!("\n   Predefined colors:");
    println!("   White: {}", Color::WHITE.to_hex());
    println!("   Black: {}", Color::BLACK.to_hex());
    println!("   Transparent: {:?}", Color::TRANSPARENT);
}

fn color_mixing_example() {
    let red = Color::rgb(255, 0, 0);
    let blue = Color::rgb(0, 0, 255);

    println!("   Mixing red and blue:");
    for i in 0..=10 {
        let t = i as f32 / 10.0;
        let mixed = Color::lerp(red, blue, t);
        println!(
            "   t={:.1}: {} (R:{}, G:{}, B:{})",
            t,
            mixed.to_hex(),
            mixed.r(),
            mixed.g(),
            mixed.b()
        );
    }

    println!("\n   Creating a purple:");
    let purple = Color::lerp(red, blue, 0.5);
    println!("   Purple (50% mix): {}", purple.to_hex());
}

fn alpha_blending_example() {
    let background = Color::rgb(255, 255, 255); // White background
    let foreground = Color::rgba(255, 0, 0, 128); // 50% transparent red

    println!("   Blending semi-transparent red over white background:");
    println!("   Background: {}", background.to_hex());
    println!("   Foreground: {:?} (alpha={})", foreground, foreground.a());

    let blended = foreground.blend_over(background);
    println!(
        "   Result: {} (R:{}, G:{}, B:{})",
        blended.to_hex(),
        blended.r(),
        blended.g(),
        blended.b()
    );

    println!("\n   Multiple layers:");
    let layer1 = Color::rgba(255, 0, 0, 100); // Red, low opacity
    let layer2 = Color::rgba(0, 255, 0, 100); // Green, low opacity
    let layer3 = Color::rgba(0, 0, 255, 100); // Blue, low opacity

    let result = layer3.blend_over(layer2.blend_over(layer1.blend_over(Color::WHITE)));
    println!("   Red + Green + Blue over white: {}", result.to_hex());
}

fn hsl_operations_example() {
    let base_color = Color::from_hex("#2196F3").unwrap(); // Material Blue

    println!("   Base color: {}", base_color.to_hex());

    // Lighten
    println!("\n   Lightening:");
    for i in 1..=3 {
        let amount = i as f32 * 0.1;
        let lighter = base_color.lighten(amount);
        println!("   +{}%: {}", (amount * 100.0) as i32, lighter.to_hex());
    }

    // Darken
    println!("\n   Darkening:");
    for i in 1..=3 {
        let amount = i as f32 * 0.1;
        let darker = base_color.darken(amount);
        println!("   -{}%: {}", (amount * 100.0) as i32, darker.to_hex());
    }
}

fn practical_examples() {
    // Button states
    println!("   Button States:");
    let button_primary = Color::from_hex("#2196F3").unwrap();
    let button_hover = button_primary.lighten(0.1);
    let button_active = button_primary.darken(0.1);
    let button_disabled = Color::lerp(button_primary, Color::rgb(200, 200, 200), 0.5);

    println!("   Normal: {}", button_primary.to_hex());
    println!("   Hover: {}", button_hover.to_hex());
    println!("   Active: {}", button_active.to_hex());
    println!("   Disabled: {}", button_disabled.to_hex());

    // Overlay/Modal background
    println!("\n   Modal Overlay:");
    let overlay = Color::rgba(0, 0, 0, 128); // 50% black
    println!("   Overlay: {:?}", overlay);
    let page_color = Color::rgb(255, 255, 255);
    let dimmed_page = overlay.blend_over(page_color);
    println!("   Dimmed page: {}", dimmed_page.to_hex());

    // Shadow
    println!("\n   Drop Shadow:");
    let shadow = Color::rgba(0, 0, 0, 51); // ~20% black
    println!("   Shadow color: {:?}", shadow);
}

fn gradient_example() {
    println!("   Linear gradient from blue to green:");
    let start_color = Color::from_hex("#2196F3").unwrap();
    let end_color = Color::from_hex("#4CAF50").unwrap();

    println!("   Start: {}", start_color.to_hex());
    for i in 1..=4 {
        let t = i as f32 / 5.0;
        let gradient_color = Color::lerp(start_color, end_color, t);
        println!("   {}%: {}", (t * 100.0) as i32, gradient_color.to_hex());
    }
    println!("   End: {}", end_color.to_hex());
}

fn accessibility_example() {
    // WCAG contrast recommendations
    println!("   Text color recommendations:");

    let dark_background = Color::from_hex("#212121").unwrap();
    let light_text = Color::rgb(255, 255, 255);
    println!(
        "   Dark mode: {} text on {} background",
        light_text.to_hex(),
        dark_background.to_hex()
    );

    let light_background = Color::rgb(255, 255, 255);
    let dark_text = Color::from_hex("#212121").unwrap();
    println!(
        "   Light mode: {} text on {} background",
        dark_text.to_hex(),
        light_background.to_hex()
    );

    // Focus indicators
    println!("\n   Focus indicators:");
    let focus_color = Color::from_hex("#2196F3").unwrap();
    let focus_ring = focus_color.with_alpha(128); // 50% opacity
    println!("   Focus ring: {:?}", focus_ring);
}
