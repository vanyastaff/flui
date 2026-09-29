//! Gradient types for styling

// Re-export TileMode from painting module
pub use crate::paint::TileMode;
use crate::{Alignment, styling::Color};

/// A description of a color gradient, similar to Flutter's `Gradient`.
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
    #[inline]
    pub fn colors(&self) -> &[Color] {
        match self {
            Gradient::Linear(g) => &g.colors,
            Gradient::Radial(g) => &g.colors,
            Gradient::Sweep(g) => &g.colors,
        }
    }

    /// Returns the color stops in this gradient, if any.
    #[inline]
    pub fn stops(&self) -> Option<&[f64]> {
        match self {
            Gradient::Linear(g) => g.stops.as_deref(),
            Gradient::Radial(g) => g.stops.as_deref(),
            Gradient::Sweep(g) => g.stops.as_deref(),
        }
    }

    /// Linearly interpolate between two gradients.
    ///
    /// Returns `None` if the gradients are of different kinds, or either
    /// side cannot be sampled (see [`LinearGradient::lerp`]).
    #[inline]
    pub fn lerp(a: &Self, b: &Self, t: f64) -> Option<Self> {
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

/// A gradient that transitions colors along a line between two alignment
/// points, similar to Flutter's `LinearGradient`.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LinearGradient {
    /// The offset at which the gradient begins.
    pub begin: Alignment,

    /// The offset at which the gradient ends.
    pub end: Alignment,

    /// The colors the gradient should obtain at each of the stops.
    pub colors: Vec<Color>,

    /// A list of values from 0.0 to 1.0 that denote fractions along the
    /// gradient.
    ///
    /// If None, the colors are evenly spaced.
    pub stops: Option<Vec<f64>>,

    /// How this gradient should tile the plane beyond the region defined by
    /// begin and end.
    pub tile_mode: TileMode,
}

impl LinearGradient {
    /// Creates a linear gradient.
    #[inline]
    pub fn new(
        begin: Alignment,
        end: Alignment,
        colors: Vec<Color>,
        stops: Option<Vec<f64>>,
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
    #[inline]
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
    #[inline]
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
    /// A common pattern for basic gradients. Transitions from `start_color` to
    /// `end_color` along the specified alignment axis.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_painting::Alignment;
    /// use flui_painting::styling::{Color, LinearGradient};
    ///
    /// // Simple fade from red to blue, left to right
    /// let gradient = LinearGradient::simple(
    ///     Color::RED,
    ///     Color::BLUE,
    ///     Alignment::CENTER_LEFT,
    ///     Alignment::CENTER_RIGHT,
    /// );
    /// ```
    #[inline]
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
    /// use flui_painting::styling::{Color, LinearGradient};
    ///
    /// let gradient = LinearGradient::diagonal(vec![Color::RED, Color::YELLOW, Color::BLUE]);
    /// ```
    #[inline]
    pub fn diagonal(colors: Vec<Color>) -> Self {
        Self::new(
            Alignment::TOP_LEFT,
            Alignment::BOTTOM_RIGHT,
            colors,
            None,
            TileMode::Clamp,
        )
    }

    /// Linearly interpolate between two linear gradients, like Flutter's
    /// `LinearGradient.lerp`: the result has a stop wherever either side
    /// has one, each coloured by lerping the two gradients sampled there,
    /// so gradients with different colour counts or stops interpolate.
    ///
    /// Returns `None` if either side has no colours, or explicit stops
    /// that do not match its colours one for one.
    #[inline]
    pub fn lerp(a: &Self, b: &Self, t: f64) -> Option<Self> {
        // Flutter returns `a` when both are the same object; equal values
        // are the closest this has to identity.
        if a == b {
            return Some(a.clone());
        }
        let t = t.clamp(0.0, 1.0);
        let (colors, stops) = interpolate_colors_and_stops(
            (&a.colors, a.stops.as_deref()),
            (&b.colors, b.stops.as_deref()),
            t,
        )?;
        Some(Self {
            begin: Alignment::lerp(a.begin, b.begin, t),
            end: Alignment::lerp(a.end, b.end, t),
            colors,
            stops: Some(stops),
            tile_mode: if t < 0.5 { a.tile_mode } else { b.tile_mode },
        })
    }
}

/// A gradient that radiates outward from a center point, similar to
/// Flutter's `RadialGradient`.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RadialGradient {
    /// The center of the gradient.
    pub center: Alignment,

    /// The radius of the gradient, as a fraction of the shortest side of the
    /// paint box.
    pub radius: f64,

    /// The colors the gradient should obtain at each of the stops.
    pub colors: Vec<Color>,

    /// A list of values from 0.0 to 1.0 that denote fractions along the
    /// gradient.
    pub stops: Option<Vec<f64>>,

