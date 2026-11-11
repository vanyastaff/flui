//! A patch's damage is the footprint of its operations under the version
//! before and after it, closed over what filters read, and it covers what a
//! full diff of the two versions finds (ADR-0304 §4, §6, §11).

use std::collections::HashMap;
use std::sync::Arc;

use flui_foundation::geometry::{Matrix4, Offset, Point, Rect, Size};
use flui_foundation::{LayerId, RenderId, SurfaceGeneration};
use flui_layer::{
    BackdropFilterLayer, BoundaryContent, Chunk, ClipRectLayer, Closure, ContentToken,
    ContentVersion, DamageMode, DamageRect, DamageRegion, Entry, FollowerLayer, ImageFilterLayer,
    Layer, LayerDiffer, LayerLink, LayerNode, LayerTree, LeaderClip, LeaderLayer, OffsetLayer,
    OpacityLayer, PictureLayer, Placement, Region, Scene, SceneNodeId, SceneOp, SceneOwner,
    SceneVersion, SurfaceKey, TransformLayer, close_over, read_closure, resolve_follower_transform,
};
use flui_painting::paint::{BlendMode, Clip, ImageFilter};
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

    // The root's placement offsets the whole scene, as `flatten` draws it.
    let mut writer = owner.writer();
    writer.push(place(root, 50.0, 50.0));
    owner.apply(SURFACE).expect("the root moves");
    let damage = rects(owner.patch_damage());
    assert!(
        covered(&damage, covering(410.0, 300.0, 450.0, 320.0)),
        "{damage:?}"
    );
    assert!(
        covered(&damage, covering(460.0, 350.0, 500.0, 370.0)),
        "{damage:?}"
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

/// A follower resolves against the leader the scene draws: a leader of the
/// same link in a published node no slot reaches never shadows it, so moving
/// the attached leader moves the follower's footprint.
pub(crate) fn an_unattached_leader_never_shadows_the_attached_one() {
    let link = LayerLink::new();
    let mut owner = SceneOwner::new();
    let mut writer = owner.writer();
    let root = writer.current().root();
    let (p, leader, stray, follower) = (
        writer.mint_node(),
        writer.mint_node(),
        writer.mint_node(),
        writer.mint_node(),
    );
    let leading = || {
        let mut content = BoundaryContent::builder();
        content.push_effect(
            Layer::from(LeaderLayer::new(link, Size::new(10.0, 10.0))),
            RenderId::new(1),
            0,
        );
        content.finish()
    };
    writer.push(publish(leader, leading()));
    let version = writer.next_content_version().expect("a version");
    let mut following = BoundaryContent::builder();
    following.push_effect(Layer::from(FollowerLayer::new(link)), RenderId::new(2), 0);
    following.chunk(chunk(2, version, Rect::from_xywh(0.0, 0.0, 20.0, 20.0)));
    writer.push(publish(follower, following.finish()));
    let mut holding = BoundaryContent::builder();
    holding.slot(leader);
    writer.push(publish(p, holding.finish()));
    let mut top = BoundaryContent::builder();
    top.slot(p);
    top.slot(follower);
    writer.push(publish(root, top.finish()));
    writer.push(place(p, 100.0, 100.0));
    // Indexed after the attached leader, and no slot names it.
    writer.push(publish(stray, leading()));
    owner
        .apply(SURFACE)
        .expect("an attached leader under p and a stray one");

    let mut writer = owner.writer();
    writer.push(place(p, 300.0, 300.0));
    owner.apply(SURFACE).expect("the attached leader moves");
    let damage = rects(owner.patch_damage());
    assert!(
        covered(&damage, covering(100.0, 100.0, 120.0, 120.0)),
        "{damage:?}"
    );
    assert!(
        covered(&damage, covering(300.0, 300.0, 320.0, 320.0)),
        "{damage:?}"
    );
}

/// A magnifier's bilinear read reaches half a source pixel past its source
/// before the map spreads it: a 10× linear magnifier over 10..20 draws
/// 95..205, and moving it damages all of that.
pub(crate) fn a_magnifier_damages_its_sampling_fringe() {
    let mut owner = SceneOwner::new();
    let mut writer = owner.writer();
    let root = writer.current().root();
    let lens = writer.mint_node();
    let version = writer.next_content_version().expect("a version");
    let mut content = BoundaryContent::builder();
    content.push_effect(
        Layer::from(ImageFilterLayer::new(
            ImageFilter::try_magnify(Point::new(0.0, 0.0), 10.0).expect("a finite magnifier"),
        )),
        RenderId::new(1),
        0,
    );
    content.chunk(chunk(1, version, Rect::from_ltrb(10.0, 10.0, 20.0, 20.0)));
    writer.push(publish(lens, content.finish()));
    let mut top = BoundaryContent::builder();
    top.slot(lens);
    writer.push(publish(root, top.finish()));
    owner.apply(SURFACE).expect("a magnifier at the origin");

    let mut writer = owner.writer();
    writer.push(place(lens, 300.0, 0.0));
    owner.apply(SURFACE).expect("the magnifier moves");
    let damage = rects(owner.patch_damage());
    assert!(
        covered(&damage, covering(95.0, 95.0, 205.0, 205.0)),
        "{damage:?}"
    );
    assert!(
        covered(&damage, covering(395.0, 95.0, 505.0, 205.0)),
        "{damage:?}"
    );
}

/// Footprints count follower nesting as the renderer does: the follower
/// that draws resolves at depth 0, so a chain of seventeen followers, each
/// leader after the first inside the follower before it, still draws, and
/// moving the first leader damages it.
pub(crate) fn a_follower_at_the_nesting_limit_damages_what_it_draws() {
    let mut owner = SceneOwner::new();
    let mut writer = owner.writer();
    let root = writer.current().root();
    let (p, anchor) = (writer.mint_node(), writer.mint_node());
    let links: Vec<LayerLink> = (0..17).map(|_| LayerLink::new()).collect();
    let leader = |link: LayerLink| Layer::from(LeaderLayer::new(link, Size::new(10.0, 10.0)));
    let mut first = BoundaryContent::builder();
    first.push_effect(leader(links[0]), RenderId::new(1), 0);
    writer.push(publish(anchor, first.finish()));
    let mut holding = BoundaryContent::builder();
    holding.slot(anchor);
    writer.push(publish(p, holding.finish()));
    let version = writer.next_content_version().expect("a version");
    let mut top = BoundaryContent::builder();
    top.slot(p);
    for (ordinal, pair) in (0_u32..).step_by(2).zip(links.windows(2)) {
        top.push_effect(
            Layer::from(FollowerLayer::new(pair[0])),
            RenderId::new(2),
            ordinal,
        );
        top.push_effect(leader(pair[1]), RenderId::new(2), ordinal + 1);
        top.pop_effect();
        top.pop_effect();
    }
    top.push_effect(
        Layer::from(FollowerLayer::new(links[16])),
        RenderId::new(2),
        100,
    );
    top.chunk(chunk(2, version, Rect::from_xywh(0.0, 0.0, 20.0, 20.0)));
    top.pop_effect();
    writer.push(publish(root, top.finish()));
    writer.push(place(p, 100.0, 100.0));
    owner
        .apply(SURFACE)
        .expect("seventeen followers chained through their leaders");

    let mut writer = owner.writer();
    writer.push(place(p, 300.0, 300.0));
    owner.apply(SURFACE).expect("the first leader moves");
    let damage = rects(owner.patch_damage());
    assert!(
        covered(&damage, covering(100.0, 100.0, 120.0, 120.0)),
        "{damage:?}"
    );
    assert!(
        covered(&damage, covering(300.0, 300.0, 320.0, 320.0)),
        "{damage:?}"
    );
}

/// A clip on the leader's branch hides a follower under `LeaderClip::Hide`
/// without changing its transform: the follower's footprint is damaged.
pub(crate) fn a_clip_on_the_leaders_branch_that_hides_its_follower_damages_it() {
    let link = LayerLink::new();
    let mut owner = SceneOwner::new();
    let mut writer = owner.writer();
    let root = writer.current().root();
    let (p, leader, follower) = (writer.mint_node(), writer.mint_node(), writer.mint_node());
    let mut leading = BoundaryContent::builder();
    leading.push_effect(
        Layer::from(LeaderLayer::new(link, Size::new(10.0, 10.0))),
        RenderId::new(1),
        0,
    );
    writer.push(publish(leader, leading.finish()));
    let version = writer.next_content_version().expect("a version");
    let mut following = BoundaryContent::builder();
    following.push_effect(
        Layer::from(FollowerLayer::new(link).with_leader_clip(LeaderClip::Hide)),
        RenderId::new(2),
        0,
    );
    following.chunk(chunk(2, version, Rect::from_xywh(0.0, 0.0, 20.0, 20.0)));
    writer.push(publish(follower, following.finish()));
    let clipped = |clip: Rect<f64>| {
        let mut content = BoundaryContent::builder();
        content.push_effect(
            Layer::from(ClipRectLayer::new(clip, Clip::HardEdge)),
            RenderId::new(3),
            0,
        );
        content.slot(leader);
        content.finish()
    };
    writer.push(publish(p, clipped(Rect::from_xywh(0.0, 0.0, 50.0, 50.0))));
    let mut top = BoundaryContent::builder();
    top.slot(p);
    top.slot(follower);
    writer.push(publish(root, top.finish()));
    writer.push(place(p, 100.0, 100.0));
    owner
        .apply(SURFACE)
        .expect("a leader inside a clip, its follower at the root");

    let mut writer = owner.writer();
    writer.push(publish(
        p,
        clipped(Rect::from_xywh(200.0, 200.0, 50.0, 50.0)),
    ));
    owner
        .apply(SURFACE)
        .expect("the clip moves off the leader's anchor");
    let damage = rects(owner.patch_damage());
    assert!(
        covered(&damage, covering(100.0, 100.0, 120.0, 120.0)),
        "the hidden follower's pixels: {damage:?}"
    );
}

/// A follower under `LeaderClip::Hide` that a clip on its leader's branch
/// hides takes with it the leader inside it, and so another follower that
/// escapes clips: the second follower's footprint is damaged, though only
/// the first one's leader branch changed.
pub(crate) fn a_hidden_follower_hides_the_followers_of_the_leader_inside_it() {
    let (outer, inner) = (LayerLink::new(), LayerLink::new());
    let mut owner = SceneOwner::new();
    let mut writer = owner.writer();
    let root = writer.current().root();
    let (p, leader, hiding, escaping) = (
        writer.mint_node(),
        writer.mint_node(),
        writer.mint_node(),
        writer.mint_node(),
    );
    let mut leading = BoundaryContent::builder();
    leading.push_effect(
        Layer::from(LeaderLayer::new(outer, Size::new(10.0, 10.0))),
        RenderId::new(1),
        0,
    );
    writer.push(publish(leader, leading.finish()));
    let mut relay = BoundaryContent::builder();
    relay.push_effect(
        Layer::from(FollowerLayer::new(outer).with_leader_clip(LeaderClip::Hide)),
        RenderId::new(2),
        0,
    );
    relay.push_effect(
        Layer::from(LeaderLayer::new(inner, Size::new(10.0, 10.0))),
        RenderId::new(2),
        1,
    );
    writer.push(publish(hiding, relay.finish()));
    let version = writer.next_content_version().expect("a version");
    let mut following = BoundaryContent::builder();
    following.push_effect(Layer::from(FollowerLayer::new(inner)), RenderId::new(3), 0);
    following.chunk(chunk(3, version, Rect::from_xywh(0.0, 0.0, 20.0, 20.0)));
    writer.push(publish(escaping, following.finish()));
    let clipped = |clip: Rect<f64>| {
        let mut content = BoundaryContent::builder();
        content.push_effect(
            Layer::from(ClipRectLayer::new(clip, Clip::HardEdge)),
            RenderId::new(4),
            0,
        );
        content.slot(leader);
        content.finish()
    };
    writer.push(publish(p, clipped(Rect::from_xywh(0.0, 0.0, 50.0, 50.0))));
    let mut top = BoundaryContent::builder();
    top.slot(p);
    top.slot(hiding);
    top.slot(escaping);
    writer.push(publish(root, top.finish()));
    writer.push(place(p, 100.0, 100.0));
    owner
        .apply(SURFACE)
        .expect("a follower that hides, holding the leader of one that escapes");

    let mut writer = owner.writer();
    writer.push(publish(
        p,
        clipped(Rect::from_xywh(200.0, 200.0, 50.0, 50.0)),
    ));
    owner
        .apply(SURFACE)
        .expect("the clip moves off the first leader's anchor");
    let damage = rects(owner.patch_damage());
    assert!(
        covered(&damage, covering(100.0, 100.0, 120.0, 120.0)),
        "the escaping follower's pixels: {damage:?}"
    );
}

/// A filter under a shrinking ancestor spreads by the renderer's
/// device-pixel kernel, not by its local reach scaled down: a move of its
/// boundary damages where the kernel lands, even when the shrunk content
/// itself lies off the surface.
pub(crate) fn a_filter_under_a_shrinking_ancestor_damages_its_device_kernel() {
    let mut owner = SceneOwner::new();
    let mut writer = owner.writer();
    let root = writer.current().root();
    let filtered = writer.mint_node();
    let version = writer.next_content_version().expect("a version");
    let mut content = BoundaryContent::builder();
    content.push_effect(
        Layer::from(ImageFilterLayer::new(ImageFilter::Dilate { radius: 20.0 })),
        RenderId::new(1),
        0,
    );
    content.chunk(chunk(1, version, Rect::from_xywh(0.0, 0.0, 100.0, 100.0)));
    writer.push(publish(filtered, content.finish()));
    let mut top = BoundaryContent::builder();
    top.push_effect(
        Layer::from(TransformLayer::new(
            Matrix4::translation(-10.0, 100.0, 0.0) * Matrix4::scaling(0.01, 0.01, 1.0),
        )),
        RenderId::new(2),
        0,
    );
    top.slot(filtered);
    writer.push(publish(root, top.finish()));
    owner
        .apply(SURFACE)
        .expect("a dilated boundary under a shrink");

    let mut writer = owner.writer();
    writer.push(place(filtered, 10.0, 0.0));
    owner.apply(SURFACE).expect("the boundary moves");
    let damage = rects(owner.patch_damage());
    assert!(
        covered(&damage, covering(0.0, 90.0, 8.0, 110.0)),
        "the kernel's on-surface pixels: {damage:?}"
    );
}

/// A boundary of thirty thousand nested effects publishes and moves on a
/// thread of 256 KiB of stack: its footprint folds the chain without
/// recursion.
pub(crate) fn a_deep_effect_chain_applies_without_recursion() {
    const DEPTH: u32 = 30_000;
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut owner = SceneOwner::new();
            let mut writer = owner.writer();
            let root = writer.current().root();
            let deep = writer.mint_node();
            let version = writer.next_content_version().expect("a version");
            let mut content = BoundaryContent::builder();
            for ordinal in 0..DEPTH {
                content.push_effect(
                    Layer::from(OffsetLayer::new(Offset::new(0.001, 0.0))),
                    RenderId::new(1),
                    ordinal,
                );
            }
            content.chunk(chunk(1, version, Rect::from_ltrb(0.0, 0.0, 10.0, 10.0)));
            writer.push(publish(deep, content.finish()));
            let mut top = BoundaryContent::builder();
            top.slot(deep);
            writer.push(publish(root, top.finish()));
            owner.apply(SURFACE).expect("a deep chain");

            let mut writer = owner.writer();
            writer.push(place(deep, 50.0, 0.0));
            owner.apply(SURFACE).expect("the deep boundary moves");
            let damage = rects(owner.patch_damage());
            assert!(
                covered(&damage, covering(30.0, 0.0, 40.0, 10.0)),
                "the chain's offsets carried: {damage:?}"
            );
            assert!(
                covered(&damage, covering(80.0, 0.0, 90.0, 10.0)),
                "and the placement: {damage:?}"
            );
        })
        .expect("a test thread")
        .join()
        .expect("no stack overflow");
}

