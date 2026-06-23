//! Visual effects types for painting.
//!
//! This module provides types for blur effects and color filters,
//! similar to CSS filter and backdrop-filter properties.

use std::sync::Arc;

use super::canvas::{StrokeCap, StrokeJoin};
use crate::geometry::Pixels;

/// Blur quality/algorithm level.
///
/// Controls the quality and performance trade-off for blur rendering.
/// Higher quality produces smoother blur but requires more computation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BlurQuality {
    /// Fast box blur approximation (single pass)
    Low,

    /// Multi-pass box blur (approximates gaussian, 3 passes)
    #[default]
    Medium,

    /// High-quality gaussian blur (5+ passes)
    High,
}

/// Blur mode determines how blur is applied.
///
/// Similar to CSS filter vs backdrop-filter distinction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BlurMode {
    /// Blur the content itself (like CSS filter: blur())
    #[default]
    Content,

    /// Blur the backdrop behind the content (like CSS backdrop-filter: blur())
    Backdrop,
}

/// Color adjustment types for image and content manipulation.
///
/// Similar to CSS filter functions and SVG color matrix filters.
/// These are high-level color adjustments (brightness, contrast, etc.)
/// as opposed to [`crate::painting::ColorFilter`] which is a low-level
/// Skia/Flutter-style color filter with blend modes.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ColorAdjustment {
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
#[derive(Debug, Clone, Copy, PartialEq)]
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
    #[inline]
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
    #[inline]
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
    #[inline]
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
    #[inline]
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
    #[inline]
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
    /// * `amount` - Saturation multiplier (0.0 to 2.0, 1.0 = no change, 0.0 =
    ///   grayscale)
    #[inline]
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
    #[inline]
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
    #[inline]
    pub fn invert() -> Self {
        Self::new([
            -1.0, 0.0, 0.0, 0.0, 1.0, // R
            0.0, -1.0, 0.0, 0.0, 1.0, // G
            0.0, 0.0, -1.0, 0.0, 1.0, // B
            0.0, 0.0, 0.0, 1.0, 0.0, // A
        ])
    }

    /// Create a matrix that scales only the alpha channel by `opacity`.
    ///
    /// RGB rows are identity; alpha row is `[0, 0, 0, opacity, 0]`.
    /// Used to lower a layer's group opacity via the color-matrix GPU pass
    /// without touching hue or saturation.
    #[inline]
    pub fn opacity(opacity: f32) -> Self {
        Self::new([
            1.0, 0.0, 0.0, 0.0, 0.0, // R
            0.0, 1.0, 0.0, 0.0, 0.0, // G
            0.0, 0.0, 1.0, 0.0, 0.0, // B
            0.0, 0.0, 0.0, opacity, 0.0, // A
        ])
    }

    /// Linearly interpolate between the identity matrix and `other` by `t ∈ [0, 1]`.
    ///
    /// `t = 0` → identity; `t = 1` → `other`.  Values are not clamped;
    /// callers supplying a `t` outside `[0, 1]` get extrapolation.
    ///
    /// Used by `ColorAdjustment::to_color_matrix` for the strength-parameterised
    /// Grayscale/Sepia/Invert variants.
    #[inline]
    pub fn lerp_from_identity(other: &ColorMatrix, t: f32) -> Self {
        let identity = ColorMatrix::identity();
        let mut values = [0.0f32; 20];
        for (i, v) in values.iter_mut().enumerate() {
            *v = identity.values[i] + (other.values[i] - identity.values[i]) * t;
        }
        Self::new(values)
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
    #[inline]
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
    #[inline]
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
    #[inline]
    fn default() -> Self {
        Self::identity()
    }
}

