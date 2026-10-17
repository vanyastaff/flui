//! Placing an image into a rectangle: fit, alignment, repetition, or a
//! stretchable centre slice. One contract shared by image render objects and
//! decoration images.
//!
//! An image has a size in decoded pixels and a *natural* size in logical
//! pixels (its pixels divided by its scale). Fitting works on the natural
//! size; the source region it crops is then converted back to decoded
//! pixels, axis by axis, so a scaled or non-uniformly sized image keeps its
//! logical geometry.

use flui_foundation::geometry::{Rect, Size};

use crate::{
    Alignment, BoxFit,
    canvas::Canvas,
    paint::{ColorFilter, Image, ImageRepeat, Paint},
};

/// How an image is placed into the rectangle it paints.
///
/// The two placements are separate variants because they do not combine: a
/// centre slice always fills its rectangle, so a fit that crops or letterboxes
/// it, or a repetition, has no meaning there.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum ImagePlacement {
    /// Inscribe the image with `fit`, place it at `alignment` and optionally
    /// repeat it to cover the rest of the rectangle.
    Fitted {
        /// How the natural size is scaled into the rectangle.
        fit: BoxFit,
        /// Where the fitted image sits; on a cropping fit, also which part of
        /// the image stays visible.
        alignment: Alignment,
        /// Whether copies of the fitted image cover the rest of the rectangle.
        repeat: ImageRepeat,
    },
    /// Nine-slice: `center`, in decoded image pixels, stretches in both
    /// directions; the corners keep their natural size and the edges stretch
    /// along their own side. When the rectangle is smaller than the corners
    /// on an axis, the corners shrink proportionally on that axis.
    Sliced {
        /// The stretchable middle of the image, in decoded pixels.
        center: Rect<f64>,
    },
}

impl Default for ImagePlacement {
    /// Centred, scaled down to fit, not repeated.
    fn default() -> Self {
        Self::fitted(BoxFit::ScaleDown, Alignment::CENTER)
    }
}

impl ImagePlacement {
    /// A fitted, unrepeated placement.
    #[must_use]
    pub const fn fitted(fit: BoxFit, alignment: Alignment) -> Self {
        Self::Fitted {
            fit,
            alignment,
            repeat: ImageRepeat::NoRepeat,
        }
    }

    /// A nine-slice placement around `center`, in decoded image pixels.
    #[must_use]
    pub const fn sliced(center: Rect<f64>) -> Self {
        Self::Sliced { center }
    }

    /// Where one copy of the image lands inside `rect`, before repetition.
    ///
    /// `pixels` is the decoded size and `natural` the logical size. `None`
    /// when either is degenerate or non-finite, when `rect` is empty, or when
    /// a centre slice lies outside the image: nothing would be painted.
    #[must_use]
    pub fn destination(
        &self,
        rect: Rect<f64>,
        pixels: Size<f64>,
        natural: Size<f64>,
    ) -> Option<Rect<f64>> {
        match *self {
            Self::Fitted { fit, alignment, .. } => {
                fitted(fit, alignment, rect, pixels, natural).map(|placed| placed.tile)
            }
            Self::Sliced { center } => slices(center, rect, pixels, natural).map(|_| rect),
        }
    }

    /// Paint `image`, whose logical size is `natural`, into `rect`.
    ///
    /// `filter` and `paint` apply to every drawn region alike.
    pub fn paint(
        &self,
        canvas: &mut Canvas,
        rect: Rect<f64>,
        image: &Image,
        natural: Size<f64>,
        filter: Option<ColorFilter>,
        paint: Option<&Paint>,
    ) {
        let pixels = Size::new(f64::from(image.width()), f64::from(image.height()));
        match *self {
            Self::Fitted {
                fit,
                alignment,
                repeat,
            } => {
                let Some(placed) = fitted(fit, alignment, rect, pixels, natural) else {
                    return;
                };
                paint_fitted(canvas, rect, image, pixels, placed, repeat, filter, paint);
            }
            Self::Sliced { center } => {
                let Some(pieces) = slices(center, rect, pixels, natural) else {
                    return;
                };
                for (src, dst) in pieces {
                    if src.width() > 0.0
                        && src.height() > 0.0
                        && dst.width() > 0.0
                        && dst.height() > 0.0
                    {
                        canvas.draw_image_region_effects(
                            image.clone(),
                            src,
                            dst,
                            None,
                            ImageRepeat::NoRepeat,
                            filter,
                            paint,
                        );
                    }
                }
            }
        }
    }
}

