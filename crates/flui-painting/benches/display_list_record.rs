//! Criterion benchmark: recording a display list.
//!
//! One `DrawCommand` per op, each stamped with the canvas's absolute
//! transform (`Canvas::record`), paints interned per recording, paths shared
//! by `Arc`. The two shapes bound what a frame's paint walk pays:
//! - `record_mixed_scene`: a representative run — rects, rounded rects, a
//!   clip scope, a stroked path, a translated group — per iteration.
//! - `draw_picture_replay`: re-stamping a recorded list under a transform,
//!   the cost of `Canvas::draw_picture` per command.
//!
//! Run with `cargo bench -p flui-painting --bench display_list_record`.

use std::hint::black_box;

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use flui_foundation::geometry::{Point, RRect, Rect};
use flui_painting::{Canvas, DisplayList, Paint};
use flui_painting::{paint::Path, styling::Color};

/// Ops recorded by one `record_scene` call.
// save, clip, rect, path, 16 × (rect + rrect), save, circle, line, restore, restore.
const OPS_PER_SCENE: u64 = 4 + 2 * 16 + 5;

fn record_scene(canvas: &mut Canvas, path: &Path) {
    let fill = Paint::fill(Color::rgb(30, 60, 90));
    let stroke = Paint::stroke(Color::rgb(240, 240, 240), 2.0);
    canvas.save();
    canvas.clip_rect(Rect::from_xywh(0.0, 0.0, 800.0, 600.0));
    canvas.draw_rect(Rect::from_xywh(0.0, 0.0, 800.0, 600.0), &fill);
    canvas.draw_path(path, &stroke);
    for i in 0..16 {
        let x = i as f64 * 48.0;
        canvas.draw_rect(Rect::from_xywh(x, 10.0, 40.0, 40.0), &fill);
        canvas.draw_rrect(
            RRect::from_rect_circular(Rect::from_xywh(x, 60.0, 40.0, 40.0), 8.0),
            &stroke,
        );
    }
    canvas.save();
    canvas.translate(100.0, 100.0);
    canvas.draw_circle(Point::new(0.0, 0.0), 20.0, &fill);
    canvas.draw_line(Point::new(0.0, 0.0), Point::new(50.0, 50.0), &stroke);
    canvas.restore();
    canvas.restore();
}

fn star_path() -> Path {
    let mut path = Path::new();
    path.move_to(Point::new(50.0, 0.0));
    for i in 1..5 {
        let angle = i as f64 * 4.0 * std::f64::consts::PI / 5.0;
        path.line_to(Point::new(
            50.0 + 50.0 * angle.sin(),
            50.0 - 50.0 * angle.cos(),
        ));
    }
    path.close();
    path
}

fn bench_record(c: &mut Criterion) {
    let path = star_path();
    let mut group = c.benchmark_group("display_list_record");
    group.throughput(Throughput::Elements(OPS_PER_SCENE));
    group.bench_function("record_mixed_scene", |b| {
        b.iter_batched(
            Canvas::new,
            |mut canvas| {
                record_scene(&mut canvas, black_box(&path));
                canvas.finish()
            },
            BatchSize::SmallInput,
        );
    });

    let picture: DisplayList = {
        let mut canvas = Canvas::new();
        record_scene(&mut canvas, &path);
        canvas.finish()
    };
    assert_eq!(picture.len() as u64, OPS_PER_SCENE);
    group.bench_function("draw_picture_replay", |b| {
        b.iter_batched(
            || {
                let mut canvas = Canvas::new();
                canvas.translate(10.0, 20.0);
                canvas
            },
            |mut canvas| {
                canvas.draw_picture(black_box(&picture));
                canvas.finish()
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

criterion_group!(benches, bench_record);
criterion_main!(benches);
