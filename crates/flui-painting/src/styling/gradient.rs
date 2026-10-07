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
        let begin = lerp_alignment(a.begin, b.begin, t)?;
        let end = lerp_alignment(a.end, b.end, t)?;
        if !(end.x - begin.x).is_finite()
            || !(end.y - begin.y).is_finite()
            || distorted_span(a.begin.x, a.end.x, b.begin.x, b.end.x, end.x - begin.x, t)
            || distorted_span(a.begin.y, a.end.y, b.begin.y, b.end.y, end.y - begin.y, t)
            || !valid_linear_projection(begin, end)
        {
            return None;
        }
        // Preserve the representation of valid equal gradients.
        if a == b {
            return Some(a.clone());
        }
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
    /// Non-finite geometry, negative input radii, and unrepresentable
    /// normalized circle values are rejected before paint-bounds resolution.
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
        let center = lerp_alignment(a.center, b.center, t)?;
        let radius = lerp_radius(a.radius, b.radius, t)?;

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
        let a_focal = a.focal.unwrap_or(a.center);
        let b_focal = b.focal.unwrap_or(b.center);
        let mixed_focal = focal.unwrap_or(center);
        if distorted_span(
            a.center.x,
            a_focal.x,
            b.center.x,
            b_focal.x,
            mixed_focal.x - center.x,
            t,
        ) || distorted_span(
            a.center.y,
            a_focal.y,
            b.center.y,
            b_focal.y,
            mixed_focal.y - center.y,
            t,
        ) || !valid_normalized_circles(center, mixed_focal, radius, focal_radius.unwrap_or(0.0))
        {
            return None;
        }
        if focal.unwrap_or(center) == center
            && focal_radius.unwrap_or(0.0) == radius
            && radius != 0.0
        {
            return None;
        }
        // Equal gradients preserve their representation after validating geometry.
        if a == b {
            return Some(a.clone());
        }
        let (colors, stops) = interpolate_colors_and_stops(
            (&a.colors, a.stops.as_deref()),
            (&b.colors, b.stops.as_deref()),
            t,
        )?;
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
    /// Returns `None` when the phase-reduced angle span cannot be represented
    /// by the renderer's `f32` angles, including a nonzero span lost to rounding.
    #[inline]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "narrowing checks the renderer's angle representation before publication"
    )]
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
        let center = lerp_alignment(a.center, b.center, t)?;
        let start_angle = lerp_finite(a.start_angle, b.start_angle, t)?;
        let end_angle = lerp_finite(a.end_angle, b.end_angle, t)?;
        // The renderer reduces the phase before narrowing; absolute angles
        // can exceed f32 while their signed span remains representable.
        let span = end_angle - start_angle;
        let phase = start_angle.rem_euclid(std::f64::consts::TAU);
        let packed_end = (phase + span) as f32;
        let packed_span = packed_end - phase as f32;
        // Extrapolation must not amplify packing error into a changed angular
        // scale. Bounded interpolation and unchanged gradients retain the
        // renderer's existing small-span representation.
        if !span.is_finite()
            || distorted_span(
                a.start_angle,
                a.end_angle,
                b.start_angle,
                b.end_angle,
                span,
                t,
            )
            || !packed_end.is_finite()
            || !packed_span.is_finite()
            || ![center.x, center.y].into_iter().all(|value| {
                let local = value.mul_add(0.5, 0.5);
                (local as f32).is_finite() && (local == 0.0 || local as f32 != 0.0)
            })
            || (span != 0.0 && packed_span == 0.0)
            || (!(0.0..=1.0).contains(&t)
                && (a.start_angle != b.start_angle || a.end_angle != b.end_angle)
                && (f64::from(packed_span) - span).abs()
                    > span.abs().max(f64::from(packed_span).abs()) * f64::from(f32::EPSILON))
        {
            return None;
        }
        // Preserve the representation of valid equal gradients.
        if a == b {
            return Some(a.clone());
        }
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

