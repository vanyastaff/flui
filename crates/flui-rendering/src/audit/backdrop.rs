//! The walk the contrast rule reads a frame with: every text the scene paints, in paint order,
//! with the colour painted behind it, or why that colour is not known.
//!
//! The scene is walked as the raster side draws it (`flui-engine`'s `walk_scene`): a node at its
//! placement, each effect entry's layer over the entries under it, each chunk's commands in
//! order, each slot's child node where it stands. Geometry is taken to the window's logical
//! pixels. Each paint is kept as the region it may touch and, for a solid fill, the region it
//! surely covers; a text's backdrop is read back from the paints before it (module docs of
//! [`super`]).
//!
//! Regions are under-approximated where they must cover and over-approximated where they may
//! touch: a rounded rectangle surely covers its two bands clear of the corners, a clip by a path
//! or a superellipse surely leaves nothing, and a paint whose extent the walk does not compute
//! may touch everything its clip leaves. So an unknown shape can make a text not checked, never
//! judged against the wrong colour.

use flui_foundation::RenderId;
use flui_foundation::geometry::{Matrix4, Point, RRect, Rect};
use flui_layer::{Chunk, Entry, EntryAddress, EntryIndex, Layer, SceneNodeId, SceneVersion};
use flui_painting::paint::{BlendMode, ClipOp, Paint, PaintStyle};
use flui_painting::styling::Color;
use flui_painting::{DrawOp, ShapedParagraph};
use smallvec::SmallVec;

use super::ContrastUnknown;

/// Rectangles closer than this, in logical pixels, are the same edge: a fill that covers a
/// text up to rounding still covers it, and two that only touch do not overlap.
const EDGE_EPSILON: f64 = 1e-6;

/// The most rectangles a clip keeps as the region it surely leaves. Nested rounded clips
/// multiply the bands; past this the smallest are dropped, which only shrinks what is surely
/// covered.
const MAX_SURE_RECTS: usize = 8;

/// How far past its path a shadow may darken, per unit of elevation, in logical pixels: a
/// generous bound on the blur a shadow of that elevation spreads.
const SHADOW_SPREAD_PER_ELEVATION: f64 = 4.0;

/// How far past its glyphs' box a text shadow may darken, per unit of blur sigma: three sigma
/// hold all but a negligible part of a Gaussian.
const SHADOW_SPREAD_PER_SIGMA: f64 = 3.0;

/// Whether a paragraph's spans are bold for the large-text threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Boldness {
    /// Every span was shaped at weight 700 or more.
    Bold,
    /// No span was.
    Regular,
    /// Some were and some were not.
    Mixed,
}

/// One glyph run of a painted text.
#[derive(Debug, Clone, Copy)]
pub(super) struct RunFacts {
    /// The colour its glyphs paint in.
    pub(super) color: Color,
    /// Its font size in logical pixels, with the scale it is painted under.
    pub(super) font_size: f64,
}

/// One text the frame paints, and what is painted behind it.
#[derive(Debug, Clone)]
pub(super) struct PaintedText {
    pub(super) painter: RenderId,
    pub(super) text: String,
    pub(super) runs: Vec<RunFacts>,
    pub(super) bold: Boldness,
    /// The opaque colour behind the text, or why it is not known.
    pub(super) backdrop: Result<Color, ContrastUnknown>,
}

/// Every text the frame `frame` of `version` paints, in paint order, its geometry divided by
/// `device_pixel_ratio` to logical pixels. `None` when `version` holds no node `frame`.
pub(super) fn painted_texts(
    version: &SceneVersion,
    frame: SceneNodeId,
    device_pixel_ratio: f64,
) -> Option<Vec<PaintedText>> {
    version.node(frame)?;
    let to_logical = device_pixel_ratio.recip();
    let root = Scope {
        transform: Matrix4::scaling(to_logical, to_logical, 1.0),
        clip: Clip::everywhere(),
        effect: Effect::Resolved,
    };
    let mut walk = Walk {
        version,
        paints: Vec::new(),
        texts: Vec::new(),
    };
    walk.run(frame, root);
    Some(walk.texts)
}

