// criterion_group!/criterion_main! generate public functions that have no docs;
// missing_docs on a bench binary is noise (no external consumers of the items).
#![allow(
    missing_docs,
    reason = "criterion macros generate undocumented public fns"
)]
//! Criterion benchmarks for `OffscreenRenderer` resource-caching hygiene.
//!
//! ## Purpose
//!
//! These benches isolate the **CPU-side overhead** of `render_masked` and
//! `render_blur` — specifically the cost of the per-call GPU resource creation
//! that was eliminated by audit findings #2 and #3:
//!
//! - Finding #2: `render_masked` created one `wgpu::Sampler` and one
//!   fullscreen-quad `wgpu::Buffer` on every call.  Both are invariant.
//! - Finding #3: `render_blur` created the same sampler + VB on every call,
//!   plus `2 × iterations` (up to 10) `create_buffer_init` calls for
//!   `BlurParams` uniform buffers.  All of these are now cached or
//!   write-updated.
//!
//! ## What is measured
//!
//! Each benchmark runs a tight loop of CPU-observed wall time that covers:
//!
//! - the call into `render_masked` / `render_blur` (uniform buffer alloc,
//!   bind group creation, command encoding, queue submit)
//! - `device.poll(wait_indefinitely())` — blocks until the GPU consumes the
//!   submitted work, so the measurement captures the full CPU-observable
//!   round-trip and GPU stalls are visible
//!
//! The benchmark does NOT capture pure GPU execution time (shader runtime,
//! memory bandwidth) — that requires timestamp queries.  The measured delta
//! is the reduction in CPU-observable latency from eliminating allocation calls.
//!
//! ## GPU guard
//!
//! Skipped automatically when no adapter is available (headless CI without GPU).
//! On DX12 machines the bench runs unconditionally.
//!
//! ## Reproduction
//!
//! ```text
//! cargo bench -p flui-engine --bench offscreen_resource_cache
//! ```

use std::hint::black_box;
use std::sync::Arc;

use bytemuck::cast_slice;
use criterion::{Criterion, criterion_group, criterion_main};
use flui_engine::wgpu::OffscreenRenderer;
use flui_types::{
    Rect, Size,
    geometry::{Pixels, px},
    painting::{BlendMode, Shader},
    styling::Color,
};
use wgpu::util::DeviceExt as _;

// ---------------------------------------------------------------------------
// Backend selection — mirrors render_throughput.rs
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

/// Attempt to acquire a headless wgpu device + queue.
///
/// Returns `None` when no adapter is present (headless CI).
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
                label: Some("offscreen-bench-device"),
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
// Shared constants — invariant across both benchmarks
// ---------------------------------------------------------------------------

/// Side length in texels for all bench textures.
/// 256×256 gives the renderer a realistic-sized pass without large GPU memory
/// pressure on the bench machine.
const BENCH_SIDE_TEXELS: u32 = 256;

/// Side length as `f32` logical pixels.
///
/// Kept as a named literal (not derived via `as f32`) so the declaration is
/// the single source of truth and does not trigger cast lints.
const BENCH_SIDE_PX: f32 = 256.0;

// ---------------------------------------------------------------------------
// Source texture helper
// ---------------------------------------------------------------------------

/// Create a 256×256 source texture usable as a `render_masked` / `render_blur` input.
///
/// `TEXTURE_BINDING` lets the offscreen sampler read it;
/// `RENDER_ATTACHMENT` allows it to be used as a render target (required by
/// `render_blur` which writes into pooled textures at the same size).
/// `COPY_DST` is not required here but harmless and consistent with test usage.
fn make_source_texture(device: &wgpu::Device, format: wgpu::TextureFormat) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("bench-source"),
        size: wgpu::Extent3d {
            width: BENCH_SIDE_TEXELS,
            height: BENCH_SIDE_TEXELS,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}

// ---------------------------------------------------------------------------
// `render_masked` benchmark
// ---------------------------------------------------------------------------

/// Measures the per-call overhead of `OffscreenRenderer::render_masked`.
///
/// Each iteration:
/// 1. Calls `render_masked` with a solid-colour shader on a 256×256 input.
/// 2. Drops the returned `MaskedRenderResult` (returns the texture to the pool
///    so the pool stays warm across iterations).
/// 3. Polls until the GPU completes (ensures each measurement is a full round-trip).
///
/// The `OffscreenRenderer` is constructed once before the loop, so pipeline
/// compilation is excluded from the measurement.  The formerly per-call GPU
/// allocations (sampler, vertex buffer) are now constructor-time — they are
/// therefore absent from the measured iterations, which is the point.
fn bench_render_masked(c: &mut Criterion) {
    let Some((device, queue)) = try_create_gpu() else {
        println!("skipping offscreen_resource_cache benches: no GPU available");
        return;
    };

    let format = wgpu::TextureFormat::Rgba8UnormSrgb;

    // Build the renderer once — sampler + fullscreen VB are constructor-time.
    let mut offscreen = OffscreenRenderer::new(Arc::clone(&device), Arc::clone(&queue), format);

    let child_bounds =
        Rect::<Pixels>::from_ltrb(px(0.0), px(0.0), px(BENCH_SIDE_PX), px(BENCH_SIDE_PX));
    let result_size: Size<Pixels> = Size::new(px(BENCH_SIDE_PX), px(BENCH_SIDE_PX));
    let mask_shader = Shader::solid(Color::rgb(255, 128, 0));

    // Warm-up: one render pass ensures pipeline compilation is excluded.
    {
        let source = make_source_texture(&device, format);
        let _ = offscreen.render_masked(
            child_bounds,
            result_size,
            &mask_shader,
            BlendMode::SrcOver,
            &source,
        );
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
    }

    let source = make_source_texture(&device, format);

    c.bench_function("render_masked_256x256_solid", |b| {
        b.iter(|| {
            // `black_box` on all inputs prevents the compiler from constant-folding
            // the call.  The returned result is black_boxed so the compiler cannot
            // prove the call is a no-op and eliminate it.
            let masked_result = offscreen.render_masked(
                black_box(child_bounds),
                black_box(result_size),
                black_box(&mask_shader),
                black_box(BlendMode::SrcOver),
                black_box(&source),
            );
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            // Dropping `masked_result` here returns the texture to the pool,
            // keeping the pool in a warm steady state for every iteration.
            black_box(masked_result)
        });
    });
}