/// A clip around a filter under a shrinking effect bounds what the filter's
/// device kernel draws, not the shrunk content alone: moving the boundary
/// damages the clipped dilation.
pub(crate) fn a_clip_over_a_shrunk_filter_keeps_its_device_kernel() {
    let mut owner = SceneOwner::new();
    let mut writer = owner.writer();
    let root = writer.current().root();
    let filtered = writer.mint_node();
    let version = writer.next_content_version().expect("a version");
    let mut content = BoundaryContent::builder();
    content.push_effect(
        Layer::from(ClipRectLayer::new(
            Rect::from_ltrb(80.0, 80.0, 90.0, 90.0),
            Clip::HardEdge,
        )),
        RenderId::new(1),
        0,
    );
    content.push_effect(
        Layer::from(TransformLayer::new(Matrix4::scaling(0.01, 0.01, 1.0))),
        RenderId::new(1),
        1,
    );
    content.push_effect(
        Layer::from(ImageFilterLayer::new(ImageFilter::Dilate { radius: 20.0 })),
        RenderId::new(1),
        2,
    );
    content.chunk(chunk(
        1,
        version,
        Rect::from_ltrb(10_000.0, 10_000.0, 10_100.0, 10_100.0),
    ));
    writer.push(publish(filtered, content.finish()));
    let mut top = BoundaryContent::builder();
    top.slot(filtered);
    writer.push(publish(root, top.finish()));
    owner
        .apply(SURFACE)
        .expect("a dilation clipped under a shrink");

    let mut writer = owner.writer();
    writer.push(place(filtered, 5.0, 0.0));
    owner.apply(SURFACE).expect("the boundary moves");
    let damage = rects(owner.patch_damage());
    assert!(
        covered(&damage, covering(80.0, 80.0, 95.0, 90.0)),
        "the clipped dilation at both placements: {damage:?}"
    );
}

