// criterion_group!/criterion_main! generate public functions that have no docs;
// missing_docs on a bench binary is noise (no external consumers of the items).
//! Render-throughput and per-frame allocation micro-benchmarks for flui-engine.
//!
//! Benchmark groups (the damage ones are documented on their functions):
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
//! - `draw_segment_seal` — measures `DrawBatcher::finish_current_segment` with a
//!   populated segment, isolating the per-seal allocation cost.  After the
//!   `mem::take` fix the slot is left as a zero-cap default (`DrawSegment::default`)
//!   rather than calling `DrawSegment::new()` (7 × `Vec::with_capacity` burst).

use std::hint::black_box;
use std::sync::Arc;

use criterion::{Criterion, criterion_group, criterion_main};
use flui_engine::PathCache;
use flui_engine::WgpuPainter;
use flui_foundation::geometry::Offset;
use flui_foundation::geometry::Rect;
use flui_painting::Paint;
use flui_painting::{paint::Shader, styling::Color};

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
                apply_limit_buckets: false,
            })
            .await
            .ok()?;

        eprintln!("benchmark adapter: {:?}", adapter.get_info());

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
fn build_frame(painter: &mut WgpuPainter, label: &std::sync::Arc<flui_painting::ShapedParagraph>) {
    // 50 solid rects — 10 columns × 5 rows across the viewport.
    // Colours vary per rect so the compiler cannot constant-fold the loop.
    for i in 0_u32..50 {
        let col = (i % 10) as f32;
        let row = (i / 10) as f32;
        let x = col * 80.0;
        let y = row * 100.0;
        let rect = flui_foundation::geometry::Rect::from_ltrb(
            f64::from(x),
            f64::from(y),
            f64::from(x + 70.0),
            f64::from(y + 90.0),
        );
        let hue = i as f32 / 50.0;
        let color = Color::from_rgba_f32_array([hue, 0.5, 1.0 - hue, 1.0]);
        let paint = Paint::fill(black_box(color));
        painter.draw_rect(black_box(rect), &paint);
    }

    // 1 linear gradient (4 colour stops — exercises SmallVec<GradientStop>)
    let gradient_rect = flui_foundation::geometry::Rect::from_ltrb(0.0, 500.0, 800.0, 600.0);
    let gradient_paint = Paint::fill(Color::WHITE).with_shader(Shader::simple_linear(
        Offset::new(0.0, 500.0),
        Offset::new(800.0, 600.0),
        GRADIENT_COLORS.to_vec(),
    ));
    painter.draw_rect(black_box(gradient_rect), &gradient_paint);

    // 1 text label (exercises the run placement + glyph-key path)
    painter.draw_paragraph(
        std::sync::Arc::clone(black_box(label)),
        flui_foundation::geometry::Point::new(10.0, 480.0),
        Color::WHITE,
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
    // The label is shaped once, as a widget's `TextPainter` would cache it:
    // the bench measures the engine, not the shaper.
    let spans = [("Hello, flui bench!".to_owned(), None)];
    let label = std::sync::Arc::new(
        flui_painting::TextContext::new(&flui_painting::FontCollection::new())
            .shape(&flui_painting::parley_text::ParagraphSpec {
                font_weight_adjustment: 0,
                spans: &spans,
                default_style: None,
                font_size: 24.0,
                max_width: None,
                min_width: 0.0,
                text_align: flui_painting::typography::TextAlign::Start,
                line_height: None,
                direction: flui_painting::typography::TextDirection::Ltr,
                max_lines: None,
                ellipsis: None,
            })
            .to_shaped(None),
    );
    let mut painter = WgpuPainter::with_shared_device(
        Arc::clone(&device),
        Arc::clone(&queue),
        format,
        (width, height),
    );

    // Warm-up frame: ensures pipeline caches (path, text buffer, gradient-stop
    // SmallVec) are in steady state before criterion starts measurement.
    {
        painter.begin_frame().expect("benchmark frame begins");
        build_frame(&mut painter, &label);
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("bench-warmup"),
        });
        painter
            .render_to_view(&view, &mut enc)
            .expect("warm frame encodes");
        queue.submit([enc.finish()]);
        painter.finish_frame();
        // wait_indefinitely() blocks until the most recent submission completes.
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
    }

    // Each iteration measures: fill draw-list → encode GPU commands → submit.
    // `device.poll(wait_indefinitely())` ensures the GPU has consumed the
    // commands so each measurement covers the full CPU-observable round-trip —
    // which is what the Phase-1 allocation-reduction work optimised.
    c.bench_function("painter_render_50rects_gradient_text", |b| {
        b.iter(|| {
            painter.begin_frame().expect("benchmark frame begins");
            build_frame(&mut painter, &label);
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("bench-frame"),
            });
            painter
                .render_to_view(&view, &mut enc)
                .expect("benchmark frame encodes");
            queue.submit([enc.finish()]);
            painter.finish_frame();
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            // black_box the result so the compiler cannot prove the render
            // call is a no-op and eliminate it.
        });
    });
}

