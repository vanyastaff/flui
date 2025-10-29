//! Visual effects types for painting.
//!
//! This module provides types for blur effects and color filters,
//! similar to CSS filter and backdrop-filter properties.

use std::sync::Arc;
use super::canvas::{StrokeCap, StrokeJoin};

/// Blur quality/algorithm level.
///
/// Controls the quality and performance trade-off for blur rendering.
/// Higher quality produces smoother blur but requires more computation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BlurQuality {
    /// Fast box blur approximation (single pass)
    Low,

    /// Multi-pass box blur (approximates gaussian, 3 passes)
    Medium,

    /// High-quality gaussian blur (5+ passes)
    High,
}

impl Default for BlurQuality {
    fn default() -> Self {
        Self::Medium
    }
}

/// Blur mode determines how blur is applied.
///
/// Similar to CSS filter vs backdrop-filter distinction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BlurMode {
    /// Blur the content itself (like CSS filter: blur())
    Content,

    /// Blur the backdrop behind the content (like CSS backdrop-filter: blur())
    Backdrop,
}

impl Default for BlurMode {
    fn default() -> Self {
        Self::Content
    }
}

/// Color filter types for image and content manipulation.
///
/// Similar to CSS filter functions and SVG color matrix filters.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ColorFilter {
    /// Adjust brightness (-1.0 to 1.0, 0.0 = no change)
    Brightness(f32),

    /// Adjust contrast (0.0 to 2.0, 1.0 = no change)
    Contrast(f32),

    /// Adjust saturation (0.0 to 2.0, 1.0 = no change, 0.0 = grayscale)
    Saturation(f32),

    /// Rotate hue (0.0 to 360.0 degrees)
    HueRotate(f32),

    /// Convert to grayscale (0.0 = no effect, 1.0 = full grayscale)
    Grayscale(f32),

    /// Apply sepia tone (0.0 = no effect, 1.0 = full sepia)
    Sepia(f32),

    /// Invert colors (0.0 = no effect, 1.0 = full inversion)
    Invert(f32),

    /// Adjust opacity (0.0 = transparent, 1.0 = opaque)
    Opacity(f32),

    /// Custom 5×4 color matrix transformation
    Matrix(ColorMatrix),
}

/// A 5×4 color transformation matrix.
///
/// Similar to SVG feColorMatrix. The matrix is applied as:
/// ```text
/// | R' |   | r0 r1 r2 r3 r4 |   | R |
/// | G' | = | g0 g1 g2 g3 g4 | × | G |
/// | B' |   | b0 b1 b2 b3 b4 |   | B |
/// | A' |   | a0 a1 a2 a3 a4 |   | A |
///                                 | 1 |
/// ```
///
/// # Examples
///
/// ```rust,ignore
/// use flui_types::painting::effects::ColorMatrix;
///
/// // Identity matrix (no change)
/// let identity = ColorMatrix::identity();
///
/// // Grayscale
/// let grayscale = ColorMatrix::grayscale();
///
/// // Custom matrix
/// let custom = ColorMatrix::new([
///     1.0, 0.0, 0.0, 0.0, 0.0,  // R
///     0.0, 1.0, 0.0, 0.0, 0.0,  // G
///     0.0, 0.0, 1.0, 0.0, 0.0,  // B
///     0.0, 0.0, 0.0, 1.0, 0.0,  // A
/// ]);
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ColorMatrix {
    /// Matrix values in row-major order: [r0-r4, g0-g4, b0-b4, a0-a4]
    pub values: [f32; 20],
}

impl ColorMatrix {
    /// Create a new color matrix from values.
    #[inline]
    pub const fn new(values: [f32; 20]) -> Self {
        Self { values }
    }

    /// Create an identity matrix (no transformation).
    pub fn identity() -> Self {
        Self::new([
            1.0, 0.0, 0.0, 0.0, 0.0, // R
            0.0, 1.0, 0.0, 0.0, 0.0, // G
            0.0, 0.0, 1.0, 0.0, 0.0, // B
            0.0, 0.0, 0.0, 1.0, 0.0, // A
        ])
    }

