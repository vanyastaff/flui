//! Integration tests for Color ApproxEq implementation

use flui_types::geometry::ApproxEq;
use flui_types::Color;

#[test]
fn test_approx_eq_identical() {
    let c1 = Color::rgb(100, 150, 200);
    let c2 = Color::rgb(100, 150, 200);
    assert!(c1.approx_eq(&c2));
}

#[test]
fn test_approx_eq_one_unit_difference() {
    let c1 = Color::rgb(100, 150, 200);
    let c2 = Color::rgb(100, 151, 200);
    let c3 = Color::rgb(101, 150, 200);
    let c4 = Color::rgb(100, 150, 201);

    // 1 unit difference should be within default epsilon
    assert!(c1.approx_eq(&c2));
    assert!(c1.approx_eq(&c3));
    assert!(c1.approx_eq(&c4));
}

#[test]
fn test_approx_eq_alpha_channel() {
    let c1 = Color::rgba(100, 150, 200, 255);
    let c2 = Color::rgba(100, 150, 200, 254);

    // 1 unit alpha difference should be within epsilon
    assert!(c1.approx_eq(&c2));
}

#[test]
fn test_approx_eq_large_difference() {
    let c1 = Color::rgb(100, 150, 200);
    let c2 = Color::rgb(105, 150, 200);

    // 5 unit difference should exceed default epsilon
    assert!(!c1.approx_eq(&c2));
}

#[test]
fn test_approx_eq_eps_custom_epsilon() {
    let c1 = Color::rgb(100, 150, 200);
    let c2 = Color::rgb(110, 150, 200);

    // 10 units = 10/255 ≈ 0.039
    assert!(!c1.approx_eq(&c2));

    // But should pass with larger epsilon
    assert!(c1.approx_eq_eps(&c2, 0.05));
}

#[test]
fn test_approx_eq_lerp_precision() {
    let c1 = Color::rgb(0, 0, 0);
    let c2 = Color::rgb(100, 100, 100);

    // Lerp at 0.5 should give (50, 50, 50)
    let mid = Color::lerp(c1, c2, 0.5);
    let expected = Color::rgb(50, 50, 50);

    assert!(mid.approx_eq(&expected));
}

#[test]
fn test_approx_eq_blend_precision() {
    let foreground = Color::rgba(255, 0, 0, 128); // 50% transparent red
    let background = Color::rgb(0, 0, 255); // opaque blue

    let blended = foreground.blend_over(background);

    // Expected: roughly purple (127, 0, 127)
    let expected = Color::rgb(127, 0, 127);

    // Blending calculations may have rounding errors
    assert!(blended.approx_eq_eps(&expected, 0.01));
}

#[test]
fn test_approx_eq_epsilon_boundary() {
    let c1 = Color::rgb(100, 100, 100);

    // Test at exactly 1/255 difference
    let c2 = Color::from_rgba_f32_array([
        100.0 / 255.0 + 1.0 / 255.0,
        100.0 / 255.0,
        100.0 / 255.0,
        1.0,
    ]);

    // Should be within epsilon
    assert!(c1.approx_eq(&c2));
}

#[test]
fn test_default_epsilon_value() {
    // Verify default epsilon is 1/255
    assert!((Color::DEFAULT_EPSILON - 1.0 / 255.0).abs() < 1e-10);
}

#[test]
fn test_approx_eq_all_channels_differ() {
    let c1 = Color::rgba(100, 150, 200, 255);
    let c2 = Color::rgba(101, 151, 201, 254);

    // All channels differ by 1 unit - should still pass
    assert!(c1.approx_eq(&c2));
}

#[test]
fn test_approx_eq_zero_epsilon() {
    let c1 = Color::rgb(100, 100, 100);
    let c2 = Color::rgb(100, 100, 100);
    let c3 = Color::rgb(100, 100, 101);

    // Zero epsilon means exact equality only
    assert!(c1.approx_eq_eps(&c2, 0.0));
    assert!(!c1.approx_eq_eps(&c3, 0.0));
}

#[test]
fn test_approx_eq_grayscale() {
    let gray1 = Color::rgb(128, 128, 128);
    let gray2 = Color::rgb(128, 128, 129);

    assert!(gray1.approx_eq(&gray2));
}

#[test]
fn test_approx_eq_black_and_white() {
    let black = Color::rgb(0, 0, 0);
    let almost_black = Color::rgb(1, 0, 0);
    let white = Color::rgb(255, 255, 255);
    let almost_white = Color::rgb(254, 255, 255);

    assert!(black.approx_eq(&almost_black));
    assert!(white.approx_eq(&almost_white));
    assert!(!black.approx_eq(&white));
}

#[test]
fn test_approx_eq_rgba_f32_roundtrip() {
    // Test conversion to f32 and back
    let original = Color::rgba(100, 150, 200, 255);
    let (r, g, b, a) = original.to_rgba_f32();
    let roundtrip = Color::from_rgba_f32_array([r, g, b, a]);

    assert!(original.approx_eq(&roundtrip));
}
