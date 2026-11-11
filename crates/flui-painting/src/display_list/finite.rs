//! Non-finite input is refused when it is recorded (ADR-0306 §4).
//!
//! [`DrawOp::is_finite`] checks every `f64` an op holds, its path's points and
//! its paint's values; [`refusal`] decides, by the op's kind, what a list
//! records in place of an op that fails the check or arrives under a
//! non-finite transform. Both match every variant with no wildcard, so a new
//! variant states its check and its refusal.

use std::sync::Arc;

use flui_foundation::geometry::{Matrix4, Offset, Point, RRect, RSuperellipse, Radius, Rect};

use super::command::DrawOp;
use crate::paint::image::ColorFilter;
use crate::paint::{ClipOp, DashPattern, ImageShader, Paint, Shader};

/// How many ops a list refused because they were not finite, by reason.
///
/// A refused draw is left out, a refused clip clips everything, a refused
/// save-layer opens unbounded, and a scope marker under a non-finite
/// transform is kept under the identity (ADR-0306 §4). The counts saturate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct NonFiniteOps {
    /// Ops recorded under a transform with a non-finite element.
    pub transform: usize,
    /// Ops holding a non-finite value.
    pub geometry: usize,
}

impl NonFiniteOps {
    /// Whether nothing was refused.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.transform == 0 && self.geometry == 0
    }

    /// Both counts of `self` and `other`, saturating.
    #[must_use]
    pub(crate) const fn saturating_add(self, other: Self) -> Self {
        Self {
            transform: self.transform.saturating_add(other.transform),
            geometry: self.geometry.saturating_add(other.geometry),
        }
    }
}

/// Why an op was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NonFinite {
    /// The transform it was recorded under has a non-finite element.
    Transform,
    /// The op holds a non-finite value.
    Geometry,
}

impl NonFiniteOps {
    pub(crate) fn count(&mut self, reason: NonFinite) {
        let slot = match reason {
            NonFinite::Transform => &mut self.transform,
            NonFinite::Geometry => &mut self.geometry,
        };
        *slot = slot.saturating_add(1);
    }
}

/// What a list records in place of a refused op, and whether the refusal counts.
pub(crate) enum Refused {
    /// Nothing is recorded; the refusal counts.
    Omitted,
    /// `op` is recorded under the identity; the refusal counts.
    Replaced(DrawOp),
    /// A scope marker, kept under the identity: it holds no geometry, so nothing
    /// is lost and nothing counts.
    Marker(DrawOp),
}

/// What replaces `op`, refused by its kind (ADR-0306 §4).
pub(crate) fn refusal(op: DrawOp) -> Refused {
    match op {
        DrawOp::ClipRect { clip_behavior, .. }
        | DrawOp::ClipRRect { clip_behavior, .. }
        | DrawOp::ClipRSuperellipse { clip_behavior, .. }
        | DrawOp::ClipPath { clip_behavior, .. } => Refused::Replaced(DrawOp::ClipRect {
            rect: Rect::ZERO,
            clip_op: ClipOp::Intersect,
            clip_behavior,
        }),
        DrawOp::SaveLayer { paint, .. } => Refused::Replaced(DrawOp::SaveLayer {
            bounds: None,
            paint,
        }),
        DrawOp::Save => Refused::Marker(DrawOp::Save),
        DrawOp::Restore => Refused::Marker(DrawOp::Restore),
        DrawOp::RestoreLayer => Refused::Marker(DrawOp::RestoreLayer),
        DrawOp::Line { .. }
        | DrawOp::Rect { .. }
        | DrawOp::RRect { .. }
        | DrawOp::Circle { .. }
        | DrawOp::Oval { .. }
        | DrawOp::Path { .. }
        | DrawOp::Paragraph { .. }
        | DrawOp::ParagraphShadow { .. }
        | DrawOp::Image { .. }
        | DrawOp::ImageRegion { .. }
        | DrawOp::ImageRepeat { .. }
        | DrawOp::ImageNineSlice { .. }
        | DrawOp::ImageFiltered { .. }
        | DrawOp::Texture { .. }
        | DrawOp::Shadow { .. }
        | DrawOp::Arc { .. }
        | DrawOp::DRRect { .. }
        | DrawOp::Points { .. }
        | DrawOp::Vertices { .. }
        | DrawOp::Color { .. }
        | DrawOp::Paint { .. }
        | DrawOp::Atlas { .. } => Refused::Omitted,
    }
}

