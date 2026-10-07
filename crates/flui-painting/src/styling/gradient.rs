//! Gradient types for styling

// Re-export TileMode from painting module
pub use crate::paint::TileMode;
use crate::{Alignment, styling::Color};

/// A description of a color gradient.
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
/// points.
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

    /// Linearly interpolate between two linear gradients: the result has a
    /// stop wherever either side has one, each coloured by lerping the two
    /// gradients sampled there, so gradients with different colour counts or stops interpolate.
    ///
    /// Returns `None` if either side has no colours, explicit stops do not
    /// match its colours one for one, stops are outside `0..=1` or descending, or
    /// geometry or `t` is non-finite, or extrapolated geometry overflows.
    /// Geometry extrapolates outside `0..=1`; colors saturate at the endpoints.
    /// Repeated stops preserve a hard transition.
    #[inline]
    pub fn lerp(a: &Self, b: &Self, t: f64) -> Option<Self> {
        if !t.is_finite()
            || !valid_alignment(a.begin)
            || !valid_alignment(a.end)
            || !valid_alignment(b.begin)
            || !valid_alignment(b.end)
            || !valid_stops(&a.colors, a.stops.as_deref())
            || !valid_stops(&b.colors, b.stops.as_deref())
        {
            return None;
        }
        // Equal gradients short-circuit to `a`.
        if a == b {
            return Some(a.clone());
        }
        let begin = lerp_alignment(a.begin, b.begin, t)?;
        let end = lerp_alignment(a.end, b.end, t)?;
        let (colors, stops) = interpolate_colors_and_stops(
            (&a.colors, a.stops.as_deref()),
            (&b.colors, b.stops.as_deref()),
            t,
        )?;
        Some(Self {
            begin,
            end,
            colors,
            stops: Some(stops),
            tile_mode: if t < 0.5 { a.tile_mode } else { b.tile_mode },
        })
    }
}

/// A gradient that radiates outward from a center point.
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
    /// Linearly interpolate between two radial gradients. Colours and stops
    /// combine as in [`LinearGradient::lerp`], the radii never go below
    /// zero, and a missing focal radius counts as `0.0`.
    /// Non-finite geometry or negative input radii are rejected.
    ///
    /// A focal point on one side only moves to or from the other side's
    /// *center*, because a gradient without a focal point is focused on its
    /// center: the result at `t = 1` paints exactly like `b`. Lerping
    /// toward `Alignment(0, 0)` instead would jump whenever that center is
    /// anywhere else.
    #[inline]
    pub fn lerp(a: &Self, b: &Self, t: f64) -> Option<Self> {
        if !t.is_finite()
            || !valid_alignment(a.center)
            || !valid_alignment(b.center)
            || !valid_radius(a.radius)
            || !valid_radius(b.radius)
            || a.focal.is_some_and(|focal| !valid_alignment(focal))
            || b.focal.is_some_and(|focal| !valid_alignment(focal))
            || a.focal_radius.is_some_and(|radius| !valid_radius(radius))
            || b.focal_radius.is_some_and(|radius| !valid_radius(radius))
            || !valid_stops(&a.colors, a.stops.as_deref())
            || !valid_stops(&b.colors, b.stops.as_deref())
        {
            return None;
        }
        // Equal gradients short-circuit to `a`.
        if a == b {
            return Some(a.clone());
        }
        let center = lerp_alignment(a.center, b.center, t)?;
        let radius = lerp_radius(a.radius, b.radius, t)?;
        let (colors, stops) = interpolate_colors_and_stops(
            (&a.colors, a.stops.as_deref()),
            (&b.colors, b.stops.as_deref()),
            t,
        )?;

        let focal = match (a.focal, b.focal) {
            (Some(a_focal), Some(b_focal)) => Some(lerp_alignment(a_focal, b_focal, t)?),
            (Some(a_focal), None) => Some(lerp_alignment(a_focal, b.center, t)?),
            (None, Some(b_focal)) => Some(lerp_alignment(a.center, b_focal, t)?),
            (None, None) => None,
        };
        let focal_radius = match (a.focal_radius, b.focal_radius) {
            (None, None) => None,
            (a_r, b_r) => Some(lerp_radius(a_r.unwrap_or(0.0), b_r.unwrap_or(0.0), t)?),
        };
        if focal.unwrap_or(center) == center
            && focal_radius.unwrap_or(0.0) == radius
            && radius != 0.0
        {
            return None;
        }
        Some(Self {
            center,
            radius,
            colors,
            stops: Some(stops),
            tile_mode: if t < 0.5 { a.tile_mode } else { b.tile_mode },
            focal,
            focal_radius,
        })
    }
}

