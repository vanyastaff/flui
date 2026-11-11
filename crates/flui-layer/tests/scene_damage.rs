//! A patch's damage is the footprint of its operations under the version
//! before and after it, closed over what filters read, and it covers what a
//! full diff of the two versions finds (ADR-0304 §4, §6, §11).

use std::collections::HashMap;
use std::sync::Arc;

use flui_foundation::geometry::{Matrix4, Offset, Rect, Size};
use flui_foundation::{LayerId, RenderId, SurfaceGeneration};
use flui_layer::{
    BackdropFilterLayer, BoundaryContent, Chunk, ClipRectLayer, Closure, ContentToken,
    ContentVersion, DamageMode, DamageRect, DamageRegion, Entry, FollowerLayer, Layer, LayerDiffer,
    LayerLink, LayerNode, LayerTree, LeaderLayer, OffsetLayer, OpacityLayer, PictureLayer,
    Placement, Region, Scene, SceneNodeId, SceneOp, SceneOwner, SceneVersion, SurfaceKey,
    TransformLayer, close_over, read_closure, resolve_follower_transform,
};
use flui_painting::paint::{BlendMode, ImageFilter};
use flui_painting::styling::Color;
use flui_painting::{Canvas, Paint};

const SURFACE: SurfaceKey = SurfaceKey {
    size: (800, 600),
    generation: SurfaceGeneration::ZERO,
};

fn paint(rect: Rect<f64>) -> flui_painting::DisplayList {
    let mut canvas = Canvas::new();
    canvas.draw_rect(rect, &Paint::fill(Color::RED));
    canvas.finish()
}

fn chunk(owner: u32, version: ContentVersion, rect: Rect<f64>) -> Arc<Chunk> {
    Arc::new(Chunk::new(
        RenderId::new(owner as usize),
        0,
        version,
        paint(rect),
    ))
}

fn place(node: SceneNodeId, x: f64, y: f64) -> SceneOp {
    SceneOp::Place {
        node,
        placement: Placement::try_new(Offset::new(x, y)).expect("a finite offset"),
    }
}

fn publish(node: SceneNodeId, content: BoundaryContent) -> SceneOp {
    SceneOp::Publish {
        node,
        content: Arc::new(content),
    }
}

fn covering(l: f64, t: f64, r: f64, b: f64) -> DamageRect {
    DamageRect::try_covering(Rect::from_ltrb(l, t, r, b), SURFACE.size)
        .expect("finite")
        .expect("on the surface")
}

fn rects(damage: DamageRegion) -> Vec<DamageRect> {
    match damage {
        DamageRegion::Region(region) => region.rects().to_vec(),
        other => panic!("expected a region, got {other:?}"),
    }
}

/// Whether one rect of `damage` covers `rect`.
fn covered(damage: &[DamageRect], rect: DamageRect) -> bool {
    damage.iter().any(|outer| {
        outer.left() <= rect.left()
            && outer.top() <= rect.top()
            && outer.right() >= rect.right()
            && outer.bottom() >= rect.bottom()
    })
}

// scene_patch_damage

/// Moving a boundary damages its subtree where it was and where it is, and
/// nothing between.
pub(crate) fn a_place_damages_the_subtree_under_both_placements() {
    let mut owner = SceneOwner::new();
    let mut writer = owner.writer();
    let root = writer.current().root();
    let (a, b) = (writer.mint_node(), writer.mint_node());
    let version = writer.next_content_version().expect("a version");
    let mut leaf = BoundaryContent::builder();
    leaf.chunk(chunk(1, version, Rect::from_xywh(0.0, 0.0, 40.0, 20.0)));
    writer.push(publish(b, leaf.finish()));
    let mut middle = BoundaryContent::builder();
    middle.slot(b);
    writer.push(publish(a, middle.finish()));
    let mut top = BoundaryContent::builder();
    top.slot(a);
    writer.push(publish(root, top.finish()));
    writer.push(place(a, 100.0, 100.0));
    writer.push(place(b, 10.0, 0.0));
    owner.apply(SURFACE).expect("root -> a -> b");

    let mut writer = owner.writer();
    writer.push(place(a, 400.0, 300.0));
    owner.apply(SURFACE).expect("a move");
    let damage = rects(owner.patch_damage());
    assert_eq!(damage.len(), 2, "{damage:?}");
    assert!(covered(&damage, covering(110.0, 100.0, 150.0, 120.0)));
    assert!(covered(&damage, covering(410.0, 300.0, 450.0, 320.0)));
    assert!(
        !covered(&damage, covering(200.0, 200.0, 201.0, 201.0)),
        "nothing between the two placements"
    );
}

