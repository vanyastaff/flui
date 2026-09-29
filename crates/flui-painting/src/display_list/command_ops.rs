//! `DrawCommand::bounds` — the per-op geometry a recorded command
//! contributes to its [`DisplayList`](super::DisplayList)'s cached extent —
//! and `DrawCommand::damage_extent`, its conservative ink-covering sibling.

use flui_foundation::geometry::{Rect, Size};

use super::command::{DrawCommand, DrawOp};

/// How far a recorded command, or a whole display list, can change pixels.
///
/// The answer a damage producer needs: [`Bounded`](Self::Bounded) is a rect
/// that covers every pixel the content may touch (ink overflow, stroke joins
/// and blur included), and [`Unbounded`](Self::Unbounded) means the content
/// can reach any pixel its clip lets it — a full-canvas fill, an unbounded
/// save-layer, or a command under a perspective transform whose image this
/// crate does not bound.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DamageExtent {
    /// Every pixel the content changes lies inside this rect.
    Bounded(Rect<f64>),
    /// The content can change any pixel its clip allows.
    Unbounded,
}

impl DamageExtent {
    /// The smallest extent covering both; `Unbounded` absorbs everything.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        match (self, other) {
            (Self::Bounded(a), Self::Bounded(b)) => Self::Bounded(a.union(&b)),
            _ => Self::Unbounded,
        }
    }
}

/// Whether `matrix` maps the plane projectively rather than affinely.
///
/// `Matrix4::transform_rect` divides by `w`, which is sound only while every
/// corner stays in front of the viewer; a damage extent never relies on it
/// then.
fn is_projective(matrix: &flui_foundation::geometry::Matrix4) -> bool {
    let m = &matrix.m;
    m[3].abs() > f64::EPSILON || m[7].abs() > f64::EPSILON || (m[15] - 1.0).abs() > f64::EPSILON
}

impl DrawCommand {
    /// The command's conservative damage extent in the display list's
    /// coordinate space — see [`DrawOp::damage_bounds`]. `None` for ops that
    /// draw nothing (clips and scope markers).
    pub(crate) fn damage_extent(&self) -> Option<DamageExtent> {
        match self.op.damage_bounds()? {
            DamageExtent::Bounded(_) if is_projective(&self.transform) => {
                Some(DamageExtent::Unbounded)
            }
            DamageExtent::Bounded(local) => {
                Some(DamageExtent::Bounded(self.transform.transform_rect(&local)))
            }
            DamageExtent::Unbounded => Some(DamageExtent::Unbounded),
        }
    }

    /// The bounding rectangle of this command in the display list's
    /// coordinate space: the op's local bounds mapped through the
    /// command's transform.
    ///
    /// Used to calculate the DisplayList's overall bounds. `None` for ops
    /// that draw nothing or draw everywhere (clips, scope markers, `Color`,
    /// `Paint`).
    pub(crate) fn bounds(&self) -> Option<Rect<f64>> {
        self.op
            .local_bounds()
            .map(|local| self.transform.transform_rect(&local))
    }
}

/// Smallest-enclosing axis-aligned box of a point set, `None` when empty.
fn point_bounds<'a>(
    mut points: impl Iterator<Item = &'a flui_foundation::geometry::Point<f64>>,
) -> Option<Rect<f64>> {
    let first = points.next()?;
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (first.x, first.y, first.x, first.y);
    for point in points {
        min_x = min_x.min(point.x);
        min_y = min_y.min(point.y);
        max_x = max_x.max(point.x);
        max_y = max_y.max(point.y);
    }
    Some(Rect::from_ltrb(min_x, min_y, max_x, max_y))
}

impl DrawOp {
    /// Every pixel this op may change, in its own (pre-transform) coordinate
    /// space; `None` for an op that draws nothing.
    ///
    /// Starts from `local_bounds` and widens it wherever that box is
    /// the layout answer rather than the ink:
    ///
    /// - a stroke reaches a full stroke width past its geometry (a miter
    ///   join's point), and twice that on open paths, lines and points (a
    ///   square cap on top of a miter), not the half width `local_bounds`
    ///   adds;
    /// - a paragraph's glyphs overflow its laid-out box (ascenders, italic
    ///   overhang, combining marks) by up to about half a line, so the box
    ///   grows by half the layout height on every side;
    /// - a shadow's blur spreads about twice its elevation;
    /// - a full-canvas fill (`Color`, `Paint`), an unbounded `SaveLayer` and
    ///   a filtered image (whose filter can spread arbitrarily) are
    ///   [`DamageExtent::Unbounded`].
    pub fn damage_bounds(&self) -> Option<DamageExtent> {
        let stroke = |paint: &crate::paint::Paint| paint.effective_stroke_width();
        let bounded = |rect: Rect<f64>| Some(DamageExtent::Bounded(rect));
        match self {
            DrawOp::Rect { rect, paint } | DrawOp::Oval { rect, paint } => {
                bounded(rect.expand(stroke(paint)))
            }
            DrawOp::Arc { rect, paint, .. } => bounded(rect.expand(stroke(paint) * 2.0)),
            DrawOp::RRect { rrect, paint } => bounded(rrect.bounding_rect().expand(stroke(paint))),
            DrawOp::DRRect { outer, paint, .. } => {
                bounded(outer.bounding_rect().expand(stroke(paint)))
            }
            DrawOp::Circle {
                center,
                radius,
                paint,
            } => {
                let reach = *radius + stroke(paint);
                bounded(Rect::from_center_size(
                    *center,
                    Size::new(reach * 2.0, reach * 2.0),
                ))
            }
            DrawOp::Line { p1, p2, paint } => point_bounds([p1, p2].into_iter())
                .and_then(|b| bounded(b.expand(stroke(paint) * 2.0))),
            DrawOp::Path { path, paint } => {
                bounded(path.compute_bounds().expand(stroke(paint) * 2.0))
            }
            DrawOp::Points { points, paint, .. } => point_bounds(points.iter())
                .and_then(|b| bounded(b.expand(stroke(paint).max(1.0) * 2.0))),
            DrawOp::Shadow {
                path, elevation, ..
            } => bounded(path.compute_bounds().expand(*elevation * 2.0)),
            DrawOp::Paragraph { layout, offset, .. } => {
                let size = layout.metrics().size();
                let overflow = size.height * 0.5;
                bounded(
                    Rect::from_xywh(offset.dx, offset.dy, size.width, size.height).expand(overflow),
                )
            }
            DrawOp::SaveLayer { bounds: None, .. }
            | DrawOp::Color { .. }
            | DrawOp::Paint { .. }
            | DrawOp::ImageFiltered { .. } => Some(DamageExtent::Unbounded),
            DrawOp::SaveLayer {
                bounds: Some(bounds),
                ..
            } => bounded(*bounds),
            DrawOp::Image { .. }
            | DrawOp::ImageRepeat { .. }
            | DrawOp::ImageNineSlice { .. }
            | DrawOp::Texture { .. }
            | DrawOp::Vertices { .. }
            | DrawOp::Atlas { .. } => self.local_bounds().map(DamageExtent::Bounded),
            DrawOp::ClipRect { .. }
            | DrawOp::ClipRRect { .. }
            | DrawOp::ClipRSuperellipse { .. }
            | DrawOp::ClipPath { .. }
            | DrawOp::RestoreLayer
            | DrawOp::Save
            | DrawOp::Restore => None,
        }
    }

