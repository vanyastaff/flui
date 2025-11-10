//! Paint, Stroke, Gradient, and Shadow definitions
//!
//! This module provides unified styling for all rendering operations.
//! Follows SOLID principles with clear separation of concerns:
//! - Paint handles fill and stroke style
//! - Stroke is a separate struct for stroke-specific properties
//! - Gradient is optional and overrides solid color
//!
//! All types support the `bon` builder pattern for ergonomic construction.

use bon::Builder;
use flui_types::{
    painting::{BlendMode, PaintingStyle, StrokeCap, StrokeJoin},
    styling::Color,
    Point,
};

// ===== Paint (unified from compat + primitives) =====

/// Unified paint style for all rendering operations
///
/// Supports both solid colors and gradients, with optional stroke properties.
/// Follows the principle of separation of concerns:
/// - Fill and stroke share the same Paint base
/// - Stroke-specific properties are in separate Stroke struct
///
/// # Examples
/// ```ignore
/// // Solid fill (convenience method)
/// let fill = Paint::fill(Color::RED);
///
/// // Stroke with width (convenience method)
/// let stroke = Paint::stroke(Color::BLUE).with_stroke(Stroke::new(2.0));
///
/// // Using bon builder
/// let paint = Paint::builder()
///     .color(Color::RED)
///     .style(PaintingStyle::Fill)
///     .anti_alias(true)
///     .build();
///
/// // Gradient fill
/// let gradient = Gradient::linear(
///     Point::new(0.0, 0.0),
///     Point::new(100.0, 0.0),
///     vec![
///         GradientStop::new(0.0, Color::RED),
///         GradientStop::new(1.0, Color::BLUE),
///     ],
/// );
/// let paint = Paint::gradient(gradient);
/// ```
#[derive(Debug, Clone, Builder)]
pub struct Paint {
    /// Base color (used when gradient is None)
    #[builder(default = Color::BLACK)]
    color: Color,

    /// Painting style: fill or stroke
    #[builder(default = PaintingStyle::Fill)]
    style: PaintingStyle,

    /// Optional gradient (overrides color if present)
    #[builder(into)]
    gradient: Option<Gradient>,

    /// Optional stroke properties (only relevant for stroke style)
    #[builder(into)]
    stroke: Option<Stroke>,

    /// Anti-aliasing enabled
    #[builder(default = true)]
    anti_alias: bool,

    /// Blend mode for compositing
    #[builder(default = BlendMode::SrcOver)]
    blend_mode: BlendMode,
}

impl Default for Paint {
    fn default() -> Self {
        Self {
            color: Color::BLACK,
            style: PaintingStyle::Fill,
            gradient: None,
            stroke: None,
            anti_alias: true,
            blend_mode: BlendMode::SrcOver,
        }
    }
}

impl Paint {
    /// Create a new paint with default settings (black fill, anti-aliased)
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a fill paint with solid color (convenience constructor)
    #[inline]
    pub fn fill(color: Color) -> Self {
        Self {
            color,
            style: PaintingStyle::Fill,
            gradient: None,
            stroke: None,
            anti_alias: true,
            blend_mode: BlendMode::SrcOver,
        }
    }

    /// Create a stroke paint with solid color (convenience constructor)
    ///
    /// Note: Chain with `.with_stroke(Stroke::new(width))` to set stroke properties
    #[inline]
    pub fn stroke(color: Color) -> Self {
        Self {
            color,
            style: PaintingStyle::Stroke,
            gradient: None,
            stroke: None,
            anti_alias: true,
            blend_mode: BlendMode::SrcOver,
        }
    }

    /// Create a gradient fill paint (convenience constructor)
    #[inline]
    pub fn gradient(gradient: Gradient) -> Self {
        Self {
            color: Color::WHITE, // Fallback if gradient fails
            style: PaintingStyle::Fill,
            gradient: Some(gradient),
            stroke: None,
            anti_alias: true,
            blend_mode: BlendMode::SrcOver,
        }
    }

    // ===== Getters =====

    /// Get the paint color
    #[inline]
    pub fn get_color(&self) -> Color {
        self.color
    }

    /// Get the painting style
    #[inline]
    pub fn get_style(&self) -> PaintingStyle {
        self.style
    }

    /// Get the gradient (if any)
    #[inline]
    pub fn get_gradient(&self) -> Option<&Gradient> {
        self.gradient.as_ref()
    }

    /// Get the stroke properties (if any)
    #[inline]
    pub fn get_stroke(&self) -> Option<&Stroke> {
        self.stroke.as_ref()
    }

    /// Check if anti-aliasing is enabled
    #[inline]
    pub fn get_anti_alias(&self) -> bool {
        self.anti_alias
    }

