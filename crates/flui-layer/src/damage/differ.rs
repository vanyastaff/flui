//! [`LayerDiffer`]: one walk per frame that records each stamped boundary's
//! content token, placement and own region, and a comparison against the
//! previous frame's records.

use std::collections::HashMap;

use flui_foundation::geometry::{Matrix4, Rect};
use flui_foundation::{LayerId, RenderId};
use flui_painting::DamageExtent;
use flui_painting::paint::ImageFilter;

use super::DamageMode;
use crate::{
    ContentToken, DamageRect, DamageRegion, Layer, LayerTree, Scene, resolve_follower_offset,
};

/// Produces a frame's [`DamageRegion`] by comparing its repaint-boundary
/// stamps with the previous frame's.
///
/// Holds one frame of records: for every stamped layer, its boundary's
/// [`ContentToken`], its placement (the accumulated transform and the effect
/// layers above it) and its own region in surface pixels. A boundary
/// contributes damage when it was added (its new region), removed (its old
/// region), or kept with a different token or placement (both), or kept but
/// painted in a different order relative to the other kept boundaries (both:
/// every kept boundary outside the longest run still in its old order).
/// Content that
/// cannot be vouched for by a token — textures (a texture layer or a picture's
/// texture draw), platform views, live canvases, performance overlays and
/// anything under a follower — is damaged on every frame at its old and new
/// positions. A backdrop filter whose (blur-reach
/// widened) bounds meet the damage joins it, repeatedly, until nothing more
/// joins; so does a foreground image filter's footprint, and the whole
/// surface when a colour or image filter that paints transparent pixels is
/// present.
///
/// The answer is [`DamageRegion::Full`] when the frames cannot be paired:
/// the first frame, a surface size change, a root that no boundary stamped or
/// whose boundary or placement changed, a boundary stamped twice, or a
/// damaged area above the mode's `full_above` fraction. It is
/// [`DamageRegion::Unchanged`] when nothing differs.
///
/// Damage is relative to the previous scene passed to [`Self::diff`]. A caller
/// that renders a scene some other way (a plugin frame, a recovery path) calls
/// [`Self::forget`] so the next diff does not assume the screen shows the
/// scene this differ last saw.
#[derive(Debug)]
pub struct LayerDiffer {
    mode: DamageMode,
    previous: Option<Frame>,
}

impl Default for LayerDiffer {
    fn default() -> Self {
        Self::new(DamageMode::default())
    }
}

impl LayerDiffer {
    /// A differ with no previous frame, so its first diff is `Full`.
    #[must_use]
    pub fn new(mode: DamageMode) -> Self {
        Self {
            mode,
            previous: None,
        }
    }

    /// Switches mode. Any switch forgets the previous frame, so the next diff
    /// is `Full`; switching [`DamageMode::Off`] also releases its records.
    pub fn set_mode(&mut self, mode: DamageMode) {
        self.mode = mode;
        self.previous = None;
    }

    /// Drops the previous frame's records, so the next diff is `Full`.
    pub fn forget(&mut self) {
        self.previous = None;
    }

    /// How many boundary records the differ holds from the previous frame —
    /// zero with the mode off.
    #[must_use]
    pub fn retained_boundaries(&self) -> usize {
        self.previous
            .as_ref()
            .map_or(0, |frame| frame.boundaries.len())
    }

    /// The damage of `scene` against the scene this differ saw last, on a
    /// surface of `surface` physical pixels `(width, height)`.
    pub fn diff(&mut self, scene: &Scene, surface: (u32, u32)) -> DamageRegion {
        let DamageMode::On { full_above } = self.mode else {
            self.previous = None;
            return DamageRegion::Full;
        };
        if surface.0 == 0 || surface.1 == 0 {
            self.previous = None;
            return DamageRegion::Full;
        }
        let current = Frame::capture(scene.tree(), surface);
        let region = match (&self.previous, &current) {
            (Some(previous), Some(current)) => compare(previous, current, full_above),
            _ => DamageRegion::Full,
        };
        self.previous = current;
        region
    }
}

