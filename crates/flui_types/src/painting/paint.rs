//! Paint and painting styles for rendering.
//!
//! This module provides the `Paint` type and related styling information
//! for controlling how shapes and paths are rendered.

use crate::painting::{BlendMode, Shader, StrokeCap, StrokeJoin};
use crate::styling::Color;

/// Description of how to paint on a canvas
///
/// Contains color, style (fill/stroke), stroke width, blend mode, etc.
/// This is the painting equivalent of CSS styles.
///
/// # Examples
///
/// ```
/// use flui_types::painting::{Paint, PaintStyle};
/// use flui_types::styling::Color;
///
/// // Fill paint
/// let fill = Paint::fill(Color::RED);
///
/// // Stroke paint
/// let stroke = Paint::stroke(Color::BLUE, 2.0);
///
/// // Custom paint with builder
/// let custom = Paint::builder()
///     .color(Color::GREEN)
///     .style(PaintStyle::Stroke)
///     .stroke_width(3.0)
///     .build();
/// ```
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Paint {
    /// Paint style (fill or stroke)
    pub style: PaintStyle,

    /// Color (RGBA)
    pub color: Color,

    /// Stroke width (only used for stroke style)
    pub stroke_width: f32,

    /// Stroke cap style
    pub stroke_cap: StrokeCap,

    /// Stroke join style
    pub stroke_join: StrokeJoin,

    /// Blend mode
    pub blend_mode: BlendMode,

    /// Anti-aliasing enabled
    pub anti_alias: bool,

    /// Optional shader (gradient, image pattern, etc.)
    pub shader: Option<Shader>,
}

impl Paint {
    /// Creates a fill paint with the given color
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::painting::Paint;
    /// use flui_types::styling::Color;
    ///
    /// let paint = Paint::fill(Color::RED);
    /// assert_eq!(paint.color, Color::RED);
    /// ```
    #[inline]
    #[must_use]
    pub const fn fill(color: Color) -> Self {
        Self {
            style: PaintStyle::Fill,
            color,
            stroke_width: 0.0,
            stroke_cap: StrokeCap::Butt,
            stroke_join: StrokeJoin::Miter,
            blend_mode: BlendMode::SrcOver,
            anti_alias: true,
            shader: None,
        }
    }

    /// Creates a stroke paint with the given color and width
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::painting::Paint;
    /// use flui_types::styling::Color;
    ///
    /// let paint = Paint::stroke(Color::BLUE, 2.0);
    /// assert_eq!(paint.color, Color::BLUE);
    /// assert_eq!(paint.stroke_width, 2.0);
    /// ```
    ///
    /// # Panics
    ///
    /// In debug builds, panics if `width` is negative or NaN.
    #[inline]
    #[must_use]
    pub fn stroke(color: Color, width: f32) -> Self {
        debug_assert!(
            width >= 0.0 && !width.is_nan(),
            "Stroke width must be non-negative and not NaN, got: {}",
            width
        );
        Self {
            style: PaintStyle::Stroke,
            color,
            stroke_width: width,
            stroke_cap: StrokeCap::Butt,
            stroke_join: StrokeJoin::Miter,
            blend_mode: BlendMode::SrcOver,
            anti_alias: true,
            shader: None,
        }
    }

    /// Creates a new paint builder
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::painting::{Paint, PaintStyle};
    /// use flui_types::styling::Color;
    ///
    /// let paint = Paint::builder()
    ///     .color(Color::GREEN)
    ///     .style(PaintStyle::Stroke)
    ///     .stroke_width(3.0)
    ///     .build();
    /// ```
    #[inline]
    #[must_use]
    pub const fn builder() -> PaintBuilder {
        PaintBuilder::new()
    }

    /// Sets the paint style (fill or stroke)
    #[inline]
    #[must_use]
    pub const fn with_style(mut self, style: PaintStyle) -> Self {
        self.style = style;
        self
    }

    /// Sets the color
    #[inline]
    #[must_use]
    pub const fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Sets the stroke width.
    ///
    /// # Panics
    ///
    /// In debug builds, panics if `width` is negative or NaN.
    #[inline]
    #[must_use]
    pub fn with_stroke_width(mut self, width: f32) -> Self {
        debug_assert!(
            width >= 0.0 && !width.is_nan(),
            "Stroke width must be non-negative and not NaN, got: {}",
            width
        );
        self.stroke_width = width;
        self
    }

    /// Sets the stroke cap
    #[inline]
    #[must_use]
    pub const fn with_stroke_cap(mut self, cap: StrokeCap) -> Self {
        self.stroke_cap = cap;
        self
    }

