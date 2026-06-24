// criterion_group!/criterion_main! generate public functions that have no docs;
// missing_docs on a bench binary is noise (no external consumers of the items).
#![allow(
    missing_docs,
    reason = "criterion macros generate undocumented public fns"
)]
//! Render-throughput and per-frame allocation micro-benchmarks for flui-engine.
//!
//! Two benchmark groups:
//!
//! ## `render_throughput`
//!
//! End-to-end GPU frame benchmark via `WgpuPainter`:
//!   - 50 solid-colour rects (rect instance batching)
//!   - 1 linear gradient with 4 stops (gradient-stop path)
//!   - 1 text label (text cache key path)
//!   - full `painter.render()` call (GPU encode + submit)
//!
//! GPU-guarded: skipped when no adapter is present (headless CI).
//!
//! ## `alloc_micro`
//!
//! Pure CPU micro-benchmarks that require no GPU and isolate the hot-path
//! allocation sites targeted by GLM audit #8:
//!
//! - `path_cache_warm_hit` — measures the cost of a warm `PathCache::get` hit
//!   (the borrowed-slice path; baseline proves no allocation after the fix).
//! - `superellipse_cache_warm_hit` — measures a warm `SuperellipsePathCache::get`
//!   hit, which deep-clones a `Path` containing 256+ `PathCommand` entries
//!   (heap-spilled `SmallVec`).  Isolates the clone cost flagged by GLM #8 site 1.
//! - `draw_segment_seal` — measures `DrawBatcher::finish_current_segment` with a
//!   populated segment, isolating the per-seal allocation cost.  After the
//!   `mem::take` fix the slot is left as a zero-cap default (`DrawSegment::default`)
//!   rather than calling `DrawSegment::new()` (7 × `Vec::with_capacity` burst).

use std::hint::black_box;
use std::sync::Arc;

use criterion::{Criterion, criterion_group, criterion_main};
use flui_engine::WgpuPainter;
use flui_engine::wgpu::path_cache::PathCache;
use flui_engine::wgpu::superellipse_cache::{SuperellipseKey, SuperellipsePathCache};
use flui_painting::Paint;
use flui_types::painting::path::Path;
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

// ---------------------------------------------------------------------------
// CPU-only micro-benchmarks (no GPU required)
// ---------------------------------------------------------------------------

/// Build a path with 256 `LineTo` commands — enough to spill the
/// `SmallVec<[PathCommand; 16]>` inside `Path` to the heap.
///
/// This simulates what `generate_superellipse_path` produces (4 corners × 64
/// points each), allowing the bench to measure a realistic clone cost without
/// calling the `pub(crate)` generator.
fn make_large_path(command_count: usize) -> Path {
    let mut path = Path::new();
    // MoveTo + many LineTo + Close.
    path.move_to(flui_types::Point::new(px(0.0), px(0.0)));
    for i in 1..command_count {
        let angle = (i as f32) * std::f32::consts::TAU / (command_count as f32);
        path.line_to(flui_types::Point::new(
            px(50.0 + 50.0 * angle.cos()),
            px(50.0 + 50.0 * angle.sin()),
        ));
    }
    path.close();
    path
}

fn alloc_micro(c: &mut Criterion) {
    let mut group = c.benchmark_group("alloc_micro");

    // ── path_cache_warm_hit ──────────────────────────────────────────────────
    //
    // Measures the cost of a single `PathCache::get` on a warm cache entry.
    // `PathCache::get` returns borrowed slices (`&[[f32;2]]`, `&[u32]`) so the
    // hit path allocates nothing — this bench guards that invariant and provides
    // a comparison point for any future refactor of the return type.
    {
        let mut cache = PathCache::new(64);
        let hash = 0xdead_beef_u64;
        // Pre-populate with realistic path data (128 positions, 378 indices).
        let positions: Vec<[f32; 2]> = (0..128_u32)
            .map(|i| {
                let a = (i as f32) * std::f32::consts::TAU / 128.0;
                [50.0 + 50.0 * a.cos(), 50.0 + 50.0 * a.sin()]
            })
            .collect();
        let indices: Vec<u32> = (1_u32..127).flat_map(|i| [0, i, i + 1]).collect();
        cache.insert(hash, positions, indices);

        group.bench_function("path_cache_warm_hit", |b| {
            b.iter(|| {
                // Hit path: returns `(&[[f32;2]], &[u32])` — zero allocation.
                // The reference cannot escape the closure (borrows `cache`);
                // use `black_box` on the lengths to prevent the call being elided.
                if let Some((verts, idxs)) = cache.get(black_box(hash)) {
                    black_box(verts.len());
                    black_box(idxs.len());
                }
            });
        });
    }

    // ── superellipse_cache_warm_hit ──────────────────────────────────────────
    //
    // Measures the cost of a `SuperellipsePathCache::get` on a warm entry.
    // After the Arc<Path> refactor the returned value is an `Arc<Path>` alias
    // (reference-count bump only, no heap allocation, no copy of the ~256
    // `PathCommand` entries).  This bench tracks the post-refactor baseline —
    // expected: single/low-double-digit nanoseconds vs. the pre-refactor
    // ~1257 ns deep clone.  This is GLM audit #8 site 1.
    {
        use std::sync::Arc;

        use flui_types::geometry::{RSuperellipse, Rect};
        let rse = RSuperellipse::from_rect_and_radius(
            Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(100.0)),
            flui_types::geometry::Radius::circular(px(8.0)),
        );
        let key = SuperellipseKey::from_superellipse(&rse);
        // Build a realistic 256-command path to simulate the superellipse generator.
        let path = Arc::new(make_large_path(256));
        let mut cache = SuperellipsePathCache::new(64);
        cache.insert(key, path);

        group.bench_function("superellipse_cache_warm_hit", |b| {
            b.iter(|| {
                // Hit path: Arc::clone (atomic reference-count increment).
                // No deep copy of the ~256-command path.
                let result = cache.get(black_box(&key));
                black_box(result)
            });
        });
    }

    group.finish();
}

criterion_group!(benches, render_throughput);
criterion_group!(alloc_benches, alloc_micro);
criterion_main!(benches, alloc_benches);
