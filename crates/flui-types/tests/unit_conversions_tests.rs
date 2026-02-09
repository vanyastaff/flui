//! Unit conversion tests for Phase 7 (User Story 5)
//!
//! Tests explicit conversion methods between unit types:
//! - Pixels ↔ DevicePixels
//! - Pixels ↔ ScaledPixels
//! - Pixels ↔ Rems
//! - Round-trip conversions

use flui_types::geometry::{
    device_px, px, rems, scaled_px, DevicePixels, Pixels, Rems, ScaledPixels,
};

// ============================================================================
// T063: Pixels::to_device_pixels(scale)
// ============================================================================

#[test]
fn test_pixels_to_device_pixels_1x() {
    let logical = px(100.0);
    let device = logical.to_device_pixels(1.0);
    assert_eq!(device, device_px(100));
}

#[test]
fn test_pixels_to_device_pixels_2x_retina() {
    let logical = px(100.0);
    let device = logical.to_device_pixels(2.0);
    assert_eq!(device, device_px(200));
}

#[test]
fn test_pixels_to_device_pixels_1_5x() {
    let logical = px(100.0);
    let device = logical.to_device_pixels(1.5);
    // Should round to nearest integer
    assert_eq!(device, device_px(150));
}

#[test]
fn test_pixels_to_device_pixels_fractional() {
    let logical = px(10.5);
    let device = logical.to_device_pixels(2.0);
    // 10.5 * 2.0 = 21.0, rounds to 21
    assert_eq!(device, device_px(21));
}

#[test]
fn test_pixels_to_device_pixels_rounding() {
    let logical = px(10.4);
    let device = logical.to_device_pixels(2.0);
    // 10.4 * 2.0 = 20.8, rounds to 21
    assert_eq!(device, device_px(21));
}

#[test]
fn test_pixels_to_device_pixels_zero() {
    let logical = px(0.0);
    let device = logical.to_device_pixels(2.0);
    assert_eq!(device, device_px(0));
}

#[test]
fn test_pixels_to_device_pixels_negative() {
    let logical = px(-50.0);
    let device = logical.to_device_pixels(2.0);
    assert_eq!(device, device_px(-100));
}

// ============================================================================
// T064: DevicePixels::to_pixels(scale) (to_logical_pixels)
// ============================================================================

#[test]
fn test_device_pixels_to_logical_1x() {
    let device = device_px(100);
    let logical = device.to_pixels(1.0);
    assert_eq!(logical, px(100.0));
}

#[test]
fn test_device_pixels_to_logical_2x_retina() {
    let device = device_px(200);
    let logical = device.to_pixels(2.0);
    assert_eq!(logical, px(100.0));
}

#[test]
fn test_device_pixels_to_logical_1_5x() {
    let device = device_px(150);
    let logical = device.to_pixels(1.5);
    assert_eq!(logical, px(100.0));
}

#[test]
fn test_device_pixels_to_logical_fractional_result() {
    let device = device_px(101);
    let logical = device.to_pixels(2.0);
    // 101 / 2.0 = 50.5
    assert_eq!(logical, px(50.5));
}

#[test]
fn test_device_pixels_to_logical_zero() {
    let device = device_px(0);
    let logical = device.to_pixels(2.0);
    assert_eq!(logical, px(0.0));
}

#[test]
fn test_device_pixels_to_logical_negative() {
    let device = device_px(-100);
    let logical = device.to_pixels(2.0);
    assert_eq!(logical, px(-50.0));
}

// ============================================================================
// T065: Pixels::to_rems(base_font_size) - NOT YET IMPLEMENTED
// ============================================================================

// Note: This method doesn't exist yet on Pixels type, only on Rems → Pixels
// We can implement it or test the reverse direction
// For now, testing the existing Rems::to_pixels() method

#[test]
fn test_rems_to_pixels_standard_16px_base() {
    let r = rems(1.0);
    let base_font = px(16.0);
    let pixels = r.to_pixels(base_font);
    assert_eq!(pixels, px(16.0));
}

#[test]
fn test_rems_to_pixels_2rem() {
    let r = rems(2.0);
    let base_font = px(16.0);
    let pixels = r.to_pixels(base_font);
    assert_eq!(pixels, px(32.0));
}

#[test]
fn test_rems_to_pixels_fractional() {
    let r = rems(1.5);
    let base_font = px(16.0);
    let pixels = r.to_pixels(base_font);
    assert_eq!(pixels, px(24.0));
}

#[test]
fn test_rems_to_pixels_accessibility_large() {
    let r = rems(1.0);
    let large_base = px(20.0); // User increased font size
    let pixels = r.to_pixels(large_base);
    assert_eq!(pixels, px(20.0));
}

