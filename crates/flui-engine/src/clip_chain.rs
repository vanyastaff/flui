//! Persistent, admitted clip expressions and bounded mature-curve lowering.
use std::sync::Arc;

use crate::{
    clip_geometry::{ValidatedAffine, ValidatedClip},
    clip_mask::{MaskEdge, MaskNode},
    command_ir::RecordError,
    error::{EngineError, EngineResult, GeometryError},
    recording_budget::RecordingBudget,
};
use flui_foundation::geometry::{Point, Radius, Rect};
use flui_painting::paint::{PathCommand, PathFillType};
use lyon::path::{PathEvent, iterator::Flattened};

pub(crate) const MAX_CLIP_DEPTH: usize = 64;
pub(crate) const MAX_CLIP_EDGES: usize = 512;
pub(crate) const MAX_PATH_COMMANDS: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClipOp {
    Intersect,
    Difference,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ClipChain(Option<Arc<ClipLink>>);

#[derive(Debug)]
struct ClipLink {
    parent: ClipChain,
    shape: ValidatedClip,
    affine: ValidatedAffine,
    op: ClipOp,
    hard: bool,
    depth: usize,
    charge: ClipCharge,
}

#[derive(Debug)]
struct ClipCharge {
    budget: Arc<RecordingBudget>,
    bytes: usize,
}
impl Drop for ClipCharge {
    fn drop(&mut self) {
        self.budget.release(self.bytes, 1);
    }
}

impl PartialEq for ClipChain {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }
}
impl Eq for ClipChain {}

impl std::hash::Hash for ClipChain {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let identity = self.0.as_ref().map_or(0, |link| Arc::as_ptr(link) as usize);
        std::hash::Hash::hash(&identity, state);
    }
}

pub(crate) struct ClipTape {
    pub(crate) nodes: Vec<MaskNode>,
    pub(crate) edges: Vec<MaskEdge>,
}

impl ClipChain {
    /// Conservative root-space output bound; Difference never shrinks it.
    /// None is unbounded. A disjoint or singular intersection is empty.
    pub(crate) fn root_bounds(&self) -> EngineResult<Option<Rect<f64>>> {
        let mut result: Option<Rect<f64>> = None;
        let mut cursor = self.0.as_deref();
        while let Some(link) = cursor {
            if matches!(link.op, ClipOp::Intersect) {
                let local = match &link.shape {
                    ValidatedClip::Rect(rect) => *rect,
                    ValidatedClip::RRect(rect) => rect.rect,
                    ValidatedClip::RSuperellipse(rect) => rect.outer_rect(),
                    ValidatedClip::Path(path) => path.compute_bounds(),
                };
                let Some(mapped) = link.affine.map_bounds(local)? else {
                    return Ok(Some(Rect::ZERO));
                };
                result = Some(match result {
                    Some(previous) => previous.intersect(&mapped).unwrap_or(Rect::ZERO),
                    None => mapped,
                });
            }
            cursor = link.parent.0.as_deref();
        }
        Ok(result)
    }
    pub(crate) fn has_antialias(&self) -> bool {
        let mut cursor = self.0.as_deref();
        while let Some(link) = cursor {
            if !link.hard {
                return true;
            }
            cursor = link.parent.0.as_deref();
        }
        false
    }
    pub(crate) fn is_unclipped(&self) -> bool {
        self.0.is_none()
    }

    pub(crate) fn append(
        &self,
        shape: ValidatedClip,
        affine: ValidatedAffine,
        op: ClipOp,
        hard: bool,
        budget: &Arc<RecordingBudget>,
    ) -> Result<Self, RecordError> {
        if let Some(error) = budget.error() {
            return Err(error);
        }
        if affine.is_empty() && matches!(op, ClipOp::Difference) {
            return Ok(self.clone());
        }
        let depth = self.0.as_ref().map_or(1, |node| node.depth + 1);
        if depth > MAX_CLIP_DEPTH {
            let error = RecordError::Limit {
                resource: "clip chain depth",
                requested: depth,
                limit: MAX_CLIP_DEPTH,
            };
            budget.record_error(error.clone());
            return Err(error);
        }
        let commands = shape.as_path().map_or(0, |path| path.commands().len());
        if commands > MAX_PATH_COMMANDS {
            let error = RecordError::Geometry(GeometryError::PathCommandLimit {
                requested: commands,
                limit: MAX_PATH_COMMANDS,
            });
            budget.record_error(error.clone());
            return Err(error);
        }
        // Conservatively charge shared COW command storage for each admitted
        // owner, plus the Arc's two counters; this is not allocator RSS.
        let bytes = std::mem::size_of::<ClipLink>()
            + 2 * std::mem::size_of::<usize>()
            + commands * std::mem::size_of::<PathCommand>();
        if !budget.charge(bytes, 1) {
            return Err(budget
                .error()
                .expect("BUG: failed clip admission records its error"));
        }
        Ok(Self(Some(Arc::new(ClipLink {
            parent: self.clone(),
            shape,
            affine,
            op,
            hard,
            depth,
            charge: ClipCharge {
                budget: Arc::clone(budget),
                bytes,
            },
        }))))
    }

