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

#[derive(Copy, Clone, Default, PartialEq)]
#[repr(transparent)]
pub struct Pixels(pub f32);

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

    #[inline]
    pub const fn new(value: f32) -> Self {
        Self(value)
    }

    #[inline]
    pub const fn get(self) -> f32 {
        self.0
    }

    #[must_use]
    pub fn floor(self) -> Self {
        Self(self.0.floor())
    }

    #[must_use]
    pub fn round(self) -> Self {
        Self(self.0.round())
    }

    #[must_use]
    pub fn ceil(self) -> Self {
        Self(self.0.ceil())
    }

    #[must_use]
    pub fn trunc(self) -> Self {
        Self(self.0.trunc())
    }

    #[must_use]
    pub fn abs(self) -> Self {
        Self(self.0.abs())
    }

    #[must_use]
    pub fn sqrt(self) -> Self {
        Self(self.0.sqrt())
    }

    #[must_use]
    pub fn signum(self) -> Self {
        Self(self.0.signum())
    }

    #[inline]
    pub fn signum_raw(self) -> f32 {
        self.0.signum()
    }

    #[must_use]
    pub fn fract(self) -> Self {
        Self(self.0.fract())
    }

    #[must_use]
    pub fn atan2(self, other: Self) -> f32 {
        self.0.atan2(other.0)
    }

    #[must_use]
    pub fn pow(self, exponent: f32) -> Self {
        Self(self.0.powf(exponent))
    }

    #[must_use]
    pub fn scale(self, factor: f32) -> ScaledPixels {
        ScaledPixels(self.0 * factor)
    }

    #[inline]
    pub fn is_finite(self) -> bool {
        self.0.is_finite()
    }

    #[inline]
    pub fn is_nan(self) -> bool {
        self.0.is_nan()
    }

    #[inline]
    pub fn is_infinite(self) -> bool {
        self.0.is_infinite()
    }

    #[must_use]
    pub fn min(self, other: Self) -> Self {
        Self(self.0.min(other.0))
    }

    #[must_use]
    pub fn max(self, other: Self) -> Self {
        Self(self.0.max(other.0))
    }

    #[must_use]
    pub const fn max_const(self, other: Self) -> Self {
        if self.0 > other.0 {
            self
        } else {
            other
        }
    }

    #[must_use]
    pub const fn min_const(self, other: Self) -> Self {
        if self.0 < other.0 {
            self
        } else {
            other
        }
    }

    #[must_use]
    pub fn clamp(self, min: Self, max: Self) -> Self {
        Self(self.0.clamp(min.0, max.0))
    }

    #[inline]
    pub fn new_clamped(value: f32) -> Self {
        if value.is_finite() {
            Self(value.max(0.0))
        } else {
            Self::ZERO
        }
    }

    #[inline]
    pub fn try_new(value: f32) -> Option<Self> {
        if value.is_finite() && value >= 0.0 {
            Some(Self(value))
        } else {
            None
        }
    }

    #[inline]
    pub const fn from_i32(value: i32) -> Self {
        Self(value as f32)
    }

    #[must_use]
    pub fn to_device_pixels(self, scale_factor: f32) -> DevicePixels {
        DevicePixels((self.0 * scale_factor).round() as i32)
    }

    #[must_use]
    pub fn from_device_pixels(device: DevicePixels, scale_factor: f32) -> Self {
        Pixels(device.0 as f32 / scale_factor)
    }

    #[must_use]
    pub fn from_scaled_pixels(scaled: ScaledPixels, scale_factor: f32) -> Self {
        Pixels(scaled.0 / scale_factor)
    }

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

impl Mul<Pixels> for Pixels {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: Pixels) -> Self::Output {
        Self(self.0 * rhs.0)
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

impl Mul<f32> for Pixels {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: f32) -> Self::Output {
        Self(self.0 * rhs)
    }
}

impl MulAssign<Pixels> for Pixels {
    #[inline]
    fn mul_assign(&mut self, rhs: Pixels) {
        self.0 *= rhs.0;
    }
}

impl MulAssign<f32> for Pixels {
    #[inline]
    fn mul_assign(&mut self, rhs: f32) {
        self.0 *= rhs;
    }
}

