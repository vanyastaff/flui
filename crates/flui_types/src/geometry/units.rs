//! Unit types for type-safe coordinate systems.
//!
//! This module provides distinct types for different pixel coordinate systems
//! to prevent mixing logical pixels, device pixels, and scaled pixels.
//!
//! # Unit Types
//!
//! - [`Pixels`] - Logical pixels used in layouts and measurements
//! - [`DevicePixels`] - Physical pixels on the display device
//! - [`ScaledPixels`] - Pixels scaled by display scaling factor
//!
//! # Design Philosophy
//!
//! Following GPUI's approach, these types provide:
//! - **Type safety**: Prevents mixing different coordinate systems
//! - **Zero cost**: Transparent newtype wrappers with no runtime overhead
//! - **Ergonomic**: Rich operator overloads and conversions
//! - **GPU-friendly**: f32/i32 for optimal GPU performance
//!
//! # Examples
//!
//! ```rust
//! use flui_types::geometry::{Pixels, DevicePixels, ScaledPixels, px, device_px};
//!
//! // Type-safe logical pixels
//! let width = px(100.0);
//! let height = px(200.0);
//!
//! // Scale for high-DPI display (2x Retina)
//! let scaled = width.scale(2.0);
//!
//! // Convert to device pixels
//! let device = scaled.to_device_pixels();
//! assert_eq!(device, device_px(200));
//! ```

use std::fmt::{self, Debug, Display};
use std::iter::Sum;
use std::ops::{
    Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Rem, RemAssign, Sub, SubAssign,
};

use super::traits::{NumericUnit, Unit};

// ============================================================================
// PIXELS - Logical pixels (layout and measurement)
// ============================================================================

/// Logical pixels used for layout, measurement, and most UI calculations.
///
/// `Pixels` represents the logical coordinate system that is independent of
/// physical display resolution. This is the primary unit used throughout FLUI
/// for specifying sizes, positions, and offsets.
///
/// # Display Scaling
///
/// On high-DPI displays (Retina, 4K, etc.), one logical pixel may correspond
/// to multiple physical pixels. Use [`scale()`](Pixels::scale) to convert to
/// [`ScaledPixels`] for rendering.
///
/// # Examples
///
/// ```rust
/// use flui_types::geometry::{Pixels, px};
///
/// let width = px(100.0);
/// let height = px(200.0);
/// let area = width.get() * height.get();  // 20000.0
///
/// // Math operations
/// let doubled = width * 2.0;
/// let half = width / 2.0;
///
/// // Rounding
/// let rounded = px(123.7).round();
/// assert_eq!(rounded, px(124.0));
/// ```
#[derive(Clone, Copy, Default, PartialEq)]
#[repr(transparent)]
pub struct Pixels(pub f32);

/// Convenience function to create [`Pixels`] from a float value.
///
/// # Examples
///
/// ```rust
/// use flui_types::geometry::px;
///
/// let width = px(100.0);
/// let height = px(200.5);
/// ```
#[inline]
pub const fn px(value: f32) -> Pixels {
    Pixels(value)
}

impl Pixels {
    /// Zero pixels.
    pub const ZERO: Pixels = Pixels(0.0);

    /// Maximum representable pixels value.
    pub const MAX: Pixels = Pixels(f32::MAX);

    /// Minimum representable pixels value.
    pub const MIN: Pixels = Pixels(f32::MIN);

    /// Creates a new `Pixels` value.
    #[inline]
    pub const fn new(value: f32) -> Self {
        Self(value)
    }

    /// Returns the inner f32 value.
    #[inline]
    pub const fn get(self) -> f32 {
        self.0
    }

    /// Floors the value to the nearest whole number.
    #[inline]
    #[must_use]
    pub fn floor(self) -> Self {
        Self(self.0.floor())
    }

    /// Rounds the value to the nearest whole number.
    #[inline]
    #[must_use]
    pub fn round(self) -> Self {
        Self(self.0.round())
    }

    /// Ceils the value to the nearest whole number.
    #[inline]
    #[must_use]
    pub fn ceil(self) -> Self {
        Self(self.0.ceil())
    }

    /// Truncates the fractional part.
    #[inline]
    #[must_use]
    pub fn trunc(self) -> Self {
        Self(self.0.trunc())
    }

    /// Returns the absolute value.
    #[inline]
    #[must_use]
    pub fn abs(self) -> Self {
        Self(self.0.abs())
    }

    /// Returns the sign as a raw f32 value (-1.0, 0.0, or 1.0).
    ///
    /// **Note:** For a `Pixels`-wrapped result, use the [`Sign`] trait method
    /// via `Sign::signum(value)` or import the trait.
    ///
    /// [`Sign`]: super::traits::Sign
    #[inline]
    pub fn signum_raw(self) -> f32 {
        self.0.signum()
    }

    /// Raises the value to the given power.
    #[inline]
    #[must_use]
    pub fn pow(self, exponent: f32) -> Self {
        Self(self.0.powf(exponent))
    }

    /// Scales the value by the given factor, producing [`ScaledPixels`].
    ///
    /// This is used for high-DPI displays where the scale factor represents
    /// the ratio of physical to logical pixels (e.g., 2.0 for Retina displays).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::px;
    ///
    /// let logical = px(100.0);
    /// let scaled = logical.scale(2.0);  // 200.0 scaled pixels
    /// ```
    #[inline]
    #[must_use]
    pub fn scale(self, factor: f32) -> ScaledPixels {
        ScaledPixels(self.0 * factor)
    }

    /// Checks if the value is finite (not infinite or NaN).
    #[inline]
    pub fn is_finite(self) -> bool {
        self.0.is_finite()
    }

    /// Checks if the value is NaN.
    #[inline]
    pub fn is_nan(self) -> bool {
        self.0.is_nan()
    }

    /// Checks if the value is infinite.
    #[inline]
    pub fn is_infinite(self) -> bool {
        self.0.is_infinite()
    }

    /// Returns the minimum of two values.
    #[inline]
    #[must_use]
    pub fn min(self, other: Self) -> Self {
        Self(self.0.min(other.0))
    }

    /// Returns the maximum of two values.
    #[inline]
    #[must_use]
    pub fn max(self, other: Self) -> Self {
        Self(self.0.max(other.0))
    }

    /// Maximum of two values (const version).
    ///
    /// Useful for const contexts where `f32::max` cannot be used.
    #[inline]
    #[must_use]
    pub const fn max_const(self, other: Self) -> Self {
        if self.0 > other.0 {
            self
        } else {
            other
        }
    }

    /// Minimum of two values (const version).
    #[inline]
    #[must_use]
    pub const fn min_const(self, other: Self) -> Self {
        if self.0 < other.0 {
            self
        } else {
            other
        }
    }

    /// Clamps the value between min and max.
    #[inline]
    #[must_use]
    pub fn clamp(self, min: Self, max: Self) -> Self {
        Self(self.0.clamp(min.0, max.0))
    }

    /// Creates a new `Pixels` value, clamping to valid range.
    ///
    /// Invalid values (NaN, infinity) are replaced with zero.
    /// Negative values are clamped to zero.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Pixels;
    ///
    /// let valid = Pixels::new_clamped(100.0);
    /// assert_eq!(valid.get(), 100.0);
    ///
    /// let negative = Pixels::new_clamped(-50.0);
    /// assert_eq!(negative.get(), 0.0);
    ///
    /// let nan = Pixels::new_clamped(f32::NAN);
    /// assert_eq!(nan.get(), 0.0);
    /// ```
    #[inline]
    pub fn new_clamped(value: f32) -> Self {
        if value.is_finite() {
            Self(value.max(0.0))
        } else {
            Self::ZERO
        }
    }

