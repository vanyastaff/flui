//! What an effect reads and writes, and the damage that closes over it
//! (ADR-0304 §6).
//!
//! A filter samples pixels around the ones it changes, so damage that meets
//! its input changes its whole output, and a retained target outside the
//! damage holds the previous frame's final composition, not the input the
//! filter needs. [`read_closure`] is the one answer to "what does this effect
//! read and write"; [`close_over`] grows a damage region over every footprint
//! it meets, to a fixed point. The scene owner, the differ and the engine's
//! backdrop planning all call these two.

use flui_foundation::geometry::{Matrix4, Rect};
use flui_painting::DamageExtent;
use flui_painting::paint::ImageFilter;

use crate::{DamageRect, DamageRegion, FullReason, Layer};

/// The most rounds [`close_over`] runs before the frame is
/// `Full(Unbounded)`: a chain of filters each exposing the next cannot turn
/// planning into quadratic work.
pub const MAX_CLOSURE_ROUNDS: usize = 64;

/// What an effect reads and what it changes, in surface pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Closure {
    /// It changes only the pixels its content draws, from those pixels.
    None,
    /// It changes `writes` from what lies in `reads`: damage that meets
    /// either takes both.
    Bounded {
        /// What it samples.
        reads: Rect<f64>,
        /// What it changes.
        writes: Rect<f64>,
    },
    /// It reads or changes pixels without a bound: any damage under it is
    /// the whole surface.
    Unbounded,
}

/// What `effect` reads and writes.
///
/// `ctm` maps the space the effect's children draw in to the surface, and
/// `content` is what they draw, in surface pixels. A backdrop filter reads
/// the kernel's reach around its bounds as the renderer implements it, under
/// the norm of `ctm`. A foreground image filter writes where its kernel or
/// transform sends its content and reads only what it writes: it samples its
/// children, not the surface, but the renderer composites its whole output
/// unscissored, so damage that meets it takes all of it. A colour or image
/// filter that paints transparent pixels has no bound. Every other effect
/// changes only the pixels its content draws.
#[must_use]
pub fn read_closure(effect: &Layer, ctm: &Matrix4, content: Rect<f64>) -> Closure {
    match effect {
        Layer::BackdropFilter(backdrop) => {
            let Some(writes) = DamageExtent::rect(backdrop.bounds())
                .transformed(ctm)
                .covering_rect()
            else {
                return Closure::Unbounded;
            };
            let reach = device_reach(backdrop.filter(), ctm)
                .max(transform_reach(backdrop.filter(), backdrop.bounds(), ctm))
                // The kernel's taps land on whole pixels of a rect that is
                // not pixel-aligned.
                + 1.0;
            if !reach.is_finite() {
                return Closure::Unbounded;
            }
            Closure::Bounded {
                reads: writes.expand(reach),
                writes,
            }
        }
        Layer::ImageFilter(filter) => filter_closure(filter.filter(), ctm, content),
        Layer::ColorFilter(filter) if filter.color_filter().modifies_transparent_black() => {
            Closure::Unbounded
        }
        _ => Closure::None,
    }
}

/// [`read_closure`] of a foreground image filter applying `filter` to
/// `content`.
pub(crate) fn filter_closure(filter: &ImageFilter, ctm: &Matrix4, content: Rect<f64>) -> Closure {
    if filter.modifies_transparent_black() {
        return Closure::Unbounded;
    }
    match footprint(filter, content, ctm) {
        Some(writes) => Closure::Bounded {
            reads: writes,
            writes,
        },
        None => Closure::Unbounded,
    }
}

