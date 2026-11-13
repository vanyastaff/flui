//! What an owner releases comes back emptied through `reclaim` and
//! `restock`, and its next applies and composited-layer updates write into it
//! instead of allocating, while every version someone still holds keeps what
//! it was applied with (ADR-0304 §7).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_foundation::RenderId;
use flui_foundation::SurfaceGeneration;
use flui_foundation::geometry::{Matrix4, Offset, Rect};
use flui_layer::{
    AnnotatedRegionLayer, Applied, BoundaryContent, Bounds, Chunk, ContentVersion, EntryIndex,
    Layer, Placement, SceneNodeId, SceneOp, SceneOwner, SceneVersion, SurfaceKey, TransformLayer,
};
use flui_painting::styling::Color;
use flui_painting::{Canvas, DisplayList, Paint};

const SURFACE: SurfaceKey = SurfaceKey {
    size: (800, 600),
    generation: SurfaceGeneration::ZERO,
};

/// Boundaries under the root: more than a table leaf holds, so a write
/// copies a branch as well as a leaf.
const NODES: usize = 12;

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

fn translate(dx: f64) -> Layer {
    Layer::from(TransformLayer::new(Matrix4::translation(dx, 0.0, 0.0)))
}

/// A scene of `NODES` boundaries, each one transform over one chunk, slotted
/// under the root.
struct Scene {
    owner: SceneOwner,
    nodes: Vec<SceneNodeId>,
    effect: EntryIndex,
    /// An entry that is no effect: the chunk under it.
    plain: EntryIndex,
}

impl Scene {
    fn new() -> Self {
        let mut owner = SceneOwner::new();
        let mut writer = owner.writer();
        let root = writer.current().root();
        let version = writer.next_content_version().expect("a version");
        let nodes: Vec<SceneNodeId> = (0..NODES).map(|_| writer.mint_node()).collect();
        let mut effect = None;
        let mut plain = None;
        for (index, &node) in nodes.iter().enumerate() {
            let mut content = BoundaryContent::builder();
            effect = Some(content.push_effect(translate(0.0), RenderId::new(index + 1), 0));
            plain = Some(content.chunk(chunk(index + 1, version)));
            content.pop_effect();
            writer.push(SceneOp::Publish {
                node,
                content: Arc::new(content.finish()),
            });
            writer.push(SceneOp::Place {
                node,
                placement: Placement::try_new(Offset::new(20.0 * index as f64, 0.0))
                    .expect("a finite offset"),
            });
        }
        let mut holder = BoundaryContent::builder();
        for &node in &nodes {
            holder.slot(node);
        }
        writer.push(SceneOp::Publish {
            node: root,
            content: Arc::new(holder.finish()),
        });
        drop(owner.apply(SURFACE).expect("the first frame applies"));
        Self {
            owner,
            nodes,
            effect: effect.expect("each boundary has its effect"),
            plain: plain.expect("each boundary has its chunk"),
        }
    }

    /// Moves boundary `at` by `dx` with a composited-layer update.
    fn slide(&mut self, at: usize, dx: f64) -> Applied {
        let node = self.nodes[at];
        let content = Arc::clone(
            self.owner
                .current()
                .node(node)
                .expect("a live boundary")
                .content(),
        );
        let mut writer = self.owner.writer();
        let moved = writer
            .with_effects(&content, [(self.effect, translate(dx))])
            .expect("the boundary's effect");
        writer.push(SceneOp::Publish {
            node,
            content: moved,
        });
        drop(content);
        self.owner.apply(SURFACE).expect("an effect update applies")
    }
}

fn picture(version: &SceneVersion) -> String {
    format!("{:?}", version.flatten())
}

/// A version the raster side, a pipeline or a probe still holds keeps every
/// node it was applied with while the owner reuses what it held alone, and
/// the current version equals one built by an owner that reclaims nothing.
#[test]
fn reclaimed_buffers_never_change_a_version_someone_holds() {
    let mut reclaiming = Scene::new();
    let mut control = Scene::new();
    let mut held: Vec<(SceneVersion, String)> = Vec::new();
    for frame in 0..48_usize {
        let at = (frame * 5) % NODES;
        let dx = frame as f64;
        let released = reclaiming.slide(at, dx).released;
        drop(control.slide(at, dx));
        let current = reclaiming.owner.current().clone();
        assert_eq!(
            picture(&current),
            picture(control.owner.current()),
            "frame {frame}: the reclaiming owner draws what the control does"
        );
        // Every third version is held for a while, as a raster side holds the
        // one it draws.
        if frame % 3 == 0 {
            held.push((current.clone(), picture(&current)));
        }
        if held.len() > 4 {
            held.remove(0);
        }
        let shown = (frame % 2 == 0).then(|| current.id());
        drop(current);
        reclaiming.owner.restock(released.reclaim());
        let completed = reclaiming.owner.completed(shown);
        reclaiming.owner.restock(completed.reclaim());
        for (version, drawn) in &held {
            assert_eq!(
                &picture(version),
                drawn,
                "frame {frame}: a held version kept what it was applied with"
            );
        }
    }
}