/// An effect whose layer changes damages every slot under it, the subtrees
/// they name included, and leaves the entries beside it alone.
pub(crate) fn an_effect_chain_change_damages_every_slot_under_it() {
    let mut owner = SceneOwner::new();
    let mut writer = owner.writer();
    let root = writer.current().root();
    let (b, c) = (writer.mint_node(), writer.mint_node());
    for (node, owner_id) in [(b, 2), (c, 3)] {
        let version = writer.next_content_version().expect("a version");
        let mut leaf = BoundaryContent::builder();
        leaf.chunk(chunk(
            owner_id,
            version,
            Rect::from_xywh(0.0, 0.0, 30.0, 30.0),
        ));
        writer.push(publish(node, leaf.finish()));
    }
    let beside = writer.next_content_version().expect("a version");
    let beside = chunk(9, beside, Rect::from_xywh(600.0, 500.0, 20.0, 20.0));
    let top = |alpha: f64| {
        let mut top = BoundaryContent::builder();
        top.push_effect(Layer::from(OpacityLayer::new(alpha)), RenderId::new(1), 0);
        top.slot(b);
        top.slot(c);
        top.pop_effect();
        top.chunk(Arc::clone(&beside));
        top.finish()
    };
    writer.push(publish(root, top(1.0)));
    writer.push(place(b, 0.0, 0.0));
    writer.push(place(c, 200.0, 0.0));
    owner.apply(SURFACE).expect("an opacity over two slots");

    let mut writer = owner.writer();
    writer.push(publish(root, top(0.5)));
    owner.apply(SURFACE).expect("a fade");
    let damage = rects(owner.patch_damage());
    assert!(
        covered(&damage, covering(0.0, 0.0, 30.0, 30.0)),
        "{damage:?}"
    );
    assert!(
        covered(&damage, covering(200.0, 0.0, 230.0, 30.0)),
        "{damage:?}"
    );
    assert!(
        !covered(&damage, covering(600.0, 500.0, 620.0, 520.0)),
        "the chunk beside the effect keeps its pixels: {damage:?}"
    );
}

/// A leader whose boundary moves to another parent moves its follower: the
/// follower is resolved after the operations and damaged under its old and
/// new transform, though its own boundary was not touched.
pub(crate) fn a_follower_resolves_after_its_leaders_boundary_moves_parents() {
    let link = LayerLink::new();
    let mut owner = SceneOwner::new();
    let mut writer = owner.writer();
    let root = writer.current().root();
    let (p, q, leader, follower) = (
        writer.mint_node(),
        writer.mint_node(),
        writer.mint_node(),
        writer.mint_node(),
    );
    let mut leading = BoundaryContent::builder();
    leading.push_effect(
        Layer::from(LeaderLayer::new(link, Size::new(10.0, 10.0))),
        RenderId::new(1),
        0,
    );
    writer.push(publish(leader, leading.finish()));
    let version = writer.next_content_version().expect("a version");
    let mut following = BoundaryContent::builder();
    following.push_effect(Layer::from(FollowerLayer::new(link)), RenderId::new(2), 0);
    following.chunk(chunk(2, version, Rect::from_xywh(0.0, 0.0, 20.0, 20.0)));
    writer.push(publish(follower, following.finish()));
    let holding = |child: Option<SceneNodeId>| {
        let mut content = BoundaryContent::builder();
        if let Some(child) = child {
            content.slot(child);
        }
        content.finish()
    };
    writer.push(publish(p, holding(Some(leader))));
    writer.push(publish(q, holding(None)));
    let mut top = BoundaryContent::builder();
    top.slot(p);
    top.slot(q);
    top.slot(follower);
    writer.push(publish(root, top.finish()));
    writer.push(place(p, 100.0, 100.0));
    writer.push(place(q, 500.0, 300.0));
    owner
        .apply(SURFACE)
        .expect("a leader under p, its follower at the root");
    let resolved = owner.followers_resolved();

    let mut writer = owner.writer();
    writer.push(publish(p, holding(None)));
    writer.push(publish(q, holding(Some(leader))));
    owner.apply(SURFACE).expect("the leader moves to q");
    assert!(owner.followers_resolved() > resolved);
    let damage = rects(owner.patch_damage());
    assert!(
        covered(&damage, covering(100.0, 100.0, 120.0, 120.0)),
        "{damage:?}"
    );
    assert!(
        covered(&damage, covering(500.0, 300.0, 520.0, 320.0)),
        "{damage:?}"
    );
}

