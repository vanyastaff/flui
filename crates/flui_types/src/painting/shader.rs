//! Shader types for painting.

use crate::geometry::{Offset, Size, NumericUnit};
use crate::painting::{BlurStyle, TileMode};
use crate::styling::{Color, Color32};

/// A shader (or gradient) to use when filling a shape.
///
/// Similar to Flutter's `Shader`. This is a placeholder type that will be
/// implemented more fully when we have actual rendering capabilities.
///
/// # Examples
///
/// ```
/// use flui_types::painting::Shader;
/// use flui_types::styling::Color;
///
/// let shader = Shader::linear_gradient(
///     flui_types::geometry::Offset::ZERO,
///     flui_types::geometry::Offset::new(100.0, 100.0),
///     vec![Color::RED, Color::BLUE],
///     None,
///     flui_types::painting::TileMode::Clamp,
/// );
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Shader {
    /// A linear gradient shader.
    LinearGradient {
        /// The starting point of the gradient.
        from: Offset<f32>,
        /// The ending point of the gradient.
        to: Offset<f32>,
        /// The colors in the gradient.
        colors: Vec<Color>,
        /// Optional color stops (0.0 to 1.0).
        stops: Option<Vec<f32>>,
        /// How to tile the gradient.
        tile_mode: TileMode,
    },

    /// A radial gradient shader.
    RadialGradient {
        /// The center of the gradient.
        center: Offset<f32>,
        /// The radius of the gradient.
        radius: f32,
        /// The colors in the gradient.
        colors: Vec<Color>,
        /// Optional color stops (0.0 to 1.0).
        stops: Option<Vec<f32>>,
        /// How to tile the gradient.
        tile_mode: TileMode,
        /// Optional focal point.
        focal: Option<Offset<f32>>,
        /// Optional focal radius.
        focal_radius: Option<f32>,
    },

    /// A sweep (angular/conic) gradient shader.
    SweepGradient {
        /// The center of the gradient.
        center: Offset<f32>,
        /// The colors in the gradient.
        colors: Vec<Color>,
        /// Optional color stops (0.0 to 1.0).
        stops: Option<Vec<f32>>,
        /// How to tile the gradient.
        tile_mode: TileMode,
        /// The starting angle in radians.
        start_angle: f32,
        /// The ending angle in radians.
        end_angle: f32,
    },

    /// An image shader.
    Image(ImageShader),
}