    /// Get the blend mode
    #[inline]
    pub fn get_blend_mode(&self) -> BlendMode {
        self.blend_mode
    }

    // ===== Query methods =====

    /// Check if this is a fill paint
    #[inline]
    pub fn is_fill(&self) -> bool {
        self.style == PaintingStyle::Fill
    }

    /// Check if this is a stroke paint
    #[inline]
    pub fn is_stroke(&self) -> bool {
        self.style == PaintingStyle::Stroke
    }

    /// Check if this paint uses a gradient
    #[inline]
    pub fn has_gradient(&self) -> bool {
        self.gradient.is_some()
    }
}

// ===== Stroke (separate from Paint for clean API) =====

/// Stroke properties for line and shape outlines
///
/// Separate from Paint to allow clean API where stroke is optional.
/// Only relevant when Paint::style is PaintingStyle::Stroke.
///
/// # Examples
/// ```ignore
/// // Convenience constructor
/// let stroke = Stroke::new(2.0);
///
/// // Using bon builder
/// let stroke = Stroke::builder()
///     .width(2.0)
///     .cap(StrokeCap::Round)
///     .join(StrokeJoin::Round)
///     .build();
/// ```
#[derive(Debug, Clone, Copy, Builder)]
pub struct Stroke {
    /// Stroke width in pixels
    width: f32,

    /// Cap style for line endings
    #[builder(default = StrokeCap::Butt)]
    cap: StrokeCap,

    /// Join style for corners
    #[builder(default = StrokeJoin::Miter)]
    join: StrokeJoin,

    /// Miter limit (only applies when join is Miter)
    #[builder(default = 4.0)]
    miter_limit: f32,
}

impl Default for Stroke {
    fn default() -> Self {
        Self {
            width: 1.0,
            cap: StrokeCap::Butt,
            join: StrokeJoin::Miter,
            miter_limit: 4.0,
        }
    }
}

impl Stroke {
    /// Create a new stroke with specified width (convenience constructor)
    #[inline]
    pub fn new(width: f32) -> Self {
        Self {
            width,
            ..Default::default()
        }
    }

    // ===== Getters =====

    /// Get stroke width
    #[inline]
    pub fn width(&self) -> f32 {
        self.width
    }

    /// Get cap style
    #[inline]
    pub fn cap(&self) -> StrokeCap {
        self.cap
    }

    /// Get join style
    #[inline]
    pub fn join(&self) -> StrokeJoin {
        self.join
    }

    /// Get miter limit
    #[inline]
    pub fn miter_limit(&self) -> f32 {
        self.miter_limit
    }
}

// ===== Gradient =====

/// Gradient definition for fill painting
///
/// Supports linear, radial, and conic (sweep) gradients.
///
/// # Examples
/// ```ignore
/// // Linear gradient from red to blue (convenience constructor)
/// let gradient = Gradient::linear(
///     Point::new(0.0, 0.0),
///     Point::new(100.0, 0.0),
///     vec![
///         GradientStop::new(0.0, Color::RED),
///         GradientStop::new(1.0, Color::BLUE),
///     ],
/// );
///
/// // Radial gradient (convenience constructor)
/// let gradient = Gradient::radial(
///     Point::new(50.0, 50.0),
///     50.0,
///     vec![
///         GradientStop::new(0.0, Color::WHITE),
///         GradientStop::new(1.0, Color::BLACK),
///     ],
/// );
///
/// // Using bon builder
/// let gradient = Gradient::builder()
///     .gradient_type(GradientType::Linear)
///     .start(Point::new(0.0, 0.0))
///     .end(Point::new(100.0, 0.0))
///     .stops(vec![
///         GradientStop::new(0.0, Color::RED),
///         GradientStop::new(1.0, Color::BLUE),
///     ])
///     .build();
/// ```
#[derive(Debug, Clone, Builder)]
pub struct Gradient {
    /// Type of gradient
    pub gradient_type: GradientType,

    /// Gradient color stops (position 0.0 to 1.0)
    pub stops: Vec<GradientStop>,

    /// Start point (for linear) or center (for radial/conic)
    pub start: Point,

    /// End point (for linear only)
    #[builder(default = Point::ZERO)]
    pub end: Point,

    /// Radius (for radial only)
    #[builder(default = 0.0)]
    pub radius: f32,

    /// Rotation angle in radians (for conic only)
    #[builder(default = 0.0)]
    pub rotation: f32,
}

impl Gradient {
    /// Create a linear gradient (convenience constructor)
    pub fn linear(start: Point, end: Point, stops: Vec<GradientStop>) -> Self {
        Self {
            gradient_type: GradientType::Linear,
            stops,
            start,
            end,
            radius: 0.0,
            rotation: 0.0,
        }
    }

