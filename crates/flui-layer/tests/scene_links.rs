//! A scene version resolves its followers, leaders and annotations exactly as
//! the layer tree it flattens to does, so the paint side can read them off the
//! applied version without building a tree (ADR-0304 §8).

use std::sync::Arc;

use flui_foundation::RenderId;
use flui_foundation::geometry::{Matrix4, Offset, Point, Rect, Size};
use flui_foundation::inspect::SnapshotQuery;
use flui_layer::testing::inspect::structure;
use flui_layer::{
    AnnotatedRegionLayer, BoundaryContent, Chunk, ClipRectLayer, ContentVersion, FollowerLayer,
    Layer, LayerLink, LeaderClip, LeaderLayer, OffsetLayer, Placement, SceneNodeId, SceneOp,
    SceneOwner, SurfaceKey, TransformLayer, leader_origin, resolve_follower_transform,
};
use flui_painting::Alignment;
use flui_painting::paint::Clip;
use flui_painting::styling::Color;
use flui_painting::{Canvas, Paint};

const SURFACE: SurfaceKey = SurfaceKey {
    size: (800, 600),
    generation: flui_foundation::SurfaceGeneration::ZERO,
};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % n as u64).expect("below a usize")
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    fn unit(&mut self) -> f64 {
        f64::from(u32::try_from(self.next() % 1_000).expect("below 1 000")) / 1_000.0
    }

    fn signed(&mut self, scale: f64) -> f64 {
        (self.unit() * 2.0 - 1.0) * scale
    }
}

const LINKS: usize = 3;

/// One random effect, with each link led and followed at most once so a
/// layer of the flattened tree is named by its link alone.
fn effect(rng: &mut Rng, links: &[LayerLink], led: &mut [bool], followed: &mut [bool]) -> Layer {
    match rng.below(7) {
        0 => Layer::from(OffsetLayer::new(Offset::new(
            rng.signed(50.0),
            rng.signed(50.0),
        ))),
        1 => {
            let scale = 0.5 + rng.unit() * 1.5;
            Layer::from(TransformLayer::new(
                Matrix4::translation(rng.signed(40.0), rng.signed(40.0), 0.0)
                    * Matrix4::rotation_z(rng.signed(1.0))
                    * Matrix4::scaling(scale, scale, 1.0),
            ))
        }
        2 => {
            let (x, y) = (rng.signed(60.0), rng.signed(60.0));
            Layer::from(ClipRectLayer::new(
                Rect::from_xywh(x, y, 20.0 + rng.unit() * 80.0, 20.0 + rng.unit() * 80.0),
                Clip::HardEdge,
            ))
        }
        3 => {
            let link = rng.below(LINKS);
            if led[link] {
                return Layer::from(OffsetLayer::zero());
            }
            led[link] = true;
            Layer::from(LeaderLayer::with_offset(
                links[link],
                Size::new(10.0 + rng.unit() * 40.0, 10.0 + rng.unit() * 40.0),
                Offset::new(rng.signed(30.0), rng.signed(30.0)),
            ))
        }
        6 => {
            let value = Arc::new(u32::try_from(rng.below(1_000)).expect("below 1 000"));
            if rng.chance(30) {
                Layer::from(AnnotatedRegionLayer::sized_by_parent(value))
            } else {
                let (x, y) = (rng.signed(60.0), rng.signed(60.0));
                Layer::from(AnnotatedRegionLayer::new(
                    Rect::from_xywh(x, y, 10.0 + rng.unit() * 90.0, 10.0 + rng.unit() * 90.0),
                    value,
                ))
            }
        }
        _ => {
            let link = rng.below(LINKS);
            if followed[link] {
                return Layer::from(OffsetLayer::zero());
            }
            followed[link] = true;
            let clip = if rng.chance(50) {
                LeaderClip::Hide
            } else {
                LeaderClip::Escape
            };
            Layer::from(
                FollowerLayer::new(links[link])
                    .with_leader_clip(clip)
                    .with_show_when_unlinked(rng.chance(50))
                    .with_target_offset(Offset::new(rng.signed(20.0), rng.signed(20.0)))
                    .with_leader_anchor(Alignment::new(rng.signed(1.0), rng.signed(1.0)))
                    .with_follower_anchor(Alignment::new(rng.signed(1.0), rng.signed(1.0)))
                    .with_size(Size::new(10.0, 10.0)),
            )
        }
    }
}

fn chunk(owner: usize, version: ContentVersion) -> Arc<Chunk> {
    let mut canvas = Canvas::new();
    canvas.draw_rect(
        Rect::from_xywh(0.0, 0.0, 10.0, 10.0),
        &Paint::fill(Color::RED),
    );
    Arc::new(Chunk::new(
        RenderId::new(owner),
        0,
        version,
        canvas.finish(),
    ))
}