    /// Tries to create a new `Pixels` value, returning `None` for invalid values.
    ///
    /// Returns `None` if the value is NaN, infinite, or negative.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Pixels;
    ///
    /// assert!(Pixels::try_new(100.0).is_some());
    /// assert!(Pixels::try_new(0.0).is_some());
    /// assert!(Pixels::try_new(-1.0).is_none());
    /// assert!(Pixels::try_new(f32::NAN).is_none());
    /// assert!(Pixels::try_new(f32::INFINITY).is_none());
    /// ```
    #[inline]
    pub fn try_new(value: f32) -> Option<Self> {
        if value.is_finite() && value >= 0.0 {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Pixels from an integer value.
    #[inline]
    pub const fn from_i32(value: i32) -> Self {
        Self(value as f32)
    }

    /// Converts to device pixels using the given scale factor.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::{px, device_px};
    ///
    /// // On a 2x Retina display
    /// let logical = px(100.0);
    /// let device = logical.to_device_pixels(2.0);
    /// assert_eq!(device, device_px(200));
    /// ```
    #[inline]
    #[must_use]
    pub fn to_device_pixels(self, scale_factor: f32) -> DevicePixels {
        DevicePixels((self.0 * scale_factor).round() as i32)
    }

    /// Creates from device pixels using the given scale factor.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::{Pixels, device_px};
    ///
    /// // Convert device pixels to logical pixels
    /// let device = device_px(200);
    /// let logical = Pixels::from_device_pixels(device, 2.0);
    /// assert_eq!(logical.get(), 100.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn from_device_pixels(device: DevicePixels, scale_factor: f32) -> Self {
        Pixels(device.0 as f32 / scale_factor)
    }

    /// Creates from scaled pixels (same value, different type).
    #[inline]
    #[must_use]
    pub fn from_scaled_pixels(scaled: ScaledPixels, scale_factor: f32) -> Self {
        Pixels(scaled.0 / scale_factor)
    }

    /// Maps the inner value with a function.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::px;
    ///
    /// let value = px(100.0);
    /// let squared = value.map(|v| v * v);
    /// assert_eq!(squared.get(), 10000.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn map(self, f: impl FnOnce(f32) -> f32) -> Self {
        Pixels(f(self.0))
    }
}

// ============================================================================
// TRAIT IMPLEMENTATIONS - Unit and NumericUnit
// ============================================================================

impl Unit for Pixels {
    type Scalar = f32;

    #[inline]
    fn one() -> Self {
        Pixels(1.0)
    }

    const MIN: Self = Pixels::MIN;
    const MAX: Self = Pixels::MAX;
}

impl NumericUnit for Pixels {
    #[inline]
    fn abs(self) -> Self {
        Self(self.0.abs())
    }

    #[inline]
    fn min(self, other: Self) -> Self {
        Self(self.0.min(other.0))
    }

    #[inline]
    fn max(self, other: Self) -> Self {
        Self(self.0.max(other.0))
    }
}

// ============================================================================
// ARITHMETIC OPERATORS
// ============================================================================

// Arithmetic operators
impl Add for Pixels {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl AddAssign for Pixels {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl Sub for Pixels {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl SubAssign for Pixels {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl Mul<f32> for Pixels {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: f32) -> Self::Output {
        Self(self.0 * rhs)
    }
}

impl Mul<Pixels> for f32 {
    type Output = Pixels;
    #[inline]
    fn mul(self, rhs: Pixels) -> Self::Output {
        Pixels(self * rhs.0)
    }
}

impl Mul<usize> for Pixels {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: usize) -> Self::Output {
        Self(self.0 * rhs as f32)
    }
}

impl Mul<Pixels> for usize {
    type Output = Pixels;
    #[inline]
    fn mul(self, rhs: Pixels) -> Self::Output {
        Pixels(self as f32 * rhs.0)
    }
}

impl Mul<i32> for Pixels {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: i32) -> Self::Output {
        Self(self.0 * rhs as f32)
    }
}

impl Mul<Pixels> for i32 {
    type Output = Pixels;
    #[inline]
    fn mul(self, rhs: Pixels) -> Self::Output {
        Pixels(self as f32 * rhs.0)
    }
}

impl MulAssign<f32> for Pixels {
    #[inline]
    fn mul_assign(&mut self, rhs: f32) {
        self.0 *= rhs;
    }
}

impl Div for Pixels {
    type Output = f32;
    #[inline]
    fn div(self, rhs: Self) -> Self::Output {
        self.0 / rhs.0
    }
}

impl Div<f32> for Pixels {
    type Output = Self;
    #[inline]
    fn div(self, rhs: f32) -> Self::Output {
        Self(self.0 / rhs)
    }
}

impl DivAssign<f32> for Pixels {
    #[inline]
    fn div_assign(&mut self, rhs: f32) {
        self.0 /= rhs;
    }
}

impl Rem for Pixels {
    type Output = Self;
    #[inline]
    fn rem(self, rhs: Self) -> Self::Output {
        Self(self.0 % rhs.0)
    }
}

impl RemAssign for Pixels {
    #[inline]
    fn rem_assign(&mut self, rhs: Self) {
        self.0 %= rhs.0;
    }
}

impl Neg for Pixels {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self::Output {
        Self(-self.0)
    }
}

// Conversions
impl From<f32> for Pixels {
    #[inline]
    fn from(value: f32) -> Self {
        Self(value)
    }
}

impl From<f64> for Pixels {
    #[inline]
    fn from(value: f64) -> Self {
        Self(value as f32)
    }
}

impl From<i32> for Pixels {
    #[inline]
    fn from(value: i32) -> Self {
        Self(value as f32)
    }
}

impl From<u32> for Pixels {
    #[inline]
    fn from(value: u32) -> Self {
        Self(value as f32)
    }
}

impl From<usize> for Pixels {
    #[inline]
    fn from(value: usize) -> Self {
        Self(value as f32)
    }
}

impl From<Pixels> for f32 {
    #[inline]
    fn from(pixels: Pixels) -> Self {
        pixels.0
    }
}

impl From<Pixels> for f64 {
    #[inline]
    fn from(pixels: Pixels) -> Self {
        pixels.0 as f64
    }
}

impl From<Pixels> for i32 {
    #[inline]
    fn from(pixels: Pixels) -> Self {
        pixels.0 as i32
    }
}

impl From<Pixels> for u32 {
    #[inline]
    fn from(pixels: Pixels) -> Self {
        pixels.0 as u32
    }
}

impl From<Pixels> for usize {
    #[inline]
    fn from(pixels: Pixels) -> Self {
        pixels.0 as usize
    }
}

// Formatting
impl Display for Pixels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}px", self.0)
    }
}

impl Debug for Pixels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(self, f)
    }
}

// Ordering (using total_cmp for proper NaN handling)
impl Eq for Pixels {}

impl PartialOrd for Pixels {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Pixels {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

// Hashing (using to_bits for proper NaN handling)
impl std::hash::Hash for Pixels {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

// Sum trait for iterator support
impl Sum for Pixels {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Pixels::ZERO, |acc, x| acc + x)
    }
}

impl<'a> Sum<&'a Pixels> for Pixels {
    fn sum<I: Iterator<Item = &'a Self>>(iter: I) -> Self {
        iter.fold(Pixels::ZERO, |acc, x| acc + *x)
    }
}

// ============================================================================
// DEVICE PIXELS - Physical display pixels
// ============================================================================

/// Physical pixels on the display device.
///
/// `DevicePixels` represents actual hardware pixels on the screen. This is used
/// when interfacing with GPU rendering, framebuffers, or other operations that
/// require precise pixel-level control.
///
/// Unlike logical [`Pixels`], device pixels are not affected by display scaling
/// and always correspond to real hardware pixels.
///
/// # Examples
///
/// ```rust
/// use flui_types::geometry::{DevicePixels, device_px};
///
/// let width = device_px(1920);
/// let height = device_px(1080);
///
/// // Calculate buffer size
/// let bytes_per_pixel = 4;  // RGBA
/// let buffer_size = width.to_bytes(bytes_per_pixel);
/// ```
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct DevicePixels(pub i32);

/// Convenience function to create [`DevicePixels`].
///
/// # Examples
///
/// ```rust
/// use flui_types::geometry::device_px;
///
/// let width = device_px(1920);
/// let height = device_px(1080);
/// ```
#[inline]
pub const fn device_px(value: i32) -> DevicePixels {
    DevicePixels(value)
}

impl DevicePixels {
    /// Zero device pixels.
    pub const ZERO: DevicePixels = DevicePixels(0);

    /// Creates a new `DevicePixels` value.
    #[inline]
    pub const fn new(value: i32) -> Self {
        Self(value)
    }

    /// Returns the inner i32 value.
    #[inline]
    pub const fn get(self) -> i32 {
        self.0
    }

    /// Converts to bytes assuming the given bytes per pixel.
    ///
    /// Useful for calculating buffer sizes for framebuffers or textures.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::device_px;
    ///
    /// let pixels = device_px(100);
    /// let bytes = pixels.to_bytes(4);  // RGBA = 4 bytes per pixel
    /// assert_eq!(bytes, 400);
    /// ```
    #[inline]
    pub fn to_bytes(self, bytes_per_pixel: u8) -> u32 {
        (self.0 as u32) * (bytes_per_pixel as u32)
    }

    /// Returns the absolute value.
    #[inline]
    #[must_use]
    pub fn abs(self) -> Self {
        Self(self.0.abs())
    }

    /// Returns the minimum of two values.
    #[inline]
    #[must_use]
    pub fn min(self, other: Self) -> Self {
        Self(self.0.min(other.0))
    }