/// `damage` grown over every closure it meets, on a surface of `surface`
/// device pixels.
///
/// Damage that meets a closure's `reads ∪ writes`, unless one rect of the
/// region already contains it, takes all of it. The joins and the region's
/// merges repeat to a fixed point, at most [`MAX_CLOSURE_ROUNDS`] rounds,
/// past which the answer is `Full(Unbounded)`. Damage that meets a
/// [`Closure::Unbounded`] is `Full(Unbounded)`, and a closure that is not
/// finite makes it `Full(NonFinite)`. `Unchanged` and `Full` damage come back
/// as they are.
#[must_use]
pub fn close_over(
    damage: DamageRegion,
    closures: impl IntoIterator<Item = Closure>,
    surface: (u32, u32),
) -> DamageRegion {
    let DamageRegion::Region(mut region) = damage else {
        return damage;
    };
    let mut footprints: Vec<DamageRect> = Vec::new();
    for closure in closures {
        match closure {
            Closure::None => {}
            Closure::Unbounded => return DamageRegion::Full(FullReason::Unbounded),
            Closure::Bounded { reads, writes } => {
                match DamageRect::try_covering(reads.union(&writes), surface) {
                    Ok(Some(footprint)) => footprints.push(footprint),
                    Ok(None) => {}
                    Err(_) => return DamageRegion::Full(FullReason::NonFinite),
                }
            }
        }
    }
    for _ in 0..MAX_CLOSURE_ROUNDS {
        let mut grew = false;
        for &footprint in &footprints {
            let rects = region.rects();
            if rects.iter().any(|rect| rect.overlaps(footprint))
                && !rects.iter().any(|rect| contains(*rect, footprint))
            {
                region.insert(footprint);
                grew = true;
            }
        }
        if !grew {
            return DamageRegion::Region(region);
        }
    }
    DamageRegion::Full(FullReason::Unbounded)
}

fn contains(outer: DamageRect, inner: DamageRect) -> bool {
    outer.left() <= inner.left()
        && outer.top() <= inner.top()
        && outer.right() >= inner.right()
        && outer.bottom() >= inner.bottom()
}

/// How far past its input rect `filter` reads, in local units.
fn filter_reach(filter: &ImageFilter) -> f64 {
    match filter {
        ImageFilter::Blur { sigma_x, sigma_y } => 3.0 * sigma_x.abs().max(sigma_y.abs()),
        ImageFilter::Dilate { radius } | ImageFilter::Erode { radius } => radius.abs(),
        ImageFilter::Compose(filters) => filters.iter().map(filter_reach).sum(),
        _ => 0.0,
    }
}

/// How far past its input `filter` reaches in surface pixels under
/// `transform`: the larger of its local reach scaled by the transform and the
/// growth the GPU renderer gives it.
///
/// The renderer takes a filter's sigma and radius as physical pixels
/// whatever the transform, and grows its output by the kernel half-width:
/// `ceil(sqrt(3) x sigma)` for a blur (`flui-engine`'s `kernel_radius`),
/// `ceil(radius)` for a morphology, summed through a composition. Under a
/// shrinking transform that exceeds the scaled local reach; under a
/// magnifying one the scaled reach is larger. Damage takes whichever is
/// further, so it covers either reading of the sigma.
fn device_reach(filter: &ImageFilter, transform: &Matrix4) -> f64 {
    (filter_reach(filter) * max_scale(transform)).max(renderer_growth(filter))
}

