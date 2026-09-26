//! Paint and painting styles for rendering.
//!
//! This module provides the `Paint` type and related styling information
//! for controlling how shapes and paths are rendered.

use crate::{
    painting::{BlendMode, Shader, StrokeCap, StrokeJoin},
    styling::Color,
};

/// Dash pattern for stroked paths.
///
/// Defines an alternating pattern of dash and gap lengths that is applied
/// when stroking a path. The pattern repeats cyclically along the path.
///
/// # Examples
///
/// ```rust
/// use flui_types::painting::paint::DashPattern;
///
/// // Simple dashed line: 10px dash, 5px gap
/// let dashes = DashPattern {
///     intervals: vec![10.0, 5.0],
///     phase: 0.0,
/// };
///
/// // Dash-dot pattern: 10px dash, 3px gap, 2px dot, 3px gap
/// let dash_dot = DashPattern {
///     intervals: vec![10.0, 3.0, 2.0, 3.0],
///     phase: 0.0,
/// };
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DashPattern {
    /// Alternating dash/gap lengths.
    ///
    /// Must contain an even number of entries. If an odd number is provided,
    /// the pattern is conceptually repeated to make it even (e.g., `[5, 3, 2]`
    /// becomes `[5, 3, 2, 5, 3, 2]`).
    pub intervals: Vec<f32>,

    /// Starting offset into the pattern.
    ///
    /// A phase of 0.0 starts at the beginning of the first dash.
    /// Positive values shift the pattern forward along the path.
    pub phase: f32,
}

impl DashPattern {
    /// Creates a new dash pattern with the given intervals and phase.
    #[must_use]
    #[inline]
    pub fn new(intervals: Vec<f32>, phase: f32) -> Self {
        Self { intervals, phase }
    }

    /// Returns the total length of one cycle of the dash pattern.
    #[must_use]
    pub fn cycle_length(&self) -> f32 {
        self.intervals.iter().sum()
    }

    /// Returns true if the pattern has valid intervals (non-empty, all positive).
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.intervals.is_empty() && self.intervals.iter().all(|&v| v > 0.0)
    }
}

