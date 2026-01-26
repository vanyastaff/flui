//! Color operations tests for Phase 9 (User Story 7)
//!
//! Tests for Color construction, blending, and manipulation:
//! - RGB and hex construction
//! - Color mixing (lerp)
//! - Alpha blending (blend_over)
//! - HSL-based lightening/darkening
//! - Opacity manipulation

use flui_types::styling::Color;

// ============================================================================
// T085: Color::from_rgb() and Color::from_rgba()
// ============================================================================

#[test]
fn test_color_from_rgb() {
    let red = Color::rgb(255, 0, 0);
    assert_eq!(red.r, 255);
    assert_eq!(red.g, 0);
    assert_eq!(red.b, 0);
    assert_eq!(red.a, 255); // Should be fully opaque
}

#[test]
fn test_color_from_rgba() {
    let semi_transparent_blue = Color::rgba(0, 0, 255, 128);
    assert_eq!(semi_transparent_blue.r, 0);
    assert_eq!(semi_transparent_blue.g, 0);
    assert_eq!(semi_transparent_blue.b, 255);
    assert_eq!(semi_transparent_blue.a, 128);
}

#[test]
fn test_color_from_rgb_white() {
    let white = Color::rgb(255, 255, 255);
    assert_eq!(white.r, 255);
    assert_eq!(white.g, 255);
    assert_eq!(white.b, 255);
    assert_eq!(white.a, 255);
}

#[test]
fn test_color_from_rgb_black() {
    let black = Color::rgb(0, 0, 0);
    assert_eq!(black.r, 0);
    assert_eq!(black.g, 0);
    assert_eq!(black.b, 0);
    assert_eq!(black.a, 255);
}

#[test]
fn test_color_rgba_zero_alpha() {
    let transparent = Color::rgba(255, 0, 0, 0);
    assert_eq!(transparent.a, 0);
}

// ============================================================================
// T086: Color::from_hex() valid formats
// ============================================================================

#[test]
fn test_color_from_hex_rrggbb() {
    let red = Color::from_hex("#FF0000").unwrap();
    assert_eq!(red, Color::rgb(255, 0, 0));
}

#[test]
fn test_color_from_hex_without_hash() {
    let blue = Color::from_hex("0000FF").unwrap();
    assert_eq!(blue, Color::rgb(0, 0, 255));
}

#[test]
fn test_color_from_hex_aarrggbb() {
    let semi_transparent_red = Color::from_hex("#80FF0000").unwrap();
    assert_eq!(semi_transparent_red, Color::rgba(255, 0, 0, 128));
}

#[test]
fn test_color_from_hex_lowercase() {
    let green = Color::from_hex("#00ff00").unwrap();
    assert_eq!(green, Color::rgb(0, 255, 0));
}

#[test]
fn test_color_from_hex_mixed_case() {
    let color = Color::from_hex("#AbCdEf").unwrap();
    assert_eq!(color, Color::rgb(0xAB, 0xCD, 0xEF));
}

#[test]
fn test_color_from_hex_white() {
    let white = Color::from_hex("#FFFFFF").unwrap();
    assert_eq!(white, Color::WHITE);
}

#[test]
fn test_color_from_hex_black() {
    let black = Color::from_hex("#000000").unwrap();
    assert_eq!(black, Color::BLACK);
}

// ============================================================================
// T087: Color::from_hex() invalid formats
// ============================================================================

#[test]
fn test_color_from_hex_invalid_length() {
    let result = Color::from_hex("#FFF");
    assert!(result.is_err(), "Should fail on 3-character hex");
}

#[test]
fn test_color_from_hex_invalid_characters() {
    let result = Color::from_hex("#GGGGGG");
    assert!(result.is_err(), "Should fail on invalid hex characters");
}

#[test]
fn test_color_from_hex_empty_string() {
    let result = Color::from_hex("");
    assert!(result.is_err(), "Should fail on empty string");
}