/// How far past `bounds` a transform filter reads, in surface pixels under
/// `transform`: where its inverse sends the bounds, mapped to the surface and
/// measured against the bounds mapped the same way. Measuring in surface
/// space keeps a sheared ancestor's mixing of the axes. A magnifier about a
/// point inside reads within its bounds; a shrinking or displacing transform
/// reads outside. A projective ancestor has no bound.
fn transform_reach(filter: &ImageFilter, bounds: Rect<f64>, transform: &Matrix4) -> f64 {
    match filter {
        ImageFilter::Transform { transform: map, .. } => {
            if is_projective(transform) {
                return f64::INFINITY;
            }
            let m = &transform.m;
            let to_surface =
                |(x, y): (f64, f64)| (m[0] * x + m[4] * y + m[12], m[1] * x + m[5] * y + m[13]);
            let [xx, yx, xy, yy, x0, y0] = map.inverse_coefficients();
            let corners = [
                bounds.top_left(),
                bounds.top_right(),
                bounds.bottom_left(),
                bounds.bottom_right(),
            ];
            let box_of = |points: [(f64, f64); 4]| {
                points.iter().fold(
                    (
                        f64::INFINITY,
                        f64::INFINITY,
                        f64::NEG_INFINITY,
                        f64::NEG_INFINITY,
                    ),
                    |(left, top, right, bottom), &(x, y)| {
                        (left.min(x), top.min(y), right.max(x), bottom.max(y))
                    },
                )
            };
            let (left, top, right, bottom) =
                box_of(corners.map(|corner| to_surface((corner.x, corner.y))));
            let (read_left, read_top, read_right, read_bottom) = box_of(corners.map(|corner| {
                to_surface((
                    xx * corner.x + xy * corner.y + x0,
                    yx * corner.x + yy * corner.y + y0,
                ))
            }));
            (left - read_left)
                .max(read_right - right)
                .max(top - read_top)
                .max(read_bottom - bottom)
                .max(0.0)
                // A bilinear read also takes the texel past the edge.
                + 1.0
        }
        ImageFilter::Compose(filters) => filters
            .iter()
            .map(|filter| transform_reach(filter, bounds, transform))
            .sum(),
        _ => 0.0,
    }
}
/// The growth the GPU renderer gives `filter`, in physical pixels.
fn renderer_growth(filter: &ImageFilter) -> f64 {
    match filter {
        ImageFilter::Blur { sigma_x, sigma_y } => {
            (3.0_f64.sqrt() * sigma_x.abs().max(sigma_y.abs())).ceil()
        }
        ImageFilter::Dilate { radius } | ImageFilter::Erode { radius } => radius.abs().ceil(),
        ImageFilter::Compose(filters) => filters.iter().map(renderer_growth).sum(),
        _ => 0.0,
    }
}

/// The largest factor `transform` stretches a local length by.
fn max_scale(transform: &Matrix4) -> f64 {
    let m = &transform.m;
    m[0].hypot(m[1]).max(m[4].hypot(m[5]))
}

/// Whether `transform` maps the plane projectively; see
/// `flui_painting`'s damage extent for why a projective map is not bounded.
pub(crate) fn is_projective(transform: &Matrix4) -> bool {
    let m = &transform.m;
    m[3].abs() > f64::EPSILON || m[7].abs() > f64::EPSILON || (m[15] - 1.0).abs() > f64::EPSILON
}

/// Where `filter` draws what it takes from `input` (surface pixels) under
/// `transform`: spread by a kernel's reach, or moved where a transform
/// sends it, the sampling's reach before the map and a pixel after it. `None` when that has no
/// bound, under a projective ancestor.
pub(crate) fn footprint(
    filter: &ImageFilter,
    input: Rect<f64>,
    transform: &Matrix4,
) -> Option<Rect<f64>> {
    match filter {
        ImageFilter::Transform {
            transform: map,
            sampling,
        } => {
            if is_projective(transform) {
                return None;
            }
            let [xx, yx, xy, yy, x0, y0] = map.inverse_coefficients();
            let mut shown_to_source = Matrix4::IDENTITY;
            shown_to_source.m[0] = xx;
            shown_to_source.m[1] = yx;
            shown_to_source.m[4] = xy;
            shown_to_source.m[5] = yy;
            shown_to_source.m[12] = x0;
            shown_to_source.m[13] = y0;
            let moved = *transform * shown_to_source.try_inverse()? * transform.try_inverse()?;
            // A read reaches past its sample point before the map spreads it,
            // as the renderer inflates its input support (a 10× linear
            // magnifier draws five device pixels past the mapped source).
            let read = input.expand(sampling.reach());
            Some(moved.transform_rect(&read).expand(1.0)).filter(Rect::is_finite)
        }
        ImageFilter::Compose(filters) => filters
            .iter()
            .try_fold(input, |input, filter| footprint(filter, input, transform)),
        _ => Some(input.expand(device_reach(filter, transform))),
    }
}