    /// The op's bounds in its own (pre-transform) coordinate space, with
    /// half the stroke width added where the paint strokes.
    pub(crate) fn local_bounds(&self) -> Option<Rect<f64>> {
        match self {
            DrawOp::Rect { rect, paint } | DrawOp::Oval { rect, paint } => {
                let outset = paint.effective_stroke_width() * 0.5;
                Some(rect.expand(outset))
            }
            DrawOp::Arc { rect, paint, .. } => {
                let outset = paint.effective_stroke_width() * 0.5;
                Some(rect.expand(outset))
            }
            DrawOp::RRect { rrect, paint } => {
                let outset = paint.effective_stroke_width() * 0.5;
                Some(rrect.bounding_rect().expand(outset))
            }
            DrawOp::DRRect { outer, paint, .. } => {
                let outset = paint.effective_stroke_width() * 0.5;
                Some(outer.bounding_rect().expand(outset))
            }
            DrawOp::Circle {
                center,
                radius,
                paint,
            } => {
                let effective_radius = *radius + (paint.effective_stroke_width() * 0.5);
                let size = Size::new(effective_radius * 2.0, effective_radius * 2.0);
                Some(Rect::from_center_size(*center, size))
            }
            DrawOp::Image { dst, .. }
            | DrawOp::ImageRepeat { dst, .. }
            | DrawOp::ImageNineSlice { dst, .. }
            | DrawOp::ImageFiltered { dst, .. }
            | DrawOp::Texture { dst, .. } => Some(*dst),
            DrawOp::Line { p1, p2, paint } => {
                let stroke_half = paint.effective_stroke_width() * 0.5;
                point_bounds([p1, p2].into_iter()).map(|b| b.expand(stroke_half))
            }
            DrawOp::Path { path, paint } => {
                let outset = paint.effective_stroke_width() * 0.5;
                Some(path.compute_bounds().expand(outset))
            }
            DrawOp::Shadow {
                path, elevation, ..
            } => Some(path.compute_bounds().expand(*elevation)),
            DrawOp::Points { points, paint, .. } => {
                let stroke_half = paint.effective_stroke_width() * 0.5;
                point_bounds(points.iter()).map(|b| b.expand(stroke_half))
            }
            DrawOp::Vertices { vertices, .. } => point_bounds(vertices.iter()),
            DrawOp::Atlas {
                sprites,
                transforms: sprite_transforms,
                ..
            } => sprites
                .iter()
                .zip(sprite_transforms.iter())
                .map(|(sprite_rect, sprite_transform)| sprite_transform.transform_rect(sprite_rect))
                .reduce(|acc, r| acc.union(&r)),
            // The laid-out box the recorder measured, not the ink extent of
            // the glyphs — the `Offset.zero & size` box a paragraph render
            // object reports. A text op that contributed nothing here would
            // silently drop visible text from every bounds computation built
            // on this.
            DrawOp::Paragraph { layout, offset, .. } => {
                let size = layout.metrics().size();
                Some(Rect::from_xywh(
                    offset.dx,
                    offset.dy,
                    size.width,
                    size.height,
                ))
            }
            DrawOp::SaveLayer { bounds, .. } => *bounds,
            // Full-target fills, clips, and scope markers contribute no
            // bounds of their own.
            DrawOp::Color { .. }
            | DrawOp::Paint { .. }
            | DrawOp::ClipRect { .. }
            | DrawOp::ClipRRect { .. }
            | DrawOp::ClipRSuperellipse { .. }
            | DrawOp::ClipPath { .. }
            | DrawOp::RestoreLayer
            | DrawOp::Save
            | DrawOp::Restore => None,
        }
    }
}
