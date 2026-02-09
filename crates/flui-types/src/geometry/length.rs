//! Length units for flexible, CSS-like sizing and spacing.
//!
//! This module provides a hierarchy of length types inspired by GPUI and CSS,
//! enabling type-safe, ergonomic specification of dimensions with support for:
//!
//! - **Absolute units**: [`Pixels`], [`Rems`]
//! - **Relative units**: [`Percentage`], fractions
//! - **Automatic sizing**: [`Auto`](Length::Auto)
//!
//! # Type Hierarchy
//!
//! ```text
//! Length
//!   ├─ Definite
//!   │    ├─ Absolute
//!   │    │    ├─ Pixels
//!   │    │    └─ Rems
//!   │    └─ Fraction (percentage of parent)
//!   └─ Auto (layout-determined)
//! ```
//!
//! # Examples
//!
//! ```rust
//! use flui_types::geometry::{px, rems, relative, auto, Pixels};
//!
//! // Absolute lengths
//! let width = px(100.0);          // 100 pixels
//! let font_size = rems(1.5);      // 1.5 root em units
//!
//! // Relative length (50% of parent)
//! let half = relative(0.5);
//!
//! // Automatic sizing
//! let flexible = auto();
//!
//! // Convert to pixels with context
//! let rem_size = px(16.0);  // 1rem = 16px
//! let parent_width = px(200.0);
//!
//! let absolute_px = font_size.to_pixels(rem_size);        // 24px
//! let relative_px = half.to_pixels(parent_width, rem_size); // 100px
//! ```

use super::traits::IsZero;
use super::{px, ParseLengthError, Pixels};
use std::fmt::{self, Debug, Display};
use std::str::FromStr;

// ============================================================================
// REMS - Root em units for scalable typography
// ============================================================================

/// Root em units for font-relative sizing.
///
/// One rem equals the root font size. Commonly used for scalable typography.
#[derive(Copy, Clone, Default, PartialEq)]
#[repr(transparent)]
pub struct Rems(pub f32);

/// Creates a new Rems value.
#[inline]
pub const fn rems(value: f32) -> Rems {
    Rems(value)
}

impl Rems {
    /// Zero rems.
    pub const ZERO: Rems = Rems(0.0);

    /// Creates a new Rems value.
    #[inline]
    pub const fn new(value: f32) -> Self {
        Self(value)
    }

    /// Returns the raw f32 value.
    #[inline]
    pub const fn get(self) -> f32 {
        self.0
    }

    /// Converts to pixels using the given rem size.
    #[inline]
    #[must_use]
    pub fn to_pixels(self, rem_size: Pixels) -> Pixels {
        px(self.0 * rem_size.get())
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

    /// Clamps the value to the given range.
    #[inline]
    #[must_use]
    pub fn clamp(self, min: Self, max: Self) -> Self {
        Self(self.0.clamp(min.0, max.0))
    }

    /// Returns the sign of the value (-1, 0, or 1).
    #[inline]
    pub fn signum(self) -> f32 {
        self.0.signum()
    }

    /// Returns `true` if the value is finite (not NaN or infinite).
    #[inline]
    pub fn is_finite(self) -> bool {
        self.0.is_finite()
    }

    /// Returns the largest integer less than or equal to the value.
    #[inline]
    #[must_use]
    pub fn floor(self) -> Self {
        Self(self.0.floor())
    }

    /// Returns the smallest integer greater than or equal to the value.
    #[inline]
    #[must_use]
    pub fn ceil(self) -> Self {
        Self(self.0.ceil())
    }

    /// Returns the nearest integer to the value.
    #[inline]
    #[must_use]
    pub fn round(self) -> Self {
        Self(self.0.round())
    }

    /// Returns the integer part of the value.
    #[inline]
    #[must_use]
    pub fn trunc(self) -> Self {
        Self(self.0.trunc())
    }

    /// Scales the value by the given factor.
    #[inline]
    #[must_use]
    pub fn scale(self, factor: f32) -> Self {
        Self(self.0 * factor)
    }

    /// Applies a function to the underlying value.
    #[inline]
    #[must_use]
    pub fn map(self, f: impl FnOnce(f32) -> f32) -> Self {
        Self(f(self.0))
    }

    /// Returns `true` if the value is NaN.
    #[inline]
    pub fn is_nan(self) -> bool {
        self.0.is_nan()
    }

    /// Returns `true` if the value is infinite.
    #[inline]
    pub fn is_infinite(self) -> bool {
        self.0.is_infinite()
    }

    /// Linearly interpolates between this value and another.
    #[inline]
    #[must_use]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self(self.0 + (other.0 - self.0) * t)
    }
}