/// One frame's records.
#[derive(Debug)]
struct Frame {
    surface: (u32, u32),
    /// The root layer's boundary.
    root: RenderId,
    /// Every effect layer on some stamped layer's ancestor chain, cloned once
    /// per frame; [`Record::effects`] indexes into it.
    effects: Vec<Layer>,
    boundaries: HashMap<RenderId, Record>,
    /// Content no token vouches for, in surface pixels.
    volatile: Option<Rect<f64>>,
    /// Every backdrop filter's surface bounds and how far its filter reads
    /// past them, in surface pixels.
    backdrops: Vec<(Rect<f64>, f64)>,
    /// Every foreground image filter's footprint in surface pixels: its
    /// children's extents grown by how far the filter spreads them; and the
    /// whole surface for each colour or image filter that paints transparent
    /// pixels.
    foreground: Vec<Rect<f64>>,
}

/// A foreground image filter the walk is inside of, accumulating its input.
#[derive(Debug)]
struct ForegroundFilter {
    /// The filter this one is nested in.
    parent: Option<usize>,
    /// The union of its children's extents, unclipped, in surface pixels.
    input: Option<Rect<f64>>,
    /// How far the filter spreads its input, in surface pixels.
    reach: f64,
}

/// One stamped layer.
#[derive(Debug)]
struct Record {
    content: ContentToken,
    /// Surface transform of the stamped layer's children.
    transform: Matrix4,
    /// Effect layers above and at the stamped layer, root first, as indices
    /// into [`Frame::effects`].
    effects: Vec<usize>,
    /// The pixels the boundary's own content can reach: its subtree minus
    /// every nested boundary's.
    region: Option<Rect<f64>>,
    /// Position in the frame's paint order among stamped layers (the walk
    /// is a pre-order, back to front).
    order: usize,
}

/// How far a subtree's content can reach.
#[derive(Debug, Clone, Copy)]
enum Reach {
    /// Its drawing extents, mapped by the transform and clipped.
    Exact,
    /// Anywhere inside this rect (the whole surface for `None`): under an
    /// image filter, whose output spreads past its input, or a perspective
    /// transform this walk does not bound.
    Within(Option<Rect<f64>>),
}

/// What the walk carries from a layer to its children.
#[derive(Debug, Clone, Copy)]
struct Ctx {
    transform: Matrix4,
    clip: Option<Rect<f64>>,
    reach: Reach,
    /// Head of this layer's effect chain in the walk's chain vector.
    effects: Option<usize>,
    owner: Option<RenderId>,
    volatile: bool,
    /// The innermost foreground image filter above this layer, as an index
    /// into the walk's filter list.
    filter: Option<usize>,
}