/// What paint under a scope reaches the screen as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Effect {
    /// As recorded.
    Resolved,
    /// Changed by a layer the walk does not resolve (opacity, a colour filter, a shader mask),
    /// inside the extent it was recorded with.
    Unresolved,
    /// Changed and spread past its recorded extent (an image filter's blur).
    Spread,
}

impl Effect {
    fn unresolved(self) -> Self {
        match self {
            Self::Resolved | Self::Unresolved => Self::Unresolved,
            Self::Spread => Self::Spread,
        }
    }
}

/// What a clip surely leaves, or a solid fill surely covers, whole.
#[derive(Debug, Clone)]
enum Cover {
    /// The whole plane.
    Plane,
    /// The union of these rectangles; none for nothing.
    Rects(SmallVec<[Rect<f64>; 4]>),
}

impl Cover {
    /// This cover inside the union `rects`.
    fn within(&self, rects: &[Rect<f64>]) -> Self {
        match self {
            Self::Plane => Self::Rects(rects.iter().copied().collect()),
            Self::Rects(own) => Self::Rects(intersect_unions(own, rects)),
        }
    }

    /// Whether some part of the cover holds the whole of `rect`.
    fn holds(&self, rect: &Rect<f64>) -> bool {
        match self {
            Self::Plane => true,
            Self::Rects(rects) => rects
                .iter()
                .any(|own| own.inflate(EDGE_EPSILON, EDGE_EPSILON).contains_rect(rect)),
        }
    }
}

/// The region a clip leaves: where paint may show, and where it surely shows.
#[derive(Debug, Clone)]
struct Clip {
    /// The bounding box of what the clip leaves; `None` for no clip.
    may: Option<Rect<f64>>,
    /// What the clip surely leaves whole.
    sure: Cover,
}

impl Clip {
    fn everywhere() -> Self {
        Self {
            may: None,
            sure: Cover::Plane,
        }
    }

    /// The clip narrowed by `region` with `op`.
    fn narrowed(&self, region: &Region, op: ClipOp) -> Self {
        match op {
            ClipOp::Intersect => Self {
                may: Some(match self.may {
                    Some(may) => overlap(&may, &region.may),
                    None => region.may,
                }),
                sure: self.sure.within(&region.sure),
            },
            // What a difference leaves is not a rectangle: nothing of it is surely left.
            ClipOp::Difference => Self {
                may: self.may,
                sure: Cover::Rects(SmallVec::new()),
            },
        }
    }

    /// `rect` inside what the clip may leave; `None` when nothing of it shows.
    fn shown(&self, rect: Rect<f64>) -> Option<Rect<f64>> {
        let shown = match self.may {
            Some(may) => overlap(&may, &rect),
            None => rect,
        };
        (!shown.is_empty()).then_some(shown)
    }
}

/// A shape's extent: the box it may touch and the rectangles it surely covers.
#[derive(Debug, Clone)]
struct Region {
    may: Rect<f64>,
    sure: SmallVec<[Rect<f64>; 2]>,
}

impl Region {
    /// `rect` under `transform`: exact when the transform keeps rectangles rectangles.
    fn of_rect(transform: &Matrix4, rect: Rect<f64>) -> Self {
        let may = transform.transform_rect(&rect);
        let mut sure = SmallVec::new();
        if keeps_rects(transform) {
            sure.push(may);
        }
        Self { may, sure }
    }

    /// `rrect` under `transform`: the box may be touched, and the two bands clear of the
    /// corners are surely covered.
    fn of_rrect(transform: &Matrix4, rrect: &RRect) -> Self {
        let rect = rrect.rect;
        let may = transform.transform_rect(&rect);
        let mut sure = SmallVec::new();
        if keeps_rects(transform) {
            let left = rrect.top_left.x.max(rrect.bottom_left.x);
            let right = rrect.top_right.x.max(rrect.bottom_right.x);
            let top = rrect.top_left.y.max(rrect.top_right.y);
            let bottom = rrect.bottom_left.y.max(rrect.bottom_right.y);
            for band in [
                Rect::from_ltrb(
                    rect.left() + left,
                    rect.top(),
                    rect.right() - right,
                    rect.bottom(),
                ),
                Rect::from_ltrb(
                    rect.left(),
                    rect.top() + top,
                    rect.right(),
                    rect.bottom() - bottom,
                ),
            ] {
                if !band.is_empty() {
                    sure.push(transform.transform_rect(&band));
                }
            }
        }
        Self { may, sure }
    }