/// The same clip over a shrunk filter when the filter arrives in a slotted
/// child: growing the parent's bounds keeps the device kernel through the
/// clip, so moving the parent damages the clipped dilation.
pub(crate) fn a_clip_over_a_shrunk_slotted_filter_keeps_its_device_kernel() {
    let mut owner = SceneOwner::new();
    let mut writer = owner.writer();
    let root = writer.current().root();
    let (parent, child) = (writer.mint_node(), writer.mint_node());
    writer.push(publish(child, BoundaryContent::builder().finish()));
    let mut clipped = BoundaryContent::builder();
    clipped.push_effect(
        Layer::from(ClipRectLayer::new(
            Rect::from_ltrb(80.0, 80.0, 90.0, 90.0),
            Clip::HardEdge,
        )),
        RenderId::new(1),
        0,
    );
    clipped.push_effect(
        Layer::from(TransformLayer::new(Matrix4::scaling(0.01, 0.01, 1.0))),
        RenderId::new(1),
        1,
    );
    clipped.slot(child);
    writer.push(publish(parent, clipped.finish()));
    let mut top = BoundaryContent::builder();
    top.slot(parent);
    writer.push(publish(root, top.finish()));
    owner
        .apply(SURFACE)
        .expect("an empty child under a clip and a shrink");

    let mut writer = owner.writer();
    let version = writer.next_content_version().expect("a version");
    let mut filtered = BoundaryContent::builder();
    filtered.push_effect(
        Layer::from(ImageFilterLayer::new(ImageFilter::Dilate { radius: 20.0 })),
        RenderId::new(2),
        0,
    );
    filtered.chunk(chunk(
        2,
        version,
        Rect::from_ltrb(10_000.0, 10_000.0, 10_100.0, 10_100.0),
    ));
    writer.push(publish(child, filtered.finish()));
    owner.apply(SURFACE).expect("the child takes a dilation");

    let mut writer = owner.writer();
    writer.push(place(parent, 5.0, 0.0));
    owner.apply(SURFACE).expect("the parent moves");
    let damage = rects(owner.patch_damage());
    assert!(
        covered(&damage, covering(80.0, 80.0, 95.0, 90.0)),
        "the clipped dilation at both placements: {damage:?}"
    );
}