impl DrawOp {
    /// Whether every value this op holds is finite: coordinates, radii,
    /// angles, elevation, opacity, source and tile rectangles, an atlas's
    /// transforms, vertices and texture coordinates, a path's points, and its
    /// paint's values.
    ///
    /// `paint_finite` answers for a paint; the canvas answers from the
    /// verdict it took once, when it interned the paint.
    pub(crate) fn is_finite(&self, paint_finite: impl Fn(&Arc<Paint>) -> bool) -> bool {
        let optional_paint = |paint: &Option<Arc<Paint>>| paint.as_ref().is_none_or(&paint_finite);
        match self {
            DrawOp::ClipRect { rect, .. } => rect_finite(rect),
            DrawOp::ClipRRect { rrect, .. } => rrect_finite(rrect),
            DrawOp::ClipRSuperellipse { rsuperellipse, .. } => rsuperellipse_finite(rsuperellipse),
            DrawOp::ClipPath { path, .. } => path.is_finite(),
            DrawOp::Line { p1, p2, paint } => {
                point_finite(p1) && point_finite(p2) && paint_finite(paint)
            }
            DrawOp::Rect { rect, paint } | DrawOp::Oval { rect, paint } => {
                rect_finite(rect) && paint_finite(paint)
            }
            DrawOp::RRect { rrect, paint } => rrect_finite(rrect) && paint_finite(paint),
            DrawOp::Circle {
                center,
                radius,
                paint,
            } => point_finite(center) && radius.is_finite() && paint_finite(paint),
            DrawOp::Path { path, paint } => path.is_finite() && paint_finite(paint),
            DrawOp::Paragraph { offset, .. } | DrawOp::ParagraphShadow { offset, .. } => {
                offset_finite(offset)
            }
            DrawOp::Image { dst, paint, .. } | DrawOp::ImageRepeat { dst, paint, .. } => {
                rect_finite(dst) && optional_paint(paint)
            }
            DrawOp::ImageRegion {
                src,
                dst,
                tile,
                filter,
                paint,
                ..
            } => {
                rect_finite(src)
                    && rect_finite(dst)
                    && tile.as_ref().is_none_or(rect_finite)
                    && filter.as_ref().is_none_or(color_filter_finite)
                    && optional_paint(paint)
            }
            DrawOp::ImageNineSlice {
                center_slice,
                dst,
                paint,
                ..
            } => rect_finite(center_slice) && rect_finite(dst) && optional_paint(paint),
            DrawOp::ImageFiltered {
                dst, filter, paint, ..
            } => rect_finite(dst) && color_filter_finite(filter) && optional_paint(paint),
            DrawOp::Texture {
                dst, src, opacity, ..
            } => rect_finite(dst) && src.as_ref().is_none_or(rect_finite) && opacity.is_finite(),
            DrawOp::Shadow {
                path, elevation, ..
            } => path.is_finite() && elevation.is_finite(),
            DrawOp::Arc {
                rect,
                start_angle,
                sweep_angle,
                paint,
                ..
            } => {
                rect_finite(rect)
                    && start_angle.is_finite()
                    && sweep_angle.is_finite()
                    && paint_finite(paint)
            }
            DrawOp::DRRect {
                outer,
                inner,
                paint,
            } => rrect_finite(outer) && rrect_finite(inner) && paint_finite(paint),
            DrawOp::Points { points, paint, .. } => {
                points.iter().all(point_finite) && paint_finite(paint)
            }
            DrawOp::Vertices {
                vertices,
                tex_coords,
                paint,
                ..
            } => {
                vertices.iter().all(point_finite)
                    && tex_coords
                        .as_ref()
                        .is_none_or(|coords| coords.iter().all(point_finite))
                    && paint_finite(paint)
            }
            DrawOp::Color { .. } => true,
            DrawOp::Paint { paint } => paint_finite(paint),
            DrawOp::Atlas {
                sprites,
                transforms,
                paint,
                ..
            } => {
                sprites.iter().all(rect_finite)
                    && transforms.iter().all(matrix_finite)
                    && optional_paint(paint)
            }
            DrawOp::SaveLayer { bounds, paint } => {
                bounds.as_ref().is_none_or(rect_finite) && paint_finite(paint)
            }
            DrawOp::RestoreLayer | DrawOp::Save | DrawOp::Restore => true,
        }
    }
}

