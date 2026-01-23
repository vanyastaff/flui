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

use super::{px, ParseLengthError, Pixels};
use std::fmt::{self, Debug, Display};
use std::str::FromStr;

// ============================================================================
// REMS - Root em units for scalable typography
// ============================================================================

#[derive(Copy, Clone, Default, PartialEq)]
#[repr(transparent)]
pub struct Rems(pub f32);

#[inline]
pub const fn rems(value: f32) -> Rems {
    Rems(value)
}

impl Rems {
    /// Zero rems.
    pub const ZERO: Rems = Rems(0.0);

    #[inline]
    pub const fn new(value: f32) -> Self {
        Self(value)
    }

    #[inline]
    pub const fn get(self) -> f32 {
        self.0
    }

    #[must_use]
    pub fn to_pixels(self, rem_size: Pixels) -> Pixels {
        px(self.0 * rem_size.get())
    }

    #[inline]
    pub fn is_zero(self) -> bool {
        self.0.abs() < f32::EPSILON
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
    pub fn signum(self) -> f32 {
        self.0.signum()
    }

    #[inline]
    pub fn is_finite(self) -> bool {
        self.0.is_finite()
    }

    #[must_use]
    pub fn floor(self) -> Self {
        Self(self.0.floor())
    }

    #[must_use]
    pub fn ceil(self) -> Self {
        Self(self.0.ceil())
    }

    #[must_use]
    pub fn round(self) -> Self {
        Self(self.0.round())
    }

    #[must_use]
    pub fn trunc(self) -> Self {
        Self(self.0.trunc())
    }

    #[must_use]
    pub fn scale(self, factor: f32) -> Self {
        Self(self.0 * factor)
    }

    #[must_use]
    pub fn map(self, f: impl FnOnce(f32) -> f32) -> Self {
        Self(f(self.0))
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
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self(self.0 + (other.0 - self.0) * t)
    }
}

impl Display for Rems {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}rem", self.0)
    }
}

impl Debug for Rems {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(self, f)
    }
}

impl From<Pixels> for Rems {
    #[inline]
    fn from(value: Pixels) -> Self {
        Self(value.0)
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
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Rems {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

// Hashing (using to_bits for proper NaN handling)
impl std::hash::Hash for Rems {
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
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Rems::ZERO, |acc, x| acc + x)
    }
}

impl<'a> std::iter::Sum<&'a Rems> for Rems {
    fn sum<I: Iterator<Item = &'a Self>>(iter: I) -> Self {
        iter.fold(Rems::ZERO, |acc, x| acc + *x)
    }
}

// ============================================================================
// PERCENTAGE - Relative percentage values
// ============================================================================

#[repr(transparent)]
pub struct Percentage(pub f32);

impl Percentage {
    /// Zero percent.
    pub const ZERO: Percentage = Percentage(0.0);

    /// One hundred percent.
    pub const FULL: Percentage = Percentage(1.0);

    #[inline]
    pub const fn new(value: f32) -> Self {
        Self(value)
    }

    #[inline]
    pub const fn get(self) -> f32 {
        self.0
    }

    #[must_use]
    pub fn of(self, base: Pixels) -> Pixels {
        px(self.0 * base.get())
    }
}

impl Display for Percentage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}%", self.0 * 100.0)
    }
}

impl Debug for Percentage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(self, f)
    }
}

// ============================================================================
// ABSOLUTE LENGTH - Pixels or Rems
// ============================================================================

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
    pub fn is_zero(&self) -> bool {
        match self {
            AbsoluteLength::Pixels(px) => px.get() == 0.0,
            AbsoluteLength::Rems(rems) => rems.is_zero(),
        }
    }

    #[must_use]
    pub fn to_pixels(self, rem_size: Pixels) -> Pixels {
        match self {
            AbsoluteLength::Pixels(pixels) => pixels,
            AbsoluteLength::Rems(rems) => rems.to_pixels(rem_size),
        }
    }

    #[must_use]
    pub fn to_rems(self, rem_size: Pixels) -> Rems {
        match self {
            AbsoluteLength::Pixels(pixels) => rems(pixels.get() / rem_size.get()),
            AbsoluteLength::Rems(rems) => rems,
        }
    }
}

