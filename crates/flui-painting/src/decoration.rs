//! `BoxDecoration` painting — the orchestrating painter the canvas
//! primitives were waiting for.
//!
//! Flutter's `_BoxDecorationPainter` (box_decoration.dart) draws, in
//! order: shadows → background (color/gradient) → image → border. All
//! the primitives (rect/rrect/drrect/gradient/shadow/image) already
//! exist on [`Canvas`]; this module sequences them and resolves the
//! alignment-relative gradient geometry against the concrete paint
//! rect.
//!
//! Everything here is sans-IO: commands are recorded into the canvas's
//! display list, never rasterized — the same contract as the rest of
//! the fragment paint model.

use flui_types::{
    Color, Offset, Pixels, Point, RRect, Rect,
    painting::{Paint, Path, Shader},
    styling::{BoxDecoration, BoxShadow, Gradient},
};

use crate::canvas::Canvas;

/// Paints `decoration` into `rect` on `canvas` in Flutter's order:
/// shadows, then background color/gradient, then the decoration image,
/// then the border.
pub fn paint_box_decoration(
    canvas: &mut Canvas,
    rect: Rect<Pixels>,
    decoration: &BoxDecoration<Pixels>,
) {
    let rrect = decoration_rrect(rect, decoration);

    // 1. Shadows (behind everything). Inset shadows are an inner-glow
    //    effect drawn INSIDE the shape above the background — a
    //    different compositing path the engine does not expose yet;
    //    painting them as outer drop shadows would be visually wrong,
    //    so they are skipped loudly rather than rendered wrongly.
    if let Some(shadows) = &decoration.box_shadow {
        for shadow in shadows {
            if shadow.inset {
                tracing::warn!(?shadow.color, "inset box shadows are not painted yet");
                continue;
            }
            paint_shadow(canvas, rect, rrect, shadow);
        }
    }

    // 2. Background: a gradient wins over a flat color (Flutter:
    //    "if gradient is specified, color has no effect").
    if let Some(gradient) = &decoration.gradient {
        let shader = resolve_gradient(gradient, rect);
        match &rrect {
            Some(rrect) => canvas.draw_gradient_rrect(*rrect, shader),
            None => canvas.draw_gradient(rect, shader),
        }
    } else if let Some(color) = decoration.color {
        let paint = Paint::fill(color);
        match &rrect {
            Some(rrect) => canvas.draw_rrect(*rrect, &paint),
            None => canvas.draw_rect(rect, &paint),
        }
    }

    // 3. Decoration image (above the background, below the border).
    if let Some(image) = &decoration.image {
        paint_decoration_image(canvas, rect, image);
    }

    // 4. Border (on top).
    if let Some(border) = &decoration.border {
        paint_border(canvas, rect, rrect, border);
    }
}

/// Hit test against the decoration's geometry: inside the rounded
/// rect when a border radius is set, inside the plain rect otherwise
/// (Flutter `BoxDecoration.hitTest`).
#[must_use]
pub fn box_decoration_hit_test(
    rect: Rect<Pixels>,
    decoration: &BoxDecoration<Pixels>,
    position: Offset<Pixels>,
) -> bool {
    let point = Point::new(position.dx, position.dy);
    if !rect.contains(point) {
        return false;
    }
    match decoration_rrect(rect, decoration) {
        Some(rrect) => rrect_contains(&rrect, point),
        None => true,
    }
}

/// The decoration's rounded rect, when a border radius is set.
fn decoration_rrect(rect: Rect<Pixels>, decoration: &BoxDecoration<Pixels>) -> Option<RRect> {
    decoration.border_radius.map(|radius| {
        RRect::from_rect_and_corners(
            rect,
            radius.top_left,
            radius.top_right,
            radius.bottom_right,
            radius.bottom_left,
        )
    })
}

/// One box shadow: the casting silhouette is the decoration's shape
/// (the rounded rect, or the plain rect as a zero-radius one),
/// inflated by the spread radius and displaced by the offset; the blur
/// radius rides as the shadow primitive's elevation (the engine's blur
/// input).
fn paint_shadow(
    canvas: &mut Canvas,
    rect: Rect<Pixels>,
    rrect: Option<RRect>,
    shadow: &BoxShadow<Pixels>,
) {
    let base = rrect.unwrap_or_else(|| RRect::from_rect_circular(rect, Pixels(0.0)));
    let mut silhouette = base.inflate(shadow.spread_radius);
    silhouette.rect = silhouette.rect.translate_offset(shadow.offset);
    canvas.draw_shadow(
        &Path::from_rrect(silhouette),
        shadow.color,
        shadow.blur_radius.get(),
    );
}