#[test]
fn test_rems_to_pixels_small_base() {
    let r = rems(1.0);
    let small_base = px(12.0);
    let pixels = r.to_pixels(small_base);
    assert_eq!(pixels, px(12.0));
}

// Test converting pixels to rems (manual calculation)
#[test]
fn test_pixels_to_rems_manual() {
    let pixels = px(32.0);
    let base_font = px(16.0);
    // Manual conversion: 32 / 16 = 2.0 rems
    let expected_rems = rems(pixels.get() / base_font.get());
    assert_eq!(expected_rems, rems(2.0));
}

// ============================================================================
// T066: Round-trip conversions (property tests)
// ============================================================================

#[test]
fn test_round_trip_pixels_device_pixels_1x() {
    let original = px(100.0);
    let device = original.to_device_pixels(1.0);
    let back = device.to_pixels(1.0);
    assert_eq!(back, original);
}

#[test]
fn test_round_trip_pixels_device_pixels_2x() {
    let original = px(100.0);
    let device = original.to_device_pixels(2.0);
    let back = device.to_pixels(2.0);
    assert_eq!(back, original);
}

#[test]
fn test_round_trip_pixels_device_pixels_fractional() {
    // Round-trip may lose precision due to rounding
    let original = px(100.5);
    let scale = 2.0;
    let device = original.to_device_pixels(scale);
    let back = device.to_pixels(scale);

    // 100.5 * 2.0 = 201.0 (rounds to 201)
    // 201 / 2.0 = 100.5 (exact)
    assert_eq!(back, original);
}

#[test]
fn test_round_trip_pixels_device_pixels_precision_loss() {
    // Test case where rounding causes precision loss
    let original = px(10.4);
    let scale = 2.0;
    let device = original.to_device_pixels(scale);
    let back = device.to_pixels(scale);

    // 10.4 * 2.0 = 20.8 (rounds to 21)
    // 21 / 2.0 = 10.5 (lost precision, difference of 0.1)
    // This is expected behavior - rounding can cause up to 0.5 / scale_factor precision loss
    let max_precision_loss = px(0.5 / scale);
    assert!(
        (back - original).abs() <= max_precision_loss,
        "Round-trip precision loss exceeded tolerance: {} -> {} -> {} (diff: {}, max: {})",
        original,
        device,
        back,
        (back - original).abs(),
        max_precision_loss
    );
}

#[test]
fn test_round_trip_rems_pixels() {
    let original_rems = rems(2.0);
    let base_font = px(16.0);
    let pixels = original_rems.to_pixels(base_font);
    let back_rems = rems(pixels.get() / base_font.get());
    assert_eq!(back_rems, original_rems);
}

#[test]
fn test_round_trip_rems_pixels_fractional_base() {
    let original_rems = rems(1.5);
    let base_font = px(18.5);
    let pixels = original_rems.to_pixels(base_font);
    let back_rems = rems(pixels.get() / base_font.get());

    // Should be exact with f32 arithmetic
    let epsilon = 1e-5;
    assert!(
        (back_rems.get() - original_rems.get()).abs() < epsilon,
        "Round-trip failed: {} -> {} -> {}",
        original_rems,
        pixels,
        back_rems
    );
}

// ============================================================================
// Scaled pixels conversions
// ============================================================================

#[test]
fn test_pixels_scale_to_scaled_pixels() {
    let logical = px(100.0);
    let scaled = logical.scale(2.0);
    assert_eq!(scaled, scaled_px(200.0));
}

#[test]
fn test_scaled_pixels_to_device_pixels() {
    let scaled = scaled_px(200.0);
    let device = scaled.to_device_pixels();
    // Should round to nearest integer
    assert_eq!(device, device_px(200));
}

#[test]
fn test_scaled_pixels_to_pixels() {
    let scaled = scaled_px(200.0);
    let logical = scaled.to_pixels(2.0);
    assert_eq!(logical, px(100.0));
}

#[test]
fn test_round_trip_pixels_scaled_device() {
    let original = px(100.0);
    let scale = 2.0;

    // Pixels -> ScaledPixels -> DevicePixels
    let scaled = original.scale(scale);
    let device = scaled.to_device_pixels();

    // DevicePixels -> Pixels
    let back = device.to_pixels(scale);

    assert_eq!(back, original);
}

#[test]
fn test_pixels_from_scaled_pixels() {
    let scaled = scaled_px(200.0);
    let logical = Pixels::from_scaled_pixels(scaled, 2.0);
    assert_eq!(logical, px(100.0));
}

