//! A composited-layer update replaces effect layers and shares the rest of
//! the content: it damages and bounds what a content built afresh with the
//! same layers would (ADR-0304 §2, §4).

use std::sync::Arc;

use flui_foundation::RenderId;
use flui_foundation::SurfaceGeneration;
use flui_foundation::geometry::{Matrix4, Offset, Rect};
use flui_layer::{
    BoundaryContent, Chunk, ClipRectLayer, ContentVersion, DamageRect, DamageRegion, Entry,
    EntryIndex, ImageFilterLayer, Layer, OpacityLayer, Placement, SceneNodeId, SceneOp, SceneOwner,
    SceneWriter, SurfaceKey, TextureLayer, TransformLayer,
};
use flui_painting::paint::{Clip, ImageFilter, TextureId};
use flui_painting::styling::Color;
use flui_painting::{Canvas, Paint};

const SURFACE: SurfaceKey = SurfaceKey {
    size: (800, 600),
    generation: SurfaceGeneration::ZERO,
};

/// A row of the table, run under each damage path.
pub(crate) type Row = fn(&mut Run);
/// How a row's owners damage an effect update, and what they published.
pub(crate) struct Run {
    unshared: bool,
    /// Each damage a row read, with the version it was read on, flattened.
    pub(crate) seen: Vec<(DamageRegion, String)>,
}

impl Run {
    /// The damage path production takes: the inside and the chain the
    /// versions share are read once.
    pub(crate) fn shared() -> Self {
        Self {
            unshared: false,
            seen: Vec::new(),
        }
    }

    /// Each version's own scope and chain, the path the shared one must
    /// equal.
    pub(crate) fn unshared() -> Self {
        Self {
            unshared: true,
            seen: Vec::new(),
        }
    }

    fn owner(&self) -> SceneOwner {
        let mut owner = SceneOwner::new();
        if self.unshared {
            owner.force_unshared_effect_updates();
        }
        owner
    }

    /// The last patch's damage, recorded with the version it published.
    fn damage(&mut self, owner: &SceneOwner) -> DamageRegion {
        let damage = owner.patch_damage();
        self.seen
            .push((damage, format!("{:?}", owner.current().flatten())));
        damage
    }
}

fn chunk(owner: u32, version: ContentVersion, rect: Rect<f64>) -> Arc<Chunk> {
    let mut canvas = Canvas::new();
    canvas.draw_rect(rect, &Paint::fill(Color::RED));
    Arc::new(Chunk::new(
        RenderId::new(owner as usize),
        0,
        version,
        canvas.finish(),
    ))
}

fn publish(node: SceneNodeId, content: Arc<BoundaryContent>) -> SceneOp {
    SceneOp::Publish { node, content }
}

fn place(node: SceneNodeId, x: f64, y: f64) -> SceneOp {
    SceneOp::Place {
        node,
        placement: Placement::try_new(Offset::new(x, y)).expect("a finite offset"),
    }
}

fn translate(dx: f64, dy: f64) -> Layer {
    Layer::from(TransformLayer::new(Matrix4::translation(dx, dy, 0.0)))
}