#[test]
fn test_color_from_hex_too_long() {
    let result = Color::from_hex("#FFFFFFFFFF");
    assert!(result.is_err(), "Should fail on too many characters");
}

// ============================================================================
// T088: Color::lerp() (mix) boundaries
// ============================================================================

#[test]
fn test_color_lerp_at_zero() {
    let red = Color::rgb(255, 0, 0);
    let blue = Color::rgb(0, 0, 255);

    let result = Color::lerp(red, blue, 0.0);
    assert_eq!(result, red, "lerp at t=0.0 should return first color");
}

#[test]
fn test_color_lerp_at_one() {
    let red = Color::rgb(255, 0, 0);
    let blue = Color::rgb(0, 0, 255);

    let result = Color::lerp(red, blue, 1.0);
    assert_eq!(result, blue, "lerp at t=1.0 should return second color");
}

#[test]
fn test_color_lerp_at_half() {
    let black = Color::rgb(0, 0, 0);
    let white = Color::rgb(255, 255, 255);

    let result = Color::lerp(black, white, 0.5);
    // Should be approximately middle gray (127 or 128)
    assert!((result.r as i32 - 127).abs() <= 1);
    assert!((result.g as i32 - 127).abs() <= 1);
    assert!((result.b as i32 - 127).abs() <= 1);
}

#[test]
fn test_color_lerp_red_to_green() {
    let red = Color::rgb(255, 0, 0);
    let green = Color::rgb(0, 255, 0);

    let result = Color::lerp(red, green, 0.25);
    // 25% towards green: R should decrease, G should increase
    assert!(result.r > 128, "Red should still be dominant");
    assert!(result.g < 128, "Green should be increasing");
    assert_eq!(result.b, 0);
}

#[test]
fn test_color_lerp_with_alpha() {
    let opaque = Color::rgba(255, 0, 0, 255);
    let transparent = Color::rgba(255, 0, 0, 0);

    let result = Color::lerp(opaque, transparent, 0.5);
    assert_eq!(result.r, 255);
    assert_eq!(result.g, 0);
    assert_eq!(result.b, 0);
    assert!(
        (result.a as i32 - 127).abs() <= 1,
        "Alpha should be interpolated"
    );
}

#[test]
fn test_color_lerp_identical_colors() {
    let color = Color::rgb(100, 150, 200);
    let result = Color::lerp(color, color, 0.5);
    assert_eq!(result, color);
}

// ============================================================================
// T089: Color::blend_over() alpha compositing
// ============================================================================

#[test]
fn test_blend_over_opaque_over_opaque() {
    let foreground = Color::rgb(255, 0, 0); // Red
    let background = Color::rgb(0, 0, 255); // Blue

    let result = foreground.blend_over(background);
    // Opaque foreground should completely cover background
    assert_eq!(result, foreground);
}

#[test]
fn test_blend_over_transparent_over_opaque() {
    let foreground = Color::rgba(255, 0, 0, 0); // Fully transparent red
    let background = Color::rgb(0, 0, 255); // Blue

    let result = foreground.blend_over(background);
    // Fully transparent foreground should show background
    assert_eq!(result, background);
}

#[test]
fn test_blend_over_semi_transparent() {
    let foreground = Color::rgba(255, 0, 0, 128); // 50% transparent red
    let background = Color::rgb(0, 0, 255); // Blue

    let result = foreground.blend_over(background);

    // Result should be a mix of red and blue
    assert!(result.r > 0, "Should have red component");
    assert!(result.b > 0, "Should have blue component");
    assert!(result.r > result.b, "Red should be more prominent");
}

#[test]
fn test_blend_over_white_over_black() {
    let foreground = Color::rgba(255, 255, 255, 128); // 50% transparent white
    let background = Color::rgb(0, 0, 0); // Black

    let result = foreground.blend_over(background);

    // Should be gray
    assert!(result.r > 100 && result.r < 155);
    assert_eq!(result.r, result.g);
    assert_eq!(result.g, result.b);
}

