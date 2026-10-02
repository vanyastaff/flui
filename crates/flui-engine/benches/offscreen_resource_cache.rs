//! End-to-end ordered layer effects: recording, GPU completion and RGBA readback.
//! Trees and the renderer are reused. Pixel preconditions run outside timing.
//! Old mask/backdrop implementations omitted work and are not equivalent baselines.
//! Optional individual capture quantiles: `FLUI_BENCH_LATENCY_SAMPLES=200`.
//! `FLUI_BENCH_LATENCY_FILTER=mask_depth8_512` restricts this separate report.
//! Quantiles include completion/readback, not pure GPU time. Adapter metadata,
//! internal pass counts and peak admitted bytes are not exposed by this API.
use criterion::{Criterion, criterion_group, criterion_main};
use flui_engine::HeadlessRenderer;
use flui_foundation::geometry::Rect;
use flui_layer::{
    BackdropFilterLayer, Layer, LayerTree, OffsetLayer, PictureLayer, ShaderMaskLayer,
};
use flui_painting::{BlendMode, Canvas, Paint, Shader, paint::ImageFilter, styling::Color};
use std::hint::black_box;

fn picture(side: u32, split: bool) -> Layer {
    let mut canvas = Canvas::new();
    let side = f64::from(side);
    canvas.draw_rect(
        Rect::from_xywh(0.0, 0.0, side, side),
        &Paint::fill(if split { Color::WHITE } else { Color::RED }).with_anti_alias(false),
    );
    if split {
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, side * 0.5, side),
            &Paint::fill(Color::BLACK).with_anti_alias(false),
        );
    }
    Layer::from(PictureLayer::new(canvas.finish()))
}
fn masks(side: u32, depth: u32) -> LayerTree {
    let mut tree = LayerTree::new(Layer::from(OffsetLayer::zero()));
    let mut parent = tree.root();
    for _ in 0..depth {
        parent = tree.push_child(
            parent,
            Layer::from(ShaderMaskLayer::new(
                Shader::solid(Color::rgba(255, 255, 255, 128)),
                BlendMode::DstIn,
                Rect::from_xywh(0.0, 0.0, f64::from(side), f64::from(side)),
            )),
        );
    }
    tree.push_child(parent, picture(side, false));
    tree
}
fn backdrops(side: u32, anisotropic: bool) -> LayerTree {
    let mut tree = LayerTree::new(picture(side, true));
    for inset in [4.0, 8.0] {
        tree.push_child(
            tree.root(),
            Layer::from(BackdropFilterLayer::new(
                ImageFilter::Blur {
                    sigma_x: 3.0,
                    sigma_y: if anisotropic { 0.0 } else { 3.0 },
                },
                BlendMode::Src,
                Rect::from_xywh(
                    inset,
                    inset,
                    f64::from(side) - 2.0 * inset,
                    f64::from(side) - 2.0 * inset,
                ),
            )),
        );
    }
    tree
}
fn pixel(bytes: &[u8], side: u32, x: u32, y: u32) -> [u8; 4] {
    let index = ((y * side + x) * 4) as usize;
    bytes[index..index + 4].try_into().expect("RGBA pixel")
}
fn report_latency(renderer: &HeadlessRenderer, tree: &LayerTree, side: u32, label: &str) {
    let Some(samples) = std::env::var("FLUI_BENCH_LATENCY_SAMPLES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|&count| count > 0 && count <= 10_000)
    else {
        return;
    };
    if std::env::var("FLUI_BENCH_LATENCY_FILTER").is_ok_and(|filter| !label.contains(&filter)) {
        return;
    }
    let mut durations = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = std::time::Instant::now();
        let bytes = renderer
            .render_layer_tree(tree, (side, side))
            .expect("latency capture");
        durations.push(start.elapsed());
        black_box(bytes);
    }
    durations.sort_unstable();
    let percentile =
        |percent: usize| durations[(samples * percent).div_ceil(100) - 1].as_secs_f64() * 1000.0;
    println!(
        "capture_completion_readback {label}: n={samples} p50={:.3}ms p95={:.3}ms p99={:.3}ms",
        percentile(50),
        percentile(95),
        percentile(99)
    );
}
fn captures(c: &mut Criterion) {
    let Ok(renderer) = pollster::block_on(HeadlessRenderer::new()) else {
        return;
    };
    let mut group = c.benchmark_group("layer_capture_completion_readback");
    group.sample_size(20);
    for side in [128, 512] {
        for depth in [0, 1, 4, 8] {
            let tree = masks(side, depth);
            let bytes = renderer
                .render_layer_tree(&tree, (side, side))
                .expect("warm layer capture");
            let actual = pixel(&bytes, side, side / 2, side / 2);
            let expected = (255.0 * (1.0 - (128.0_f64 / 255.0).powi(depth as i32))).round() as u8;
            assert!(
                actual[0] == 255
                    && actual[1].abs_diff(expected) <= 2
                    && actual[2].abs_diff(expected) <= 2
                    && actual[3] == 255,
                "nested masks must perform every opacity operation: {actual:?}, depth={depth}"
            );
            let label = format!("mask_depth{depth}_{side}");
            report_latency(&renderer, &tree, side, &label);
            group.bench_function(label, |b| {
                b.iter(|| {
                    black_box(
                        renderer
                            .render_layer_tree(black_box(&tree), (side, side))
                            .expect("ordered capture"),
                    )
                });
            });
        }
        for anisotropic in [false, true] {
            let tree = backdrops(side, anisotropic);
            let bytes = renderer
                .render_layer_tree(&tree, (side, side))
                .expect("warm backdrop capture");
            let edge = pixel(&bytes, side, side / 2, side / 2);
            assert!(
                edge[0] > 20
                    && edge[0] < 235
                    && edge[0] == edge[1]
                    && edge[1] == edge[2]
                    && edge[3] == 255,
                "overlapping filters must soften the source discontinuity: {edge:?}"
            );
            assert_eq!(pixel(&bytes, side, 1, side / 2), [0, 0, 0, 255]);
            let label = format!(
                "overlapping_backdrops_{}_{side}",
                if anisotropic {
                    "horizontal"
                } else {
                    "isotropic"
                }
            );
            report_latency(&renderer, &tree, side, &label);
            group.bench_function(label, |b| {
                b.iter(|| {
                    black_box(
                        renderer
                            .render_layer_tree(black_box(&tree), (side, side))
                            .expect("ordered capture"),
                    )
                });
            });
        }
    }
    group.finish();
}
#[path = "offscreen_resource_cache/clip_startup.rs"]
mod clip_startup;
criterion_group!(benches, captures, clip_startup::bench);
criterion_main!(benches);
