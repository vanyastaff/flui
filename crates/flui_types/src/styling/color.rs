//! Color types and utilities for Flui.
//!
//! This module provides a comprehensive Color type with conversions between
//! different color spaces (RGB, HSL, HSV), similar to Flutter's Color system.

/// A color in the RGBA color space.
///
/// Colors are represented using 8-bit channels (0-255) for red, green, blue, and alpha.
///
/// # Examples
///
/// ```
/// use flui_types::Color;
///
/// // Create from RGB
/// let red = Color::rgb(255, 0, 0);
///
/// // Create from RGBA with transparency
/// let semi_transparent_blue = Color::rgba(0, 0, 255, 128);
///
/// // Create from hex string
/// let green = Color::from_hex("#00FF00").unwrap();
///
/// // Adjust opacity
/// let faded = red.with_opacity(0.5);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Color {
    /// Red channel (0-255)
    pub r: u8,
    /// Green channel (0-255)
    pub g: u8,
    /// Blue channel (0-255)
    pub b: u8,
    /// Alpha channel (0-255, 0 = transparent, 255 = opaque)
    pub a: u8,
}

impl Color {
    // ===== Constructors =====

    /// Creates a color from RGBA values (0-255).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color;
    ///
    /// let red = Color::rgba(255, 0, 0, 255);
    /// let semi_transparent = Color::rgba(100, 200, 150, 128);
    /// ```
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Creates a fully opaque color from RGB values (0-255).
    ///
    /// Equivalent to `Color::rgba(r, g, b, 255)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color;
    ///
    /// let blue = Color::rgb(0, 0, 255);
    /// assert!(blue.is_opaque());
    /// ```
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 255)
    }

    /// Creates a color from a 32-bit ARGB value.
    ///
    /// Format: 0xAARRGGBB (alpha, red, green, blue)
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color;
    ///
    /// let red = Color::from_argb(0xFFFF0000);
    /// assert_eq!(red, Color::rgb(255, 0, 0));
    /// ```
    pub const fn from_argb(argb: u32) -> Self {
        let a = ((argb >> 24) & 0xFF) as u8;
        let r = ((argb >> 16) & 0xFF) as u8;
        let g = ((argb >> 8) & 0xFF) as u8;
        let b = (argb & 0xFF) as u8;
        Self::rgba(r, g, b, a)
    }

    /// Creates a color from a hex string.
    ///
    /// Supports formats: "#RRGGBB", "RRGGBB", "#AARRGGBB", "AARRGGBB"
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color;
    ///
    /// let red = Color::from_hex("#FF0000").unwrap();
    /// let blue = Color::from_hex("0000FF").unwrap();
    /// let semi_transparent = Color::from_hex("#80FF0000").unwrap();
    ///
    /// assert!(Color::from_hex("invalid").is_err());
    /// ```
    pub fn from_hex(hex: &str) -> Result<Self, ParseColorError> {
        let hex = hex.trim_start_matches('#');

        match hex.len() {
            6 => {
                let rgb = u32::from_str_radix(hex, 16)
                    .map_err(|_| ParseColorError::InvalidHex)?;
                Ok(Self::from_argb((0xFF << 24) | rgb))
            }
            8 => {
                let argb = u32::from_str_radix(hex, 16)
                    .map_err(|_| ParseColorError::InvalidHex)?;
                Ok(Self::from_argb(argb))
            }
            _ => Err(ParseColorError::InvalidLength),
        }
    }

    // ===== Component accessors =====

    /// Gets the red component (0-255).
    pub const fn red(&self) -> u8 {
        self.r
    }

    /// Gets the green component (0-255).
    pub const fn green(&self) -> u8 {
        self.g
    }

    /// Gets the blue component (0-255).
    pub const fn blue(&self) -> u8 {
        self.b
    }

    /// Gets the alpha component (0-255).
    pub const fn alpha(&self) -> u8 {
        self.a
    }

    /// Gets the opacity as a float (0.0-1.0).
    ///
    /// This is alpha / 255.0
    pub fn opacity(&self) -> f32 {
        self.a as f32 / 255.0
    }

    // ===== Modifiers =====

    /// Returns a new color with the specified alpha value (0-255).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color;
    ///
    /// let opaque_red = Color::rgb(255, 0, 0);
    /// let transparent_red = opaque_red.with_alpha(128);
    ///
    /// assert_eq!(transparent_red.alpha(), 128);
    /// ```
    pub const fn with_alpha(&self, alpha: u8) -> Self {
        Self::rgba(self.r, self.g, self.b, alpha)
    }

    /// Returns a new color with the specified opacity (0.0-1.0).
    ///
    /// Values are clamped to the valid range.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color;
    ///
    /// let opaque = Color::rgb(255, 0, 0);
    /// let half = opaque.with_opacity(0.5);
    ///
    /// assert_eq!(half.alpha(), 127); // 0.5 * 255
    /// ```
    pub fn with_opacity(&self, opacity: f32) -> Self {
        let alpha = (opacity.clamp(0.0, 1.0) * 255.0) as u8;
        self.with_alpha(alpha)
    }

    /// Returns a new color with the specified red component.
    pub const fn with_red(&self, red: u8) -> Self {
        Self::rgba(red, self.g, self.b, self.a)
    }

    /// Returns a new color with the specified green component.
    pub const fn with_green(&self, green: u8) -> Self {
        Self::rgba(self.r, green, self.b, self.a)
    }

    /// Returns a new color with the specified blue component.
    pub const fn with_blue(&self, blue: u8) -> Self {
        Self::rgba(self.r, self.g, blue, self.a)
    }

    // ===== Checks =====

    /// Returns true if this color is fully transparent (alpha = 0).
    pub const fn is_transparent(&self) -> bool {
        self.a == 0
    }

    /// Returns true if this color is fully opaque (alpha = 255).
    pub const fn is_opaque(&self) -> bool {
        self.a == 255
    }

    // ===== Operations =====

    /// Linear interpolation between two colors.
    ///
    /// When `t` = 0.0, returns `self`. When `t` = 1.0, returns `other`.
    /// Values are clamped to [0.0, 1.0].
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color;
    ///
    /// let red = Color::rgb(255, 0, 0);
    /// let blue = Color::rgb(0, 0, 255);
    ///
    /// let purple = Color::lerp(red, blue, 0.5);
    /// assert!(purple.red() > 0 && purple.blue() > 0);
    /// ```
    pub fn lerp(a: Color, b: Color, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        let lerp_u8 = |a: u8, b: u8| {
            (a as f32 + (b as f32 - a as f32) * t) as u8
        };

        Color::rgba(
            lerp_u8(a.r, b.r),
            lerp_u8(a.g, b.g),
            lerp_u8(a.b, b.b),
            lerp_u8(a.a, b.a),
        )
    }

    /// Converts to a 32-bit ARGB value (0xAARRGGBB).
    pub const fn to_argb(&self) -> u32 {
        ((self.a as u32) << 24)
            | ((self.r as u32) << 16)
            | ((self.g as u32) << 8)
            | (self.b as u32)
    }

    /// Converts to a hex string (format: "#AARRGGBB" or "#RRGGBB" if fully opaque).
    pub fn to_hex(&self) -> String {
        if self.is_opaque() {
            format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", self.a, self.r, self.g, self.b)
        }
    }

    /// Converts to RGBA f32 tuple (0.0-1.0 range).
    pub fn to_rgba_f32(&self) -> (f32, f32, f32, f32) {
        (
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            self.a as f32 / 255.0,
        )
    }

    // ===== Common color constants =====

    /// Fully transparent (alpha = 0).
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);

    /// Black (0, 0, 0).
    pub const BLACK: Color = Color::rgb(0, 0, 0);

    /// White (255, 255, 255).
    pub const WHITE: Color = Color::rgb(255, 255, 255);

    /// Red (255, 0, 0).
    pub const RED: Color = Color::rgb(255, 0, 0);

    /// Green (0, 255, 0).
    pub const GREEN: Color = Color::rgb(0, 255, 0);

    /// Blue (0, 0, 255).
    pub const BLUE: Color = Color::rgb(0, 0, 255);

    /// Yellow (255, 255, 0).
    pub const YELLOW: Color = Color::rgb(255, 255, 0);

    /// Cyan (0, 255, 255).
    pub const CYAN: Color = Color::rgb(0, 255, 255);

    /// Magenta (255, 0, 255).
    pub const MAGENTA: Color = Color::rgb(255, 0, 255);

    /// Gray (128, 128, 128).
    pub const GRAY: Color = Color::rgb(128, 128, 128);

    /// Light gray (192, 192, 192).
    pub const LIGHT_GRAY: Color = Color::rgb(192, 192, 192);

    /// Dark gray (64, 64, 64).
    pub const DARK_GRAY: Color = Color::rgb(64, 64, 64);

    // Material Design colors will be added in future commits
}