/// Whether every value of `paint` is finite: stroke width, dash intervals and
/// phase, and its shader's coordinates, radii, angles and stops.
pub(crate) fn paint_finite(paint: &Paint) -> bool {
    paint.stroke_width.is_finite()
        && paint.dash_pattern.as_ref().is_none_or(dash_finite)
        && paint.shader.as_ref().is_none_or(shader_finite)
}

/// Whether every element of `matrix` is finite.
pub(crate) fn matrix_finite(matrix: &Matrix4) -> bool {
    matrix.m.iter().all(|value| value.is_finite())
}

fn dash_finite(dash: &DashPattern) -> bool {
    dash.phase.is_finite() && dash.intervals.iter().all(|value| value.is_finite())
}

fn stops_finite(stops: Option<&Vec<f64>>) -> bool {
    stops.is_none_or(|stops| stops.iter().all(|value| value.is_finite()))
}

fn shader_finite(shader: &Shader) -> bool {
    match shader {
        Shader::LinearGradient {
            from, to, stops, ..
        } => offset_finite(from) && offset_finite(to) && stops_finite(stops.as_ref()),
        Shader::RadialGradient {
            center,
            radius,
            stops,
            focal,
            focal_radius,
            ..
        } => {
            offset_finite(center)
                && radius.is_finite()
                && stops_finite(stops.as_ref())
                && focal.as_ref().is_none_or(offset_finite)
                && focal_radius.is_none_or(f64::is_finite)
        }
        Shader::SweepGradient {
            center,
            stops,
            start_angle,
            end_angle,
            ..
        } => {
            offset_finite(center)
                && stops_finite(stops.as_ref())
                && start_angle.is_finite()
                && end_angle.is_finite()
        }
        Shader::Solid { .. } => true,
        Shader::Image(ImageShader { transform, .. }) => transform
            .as_ref()
            .is_none_or(|rows| rows.iter().flatten().all(|value| value.is_finite())),
    }
}

fn color_filter_finite(filter: &ColorFilter) -> bool {
    match filter {
        ColorFilter::Matrix(matrix) => matrix.values.iter().all(|value| value.is_finite()),
        ColorFilter::Mode { .. }
        | ColorFilter::LinearToSrgbGamma
        | ColorFilter::SrgbToLinearGamma => true,
    }
}

fn point_finite(point: &Point<f64>) -> bool {
    point.x.is_finite() && point.y.is_finite()
}

fn offset_finite(offset: &Offset<f64>) -> bool {
    offset.dx.is_finite() && offset.dy.is_finite()
}

fn rect_finite(rect: &Rect<f64>) -> bool {
    point_finite(&rect.min) && point_finite(&rect.max)
}

fn radius_finite(radius: Radius<f64>) -> bool {
    radius.x.is_finite() && radius.y.is_finite()
}

fn rrect_finite(rrect: &RRect) -> bool {
    rect_finite(&rrect.rect)
        && radius_finite(rrect.top_left)
        && radius_finite(rrect.top_right)
        && radius_finite(rrect.bottom_right)
        && radius_finite(rrect.bottom_left)
}

fn rsuperellipse_finite(shape: &RSuperellipse) -> bool {
    rect_finite(&shape.outer_rect())
        && radius_finite(shape.tl_radius())
        && radius_finite(shape.tr_radius())
        && radius_finite(shape.br_radius())
        && radius_finite(shape.bl_radius())
}
