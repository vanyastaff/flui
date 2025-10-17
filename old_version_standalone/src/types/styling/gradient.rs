//! Gradient types for colorful backgrounds
//!
//! This module contains types for representing gradients,
//! similar to Flutter's Gradient system.

use crate::types::core::{Color, Point};

/// Defines how the gradient should tile beyond its defined bounds.
///
/// Similar to Flutter's `TileMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TileMode {
    /// Render the last color beyond the edge of the gradient.
    #[default]
    Clamp,

    /// Repeat the gradient from the beginning.
    Repeat,

    /// Repeat the gradient backwards, then forwards, then backwards, etc.
    Mirror,

    /// Render transparent beyond the edge of the gradient.
    Decal,
}

/// A color and position in a gradient.
///
/// The position is a value from 0.0 to 1.0 indicating where in the gradient
/// this color should appear.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradientStop {
    /// The position of this stop in the gradient (0.0 to 1.0).
    pub position: f32,

    /// The color at this stop.
    pub color: Color,
}

impl GradientStop {
    /// Create a new gradient stop.
    pub fn new(position: f32, color: impl Into<Color>) -> Self {
        Self {
            position,
            color: color.into(),
        }
    }

    /// Create a gradient stop at the start (position 0.0).
    pub fn start(color: impl Into<Color>) -> Self {
        Self::new(0.0, color)
    }

    /// Create a gradient stop at the end (position 1.0).
    pub fn end(color: impl Into<Color>) -> Self {
        Self::new(1.0, color)
    }

    /// Create a gradient stop at the middle (position 0.5).
    pub fn middle(color: impl Into<Color>) -> Self {
        Self::new(0.5, color)
    }
}

impl From<(f32, Color)> for GradientStop {
    fn from((position, color): (f32, Color)) -> Self {
        Self { position, color }
    }
}

/// The base properties shared by all gradient types.
#[derive(Debug, Clone, PartialEq)]
pub struct GradientBase {
    /// The colors and stops defining the gradient.
    pub stops: Vec<GradientStop>,

    /// How to tile the gradient outside its defined bounds.
    pub tile_mode: TileMode,
}

impl GradientBase {
    /// Create a new gradient base with the given stops.
    pub fn new(stops: Vec<GradientStop>, tile_mode: TileMode) -> Self {
        Self { stops, tile_mode }
    }

    /// Create a simple two-color gradient.
    pub fn two_colors(start_color: impl Into<Color>, end_color: impl Into<Color>) -> Self {
        Self {
            stops: vec![
                GradientStop::start(start_color),
                GradientStop::end(end_color),
            ],
            tile_mode: TileMode::Clamp,
        }
    }

    /// Create from a list of colors with evenly distributed positions.
    pub fn from_colors(colors: Vec<Color>) -> Self {
        let count = colors.len();
        if count == 0 {
            return Self {
                stops: vec![],
                tile_mode: TileMode::Clamp,
            };
        }

        let stops = colors
            .into_iter()
            .enumerate()
            .map(|(i, color)| {
                let position = if count == 1 {
                    0.0
                } else {
                    i as f32 / (count - 1) as f32
                };
                GradientStop::new(position, color)
            })
            .collect();

        Self {
            stops,
            tile_mode: TileMode::Clamp,
        }
    }

    /// Get the number of color stops.
    pub fn len(&self) -> usize {
        self.stops.len()
    }

    /// Check if there are no color stops.
    pub fn is_empty(&self) -> bool {
        self.stops.is_empty()
    }
}

/// A 2D gradient that interpolates colors linearly between two points.
///
/// Similar to Flutter's `LinearGradient`.
#[derive(Debug, Clone, PartialEq)]
pub struct LinearGradient {
    /// The base gradient properties.
    pub base: GradientBase,

    /// The starting point of the gradient.
    ///
    /// Coordinates are in the unit square (0.0 to 1.0).
    pub begin: Point,

    /// The ending point of the gradient.
    ///
    /// Coordinates are in the unit square (0.0 to 1.0).
    pub end: Point,
}

impl LinearGradient {
    /// Create a new linear gradient.
    pub fn new(begin: impl Into<Point>, end: impl Into<Point>, stops: Vec<GradientStop>, tile_mode: TileMode) -> Self {
        Self {
            base: GradientBase::new(stops, tile_mode),
            begin: begin.into(),
            end: end.into(),
        }
    }