    /// Create a grayscale matrix.
    ///
    /// Uses luminance weights: R=0.2126, G=0.7152, B=0.0722
    pub fn grayscale() -> Self {
        let r = 0.2126;
        let g = 0.7152;
        let b = 0.0722;

        Self::new([
            r, g, b, 0.0, 0.0, // R
            r, g, b, 0.0, 0.0, // G
            r, g, b, 0.0, 0.0, // B
            0.0, 0.0, 0.0, 1.0, 0.0, // A
        ])
    }

    /// Create a sepia matrix.
    pub fn sepia() -> Self {
        Self::new([
            0.393, 0.769, 0.189, 0.0, 0.0, // R
            0.349, 0.686, 0.168, 0.0, 0.0, // G
            0.272, 0.534, 0.131, 0.0, 0.0, // B
            0.0, 0.0, 0.0, 1.0, 0.0, // A
        ])
    }

    /// Create a brightness adjustment matrix.
    ///
    /// # Arguments
    ///
    /// * `amount` - Brightness adjustment (-1.0 to 1.0, 0.0 = no change)
    pub fn brightness(amount: f32) -> Self {
        Self::new([
            1.0, 0.0, 0.0, 0.0, amount, // R
            0.0, 1.0, 0.0, 0.0, amount, // G
            0.0, 0.0, 1.0, 0.0, amount, // B
            0.0, 0.0, 0.0, 1.0, 0.0, // A
        ])
    }

    /// Create a contrast adjustment matrix.
    ///
    /// # Arguments
    ///
    /// * `amount` - Contrast multiplier (0.0 to 2.0, 1.0 = no change)
    pub fn contrast(amount: f32) -> Self {
        let offset = 0.5 * (1.0 - amount);

        Self::new([
            amount, 0.0, 0.0, 0.0, offset, // R
            0.0, amount, 0.0, 0.0, offset, // G
            0.0, 0.0, amount, 0.0, offset, // B
            0.0, 0.0, 0.0, 1.0, 0.0, // A
        ])
    }

    /// Create a saturation adjustment matrix.
    ///
    /// # Arguments
    ///
    /// * `amount` - Saturation multiplier (0.0 to 2.0, 1.0 = no change, 0.0 = grayscale)
    pub fn saturation(amount: f32) -> Self {
        let r = 0.2126 * (1.0 - amount);
        let g = 0.7152 * (1.0 - amount);
        let b = 0.0722 * (1.0 - amount);

        Self::new([
            r + amount,
            g,
            b,
            0.0,
            0.0, // R
            r,
            g + amount,
            b,
            0.0,
            0.0, // G
            r,
            g,
            b + amount,
            0.0,
            0.0, // B
            0.0,
            0.0,
            0.0,
            1.0,
            0.0, // A
        ])
    }

    /// Create a hue rotation matrix.
    ///
    /// # Arguments
    ///
    /// * `degrees` - Hue rotation in degrees (0.0 to 360.0)
    pub fn hue_rotate(degrees: f32) -> Self {
        let radians = degrees * std::f32::consts::PI / 180.0;
        let cos = radians.cos();
        let sin = radians.sin();

        // Hue rotation matrix using standard RGB rotation
        let r_weight = 0.2126;
        let g_weight = 0.7152;
        let b_weight = 0.0722;

        Self::new([
            r_weight + cos * (1.0 - r_weight) + sin * (-r_weight),
            g_weight + cos * (-g_weight) + sin * (-g_weight),
            b_weight + cos * (-b_weight) + sin * (1.0 - b_weight),
            0.0,
            0.0,
            r_weight + cos * (-r_weight) + sin * 0.143,
            g_weight + cos * (1.0 - g_weight) + sin * 0.140,
            b_weight + cos * (-b_weight) + sin * -0.283,
            0.0,
            0.0,
            r_weight + cos * (-r_weight) + sin * (-(1.0 - r_weight)),
            g_weight + cos * (-g_weight) + sin * g_weight,
            b_weight + cos * (1.0 - b_weight) + sin * b_weight,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
        ])
    }