/// A filtered boundary's filter inputs are read only once the damage meets
/// it, and then in one fold of its content: fifty nested blurs over two
/// hundred chunks cost nothing when a node far away moves, and each
/// version's entries once when a chunk under them changes.
pub(crate) fn filter_inputs_are_read_once_the_damage_reaches_them() {
    const FILTERS: u32 = 50;
    const CHUNKS: u32 = 200;
    let mut owner = SceneOwner::new();
    let mut writer = owner.writer();
    let root = writer.current().root();
    let (filtered, mover) = (writer.mint_node(), writer.mint_node());
    let blurred = |writer: &mut flui_layer::SceneWriter<'_>, changed: f64| {
        let version = writer.next_content_version().expect("a version");
        let mut content = BoundaryContent::builder();
        for ordinal in 0..FILTERS {
            content.push_effect(
                Layer::from(ImageFilterLayer::new(ImageFilter::blur(2.0))),
                RenderId::new(1),
                ordinal,
            );
        }
        for index in 0..CHUNKS {
            let height = if index == 0 { changed } else { 10.0 };
            content.chunk(chunk(
                1,
                version,
                Rect::from_xywh(500.0, f64::from(index), 10.0, height),
            ));
        }
        content.finish()
    };
    let content = blurred(&mut writer, 10.0);
    writer.push(publish(filtered, content));
    let version = writer.next_content_version().expect("a version");
    let mut far = BoundaryContent::builder();
    far.chunk(chunk(2, version, Rect::from_xywh(0.0, 0.0, 10.0, 10.0)));
    writer.push(publish(mover, far.finish()));
    let mut top = BoundaryContent::builder();
    top.slot(filtered);
    top.slot(mover);
    writer.push(publish(root, top.finish()));
    owner
        .apply(SURFACE)
        .expect("nested blurs and a node far from them");

    let reads = owner.filter_input_reads();
    let mut writer = owner.writer();
    writer.push(place(mover, 0.0, 20.0));
    owner.apply(SURFACE).expect("the far node moves");
    assert_eq!(owner.filter_input_reads(), reads, "damage that misses them");

    let mut writer = owner.writer();
    let content = blurred(&mut writer, 12.0);
    writer.push(publish(filtered, content));
    owner.apply(SURFACE).expect("a chunk under the blurs grows");
    assert_eq!(
        owner.filter_input_reads() - reads,
        2 * u64::from(FILTERS + CHUNKS),
        "one fold of each version"
    );
}