#[test]
fn test_pixels_from_device_pixels() {
    let device = device_px(200);
    let logical = Pixels::from_device_pixels(device, 2.0);
    assert_eq!(logical, px(100.0));
}

// ============================================================================
// Edge cases
// ============================================================================

#[test]
fn test_conversion_with_zero_scale() {
    let logical = px(100.0);
    // Division by zero should produce infinity
    let device = logical.to_device_pixels(0.0);
    // 100.0 * 0.0 = 0.0
    assert_eq!(device, device_px(0));
}

#[test]
fn test_conversion_very_large_scale() {
    let logical = px(1.0);
    let huge_scale = 1000.0;
    let device = logical.to_device_pixels(huge_scale);
    assert_eq!(device, device_px(1000));
}

#[test]
fn test_conversion_very_small_scale() {
    let logical = px(100.0);
    let tiny_scale = 0.01;
    let device = logical.to_device_pixels(tiny_scale);
    // 100.0 * 0.01 = 1.0
    assert_eq!(device, device_px(1));
}

#[test]
fn test_negative_scale_factor() {
    let logical = px(100.0);
    let device = logical.to_device_pixels(-2.0);
    // 100.0 * -2.0 = -200.0
    assert_eq!(device, device_px(-200));
}

// ============================================================================
// Conversion consistency across scale factors
// ============================================================================

#[test]
fn test_conversion_consistency_1x_vs_2x() {
    let logical = px(50.0);

    let device_1x = logical.to_device_pixels(1.0);
    let device_2x = logical.to_device_pixels(2.0);

    // At 2x scale, should be exactly double
    assert_eq!(device_2x.get(), device_1x.get() * 2);
}

#[test]
fn test_conversion_proportional_scaling() {
    let logical1 = px(100.0);
    let logical2 = px(200.0);
    let scale = 1.5;

    let device1 = logical1.to_device_pixels(scale);
    let device2 = logical2.to_device_pixels(scale);

    // logical2 is 2x logical1, so device2 should be 2x device1
    assert_eq!(device2.get(), device1.get() * 2);
}

// ============================================================================
// Real-world use cases
// ============================================================================

#[test]
fn test_retina_display_conversion() {
    // Common Retina display: 2x scale factor
    let button_width = px(100.0);
    let retina_scale = 2.0;

    let device_width = button_width.to_device_pixels(retina_scale);
    assert_eq!(device_width, device_px(200));

    // Round-trip
    let back = device_width.to_pixels(retina_scale);
    assert_eq!(back, button_width);
}

#[test]
fn test_android_mdpi_conversion() {
    // Android MDPI: 1x scale factor (160 DPI baseline)
    let view_height = px(48.0); // 48dp
    let mdpi_scale = 1.0;

    let device_height = view_height.to_device_pixels(mdpi_scale);
    assert_eq!(device_height, device_px(48));
}

#[test]
fn test_android_xxhdpi_conversion() {
    // Android XXHDPI: 3x scale factor
    let view_height = px(48.0); // 48dp
    let xxhdpi_scale = 3.0;

    let device_height = view_height.to_device_pixels(xxhdpi_scale);
    assert_eq!(device_height, device_px(144));
}

#[test]
fn test_windows_125_percent_scaling() {
    // Windows 125% scaling = 1.25x
    let window_width = px(800.0);
    let windows_scale = 1.25;

    let device_width = window_width.to_device_pixels(windows_scale);
    // 800.0 * 1.25 = 1000.0
    assert_eq!(device_width, device_px(1000));
}

#[test]
fn test_font_size_rem_conversion() {
    // Common web scenario: 1rem = 16px base
    let heading_size = rems(2.0); // h1 size
    let base_font = px(16.0);

    let pixels = heading_size.to_pixels(base_font);
    assert_eq!(pixels, px(32.0));
}

#[test]
fn test_accessible_font_size_rem_conversion() {
    // User increased browser font size to 20px
    let paragraph_size = rems(1.0);
    let user_base_font = px(20.0);

    let pixels = paragraph_size.to_pixels(user_base_font);
    assert_eq!(pixels, px(20.0));
}

#[test]
fn test_padding_rem_conversion_scales_with_font() {
    // Padding defined in rems scales with font size
    let padding = rems(0.5);

    let normal_font = px(16.0);
    let large_font = px(24.0);

    let normal_padding = padding.to_pixels(normal_font);
    let large_padding = padding.to_pixels(large_font);

    assert_eq!(normal_padding, px(8.0));
    assert_eq!(large_padding, px(12.0));

    // Padding grew proportionally with font size
    assert_eq!(large_padding / normal_padding, large_font / normal_font);
}