    /// Create a radial gradient (convenience constructor)
    pub fn radial(center: Point, radius: f32, stops: Vec<GradientStop>) -> Self {
        Self {
            gradient_type: GradientType::Radial,
            stops,
            start: center,
            end: center,
            radius,
            rotation: 0.0,
        }
    }

    /// Create a conic (sweep) gradient (convenience constructor)
    pub fn conic(center: Point, rotation: f32, stops: Vec<GradientStop>) -> Self {
        Self {
            gradient_type: GradientType::Conic,
            stops,
            start: center,
            end: center,
            radius: 0.0,
            rotation,
        }
    }
}

/// Gradient type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GradientType {
    /// Linear gradient from start to end point
    Linear,
    /// Radial gradient from center with radius
    Radial,
    /// Conic gradient (sweep) around center
    Conic,
}

/// Gradient color stop
///
/// Defines a color at a specific position (0.0 to 1.0) along the gradient.
///
/// # Examples
/// ```ignore
/// // Convenience constructor
/// let stop = GradientStop::new(0.5, Color::RED);
///
/// // Using bon builder
/// let stop = GradientStop::builder()
///     .position(0.5)
///     .color(Color::RED)
///     .build();
/// ```
#[derive(Debug, Clone, Copy, Builder)]
pub struct GradientStop {
    /// Position in gradient (0.0 = start, 1.0 = end)
    pub position: f32,
    /// Color at this position
    pub color: Color,
}

impl GradientStop {
    /// Create a new gradient stop (convenience constructor)
    #[inline]
    pub fn new(position: f32, color: Color) -> Self {
        Self { position, color }
    }
}

// ===== Shadow =====

/// Shadow effect definition
///
/// Defines a drop shadow with color, offset, and blur.
///
/// # Examples
/// ```ignore
/// // Convenience constructor
/// let shadow = Shadow::new(
///     Color::rgba(0, 0, 0, 128),
///     Point::new(4.0, 4.0),
///     8.0,
/// );
///
/// // Using bon builder
/// let shadow = Shadow::builder()
///     .color(Color::rgba(0, 0, 0, 128))
///     .offset(Point::new(4.0, 4.0))
///     .blur_radius(8.0)
///     .spread_radius(2.0)
///     .build();
/// ```
#[derive(Debug, Clone, Copy, Builder)]
pub struct Shadow {
    /// Shadow color
    pub color: Color,

    /// Shadow offset from shape
    pub offset: Point,

    /// Blur radius (sigma for Gaussian blur)
    pub blur_radius: f32,

    /// Spread radius (expand shadow before blur)
    #[builder(default = 0.0)]
    pub spread_radius: f32,
}

impl Shadow {
    /// Create a new shadow (convenience constructor)
    pub fn new(color: Color, offset: Point, blur_radius: f32) -> Self {
        Self {
            color,
            offset,
            blur_radius,
            spread_radius: 0.0,
        }
    }

    /// Set spread radius (for chaining)
    #[inline]
    pub fn with_spread(mut self, spread_radius: f32) -> Self {
        self.spread_radius = spread_radius;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paint_fill() {
        let paint = Paint::fill(Color::RED);
        assert_eq!(paint.color, Color::RED);
        assert_eq!(paint.style, PaintingStyle::Fill);
        assert!(paint.is_fill());
        assert!(!paint.is_stroke());
        assert!(!paint.has_gradient());
    }

    #[test]
    fn test_paint_stroke() {
        let paint = Paint::stroke(Color::BLUE);
        assert_eq!(paint.color, Color::BLUE);
        assert_eq!(paint.style, PaintingStyle::Stroke);
        assert!(paint.is_stroke());
        assert!(!paint.is_fill());
    }

    #[test]
    fn test_paint_gradient() {
        let gradient = Gradient::linear(
            Point::new(0.0, 0.0),
            Point::new(100.0, 0.0),
            vec![
                GradientStop::new(0.0, Color::RED),
                GradientStop::new(1.0, Color::BLUE),
            ],
        );
        let paint = Paint::gradient(gradient);
        assert!(paint.has_gradient());
        assert_eq!(paint.style, PaintingStyle::Fill);
    }

    #[test]
    fn test_stroke_defaults() {
        let stroke = Stroke::default();
        assert_eq!(stroke.width, 1.0);
        assert_eq!(stroke.miter_limit, 4.0);
    }

    #[test]
    fn test_stroke_builder() {
        let stroke = Stroke::builder()
            .width(2.5)
            .cap(StrokeCap::Round)
            .join(StrokeJoin::Bevel)
            .build();
        assert_eq!(stroke.width(), 2.5);
        assert_eq!(stroke.cap(), StrokeCap::Round);
        assert_eq!(stroke.join(), StrokeJoin::Bevel);
    }
}