    /// A shape known only by its bounding box `rect`: nothing of it is surely covered.
    fn of_bounds(transform: &Matrix4, rect: Rect<f64>) -> Self {
        Self {
            may: transform.transform_rect(&rect),
            sure: SmallVec::new(),
        }
    }
}

/// The transform, clip and effect paint at one point of the walk is under.
#[derive(Debug, Clone)]
struct Scope {
    /// From the space paint is recorded in to the window's logical pixels.
    transform: Matrix4,
    clip: Clip,
    effect: Effect,
}

/// What one paint did, as the backdrop read sees it.
#[derive(Debug)]
struct Painted {
    /// What it may touch; `None` for anywhere.
    may: Option<Rect<f64>>,
    /// A solid colour over what it surely covers; `None` for any other paint.
    solid: Option<(Color, Cover)>,
}

impl Painted {
    fn touches(&self, rect: &Rect<f64>) -> bool {
        self.may
            .is_none_or(|may| may.overlaps(&rect.inflate(-EDGE_EPSILON, -EDGE_EPSILON)))
    }
}

/// An effect entry the walk opened, and the scope it opened over.
struct Open {
    entry: EntryIndex,
    before: Scope,
    /// Whether its subtree draws: a follower with no leader draws nothing.
    entered: bool,
}

/// One frame of the walk's own stack.
enum Visit {
    /// A node, entered at its placement under the scope where its slot stands.
    Node(SceneNodeId, Scope),
    /// The rest of a node's entries from `next`, under the effects `open`.
    Entries {
        node: SceneNodeId,
        next: usize,
        open: Vec<Open>,
        scope: Scope,
    },
}

struct Walk<'v> {
    version: &'v SceneVersion,
    /// Every paint so far, in paint order.
    paints: Vec<Painted>,
    texts: Vec<PaintedText>,
}