    /// Caller admits CPU peak before lowering and cumulative mask work before GPU allocation.
    pub(crate) fn lower(&self) -> EngineResult<ClipTape> {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        nodes.try_reserve_exact(MAX_CLIP_DEPTH).map_err(|source| {
            EngineError::PreparedResourceAllocation {
                resource: "clip nodes",
                source,
            }
        })?;
        edges.try_reserve_exact(MAX_CLIP_EDGES).map_err(|source| {
            EngineError::PreparedResourceAllocation {
                resource: "clip edges",
                source,
            }
        })?;
        let mut ordered = [None; MAX_CLIP_DEPTH];
        let mut cursor = self.0.as_deref();
        let mut count = 0;
        while let Some(link) = cursor {
            ordered[count] = Some(link);
            count += 1;
            cursor = link.parent.0.as_deref();
        }
        for link in ordered[..count].iter().rev().flatten() {
            let _ = &link.charge;
            let mut node = MaskNode {
                bounds: [0.0; 4],
                radii_x: [0.0; 4],
                radii_y: [0.0; 4],
                inverse: [1.0, 0.0, 0.0, 1.0],
                translation: [0.0; 4],
                meta: [
                    0,
                    u32::from(matches!(link.op, ClipOp::Difference)),
                    u32::from(link.hard),
                    0,
                ],
                path_range: [0; 4],
            };
            if let Some(inv) = link.affine.inverse_coefficients()? {
                node.inverse.copy_from_slice(&inv[..4]);
                node.translation[..2].copy_from_slice(&inv[4..]);
                match &link.shape {
                    ValidatedClip::Rect(rect) => node.bounds = pack_rect(*rect)?,
                    ValidatedClip::RRect(rect) => {
                        node.meta[0] = 1;
                        node.bounds = pack_rect(rect.rect)?;
                        let radii = normalize_radii(
                            rect.rect,
                            [
                                rect.top_left,
                                rect.top_right,
                                rect.bottom_right,
                                rect.bottom_left,
                            ],
                        );
                        pack_radii(&mut node, radii)?;
                    }
                    ValidatedClip::RSuperellipse(rect) => {
                        node.meta[0] = 2;
                        node.bounds = pack_rect(rect.outer_rect())?;
                        pack_radii(
                            &mut node,
                            normalize_radii(
                                rect.outer_rect(),
                                [
                                    rect.tl_radius(),
                                    rect.tr_radius(),
                                    rect.br_radius(),
                                    rect.bl_radius(),
                                ],
                            ),
                        )?;
                    }
                    ValidatedClip::Path(path) => {
                        node.meta[0] = 3;
                        node.meta[3] = u32::from(matches!(path.fill_type(), PathFillType::EvenOdd));
                        node.path_range[0] = edges.len() as u32;
                        flatten_path(path, link.affine, &mut edges)?;
                        node.path_range[1] = edges.len() as u32 - node.path_range[0];
                    }
                }
            }
            // Singular intersection remains an empty rectangular node, so it
            // cannot erase earlier clip history or open any subsequent draw.
            nodes.push(node);
        }
        Ok(ClipTape { nodes, edges })
    }
}

fn packed(value: f64) -> Result<f32, GeometryError> {
    crate::clip_geometry::pack_clip_value(value, "clip payload")
}

fn pack_rect(rect: Rect<f64>) -> Result<[f32; 4], GeometryError> {
    let result = [
        packed(rect.left())?,
        packed(rect.top())?,
        packed(rect.width())?,
        packed(rect.height())?,
    ];
    let right = result[0] + result[2];
    let bottom = result[1] + result[3];
    if !right.is_finite()
        || !bottom.is_finite()
        || (rect.width() > 0.0 && right == result[0])
        || (rect.height() > 0.0 && bottom == result[1])
    {
        return Err(GeometryError::Unrepresentable {
            context: "clip bounds extent",
        });
    }
    Ok(result)
}

