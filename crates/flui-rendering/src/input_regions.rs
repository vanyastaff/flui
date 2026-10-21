//! Input regions: screen areas a render object claims for the application's own input,
//! collected per window at compositing and handed to the OS (ADR-0217 §5).
//!
//! A render object reports regions from
//! [`RenderObject::describe_input_regions`](crate::traits::RenderObject::describe_input_regions)
//! as rectangles in its own local coordinates, into the frame's [`InputRegionSink`].
//! [`PipelineOwner::input_regions`](crate::pipeline::PipelineOwner::input_regions) walks the
//! laid-out tree and maps each rectangle into window-logical coordinates:
//!
//! - through the committed paint transform (`RenderObject::apply_paint_transform`), taking
//!   the axis-aligned bounding box under a rotation, skew or perspective;
//! - intersected with every ancestor's approximate paint clip and with the window, and
//!   dropped when nothing is left;
//! - dropped with a diagnostic when a corner is not finite (an overflowing transform), or
//!   when under a perspective transform a corner's homogeneous `w` is not above a small
//!   epsilon. `w` is affine over the rectangle, so four positive corners mean no point of it
//!   reaches the `w = 0` horizon; a region that does would project to an unbounded or
//!   mirrored shape even when its corner quotients are finite.
//!
//! The walk follows what paint draws: it skips a child its parent did not lay out this
//! pass, a child the parent does not present (`RenderObject::visits_child_for_semantics`,
//! answered by a theater's covered entries, an offstage child, a transparent animated
//! opacity), a node that skips paint, a node still needing layout and a sliver that is not
//! visible.

use flui_foundation::geometry::{Matrix4, Point, Rect};
use smallvec::SmallVec;

pub use flui_platform::input_regions::{InputRegion, InputRegionKind};

/// Where a render object writes its input regions for one frame, in its own local
/// coordinates. Only the pipeline creates one.
#[derive(Debug, Default)]
pub struct InputRegionSink {
    pending: SmallVec<[(InputRegionKind, Rect); 2]>,
}

impl InputRegionSink {
    /// Claims `rect`, in the reporting render object's local coordinates, as a `kind`
    /// region. An empty `rect` claims nothing; a non-finite one is dropped with a
    /// diagnostic.
    pub fn add(&mut self, kind: InputRegionKind, rect: Rect) {
        self.pending.push((kind, rect));
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub(crate) fn drain(&mut self) -> impl Iterator<Item = (InputRegionKind, Rect)> + '_ {
        self.pending.drain(..)
    }
}

/// A homogeneous `w` at or below this is treated as reaching the perspective horizon.
const HORIZON_EPSILON: f64 = 1e-9;

/// Why a region could not be mapped into window coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnmappableRegion {
    /// A transformed corner, or the transform itself, is not finite.
    NonFinite,
    /// A corner sits at or behind the perspective horizon (`w <= epsilon`).
    BeyondHorizon,
}

/// `rect` mapped through `transform` to its axis-aligned bounds, or why it cannot be.
pub(crate) fn map_rect(transform: &Matrix4, rect: Rect) -> Result<Rect, UnmappableRegion> {
    if ![rect.left(), rect.top(), rect.right(), rect.bottom()]
        .iter()
        .all(|edge| edge.is_finite())
    {
        return Err(UnmappableRegion::NonFinite);
    }
    let m = transform.to_col_major_array();
    if m.iter().any(|value| !value.is_finite()) {
        return Err(UnmappableRegion::NonFinite);
    }
    let corners = [
        rect.min,
        Point::new(rect.max.x, rect.min.y),
        rect.max,
        Point::new(rect.min.x, rect.max.y),
    ];
    let mut min = Point::new(f64::INFINITY, f64::INFINITY);
    let mut max = Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for corner in corners {
        let x = m[0] * corner.x + m[4] * corner.y + m[12];
        let y = m[1] * corner.x + m[5] * corner.y + m[13];
        let w = m[3] * corner.x + m[7] * corner.y + m[15];
        if !w.is_finite() || w <= HORIZON_EPSILON {
            return Err(if w.is_finite() {
                UnmappableRegion::BeyondHorizon
            } else {
                UnmappableRegion::NonFinite
            });
        }
        let (x, y) = (x / w, y / w);
        if !x.is_finite() || !y.is_finite() {
            return Err(UnmappableRegion::NonFinite);
        }
        min = Point::new(min.x.min(x), min.y.min(y));
        max = Point::new(max.x.max(x), max.y.max(y));
    }
    let bounds = Rect::from_ltrb(min.x, min.y, max.x, max.y);
    if bounds.width().is_finite() && bounds.height().is_finite() {
        Ok(bounds)
    } else {
        Err(UnmappableRegion::NonFinite)
    }
}