/// Followers pair across a patch in one pass over a republished boundary's
/// links and none over a kept one: two thousand sibling followers read
/// nothing when another node moves, and each side's links once when their
/// boundary is republished.
pub(crate) fn sibling_followers_pair_in_one_pass_over_their_links() {
    const FOLLOWERS: u32 = 2_000;
    let links: Vec<LayerLink> = (0..4).map(|_| LayerLink::new()).collect();
    let crowd = |extra: u32| {
        let mut content = BoundaryContent::builder();
        for ordinal in 0..FOLLOWERS + extra {
            let link = links[usize::try_from(ordinal).expect("a small index") % links.len()];
            content.push_effect(
                Layer::from(FollowerLayer::new(link)),
                RenderId::new(1),
                ordinal,
            );
            content.pop_effect();
        }
        content.finish()
    };
    let mut owner = SceneOwner::new();
    let mut writer = owner.writer();
    let root = writer.current().root();
    let (crowded, mover) = (writer.mint_node(), writer.mint_node());
    writer.push(publish(crowded, crowd(0)));
    writer.push(publish(mover, BoundaryContent::builder().finish()));
    let mut top = BoundaryContent::builder();
    top.slot(crowded);
    top.slot(mover);
    writer.push(publish(root, top.finish()));
    owner
        .apply(SURFACE)
        .expect("two thousand sibling followers");

    let reads = owner.follower_pairing_reads();
    let mut writer = owner.writer();
    writer.push(place(mover, 5.0, 5.0));
    owner.apply(SURFACE).expect("another node moves");
    assert_eq!(owner.follower_pairing_reads(), reads, "a kept boundary");

    let mut writer = owner.writer();
    writer.push(publish(crowded, crowd(1)));
    owner.apply(SURFACE).expect("one follower more");
    assert_eq!(
        owner.follower_pairing_reads() - reads,
        u64::from(2 * FOLLOWERS + 1),
        "each side's links once"
    );
}