/// Paint style and properties for rendering shapes and paths.
///
/// Contains all the information needed to render a shape, including color,
/// stroke/fill style, blend mode, and optional shader (gradient, pattern).
#[derive(Clone, Debug, PartialEq)]
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

    /// Optional dash pattern for stroked paths.
    ///
    /// When set and `style` is `PaintStyle::Stroke`, the stroke will be
    /// rendered as a dashed line following this pattern.
    pub dash_pattern: Option<DashPattern>,
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
            dash_pattern: None,
        }
    }

    /// Creates a stroke paint with the given color and width.
    #[must_use]
    #[inline]
    pub fn stroke(color: Color, width: f32) -> Self {
        debug_assert!(
            width >= 0.0 && !width.is_nan(),
            "Stroke width must be non-negative and not NaN, got: {width}",
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
            dash_pattern: None,
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
            "Stroke width must be non-negative and not NaN, got: {width}",
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

    /// Sets the dash pattern for stroked paths.
    ///
    /// The dash pattern defines alternating dash/gap lengths.
    /// This only has an effect when `style` is `PaintStyle::Stroke`.
    ///
    /// # Arguments
    ///
    /// * `intervals` - Alternating dash and gap lengths (must be non-empty)
    /// * `phase` - Starting offset into the pattern
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::painting::Paint;
    /// use flui_types::styling::Color;
    ///
    /// // Dashed line: 10px dash, 5px gap
    /// let paint = Paint::stroke(Color::BLACK, 2.0)
    ///     .with_dash(vec![10.0, 5.0], 0.0);
    /// ```
    #[must_use]
    #[inline]
    pub fn with_dash(mut self, intervals: Vec<f32>, phase: f32) -> Self {
        self.dash_pattern = Some(DashPattern::new(intervals, phase));
        self
    }

    /// Returns true if a dash pattern is set.
    #[must_use]
    #[inline]
    pub const fn has_dash(&self) -> bool {
        self.dash_pattern.is_some()
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

    /// Returns true if the paint is known to be fully opaque: an opaque
    /// color, a blend mode that replaces what is below, and no shader (a
    /// shader's own colors may be translucent whatever `color` says).
    #[must_use]
    #[inline]
    pub const fn is_opaque(&self) -> bool {
        self.color.a == 255
            && matches!(self.blend_mode, BlendMode::SrcOver | BlendMode::Src)
            && self.shader.is_none()
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
#[derive(Clone, Debug, Default, Copy, PartialEq, Eq, Hash)]
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
#[derive(Debug)]
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

    /// Sets the dash pattern.
    #[must_use]
    #[inline]
    pub fn dash(mut self, intervals: Vec<f32>, phase: f32) -> Self {
        self.paint.dash_pattern = Some(DashPattern::new(intervals, phase));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Offset;

    fn shader() -> Shader {
        Shader::simple_linear(Offset::ZERO, Offset::ZERO, vec![Color::RED, Color::BLUE])
    }

    #[test]
    fn dash_pattern() {
        let dash = DashPattern::new(vec![4.0, 2.5, 1.5], 3.0);
        assert_eq!((dash.cycle_length(), dash.phase), (8.0, 3.0));
        assert!(dash.is_valid());
        assert!(!DashPattern::new(vec![], 0.0).is_valid());
        assert!(!DashPattern::new(vec![4.0, 0.0], 0.0).is_valid());
        assert!(!DashPattern::new(vec![4.0, -1.0], 0.0).is_valid());
    }

    #[test]
    fn fill_and_stroke() {
        let fill = Paint::fill(Color::RED);
        assert_eq!(fill, Paint::default().with_color(Color::RED));
        assert!(fill.is_fill() && !fill.is_stroke() && fill.style.is_fill());
        assert_eq!(fill.effective_stroke_width(), 0.0);

        let stroke = Paint::stroke(Color::RED, 3.0);
        assert_eq!(
            stroke,
            fill.clone()
                .with_style(PaintStyle::Stroke)
                .with_stroke_width(3.0)
        );
        assert!(stroke.is_stroke() && !stroke.is_fill() && stroke.style.is_stroke());
        assert_eq!(stroke.effective_stroke_width(), 3.0);
        // A fill ignores whatever stroke width it carries.
        assert_eq!(
            stroke.with_style(PaintStyle::Fill).effective_stroke_width(),
            0.0
        );
        assert!(!PaintStyle::Fill.is_stroke() && !PaintStyle::Stroke.is_fill());
    }

    /// The builder and the `with_*` chain set the same fields.
    #[test]
    fn builder_matches_setters() {
        let built = Paint::builder()
            .style(PaintStyle::Stroke)
            .color(Color::GREEN)
            .stroke_width(2.0)
            .stroke_cap(StrokeCap::Round)
            .stroke_join(StrokeJoin::Bevel)
            .blend_mode(BlendMode::Multiply)
            .anti_alias(false)
            .shader(shader())
            .dash(vec![1.0, 2.0], 0.5)
            .build();
        let chained = Paint::default()
            .with_style(PaintStyle::Stroke)
            .with_color(Color::GREEN)
            .with_stroke_width(2.0)
            .with_stroke_cap(StrokeCap::Round)
            .with_stroke_join(StrokeJoin::Bevel)
            .with_blend_mode(BlendMode::Multiply)
            .with_anti_alias(false)
            .with_shader(shader())
            .with_dash(vec![1.0, 2.0], 0.5);
        assert_eq!(built, chained);
        assert_eq!(built.stroke_cap, StrokeCap::Round);
        assert_eq!(built.stroke_join, StrokeJoin::Bevel);
        assert_eq!(built.blend_mode, BlendMode::Multiply);
        assert!(built.has_shader() && built.has_dash() && !built.is_anti_aliased());
        assert_eq!(
            built.dash_pattern,
            Some(DashPattern::new(vec![1.0, 2.0], 0.5))
        );

        let plain = PaintBuilder::default().build();
        assert_eq!(plain, Paint::fill(Color::BLACK));
        assert!(!plain.has_shader() && !plain.has_dash() && plain.is_anti_aliased());
    }

    /// Opaque needs an opaque color, a replacing blend mode and no shader.
    #[test]
    fn opacity_queries() {
        let opaque = Paint::fill(Color::RED);
        assert!(opaque.is_opaque() && !opaque.is_transparent());
        assert!(opaque.clone().with_blend_mode(BlendMode::Src).is_opaque());
        assert!(
            !opaque
                .clone()
                .with_blend_mode(BlendMode::Multiply)
                .is_opaque()
        );
        assert!(!opaque.clone().with_alpha(254).is_opaque());
        assert!(!opaque.clone().with_shader(shader()).is_opaque());

        let clear = opaque.clone().with_alpha(0);
        assert!(clear.is_transparent() && !clear.is_opaque());
        assert_eq!(clear.color, Color::RED.with_alpha(0));
        assert_eq!(opaque.with_opacity(0.5).color, Color::RED.with_opacity(0.5));
    }
}