    /// Returns the maximum of two values.
    #[inline]
    #[must_use]
    pub fn max(self, other: Self) -> Self {
        Self(self.0.max(other.0))
    }

    /// Clamps the value between min and max.
    #[inline]
    #[must_use]
    pub fn clamp(self, min: Self, max: Self) -> Self {
        Self(self.0.clamp(min.0, max.0))
    }

    /// Returns the sign of the value (-1, 0, or 1).
    #[inline]
    pub fn signum(self) -> i32 {
        self.0.signum()
    }

    /// Maximum of two values (const version).
    #[inline]
    #[must_use]
    pub const fn max_const(self, other: Self) -> Self {
        if self.0 > other.0 {
            self
        } else {
            other
        }
    }

    /// Minimum of two values (const version).
    #[inline]
    #[must_use]
    pub const fn min_const(self, other: Self) -> Self {
        if self.0 < other.0 {
            self
        } else {
            other
        }
    }

    /// Converts to logical pixels using the given scale factor.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::{device_px, px};
    ///
    /// let device = device_px(200);
    /// let logical = device.to_pixels(2.0);
    /// assert_eq!(logical.get(), 100.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn to_pixels(self, scale_factor: f32) -> Pixels {
        Pixels(self.0 as f32 / scale_factor)
    }

    /// Converts to scaled pixels.
    ///
    /// This is essentially a type conversion since ScaledPixels is already
    /// at device-pixel resolution before final rounding.
    #[inline]
    #[must_use]
    pub fn to_scaled_pixels(self) -> ScaledPixels {
        ScaledPixels(self.0 as f32)
    }

    /// Maps the inner value with a function.
    #[inline]
    #[must_use]
    pub fn map(self, f: impl FnOnce(i32) -> i32) -> Self {
        DevicePixels(f(self.0))
    }
}

// Arithmetic operators for DevicePixels (using saturating arithmetic to prevent overflow)
impl Add for DevicePixels {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0.saturating_add(rhs.0))
    }
}

impl AddAssign for DevicePixels {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.0 = self.0.saturating_add(rhs.0);
    }
}

impl Sub for DevicePixels {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0.saturating_sub(rhs.0))
    }
}

impl SubAssign for DevicePixels {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.0 = self.0.saturating_sub(rhs.0);
    }
}

impl Div for DevicePixels {
    type Output = i32;
    #[inline]
    fn div(self, rhs: Self) -> Self::Output {
        self.0 / rhs.0
    }
}

impl Mul<i32> for DevicePixels {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: i32) -> Self::Output {
        Self(self.0.saturating_mul(rhs))
    }
}

impl Neg for DevicePixels {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self::Output {
        Self(-self.0)
    }
}

// Conversions
impl From<i32> for DevicePixels {
    #[inline]
    fn from(value: i32) -> Self {
        Self(value)
    }
}

impl From<u32> for DevicePixels {
    #[inline]
    fn from(value: u32) -> Self {
        Self(value as i32)
    }
}

impl From<usize> for DevicePixels {
    #[inline]
    fn from(value: usize) -> Self {
        Self(value as i32)
    }
}

impl From<DevicePixels> for i32 {
    #[inline]
    fn from(pixels: DevicePixels) -> Self {
        pixels.0
    }
}

impl From<DevicePixels> for u32 {
    #[inline]
    fn from(pixels: DevicePixels) -> Self {
        pixels.0 as u32
    }
}

impl From<DevicePixels> for u64 {
    #[inline]
    fn from(pixels: DevicePixels) -> Self {
        pixels.0 as u64
    }
}

impl From<DevicePixels> for usize {
    #[inline]
    fn from(pixels: DevicePixels) -> Self {
        pixels.0 as usize
    }
}

// Formatting
impl Display for DevicePixels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}dpx", self.0)
    }
}

impl Debug for DevicePixels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} px (device)", self.0)
    }
}

// ============================================================================
// DEVICE PIXELS - TRAIT IMPLEMENTATIONS
// ============================================================================

impl Unit for DevicePixels {
    type Scalar = i32;

    #[inline]
    fn one() -> Self {
        DevicePixels(1)
    }

    const MIN: Self = DevicePixels(i32::MIN);
    const MAX: Self = DevicePixels(i32::MAX);
}

impl NumericUnit for DevicePixels {
    #[inline]
    fn abs(self) -> Self {
        Self(self.0.abs())
    }

    #[inline]
    fn min(self, other: Self) -> Self {
        Self(self.0.min(other.0))
    }

    #[inline]
    fn max(self, other: Self) -> Self {
        Self(self.0.max(other.0))
    }
}

// Implement Mul<f32> for DevicePixels to satisfy NumericUnit trait bound
impl Mul<f32> for DevicePixels {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: f32) -> Self::Output {
        DevicePixels((self.0 as f32 * rhs).round() as i32)
    }
}

// Implement Div<f32> for DevicePixels to satisfy NumericUnit trait bound
impl Div<f32> for DevicePixels {
    type Output = Self;

    #[inline]
    fn div(self, rhs: f32) -> Self::Output {
        DevicePixels((self.0 as f32 / rhs).round() as i32)
    }
}

// Sum trait for iterator support
impl Sum for DevicePixels {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(DevicePixels::ZERO, |acc, x| acc + x)
    }
}

impl<'a> Sum<&'a DevicePixels> for DevicePixels {
    fn sum<I: Iterator<Item = &'a Self>>(iter: I) -> Self {
        iter.fold(DevicePixels::ZERO, |acc, x| acc + *x)
    }
}

// ============================================================================
// SCALED PIXELS - Display-scaled pixels
// ============================================================================

/// Pixels scaled by the display's scale factor.
///
/// `ScaledPixels` represents logical pixels multiplied by the display scale
/// factor. This is the intermediate unit between logical [`Pixels`] and
/// physical [`DevicePixels`].
///
/// On a 2x Retina display:
/// - 100 logical [`Pixels`] → 200 [`ScaledPixels`] → 200 [`DevicePixels`]
///
/// # Examples
///
/// ```rust
/// use flui_types::geometry::{px, scaled_px};
///
/// let logical = px(100.0);
/// let scaled = logical.scale(2.0);
///
/// // Convert to device pixels (rounded)
/// let device = scaled.to_device_pixels();
/// ```
#[derive(Clone, Copy, Default, PartialEq)]
#[repr(transparent)]
pub struct ScaledPixels(pub f32);

/// Convenience function to create [`ScaledPixels`].
#[inline]
pub const fn scaled_px(value: f32) -> ScaledPixels {
    ScaledPixels(value)
}

impl ScaledPixels {
    /// Zero scaled pixels.
    pub const ZERO: ScaledPixels = ScaledPixels(0.0);

    /// Creates a new `ScaledPixels` value.
    #[inline]
    pub const fn new(value: f32) -> Self {
        Self(value)
    }

    /// Returns the inner f32 value.
    #[inline]
    pub const fn get(self) -> f32 {
        self.0
    }

    /// Floors the value to the nearest whole number.
    #[inline]
    #[must_use]
    pub fn floor(self) -> Self {
        Self(self.0.floor())
    }

    /// Rounds the value to the nearest whole number.
    #[inline]
    #[must_use]
    pub fn round(self) -> Self {
        Self(self.0.round())
    }

    /// Ceils the value to the nearest whole number.
    #[inline]
    #[must_use]
    pub fn ceil(self) -> Self {
        Self(self.0.ceil())
    }

    /// Truncates the fractional part.
    #[inline]
    #[must_use]
    pub fn trunc(self) -> Self {
        Self(self.0.trunc())
    }

    /// Returns the absolute value.
    #[inline]
    #[must_use]
    pub fn abs(self) -> Self {
        Self(self.0.abs())
    }

    /// Converts to [`DevicePixels`] by rounding to the nearest integer.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::{scaled_px, device_px};
    ///
    /// let scaled = scaled_px(199.7);
    /// assert_eq!(scaled.to_device_pixels(), device_px(200));
    /// ```
    #[inline]
    #[must_use]
    pub fn to_device_pixels(self) -> DevicePixels {
        DevicePixels(self.0.round() as i32)
    }

    /// Checks if the value is finite (not infinite or NaN).
    #[inline]
    pub fn is_finite(self) -> bool {
        self.0.is_finite()
    }

    /// Returns the minimum of two values.
    #[inline]
    #[must_use]
    pub fn min(self, other: Self) -> Self {
        Self(self.0.min(other.0))
    }

    /// Returns the maximum of two values.
    #[inline]
    #[must_use]
    pub fn max(self, other: Self) -> Self {
        Self(self.0.max(other.0))
    }

    /// Clamps the value between min and max.
    #[inline]
    #[must_use]
    pub fn clamp(self, min: Self, max: Self) -> Self {
        Self(self.0.clamp(min.0, max.0))
    }

