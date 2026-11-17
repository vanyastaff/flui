//! What an effect reads and writes, and the damage that closes over it
//! (ADR-0304 §6).
//!
//! A filter samples pixels around the ones it changes, so damage that meets
//! its input changes its whole output, and a retained target outside the
//! damage holds the previous frame's final composition, not the input the
//! filter needs. [`read_closure`] is the one answer to "what does this effect
//! read and write"; [`close_over`] grows a damage region over every footprint
//! it meets, to a fixed point. The scene owner, the differ and the engine's
//! backdrop planning collect a frame's effects into a [`Closures`] set with
//! [`Closures::push_effect`], which joins a backdrop group's members, and
//! close over it.

use flui_foundation::geometry::{Matrix4, Rect, cover};
use flui_painting::DamageExtent;
use flui_painting::paint::ImageFilter;

use rustc_hash::FxHashMap;

use crate::{BackdropGroupKey, DamageRect, DamageRegion, FullReason, Layer};

/// The most rounds [`close_over`] runs before the frame is
/// `Full(Unbounded)`: a chain of filters each exposing the next cannot turn
/// planning into quadratic work.
pub const MAX_CLOSURE_ROUNDS: usize = 64;

/// What an effect reads and what it changes, in surface pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
#[expect(
    clippy::exhaustive_enums,
    reason = "an effect reaches its own pixels, a bound, or the whole surface, and the damage planner handles each"
)]
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

/// The closures of a frame's effects, and the backdrop groups they join.
///
/// The members of one [`BackdropGroupKey`] read one backdrop, captured where
/// the first of them that writes paints. Damage that meets any member takes
/// the rect over every member's footprint, so every member lies in one rect
/// of the region: a pass that redraws one member redraws them all, and the
/// member that captures is the one that captures in a full frame.
#[derive(Debug, Default, Clone)]
pub struct Closures {
    /// Each closure, with the index in `groups` of the group it joins.
    entries: Vec<(Closure, Option<u32>)>,
    /// The groups met so far, by first appearance.
    groups: FxHashMap<BackdropGroupKey, u32>,
}

impl Closures {
    /// No closures.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a closure that no layer gives: a chunk that reads the
    /// destination, or a filter with no bound. An effect layer goes through
    /// [`Closures::push_effect`], which keeps its backdrop group.
    pub fn push(&mut self, closure: Closure) {
        self.entries.push((closure, None));
    }

    /// Adds [`read_closure`] of `effect`, joined to its backdrop group when
    /// it is a grouped backdrop filter.
    pub fn push_effect(&mut self, effect: &Layer, ctm: &Matrix4, content: Rect<f64>) {
        let closure = read_closure(effect, ctm, content);
        let group = match effect {
            Layer::BackdropFilter(backdrop) => {
                backdrop.group().map(|group| self.group_index(group))
            }
            _ => None,
        };
        self.entries.push((closure, group));
    }

    /// Adds the grouped closures of `other`, another frame's, each joining
    /// the same group here: a member that joins or leaves a group between the
    /// two frames moves the group's capture, so the damage takes the group as
    /// either frame holds it.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn extend_groups(&mut self, other: &Closures) {
        let mut remap = vec![0_u32; other.groups.len()];
        for (key, &index) in &other.groups {
            remap[index as usize] = self.group_index(key);
        }
        self.entries
            .extend(other.entries.iter().filter_map(|&(closure, group)| {
                group.map(|group| (closure, Some(remap[group as usize])))
            }));
    }

    fn group_index(&mut self, group: &BackdropGroupKey) -> u32 {
        let next = u32::try_from(self.groups.len())
            .expect("BUG: a frame holds fewer than 2^32 backdrop groups");
        *self.groups.entry(group.clone()).or_insert(next)
    }

    /// Forgets every closure, keeping the storage.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.groups.clear();
    }
}

