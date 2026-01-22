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

    /// Returns the sign of the value (-1.0, 0.0, or 1.0).
    #[inline]
    pub fn signum(self) -> f32 {
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

    /// Clamps the value between min and max.
    #[inline]
    #[must_use]
    pub fn clamp(self, min: Self, max: Self) -> Self {
        Self(self.0.clamp(min.0, max.0))
    }
}

// ============================================================================
// TRAIT IMPLEMENTATIONS - Unit and NumericUnit
// ============================================================================

impl Unit for Pixels {
    type Scalar = f32;

    #[inline]
    fn zero() -> Self {
        Self::ZERO
    }
}

impl NumericUnit for Pixels {
    #[inline]
    fn add(self, other: Self) -> Self {
        Pixels(self.0 + other.0)
    }

    #[inline]
    fn sub(self, other: Self) -> Self {
        Pixels(self.0 - other.0)
    }

    #[inline]
    fn mul(self, scalar: f32) -> Self {
        Pixels(self.0 * scalar)
    }

    #[inline]
    fn div(self, scalar: f32) -> Self {
        Pixels(self.0 / scalar)
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
}

// Arithmetic operators for DevicePixels
impl Add for DevicePixels {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl AddAssign for DevicePixels {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl Sub for DevicePixels {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl SubAssign for DevicePixels {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
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
        Self(self.0 * rhs)
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
    fn zero() -> Self {
        Self::ZERO
    }
}

impl NumericUnit for DevicePixels {
    #[inline]
    fn add(self, other: Self) -> Self {
        DevicePixels(self.0.saturating_add(other.0))
    }

    #[inline]
    fn sub(self, other: Self) -> Self {
        DevicePixels(self.0.saturating_sub(other.0))
    }

    #[inline]
    fn mul(self, scalar: f32) -> Self {
        DevicePixels((self.0 as f32 * scalar).round() as i32)
    }