fn clip(rect: Rect<f64>) -> Layer {
    Layer::from(ClipRectLayer::new(rect, Clip::HardEdge))
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

fn covered(damage: &[DamageRect], rect: DamageRect) -> bool {
    damage.iter().any(|outer| {
        outer.left() <= rect.left()
            && outer.top() <= rect.top()
            && outer.right() >= rect.right()
            && outer.bottom() >= rect.bottom()
    })
}

/// A boundary `node` the root slots, its content published and applied.
fn scene_with(
    run: &Run,
    content: impl FnOnce(&mut SceneWriter<'_>) -> (SceneNodeId, Arc<BoundaryContent>),
) -> (SceneOwner, SceneNodeId, Arc<BoundaryContent>) {
    let mut owner = run.owner();
    let mut writer = owner.writer();
    let (node, content) = content(&mut writer);
    let root = writer.current().root();
    writer.push(publish(node, Arc::clone(&content)));
    let mut top = BoundaryContent::builder();
    top.slot(node);
    writer.push(publish(root, Arc::new(top.finish())));
    owner.apply(SURFACE).expect("a root over one boundary");
    (owner, node, content)
}

/// Updating an effect nested under another damages the nested effect's
/// scope under its old and new layer, though the outer one is unchanged, and
/// the boundary's subtree bounds follow it: a later move of the boundary
/// damages the chunk where the new transform put it.
pub(crate) fn a_nested_effect_update_damages_its_scope_and_moves_the_bounds(run: &mut Run) {
    let mut inner = None;
    let (mut owner, node, content) = scene_with(run, |writer| {
        let node = writer.mint_node();
        let version = writer.next_content_version().expect("a version");
        let mut content = BoundaryContent::builder();
        content.push_effect(
            clip(Rect::from_xywh(0.0, 0.0, 400.0, 400.0)),
            RenderId::new(1),
            0,
        );
        inner = Some(content.push_effect(translate(0.0, 0.0), RenderId::new(2), 0));
        content.chunk(chunk(3, version, Rect::from_xywh(10.0, 10.0, 10.0, 10.0)));
        content.pop_effect();
        content.chunk(chunk(4, version, Rect::from_xywh(0.0, 300.0, 10.0, 10.0)));
        (node, Arc::new(content.finish()))
    });
    let inner = inner.expect("the inner effect");

    let mut writer = owner.writer();
    let updated = writer
        .with_effects(&content, [(inner, translate(200.0, 0.0))])
        .expect("an effect entry");
    writer.push(publish(node, updated));
    owner.apply(SURFACE).expect("the inner transform changes");
    let damage = rects(run.damage(&owner));
    assert!(
        covered(&damage, covering(10.0, 10.0, 20.0, 20.0)),
        "{damage:?}"
    );
    assert!(
        covered(&damage, covering(210.0, 10.0, 220.0, 20.0)),
        "{damage:?}"
    );
    assert!(
        !covered(&damage, covering(0.0, 300.0, 10.0, 310.0)),
        "the chunk beside the inner effect keeps its pixels: {damage:?}"
    );

    let mut writer = owner.writer();
    writer.push(place(node, 0.0, 100.0));
    owner.apply(SURFACE).expect("the boundary moves");
    let damage = rects(run.damage(&owner));
    assert!(
        covered(&damage, covering(210.0, 110.0, 220.0, 120.0)),
        "the subtree bounds hold the chunk where the new transform draws it: {damage:?}"
    );
}

/// Updating an effect damages the chunks of an unchanged effect under it
/// under both of the outer layers, and nothing outside its scope.
pub(crate) fn an_outer_effect_update_damages_its_unchanged_descendants(run: &mut Run) {
    let mut outer = None;
    let (mut owner, node, content) = scene_with(run, |writer| {
        let node = writer.mint_node();
        let version = writer.next_content_version().expect("a version");
        let mut content = BoundaryContent::builder();
        outer = Some(content.push_effect(translate(0.0, 0.0), RenderId::new(1), 0));
        content.push_effect(Layer::from(OpacityLayer::new(0.5)), RenderId::new(2), 0);
        content.chunk(chunk(3, version, Rect::from_xywh(10.0, 10.0, 10.0, 10.0)));
        content.pop_effect();
        content.pop_effect();
        content.chunk(chunk(4, version, Rect::from_xywh(600.0, 500.0, 10.0, 10.0)));
        (node, Arc::new(content.finish()))
    });
    let outer = outer.expect("the outer effect");

    let mut writer = owner.writer();
    let updated = writer
        .with_effects(&content, [(outer, translate(300.0, 0.0))])
        .expect("an effect entry");
    writer.push(publish(node, updated));
    owner.apply(SURFACE).expect("the outer transform changes");
    let damage = rects(run.damage(&owner));
    assert!(
        covered(&damage, covering(10.0, 10.0, 20.0, 20.0)),
        "{damage:?}"
    );
    assert!(
        covered(&damage, covering(310.0, 10.0, 320.0, 20.0)),
        "{damage:?}"
    );
    assert!(
        !covered(&damage, covering(600.0, 500.0, 610.0, 510.0)),
        "the chunk outside the outer effect keeps its pixels: {damage:?}"
    );
}

/// One shape of content: the layers it is built with, beside the slots and
/// chunks `build` places around them.
struct Shape {
    name: &'static str,
    layers: Vec<Layer>,
    /// Which of `layers` the update replaces, and with what.
    replaced: Vec<(usize, Layer)>,
}

/// The content of `shape` with `layers`, around the same chunks and slots,
/// and the entry of each effect.
fn build(
    layers: &[Layer],
    chunks: &[Arc<Chunk>],
    slots: &[SceneNodeId],
) -> (BoundaryContent, Vec<EntryIndex>, EntryIndex) {
    let mut content = BoundaryContent::builder();
    let mut effects = Vec::new();
    let first = content.chunk(Arc::clone(&chunks[0]));
    for (ordinal, layer) in layers.iter().enumerate() {
        let ordinal = u32::try_from(ordinal).expect("a few layers");
        effects.push(content.push_effect(layer.clone(), RenderId::new(1), ordinal));
        content.chunk(Arc::clone(
            &chunks[1 + ordinal as usize % (chunks.len() - 1)],
        ));
        if let Some(slot) = slots.get(ordinal as usize) {
            content.slot(*slot);
        }
        // Every other effect closes before the next opens: siblings and
        // nesting in one stream.
        if ordinal % 2 == 1 {
            content.pop_effect();
        }
    }
    while content.pop_effect().is_some() {}
    content.chunk(Arc::clone(&chunks[0]));
    (content.finish(), effects, first)
}

/// The entries, as their kind, parent and what they name.
fn shape_of(content: &BoundaryContent) -> Vec<String> {
    content
        .entries()
        .iter()
        .enumerate()
        .map(|(position, entry)| match entry {
            Entry::Effect {
                owner,
                ordinal,
                parent,
            } => format!("{position} effect {owner:?}/{ordinal} under {parent:?}"),
            Entry::Chunk { chunk, parent } => {
                format!("{position} chunk {:p} under {parent:?}", Arc::as_ptr(chunk))
            }
            Entry::Slot { node, parent } => format!("{position} slot {node:?} under {parent:?}"),
        })
        .collect()
}

/// A content an effect update derives equals one built afresh with the same
/// layers: its entries, its layers, its slots, the damage of publishing it
/// over the content before and the bounds a later move damages.
pub(crate) fn an_effect_update_equals_a_content_built_afresh(run: &mut Run) {
    let shapes = [
        Shape {
            name: "a transform under a clip",
            layers: vec![
                clip(Rect::from_xywh(0.0, 0.0, 500.0, 400.0)),
                translate(0.0, 0.0),
            ],
            replaced: vec![(1, translate(40.0, 20.0))],
        },
        Shape {
            name: "an image filter and an opacity",
            layers: vec![
                Layer::from(OpacityLayer::new(1.0)),
                translate(5.0, 5.0),
                Layer::from(ImageFilterLayer::new(ImageFilter::blur(2.0))),
            ],
            replaced: vec![
                (0, Layer::from(OpacityLayer::new(0.5))),
                (
                    2,
                    Layer::from(ImageFilterLayer::new(ImageFilter::blur(6.0))),
                ),
            ],
        },
        Shape {
            name: "a texture and nested transforms",
            layers: vec![
                translate(0.0, 0.0),
                Layer::from(TextureLayer::new(
                    TextureId::new(3),
                    Rect::from_xywh(0.0, 0.0, 20.0, 20.0),
                )),
                translate(10.0, 0.0),
                translate(0.0, 10.0),
            ],
            replaced: vec![(0, translate(100.0, 0.0)), (3, translate(0.0, 50.0))],
        },
    ];
    for shape in shapes {
        let name = shape.name;
        let mut owner = run.owner();
        let mut writer = owner.writer();
        let root = writer.current().root();
        let node = writer.mint_node();
        let version = writer.next_content_version().expect("a version");
        let chunks: Vec<Arc<Chunk>> = (0..3)
            .map(|index| {
                let at = f64::from(index) * 60.0;
                chunk(10 + index, version, Rect::from_xywh(at, at, 30.0, 30.0))
            })
            .collect();
        let slots: Vec<SceneNodeId> = (0..2).map(|_| writer.mint_node()).collect();
        for (index, &slot) in slots.iter().enumerate() {
            let mut leaf = BoundaryContent::builder();
            leaf.chunk(chunk(
                20 + u32::try_from(index).expect("two slots"),
                version,
                Rect::from_xywh(0.0, 0.0, 15.0, 15.0),
            ));
            writer.push(publish(slot, Arc::new(leaf.finish())));
            writer.push(place(slot, 100.0 * (1.0 + index as f64), 50.0));
        }
        let (before, effects, first) = build(&shape.layers, &chunks, &slots);
        let before = Arc::new(before);
        let mut fresh_layers = shape.layers.clone();
        for (at, layer) in &shape.replaced {
            fresh_layers[*at] = layer.clone();
        }
        let (fresh, _, _) = build(&fresh_layers, &chunks, &slots);
        let updated = writer
            .with_effects(
                &before,
                shape
                    .replaced
                    .iter()
                    .map(|(at, layer)| (effects[*at], layer.clone())),
            )
            .expect("effect entries");

        assert_eq!(shape_of(&updated), shape_of(&fresh), "{name}: entries");
        for &effect in &effects {
            assert_eq!(
                format!("{:?}", updated.effect_layer(effect)),
                format!("{:?}", fresh.effect_layer(effect)),
                "{name}: the layer at {effect:?}"
            );
        }
        assert_eq!(
            updated.slots().collect::<Vec<_>>(),
            fresh.slots().collect::<Vec<_>>(),
            "{name}: slots"
        );
        assert_eq!(
            updated.effect_layer(first).map(|_| ()),
            None,
            "{name}: a chunk entry holds no layer"
        );

        let mut top = BoundaryContent::builder();
        top.slot(node);
        writer.push(publish(node, Arc::clone(&before)));
        writer.push(publish(root, Arc::new(top.finish())));
        owner.apply(SURFACE).expect("the shape under the root");
        // Each content over `before`, then moved: the damage of the publish
        // and of the move, which reads the bounds the publish left.
        let mut outcomes = Vec::new();
        for content in [updated, Arc::new(fresh)] {
            let mut writer = owner.writer();
            writer.push(publish(node, Arc::clone(&before)));
            writer.push(place(node, 0.0, 0.0));
            owner.apply(SURFACE).expect("back to the content before");
            let mut writer = owner.writer();
            writer.push(publish(node, content));
            owner.apply(SURFACE).expect("the layers change");
            let published = run.damage(&owner);
            let mut writer = owner.writer();
            writer.push(place(node, 7.0, 3.0));
            owner.apply(SURFACE).expect("the boundary moves");
            outcomes.push((published, run.damage(&owner)));
        }
        assert_eq!(outcomes[0], outcomes[1], "{name}: damage and bounds");
        assert_ne!(
            outcomes[0].0,
            DamageRegion::Unchanged,
            "{name}: the update damages"
        );
    }
}

/// An update that changes an effect and one nested under it damages the
/// nested chunk under both old layers and both new ones: the outer effect's
/// inside differs between the versions, so it is folded under each.
pub(crate) fn an_update_of_an_effect_and_its_descendant_folds_each_inside(run: &mut Run) {
    let mut outer = None;
    let mut inner = None;
    let (mut owner, node, content) = scene_with(run, |writer| {
        let node = writer.mint_node();
        let version = writer.next_content_version().expect("a version");
        let mut content = BoundaryContent::builder();
        outer = Some(content.push_effect(translate(0.0, 0.0), RenderId::new(1), 0));
        inner = Some(content.push_effect(translate(0.0, 0.0), RenderId::new(2), 0));
        content.chunk(chunk(3, version, Rect::from_xywh(10.0, 10.0, 10.0, 10.0)));
        content.pop_effect();
        content.pop_effect();
        (node, Arc::new(content.finish()))
    });
    let (outer, inner) = (outer.expect("the outer"), inner.expect("the inner"));

    let mut writer = owner.writer();
    let updated = writer
        .with_effects(
            &content,
            [
                (outer, translate(100.0, 0.0)),
                (inner, translate(0.0, 50.0)),
            ],
        )
        .expect("effect entries");
    writer.push(publish(node, updated));
    owner.apply(SURFACE).expect("both effects change");
    let damage = rects(run.damage(&owner));
    assert!(
        covered(&damage, covering(10.0, 10.0, 20.0, 20.0)),
        "the chunk under both old layers: {damage:?}"
    );
    assert!(
        covered(&damage, covering(110.0, 60.0, 120.0, 70.0)),
        "the chunk under both new layers: {damage:?}"
    );
}

/// An effect update in the patch that also moves its boundary under another
/// parent damages the effect's scope through the old parent's chain under
/// the old version and the new parent's under the new one.
pub(crate) fn an_effect_update_of_a_reparented_boundary_reads_each_versions_chain(run: &mut Run) {
    let mut owner = run.owner();
    let mut writer = owner.writer();
    let root = writer.current().root();
    let (left, right, node) = (writer.mint_node(), writer.mint_node(), writer.mint_node());
    let version = writer.next_content_version().expect("a version");
    let mut moving = BoundaryContent::builder();
    let effect = moving.push_effect(translate(0.0, 0.0), RenderId::new(1), 0);
    moving.chunk(chunk(2, version, Rect::from_xywh(0.0, 0.0, 10.0, 10.0)));
    moving.pop_effect();
    let moving = Arc::new(moving.finish());
    writer.push(publish(node, Arc::clone(&moving)));
    let mut holder = BoundaryContent::builder();
    holder.slot(node);
    writer.push(publish(left, Arc::new(holder.finish())));
    writer.push(publish(
        right,
        Arc::new(BoundaryContent::builder().finish()),
    ));
    let mut top = BoundaryContent::builder();
    top.slot(left);
    top.slot(right);
    writer.push(publish(root, Arc::new(top.finish())));
    writer.push(place(right, 400.0, 300.0));
    owner
        .apply(SURFACE)
        .expect("two parents, the boundary under the left");

    let mut writer = owner.writer();
    let updated = writer
        .with_effects(&moving, [(effect, translate(20.0, 0.0))])
        .expect("an effect entry");
    writer.push(publish(node, updated));
    writer.push(publish(left, Arc::new(BoundaryContent::builder().finish())));
    let mut holder = BoundaryContent::builder();
    holder.slot(node);
    writer.push(publish(right, Arc::new(holder.finish())));
    owner
        .apply(SURFACE)
        .expect("the effect changes as the boundary moves to the right parent");
    let damage = rects(run.damage(&owner));
    assert!(
        covered(&damage, covering(0.0, 0.0, 10.0, 10.0)),
        "under the left parent with the old layer: {damage:?}"
    );
    assert!(
        covered(&damage, covering(420.0, 300.0, 430.0, 310.0)),
        "under the right parent with the new layer: {damage:?}"
    );
}

/// An effect update in the patch that also moves a boundary its scope slots
/// damages the slot where each version draws it: the slot differs between
/// the versions, so the effect's inside is folded under each, and nothing is
/// drawn where neither version put the slot.
pub(crate) fn an_effect_update_over_a_moved_slot_folds_each_inside(run: &mut Run) {
    let mut effect = None;
    let mut child = None;
    let (mut owner, node, content) = scene_with(run, |writer| {
        let node = writer.mint_node();
        let slotted = writer.mint_node();
        let version = writer.next_content_version().expect("a version");
        let mut leaf = BoundaryContent::builder();
        leaf.chunk(chunk(4, version, Rect::from_xywh(0.0, 0.0, 10.0, 10.0)));
        writer.push(publish(slotted, Arc::new(leaf.finish())));
        writer.push(place(slotted, 100.0, 50.0));
        let mut content = BoundaryContent::builder();
        effect = Some(content.push_effect(translate(0.0, 0.0), RenderId::new(1), 0));
        content.slot(slotted);
        content.pop_effect();
        child = Some(slotted);
        (node, Arc::new(content.finish()))
    });
    let (effect, child) = (effect.expect("the effect"), child.expect("the slot"));

    let mut writer = owner.writer();
    let updated = writer
        .with_effects(&content, [(effect, translate(40.0, 0.0))])
        .expect("an effect entry");
    writer.push(publish(node, updated));
    writer.push(place(child, 300.0, 50.0));
    owner
        .apply(SURFACE)
        .expect("the effect changes and the slot moves");
    let damage = rects(run.damage(&owner));
    assert!(
        covered(&damage, covering(100.0, 50.0, 110.0, 60.0)),
        "the slot where the old version drew it: {damage:?}"
    );
    assert!(
        covered(&damage, covering(340.0, 50.0, 350.0, 60.0)),
        "the slot where the new version draws it: {damage:?}"
    );
}