/// A fitted image: the decoded source region and its logical tile.
#[derive(Debug, Clone, Copy)]
struct Placed {
    src: Rect<f64>,
    tile: Rect<f64>,
}

/// The natural size bounded to finite values with its ratio preserved; an
/// extent past `f64::MAX` (a tiny positive scale) keeps a meaningful aspect.
fn bounded_natural(pixels: Size<f64>, natural: Size<f64>) -> Option<Size<f64>> {
    let positive = |value: f64| value > 0.0 && !value.is_nan();
    if ![pixels.width, pixels.height]
        .into_iter()
        .all(|v| v.is_finite() && v > 0.0)
        || !positive(natural.width)
        || !positive(natural.height)
    {
        return None;
    }
    if natural.width.is_finite() && natural.height.is_finite() {
        return Some(natural);
    }
    // Both extents overflowed from the same scale: their pixel ratio is the
    // logical ratio.
    let longest = pixels.width.max(pixels.height);
    Some(Size::new(
        (pixels.width / longest) * f64::MAX,
        (pixels.height / longest) * f64::MAX,
    ))
}

fn valid_rect(rect: Rect<f64>) -> bool {
    [rect.left(), rect.top(), rect.right(), rect.bottom()]
        .into_iter()
        .all(f64::is_finite)
        && rect.width() > 0.0
        && rect.height() > 0.0
}

fn fitted(
    fit: BoxFit,
    alignment: Alignment,
    rect: Rect<f64>,
    pixels: Size<f64>,
    natural: Size<f64>,
) -> Option<Placed> {
    if !valid_rect(rect) {
        return None;
    }
    let natural = bounded_natural(pixels, natural)?;
    let output = Size::new(rect.width(), rect.height());
    let sizes = fit.apply(natural, output);
    if !(sizes.destination.width > 0.0 && sizes.destination.height > 0.0) {
        return None;
    }
    let ax = f64::midpoint(alignment.x, 1.0);
    let ay = f64::midpoint(alignment.y, 1.0);
    // Normalize before multiplying by decoded pixels so neither a tiny scale
    // nor a large pixel/natural ratio overflows.
    let to_px_x = |logical: f64| (logical / natural.width) * pixels.width;
    let to_px_y = |logical: f64| (logical / natural.height) * pixels.height;
    let src = Rect::from_xywh(
        to_px_x(ax * (natural.width - sizes.source.width)),
        to_px_y(ay * (natural.height - sizes.source.height)),
        to_px_x(sizes.source.width),
        to_px_y(sizes.source.height),
    );
    let tile = Rect::from_xywh(
        rect.left() + ax * (output.width - sizes.destination.width),
        rect.top() + ay * (output.height - sizes.destination.height),
        sizes.destination.width,
        sizes.destination.height,
    );
    Some(Placed { src, tile })
}

