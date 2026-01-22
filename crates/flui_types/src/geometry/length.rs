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

/// Root em units for typography and scalable spacing.
///
/// `Rems` (root em) is a CSS-inspired unit that scales relative to the
/// root font size. This enables consistent, accessible scaling across
/// the entire UI.
///
/// # Conversion
///
/// Rems must be converted to [`Pixels`] using a rem size context:
///
/// ```rust
/// use flui_types::geometry::{rems, px};
///
/// let size = rems(1.5);
/// let rem_size = px(16.0);  // 1rem = 16px (typical default)
/// let pixels = size.to_pixels(rem_size);
/// assert_eq!(pixels, px(24.0));  // 1.5 * 16 = 24
/// ```
///
/// # Common rem sizes
///
/// - Desktop: 16px
/// - Mobile: 14-16px
/// - Accessibility (large text): 18-20px
#[derive(Clone, Copy, Default, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct Rems(pub f32);

/// Convenience function to create [`Rems`].
///
/// # Examples
///
/// ```rust
/// use flui_types::geometry::rems;
///
/// let heading = rems(2.0);    // 2rem
/// let body = rems(1.0);       // 1rem (base)
/// let small = rems(0.875);    // 0.875rem
/// ```
#[inline]
pub const fn rems(value: f32) -> Rems {
    Rems(value)
}

impl Rems {
    /// Zero rems.
    pub const ZERO: Rems = Rems(0.0);

    /// Creates a new `Rems` value.
    #[inline]
    pub const fn new(value: f32) -> Self {
        Self(value)
    }

    /// Returns the inner f32 value.
    #[inline]
    pub const fn get(self) -> f32 {
        self.0
    }