impl Div<Pixels> for Pixels {
    type Output = f32;
    #[inline]
    fn div(self, rhs: Pixels) -> Self::Output {
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

impl DivAssign<Pixels> for Pixels {
    #[inline]
    fn div_assign(&mut self, rhs: Pixels) {
        self.0 /= rhs.0;
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
// PIXEL DELTA - Type-safe displacement/velocity in pixels
// ============================================================================

#[derive(Copy, Clone, Default, PartialEq)]
#[repr(transparent)]
pub struct PixelDelta(pub f32);

#[inline]
pub const fn delta_px(value: f32) -> PixelDelta {
    PixelDelta(value)
}

impl PixelDelta {
    /// Zero delta.
    pub const ZERO: PixelDelta = PixelDelta(0.0);

    /// Maximum representable delta value.
    pub const MAX: PixelDelta = PixelDelta(f32::MAX);

    /// Minimum representable delta value.
    pub const MIN: PixelDelta = PixelDelta(f32::MIN);

    #[inline]
    pub const fn new(value: f32) -> Self {
        Self(value)
    }

    #[inline]
    pub const fn get(self) -> f32 {
        self.0
    }

    #[must_use]
    pub fn floor(self) -> Self {
        Self(self.0.floor())
    }

    #[must_use]
    pub fn round(self) -> Self {
        Self(self.0.round())
    }

    #[must_use]
    pub fn ceil(self) -> Self {
        Self(self.0.ceil())
    }

    #[must_use]
    pub fn trunc(self) -> Self {
        Self(self.0.trunc())
    }

    #[must_use]
    pub fn abs(self) -> Self {
        Self(self.0.abs())
    }

    #[inline]
    pub fn signum_raw(self) -> f32 {
        self.0.signum()
    }

    #[inline]
    pub fn is_finite(self) -> bool {
        self.0.is_finite()
    }

    #[inline]
    pub fn is_nan(self) -> bool {
        self.0.is_nan()
    }

    #[inline]
    pub fn is_infinite(self) -> bool {
        self.0.is_infinite()
    }

    #[must_use]
    pub fn min(self, other: Self) -> Self {
        Self(self.0.min(other.0))
    }

    #[must_use]
    pub fn max(self, other: Self) -> Self {
        Self(self.0.max(other.0))
    }

    #[must_use]
    pub fn clamp(self, min: Self, max: Self) -> Self {
        Self(self.0.clamp(min.0, max.0))
    }

    #[must_use]
    pub fn map(self, f: impl FnOnce(f32) -> f32) -> Self {
        PixelDelta(f(self.0))
    }

    #[must_use]
    pub fn to_pixels(self) -> Pixels {
        Pixels(self.0)
    }
}

// ============================================================================
// PIXEL DELTA - TRAIT IMPLEMENTATIONS
// ============================================================================

impl Unit for PixelDelta {
    type Scalar = f32;

    #[inline]
    fn one() -> Self {
        PixelDelta(1.0)
    }

    const MIN: Self = PixelDelta::MIN;
    const MAX: Self = PixelDelta::MAX;
}

impl NumericUnit for PixelDelta {
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
// PIXEL DELTA - ARITHMETIC OPERATORS
// ============================================================================

impl Add for PixelDelta {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl AddAssign for PixelDelta {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl Sub for PixelDelta {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl SubAssign for PixelDelta {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl Mul<f32> for PixelDelta {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: f32) -> Self::Output {
        Self(self.0 * rhs)
    }
}

impl Mul<PixelDelta> for f32 {
    type Output = PixelDelta;
    #[inline]
    fn mul(self, rhs: PixelDelta) -> Self::Output {
        PixelDelta(self * rhs.0)
    }
}

impl MulAssign<f32> for PixelDelta {
    #[inline]
    fn mul_assign(&mut self, rhs: f32) {
        self.0 *= rhs;
    }
}

impl Div for PixelDelta {
    type Output = f32;
    #[inline]
    fn div(self, rhs: Self) -> Self::Output {
        self.0 / rhs.0
    }
}

impl Div<f32> for PixelDelta {
    type Output = Self;
    #[inline]
    fn div(self, rhs: f32) -> Self::Output {
        Self(self.0 / rhs)
    }
}

impl DivAssign<f32> for PixelDelta {
    #[inline]
    fn div_assign(&mut self, rhs: f32) {
        self.0 /= rhs;
    }
}

impl Rem for PixelDelta {
    type Output = Self;
    #[inline]
    fn rem(self, rhs: Self) -> Self::Output {
        Self(self.0 % rhs.0)
    }
}

impl RemAssign for PixelDelta {
    #[inline]
    fn rem_assign(&mut self, rhs: Self) {
        self.0 %= rhs.0;
    }
}

impl Neg for PixelDelta {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self::Output {
        Self(-self.0)
    }
}

// ============================================================================
// PIXEL DELTA - CONVERSIONS
// ============================================================================

impl From<Pixels> for PixelDelta {
    #[inline]
    fn from(value: Pixels) -> Self {
        Self(value.0)
    }
}

impl From<f64> for PixelDelta {
    #[inline]
    fn from(value: f64) -> Self {
        Self(value as f32)
    }
}

impl From<PixelDelta> for f32 {
    #[inline]
    fn from(delta: PixelDelta) -> Self {
        delta.0
    }
}

impl From<PixelDelta> for f64 {
    #[inline]
    fn from(delta: PixelDelta) -> Self {
        delta.0 as f64
    }
}

// ============================================================================
// PIXEL DELTA - FORMATTING
// ============================================================================

impl Display for PixelDelta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}Δpx", self.0)
    }
}

impl Debug for PixelDelta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(self, f)
    }
}

// ============================================================================
// PIXEL DELTA - ORDERING & HASHING
// ============================================================================

impl Eq for PixelDelta {}

impl PartialOrd for PixelDelta {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PixelDelta {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl std::hash::Hash for PixelDelta {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

// ============================================================================
// PIXEL DELTA - SUM TRAIT
// ============================================================================

impl Sum for PixelDelta {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(PixelDelta::ZERO, |acc, x| acc + x)
    }
}

impl<'a> Sum<&'a PixelDelta> for PixelDelta {
    fn sum<I: Iterator<Item = &'a Self>>(iter: I) -> Self {
        iter.fold(PixelDelta::ZERO, |acc, x| acc + *x)
    }
}

// ============================================================================
// DEVICE PIXELS - Physical display pixels
// ============================================================================

#[derive(Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct DevicePixels(pub i32);

#[inline]
pub const fn device_px(value: i32) -> DevicePixels {
    DevicePixels(value)
}

impl DevicePixels {
    /// Zero device pixels.
    pub const ZERO: DevicePixels = DevicePixels(0);

    #[inline]
    pub const fn new(value: i32) -> Self {
        Self(value)
    }

    #[inline]
    pub const fn get(self) -> i32 {
        self.0
    }

    #[inline]
    pub fn to_bytes(self, bytes_per_pixel: u8) -> u32 {
        (self.0 as u32) * (bytes_per_pixel as u32)
    }

    #[must_use]
    pub fn abs(self) -> Self {
        Self(self.0.abs())
    }

    #[must_use]
    pub fn min(self, other: Self) -> Self {
        Self(self.0.min(other.0))
    }

    #[must_use]
    pub fn max(self, other: Self) -> Self {
        Self(self.0.max(other.0))
    }

    #[must_use]
    pub fn clamp(self, min: Self, max: Self) -> Self {
        Self(self.0.clamp(min.0, max.0))
    }

    #[inline]
    pub fn signum(self) -> i32 {
        self.0.signum()
    }

    #[must_use]
    pub const fn max_const(self, other: Self) -> Self {
        if self.0 > other.0 {
            self
        } else {
            other
        }
    }

    #[must_use]
    pub const fn min_const(self, other: Self) -> Self {
        if self.0 < other.0 {
            self
        } else {
            other
        }
    }

    #[must_use]
    pub fn to_pixels(self, scale_factor: f32) -> Pixels {
        Pixels(self.0 as f32 / scale_factor)
    }

    #[must_use]
    pub fn to_scaled_pixels(self) -> ScaledPixels {
        ScaledPixels(self.0 as f32)
    }

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

impl Mul<f32> for DevicePixels {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: f32) -> Self::Output {
        Self((self.0 as f32 * rhs) as i32)
    }
}

impl Div<f32> for DevicePixels {
    type Output = Self;
    #[inline]
    fn div(self, rhs: f32) -> Self::Output {
        Self((self.0 as f32 / rhs) as i32)
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
impl Mul<Pixels> for DevicePixels {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: Pixels) -> Self::Output {
        DevicePixels((self.0 as f32 * rhs.0).round() as i32)
    }
}

// Implement Div<Pixels> for DevicePixels to satisfy NumericUnit trait bound
impl Div<Pixels> for DevicePixels {
    type Output = Self;

    #[inline]
    fn div(self, rhs: Pixels) -> Self::Output {
        DevicePixels((self.0 as f32 / rhs.0).round() as i32)
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

#[derive(Copy, Clone, Default, PartialEq)]
#[repr(transparent)]
pub struct ScaledPixels(pub f32);

#[inline]
pub const fn scaled_px(value: f32) -> ScaledPixels {
    ScaledPixels(value)
}

impl ScaledPixels {
    /// Zero scaled pixels.
    pub const ZERO: ScaledPixels = ScaledPixels(0.0);

    #[inline]
    pub const fn new(value: f32) -> Self {
        Self(value)
    }

    #[inline]
    pub const fn get(self) -> f32 {
        self.0
    }

    #[must_use]
    pub fn floor(self) -> Self {
        Self(self.0.floor())
    }

    #[must_use]
    pub fn round(self) -> Self {
        Self(self.0.round())
    }

    #[must_use]
    pub fn ceil(self) -> Self {
        Self(self.0.ceil())
    }

    #[must_use]
    pub fn trunc(self) -> Self {
        Self(self.0.trunc())
    }

    #[must_use]
    pub fn abs(self) -> Self {
        Self(self.0.abs())
    }

    #[must_use]
    pub fn to_device_pixels(self) -> DevicePixels {
        DevicePixels(self.0.round() as i32)
    }

    #[inline]
    pub fn is_finite(self) -> bool {
        self.0.is_finite()
    }

    #[must_use]
    pub fn min(self, other: Self) -> Self {
        Self(self.0.min(other.0))
    }

    #[must_use]
    pub fn max(self, other: Self) -> Self {
        Self(self.0.max(other.0))
    }

    #[must_use]
    pub fn clamp(self, min: Self, max: Self) -> Self {
        Self(self.0.clamp(min.0, max.0))
    }

    #[inline]
    pub fn signum(self) -> f32 {
        self.0.signum()
    }

    #[must_use]
    pub fn to_pixels(self, scale_factor: f32) -> Pixels {
        Pixels(self.0 / scale_factor)
    }

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

impl Div<Pixels> for ScaledPixels {
    type Output = Self;
    #[inline]
    fn div(self, rhs: Pixels) -> Self::Output {
        Self(self.0 / rhs.0)
    }
}

impl DivAssign<Pixels> for ScaledPixels {
    #[inline]
    fn div_assign(&mut self, rhs: Pixels) {
        self.0 /= rhs.0;
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
impl From<Pixels> for ScaledPixels {
    #[inline]
    fn from(value: Pixels) -> Self {
        Self(value.0)
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

#[derive(Copy, Clone, Default, PartialEq)]
#[repr(transparent)]
pub struct Radians(pub f32);

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

    #[inline]
    pub const fn new(value: f32) -> Self {
        Self(value)
    }

    #[inline]
    pub const fn get(self) -> f32 {
        self.0
    }

    #[inline]
    pub fn from_degrees(degrees: f32) -> Self {
        Self(degrees.to_radians())
    }

    #[inline]
    pub fn to_degrees(self) -> f32 {
        self.0.to_degrees()
    }

    #[inline]
    pub fn normalize(self) -> Self {
        let tau = std::f32::consts::TAU;
        Self(self.0.rem_euclid(tau))
    }

    #[inline]
    pub fn is_zero(self) -> bool {
        self.0.abs() < f32::EPSILON
    }

    #[inline]
    pub fn is_finite(self) -> bool {
        self.0.is_finite()
    }

    #[must_use]
    pub fn abs(self) -> Self {
        Self(self.0.abs())
    }

    #[inline]
    pub fn signum(self) -> f32 {
        self.0.signum()
    }

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
impl From<Pixels> for Radians {
    #[inline]
    fn from(value: Pixels) -> Self {
        Self(value.0)
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

impl Mul<Pixels> for Radians {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: Pixels) -> Self::Output {
        Self(self.0 * rhs.0)
    }
}

impl MulAssign<Pixels> for Radians {
    #[inline]
    fn mul_assign(&mut self, rhs: Pixels) {
        self.0 *= rhs.0;
    }
}

impl Div<Pixels> for Radians {
    type Output = Self;

    #[inline]
    fn div(self, rhs: Pixels) -> Self::Output {
        Self(self.0 / rhs.0)
    }
}

impl DivAssign<Pixels> for Radians {
    #[inline]
    fn div_assign(&mut self, rhs: Pixels) {
        self.0 /= rhs.0;
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