// Coordinate interpolation can distort a span under a large common translation.
// Compare independently interpolated spans, allowing rounding below the
// renderer's relative f32 precision rather than rejecting ordinary f64 noise.
fn distorted_span(a_start: f64, a_end: f64, b_start: f64, b_end: f64, mixed: f64, t: f64) -> bool {
    if !mixed.is_finite() {
        return true;
    }
    let Some((a, a_error)) = span_parts(a_end, a_start) else {
        return false;
    };
    let Some((b, b_error)) = span_parts(b_end, b_start) else {
        return false;
    };
    let Some((span, error)) = lerp_finite(a, b, t).zip(lerp_finite(a_error, b_error, t)) else {
        return true;
    };
    let span = span + error;
    !span.is_finite()
        || (span - mixed).abs() > span.abs().max(mixed.abs()) * f64::from(f32::EPSILON)
}

// Error-free TwoDiff decomposition: keep the low part when opposite large
// endpoint spans cancel, so their finite residual is not mistaken for zero.
fn span_parts(end: f64, start: f64) -> Option<(f64, f64)> {
    let span = end - start;
    if !span.is_finite() {
        return None;
    }
    let virtual_start = end - span;
    let virtual_end = span + virtual_start;
    Some((span, (end - virtual_end) + (virtual_start - start)))
}

fn valid_radius(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

fn valid_normalized_circles(
    center: Alignment,
    focal: Alignment,
    radius: f64,
    focal_radius: f64,
) -> bool {
    // Normalize relative to a unit paint box, using the engine's shared circle
    // scale. A raw radius above f32::MAX can still pack successfully. Actual
    // bounds remain engine checks.
    let positions = [center.x, center.y, focal.x, focal.y].map(|value| value.mul_add(0.5, 0.5));
    valid_circles(positions, radius, focal_radius, [1.0, 1.0])
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "validate normalized renderer packing"
)]
fn valid_circles(positions: [f64; 4], radius: f64, focal_radius: f64, bounds: [f64; 2]) -> bool {
    if !positions.into_iter().chain(bounds).all(f64::is_finite)
        || !valid_radius(radius)
        || !valid_radius(focal_radius)
        || (radius != 0.0
            && radius == focal_radius
            && positions[0] == positions[2]
            && positions[1] == positions[3])
    {
        return false;
    }
    let scale = positions.iter().fold(
        radius
            .max(focal_radius)
            .max(bounds[0].abs())
            .max(bounds[1].abs()),
        |scale, value| scale.max(value.abs()),
    );
    let scale = if scale == 0.0 { 1.0 } else { scale };
    let values = [
        positions[0] / scale,
        positions[1] / scale,
        radius / scale,
        positions[2] / scale,
        positions[3] / scale,
        focal_radius / scale,
        scale.recip(),
    ];
    if !values
        .into_iter()
        .all(|value| (value as f32).is_finite() && (value == 0.0 || value as f32 != 0.0))
    {
        return false;
    }
    let packed = values.map(|value| value as f32);
    if ![(bounds[0], packed[3]), (bounds[1], packed[4])]
        .into_iter()
        .all(|(dimension, focal)| {
            let normalized = dimension as f32 * packed[6];
            normalized.is_finite()
                && (dimension == 0.0 || (normalized != 0.0 && normalized - focal != -focal))
        })
    {
        return false;
    }
    let dx = packed[0] - packed[3];
    let dy = packed[1] - packed[4];
    let dr = packed[2] - packed[5];
    let quadratic = dx * dx + dy * dy - dr * dr;
    let native_dx = values[0] - values[3];
    let native_dy = values[1] - values[4];
    let native_dr = values[2] - values[5];
    let native_quadratic = native_dx * native_dx + native_dy * native_dy - native_dr * native_dr;
    if !quadratic.is_finite()
        || (native_quadratic == 0.0) != (quadratic == 0.0)
        || (native_quadratic > 0.0 && quadratic < 0.0)
        || (native_quadratic < 0.0 && quadratic > 0.0)
    {
        return false;
    }
    [(0, 3), (1, 4), (2, 5)].into_iter().all(|(outer, inner)| {
        values[outer] - values[inner] == 0.0 || packed[outer] - packed[inner] != 0.0
    })
}

