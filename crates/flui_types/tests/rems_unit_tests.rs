//! Unit tests for Rems (root em units) implementation
//!
//! Tests the Rems type for font-relative sizing, fulfilling User Story 4 requirements.

use flui_types::geometry::{Rems, rems, Pixels, px};
use flui_types::geometry::traits::Unit;

// ============================================================================
// T055: Rems::new() and arithmetic tests
// ============================================================================

#[test]
fn test_rems_construction() {
    let r = Rems::new(1.5);
    assert_eq!(r.get(), 1.5);

    let r2 = rems(2.0);
    assert_eq!(r2.get(), 2.0);
}

#[test]
fn test_rems_zero() {
    let zero = Rems::ZERO;
    assert_eq!(zero.get(), 0.0);
    assert!(zero.is_zero());

    let default = Rems::default();
    assert_eq!(default, zero);
}

#[test]
fn test_rems_unit_trait() {
    // Unit trait implementation
    assert_eq!(Rems::zero(), Rems::ZERO);
    assert_eq!(Rems::one(), rems(1.0));

    // Min/Max constants
    assert_eq!(Rems::MIN.get(), f32::MIN);
    assert_eq!(Rems::MAX.get(), f32::MAX);
}

#[test]
fn test_rems_arithmetic_add() {
    let a = rems(1.5);
    let b = rems(2.0);

    let sum = a + b;
    assert_eq!(sum.get(), 3.5);
}

#[test]
fn test_rems_arithmetic_sub() {
    let a = rems(3.0);
    let b = rems(1.5);

    let diff = a - b;
    assert_eq!(diff.get(), 1.5);
}

#[test]
fn test_rems_arithmetic_mul() {
    let r = rems(2.0);
    let factor = 3.0;

    let product = r * factor;
    assert_eq!(product.get(), 6.0);

    // Commutative
    let product2 = factor * r;
    assert_eq!(product2.get(), 6.0);
}

#[test]
fn test_rems_arithmetic_div() {
    let r = rems(6.0);
    let divisor = 2.0;

    let quotient = r / divisor;
    assert_eq!(quotient.get(), 3.0);

    // Division of Rems by Rems yields f32
    let r1 = rems(9.0);
    let r2 = rems(3.0);
    let ratio: f32 = r1 / r2;
    assert_eq!(ratio, 3.0);
}

#[test]
fn test_rems_arithmetic_neg() {
    let r = rems(5.0);
    let neg = -r;
    assert_eq!(neg.get(), -5.0);
}

#[test]
fn test_rems_add_assign() {
    let mut r = rems(1.0);
    r += rems(2.0);
    assert_eq!(r.get(), 3.0);
}

#[test]
fn test_rems_sub_assign() {
    let mut r = rems(5.0);
    r -= rems(2.0);
    assert_eq!(r.get(), 3.0);
}

#[test]
fn test_rems_mul_assign() {
    let mut r = rems(2.0);
    r *= 3.0;
    assert_eq!(r.get(), 6.0);
}

#[test]
fn test_rems_div_assign() {
    let mut r = rems(6.0);
    r /= 2.0;
    assert_eq!(r.get(), 3.0);
}

// ============================================================================
// T056: Rems::to_pixels(base_font_size) tests
// ============================================================================

#[test]
fn test_rems_to_pixels_standard() {
    let r = rems(1.0);
    let base_font = px(16.0);  // Standard 1rem = 16px

    let pixels = r.to_pixels(base_font);
    assert_eq!(pixels.get(), 16.0);
}

#[test]
fn test_rems_to_pixels_scaled() {
    let r = rems(1.5);
    let base_font = px(16.0);

    let pixels = r.to_pixels(base_font);
    assert_eq!(pixels.get(), 24.0);  // 1.5 * 16 = 24
}

#[test]
fn test_rems_to_pixels_custom_base() {
    let r = rems(2.0);
    let base_font = px(20.0);  // Larger base font

    let pixels = r.to_pixels(base_font);
    assert_eq!(pixels.get(), 40.0);  // 2.0 * 20 = 40
}

#[test]
fn test_rems_to_pixels_small_value() {
    let r = rems(0.5);
    let base_font = px(16.0);

    let pixels = r.to_pixels(base_font);
    assert_eq!(pixels.get(), 8.0);  // 0.5 * 16 = 8
}

#[test]
fn test_rems_to_pixels_zero() {
    let r = rems(0.0);
    let base_font = px(16.0);

    let pixels = r.to_pixels(base_font);
    assert_eq!(pixels.get(), 0.0);
}

#[test]
fn test_rems_to_pixels_accessibility() {
    // User increases base font for accessibility
    let padding = rems(1.0);

    let normal_base = px(16.0);
    let large_base = px(20.0);
    let xlarge_base = px(24.0);

    // Padding scales with user preference
    assert_eq!(padding.to_pixels(normal_base).get(), 16.0);
    assert_eq!(padding.to_pixels(large_base).get(), 20.0);
    assert_eq!(padding.to_pixels(xlarge_base).get(), 24.0);
}

// ============================================================================
// Additional utility tests
// ============================================================================

#[test]
fn test_rems_abs() {
    let positive = rems(5.0);
    let negative = rems(-5.0);

    assert_eq!(positive.abs().get(), 5.0);
    assert_eq!(negative.abs().get(), 5.0);
}

#[test]
fn test_rems_min_max() {
    let a = rems(3.0);
    let b = rems(7.0);

    assert_eq!(a.min(b).get(), 3.0);
    assert_eq!(a.max(b).get(), 7.0);
}

#[test]
fn test_rems_copy_semantics() {
    let a = rems(42.0);
    let b = a; // Copy, not move

    assert_eq!(a, b);
    assert_eq!(a.get(), 42.0); // a still accessible
}

#[test]
fn test_rems_equality() {
    let a = rems(1.5);
    let b = rems(1.5);
    let c = rems(2.0);

    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn test_rems_ordering() {
    let small = rems(1.0);
    let large = rems(2.0);

    assert!(small < large);
    assert!(large > small);
    assert!(small <= large);
    assert!(large >= small);
}

#[test]
fn test_rems_debug() {
    let r = rems(1.5);
    let debug_str = format!("{:?}", r);
    assert!(debug_str.contains("1.5"));
}

#[test]
fn test_rems_display() {
    let r = rems(2.5);
    let display_str = format!("{}", r);
    assert!(display_str.contains("2.5"));
}