impl Walk<'_> {
    /// Walks the node `frame` and everything under it. A version's slots form a tree
    /// (`SceneOwner::apply` refuses a cycle and a second parent), so each node is entered once,
    /// and the walk keeps its own stack, so a deep scene does not grow the thread's.
    fn run(&mut self, frame: SceneNodeId, root: Scope) {
        let mut stack = vec![Visit::Node(frame, root)];
        while let Some(visit) = stack.pop() {
            match visit {
                Visit::Node(id, mut scope) => {
                    let Some(node) = self.version.node(id) else {
                        continue;
                    };
                    let offset = node.placement().offset();
                    scope.transform *= Matrix4::translation(offset.dx, offset.dy, 0.0);
                    stack.push(Visit::Entries {
                        node: id,
                        next: 0,
                        open: Vec::new(),
                        scope,
                    });
                }
                Visit::Entries {
                    node: id,
                    next,
                    mut open,
                    mut scope,
                } => {
                    let Some(node) = self.version.node(id) else {
                        continue;
                    };
                    let content = node.content();
                    let entries = content.entries();
                    let mut position = next;
                    while let Some(entry) = entries.get(position) {
                        let index = EntryIndex::try_from(position)
                            .expect("BUG: a boundary's entries are indexed by u32");
                        position += 1;
                        // An effect's scope is contiguous: an entry whose parent is not the
                        // innermost open effect closes the effects above its parent.
                        while let Some(top) = open.last()
                            && Some(top.entry) != entry.parent()
                        {
                            let closed = open.pop().expect("BUG: the loop saw a top");
                            scope = closed.before;
                        }
                        let skipping = open.last().is_some_and(|top| !top.entered);
                        match entry {
                            Entry::Effect { .. } => {
                                let before = scope.clone();
                                let entered = !skipping
                                    && match content.effect_layer(index) {
                                        Some(layer) => {
                                            let at = EntryAddress {
                                                node: id,
                                                entry: index,
                                            };
                                            match self.enter(layer, at, &scope) {
                                                Some(inner) => {
                                                    scope = inner;
                                                    true
                                                }
                                                None => false,
                                            }
                                        }
                                        None => false,
                                    };
                                open.push(Open {
                                    entry: index,
                                    before,
                                    entered,
                                });
                            }
                            Entry::Chunk { chunk, .. } if !skipping => self.chunk(chunk, &scope),
                            Entry::Slot { node: child, .. } if !skipping => {
                                let child = *child;
                                let at_slot = scope.clone();
                                stack.push(Visit::Entries {
                                    node: id,
                                    next: position,
                                    open,
                                    scope,
                                });
                                stack.push(Visit::Node(child, at_slot));
                                break;
                            }
                            Entry::Chunk { .. } | Entry::Slot { .. } => {}
                        }
                    }
                }
            }
        }
    }

    /// The scope inside the effect `layer` at `at`, opened over `scope`; `None` when nothing
    /// under it draws.
    fn enter(&mut self, layer: &Layer, at: EntryAddress, scope: &Scope) -> Option<Scope> {
        let mut inner = scope.clone();
        if let Layer::Transform(transform) = layer {
            inner.transform *= *transform.transform();
        } else {
            let offset = layer.local_translation();
            if !offset.is_zero() {
                inner.transform *= Matrix4::translation(offset.dx, offset.dy, 0.0);
            }
        }
        match layer {
            Layer::Offset(_)
            | Layer::Transform(_)
            | Layer::Leader(_)
            | Layer::AnnotatedRegion(_) => {}
            Layer::ClipRect(clip) => {
                if clip.clips() {
                    let region = Region::of_rect(&scope.transform, clip.clip_rect());
                    inner.clip = scope.clip.narrowed(&region, ClipOp::Intersect);
                }
            }
            Layer::ClipRRect(clip) => {
                if clip.clips() {
                    let region = Region::of_rrect(&scope.transform, clip.clip_rrect());
                    inner.clip = scope.clip.narrowed(&region, ClipOp::Intersect);
                }
            }
            Layer::ClipPath(clip) => {
                if clip.clips() {
                    let region = Region::of_bounds(&scope.transform, clip.bounds());
                    inner.clip = scope.clip.narrowed(&region, ClipOp::Intersect);
                }
            }
            Layer::ClipSuperellipse(clip) => {
                if clip.clips() {
                    let region = Region::of_bounds(&scope.transform, clip.bounds());
                    inner.clip = scope.clip.narrowed(&region, ClipOp::Intersect);
                }
            }
            Layer::Opacity(opacity) => {
                let identity = opacity.alpha() >= 1.0 && opacity.blend() == BlendMode::SrcOver;
                if !identity {
                    inner.effect = inner.effect.unresolved();
                }
            }
            Layer::ColorFilter(_) | Layer::ShaderMask(_) => {
                inner.effect = inner.effect.unresolved();
            }
            Layer::ImageFilter(_) | Layer::Picture(_) => inner.effect = Effect::Spread,
            Layer::BackdropFilter(filter) => {
                // It filters what is already behind its bounds; what it holds paints as recorded.
                let region = Region::of_bounds(&scope.transform, filter.bounds());
                self.paint_other(Some(region.may), scope);
            }
            Layer::Follower(_) => {
                let transform = self.version.follower_transform(at)?;
                inner.transform = scope.transform * transform;
            }
        }
        Some(inner)
    }

    /// The commands of `chunk`, under `scope`.
    #[expect(
        clippy::too_many_lines,
        reason = "one arm per draw op, naming every op so a new one is decided here"
    )]
    fn chunk(&mut self, chunk: &Chunk, scope: &Scope) {
        let mut current = scope.clone();
        // A save inside the list is closed by its restore; a list that leaves one open ends
        // with it, as the replay does.
        let mut saved: Vec<Scope> = Vec::new();
        for command in chunk.commands() {
            let transform = scope.transform * *command.transform;
            let stroke = |paint: &Paint| match paint.style {
                PaintStyle::Stroke => paint.stroke_width.abs(),
                PaintStyle::Fill => 0.0,
            };
            match command.op {
                DrawOp::Save => saved.push(current.clone()),
                DrawOp::SaveLayer { paint, .. } => {
                    saved.push(current.clone());
                    if solid(paint).is_none_or(|color| !color.is_opaque()) {
                        current.effect = current.effect.unresolved();
                    }
                }
                DrawOp::Restore | DrawOp::RestoreLayer => {
                    if let Some(outer) = saved.pop() {
                        current = outer;
                    }
                }
                DrawOp::ClipRect { rect, clip_op, .. } => {
                    let region = Region::of_rect(&transform, *rect);
                    current.clip = current.clip.narrowed(&region, *clip_op);
                }
                DrawOp::ClipRRect { rrect, clip_op, .. } => {
                    let region = Region::of_rrect(&transform, rrect);
                    current.clip = current.clip.narrowed(&region, *clip_op);
                }
                DrawOp::ClipRSuperellipse {
                    rsuperellipse,
                    clip_op,
                    ..
                } => {
                    let region = Region::of_bounds(&transform, rsuperellipse.outer_rect());
                    current.clip = current.clip.narrowed(&region, *clip_op);
                }
                DrawOp::ClipPath { path, clip_op, .. } => {
                    let region =
                        Region::of_bounds(&transform, command.payload(*path).compute_bounds());
                    current.clip = current.clip.narrowed(&region, *clip_op);
                }
                DrawOp::Paragraph {
                    paragraph,
                    offset,
                    color,
                } => self.text(
                    chunk.owner(),
                    paragraph,
                    *offset,
                    *color,
                    &transform,
                    &current,
                ),
                DrawOp::ParagraphShadow {
                    paragraph,
                    offset,
                    sigma,
                    ..
                } => {
                    let spread = SHADOW_SPREAD_PER_SIGMA * f64::from(sigma.get());
                    let local = text_box(paragraph, *offset).inflate(spread, spread);
                    self.paint_other(Some(transform.transform_rect(&local)), &current);
                }
                DrawOp::Rect { rect, paint } => {
                    self.fill(
                        Region::of_rect(&transform, *rect),
                        paint,
                        stroke(paint),
                        &current,
                    );
                }
                DrawOp::RRect { rrect, paint } => {
                    self.fill(
                        Region::of_rrect(&transform, rrect),
                        paint,
                        stroke(paint),
                        &current,
                    );
                }
                DrawOp::Color { color, blend_mode } => {
                    let solid = (*blend_mode == BlendMode::SrcOver).then_some(*color);
                    self.flood(solid, &current);
                }
                DrawOp::Paint { paint } => self.flood(solid(paint), &current),
                DrawOp::Circle {
                    center,
                    radius,
                    paint,
                } => {
                    let reach = radius.abs() + stroke(paint);
                    let local = Rect::from_ltrb(
                        center.x - reach,
                        center.y - reach,
                        center.x + reach,
                        center.y + reach,
                    );
                    self.paint_other(Some(transform.transform_rect(&local)), &current);
                }
                DrawOp::Oval { rect, paint } | DrawOp::Arc { rect, paint, .. } => {
                    let reach = stroke(paint);
                    self.paint_other(
                        Some(transform.transform_rect(&rect.inflate(reach, reach))),
                        &current,
                    );
                }
                DrawOp::Line { p1, p2, paint } => {
                    let reach = stroke(paint).max(paint.stroke_width.abs());
                    let local = Rect::from_points(*p1, *p2).inflate(reach, reach);
                    self.paint_other(Some(transform.transform_rect(&local)), &current);
                }
                DrawOp::Path { path, paint } => {
                    let reach = stroke(paint);
                    let local = command
                        .payload(*path)
                        .compute_bounds()
                        .inflate(reach, reach);
                    self.paint_other(Some(transform.transform_rect(&local)), &current);
                }
                DrawOp::DRRect { rrects, paint } => {
                    let reach = stroke(paint);
                    let local = command.payload(*rrects).outer.rect.inflate(reach, reach);
                    self.paint_other(Some(transform.transform_rect(&local)), &current);
                }
                DrawOp::Shadow {
                    path, elevation, ..
                } => {
                    let spread = SHADOW_SPREAD_PER_ELEVATION * elevation.abs();
                    let local = command
                        .payload(*path)
                        .compute_bounds()
                        .inflate(spread, spread);
                    self.paint_other(Some(transform.transform_rect(&local)), &current);
                }
                DrawOp::Image { dst, .. }
                | DrawOp::ImageRepeat { dst, .. }
                | DrawOp::ImageNineSlice { dst, .. }
                | DrawOp::ImageFiltered { dst, .. }
                | DrawOp::Texture { dst, .. } => {
                    self.paint_other(Some(transform.transform_rect(dst)), &current);
                }
                DrawOp::Points { points, paint, .. } => {
                    let reach = paint.stroke_width.abs();
                    let local = points
                        .iter()
                        .map(|point| Rect::from_points(*point, *point))
                        .reduce(|all, point| all.union(&point))
                        .map(|all| all.inflate(reach, reach));
                    if let Some(local) = local {
                        self.paint_other(Some(transform.transform_rect(&local)), &current);
                    }
                }
                DrawOp::Vertices { vertices, .. } => {
                    let local = vertices
                        .iter()
                        .map(|point| Rect::from_points(*point, *point))
                        .reduce(|all, point| all.union(&point));
                    if let Some(local) = local {
                        self.paint_other(Some(transform.transform_rect(&local)), &current);
                    }
                }
                // Its sprites land where their own transforms put them: anywhere its clip leaves.
                DrawOp::ImageRegion { .. } | DrawOp::Atlas { .. } => {
                    self.paint_other(None, &current);
                }
            }
        }
    }

    /// A fill of `region` with `paint`, stroked `stroke` wide (0 for a fill).
    fn fill(&mut self, region: Region, paint: &Paint, stroke: f64, current: &Scope) {
        let color = solid(paint).filter(|_| stroke == 0.0);
        if color.is_some_and(|color| color.is_transparent()) {
            return;
        }
        let may = if stroke > 0.0 {
            region.may.inflate(stroke, stroke)
        } else {
            region.may
        };
        let solid = color
            .filter(|_| current.effect == Effect::Resolved)
            .map(|color| (color, current.clip.sure.within(&region.sure)));
        self.push(Some(may), solid, current);
    }

    /// A paint over everything its clip leaves, solid in `color` when it is one.
    fn flood(&mut self, color: Option<Color>, current: &Scope) {
        if color.is_some_and(|color| color.is_transparent()) {
            return;
        }
        let solid = color
            .filter(|_| current.effect == Effect::Resolved)
            .map(|color| (color, current.clip.sure.clone()));
        self.push(None, solid, current);
    }

    /// A paint that does not resolve to one colour, touching `may` (`None`: anywhere).
    fn paint_other(&mut self, may: Option<Rect<f64>>, current: &Scope) {
        self.push(may, None, current);
    }

    /// Keeps a paint touching `may` (`None`: anywhere) under `current`: inside its clip, or
    /// anywhere its clip leaves under a spreading effect or when its extent is not finite. A
    /// paint nothing of which shows is not kept.
    fn push(&mut self, may: Option<Rect<f64>>, solid: Option<(Color, Cover)>, current: &Scope) {
        let may = match may {
            Some(may) if may.is_finite() && current.effect != Effect::Spread => Some(may),
            _ => current.clip.may,
        };
        let may = match may {
            Some(may) => match current.clip.shown(may) {
                Some(shown) => Some(shown),
                None => return,
            },
            None => None,
        };
        self.paints.push(Painted { may, solid });
    }

    /// A paragraph `painter` painted at `offset` in `color`, under `transform` and `current`.
    fn text(
        &mut self,
        painter: RenderId,
        paragraph: &ShapedParagraph,
        offset: flui_foundation::geometry::Offset<f64>,
        color: Color,
        transform: &Matrix4,
        current: &Scope,
    ) {
        let bounds = transform.transform_rect(&text_box(paragraph, offset));
        if !bounds.is_finite() {
            self.paint_other(None, current);
            return;
        }
        let Some(shown) = current.clip.shown(bounds) else {
            return;
        };
        let scale = axis_scale(transform);
        let runs: Vec<RunFacts> = paragraph
            .runs()
            .filter(|run| !run.glyphs().is_empty())
            .map(|run| RunFacts {
                color: run.color().unwrap_or(color),
                font_size: f64::from(run.font_size()) * scale,
            })
            .collect();
        if !runs.is_empty() && !paragraph.text().trim().is_empty() {
            let backdrop = if current.effect != Effect::Resolved || !keeps_rects(transform) {
                Err(ContrastUnknown::UnresolvedEffect)
            } else {
                self.backdrop_of(&shown)
            };
            self.texts.push(PaintedText {
                painter,
                text: paragraph.text().to_owned(),
                runs,
                bold: boldness(paragraph.span_weights()),
                backdrop,
            });
        }
        self.paints.push(Painted {
            may: Some(shown),
            solid: None,
        });
    }

    /// The opaque colour behind `rect`, from the paints so far: the last solid fill that covers
    /// it whole, with the translucent ones above it composited on.
    fn backdrop_of(&self, rect: &Rect<f64>) -> Result<Color, ContrastUnknown> {
        let mut over: Vec<Color> = Vec::new();
        for paint in self.paints.iter().rev() {
            if !paint.touches(rect) {
                continue;
            }
            match &paint.solid {
                Some((color, sure)) if sure.holds(rect) => {
                    if color.is_opaque() {
                        return Ok(over
                            .iter()
                            .rev()
                            .fold(*color, |below, above| above.blend_over(below)));
                    }
                    over.push(*color);
                }
                _ => return Err(ContrastUnknown::UnresolvedBackdrop),
            }
        }
        Err(ContrastUnknown::NothingOpaqueBehind)
    }
}