/// A patch that changes no pixel damages nothing: a content republished
/// with the same entries, or a change to a node the root does not reach.
pub(crate) fn a_patch_that_changes_no_pixel_damages_nothing() {
    let mut owner = SceneOwner::new();
    let mut writer = owner.writer();
    let root = writer.current().root();
    let (a, loose) = (writer.mint_node(), writer.mint_node());
    let version = writer.next_content_version().expect("a version");
    let drawn = chunk(1, version, Rect::from_xywh(10.0, 10.0, 40.0, 40.0));
    let content = |drawn: &Arc<Chunk>| {
        let mut content = BoundaryContent::builder();
        content.chunk(Arc::clone(drawn));
        content.finish()
    };
    writer.push(publish(a, content(&drawn)));
    writer.push(publish(loose, content(&drawn)));
    let mut top = BoundaryContent::builder();
    top.slot(a);
    writer.push(publish(root, top.finish()));
    owner.apply(SURFACE).expect("a drawn leaf and a loose one");

    let mut writer = owner.writer();
    writer.push(publish(a, content(&drawn)));
    let moved = writer.next_content_version().expect("a version");
    writer.push(publish(
        loose,
        content(&chunk(1, moved, Rect::from_xywh(300.0, 300.0, 9.0, 9.0))),
    ));
    writer.push(place(loose, 50.0, 50.0));
    owner
        .apply(SURFACE)
        .expect("the same entries, and a loose change");
    assert_eq!(owner.patch_damage(), DamageRegion::Unchanged);
}

/// Damage that meets what a backdrop blur reads takes the backdrop and its
/// reach, though the backdrop itself did not change (ADR-0304 §6).
pub(crate) fn damage_under_a_backdrop_takes_what_it_reads() {
    let leaf = |version: ContentVersion| {
        let mut content = BoundaryContent::builder();
        content.chunk(chunk(2, version, Rect::from_xywh(190.0, 190.0, 20.0, 20.0)));
        content.finish()
    };
    let mut owner = SceneOwner::new();
    let mut writer = owner.writer();
    let root = writer.current().root();
    let a = writer.mint_node();
    let version = writer.next_content_version().expect("a version");
    writer.push(publish(a, leaf(version)));
    let mut top = BoundaryContent::builder();
    top.slot(a);
    top.push_effect(
        Layer::from(BackdropFilterLayer::new(
            ImageFilter::blur(2.0),
            BlendMode::SrcOver,
            Rect::from_xywh(200.0, 200.0, 100.0, 100.0),
        )),
        RenderId::new(1),
        0,
    );
    top.pop_effect();
    writer.push(publish(root, top.finish()));
    owner.apply(SURFACE).expect("a leaf under a backdrop");

    let mut writer = owner.writer();
    let version = writer.next_content_version().expect("a version");
    writer.push(publish(a, leaf(version)));
    owner.apply(SURFACE).expect("a repaint under the backdrop");
    let damage = rects(owner.patch_damage());
    assert!(
        covered(&damage, covering(193.0, 193.0, 307.0, 307.0)),
        "{damage:?}"
    );
}

// patch_damage_covers_diff_damage

/// A small deterministic generator, so a failing case is reproducible by its
/// seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    fn coord(&mut self, max: usize) -> f64 {
        self.below(max) as f64
    }

    fn rect(&mut self) -> Rect<f64> {
        Rect::from_xywh(
            self.coord(700),
            self.coord(500),
            5.0 + self.coord(115),
            5.0 + self.coord(115),
        )
    }
}