// ---------------------------------------------------------------------------
// `render_blur` benchmark
// ---------------------------------------------------------------------------

/// Measures the per-call overhead of `OffscreenRenderer::render_blur` with
/// sigma 5.0 (→ 3 downsample + 3 upsample passes) on a 256×256 input.
///
/// Each iteration:
/// 1. Calls `render_blur` with the cached pooled input texture.
/// 2. Drops the blurred output (returns it to the pool).
/// 3. Polls until the GPU completes.
///
/// Formerly per-call GPU allocations eliminated in this path:
/// - 1× `create_sampler` (now a cached struct field)
/// - 1× `create_buffer_init` for the fullscreen-quad VB (now a cached struct field)
/// - 6× `create_buffer_init` for `BlurParams` uniform buffers (now `queue.write_buffer`
///   into pre-allocated `COPY_DST` buffers)
fn bench_render_blur(c: &mut Criterion) {
    let Some((device, queue)) = try_create_gpu() else {
        // GPU unavailability was already printed by bench_render_masked.
        return;
    };

    let format = wgpu::TextureFormat::Rgba8UnormSrgb;

    let mut offscreen = OffscreenRenderer::new(Arc::clone(&device), Arc::clone(&queue), format);

    // Acquire the input texture via the renderer's pool so it is a warm
    // `PooledTexture` exactly as production code hands it to `render_blur`.
    let pool = Arc::clone(offscreen.texture_pool());
    let blur_input = pool.acquire(BENCH_SIDE_TEXELS, BENCH_SIDE_TEXELS, format);

    // sigma = 5.0 → iterations = ceil(5.0 / 2.0).clamp(1, 5) = 3
    let blur_sigma: f32 = 5.0;

    // Warm-up: one blur call so pipeline compilation is excluded.
    {
        let blur_output = offscreen.render_blur(&blur_input, blur_sigma);
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        drop(blur_output);
    }

    c.bench_function("render_blur_256x256_sigma5_3passes", |b| {
        b.iter(|| {
            let blur_output = offscreen.render_blur(black_box(&blur_input), black_box(blur_sigma));
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            // Drop returns the output texture to the pool — pool stays warm.
            black_box(blur_output)
        });
    });
}

// ---------------------------------------------------------------------------
// Allocation-overhead calibration
// ---------------------------------------------------------------------------

/// Measures the raw cost of the GPU allocations that were eliminated.
///
/// This bench creates the same resources the old per-call code created on every
/// `render_masked` + `render_blur` invocation, providing a lower-bound estimate
/// of the allocation overhead that the caching change removes from those paths.
///
/// Resources measured (matching the pre-patch call sites exactly):
/// - 1× `create_sampler` (ClampToEdge × Linear — was in render_masked AND render_blur)
/// - 1× `create_buffer_init` VERTEX (fullscreen quad VB — was in render_masked AND render_blur)
/// - 1× `create_buffer_init` UNIFORM (BlurParams per-iteration — was 2×iterations in render_blur)
///
/// This is NOT a full render call — it measures only the allocation side. The
/// number gives the additive allocation cost per call that is now a one-time
/// constructor + write-update cost.
fn bench_allocation_overhead_baseline(c: &mut Criterion) {
    let Some((device, _queue)) = try_create_gpu() else {
        return;
    };

    let mut group = c.benchmark_group("eliminated_allocation_overhead");

    // 1× create_sampler (was created on every render_masked AND render_blur call)
    group.bench_function("create_linear_sampler", |b| {
        b.iter(|| {
            black_box(device.create_sampler(&wgpu::SamplerDescriptor {
                label: None,
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            }))
        });
    });

    // 1× create_buffer_init VERTEX (fullscreen quad — was created every render_masked + render_blur)
    let quad_data: [[f32; 4]; 6] = [
        [-1.0, -1.0, 0.0, 1.0],
        [1.0, -1.0, 1.0, 1.0],
        [-1.0, 1.0, 0.0, 0.0],
        [-1.0, 1.0, 0.0, 0.0],
        [1.0, -1.0, 1.0, 1.0],
        [1.0, 1.0, 1.0, 0.0],
    ];
    group.bench_function("create_fullscreen_quad_vb", |b| {
        b.iter(|| {
            black_box(
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: black_box(cast_slice(&quad_data)),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
            )
        });
    });

    // 1× create_buffer_init UNIFORM (BlurParams — was 2×iterations per render_blur call,
    // up to 10 per call at max iterations=5; this measures the cost of one such creation)
    let params_data = [0u8; 16]; // size_of::<BlurParams>() = 16
    group.bench_function("create_blur_uniform_buffer_init", |b| {
        b.iter(|| {
            black_box(
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: black_box(&params_data),
                    usage: wgpu::BufferUsages::UNIFORM,
                }),
            )
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

criterion_group!(
    offscreen_benches,
    bench_render_masked,
    bench_render_blur,
    bench_allocation_overhead_baseline
);
criterion_main!(offscreen_benches);