/// Filters spread by the sum of the kernels along a chain and the maximum
/// across siblings, a magnifier scaling what it takes: each stack, inside a
/// boundary under a 0.01 scale over 10 000..10 100, draws over the device
/// span given, and moving the boundary by one device pixel damages both
/// ends of it.
pub(crate) fn nested_filter_kernels_add_along_the_chain() {
    let dilate = || ImageFilter::Dilate { radius: 20.0 };
    let blur = || ImageFilter::blur(10.0);
    let magnify = || {
        ImageFilter::try_magnify(Point::new(10_050.0, 10_050.0), 2.0).expect("a finite magnifier")
    };
    // Outermost first; the renderer's span: a dilate grows 20 px, a blur of
    // sigma 10 grows ceil(sqrt(3) * 10) = 18 px, a 2x magnifier about the
    // centre doubles what it takes, half a source pixel of read and one of
    // rounding included. Three deep, so that the outermost filter's own
    // closure, which sees its input's kernel, does not stand in for the sum.
    let stacks: [(&str, Vec<ImageFilter>, (f64, f64)); 4] = [
        (
            "three dilates",
            vec![dilate(), dilate(), dilate()],
            (40.0, 161.0),
        ),
        (
            "dilate over two blurs",
            vec![dilate(), blur(), blur()],
            (44.0, 157.0),
        ),
        ("three blurs", vec![blur(), blur(), blur()], (46.0, 155.0)),
        (
            "two dilates over a magnifier over a dilate",
            vec![dilate(), dilate(), magnify(), dilate()],
            (18.0, 183.0),
        ),
    ];
    for (name, filters, (left, right)) in stacks {
        let mut owner = SceneOwner::new();
        let mut writer = owner.writer();
        let root = writer.current().root();
        let filtered = writer.mint_node();
        let version = writer.next_content_version().expect("a version");
        let mut content = BoundaryContent::builder();
        for (ordinal, filter) in (0_u32..).zip(filters) {
            content.push_effect(
                Layer::from(ImageFilterLayer::new(filter)),
                RenderId::new(1),
                ordinal,
            );
        }
        content.chunk(chunk(
            1,
            version,
            Rect::from_ltrb(10_000.0, 10_000.0, 10_100.0, 10_100.0),
        ));
        writer.push(publish(filtered, content.finish()));
        let mut top = BoundaryContent::builder();
        top.push_effect(
            Layer::from(TransformLayer::new(Matrix4::scaling(0.01, 0.01, 1.0))),
            RenderId::new(2),
            0,
        );
        top.slot(filtered);
        writer.push(publish(root, top.finish()));
        owner.apply(SURFACE).expect("a filter stack under a shrink");

        let mut writer = owner.writer();
        writer.push(place(filtered, 100.0, 0.0));
        owner.apply(SURFACE).expect("the boundary moves");
        let damage = rects(owner.patch_damage());
        assert!(
            covered(&damage, covering(left, 100.0, left + 1.0, 101.0)),
            "{name}: the old left end {left}: {damage:?}"
        );
        assert!(
            covered(&damage, covering(right, 100.0, right + 1.0, 101.0)),
            "{name}: the new right end {right}: {damage:?}"
        );
    }
}