    /// Sets the stroke join
    #[inline]
    #[must_use]
    pub const fn with_stroke_join(mut self, join: StrokeJoin) -> Self {
        self.stroke_join = join;
        self
    }

    /// Sets the blend mode
    #[inline]
    #[must_use]
    pub const fn with_blend_mode(mut self, blend_mode: BlendMode) -> Self {
        self.blend_mode = blend_mode;
        self
    }

    /// Sets anti-aliasing
    #[inline]
    #[must_use]
    pub const fn with_anti_alias(mut self, aa: bool) -> Self {
        self.anti_alias = aa;
        self
    }

    /// Sets the shader
    #[inline]
    #[must_use]
    pub fn with_shader(mut self, shader: Shader) -> Self {
        self.shader = Some(shader);
        self
    }

    /// Returns true if this paint uses fill style
    #[inline]
    #[must_use]
    pub const fn is_fill(&self) -> bool {
        matches!(self.style, PaintStyle::Fill)
    }

    /// Returns true if this paint uses stroke style
    #[inline]
    #[must_use]
    pub const fn is_stroke(&self) -> bool {
        matches!(self.style, PaintStyle::Stroke)
    }

    /// Returns true if this paint has a shader
    #[inline]
    #[must_use]
    pub const fn has_shader(&self) -> bool {
        self.shader.is_some()
    }

    /// Returns true if anti-aliasing is enabled
    #[inline]
    #[must_use]
    pub const fn is_anti_aliased(&self) -> bool {
        self.anti_alias
    }

    /// Returns the effective stroke width (0.0 for fill style)
    #[inline]
    #[must_use]
    pub const fn effective_stroke_width(&self) -> f32 {
        match self.style {
            PaintStyle::Stroke => self.stroke_width,
            PaintStyle::Fill => 0.0,
        }
    }

    /// Returns true if this paint is fully opaque
    #[inline]
    #[must_use]
    pub const fn is_opaque(&self) -> bool {
        self.color.a == 255 && matches!(self.blend_mode, BlendMode::SrcOver | BlendMode::Src)
    }

    /// Returns true if this paint is fully transparent
    #[inline]
    #[must_use]
    pub const fn is_transparent(&self) -> bool {
        self.color.a == 0
    }

    /// Creates a copy with modified alpha
    #[inline]
    #[must_use]
    pub fn with_alpha(mut self, alpha: u8) -> Self {
        self.color = self.color.with_alpha(alpha);
        self
    }

    /// Creates a copy with modified opacity (0.0 to 1.0)
    #[inline]
    #[must_use]
    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.color = self.color.with_opacity(opacity);
        self
    }
}

impl Default for Paint {
    fn default() -> Self {
        Self::fill(Color::BLACK)
    }
}

/// Paint style (fill or stroke)
///
/// # Examples
///
/// ```
/// use flui_types::painting::PaintStyle;
///
/// let fill = PaintStyle::Fill;
/// let stroke = PaintStyle::Stroke;
///
/// assert_eq!(fill, PaintStyle::Fill);
/// assert_eq!(stroke, PaintStyle::Stroke);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PaintStyle {
    /// Fill the shape
    #[default]
    Fill,
    /// Stroke the outline
    Stroke,
}

impl PaintStyle {
    /// Returns true if this is fill style
    #[inline]
    #[must_use]
    pub const fn is_fill(&self) -> bool {
        matches!(self, PaintStyle::Fill)
    }

    /// Returns true if this is stroke style
    #[inline]
    #[must_use]
    pub const fn is_stroke(&self) -> bool {
        matches!(self, PaintStyle::Stroke)
    }
}

/// Builder for Paint
///
/// Provides a fluent API for constructing Paint objects with custom settings.
///
/// # Examples
///
/// ```
/// use flui_types::painting::{PaintBuilder, PaintStyle, StrokeCap};
/// use flui_types::styling::Color;
///
/// let paint = PaintBuilder::new()
///     .color(Color::GREEN)
///     .style(PaintStyle::Stroke)
///     .stroke_width(3.0)
///     .stroke_cap(StrokeCap::Round)
///     .build();
/// ```
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PaintBuilder {
    paint: Paint,
}

