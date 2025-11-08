//! Paint, Stroke, Gradient, and Shadow definitions
//!
//! This module provides unified styling for all rendering operations.
//! Follows SOLID principles with clear separation of concerns:
//! - Paint handles fill and stroke style
//! - Stroke is a separate struct for stroke-specific properties
//! - Gradient is optional and overrides solid color

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
/// // Solid fill
/// let fill = Paint::fill(Color::RED);
///
/// // Stroke with width
/// let stroke = Paint::stroke(Color::BLUE).with_stroke(Stroke::new(2.0));
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
#[derive(Debug, Clone)]
pub struct Paint {
    /// Base color (used when gradient is None)
    pub color: Color,

    /// Painting style: fill or stroke
    pub style: PaintingStyle,

    /// Optional gradient (overrides color if present)
    pub gradient: Option<Gradient>,

    /// Optional stroke properties (only relevant for stroke style)
    pub stroke: Option<Stroke>,

    /// Anti-aliasing enabled
    pub anti_alias: bool,

    /// Blend mode for compositing
    pub blend_mode: BlendMode,
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

    /// Create a fill paint with solid color
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

    /// Create a stroke paint with solid color
    ///
    /// Note: Use with_stroke() to set stroke width and other properties
    #[inline]
    pub fn stroke(color: Color) -> Self {
        Self {
            color,
            style: PaintingStyle::Stroke,
            gradient: None,
            stroke: None, // Set via with_stroke()
            anti_alias: true,
            blend_mode: BlendMode::SrcOver,
        }
    }

    /// Create a gradient fill paint
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

    // ===== Builder methods =====

    /// Set color
    #[inline]
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Set painting style (fill or stroke)
    #[inline]
    pub fn with_style(mut self, style: PaintingStyle) -> Self {
        self.style = style;
        self
    }

    /// Set gradient
    #[inline]
    pub fn with_gradient(mut self, gradient: Gradient) -> Self {
        self.gradient = Some(gradient);
        self
    }

    /// Set stroke properties
    #[inline]
    pub fn with_stroke(mut self, stroke: Stroke) -> Self {
        self.stroke = Some(stroke);
        self
    }

    /// Set blend mode
    #[inline]
    pub fn with_blend_mode(mut self, blend_mode: BlendMode) -> Self {
        self.blend_mode = blend_mode;
        self
    }

    /// Set anti-aliasing
    #[inline]
    pub fn with_anti_alias(mut self, anti_alias: bool) -> Self {
        self.anti_alias = anti_alias;
        self
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
/// let stroke = Stroke::new(2.0)
///     .with_cap(StrokeCap::Round)
///     .with_join(StrokeJoin::Round);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Stroke {
    /// Stroke width in pixels
    pub width: f32,

    /// Cap style for line endings
    pub cap: StrokeCap,

    /// Join style for corners
    pub join: StrokeJoin,

    /// Miter limit (only applies when join is Miter)
    pub miter_limit: f32,
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
    /// Create a new stroke with specified width
    #[inline]
    pub fn new(width: f32) -> Self {
        Self {
            width,
            ..Default::default()
        }
    }

    /// Set cap style
    #[inline]
    pub fn with_cap(mut self, cap: StrokeCap) -> Self {
        self.cap = cap;
        self
    }

    /// Set join style
    #[inline]
    pub fn with_join(mut self, join: StrokeJoin) -> Self {
        self.join = join;
        self
    }

    /// Set miter limit
    #[inline]
    pub fn with_miter_limit(mut self, miter_limit: f32) -> Self {
        self.miter_limit = miter_limit;
        self
    }
}

// ===== Gradient =====

/// Gradient definition for fill painting
///
/// Supports linear, radial, and conic (sweep) gradients.
///
/// # Examples
/// ```ignore
/// // Linear gradient from red to blue
/// let gradient = Gradient::linear(
///     Point::new(0.0, 0.0),
///     Point::new(100.0, 0.0),
///     vec![
///         GradientStop::new(0.0, Color::RED),
///         GradientStop::new(1.0, Color::BLUE),
///     ],
/// );
///
/// // Radial gradient
/// let gradient = Gradient::radial(
///     Point::new(50.0, 50.0),
///     50.0,
///     vec![
///         GradientStop::new(0.0, Color::WHITE),
///         GradientStop::new(1.0, Color::BLACK),
///     ],
/// );
/// ```
#[derive(Debug, Clone)]
pub struct Gradient {
    /// Type of gradient
    pub gradient_type: GradientType,

    /// Gradient color stops (position 0.0 to 1.0)
    pub stops: Vec<GradientStop>,

    /// Start point (for linear) or center (for radial/conic)
    pub start: Point,

    /// End point (for linear only)
    pub end: Point,

    /// Radius (for radial only)
    pub radius: f32,

    /// Rotation angle in radians (for conic only)
    pub rotation: f32,
}

impl Gradient {
    /// Create a linear gradient
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

    /// Create a radial gradient
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

    /// Create a conic (sweep) gradient
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
#[derive(Debug, Clone, Copy)]
pub struct GradientStop {
    /// Position in gradient (0.0 = start, 1.0 = end)
    pub position: f32,
    /// Color at this position
    pub color: Color,
}

impl GradientStop {
    /// Create a new gradient stop
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
/// let shadow = Shadow::new(
///     Color::rgba(0, 0, 0, 128),
///     Point::new(4.0, 4.0),
///     8.0,
/// );
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Shadow {
    /// Shadow color
    pub color: Color,

    /// Shadow offset from shape
    pub offset: Point,

    /// Blur radius (sigma for Gaussian blur)
    pub blur_radius: f32,

    /// Spread radius (expand shadow before blur)
    pub spread_radius: f32,
}

impl Shadow {
    /// Create a new shadow
    pub fn new(color: Color, offset: Point, blur_radius: f32) -> Self {
        Self {
            color,
            offset,
            blur_radius,
            spread_radius: 0.0,
        }
    }

    /// Set spread radius
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
        let stroke = Stroke::new(2.5)
            .with_cap(StrokeCap::Round)
            .with_join(StrokeJoin::Bevel);
        assert_eq!(stroke.width, 2.5);
        assert_eq!(stroke.cap, StrokeCap::Round);
        assert_eq!(stroke.join, StrokeJoin::Bevel);
    }
}