fn valid_linear_projection(begin: Alignment, end: Alignment) -> bool {
    valid_resolved_linear_projection(
        [begin.x.mul_add(0.5, 0.5), begin.y.mul_add(0.5, 0.5)],
        [(end.x - begin.x) * 0.5, (end.y - begin.y) * 0.5],
        [1.0, 1.0],
    )
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "validate bounds-local renderer packing"
)]
pub(crate) fn valid_bounds(
    gradient: &Gradient,
    bounds: flui_foundation::geometry::Rect<f64>,
) -> bool {
    let center = bounds.center();
    let local = |alignment: Alignment| {
        [
            (center.x + alignment.x * (bounds.width() / 2.0)) - bounds.left(),
            (center.y + alignment.y * (bounds.height() / 2.0)) - bounds.top(),
        ]
    };
    match gradient {
        Gradient::Linear(linear) => valid_linear_bounds(linear, bounds),
        Gradient::Radial(radial) => {
            let center = local(radial.center);
            let focal = local(radial.focal.unwrap_or(radial.center));
            let half_side = (bounds.width() / 2.0).min(bounds.height() / 2.0);
            valid_circles(
                [center[0], center[1], focal[0], focal[1]],
                radial.radius * half_side * 2.0,
                radial.focal_radius.unwrap_or(0.0) * half_side * 2.0,
                [bounds.width(), bounds.height()],
            )
        }
        Gradient::Sweep(sweep) => local(sweep.center).into_iter().all(|value| {
            value.is_finite() && (value as f32).is_finite() && (value == 0.0 || value as f32 != 0.0)
        }),
    }
}

// The decoration producer knows the real paint box, unlike interpolation.
fn valid_linear_bounds(
    gradient: &LinearGradient,
    bounds: flui_foundation::geometry::Rect<f64>,
) -> bool {
    let center = bounds.center();
    let at = |alignment: Alignment| {
        [
            center.x + alignment.x * (bounds.width() / 2.0),
            center.y + alignment.y * (bounds.height() / 2.0),
        ]
    };
    let from = at(gradient.begin);
    let to = at(gradient.end);
    valid_resolved_linear_projection(
        [from[0] - bounds.left(), from[1] - bounds.top()],
        [to[0] - from[0], to[1] - from[1]],
        [bounds.width(), bounds.height()],
    )
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "validate renderer projection packing"
)]
fn valid_resolved_linear_projection(start: [f64; 2], delta: [f64; 2], bounds: [f64; 2]) -> bool {
    if !start.into_iter().chain(delta).all(f64::is_finite) {
        return false;
    }
    let scale = delta[0].abs().max(delta[1].abs());
    if scale == 0.0 {
        return true;
    }
    let normalized = delta.map(|value| value / scale);
    let norm = normalized[0] * normalized[0] + normalized[1] * normalized[1];
    if scale <= 0.01 && scale * scale * norm <= 0.0001 {
        return true;
    }
    let [a, b] = normalized.map(|value| (value / norm) / scale);
    let c = -(start[0] * a + start[1] * b);
    let coefficients = [a, b, c];
    if !coefficients.into_iter().all(|value| {
        value.is_finite() && (value as f32).is_finite() && (value == 0.0 || value as f32 != 0.0)
    }) {
        return false;
    }
    let packed = coefficients.map(|value| value as f32);
    let bound = f64::from(packed[0]).abs() * f64::from(bounds[0] as f32)
        + f64::from(packed[1]).abs() * f64::from(bounds[1] as f32)
        + f64::from(packed[2]).abs();
    bound.is_finite() && bound <= f64::from(f32::MAX) * 0.5
}

fn lerp_radius(a: f64, b: f64, t: f64) -> Option<f64> {
    // Admitted radii are nonnegative, so subtraction cannot overflow.
    // Clamp negative infinity before checking the published radius.
    let value = (b - a).mul_add(t, a).max(0.0);
    valid_radius(value).then_some(value)
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
