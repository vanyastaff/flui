//! Each participant's own coordinate plane (ADR-0209 §1): the local space
//! of the render node its deltas are measured in, and how a step crosses
//! between that plane and the presentation's.
//!
//! The channel carries what is left of a step in presentation (root)
//! coordinates and converts it at each participant: a delta and a focal
//! point into the participant's plane before it is asked, what it took back
//! out after. A participant inside a scaled, rotated or projected subtree (an
//! `InteractiveViewer`'s content, a `Transform`) is offered the motion as it
//! sees it, and the participants around it share one displacement.
//!
//! Under an affine transform a delta and a velocity map through the
//! transform's linear part. Under a projection a delta is the chord the
//! focal point travels, and a velocity maps through the projection's
//! tangent at the focal point. A plane that cannot be crossed — a singular
//! or non-finite transform, a focal point behind the camera — offers its
//! participant nothing; no conversion publishes a value that is not finite.

use std::cell::RefCell;
use std::rc::Rc;

use flui_foundation::geometry::{Matrix4, Point, Size, Vec2};
use flui_objects::SubtreeAnchor;
use flui_rendering::pipeline::{PipelineOwner, WeakPipelineCell};

/// A conversion's result below this share of its largest component is the
/// rounding of a quarter turn or of an inverse, not motion: it reads as
/// zero, so a vertical delta through a rotated plane stays on one axis.
const ROUNDING: f64 = 1e-12;

/// The 2×2 identity, row by row.
const IDENTITY: [f64; 4] = [1.0, 0.0, 0.0, 1.0];

/// The device pixel ratio of the presentation `pipeline` belongs to, read at
/// the moment of the call so a window moved to another monitor is honoured.
/// `1.0` once the presentation is gone or while a frame holds the pipeline.
pub(crate) fn presentation_device_pixel_ratio(pipeline: Option<&WeakPipelineCell>) -> f64 {
    pipeline
        .and_then(WeakPipelineCell::upgrade)
        .and_then(|cell| cell.try_with(PipelineOwner::device_pixel_ratio))
        .unwrap_or(1.0)
}

/// Where one participant's plane is read: the presentation, acquired in a
/// lifecycle hook, and the render node its deltas are local to, published
/// while it is mounted.
#[derive(Clone, Default)]
pub(crate) struct PlaneSource {
    pipeline: Rc<RefCell<Option<WeakPipelineCell>>>,
    anchor: SubtreeAnchor,
}

impl PlaneSource {
    /// The presentation the plane is read from; set in a lifecycle hook.
    pub(crate) fn set_pipeline(&self, pipeline: Option<WeakPipelineCell>) {
        *self.pipeline.borrow_mut() = pipeline;
    }

    pub(crate) fn pipeline(&self) -> Option<WeakPipelineCell> {
        self.pipeline.borrow().clone()
    }

    /// The anchor to mount around the participant's subtree.
    pub(crate) fn anchor(&self) -> SubtreeAnchor {
        self.anchor.clone()
    }

    /// The plane as the last complete layout placed it; `None` before the
    /// node is laid out, while a frame holds the layout, or once the
    /// presentation is gone.
    pub(crate) fn sample(&self) -> Option<Plane> {
        let pipeline = self.pipeline.borrow().as_ref()?.upgrade()?;
        let node = self.anchor.get()?;
        pipeline
            .try_with_layout(|owner| {
                let size = owner.box_size(node)?;
                let to_root = owner.transform_to(node, owner.root_id()?)?;
                Some(Plane {
                    to_root,
                    size,
                    device_pixel_ratio: Some(owner.device_pixel_ratio())
                        .filter(|ratio| ratio.is_finite() && *ratio > 0.0),
                })
            })
            .flatten()
    }
}

impl std::fmt::Debug for PlaneSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlaneSource")
            .field("anchored", &self.anchor.is_anchored())
            .finish_non_exhaustive()
    }
}

/// One participant's plane: the map from its local space to the
/// presentation's, its box, and the presentation's pixel ratio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Plane {
    to_root: Matrix4,
    size: Size,
    /// The presentation's ratio, when it was read and is finite and
    /// positive.
    device_pixel_ratio: Option<f64>,
}

impl Plane {
    /// The plane of the presentation itself.
    pub(crate) const ROOT: Self = Self {
        to_root: Matrix4::IDENTITY,
        size: Size::ZERO,
        device_pixel_ratio: None,
    };

    /// The presentation's device pixel ratio, when it was read.
    pub(crate) fn device_pixel_ratio(self) -> Option<f64> {
        self.device_pixel_ratio
    }

    /// The middle of the participant's box, in the presentation: the focal
    /// point of input that has no position of its own.
    pub(crate) fn centre(self) -> Option<Point> {
        self.to_root_point(Point::new(self.size.width / 2.0, self.size.height / 2.0))
    }

    /// The homography of the local `z = 0` plane, row by row:
    /// `[x', y', w'] = H · [x, y, 1]`.
    fn homography(self) -> [f64; 9] {
        let m = &self.to_root.m;
        [m[0], m[4], m[12], m[1], m[5], m[13], m[3], m[7], m[15]]
    }

    /// The linear part of an affine plane, `None` for a projection.
    fn linear(self) -> Option<[f64; 4]> {
        let [xx, xy, _, yx, yy, _, wx, wy, ww] = self.homography();
        (wx == 0.0 && wy == 0.0 && ww > 0.0 && ww.is_finite())
            .then(|| [xx / ww, xy / ww, yx / ww, yy / ww])
    }

    pub(crate) fn to_root_point(self, local: Point) -> Option<Point> {
        let (x, y) = self.to_root.project_plane_point(local.x, local.y)?;
        Some(Point::new(x, y))
    }