    /// How this gradient should tile the plane beyond the region defined by
    /// center and radius.
    pub tile_mode: TileMode,

    /// The focal point of the gradient.
    ///
    /// If specified, the gradient will appear to be focused along the vector
    /// from center to focal.
    pub focal: Option<Alignment>,

    /// The radius of the focal point of gradient, as a fraction of the shortest
    /// side.
    pub focal_radius: Option<f64>,
}

impl RadialGradient {
    /// Creates a radial gradient.
    #[inline]
    pub fn new(
        center: Alignment,
        radius: f64,
        colors: Vec<Color>,
        stops: Option<Vec<f64>>,
        tile_mode: TileMode,
        focal: Option<Alignment>,
        focal_radius: Option<f64>,
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
    #[inline]
    pub fn centered(radius: f64, colors: Vec<Color>) -> Self {
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
    /// Uses radius of 0.5, which ensures the gradient reaches from center to
    /// edges. Common pattern for spotlight effects, vignettes, and circular
    /// buttons.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_painting::styling::{Color, RadialGradient};
    ///
    /// // White center fading to black edges
    /// let gradient = RadialGradient::circular(vec![Color::WHITE, Color::BLACK]);
    /// ```
    #[inline]
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
    /// Linearly interpolate between two radial gradients, like Flutter's
    /// `RadialGradient.lerp`. Colours and stops combine as in
    /// [`LinearGradient::lerp`], the radii never go below zero, and a
    /// missing focal radius counts as `0.0`, Flutter's default.
    ///
    /// A focal point on one side only moves to or from the other side's
    /// *center*, because a gradient without a focal point is focused on its
    /// center: the result at `t = 1` paints exactly like `b`. Flutter lerps
    /// it toward `Alignment(0, 0)` instead (`AlignmentGeometry.lerp` with a
    /// null end), which jumps whenever that center is anywhere else.
    #[inline]
    pub fn lerp(a: &Self, b: &Self, t: f64) -> Option<Self> {
        // Flutter returns `a` when both are the same object; equal values
        // are the closest this has to identity.
        if a == b {
            return Some(a.clone());
        }
        let t = t.clamp(0.0, 1.0);
        let (colors, stops) = interpolate_colors_and_stops(
            (&a.colors, a.stops.as_deref()),
            (&b.colors, b.stops.as_deref()),
            t,
        )?;

        let focal = match (a.focal, b.focal) {
            (Some(a_focal), Some(b_focal)) => Some(Alignment::lerp(a_focal, b_focal, t)),
            (Some(a_focal), None) => Some(Alignment::lerp(a_focal, b.center, t)),
            (None, Some(b_focal)) => Some(Alignment::lerp(a.center, b_focal, t)),
            (None, None) => None,
        };
        let focal_radius = match (a.focal_radius, b.focal_radius) {
            (None, None) => None,
            (a_r, b_r) => Some(lerp_f32(a_r.unwrap_or(0.0), b_r.unwrap_or(0.0), t).max(0.0)),
        };
        Some(Self {
            center: Alignment::lerp(a.center, b.center, t),
            radius: lerp_f32(a.radius, b.radius, t).max(0.0),
            colors,
            stops: Some(stops),
            tile_mode: if t < 0.5 { a.tile_mode } else { b.tile_mode },
            focal,
            focal_radius,
        })
    }
}

/// A gradient that sweeps through an arc of angles around a center point,
/// similar to Flutter's `SweepGradient`.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SweepGradient {
    /// The center of the gradient.
    pub center: Alignment,

    /// The colors the gradient should obtain at each of the stops.
    pub colors: Vec<Color>,

    /// A list of values from 0.0 to 1.0 that denote fractions along the
    /// gradient.
    pub stops: Option<Vec<f64>>,

    /// How this gradient should tile the plane beyond the region.
    pub tile_mode: TileMode,

    /// The angle in radians at which stop 0.0 of the gradient is placed.
    pub start_angle: f64,

    /// The angle in radians at which stop 1.0 of the gradient is placed.
    pub end_angle: f64,
}

impl SweepGradient {
    /// Creates a sweep gradient.
    #[inline]
    pub fn new(
        center: Alignment,
        colors: Vec<Color>,
        stops: Option<Vec<f64>>,
        tile_mode: TileMode,
        start_angle: f64,
        end_angle: f64,
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

    /// Creates a simple sweep gradient centered in the box that goes full
    /// circle.
    #[inline]
    pub fn centered(colors: Vec<Color>) -> Self {
        Self::new(
            Alignment::CENTER,
            colors,
            None,
            TileMode::Clamp,
            0.0,
            std::f64::consts::TAU,
        )
    }

    /// Linearly interpolate between two sweep gradients, like Flutter's
    /// `SweepGradient.lerp`. Colours and stops combine as in
    /// [`LinearGradient::lerp`]; the angles never go below zero.
    #[inline]
    pub fn lerp(a: &Self, b: &Self, t: f64) -> Option<Self> {
        // Flutter returns `a` when both are the same object; equal values
        // are the closest this has to identity.
        if a == b {
            return Some(a.clone());
        }
        let t = t.clamp(0.0, 1.0);
        let (colors, stops) = interpolate_colors_and_stops(
            (&a.colors, a.stops.as_deref()),
            (&b.colors, b.stops.as_deref()),
            t,
        )?;
        Some(Self {
            center: Alignment::lerp(a.center, b.center, t),
            colors,
            stops: Some(stops),
            tile_mode: if t < 0.5 { a.tile_mode } else { b.tile_mode },
            start_angle: lerp_f32(a.start_angle, b.start_angle, t).max(0.0),
            end_angle: lerp_f32(a.end_angle, b.end_angle, t).max(0.0),
        })
    }
}

fn lerp_f32(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Flutter's `Gradient._impliedStops`: the explicit stops, or the colours
/// spread evenly from 0 to 1. `None` for stops that do not pair one for
/// one with the colours, or no colours at all.
fn implied_stops(colors: &[Color], stops: Option<&[f64]>) -> Option<Vec<f64>> {
    match stops {
        _ if colors.is_empty() => None,
        Some(stops) if stops.len() != colors.len() => None,
        Some(stops) => Some(stops.to_vec()),
        None if colors.len() == 1 => Some(vec![0.0]),
        None => {
            #[expect(clippy::cast_precision_loss)] // a colour count
            let separation = 1.0 / (colors.len() - 1) as f64;
            #[expect(clippy::cast_precision_loss)]
            Some((0..colors.len()).map(|i| i as f64 * separation).collect())
        }
    }
}

/// Flutter's gradient `_sample`: the colour at `t` along `colors` placed at
/// `stops`, holding the end colours beyond the first and last stop.
#[expect(
    clippy::expect_used,
    reason = "`t` lies strictly inside the stops here, so a stop at or below it exists"
)]
fn sample(colors: &[Color], stops: &[f64], t: f64) -> Color {
    let (first, last) = (stops[0], stops[stops.len() - 1]);
    if t <= first {
        return colors[0];
    }
    if t >= last {
        return colors[colors.len() - 1];
    }
    let i = stops
        .iter()
        .rposition(|&s| s <= t)
        .expect("BUG: t > the first stop, so some stop is at or below it");
    Color::lerp(
        colors[i],
        colors[i + 1],
        (t - stops[i]) / (stops[i + 1] - stops[i]),
    )
}

/// Flutter's `_interpolateColorsAndStops`: a stop wherever either gradient
/// has one, coloured by lerping both gradients sampled there.
fn interpolate_colors_and_stops(
    (a_colors, a_stops): (&[Color], Option<&[f64]>),
    (b_colors, b_stops): (&[Color], Option<&[f64]>),
    t: f64,
) -> Option<(Vec<Color>, Vec<f64>)> {
    let a_stops = implied_stops(a_colors, a_stops)?;
    let b_stops = implied_stops(b_colors, b_stops)?;
    let mut stops: Vec<f64> = a_stops.iter().chain(&b_stops).copied().collect();
    stops.sort_by(f64::total_cmp);
    stops.dedup();
    let colors = stops
        .iter()
        .map(|&s| {
            Color::lerp(
                sample(a_colors, &a_stops, s),
                sample(b_colors, &b_stops, s),
                t,
            )
        })
        .collect();
    Some((colors, stops))
}

/// Base trait for gradient transformations.
///
/// Similar to Flutter's `GradientTransform`.
pub trait GradientTransform: std::fmt::Debug {
    /// Transform the gradient according to this transformation.
    ///
    /// Returns a transformation matrix that should be applied to the gradient.
    fn transform(&self) -> [[f64; 3]; 3];
}

/// A gradient transform that rotates the gradient by a fixed angle, similar
/// to Flutter's `GradientRotation`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GradientRotation {
    /// The angle in radians to rotate the gradient.
    pub radians: f64,
}

impl GradientRotation {
    /// Creates a new gradient rotation.
    #[inline]
    pub const fn new(radians: f64) -> Self {
        Self { radians }
    }
}

impl GradientTransform for GradientRotation {
    #[inline]
    fn transform(&self) -> [[f64; 3]; 3] {
        let cos = self.radians.cos();
        let sin = self.radians.sin();

        [[cos, -sin, 0.0], [sin, cos, 0.0], [0.0, 0.0, 1.0]]
    }
}
