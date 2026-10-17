//! Committed viewport planes and finite displacement/velocity conversion.

use flui_foundation::{
    RenderId,
    geometry::{Axis, Matrix4, Point, Vec2},
};
use flui_rendering::pipeline::WeakPipelineCell;

#[derive(Debug, Clone, Copy)]
pub(super) struct ViewportGeometry {
    forward: Matrix4,
    inverse: Matrix4,
    root_focal: Point,
    pub(super) local_focal: Point,
    pub(super) device_pixel_ratio: f64,
}

impl ViewportGeometry {
    pub(super) fn sample(
        pipeline: &WeakPipelineCell,
        node: RenderId,
        root_focal: Point,
    ) -> Option<Self> {
        let pipeline = pipeline.upgrade()?;
        pipeline
            .try_with_layout(|owner| {
                owner.box_size(node)?;
                let forward = owner.transform_to(node, owner.root_id()?)?;
                let inverse = forward.try_inverse()?;
                let (x, y) = inverse.unproject_to_plane(root_focal.x, root_focal.y)?;
                forward.project_plane_point(x, y)?;
                Some(Self {
                    forward,
                    inverse,
                    root_focal,
                    local_focal: Point::new(x, y),
                    device_pixel_ratio: owner.device_pixel_ratio(),
                })
            })
            .flatten()
    }

    pub(super) fn local_delta(self, root: Vec2) -> Option<Vec2> {
        if !root.is_finite() {
            return None;
        }
        if root == Vec2::ZERO {
            return Some(Vec2::ZERO);
        }
        let endpoint = self.root_focal - root;
        let (x, y) = self.inverse.unproject_to_plane(endpoint.x, endpoint.y)?;
        let delta = self.local_focal - Point::new(x, y);
        delta.is_finite().then_some(delta)
    }

    pub(super) fn root_delta(self, local: Vec2) -> Option<Vec2> {
        if !local.is_finite() {
            return None;
        }
        if local == Vec2::ZERO {
            return Some(Vec2::ZERO);
        }
        let endpoint = self.local_focal - local;
        let (x, y) = self.forward.project_plane_point(endpoint.x, endpoint.y)?;
        let (origin_x, origin_y) = self
            .forward
            .project_plane_point(self.local_focal.x, self.local_focal.y)?;
        let delta = Vec2::new(origin_x - x, origin_y - y);
        delta.is_finite().then_some(delta)
    }

    /// Refuse a partial-consumption rectangle before invoking authored code if
    /// any admitted component choice could cross the camera or overflow the
    /// presentation-space budget. Its extrema lie at these four corners.
    pub(super) fn offer(self, remaining: Vec2, total: Vec2) -> Option<Vec2> {
        let local = self.local_delta(remaining)?;
        for partial in [
            Vec2::ZERO,
            Vec2::new(local.x, 0.0),
            Vec2::new(0.0, local.y),
            local,
        ] {
            let consumed = self.root_delta(partial)?;
            let next = remaining - consumed;
            if !next.is_finite() || !(total - next).is_finite() {
                return None;
            }
        }
        Some(local)
    }

    fn tangent(self) -> Option<[Vec2; 2]> {
        let m = self.forward.m;
        let scale = [m[0], m[1], m[3], m[4], m[5], m[7], m[12], m[13], m[15]]
            .into_iter()
            .fold(0.0_f64, |scale, value| scale.max(value.abs()));
        if scale == 0.0 {
            return None;
        }
        let m = m.map(|value| value / scale);
        let (root_x, root_y) = self
            .forward
            .project_plane_point(self.local_focal.x, self.local_focal.y)?;
        let point_scale = self
            .local_focal
            .x
            .abs()
            .max(self.local_focal.y.abs())
            .max(1.0);
        let weight = m[3] * (self.local_focal.x / point_scale)
            + m[7] * (self.local_focal.y / point_scale)
            + m[15] / point_scale;
        if weight <= 0.0 || !weight.is_finite() {
            return None;
        }
        let divide = |numerator: f64| {
            let denominator = weight * point_scale;
            if denominator.is_finite() {
                numerator / denominator
            } else {
                (numerator / point_scale) / weight
            }
        };
        let x = Vec2::new(
            divide(root_x.mul_add(-m[3], m[0])),
            divide(root_y.mul_add(-m[3], m[1])),
        );
        let y = Vec2::new(
            divide(root_x.mul_add(-m[7], m[4])),
            divide(root_y.mul_add(-m[7], m[5])),
        );
        (x.is_finite() && y.is_finite()).then_some([x, y])
    }

    pub(super) fn root_velocity(self, local: Vec2) -> Option<Vec2> {
        let [x, y] = self.tangent()?;
        let result = Vec2::new(
            x.x.mul_add(local.x, y.x * local.y),
            x.y.mul_add(local.x, y.y * local.y),
        );
        result.is_finite().then_some(result)
    }

    pub(super) fn local_velocity(self, root: Vec2) -> Option<Vec2> {
        let [x, y] = self.tangent()?;
        let scale = x.x.abs().max(x.y.abs()).max(y.x.abs()).max(y.y.abs());
        if scale == 0.0 {
            return None;
        }
        let (xx, xy, yx, yy) = (x.x / scale, y.x / scale, x.y / scale, y.y / scale);
        let determinant = xx.mul_add(yy, -xy * yx);
        if determinant == 0.0 || !determinant.is_finite() {
            return None;
        }
        let root = root / scale;
        let result = Vec2::new(
            yy.mul_add(root.x, -xy * root.y) / determinant,
            xx.mul_add(root.y, -yx * root.x) / determinant,
        );
        result.is_finite().then_some(result)
    }

    pub(super) fn root_axis(self, axis: Axis) -> Option<Vec2> {
        let [x, y] = self.tangent()?;
        let axis = match axis {
            Axis::Horizontal => x,
            Axis::Vertical => y,
        };
        let scale = axis.x.abs().max(axis.y.abs());
        if scale == 0.0 {
            return None;
        }
        let axis = axis / scale;
        let unit = axis / axis.x.hypot(axis.y);
        unit.is_finite().then_some(unit)
    }
}
