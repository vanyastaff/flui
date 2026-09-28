//! Transitional names for the scalar types that replaced FLUI's unit wrappers (ADR-0098).
//!
//! `Pixels`, `PixelDelta` and `DevicePixels` were newtypes; a logical length is now a plain
//! `f64` and a device coordinate an `i32`. These aliases exist only while the workspace
//! migrates, so imports that still name them keep resolving; they are removed with the last
//! reference.

/// Transitional alias: a logical length is an `f64`.
pub type Pixels = f64;

/// Transitional alias: a logical delta is an `f64`.
pub type PixelDelta = f64;

/// Transitional alias: a device-pixel coordinate is an `i32`.
pub type DevicePixels = i32;

/// Transitional identity for a logical length.
#[inline]
#[must_use]
pub const fn px(value: f64) -> f64 {
    value
}

/// Transitional identity for a logical delta.
#[inline]
#[must_use]
pub const fn delta_px(value: f64) -> f64 {
    value
}

/// Transitional identity for a device-pixel coordinate.
#[inline]
#[must_use]
pub const fn device_px(value: i32) -> i32 {
    value
}