impl PaintBuilder {
    /// Creates a new paint builder with default values
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            paint: Paint::fill(Color::BLACK),
        }
    }

    /// Sets the paint style
    #[inline]
    #[must_use]
    pub const fn style(mut self, style: PaintStyle) -> Self {
        self.paint.style = style;
        self
    }

    /// Sets the color
    #[inline]
    #[must_use]
    pub const fn color(mut self, color: Color) -> Self {
        self.paint.color = color;
        self
    }

    /// Sets the stroke width
    #[inline]
    #[must_use]
    pub const fn stroke_width(mut self, width: f32) -> Self {
        self.paint.stroke_width = width;
        self
    }

    /// Sets the stroke cap
    #[inline]
    #[must_use]
    pub const fn stroke_cap(mut self, cap: StrokeCap) -> Self {
        self.paint.stroke_cap = cap;
        self
    }

    /// Sets the stroke join
    #[inline]
    #[must_use]
    pub const fn stroke_join(mut self, join: StrokeJoin) -> Self {
        self.paint.stroke_join = join;
        self
    }

    /// Sets the blend mode
    #[inline]
    #[must_use]
    pub const fn blend_mode(mut self, blend_mode: BlendMode) -> Self {
        self.paint.blend_mode = blend_mode;
        self
    }

    /// Sets anti-aliasing
    #[inline]
    #[must_use]
    pub const fn anti_alias(mut self, aa: bool) -> Self {
        self.paint.anti_alias = aa;
        self
    }

    /// Sets the shader
    #[inline]
    #[must_use]
    pub fn shader(mut self, shader: Shader) -> Self {
        self.paint.shader = Some(shader);
        self
    }

    /// Builds the Paint
    #[inline]
    #[must_use]
    pub fn build(self) -> Paint {
        self.paint
    }
}

impl Default for PaintBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paint_fill() {
        let paint = Paint::fill(Color::RED);
        assert_eq!(paint.style, PaintStyle::Fill);
        assert_eq!(paint.color, Color::RED);
        assert!(paint.is_fill());
        assert!(!paint.is_stroke());
    }

    #[test]
    fn test_paint_stroke() {
        let paint = Paint::stroke(Color::BLUE, 2.0);
        assert_eq!(paint.style, PaintStyle::Stroke);
        assert_eq!(paint.color, Color::BLUE);
        assert_eq!(paint.stroke_width, 2.0);
        assert!(paint.is_stroke());
        assert!(!paint.is_fill());
    }

    #[test]
    fn test_paint_builder() {
        let paint = Paint::builder()
            .color(Color::GREEN)
            .style(PaintStyle::Stroke)
            .stroke_width(3.0)
            .stroke_cap(StrokeCap::Round)
            .build();

        assert_eq!(paint.color, Color::GREEN);
        assert_eq!(paint.style, PaintStyle::Stroke);
        assert_eq!(paint.stroke_width, 3.0);
        assert_eq!(paint.stroke_cap, StrokeCap::Round);
    }

    #[test]
    fn test_paint_with_methods() {
        let paint = Paint::fill(Color::RED)
            .with_stroke_width(5.0)
            .with_stroke_cap(StrokeCap::Square);

        assert_eq!(paint.stroke_width, 5.0);
        assert_eq!(paint.stroke_cap, StrokeCap::Square);
    }

    #[test]
    fn test_paint_effective_stroke_width() {
        let fill = Paint::fill(Color::RED);
        assert_eq!(fill.effective_stroke_width(), 0.0);

        let stroke = Paint::stroke(Color::BLUE, 3.0);
        assert_eq!(stroke.effective_stroke_width(), 3.0);
    }

    #[test]
    fn test_paint_opacity() {
        let paint = Paint::fill(Color::RED);
        assert!(paint.is_opaque());
        assert!(!paint.is_transparent());

        let transparent = Paint::fill(Color::TRANSPARENT);
        assert!(!transparent.is_opaque());
        assert!(transparent.is_transparent());
    }

    #[test]
    fn test_paint_with_alpha() {
        let paint = Paint::fill(Color::RED).with_alpha(128);
        assert_eq!(paint.color.a, 128);
    }

    #[test]
    fn test_paint_with_opacity() {
        let paint = Paint::fill(Color::RED).with_opacity(0.5);
        assert_eq!(paint.color.a, 127); // 0.5 * 255 ≈ 127
    }

    #[test]
    fn test_paint_style_methods() {
        let fill = PaintStyle::Fill;
        assert!(fill.is_fill());
        assert!(!fill.is_stroke());

        let stroke = PaintStyle::Stroke;
        assert!(!stroke.is_fill());
        assert!(stroke.is_stroke());
    }

    #[test]
    fn test_paint_has_shader() {
        let paint = Paint::fill(Color::RED);
        assert!(!paint.has_shader());

        let shader = Shader::simple_linear(
            crate::geometry::Offset::ZERO,
            crate::geometry::Offset::new(100.0, 0.0),
            vec![Color::RED, Color::BLUE],
        );
        let paint_with_shader = paint.with_shader(shader);
        assert!(paint_with_shader.has_shader());
    }
}