impl Shader {
    /// Creates a linear gradient shader.
    #[inline]
    #[must_use]
    pub fn linear_gradient(
        from: Offset<f32>,
        to: Offset<f32>,
        colors: Vec<Color>,
        stops: Option<Vec<f32>>,
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
    #[allow(clippy::too_many_arguments)]
    pub fn radial_gradient(
        center: Offset<f32>,
        radius: f32,
        colors: Vec<Color>,
        stops: Option<Vec<f32>>,
        tile_mode: TileMode,
        focal: Option<Offset<f32>>,
        focal_radius: Option<f32>,
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
        center: Offset<f32>,
        colors: Vec<Color>,
        stops: Option<Vec<f32>>,
        tile_mode: TileMode,
        start_angle: f32,
        end_angle: f32,
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
    /// use flui_types::painting::Shader;
    /// use flui_types::geometry::Offset;
    /// use flui_types::styling::Color;
    ///
    /// let shader = Shader::simple_linear(
    ///     Offset::ZERO,
    ///     Offset::new(100.0, 0.0),
    ///     vec![Color::RED, Color::BLUE],
    /// );
    /// ```
    #[inline]
    #[must_use]
    pub fn simple_linear(from: Offset<f32>, to: Offset<f32>, colors: Vec<Color>) -> Self {
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
    /// use flui_types::painting::Shader;
    /// use flui_types::geometry::Offset;
    /// use flui_types::styling::Color;
    ///
    /// let shader = Shader::simple_radial(
    ///     Offset::new(50.0, 50.0),
    ///     25.0,
    ///     vec![Color::RED, Color::BLUE],
    /// );
    /// ```
    #[inline]
    #[must_use]
    pub fn simple_radial(center: Offset<f32>, radius: f32, colors: Vec<Color>) -> Self {
        Self::radial_gradient(center, radius, colors, None, TileMode::Clamp, None, None)
    }

    /// Creates a simple sweep (conic) gradient with default settings.
    ///
    /// This is a convenience method that creates a full 360° sweep gradient with:
    /// - No color stops (colors evenly distributed)
    /// - Full rotation (0 to 2π radians)
    /// - TileMode::Clamp (no repeating)
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::painting::Shader;
    /// use flui_types::geometry::Offset;
    /// use flui_types::styling::Color;
    ///
    /// let shader = Shader::simple_sweep(
    ///     Offset::new(50.0, 50.0),
    ///     vec![Color::RED, Color::GREEN, Color::BLUE],
    /// );
    /// ```
    #[inline]
    #[must_use]
    pub fn simple_sweep(center: Offset<f32>, colors: Vec<Color>) -> Self {
        Self::sweep_gradient(
            center,
            colors,
            None,
            TileMode::Clamp,
            0.0,
            std::f32::consts::TAU,
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
            Shader::Image(_) => 0,
        }
    }

    /// Returns true if this shader uses color stops.
    #[inline]
    #[must_use]
    pub fn has_stops(&self) -> bool {
        match self {
            Shader::LinearGradient { stops, .. }
            | Shader::RadialGradient { stops, .. }
            | Shader::SweepGradient { stops, .. } => stops.is_some(),
            Shader::Image(_) => false,
        }
    }
}

/// A shader that tiles an image.
///
/// Similar to Flutter's `ImageShader`.
///
/// # Examples
///
/// ```
/// use flui_types::painting::{ImageShader, TileMode};
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
    pub transform: Option<[[f32; 3]; 3]>,

    /// Optional filter quality.
    pub filter_quality: Option<crate::painting::FilterQuality>,
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
    pub const fn with_transform(mut self, transform: [[f32; 3]; 3]) -> Self {
        self.transform = Some(transform);
        self
    }

    /// Creates a new image shader with filter quality.
    #[inline]
    #[must_use]
    pub const fn with_filter_quality(mut self, quality: crate::painting::FilterQuality) -> Self {
        self.filter_quality = Some(quality);
        self
    }

    /// Returns true if this shader has a transformation.
    #[inline]
    #[must_use]
    pub const fn has_transform(&self) -> bool {
        self.transform.is_some()
    }

    /// Returns the effective filter quality (defaults to Low).
    #[inline]
    #[must_use]
    pub const fn effective_filter_quality(&self) -> crate::painting::FilterQuality {
        match self.filter_quality {
            Some(quality) => quality,
            None => crate::painting::FilterQuality::Low,
        }
    }
}

/// A mask filter to apply to a shape or image.
///
/// Similar to Flutter's `MaskFilter`.
///
/// # Examples
///
/// ```
/// use flui_types::painting::{MaskFilter, BlurStyle};
///
/// let filter = MaskFilter::blur(BlurStyle::Normal, 5.0);
/// assert_eq!(filter.sigma, 5.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MaskFilter {
    /// The style of blur to apply.
    pub style: BlurStyle,

    /// The standard deviation of the Gaussian blur.
    ///
    /// This is the blur radius in logical pixels.
    pub sigma: f32,
}

impl MaskFilter {
    /// Creates a new blur mask filter.
    #[inline]
    #[must_use]
    pub const fn blur(style: BlurStyle, sigma: f32) -> Self {
        Self { style, sigma }
    }

    /// Creates a normal blur with the given sigma.
    #[inline]
    #[must_use]
    pub const fn normal(sigma: f32) -> Self {
        Self::blur(BlurStyle::Normal, sigma)
    }

    /// Creates a solid blur with the given sigma.
    #[inline]
    #[must_use]
    pub const fn solid(sigma: f32) -> Self {
        Self::blur(BlurStyle::Solid, sigma)
    }

    /// Creates an outer blur with the given sigma.
    #[inline]
    #[must_use]
    pub const fn outer(sigma: f32) -> Self {
        Self::blur(BlurStyle::Outer, sigma)
    }

    /// Creates an inner blur with the given sigma.
    #[inline]
    #[must_use]
    pub const fn inner(sigma: f32) -> Self {
        Self::blur(BlurStyle::Inner, sigma)
    }

    /// Returns the blur radius (approximately 2 * sigma).
    #[inline]
    #[must_use]
    pub const fn blur_radius(&self) -> f32 {
        self.sigma * 2.0
    }

