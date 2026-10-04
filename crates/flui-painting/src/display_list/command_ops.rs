//! `DrawCommand::bounds` — the per-op geometry a recorded command
//! contributes to its [`DisplayList`](super::DisplayList)'s cached extent —
//! and `DrawCommand::damage_extent`, its conservative ink-covering sibling.

use flui_foundation::geometry::{Matrix4, Rect, Size};

use super::command::{DrawCommand, DrawOp};

/// How far a recorded command, or a whole display list, can change pixels.
///
/// The answer a damage producer needs: [`Bounded`](Self::Bounded) covers
/// every pixel the content may touch (ink overflow, stroke joins and blur
/// included), and [`Unbounded`](Self::Unbounded) means the content can reach
/// any pixel its clip lets it — a full-canvas fill, an unbounded save-layer,
/// or a command under a perspective transform whose image this crate does
/// not bound.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DamageExtent {
    /// Every pixel the content changes lies inside `rect` mapped to the
    /// target, grown there by `spread` times the mapping's largest stretch.
    Bounded {
        /// The content's geometry, ink that scales with it included.
        rect: Rect<f64>,
        /// Ink that reaches equally far in every direction of the target,
        /// however the mapping stretches each axis: a shadow's blur, whose
        /// sigma the renderer takes from the transform's largest scale and
        /// applies isotropically. Growing `rect` in its own space instead
        /// falls short along an axis the transform compresses.
        spread: f64,
    },
    /// The content can change any pixel its clip allows.
    Unbounded,
}

impl DamageExtent {
    /// An extent of exactly `rect`, with no isotropic spread.
    #[must_use]
    pub fn rect(rect: Rect<f64>) -> Self {
        Self::Bounded { rect, spread: 0.0 }
    }

    /// The smallest extent covering both; `Unbounded` absorbs everything.
    ///
    /// The larger spread applies to the joined rect, which only ever widens
    /// the answer.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        match (self, other) {
            (
                Self::Bounded {
                    rect: a,
                    spread: sa,
                },
                Self::Bounded {
                    rect: b,
                    spread: sb,
                },
            ) => Self::Bounded {
                rect: a.union(&b),
                spread: sa.max(sb),
            },
            _ => Self::Unbounded,
        }
    }

    /// The extent in the space `matrix` maps into: the rect mapped as a
    /// rect, the spread multiplied by the largest factor `matrix` stretches
    /// a length by. `Unbounded` under a projective `matrix`, whose image of
    /// a rect this crate does not bound.
    #[must_use]
    pub fn transformed(self, matrix: &Matrix4) -> Self {
        match self {
            Self::Bounded { .. } if is_projective(matrix) => Self::Unbounded,
            Self::Bounded { rect, spread } => Self::Bounded {
                rect: matrix.transform_rect(&rect),
                spread: spread * stretch(matrix),
            },
            Self::Unbounded => Self::Unbounded,
        }
    }

    /// The rect covering every pixel of a bounded extent in its own space:
    /// the rect grown by the spread. `None` for `Unbounded`.
    #[must_use]
    pub fn covering_rect(self) -> Option<Rect<f64>> {
        match self {
            Self::Bounded { rect, spread } => Some(rect.expand(spread)),
            Self::Unbounded => None,
        }
    }
}

/// Whether `matrix` maps the plane projectively rather than affinely.
///
/// `Matrix4::transform_rect` divides by `w`, which is sound only while every
/// corner stays in front of the viewer; a damage extent never relies on it
/// then.
fn is_projective(matrix: &Matrix4) -> bool {
    let m = &matrix.m;
    m[3].abs() > f64::EPSILON || m[7].abs() > f64::EPSILON || (m[15] - 1.0).abs() > f64::EPSILON
}

/// The largest factor the plane part of `matrix` stretches any length by:
/// the spectral norm of its 2x2 linear part.
///
/// Not less than the longer basis column the renderer scales a shadow's
/// sigma by, and submultiplicative, so a spread mapped through a command's
/// transform and then a layer's still covers the blur the renderer draws
/// under their product.
fn stretch(matrix: &Matrix4) -> f64 {
    let [x_x, x_y, _, _, y_x, y_y, ..] = matrix.m;
    let sum = x_x * x_x + x_y * x_y + y_x * y_x + y_y * y_y;
    let det = x_x * y_y - x_y * y_x;
    let discriminant = (sum * sum - 4.0 * det * det).max(0.0);
    f64::midpoint(sum, discriminant.sqrt()).sqrt()
}