impl Display for Rems {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}rem", self.0)
    }
}

impl Debug for Rems {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(self, f)
    }
}

impl From<Rems> for f32 {
    #[inline]
    fn from(rems: Rems) -> Self {
        rems.0
    }
}

// Ordering (using total_cmp for proper NaN handling)
impl Eq for Rems {}

impl PartialOrd for Rems {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Rems {
    #[inline]
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

// Hashing (using to_bits for proper NaN handling)
impl std::hash::Hash for Rems {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

impl std::str::FromStr for Rems {
    type Err = ParseLengthError;

    /// Parses a `Rems` value from a string.
    ///
    /// Supported formats:
    /// - `"1.5"` - bare number
    /// - `"1.5rem"` - with "rem" suffix
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::Rems;
    ///
    /// let r: Rems = "1.5".parse().unwrap();
    /// assert_eq!(r.get(), 1.5);
    ///
    /// let r: Rems = "2rem".parse().unwrap();
    /// assert_eq!(r.get(), 2.0);
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let num_str = s.strip_suffix("rem").unwrap_or(s).trim();

        num_str
            .parse::<f32>()
            .map(Rems)
            .map_err(|_| ParseLengthError {
                input: s.to_string(),
                expected: "a number like '1.5' or '1.5rem'",
            })
    }
}

// ============================================================================
// REMS - TRAIT IMPLEMENTATIONS
// ============================================================================

impl super::traits::Unit for Rems {
    type Scalar = f32;

    #[inline]
    fn one() -> Self {
        Rems(1.0)
    }

    const MIN: Self = Rems(f32::MIN);
    const MAX: Self = Rems(f32::MAX);
}

impl super::traits::NumericUnit for Rems {
    #[inline]
    fn abs(self) -> Self {
        Rems(self.0.abs())
    }

    #[inline]
    fn min(self, other: Self) -> Self {
        Rems(self.0.min(other.0))
    }

    #[inline]
    fn max(self, other: Self) -> Self {
        Rems(self.0.max(other.0))
    }
}

// ============================================================================
// REMS - ARITHMETIC OPERATORS
// ============================================================================

use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

impl Add for Rems {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Rems(self.0 + rhs.0)
    }
}

impl AddAssign for Rems {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl Sub for Rems {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Rems(self.0 - rhs.0)
    }
}

impl SubAssign for Rems {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl Mul<f32> for Rems {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: f32) -> Self::Output {
        Rems(self.0 * rhs)
    }
}

impl Mul<Rems> for f32 {
    type Output = Rems;
    #[inline]
    fn mul(self, rhs: Rems) -> Self::Output {
        Rems(self * rhs.0)
    }
}

impl MulAssign<f32> for Rems {
    #[inline]
    fn mul_assign(&mut self, rhs: f32) {
        self.0 *= rhs;
    }
}

impl Div<f32> for Rems {
    type Output = Self;
    #[inline]
    fn div(self, rhs: f32) -> Self::Output {
        Rems(self.0 / rhs)
    }
}

impl Div for Rems {
    type Output = f32;
    #[inline]
    fn div(self, rhs: Self) -> Self::Output {
        self.0 / rhs.0
    }
}

impl DivAssign<f32> for Rems {
    #[inline]
    fn div_assign(&mut self, rhs: f32) {
        self.0 /= rhs;
    }
}

impl Neg for Rems {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self::Output {
        Rems(-self.0)
    }
}

// ============================================================================
// REMS - ADDITIONAL TRAIT IMPLEMENTATIONS
// ============================================================================

// Note: Half, Double, IsZero, Sign, ApproxEq, GeometryOps are implemented in traits.rs

impl std::iter::Sum for Rems {
    #[inline]
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Rems::ZERO, |acc, x| acc + x)
    }
}

impl<'a> std::iter::Sum<&'a Rems> for Rems {
    #[inline]
    fn sum<I: Iterator<Item = &'a Self>>(iter: I) -> Self {
        iter.fold(Rems::ZERO, |acc, x| acc + *x)
    }
}