    /// Returns true if this filter affects the interior of shapes.
    #[inline]
    #[must_use]
    pub const fn affects_interior(&self) -> bool {
        matches!(self.style, BlurStyle::Normal | BlurStyle::Inner)
    }

    /// Returns true if this filter affects the exterior of shapes.
    #[inline]
    #[must_use]
    pub const fn affects_exterior(&self) -> bool {
        matches!(
            self.style,
            BlurStyle::Normal | BlurStyle::Outer | BlurStyle::Solid
        )
    }
}

/// Shader specification for shader mask effects
///
/// Defines the type of shader to apply as a mask using relative coordinates (0.0-1.0).
/// This is a unified type used across flui_rendering, flui_painting, and flui_engine.
///
/// # Coordinate System
///
/// All positions and sizes are specified as relative values (0.0-1.0) that get
/// converted to absolute coordinates based on the target region size.
///
/// # Examples
///
/// ```
/// use flui_types::painting::ShaderSpec;
/// use flui_types::styling::Color32;
///
/// // Linear gradient from left to right
/// let gradient = ShaderSpec::LinearGradient {
///     start: (0.0, 0.0),
///     end: (1.0, 0.0),
///     colors: vec![Color32::WHITE, Color32::BLACK],
/// };
///
/// // Radial gradient from center
/// let radial = ShaderSpec::RadialGradient {
///     center: (0.5, 0.5),
///     radius: 0.5,
///     colors: vec![Color32::RED, Color32::TRANSPARENT],
/// };
/// ```
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum ShaderSpec {
    /// Linear gradient shader
    LinearGradient {
        /// Start point (relative to size, 0.0-1.0)
        start: (f32, f32),
        /// End point (relative to size, 0.0-1.0)
        end: (f32, f32),
        /// Colors (at least 2 recommended)
        colors: Vec<Color32>,
    },
    /// Radial gradient shader
    RadialGradient {
        /// Center point (relative to size, 0.0-1.0)
        center: (f32, f32),
        /// Radius (relative to size, 0.0-1.0)
        radius: f32,
        /// Colors (at least 2 recommended)
        colors: Vec<Color32>,
    },
    /// Solid color (for testing and simple masks)
    Solid(Color32),
}

impl ShaderSpec {
    /// Convert ShaderSpec to Shader with absolute coordinates
    ///
    /// Converts relative coordinates (0.0-1.0) to absolute offsets based on the size.
    ///
    /// # Arguments
    ///
    /// * `size` - Size of the region for absolute positioning
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::painting::ShaderSpec;
    /// use flui_types::styling::Color32;
    /// use flui_types::Size;
    ///
    /// let spec = ShaderSpec::LinearGradient {
    ///     start: (0.0, 0.0),
    ///     end: (1.0, 0.0),
    ///     colors: vec![Color32::WHITE, Color32::BLACK],
    /// };
    /// let shader = spec.to_shader(Size::new(100.0, 100.0));
    /// ```
    #[must_use]
    pub fn to_shader<T>(&self, size: Size<T>) -> Shader
    where
        T: NumericUnit + Into<f32>,
    {
        match self {
            ShaderSpec::LinearGradient { start, end, colors } => {
                // Convert relative positions to absolute offsets
                let from = Offset::new(start.0 * size.width.into(), start.1 * size.height.into());
                let to = Offset::new(end.0 * size.width.into(), end.1 * size.height.into());

                // Convert Color32 to Color
                let converted_colors: Vec<Color> = colors
                    .iter()
                    .map(|c| Color::rgba(c.r(), c.g(), c.b(), c.a()))
                    .collect();

                Shader::simple_linear(from, to, converted_colors)
            }
            ShaderSpec::RadialGradient {
                center,
                radius,
                colors,
            } => {
                // Convert relative position to absolute offset
                let center_offset = Offset::new(center.0 * size.width.into(), center.1 * size.height.into());

                // Convert relative radius to absolute (use average of width/height for circular radius)
                let absolute_radius = *radius * (size.width.into() + size.height.into()) / 2.0;

                // Convert Color32 to Color
                let converted_colors: Vec<Color> = colors
                    .iter()
                    .map(|c| Color::rgba(c.r(), c.g(), c.b(), c.a()))
                    .collect();

                Shader::simple_radial(center_offset, absolute_radius, converted_colors)
            }
            ShaderSpec::Solid(color) => {
                // For solid color, create a simple linear gradient with same color
                let c = Color::rgba(color.r(), color.g(), color.b(), color.a());
                Shader::simple_linear(Offset::ZERO, Offset::new(size.width.into(), 0.0), vec![c, c])
            }
        }
    }

