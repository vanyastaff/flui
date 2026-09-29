//! Shader types for painting.

use crate::{
    paint::{BlurStyle, TileMode},
    styling::Color,
};
use flui_foundation::geometry::Offset;

/// A shader (or gradient) to use when filling a shape.
///
/// Similar to Flutter's `Shader`. This is a placeholder type that will be
/// implemented more fully when we have actual rendering capabilities.
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

    /// Returns true if this shader uses color stops.
    #[inline]
    #[must_use]
    pub fn has_stops(&self) -> bool {
        match self {
            Shader::LinearGradient { stops, .. }
            | Shader::RadialGradient { stops, .. }
            | Shader::SweepGradient { stops, .. } => stops.is_some(),
            Shader::Solid { .. } | Shader::Image(_) => false,
        }
    }

    /// Convert to GPU-ready uniform bytes for shader mask rendering.
    ///
    /// For gradient shaders, coordinates are normalized relative to `bounds`
    /// to produce 0.0-1.0 relative values. For solid shaders, bounds is ignored.
    ///
    /// # Uniform Layout
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
    #[inline]
    pub fn to_mask_uniform_data(&self, bounds: flui_foundation::geometry::Rect<f64>) -> Vec<u8> {
        fn color_to_f32x4(c: &Color) -> [f32; 4] {
            [
                f32::from(c.r) / 255.0,
                f32::from(c.g) / 255.0,
                f32::from(c.b) / 255.0,
                f32::from(c.a) / 255.0,
            ]
        }

        match self {
            Shader::LinearGradient {
                from, to, colors, ..
            } => {
                let mut data = Vec::with_capacity(48);
                let w: f64 = bounds.width();
                let h: f64 = bounds.height();
                let bx: f64 = bounds.left();
                let by: f64 = bounds.top();

                // Normalize to 0.0-1.0 relative to bounds
                let sx = if w > 0.0 { (from.dx - bx) / w } else { 0.0 };
                let sy = if h > 0.0 { (from.dy - by) / h } else { 0.0 };
                let ex = if w > 0.0 { (to.dx - bx) / w } else { 0.0 };
                let ey = if h > 0.0 { (to.dy - by) / h } else { 0.0 };

                data.extend_from_slice(&(sx as f32).to_le_bytes());
                data.extend_from_slice(&(sy as f32).to_le_bytes());
                data.extend_from_slice(&(ex as f32).to_le_bytes());
                data.extend_from_slice(&(ey as f32).to_le_bytes());

                let c0 = colors.first().map_or([0.0, 0.0, 0.0, 1.0], color_to_f32x4);
                for v in &c0 {
                    data.extend_from_slice(&v.to_le_bytes());
                }

                let c1 = colors.get(1).map_or(c0, color_to_f32x4);
                for v in &c1 {
                    data.extend_from_slice(&v.to_le_bytes());
                }

                data
            }
            Shader::RadialGradient {
                center,
                radius,
                colors,
                ..
            } => {
                let mut data = Vec::with_capacity(48);
                let w: f64 = bounds.width();
                let h: f64 = bounds.height();
                let bx: f64 = bounds.left();
                let by: f64 = bounds.top();

                let cx = if w > 0.0 { (center.dx - bx) / w } else { 0.5 };
                let cy = if h > 0.0 { (center.dy - by) / h } else { 0.5 };
                // Normalize radius relative to average of width/height
                let avg = f64::midpoint(w, h);
                let nr = if avg > 0.0 { *radius / avg } else { 0.5 };

                data.extend_from_slice(&(cx as f32).to_le_bytes());
                data.extend_from_slice(&(cy as f32).to_le_bytes());
                data.extend_from_slice(&(nr as f32).to_le_bytes());
                data.extend_from_slice(&0.0_f32.to_le_bytes()); // padding

                let c0 = colors.first().map_or([0.0, 0.0, 0.0, 1.0], color_to_f32x4);
                for v in &c0 {
                    data.extend_from_slice(&v.to_le_bytes());
                }

                let c1 = colors.get(1).map_or(c0, color_to_f32x4);
                for v in &c1 {
                    data.extend_from_slice(&v.to_le_bytes());
                }

                data
            }
            Shader::Solid { color } => {
                let mut data = Vec::with_capacity(16);
                let c = color_to_f32x4(color);
                for v in &c {
                    data.extend_from_slice(&v.to_le_bytes());
                }
                data
            }
            // SweepGradient: 48-byte layout matching sweep_gradient.wgsl Uniforms:
            //   offset  0: center     vec2<f32>  (8 bytes)
            //   offset  8: start_angle f32       (4 bytes)
            //   offset 12: end_angle f32       (4 bytes)
            //   offset 16: start_color vec4<f32> (16 bytes)
            //   offset 32: end_color   vec4<f32> (16 bytes)
            Shader::SweepGradient {
                center,
                colors,
                start_angle,
                end_angle,
                ..
            } => {
                let mut data = Vec::with_capacity(48);
                let w: f64 = bounds.width();
                let h: f64 = bounds.height();
                let bx: f64 = bounds.left();
                let by: f64 = bounds.top();

                // Normalize center to 0.0-1.0 relative to bounds (mirrors the
                // radial gradient arm).
                let cx = if w > 0.0 { (center.dx - bx) / w } else { 0.5 };
                let cy = if h > 0.0 { (center.dy - by) / h } else { 0.5 };

                data.extend_from_slice(&(cx as f32).to_le_bytes()); // center.x
                data.extend_from_slice(&(cy as f32).to_le_bytes()); // center.y
                data.extend_from_slice(&(*start_angle as f32).to_le_bytes()); // start_angle
                data.extend_from_slice(&(*end_angle as f32).to_le_bytes()); // end_angle

                let c0 = colors.first().map_or([0.0, 0.0, 0.0, 1.0], color_to_f32x4);
                for v in &c0 {
                    data.extend_from_slice(&v.to_le_bytes());
                }

                let c1 = colors.get(1).map_or(c0, color_to_f32x4);
                for v in &c1 {
                    data.extend_from_slice(&v.to_le_bytes());
                }

                data
            }
            // Image shaders fall back to opaque white (no mask effect)
            _ => {
                let mut data = Vec::with_capacity(16);
                for v in &[1.0_f32, 1.0, 1.0, 1.0] {
                    data.extend_from_slice(&v.to_le_bytes());
                }
                data
            }
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

    /// Returns the effective filter quality (defaults to Low).
    #[inline]
    #[must_use]
    pub const fn effective_filter_quality(&self) -> crate::paint::FilterQuality {
        match self.filter_quality {
            Some(quality) => quality,
            None => crate::paint::FilterQuality::Low,
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
/// use flui_painting::paint::{BlurStyle, MaskFilter};
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
    pub sigma: f64,
}

impl MaskFilter {
    /// Creates a new blur mask filter.
    #[inline]
    #[must_use]
    pub const fn blur(style: BlurStyle, sigma: f64) -> Self {
        Self { style, sigma }
    }

    /// Creates a normal blur with the given sigma.
    #[inline]
    #[must_use]
    pub const fn normal(sigma: f64) -> Self {
        Self::blur(BlurStyle::Normal, sigma)
    }

    /// Creates a solid blur with the given sigma.
    #[inline]
    #[must_use]
    pub const fn solid(sigma: f64) -> Self {
        Self::blur(BlurStyle::Solid, sigma)
    }

    /// Creates an outer blur with the given sigma.
    #[inline]
    #[must_use]
    pub const fn outer(sigma: f64) -> Self {
        Self::blur(BlurStyle::Outer, sigma)
    }

    /// Creates an inner blur with the given sigma.
    #[inline]
    #[must_use]
    pub const fn inner(sigma: f64) -> Self {
        Self::blur(BlurStyle::Inner, sigma)
    }

    /// Returns the blur radius (approximately 2 * sigma).
    #[inline]
    #[must_use]
    pub const fn blur_radius(&self) -> f64 {
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

#[cfg(test)]
mod tests {
    use super::*;

    // --- Regression tests for to_mask_uniform_data correctness ---

    mod uniforms {
        use super::super::*;
        use flui_foundation::geometry::Rect;

        fn floats(bytes: &[u8]) -> Vec<f32> {
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b))
                .collect()
        }

        fn off(x: f64, y: f64) -> Offset<f64> {
            Offset::new(x, y)
        }

        /// A 200×100 box at (100, 50).
        fn bounds() -> Rect<f64> {
            Rect::from_ltrb(100.0, 50.0, 300.0, 150.0)
        }

        const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
        const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

        /// Endpoints normalized into the box, then the first two colors.
        #[test]
        fn linear_layout() {
            let s = Shader::simple_linear(
                off(150.0, 75.0),
                off(250.0, 125.0),
                vec![Color::RED, Color::BLUE, Color::GREEN],
            );
            let v = floats(&s.to_mask_uniform_data(bounds()));
            assert_eq!(v[..4], [0.25, 0.25, 0.75, 0.75]);
            assert_eq!(v[4..8], RED);
            assert_eq!(v[8..12], BLUE);
        }

        /// Center normalized into the box, radius over the mean of width and
        /// height, then padding and the first two colors.
        #[test]
        fn radial_layout() {
            let s = Shader::simple_radial(off(200.0, 75.0), 75.0, vec![Color::RED, Color::BLUE]);
            let v = floats(&s.to_mask_uniform_data(bounds()));
            assert_eq!(v[..4], [0.5, 0.25, 0.5, 0.0]);
            assert_eq!(v[4..8], RED);
            assert_eq!(v[8..12], BLUE);
            // The simple form is a clamped gradient with no stops or focal.
            let full = Shader::radial_gradient(
                off(200.0, 75.0),
                75.0,
                vec![Color::RED, Color::BLUE],
                None,
                TileMode::Clamp,
                None,
                None,
            );
            assert_eq!(s, full);
        }

        /// A zero-size box falls back to 0 (linear) or the center (radial,
        /// sweep); one color is used for both ends; none is opaque black.
        #[test]
        fn degenerate_boxes_and_color_lists() {
            let empty = Rect::from_ltrb(10.0, 10.0, 10.0, 10.0);
            let linear = Shader::simple_linear(off(1.0, 2.0), off(3.0, 4.0), vec![Color::RED]);
            let v = floats(&linear.to_mask_uniform_data(empty));
            assert_eq!(v[..4], [0.0; 4]);
            assert_eq!((&v[4..8], &v[8..12]), (&RED[..], &RED[..]));

            let radial = Shader::simple_radial(off(1.0, 2.0), 3.0, vec![]);
            let v = floats(&radial.to_mask_uniform_data(empty));
            assert_eq!(v[..3], [0.5, 0.5, 0.5]);
            assert_eq!(
                (&v[4..8], &v[8..12]),
                (&[0.0, 0.0, 0.0, 1.0][..], &[0.0, 0.0, 0.0, 1.0][..])
            );

            let sweep = Shader::simple_sweep(off(1.0, 2.0), vec![Color::BLUE]);
            let v = floats(&sweep.to_mask_uniform_data(empty));
            assert_eq!(v[..4], [0.5, 0.5, 0.0, std::f32::consts::TAU]);
            assert_eq!(v[8..12], BLUE);
            // Only one axis collapsed still normalizes the other.
            let flat = Rect::from_ltrb(0.0, 0.0, 100.0, 0.0);
            let v = floats(
                &Shader::simple_linear(off(25.0, 9.0), off(75.0, 9.0), vec![])
                    .to_mask_uniform_data(flat),
            );
            assert_eq!(v[..4], [0.25, 0.0, 0.75, 0.0]);
            let v = floats(
                &Shader::simple_radial(off(25.0, 9.0), 10.0, vec![]).to_mask_uniform_data(flat),
            );
            assert_eq!(v[..3], [0.25, 0.5, 0.2]);
        }
    }
}