/// A picture effect changes what its damage extent covers: one that fills
/// the canvas and then draws a small rect damages the whole surface when its
/// fill changes, though its bounds hold only the rect.
pub(crate) fn a_picture_effect_damages_its_damage_extent() {
    let filled = |color: Color| {
        let mut canvas = Canvas::new();
        canvas.draw_color(color, BlendMode::SrcOver);
        canvas.draw_rect(
            Rect::from_xywh(10.0, 10.0, 10.0, 10.0),
            &Paint::fill(Color::RED),
        );
        let mut content = BoundaryContent::builder();
        content.push_effect(
            Layer::from(PictureLayer::new(canvas.finish())),
            RenderId::new(1),
            0,
        );
        content.finish()
    };
    let mut owner = SceneOwner::new();
    let mut writer = owner.writer();
    let root = writer.current().root();
    let painted = writer.mint_node();
    writer.push(publish(painted, filled(Color::BLUE)));
    let mut top = BoundaryContent::builder();
    top.slot(painted);
    writer.push(publish(root, top.finish()));
    owner.apply(SURFACE).expect("a filled picture");

    let mut writer = owner.writer();
    writer.push(publish(painted, filled(Color::GREEN)));
    owner.apply(SURFACE).expect("the fill changes");
    let whole = covering(0.0, 0.0, 800.0, 600.0);
    match owner.patch_damage() {
        DamageRegion::Full(_) => {}
        DamageRegion::Region(region) => assert!(covered(region.rects(), whole), "{region:?}"),
        DamageRegion::Unchanged => panic!("the fill changes pixels"),
    }
}