impl Default for Color {
    fn default() -> Self {
        Color::TRANSPARENT
    }
}

// ===== Conversions =====

impl From<(u8, u8, u8)> for Color {
    fn from((r, g, b): (u8, u8, u8)) -> Self {
        Color::rgb(r, g, b)
    }
}

impl From<(u8, u8, u8, u8)> for Color {
    fn from((r, g, b, a): (u8, u8, u8, u8)) -> Self {
        Color::rgba(r, g, b, a)
    }
}

impl From<[u8; 3]> for Color {
    fn from([r, g, b]: [u8; 3]) -> Self {
        Color::rgb(r, g, b)
    }
}

impl From<[u8; 4]> for Color {
    fn from([r, g, b, a]: [u8; 4]) -> Self {
        Color::rgba(r, g, b, a)
    }
}

// ===== Error types =====

/// Error type for color parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseColorError {
    /// Invalid hex string format
    InvalidHex,
    /// Invalid string length (must be 6 or 8 characters)
    InvalidLength,
}

impl std::fmt::Display for ParseColorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseColorError::InvalidHex => write!(f, "Invalid hex color format"),
            ParseColorError::InvalidLength => {
                write!(f, "Invalid hex color length (expected 6 or 8 characters)")
            }
        }
    }
}

impl std::error::Error for ParseColorError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_creation() {
        let red = Color::rgb(255, 0, 0);
        assert_eq!(red.red(), 255);
        assert_eq!(red.green(), 0);
        assert_eq!(red.blue(), 0);
        assert_eq!(red.alpha(), 255);