/// One entry of a model boundary.
#[derive(Clone)]
enum Item {
    Chunk {
        owner: u32,
        version: ContentVersion,
        rect: Rect<f64>,
    },
    Effect {
        owner: u32,
        layer: Layer,
        items: Vec<Item>,
    },
    Slot(usize),
}

/// The scene a case drives, as plain data: node 0 is the root.
struct Model {
    ids: Vec<SceneNodeId>,
    items: Vec<Vec<Item>>,
    owners: u32,
    leader: Option<(usize, LayerLink)>,
}

impl Model {
    fn owner(&mut self) -> u32 {
        self.owners += 1;
        self.owners
    }

    fn content(&self, node: usize) -> BoundaryContent {
        fn build(model: &Model, builder: &mut flui_layer::ContentBuilder, items: &[Item]) {
            for item in items {
                match item {
                    Item::Chunk {
                        owner,
                        version,
                        rect,
                    } => {
                        builder.chunk(chunk(*owner, *version, *rect));
                    }
                    Item::Effect {
                        owner,
                        layer,
                        items,
                    } => {
                        builder.push_effect(layer.clone(), RenderId::new(*owner as usize), 0);
                        build(model, builder, items);
                        builder.pop_effect();
                    }
                    Item::Slot(child) => {
                        builder.slot(model.ids[*child]);
                    }
                }
            }
        }
        let mut builder = BoundaryContent::builder();
        build(self, &mut builder, &self.items[node]);
        builder.finish()
    }
}

/// An effect the oracle bounds the way the patch does: an opacity, an
/// affine transform, a clip or a backdrop blur.
fn effect(rng: &mut Rng) -> Layer {
    match rng.below(4) {
        0 => Layer::from(OpacityLayer::new([0.3, 0.6, 1.0][rng.below(3)])),
        1 => {
            let scale = [0.5, 1.0, 2.0][rng.below(3)];
            Layer::from(TransformLayer::new(
                Matrix4::translation(rng.coord(200) - 100.0, rng.coord(200) - 100.0, 0.0)
                    * Matrix4::scaling(scale, scale, 1.0),
            ))
        }
        2 => Layer::from(ClipRectLayer::hard_edge(rng.rect())),
        _ => Layer::from(BackdropFilterLayer::new(
            ImageFilter::blur([1.0, 3.0][rng.below(2)]),
            BlendMode::SrcOver,
            rng.rect(),
        )),
    }
}

fn new_chunk(rng: &mut Rng, model: &mut Model, writer: &mut flui_layer::SceneWriter<'_>) -> Item {
    Item::Chunk {
        owner: model.owner(),
        version: writer.next_content_version().expect("a version"),
        rect: rng.rect(),
    }
}

/// A random tree of up to eight boundaries, with a leader in one of them
/// and its follower drawn last by the root.
fn initial(rng: &mut Rng, owner: &mut SceneOwner, model: &mut Model) {
    let mut writer = owner.writer();
    let count = 2 + rng.below(7);
    model.ids.push(writer.current().root());
    model.items.push(Vec::new());
    for node in 1..count {
        model.ids.push(writer.mint_node());
        model.items.push(Vec::new());
        let parent = rng.below(node);
        model.items[parent].push(Item::Slot(node));
    }
    for node in 0..count {
        for _ in 0..rng.below(4) {
            let item = if rng.chance(70) {
                new_chunk(rng, model, &mut writer)
            } else {
                let items = vec![new_chunk(rng, model, &mut writer)];
                Item::Effect {
                    owner: model.owner(),
                    layer: effect(rng),
                    items,
                }
            };
            let at = rng.below(model.items[node].len() + 1);
            model.items[node].insert(at, item);
        }
    }
    if rng.chance(50) && count > 1 {
        let link = LayerLink::new();
        let node = 1 + rng.below(count - 1);
        let owner = model.owner();
        model.items[node].insert(
            0,
            Item::Effect {
                owner,
                layer: Layer::from(LeaderLayer::with_offset(
                    link,
                    Size::new(10.0, 10.0),
                    Offset::new(rng.coord(100), rng.coord(100)),
                )),
                items: Vec::new(),
            },
        );
        let items = vec![new_chunk(rng, model, &mut writer)];
        let owner = model.owner();
        model.items[0].push(Item::Effect {
            owner,
            layer: Layer::from(FollowerLayer::new(link)),
            items,
        });
        model.leader = Some((node, link));
    }
    for node in (0..count).rev() {
        writer.push(publish(model.ids[node], model.content(node)));
        if node > 0 {
            writer.push(place(model.ids[node], rng.coord(400), rng.coord(300)));
        }
    }
    owner.apply(SURFACE).expect("a generated scene applies");
}