// ============================================================================
// PERCENTAGE - Relative percentage values
// ============================================================================

/// Percentage value (0.0 = 0%, 1.0 = 100%).
#[derive(Copy, Clone, Default, PartialEq)]
#[repr(transparent)]
pub struct Percentage(pub f32);

impl Percentage {
    /// Zero percent.
    pub const ZERO: Percentage = Percentage(0.0);

    /// One hundred percent.
    pub const FULL: Percentage = Percentage(1.0);

    /// Creates a new percentage value.
    #[inline]
    pub const fn new(value: f32) -> Self {
        Self(value)
    }

    /// Returns the raw value (0.0 to 1.0).
    #[inline]
    pub const fn get(self) -> f32 {
        self.0
    }

    /// Applies this percentage to a base value.
    #[must_use]
    pub fn of(self, base: Pixels) -> Pixels {
        px(self.0 * base.get())
    }
}

impl Display for Percentage {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}%", self.0 * 100.0)
    }
}

impl Debug for Percentage {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(self, f)
    }
}

impl Eq for Percentage {}

impl PartialOrd for Percentage {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Percentage {
    #[inline]
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl std::hash::Hash for Percentage {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

// ============================================================================
// ABSOLUTE LENGTH - Pixels or Rems
// ============================================================================

/// An absolute length that can be either pixels or rems.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AbsoluteLength {
    /// Length in pixels.
    Pixels(Pixels),
    /// Length in root em units.
    Rems(Rems),
}

impl AbsoluteLength {
    /// Checks if the length is zero.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::{AbsoluteLength, px, rems};
    ///
    /// let zero_px: AbsoluteLength = px(0.0).into();
    /// let zero_rem: AbsoluteLength = rems(0.0).into();
    /// let nonzero: AbsoluteLength = px(10.0).into();
    ///
    /// assert!(zero_px.is_zero());
    /// assert!(zero_rem.is_zero());
    /// assert!(!nonzero.is_zero());
    /// ```
    pub fn is_zero(self) -> bool {
        match self {
            AbsoluteLength::Pixels(px) => px.is_zero(),
            AbsoluteLength::Rems(rems) => rems.is_zero(),
        }
    }

    /// Converts to pixels using the given rem size.
    #[must_use]
    pub fn to_pixels(self, rem_size: Pixels) -> Pixels {
        match self {
            AbsoluteLength::Pixels(pixels) => pixels,
            AbsoluteLength::Rems(rems) => rems.to_pixels(rem_size),
        }
    }

    /// Converts to rems using the given rem size.
    #[must_use]
    pub fn to_rems(self, rem_size: Pixels) -> Rems {
        match self {
            AbsoluteLength::Pixels(pixels) => rems(pixels.get() / rem_size.get()),
            AbsoluteLength::Rems(rems) => rems,
        }
    }
}

impl Default for AbsoluteLength {
    #[inline]
    fn default() -> Self {
        Self::Pixels(Pixels::ZERO)
    }
}

impl From<Pixels> for AbsoluteLength {
    #[inline]
    fn from(pixels: Pixels) -> Self {
        Self::Pixels(pixels)
    }
}

impl From<Rems> for AbsoluteLength {
    #[inline]
    fn from(rems: Rems) -> Self {
        Self::Rems(rems)
    }
}

impl Display for AbsoluteLength {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pixels(pixels) => write!(f, "{pixels}"),
            Self::Rems(rems) => write!(f, "{rems}"),
        }
    }
}

impl FromStr for AbsoluteLength {
    type Err = ParseLengthError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();

        // Try parsing as pixels first (e.g., "100px" or "100")
        if let Some(num_str) = s.strip_suffix("px") {
            let num = num_str
                .trim()
                .parse::<f32>()
                .map_err(|_| ParseLengthError {
                    input: s.to_string(),
                    expected: "valid number with 'px' suffix (e.g., '100px')",
                })?;
            return Ok(AbsoluteLength::Pixels(px(num)));
        }

        // Try parsing as rems (e.g., "2rem")
        if let Some(num_str) = s.strip_suffix("rem") {
            let num = num_str
                .trim()
                .parse::<f32>()
                .map_err(|_| ParseLengthError {
                    input: s.to_string(),
                    expected: "valid number with 'rem' suffix (e.g., '2rem')",
                })?;
            return Ok(AbsoluteLength::Rems(rems(num)));
        }

