//! Engine benchmarks for flui-engine.
//!
//! Run with: cargo bench -p flui-engine

use criterion::{criterion_group, criterion_main, Criterion};

use flui_engine::debug::DebugEncoder;
use flui_engine::vertex::RectInstance;
use flui_layer::{CanvasLayer, Layer, Scene};
use flui_types::geometry::units::px;
use flui_types::geometry::Size;

fn bench_empty_scene_dispatch(c: &mut Criterion) {
    let scene = Scene::empty(Size::new(px(1920.0), px(1080.0)));
    let mut encoder = DebugEncoder::new();

    c.bench_function("empty_scene_dispatch", |b| {
        b.iter(|| {
            encoder.process_scene(&scene);
            encoder.reset();
        });
    });
}

fn bench_canvas_layer_scene_dispatch(c: &mut Criterion) {
    let scene = Scene::from_layer(
        Size::new(px(1920.0), px(1080.0)),
        Layer::Canvas(CanvasLayer::new()),
        0,
    );
    let mut encoder = DebugEncoder::new();

    c.bench_function("canvas_layer_scene_dispatch", |b| {
        b.iter(|| {
            encoder.process_scene(&scene);
            encoder.reset();
        });
    });
}

fn bench_debug_encoder_reset(c: &mut Criterion) {
    let mut encoder = DebugEncoder::new();
    // Process a scene so there is state to reset
    let scene = Scene::from_layer(
        Size::new(px(1920.0), px(1080.0)),
        Layer::Canvas(CanvasLayer::new()),
        0,
    );
    encoder.process_scene(&scene);

    c.bench_function("debug_encoder_reset", |b| {
        b.iter(|| {
            encoder.reset();
        });
    });
}

fn bench_shape_batcher_1000_rects(c: &mut Criterion) {
    use flui_engine::batchers::shapes::ShapeBatcher;

    c.bench_function("shape_batcher_1000_rects", |b| {
        let mut batcher = ShapeBatcher::new();
        b.iter(|| {
            for i in 0..1000 {
                let x = (i % 50) as f32 * 20.0;
                let y = (i / 50) as f32 * 20.0;
                batcher.add_rect(
                    x,
                    y,
                    18.0,
                    18.0,
                    [1.0, 0.0, 0.0, 1.0],
                    [0.0; 4],
                    [1.0, 0.0, 0.0, 1.0],
                );
            }
            batcher.clear();
        });
    });
}

fn bench_state_stack_operations(c: &mut Criterion) {
    use flui_engine::frame::state_stack::StateStack;
    use glam::Mat4;

    c.bench_function("state_stack_100_push_pop", |b| {
        let mut stack = StateStack::new();
        let transform = Mat4::from_translation(glam::Vec3::new(10.0, 20.0, 0.0));
        b.iter(|| {
            for _ in 0..100 {
                stack.transform.push(transform);
            }
            for _ in 0..100 {
                stack.transform.pop();
            }
        });
    });
}

fn bench_text_cache_lookup(c: &mut Criterion) {
    use flui_engine::text::cache::{ShapeCache, TextCacheKey};

    let mut cache = ShapeCache::<String>::new(1024, 120);
    // Pre-populate
    for i in 0..500 {
        let key = TextCacheKey::new(&format!("text_{}", i), 14.0, "sans-serif", 400);
        cache.insert(key, format!("shaped_{}", i), 0);
    }

    let lookup_key = TextCacheKey::new("text_250", 14.0, "sans-serif", 400);

    c.bench_function("text_cache_lookup", |b| {
        b.iter(|| {
            cache.get(&lookup_key, 1);
        });
    });
}

fn bench_rect_instance_create(c: &mut Criterion) {
    c.bench_function("rect_instance_create", |b| {
        b.iter(|| RectInstance::rect([10.0, 20.0, 100.0, 50.0], [1.0, 0.0, 0.0, 1.0]));
    });
}

criterion_group!(
    benches,
    bench_empty_scene_dispatch,
    bench_canvas_layer_scene_dispatch,
    bench_debug_encoder_reset,
    bench_shape_batcher_1000_rects,
    bench_state_stack_operations,
    bench_text_cache_lookup,
    bench_rect_instance_create,
);
criterion_main!(benches);