    /// Create a simple two-color linear gradient.
    pub fn two_colors(begin: impl Into<Point>, end: impl Into<Point>, start_color: impl Into<Color>, end_color: impl Into<Color>) -> Self {
        Self {
            base: GradientBase::two_colors(start_color, end_color),
            begin: begin.into(),
            end: end.into(),
        }
    }

    /// Create a horizontal gradient from left to right.
    pub fn horizontal(start_color: impl Into<Color>, end_color: impl Into<Color>) -> Self {
        Self::two_colors(
            Point::new(0.0, 0.5),
            Point::new(1.0, 0.5),
            start_color,
            end_color,
        )
    }

    /// Create a vertical gradient from top to bottom.
    pub fn vertical(start_color: impl Into<Color>, end_color: impl Into<Color>) -> Self {
        Self::two_colors(
            Point::new(0.5, 0.0),
            Point::new(0.5, 1.0),
            start_color,
            end_color,
        )
    }

    /// Create a diagonal gradient from top-left to bottom-right.
    pub fn diagonal(start_color: impl Into<Color>, end_color: impl Into<Color>) -> Self {
        Self::two_colors(
            Point::new(0.0, 0.0),
            Point::new(1.0, 1.0),
            start_color,
            end_color,
        )
    }

    /// Get the angle of this gradient in radians.
    pub fn angle(&self) -> f32 {
        let delta = self.end - self.begin;
        delta.dy.atan2(delta.dx)
    }

    /// Create a linear gradient at a specific angle (in radians, 0 = horizontal right).
    pub fn from_angle(angle: f32, start_color: impl Into<Color>, end_color: impl Into<Color>) -> Self {
        let dx = angle.cos() * 0.5;
        let dy = angle.sin() * 0.5;
        let center = Point::new(0.5, 0.5);

        Self::two_colors(
            Point::new(center.x - dx, center.y - dy),
            Point::new(center.x + dx, center.y + dy),
            start_color,
            end_color,
        )
    }
}

/// A 2D gradient that interpolates colors radially from a center point.
///
/// Similar to Flutter's `RadialGradient`.
#[derive(Debug, Clone, PartialEq)]
pub struct RadialGradient {
    /// The base gradient properties.
    pub base: GradientBase,

    /// The center of the gradient.
    ///
    /// Coordinates are in the unit square (0.0 to 1.0).
    pub center: Point,

    /// The radius of the gradient.
    ///
    /// This is relative to the shortest side of the bounding box.
    pub radius: f32,

    /// The focal point of the gradient.
    ///
    /// If None, the focal point is the same as the center.
    pub focal: Option<Point>,

    /// The radius of the focal point.
    pub focal_radius: f32,
}

impl RadialGradient {
    /// Create a new radial gradient.
    pub fn new(
        center: impl Into<Point>,
        radius: f32,
        stops: Vec<GradientStop>,
        tile_mode: TileMode,
        focal: Option<Point>,
        focal_radius: f32,
    ) -> Self {
        Self {
            base: GradientBase::new(stops, tile_mode),
            center: center.into(),
            radius,
            focal,
            focal_radius,
        }
    }

    /// Create a simple two-color radial gradient.
    pub fn two_colors(center: impl Into<Point>, radius: f32, start_color: impl Into<Color>, end_color: impl Into<Color>) -> Self {
        Self {
            base: GradientBase::two_colors(start_color, end_color),
            center: center.into(),
            radius,
            focal: None,
            focal_radius: 0.0,
        }
    }

    /// Create a radial gradient centered in the middle.
    pub fn centered(radius: f32, start_color: impl Into<Color>, end_color: impl Into<Color>) -> Self {
        Self::two_colors(Point::new(0.5, 0.5), radius, start_color, end_color)
    }

    /// Create a radial gradient that fills the entire box.
    pub fn circle(start_color: impl Into<Color>, end_color: impl Into<Color>) -> Self {
        Self::centered(0.5, start_color, end_color)
    }
}

/// A 2D gradient that interpolates colors in a sweep around a center point.
///
/// Similar to Flutter's `SweepGradient`.
#[derive(Debug, Clone, PartialEq)]
pub struct SweepGradient {
    /// The base gradient properties.
    pub base: GradientBase,

    /// The center of the gradient.
    ///
    /// Coordinates are in the unit square (0.0 to 1.0).
    pub center: Point,

