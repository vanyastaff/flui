//! Unit tests for Unit trait implementations
//!
//! Tests the core Unit trait and its implementations (Pixels, DevicePixels, etc.)
//! following the contracts defined in specs/001-flui-types/contracts/README.md

use flui_types::geometry::traits::Unit;
use flui_types::geometry::{DevicePixels, Pixels, Radians, ScaledPixels};

#[test]
fn test_pixels_zero() {
    let zero = Pixels::zero();
    assert_eq!(zero, Pixels(0.0));
    assert_eq!(zero, Pixels::default());
}

#[test]
fn test_pixels_one() {
    let one = Pixels::one();
    assert_eq!(one, Pixels(1.0));
}

#[test]
fn test_pixels_min_max() {
    assert_eq!(Pixels::MIN, Pixels(f32::MIN));
    assert_eq!(Pixels::MAX, Pixels(f32::MAX));
}

#[test]
fn test_device_pixels_zero() {
    let zero = DevicePixels::zero();
    assert_eq!(zero, DevicePixels(0));
    assert_eq!(zero, DevicePixels::default());
}

#[test]
fn test_device_pixels_one() {
    let one = DevicePixels::one();
    assert_eq!(one, DevicePixels(1));
}

#[test]
fn test_device_pixels_min_max() {
    assert_eq!(DevicePixels::MIN, DevicePixels(i32::MIN));
    assert_eq!(DevicePixels::MAX, DevicePixels(i32::MAX));
}

#[test]
fn test_scaled_pixels_zero() {
    let zero = ScaledPixels::zero();
    assert_eq!(zero, ScaledPixels(0.0));
    assert_eq!(zero, ScaledPixels::default());
}

#[test]
fn test_scaled_pixels_one() {
    let one = ScaledPixels::one();
    assert_eq!(one, ScaledPixels(1.0));
}

#[test]
fn test_scaled_pixels_min_max() {
    assert_eq!(ScaledPixels::MIN, ScaledPixels(f32::MIN));
    assert_eq!(ScaledPixels::MAX, ScaledPixels(f32::MAX));
}

#[test]
fn test_radians_zero() {
    let zero = Radians::zero();
    assert_eq!(zero, Radians(0.0));
    assert_eq!(zero, Radians::default());
}

#[test]
fn test_radians_one() {
    let one = Radians::one();
    assert_eq!(one, Radians(1.0));
}

#[test]
fn test_radians_min_max() {
    assert_eq!(Radians::MIN, Radians(f32::MIN));
    assert_eq!(Radians::MAX, Radians(f32::MAX));
}

// Test that Unit types are Copy
#[test]
fn test_pixels_copy() {
    let a = Pixels(42.0);
    let b = a; // Copy, not move
    assert_eq!(a, b);
    assert_eq!(a, Pixels(42.0)); // a still accessible
}

#[test]
fn test_device_pixels_copy() {
    let a = DevicePixels(42);
    let b = a; // Copy, not move
    assert_eq!(a, b);
    assert_eq!(a, DevicePixels(42)); // a still accessible
}

// Test that Unit types implement Debug
#[test]
fn test_pixels_debug() {
    let p = Pixels(100.5);
    let debug_str = format!("{:?}", p);
    assert!(debug_str.contains("100.5"));
}

// Test that Unit types implement PartialEq and Eq
#[test]
fn test_pixels_equality() {
    let a = Pixels(42.0);
    let b = Pixels(42.0);
    let c = Pixels(43.0);

    assert_eq!(a, b);
    assert_ne!(a, c);
}

// Test that Unit types implement PartialOrd and Ord
#[test]
fn test_pixels_ordering() {
    let small = Pixels(10.0);
    let large = Pixels(100.0);

    assert!(small < large);
    assert!(large > small);
    assert!(small <= large);
    assert!(large >= small);
}

// Test that Unit types implement Hash
#[test]
fn test_pixels_hash() {
    use std::collections::HashSet;

    let mut set = HashSet::new();
    set.insert(Pixels(42.0));
    set.insert(Pixels(42.0)); // Duplicate
    set.insert(Pixels(43.0));

    assert_eq!(set.len(), 2); // Only unique values
    assert!(set.contains(&Pixels(42.0)));
    assert!(set.contains(&Pixels(43.0)));
}