/// A gradient that sweeps through an arc of angles around a center point.
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

    /// Linearly interpolate between two sweep gradients. Colours and stops
    /// combine as in [`LinearGradient::lerp`]; finite signed angles extrapolate.
    #[inline]
    pub fn lerp(a: &Self, b: &Self, t: f64) -> Option<Self> {
        if !t.is_finite()
            || !valid_alignment(a.center)
            || !valid_alignment(b.center)
            || !a.start_angle.is_finite()
            || !b.start_angle.is_finite()
            || !a.end_angle.is_finite()
            || !b.end_angle.is_finite()
            || !valid_stops(&a.colors, a.stops.as_deref())
            || !valid_stops(&b.colors, b.stops.as_deref())
        {
            return None;
        }
        // Equal gradients short-circuit to `a`.
        if a == b {
            return Some(a.clone());
        }
        let center = lerp_alignment(a.center, b.center, t)?;
        let start_angle = lerp_finite(a.start_angle, b.start_angle, t)?;
        let end_angle = lerp_finite(a.end_angle, b.end_angle, t)?;
        let (colors, stops) = interpolate_colors_and_stops(
            (&a.colors, a.stops.as_deref()),
            (&b.colors, b.stops.as_deref()),
            t,
        )?;
        Some(Self {
            center,
            colors,
            stops: Some(stops),
            tile_mode: if t < 0.5 { a.tile_mode } else { b.tile_mode },
            start_angle,
            end_angle,
        })
    }
}

fn valid_alignment(value: Alignment) -> bool {
    value.x.is_finite() && value.y.is_finite()
}

fn valid_radius(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

fn lerp_radius(a: f64, b: f64, t: f64) -> Option<f64> {
    // Admitted radii are nonnegative, so subtraction cannot overflow.
    // Clamp negative infinity before checking the published radius.
    let value = (b - a).mul_add(t, a).max(0.0);
    value.is_finite().then_some(value)
}

fn lerp_alignment(a: Alignment, b: Alignment, t: f64) -> Option<Alignment> {
    Some(Alignment::new(
        lerp_finite(a.x, b.x, t)?,
        lerp_finite(a.y, b.y, t)?,
    ))
}

fn lerp_finite(a: f64, b: f64, t: f64) -> Option<f64> {
    let value = if t == 0.0 || a == b {
        a
    } else if t == 1.0 {
        b
    } else {
        let span = b - a;
        if span.is_finite() {
            span.mul_add(t, a)
        } else {
            // Opposite finite extremes can overflow the subtraction even
            // though their weighted interpolation is representable.
            a * (1.0 - t) + b * t
        }
    };
    value.is_finite().then_some(value)
}

fn valid_stops(colors: &[Color], stops: Option<&[f64]>) -> bool {
    !colors.is_empty()
        && stops.is_none_or(|stops| {
            stops.len() == colors.len()
                && stops.iter().all(|stop| (0.0..=1.0).contains(stop))
                && stops.windows(2).all(|pair| pair[0] <= pair[1])
        })
}

/// The explicit stops, or the colours
/// spread evenly from 0 to 1. `None` for stops that do not pair one for
/// one with the colours, or no colours at all.
fn implied_stops(colors: &[Color], stops: Option<&[f64]>) -> Option<Vec<f64>> {
    if !valid_stops(colors, stops) {
        return None;
    }
    match stops {
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

/// The colour at `t` along `colors` placed at
/// `stops`, holding the end colours beyond the first and last stop.
fn sample(colors: &[Color], stops: &[f64], t: f64, before: bool) -> Color {
    let index = stops.partition_point(|&stop| if before { stop < t } else { stop <= t });
    if index == 0 {
        return colors[0];
    }
    if index == stops.len() {
        return colors[colors.len() - 1];
    }
    Color::lerp(
        colors[index - 1],
        colors[index],
        (t - stops[index - 1]) / (stops[index] - stops[index - 1]),
    )
}

/// A stop wherever either gradient
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
    let mut colors = Vec::with_capacity(stops.len());
    let mut merged_stops = Vec::with_capacity(stops.len());
    for stop in stops {
        let repeated = |stops: &[f64]| {
            stops.partition_point(|&s| s <= stop) - stops.partition_point(|&s| s < stop) > 1
        };
        if repeated(&a_stops) || repeated(&b_stops) {
            merged_stops.push(stop);
            colors.push(Color::lerp(
                sample(a_colors, &a_stops, stop, true),
                sample(b_colors, &b_stops, stop, true),
                t,
            ));
        }
        merged_stops.push(stop);
        colors.push(Color::lerp(
            sample(a_colors, &a_stops, stop, false),
            sample(b_colors, &b_stops, stop, false),
            t,
        ));
    }
    Some((colors, merged_stops))
}

/// Base trait for gradient transformations.
pub trait GradientTransform: std::fmt::Debug {
    /// Transform the gradient according to this transformation.
    ///
    /// Returns a transformation matrix that should be applied to the gradient.
    fn transform(&self) -> [[f64; 3]; 3];
}

/// A gradient transform that rotates the gradient by a fixed angle.
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
