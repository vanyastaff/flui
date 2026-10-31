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
/// as opposed to [`crate::paint::ColorFilter`] which is a low-level
/// Skia-style color filter with blend modes.
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
/// use flui_painting::paint::effects::ColorMatrix;
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

    /// Whether the matrix turns a transparent black pixel into a visible one:
    /// it maps `(0, 0, 0, 0)` to its offset column, whose alpha is visible
    /// when positive. The colour offsets alone are not, since a pixel of
    /// zero alpha stays invisible whatever its colour.
    ///
    /// The one answer [`ColorFilter::modifies_transparent_black`] and
    /// [`ImageFilter::modifies_transparent_black`] give for a matrix.
    ///
    /// [`ColorFilter::modifies_transparent_black`]: crate::paint::ColorFilter::modifies_transparent_black
    #[inline]
    #[must_use]
    pub fn modifies_transparent_black(&self) -> bool {
        self.values[19] > 0.0
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
        let mut values = [0.0_f32; 20];
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
        // Every finite f32 product and four-term sum fits in f64. Narrow only
        // after saturation, so intermediate overflow cannot erase cancellation.
        std::array::from_fn(|row| {
            let weights = &self.values[row * 5..row * 5 + 5];
            let transformed = weights[..4]
                .iter()
                .zip(color)
                .fold(0.0_f64, |sum, (weight, channel)| {
                    sum + f64::from(*weight) * f64::from(channel)
                });
            (transformed + f64::from(weights[4])).clamp(0.0, 1.0) as f32
        })
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
    pub width: f64,

    /// Line cap style
    pub cap: StrokeCap,

    /// Line join style
    pub join: StrokeJoin,

    /// Miter limit for miter joins (only used when join is Miter)
    pub miter_limit: f64,

    /// Optional dash pattern (alternating dash/gap lengths)
    pub dash_pattern: Option<Arc<Vec<f64>>>,

    /// Offset into the dash pattern
    pub dash_offset: f64,
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
    pub fn with_width(mut self, width: f64) -> Self {
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
    pub fn with_miter_limit(mut self, miter_limit: f64) -> Self {
        self.miter_limit = miter_limit;
        self
    }

    /// Set dash pattern.
    #[inline]
    #[must_use]
    pub fn with_dash_pattern(mut self, pattern: Vec<f64>) -> Self {
        self.dash_pattern = Some(Arc::new(pattern));
        self
    }

    /// Set dash offset.
    #[inline]
    #[must_use]
    pub fn with_dash_offset(mut self, offset: f64) -> Self {
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
/// use flui_painting::paint::effects::{ImageFilter, ColorAdjustment};
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
        sigma_x: f64,
        /// Vertical blur radius (sigma)
        sigma_y: f64,
    },

    /// Dilate (expand bright areas) with specified radius.
    ///
    /// Morphological dilation operation - grows bright regions.
    Dilate {
        /// Dilation radius in pixels
        radius: f64,
    },

    /// Erode (expand dark areas) with specified radius.
    ///
    /// Morphological erosion operation - shrinks bright regions.
    Erode {
        /// Erosion radius in pixels
        radius: f64,
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

    /// The source seen through a 2D affine transform: the pixel at a point
    /// shows the source at the point `transform` maps it from, sampled the
    /// way `sampling` says. [`ImageFilter::try_magnify`] builds the common
    /// case. As a backdrop filter it draws a magnifier.
    Transform {
        /// Maps a point of the source to where it shows, in the filter's
        /// local space.
        transform: ImageTransform,
        /// How the source is read between its pixels.
        sampling: ImageSampling,
    },

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
        overflow_h: f64,
        /// Vertical overflow in pixels
        overflow_v: f64,
        /// Container size
        container_size: flui_foundation::geometry::Size<f64>,
    },
}

impl ImageFilter {
    /// The source magnified `scale` times about `focal`, in the filter's
    /// local space, read with linear sampling: a magnifier when used as a
    /// backdrop filter.
    ///
    /// # Errors
    ///
    /// [`InvalidImageTransform`] for a `scale` that is not finite and
    /// positive, or a `focal` that is not finite.
    pub fn try_magnify(
        focal: flui_foundation::geometry::Point<f64>,
        scale: f64,
    ) -> Result<Self, InvalidImageTransform> {
        Ok(Self::Transform {
            transform: ImageTransform::try_scale_about(focal, scale, scale)?,
            sampling: ImageSampling::Linear,
        })
    }

    /// Create a blur filter with the same sigma for both axes.
    #[inline]
    #[must_use]
    pub fn blur(sigma: f64) -> Self {
        Self::Blur {
            sigma_x: sigma,
            sigma_y: sigma,
        }
    }

    /// Create a blur filter with different horizontal and vertical sigma.
    #[inline]
    #[must_use]
    pub fn blur_directional(sigma_x: f64, sigma_y: f64) -> Self {
        Self::Blur { sigma_x, sigma_y }
    }

    /// Create a dilate filter with specified radius.
    #[inline]
    #[must_use]
    pub fn dilate(radius: f64) -> Self {
        Self::Dilate { radius }
    }

    /// Create an erode filter with specified radius.
    #[inline]
    #[must_use]
    pub fn erode(radius: f64) -> Self {
        Self::Erode { radius }
    }

    /// Whether the filter turns a transparent black pixel into a visible
    /// one, so a layer it filters changes pixels its children never inked.
    ///
    /// A colour matrix or adjustment answers as its matrix does
    /// ([`ColorMatrix::modifies_transparent_black`]); a composition does when
    /// any filter in it does. A blur or a morphology of transparent input is
    /// transparent, and so is a transform of it.
    #[must_use]
    pub fn modifies_transparent_black(&self) -> bool {
        match self {
            Self::Matrix(matrix) => matrix.modifies_transparent_black(),
            Self::ColorAdjust(adjustment) => {
                adjustment.to_color_matrix().modifies_transparent_black()
            }
            Self::Compose(filters) => filters.iter().any(Self::modifies_transparent_black),
            Self::Blur { .. }
            | Self::Dilate { .. }
            | Self::Erode { .. }
            | Self::Transform { .. } => false,
            #[cfg(debug_assertions)]
            Self::OverflowIndicator { .. } => false,
        }
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
        overflow_h: f64,
        overflow_v: f64,
        container_size: flui_foundation::geometry::Size<f64>,
    ) -> Self {
        Self::OverflowIndicator {
            overflow_h,
            overflow_v,
            container_size,
        }
    }
}

/// An invertible 2D affine transform with finite coefficients, as
/// [`ImageFilter::Transform`] takes it: a point `(x, y)` maps to
/// `(xx·x + xy·y + x0, yx·x + yy·y + y0)`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ImageTransform {
    xx: f64,
    yx: f64,
    xy: f64,
    yy: f64,
    x0: f64,
    y0: f64,
}