    /// Converts to [`Pixels`] using the given rem size.
    ///
    /// # Arguments
    ///
    /// * `rem_size` - The size of 1rem in pixels (typically 16.0)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::{rems, px};
    ///
    /// let size = rems(2.0);
    /// assert_eq!(size.to_pixels(px(16.0)), px(32.0));
    /// assert_eq!(size.to_pixels(px(20.0)), px(40.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn to_pixels(self, rem_size: Pixels) -> Pixels {
        px(self.0 * rem_size.get())
    }

    /// Checks if the value is zero.
    #[inline]
    pub fn is_zero(self) -> bool {
        self.0 == 0.0
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

impl From<f32> for Rems {
    #[inline]
    fn from(value: f32) -> Self {
        Self(value)
    }
}

impl From<Rems> for f32 {
    #[inline]
    fn from(rems: Rems) -> Self {
        rems.0
    }
}

// ============================================================================
// REMS - TRAIT IMPLEMENTATIONS
// ============================================================================

impl super::traits::Unit for Rems {
    type Scalar = f32;

    #[inline]
    fn zero() -> Self {
        Self::ZERO
    }
}

impl super::traits::NumericUnit for Rems {
    #[inline]
    fn add(self, other: Self) -> Self {
        Rems(self.0 + other.0)
    }

    #[inline]
    fn sub(self, other: Self) -> Self {
        Rems(self.0 - other.0)
    }

    #[inline]
    fn mul(self, scalar: f32) -> Self {
        Rems(self.0 * scalar)
    }

    #[inline]
    fn div(self, scalar: f32) -> Self {
        Rems(self.0 / scalar)
    }
}

// ============================================================================
// PERCENTAGE - Relative percentage values
// ============================================================================

/// Percentage value (0.0 to 1.0 typically, but can exceed).
///
/// Used for expressing values as fractions of a base value.
///
/// # Examples
///
/// ```rust
/// use flui_types::geometry::Percentage;
///
/// let half = Percentage(0.5);       // 50%
/// let full = Percentage(1.0);       // 100%
/// let one_fifty = Percentage(1.5);  // 150%
/// ```
#[derive(Clone, Copy, Default, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct Percentage(pub f32);

impl Percentage {
    /// Zero percent.
    pub const ZERO: Percentage = Percentage(0.0);

    /// One hundred percent.
    pub const FULL: Percentage = Percentage(1.0);

    /// Creates a new `Percentage`.
    #[inline]
    pub const fn new(value: f32) -> Self {
        Self(value)
    }

    /// Returns the inner f32 value.
    #[inline]
    pub const fn get(self) -> f32 {
        self.0
    }

    /// Applies the percentage to a base value.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::{Percentage, px};
    ///
    /// let half = Percentage(0.5);
    /// let base = px(200.0);
    /// assert_eq!(half.of(base), px(100.0));
    /// ```
    #[inline]
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

/// Absolute length specified in concrete units.
///
/// `AbsoluteLength` represents a length in either pixels or rems,
/// both of which can be converted to pixels given the appropriate context.
///
/// # Examples
///
/// ```rust
/// use flui_types::geometry::{AbsoluteLength, px, rems};
///
/// let pixel_length: AbsoluteLength = px(100.0).into();
/// let rem_length: AbsoluteLength = rems(2.0).into();
///
/// let rem_size = px(16.0);
/// assert_eq!(pixel_length.to_pixels(rem_size), px(100.0));
/// assert_eq!(rem_length.to_pixels(rem_size), px(32.0));
/// ```
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

    /// Converts to [`Pixels`] using the given rem size.
    ///
    /// # Arguments
    ///
    /// * `rem_size` - The size of 1rem in pixels
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::{AbsoluteLength, px, rems};
    ///
    /// let rem_size = px(16.0);
    ///
    /// let px_len: AbsoluteLength = px(100.0).into();
    /// assert_eq!(px_len.to_pixels(rem_size), px(100.0));
    ///
    /// let rem_len: AbsoluteLength = rems(2.0).into();
    /// assert_eq!(rem_len.to_pixels(rem_size), px(32.0));
    /// ```
    #[must_use]
    pub fn to_pixels(self, rem_size: Pixels) -> Pixels {
        match self {
            AbsoluteLength::Pixels(pixels) => pixels,
            AbsoluteLength::Rems(rems) => rems.to_pixels(rem_size),
        }
    }

    /// Converts to [`Rems`] using the given rem size.
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

/// Definite length that can be absolute or relative to a parent size.
///
/// `DefiniteLength` represents either:
/// - An absolute length ([`Pixels`] or [`Rems`])
/// - A fraction of a parent size (e.g., 0.5 = 50%)
///
/// # Examples
///
/// ```rust
/// use flui_types::geometry::{DefiniteLength, px, rems, relative};
///
/// let absolute: DefiniteLength = px(100.0).into();
/// let rem_based: DefiniteLength = rems(2.0).into();
/// let half: DefiniteLength = relative(0.5);
///
/// let rem_size = px(16.0);
/// let parent_size = px(200.0);
///
/// assert_eq!(absolute.to_pixels(parent_size, rem_size), px(100.0));
/// assert_eq!(rem_based.to_pixels(parent_size, rem_size), px(32.0));
/// assert_eq!(half.to_pixels(parent_size, rem_size), px(100.0));
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DefiniteLength {
    /// Absolute length (pixels or rems).
    Absolute(AbsoluteLength),
    /// Fraction of parent size (0.5 = 50%).
    Fraction(f32),
}

/// Constructs a fractional [`DefiniteLength`].
///
/// # Examples
///
/// ```rust
/// use flui_types::geometry::relative;
///
/// let half = relative(0.5);       // 50% of parent
/// let third = relative(0.333);    // 33.3% of parent
/// let double = relative(2.0);     // 200% of parent
/// ```
#[inline]
pub const fn relative(fraction: f32) -> DefiniteLength {
    DefiniteLength::Fraction(fraction)
}

impl DefiniteLength {
    /// Converts to [`Pixels`] given a parent size and rem size.
    ///
    /// # Arguments
    ///
    /// * `parent_size` - The parent's size in pixels (for fraction calculation)
    /// * `rem_size` - The size of 1rem in pixels (for rem conversion)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::{px, rems, relative, DefiniteLength, AbsoluteLength};
    ///
    /// let rem_size = px(16.0);
    /// let parent_size = px(200.0);
    ///
    /// // Absolute pixel
    /// let abs_px = DefiniteLength::Absolute(AbsoluteLength::Pixels(px(100.0)));
    /// assert_eq!(abs_px.to_pixels(parent_size, rem_size), px(100.0));
    ///
    /// // Absolute rem
    /// let abs_rem = DefiniteLength::Absolute(AbsoluteLength::Rems(rems(2.0)));
    /// assert_eq!(abs_rem.to_pixels(parent_size, rem_size), px(32.0));
    ///
    /// // Fractional
    /// let frac = DefiniteLength::Fraction(0.5);
    /// assert_eq!(frac.to_pixels(parent_size, rem_size), px(100.0));
    /// ```
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

/// CSS-like length that can be definite or automatic.
///
/// `Length` is the most flexible unit, supporting:
/// - Absolute units: pixels, rems
/// - Relative units: fractions (percentages)
/// - Automatic sizing: let layout determine the value
///
/// # Examples
///
/// ```rust
/// use flui_types::geometry::{Length, px, rems, relative, auto};
///
/// let fixed: Length = px(100.0).into();
/// let rem_based: Length = rems(2.0).into();
/// let half: Length = relative(0.5).into();
/// let flexible: Length = auto();
///
/// assert!(matches!(flexible, Length::Auto));
/// assert!(matches!(fixed, Length::Definite(_)));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Length {
    /// Definite length with concrete value.
    Definite(DefiniteLength),
    /// Automatic length determined by layout.
    #[default]
    Auto,
}

/// Constructs an automatic [`Length`].
///
/// # Examples
///
/// ```rust
/// use flui_types::geometry::{Length, auto};
///
/// let width: Length = auto();
/// assert!(matches!(width, Length::Auto));
/// ```
#[inline]
pub const fn auto() -> Length {
    Length::Auto
}

impl Length {
    /// Checks if this is an auto length.
    #[inline]
    pub fn is_auto(&self) -> bool {
        matches!(self, Length::Auto)
    }

    /// Checks if this is a definite length.
    #[inline]
    pub fn is_definite(&self) -> bool {
        matches!(self, Length::Definite(_))
    }

    /// Converts to pixels if definite, returns None if auto.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rems_conversion() {
        let size = rems(2.0);
        let rem_size = px(16.0);

        assert_eq!(size.to_pixels(rem_size), px(32.0));
        assert_eq!(rems(1.0).to_pixels(px(20.0)), px(20.0));
    }

    #[test]
    fn test_percentage() {
        let half = Percentage(0.5);
        assert_eq!(half.of(px(200.0)), px(100.0));

        let full = Percentage::FULL;
        assert_eq!(full.of(px(100.0)), px(100.0));
    }

    #[test]
    fn test_absolute_length() {
        let rem_size = px(16.0);

        let px_len = AbsoluteLength::Pixels(px(100.0));
        assert_eq!(px_len.to_pixels(rem_size), px(100.0));

        let rem_len = AbsoluteLength::Rems(rems(2.0));
        assert_eq!(rem_len.to_pixels(rem_size), px(32.0));
    }

    #[test]
    fn test_definite_length() {
        let rem_size = px(16.0);
        let parent_size = px(200.0);

        // Absolute pixel
        let abs_px = DefiniteLength::Absolute(AbsoluteLength::Pixels(px(100.0)));
        assert_eq!(abs_px.to_pixels(parent_size, rem_size), px(100.0));

        // Absolute rem
        let abs_rem = DefiniteLength::Absolute(AbsoluteLength::Rems(rems(2.0)));
        assert_eq!(abs_rem.to_pixels(parent_size, rem_size), px(32.0));

        // Fraction
        let frac = DefiniteLength::Fraction(0.5);
        assert_eq!(frac.to_pixels(parent_size, rem_size), px(100.0));
    }

    #[test]
    fn test_length() {
        let auto_len = Length::Auto;
        assert!(auto_len.is_auto());
        assert!(!auto_len.is_definite());

        let def_len = Length::Definite(DefiniteLength::Absolute(AbsoluteLength::Pixels(px(100.0))));
        assert!(!def_len.is_auto());
        assert!(def_len.is_definite());
    }

    #[test]
    fn test_constructors() {
        let _px_len: Length = px(100.0).into();
        let _rem_len: Length = rems(2.0).into();
        let _frac_len: Length = relative(0.5).into();
        let _auto_len: Length = auto();
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", rems(2.0)), "2rem");
        assert_eq!(format!("{}", Percentage(0.5)), "50%");
        assert_eq!(format!("{}", AbsoluteLength::Pixels(px(100.0))), "100px");
        assert_eq!(format!("{}", DefiniteLength::Fraction(0.5)), "50%");
        assert_eq!(format!("{}", Length::Auto), "auto");
    }

    #[test]
    fn test_absolute_length_from_str() {
        // Parse pixels with suffix
        let result: AbsoluteLength = "100px".parse().unwrap();
        assert_eq!(result, AbsoluteLength::Pixels(px(100.0)));

        // Parse pixels with whitespace
        let result: AbsoluteLength = "  100px  ".parse().unwrap();
        assert_eq!(result, AbsoluteLength::Pixels(px(100.0)));

        // Parse bare number as pixels
        let result: AbsoluteLength = "123.5".parse().unwrap();
        assert_eq!(result, AbsoluteLength::Pixels(px(123.5)));

        // Parse rems
        let result: AbsoluteLength = "2rem".parse().unwrap();
        assert_eq!(result, AbsoluteLength::Rems(rems(2.0)));

        // Parse rems with whitespace
        let result: AbsoluteLength = "  1.5rem  ".parse().unwrap();
        assert_eq!(result, AbsoluteLength::Rems(rems(1.5)));

        // Parse rems with whitespace before suffix
        let result: AbsoluteLength = "2 rem".parse().unwrap();
        assert_eq!(result, AbsoluteLength::Rems(rems(2.0)));
    }

    #[test]
    fn test_absolute_length_from_str_errors() {
        // Invalid number
        assert!("abc".parse::<AbsoluteLength>().is_err());

        // Invalid suffix
        assert!("100%".parse::<AbsoluteLength>().is_err());

        // Empty string
        assert!("".parse::<AbsoluteLength>().is_err());

        // Just suffix
        assert!("px".parse::<AbsoluteLength>().is_err());
        assert!("rem".parse::<AbsoluteLength>().is_err());
    }

    #[test]
    fn test_definite_length_from_str() {
        // Parse pixels
        let result: DefiniteLength = "100px".parse().unwrap();
        assert_eq!(
            result,
            DefiniteLength::Absolute(AbsoluteLength::Pixels(px(100.0)))
        );

        // Parse rems
        let result: DefiniteLength = "2rem".parse().unwrap();
        assert_eq!(
            result,
            DefiniteLength::Absolute(AbsoluteLength::Rems(rems(2.0)))
        );

        // Parse percentage
        let result: DefiniteLength = "50%".parse().unwrap();
        assert_eq!(result, DefiniteLength::Fraction(0.5));

        // Parse percentage with whitespace
        let result: DefiniteLength = "  75%  ".parse().unwrap();
        assert_eq!(result, DefiniteLength::Fraction(0.75));

        // Parse percentage with decimal
        let result: DefiniteLength = "33.33%".parse().unwrap();
        if let DefiniteLength::Fraction(f) = result {
            assert!((f - 0.3333).abs() < 0.0001);
        } else {
            panic!("Expected Fraction variant");
        }

        // Parse 100%
        let result: DefiniteLength = "100%".parse().unwrap();
        assert_eq!(result, DefiniteLength::Fraction(1.0));

        // Parse bare number as pixels
        let result: DefiniteLength = "123.5".parse().unwrap();
        assert_eq!(
            result,
            DefiniteLength::Absolute(AbsoluteLength::Pixels(px(123.5)))
        );
    }

    #[test]
    fn test_definite_length_from_str_errors() {
        // Invalid number
        assert!("abc".parse::<DefiniteLength>().is_err());

        // Invalid suffix
        assert!("100auto".parse::<DefiniteLength>().is_err());

        // Empty string
        assert!("".parse::<DefiniteLength>().is_err());

        // Just suffix
        assert!("%".parse::<DefiniteLength>().is_err());
    }

    #[test]
    fn test_length_from_str() {
        // Parse auto
        let result: Length = "auto".parse().unwrap();
        assert_eq!(result, Length::Auto);

        // Parse auto with different cases
        let result: Length = "AUTO".parse().unwrap();
        assert_eq!(result, Length::Auto);

        let result: Length = "Auto".parse().unwrap();
        assert_eq!(result, Length::Auto);

        // Parse auto with whitespace
        let result: Length = "  auto  ".parse().unwrap();
        assert_eq!(result, Length::Auto);

        // Parse pixels
        let result: Length = "100px".parse().unwrap();
        assert_eq!(
            result,
            Length::Definite(DefiniteLength::Absolute(AbsoluteLength::Pixels(px(100.0))))
        );

        // Parse rems
        let result: Length = "2rem".parse().unwrap();
        assert_eq!(
            result,
            Length::Definite(DefiniteLength::Absolute(AbsoluteLength::Rems(rems(2.0))))
        );

        // Parse percentage
        let result: Length = "50%".parse().unwrap();
        assert_eq!(result, Length::Definite(DefiniteLength::Fraction(0.5)));

        // Parse bare number as pixels
        let result: Length = "123.5".parse().unwrap();
        assert_eq!(
            result,
            Length::Definite(DefiniteLength::Absolute(AbsoluteLength::Pixels(px(123.5))))
        );
    }

    #[test]
    fn test_length_from_str_errors() {
        // Invalid number
        assert!("abc".parse::<Length>().is_err());

        // Invalid suffix
        assert!("100invalid".parse::<Length>().is_err());

        // Empty string
        assert!("".parse::<Length>().is_err());

        // Misspelled auto
        assert!("autto".parse::<Length>().is_err());
        assert!("autoo".parse::<Length>().is_err());
    }
}
