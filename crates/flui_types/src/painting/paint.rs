//! Paint and painting styles for rendering.
//!
//! This module provides the `Paint` type and related styling information
//! for controlling how shapes and paths are rendered.

use crate::painting::{BlendMode, Shader, StrokeCap, StrokeJoin};
use crate::styling::Color;

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

    #[must_use]
    pub const fn builder() -> PaintBuilder {
        PaintBuilder::new()
    }

    #[must_use]
    pub const fn with_style(mut self, style: PaintStyle) -> Self {
        self.style = style;
        self
    }

    #[must_use]
    pub const fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

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

    #[must_use]
    pub const fn with_stroke_cap(mut self, cap: StrokeCap) -> Self {
        self.stroke_cap = cap;
        self
    }

    #[must_use]
    pub const fn with_stroke_join(mut self, join: StrokeJoin) -> Self {
        self.stroke_join = join;
        self
    }

    #[must_use]
    pub const fn with_blend_mode(mut self, blend_mode: BlendMode) -> Self {
        self.blend_mode = blend_mode;
        self
    }

    #[must_use]
    pub const fn with_anti_alias(mut self, aa: bool) -> Self {
        self.anti_alias = aa;
        self
    }

    #[must_use]
    pub fn with_shader(mut self, shader: Shader) -> Self {
        self.shader = Some(shader);
        self
    }

    #[must_use]
    pub const fn is_fill(&self) -> bool {
        matches!(self.style, PaintStyle::Fill)
    }

    #[must_use]
    pub const fn is_stroke(&self) -> bool {
        matches!(self.style, PaintStyle::Stroke)
    }

    #[must_use]
    pub const fn has_shader(&self) -> bool {
        self.shader.is_some()
    }

    #[must_use]
    pub const fn is_anti_aliased(&self) -> bool {
        self.anti_alias
    }

    #[must_use]
    pub const fn effective_stroke_width(&self) -> f32 {
        match self.style {
            PaintStyle::Stroke => self.stroke_width,
            PaintStyle::Fill => 0.0,
        }
    }

    #[must_use]
    pub const fn is_opaque(&self) -> bool {
        self.color.a == 255 && matches!(self.blend_mode, BlendMode::SrcOver | BlendMode::Src)
    }

    #[must_use]
    pub const fn is_transparent(&self) -> bool {
        self.color.a == 0
    }

    #[must_use]
    pub fn with_alpha(mut self, alpha: u8) -> Self {
        self.color = self.color.with_alpha(alpha);
        self
    }

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

#[derive(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PaintStyle {
    #[default]
    Fill,
    /// Stroke the outline
    Stroke,
}

impl PaintStyle {
    #[must_use]
    pub const fn is_fill(&self) -> bool {
        matches!(self, PaintStyle::Fill)
    }

    #[must_use]
    pub const fn is_stroke(&self) -> bool {
        matches!(self, PaintStyle::Stroke)
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PaintBuilder {
    paint: Paint,
}

impl PaintBuilder {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            paint: Paint::fill(Color::BLACK),
        }
    }

    #[must_use]
    pub const fn style(mut self, style: PaintStyle) -> Self {
        self.paint.style = style;
        self
    }

    #[must_use]
    pub const fn color(mut self, color: Color) -> Self {
        self.paint.color = color;
        self
    }

    #[must_use]
    pub const fn stroke_width(mut self, width: f32) -> Self {
        self.paint.stroke_width = width;
        self
    }

    #[must_use]
    pub const fn stroke_cap(mut self, cap: StrokeCap) -> Self {
        self.paint.stroke_cap = cap;
        self
    }

    #[must_use]
    pub const fn stroke_join(mut self, join: StrokeJoin) -> Self {
        self.paint.stroke_join = join;
        self
    }

    #[must_use]
    pub const fn blend_mode(mut self, blend_mode: BlendMode) -> Self {
        self.paint.blend_mode = blend_mode;
        self
    }

    #[must_use]
    pub const fn anti_alias(mut self, aa: bool) -> Self {
        self.paint.anti_alias = aa;
        self
    }

    #[must_use]
    pub fn shader(mut self, shader: Shader) -> Self {
        self.paint.shader = Some(shader);
        self
    }

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