/// Resolves an alignment-relative [`Gradient`] into a pixel-space
/// [`Shader`] for the given rect. Alignment is the (-1,-1)..(1,1)
/// space over the rect; the radial radius is a fraction of the
/// shortest side (Flutter parity).
#[must_use]
pub fn resolve_gradient(gradient: &Gradient, rect: Rect<Pixels>) -> Shader {
    let center = rect.center();
    let half_w = rect.width().get() / 2.0;
    let half_h = rect.height().get() / 2.0;
    let at = |alignment: flui_types::Alignment| {
        Offset::new(
            Pixels(center.x.get() + alignment.x * half_w),
            Pixels(center.y.get() + alignment.y * half_h),
        )
    };

    match gradient {
        Gradient::Linear(linear) => Shader::LinearGradient {
            from: at(linear.begin),
            to: at(linear.end),
            colors: linear.colors.clone(),
            stops: linear.stops.clone(),
            tile_mode: linear.tile_mode,
        },
        Gradient::Radial(radial) => Shader::RadialGradient {
            center: at(radial.center),
            radius: radial.radius * half_w.min(half_h) * 2.0,
            colors: radial.colors.clone(),
            stops: radial.stops.clone(),
            tile_mode: radial.tile_mode,
            focal: None,
            focal_radius: None,
        },
        Gradient::Sweep(sweep) => Shader::SweepGradient {
            center: at(sweep.center),
            colors: sweep.colors.clone(),
            stops: sweep.stops.clone(),
            tile_mode: sweep.tile_mode,
            start_angle: sweep.start_angle,
            end_angle: sweep.end_angle,
        },
    }
}

/// The decoration image, fitted into the rect per its `BoxFit` (the
/// repeat modes tile the image at its natural size).
fn paint_decoration_image(
    canvas: &mut Canvas,
    rect: Rect<Pixels>,
    image: &flui_types::styling::DecorationImage,
) {
    use flui_types::layout::BoxFit;
    use flui_types::styling::ImageRepeat;

    if image.repeat != ImageRepeat::NoRepeat {
        canvas.draw_image_repeat(image.image.clone(), rect, image.repeat, None);
        return;
    }

    #[allow(clippy::cast_precision_loss)]
    // image dimensions are far below f32's 24-bit integer range
    let (src_w, src_h) = (image.image.width() as f32, image.image.height() as f32);
    let (dst_w, dst_h) = (rect.width().get(), rect.height().get());
    let fit = image.fit.unwrap_or(BoxFit::ScaleDown);

    let (out_w, out_h) = if src_w <= 0.0 || src_h <= 0.0 {
        (dst_w, dst_h)
    } else {
        match fit {
            BoxFit::Fill => (dst_w, dst_h),
            BoxFit::Contain => {
                let scale = (dst_w / src_w).min(dst_h / src_h);
                (src_w * scale, src_h * scale)
            }
            BoxFit::Cover => {
                let scale = (dst_w / src_w).max(dst_h / src_h);
                (src_w * scale, src_h * scale)
            }
            BoxFit::FitWidth => {
                let scale = dst_w / src_w;
                (dst_w, src_h * scale)
            }
            BoxFit::FitHeight => {
                let scale = dst_h / src_h;
                (src_w * scale, dst_h)
            }
            BoxFit::None => (src_w, src_h),
            BoxFit::ScaleDown => {
                let scale = (dst_w / src_w).min(dst_h / src_h).min(1.0);
                (src_w * scale, src_h * scale)
            }
        }
    };

    // Alignment positions the fitted box within the paint rect.
    let free_w = dst_w - out_w;
    let free_h = dst_h - out_h;
    let left = rect.min.x.get() + f32::midpoint(image.alignment.x, 1.0) * free_w;
    let top = rect.min.y.get() + f32::midpoint(image.alignment.y, 1.0) * free_h;
    let dst = Rect::from_ltrb(
        Pixels(left),
        Pixels(top),
        Pixels(left + out_w),
        Pixels(top + out_h),
    );

    let paint = (image.opacity < 1.0).then(|| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        // clamped 0..=1 then scaled to u8 range
        let alpha = (image.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
        Paint::fill(Color::rgba(255, 255, 255, alpha))
    });
    canvas.draw_image(image.image.clone(), dst, paint.as_ref());
}

