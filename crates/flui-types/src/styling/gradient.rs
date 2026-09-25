//! Gradient types for styling

// Re-export TileMode from painting module
pub use crate::painting::TileMode;
use crate::{layout::Alignment, styling::Color};

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
    pub fn stops(&self) -> Option<&[f32]> {
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
    pub stops: Option<Vec<f32>>,

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
    /// use flui_types::{
    ///     layout::Alignment,
    ///     styling::{Color, LinearGradient},
    /// };
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
    /// use flui_types::styling::{Color, LinearGradient};
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
    pub fn lerp(a: &Self, b: &Self, t: f32) -> Option<Self> {
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
    pub radius: f32,

    /// The colors the gradient should obtain at each of the stops.
    pub colors: Vec<Color>,

    /// A list of values from 0.0 to 1.0 that denote fractions along the
    /// gradient.
    pub stops: Option<Vec<f32>>,

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
    pub focal_radius: Option<f32>,
}

impl RadialGradient {
    /// Creates a radial gradient.
    #[inline]
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
    #[inline]
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
    /// Uses radius of 0.5, which ensures the gradient reaches from center to
    /// edges. Common pattern for spotlight effects, vignettes, and circular
    /// buttons.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::styling::{Color, RadialGradient};
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
    /// [`LinearGradient::lerp`]; the radii never go below zero; a focal
    /// point on one side only moves toward or away from `Alignment(0, 0)`
    /// (Flutter's `AlignmentGeometry.lerp` with a null end), and a missing
    /// focal radius counts as `0.0`, Flutter's default.
    #[inline]
    pub fn lerp(a: &Self, b: &Self, t: f32) -> Option<Self> {
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
        let scaled = |f: Alignment, s: f32| Alignment::new(f.x * s, f.y * s);
        let focal = match (a.focal, b.focal) {
            (Some(a_focal), Some(b_focal)) => Some(Alignment::lerp(a_focal, b_focal, t)),
            (Some(a_focal), None) => Some(scaled(a_focal, 1.0 - t)),
            (None, Some(b_focal)) => Some(scaled(b_focal, t)),
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
    #[inline]
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
            std::f32::consts::TAU,
        )
    }

    /// Linearly interpolate between two sweep gradients, like Flutter's
    /// `SweepGradient.lerp`. Colours and stops combine as in
    /// [`LinearGradient::lerp`]; the angles never go below zero.
    #[inline]
    pub fn lerp(a: &Self, b: &Self, t: f32) -> Option<Self> {
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

fn lerp_f32(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Flutter's `Gradient._impliedStops`: the explicit stops, or the colours
/// spread evenly from 0 to 1. `None` for stops that do not pair one for
/// one with the colours, or no colours at all.
fn implied_stops(colors: &[Color], stops: Option<&[f32]>) -> Option<Vec<f32>> {
    match stops {
        _ if colors.is_empty() => None,
        Some(stops) if stops.len() != colors.len() => None,
        Some(stops) => Some(stops.to_vec()),
        None if colors.len() == 1 => Some(vec![0.0]),
        None => {
            #[expect(clippy::cast_precision_loss)] // a colour count
            let separation = 1.0 / (colors.len() - 1) as f32;
            #[expect(clippy::cast_precision_loss)]
            Some((0..colors.len()).map(|i| i as f32 * separation).collect())
        }
    }
}

/// Flutter's gradient `_sample`: the colour at `t` along `colors` placed at
/// `stops`, holding the end colours beyond the first and last stop.
fn sample(colors: &[Color], stops: &[f32], t: f32) -> Color {
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
    (a_colors, a_stops): (&[Color], Option<&[f32]>),
    (b_colors, b_stops): (&[Color], Option<&[f32]>),
    t: f32,
) -> Option<(Vec<Color>, Vec<f32>)> {
    let a_stops = implied_stops(a_colors, a_stops)?;
    let b_stops = implied_stops(b_colors, b_stops)?;
    let mut stops: Vec<f32> = a_stops.iter().chain(&b_stops).copied().collect();
    stops.sort_by(f32::total_cmp);
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
    fn transform(&self) -> [[f32; 3]; 3];
}

/// A gradient transform that rotates the gradient by a fixed angle, similar
/// to Flutter's `GradientRotation`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GradientRotation {
    /// The angle in radians to rotate the gradient.
    pub radians: f32,
}

impl GradientRotation {
    /// Creates a new gradient rotation.
    #[inline]
    pub const fn new(radians: f32) -> Self {
        Self { radians }
    }
}

impl GradientTransform for GradientRotation {
    #[inline]
    fn transform(&self) -> [[f32; 3]; 3] {
        let cos = self.radians.cos();
        let sin = self.radians.sin();

        [[cos, -sin, 0.0], [sin, cos, 0.0], [0.0, 0.0, 1.0]]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Values are exact in binary so endpoints and midpoints compare exactly.
    fn two() -> Vec<Color> {
        vec![Color::rgb(0, 0, 0), Color::rgb(200, 100, 50)]
    }
    fn two_b() -> Vec<Color> {
        vec![Color::rgb(100, 50, 250), Color::rgb(0, 0, 0)]
    }

    fn linear(colors: Vec<Color>, stops: [f32; 2], tile_mode: TileMode) -> LinearGradient {
        LinearGradient::new(
            Alignment::TOP_LEFT,
            Alignment::CENTER,
            colors,
            Some(stops.to_vec()),
            tile_mode,
        )
    }

    fn radial(radius: f32, stops: [f32; 2], focal: f32) -> RadialGradient {
        RadialGradient::new(
            Alignment::new(focal, 0.0),
            radius,
            if focal == 0.0 { two() } else { two_b() },
            Some(stops.to_vec()),
            if focal == 0.0 {
                TileMode::Clamp
            } else {
                TileMode::Mirror
            },
            Some(Alignment::new(focal, focal)),
            Some(focal / 2.0),
        )
    }

    fn sweep(start: f32, end: f32) -> SweepGradient {
        SweepGradient::new(
            Alignment::CENTER,
            two(),
            Some(vec![0.0, 1.0]),
            TileMode::Repeat,
            start,
            end,
        )
    }

    /// Flutter's `LinearGradient.lerp`: a stop wherever either side has one
    /// (here 0, 0.5 and 1), coloured by lerping both sides sampled there.
    /// `a` holds its last colour past 0.5 and `b` its first before 0.5.
    #[test]
    fn linear_lerp() {
        let a = linear(two(), [0.0, 0.5], TileMode::Clamp);
        let b = LinearGradient {
            begin: Alignment::BOTTOM_RIGHT,
            ..linear(two_b(), [0.5, 1.0], TileMode::Mirror)
        };
        let union = Some(vec![0.0, 0.5, 1.0]);
        let mid = LinearGradient::lerp(&a, &b, 0.5).unwrap();
        assert_eq!(mid.begin, Alignment::CENTER);
        assert_eq!(mid.stops, union);
        assert_eq!(
            mid.colors,
            [
                Color::rgb(50, 25, 125),
                Color::rgb(150, 75, 150),
                Color::rgb(100, 50, 25)
            ]
        );
        // The ends are each side resampled at the joint stops.
        let start = LinearGradient::lerp(&a, &b, 0.0).unwrap();
        assert_eq!(
            (start.stops, start.colors),
            (union.clone(), vec![two()[0], two()[1], two()[1]])
        );
        let end = LinearGradient::lerp(&a, &b, 1.0).unwrap();
        assert_eq!(end.colors, [two_b()[0], two_b()[0], two_b()[1]]);
        // The tile mode switches to `b`'s at exactly t = 0.5, and t clamps.
        assert_eq!(mid.tile_mode, TileMode::Mirror);
        assert_eq!(
            LinearGradient::lerp(&a, &b, 0.25).unwrap().tile_mode,
            TileMode::Clamp
        );
        assert_eq!(LinearGradient::lerp(&a, &b, 3.0), Some(end));
        // A gradient lerps to itself unchanged.
        assert_eq!(LinearGradient::lerp(&a, &a, 0.5), Some(a.clone()));
    }

    #[test]
    fn radial_lerp() {
        let (a, b) = (radial(1.0, [0.0, 0.5], 0.0), radial(3.0, [0.5, 1.0], 1.0));
        let mid = RadialGradient::lerp(&a, &b, 0.5).unwrap();
        assert_eq!(mid.center, Alignment::new(0.5, 0.0));
        assert_eq!(mid.radius, 2.0);
        assert_eq!(mid.stops, Some(vec![0.0, 0.5, 1.0]));
        assert_eq!(mid.colors[1], Color::rgb(150, 75, 150));
        assert_eq!(mid.focal, Some(Alignment::new(0.5, 0.5)));
        assert_eq!(mid.focal_radius, Some(0.25));
        assert_eq!(mid.tile_mode, TileMode::Mirror);
        assert_eq!(
            RadialGradient::lerp(&a, &b, 0.25).unwrap().tile_mode,
            TileMode::Clamp
        );
        // Both focal radii non-zero: 0.25 to 0.5.
        let mid = RadialGradient::lerp(&radial(1.0, [0.0, 0.5], 0.5), &b, 0.5).unwrap();
        assert_eq!(mid.focal_radius, Some(0.375));

        // A focal point on one side scales toward Alignment(0, 0), and a
        // missing focal radius counts as 0.
        let unfocused = RadialGradient {
            focal: None,
            focal_radius: None,
            ..a.clone()
        };
        let quarter = RadialGradient::lerp(&b, &unfocused, 0.25).unwrap();
        assert_eq!(quarter.focal, Some(Alignment::new(0.75, 0.75)));
        assert_eq!(quarter.focal_radius, Some(0.375));
        let three_quarters = RadialGradient::lerp(&unfocused, &b, 0.75).unwrap();
        assert_eq!(three_quarters.focal, Some(Alignment::new(0.75, 0.75)));
        let wider = RadialGradient {
            radius: 2.0,
            ..unfocused.clone()
        };
        let neither = RadialGradient::lerp(&unfocused, &wider, 0.5).unwrap();
        assert_eq!((neither.focal, neither.focal_radius), (None, None));
        // The radius never goes below zero.
        let inverted = RadialGradient {
            radius: -3.0,
            ..b.clone()
        };
        assert_eq!(
            RadialGradient::lerp(&a, &inverted, 0.5).unwrap().radius,
            0.0
        );
    }

    #[test]
    fn sweep_lerp() {
        let a = SweepGradient {
            colors: vec![Color::rgb(0, 0, 0), Color::rgb(200, 100, 52)],
            ..sweep(1.0, 2.0)
        };
        let b = SweepGradient {
            colors: two_b(),
            stops: Some(vec![0.5, 1.0]),
            tile_mode: TileMode::Mirror,
            ..sweep(3.0, 4.0)
        };
        let mid = SweepGradient::lerp(&a, &b, 0.5).unwrap();
        assert_eq!((mid.start_angle, mid.end_angle), (2.0, 3.0));
        assert_eq!(mid.stops, Some(vec![0.0, 0.5, 1.0]));
        // At 0.5 `a` is halfway along its ramp, (100, 50, 26).
        assert_eq!(
            mid.colors,
            [
                Color::rgb(50, 25, 125),
                Color::rgb(100, 50, 138),
                Color::rgb(100, 50, 26)
            ]
        );
        assert_eq!(mid.tile_mode, TileMode::Mirror);
        assert_eq!(
            SweepGradient::lerp(&a, &b, 0.25).unwrap().tile_mode,
            TileMode::Repeat
        );
        // Angles never go below zero.
        let backward = sweep(-5.0, 4.0);
        assert_eq!(
            SweepGradient::lerp(&a, &backward, 0.5).unwrap().start_angle,
            0.0
        );
    }

    /// Different colour counts interpolate: two colours spread evenly
    /// against three meet at stops 0, 0.5 and 1. Only an empty side, or
    /// stops that do not pair with the colours, has no lerp.
    #[test]
    fn lerp_across_colour_counts() {
        let a = LinearGradient {
            stops: None,
            ..linear(two(), [0.0, 1.0], TileMode::Clamp)
        };
        let three = LinearGradient {
            colors: vec![Color::BLACK, Color::rgb(0, 50, 75), Color::BLACK],
            ..a.clone()
        };
        let mid = LinearGradient::lerp(&a, &three, 0.5).unwrap();
        assert_eq!(mid.stops, Some(vec![0.0, 0.5, 1.0]));
        // `a` at 0.5 is (100, 50, 25); at 1 it is (200, 100, 50).
        assert_eq!(
            mid.colors,
            [
                Color::BLACK,
                Color::rgb(50, 50, 50),
                Color::rgb(100, 50, 25)
            ]
        );
        let r3 = RadialGradient {
            colors: three.colors.clone(),
            stops: None,
            ..radial(1.0, [0.0, 1.0], 0.0)
        };
        let r2 = RadialGradient {
            stops: None,
            ..radial(1.0, [0.0, 1.0], 0.0)
        };
        assert_eq!(
            RadialGradient::lerp(&r2, &r3, 0.5).unwrap().colors,
            mid.colors
        );
        let s3 = SweepGradient {
            colors: three.colors.clone(),
            stops: None,
            ..sweep(0.0, 1.0)
        };
        let s2 = SweepGradient {
            stops: None,
            ..sweep(0.0, 1.0)
        };
        assert_eq!(
            SweepGradient::lerp(&s2, &s3, 0.5).unwrap().colors,
            mid.colors
        );

        let empty = LinearGradient {
            colors: vec![],
            stops: None,
            ..a.clone()
        };
        assert_eq!(LinearGradient::lerp(&a, &empty, 0.5), None);
        let mismatched = LinearGradient {
            stops: Some(vec![0.0, 0.5, 1.0]),
            ..a.clone()
        };
        assert_eq!(LinearGradient::lerp(&mismatched, &three, 0.5), None);
        // One colour is a flat gradient: every stop takes it.
        let flat = LinearGradient {
            colors: vec![Color::WHITE],
            stops: None,
            ..a.clone()
        };
        let with_flat = LinearGradient::lerp(&flat, &three, 0.0).unwrap();
        assert_eq!(with_flat.colors, [Color::WHITE; 3]);
    }

    #[test]
    fn gradient_dispatch() {
        let l = Gradient::Linear(linear(two(), [0.0, 0.5], TileMode::Clamp));
        let r = Gradient::Radial(radial(1.0, [0.0, 0.5], 0.0));
        let s = Gradient::Sweep(sweep(0.0, 1.0));
        for g in [&l, &r, &s] {
            assert_eq!(g.colors(), two().as_slice());
            assert_eq!(Gradient::lerp(g, g, 0.5).as_ref(), Some(g));
        }
        assert_eq!(l.stops(), Some([0.0, 0.5].as_slice()));
        assert_eq!(r.stops(), Some([0.0, 0.5].as_slice()));
        assert_eq!(s.stops(), Some([0.0, 1.0].as_slice()));
        assert_eq!(Gradient::lerp(&l, &r, 0.5), None);
        assert_eq!(Gradient::lerp(&r, &s, 0.5), None);
        assert_eq!(Gradient::lerp(&s, &l, 0.5), None);
    }

    #[test]
    fn constructors() {
        let c = two();
        let geometry = |g: LinearGradient| (g.begin, g.end, g.stops, g.tile_mode);
        let clamp = TileMode::Clamp;
        assert_eq!(
            geometry(LinearGradient::horizontal(c.clone())),
            (Alignment::CENTER_LEFT, Alignment::CENTER_RIGHT, None, clamp)
        );
        assert_eq!(
            geometry(LinearGradient::vertical(c.clone())),
            (Alignment::TOP_CENTER, Alignment::BOTTOM_CENTER, None, clamp)
        );
        assert_eq!(
            geometry(LinearGradient::diagonal(c.clone())),
            (Alignment::TOP_LEFT, Alignment::BOTTOM_RIGHT, None, clamp)
        );
        let simple = LinearGradient::simple(
            Color::RED,
            Color::BLUE,
            Alignment::TOP_LEFT,
            Alignment::CENTER,
        );
        assert_eq!(
            simple,
            LinearGradient::new(
                Alignment::TOP_LEFT,
                Alignment::CENTER,
                vec![Color::RED, Color::BLUE],
                None,
                clamp
            )
        );

        assert_eq!(
            RadialGradient::centered(0.25, c.clone()),
            RadialGradient::new(Alignment::CENTER, 0.25, c.clone(), None, clamp, None, None)
        );
        assert_eq!(
            RadialGradient::circular(c.clone()),
            RadialGradient::new(Alignment::CENTER, 0.5, c.clone(), None, clamp, None, None)
        );
        assert_eq!(
            SweepGradient::centered(c.clone()),
            SweepGradient::new(
                Alignment::CENTER,
                c,
                None,
                clamp,
                0.0,
                std::f32::consts::TAU
            )
        );
    }

    #[test]
    fn rotation_matrix() {
        let m = GradientRotation::new(std::f32::consts::FRAC_PI_2).transform();
        let expected = [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
        for (row, want) in m.iter().zip(expected) {
            for (x, w) in row.iter().zip(want) {
                assert!((x - w).abs() < 1e-6, "{m:?}");
            }
        }
    }
}