    /// Returns the sign of the value (-1.0, 0.0, or 1.0).
    #[inline]
    pub fn signum(self) -> f32 {
        self.0.signum()
    }

    /// Converts to logical pixels using the given scale factor.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::{scaled_px, px};
    ///
    /// let scaled = scaled_px(200.0);
    /// let logical = scaled.to_pixels(2.0);
    /// assert_eq!(logical.get(), 100.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn to_pixels(self, scale_factor: f32) -> Pixels {
        Pixels(self.0 / scale_factor)
    }

    /// Maps the inner value with a function.
    #[inline]
    #[must_use]
    pub fn map(self, f: impl FnOnce(f32) -> f32) -> Self {
        ScaledPixels(f(self.0))
    }
}

// Ordering (using total_cmp for proper NaN handling)
impl Eq for ScaledPixels {}

impl PartialOrd for ScaledPixels {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScaledPixels {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

// Hashing (using to_bits for proper NaN handling)
impl std::hash::Hash for ScaledPixels {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

// Arithmetic operators
impl Add for ScaledPixels {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl AddAssign for ScaledPixels {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl Sub for ScaledPixels {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl SubAssign for ScaledPixels {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl Mul<f32> for ScaledPixels {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: f32) -> Self::Output {
        Self(self.0 * rhs)
    }
}

impl Mul<ScaledPixels> for f32 {
    type Output = ScaledPixels;
    #[inline]
    fn mul(self, rhs: ScaledPixels) -> Self::Output {
        ScaledPixels(self * rhs.0)
    }
}

impl MulAssign<f32> for ScaledPixels {
    #[inline]
    fn mul_assign(&mut self, rhs: f32) {
        self.0 *= rhs;
    }
}

impl Div for ScaledPixels {
    type Output = f32;
    #[inline]
    fn div(self, rhs: Self) -> Self::Output {
        self.0 / rhs.0
    }
}

impl Div<f32> for ScaledPixels {
    type Output = Self;
    #[inline]
    fn div(self, rhs: f32) -> Self::Output {
        Self(self.0 / rhs)
    }
}

impl DivAssign<f32> for ScaledPixels {
    #[inline]
    fn div_assign(&mut self, rhs: f32) {
        self.0 /= rhs;
    }
}

impl Neg for ScaledPixels {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self::Output {
        Self(-self.0)
    }
}

impl Rem for ScaledPixels {
    type Output = Self;
    #[inline]
    fn rem(self, rhs: Self) -> Self::Output {
        Self(self.0 % rhs.0)
    }
}

impl RemAssign for ScaledPixels {
    #[inline]
    fn rem_assign(&mut self, rhs: Self) {
        self.0 %= rhs.0;
    }
}

// ============================================================================
// SCALED PIXELS - TRAIT IMPLEMENTATIONS
// ============================================================================

impl Unit for ScaledPixels {
    type Scalar = f32;

    #[inline]
    fn one() -> Self {
        ScaledPixels(1.0)
    }

    const MIN: Self = ScaledPixels(f32::MIN);
    const MAX: Self = ScaledPixels(f32::MAX);
}

impl NumericUnit for ScaledPixels {
    #[inline]
    fn abs(self) -> Self {
        Self(self.0.abs())
    }

    #[inline]
    fn min(self, other: Self) -> Self {
        Self(self.0.min(other.0))
    }

    #[inline]
    fn max(self, other: Self) -> Self {
        Self(self.0.max(other.0))
    }
}

// Sum trait for iterator support
impl Sum for ScaledPixels {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(ScaledPixels::ZERO, |acc, x| acc + x)
    }
}

impl<'a> Sum<&'a ScaledPixels> for ScaledPixels {
    fn sum<I: Iterator<Item = &'a Self>>(iter: I) -> Self {
        iter.fold(ScaledPixels::ZERO, |acc, x| acc + *x)
    }
}

// Conversions
impl From<f32> for ScaledPixels {
    #[inline]
    fn from(value: f32) -> Self {
        Self(value)
    }
}

impl From<ScaledPixels> for f32 {
    #[inline]
    fn from(pixels: ScaledPixels) -> Self {
        pixels.0
    }
}

// Formatting
impl Display for ScaledPixels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}spx", self.0)
    }
}

impl Debug for ScaledPixels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} px (scaled)", self.0)
    }
}

// ============================================================================
// String parsing (FromStr)
// ============================================================================

/// Error type for parsing length values from strings.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseLengthError {
    /// The input string that failed to parse.
    pub input: String,
    /// Description of what formats are expected.
    pub expected: &'static str,
}

impl Display for ParseLengthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Failed to parse '{}': expected {}",
            self.input, self.expected
        )
    }
}

impl std::error::Error for ParseLengthError {}

impl std::str::FromStr for Pixels {
    type Err = ParseLengthError;

    /// Parses a `Pixels` value from a string.
    ///
    /// Supported formats:
    /// - `"100"` - bare number
    /// - `"100px"` - with "px" suffix
    /// - `"100.5"` - decimal values
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::Pixels;
    ///
    /// let p: Pixels = "100".parse().unwrap();
    /// assert_eq!(p.get(), 100.0);
    ///
    /// let p: Pixels = "100px".parse().unwrap();
    /// assert_eq!(p.get(), 100.0);
    ///
    /// let p: Pixels = "123.5".parse().unwrap();
    /// assert_eq!(p.get(), 123.5);
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let num_str = s.strip_suffix("px").unwrap_or(s).trim();

        num_str
            .parse::<f32>()
            .map(Pixels)
            .map_err(|_| ParseLengthError {
                input: s.to_string(),
                expected: "a number like '100' or '100px'",
            })
    }
}

impl std::str::FromStr for DevicePixels {
    type Err = ParseLengthError;

    /// Parses a `DevicePixels` value from a string.
    ///
    /// Supported formats:
    /// - `"1920"` - bare integer
    /// - `"1920dpx"` - with "dpx" suffix
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::DevicePixels;
    ///
    /// let dp: DevicePixels = "1920".parse().unwrap();
    /// assert_eq!(dp.get(), 1920);
    ///
    /// let dp: DevicePixels = "1920dpx".parse().unwrap();
    /// assert_eq!(dp.get(), 1920);
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let num_str = s.strip_suffix("dpx").unwrap_or(s).trim();

        num_str
            .parse::<i32>()
            .map(DevicePixels)
            .map_err(|_| ParseLengthError {
                input: s.to_string(),
                expected: "an integer like '1920' or '1920dpx'",
            })
    }
}

impl std::str::FromStr for ScaledPixels {
    type Err = ParseLengthError;

    /// Parses a `ScaledPixels` value from a string.
    ///
    /// Supported formats:
    /// - `"200"` - bare number
    /// - `"200spx"` - with "spx" suffix
    /// - `"200.5"` - decimal values
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::ScaledPixels;
    ///
    /// let sp: ScaledPixels = "200".parse().unwrap();
    /// assert_eq!(sp.get(), 200.0);
    ///
    /// let sp: ScaledPixels = "200.5spx".parse().unwrap();
    /// assert_eq!(sp.get(), 200.5);
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let num_str = s.strip_suffix("spx").unwrap_or(s).trim();

        num_str
            .parse::<f32>()
            .map(ScaledPixels)
            .map_err(|_| ParseLengthError {
                input: s.to_string(),
                expected: "a number like '200' or '200spx'",
            })
    }
}

// ============================================================================
// RADIANS - Type-safe angle representation
// ============================================================================

/// Represents an angle in radians.
///
/// `Radians` provides a type-safe wrapper around `f32` for working with angles,
/// making code more explicit and preventing confusion between radians and degrees.
///
/// # Examples
///
/// ```rust
/// use flui_types::geometry::{radians, Radians};
/// use std::f32::consts::PI;
///
/// // Create from radians
/// let angle = radians(PI / 2.0);
///
/// // Convert from degrees
/// let right_angle = Radians::from_degrees(90.0);
/// assert!((right_angle.0 - PI / 2.0).abs() < 1e-6);
///
/// // Convert to degrees
/// assert!((angle.to_degrees() - 90.0).abs() < 1e-6);
///
/// // Normalize to [0, 2π)
/// let normalized = radians(PI * 3.0).normalize();
/// assert!((normalized.0 - PI).abs() < 1e-6);
/// ```
#[derive(Clone, Copy, Default, PartialEq)]
#[repr(transparent)]
pub struct Radians(pub f32);

/// Convenience function to create [`Radians`] from a raw value.
///
/// # Examples
///
/// ```rust
/// use flui_types::geometry::radians;
/// use std::f32::consts::PI;
///
/// let half_pi = radians(PI / 2.0);
/// let full_circle = radians(PI * 2.0);
/// ```
#[inline]
pub const fn radians(value: f32) -> Radians {
    Radians(value)
}