    /// Convert shader spec to uniform data buffer for GPU
    ///
    /// Returns a byte array containing the shader uniform data in the format
    /// expected by the GPU shader.
    ///
    /// # Uniform Layout
    ///
    /// Different shader types have different uniform layouts:
    ///
    /// **LinearGradient:**
    /// - `vec2<f32>` start (8 bytes)
    /// - `vec2<f32>` end (8 bytes)
    /// - `vec4<f32>` color0 (16 bytes)
    /// - `vec4<f32>` color1 (16 bytes)
    ///
    /// **RadialGradient:**
    /// - `vec2<f32>` center (8 bytes)
    /// - `f32` radius (4 bytes)
    /// - `f32` padding (4 bytes)
    /// - `vec4<f32>` color0 (16 bytes)
    /// - `vec4<f32>` color1 (16 bytes)
    ///
    /// **Solid:**
    /// - `vec4<f32>` color (16 bytes)
    #[must_use]
    pub fn to_uniform_data(&self) -> Vec<u8> {
        match self {
            ShaderSpec::LinearGradient { start, end, colors } => {
                let mut data = Vec::with_capacity(48);

                // Start point
                data.extend_from_slice(&start.0.to_le_bytes());
                data.extend_from_slice(&start.1.to_le_bytes());

                // End point
                data.extend_from_slice(&end.0.to_le_bytes());
                data.extend_from_slice(&end.1.to_le_bytes());

                // Color 0 (RGBA normalized to 0.0-1.0)
                let c0 = colors.first().copied().unwrap_or(Color32::BLACK);
                data.extend_from_slice(&(c0.r() as f32 / 255.0).to_le_bytes());
                data.extend_from_slice(&(c0.g() as f32 / 255.0).to_le_bytes());
                data.extend_from_slice(&(c0.b() as f32 / 255.0).to_le_bytes());
                data.extend_from_slice(&(c0.a() as f32 / 255.0).to_le_bytes());

                // Color 1
                let c1 = colors.get(1).copied().unwrap_or(c0);
                data.extend_from_slice(&(c1.r() as f32 / 255.0).to_le_bytes());
                data.extend_from_slice(&(c1.g() as f32 / 255.0).to_le_bytes());
                data.extend_from_slice(&(c1.b() as f32 / 255.0).to_le_bytes());
                data.extend_from_slice(&(c1.a() as f32 / 255.0).to_le_bytes());

                data
            }
            ShaderSpec::RadialGradient {
                center,
                radius,
                colors,
            } => {
                let mut data = Vec::with_capacity(48);

                // Center point
                data.extend_from_slice(&center.0.to_le_bytes());
                data.extend_from_slice(&center.1.to_le_bytes());

                // Radius + padding
                data.extend_from_slice(&radius.to_le_bytes());
                data.extend_from_slice(&0.0f32.to_le_bytes()); // padding

                // Color 0
                let c0 = colors.first().copied().unwrap_or(Color32::BLACK);
                data.extend_from_slice(&(c0.r() as f32 / 255.0).to_le_bytes());
                data.extend_from_slice(&(c0.g() as f32 / 255.0).to_le_bytes());
                data.extend_from_slice(&(c0.b() as f32 / 255.0).to_le_bytes());
                data.extend_from_slice(&(c0.a() as f32 / 255.0).to_le_bytes());

                // Color 1
                let c1 = colors.get(1).copied().unwrap_or(c0);
                data.extend_from_slice(&(c1.r() as f32 / 255.0).to_le_bytes());
                data.extend_from_slice(&(c1.g() as f32 / 255.0).to_le_bytes());
                data.extend_from_slice(&(c1.b() as f32 / 255.0).to_le_bytes());
                data.extend_from_slice(&(c1.a() as f32 / 255.0).to_le_bytes());

                data
            }
            ShaderSpec::Solid(color) => {
                let mut data = Vec::with_capacity(16);

                // Single color (RGBA normalized)
                data.extend_from_slice(&(color.r() as f32 / 255.0).to_le_bytes());
                data.extend_from_slice(&(color.g() as f32 / 255.0).to_le_bytes());
                data.extend_from_slice(&(color.b() as f32 / 255.0).to_le_bytes());
                data.extend_from_slice(&(color.a() as f32 / 255.0).to_le_bytes());

                data
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shader_linear_gradient() {
        let shader = Shader::linear_gradient(
            Offset::ZERO,
            Offset::new(100.0, 100.0),
            vec![Color::RED, Color::BLUE],
            None,
            TileMode::Clamp,
        );

        match shader {
            Shader::LinearGradient {
                from, to, colors, ..
            } => {
                assert_eq!(from, Offset::ZERO);
                assert_eq!(to, Offset::new(100.0, 100.0));
                assert_eq!(colors.len(), 2);
            }
            _ => panic!("Wrong shader type"),
        }
    }

    #[test]
    fn test_shader_radial_gradient() {
        let shader = Shader::radial_gradient(
            Offset::new(50.0, 50.0),
            25.0,
            vec![Color::RED, Color::BLUE],
            None,
            TileMode::Clamp,
            None,
            None,
        );

        match shader {
            Shader::RadialGradient {
                center,
                radius,
                colors,
                ..
            } => {
                assert_eq!(center, Offset::new(50.0, 50.0));
                assert_eq!(radius, 25.0);
                assert_eq!(colors.len(), 2);
            }
            _ => panic!("Wrong shader type"),
        }
    }

    #[test]
    fn test_shader_sweep_gradient() {
        let shader = Shader::sweep_gradient(
            Offset::new(50.0, 50.0),
            vec![Color::RED, Color::BLUE],
            None,
            TileMode::Clamp,
            0.0,
            std::f32::consts::TAU,
        );

        match shader {
            Shader::SweepGradient {
                center,
                colors,
                start_angle,
                end_angle,
                ..
            } => {
                assert_eq!(center, Offset::new(50.0, 50.0));
                assert_eq!(colors.len(), 2);
                assert_eq!(start_angle, 0.0);
                assert_eq!(end_angle, std::f32::consts::TAU);
            }
            _ => panic!("Wrong shader type"),
        }
    }

    #[test]
    fn test_image_shader_new() {
        let shader = ImageShader::new(TileMode::Repeat, TileMode::Mirror);

        assert_eq!(shader.tile_mode_x, TileMode::Repeat);
        assert_eq!(shader.tile_mode_y, TileMode::Mirror);
        assert!(shader.transform.is_none());
        assert!(shader.filter_quality.is_none());
    }

    #[test]
    fn test_image_shader_with_transform() {
        let transform = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let shader = ImageShader::new(TileMode::Repeat, TileMode::Repeat).with_transform(transform);

        assert_eq!(shader.transform, Some(transform));
    }

    #[test]
    fn test_image_shader_with_filter_quality() {
        let shader = ImageShader::new(TileMode::Repeat, TileMode::Repeat)
            .with_filter_quality(crate::painting::FilterQuality::High);

        assert_eq!(
            shader.filter_quality,
            Some(crate::painting::FilterQuality::High)
        );
    }

    #[test]
    fn test_mask_filter_blur() {
        let filter = MaskFilter::blur(BlurStyle::Normal, 5.0);

        assert_eq!(filter.style, BlurStyle::Normal);
        assert_eq!(filter.sigma, 5.0);
    }

    #[test]
    fn test_mask_filter_normal() {
        let filter = MaskFilter::normal(3.0);

        assert_eq!(filter.style, BlurStyle::Normal);
        assert_eq!(filter.sigma, 3.0);
    }

    #[test]
    fn test_mask_filter_solid() {
        let filter = MaskFilter::solid(4.0);

        assert_eq!(filter.style, BlurStyle::Solid);
        assert_eq!(filter.sigma, 4.0);
    }

    #[test]
    fn test_mask_filter_outer() {
        let filter = MaskFilter::outer(2.0);

        assert_eq!(filter.style, BlurStyle::Outer);
        assert_eq!(filter.sigma, 2.0);
    }

    #[test]
    fn test_mask_filter_inner() {
        let filter = MaskFilter::inner(6.0);

        assert_eq!(filter.style, BlurStyle::Inner);
        assert_eq!(filter.sigma, 6.0);
    }
}