#[test]
fn test_blend_over_preserves_opacity() {
    let foreground = Color::rgba(255, 0, 0, 200);
    let background = Color::rgba(0, 0, 255, 200);

    let result = foreground.blend_over(background);

    // Result alpha should be >= max of input alphas
    assert!(result.a >= 200);
}

// ============================================================================
// T091: Color::lighten() and Color::darken()
// ============================================================================

#[test]
fn test_lighten_basic() {
    let color = Color::rgb(100, 100, 100);
    let lighter = color.lighten(0.2);

    // Should be brighter
    assert!(lighter.r > color.r);
    assert!(lighter.g > color.g);
    assert!(lighter.b > color.b);
}

#[test]
fn test_lighten_red() {
    let red = Color::rgb(200, 0, 0);
    let lighter = red.lighten(0.1);

    assert!(lighter.r >= red.r);
    // Lightening should increase other channels too (via HSL)
    assert!(lighter.g >= red.g);
    assert!(lighter.b >= red.b);
}

#[test]
fn test_lighten_already_white() {
    let white = Color::WHITE;
    let lighter = white.lighten(0.5);

    // Can't lighten white further
    assert_eq!(lighter, white);
}

#[test]
fn test_darken_basic() {
    let color = Color::rgb(150, 150, 150);
    let darker = color.darken(0.2);

    // Should be darker
    assert!(darker.r < color.r);
    assert!(darker.g < color.g);
    assert!(darker.b < color.b);
}

#[test]
fn test_darken_blue() {
    let blue = Color::rgb(0, 0, 200);
    let darker = blue.darken(0.1);

    assert!(darker.b <= blue.b);
    // Darkening should decrease all channels
    assert!(darker.r <= blue.r);
    assert!(darker.g <= blue.g);
}

#[test]
fn test_darken_already_black() {
    let black = Color::BLACK;
    let darker = black.darken(0.5);

    // Can't darken black further
    assert_eq!(darker, black);
}

#[test]
fn test_lighten_darken_effect() {
    let original = Color::rgb(128, 128, 128);
    let lighter = original.lighten(0.2);
    let darker = original.darken(0.2);

    // Verify lighten makes it brighter
    assert!(
        lighter.r > original.r || lighter.g > original.g || lighter.b > original.b,
        "Lighten should increase at least one channel"
    );

    // Verify darken makes it darker
    assert!(
        darker.r < original.r && darker.g < original.g && darker.b < original.b,
        "Darken should decrease all channels"
    );

    // Lighter should be brighter than darker
    assert!(lighter.r > darker.r);
    assert!(lighter.g > darker.g);
    assert!(lighter.b > darker.b);
}

// ============================================================================
// T103: Color::with_opacity()
// ============================================================================

#[test]
fn test_with_opacity_full() {
    let color = Color::rgb(255, 0, 0);
    let opaque = color.with_opacity(1.0);

    assert_eq!(opaque.a, 255);
    assert_eq!(opaque.r, color.r);
    assert_eq!(opaque.g, color.g);
    assert_eq!(opaque.b, color.b);
}

#[test]
fn test_with_opacity_half() {
    let color = Color::rgb(255, 0, 0);
    let semi = color.with_opacity(0.5);

    assert!((semi.a as i32 - 127).abs() <= 1);
    assert_eq!(semi.r, color.r);
    assert_eq!(semi.g, color.g);
    assert_eq!(semi.b, color.b);
}

#[test]
fn test_with_opacity_zero() {
    let color = Color::rgb(255, 0, 0);
    let transparent = color.with_opacity(0.0);

    assert_eq!(transparent.a, 0);
}

#[test]
fn test_with_opacity_changes_only_alpha() {
    let original = Color::rgba(100, 150, 200, 255);
    let modified = original.with_opacity(0.3);

    assert_eq!(modified.r, original.r);
    assert_eq!(modified.g, original.g);
    assert_eq!(modified.b, original.b);
    assert_ne!(modified.a, original.a);
}

// ============================================================================
// T104: Named color constants
// ============================================================================

