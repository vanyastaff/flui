//! Gradient types for styling

use crate::layout::Alignment;
use crate::styling::Color;

// Re-export TileMode from painting module
pub use crate::painting::TileMode;

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Gradient {
    /// A linear gradient.
    Linear(LinearGradient),

    /// A radial gradient.
    Radial(RadialGradient),

    /// A sweep gradient.
    Sweep(SweepGradient),
}

impl Gradient {
    /// Returns the colors in this gradient.
    pub fn colors(&self) -> &[Color] {
        match self {
            Gradient::Linear(g) => &g.colors,
            Gradient::Radial(g) => &g.colors,
            Gradient::Sweep(g) => &g.colors,
        }
    }

    /// Returns the color stops in this gradient, if any.
    pub fn stops(&self) -> Option<&[f32]> {
        match self {
            Gradient::Linear(g) => g.stops.as_deref(),
            Gradient::Radial(g) => g.stops.as_deref(),
            Gradient::Sweep(g) => g.stops.as_deref(),
        }
    }

    /// Linearly interpolate between two gradients.
    ///
    /// Returns None if the gradients are of different types or have
    /// different numbers of colors.
    pub fn lerp(a: &Self, b: &Self, t: f32) -> Option<Self> {
        let t = t.clamp(0.0, 1.0);
        match (a, b) {
            (Gradient::Linear(a), Gradient::Linear(b)) => {
                LinearGradient::lerp(a, b, t).map(Gradient::Linear)
            }
            (Gradient::Radial(a), Gradient::Radial(b)) => {
                RadialGradient::lerp(a, b, t).map(Gradient::Radial)
            }
            (Gradient::Sweep(a), Gradient::Sweep(b)) => {
                SweepGradient::lerp(a, b, t).map(Gradient::Sweep)
            }
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LinearGradient {
    /// The offset at which the gradient begins.
    pub begin: Alignment,

    /// The offset at which the gradient ends.
    pub end: Alignment,

    /// The colors the gradient should obtain at each of the stops.
    pub colors: Vec<Color>,

    /// A list of values from 0.0 to 1.0 that denote fractions along the gradient.
    ///
    /// If None, the colors are evenly spaced.
    pub stops: Option<Vec<f32>>,

    /// How this gradient should tile the plane beyond the region defined by begin and end.
    pub tile_mode: TileMode,
}

impl LinearGradient {
    /// Creates a linear gradient.
    pub fn new(
        begin: Alignment,
        end: Alignment,
        colors: Vec<Color>,
        stops: Option<Vec<f32>>,
        tile_mode: TileMode,
    ) -> Self {
        Self {
            begin,
            end,
            colors,
            stops,
            tile_mode,
        }
    }

    /// Creates a simple linear gradient from left to right.
    pub fn horizontal(colors: Vec<Color>) -> Self {
        Self::new(
            Alignment::CENTER_LEFT,
            Alignment::CENTER_RIGHT,
            colors,
            None,
            TileMode::Clamp,
        )
    }

    /// Creates a simple linear gradient from top to bottom.
    pub fn vertical(colors: Vec<Color>) -> Self {
        Self::new(
            Alignment::TOP_CENTER,
            Alignment::BOTTOM_CENTER,
            colors,
            None,
            TileMode::Clamp,
        )
    }

    /// Creates a simple two-color linear gradient.
    ///
    /// A common pattern for basic gradients. Transitions from `start_color` to `end_color`
    /// along the specified alignment axis.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::styling::{LinearGradient, Color};
    /// use flui_types::layout::Alignment;
    ///
    /// // Simple fade from red to blue, left to right
    /// let gradient = LinearGradient::simple(
    ///     Color::RED,
    ///     Color::BLUE,
    ///     Alignment::CENTER_LEFT,
    ///     Alignment::CENTER_RIGHT,
    /// );
    /// ```
    pub fn simple(start_color: Color, end_color: Color, begin: Alignment, end: Alignment) -> Self {
        Self::new(
            begin,
            end,
            vec![start_color, end_color],
            None,
            TileMode::Clamp,
        )
    }

    /// Creates a diagonal linear gradient from top-left to bottom-right.
    ///
    /// Common pattern for modern UI designs.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::styling::{LinearGradient, Color};
    ///
    /// let gradient = LinearGradient::diagonal(vec![Color::RED, Color::YELLOW, Color::BLUE]);
    /// ```
    pub fn diagonal(colors: Vec<Color>) -> Self {
        Self::new(
            Alignment::TOP_LEFT,
            Alignment::BOTTOM_RIGHT,
            colors,
            None,
            TileMode::Clamp,
        )
    }

    /// Linearly interpolate between two linear gradients.
    ///
    /// Returns None if the gradients have different numbers of colors.
    pub fn lerp(a: &Self, b: &Self, t: f32) -> Option<Self> {
        if a.colors.len() != b.colors.len() {
            return None;
        }

        let t = t.clamp(0.0, 1.0);
        let colors = a
            .colors
            .iter()
            .zip(&b.colors)
            .map(|(a_color, b_color)| Color::lerp(*a_color, *b_color, t))
            .collect();

        let stops = match (&a.stops, &b.stops) {
            (Some(a_stops), Some(b_stops)) if a_stops.len() == b_stops.len() => Some(
                a_stops
                    .iter()
                    .zip(b_stops)
                    .map(|(a_stop, b_stop)| a_stop + (b_stop - a_stop) * t)
                    .collect(),
            ),
            _ => None,
        };

        Some(Self {
            begin: Alignment::lerp(a.begin, b.begin, t),
            end: Alignment::lerp(a.end, b.end, t),
            colors,
            stops,
            tile_mode: if t < 0.5 { a.tile_mode } else { b.tile_mode },
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RadialGradient {
    /// The center of the gradient.
    pub center: Alignment,

    /// The radius of the gradient, as a fraction of the shortest side of the paint box.
    pub radius: f32,

    /// The colors the gradient should obtain at each of the stops.
    pub colors: Vec<Color>,

    /// A list of values from 0.0 to 1.0 that denote fractions along the gradient.
    pub stops: Option<Vec<f32>>,

    /// How this gradient should tile the plane beyond the region defined by center and radius.
    pub tile_mode: TileMode,

    /// The focal point of the gradient.
    ///
    /// If specified, the gradient will appear to be focused along the vector from
    /// center to focal.
    pub focal: Option<Alignment>,

    /// The radius of the focal point of gradient, as a fraction of the shortest side.
    pub focal_radius: Option<f32>,
}

impl RadialGradient {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        center: Alignment,
        radius: f32,
        colors: Vec<Color>,
        stops: Option<Vec<f32>>,
        tile_mode: TileMode,
        focal: Option<Alignment>,
        focal_radius: Option<f32>,
    ) -> Self {
        Self {
            center,
            radius,
            colors,
            stops,
            tile_mode,
            focal,
            focal_radius,
        }
    }

    /// Creates a simple radial gradient centered in the box.
    pub fn centered(radius: f32, colors: Vec<Color>) -> Self {
        Self::new(
            Alignment::CENTER,
            radius,
            colors,
            None,
            TileMode::Clamp,
            None,
            None,
        )
    }

    /// Creates a circular radial gradient that fills the entire box.
    ///
    /// Uses radius of 0.5, which ensures the gradient reaches from center to edges.
    /// Common pattern for spotlight effects, vignettes, and circular buttons.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::styling::{RadialGradient, Color};
    ///
    /// // White center fading to black edges
    /// let gradient = RadialGradient::circular(vec![Color::WHITE, Color::BLACK]);
    /// ```
    pub fn circular(colors: Vec<Color>) -> Self {
        Self::new(
            Alignment::CENTER,
            0.5,
            colors,
            None,
            TileMode::Clamp,
            None,
            None,
        )
    }

    /// Linearly interpolate between two radial gradients.
    pub fn lerp(a: &Self, b: &Self, t: f32) -> Option<Self> {
        if a.colors.len() != b.colors.len() {
            return None;
        }

        let t = t.clamp(0.0, 1.0);
        let colors = a
            .colors
            .iter()
            .zip(&b.colors)
            .map(|(a_color, b_color)| Color::lerp(*a_color, *b_color, t))
            .collect();

        let stops = match (&a.stops, &b.stops) {
            (Some(a_stops), Some(b_stops)) if a_stops.len() == b_stops.len() => Some(
                a_stops
                    .iter()
                    .zip(b_stops)
                    .map(|(a_stop, b_stop)| a_stop + (b_stop - a_stop) * t)
                    .collect(),
            ),
            _ => None,
        };

        let focal = match (a.focal, b.focal) {
            (Some(a_focal), Some(b_focal)) => Some(Alignment::lerp(a_focal, b_focal, t)),
            _ => None,
        };

        let focal_radius = match (a.focal_radius, b.focal_radius) {
            (Some(a_r), Some(b_r)) => Some(a_r + (b_r - a_r) * t),
            _ => None,
        };

        Some(Self {
            center: Alignment::lerp(a.center, b.center, t),
            radius: a.radius + (b.radius - a.radius) * t,
            colors,
            stops,
            tile_mode: if t < 0.5 { a.tile_mode } else { b.tile_mode },
            focal,
            focal_radius,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SweepGradient {
    /// The center of the gradient.
    pub center: Alignment,

    /// The colors the gradient should obtain at each of the stops.
    pub colors: Vec<Color>,

    /// A list of values from 0.0 to 1.0 that denote fractions along the gradient.
    pub stops: Option<Vec<f32>>,

    /// How this gradient should tile the plane beyond the region.
    pub tile_mode: TileMode,

    /// The angle in radians at which stop 0.0 of the gradient is placed.
    pub start_angle: f32,

    /// The angle in radians at which stop 1.0 of the gradient is placed.
    pub end_angle: f32,
}

impl SweepGradient {
    /// Creates a sweep gradient.
    pub fn new(
        center: Alignment,
        colors: Vec<Color>,
        stops: Option<Vec<f32>>,
        tile_mode: TileMode,
        start_angle: f32,
        end_angle: f32,
    ) -> Self {
        Self {
            center,
            colors,
            stops,
            tile_mode,
            start_angle,
            end_angle,
        }
    }

    /// Creates a simple sweep gradient centered in the box that goes full circle.
    pub fn centered(colors: Vec<Color>) -> Self {
        Self::new(
            Alignment::CENTER,
            colors,
            None,
            TileMode::Clamp,
            0.0,
            std::f32::consts::TAU,
        )
    }

    /// Linearly interpolate between two sweep gradients.
    pub fn lerp(a: &Self, b: &Self, t: f32) -> Option<Self> {
        if a.colors.len() != b.colors.len() {
            return None;
        }

        let t = t.clamp(0.0, 1.0);
        let colors = a
            .colors
            .iter()
            .zip(&b.colors)
            .map(|(a_color, b_color)| Color::lerp(*a_color, *b_color, t))
            .collect();

        let stops = match (&a.stops, &b.stops) {
            (Some(a_stops), Some(b_stops)) if a_stops.len() == b_stops.len() => Some(
                a_stops
                    .iter()
                    .zip(b_stops)
                    .map(|(a_stop, b_stop)| a_stop + (b_stop - a_stop) * t)
                    .collect(),
            ),
            _ => None,
        };

        Some(Self {
            center: Alignment::lerp(a.center, b.center, t),
            colors,
            stops,
            tile_mode: if t < 0.5 { a.tile_mode } else { b.tile_mode },
            start_angle: a.start_angle + (b.start_angle - a.start_angle) * t,
            end_angle: a.end_angle + (b.end_angle - a.end_angle) * t,
        })
    }
}

/// Base trait for gradient transformations.
///
/// Similar to Flutter's `GradientTransform`.
pub trait GradientTransform: std::fmt::Debug {
    /// Transform the gradient according to this transformation.
    ///
    /// Returns a transformation matrix that should be applied to the gradient.
    fn transform(&self) -> [[f32; 3]; 3];
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GradientRotation {
    /// The angle in radians to rotate the gradient.
    pub radians: f32,
}

impl GradientRotation {
    /// Creates a new gradient rotation.
    pub const fn new(radians: f32) -> Self {
        Self { radians }
    }
}

impl GradientTransform for GradientRotation {
    fn transform(&self) -> [[f32; 3]; 3] {
        let cos = self.radians.cos();
        let sin = self.radians.sin();

        [[cos, -sin, 0.0], [sin, cos, 0.0], [0.0, 0.0, 1.0]]
    }
}