// ---------------------------------------------------------------------------
// CPU-only micro-benchmarks (no GPU required)
// ---------------------------------------------------------------------------

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

    group.finish();
}

/// The public recording path on warm geometry, including colour/transform
/// reconstruction and the recording budget. No GPU encoding is timed.
fn warm_path_recording(c: &mut Criterion) {
    use flui_foundation::geometry::Point;
    use flui_painting::paint::path::Path;

    let Some((device, queue)) = try_create_gpu() else {
        return;
    };
    let mut path = Path::new();
    for i in 0..128 {
        let angle = f64::from(i) * std::f64::consts::TAU / 128.0;
        let point = Point::new(64.0 + 48.0 * angle.cos(), 64.0 + 48.0 * angle.sin());
        if i == 0 {
            path.move_to(point);
        } else {
            path.line_to(point);
        }
    }
    path.close();
    // Strokes use the normal tessellated path rather than allocating an SSAA
    // segment per draw. The benchmark measures the actual cache consumer.
    let paint = Paint::stroke(Color::RED, 2.0).with_anti_alias(false);
    let mut painter =
        WgpuPainter::with_shared_device(device, queue, wgpu::TextureFormat::Rgba8Unorm, (128, 128));
    painter.begin_frame().expect("warm path frame begins");
    painter.draw_path(&path, &paint);
    painter.finish_frame();
    let mut group = c.benchmark_group("warm_path_cpu_record");
    for count in [1, 64] {
        group.bench_function(count.to_string(), |b| {
            b.iter(|| {
                painter.begin_frame().expect("path recording frame begins");
                for _ in 0..count {
                    painter.draw_path(black_box(&path), black_box(&paint));
                }
                painter.finish_frame();
            });
        });
    }
    group.finish();
}

// ============================================================================
// damage_scissor — what a partial repaint would save
// ============================================================================

/// The same scene rasterised full-screen versus clipped to a small damage
/// rect, at a realistic surface size.
///
/// `DamageTracker` and the scissor that consumes it both exist and are wired
/// in `render_scene`; what is missing is a PRODUCER — `Renderer::mark_dirty`
/// has no production caller, so every frame calls `mark_full_repaint` and the
/// scissor never narrows. This measures what that costs, so the producer, when
/// it lands, has a baseline to be checked against rather than an assertion.
///
/// The layer counts are the axis that matters: with the damage rect fixed at
/// 0.8% of the surface, the scissored cost stays roughly flat while the full
/// cost grows with the fragment work, so the ratio shows how much of a frame
/// is pixels that did not change.
fn damage_scissor(c: &mut Criterion) {
    let Some((device, queue)) = try_create_gpu() else {
        println!("skipping damage benches: no GPU available");
        return;
    };

    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let (width, height) = (1920_u32, 1080_u32);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("damage-bench-target"),
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

    let mut painter = WgpuPainter::with_shared_device(
        Arc::clone(&device),
        Arc::clone(&queue),
        format,
        (width, height),
    );

    // Translucent full-surface rects: the shape that makes fragment work, and
    // the one a scissor can actually cull. A UI's own layers are smaller, so
    // treat these as an upper bound on the saving per layer, not a forecast.
    fn build(painter: &mut WgpuPainter, layers: u32, damage: Option<f32>, w: f32, h: f32) {
        painter.begin_frame().expect("benchmark frame begins");
        painter.save();
        if let Some(side) = damage {
            painter.clip_rect(
                Rect::from_xywh(0.0, 0.0, f64::from(side), f64::from(side)),
                flui_painting::paint::Clip::HardEdge,
            );
        }
        for i in 0..layers {
            let f = i as f32;
            painter.draw_rect(
                Rect::from_xywh(
                    f64::from(f * 2.0),
                    f64::from(f * 1.5),
                    f64::from(w),
                    f64::from(h),
                ),
                &Paint::fill(Color::rgba(0, 0, 255, 40)),
            );
        }
        painter.restore();
    }

    const VARIANTS: [(&str, Option<f32>); 2] = [("full", None), ("damage_128px", Some(128.0))];

    let mut group = c.benchmark_group("damage_scissor");
    for &layers in &[4_u32, 16, 64] {
        // Warm BOTH variants before timing EITHER. The damaged variant is
        // timed second, so anything that makes later work look faster — GPU
        // clock ramp, warmed caches — would flatter exactly the case whose
        // saving this benchmark reports. Warming both first removes the
        // cache half; the probe that produced these numbers also ran the
        // order reversed and agreed, which removes the ramp half.
        for (_, damage) in VARIANTS {
            for _ in 0..2 {
                build(&mut painter, layers, damage, width as f32, height as f32);
                let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("damage-warmup"),
                });
                painter
                    .render_to_view(&view, &mut enc)
                    .expect("warm frame encodes");
                queue.submit([enc.finish()]);
                painter.finish_frame();
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
            }
        }

        for (label, damage) in VARIANTS {
            group.bench_function(format!("{label}/{layers}"), |b| {
                b.iter(|| {
                    build(&mut painter, layers, damage, width as f32, height as f32);
                    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("damage-frame"),
                    });
                    painter
                        .render_to_view(&view, &mut enc)
                        .expect("benchmark frame encodes");
                    queue.submit([enc.finish()]);
                    painter.finish_frame();
                    let _ = device.poll(wgpu::PollType::wait_indefinitely());
                });
            });
        }
    }
    group.finish();
}

