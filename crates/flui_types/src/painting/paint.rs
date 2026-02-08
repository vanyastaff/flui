//! Paint and painting styles for rendering.
//!
//! This module provides the `Paint` type and related styling information
//! for controlling how shapes and paths are rendered.

use crate::painting::{BlendMode, Shader, StrokeCap, StrokeJoin};
use crate::styling::Color;

/// Paint style and properties for rendering shapes and paths.
///
/// Contains all the information needed to render a shape, including color,
/// stroke/fill style, blend mode, and optional shader (gradient, pattern).
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Paint {
    /// Paint style (fill or stroke).
    pub style: PaintStyle,

    /// Color (RGBA).
    pub color: Color,

    /// Stroke width (only used for stroke style).
    pub stroke_width: f32,

    /// Stroke cap style.
    pub stroke_cap: StrokeCap,

    /// Stroke join style.
    pub stroke_join: StrokeJoin,

    /// Blend mode.
    pub blend_mode: BlendMode,

    /// Anti-aliasing enabled.
    pub anti_alias: bool,

    /// Optional shader (gradient, image pattern, etc.).
    pub shader: Option<Shader>,
}

impl Paint {
    /// Creates a fill paint with the given color.
    #[must_use]
    #[inline]
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

    /// Creates a stroke paint with the given color and width.
    #[must_use]
    #[inline]
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

    /// Creates a paint builder for fluent construction.
    #[must_use]
    #[inline]
    pub const fn builder() -> PaintBuilder {
        PaintBuilder::new()
    }

    /// Sets the paint style.
    #[must_use]
    #[inline]
    pub const fn with_style(mut self, style: PaintStyle) -> Self {
        self.style = style;
        self
    }

    /// Sets the color.
    #[must_use]
    #[inline]
    pub const fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Sets the stroke width.
    #[must_use]
    #[inline]
    pub fn with_stroke_width(mut self, width: f32) -> Self {
        debug_assert!(
            width >= 0.0 && !width.is_nan(),
            "Stroke width must be non-negative and not NaN, got: {}",
            width
        );
        self.stroke_width = width;
        self
    }

    /// Sets the stroke cap style.
    #[must_use]
    #[inline]
    pub const fn with_stroke_cap(mut self, cap: StrokeCap) -> Self {
        self.stroke_cap = cap;
        self
    }

    /// Sets the stroke join style.
    #[must_use]
    #[inline]
    pub const fn with_stroke_join(mut self, join: StrokeJoin) -> Self {
        self.stroke_join = join;
        self
    }

    /// Sets the blend mode.
    #[must_use]
    #[inline]
    pub const fn with_blend_mode(mut self, blend_mode: BlendMode) -> Self {
        self.blend_mode = blend_mode;
        self
    }

    /// Sets anti-aliasing.
    #[must_use]
    #[inline]
    pub const fn with_anti_alias(mut self, aa: bool) -> Self {
        self.anti_alias = aa;
        self
    }

    /// Sets the shader.
    #[must_use]
    #[inline]
    pub fn with_shader(mut self, shader: Shader) -> Self {
        self.shader = Some(shader);
        self
    }

    /// Returns true if this is a fill paint.
    #[must_use]
    #[inline]
    pub const fn is_fill(&self) -> bool {
        matches!(self.style, PaintStyle::Fill)
    }

    /// Returns true if this is a stroke paint.
    #[must_use]
    #[inline]
    pub const fn is_stroke(&self) -> bool {
        matches!(self.style, PaintStyle::Stroke)
    }

    /// Returns true if a shader is set.
    #[must_use]
    #[inline]
    pub const fn has_shader(&self) -> bool {
        self.shader.is_some()
    }

    /// Returns true if anti-aliasing is enabled.
    #[must_use]
    #[inline]
    pub const fn is_anti_aliased(&self) -> bool {
        self.anti_alias
    }

