//! A cold process's first frame: the device request, the first capture (which
//! builds the context's pipelines and the painter) and a second capture of the
//! same tree, each timed once. One process is one sample, since a driver keeps
//! what it compiled for the process's life; run it in a loop for a
//! distribution, with the driver's own disk cache off for a cold start (Mesa:
//! `MESA_SHADER_CACHE_DISABLE=true`).
//!
//! `FLUI_BENCH_BUILD_MS` stands for the application's first build between the
//! device and the first frame (default 0): the pipeline warm-up overlaps it
//! (ADR-0305 §1), so the first capture waits only for what is left.
use std::time::{Duration, Instant};

use flui_engine::HeadlessRenderer;
use flui_foundation::geometry::{Point, Rect};
use flui_layer::{Layer, LayerTree, PictureLayer};
use flui_painting::{Canvas, Paint, styling::Color};

fn tree() -> LayerTree {
    let mut canvas = Canvas::new();
    canvas.draw_rect(
        Rect::from_xywh(0.0, 0.0, 64.0, 64.0),
        &Paint::fill(Color::RED),
    );
    canvas.draw_circle(Point::new(32.0, 32.0), 16.0, &Paint::fill(Color::BLUE));
    LayerTree::new(Layer::from(PictureLayer::new(canvas.finish())))
}

fn main() {
    let build = std::env::var("FLUI_BENCH_BUILD_MS")
        .ok()
        .and_then(|ms| ms.parse::<u64>().ok())
        .map_or(Duration::ZERO, Duration::from_millis);
    let start = Instant::now();
    let renderer = pollster::block_on(HeadlessRenderer::new()).expect("a GPU adapter");
    let device = start.elapsed();
    std::thread::sleep(build);
    let tree = tree();
    let first = Instant::now();
    let _pixels = renderer
        .render_layer_tree(&tree, (128, 128))
        .expect("the first capture");
    let first = first.elapsed();
    let second = Instant::now();
    let _pixels = renderer
        .render_layer_tree(&tree, (128, 128))
        .expect("the second capture");
    let second = second.elapsed();
    println!(
        "first_frame build_ms={} device_ms={:.2} first_capture_ms={:.2} second_capture_ms={:.2}",
        build.as_millis(),
        device.as_secs_f64() * 1e3,
        first.as_secs_f64() * 1e3,
        second.as_secs_f64() * 1e3,
    );
}
