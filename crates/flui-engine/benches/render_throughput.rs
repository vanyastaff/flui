// criterion_group!/criterion_main! generate public functions that have no docs;
// missing_docs on a bench binary is noise (no external consumers of the items).
#![allow(
    missing_docs,
    reason = "criterion macros generate undocumented public fns"
)]
//! End-to-end render-throughput benchmark for flui-engine.
//!
//! Measures the CPU-side cost of a representative frame through `WgpuPainter`:
//!   - command encoding for ~50 rects
//!   - a linear gradient with 4 colour stops
//!   - a text label
//!   - the full `painter.render()` call (GPU encode + submit)
//!
//! # GPU-availability guard
//!
//! Device creation is attempted once at startup. If no GPU is available
//! (common in headless CI without a software rasteriser) the process prints
//! a diagnostic and exits cleanly so the suite still "passes" in compile-only
//! CI jobs (`cargo bench --no-run`). The bench runs only where a GPU exists.
//!
//! # Micro-benchmark scope note — DEFERRED
//!
//! The hottest CPU micro-functions touched by the Phase-1 perf work —
//! `build_gradient_stops` and `RichTextCacheKey::new` — are `pub(crate)` /
//! private and are not accessible from an external `benches/` crate. Widening
//! their visibility solely to bench them would pollute the public API surface
//! (api.md). They are therefore NOT benched here; this file reaches them
//! indirectly via the public `WgpuPainter` draw API, which is the correct
//! integration level for a baseline-regression bench. Micro-benches for those
//! paths are a follow-up task.

use std::hint::black_box;
use std::sync::Arc;

use criterion::{Criterion, criterion_group, criterion_main};
use flui_engine::WgpuPainter;
use flui_painting::Paint;
use flui_types::{Offset, geometry::px, painting::Shader, styling::Color};

// ---------------------------------------------------------------------------
// Platform backend selection (mirrors Renderer::select_backend)
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
const BACKENDS: wgpu::Backends = wgpu::Backends::DX12;
#[cfg(target_os = "macos")]
const BACKENDS: wgpu::Backends = wgpu::Backends::METAL;
#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
const BACKENDS: wgpu::Backends = wgpu::Backends::VULKAN;

// ---------------------------------------------------------------------------
// GPU setup helper
// ---------------------------------------------------------------------------

/// Attempt to acquire a headless wgpu device and queue.
///
/// Returns `None` when no adapter is available (CI without GPU, etc.).
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
            })
            .await
            .ok()?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("bench-device"),
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

// ---------------------------------------------------------------------------
// Gradient stop colours
//
// `Color::rgb` is `const` so these can live in a static slice, avoiding
// a heap allocation on every bench iteration. The gradient-stop SmallVec
// in the painter is built from the `Vec` we pass to `simple_linear`, so the
// allocation is once per `build_frame` call (not per criterion sample).
// ---------------------------------------------------------------------------

static GRADIENT_COLORS: &[Color] = &[
    Color::rgb(255, 0, 0),
    Color::rgb(255, 255, 0),
    Color::rgb(0, 255, 0),
    Color::rgb(0, 0, 255),
];

// ---------------------------------------------------------------------------
// Frame builder
// ---------------------------------------------------------------------------

/// Issue a representative draw list into `painter`:
///   - 50 solid-colour rects (exercises rect instance batching)
///   - 1 linear gradient rect with 4 stops (exercises gradient-stop path)
///   - 1 text call (exercises text cache key path)
///
/// All geometry fits inside an 800×600 viewport.
fn build_frame(painter: &mut WgpuPainter) {
    // 50 solid rects — 10 columns × 5 rows across the viewport.
    // Colours vary per rect so the compiler cannot constant-fold the loop.
    for i in 0_u32..50 {
        let col = (i % 10) as f32;
        let row = (i / 10) as f32;
        let x = col * 80.0;
        let y = row * 100.0;
        let rect = flui_types::Rect::from_ltrb(px(x), px(y), px(x + 70.0), px(y + 90.0));
        let hue = i as f32 / 50.0;
        let color = Color::from_rgba_f32_array([hue, 0.5, 1.0 - hue, 1.0]);
        let paint = Paint::fill(black_box(color));
        painter.rect(black_box(rect), &paint);
    }

    // 1 linear gradient (4 colour stops — exercises SmallVec<GradientStop>)
    let gradient_rect = flui_types::Rect::from_ltrb(px(0.0), px(500.0), px(800.0), px(600.0));
    let gradient_paint = Paint::fill(Color::WHITE).with_shader(Shader::simple_linear(
        Offset::new(px(0.0), px(500.0)),
        Offset::new(px(800.0), px(600.0)),
        GRADIENT_COLORS.to_vec(),
    ));
    painter.rect(black_box(gradient_rect), &gradient_paint);

    // 1 text label (exercises text buffer + cache-key path)
    let text_paint = Paint::fill(Color::WHITE);
    painter.text(
        black_box("Hello, flui bench!"),
        flui_types::Point::new(px(10.0), px(480.0)),
        24.0,
        &text_paint,
    );
}

// ---------------------------------------------------------------------------
// Benchmark
// ---------------------------------------------------------------------------

fn render_throughput(c: &mut Criterion) {
    let Some((device, queue)) = try_create_gpu() else {
        println!("skipping render benches: no GPU available");
        return;
    };

    // Rgba8UnormSrgb is universally supported for offscreen render targets.
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let (width, height) = (800_u32, 600_u32);

    // Offscreen render target — created once, reused across all iterations.
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("bench-target"),
        size: wgpu::Extent3d {
            width,
            height,
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

    // Build the painter once — shader compilation is a one-time cost that is
    // NOT part of the benchmark; it runs before `bench_function` is called.
    let mut painter = WgpuPainter::with_shared_device(
        Arc::clone(&device),
        Arc::clone(&queue),
        format,
        (width, height),
    );

    // Warm-up frame: ensures pipeline caches (path, text buffer, gradient-stop
    // SmallVec) are in steady state before criterion starts measurement.
    {
        build_frame(&mut painter);
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("bench-warmup"),
        });
        let _ = painter.render_to_view(&view, &mut enc);
        queue.submit([enc.finish()]);
        // wait_indefinitely() blocks until the most recent submission completes.
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
    }

    // Each iteration measures: fill draw-list → encode GPU commands → submit.
    // `device.poll(wait_indefinitely())` ensures the GPU has consumed the
    // commands so each measurement covers the full CPU-observable round-trip —
    // which is what the Phase-1 allocation-reduction work optimised.
    c.bench_function("painter_render_50rects_gradient_text", |b| {
        b.iter(|| {
            build_frame(&mut painter);
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("bench-frame"),
            });
            let result = painter.render_to_view(&view, &mut enc);
            queue.submit([enc.finish()]);
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            // black_box the result so the compiler cannot prove the render
            // call is a no-op and eliminate it.
            black_box(result)
        });
    });
}

criterion_group!(benches, render_throughput);
criterion_main!(benches);