/// `damage_retained_target`: what a partial frame costs end to end once it
/// renders through the retained target (ADR-0087 §4), against the full
/// direct frame it replaces, at 1920x1080.
///
/// - `full_direct/N`: the whole frame straight into the "swapchain" texture.
/// - `partial_blit_128px/N`: the scissored clear and the N layers, scissored
///   to a 128 px damage, into the retained target, then the full-surface
///   blit onto the "swapchain" texture — the path a partial frame takes.
/// - `blit_only`: the blit alone, the fixed cost every retained frame adds.
/// - `candidate_copy_partial_blit_128px/N`: fresh candidate allocation, full
///   committed-image copy, partial draw and surface blit. This models the
///   transactional target cost with public painter operations; it does not
///   exercise the private FrameProtocol or DeviceDomain admission itself.
///
/// The retained target's memory is printed once: it is the per-window cost
/// ADR-0087 §4 asks to record. Bandwidth on tile-based mobile GPUs is not
/// measured here; a desktop adapter says nothing about it.
fn damage_retained_target(c: &mut Criterion) {
    let Some((device, queue)) = try_create_gpu() else {
        println!("skipping damage_retained_target: no GPU available");
        return;
    };
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let (width, height) = (1920_u32, 1080_u32);
    let target = |label: &str, usage: wgpu::TextureUsages| {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view)
    };
    let (_surface, surface_view) = target(
        "retained-bench-surface",
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let (retained, retained_view) = target(
        "retained-bench-target",
        wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
    );
    println!(
        "damage_retained_target: retained target {} bytes at {width}x{height}",
        u64::from(width) * u64::from(height) * 4
    );

    let mut painter = WgpuPainter::with_shared_device(
        Arc::clone(&device),
        Arc::clone(&queue),
        format,
        (width, height),
    );
    let mut offscreen =
        flui_engine::OffscreenRenderer::new(Arc::clone(&device), Arc::clone(&queue));

    fn record(painter: &mut WgpuPainter, layers: u32, damage: Option<f64>, w: f64, h: f64) {
        painter.begin_frame().expect("benchmark frame begins");
        painter.save();
        if let Some(side) = damage {
            let rect = Rect::from_xywh(0.0, 0.0, side, side);
            painter.clip_rect(rect, flui_painting::paint::Clip::HardEdge);
            // The partial frame's clear, as `damage::begin_partial` draws it.
            painter.draw_rect(rect.expand(1.0), &Paint::fill(Color::WHITE));
        }
        for i in 0..layers {
            let f = f64::from(i);
            painter.draw_rect(
                Rect::from_xywh(f * 2.0, f * 1.5, w, h),
                &Paint::fill(Color::rgba(0, 0, 255, 40)),
            );
        }
        painter.restore();
    }
    let finish = |encoder: wgpu::CommandEncoder| {
        queue.submit([encoder.finish()]);
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
    };
    let encoder = |label: &str| {
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(label) })
    };

    let mut group = c.benchmark_group("damage_retained_target");
    for &layers in &[4_u32, 16, 64] {
        // Warm both paths before timing either (see `damage_scissor`).
        for _ in 0..2 {
            record(
                &mut painter,
                layers,
                None,
                f64::from(width),
                f64::from(height),
            );
            let mut enc = encoder("retained-warmup");
            painter
                .render_to_view(&surface_view, &mut enc)
                .expect("warm direct frame encodes");
            finish(enc);
            painter.finish_frame();
            record(
                &mut painter,
                layers,
                Some(128.0),
                f64::from(width),
                f64::from(height),
            );
            let mut enc = encoder("retained-warmup");
            painter
                .render_to_view(&retained_view, &mut enc)
                .expect("warm retained frame encodes");
            finish(enc);
            painter.finish_frame();
            offscreen
                .blit_to_surface(&retained, &surface_view, format)
                .expect("retained blit");
        }
        group.bench_function(format!("full_direct/{layers}"), |b| {
            b.iter(|| {
                record(
                    &mut painter,
                    layers,
                    None,
                    f64::from(width),
                    f64::from(height),
                );
                let mut enc = encoder("retained-full");
                painter
                    .render_to_view(&surface_view, &mut enc)
                    .expect("benchmark frame encodes");
                finish(enc);
                painter.finish_frame();
            });
        });
        group.bench_function(format!("partial_blit_128px/{layers}"), |b| {
            b.iter(|| {
                record(
                    &mut painter,
                    layers,
                    Some(128.0),
                    f64::from(width),
                    f64::from(height),
                );
                let mut enc = encoder("retained-partial");
                painter
                    .render_to_view(&retained_view, &mut enc)
                    .expect("benchmark frame encodes");
                finish(enc);
                painter.finish_frame();
                offscreen
                    .blit_to_surface(&retained, &surface_view, format)
                    .expect("retained blit");
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
            });
        });
        // Matched queue/wait control for candidate_copy_partial_blit below:
        // one final wait, same recording and blit, no candidate allocation/copy.
        group.bench_function(format!("partial_queued_blit_128px/{layers}"), |b| {
            b.iter(|| {
                record(
                    &mut painter,
                    layers,
                    Some(128.0),
                    f64::from(width),
                    f64::from(height),
                );
                let mut enc = encoder("retained-queued-partial");
                painter
                    .render_to_view(&retained_view, &mut enc)
                    .expect("matched partial frame encodes");
                queue.submit([enc.finish()]);
                painter.finish_frame();
                offscreen
                    .blit_to_surface(&retained, &surface_view, format)
                    .expect("matched retained blit admitted");
                device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .expect("matched partial GPU round trip");
            });
        });
        // Texture::clone shares a GPU allocation, not a CPU pixel copy.
        // Candidate lifetime is queue-ordered here; this model excludes the
        // production target permit ledger and its admission/retirement costs.
        let mut committed = retained.clone();
        group.bench_function(format!("candidate_copy_partial_blit_128px/{layers}"), |b| {
            b.iter(|| {
                let (candidate, candidate_view) = target(
                    "retained-bench-candidate",
                    wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_SRC
                        | wgpu::TextureUsages::COPY_DST,
                );
                let mut copy = encoder("candidate-seed-copy");
                copy.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &committed,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: &candidate,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                );
                // Same queue ordering as candidate seeding before partial work.
                queue.submit([copy.finish()]);
                record(
                    &mut painter,
                    layers,
                    Some(128.0),
                    f64::from(width),
                    f64::from(height),
                );
                let mut enc = encoder("candidate-partial");
                painter
                    .render_to_view(&candidate_view, &mut enc)
                    .expect("benchmark frame encodes");
                queue.submit([enc.finish()]);
                painter.finish_frame();
                offscreen
                    .blit_to_surface(&candidate, &surface_view, format)
                    .expect("candidate blit admitted");
                device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .expect("candidate GPU round trip");
                committed = candidate;
            });
        });
        // Matched two-slot model: same copy/partial/blit, texture allocations outside timing.
        let (mut reusable, mut reusable_view) = target(
            "reusable-candidate",
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
        );
        let mut committed = retained.clone();
        let mut committed_view = retained_view.clone();
        group.bench_function(
            format!("reused_candidate_copy_partial_blit_128px/{layers}"),
            |b| {
                b.iter(|| {
                    let candidate = reusable.clone();
                    let candidate_view = reusable_view.clone();
                    let mut copy = encoder("candidate-seed-copy");
                    copy.copy_texture_to_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: &committed,
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        wgpu::TexelCopyTextureInfo {
                            texture: &candidate,
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        wgpu::Extent3d {
                            width,
                            height,
                            depth_or_array_layers: 1,
                        },
                    );
                    // Same queue ordering as candidate seeding before partial work.
                    queue.submit([copy.finish()]);
                    record(
                        &mut painter,
                        layers,
                        Some(128.0),
                        f64::from(width),
                        f64::from(height),
                    );
                    let mut enc = encoder("candidate-partial");
                    painter
                        .render_to_view(&candidate_view, &mut enc)
                        .expect("benchmark frame encodes");
                    queue.submit([enc.finish()]);
                    painter.finish_frame();
                    offscreen
                        .blit_to_surface(&candidate, &surface_view, format)
                        .expect("candidate blit admitted");
                    device
                        .poll(wgpu::PollType::wait_indefinitely())
                        .expect("candidate GPU round trip");
                    reusable = std::mem::replace(&mut committed, candidate);
                    reusable_view = std::mem::replace(&mut committed_view, candidate_view);
                });
            },
        );
    }
    group.bench_function("blit_only", |b| {
        b.iter(|| {
            offscreen
                .blit_to_surface(&retained, &surface_view, format)
                .expect("retained blit");
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
        });
    });
    group.finish();
}