/// The chunks of `items`, as paths of top-level and nested positions.
fn chunk_paths(items: &[Item]) -> Vec<(usize, Option<usize>)> {
    let mut paths = Vec::new();
    for (index, item) in items.iter().enumerate() {
        match item {
            Item::Chunk { .. } => paths.push((index, None)),
            Item::Effect { items, .. } => {
                for (inner, item) in items.iter().enumerate() {
                    if matches!(item, Item::Chunk { .. }) {
                        paths.push((index, Some(inner)));
                    }
                }
            }
            Item::Slot(_) => {}
        }
    }
    paths
}

/// One random patch of one to three marks, moves, effect changes, added or
/// dropped chunks, swaps and leader moves; returns its operation count.
fn mutate(rng: &mut Rng, owner: &mut SceneOwner, model: &mut Model, plant: bool) -> usize {
    let mut writer = owner.writer();
    let mut ops = Vec::new();
    let mut republish = Vec::new();
    let count = model.ids.len();
    for _ in 0..=rng.below(3) {
        let node = rng.below(count);
        match rng.below(6) {
            0 => {
                let paths = chunk_paths(&model.items[node]);
                if paths.is_empty() {
                    continue;
                }
                let (outer, inner) = paths[rng.below(paths.len())];
                let version = writer.next_content_version().expect("a version");
                let moved = rng.chance(50).then(|| rng.rect());
                let item = match inner {
                    None => &mut model.items[node][outer],
                    Some(inner) => match &mut model.items[node][outer] {
                        Item::Effect { items, .. } => &mut items[inner],
                        _ => unreachable!("a nested path is under an effect"),
                    },
                };
                if let Item::Chunk {
                    version: old, rect, ..
                } = item
                {
                    *old = version;
                    if let Some(moved) = moved {
                        *rect = moved;
                    }
                }
                republish.push(node);
            }
            1 if node > 0 => {
                ops.push(place(model.ids[node], rng.coord(400), rng.coord(300)));
            }
            2 => {
                let effects: Vec<usize> = (0..model.items[node].len())
                    .filter(|&index| {
                        matches!(&model.items[node][index], Item::Effect { layer, .. }
                            if !matches!(layer, Layer::Leader(_) | Layer::Follower(_)))
                    })
                    .collect();
                if effects.is_empty() {
                    continue;
                }
                let index = effects[rng.below(effects.len())];
                if let Item::Effect { layer, .. } = &mut model.items[node][index] {
                    *layer = effect(rng);
                }
                republish.push(node);
            }
            3 => {
                let paths: Vec<usize> = (0..model.items[node].len())
                    .filter(|&index| matches!(model.items[node][index], Item::Chunk { .. }))
                    .collect();
                if rng.chance(50) && !paths.is_empty() {
                    model.items[node].remove(paths[rng.below(paths.len())]);
                } else {
                    let item = new_chunk(rng, model, &mut writer);
                    let at = rng.below(model.items[node].len() + 1);
                    model.items[node].insert(at, item);
                }
                republish.push(node);
            }
            4 => {
                let items = &mut model.items[node];
                let pairs: Vec<usize> = (1..items.len())
                    .filter(|&index| {
                        matches!(items[index - 1], Item::Chunk { .. })
                            && matches!(items[index], Item::Chunk { .. })
                    })
                    .collect();
                if pairs.is_empty() {
                    continue;
                }
                let at = pairs[rng.below(pairs.len())];
                items.swap(at - 1, at);
                republish.push(node);
            }
            _ => {
                let Some((leader, link)) = model.leader else {
                    continue;
                };
                let at = model.items[leader]
                    .iter()
                    .position(|item| {
                        matches!(
                            item,
                            Item::Effect {
                                layer: Layer::Leader(_),
                                ..
                            }
                        )
                    })
                    .expect("the leader's boundary holds it");
                let Item::Effect { layer, .. } = &mut model.items[leader][at] else {
                    unreachable!("the position holds the leader");
                };
                *layer = Layer::from(LeaderLayer::with_offset(
                    link,
                    Size::new(10.0, 10.0),
                    Offset::new(rng.coord(300), rng.coord(300)),
                ));
                republish.push(leader);
            }
        }
    }
    republish.sort_unstable();
    republish.dedup();
    for node in republish {
        ops.push(publish(model.ids[node], model.content(node)));
    }
    let total = ops.len();
    for op in ops {
        writer.push(op);
    }
    if plant && total > 0 {
        owner.plant_lost_footprint(rng.below(total));
    }
    owner.apply(SURFACE).expect("a generated patch applies");
    total
}