impl Frame {
    /// Walks `tree` once; `None` when its boundaries cannot be paired with
    /// another frame's (an unstamped root, or one boundary stamped twice).
    fn capture(tree: &LayerTree, surface: (u32, u32)) -> Option<Self> {
        let root = tree.get(tree.root())?.render_id()?;
        let full = Rect::from_ltrb(0.0, 0.0, f64::from(surface.0), f64::from(surface.1));
        let mut frame = Self {
            surface,
            root,
            effects: Vec::new(),
            boundaries: HashMap::new(),
            volatile: None,
            backdrops: Vec::new(),
            foreground: Vec::new(),
        };
        let mut filters: Vec<ForegroundFilter> = Vec::new();
        // (parent link, index into `frame.effects`): a persistent list, so a
        // layer's chain is its parent's plus at most one link.
        let mut chain: Vec<(Option<usize>, usize)> = Vec::new();
        let mut stack: Vec<(LayerId, Ctx)> = vec![(
            tree.root(),
            Ctx {
                transform: Matrix4::IDENTITY,
                clip: None,
                reach: Reach::Exact,
                effects: None,
                owner: None,
                volatile: false,
                filter: None,
            },
        )];

        while let Some((id, parent)) = stack.pop() {
            let Some(node) = tree.get(id) else {
                continue;
            };
            let layer = node.layer();
            let mut ctx = parent;

            match layer {
                Layer::Transform(transform) => ctx.transform *= *transform.transform(),
                Layer::Follower(_) => {
                    // Placed at composite time against its leader, which may
                    // sit in another boundary: nothing here vouches for where
                    // it lands next frame.
                    let Some(offset) = resolve_follower_offset(tree, id) else {
                        continue;
                    };
                    ctx.transform *= Matrix4::translation(offset.dx, offset.dy, 0.0);
                    ctx.volatile = true;
                }
                _ => {
                    let offset = layer.local_translation();
                    if offset.dx != 0.0 || offset.dy != 0.0 {
                        ctx.transform *= Matrix4::translation(offset.dx, offset.dy, 0.0);
                    }
                }
            }
            let projective = is_projective(&ctx.transform);
            if matches!(ctx.reach, Reach::Exact)
                && (projective || matches!(layer, Layer::ImageFilter(_)))
            {
                ctx.reach = Reach::Within(ctx.clip);
            }

            if let Layer::ImageFilter(filter) = layer {
                filters.push(ForegroundFilter {
                    parent: ctx.filter,
                    input: None,
                    reach: device_reach(filter.filter(), &ctx.transform),
                });
                ctx.filter = Some(filters.len() - 1);
            }
            if is_effect(layer) {
                frame.effects.push(layer.clone());
                chain.push((ctx.effects, frame.effects.len() - 1));
                ctx.effects = Some(chain.len() - 1);
            }
            if let Some(bounds) = clip_bounds(layer)
                && !projective
            {
                let clip = ctx.transform.transform_rect(&bounds);
                ctx.clip = Some(match ctx.clip {
                    // An empty intersection keeps a zero-sized clip, which
                    // every extent below then intersects to nothing.
                    Some(outer) => outer.intersect(&clip).unwrap_or(Rect::from_ltrb(
                        clip.left(),
                        clip.top(),
                        clip.left(),
                        clip.top(),
                    )),
                    None => clip,
                });
            }

            if let Some(stamp) = node.boundary() {
                let render_id = stamp.render_id();
                if frame.boundaries.contains_key(&render_id) {
                    return None;
                }
                let order = frame.boundaries.len();
                frame.boundaries.insert(
                    render_id,
                    Record {
                        content: stamp.content().clone(),
                        transform: ctx.transform,
                        effects: materialize(&chain, ctx.effects),
                        region: None,
                        order,
                    },
                );
                ctx.owner = Some(render_id);
            }

            // Whether the composite this extent describes honours the clips
            // above it. An offscreen effect's result (a shader mask, a
            // backdrop filter) is composited over its bounds with no scissor
            // and captured without the ancestor clip, and a save layer over
            // the viewport (an opacity layer whose blend changes pixels a
            // transparent source covers, a colour filter that paints
            // transparent pixels) is taken as reaching past the clip too:
            // the unclipped answer can only repaint more.
            let mut clipped = true;
            let extent = match layer {
                Layer::Picture(picture) => {
                    // A texture draw's pixels come from a texture its
                    // producer replaces behind the same id: the picture's
                    // identity does not vouch for them.
                    if let Some(rect) = picture
                        .picture()
                        .volatile_extent()
                        .and_then(|extent| place(extent, &ctx, full, true))
                    {
                        frame.volatile = Some(join(frame.volatile, rect));
                    }
                    picture.picture().damage_extent()
                }
                Layer::Canvas(canvas) => {
                    ctx.volatile = true;
                    canvas.display_list().damage_extent()
                }
                Layer::Texture(texture) => {
                    ctx.volatile = true;
                    Some(DamageExtent::rect(texture.bounds()))
                }
                Layer::PlatformView(view) => {
                    ctx.volatile = true;
                    Some(DamageExtent::rect(view.bounds()))
                }
                Layer::PerformanceOverlay(overlay) => {
                    ctx.volatile = true;
                    Some(DamageExtent::rect(overlay.bounds()))
                }
                // The masked result composites over the mask's whole bounds
                // with the mask's blend mode: under a destination-replacing
                // mode (Src, Clear, DstIn...) the pixels it changes reach the
                // whole rect, not just the children's ink.
                Layer::ShaderMask(mask) => {
                    clipped = false;
                    Some(DamageExtent::rect(mask.bounds()))
                }
                // Such a layer composites over the whole viewport, and every
                // pixel of it can change.
                Layer::Opacity(opacity)
                    if !opacity.blend().keeps_destination_under_transparent_source() =>
                {
                    clipped = false;
                    Some(DamageExtent::Unbounded)
                }
                // The renderer records its child under the damage scissor
                // but composites the filtered result without one, so outside
                // the damage it filters a truncated input over pixels that
                // already hold the filter's output. It is a viewport-wide
                // footprint: damage anywhere takes all of it, even while the
                // layer itself is unchanged. A colour filter and an image
                // filter answer the same question through the same predicate.
                Layer::ColorFilter(filter)
                    if filter.color_filter().modifies_transparent_black() =>
                {
                    clipped = false;
                    frame.foreground.push(full);
                    Some(DamageExtent::Unbounded)
                }
                Layer::ImageFilter(filter) if filter.filter().modifies_transparent_black() => {
                    clipped = false;
                    frame.foreground.push(full);
                    Some(DamageExtent::Unbounded)
                }
                Layer::BackdropFilter(backdrop) => {
                    clipped = false;
                    let bounds = DamageExtent::rect(backdrop.bounds());
                    if let Some(rect) = place(bounds, &ctx, full, false) {
                        let reach = device_reach(backdrop.filter(), &ctx.transform);
                        frame.backdrops.push((rect, reach));
                    }
                    Some(bounds)
                }
                _ => None,
            };
            if let (Some(extent), Some(filter)) = (extent, ctx.filter) {
                let input = extent
                    .transformed(&ctx.transform)
                    .covering_rect()
                    .filter(Rect::is_finite)
                    .unwrap_or(full);
                filters[filter].input = Some(join(filters[filter].input, input));
            }
            if let Some(rect) = extent.and_then(|extent| place(extent, &ctx, full, clipped)) {
                if let Some(record) = ctx.owner.and_then(|owner| frame.boundaries.get_mut(&owner)) {
                    record.region = Some(join(record.region, rect));
                }
                if ctx.volatile {
                    frame.volatile = Some(join(frame.volatile, rect));
                }
            }

            for &child in node.children().iter().rev() {
                stack.push((child, ctx));
            }
        }
        // A nested filter comes after its parent in the list, so walking it
        // backwards folds each footprint into its parent's input first.
        for index in (0..filters.len()).rev() {
            let Some(input) = filters[index].input else {
                continue;
            };
            let Some(footprint) = input.expand(filters[index].reach).intersect(&full) else {
                continue;
            };
            frame.foreground.push(footprint);
            if let Some(parent) = filters[index].parent {
                filters[parent].input = Some(join(filters[parent].input, footprint));
            }
        }
        Some(frame)
    }
}

