//! Shader types for painting.

use crate::{paint::TileMode, styling::Color};
use flui_foundation::geometry::Offset;

/// A shader (or gradient) to use when filling a shape.
///
/// # Examples
///
/// ```
/// use flui_painting::{paint::Shader, styling::Color};
///
/// let shader = Shader::linear_gradient(
///     flui_foundation::geometry::Offset::ZERO,
///     flui_foundation::geometry::Offset::new(100.0, 100.0),
///     vec![Color::RED, Color::BLUE],
///     None,
///     flui_painting::paint::TileMode::Clamp,
/// );
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Shader {
    /// A linear gradient shader.
    LinearGradient {
        /// The starting point of the gradient.
        from: Offset<f64>,
        /// The ending point of the gradient.
        to: Offset<f64>,
        /// The colors in the gradient.
        colors: Vec<Color>,
        /// Optional color stops (0.0 to 1.0).
        stops: Option<Vec<f64>>,
        /// How to tile the gradient.
        tile_mode: TileMode,
    },

    /// A radial gradient shader.
    RadialGradient {
        /// The center of the gradient.
        center: Offset<f64>,
        /// The radius of the gradient.
        radius: f64,
        /// The colors in the gradient.
        colors: Vec<Color>,
        /// Optional color stops (0.0 to 1.0).
        stops: Option<Vec<f64>>,
        /// How to tile the gradient.
        tile_mode: TileMode,
        /// Optional focal point.
        focal: Option<Offset<f64>>,
        /// Optional focal radius.
        focal_radius: Option<f64>,
    },

    /// A sweep (angular/conic) gradient shader.
    SweepGradient {
        /// The center of the gradient.
        center: Offset<f64>,
        /// The colors in the gradient.
        colors: Vec<Color>,
        /// Optional color stops (0.0 to 1.0).
        stops: Option<Vec<f64>>,
        /// How to tile the gradient.
        tile_mode: TileMode,
        /// The starting angle in radians.
        start_angle: f64,
        /// The ending angle in radians.
        end_angle: f64,
    },

    /// A solid color shader (useful for masks and testing).
    Solid {
        /// The solid color.
        color: Color,
    },

    /// An image shader.
    Image(ImageShader),
}

impl Shader {
    /// Creates a linear gradient shader.
    #[inline]
    #[must_use]
    pub fn linear_gradient(
        from: Offset<f64>,
        to: Offset<f64>,
        colors: Vec<Color>,
        stops: Option<Vec<f64>>,
        tile_mode: TileMode,
    ) -> Self {
        Shader::LinearGradient {
            from,
            to,
            colors,
            stops,
            tile_mode,
        }
    }

    /// Creates a radial gradient shader.
    #[inline]
    #[must_use]
    pub fn radial_gradient(
        center: Offset<f64>,
        radius: f64,
        colors: Vec<Color>,
        stops: Option<Vec<f64>>,
        tile_mode: TileMode,
        focal: Option<Offset<f64>>,
        focal_radius: Option<f64>,
    ) -> Self {
        Shader::RadialGradient {
            center,
            radius,
            colors,
            stops,
            tile_mode,
            focal,
            focal_radius,
        }
    }

    /// Creates a sweep gradient shader.
    #[inline]
    #[must_use]
    pub fn sweep_gradient(
        center: Offset<f64>,
        colors: Vec<Color>,
        stops: Option<Vec<f64>>,
        tile_mode: TileMode,
        start_angle: f64,
        end_angle: f64,
    ) -> Self {
        Shader::SweepGradient {
            center,
            colors,
            stops,
            tile_mode,
            start_angle,
            end_angle,
        }
    }

    /// Creates a solid color shader.
    #[inline]
    #[must_use]
    pub fn solid(color: Color) -> Self {
        Shader::Solid { color }
    }

    /// Creates an image shader.
    #[inline]
    #[must_use]
    pub fn image(shader: ImageShader) -> Self {
        Shader::Image(shader)
    }