impl Default for AbsoluteLength {
    fn default() -> Self {
        Self::Pixels(Pixels::ZERO)
    }
}

impl From<Pixels> for AbsoluteLength {
    fn from(pixels: Pixels) -> Self {
        Self::Pixels(pixels)
    }
}

impl From<Rems> for AbsoluteLength {
    fn from(rems: Rems) -> Self {
        Self::Rems(rems)
    }
}

impl Display for AbsoluteLength {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pixels(pixels) => write!(f, "{}", pixels),
            Self::Rems(rems) => write!(f, "{}", rems),
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DefiniteLength {
    /// Absolute length (pixels or rems).
    Absolute(AbsoluteLength),
    /// Fraction of parent size (0.5 = 50%).
    Fraction(f32),
}

#[inline]
pub const fn relative(fraction: f32) -> DefiniteLength {
    DefiniteLength::Fraction(fraction)
}

impl DefiniteLength {
    #[must_use]
    pub fn to_pixels(self, parent_size: Pixels, rem_size: Pixels) -> Pixels {
        match self {
            DefiniteLength::Absolute(abs) => abs.to_pixels(rem_size),
            DefiniteLength::Fraction(frac) => px(parent_size.get() * frac),
        }
    }

    /// Checks if the length is zero.
    pub fn is_zero(&self) -> bool {
        match self {
            DefiniteLength::Absolute(abs) => abs.is_zero(),
            DefiniteLength::Fraction(frac) => *frac == 0.0,
        }
    }
}

impl Default for DefiniteLength {
    fn default() -> Self {
        Self::Absolute(AbsoluteLength::default())
    }
}

impl From<Pixels> for DefiniteLength {
    fn from(pixels: Pixels) -> Self {
        Self::Absolute(AbsoluteLength::Pixels(pixels))
    }
}

impl From<Rems> for DefiniteLength {
    fn from(rems: Rems) -> Self {
        Self::Absolute(AbsoluteLength::Rems(rems))
    }
}

impl From<AbsoluteLength> for DefiniteLength {
    fn from(abs: AbsoluteLength) -> Self {
        Self::Absolute(abs)
    }
}

impl Display for DefiniteLength {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absolute(abs) => write!(f, "{}", abs),
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

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Length {
    /// Definite length with concrete value.
    Definite(DefiniteLength),
    #[default]
    Auto,
}

#[inline]
pub const fn auto() -> Length {
    Length::Auto
}

impl Length {
    #[inline]
    pub fn is_auto(&self) -> bool {
        matches!(self, Length::Auto)
    }

    #[inline]
    pub fn is_definite(&self) -> bool {
        matches!(self, Length::Definite(_))
    }

    #[must_use]
    pub fn to_pixels(self, parent_size: Pixels, rem_size: Pixels) -> Option<Pixels> {
        match self {
            Length::Definite(def) => Some(def.to_pixels(parent_size, rem_size)),
            Length::Auto => None,
        }
    }
}

impl From<Pixels> for Length {
    fn from(pixels: Pixels) -> Self {
        Self::Definite(pixels.into())
    }
}

impl From<Rems> for Length {
    fn from(rems: Rems) -> Self {
        Self::Definite(rems.into())
    }
}

impl From<AbsoluteLength> for Length {
    fn from(abs: AbsoluteLength) -> Self {
        Self::Definite(abs.into())
    }
}

impl From<DefiniteLength> for Length {
    fn from(def: DefiniteLength) -> Self {
        Self::Definite(def)
    }
}

impl Display for Length {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Definite(def) => write!(f, "{}", def),
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