    /// Returns the effective stroke width (0 for fill).
    #[must_use]
    #[inline]
    pub const fn effective_stroke_width(&self) -> f32 {
        match self.style {
            PaintStyle::Stroke => self.stroke_width,
            PaintStyle::Fill => 0.0,
        }
    }

    /// Returns true if the paint is fully opaque.
    #[must_use]
    #[inline]
    pub const fn is_opaque(&self) -> bool {
        self.color.a == 255 && matches!(self.blend_mode, BlendMode::SrcOver | BlendMode::Src)
    }

    /// Returns true if the paint is fully transparent.
    #[must_use]
    #[inline]
    pub const fn is_transparent(&self) -> bool {
        self.color.a == 0
    }

    /// Sets the alpha channel.
    #[must_use]
    #[inline]
    pub fn with_alpha(mut self, alpha: u8) -> Self {
        self.color = self.color.with_alpha(alpha);
        self
    }

    /// Sets the opacity (0.0 to 1.0).
    #[must_use]
    #[inline]
    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.color = self.color.with_opacity(opacity);
        self
    }
}

impl Default for Paint {
    #[inline]
    fn default() -> Self {
        Self::fill(Color::BLACK)
    }
}

/// Paint style: fill or stroke.
#[derive(Clone, Debug, Default, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PaintStyle {
    /// Fill the interior.
    #[default]
    Fill,
    /// Stroke the outline.
    Stroke,
}

impl PaintStyle {
    /// Returns true if this is a fill style.
    #[must_use]
    #[inline]
    pub const fn is_fill(&self) -> bool {
        matches!(self, PaintStyle::Fill)
    }

    /// Returns true if this is a stroke style.
    #[must_use]
    #[inline]
    pub const fn is_stroke(&self) -> bool {
        matches!(self, PaintStyle::Stroke)
    }
}

/// Builder for constructing `Paint` instances.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PaintBuilder {
    paint: Paint,
}

impl PaintBuilder {
    /// Creates a new paint builder with default values.
    #[must_use]
    #[inline]
    pub const fn new() -> Self {
        Self {
            paint: Paint::fill(Color::BLACK),
        }
    }

    /// Sets the paint style.
    #[must_use]
    #[inline]
    pub const fn style(mut self, style: PaintStyle) -> Self {
        self.paint.style = style;
        self
    }

    /// Sets the color.
    #[must_use]
    #[inline]
    pub const fn color(mut self, color: Color) -> Self {
        self.paint.color = color;
        self
    }

    /// Sets the stroke width.
    #[must_use]
    #[inline]
    pub const fn stroke_width(mut self, width: f32) -> Self {
        self.paint.stroke_width = width;
        self
    }

    /// Sets the stroke cap.
    #[must_use]
    #[inline]
    pub const fn stroke_cap(mut self, cap: StrokeCap) -> Self {
        self.paint.stroke_cap = cap;
        self
    }

    /// Sets the stroke join.
    #[must_use]
    #[inline]
    pub const fn stroke_join(mut self, join: StrokeJoin) -> Self {
        self.paint.stroke_join = join;
        self
    }

    /// Sets the blend mode.
    #[must_use]
    #[inline]
    pub const fn blend_mode(mut self, blend_mode: BlendMode) -> Self {
        self.paint.blend_mode = blend_mode;
        self
    }

    /// Sets anti-aliasing.
    #[must_use]
    #[inline]
    pub const fn anti_alias(mut self, aa: bool) -> Self {
        self.paint.anti_alias = aa;
        self
    }

    /// Sets the shader.
    #[must_use]
    #[inline]
    pub fn shader(mut self, shader: Shader) -> Self {
        self.paint.shader = Some(shader);
        self
    }

    /// Builds the paint.
    #[must_use]
    #[inline]
    pub fn build(self) -> Paint {
        self.paint
    }
}

impl Default for PaintBuilder {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}