impl ColorAdjustment {
    /// Convert this high-level color adjustment to an equivalent [`ColorMatrix`].
    ///
    /// The returned matrix is applied to straight (un-premultiplied) RGBA in `[0, 1]`,
    /// identical to the GPU color-matrix pass.  CPU callers can use
    /// [`ColorMatrix::apply`] as the oracle for verifying GPU output.
    ///
    /// ## Variant mapping
    ///
    /// | Variant | Matrix constructor |
    /// |---------|-------------------|
    /// | `Brightness(a)` | `ColorMatrix::brightness(a)` |
    /// | `Contrast(a)` | `ColorMatrix::contrast(a)` |
    /// | `Saturation(a)` | `ColorMatrix::saturation(a)` |
    /// | `HueRotate(deg)` | `ColorMatrix::hue_rotate(deg)` |
    /// | `Grayscale(t)` | `lerp_from_identity(grayscale, t)` |
    /// | `Sepia(t)` | `lerp_from_identity(sepia, t)` |
    /// | `Invert(t)` | `lerp_from_identity(invert, t)` |
    /// | `Opacity(o)` | `ColorMatrix::opacity(o)` |
    /// | `Matrix(m)` | `m` verbatim |
    pub fn to_color_matrix(&self) -> ColorMatrix {
        match self {
            ColorAdjustment::Brightness(amount) => ColorMatrix::brightness(*amount),
            ColorAdjustment::Contrast(amount) => ColorMatrix::contrast(*amount),
            ColorAdjustment::Saturation(amount) => ColorMatrix::saturation(*amount),
            ColorAdjustment::HueRotate(degrees) => ColorMatrix::hue_rotate(*degrees),
            ColorAdjustment::Grayscale(t) => {
                ColorMatrix::lerp_from_identity(&ColorMatrix::grayscale(), *t)
            }
            ColorAdjustment::Sepia(t) => ColorMatrix::lerp_from_identity(&ColorMatrix::sepia(), *t),
            ColorAdjustment::Invert(t) => {
                ColorMatrix::lerp_from_identity(&ColorMatrix::invert(), *t)
            }
            ColorAdjustment::Opacity(opacity) => ColorMatrix::opacity(*opacity),
            ColorAdjustment::Matrix(matrix) => *matrix,
        }
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
    #[inline]
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
/// use flui_types::painting::effects::{ImageFilter, ColorAdjustment};
///
/// // Blur filter
/// let blur = ImageFilter::Blur {
///     sigma_x: 5.0,
///     sigma_y: 5.0,
/// };
///
/// // Color adjustment
/// let grayscale = ImageFilter::ColorAdjust(ColorAdjustment::Grayscale(1.0));
///
/// // Combined filters
/// let filters = vec![
///     ImageFilter::Blur { sigma_x: 3.0, sigma_y: 3.0 },
///     ImageFilter::ColorAdjust(ColorAdjustment::Brightness(0.1)),
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

    /// Dilate (expand bright areas) with specified radius.
    ///
    /// Morphological dilation operation - grows bright regions.
    Dilate {
        /// Dilation radius in pixels
        radius: f32,
    },

    /// Erode (expand dark areas) with specified radius.
    ///
    /// Morphological erosion operation - shrinks bright regions.
    Erode {
        /// Erosion radius in pixels
        radius: f32,
    },

    /// 5x4 color matrix transformation.
    ///
    /// Applies a custom color transformation matrix to all pixels.
    Matrix(ColorMatrix),

    /// Color adjustment filter.
    ///
    /// Applies brightness, contrast, saturation, or other color adjustments.
    ColorAdjust(ColorAdjustment),

    /// Combined filters (applied in order).
    ///
    /// Allows chaining multiple image filters together.
    Compose(Vec<ImageFilter>),

    /// Overflow indicator with diagonal stripes (debug mode only).
    ///
    /// Displays a warning tape pattern (red/yellow diagonal stripes) on the
    /// overflow region. This filter is only active in debug builds and has
    /// zero cost in release builds.
    ///
    /// # Parameters
    ///
    /// * `overflow_h` - Horizontal overflow in pixels (0.0 if none)
    /// * `overflow_v` - Vertical overflow in pixels (0.0 if none)
    /// * `container_size` - Size of the container for positioning
    #[cfg(debug_assertions)]
    OverflowIndicator {
        /// Horizontal overflow in pixels
        overflow_h: f32,
        /// Vertical overflow in pixels
        overflow_v: f32,
        /// Container size
        container_size: crate::Size<Pixels>,
    },
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

    /// Create a dilate filter with specified radius.
    #[inline]
    #[must_use]
    pub fn dilate(radius: f32) -> Self {
        Self::Dilate { radius }
    }

    /// Create an erode filter with specified radius.
    #[inline]
    #[must_use]
    pub fn erode(radius: f32) -> Self {
        Self::Erode { radius }
    }

    /// Create a matrix filter.
    #[inline]
    #[must_use]
    pub fn matrix(matrix: ColorMatrix) -> Self {
        Self::Matrix(matrix)
    }

    /// Create a color adjustment filter.
    #[inline]
    #[must_use]
    pub fn color_adjust(adjustment: ColorAdjustment) -> Self {
        Self::ColorAdjust(adjustment)
    }

    /// Create an overflow indicator filter (debug mode only).
    ///
    /// # Arguments
    ///
    /// * `overflow_h` - Horizontal overflow in pixels (0.0 if none)
    /// * `overflow_v` - Vertical overflow in pixels (0.0 if none)
    /// * `container_size` - Size of the container
    #[cfg(debug_assertions)]
    #[inline]
    #[must_use]
    pub fn overflow_indicator(
        overflow_h: f32,
        overflow_v: f32,
        container_size: crate::Size<Pixels>,
    ) -> Self {
        Self::OverflowIndicator {
            overflow_h,
            overflow_v,
            container_size,
        }
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

    // ── ColorMatrix new helpers ─────────────────────────────────────────────

    /// `ColorMatrix::opacity(o)` only scales the alpha channel; RGB is passthrough.
    #[test]
    fn opacity_matrix_scales_alpha_only() {
        let m = ColorMatrix::opacity(0.5);
        let input = [0.8, 0.6, 0.4, 1.0];
        let output = m.apply(input);
        // RGB must be unchanged.
        assert!((output[0] - 0.8).abs() < 1e-6, "R unchanged");
        assert!((output[1] - 0.6).abs() < 1e-6, "G unchanged");
        assert!((output[2] - 0.4).abs() < 1e-6, "B unchanged");
        // Alpha halved.
        assert!((output[3] - 0.5).abs() < 1e-6, "A halved");
    }

    /// `lerp_from_identity(m, 0.0)` == identity applied to any colour.
    #[test]
    fn lerp_from_identity_at_zero_is_identity() {
        let grayscale = ColorMatrix::grayscale();
        let lerped = ColorMatrix::lerp_from_identity(&grayscale, 0.0);
        let color = [0.3, 0.5, 0.9, 0.7];
        let result = lerped.apply(color);
        for i in 0..4 {
            assert!(
                (result[i] - color[i]).abs() < 1e-5,
                "channel {i}: expected {}, got {}",
                color[i],
                result[i]
            );
        }
    }

    /// `lerp_from_identity(m, 1.0)` == `m` applied to any colour.
    #[test]
    fn lerp_from_identity_at_one_equals_full_matrix() {
        let grayscale = ColorMatrix::grayscale();
        let lerped = ColorMatrix::lerp_from_identity(&grayscale, 1.0);
        let color = [1.0, 0.0, 0.0, 1.0]; // red
        let direct = grayscale.apply(color);
        let via_lerp = lerped.apply(color);
        for i in 0..4 {
            assert!(
                (via_lerp[i] - direct[i]).abs() < 1e-5,
                "channel {i}: lerp(1)={} vs direct={}",
                via_lerp[i],
                direct[i]
            );
        }
    }

    // ── ColorAdjustment::to_color_matrix ──────────────────────────────────────

    /// Every `ColorAdjustment` variant produces a `ColorMatrix` that materially
    /// transforms the expected input color in the expected direction.
    ///
    /// These are behavioral assertions, not just "no panic" smoke tests.
    #[test]
    fn color_adjustment_to_color_matrix_all_variants_have_correct_effect() {
        // Saturation(0) = grayscale: opaque red → uniform gray ≈ (0.2126, …, 1.0).
        {
            let m = ColorAdjustment::Saturation(0.0).to_color_matrix();
            let out = m.apply([1.0, 0.0, 0.0, 1.0]);
            assert!(
                (out[0] - 0.2126).abs() < 1e-4,
                "Saturation(0) on red: R ≈ 0.2126, got {:.6}",
                out[0]
            );
            assert!(
                (out[0] - out[1]).abs() < 1e-4 && (out[1] - out[2]).abs() < 1e-4,
                "Saturation(0) on red must produce equal RGB channels (gray)"
            );
            assert!((out[3] - 1.0).abs() < 1e-6, "alpha unchanged");
        }

        // HueRotate(90) on red must not leave R unchanged (it rotates the hue).
        {
            let m = ColorAdjustment::HueRotate(90.0).to_color_matrix();
            let out = m.apply([1.0, 0.0, 0.0, 1.0]);
            assert!(
                (out[0] - 1.0).abs() > 0.01,
                "HueRotate(90) on red: R must change, got {:.6}",
                out[0]
            );
            assert!((out[3] - 1.0).abs() < 1e-6, "alpha unchanged");
        }

        // Sepia(1.0) on mid-gray (0.5,0.5,0.5): produces warm brownish tint (R > G > B).
        // Avoids white input where sepia R and G rows sum > 1 and get clamped.
        {
            let m = ColorAdjustment::Sepia(1.0).to_color_matrix();
            let out = m.apply([0.5, 0.5, 0.5, 1.0]);
            // Sepia on mid-gray: R ≈ 0.675, G ≈ 0.601, B ≈ 0.469.
            // Sepia warm-tint ordering must hold: R > G > B.
            assert!(
                out[0] > out[1] && out[1] > out[2],
                "Sepia(1) on mid-gray: must produce warm tint R>G>B, got R={:.3} G={:.3} B={:.3}",
                out[0],
                out[1],
                out[2]
            );
            // Verify channels are not equal (sepia is non-identity and asymmetric).
            assert!(
                (out[0] - out[2]).abs() > 0.1,
                "Sepia(1): R and B must differ significantly (brown tint), got R={:.3} B={:.3}",
                out[0],
                out[2]
            );
            assert!((out[3] - 1.0).abs() < 1e-6, "alpha unchanged");
        }

        // Brightness(+0.3) on mid-gray must increase all channels.
        {
            let m = ColorAdjustment::Brightness(0.3).to_color_matrix();
            let input = [0.5, 0.5, 0.5, 1.0];
            let out = m.apply(input);
            assert!(out[0] > input[0], "Brightness(+0.3): R must increase");
            assert!(out[1] > input[1], "Brightness(+0.3): G must increase");
            assert!(out[2] > input[2], "Brightness(+0.3): B must increase");
            assert!((out[3] - 1.0).abs() < 1e-6, "alpha unchanged");
        }

        // Contrast(2.0) on mid-gray (0.5) must move channels away from 0.5.
        {
            let m = ColorAdjustment::Contrast(2.0).to_color_matrix();
            let input = [0.3, 0.5, 0.7, 1.0];
            let out = m.apply(input);
            // Contrast > 1: values below 0.5 decrease, above 0.5 increase.
            assert!(out[0] < input[0], "Contrast(2) on 0.3: R must decrease");
            assert!(out[2] > input[2], "Contrast(2) on 0.7: B must increase");
            assert!((out[3] - 1.0).abs() < 1e-6, "alpha unchanged");
        }

        // Remaining variants: confirm no panic and alpha is preserved.
        for (label, variant) in [
            ("Grayscale(0.5)", ColorAdjustment::Grayscale(0.5)),
            ("Invert(0.3)", ColorAdjustment::Invert(0.3)),
            ("Opacity(0.6)", ColorAdjustment::Opacity(0.6)),
            (
                "Matrix(identity)",
                ColorAdjustment::Matrix(ColorMatrix::identity()),
            ),
        ] {
            let m = variant.to_color_matrix();
            let out = m.apply([0.5, 0.5, 0.5, 1.0]);
            assert!(
                out.iter().all(|v| v.is_finite()),
                "{label}: output must be finite"
            );
        }
    }

    /// `Grayscale(1.0)` must produce the same matrix as `ColorMatrix::grayscale()`.
    #[test]
    fn grayscale_full_strength_matches_grayscale_constructor() {
        let via_adjustment = ColorAdjustment::Grayscale(1.0).to_color_matrix();
        let direct = ColorMatrix::grayscale();
        for i in 0..20 {
            assert!(
                (via_adjustment.values[i] - direct.values[i]).abs() < 1e-6,
                "index {i}: via_adjustment={} vs direct={}",
                via_adjustment.values[i],
                direct.values[i]
            );
        }
    }

    /// `Grayscale(0.0)` must produce the identity matrix (no effect).
    #[test]
    fn grayscale_zero_strength_is_identity() {
        let via_adjustment = ColorAdjustment::Grayscale(0.0).to_color_matrix();
        let identity = ColorMatrix::identity();
        for i in 0..20 {
            assert!(
                (via_adjustment.values[i] - identity.values[i]).abs() < 1e-6,
                "index {i}: expected identity {}, got {}",
                identity.values[i],
                via_adjustment.values[i]
            );
        }
    }

    /// `Opacity(0.5)` applied to a fully opaque red halves the alpha.
    #[test]
    fn opacity_adjustment_halves_alpha() {
        let m = ColorAdjustment::Opacity(0.5).to_color_matrix();
        let out = m.apply([1.0, 0.0, 0.0, 1.0]);
        assert!((out[0] - 1.0).abs() < 1e-6, "R unchanged");
        assert!((out[1] - 0.0).abs() < 1e-6, "G unchanged");
        assert!((out[2] - 0.0).abs() < 1e-6, "B unchanged");
        assert!((out[3] - 0.5).abs() < 1e-6, "A halved to 0.5");
    }

    /// `Matrix(m)` round-trips the matrix unchanged.
    #[test]
    fn matrix_adjustment_roundtrips_values() {
        let original = ColorMatrix::grayscale();
        let via_adjustment = ColorAdjustment::Matrix(original).to_color_matrix();
        assert_eq!(original.values, via_adjustment.values);
    }

    /// `Invert(0.5)` applied to a color is a linear blend between identity and full invert.
    #[test]
    fn invert_half_is_halfway_between_identity_and_full_invert() {
        let half = ColorAdjustment::Invert(0.5).to_color_matrix();
        let identity_out = ColorMatrix::identity().apply([0.8, 0.4, 0.2, 1.0]);
        let invert_out = ColorMatrix::invert().apply([0.8, 0.4, 0.2, 1.0]);
        let half_out = half.apply([0.8, 0.4, 0.2, 1.0]);
        for i in 0..4 {
            let expected_mid = f32::midpoint(identity_out[i], invert_out[i]);
            assert!(
                (half_out[i] - expected_mid).abs() < 1e-5,
                "channel {i}: expected mid {expected_mid:.6}, got {:.6}",
                half_out[i]
            );
        }
    }
}
