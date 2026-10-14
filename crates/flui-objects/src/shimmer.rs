//! The highlight band a shimmer sweeps across a box, shared by the skeleton
//! placeholder ([`RenderSkeleton`](crate::RenderSkeleton)) and the shimmer
//! mask over arbitrary content ([`RenderShimmerMask`](crate::RenderShimmerMask)).

use flui_foundation::geometry::{Offset, Size};
use flui_painting::paint::{Shader, TileMode};
use flui_painting::styling::Color;
use flui_rendering::constraints::AxisDirection;

/// A highlight band swept across a box: its colour, the direction it travels
/// and its width as a fraction of the box's extent along that direction.
///
/// At progress `0` the band sits just before the box's leading edge and at
/// `1` just past its trailing edge, so both ends leave the box untouched and a
/// repeating sweep has no visible seam. Outside the band the gradient holds
/// its edge colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShimmerBand {
    highlight: Color,
    direction: AxisDirection,
    width: f64,
}

impl ShimmerBand {
    /// The band's width when none is configured: two fifths of the extent.
    pub const DEFAULT_WIDTH: f64 = 0.4;

    /// A band of `highlight` travelling left to right, [`DEFAULT_WIDTH`]
    /// wide.
    ///
    /// [`DEFAULT_WIDTH`]: Self::DEFAULT_WIDTH
    #[must_use]
    pub const fn new(highlight: Color) -> Self {
        Self {
            highlight,
            direction: AxisDirection::LeftToRight,
            width: Self::DEFAULT_WIDTH,
        }
    }

    /// Travel in `direction` instead.
    #[must_use]
    pub const fn with_direction(mut self, direction: AxisDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Make the band `fraction` of the extent wide. A fraction that is not
    /// finite and positive paints [`DEFAULT_WIDTH`](Self::DEFAULT_WIDTH).
    #[must_use]
    pub const fn with_width(mut self, fraction: f64) -> Self {
        self.width = fraction;
        self
    }

    /// The band's colour at its centre.
    #[must_use]
    pub const fn highlight(&self) -> Color {
        self.highlight
    }

    /// The direction the band travels.
    #[must_use]
    pub const fn direction(&self) -> AxisDirection {
        self.direction
    }

    /// The band's width as a fraction of the extent.
    #[must_use]
    pub const fn width(&self) -> f64 {
        self.width
    }

    /// The gradient of the band at `progress` across a box of `extent` whose
    /// top-left corner sits at `origin` in the painter's coordinates, fading
    /// to `edge` on both sides.
    ///
    /// `None` when there is nothing to sweep: an empty or non-finite extent
    /// along the direction, or a non-finite progress or origin.
    #[must_use]
    pub fn gradient(
        &self,
        progress: f64,
        origin: Offset,
        extent: Size,
        edge: Color,
    ) -> Option<Shader> {
        let (length, start, step) = match self.direction {
            AxisDirection::LeftToRight => (extent.width, origin.dx, 1.0),
            AxisDirection::RightToLeft => (extent.width, origin.dx + extent.width, -1.0),
            AxisDirection::TopToBottom => (extent.height, origin.dy, 1.0),
            AxisDirection::BottomToTop => (extent.height, origin.dy + extent.height, -1.0),
        };
        if !(length.is_finite() && length > 0.0 && progress.is_finite()) {
            return None;
        }
        let fraction = if self.width.is_finite() && self.width > 0.0 {
            self.width
        } else {
            Self::DEFAULT_WIDTH
        };
        let band = length * fraction;
        // The band's leading edge, measured from the box's leading edge along
        // the travel: one band before it at 0, the whole extent past it at 1.
        let lead = progress * (length + band) - band;
        let along = |distance: f64| start + step * distance;
        let (from, to) = (along(lead), along(lead + band));
        let point = |position: f64| match self.direction {
            AxisDirection::LeftToRight | AxisDirection::RightToLeft => {
                Offset::new(position, origin.dy)
            }
            AxisDirection::TopToBottom | AxisDirection::BottomToTop => {
                Offset::new(origin.dx, position)
            }
        };
        let (from, to) = (point(from), point(to));
        [from.dx, from.dy, to.dx, to.dy]
            .iter()
            .all(|value| value.is_finite())
            .then(|| {
                Shader::linear_gradient(
                    from,
                    to,
                    vec![edge, self.highlight, edge],
                    None,
                    TileMode::Clamp,
                )
            })
    }
}

/// One instant of a sweep: the band and how far it has travelled, `0..=1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShimmerSweep {
    /// The band being swept.
    pub band: ShimmerBand,
    /// How far it has travelled: `0` before the leading edge, `1` past the
    /// trailing one.
    pub progress: f64,
}