        // If no suffix, try parsing as bare number and default to pixels
        if let Ok(num) = s.parse::<f32>() {
            return Ok(AbsoluteLength::Pixels(px(num)));
        }

        Err(ParseLengthError {
            input: s.to_string(),
            expected: "absolute length (e.g., '100px', '2rem')",
        })
    }
}

// ============================================================================
// DEFINITE LENGTH - Absolute or Fractional
// ============================================================================

/// A definite length that can be either absolute or a fraction of parent size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DefiniteLength {
    /// Absolute length (pixels or rems).
    Absolute(AbsoluteLength),
    /// Fraction of parent size (0.5 = 50%).
    Fraction(f32),
}

/// Creates a fractional length relative to parent size.
#[inline]
pub const fn relative(fraction: f32) -> DefiniteLength {
    DefiniteLength::Fraction(fraction)
}

impl DefiniteLength {
    /// Converts to pixels using the given parent size and rem size.
    #[must_use]
    pub fn to_pixels(self, parent_size: Pixels, rem_size: Pixels) -> Pixels {
        match self {
            DefiniteLength::Absolute(abs) => abs.to_pixels(rem_size),
            DefiniteLength::Fraction(frac) => px(parent_size.get() * frac),
        }
    }

    /// Checks if the length is zero.
    pub fn is_zero(self) -> bool {
        match self {
            DefiniteLength::Absolute(abs) => abs.is_zero(),
            DefiniteLength::Fraction(frac) => frac == 0.0,
        }
    }
}

impl Default for DefiniteLength {
    #[inline]
    fn default() -> Self {
        Self::Absolute(AbsoluteLength::default())
    }
}

impl From<Pixels> for DefiniteLength {
    #[inline]
    fn from(pixels: Pixels) -> Self {
        Self::Absolute(AbsoluteLength::Pixels(pixels))
    }
}

impl From<Rems> for DefiniteLength {
    #[inline]
    fn from(rems: Rems) -> Self {
        Self::Absolute(AbsoluteLength::Rems(rems))
    }
}

impl From<AbsoluteLength> for DefiniteLength {
    #[inline]
    fn from(abs: AbsoluteLength) -> Self {
        Self::Absolute(abs)
    }
}

impl Display for DefiniteLength {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absolute(abs) => write!(f, "{abs}"),
            Self::Fraction(frac) => write!(f, "{}%", frac * 100.0),
        }
    }
}

impl FromStr for DefiniteLength {
    type Err = ParseLengthError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();

        // Try parsing as percentage first (e.g., "50%")
        if let Some(num_str) = s.strip_suffix('%') {
            let num = num_str
                .trim()
                .parse::<f32>()
                .map_err(|_| ParseLengthError {
                    input: s.to_string(),
                    expected: "valid number with '%' suffix (e.g., '50%')",
                })?;
            // Convert percentage to fraction (50% -> 0.5)
            return Ok(DefiniteLength::Fraction(num / 100.0));
        }

        // Try parsing as AbsoluteLength (pixels or rems)
        match s.parse::<AbsoluteLength>() {
            Ok(abs) => Ok(DefiniteLength::Absolute(abs)),
            Err(_) => Err(ParseLengthError {
                input: s.to_string(),
                expected: "definite length (e.g., '100px', '2rem', '50%')",
            }),
        }
    }
}

// ============================================================================
// LENGTH - Definite or Auto
// ============================================================================

/// A length that can be either definite or automatically determined.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Length {
    /// Definite length with concrete value.
    Definite(DefiniteLength),
    /// Automatically determined by layout.
    #[default]
    Auto,
}

/// Creates an automatic length.
#[inline]
pub const fn auto() -> Length {
    Length::Auto
}

impl Length {
    /// Returns `true` if the length is automatic.
    #[inline]
    pub fn is_auto(&self) -> bool {
        matches!(self, Length::Auto)
    }

    /// Returns `true` if the length is definite.
    #[inline]
    pub fn is_definite(&self) -> bool {
        matches!(self, Length::Definite(_))
    }

    /// Converts to pixels if definite, or returns `None` if automatic.
    #[must_use]
    pub fn to_pixels(self, parent_size: Pixels, rem_size: Pixels) -> Option<Pixels> {
        match self {
            Length::Definite(def) => Some(def.to_pixels(parent_size, rem_size)),
            Length::Auto => None,
        }
    }
}