    /// The angle in radians at which the gradient begins.
    ///
    /// 0.0 corresponds to the right (3 o'clock position).
    pub start_angle: f32,

    /// The angle in radians at which the gradient ends.
    ///
    /// If not specified, defaults to start_angle + 2π (full circle).
    pub end_angle: f32,
}

impl SweepGradient {
    /// Create a new sweep gradient.
    pub fn new(
        center: impl Into<Point>,
        start_angle: f32,
        end_angle: f32,
        stops: Vec<GradientStop>,
        tile_mode: TileMode,
    ) -> Self {
        Self {
            base: GradientBase::new(stops, tile_mode),
            center: center.into(),
            start_angle,
            end_angle,
        }
    }

    /// Create a simple two-color sweep gradient.
    pub fn two_colors(
        center: impl Into<Point>,
        start_angle: f32,
        end_angle: f32,
        start_color: impl Into<Color>,
        end_color: impl Into<Color>,
    ) -> Self {
        Self {
            base: GradientBase::two_colors(start_color, end_color),
            center: center.into(),
            start_angle,
            end_angle,
        }
    }

    /// Create a full-circle sweep gradient centered in the middle.
    pub fn centered(start_color: impl Into<Color>, end_color: impl Into<Color>) -> Self {
        Self::two_colors(
            Point::new(0.5, 0.5),
            0.0,
            std::f32::consts::TAU,
            start_color,
            end_color,
        )
    }

    /// Create a rainbow sweep gradient.
    pub fn rainbow(center: impl Into<Point>) -> Self {
        Self {
            base: GradientBase::from_colors(vec![
                Color::RED,
                Color::from_rgb(255, 127, 0), // Orange
                Color::YELLOW,
                Color::GREEN,
                Color::BLUE,
                Color::from_rgb(75, 0, 130),  // Indigo
                Color::from_rgb(148, 0, 211), // Violet
                Color::RED, // Back to red for seamless loop
            ]),
            center: center.into(),
            start_angle: 0.0,
            end_angle: std::f32::consts::TAU,
        }
    }

    /// Get the sweep angle (end_angle - start_angle).
    pub fn sweep_angle(&self) -> f32 {
        self.end_angle - self.start_angle
    }
}

/// An enumeration of all gradient types.
#[derive(Debug, Clone, PartialEq)]
pub enum Gradient {
    /// A linear gradient.
    Linear(LinearGradient),

    /// A radial gradient.
    Radial(RadialGradient),

    /// A sweep gradient.
    Sweep(SweepGradient),
}

impl Gradient {
    /// Get the base gradient properties.
    pub fn base(&self) -> &GradientBase {
        match self {
            Gradient::Linear(g) => &g.base,
            Gradient::Radial(g) => &g.base,
            Gradient::Sweep(g) => &g.base,
        }
    }

    /// Get the color stops.
    pub fn stops(&self) -> &[GradientStop] {
        &self.base().stops
    }

    /// Get the tile mode.
    pub fn tile_mode(&self) -> TileMode {
        self.base().tile_mode
    }
}

impl From<LinearGradient> for Gradient {
    fn from(gradient: LinearGradient) -> Self {
        Gradient::Linear(gradient)
    }
}

impl From<RadialGradient> for Gradient {
    fn from(gradient: RadialGradient) -> Self {
        Gradient::Radial(gradient)
    }
}