impl Radians {
    /// Zero radians.
    pub const ZERO: Radians = Radians(0.0);

    /// π radians (180 degrees).
    pub const PI: Radians = Radians(std::f32::consts::PI);

    /// 2π radians (360 degrees, full circle).
    pub const TAU: Radians = Radians(std::f32::consts::TAU);

    /// Creates a new `Radians` value.
    #[inline]
    pub const fn new(value: f32) -> Self {
        Self(value)
    }

    /// Returns the inner f32 value.
    #[inline]
    pub const fn get(self) -> f32 {
        self.0
    }

    /// Creates radians from degrees.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Radians;
    /// use std::f32::consts::PI;
    ///
    /// let angle = Radians::from_degrees(180.0);
    /// assert_eq!(angle.0, PI);
    /// ```
    #[inline]
    pub fn from_degrees(degrees: f32) -> Self {
        Self(degrees.to_radians())
    }

    /// Converts radians to degrees.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::radians;
    /// use std::f32::consts::PI;
    ///
    /// let angle = radians(PI);
    /// assert_eq!(angle.to_degrees(), 180.0);
    /// ```
    #[inline]
    pub fn to_degrees(self) -> f32 {
        self.0.to_degrees()
    }

    /// Normalizes the angle to the range [0, 2π).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::radians;
    /// use std::f32::consts::PI;
    ///
    /// // 3π normalized to π
    /// let angle = radians(PI * 3.0).normalize();
    /// assert!((angle.0 - PI).abs() < 1e-6);
    ///
    /// // -π normalized to π
    /// let angle = radians(-PI).normalize();
    /// assert!((angle.0 - PI).abs() < 1e-6);
    /// ```
    #[inline]
    pub fn normalize(self) -> Self {
        let tau = std::f32::consts::TAU;
        Self(self.0.rem_euclid(tau))
    }

    /// Checks if the angle is zero (within epsilon tolerance).
    #[inline]
    pub fn is_zero(self) -> bool {
        self.0.abs() < f32::EPSILON
    }

    /// Checks if the angle is finite (not infinite or NaN).
    #[inline]
    pub fn is_finite(self) -> bool {
        self.0.is_finite()
    }

    /// Returns the absolute value.
    #[inline]
    #[must_use]
    pub fn abs(self) -> Self {
        Self(self.0.abs())
    }

    /// Returns the sign of the value (-1.0, 0.0, or 1.0).
    #[inline]
    pub fn signum(self) -> f32 {
        self.0.signum()
    }

    /// Linear interpolation between two angles.
    ///
    /// Uses shortest path interpolation (wraps around through 0°/360° boundary).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Radians;
    ///
    /// // Lerp from 350° to 10° goes through 0°, not backwards through 180°
    /// let a = Radians::from_degrees(350.0);
    /// let b = Radians::from_degrees(10.0);
    /// let mid = a.lerp(b, 0.5);
    /// // mid is close to 0° (or 360°)
    /// assert!((mid.to_degrees() % 360.0).abs() < 5.0 || (mid.to_degrees() % 360.0 - 360.0).abs() < 5.0);
    /// ```
    #[inline]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        let diff = (other.0 - self.0).rem_euclid(std::f32::consts::TAU);
        let shortest = if diff > std::f32::consts::PI {
            diff - std::f32::consts::TAU
        } else {
            diff
        };
        Self(self.0 + shortest * t)
    }

    /// Linear interpolation without shortest-path wrapping.
    ///
    /// Unlike [`lerp`](Self::lerp), this performs simple linear interpolation
    /// without considering angle wrapping. Useful when you want to interpolate
    /// through the "long way" around the circle.
    #[inline]
    pub fn lerp_linear(self, other: Self, t: f32) -> Self {
        Self(self.0 + (other.0 - self.0) * t)
    }
}

impl Display for Radians {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}rad", self.0)
    }
}

impl Debug for Radians {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(self, f)
    }
}

// Conversions
impl From<f32> for Radians {
    #[inline]
    fn from(value: f32) -> Self {
        Self(value)
    }
}

impl From<Radians> for f32 {
    #[inline]
    fn from(radians: Radians) -> Self {
        radians.0
    }
}

// Convert from Percentage (0.0 = 0°, 1.0 = 360°)
impl From<crate::geometry::Percentage> for Radians {
    #[inline]
    fn from(percentage: crate::geometry::Percentage) -> Self {
        radians(percentage.0 * std::f32::consts::TAU)
    }
}

// Arithmetic operators
impl Add for Radians {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl AddAssign for Radians {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl Sub for Radians {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl SubAssign for Radians {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl Neg for Radians {
    type Output = Self;

    #[inline]
    fn neg(self) -> Self::Output {
        Self(-self.0)
    }
}

impl Mul<f32> for Radians {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: f32) -> Self::Output {
        Self(self.0 * rhs)
    }
}

impl MulAssign<f32> for Radians {
    #[inline]
    fn mul_assign(&mut self, rhs: f32) {
        self.0 *= rhs;
    }
}

impl Div<f32> for Radians {
    type Output = Self;

    #[inline]
    fn div(self, rhs: f32) -> Self::Output {
        Self(self.0 / rhs)
    }
}

impl DivAssign<f32> for Radians {
    #[inline]
    fn div_assign(&mut self, rhs: f32) {
        self.0 /= rhs;
    }
}

impl Rem for Radians {
    type Output = Self;
    #[inline]
    fn rem(self, rhs: Self) -> Self::Output {
        Self(self.0 % rhs.0)
    }
}

impl RemAssign for Radians {
    #[inline]
    fn rem_assign(&mut self, rhs: Self) {
        self.0 %= rhs.0;
    }
}

// ============================================================================
// RADIANS - TRAIT IMPLEMENTATIONS
// ============================================================================

impl Unit for Radians {
    type Scalar = f32;

    #[inline]
    fn one() -> Self {
        Radians(1.0)
    }

    const MIN: Self = Radians(f32::MIN);
    const MAX: Self = Radians(f32::MAX);
}

impl NumericUnit for Radians {
    #[inline]
    fn abs(self) -> Self {
        Self(self.0.abs())
    }

    #[inline]
    fn min(self, other: Self) -> Self {
        Self(self.0.min(other.0))
    }

    #[inline]
    fn max(self, other: Self) -> Self {
        Self(self.0.max(other.0))
    }
}

// Ordering (using total_cmp for proper NaN handling)
impl Eq for Radians {}

impl PartialOrd for Radians {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Radians {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

// Hashing (using to_bits for proper NaN handling)
impl std::hash::Hash for Radians {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

impl std::str::FromStr for Radians {
    type Err = ParseLengthError;

    /// Parses a `Radians` value from a string.
    ///
    /// Supported formats:
    /// - `"1.57"` - bare number (radians)
    /// - `"1.57rad"` - with "rad" suffix
    /// - `"90deg"` - with "deg" suffix (converts to radians)
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::Radians;
    /// use std::f32::consts::PI;
    ///
    /// let r: Radians = "1.57".parse().unwrap();
    /// assert!((r.get() - 1.57).abs() < 0.01);
    ///
    /// let r: Radians = "1.57rad".parse().unwrap();
    /// assert!((r.get() - 1.57).abs() < 0.01);
    ///
    /// // Degrees to radians
    /// let r: Radians = "90deg".parse().unwrap();
    /// assert!((r.get() - PI / 2.0).abs() < 0.01);
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();

        // Check for degree suffix
        if let Some(num_str) = s.strip_suffix("deg") {
            return num_str
                .trim()
                .parse::<f32>()
                .map(Radians::from_degrees)
                .map_err(|_| ParseLengthError {
                    input: s.to_string(),
                    expected: "a number like '90deg', '1.57rad', or '1.57'",
                });
        }

        // Check for radian suffix
        let num_str = s.strip_suffix("rad").unwrap_or(s).trim();

        num_str
            .parse::<f32>()
            .map(Radians)
            .map_err(|_| ParseLengthError {
                input: s.to_string(),
                expected: "a number like '90deg', '1.57rad', or '1.57'",
            })
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Percentage;

    #[test]
    fn test_pixels_arithmetic() {
        let a = px(100.0);
        let b = px(50.0);

        assert_eq!(a + b, px(150.0));
        assert_eq!(a - b, px(50.0));
        assert_eq!(a * 2.0, px(200.0));
        assert_eq!(a / 2.0, px(50.0));
        assert_eq!(a / b, 2.0);
    }