    /// Create an inversion matrix.
    pub fn invert() -> Self {
        Self::new([
            -1.0, 0.0, 0.0, 0.0, 1.0, // R
            0.0, -1.0, 0.0, 0.0, 1.0, // G
            0.0, 0.0, -1.0, 0.0, 1.0, // B
            0.0, 0.0, 0.0, 1.0, 0.0, // A
        ])
    }

    /// Apply this color matrix to a color.
    ///
    /// # Arguments
    ///
    /// * `color` - Input color as [r, g, b, a] where each component is 0.0-1.0
    ///
    /// # Returns
    ///
    /// Transformed color as [r, g, b, a]
    pub fn apply(&self, color: [f32; 4]) -> [f32; 4] {
        let [r, g, b, a] = color;

        [
            (self.values[0] * r
                + self.values[1] * g
                + self.values[2] * b
                + self.values[3] * a
                + self.values[4])
                .clamp(0.0, 1.0),
            (self.values[5] * r
                + self.values[6] * g
                + self.values[7] * b
                + self.values[8] * a
                + self.values[9])
                .clamp(0.0, 1.0),
            (self.values[10] * r
                + self.values[11] * g
                + self.values[12] * b
                + self.values[13] * a
                + self.values[14])
                .clamp(0.0, 1.0),
            (self.values[15] * r
                + self.values[16] * g
                + self.values[17] * b
                + self.values[18] * a
                + self.values[19])
                .clamp(0.0, 1.0),
        ]
    }

    /// Multiply this matrix with another matrix.
    ///
    /// This allows composing multiple color transformations.
    pub fn multiply(&self, other: &ColorMatrix) -> Self {
        let mut result = [0.0; 20];

        for row in 0..4 {
            for col in 0..5 {
                let mut sum = 0.0;
                for i in 0..4 {
                    sum += self.values[row * 5 + i] * other.values[i * 5 + col];
                }
                if col == 4 {
                    sum += self.values[row * 5 + 4];
                }
                result[row * 5 + col] = sum;
            }
        }

        Self::new(result)
    }
}

impl Default for ColorMatrix {
    fn default() -> Self {
        Self::identity()
    }
}

/// Path stroke options for rendering vector paths.
///
/// Similar to HTML Canvas stroke properties and SVG stroke attributes.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StrokeOptions {
    /// Stroke width in pixels
    pub width: f32,

    /// Line cap style
    pub cap: StrokeCap,

    /// Line join style
    pub join: StrokeJoin,

    /// Miter limit for miter joins (only used when join is Miter)
    pub miter_limit: f32,

    /// Optional dash pattern (alternating dash/gap lengths)
    pub dash_pattern: Option<Arc<Vec<f32>>>,

    /// Offset into the dash pattern
    pub dash_offset: f32,
}

impl Default for StrokeOptions {
    fn default() -> Self {
        Self {
            width: 1.0,
            cap: StrokeCap::Butt,
            join: StrokeJoin::Miter,
            miter_limit: 4.0,
            dash_pattern: None,
            dash_offset: 0.0,
        }
    }
}

impl StrokeOptions {
    /// Create new stroke options with default values.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set stroke width.
    #[inline]
    #[must_use]
    pub fn with_width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Set line cap style.
    #[inline]
    #[must_use]
    pub fn with_cap(mut self, cap: StrokeCap) -> Self {
        self.cap = cap;
        self
    }

    /// Set line join style.
    #[inline]
    #[must_use]
    pub fn with_join(mut self, join: StrokeJoin) -> Self {
        self.join = join;
        self
    }

    /// Set miter limit.
    #[inline]
    #[must_use]
    pub fn with_miter_limit(mut self, miter_limit: f32) -> Self {
        self.miter_limit = miter_limit;
        self
    }

    /// Set dash pattern.
    #[inline]
    #[must_use]
    pub fn with_dash_pattern(mut self, pattern: Vec<f32>) -> Self {
        self.dash_pattern = Some(Arc::new(pattern));
        self
    }

    /// Set dash offset.
    #[inline]
    #[must_use]
    pub fn with_dash_offset(mut self, offset: f32) -> Self {
        self.dash_offset = offset;
        self
    }
}