impl From<SweepGradient> for Gradient {
    fn from(gradient: SweepGradient) -> Self {
        Gradient::Sweep(gradient)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tile_mode() {
        assert_eq!(TileMode::default(), TileMode::Clamp);
    }

    #[test]
    fn test_gradient_stop_creation() {
        let stop = GradientStop::new(0.5, Color::RED);
        assert_eq!(stop.position, 0.5);
        assert_eq!(stop.color, Color::RED);

        let start = GradientStop::start(Color::BLUE);
        assert_eq!(start.position, 0.0);

        let end = GradientStop::end(Color::GREEN);
        assert_eq!(end.position, 1.0);

        let middle = GradientStop::middle(Color::YELLOW);
        assert_eq!(middle.position, 0.5);
    }

    #[test]
    fn test_gradient_stop_conversion() {
        let stop: GradientStop = (0.7, Color::RED).into();
        assert_eq!(stop.position, 0.7);
        assert_eq!(stop.color, Color::RED);
    }

    #[test]
    fn test_gradient_base_two_colors() {
        let base = GradientBase::two_colors(Color::RED, Color::BLUE);
        assert_eq!(base.stops.len(), 2);
        assert_eq!(base.stops[0].position, 0.0);
        assert_eq!(base.stops[0].color, Color::RED);
        assert_eq!(base.stops[1].position, 1.0);
        assert_eq!(base.stops[1].color, Color::BLUE);
    }

    #[test]
    fn test_gradient_base_from_colors() {
        let colors = vec![Color::RED, Color::GREEN, Color::BLUE];
        let base = GradientBase::from_colors(colors);

        assert_eq!(base.stops.len(), 3);
        assert_eq!(base.stops[0].position, 0.0);
        assert_eq!(base.stops[1].position, 0.5);
        assert_eq!(base.stops[2].position, 1.0);

        // Empty colors
        let empty = GradientBase::from_colors(vec![]);
        assert!(empty.is_empty());

        // Single color
        let single = GradientBase::from_colors(vec![Color::RED]);
        assert_eq!(single.stops.len(), 1);
        assert_eq!(single.stops[0].position, 0.0);
    }

    #[test]
    fn test_linear_gradient_presets() {
        let horizontal = LinearGradient::horizontal(Color::RED, Color::BLUE);
        assert_eq!(horizontal.begin.y, 0.5);
        assert_eq!(horizontal.end.y, 0.5);
        assert!(horizontal.begin.x < horizontal.end.x);

        let vertical = LinearGradient::vertical(Color::RED, Color::BLUE);
        assert_eq!(vertical.begin.x, 0.5);
        assert_eq!(vertical.end.x, 0.5);
        assert!(vertical.begin.y < vertical.end.y);

        let diagonal = LinearGradient::diagonal(Color::RED, Color::BLUE);
        assert_eq!(diagonal.begin, Point::new(0.0, 0.0));
        assert_eq!(diagonal.end, Point::new(1.0, 1.0));
    }

    #[test]
    fn test_linear_gradient_angle() {
        let horizontal = LinearGradient::horizontal(Color::RED, Color::BLUE);
        let angle = horizontal.angle();
        assert!((angle - 0.0).abs() < 0.01); // Should be approximately 0 radians

        let from_angle = LinearGradient::from_angle(std::f32::consts::PI / 2.0, Color::RED, Color::BLUE);
        assert!((from_angle.angle() - std::f32::consts::PI / 2.0).abs() < 0.01);
    }

    #[test]
    fn test_radial_gradient_creation() {
        let radial = RadialGradient::centered(0.5, Color::RED, Color::BLUE);
        assert_eq!(radial.center, Point::new(0.5, 0.5));
        assert_eq!(radial.radius, 0.5);
        assert_eq!(radial.focal, None);

        let circle = RadialGradient::circle(Color::WHITE, Color::BLACK);
        assert_eq!(circle.center, Point::new(0.5, 0.5));
        assert_eq!(circle.radius, 0.5);
    }

    #[test]
    fn test_sweep_gradient_creation() {
        let sweep = SweepGradient::centered(Color::RED, Color::BLUE);
        assert_eq!(sweep.center, Point::new(0.5, 0.5));
        assert_eq!(sweep.start_angle, 0.0);
        assert_eq!(sweep.end_angle, std::f32::consts::TAU);
        assert_eq!(sweep.sweep_angle(), std::f32::consts::TAU);

        let rainbow = SweepGradient::rainbow(Point::new(0.5, 0.5));
        assert!(rainbow.base.stops.len() > 2);
    }

    #[test]
    fn test_gradient_enum() {
        let linear = LinearGradient::horizontal(Color::RED, Color::BLUE);
        let gradient: Gradient = linear.clone().into();

        assert_eq!(gradient.stops().len(), 2);
        assert_eq!(gradient.tile_mode(), TileMode::Clamp);

        match gradient {
            Gradient::Linear(g) => assert_eq!(g, linear),
            _ => panic!("Expected Linear gradient"),
        }
    }

    #[test]
    fn test_gradient_conversions() {
        let linear = LinearGradient::horizontal(Color::RED, Color::BLUE);
        let radial = RadialGradient::circle(Color::RED, Color::BLUE);
        let sweep = SweepGradient::centered(Color::RED, Color::BLUE);

        let _g1: Gradient = linear.into();
        let _g2: Gradient = radial.into();
        let _g3: Gradient = sweep.into();
    }
}