    #[test]
    fn test_pixels_rounding() {
        assert_eq!(px(123.4).floor(), px(123.0));
        assert_eq!(px(123.4).round(), px(123.0));
        assert_eq!(px(123.6).round(), px(124.0));
        assert_eq!(px(123.4).ceil(), px(124.0));
    }

    #[test]
    fn test_pixels_scale() {
        let logical = px(100.0);
        let scaled = logical.scale(2.0);
        assert_eq!(scaled.get(), 200.0);
    }

    #[test]
    fn test_device_pixels() {
        let dp = device_px(1920);
        assert_eq!(dp.to_bytes(4), 7680); // 1920 * 4 bytes per pixel
    }

    #[test]
    fn test_scaled_to_device() {
        let scaled = scaled_px(199.7);
        assert_eq!(scaled.to_device_pixels(), device_px(200));

        let scaled2 = scaled_px(199.3);
        assert_eq!(scaled2.to_device_pixels(), device_px(199));
    }

    #[test]
    fn test_full_conversion_chain() {
        // Logical -> Scaled -> Device
        let logical = px(100.0);
        let scaled = logical.scale(2.0); // 2x display
        let device = scaled.to_device_pixels();

        assert_eq!(device, device_px(200));
    }

    #[test]
    fn test_pixels_ordering() {
        assert!(px(100.0) > px(50.0));
        assert!(px(50.0) < px(100.0));
        assert_eq!(px(100.0).max(px(50.0)), px(100.0));
        assert_eq!(px(100.0).min(px(50.0)), px(50.0));
    }

    #[test]
    fn test_pixels_display() {
        assert_eq!(format!("{}", px(100.0)), "100px");
        assert_eq!(format!("{}", device_px(1920)), "1920dpx");
        assert_eq!(format!("{}", scaled_px(200.0)), "200spx");
    }

    #[test]
    fn test_conversions() {
        // Pixels conversions
        let p = px(100.0);
        assert_eq!(f32::from(p), 100.0);
        assert_eq!(u32::from(p), 100);

        // DevicePixels conversions
        let dp = device_px(1920);
        assert_eq!(i32::from(dp), 1920);
        assert_eq!(usize::from(dp), 1920);
    }

    #[test]
    fn test_pixels_from_str() {
        // Bare numbers
        assert_eq!("100".parse::<Pixels>().unwrap(), px(100.0));
        assert_eq!("123.5".parse::<Pixels>().unwrap(), px(123.5));

        // With "px" suffix
        assert_eq!("100px".parse::<Pixels>().unwrap(), px(100.0));
        assert_eq!("123.5px".parse::<Pixels>().unwrap(), px(123.5));

        // With whitespace
        assert_eq!("  100  ".parse::<Pixels>().unwrap(), px(100.0));
        assert_eq!("100 px".parse::<Pixels>().unwrap(), px(100.0));

        // Invalid inputs
        assert!("abc".parse::<Pixels>().is_err());
        assert!("100rem".parse::<Pixels>().is_err());
        assert!("".parse::<Pixels>().is_err());
    }

    #[test]
    fn test_radians_creation() {
        let r = radians(1.0);
        assert_eq!(r.0, 1.0);

        let r = Radians::ZERO;
        assert_eq!(r.0, 0.0);
    }

    #[test]
    fn test_radians_conversions() {
        use std::f32::consts::PI;

        // From degrees
        assert_eq!(Radians::from_degrees(180.0).0, PI);
        assert_eq!(Radians::from_degrees(90.0).0, PI / 2.0);
        assert_eq!(Radians::from_degrees(360.0).0, PI * 2.0);

        // To degrees
        assert_eq!(radians(PI).to_degrees(), 180.0);
        assert_eq!(radians(PI / 2.0).to_degrees(), 90.0);
        assert_eq!(radians(PI * 2.0).to_degrees(), 360.0);

        // From percentage (0.0 = 0°, 1.0 = 360°)
        let half = Percentage(0.5);
        let r: Radians = half.into();
        assert!((r.0 - PI).abs() < 0.0001);

        let quarter = Percentage(0.25);
        let r: Radians = quarter.into();
        assert!((r.0 - PI / 2.0).abs() < 0.0001);
    }

    #[test]
    fn test_radians_arithmetic() {
        use std::f32::consts::PI;

        let r1 = radians(PI);
        let r2 = radians(PI / 2.0);

        assert_eq!((r1 + r2).0, PI * 1.5);
        assert_eq!((r1 - r2).0, PI / 2.0);
        assert_eq!((-r1).0, -PI);
        assert_eq!((r1 * 2.0).0, PI * 2.0);
        assert_eq!((r1 / 2.0).0, PI / 2.0);
    }

    #[test]
    fn test_radians_normalize() {
        use std::f32::consts::PI;

        // Normalize to [0, 2π)
        let normalized = radians(PI * 3.0).normalize();
        assert!((normalized.0 - PI).abs() < 0.0001);

        let normalized = radians(PI * 4.0).normalize();
        assert!(normalized.0.abs() < 0.0001);

        let normalized = radians(-PI).normalize();
        assert!((normalized.0 - PI).abs() < 0.0001);
    }

    #[test]
    fn test_unit_types_integration() {
        use crate::geometry::{Bounds, Corners, Edges, Point, Size};

        // Point works with all unit types
        let _p1: Point<Pixels> = Point::new(px(100.0), px(200.0));
        let _p2: Point<DevicePixels> = Point::new(device_px(200), device_px(400));
        let _p3: Point<ScaledPixels> = Point::new(scaled_px(150.0), scaled_px(300.0));

        // Size works with all unit types
        let _s1: Size<Pixels> = Size::new(px(100.0), px(200.0));
        let _s2: Size<DevicePixels> = Size::new(device_px(100), device_px(200));
        let _s3: Size<ScaledPixels> = Size::new(scaled_px(100.0), scaled_px(200.0));

        // Bounds works with all unit types
        let p: Point<Pixels> = Point::new(px(10.0), px(20.0));
        let s: Size<Pixels> = Size::new(px(100.0), px(50.0));
        let _b: Bounds<Pixels> = Bounds::new(p, s);

        // Edges works with all unit types
        let _e1: Edges<Pixels> = Edges::all(px(10.0));
        let _e2: Edges<DevicePixels> = Edges::all(device_px(10));
        let _e3: Edges<ScaledPixels> = Edges::all(scaled_px(10.0));

        // Corners works with all unit types
        let _c1: Corners<Pixels> = Corners::all(px(5.0));
        let _c2: Corners<DevicePixels> = Corners::all(device_px(5));
        let _c3: Corners<ScaledPixels> = Corners::all(scaled_px(5.0));

        // Type safety prevents mixing incompatible types
        // This would be a compile error:
        // let bad: Point<Pixels> = Point::new(device_px(100), device_px(200));
    }

    // ========================================================================
    // UNIT TRAIT TESTS
    // ========================================================================

    #[test]
    fn test_pixels_unit_trait() {
        use super::super::traits::Unit;

        // Test Unit trait
        let zero = Pixels::zero();
        assert_eq!(zero.0, 0.0);
        assert_eq!(zero, Pixels::ZERO);

        // Test NumericUnit trait methods
        let a = px(10.0);
        let b = px(20.0);

        assert_eq!(a.abs(), px(10.0));
        assert_eq!(px(-10.0).abs(), px(10.0));
        assert_eq!(a.min(b), px(10.0));
        assert_eq!(a.max(b), px(20.0));
    }

    #[test]
    fn test_pixels_conversions() {
        // Test From<Pixels> for f32
        let p = px(100.0);
        let f: f32 = p.into();
        assert_eq!(f, 100.0);

        // Test From<f32> for Pixels
        let p2: Pixels = 50.0.into();
        assert_eq!(p2.0, 50.0);

        // Test bidirectional conversion
        let original = px(123.456);
        let as_f32: f32 = original.into();
        let back_to_pixels: Pixels = as_f32.into();
        assert_eq!(original, back_to_pixels);
    }

    #[test]
    fn test_pixels_scale_method() {
        // Test scale() method
        let p = px(100.0);
        let scaled = p.scale(2.0);
        assert_eq!(scaled.0, 200.0);

        // Test with different scale factors
        assert_eq!(px(50.0).scale(1.5).0, 75.0);
        assert_eq!(px(100.0).scale(0.5).0, 50.0);

        // Test conversion chain
        let logical = px(100.0);
        let scaled = logical.scale(2.0);
        let device = scaled.to_device_pixels();
        assert_eq!(device, device_px(200));
    }