#[test]
fn test_named_color_red() {
    assert_eq!(Color::RED, Color::rgb(255, 0, 0));
}

#[test]
fn test_named_color_blue() {
    assert_eq!(Color::BLUE, Color::rgb(0, 0, 255));
}

#[test]
fn test_named_color_white() {
    assert_eq!(Color::WHITE, Color::rgb(255, 255, 255));
}

#[test]
fn test_named_color_black() {
    assert_eq!(Color::BLACK, Color::rgb(0, 0, 0));
}

#[test]
fn test_named_color_transparent() {
    assert_eq!(Color::TRANSPARENT, Color::rgba(0, 0, 0, 0));
}

#[test]
fn test_named_colors_are_opaque() {
    assert_eq!(Color::RED.a, 255);
    assert_eq!(Color::BLUE.a, 255);
    assert_eq!(Color::WHITE.a, 255);
    assert_eq!(Color::BLACK.a, 255);
}

#[test]
fn test_transparent_is_transparent() {
    assert_eq!(Color::TRANSPARENT.a, 0);
}

// ============================================================================
// Real-world use cases
// ============================================================================

#[test]
fn test_button_hover_effect() {
    let button_color = Color::rgb(70, 130, 180); // Steel blue
    let hover_color = button_color.lighten(0.1);

    // Hover should be lighter
    assert!(hover_color.r >= button_color.r);
    assert!(hover_color.g >= button_color.g);
    assert!(hover_color.b >= button_color.b);
}

#[test]
fn test_shadow_color_creation() {
    let base_color = Color::BLACK;
    let shadow = base_color.with_opacity(0.2);

    assert_eq!(shadow.r, 0);
    assert_eq!(shadow.g, 0);
    assert_eq!(shadow.b, 0);
    assert!(shadow.a < 100);
}

#[test]
fn test_overlay_blending() {
    let content = Color::rgb(255, 255, 255);
    let overlay = Color::rgba(0, 0, 0, 128);

    let result = overlay.blend_over(content);

    // Should be darkened white (gray)
    assert!(result.r < 255);
    assert!(result.g < 255);
    assert!(result.b < 255);
    assert!(result.r > 0);
}

#[test]
fn test_theme_color_generation() {
    let primary = Color::from_hex("#2196F3").unwrap(); // Material blue
    let darker = primary.darken(0.2);
    let lighter = primary.lighten(0.2);

    // Should create a color palette
    assert!(darker.r < primary.r);
    assert!(darker.g < primary.g);
    assert!(darker.b < primary.b);

    assert!(lighter.r >= primary.r);
    assert!(lighter.g >= primary.g);
    assert!(lighter.b >= primary.b);
}

#[test]
fn test_gradient_interpolation() {
    let start = Color::rgb(255, 0, 0); // Red
    let end = Color::rgb(0, 0, 255); // Blue

    let step1 = Color::lerp(start, end, 0.25);
    let step2 = Color::lerp(start, end, 0.5);
    let step3 = Color::lerp(start, end, 0.75);

    // Should smoothly transition from red to blue
    assert!(step1.r > step2.r);
    assert!(step2.r > step3.r);

    assert!(step1.b < step2.b);
    assert!(step2.b < step3.b);
}

#[test]
fn test_alpha_blending_stack() {
    let background = Color::WHITE;
    let layer1 = Color::rgba(255, 0, 0, 100); // Transparent red
    let layer2 = Color::rgba(0, 0, 255, 100); // Transparent blue

    // Stack layers
    let result1 = layer1.blend_over(background);
    let result2 = layer2.blend_over(result1);

    // Should have contributions from all layers
    assert!(result2.r > 0); // From red layer
    assert!(result2.b > 0); // From blue layer
}

#[test]
fn test_color_to_hex_roundtrip() {
    let original = Color::rgb(0xAB, 0xCD, 0xEF);
    let hex = original.to_hex();
    let parsed = Color::from_hex(&hex).unwrap();

    assert_eq!(parsed, original);
}
