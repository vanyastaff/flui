//! `DrawCommand::bounds` — the per-op geometry a recorded command
//! contributes to its [`DisplayList`](super::DisplayList)'s cached extent.

use flui_types::geometry::{Pixels, Rect, Size};

use super::command::{DrawCommand, DrawOp};

impl DrawCommand {
    /// The bounding rectangle of this command in the display list's
    /// coordinate space: the op's local bounds mapped through the
    /// command's transform.
    ///
    /// Used to calculate the DisplayList's overall bounds. `None` for ops
    /// that draw nothing or draw everywhere (clips, scope markers, `Color`,
    /// `Paint`).
    pub(crate) fn bounds(&self) -> Option<Rect<Pixels>> {
        self.op
            .local_bounds()
            .map(|local| self.transform.transform_rect(&local))
    }
}

/// Smallest-enclosing axis-aligned box of a point set, `None` when empty.
fn point_bounds<'a>(
    mut points: impl Iterator<Item = &'a flui_types::geometry::Point<Pixels>>,
) -> Option<Rect<Pixels>> {
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
    /// The op's bounds in its own (pre-transform) coordinate space, with
    /// half the stroke width added where the paint strokes.
    pub(crate) fn local_bounds(&self) -> Option<Rect<Pixels>> {
        match self {
            DrawOp::Rect { rect, paint } | DrawOp::Oval { rect, paint } => {
                let outset = paint.effective_stroke_width() * 0.5;
                Some(rect.expand(Pixels(outset)))
            }
            DrawOp::Arc { rect, paint, .. } => {
                let outset = paint.effective_stroke_width() * 0.5;
                Some(rect.expand(Pixels(outset)))
            }
            DrawOp::RRect { rrect, paint } => {
                let outset = paint.effective_stroke_width() * 0.5;
                Some(rrect.bounding_rect().expand(Pixels(outset)))
            }
            DrawOp::DRRect { outer, paint, .. } => {
                let outset = paint.effective_stroke_width() * 0.5;
                Some(outer.bounding_rect().expand(Pixels(outset)))
            }
            DrawOp::Circle {
                center,
                radius,
                paint,
            } => {
                let effective_radius = *radius + Pixels(paint.effective_stroke_width() * 0.5);
                let size = Size::new(effective_radius * 2.0, effective_radius * 2.0);
                Some(Rect::from_center_size(*center, size))
            }
            DrawOp::Image { dst, .. }
            | DrawOp::ImageRepeat { dst, .. }
            | DrawOp::ImageNineSlice { dst, .. }
            | DrawOp::ImageFiltered { dst, .. }
            | DrawOp::Texture { dst, .. } => Some(*dst),
            DrawOp::Line { p1, p2, paint } => {
                let stroke_half = Pixels(paint.effective_stroke_width() * 0.5);
                point_bounds([p1, p2].into_iter()).map(|b| b.expand(stroke_half))
            }
            DrawOp::Path { path, paint } => {
                let outset = paint.effective_stroke_width() * 0.5;
                Some(path.compute_bounds().expand(Pixels(outset)))
            }
            DrawOp::Shadow {
                path, elevation, ..
            } => Some(path.compute_bounds().expand(Pixels(*elevation))),
            DrawOp::Points { points, paint, .. } => {
                let stroke_half = Pixels(paint.effective_stroke_width() * 0.5);
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