/// Why an [`ImageTransform`] could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidImageTransform {
    /// A coefficient, a scale or a focal point is NaN or infinite.
    #[error("an image transform needs finite coefficients")]
    NonFinite,
    /// The transform is not invertible: a zero determinant, an inverse that
    /// is not finite, or a scale that is not positive.
    #[error("an image transform must be invertible")]
    Singular,
}

impl ImageTransform {
    /// The identity transform.
    pub const IDENTITY: Self = Self {
        xx: 1.0,
        yx: 0.0,
        xy: 0.0,
        yy: 1.0,
        x0: 0.0,
        y0: 0.0,
    };

    /// The transform with these coefficients, in the order
    /// `(xx, yx, xy, yy, x0, y0)`.
    ///
    /// # Errors
    ///
    /// [`InvalidImageTransform::NonFinite`] for a coefficient that is not
    /// finite, [`InvalidImageTransform::Singular`] when it is not invertible
    /// or its inverse is not finite.
    pub fn try_new(
        xx: f64,
        yx: f64,
        xy: f64,
        yy: f64,
        x0: f64,
        y0: f64,
    ) -> Result<Self, InvalidImageTransform> {
        if ![xx, yx, xy, yy, x0, y0]
            .iter()
            .all(|value| value.is_finite())
        {
            return Err(InvalidImageTransform::NonFinite);
        }
        let transform = Self {
            xx,
            yx,
            xy,
            yy,
            x0,
            y0,
        };
        let determinant = xx * yy - xy * yx;
        if determinant == 0.0
            || !determinant.is_finite()
            || !transform
                .inverse_coefficients()
                .iter()
                .all(|value| value.is_finite())
        {
            return Err(InvalidImageTransform::Singular);
        }
        Ok(transform)
    }

    /// Scales by `sx` and `sy` about `focal`, which stays where it is.
    ///
    /// # Errors
    ///
    /// [`InvalidImageTransform::NonFinite`] for a value that is not finite,
    /// [`InvalidImageTransform::Singular`] for a scale that is not positive.
    pub fn try_scale_about(
        focal: flui_foundation::geometry::Point<f64>,
        sx: f64,
        sy: f64,
    ) -> Result<Self, InvalidImageTransform> {
        if ![focal.x, focal.y, sx, sy]
            .iter()
            .all(|value| value.is_finite())
        {
            return Err(InvalidImageTransform::NonFinite);
        }
        if sx <= 0.0 || sy <= 0.0 {
            return Err(InvalidImageTransform::Singular);
        }
        Self::try_new(
            sx,
            0.0,
            0.0,
            sy,
            focal.x - sx * focal.x,
            focal.y - sy * focal.y,
        )
    }

    /// The coefficients `(xx, yx, xy, yy, x0, y0)`.
    #[must_use]
    pub const fn coefficients(&self) -> [f64; 6] {
        [self.xx, self.yx, self.xy, self.yy, self.x0, self.y0]
    }

    /// The inverse transform's coefficients, in [`Self::coefficients`]'s
    /// order: it maps a shown point back to the source point it shows.
    #[must_use]
    pub fn inverse_coefficients(&self) -> [f64; 6] {
        let determinant = self.xx * self.yy - self.xy * self.yx;
        let (xx, yx, xy, yy) = (
            self.yy / determinant,
            -self.yx / determinant,
            -self.xy / determinant,
            self.xx / determinant,
        );
        [
            xx,
            yx,
            xy,
            yy,
            -(xx * self.x0 + xy * self.y0),
            -(yx * self.x0 + yy * self.y0),
        ]
    }
}

/// How [`ImageFilter::Transform`] reads its source between pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum ImageSampling {
    /// The nearest source pixel.
    Nearest,
    /// Bilinear interpolation between the four nearest pixels.
    #[default]
    Linear,
}