    #[inline]
    fn div(self, scalar: f32) -> Self {
        DevicePixels((self.0 as f32 / scalar).round() as i32)
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
#[derive(Clone, Copy, Default, PartialEq, PartialOrd)]
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

// ============================================================================
// SCALED PIXELS - TRAIT IMPLEMENTATIONS
// ============================================================================

impl Unit for ScaledPixels {
    type Scalar = f32;

    #[inline]
    fn zero() -> Self {
        Self::ZERO
    }
}

impl NumericUnit for ScaledPixels {
    #[inline]
    fn add(self, other: Self) -> Self {
        ScaledPixels(self.0 + other.0)
    }

    #[inline]
    fn sub(self, other: Self) -> Self {
        ScaledPixels(self.0 - other.0)
    }

    #[inline]
    fn mul(self, scalar: f32) -> Self {
        ScaledPixels(self.0 * scalar)
    }

    #[inline]
    fn div(self, scalar: f32) -> Self {
        ScaledPixels(self.0 / scalar)
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

        // Try parsing with "px" suffix
        if let Some(num_str) = s.strip_suffix("px") {
            num_str
                .trim()
                .parse::<f32>()
                .map(Pixels)
                .map_err(|_| ParseLengthError {
                    input: s.to_string(),
                    expected: "a number like '100' or '100px'",
                })
        } else {
            // Try parsing as bare number
            s.parse::<f32>().map(Pixels).map_err(|_| ParseLengthError {
                input: s.to_string(),
                expected: "a number like '100' or '100px'",
            })
        }
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
/// assert_eq!(right_angle.0, PI / 2.0);
///
/// // Convert to degrees
/// assert_eq!(angle.to_degrees(), 90.0);
///
/// // Normalize to [0, 2π)
/// let normalized = radians(PI * 3.0).normalize();
/// assert_eq!(normalized.0, PI);
/// ```
#[derive(Clone, Copy, Default, PartialEq, PartialOrd)]
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
    /// assert_eq!(angle.0, PI);
    ///
    /// // -π normalized to π
    /// let angle = radians(-PI).normalize();
    /// assert_eq!(angle.0, PI);
    /// ```
    #[inline]
    pub fn normalize(self) -> Self {
        let tau = std::f32::consts::TAU;
        Self(self.0.rem_euclid(tau))
    }

    /// Checks if the angle is zero.
    #[inline]
    pub fn is_zero(self) -> bool {
        self.0 == 0.0
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
        use super::super::traits::{NumericUnit, Unit};

        // Test Unit trait
        let zero = Pixels::zero();
        assert_eq!(zero.0, 0.0);
        assert_eq!(zero, Pixels::ZERO);

        // Test NumericUnit trait methods (use fully qualified syntax to avoid ambiguity)
        let a = px(10.0);
        let b = px(20.0);

        assert_eq!(NumericUnit::add(a, b), px(30.0));
        assert_eq!(NumericUnit::sub(a, b), px(-10.0));
        assert_eq!(NumericUnit::mul(a, 2.0), px(20.0));
        assert_eq!(NumericUnit::div(a, 2.0), px(5.0));
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
        use super::super::traits::{NumericUnit, Unit};

        // Test Unit trait
        let zero = DevicePixels::zero();
        assert_eq!(zero.0, 0);
        assert_eq!(zero, DevicePixels::ZERO);

        // Test NumericUnit trait methods with saturating arithmetic
        let a = device_px(10);
        let b = device_px(20);

        assert_eq!(NumericUnit::add(a, b).0, 30);
        assert_eq!(NumericUnit::sub(a, b).0, -10);
        assert_eq!(NumericUnit::mul(a, 2.0).0, 20);
        assert_eq!(NumericUnit::div(a, 2.0).0, 5);

        // Test saturating behavior
        let max_val = device_px(i32::MAX);
        let one = device_px(1);
        assert_eq!(NumericUnit::add(max_val, one).0, i32::MAX); // Saturates at MAX

        let min_val = device_px(i32::MIN);
        assert_eq!(NumericUnit::sub(min_val, one).0, i32::MIN); // Saturates at MIN

        // Test rounding in multiplication/division
        let val = device_px(10);
        assert_eq!(NumericUnit::mul(val, 1.4).0, 14); // 14.0 rounds to 14
        assert_eq!(NumericUnit::mul(val, 1.6).0, 16); // 16.0 rounds to 16
        assert_eq!(NumericUnit::div(val, 3.0).0, 3); // 3.333... rounds to 3
    }

    #[test]
    fn test_scaled_pixels_unit() {
        use super::super::traits::{NumericUnit, Unit};

        // Test Unit trait
        let zero = ScaledPixels::zero();
        assert_eq!(zero.0, 0.0);
        assert_eq!(zero, ScaledPixels::ZERO);

        // Test NumericUnit trait methods
        let a = scaled_px(10.0);
        let b = scaled_px(20.0);

        assert_eq!(NumericUnit::add(a, b).0, 30.0);
        assert_eq!(NumericUnit::sub(a, b).0, -10.0);
        assert_eq!(NumericUnit::mul(a, 2.0).0, 20.0);
        assert_eq!(NumericUnit::div(a, 2.0).0, 5.0);

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
        use super::super::traits::{NumericUnit, Unit};
        use crate::geometry::rems;

        // Test Unit trait
        let zero = <crate::geometry::Rems as Unit>::zero();
        assert_eq!(zero.0, 0.0);

        // Test NumericUnit trait methods
        let a = rems(1.0);
        let b = rems(0.5);

        assert_eq!(NumericUnit::add(a, b).0, 1.5);
        assert_eq!(NumericUnit::sub(a, b).0, 0.5);
        assert_eq!(NumericUnit::mul(a, 2.0).0, 2.0);
        assert_eq!(NumericUnit::div(a, 2.0).0, 0.5);

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
}