impl DrawCommand {
    /// The command's conservative damage extent in the display list's
    /// coordinate space — see [`DrawOp::damage_bounds`]. `None` for ops that
    /// draw nothing (clips and scope markers).
    pub(crate) fn damage_extent(&self) -> Option<DamageExtent> {
        Some(self.op.damage_bounds()?.transformed(&self.transform))
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

/// The width a line or a point is drawn at: the paint's raw `stroke_width`,
/// whatever its style, and at least one pixel, as the renderer draws it.
fn line_width(paint: &crate::paint::Paint) -> f64 {
    paint.stroke_width.max(1.0)
}

/// How many elevations a shadow's ink reaches past its path. The GPU's
/// analytic shadow (`flui-engine`'s `draw_analytic_rrect_shadow`) blurs with
/// a sigma of one elevation out to three sigma, around a copy of the shape
/// offset half an elevation down, both scaled by the transform's largest
/// basis scale and applied the same on both axes; the path-fill fallback
/// shifts by at most one elevation and does not blur. The reach is therefore
/// a [`DamageExtent::Bounded`] spread, not a growth of the path's rect.
const SHADOW_REACH: f64 = 3.5;

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
    ///   adds; a line or a point is stroked whatever the paint's style (the
    ///   renderer reads `stroke_width` as is, at least one pixel), so a
    ///   fill-style line reaches as far as a stroked one;
    /// - an atlas sprite lands where the renderer places it: the sprite's
    ///   size at its transform's translation, joined with the sprite's size
    ///   mapped by the whole transform, not the source rect's position in
    ///   the image;
    /// - a paragraph's glyphs overflow its laid-out box (ascenders under a
    ///   tight line height, italic overhang, swashes, combining marks), so
    ///   it covers the box joined with the layout's glyph ink bounds, and is
    ///   unbounded when a face gives no bounds to take them from;
    /// - a shadow reaches [`SHADOW_REACH`] elevations past its path in every
    ///   direction of the target, a spread;
    /// - a full-canvas fill (`Color`, `Paint`), an unbounded `SaveLayer` and
    ///   a filtered image (whose filter can spread arbitrarily) are
    ///   [`DamageExtent::Unbounded`].
    pub(crate) fn damage_bounds(&self) -> Option<DamageExtent> {
        let stroke = |paint: &crate::paint::Paint| paint.effective_stroke_width();
        let bounded = |rect: Rect<f64>| Some(DamageExtent::rect(rect));
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
                .and_then(|b| bounded(b.expand(line_width(paint) * 2.0))),
            DrawOp::Path { path, paint } => {
                bounded(path.compute_bounds().expand(stroke(paint) * 2.0))
            }
            DrawOp::Points { points, paint, .. } => {
                point_bounds(points.iter()).and_then(|b| bounded(b.expand(line_width(paint) * 2.0)))
            }
            DrawOp::Shadow {
                path, elevation, ..
            } => Some(DamageExtent::Bounded {
                rect: path.compute_bounds(),
                spread: *elevation * SHADOW_REACH,
            }),
            DrawOp::Paragraph {
                paragraph, offset, ..
            } => {
                let size = paragraph.size();
                let layout_box = Rect::from_xywh(offset.dx, offset.dy, size.width, size.height);
                match paragraph.ink_bounds() {
                    Some(ink) => bounded(layout_box.union(&ink.translate_offset(*offset))),
                    None => Some(DamageExtent::Unbounded),
                }
            }
            DrawOp::SaveLayer { bounds: None, .. }
            | DrawOp::Color { .. }
            | DrawOp::Paint { .. }
            | DrawOp::ImageFiltered { .. } => Some(DamageExtent::Unbounded),
            DrawOp::SaveLayer {
                bounds: Some(bounds),
                ..
            } => bounded(*bounds),
            DrawOp::ImageRegion { .. }
            | DrawOp::Image { .. }
            | DrawOp::ImageRepeat { .. }
            | DrawOp::ImageNineSlice { .. }
            | DrawOp::Texture { .. }
            | DrawOp::Vertices { .. } => self.local_bounds().map(DamageExtent::rect),
            DrawOp::Atlas {
                sprites,
                transforms,
                ..
            } => sprites
                .iter()
                .zip(transforms.iter())
                .map(|(sprite, transform)| {
                    let size = Rect::from_xywh(0.0, 0.0, sprite.width(), sprite.height());
                    let placed = Rect::from_xywh(
                        transform.m[12],
                        transform.m[13],
                        sprite.width(),
                        sprite.height(),
                    );
                    placed.union(&transform.transform_rect(&size))
                })
                .reduce(|acc, rect| acc.union(&rect))
                .map(DamageExtent::rect),
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
            DrawOp::ImageRegion { dst, .. }
            | DrawOp::Image { dst, .. }
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
            DrawOp::Paragraph {
                paragraph, offset, ..
            } => {
                let size = paragraph.size();
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