/// The full diff the patch damage is checked against (ADR-0304 §11): each
/// boundary and each chunk is a stamped layer whose token changes with its
/// recording, so the differ pairs them across versions, and each follower
/// is baked into the transform it resolves to, so the differ sees where its
/// content lands instead of damaging it every frame.
struct Oracle {
    differ: LayerDiffer,
    ids: HashMap<(SceneNodeId, u8, usize), RenderId>,
    tokens: HashMap<ContentVersion, ContentToken>,
    nodes: HashMap<SceneNodeId, ContentToken>,
}

impl Oracle {
    fn new() -> Self {
        Self {
            differ: LayerDiffer::new(DamageMode::On { full_above: 2.0 }),
            ids: HashMap::new(),
            tokens: HashMap::new(),
            nodes: HashMap::new(),
        }
    }

    fn id(&mut self, key: (SceneNodeId, u8, usize)) -> RenderId {
        let next = self.ids.len() + 1;
        *self.ids.entry(key).or_insert_with(|| RenderId::new(next))
    }

    fn diff(&mut self, version: &SceneVersion) -> (DamageRegion, Vec<Closure>) {
        let stamped = self.stamped(version);
        let mut baked = LayerTree::new(copy(stamped.get(stamped.root()).expect("a root"), None));
        let root = baked.root();
        bake(&stamped, stamped.root(), &mut baked, root);
        let mut closures = Vec::new();
        backdrops(&baked, baked.root(), Matrix4::IDENTITY, &mut closures);
        (self.differ.diff(&Scene::new(baked), SURFACE.size), closures)
    }

    fn stamped(&mut self, version: &SceneVersion) -> LayerTree {
        let root = version.root();
        let node = |oracle: &mut Self, id: SceneNodeId, offset: Offset<f64>| {
            let token = oracle
                .nodes
                .entry(id)
                .or_insert_with(ContentToken::mint)
                .clone();
            LayerNode::new(Layer::from(OffsetLayer::new(offset)))
                .with_boundary(oracle.id((id, 0, 0)), token)
        };
        let placement = version.node(root).expect("a root").placement().offset();
        let mut tree = LayerTree::new(node(self, root, placement));
        let mut pending = vec![(root, tree.root())];
        while let Some((id, at)) = pending.pop() {
            let content = Arc::clone(version.node(id).expect("a slot's node").content());
            let mut placed: Vec<LayerId> = Vec::with_capacity(content.entries().len());
            for entry in content.entries() {
                let parent = entry.parent().map_or(at, |index| placed[index.get()]);
                let layer = match entry {
                    Entry::Effect { layer, .. } => tree.push_child(parent, layer.clone()),
                    Entry::Chunk { chunk, .. } => {
                        let token = self
                            .tokens
                            .entry(chunk.version())
                            .or_insert_with(ContentToken::mint)
                            .clone();
                        let key = (id, 1, chunk.owner().as_u64() as usize);
                        let stamp = self.id(key);
                        tree.push_child(
                            parent,
                            LayerNode::new(Layer::from(PictureLayer::new(
                                chunk.commands().clone(),
                            )))
                            .with_boundary(stamp, token),
                        )
                    }
                    Entry::Slot { node: child, .. } => {
                        let offset = version
                            .node(*child)
                            .expect("a slot's node")
                            .placement()
                            .offset();
                        let layer = node(self, *child, offset);
                        let layer = tree.push_child(parent, layer);
                        pending.push((*child, layer));
                        layer
                    }
                };
                placed.push(layer);
            }
        }
        tree
    }
}