/// The colour `paint` fills with when it is one solid colour composited source-over.
fn solid(paint: &Paint) -> Option<Color> {
    (paint.style == PaintStyle::Fill
        && paint.shader.is_none()
        && paint.blend_mode == BlendMode::SrcOver
        && paint.dash_pattern.is_none())
    .then_some(paint.color)
}

/// The box a paragraph at `offset` sets its glyphs in: its laid-out line boxes. Its ink bounds
/// are a damage extent, every pixel any glyph of its faces could reach, and run far past the
/// words, so a fill behind the text would never be seen to cover them.
fn text_box(
    paragraph: &ShapedParagraph,
    offset: flui_foundation::geometry::Offset<f64>,
) -> Rect<f64> {
    Rect::from_origin_size(Point::new(offset.dx, offset.dy), paragraph.size())
}

/// Whether `transform` maps every axis-aligned rectangle onto one: a scale and translation,
/// with or without a quarter turn or a flip, in the plane.
fn keeps_rects(transform: &Matrix4) -> bool {
    let m = &transform.m;
    let affine = m[3] == 0.0 && m[7] == 0.0 && m[15] == 1.0;
    affine && ((m[1] == 0.0 && m[4] == 0.0) || (m[0] == 0.0 && m[5] == 0.0))
}