/// `damage` grown over every closure it meets, on a surface of `surface`
/// device pixels.
///
/// Damage that meets a closure's `reads ∪ writes`, unless one rect of the
/// region already contains it, takes all of it; damage that meets one
/// member of a backdrop group takes the rect over every member's ([`Closures`]). The joins
/// and the region's merges repeat to a fixed point, at most
/// [`MAX_CLOSURE_ROUNDS`] rounds, past which the answer is
/// `Full(Unbounded)`. Damage that meets a [`Closure::Unbounded`] is
/// `Full(Unbounded)`, and a closure that is not finite makes it
/// `Full(NonFinite)`. `Unchanged` and `Full` damage come back as they are.
#[must_use]
pub fn close_over(damage: DamageRegion, closures: &Closures, surface: (u32, u32)) -> DamageRegion {
    let DamageRegion::Region(mut region) = damage else {
        return damage;
    };
    // Each footprint, and what damage that meets it takes: itself, or the
    // rect over every member of its group.
    let mut footprints: Vec<(DamageRect, Option<u32>)> = Vec::new();
    let mut spans: Vec<Option<DamageRect>> = vec![None; closures.groups.len()];
    for &(closure, group) in &closures.entries {
        match closure {
            Closure::None => {}
            Closure::Unbounded => return DamageRegion::Full(FullReason::Unbounded),
            Closure::Bounded { reads, writes } => {
                match DamageRect::try_covering(reads.union(&writes), surface) {
                    Ok(Some(footprint)) => {
                        if let Some(group) = group {
                            let span = &mut spans[group as usize];
                            *span = Some(span.map_or(footprint, |span| span.union(footprint)));
                        }
                        footprints.push((footprint, group));
                    }
                    Ok(None) => {}
                    Err(_) => return DamageRegion::Full(FullReason::NonFinite),
                }
            }
        }
    }
    for _ in 0..MAX_CLOSURE_ROUNDS {
        let mut grew = false;
        for &(footprint, group) in &footprints {
            let rects = region.rects();
            if !rects.iter().any(|rect| rect.overlaps(footprint)) {
                continue;
            }
            let takes = group
                .and_then(|group| spans[group as usize])
                .unwrap_or(footprint);
            if !rects.iter().any(|rect| contains(*rect, takes)) {
                region.insert(takes);
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

/// How far, in device pixels, what `filter` draws reaches past its input
/// mapped to the surface, when that input already reaches `inner` past its
/// own mapped bounds. A kernel adds the renderer's growth, which is physical
/// pixels whatever the transform above it. A magnifier (a uniform scale)
/// scales what it takes, the source pixel its input covers in part and half
/// a source pixel of bilinear read included, and
/// draws a pixel past it. Any other transform filter stretches device
/// distances by a factor its ancestors set, so what it reaches has no bound
/// known inside its boundary. Bounds in a boundary's space carry this
/// separately, since a shrinking ancestor would shrink it with them.
pub(crate) fn device_spread(filter: &ImageFilter, inner: f64) -> f64 {
    match filter {
        ImageFilter::Blur { .. } | ImageFilter::Dilate { .. } | ImageFilter::Erode { .. } => {
            inner + renderer_growth(filter)
        }
        ImageFilter::Transform {
            transform: map,
            sampling,
        } => match map.inverse_coefficients() {
            // The source is whole device pixels: one the input covers in part
            // is read whole, at most a pixel past it.
            [xx, 0.0, 0.0, yy, _, _] if xx == yy && xx > 0.0 => {
                (inner + 1.0 + sampling.reach()) / xx + 1.0
            }
            _ => f64::INFINITY,
        },
        ImageFilter::Compose(filters) => filters
            .iter()
            .fold(inner, |inner, filter| device_spread(filter, inner)),
        _ => inner,
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

/// The map a transform filter whose source-from-shown map has the
/// `inverse` coefficients applies on the surface under `transform`.
fn shown_map(inverse: [f64; 6], transform: &Matrix4) -> Option<Matrix4> {
    let [xx, yx, xy, yy, x0, y0] = inverse;
    let mut shown_to_source = Matrix4::IDENTITY;
    shown_to_source.m[0] = xx;
    shown_to_source.m[1] = yx;
    shown_to_source.m[4] = xy;
    shown_to_source.m[5] = yy;
    shown_to_source.m[12] = x0;
    shown_to_source.m[13] = y0;
    Some(*transform * shown_to_source.try_inverse()? * transform.try_inverse()?)
}

/// Where `filter` moves what it takes from `input` under `transform`, its
/// spread left out: a transform filter's map, nothing for a kernel. The
/// spread is the filter's `device_spread`, which a footprint adds once on
/// the surface. `None` when the map has no bound, under a projective
/// ancestor.
pub(crate) fn moved(
    filter: &ImageFilter,
    input: Rect<f64>,
    transform: &Matrix4,
) -> Option<Rect<f64>> {
    match filter {
        ImageFilter::Transform { transform: map, .. } => {
            if is_projective(transform) {
                return None;
            }
            let moved = shown_map(map.inverse_coefficients(), transform)?;
            Some(moved.transform_rect(&input)).filter(Rect::is_finite)
        }
        ImageFilter::Compose(filters) => filters
            .iter()
            .try_fold(input, |input, filter| moved(filter, input, transform)),
        _ => Some(input),
    }
}

/// Where `filter` draws what it takes from `input` (surface pixels) under
/// `transform`: spread by a kernel's reach, or moved where a transform
/// sends it, from the whole source pixels the input touches grown by the
/// sampling's reach, and a pixel past that once mapped. `None` when that
/// has no bound, under a projective ancestor.
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
            let moved = shown_map(map.inverse_coefficients(), transform)?;
            // The source is whole device pixels, so a pixel the input covers
            // in part is read whole, and a read reaches past its sample point
            // before the map spreads it, as the renderer inflates its input
            // support (a 10× linear magnifier draws five device pixels past
            // the mapped source).
            let read = cover(input).expand(sampling.reach());
            Some(moved.transform_rect(&read).expand(1.0)).filter(Rect::is_finite)
        }
        ImageFilter::Compose(filters) => filters
            .iter()
            .try_fold(input, |input, filter| footprint(filter, input, transform)),
        _ => Some(input.expand(device_reach(filter, transform))),
    }
}