fn normalize_radii(rect: Rect<f64>, radii: [Radius<f64>; 4]) -> [Radius<f64>; 4] {
    // Normalize the sum before adding: finite radii may overflow their sum.
    // One common factor preserves every rx/ry ratio.
    let fitting_factor = |a: f64, b: f64, extent: f64| {
        let maximum = a.max(b);
        if maximum == 0.0 {
            1.0
        } else {
            ((extent / maximum) / (a / maximum + b / maximum)).min(1.0)
        }
    };
    let factor = fitting_factor(radii[0].x, radii[1].x, rect.width())
        .min(fitting_factor(radii[3].x, radii[2].x, rect.width()))
        .min(fitting_factor(radii[0].y, radii[3].y, rect.height()))
        .min(fitting_factor(radii[1].y, radii[2].y, rect.height()));
    radii.map(|r| Radius::new(r.x * factor, r.y * factor))
}

fn pack_radii(node: &mut MaskNode, radii: [Radius<f64>; 4]) -> Result<(), GeometryError> {
    for (i, radius) in radii.into_iter().enumerate() {
        node.radii_x[i] = packed(radius.x)?;
        node.radii_y[i] = packed(radius.y)?;
    }
    Ok(())
}

fn point(p: Point<f64>) -> Result<lyon::math::Point, GeometryError> {
    Ok(lyon::math::point(packed(p.x)?, packed(p.y)?))
}

fn flatten_path(
    path: &flui_painting::paint::Path,
    affine: ValidatedAffine,
    edges: &mut Vec<MaskEdge>,
) -> EngineResult<()> {
    // Conversion emits at most Begin+End per command, then one final End.
    let capacity = path.commands().len() * 2 + 1;
    let mut events = Vec::new();
    events.try_reserve_exact(capacity).map_err(|source| {
        EngineError::PreparedResourceAllocation {
            resource: "clip path events",
            source,
        }
    })?;
    let mut contour = None;
    for command in path.commands() {
        match command {
            PathCommand::MoveTo(p) => {
                if let Some((first, last)) = contour.take() {
                    events.push(PathEvent::End {
                        last,
                        first,
                        close: true,
                    });
                }
                let at = point(p)?;
                events.push(PathEvent::Begin { at });
                contour = Some((at, at));
            }
            PathCommand::Close => {
                if let Some((first, last)) = contour.take() {
                    events.push(PathEvent::End {
                        last,
                        first,
                        close: true,
                    });
                }
            }
            other => {
                let Some((_, ref mut last)) = contour else {
                    return Err(GeometryError::InvalidExtent.into());
                };
                let from = *last;
                let event = match other {
                    PathCommand::LineTo(p) => {
                        let to = point(p)?;
                        *last = to;
                        PathEvent::Line { from, to }
                    }
                    PathCommand::QuadraticTo(a, b) => {
                        let to = point(b)?;
                        *last = to;
                        PathEvent::Quadratic {
                            from,
                            ctrl: point(a)?,
                            to,
                        }
                    }
                    PathCommand::CubicTo(a, b, c) => {
                        let to = point(c)?;
                        *last = to;
                        PathEvent::Cubic {
                            from,
                            ctrl1: point(a)?,
                            ctrl2: point(b)?,
                            to,
                        }
                    }
                    PathCommand::MoveTo(_) | PathCommand::Close => {
                        unreachable!("BUG: contour control commands handled above")
                    }
                };
                events.push(event);
            }
        }
    }
    if let Some((first, last)) = contour {
        events.push(PathEvent::End {
            last,
            first,
            close: true,
        });
    }
    let m = affine.matrix().to_cols_array();
    // Frobenius norm bounds stretch even for shear, unlike largest column.
    let scale = m[0].hypot(m[1]).hypot(m[4].hypot(m[5]));
    let tolerance = (0.1 / scale) as f32;
    // With coordinates bounded by 2^20, Wang's cubic l is below 2^51.
    // tolerance >= 2^-35 bounds l/(8*tolerance)^2 below 2^115,
    // avoiding lyon's nonfinite segment-count fallback and u32 overflow.
    if !tolerance.is_finite() || tolerance < 1.0 / 34_359_738_368.0 {
        return Err(GeometryError::Unrepresentable {
            context: "clip flatten tolerance",
        }
        .into());
    }
    for event in Flattened::new(tolerance, events.into_iter()) {
        let edge = match event {
            PathEvent::Line { from, to } => Some((from, to)),
            PathEvent::End { last, first, .. } if last != first => Some((last, first)),
            _ => None,
        };
        if let Some((from, to)) = edge {
            if edges.len() == MAX_CLIP_EDGES {
                return Err(EngineError::PreparedResourceLimit {
                    resource: "clip flattened edges",
                    requested: MAX_CLIP_EDGES + 1,
                    limit: MAX_CLIP_EDGES,
                });
            }
            if ![from.x, from.y, to.x, to.y].iter().all(|v| v.is_finite()) {
                return Err(GeometryError::NonFinite {
                    context: "flattened clip edge",
                }
                .into());
            }
            edges.push(MaskEdge([from.x, from.y, to.x, to.y]));
        }
    }
    Ok(())
}