#[expect(
    clippy::too_many_arguments,
    reason = "Records one complete fitted image operation"
)]
fn paint_fitted(
    canvas: &mut Canvas,
    rect: Rect<f64>,
    image: &Image,
    pixels: Size<f64>,
    placed: Placed,
    repeat: ImageRepeat,
    filter: Option<ColorFilter>,
    paint: Option<&Paint>,
) {
    let Placed { src, tile } = placed;
    let repeat = if tile == rect {
        ImageRepeat::NoRepeat
    } else {
        repeat
    };
    let repeated = repeat != ImageRepeat::NoRepeat;
    // An alignment outside [-1, 1] can move the source window past the
    // decoded texels. Intersecting trims the coverage on that axis without
    // changing the repeating period on the other one.
    let Some(clipped) = src.intersect(&Rect::from_xywh(0.0, 0.0, pixels.width, pixels.height))
    else {
        return;
    };
    let sx = tile.width() / src.width();
    let sy = tile.height() / src.height();
    let left_trim = (clipped.left() - src.left()) * sx;
    let top_trim = (clipped.top() - src.top()) * sy;
    let right_trim = (src.right() - clipped.right()) * sx;
    let bottom_trim = (src.bottom() - clipped.bottom()) * sy;
    let coverage = Rect::from_ltrb(
        rect.left() + left_trim,
        rect.top() + top_trim,
        rect.right() - right_trim,
        rect.bottom() - bottom_trim,
    );
    let tile = Rect::from_xywh(
        tile.left() + left_trim,
        tile.top() + top_trim,
        clipped.width() * sx,
        clipped.height() * sy,
    );
    canvas.draw_image_region_effects(
        image.clone(),
        clipped,
        if repeated { coverage } else { tile },
        repeated.then_some(tile),
        repeat,
        filter,
        paint,
    );
}

/// The nine source/destination pairs of a sliced image, row by row.
fn slices(
    center: Rect<f64>,
    rect: Rect<f64>,
    pixels: Size<f64>,
    natural: Size<f64>,
) -> Option<[(Rect<f64>, Rect<f64>); 9]> {
    if !valid_rect(rect) {
        return None;
    }
    let natural = bounded_natural(pixels, natural)?;
    let inside = |low: f64, high: f64, extent: f64| {
        low.is_finite() && high.is_finite() && 0.0 <= low && low <= high && high <= extent
    };
    if !inside(center.left(), center.right(), pixels.width)
        || !inside(center.top(), center.bottom(), pixels.height)
    {
        return None;
    }
    let xs = axis(
        [center.left(), center.right(), pixels.width],
        natural.width / pixels.width,
        rect.left(),
        rect.right(),
    );
    let ys = axis(
        [center.top(), center.bottom(), pixels.height],
        natural.height / pixels.height,
        rect.top(),
        rect.bottom(),
    );
    let sx = [0.0, center.left(), center.right(), pixels.width];
    let sy = [0.0, center.top(), center.bottom(), pixels.height];
    let piece = |row: usize, column: usize| {
        (
            Rect::from_ltrb(sx[column], sy[row], sx[column + 1], sy[row + 1]),
            Rect::from_ltrb(xs[column], ys[row], xs[column + 1], ys[row + 1]),
        )
    };
    Some([
        piece(0, 0),
        piece(0, 1),
        piece(0, 2),
        piece(1, 0),
        piece(1, 1),
        piece(1, 2),
        piece(2, 0),
        piece(2, 1),
        piece(2, 2),
    ])
}

/// Destination edges along one axis: the leading and trailing borders keep
/// their logical extent (`pixels * logical_per_pixel`), the middle takes the
/// rest; borders that do not fit shrink together, keeping their proportion.
fn axis(
    [center_start, center_end, extent]: [f64; 3],
    logical_per_pixel: f64,
    start: f64,
    end: f64,
) -> [f64; 4] {
    let lead = center_start * logical_per_pixel;
    let trail = (extent - center_end) * logical_per_pixel;
    let available = end - start;
    let borders = lead + trail;
    let shrink = if borders > available {
        available / borders
    } else {
        1.0
    };
    let lead_edge = start + lead * shrink;
    let trail_edge = (end - trail * shrink).max(lead_edge);
    [start, lead_edge, trail_edge, end]
}