    /// Creates a simple linear gradient with default settings.
    ///
    /// This is a convenience method that creates a linear gradient with:
    /// - No color stops (colors evenly distributed)
    /// - TileMode::Clamp (no repeating)
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Offset;
    /// use flui_painting::{paint::Shader, styling::Color};
    ///
    /// let shader = Shader::simple_linear(
    ///     Offset::ZERO,
    ///     Offset::new(100.0, 0.0),
    ///     vec![Color::RED, Color::BLUE],
    /// );
    /// ```
    #[inline]
    #[must_use]
    pub fn simple_linear(from: Offset<f64>, to: Offset<f64>, colors: Vec<Color>) -> Self {
        Self::linear_gradient(from, to, colors, None, TileMode::Clamp)
    }

    /// Creates a simple radial gradient with default settings.
    ///
    /// This is a convenience method that creates a radial gradient with:
    /// - No color stops (colors evenly distributed)
    /// - No focal point
    /// - TileMode::Clamp (no repeating)
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Offset;
    /// use flui_painting::{paint::Shader, styling::Color};
    ///
    /// let shader = Shader::simple_radial(
    ///     Offset::new(50.0, 50.0),
    ///     25.0,
    ///     vec![Color::RED, Color::BLUE],
    /// );
    /// ```
    #[inline]
    #[must_use]
    pub fn simple_radial(center: Offset<f64>, radius: f64, colors: Vec<Color>) -> Self {
        Self::radial_gradient(center, radius, colors, None, TileMode::Clamp, None, None)
    }

    /// Creates a simple sweep (conic) gradient with default settings.
    ///
    /// This is a convenience method that creates a full 360° sweep gradient
    /// with:
    /// - No color stops (colors evenly distributed)
    /// - Full rotation (0 to 2π radians)
    /// - TileMode::Clamp (no repeating)
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Offset;
    /// use flui_painting::{paint::Shader, styling::Color};
    ///
    /// let shader = Shader::simple_sweep(
    ///     Offset::new(50.0, 50.0),
    ///     vec![Color::RED, Color::GREEN, Color::BLUE],
    /// );
    /// ```
    #[inline]
    #[must_use]
    pub fn simple_sweep(center: Offset<f64>, colors: Vec<Color>) -> Self {
        Self::sweep_gradient(
            center,
            colors,
            None,
            TileMode::Clamp,
            0.0,
            std::f64::consts::TAU,
        )
    }

    /// Returns the number of colors in this shader.
    #[inline]
    #[must_use]
    pub fn color_count(&self) -> usize {
        match self {
            Shader::LinearGradient { colors, .. }
            | Shader::RadialGradient { colors, .. }
            | Shader::SweepGradient { colors, .. } => colors.len(),
            Shader::Solid { .. } => 1,
            Shader::Image(_) => 0,
        }
    }
}

/// A shader that tiles an image.
///
/// # Examples
///
/// ```
/// use flui_painting::paint::{ImageShader, TileMode};
///
/// let shader = ImageShader::new(TileMode::Repeat, TileMode::Repeat);
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ImageShader {
    /// How to tile the image in the x direction.
    pub tile_mode_x: TileMode,

    /// How to tile the image in the y direction.
    pub tile_mode_y: TileMode,

    /// Optional transformation matrix (3x3).
    pub transform: Option<[[f64; 3]; 3]>,

    /// Optional filter quality.
    pub filter_quality: Option<crate::paint::FilterQuality>,
}

impl ImageShader {
    /// Creates a new image shader.
    #[inline]
    #[must_use]
    pub const fn new(tile_mode_x: TileMode, tile_mode_y: TileMode) -> Self {
        Self {
            tile_mode_x,
            tile_mode_y,
            transform: None,
            filter_quality: None,
        }
    }

    /// Creates a new image shader with a transformation matrix.
    #[inline]
    #[must_use]
    pub const fn with_transform(mut self, transform: [[f64; 3]; 3]) -> Self {
        self.transform = Some(transform);
        self
    }

    /// Creates a new image shader with filter quality.
    #[inline]
    #[must_use]
    pub const fn with_filter_quality(mut self, quality: crate::paint::FilterQuality) -> Self {
        self.filter_quality = Some(quality);
        self
    }

    /// Returns true if this shader has a transformation.
    #[inline]
    #[must_use]
    pub const fn has_transform(&self) -> bool {
        self.transform.is_some()
    }
}