    #[test]
    fn test_device_pixels_unit() {
        use super::super::traits::Unit;

        // Test Unit trait
        let zero = DevicePixels::zero();
        assert_eq!(zero.0, 0);
        assert_eq!(zero, DevicePixels::ZERO);

        // Test NumericUnit trait methods
        let a = device_px(10);
        let b = device_px(20);

        assert_eq!(a.abs().0, 10);
        assert_eq!(device_px(-10).abs().0, 10);
        assert_eq!(a.min(b).0, 10);
        assert_eq!(a.max(b).0, 20);

        // Test rounding in multiplication/division
        let val = device_px(10);
        assert_eq!((val * 1.4).0, 14); // 14.0 rounds to 14
        assert_eq!((val * 1.6).0, 16); // 16.0 rounds to 16
        assert_eq!((val / 3.0).0, 3); // 3.333... rounds to 3
    }

    #[test]
    fn test_scaled_pixels_unit() {
        use super::super::traits::Unit;

        // Test Unit trait
        let zero = ScaledPixels::zero();
        assert_eq!(zero.0, 0.0);
        assert_eq!(zero, ScaledPixels::ZERO);

        // Test NumericUnit trait methods
        let a = scaled_px(10.0);
        let b = scaled_px(20.0);

        assert_eq!(a.abs().0, 10.0);
        assert_eq!(scaled_px(-10.0).abs().0, 10.0);
        assert_eq!(a.min(b).0, 10.0);
        assert_eq!(a.max(b).0, 20.0);

        // Test to_device_pixels conversion
        let sp = scaled_px(200.0);
        let dp = sp.to_device_pixels();
        assert_eq!(dp.0, 200);

        // Test rounding behavior in to_device_pixels
        assert_eq!(scaled_px(199.4).to_device_pixels().0, 199);
        assert_eq!(scaled_px(199.5).to_device_pixels().0, 200);
        assert_eq!(scaled_px(199.7).to_device_pixels().0, 200);
    }

    #[test]
    fn test_rems_unit() {
        use super::super::traits::Unit;
        use crate::geometry::rems;

        // Test Unit trait
        let zero = <crate::geometry::Rems as Unit>::zero();
        assert_eq!(zero.0, 0.0);

        // Test NumericUnit trait methods
        let a = rems(1.0);
        let b = rems(0.5);

        assert_eq!(a.abs().0, 1.0);
        assert_eq!(rems(-1.0).abs().0, 1.0);
        assert_eq!(a.min(b).0, 0.5);
        assert_eq!(a.max(b).0, 1.0);

        // Test conversions
        let r = rems(1.5);
        let f: f32 = r.into();
        assert_eq!(f, 1.5);

        let r2: crate::geometry::Rems = 2.0.into();
        assert_eq!(r2.0, 2.0);
    }

    #[test]
    fn test_unit_conversions() {
        // Test ScaledPixels from/to f32
        let sp = scaled_px(100.0);
        let f: f32 = sp.into();
        assert_eq!(f, 100.0);

        let sp2: ScaledPixels = 200.0.into();
        assert_eq!(sp2.0, 200.0);

        // Test Rems from/to f32
        use crate::geometry::rems;
        let r = rems(1.5);
        let f: f32 = r.into();
        assert_eq!(f, 1.5);

        let r2: crate::geometry::Rems = 2.5.into();
        assert_eq!(r2.0, 2.5);

        // Test bidirectional conversion for ScaledPixels
        let original = scaled_px(123.456);
        let as_f32: f32 = original.into();
        let back: ScaledPixels = as_f32.into();
        assert_eq!(original, back);
    }

    // ========================================================================
    // BOUNDARY CASE TESTS
    // ========================================================================

    #[test]
    fn test_device_pixels_saturating_arithmetic() {
        // Addition should saturate, not overflow
        let max = device_px(i32::MAX);
        let one = device_px(1);
        assert_eq!(max + one, device_px(i32::MAX));

        let large = device_px(i32::MAX - 10);
        assert_eq!(large + device_px(20), device_px(i32::MAX));

        // Subtraction should saturate
        let min = device_px(i32::MIN);
        assert_eq!(min - one, device_px(i32::MIN));

        // Multiplication should saturate
        let big = device_px(i32::MAX / 2);
        assert_eq!(big * 3, device_px(i32::MAX));

        // Negative overflow
        let neg = device_px(-1000000);
        assert_eq!(neg * 1000000, device_px(i32::MIN));
    }

    #[test]
    fn test_device_pixels_const_methods() {
        // Test const max/min
        const A: DevicePixels = device_px(10);
        const B: DevicePixels = device_px(20);
        const MAX: DevicePixels = A.max_const(B);
        const MIN: DevicePixels = A.min_const(B);

        assert_eq!(MAX, device_px(20));
        assert_eq!(MIN, device_px(10));
    }

    #[test]
    fn test_device_pixels_signum() {
        assert_eq!(device_px(100).signum(), 1);
        assert_eq!(device_px(-100).signum(), -1);
        assert_eq!(device_px(0).signum(), 0);
    }

    #[test]
    fn test_pixels_signum_raw() {
        // Test renamed inherent method
        assert_eq!(px(100.0).signum_raw(), 1.0);
        assert_eq!(px(-100.0).signum_raw(), -1.0);
        // Rust's signum() returns 1.0 for positive zero (IEEE 754)
        assert_eq!(px(0.0).signum_raw(), 1.0);

        // Test NaN handling
        assert!(px(f32::NAN).signum_raw().is_nan());
    }

    #[test]
    fn test_pixels_nan_ordering() {
        let nan = px(f32::NAN);
        let normal = px(100.0);
        let inf = px(f32::INFINITY);

        // NaN should compare as greater than everything with total_cmp
        assert!(nan > normal);
        assert!(nan > inf);
    }

    #[test]
    fn test_scaled_pixels_ordering_and_hash() {
        use std::collections::HashSet;

        // Test Ord
        assert!(scaled_px(100.0) > scaled_px(50.0));
        assert!(scaled_px(50.0) < scaled_px(100.0));
        assert_eq!(scaled_px(100.0).cmp(&scaled_px(100.0)), std::cmp::Ordering::Equal);

        // Test Hash - can be used in HashSet
        let mut set = HashSet::new();
        set.insert(scaled_px(100.0));
        set.insert(scaled_px(200.0));
        set.insert(scaled_px(100.0)); // duplicate

        assert_eq!(set.len(), 2);
        assert!(set.contains(&scaled_px(100.0)));
        assert!(set.contains(&scaled_px(200.0)));
    }

    #[test]
    fn test_scaled_pixels_clamp_and_signum() {
        // Test clamp
        assert_eq!(scaled_px(50.0).clamp(scaled_px(60.0), scaled_px(100.0)), scaled_px(60.0));
        assert_eq!(scaled_px(150.0).clamp(scaled_px(60.0), scaled_px(100.0)), scaled_px(100.0));
        assert_eq!(scaled_px(80.0).clamp(scaled_px(60.0), scaled_px(100.0)), scaled_px(80.0));

        // Test signum (Rust's signum returns 1.0 for positive zero)
        assert_eq!(scaled_px(100.0).signum(), 1.0);
        assert_eq!(scaled_px(-100.0).signum(), -1.0);
        assert_eq!(scaled_px(0.0).signum(), 1.0);
    }

    #[test]
    fn test_radians_lerp_wrapping() {

        // Lerp from 350° to 10° should go through 0°, not backwards
        let a = Radians::from_degrees(350.0);
        let b = Radians::from_degrees(10.0);
        let mid = a.lerp(b, 0.5);

        // Should be close to 0° (or 360°)
        let deg = mid.to_degrees().rem_euclid(360.0);
        assert!(deg < 5.0 || deg > 355.0, "Expected near 0°, got {}°", deg);

        // Test linear lerp (no wrapping)
        let linear_mid = a.lerp_linear(b, 0.5);
        let linear_deg = linear_mid.to_degrees();
        // Linear lerp goes the "long way" (350 - 340/2 = 180)
        assert!((linear_deg - 180.0).abs() < 1.0, "Expected near 180°, got {}°", linear_deg);
    }

    #[test]
    fn test_radians_utility_methods() {
        use std::f32::consts::PI;

        // Test is_finite
        assert!(radians(PI).is_finite());
        assert!(!radians(f32::INFINITY).is_finite());
        assert!(!radians(f32::NAN).is_finite());

        // Test abs
        assert_eq!(radians(-PI).abs(), radians(PI));
        assert_eq!(radians(PI).abs(), radians(PI));

        // Test signum (Rust's signum returns 1.0 for positive zero)
        assert_eq!(radians(PI).signum(), 1.0);
        assert_eq!(radians(-PI).signum(), -1.0);
        assert_eq!(radians(0.0).signum(), 1.0);
    }

