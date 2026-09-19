//! Text-throughput benchmark: paragraphs recorded, rasterised, and submitted.
//!
//! The scene is a text-heavy list screen — `ROWS` shaped paragraphs
//! interleaved with a row background each, so every paragraph's glyphs must
//! be ordered against geometry — rendered end to end through `WgpuPainter`
//! (record → encode → submit → wait). The paragraphs are shaped once, outside
//! the timed loop, as a widget tree's `TextPainter` caches would have them:
//! what is measured is the engine's cost per glyph, not the shaper's.
//!
//! Two shapes:
//! - `steady_state`: every glyph is already in the atlas — the per-frame cost
//!   of text that did not change.
//! - `cold_atlas`: the paragraphs' glyphs are new to the atlas every frame —
//!   the raster + upload cost the first frame of a new screen pays.
//!
//! GPU-guarded: skipped when no adapter is present. Run with
//! `cargo bench -p flui-engine --bench text_throughput`.

use std::hint::black_box;
use std::sync::Arc;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use flui_engine::WgpuPainter;
use flui_painting::{Paint, TextLayout};
use flui_types::{
    Rect,
    geometry::{Point, px},
    styling::Color,
    typography::TextDirection,
};

#[cfg(target_os = "windows")]
const BACKENDS: wgpu::Backends = wgpu::Backends::DX12;
#[cfg(target_os = "macos")]
const BACKENDS: wgpu::Backends = wgpu::Backends::METAL;
#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
const BACKENDS: wgpu::Backends = wgpu::Backends::VULKAN;

const WIDTH: u32 = 800;
const HEIGHT: u32 = 1200;
/// Rows on screen: a dense list.
const ROWS: usize = 40;
const ROW_HEIGHT: f32 = 28.0;
const FONT_SIZE: f32 = 16.0;

fn try_create_gpu() -> Option<(Arc<wgpu::Device>, Arc<wgpu::Queue>)> {
    pollster::block_on(async {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: BACKENDS,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
            .ok()?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("text-bench-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                memory_hints: wgpu::MemoryHints::Performance,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                trace: wgpu::Trace::Off,
            })
            .await
            .ok()?;
        Some((Arc::new(device), Arc::new(queue)))
    })
}

/// One row's text: distinct per row so the scene carries a realistic glyph
/// variety rather than one cached word.
fn row_text(row: usize, salt: usize) -> String {
    format!(
        "Row {row:02} · item {salt:04} — quick brown fox jumps over the lazy dog {}",
        (row * 7 + salt) % 97
    )
}

fn shape(text: &str) -> Arc<TextLayout> {
    Arc::new(TextLayout::new(
        text,
        None,
        FONT_SIZE,
        Some(WIDTH as f32 - 32.0),
        None,
        TextDirection::Ltr,
    ))
}

/// Number of glyphs across `layouts`, for the throughput denominator.
fn glyph_count(layouts: &[Arc<TextLayout>]) -> u64 {
    layouts
        .iter()
        .map(|l| l.text().chars().filter(|c| !c.is_whitespace()).count() as u64)
        .sum()
}

fn record_frame(painter: &mut WgpuPainter, layouts: &[Arc<TextLayout>]) {
    let background = Paint::fill(Color::rgb(245, 245, 245));
    let stripe = Paint::fill(Color::rgb(230, 230, 230));
    painter.draw_rect(
        Rect::from_xywh(px(0.0), px(0.0), px(WIDTH as f32), px(HEIGHT as f32)),
        &background,
    );
    for (row, layout) in layouts.iter().enumerate() {
        let y = row as f32 * ROW_HEIGHT;
        if row % 2 == 0 {
            painter.draw_rect(
                Rect::from_xywh(px(0.0), px(y), px(WIDTH as f32), px(ROW_HEIGHT)),
                &stripe,
            );
        }
        painter.draw_paragraph(
            Arc::clone(layout),
            Point::new(px(16.0), px(y + 4.0)),
            Color::rgb(33, 33, 33),
        );
    }
}

fn submit_frame(
    painter: &mut WgpuPainter,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    view: &wgpu::TextureView,
) {
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("text-bench-frame"),
    });
    let result = painter.render_to_view(view, &mut enc);
    queue.submit([enc.finish()]);
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    black_box(result).expect("the frame must render");
}

fn text_throughput(c: &mut Criterion) {
    let Some((device, queue)) = try_create_gpu() else {
        println!("skipping text benches: no GPU available");
        return;
    };
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("text-bench-target"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut painter = WgpuPainter::with_shared_device(
        Arc::clone(&device),
        Arc::clone(&queue),
        format,
        (WIDTH, HEIGHT),
    );

    let steady: Vec<Arc<TextLayout>> = (0..ROWS).map(|row| shape(&row_text(row, 0))).collect();
    let glyphs = glyph_count(&steady);

    // Warm the pipelines and the atlas with the steady scene.
    record_frame(&mut painter, &steady);
    submit_frame(&mut painter, &device, &queue, &view);

    let mut group = c.benchmark_group("text_throughput");
    group.throughput(Throughput::Elements(glyphs));
    group.bench_function("steady_state", |b| {
        b.iter(|| {
            record_frame(&mut painter, black_box(&steady));
            submit_frame(&mut painter, &device, &queue, &view);
        });
    });

    // Cold atlas: a fresh salt per frame yields digits the atlas has not seen
    // at exactly these subpixel positions, while the letters stay warm — the
    // shape of a list that scrolled to new content. Layouts are shaped
    // outside the timing (`iter_batched` setup) so only raster + upload is
    // added to the steady cost.
    let mut salt = 1usize;
    group.bench_function("cold_rows", |b| {
        b.iter_batched(
            || {
                salt += 1;
                (0..ROWS)
                    .map(|row| shape(&row_text(row, salt * 131 + row)))
                    .collect::<Vec<_>>()
            },
            |layouts| {
                record_frame(&mut painter, &layouts);
                submit_frame(&mut painter, &device, &queue, &view);
            },
            criterion::BatchSize::SmallInput,
        );
    });
    group.finish();
}

criterion_group!(benches, text_throughput);
criterion_main!(benches);