/// The first case whose version and flattened tree disagree on a
/// follower's transform or a leader's origin.
fn first_disagreement(cases: u64) -> Option<String> {
    for case in 0..cases {
        let mut rng = Rng(0x2545_F491_4F6C_DD1D ^ (case + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let links: Vec<LayerLink> = (0..LINKS).map(|_| LayerLink::new()).collect();
        let (mut led, mut followed) = ([false; LINKS], [false; LINKS]);
        let mut owner = SceneOwner::new();
        let mut writer = owner.writer();
        let count = 1 + rng.below(4);
        let mut nodes: Vec<SceneNodeId> = vec![writer.current().root()];
        let mut parents: Vec<usize> = vec![0];
        for index in 1..count {
            nodes.push(writer.mint_node());
            parents.push(rng.below(index));
        }
        let mut ops = Vec::new();
        for (index, &node) in nodes.iter().enumerate() {
            let mut content = BoundaryContent::builder();
            let mut open = 0;
            let children: Vec<usize> = (1..count).filter(|&c| parents[c] == index).collect();
            let mut pending = children.iter().copied().peekable();
            for _ in 0..rng.below(8) {
                match rng.below(4) {
                    0 | 1 => {
                        let layer = effect(&mut rng, &links, &mut led, &mut followed);
                        content.push_effect(layer, RenderId::new(index + 1), open);
                        open += 1;
                    }
                    2 => {
                        content.pop_effect();
                    }
                    _ => {
                        if let Some(child) = pending.next() {
                            content.slot(nodes[child]);
                        } else {
                            let version = writer.next_content_version().expect("a version");
                            content.chunk(chunk(index + 1, version));
                        }
                    }
                }
            }
            for child in pending {
                content.slot(nodes[child]);
            }
            ops.push(SceneOp::Publish {
                node,
                content: Arc::new(content.finish()),
            });
        }
        for &node in nodes.iter().skip(1).rev() {
            ops.push(SceneOp::Place {
                node,
                placement: Placement::try_new(Offset::new(rng.signed(80.0), rng.signed(80.0)))
                    .expect("a finite offset"),
            });
        }
        let (publishes, places) = ops.split_at(count);
        for op in publishes.iter().rev().chain(places).cloned() {
            writer.push(op);
        }
        owner.apply(SURFACE).expect("a random tree applies");
        let version = owner.current();
        let tree = version.flatten();

        let read = flui_foundation::inspect::snapshot(version, SnapshotQuery::default())
            .expect("a version has a root");
        let kinds: Vec<&str> = read.iter().map(|node| node.kind).collect();
        if kinds != structure(&tree) {
            return Some(format!(
                "case {case}: the version reads {kinds:?}, its tree {:?}",
                structure(&tree)
            ));
        }

        for &link in &links {
            let from_tree = leader_origin(&tree, link);
            let from_version = version.leader_origin(link);
            if !points_agree(from_tree, from_version) {
                return Some(format!(
                    "case {case}: leader origin {from_version:?} against the tree's {from_tree:?}"
                ));
            }
            let size_from_tree = tree
                .leaders()
                .filter(|(led, _)| *led == link)
                .filter_map(|(_, id)| Some(tree.get_layer(id)?.as_leader()?.size()))
                .last();
            if size_from_tree != version.leader_size(link) {
                return Some(format!(
                    "case {case}: leader size {:?} against the tree's {size_from_tree:?}",
                    version.leader_size(link)
                ));
            }
            let Some(at) = version.followers(link).next() else {
                continue;
            };
            let layer = tree
                .iter()
                .find(|(_, node)| {
                    node.layer()
                        .as_follower()
                        .is_some_and(|follower| follower.link() == link)
                })
                .map(|(id, _)| id);
            let from_tree = layer.and_then(|id| resolve_follower_transform(&tree, id));
            let from_version = version.follower_transform(at);
            if !matrices_agree(from_tree, from_version) {
                return Some(format!(
                    "case {case}: follower transform {from_version:?} against the tree's \
                     {from_tree:?}"
                ));
            }
        }
        for _ in 0..8 {
            let point = Point::new(rng.signed(150.0), rng.signed(150.0));
            let from_tree = tree.find_annotation::<u32>(point).map(|value| *value);
            let from_version = version.find_annotation::<u32>(point).map(|value| *value);
            if from_tree != from_version {
                return Some(format!(
                    "case {case}: annotation at {point:?} {from_version:?} against the tree's \
                     {from_tree:?}"
                ));
            }
        }
    }
    None
}

fn matrices_agree(a: Option<Matrix4>, b: Option<Matrix4>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.m.iter()
                .zip(b.m.iter())
                .all(|(x, y)| (x - y).abs() <= 1e-9)
        }
        _ => false,
    }
}

fn points_agree(a: Option<Point<f64>>, b: Option<Point<f64>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => (a.x - b.x).abs() <= 1e-9 && (a.y - b.y).abs() <= 1e-9,
        _ => false,
    }
}

/// Random trees of boundaries, transforms, clips, annotated regions, leaders
/// and followers
/// under either leader clip: every follower's transform, every leader's
/// origin and the topmost annotation at random points read off the version
/// equal what the flattened tree resolves, and the version's layer snapshot
/// reads the tree's kinds in pre-order.
pub(crate) fn a_version_resolves_links_and_annotations_as_its_flattened_tree_does() {
    if let Some(disagreement) = first_disagreement(5_000) {
        panic!("{disagreement}");
    }
}