/// Path rendering mode.
///
/// Determines how a path is rendered (filled, stroked, or both).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PathPaintMode {
    /// Fill the path interior
    Fill,

    /// Stroke the path outline
    Stroke,

    /// Both fill and stroke (fill first, then stroke)
    FillAndStroke,
}

/// Image filter for backdrop and content effects.
///
/// Combines blur and color filters into a single enum for use with
/// backdrop-filter and filter effects. Similar to CSS filter functions.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_types::painting::effects::{ImageFilter, ColorFilter};
///
/// // Blur filter
/// let blur = ImageFilter::Blur {
///     sigma_x: 5.0,
///     sigma_y: 5.0,
/// };
///
/// // Color filter
/// let grayscale = ImageFilter::Color(ColorFilter::Grayscale(1.0));
///
/// // Combined filters
/// let filters = vec![
///     ImageFilter::Blur { sigma_x: 3.0, sigma_y: 3.0 },
///     ImageFilter::Color(ColorFilter::Brightness(0.1)),
/// ];
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ImageFilter {
    /// Gaussian blur with horizontal and vertical sigma.
    ///
    /// Similar to CSS `blur()` function.
    Blur {
        /// Horizontal blur radius (sigma)
        sigma_x: f32,
        /// Vertical blur radius (sigma)
        sigma_y: f32,
    },

    /// Color manipulation filter.
    ///
    /// Applies brightness, contrast, saturation, or other color adjustments.
    Color(ColorFilter),

    /// Combined filters (applied in order).
    ///
    /// Allows chaining multiple image filters together.
    Compose(Vec<ImageFilter>),
}

impl ImageFilter {
    /// Create a blur filter with the same sigma for both axes.
    #[inline]
    #[must_use]
    pub fn blur(sigma: f32) -> Self {
        Self::Blur {
            sigma_x: sigma,
            sigma_y: sigma,
        }
    }

    /// Create a blur filter with different horizontal and vertical sigma.
    #[inline]
    #[must_use]
    pub fn blur_directional(sigma_x: f32, sigma_y: f32) -> Self {
        Self::Blur { sigma_x, sigma_y }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blur_quality_default() {
        assert_eq!(BlurQuality::default(), BlurQuality::Medium);
    }

    #[test]
    fn test_blur_mode_default() {
        assert_eq!(BlurMode::default(), BlurMode::Content);
    }

    #[test]
    fn test_color_matrix_identity() {
        let identity = ColorMatrix::identity();
        let color = [0.5, 0.6, 0.7, 0.8];
        let result = identity.apply(color);

        for i in 0..4 {
            assert!((result[i] - color[i]).abs() < 0.001);
        }
    }

    #[test]
    fn test_color_matrix_grayscale() {
        let grayscale = ColorMatrix::grayscale();
        let color = [1.0, 0.0, 0.0, 1.0]; // Red
        let result = grayscale.apply(color);

        // All RGB channels should be equal (grayscale)
        assert!((result[0] - result[1]).abs() < 0.001);
        assert!((result[1] - result[2]).abs() < 0.001);
        assert_eq!(result[3], 1.0); // Alpha unchanged
    }

    #[test]
    fn test_color_matrix_brightness() {
        let brighter = ColorMatrix::brightness(0.2);
        let color = [0.5, 0.5, 0.5, 1.0];
        let result = brighter.apply(color);

        // Should be brighter
        assert!(result[0] > color[0]);
        assert!(result[1] > color[1]);
        assert!(result[2] > color[2]);
    }

    #[test]
    fn test_stroke_options_builder() {
        let options = StrokeOptions::new()
            .with_width(2.0)
            .with_cap(StrokeCap::Round)
            .with_join(StrokeJoin::Round)
            .with_dash_pattern(vec![5.0, 3.0]);

        assert_eq!(options.width, 2.0);
        assert_eq!(options.cap, StrokeCap::Round);
        assert_eq!(options.join, StrokeJoin::Round);
        assert!(options.dash_pattern.is_some());
    }
}