    pub(crate) fn to_local_point(self, root: Point) -> Option<Point> {
        if self.to_root.is_identity() {
            return root.is_finite().then_some(root);
        }
        let (x, y) = self
            .to_root
            .try_inverse()?
            .unproject_to_plane(root.x, root.y)?;
        Some(Point::new(x, y))
    }

    /// `local`, a delta in this plane at the presentation's `focal` point,
    /// in the presentation.
    pub(crate) fn to_root_delta(self, local: Vec2, focal: Point) -> Option<Vec2> {
        if !local.is_finite() {
            return None;
        }
        if local == Vec2::ZERO {
            return Some(Vec2::ZERO);
        }
        if let Some(linear) = self.linear() {
            return apply(linear, local);
        }
        // The content offset grows against the focal point's travel: the
        // point moves from `focal_local` to `focal_local - local`.
        let focal_local = self.to_local_point(focal)?;
        let end = self.to_root_point(focal_local - local)?;
        settled(focal - end)
    }

    /// `root`, a delta in the presentation at its `focal` point, in this
    /// plane.
    pub(crate) fn to_local_delta(self, root: Vec2, focal: Point) -> Option<Vec2> {
        if !root.is_finite() {
            return None;
        }
        if root == Vec2::ZERO {
            return Some(Vec2::ZERO);
        }
        if let Some(linear) = self.linear() {
            return apply(invert(linear)?, root);
        }
        let focal_local = self.to_local_point(focal)?;
        let end = self.to_local_point(focal - root)?;
        settled(focal_local - end)
    }

    /// `local`, a velocity in this plane, in the presentation: through the
    /// plane's tangent at `focal`, not as a chord a second long.
    pub(crate) fn to_root_velocity(self, local: Vec2, focal: Point) -> Option<Vec2> {
        if !local.is_finite() {
            return None;
        }
        if local == Vec2::ZERO {
            return Some(Vec2::ZERO);
        }
        apply(self.tangent(focal)?, local)
    }

    /// `root`, a velocity in the presentation, in this plane.
    pub(crate) fn to_local_velocity(self, root: Vec2, focal: Point) -> Option<Vec2> {
        if !root.is_finite() {
            return None;
        }
        if root == Vec2::ZERO {
            return Some(Vec2::ZERO);
        }
        apply(invert(self.tangent(focal)?)?, root)
    }

    /// The presentation axis (`0` is `x`) this plane's `local` axis runs
    /// closest to at `focal`; the local axis itself when the plane cannot
    /// be crossed.
    pub(crate) fn root_axis(self, local: usize, focal: Point) -> usize {
        let unit = if local == 0 {
            Vec2::new(1.0, 0.0)
        } else {
            Vec2::new(0.0, 1.0)
        };
        match self.to_root_velocity(unit, focal) {
            Some(root) if root.x.abs() > root.y.abs() => 0,
            Some(root) if root.y.abs() > root.x.abs() => 1,
            _ => local,
        }
    }

    /// The derivative of the map to the presentation at `focal`, as a 2×2
    /// matrix: the linear part of an affine plane, the projection's tangent
    /// otherwise.
    fn tangent(self, focal: Point) -> Option<[f64; 4]> {
        if let Some(linear) = self.linear() {
            return Some(linear);
        }
        let local = self.to_local_point(focal)?;
        let [xx, xy, tx, yx, yy, ty, wx, wy, ww] = self.homography();
        let weight = wx.mul_add(local.x, wy.mul_add(local.y, ww));
        if !(weight > 0.0 && weight.is_finite()) {
            return None;
        }
        let screen_x = xx.mul_add(local.x, xy.mul_add(local.y, tx)) / weight;
        let screen_y = yx.mul_add(local.x, yy.mul_add(local.y, ty)) / weight;
        let tangent = [
            screen_x.mul_add(-wx, xx) / weight,
            screen_x.mul_add(-wy, xy) / weight,
            screen_y.mul_add(-wx, yx) / weight,
            screen_y.mul_add(-wy, yy) / weight,
        ];
        tangent
            .iter()
            .all(|value| value.is_finite())
            .then_some(tangent)
    }
}

/// The 2×2 `matrix` (`[xx, xy, yx, yy]`, row by row) times `vector`, rounded
/// as [`settled`] says.
fn apply(matrix: [f64; 4], vector: Vec2) -> Option<Vec2> {
    if matrix == IDENTITY {
        return Some(vector);
    }
    let [xx, xy, yx, yy] = matrix;
    settled(Vec2::new(
        xx.mul_add(vector.x, xy * vector.y),
        yx.mul_add(vector.x, yy * vector.y),
    ))
}

/// The inverse of the 2×2 `matrix`; `None` when it is singular.
fn invert(matrix: [f64; 4]) -> Option<[f64; 4]> {
    if matrix == IDENTITY {
        return Some(matrix);
    }
    let [xx, xy, yx, yy] = matrix;
    let determinant = xx.mul_add(yy, -xy * yx);
    if determinant == 0.0 || !determinant.is_finite() {
        return None;
    }
    let inverse = [
        yy / determinant,
        -xy / determinant,
        -yx / determinant,
        xx / determinant,
    ];
    inverse
        .iter()
        .all(|value| value.is_finite())
        .then_some(inverse)
}

/// `vector` with the rounding of a conversion cleared, or `None` when it is
/// not finite.
fn settled(vector: Vec2) -> Option<Vec2> {
    if !vector.is_finite() {
        return None;
    }
    let scale = vector.x.abs().max(vector.y.abs()) * ROUNDING;
    let clear = |value: f64| if value.abs() <= scale { 0.0 } else { value };
    Some(Vec2::new(clear(vector.x), clear(vector.y)))
}