/// The damage of `current` against `previous`.
fn compare(previous: &Frame, current: &Frame, full_above: f64) -> DamageRegion {
    if previous.surface != current.surface || previous.root != current.root {
        return DamageRegion::Full;
    }
    let (Some(old_root), Some(new_root)) = (
        previous.boundaries.get(&previous.root),
        current.boundaries.get(&current.root),
    ) else {
        return DamageRegion::Full;
    };
    if !same_placement(previous, old_root, current, new_root) {
        return DamageRegion::Full;
    }

    let mut damage: Option<Rect<f64>> = None;
    let mut add = |rect: Option<Rect<f64>>| {
        if let Some(rect) = rect {
            damage = Some(join(damage, rect));
        }
    };
    for (render_id, new) in &current.boundaries {
        match previous.boundaries.get(render_id) {
            None => add(new.region),
            Some(old) => {
                if old.content != new.content || !same_placement(previous, old, current, new) {
                    add(old.region);
                    add(new.region);
                }
            }
        }
    }
    for (render_id, old) in &previous.boundaries {
        if !current.boundaries.contains_key(render_id) {
            add(old.region);
        }
    }
    for (old, new) in moved_in_paint_order(previous, current) {
        add(old.region);
        add(new.region);
    }
    add(previous.volatile);
    add(current.volatile);

    // A backdrop filter reads what is behind it: damage that reaches its
    // input changes its whole output. A foreground image filter re-renders
    // its children under the damage scissor and composites the result
    // unscissored: damage that meets its footprint must take all of it, or
    // the filter reads a truncated input and spreads it past the damage.
    // Joining one filter can reach another, so repeat until a pass joins
    // nothing.
    if let Some(mut region) = damage {
        loop {
            let mut grew = false;
            for &footprint in &current.foreground {
                if region.overlaps(&footprint) && !region.contains_rect(&footprint) {
                    region = region.union(&footprint);
                    grew = true;
                }
            }
            for &(bounds, reach) in &current.backdrops {
                if region.overlaps(&bounds.expand(reach)) && !region.contains_rect(&bounds) {
                    region = region.union(&bounds);
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        damage = Some(region);
    }

    let Some(rect) = damage.and_then(|rect| DamageRect::covering(rect, current.surface)) else {
        return DamageRegion::Unchanged;
    };
    let surface_area = f64::from(current.surface.0) * f64::from(current.surface.1);
    if rect.area() as f64 > full_above * surface_area {
        DamageRegion::Full
    } else {
        DamageRegion::Partial(rect)
    }
}

/// The kept boundaries whose paint order changed relative to the others, as
/// `(previous, current)` records.
///
/// A boundary keeps its token and placement when a parent only reorders its
/// children, and the parent's own region excludes theirs, so without this two
/// overlapping siblings that swap places would compare unchanged and leave
/// the old topmost content on screen. The kept boundaries in current paint
/// order, keyed by their previous order, keep a longest increasing
/// subsequence in place; every boundary outside it moved. Any pair whose
/// order flipped has at least one member outside it, and damaging that
/// member's regions repaints every pixel the two share. Boundaries added or
/// removed shift no one: only relative order among kept ones counts.
fn moved_in_paint_order<'a>(
    previous: &'a Frame,
    current: &'a Frame,
) -> Vec<(&'a Record, &'a Record)> {
    let mut kept: Vec<(&Record, &Record)> = current
        .boundaries
        .iter()
        .filter_map(|(render_id, new)| Some((previous.boundaries.get(render_id)?, new)))
        .collect();
    kept.sort_unstable_by_key(|(_, new)| new.order);

    // Patience sort: `tails[k]` is the index into `kept` ending the best
    // increasing run of length `k + 1`; `links` walks each run back.
    let mut tails: Vec<usize> = Vec::new();
    let mut links: Vec<Option<usize>> = vec![None; kept.len()];
    for (index, (old, _)) in kept.iter().enumerate() {
        let length = tails.partition_point(|&tail| kept[tail].0.order < old.order);
        links[index] = length.checked_sub(1).map(|shorter| tails[shorter]);
        if length == tails.len() {
            tails.push(index);
        } else {
            tails[length] = index;
        }
    }
    let mut in_place = vec![false; kept.len()];
    let mut cursor = tails.last().copied();
    while let Some(index) = cursor {
        in_place[index] = true;
        cursor = links[index];
    }
    kept.into_iter()
        .zip(in_place)
        .filter_map(|(pair, stays)| (!stays).then_some(pair))
        .collect()
}

/// Whether a boundary sits under the same transform and the same effects in
/// both frames.
fn same_placement(previous: &Frame, old: &Record, current: &Frame, new: &Record) -> bool {
    old.transform.m == new.transform.m
        && old.effects.len() == new.effects.len()
        && old
            .effects
            .iter()
            .zip(&new.effects)
            .all(|(&a, &b)| previous.effects[a].same_effect(&current.effects[b]))
}

/// The effect chain ending at `head`, root first.
fn materialize(chain: &[(Option<usize>, usize)], head: Option<usize>) -> Vec<usize> {
    let mut effects: Vec<usize> = std::iter::successors(head, |&link| chain[link].0)
        .map(|link| chain[link].1)
        .collect();
    effects.reverse();
    effects
}

/// `extent` in surface pixels under `ctx`, clipped to the surface and, when
/// `clipped`, to the clips above it; `None` when nothing of it is on the
/// surface.
fn place(extent: DamageExtent, ctx: &Ctx, full: Rect<f64>, clipped: bool) -> Option<Rect<f64>> {
    let clip = if clipped { ctx.clip } else { None };
    let rect = match (ctx.reach, extent) {
        (Reach::Within(bound), _) if clipped => bound.unwrap_or(full),
        (Reach::Within(_), _) => full,
        (Reach::Exact, DamageExtent::Unbounded) => clip.unwrap_or(full),
        (Reach::Exact, DamageExtent::Bounded { .. }) => {
            // The walk marked a projective transform `Within` above, so the
            // mapped extent is bounded here.
            let mapped = extent
                .transformed(&ctx.transform)
                .covering_rect()
                .unwrap_or(full);
            // A non-finite transform maps to nothing measurable; the whole
            // surface is the only answer that cannot fall short.
            if mapped.is_finite() {
                match clip {
                    Some(clip) => clip.intersect(&mapped)?,
                    None => mapped,
                }
            } else {
                full
            }
        }
    };
    let rect = rect.intersect(&full)?;
    (rect.width() > 0.0 && rect.height() > 0.0).then_some(rect)
}

fn join(acc: Option<Rect<f64>>, rect: Rect<f64>) -> Rect<f64> {
    acc.map_or(rect, |acc| acc.union(&rect))
}

/// The layers whose parameters change the pixels of the subtree under them:
/// clips and effects. Pure translations (`Offset`, `Transform`, `Leader`,
/// `Follower`) are carried by the transform instead, and `AnnotatedRegion`
/// changes no pixel.
fn is_effect(layer: &Layer) -> bool {
    matches!(
        layer,
        Layer::ClipRect(_)
            | Layer::ClipRRect(_)
            | Layer::ClipPath(_)
            | Layer::ClipSuperellipse(_)
            | Layer::Opacity(_)
            | Layer::ColorFilter(_)
            | Layer::ImageFilter(_)
            | Layer::ShaderMask(_)
            | Layer::BackdropFilter(_)
    )
}

/// The local rect a clip layer confines its subtree to, if it clips.
fn clip_bounds(layer: &Layer) -> Option<Rect<f64>> {
    match layer {
        Layer::ClipRect(clip) if clip.clips() => Some(clip.bounds()),
        Layer::ClipRRect(clip) if clip.clips() => Some(clip.bounds()),
        Layer::ClipPath(clip) if clip.clips() => Some(clip.bounds()),
        Layer::ClipSuperellipse(clip) if clip.clips() => Some(clip.bounds()),
        _ => None,
    }
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
fn is_projective(transform: &Matrix4) -> bool {
    let m = &transform.m;
    m[3].abs() > f64::EPSILON || m[7].abs() > f64::EPSILON || (m[15] - 1.0).abs() > f64::EPSILON
}