/// The border, on top of everything.
///
/// A uniform border strokes the shape exactly INSIDE its edge via a
/// filled outer/inner rounded-rect pair (`draw_drrect`) — Flutter's
/// inside-stroke semantics without relying on stroke centering. A
/// non-uniform border falls back to four filled edge rects; combining
/// per-side widths with a border radius is unsupported in Flutter as
/// well (it asserts), so the radius is ignored on that path.
///
/// `pub(crate)`: also reused by `crate::table_border::paint_table_border`
/// for `TableBorder`'s outer edge, so the uniform/non-uniform split is
/// written once.
pub(crate) fn paint_border(
    canvas: &mut Canvas,
    rect: Rect<Pixels>,
    rrect: Option<RRect>,
    border: &flui_types::styling::Border<Pixels>,
) {
    if border.is_uniform() {
        // Uniform ⇒ all four sides are the same `Some` (or all `None`,
        // in which case there is nothing to draw).
        let Some(side) = border.top else {
            return;
        };
        if side.width.get() <= 0.0 {
            return;
        }
        let outer = rrect.unwrap_or_else(|| RRect::from_rect_circular(rect, Pixels(0.0)));
        let inner = outer.inflate(Pixels(-side.width.get()));
        canvas.draw_drrect(outer, inner, &Paint::fill(side.color));
        return;
    }

    let side_width = |side: &Option<flui_types::styling::BorderSide<Pixels>>| {
        side.map_or(0.0, |s| s.width.get())
    };
    let side_color = |side: &Option<flui_types::styling::BorderSide<Pixels>>| {
        side.map_or(Color::TRANSPARENT, |s| s.color)
    };
    let (l, t, r, b) = (
        side_width(&border.left),
        side_width(&border.top),
        side_width(&border.right),
        side_width(&border.bottom),
    );
    let (x0, y0, x1, y1) = (
        rect.min.x.get(),
        rect.min.y.get(),
        rect.max.x.get(),
        rect.max.y.get(),
    );
    if t > 0.0 {
        canvas.draw_rect(
            Rect::from_ltrb(Pixels(x0), Pixels(y0), Pixels(x1), Pixels(y0 + t)),
            &Paint::fill(side_color(&border.top)),
        );
    }
    if b > 0.0 {
        canvas.draw_rect(
            Rect::from_ltrb(Pixels(x0), Pixels(y1 - b), Pixels(x1), Pixels(y1)),
            &Paint::fill(side_color(&border.bottom)),
        );
    }
    if l > 0.0 {
        canvas.draw_rect(
            Rect::from_ltrb(Pixels(x0), Pixels(y0 + t), Pixels(x0 + l), Pixels(y1 - b)),
            &Paint::fill(side_color(&border.left)),
        );
    }
    if r > 0.0 {
        canvas.draw_rect(
            Rect::from_ltrb(Pixels(x1 - r), Pixels(y0 + t), Pixels(x1), Pixels(y1 - b)),
            &Paint::fill(side_color(&border.right)),
        );
    }
}

/// Point-in-rounded-rect: inside the base rect AND outside none of the
/// four corner ellipses.
fn rrect_contains(rrect: &RRect, point: Point<Pixels>) -> bool {
    let rect = rrect.rect;
    if !rect.contains(point) {
        return false;
    }
    let (px_, py) = (point.x.get(), point.y.get());
    let (x0, y0, x1, y1) = (
        rect.min.x.get(),
        rect.min.y.get(),
        rect.max.x.get(),
        rect.max.y.get(),
    );

    // For each corner: if the point lies within the corner's radius
    // box, it must satisfy the ellipse equation.
    let in_ellipse = |cx: f32, cy: f32, rx: f32, ry: f32| {
        if rx <= 0.0 || ry <= 0.0 {
            return true;
        }
        let nx = (px_ - cx) / rx;
        let ny = (py - cy) / ry;
        nx * nx + ny * ny <= 1.0
    };

    let tl = rrect.top_left;
    if px_ < x0 + tl.x.get() && py < y0 + tl.y.get() {
        return in_ellipse(x0 + tl.x.get(), y0 + tl.y.get(), tl.x.get(), tl.y.get());
    }
    let tr = rrect.top_right;
    if px_ > x1 - tr.x.get() && py < y0 + tr.y.get() {
        return in_ellipse(x1 - tr.x.get(), y0 + tr.y.get(), tr.x.get(), tr.y.get());
    }
    let bl = rrect.bottom_left;
    if px_ < x0 + bl.x.get() && py > y1 - bl.y.get() {
        return in_ellipse(x0 + bl.x.get(), y1 - bl.y.get(), bl.x.get(), bl.y.get());
    }
    let br = rrect.bottom_right;
    if px_ > x1 - br.x.get() && py > y1 - br.y.get() {
        return in_ellipse(x1 - br.x.get(), y1 - br.y.get(), br.x.get(), br.y.get());
    }
    true
}