fn copy(node: &LayerNode, layer: Option<Layer>) -> LayerNode {
    let copied = LayerNode::new(layer.unwrap_or_else(|| node.layer().clone()));
    match node.boundary() {
        Some(stamp) => copied.with_boundary(stamp.render_id(), stamp.content().clone()),
        None => copied,
    }
}

/// Copies `from`'s children into `to`, each follower as the transform it
/// resolves to, or left out with its subtree when it resolves to nothing.
fn bake(tree: &LayerTree, from: LayerId, baked: &mut LayerTree, to: LayerId) {
    for &child in tree.children(from).expect("a node of the tree") {
        let node = tree.get(child).expect("a child of the tree");
        let layer = match node.layer() {
            Layer::Follower(_) => match resolve_follower_transform(tree, child) {
                Some(transform) => Some(Layer::from(TransformLayer::new(transform))),
                None => continue,
            },
            _ => None,
        };
        let copied = baked.push_child(to, copy(node, layer));
        bake(tree, child, baked, copied);
    }
}

/// Whether `patch` covers `diff`. The differ answers one rect, which closes
/// over every backdrop its bounding box meets, so the patch's region is
/// compared the same way: its bounds, closed over the same backdrops.
fn covers(patch: DamageRegion, diff: DamageRegion, closures: &[Closure]) -> bool {
    match (patch, diff) {
        (_, DamageRegion::Unchanged) | (DamageRegion::Full(_), _) => true,
        (_, DamageRegion::Full(_)) | (DamageRegion::Unchanged, DamageRegion::Region(_)) => false,
        (DamageRegion::Region(patch), DamageRegion::Region(diff)) => {
            match close_over(
                DamageRegion::Region(Region::new(patch.bounds())),
                closures.iter().copied(),
                SURFACE.size,
            ) {
                DamageRegion::Region(closed) => covered(&[closed.bounds()], diff.bounds()),
                DamageRegion::Full(_) => true,
                DamageRegion::Unchanged => false,
            }
        }
    }
}

/// What each backdrop of `tree` reads and writes.
fn backdrops(tree: &LayerTree, id: LayerId, parent: Matrix4, out: &mut Vec<Closure>) {
    let layer = tree.get_layer(id).expect("a node of the tree");
    let ctm = if let Layer::Transform(transform) = layer {
        parent * *transform.transform()
    } else {
        let offset = layer.local_translation();
        parent * Matrix4::translation(offset.dx, offset.dy, 0.0)
    };
    if let Layer::BackdropFilter(_) = layer {
        out.push(read_closure(layer, &ctm, Rect::ZERO));
    }
    for &child in tree.children(id).expect("a node of the tree") {
        backdrops(tree, child, ctm, out);
    }
}

/// Runs `cases` random scenes of three patches each; the first case whose
/// patch damage misses the diff's, by seed and step.
fn first_miss(cases: u64, plant: bool) -> Option<String> {
    for case in 0..cases {
        let seed = 0x9E37_79B9_7F4A_7C15 ^ (case + 1).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        let mut rng = Rng(seed);
        let mut owner = SceneOwner::new();
        let mut model = Model {
            ids: Vec::new(),
            items: Vec::new(),
            owners: 0,
            leader: None,
        };
        let mut oracle = Oracle::new();
        initial(&mut rng, &mut owner, &mut model);
        let _ = oracle.diff(owner.current());
        for step in 0..3 {
            mutate(&mut rng, &mut owner, &mut model, plant);
            let (diff, closures) = oracle.diff(owner.current());
            let patch = owner.patch_damage();
            if !covers(patch, diff, &closures) {
                return Some(format!(
                    "case {case} (seed {seed:#x}) step {step}: patch {patch:?} misses diff {diff:?}"
                ));
            }
        }
    }
    None
}

/// Random trees, marks, moves and followers: the patch damage covers the
/// full diff of the versions before and after, and a planted patch that
/// drops one operation's footprint is caught.
pub(crate) fn patch_damage_covers_diff_damage() {
    if let Some(miss) = first_miss(10_000, false) {
        panic!("{miss}");
    }
    assert!(
        first_miss(10_000, true).is_some(),
        "a patch that drops one operation's footprint passes the oracle"
    );
}