/// Resolving nested followers costs their chains below the lowest common
/// ancestor: twelve followers, each with its leader beside it inside the one
/// before, resolve once in each version (24 resolutions), where both chains
/// taken from the surface resolve every enclosing follower again (531 440).
pub(crate) fn nested_followers_resolve_below_their_common_ancestor() {
    const LEVELS: usize = 12;
    let mut owner = SceneOwner::new();
    let mut writer = owner.writer();
    let root = writer.current().root();
    let mover = writer.mint_node();
    let version = writer.next_content_version().expect("a version");
    writer.push(publish(mover, BoundaryContent::builder().finish()));
    let mut top = BoundaryContent::builder();
    top.slot(mover);
    for level in 0..LEVELS {
        let link = LayerLink::new();
        let ordinal = u32::try_from(2 * level).expect("a small level");
        top.push_effect(
            Layer::from(LeaderLayer::with_offset(
                link,
                Size::new(10.0, 10.0),
                Offset::new(1.0, 1.0),
            )),
            RenderId::new(1),
            ordinal,
        );
        top.pop_effect();
        top.push_effect(
            Layer::from(FollowerLayer::new(link)),
            RenderId::new(1),
            ordinal + 1,
        );
    }
    top.chunk(chunk(1, version, Rect::from_xywh(0.0, 0.0, 20.0, 20.0)));
    writer.push(publish(root, top.finish()));
    owner.apply(SURFACE).expect("twelve nested followers");

    let resolved = owner.followers_resolved();
    let mut writer = owner.writer();
    writer.push(place(mover, 5.0, 5.0));
    owner.apply(SURFACE).expect("an unrelated move");
    let steps = owner.followers_resolved() - resolved;
    assert!(
        steps <= u64::try_from(4 * LEVELS).expect("a small count"),
        "{steps} resolutions for {LEVELS} levels"
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
            1 => {
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
    surface: ContentToken,
}

impl Oracle {
    fn new() -> Self {
        Self {
            differ: LayerDiffer::new(DamageMode::On { full_above: 2.0 }),
            ids: HashMap::new(),
            tokens: HashMap::new(),
            nodes: HashMap::new(),
            surface: ContentToken::mint(),
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
        // A constant surface boundary above the root, so a root placement is
        // a child's move the differ pairs, not an unpairable root.
        let surface = self.surface.clone();
        let mut tree = LayerTree::new(
            LayerNode::new(Layer::from(OffsetLayer::new(Offset::ZERO)))
                .with_boundary(self.id((root, 2, 0)), surface),
        );
        let top = tree.push_child(tree.root(), node(self, root, placement));
        let mut pending = vec![(root, top)];
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