impl From<Pixels> for Length {
    #[inline]
    fn from(pixels: Pixels) -> Self {
        Self::Definite(pixels.into())
    }
}

impl From<Rems> for Length {
    #[inline]
    fn from(rems: Rems) -> Self {
        Self::Definite(rems.into())
    }
}

impl From<AbsoluteLength> for Length {
    #[inline]
    fn from(abs: AbsoluteLength) -> Self {
        Self::Definite(abs.into())
    }
}

impl From<DefiniteLength> for Length {
    #[inline]
    fn from(def: DefiniteLength) -> Self {
        Self::Definite(def)
    }
}

impl Display for Length {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Definite(def) => write!(f, "{def}"),
            Self::Auto => write!(f, "auto"),
        }
    }
}

impl FromStr for Length {
    type Err = ParseLengthError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();

        // Check for "auto" first
        if s.eq_ignore_ascii_case("auto") {
            return Ok(Length::Auto);
        }

        // Try parsing as DefiniteLength
        match s.parse::<DefiniteLength>() {
            Ok(def) => Ok(Length::Definite(def)),
            Err(_) => Err(ParseLengthError {
                input: s.to_string(),
                expected: "length (e.g., '100px', '2rem', '50%', 'auto')",
            }),
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::px;

    // ---- Rems ----

    #[test]
    fn rems_construction() {
        let r = rems(1.5);
        assert_eq!(r.get(), 1.5);
        assert_eq!(Rems::new(2.0).get(), 2.0);
        assert_eq!(Rems::ZERO.get(), 0.0);
    }

    #[test]
    fn rems_to_pixels() {
        let rem_size = px(16.0);
        assert_eq!(rems(1.0).to_pixels(rem_size), px(16.0));
        assert_eq!(rems(1.5).to_pixels(rem_size), px(24.0));
        assert_eq!(rems(0.0).to_pixels(rem_size), px(0.0));
    }

    #[test]
    fn rems_arithmetic() {
        assert_eq!(rems(1.0) + rems(2.0), rems(3.0));
        assert_eq!(rems(3.0) - rems(1.0), rems(2.0));
        assert_eq!(rems(2.0) * 3.0, rems(6.0));
        assert_eq!(3.0 * rems(2.0), rems(6.0));
        assert_eq!(rems(6.0) / 2.0, rems(3.0));
        assert_eq!(rems(6.0) / rems(2.0), 3.0);
        assert_eq!(-rems(1.0), rems(-1.0));
    }

    #[test]
    fn rems_assign_arithmetic() {
        let mut r = rems(1.0);
        r += rems(2.0);
        assert_eq!(r, rems(3.0));
        r -= rems(1.0);
        assert_eq!(r, rems(2.0));
        r *= 3.0;
        assert_eq!(r, rems(6.0));
        r /= 2.0;
        assert_eq!(r, rems(3.0));
    }

    #[test]
    fn rems_math_operations() {
        assert_eq!(rems(-2.5).abs(), rems(2.5));
        assert_eq!(rems(1.0).min(rems(2.0)), rems(1.0));
        assert_eq!(rems(1.0).max(rems(2.0)), rems(2.0));
        assert_eq!(rems(5.0).clamp(rems(1.0), rems(3.0)), rems(3.0));
        assert_eq!(rems(1.7).floor(), rems(1.0));
        assert_eq!(rems(1.2).ceil(), rems(2.0));
        assert_eq!(rems(1.5).round(), rems(2.0));
        assert_eq!(rems(1.9).trunc(), rems(1.0));
    }

    #[test]
    fn rems_scale_map_lerp() {
        assert_eq!(rems(2.0).scale(3.0), rems(6.0));
        assert_eq!(rems(2.0).map(|v| v * v), rems(4.0));
        assert_eq!(rems(0.0).lerp(rems(10.0), 0.5), rems(5.0));
        assert_eq!(rems(0.0).lerp(rems(10.0), 0.0), rems(0.0));
        assert_eq!(rems(0.0).lerp(rems(10.0), 1.0), rems(10.0));
    }

    #[test]
    fn rems_validation() {
        assert!(rems(1.0).is_finite());
        assert!(!rems(f32::INFINITY).is_finite());
        assert!(rems(f32::NAN).is_nan());
        assert!(!rems(1.0).is_nan());
        assert!(rems(f32::INFINITY).is_infinite());
        assert!(!rems(1.0).is_infinite());
        assert_eq!(rems(2.5).signum(), 1.0);
        assert_eq!(rems(-2.5).signum(), -1.0);
    }

    #[test]
    fn rems_display() {
        assert_eq!(format!("{}", rems(1.5)), "1.5rem");
        assert_eq!(format!("{:?}", rems(1.5)), "1.5rem");
    }

    #[test]
    fn rems_ordering_and_hash() {
        assert!(rems(1.0) < rems(2.0));
        assert!(rems(2.0) > rems(1.0));
        assert_eq!(rems(1.0).cmp(&rems(1.0)), std::cmp::Ordering::Equal);

        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(rems(1.0));
        set.insert(rems(1.0));
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn rems_from_str() {
        assert_eq!("1.5".parse::<Rems>().unwrap(), rems(1.5));
        assert_eq!("2rem".parse::<Rems>().unwrap(), rems(2.0));
        assert_eq!("  3.0rem  ".parse::<Rems>().unwrap(), rems(3.0));
        assert!("abc".parse::<Rems>().is_err());
    }

    #[test]
    fn rems_sum() {
        let values = vec![rems(1.0), rems(2.0), rems(3.0)];
        let total: Rems = values.iter().sum();
        assert_eq!(total, rems(6.0));
        let total_owned: Rems = values.into_iter().sum();
        assert_eq!(total_owned, rems(6.0));
    }

    #[test]
    fn rems_is_zero_via_trait() {
        use crate::geometry::traits::IsZero;
        assert!(Rems::ZERO.is_zero());
        assert!(!rems(1.0).is_zero());
    }

    // ---- Percentage ----

    #[test]
    fn percentage_construction() {
        assert_eq!(Percentage::ZERO.get(), 0.0);
        assert_eq!(Percentage::FULL.get(), 1.0);
        assert_eq!(Percentage::new(0.5).get(), 0.5);
    }

    #[test]
    fn percentage_of() {
        assert_eq!(Percentage::new(0.5).of(px(200.0)), px(100.0));
        assert_eq!(Percentage::FULL.of(px(100.0)), px(100.0));
        assert_eq!(Percentage::ZERO.of(px(100.0)), px(0.0));
    }

    #[test]
    fn percentage_display() {
        assert_eq!(format!("{}", Percentage::new(0.5)), "50%");
        assert_eq!(format!("{}", Percentage::FULL), "100%");
    }

    #[test]
    fn percentage_copy_eq_hash_ord() {
        let a = Percentage::new(0.5);
        let b = a; // Copy
        assert_eq!(a, b);
        assert!(Percentage::new(0.3) < Percentage::new(0.7));

        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(a);
        set.insert(b);
        assert_eq!(set.len(), 1);
    }

    // ---- AbsoluteLength ----

    #[test]
    fn absolute_length_construction() {
        let px_len: AbsoluteLength = px(100.0).into();
        let rem_len: AbsoluteLength = rems(2.0).into();
        assert_eq!(px_len, AbsoluteLength::Pixels(px(100.0)));
        assert_eq!(rem_len, AbsoluteLength::Rems(rems(2.0)));
    }

    #[test]
    fn absolute_length_is_zero() {
        assert!(AbsoluteLength::Pixels(px(0.0)).is_zero());
        assert!(AbsoluteLength::Rems(Rems::ZERO).is_zero());
        assert!(!AbsoluteLength::Pixels(px(10.0)).is_zero());
    }

    #[test]
    fn absolute_length_to_pixels() {
        let rem_size = px(16.0);
        assert_eq!(
            AbsoluteLength::Pixels(px(100.0)).to_pixels(rem_size),
            px(100.0)
        );
        assert_eq!(
            AbsoluteLength::Rems(rems(2.0)).to_pixels(rem_size),
            px(32.0)
        );
    }

    #[test]
    fn absolute_length_to_rems() {
        let rem_size = px(16.0);
        assert_eq!(AbsoluteLength::Rems(rems(2.0)).to_rems(rem_size), rems(2.0));
        let converted = AbsoluteLength::Pixels(px(32.0)).to_rems(rem_size);
        assert_eq!(converted, rems(2.0));
    }

    #[test]
    fn absolute_length_display() {
        assert_eq!(format!("{}", AbsoluteLength::Pixels(px(100.0))), "100px");
        assert_eq!(format!("{}", AbsoluteLength::Rems(rems(2.0))), "2rem");
    }

    #[test]
    fn absolute_length_from_str() {
        assert_eq!(
            "100px".parse::<AbsoluteLength>().unwrap(),
            AbsoluteLength::Pixels(px(100.0))
        );
        assert_eq!(
            "2rem".parse::<AbsoluteLength>().unwrap(),
            AbsoluteLength::Rems(rems(2.0))
        );
        assert_eq!(
            "50".parse::<AbsoluteLength>().unwrap(),
            AbsoluteLength::Pixels(px(50.0))
        );
        assert!("abc".parse::<AbsoluteLength>().is_err());
    }

    #[test]
    fn absolute_length_default() {
        assert_eq!(
            AbsoluteLength::default(),
            AbsoluteLength::Pixels(Pixels::ZERO)
        );
    }

    // ---- DefiniteLength ----

    #[test]
    fn definite_length_construction() {
        let abs: DefiniteLength = px(100.0).into();
        let frac = relative(0.5);
        assert!(matches!(abs, DefiniteLength::Absolute(_)));
        assert!(matches!(frac, DefiniteLength::Fraction(_)));
    }

    #[test]
    fn definite_length_to_pixels() {
        let parent = px(200.0);
        let rem_size = px(16.0);
        assert_eq!(
            DefiniteLength::from(px(100.0)).to_pixels(parent, rem_size),
            px(100.0)
        );
        assert_eq!(
            DefiniteLength::from(rems(2.0)).to_pixels(parent, rem_size),
            px(32.0)
        );
        assert_eq!(relative(0.5).to_pixels(parent, rem_size), px(100.0));
    }

    #[test]
    fn definite_length_is_zero() {
        assert!(DefiniteLength::from(px(0.0)).is_zero());
        assert!(relative(0.0).is_zero());
        assert!(!DefiniteLength::from(px(10.0)).is_zero());
        assert!(!relative(0.5).is_zero());
    }

    #[test]
    fn definite_length_display() {
        assert_eq!(format!("{}", DefiniteLength::from(px(100.0))), "100px");
        assert_eq!(format!("{}", relative(0.5)), "50%");
    }

    #[test]
    fn definite_length_from_str() {
        assert_eq!(
            "100px".parse::<DefiniteLength>().unwrap(),
            DefiniteLength::from(px(100.0))
        );
        assert_eq!(
            "50%".parse::<DefiniteLength>().unwrap(),
            DefiniteLength::Fraction(0.5)
        );
        assert_eq!(
            "2rem".parse::<DefiniteLength>().unwrap(),
            DefiniteLength::Absolute(AbsoluteLength::Rems(rems(2.0)))
        );
        assert!("abc".parse::<DefiniteLength>().is_err());
    }

    // ---- Length ----

    #[test]
    fn length_construction() {
        assert!(auto().is_auto());
        assert!(!auto().is_definite());
        assert!(Length::from(px(100.0)).is_definite());
        assert!(!Length::from(px(100.0)).is_auto());
    }

    #[test]
    fn length_to_pixels() {
        let parent = px(200.0);
        let rem_size = px(16.0);
        assert_eq!(auto().to_pixels(parent, rem_size), None);
        assert_eq!(
            Length::from(px(100.0)).to_pixels(parent, rem_size),
            Some(px(100.0))
        );
    }

    #[test]
    fn length_display() {
        assert_eq!(format!("{}", auto()), "auto");
        assert_eq!(format!("{}", Length::from(px(100.0))), "100px");
    }

    #[test]
    fn length_from_str() {
        assert_eq!("auto".parse::<Length>().unwrap(), Length::Auto);
        assert_eq!("AUTO".parse::<Length>().unwrap(), Length::Auto);
        assert_eq!("100px".parse::<Length>().unwrap(), Length::from(px(100.0)));
        assert_eq!(
            "50%".parse::<Length>().unwrap(),
            Length::Definite(DefiniteLength::Fraction(0.5))
        );
        assert!("???".parse::<Length>().is_err());
    }

    #[test]
    fn length_default() {
        assert_eq!(Length::default(), Length::Auto);
    }
}