/// Once the raster side lets go of a version and a completion hands it back,
/// the owner holds its emptied nodes and contents, and the next update takes
/// them.
#[test]
fn an_update_writes_into_what_the_owner_reclaimed() {
    let mut scene = Scene::new();
    let first = scene.slide(0, 1.0);
    scene.owner.restock(first.released.reclaim());
    let raster = scene.owner.current().clone();
    let second = scene.slide(0, 2.0);
    scene.owner.restock(second.released.reclaim());
    assert_eq!(
        scene.owner.spares_len(),
        0,
        "the version the raster side holds is not taken apart"
    );
    drop(raster);
    let shown = scene.owner.current().id();
    let completed = scene.owner.completed(Some(shown));
    scene.owner.restock(completed.reclaim());
    let stocked = scene.owner.spares_len();
    assert!(
        stocked > 0,
        "the released version's nodes and the content it replaced come back"
    );
    let third = scene.slide(0, 3.0);
    assert!(
        scene.owner.spares_len() < stocked,
        "the update and its apply take what was reclaimed"
    );
    drop(third);
}

/// An annotation that records when its value drops.
#[derive(Debug)]
struct Dropped(Arc<AtomicBool>);

impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

fn annotation(flag: &Arc<AtomicBool>) -> Layer {
    Layer::from(AnnotatedRegionLayer::new(
        Rect::from_xywh(0.0, 0.0, 10.0, 10.0),
        Arc::new(Dropped(Arc::clone(flag))),
    ))
}

/// A refused composited-layer update, whether an index names no effect or
/// names one twice, runs none of the destructors of the layers it was handed
/// while the writer borrows the owner: the owner keeps them and drops them
/// when the next patch opens, where an annotation's `Drop` may reach the
/// owner again.
#[test]
fn a_refused_effect_update_drops_its_layers_outside_the_writer() {
    let mut scene = Scene::new();
    let content = Arc::clone(
        scene
            .owner
            .current()
            .node(scene.nodes[0])
            .expect("a live boundary")
            .content(),
    );
    let refusals = [
        ("an index that names no effect", scene.plain),
        ("an index named twice", scene.effect),
    ];
    for (case, second) in refusals {
        let (placed, refused) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        {
            let mut writer = scene.owner.writer();
            let outcome = writer.with_effects(
                &content,
                [
                    (scene.effect, annotation(&placed)),
                    (second, annotation(&refused)),
                ],
            );
            assert_eq!(outcome.err(), Some(second), "{case}: the update is refused");
            assert!(
                !placed.load(Ordering::SeqCst) && !refused.load(Ordering::SeqCst),
                "{case}: no layer drops while the writer borrows the owner"
            );
        }
        let _opened = scene.owner.writer();
        assert!(
            placed.load(Ordering::SeqCst) && refused.load(Ordering::SeqCst),
            "{case}: the next patch drops the layers"
        );
    }
}

/// Repaints the root with one run `width` wide through
/// [`SceneWriter::chunk`], applies it, and restocks what the apply released.
/// Returns the chunk, and the buffer `chunk` left behind.
fn repaint_root(owner: &mut SceneOwner, width: f64) -> (Arc<Chunk>, DisplayList) {
    let mut writer = owner.writer();
    let root = writer.current().root();
    let version = writer.next_content_version().expect("a version");
    let mut canvas = Canvas::new();
    canvas.draw_rect(
        Rect::from_xywh(0.0, 0.0, width, 10.0),
        &Paint::fill(Color::RED),
    );
    let mut commands = canvas.take_recording();
    let chunk = writer.chunk(RenderId::new(1), 0, version, &mut commands);
    let mut content = BoundaryContent::builder();
    content.chunk(Arc::clone(&chunk));
    writer.push(SceneOp::Publish {
        node: root,
        content: Arc::new(content.finish()),
    });
    let applied = owner.apply(SURFACE).expect("a repaint applies");
    owner.restock(applied.released.reclaim());
    (chunk, commands)
}

/// Shows the current version and restocks what its completion released.
fn show(owner: &mut SceneOwner) {
    let shown = owner.current().id();
    let completed = owner.completed(Some(shown));
    owner.restock(completed.reclaim());
}

/// Once a completion hands back the content a repaint replaced, the next
/// repaint's run is written into its chunk, commands and all, and hands back
/// that chunk's emptied buffer for the canvas to record into; a chunk someone
/// still holds is never taken.
#[test]
fn a_repaint_records_into_the_chunks_the_owner_reclaimed() {
    for held in [false, true] {
        let mut owner = SceneOwner::new();
        let (first, _) = repaint_root(&mut owner, 10.0);
        let first_at = Arc::as_ptr(&first);
        let kept = held.then(|| Arc::clone(&first));
        drop(first);
        drop(repaint_root(&mut owner, 20.0));
        show(&mut owner);
        let (third, left) = repaint_root(&mut owner, 30.0);
        assert_eq!(
            third.bounds(),
            Bounds::Finite(Rect::from_xywh(0.0, 0.0, 30.0, 10.0)),
            "held {held}: the chunk holds the new run"
        );
        if let Some(kept) = kept {
            assert_ne!(Arc::as_ptr(&third), first_at, "a held chunk is not reused");
            assert_eq!(
                kept.bounds(),
                Bounds::Finite(Rect::from_xywh(0.0, 0.0, 10.0, 10.0)),
                "a held chunk keeps what it recorded"
            );
        } else {
            assert_eq!(
                Arc::as_ptr(&third),
                first_at,
                "the run is written into the reclaimed chunk"
            );
            assert!(
                left.is_empty() && left.room() > 0,
                "the reclaimed chunk's emptied buffer comes back for the canvas"
            );
        }
    }
}