        let with_alpha = Color::rgba(0, 255, 0, 128);
        assert_eq!(with_alpha.green(), 255);
        assert_eq!(with_alpha.alpha(), 128);
    }

    #[test]
    fn test_color_from_hex() {
        let red = Color::from_hex("#FF0000").unwrap();
        assert_eq!(red, Color::RED);

        let blue_no_hash = Color::from_hex("0000FF").unwrap();
        assert_eq!(blue_no_hash, Color::BLUE);

        let with_alpha = Color::from_hex("#80FF0000").unwrap();
        assert_eq!(with_alpha.alpha(), 128);
        assert_eq!(with_alpha.red(), 255);

        // Invalid formats
        assert!(Color::from_hex("FF").is_err());
        assert!(Color::from_hex("GGGGGG").is_err());
    }

    #[test]
    fn test_color_to_hex() {
        assert_eq!(Color::RED.to_hex(), "#FF0000");
        assert_eq!(Color::BLUE.to_hex(), "#0000FF");

        let semi_transparent = Color::rgba(255, 0, 0, 128);
        assert_eq!(semi_transparent.to_hex(), "#80FF0000");
    }

    #[test]
    fn test_color_argb() {
        let color = Color::from_argb(0xFF0000FF);
        assert_eq!(color, Color::BLUE);

        let argb = Color::RED.to_argb();
        assert_eq!(argb, 0xFFFF0000);
    }

    #[test]
    fn test_color_opacity() {
        let opaque = Color::RED;
        let half = opaque.with_opacity(0.5);

        assert_eq!(half.alpha(), 127); // 0.5 * 255
        assert!((half.opacity() - 0.5).abs() < 0.01);

        // Test clamping
        let clamped_low = opaque.with_opacity(-1.0);
        assert_eq!(clamped_low.alpha(), 0);

        let clamped_high = opaque.with_opacity(2.0);
        assert_eq!(clamped_high.alpha(), 255);
    }

    #[test]
    fn test_color_with_components() {
        let color = Color::rgb(100, 150, 200);

        let r = color.with_red(255);
        assert_eq!(r, Color::rgb(255, 150, 200));

        let g = color.with_green(255);
        assert_eq!(g, Color::rgb(100, 255, 200));

        let b = color.with_blue(255);
        assert_eq!(b, Color::rgb(100, 150, 255));

        let a = color.with_alpha(128);
        assert_eq!(a.alpha(), 128);
    }

    #[test]
    fn test_color_checks() {
        assert!(Color::TRANSPARENT.is_transparent());
        assert!(!Color::RED.is_transparent());

        assert!(Color::RED.is_opaque());
        assert!(!Color::rgba(255, 0, 0, 128).is_opaque());
    }

    #[test]
    fn test_color_lerp() {
        let red = Color::RED;
        let blue = Color::BLUE;

        // At t=0, should be red
        let at_0 = Color::lerp(red, blue, 0.0);
        assert_eq!(at_0, red);

        // At t=1, should be blue
        let at_1 = Color::lerp(red, blue, 1.0);
        assert_eq!(at_1, blue);

        // At t=0.5, should be mix
        let mid = Color::lerp(red, blue, 0.5);
        assert!(mid.red() > 0 && mid.red() < 255);
        assert!(mid.blue() > 0 && mid.blue() < 255);

        // Test clamping
        let clamped = Color::lerp(red, blue, 2.0);
        assert_eq!(clamped, blue);
    }

    #[test]
    fn test_color_conversions() {
        // From tuples
        let from_tuple: Color = (255, 0, 0).into();
        assert_eq!(from_tuple, Color::RED);

        let from_tuple_alpha: Color = (255, 0, 0, 128).into();
        assert_eq!(from_tuple_alpha.alpha(), 128);

        // From arrays
        let from_array: Color = [0, 255, 0].into();
        assert_eq!(from_array, Color::GREEN);

        let from_array_alpha: Color = [0, 0, 255, 200].into();
        assert_eq!(from_array_alpha, Color::rgba(0, 0, 255, 200));
    }

    #[test]
    fn test_color_rgba_f32() {
        let red = Color::RED;
        let (r, g, b, a) = red.to_rgba_f32();

        assert!((r - 1.0).abs() < 0.01);
        assert!((g - 0.0).abs() < 0.01);
        assert!((b - 0.0).abs() < 0.01);
        assert!((a - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_color_constants() {
        assert_eq!(Color::BLACK, Color::rgb(0, 0, 0));
        assert_eq!(Color::WHITE, Color::rgb(255, 255, 255));
        assert_eq!(Color::RED, Color::rgb(255, 0, 0));
        assert_eq!(Color::GREEN, Color::rgb(0, 255, 0));
        assert_eq!(Color::BLUE, Color::rgb(0, 0, 255));
        assert_eq!(Color::YELLOW, Color::rgb(255, 255, 0));
        assert_eq!(Color::CYAN, Color::rgb(0, 255, 255));
        assert_eq!(Color::MAGENTA, Color::rgb(255, 0, 255));
        assert_eq!(Color::TRANSPARENT, Color::rgba(0, 0, 0, 0));
    }

    #[test]
    fn test_default() {
        let default: Color = Default::default();
        assert_eq!(default, Color::TRANSPARENT);
    }
}
