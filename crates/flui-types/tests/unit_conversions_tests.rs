#![expect(deprecated)]
// This target intentionally exercises the deprecated raw-scalar device conversions (to_device_pixels(f32)/from_device_pixels).
//! Unit conversion tests for Phase 7 (User Story 5)
//!
//! Tests explicit conversion methods between unit types:
//! - Pixels ↔ DevicePixels
//! - Pixels ↔ Rems
//! - Round-trip conversions

use flui_types::geometry::{Pixels, device_px, px, rems};
use proptest::prelude::*;
use rstest::rstest;

// ============================================================================
// T063: Pixels::to_device_pixels(scale)
// ============================================================================

#[rstest]
#[case::identity(100.0, 1.0, 100)]
#[case::retina_2x(100.0, 2.0, 200)]
#[case::fractional_scale(100.0, 1.5, 150)]
#[case::fractional_logical(10.5, 2.0, 21)]
#[case::rounds_up(10.4, 2.0, 21)]
#[case::zero_logical(0.0, 2.0, 0)]
#[case::negative_logical(-50.0, 2.0, -100)]
#[case::zero_scale(100.0, 0.0, 0)]
#[case::huge_scale(1.0, 1000.0, 1000)]
#[case::tiny_scale(100.0, 0.01, 1)]
#[case::negative_scale(100.0, -2.0, -200)]
#[case::android_xxhdpi(48.0, 3.0, 144)]
#[case::windows_125_percent(800.0, 1.25, 1000)]
fn pixels_to_device_pixels(#[case] logical: f32, #[case] scale: f32, #[case] expected: i32) {
    assert_eq!(px(logical).to_device_pixels(scale), device_px(expected));
}

// ============================================================================
// T064: DevicePixels::to_pixels(scale) (to_logical_pixels)
// ============================================================================

#[rstest]
#[case::identity(100, 1.0, 100.0)]
#[case::retina_2x(200, 2.0, 100.0)]
#[case::fractional_scale(150, 1.5, 100.0)]
#[case::fractional_result(101, 2.0, 50.5)]
#[case::zero(0, 2.0, 0.0)]
#[case::negative(-100, 2.0, -50.0)]
fn device_pixels_to_logical(#[case] device: i32, #[case] scale: f32, #[case] expected: f32) {
    assert_eq!(device_px(device).to_pixels(scale), px(expected));
}

// ============================================================================
// T065: Rems::to_pixels(base_font_size)
// ============================================================================

#[rstest]
#[case::one_rem_16px_base(1.0, 16.0, 16.0)]
#[case::two_rem(2.0, 16.0, 32.0)]
#[case::fractional_rem(1.5, 16.0, 24.0)]
#[case::accessibility_large_base(1.0, 20.0, 20.0)]
#[case::small_base(1.0, 12.0, 12.0)]
fn rems_to_pixels(#[case] rem_value: f32, #[case] base_font: f32, #[case] expected: f32) {
    assert_eq!(rems(rem_value).to_pixels(px(base_font)), px(expected));
}

#[test]
fn test_pixels_to_rems_manual() {
    let pixels = px(32.0);
    let base_font = px(16.0);
    let expected_rems = rems(pixels.get() / base_font.get());
    assert_eq!(expected_rems, rems(2.0));
}

// ============================================================================
// T066: Round-trip conversions
// ============================================================================

proptest! {
    /// A `Pixels` value converted to `DevicePixels` and back may lose
    /// precision from `DevicePixels` rounding to the nearest integer, but
    /// never by more than half a device pixel's worth of logical pixels —
    /// plus an f32-rounding term that grows with magnitude (the intentional
    /// rounding tolerance alone is too tight at values large enough that a
    /// single f32 ULP exceeds it).
    #[test]
    fn prop_pixels_device_pixels_roundtrip_within_rounding_tolerance(
        original in -100_000.0f32..=100_000.0,
        scale in 0.01f32..=100.0,
    ) {
        let device = px(original).to_device_pixels(scale);
        let back = device.to_pixels(scale);
        let max_precision_loss = 0.5 / scale + original.abs() * 8.0 * f32::EPSILON + 1e-4;
        prop_assert!(
            (back.get() - original).abs() <= max_precision_loss,
            "{back} vs {original}, tolerance {max_precision_loss}"
        );
    }

    /// `Rems::to_pixels` followed by dividing back out by the same base
    /// font size returns the original value exactly (no integer rounding
    /// is involved, unlike the `DevicePixels` roundtrip above).
    #[test]
    fn prop_rems_pixels_roundtrip(
        original in 0.01f32..=100.0,
        base_font in 1.0f32..=1000.0,
    ) {
        let pixels = rems(original).to_pixels(px(base_font));
        let back = rems(pixels.get() / base_font);
        prop_assert!((back.get() - original).abs() < 1e-4);
    }
}

// ============================================================================
// Scale and device pixel conversions
// ============================================================================

#[test]
fn test_pixels_scale() {
    let logical = px(100.0);
    let scaled = logical.scale(2.0);
    assert_eq!(scaled, px(200.0));
}

#[test]
fn test_pixels_from_device_pixels() {
    let device = device_px(200);
    let logical = Pixels::from_device_pixels(device, 2.0);
    assert_eq!(logical, px(100.0));
}

// ============================================================================
// Conversion consistency across scale factors
// ============================================================================

// These pin exact-integer scenarios deliberately: to_device_pixels rounds to
// the nearest integer, so proportionality between arbitrary floats doesn't
// hold in general (e.g. px(0.3) and px(0.6) at scale 1.0 round to 0 and 1,
// not a clean 2x). These specific values were chosen to fall on rounding
// boundaries where the property does hold exactly.

#[test]
fn test_conversion_consistency_1x_vs_2x() {
    let logical = px(50.0);

    let device_1x = logical.to_device_pixels(1.0);
    let device_2x = logical.to_device_pixels(2.0);

    assert_eq!(device_2x.get(), device_1x.get() * 2);
}

#[test]
fn test_conversion_proportional_scaling() {
    let logical1 = px(100.0);
    let logical2 = px(200.0);
    let scale = 1.5;

    let device1 = logical1.to_device_pixels(scale);
    let device2 = logical2.to_device_pixels(scale);

    assert_eq!(device2.get(), device1.get() * 2);
}

// ============================================================================
// Real-world use cases
// ============================================================================

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