/// CPU-observed record/encode/submit/completion costs on a warm device.
/// These timings are wall-clock round trips, not GPU timestamp measurements.
fn ordered_primitives(c: &mut Criterion) {
    let (device, queue) = try_create_gpu()
        .expect("ordered_primitives requires an available GPU; absence is not a benchmark result");
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ordered-primitives-target"),
        size: wgpu::Extent3d {
            width: 800,
            height: 600,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mut painter = WgpuPainter::with_shared_device(
        Arc::clone(&device),
        Arc::clone(&queue),
        format,
        (800, 600),
    );
    let solid = Paint::fill(Color::rgb(0, 0, 255));
    let linear = Paint::fill(Color::WHITE).with_shader(Shader::simple_linear(
        Offset::new(0.0, 0.0),
        Offset::new(800.0, 600.0),
        vec![Color::rgb(255, 0, 0), Color::rgb(255, 255, 0)],
    ));
    let radial = Paint::fill(Color::WHITE).with_shader(Shader::simple_radial(
        Offset::new(400.0, 300.0),
        500.0,
        vec![Color::rgb(0, 255, 0), Color::rgb(0, 255, 255)],
    ));
    let mut group = c.benchmark_group("ordered_primitives");
    for count in [32_u32, 256] {
        for pattern in ["solid", "solid_linear", "linear_radial"] {
            // Reuse paints so source Vec allocation is outside measured recording.
            // 96x96 quads on a 40px grid overlap across category boundaries.
            let draws: Vec<_> = (0..count)
                .map(|i| {
                    let rect = Rect::from_xywh(
                        f64::from(i % 16) * 40.0,
                        f64::from((i / 16) % 12) * 40.0,
                        96.0,
                        96.0,
                    );
                    let paint = match (pattern, i % 2) {
                        ("solid_linear", 1) | ("linear_radial", 0) => &linear,
                        ("linear_radial", _) => &radial,
                        _ => &solid,
                    };
                    (rect, paint)
                })
                .collect();
            let record = |painter: &mut WgpuPainter| {
                for &(rect, paint) in &draws {
                    painter.draw_rect(black_box(rect), black_box(paint));
                }
            };
            // Warm each workload's pipeline and allocation state explicitly.
            painter.begin_frame().expect("benchmark frame begins");
            record(&mut painter);
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            painter
                .render_to_view(&view, &mut encoder)
                .expect("warm ordered scene must encode");
            queue.submit([encoder.finish()]);
            device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("warm GPU completion must succeed");
            painter.finish_frame();

            group.bench_function(format!("{pattern}/{count}/cpu_record_drained"), |b| {
                b.iter_custom(|iterations| {
                    let mut measured = std::time::Duration::ZERO;
                    for _ in 0..iterations {
                        painter.begin_frame().expect("benchmark frame begins");
                        let started = std::time::Instant::now();
                        record(&mut painter);
                        measured += started.elapsed();
                        // Drain every recording; begin/finish alone do not discard arenas.
                        // Encoding, submission, completion and maintenance are excluded
                        // from the returned CPU recording duration.
                        let mut encoder = device
                            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
                        painter
                            .render_to_view(&view, &mut encoder)
                            .expect("record benchmark scene must drain");
                        queue.submit([encoder.finish()]);
                        device
                            .poll(wgpu::PollType::wait_indefinitely())
                            .expect("record benchmark completion must succeed");
                        painter.finish_frame();
                    }
                    measured
                });
            });
            group.bench_function(
                format!("{pattern}/{count}/cpu_observed_record_encode_submit_completion"),
                |b| {
                    b.iter(|| {
                        painter.begin_frame().expect("benchmark frame begins");
                        record(&mut painter);
                        let mut encoder = device
                            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
                        painter
                            .render_to_view(&view, &mut encoder)
                            .expect("ordered benchmark scene must encode");
                        queue.submit([encoder.finish()]);
                        device
                            .poll(wgpu::PollType::wait_indefinitely())
                            .expect("GPU completion must succeed");
                        painter.finish_frame();
                    });
                },
            );
        }
    }
    group.finish();
}

/// CPU recording plus encoding for repeated imported allocations. Completion
/// is drained outside measured time; this is not a GPU timestamp benchmark.
fn external_bindings(c: &mut Criterion) {
    use flui_engine::{
        ExternalAlpha, ExternalColorEncoding, ExternalSampling, ExternalTextureDescriptor,
    };
    use flui_painting::paint::{FilterQuality, TextureId};
    let Some((device, queue)) = try_create_gpu() else {
        return;
    };
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let make_texture = |label, usage, side| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let target = make_texture(
        "External bench target",
        wgpu::TextureUsages::RENDER_ATTACHMENT,
        256,
    );
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mut painter = WgpuPainter::with_shared_device(
        Arc::clone(&device),
        Arc::clone(&queue),
        format,
        (256, 256),
    );
    for id in 1..=16 {
        let texture = make_texture(
            "External bench allocation",
            wgpu::TextureUsages::TEXTURE_BINDING,
            16,
        );
        painter
            .external_texture_registry_mut()
            .register(
                TextureId::new(id),
                texture,
                ExternalTextureDescriptor {
                    sampling: ExternalSampling::Linear,
                    alpha: ExternalAlpha::Straight,
                    color: ExternalColorEncoding::EncodedSrgb,
                },
            )
            .expect("benchmark import");
    }
    let mut group = c.benchmark_group("external_bindings");
    for allocations in [1_u64, 16] {
        for count in [32_u32, 256] {
            group.bench_function(
                format!("allocations_{allocations}/draws_{count}/cpu_record_encode"),
                |b| {
                    b.iter_custom(|iterations| {
                        let mut measured = std::time::Duration::ZERO;
                        for _ in 0..iterations {
                            painter.begin_frame().expect("benchmark frame");
                            let mut encoder = device
                                .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
                            let started = std::time::Instant::now();
                            for i in 0..count {
                                painter.draw_texture(
                                    TextureId::new(1 + u64::from(i) % allocations),
                                    Rect::from_xywh(
                                        f64::from(i % 16) * 16.0,
                                        f64::from((i / 16) % 16) * 16.0,
                                        16.0,
                                        16.0,
                                    ),
                                    None,
                                    FilterQuality::Low,
                                    1.0,
                                );
                            }
                            painter
                                .render_to_view(&view, &mut encoder)
                                .expect("benchmark encode");
                            measured += started.elapsed();
                            // Trusted raw-submit path keeps the baseline and new lifecycle identical.
                            queue.submit([encoder.finish()]);
                            painter.finish_frame();
                            device
                                .poll(wgpu::PollType::wait_indefinitely())
                                .expect("benchmark completion");
                        }
                        measured
                    });
                },
            );
        }
    }
    group.finish();
}

criterion_group!(
    benches,
    render_throughput,
    ordered_primitives,
    external_bindings
);
criterion_group!(alloc_benches, alloc_micro, warm_path_recording);
criterion_group!(damage_benches, damage_scissor, damage_retained_target);
criterion_main!(benches, damage_benches, alloc_benches);