/// How much `transform` scales a glyph: the smaller of its two axis scales, so a squashed text
/// counts at its smaller size.
fn axis_scale(transform: &Matrix4) -> f64 {
    let m = &transform.m;
    let x = m[0].hypot(m[1]);
    let y = m[4].hypot(m[5]);
    x.min(y)
}

/// The overlap of `a` and `b`, empty when they do not overlap.
fn overlap(a: &Rect<f64>, b: &Rect<f64>) -> Rect<f64> {
    a.intersect(b)
        .unwrap_or_else(|| Rect::from_ltrb(0.0, 0.0, 0.0, 0.0))
}

/// The rectangles of `a` ∩ `b`, two unions: each pair's overlap, the largest kept.
fn intersect_unions(a: &[Rect<f64>], b: &[Rect<f64>]) -> SmallVec<[Rect<f64>; 4]> {
    let mut out: SmallVec<[Rect<f64>; 4]> = a
        .iter()
        .flat_map(|a| b.iter().map(move |b| overlap(a, b)))
        .filter(|rect| !rect.is_empty())
        .collect();
    if out.len() > MAX_SURE_RECTS {
        out.sort_by(|a, b| b.area().total_cmp(&a.area()));
        out.truncate(MAX_SURE_RECTS);
    }
    out
}

/// Whether a shaped paragraph's spans are bold, from the weights they were shaped with.
fn boldness(weights: impl Iterator<Item = f32>) -> Boldness {
    let (mut bold, mut regular) = (false, false);
    for weight in weights {
        if weight >= 700.0 {
            bold = true;
        } else {
            regular = true;
        }
    }
    match (bold, regular) {
        (true, false) => Boldness::Bold,
        (true, true) => Boldness::Mixed,
        (false, _) => Boldness::Regular,
    }
}
