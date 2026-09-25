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
    /// Returns None if the gradients are of different types or have
    /// different numbers of colors.
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

    /// Linearly interpolate between two linear gradients.
    ///
    /// Returns None if the gradients have different numbers of colors.
    #[inline]
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

    /// Linearly interpolate between two radial gradients.
    #[inline]
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

    /// Linearly interpolate between two sweep gradients.
    #[inline]
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

    #[test]
    fn linear_lerp() {
        let a = linear(two(), [0.0, 0.5], TileMode::Clamp);
        let b = LinearGradient {
            begin: Alignment::BOTTOM_RIGHT,
            ..linear(two_b(), [0.5, 1.0], TileMode::Mirror)
        };
        assert_eq!(LinearGradient::lerp(&a, &b, 0.0), Some(a.clone()));
        assert_eq!(LinearGradient::lerp(&a, &b, 1.0), Some(b.clone()));
        let mid = LinearGradient::lerp(&a, &b, 0.5).unwrap();
        assert_eq!(mid.begin, Alignment::CENTER);
        assert_eq!(
            mid.colors,
            vec![Color::rgb(50, 25, 125), Color::rgb(100, 50, 25)]
        );
        assert_eq!(mid.stops, Some(vec![0.25, 0.75]));
        // The tile mode switches to `b`'s at exactly t = 0.5.
        assert_eq!(mid.tile_mode, TileMode::Mirror);
        assert_eq!(
            LinearGradient::lerp(&a, &b, 0.25).unwrap().tile_mode,
            TileMode::Clamp
        );
        // t is clamped.
        assert_eq!(LinearGradient::lerp(&a, &b, 3.0), Some(b.clone()));
    }

    #[test]
    fn radial_lerp() {
        let (a, b) = (radial(1.0, [0.0, 0.5], 0.0), radial(3.0, [0.5, 1.0], 1.0));
        assert_eq!(RadialGradient::lerp(&a, &b, 0.0), Some(a.clone()));
        assert_eq!(RadialGradient::lerp(&a, &b, 1.0), Some(b.clone()));
        let mid = RadialGradient::lerp(&a, &b, 0.5).unwrap();
        assert_eq!(mid.center, Alignment::new(0.5, 0.0));
        assert_eq!(mid.radius, 2.0);
        assert_eq!(mid.stops, Some(vec![0.25, 0.75]));
        assert_eq!(mid.focal, Some(Alignment::new(0.5, 0.5)));
        assert_eq!(mid.focal_radius, Some(0.25));
        assert_eq!(mid.tile_mode, TileMode::Mirror);
        assert_eq!(
            RadialGradient::lerp(&a, &b, 0.25).unwrap().tile_mode,
            TileMode::Clamp
        );
        // A focal point or radius on only one side is dropped.
        let unfocused = RadialGradient {
            focal: None,
            focal_radius: None,
            ..b.clone()
        };
        let mid = RadialGradient::lerp(&a, &unfocused, 0.5).unwrap();
        assert_eq!((mid.focal, mid.focal_radius), (None, None));
        // Both focal radii non-zero: 0.25 to 0.5.
        let mid = RadialGradient::lerp(&radial(1.0, [0.0, 0.5], 0.5), &b, 0.5).unwrap();
        assert_eq!(mid.focal_radius, Some(0.375));
    }

    #[test]
    fn sweep_lerp() {
        let a = sweep(1.0, 2.0);
        let b = SweepGradient {
            stops: Some(vec![0.5, 1.0]),
            tile_mode: TileMode::Mirror,
            ..sweep(3.0, 4.0)
        };
        assert_eq!(SweepGradient::lerp(&a, &b, 0.0), Some(a.clone()));
        assert_eq!(SweepGradient::lerp(&a, &b, 1.0), Some(b.clone()));
        let mid = SweepGradient::lerp(&a, &b, 0.5).unwrap();
        assert_eq!((mid.start_angle, mid.end_angle), (2.0, 3.0));
        assert_eq!(mid.stops, Some(vec![0.25, 1.0]));
        assert_eq!(mid.tile_mode, TileMode::Mirror);
        let early = SweepGradient::lerp(&a, &b, 0.25).unwrap();
        assert_eq!(early.tile_mode, TileMode::Repeat);
    }

    /// Stops survive only when both sides have the same number; colors
    /// must match in count or there is no lerp at all.
    #[test]
    fn lerp_requires_matching_lengths() {
        let a = linear(two(), [0.0, 0.5], TileMode::Clamp);
        let three = linear(vec![Color::BLACK; 3], [0.0, 0.5], TileMode::Clamp);
        assert_eq!(LinearGradient::lerp(&a, &three, 0.5), None);
        let other_stops = LinearGradient {
            stops: Some(vec![0.0, 0.5, 1.0]),
            ..a.clone()
        };
        assert_eq!(
            LinearGradient::lerp(&a, &other_stops, 0.5).unwrap().stops,
            None
        );
        let no_stops = LinearGradient {
            stops: None,
            ..a.clone()
        };
        assert_eq!(
            LinearGradient::lerp(&a, &no_stops, 0.5).unwrap().stops,
            None
        );

        let r3 = RadialGradient {
            colors: vec![Color::BLACK; 3],
            ..radial(1.0, [0.0, 1.0], 0.0)
        };
        assert_eq!(
            RadialGradient::lerp(&radial(1.0, [0.0, 1.0], 0.0), &r3, 0.5),
            None
        );
        let s3 = SweepGradient {
            colors: vec![Color::BLACK; 3],
            ..sweep(0.0, 1.0)
        };
        assert_eq!(SweepGradient::lerp(&sweep(0.0, 1.0), &s3, 0.5), None);

        // Same colours, different stop counts: the stops are dropped.
        let r = radial(1.0, [0.0, 1.0], 0.0);
        let r_other = RadialGradient {
            stops: Some(vec![0.0, 0.5, 1.0]),
            ..r.clone()
        };
        assert_eq!(RadialGradient::lerp(&r, &r_other, 0.5).unwrap().stops, None);
        let s = sweep(0.0, 1.0);
        let s_other = SweepGradient {
            stops: Some(vec![0.0, 0.5, 1.0]),
            ..s.clone()
        };
        assert_eq!(SweepGradient::lerp(&s, &s_other, 0.5).unwrap().stops, None);
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
