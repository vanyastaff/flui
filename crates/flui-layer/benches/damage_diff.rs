// criterion_group!/criterion_main! generate public functions that have no docs;
// missing_docs on a bench binary is noise (no external consumers of the items).
//! What the damage producer costs per frame: `LayerDiffer::diff` over a scene
//! of 1,000 stamped boundaries, each an `OffsetLayer` holding one picture.
//!
//! - `unchanged`: every token as the previous frame had it — the walk and the
//!   comparison with nothing to damage.
//! - `one_changed`: one boundary minted a new token.
//! - `off`: the same scene with `DamageMode::Off`, which must not walk.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use flui_foundation::RenderId;
use flui_foundation::geometry::{Matrix4, Offset, Rect};
use flui_layer::{
    ContentToken, DamageMode, Layer, LayerDiffer, LayerNode, LayerTree, OffsetLayer, PictureLayer,
    Scene, TransformLayer,
};
use flui_painting::{Canvas, Paint, styling::Color};

const BOUNDARIES: usize = 1_000;
const SURFACE: (u32, u32) = (1920, 1080);

fn scene(root: &ContentToken, tokens: &[ContentToken]) -> Scene {
    let mut tree = LayerTree::new(
        LayerNode::new(Layer::from(TransformLayer::new(Matrix4::scaling(
            2.0, 2.0, 1.0,
        ))))
        .with_boundary(RenderId::new(1), root.clone()),
    );
    let root_id = tree.root();
    for (i, token) in tokens.iter().enumerate() {
        let at = Offset::new((i % 40) as f64 * 24.0, (i / 40) as f64 * 20.0);
        let boundary = tree.push_child(
            root_id,
            LayerNode::new(Layer::from(OffsetLayer::new(at)))
                .with_boundary(RenderId::new(i + 2), token.clone()),
        );
        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, 20.0, 16.0),
            &Paint::fill(Color::RED),
        );
        tree.push_child(boundary, Layer::from(PictureLayer::new(canvas.finish())));
    }
    Scene::new(tree)
}

fn damage_diff(c: &mut Criterion) {
    let root = ContentToken::mint();
    let tokens: Vec<ContentToken> = (0..BOUNDARIES).map(|_| ContentToken::mint()).collect();
    let mut changed = tokens.clone();
    changed[BOUNDARIES / 2] = ContentToken::mint();
    let before = scene(&root, &tokens);
    let after = scene(&root, &changed);

    let mut group = c.benchmark_group("damage_diff");
    group.bench_function("unchanged", |b| {
        let mut differ = LayerDiffer::default();
        differ.diff(&before, SURFACE);
        b.iter(|| black_box(differ.diff(black_box(&before), SURFACE)));
    });
    group.bench_function("one_changed", |b| {
        let mut differ = LayerDiffer::default();
        let mut flip = false;
        b.iter(|| {
            flip = !flip;
            let frame = if flip { &after } else { &before };
            black_box(differ.diff(black_box(frame), SURFACE))
        });
    });
    group.bench_function("off", |b| {
        let mut differ = LayerDiffer::new(DamageMode::Off);
        b.iter(|| black_box(differ.diff(black_box(&before), SURFACE)));
    });
    group.finish();
}

criterion_group!(benches, damage_diff);
criterion_main!(benches);