    #[test]
    fn test_device_pixels_from_str() {
        // Bare integers
        assert_eq!("1920".parse::<DevicePixels>().unwrap(), device_px(1920));
        assert_eq!("-100".parse::<DevicePixels>().unwrap(), device_px(-100));

        // With "dpx" suffix
        assert_eq!("1920dpx".parse::<DevicePixels>().unwrap(), device_px(1920));

        // With whitespace
        assert_eq!("  1920  ".parse::<DevicePixels>().unwrap(), device_px(1920));

        // Invalid inputs
        assert!("abc".parse::<DevicePixels>().is_err());
        assert!("100.5".parse::<DevicePixels>().is_err()); // Not an integer
        assert!("".parse::<DevicePixels>().is_err());
    }

    #[test]
    fn test_scaled_pixels_from_str() {
        // Bare numbers
        assert_eq!("200".parse::<ScaledPixels>().unwrap(), scaled_px(200.0));
        assert_eq!("200.5".parse::<ScaledPixels>().unwrap(), scaled_px(200.5));

        // With "spx" suffix
        assert_eq!("200spx".parse::<ScaledPixels>().unwrap(), scaled_px(200.0));
        assert_eq!("200.5spx".parse::<ScaledPixels>().unwrap(), scaled_px(200.5));

        // With whitespace
        assert_eq!("  200  ".parse::<ScaledPixels>().unwrap(), scaled_px(200.0));

        // Invalid inputs
        assert!("abc".parse::<ScaledPixels>().is_err());
        assert!("".parse::<ScaledPixels>().is_err());
    }

    // ========================================================================
    // RADIANS TRAIT TESTS
    // ========================================================================

    #[test]
    fn test_radians_unit_trait() {
        use super::super::traits::{NumericUnit, Unit};
        use std::f32::consts::PI;

        // Test Unit trait
        let zero = Radians::zero();
        assert_eq!(zero.0, 0.0);
        assert_eq!(zero, Radians::ZERO);

        let one = Radians::one();
        assert_eq!(one.0, 1.0);

        // Test NumericUnit trait methods
        let a = radians(PI);
        let b = radians(PI / 2.0);

        assert_eq!(a.abs(), radians(PI));
        assert_eq!(radians(-PI).abs(), radians(PI));
        // Use qualified syntax to avoid ambiguity with Ord::min/max
        assert_eq!(NumericUnit::min(a, b), radians(PI / 2.0));
        assert_eq!(NumericUnit::max(a, b), radians(PI));
    }

    #[test]
    fn test_radians_ordering_and_hash() {
        use std::collections::HashSet;
        use std::f32::consts::PI;

        // Test Ord
        assert!(radians(PI) > radians(PI / 2.0));
        assert!(radians(PI / 2.0) < radians(PI));
        assert_eq!(radians(PI).cmp(&radians(PI)), std::cmp::Ordering::Equal);

        // Test Hash - can be used in HashSet
        let mut set = HashSet::new();
        set.insert(radians(PI));
        set.insert(radians(PI / 2.0));
        set.insert(radians(PI)); // duplicate

        assert_eq!(set.len(), 2);
        assert!(set.contains(&radians(PI)));
    }

    #[test]
    fn test_radians_from_str() {
        use std::f32::consts::PI;

        // Bare numbers (radians)
        let r: Radians = "1.57".parse().unwrap();
        assert!((r.get() - 1.57).abs() < 0.01);

        // With "rad" suffix
        let r: Radians = "3.14rad".parse().unwrap();
        assert!((r.get() - 3.14).abs() < 0.01);

        // With "deg" suffix
        let r: Radians = "90deg".parse().unwrap();
        assert!((r.get() - PI / 2.0).abs() < 0.01);

        let r: Radians = "180deg".parse().unwrap();
        assert!((r.get() - PI).abs() < 0.01);

        // Whitespace
        assert!("  1.57  ".parse::<Radians>().is_ok());
        assert!("90 deg".parse::<Radians>().is_ok());

        // Invalid
        assert!("abc".parse::<Radians>().is_err());
        assert!("".parse::<Radians>().is_err());
    }

    // ========================================================================
    // NEW DX IMPROVEMENTS TESTS
    // ========================================================================

    #[test]
    fn test_pixels_to_device_pixels() {
        // Test conversion with scale factor
        let logical = px(100.0);
        assert_eq!(logical.to_device_pixels(2.0), device_px(200));
        assert_eq!(logical.to_device_pixels(1.5), device_px(150));
        assert_eq!(logical.to_device_pixels(1.0), device_px(100));

        // Test rounding
        let p = px(99.6);
        assert_eq!(p.to_device_pixels(1.0), device_px(100)); // Rounds up

        let p = px(99.4);
        assert_eq!(p.to_device_pixels(1.0), device_px(99)); // Rounds down
    }

    #[test]
    fn test_pixels_from_device_pixels() {
        let device = device_px(200);
        let logical = Pixels::from_device_pixels(device, 2.0);
        assert_eq!(logical.get(), 100.0);

        let logical = Pixels::from_device_pixels(device, 1.0);
        assert_eq!(logical.get(), 200.0);
    }

    #[test]
    fn test_pixels_map() {
        let value = px(100.0);
        let squared = value.map(|v| v * v);
        assert_eq!(squared.get(), 10000.0);

        let doubled = value.map(|v| v * 2.0);
        assert_eq!(doubled.get(), 200.0);
    }

    #[test]
    fn test_device_pixels_to_pixels() {
        let device = device_px(200);
        let logical = device.to_pixels(2.0);
        assert_eq!(logical.get(), 100.0);

        let logical = device.to_pixels(1.0);
        assert_eq!(logical.get(), 200.0);
    }

    #[test]
    fn test_device_pixels_to_scaled_pixels() {
        let device = device_px(200);
        let scaled = device.to_scaled_pixels();
        assert_eq!(scaled.get(), 200.0);
    }

    #[test]
    fn test_device_pixels_map() {
        let value = device_px(100);
        let doubled = value.map(|v| v * 2);
        assert_eq!(doubled.get(), 200);
    }

    #[test]
    fn test_scaled_pixels_to_pixels() {
        let scaled = scaled_px(200.0);
        let logical = scaled.to_pixels(2.0);
        assert_eq!(logical.get(), 100.0);
    }

    #[test]
    fn test_scaled_pixels_map() {
        let value = scaled_px(100.0);
        let squared = value.map(|v| v * v);
        assert_eq!(squared.get(), 10000.0);
    }

    #[test]
    fn test_pixels_sum_iterator() {
        let values = vec![px(10.0), px(20.0), px(30.0)];
        let total: Pixels = values.iter().sum();
        assert_eq!(total, px(60.0));

        let owned_total: Pixels = values.into_iter().sum();
        assert_eq!(owned_total, px(60.0));
    }

    #[test]
    fn test_device_pixels_sum_iterator() {
        let values = vec![device_px(10), device_px(20), device_px(30)];
        let total: DevicePixels = values.iter().sum();
        assert_eq!(total, device_px(60));

        let owned_total: DevicePixels = values.into_iter().sum();
        assert_eq!(owned_total, device_px(60));
    }

    #[test]
    fn test_scaled_pixels_sum_iterator() {
        let values = vec![scaled_px(10.0), scaled_px(20.0), scaled_px(30.0)];
        let total: ScaledPixels = values.iter().sum();
        assert_eq!(total, scaled_px(60.0));

        let owned_total: ScaledPixels = values.into_iter().sum();
        assert_eq!(owned_total, scaled_px(60.0));
    }

    #[test]
    fn test_pixels_mul_i32() {
        let p = px(100.0);
        assert_eq!(p * 3_i32, px(300.0));
        assert_eq!(3_i32 * p, px(300.0));
        assert_eq!(p * (-2_i32), px(-200.0));
    }

    #[test]
    fn test_pixels_const_min_max() {
        const A: Pixels = px(10.0);
        const B: Pixels = px(20.0);
        const MAX: Pixels = A.max_const(B);
        const MIN: Pixels = A.min_const(B);

        assert_eq!(MAX, px(20.0));
        assert_eq!(MIN, px(10.0));
    }

    #[test]
    fn test_scaled_pixels_rem() {
        let a = scaled_px(10.5);
        let b = scaled_px(3.0);
        let result = a % b;
        assert!((result.get() - 1.5).abs() < 0.0001);

        let mut c = scaled_px(10.5);
        c %= b;
        assert!((c.get() - 1.5).abs() < 0.0001);
    }

    #[test]
    fn test_radians_rem() {
        use std::f32::consts::PI;

        let a = radians(PI * 2.5);
        let b = radians(PI);
        let result = a % b;
        assert!((result.get() - PI * 0.5).abs() < 0.0001);

        let mut c = radians(PI * 2.5);
        c %= b;
        assert!((c.get() - PI * 0.5).abs() < 0.0001);
    }
}
