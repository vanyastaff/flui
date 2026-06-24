//! Advanced-blend composite driver: backdrop copy + shader composite.
//!
//! This module provides two public entry points used by the render-layer
//! advanced-blend interception (PR-3):
//!
//! - `copy_backdrop_region` — copies a device-space rect from the surface
//!   texture into a pooled offscreen texture so the compositor shader can
//!   sample it without racing the render target write.
//!
//! - `flush_advanced_layer` — runs the advanced-blend composite pass for one
//!   `AdvancedBlendOp`: copies the backdrop, builds the bind group, and
//!   executes one render pass over the op's device-space bounds.
//!
//! ## No production caller in PR-2
//!
//! `flush_advanced_layer` and `AdvancedBlendPipeline` have no production
//! call site in this PR; the renderer-layer interception that drives them is
//! wired in PR-3.  They ARE exercised by the synthetic-op GPU gate in this
//! module's `#[cfg(all(test, feature = "enable-wgpu-tests"))]` section, which
//! constitutes the authoritative correctness gate for the WGSL math.

use bytemuck::cast_slice;
use flui_types::{
    geometry::{Pixels, Rect},
    painting::BlendMode,
};
use wgpu::util::DeviceExt as _;

pub(crate) use pipeline::AdvancedBlendPipeline;
pub(crate) use pipeline::mode_to_u32;

use generated::advanced_blend;

use super::{resources::GpuResources, texture_pool::PooledTexture};

mod generated;
mod pipeline;

// ── Public types ──────────────────────────────────────────────────────────────

/// All inputs for one advanced-blend composite operation.
///
/// The `foreground` texture holds the layer's content pre-rendered into an
/// offscreen target (premultiplied RGBA).  Ownership is transferred in so the
/// caller's `PooledTexture` RAII handle is not dropped until after the render
/// pass that reads it completes.
pub(crate) struct AdvancedBlendOp {
    /// Pre-rendered foreground layer content (premultiplied RGBA).
    pub(crate) foreground: PooledTexture,
    /// The advanced blend mode to apply.
    pub(crate) mode: BlendMode,
    /// Device-space bounds of the foreground layer (origin + size in pixels).
    pub(crate) device_bounds: Rect<Pixels>,
    /// Group opacity in [0.0, 1.0].
    pub(crate) opacity: f32,
    /// Per-channel RGB tint in [0.0, 1.0] per component.
    pub(crate) tint: [f32; 3],
    /// Foreground texture UV min corner `[u_min, v_min]`.
    ///
    /// The VS-interpolated unit-quad UV `[0,1]` is remapped to
    /// `mix(src_uv_min, src_uv_max, uv)` before sampling the foreground.
    /// Pass `[0.0, 0.0]` for a full-viewport foreground (identity).
    pub(crate) src_uv_min: [f32; 2],
    /// Foreground texture UV max corner `[u_max, v_max]`.
    ///
    /// Pass `[1.0, 1.0]` for a full-viewport foreground (identity).
    pub(crate) src_uv_max: [f32; 2],
}

// ── Backdrop copy ─────────────────────────────────────────────────────────────

/// A successfully copied backdrop region ready for shader sampling.
///
/// Holds the pooled texture containing the copy and the copy geometry needed
/// to compute the backdrop UV in the fragment shader.
pub(crate) struct BackdropSample {
    /// Copy of the backdrop region (premultiplied RGBA).
    pub(crate) texture: PooledTexture,
    /// Origin of the copy rect in device pixels (rounded, clamped).
    pub(crate) copy_origin: (u32, u32),
    /// Extent of the copy rect in device pixels (≥ 1 × 1).
    pub(crate) copy_extent: (u32, u32),
}

// ── Backdrop copy ─────────────────────────────────────────────────────────────

/// Copy a device-space rectangle from `surface_texture` into a pooled offscreen
/// texture so the advanced-blend shader can sample the backdrop without reading
/// from the render target currently being written.
///
/// Returns `None` when the clamped device rect is entirely off-screen (zero
/// area after clamping against the surface extent).
///
/// ## Clamp and round policy
///
/// Mirrors `renderer.rs:1384-1396` exactly:
/// - Round each edge with `.round()` before truncation to avoid 1-pixel undersize
///   on sub-pixel boundaries (DPR ≠ 1 or fractional-offset CTMs).
/// - Clamp both edges to `[0, surface_extent]`.
/// - Derive width/height from the clamped corners (`right.saturating_sub(x)`).
/// - `max(1)` on width/height prevents a zero-extent copy (wgpu validation requires
///   non-zero extent).
pub(crate) fn copy_backdrop_region(
    surface_texture: &wgpu::Texture,
    device_rect: Rect<Pixels>,
    surface_format: wgpu::TextureFormat,
    resources: &mut GpuResources,
    encoder: &mut wgpu::CommandEncoder,
) -> Option<BackdropSample> {
    let surface_size = surface_texture.size();
    let surface_w = surface_size.width;
    let surface_h = surface_size.height;

    // Round before truncate (mirrors renderer.rs policy).
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to [0, surface_wh] before cast; truncation is intentional"
    )]
    let x = device_rect.left().0.clamp(0.0, surface_w as f32).round() as u32;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to [0, surface_wh] before cast; truncation is intentional"
    )]
    let y = device_rect.top().0.clamp(0.0, surface_h as f32).round() as u32;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to [0, surface_wh] before cast; truncation is intentional"
    )]
    let right = device_rect.right().0.clamp(0.0, surface_w as f32).round() as u32;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to [0, surface_wh] before cast; truncation is intentional"
    )]
    let bottom = device_rect.bottom().0.clamp(0.0, surface_h as f32).round() as u32;

    // Entirely off-screen after clamping → no copy possible.
    if right <= x || bottom <= y {
        tracing::warn!(
            bounds_l = device_rect.left().0,
            bounds_t = device_rect.top().0,
            bounds_r = device_rect.right().0,
            bounds_b = device_rect.bottom().0,
            surface_w,
            surface_h,
            "Advanced blend: clamped device region is empty (entirely off-screen); \
             skipping backdrop copy"
        );
        return None;
    }

    let copy_w = right.saturating_sub(x).max(1);
    let copy_h = bottom.saturating_sub(y).max(1);

    // Acquire a pooled texture matching the copy extent and surface format.
    let backdrop_copy = resources
        .layer_texture_pool_mut()
        .acquire(copy_w, copy_h, surface_format);

    encoder.copy_texture_to_texture(
        wgpu::TexelCopyTextureInfo {
            texture: surface_texture,
            mip_level: 0,
            origin: wgpu::Origin3d { x, y, z: 0 },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyTextureInfo {
            texture: backdrop_copy.texture(),
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::Extent3d {
            width: copy_w,
            height: copy_h,
            depth_or_array_layers: 1,
        },
    );

    Some(BackdropSample {
        texture: backdrop_copy,
        copy_origin: (x, y),
        copy_extent: (copy_w, copy_h),
    })
}

// ── Composite pass ────────────────────────────────────────────────────────────

/// Execute the advanced-blend composite pass for `op` onto `surface_view`.
///
/// Steps:
/// 1. Copy the backdrop region from `surface_texture` (via [`copy_backdrop_region`]).
/// 2. Build the per-draw bind group (uniform + foreground + backdrop + sampler).
/// 3. Issue one render pass over `op.device_bounds` with `LoadOp::Load` (preserving
///    existing surface content outside the blend region).
/// 4. Pooled textures (foreground, backdrop copy) are returned to the pool on drop.
#[allow(clippy::too_many_arguments)]
#[allow(
    clippy::needless_pass_by_value,
    reason = "AdvancedBlendOp fields are moved into the GPU bind group and render pass; ownership is required"
)]
pub(crate) fn flush_advanced_layer(
    op: AdvancedBlendOp,
    surface_texture: &wgpu::Texture,
    surface_view: &wgpu::TextureView,
    surface_format: wgpu::TextureFormat,
    viewport_size: (u32, u32),
    pipeline: &AdvancedBlendPipeline,
    resources: &mut GpuResources,
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
) {
    // Step 1: copy backdrop region.
    let Some(backdrop) = copy_backdrop_region(
        surface_texture,
        op.device_bounds,
        surface_format,
        resources,
        encoder,
    ) else {
        // Off-screen or zero-area: nothing to composite.
        tracing::debug!(
            mode = ?op.mode,
            bounds = ?op.device_bounds,
            "Advanced blend: skipping off-screen layer"
        );
        return;
    };

    // Step 2: build the uniform buffer and bind group.
    let (copy_origin_x, copy_origin_y) = backdrop.copy_origin;
    let (copy_extent_w, copy_extent_h) = backdrop.copy_extent;
    let (vp_w, vp_h) = viewport_size;

    // The generated `BlendUniforms::new` zero-fills the WGSL alignment padding
    // (`_pad0`); fields are passed in WGSL declaration order minus the pad.
    let uniforms = advanced_blend::BlendUniforms::new(
        [
            op.device_bounds.left().0,
            op.device_bounds.top().0,
            op.device_bounds.width().0,
            op.device_bounds.height().0,
        ],
        [vp_w as f32, vp_h as f32],
        [copy_origin_x as f32, copy_origin_y as f32],
        [copy_extent_w as f32, copy_extent_h as f32],
        op.opacity,
        op.tint,
        mode_to_u32(op.mode),
        op.src_uv_min,
        op.src_uv_max,
    );

    let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Advanced Blend Uniform Buffer"),
        contents: cast_slice(&[uniforms]),
        usage: wgpu::BufferUsages::UNIFORM,
    });

    // Nearest + ClampToEdge sampler — no filtering: the backdrop copy and
    // foreground are pixel-aligned; filtering would introduce colour error.
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("Advanced Blend Nearest Sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });

    // Per-draw bind group via the generated typed helper:
    // uniform (0) + foreground (1) + backdrop copy (2) + sampler (3).
    let bind_group = advanced_blend::WgpuBindGroup0::from_bindings(
        device,
        advanced_blend::WgpuBindGroup0Entries::new(advanced_blend::WgpuBindGroup0EntriesParams {
            blend: wgpu::BufferBinding {
                buffer: &uniform_buffer,
                offset: 0,
                size: None,
            },
            foreground_tex: op.foreground.view(),
            backdrop_tex: backdrop.texture.view(),
            nearest_sampler: &sampler,
        }),
    );

    // Step 3: render pass over op.device_bounds.
    // LoadOp::Load preserves surface content outside the blend region.
    {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Advanced Blend Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: surface_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        render_pass.set_pipeline(&pipeline.pipeline);
        bind_group.set(&mut render_pass);
        // 6 vertices synthesised in the VS from @builtin(vertex_index) — no vertex buffer.
        render_pass.draw(0..6, 0..1);
    }
    // Step 4: pooled textures (op.foreground, backdrop.texture) return to the
    // pool on drop at end of this scope — no explicit action needed.
}

// ── Synthetic-op GPU gate ─────────────────────────────────────────────────────
//
// This test module is the authoritative correctness gate for the WGSL math.
// It exercises all 15 advanced modes with a non-flat backdrop (left/right halves
// of distinct colours) and asserts that each pixel ≈ Color::blend(src, dst, mode)
// within ±1/255 in gamma space.
//
// A 1-texel UV shift makes a boundary pixel sample the wrong half → fails.
// A wrong blend formula → fails for the affected mode.
// A SrcOver fallback → fails for any mode where SrcOver ≠ the advanced mode.

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod synthetic_op_tests {
    use std::sync::Arc;

    use flui_types::{
        Color,
        geometry::{Pixels, Rect},
        painting::BlendMode,
    };
    use wgpu::util::DeviceExt as _;

    use super::{AdvancedBlendOp, AdvancedBlendPipeline, flush_advanced_layer};
    use crate::wgpu::{resources::GpuResources, texture_pool::TexturePool};

    // ── GPU test harness ──────────────────────────────────────────────────────

    fn request_device_and_queue() -> (Arc<wgpu::Device>, Arc<wgpu::Queue>) {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .expect("a GPU adapter must be available on a GPU-enabled test host");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("AdvancedBlend Synthetic Test Device"),
            ..Default::default()
        }))
        .expect("GPU device creation succeeded when adapter was found");
        (Arc::new(device), Arc::new(queue))
    }

    /// Format used for all synthetic-op textures.
    const TEST_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

    // Viewport / target dimensions.
    const TARGET_W: u32 = 4;
    const TARGET_H: u32 = 2;
    // Left half: columns 0-1; right half: columns 2-3.

    // ── Colour helpers ────────────────────────────────────────────────────────

    fn color_to_premul_f32(c: Color) -> [f32; 4] {
        let [red, green, blue, alpha] = c.to_f32_array();
        [red * alpha, green * alpha, blue * alpha, alpha]
    }

    /// Fill a 2D texture (w × h) with a solid premultiplied colour.
    fn create_solid_texture(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        w: u32,
        h: u32,
        format: wgpu::TextureFormat,
        color_pm: [f32; 4],
        usage_extra: wgpu::TextureUsages,
    ) -> wgpu::Texture {
        // Convert f32 premultiplied → u8 Rgba8Unorm bytes.
        // Clamping [0,1] before rounding makes the truncation and sign-loss safe.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "value clamped to [0,1] * 255 rounds into [0,255]; truncation and \
                      sign-loss are intentional and provably safe"
        )]
        let f32_to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;

        let mut texels: Vec<u8> = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..(w * h) {
            texels.push(f32_to_u8(color_pm[0]));
            texels.push(f32_to_u8(color_pm[1]));
            texels.push(f32_to_u8(color_pm[2]));
            texels.push(f32_to_u8(color_pm[3]));
        }
        device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("Synthetic Solid Texture"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_DST
                    | usage_extra,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &texels,
        )
    }

    /// Build a w×h surface texture with left-half = `left_pm` and right-half = `right_pm`.
    #[allow(clippy::too_many_arguments)]
    fn create_split_texture(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        w: u32,
        h: u32,
        format: wgpu::TextureFormat,
        left_pm: [f32; 4],
        right_pm: [f32; 4],
        usage_extra: wgpu::TextureUsages,
    ) -> wgpu::Texture {
        // Clamping [0,1]*255 before truncation makes the cast provably safe.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "value clamped to [0,1] * 255 rounds into [0,255]; safe"
        )]
        let f32_to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;

        let mut texels: Vec<u8> = Vec::with_capacity((w * h * 4) as usize);
        for _row in 0..h {
            for col in 0..w {
                let color_pm = if col < w / 2 { left_pm } else { right_pm };
                texels.push(f32_to_u8(color_pm[0]));
                texels.push(f32_to_u8(color_pm[1]));
                texels.push(f32_to_u8(color_pm[2]));
                texels.push(f32_to_u8(color_pm[3]));
            }
        }
        device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("Synthetic Split Surface Texture"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST
                    | usage_extra,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &texels,
        )
    }

    /// Read back all pixels from a texture via a staging buffer.
    /// Returns RGBA bytes in row-major order.
    fn readback_texture(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
        w: u32,
        h: u32,
    ) -> Vec<[u8; 4]> {
        // wgpu requires the row stride to be a multiple of COPY_BYTES_PER_ROW_ALIGNMENT (256).
        let bytes_per_row_unaligned = w * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let bytes_per_row = bytes_per_row_unaligned.div_ceil(align) * align;
        let buffer_size = u64::from(bytes_per_row * h);

        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Readback Staging Buffer"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Readback Encoder"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));

        // Map and read.
        let slice = staging.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("device poll must complete the readback copy");

        let mapped = slice.get_mapped_range();
        let raw = mapped.as_ref();

        let mut pixels = Vec::with_capacity((w * h) as usize);
        for row in 0..h {
            let row_start = (row * bytes_per_row) as usize;
            for col in 0..w {
                let offset = row_start + (col as usize) * 4;
                pixels.push([
                    raw[offset],
                    raw[offset + 1],
                    raw[offset + 2],
                    raw[offset + 3],
                ]);
            }
        }
        pixels
    }

    // ── Oracle ────────────────────────────────────────────────────────────────

    /// CPU oracle: compute expected RGBA u8 for `Color::blend(src_straight, dst_straight, mode)`.
    /// Returns RGBA in the same premultiplied encoding the GPU writes (Rgba8Unorm).
    fn oracle_pixel(src_straight: Color, dst_straight: Color, mode: BlendMode) -> [u8; 4] {
        let result = src_straight.blend(dst_straight, mode);
        // Color::blend returns a straight Color (un-premultiplied); convert to premultiplied
        // RGBA bytes for comparison with the GPU readback (which outputs premultiplied).
        let [r, g, b, a] = result.to_f32_array();
        // Clamping [0,1]*255 before truncation makes the cast provably safe.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "value clamped to [0,1] * 255 rounds into [0,255]; safe"
        )]
        let f32_to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let r_pm = f32_to_u8(r * a);
        let g_pm = f32_to_u8(g * a);
        let b_pm = f32_to_u8(b * a);
        let a_u8 = f32_to_u8(a);
        [r_pm, g_pm, b_pm, a_u8]
    }

    /// Assert two RGBA u8 pixels are within `±tolerance` in every channel.
    fn assert_pixel_close(label: &str, actual: [u8; 4], expected: [u8; 4], tolerance: u8) {
        for ch in 0..4 {
            let diff =
                u8::try_from((i16::from(actual[ch]) - i16::from(expected[ch])).unsigned_abs())
                    .expect("diff of two u8 values always fits in u8");
            assert!(
                diff <= tolerance,
                "{label}: channel {ch} — actual {a}, expected {e}, diff {diff} > tolerance {tolerance}",
                a = actual[ch],
                e = expected[ch],
            );
        }
    }

    // ── Main synthetic-op test ────────────────────────────────────────────────

    /// For each of the 15 advanced blend modes:
    ///
    /// 1. Build a `TARGET_W × TARGET_H` surface split left=D1, right=D2.
    /// 2. Render a solid-colour foreground (src S) over it with `flush_advanced_layer`.
    /// 3. Read back the result.
    /// 4. Assert left pixels ≈ oracle(S, D1, mode) and right pixels ≈ oracle(S, D2, mode),
    ///    within ±1/255.
    ///
    /// A 1-texel UV shift → boundary pixel samples wrong half → fails.
    /// Wrong blend formula → fails on the affected mode.
    /// SrcOver fallback → fails on any mode where Multiply ≠ SrcOver.
    #[test]
    fn all_15_advanced_modes_match_cpu_oracle_within_one_lsb() {
        let (device, queue) = request_device_and_queue();

        // OPAQUE colors: premul == straight (no quantization round-trip on u8↔f32 unpremul).
        // With α=255 the composite formula collapses to exactly B(Cb,Cs), so GPU == oracle
        // within ±1 LSB from pure f32 rounding.  This is STRICTER for formula correctness
        // than semi-transparent inputs, not a mask.
        let src_straight = Color::rgba(200, 120, 40, 255);
        let dst_left_straight = Color::rgba(40, 60, 220, 255);
        let dst_right_straight = Color::rgba(20, 180, 50, 255);

        let src_pm = color_to_premul_f32(src_straight);
        let dst_left_pm = color_to_premul_f32(dst_left_straight);
        let dst_right_pm = color_to_premul_f32(dst_right_straight);

        let advanced_modes = [
            BlendMode::Multiply,
            BlendMode::Screen,
            BlendMode::Overlay,
            BlendMode::Darken,
            BlendMode::Lighten,
            BlendMode::ColorDodge,
            BlendMode::ColorBurn,
            BlendMode::HardLight,
            BlendMode::SoftLight,
            BlendMode::Difference,
            BlendMode::Exclusion,
            BlendMode::Hue,
            BlendMode::Saturation,
            BlendMode::Color,
            BlendMode::Luminosity,
        ];

        let pipeline = AdvancedBlendPipeline::new(&device, TEST_FORMAT);
        let pool = TexturePool::new(Arc::clone(&device));
        let mut resources = GpuResources::new(Arc::clone(&device), Arc::clone(&queue));

        // Build the foreground pooled texture (solid src, full target size).
        // The texture pool uses RENDER_ATTACHMENT | TEXTURE_BINDING | COPY_SRC | COPY_DST.
        // We need COPY_DST for upload and TEXTURE_BINDING for shader sampling.
        // Use create_solid_texture to upload; then we need it in a PooledTexture.
        // Easiest: acquire a pooled texture, then submit a copy from a staging texture.

        // Staging foreground texture (solid src_pm).
        let fg_staging = create_solid_texture(
            &device,
            &queue,
            TARGET_W,
            TARGET_H,
            TEST_FORMAT,
            src_pm,
            wgpu::TextureUsages::COPY_SRC,
        );

        // Acquire a pooled foreground texture.
        let fg_pooled = pool.acquire(TARGET_W, TARGET_H, TEST_FORMAT);

        // Copy staging → pooled.
        {
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("FG Upload Encoder"),
            });
            enc.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &fg_staging,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: fg_pooled.texture(),
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: TARGET_W,
                    height: TARGET_H,
                    depth_or_array_layers: 1,
                },
            );
            queue.submit(std::iter::once(enc.finish()));
        }

        for mode in advanced_modes {
            // Build a fresh split surface for each mode (flush_advanced_layer modifies it).
            let surface_texture = create_split_texture(
                &device,
                &queue,
                TARGET_W,
                TARGET_H,
                TEST_FORMAT,
                dst_left_pm,
                dst_right_pm,
                wgpu::TextureUsages::empty(),
            );
            let surface_view = surface_texture.create_view(&wgpu::TextureViewDescriptor::default());

            // Build a fresh foreground pooled texture for each mode (consumed by op).
            let fg_this_mode = pool.acquire(TARGET_W, TARGET_H, TEST_FORMAT);
            {
                let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("FG Per-Mode Upload Encoder"),
                });
                enc.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &fg_staging,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: fg_this_mode.texture(),
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::Extent3d {
                        width: TARGET_W,
                        height: TARGET_H,
                        depth_or_array_layers: 1,
                    },
                );
                queue.submit(std::iter::once(enc.finish()));
            }

            let op = AdvancedBlendOp {
                foreground: fg_this_mode,
                mode,
                device_bounds: Rect::from_xywh(
                    Pixels(0.0),
                    Pixels(0.0),
                    Pixels(TARGET_W as f32),
                    Pixels(TARGET_H as f32),
                ),
                opacity: 1.0,
                tint: [1.0, 1.0, 1.0],
                // Full-viewport foreground: identity UV remap.
                src_uv_min: [0.0, 0.0],
                src_uv_max: [1.0, 1.0],
            };

            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Advanced Blend Test Encoder"),
            });

            flush_advanced_layer(
                op,
                &surface_texture,
                &surface_view,
                TEST_FORMAT,
                (TARGET_W, TARGET_H),
                &pipeline,
                &mut resources,
                &device,
                &mut encoder,
            );

            queue.submit(std::iter::once(encoder.finish()));

            // Read back result.
            let pixels = readback_texture(&device, &queue, &surface_texture, TARGET_W, TARGET_H);

            // Compute oracles.
            let expected_left = oracle_pixel(src_straight, dst_left_straight, mode);
            let expected_right = oracle_pixel(src_straight, dst_right_straight, mode);

            // Tolerance: ±1 LSB (1/255) to absorb f32 rounding across premul/unpremul.
            let tolerance = 1u8;
            let mode_label = format!("{mode:?}");

            // Check all left-half pixels (columns 0..TARGET_W/2).
            for row in 0..TARGET_H {
                for col in 0..(TARGET_W / 2) {
                    let pixel = pixels[(row * TARGET_W + col) as usize];
                    assert_pixel_close(
                        &format!("{mode_label} left col={col} row={row}"),
                        pixel,
                        expected_left,
                        tolerance,
                    );
                }
            }

            // Check all right-half pixels (columns TARGET_W/2..TARGET_W).
            for row in 0..TARGET_H {
                for col in (TARGET_W / 2)..TARGET_W {
                    let pixel = pixels[(row * TARGET_W + col) as usize];
                    assert_pixel_close(
                        &format!("{mode_label} right col={col} row={row}"),
                        pixel,
                        expected_right,
                        tolerance,
                    );
                }
            }
        }

        // Drop fg_pooled after all modes — it was only used as the staging source.
        drop(fg_pooled);
    }

    // ── Edge-case tests ───────────────────────────────────────────────────────

    /// ColorDodge and ColorBurn GPU round-trip: WGSL divide-guard branches vs oracle.
    ///
    /// Uses OPAQUE inputs so the composite formula collapses to B(Cb,Cs) and the GPU
    /// readback can be compared against `oracle_pixel` directly within ±1.
    ///
    /// Two boundary pairs are covered:
    /// - (cs=1, cb=0): ColorDodge → cb=0 guard → B=0; ColorBurn → cs=1 guard → B=0.
    /// - (cs=0, cb=1): ColorDodge → cb/(1-0)=1 → B=1; ColorBurn → cb=1 guard → B=1.
    ///
    /// Note on the prior CPU-only test: `Color::blend` returns the full W3C *composite*
    /// result, not B alone.  For semi-transparent inputs the composite adds an
    /// αs(1-αb)·Cs source-over term ≈ 0.176 even when B=0, so asserting `R≈0.0` on
    /// the composite was incorrect.  The GPU round-trip with opaque inputs avoids that
    /// confusion entirely.
    #[test]
    fn color_dodge_and_burn_boundary_inputs_match_cpu_oracle() {
        let (device, queue) = request_device_and_queue();
        let pipeline = AdvancedBlendPipeline::new(&device, TEST_FORMAT);
        let pool = TexturePool::new(Arc::clone(&device));
        let mut resources = GpuResources::new(Arc::clone(&device), Arc::clone(&queue));
        let tolerance = 1u8;

        // Helper: run one flush and read back the single-pixel result.
        let mut run_blend = |src: Color, dst: Color, mode: BlendMode, label: &str| -> [u8; 4] {
            let src_pm = color_to_premul_f32(src);
            let dst_pm = color_to_premul_f32(dst);
            let surface = create_solid_texture(
                &device,
                &queue,
                1,
                1,
                TEST_FORMAT,
                dst_pm,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            );
            let surface_view = surface.create_view(&wgpu::TextureViewDescriptor::default());
            let fg_staging = create_solid_texture(
                &device,
                &queue,
                1,
                1,
                TEST_FORMAT,
                src_pm,
                wgpu::TextureUsages::COPY_SRC,
            );
            let fg_pooled = pool.acquire(1, 1, TEST_FORMAT);
            {
                let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("DodgeBurn Upload"),
                });
                enc.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &fg_staging,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: fg_pooled.texture(),
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                );
                queue.submit(std::iter::once(enc.finish()));
            }
            let op = AdvancedBlendOp {
                foreground: fg_pooled,
                mode,
                device_bounds: Rect::from_xywh(Pixels(0.0), Pixels(0.0), Pixels(1.0), Pixels(1.0)),
                opacity: 1.0,
                tint: [1.0, 1.0, 1.0],
                src_uv_min: [0.0, 0.0],
                src_uv_max: [1.0, 1.0],
            };
            let mut encoder = device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(label) });
            flush_advanced_layer(
                op,
                &surface,
                &surface_view,
                TEST_FORMAT,
                (1, 1),
                &pipeline,
                &mut resources,
                &device,
                &mut encoder,
            );
            queue.submit(std::iter::once(encoder.finish()));
            readback_texture(&device, &queue, &surface, 1, 1)[0]
        };

        // ── (cs=1, cb=0): white src over black dst ────────────────────────────
        let src_white = Color::rgba(255, 255, 255, 255); // cs=1.0
        let dst_black = Color::rgba(0, 0, 0, 255); // cb=0.0

        // ColorDodge: cb<=0 guard → B=0 → composite with opaque inputs = 0.
        let got_dodge_a = run_blend(src_white, dst_black, BlendMode::ColorDodge, "DodgeA");
        let exp_dodge_a = oracle_pixel(src_white, dst_black, BlendMode::ColorDodge);
        assert_pixel_close("ColorDodge cs=1 cb=0", got_dodge_a, exp_dodge_a, tolerance);

        // ColorBurn: cs>=1 guard → B=0 → composite = 0.
        let got_burn_a = run_blend(src_white, dst_black, BlendMode::ColorBurn, "BurnA");
        let exp_burn_a = oracle_pixel(src_white, dst_black, BlendMode::ColorBurn);
        assert_pixel_close("ColorBurn cs=1 cb=0", got_burn_a, exp_burn_a, tolerance);

        // ── (cs=0, cb=1): black src over white dst ────────────────────────────
        let src_black = Color::rgba(0, 0, 0, 255); // cs=0.0
        let dst_white = Color::rgba(255, 255, 255, 255); // cb=1.0

        // ColorDodge: cb/(1-cs) = cb/1 = cb; min(1,1)=1 → B=1 → composite = 1.
        let got_dodge_b = run_blend(src_black, dst_white, BlendMode::ColorDodge, "DodgeB");
        let exp_dodge_b = oracle_pixel(src_black, dst_white, BlendMode::ColorDodge);
        assert_pixel_close("ColorDodge cs=0 cb=1", got_dodge_b, exp_dodge_b, tolerance);

        // ColorBurn: cb>=1 guard → B=1 → composite = 1.
        let got_burn_b = run_blend(src_black, dst_white, BlendMode::ColorBurn, "BurnB");
        let exp_burn_b = oracle_pixel(src_black, dst_white, BlendMode::ColorBurn);
        assert_pixel_close("ColorBurn cs=0 cb=1", got_burn_b, exp_burn_b, tolerance);
    }

    /// Fully transparent source over fully transparent backdrop → transparent output.
    ///
    /// out_a = 0 → the shader must return vec4(0) without NaN-propagation
    /// from the division guards.
    #[test]
    fn transparent_source_over_transparent_backdrop_yields_transparent() {
        let (device, queue) = request_device_and_queue();

        let transparent_pm = [0.0f32; 4];
        let surface_texture = create_split_texture(
            &device,
            &queue,
            4,
            2,
            TEST_FORMAT,
            transparent_pm,
            transparent_pm,
            wgpu::TextureUsages::empty(),
        );
        let surface_view = surface_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let pool = TexturePool::new(Arc::clone(&device));
        let fg_transparent = pool.acquire(4, 2, TEST_FORMAT);
        // Explicitly upload transparent (all-zero) pixels — do not rely on pool zero-init,
        // which is an implementation detail not guaranteed by the pool contract.
        let transparent_bytes = vec![0u8; (4 * 2 * 4) as usize]; // w=4, h=2, 4 bytes/pixel
        queue.write_texture(
            fg_transparent.texture().as_image_copy(),
            &transparent_bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * 4),
                rows_per_image: Some(2),
            },
            wgpu::Extent3d {
                width: 4,
                height: 2,
                depth_or_array_layers: 1,
            },
        );

        let pipeline = AdvancedBlendPipeline::new(&device, TEST_FORMAT);
        let mut resources = GpuResources::new(Arc::clone(&device), Arc::clone(&queue));

        let op = AdvancedBlendOp {
            foreground: fg_transparent,
            mode: BlendMode::Multiply, // any mode — out_a=0 must short-circuit
            device_bounds: Rect::from_xywh(Pixels(0.0), Pixels(0.0), Pixels(4.0), Pixels(2.0)),
            opacity: 1.0,
            tint: [1.0, 1.0, 1.0],
            src_uv_min: [0.0, 0.0],
            src_uv_max: [1.0, 1.0],
        };

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Transparent Test Encoder"),
        });
        flush_advanced_layer(
            op,
            &surface_texture,
            &surface_view,
            TEST_FORMAT,
            (4, 2),
            &pipeline,
            &mut resources,
            &device,
            &mut encoder,
        );
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_texture(&device, &queue, &surface_texture, 4, 2);
        for (i, pixel) in pixels.iter().enumerate() {
            assert_eq!(
                *pixel,
                [0, 0, 0, 0],
                "transparent+transparent pixel {i} must be fully transparent, got {pixel:?}"
            );
        }
    }

    /// Backdrop UV correctness with a non-zero `copy_origin`.
    ///
    /// The surface is 6 × 2: left 2 columns = colour A, right 4 columns = colour B.
    /// The foreground op covers columns 2-5 (x=2, width=4) — so `copy_origin=(2,0)`,
    /// `copy_extent=(4,2)`.  Column 2 of the surface maps to texel 0 of the backdrop
    /// copy; column 3 → texel 1, etc.
    ///
    /// With the correct UV formula `bd_uv = (frag_pos.xy - copy_origin) / copy_extent`,
    /// every sampled backdrop texel = colour B.
    ///
    /// If `+0.5` is incorrectly added again after `frag_pos`, column 2
    /// (frag_pos.x ≈ 2.5) would sample `(2.5-2+0.5)/4 = 0.25` instead of
    /// `(2.5-2)/4 = 0.125` — the first texel of the copy still lands inside B,
    /// so the test would pass despite the bug on a narrow region.  Therefore we
    /// also include a case where the copy region starts mid-surface and the sub-pixel
    /// alignment matters: `copy_origin=(1,0)`, op x=1.
    ///
    /// Both the column-1 and column-5 pixels are asserted against the oracle.
    #[test]
    fn backdrop_uv_is_correct_with_nonzero_copy_origin() {
        // Surface: 6 wide × 2 tall.
        // `create_split_texture` puts col < w/2 = left, col >= w/2 = right.
        // For w=6: cols 0,1,2 = LEFT (red); cols 3,4,5 = RIGHT (blue).
        //
        // Op covers x=1, width=4 → surface columns 1,2,3,4 → copy_origin=(1,0),
        // copy_extent=(4,2).  With the correct UV formula each covered surface column `c`
        // samples its own original backdrop: col c's expected = oracle(src, surface_at_c).
        //
        // With an incorrect +0.5 on bd_uv the boundary between left and right shifts;
        // a ±1-LSB assertion on the per-column oracle catches the offset.
        //
        // OPAQUE colors: no premul→u8→unpremul quantization error (α=255 keeps ±1 tight).
        const SURF_W: u32 = 6;
        const SURF_H: u32 = 2;

        let (device, queue) = request_device_and_queue();

        // Deliberately distinct opaque colors so a UV regression misassigns a column.
        let color_left = Color::rgba(200, 20, 20, 255); // opaque red
        let color_right = Color::rgba(20, 20, 200, 255); // opaque blue
        let src_straight = Color::rgba(180, 100, 30, 255); // opaque orange

        let left_pm = color_to_premul_f32(color_left);
        let right_pm = color_to_premul_f32(color_right);
        let src_pm = color_to_premul_f32(src_straight);

        let surface_texture = create_split_texture(
            &device,
            &queue,
            SURF_W,
            SURF_H,
            TEST_FORMAT,
            left_pm,
            right_pm,
            wgpu::TextureUsages::empty(),
        );
        let surface_view = surface_texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Foreground: solid orange, 4 × SURF_H.
        // COPY_SRC is required because we copy FROM this texture into the pooled texture.
        let fg_staging = create_solid_texture(
            &device,
            &queue,
            4,
            SURF_H,
            TEST_FORMAT,
            src_pm,
            wgpu::TextureUsages::COPY_SRC,
        );

        let pool = TexturePool::new(Arc::clone(&device));
        let fg_pooled = pool.acquire(4, SURF_H, TEST_FORMAT);
        {
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("NonZeroOrigin FG Upload"),
            });
            enc.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &fg_staging,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: fg_pooled.texture(),
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: 4,
                    height: SURF_H,
                    depth_or_array_layers: 1,
                },
            );
            queue.submit(std::iter::once(enc.finish()));
        }

        let pipeline = AdvancedBlendPipeline::new(&device, TEST_FORMAT);
        let mut resources = GpuResources::new(Arc::clone(&device), Arc::clone(&queue));

        let op = AdvancedBlendOp {
            foreground: fg_pooled,
            mode: BlendMode::Multiply,
            device_bounds: Rect::from_xywh(
                Pixels(1.0),
                Pixels(0.0),
                Pixels(4.0),
                Pixels(SURF_H as f32),
            ),
            opacity: 1.0,
            tint: [1.0, 1.0, 1.0],
            // Foreground is 4×SURF_H, not full-viewport (SURF_W=6) — identity
            // UV within the foreground texture itself (the texture IS 4 wide).
            src_uv_min: [0.0, 0.0],
            src_uv_max: [1.0, 1.0],
        };

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("NonZeroOrigin Blend Encoder"),
        });
        flush_advanced_layer(
            op,
            &surface_texture,
            &surface_view,
            TEST_FORMAT,
            (SURF_W, SURF_H),
            &pipeline,
            &mut resources,
            &device,
            &mut encoder,
        );
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_texture(&device, &queue, &surface_texture, SURF_W, SURF_H);
        let tolerance = 1u8;

        // Column 0: outside op bounds — must NOT be blended.
        // With opaque colors the untouched pixel equals the original premul-u8 surface value,
        // which ≠ Multiply(src, left) since src.r=180/255 < 1 (Multiply always darkens).
        for row in 0..SURF_H {
            let untouched = pixels[(row * SURF_W) as usize];
            let would_be_blended = oracle_pixel(src_straight, color_left, BlendMode::Multiply);
            assert_ne!(
                untouched, would_be_blended,
                "row={row}: col 0 is outside op bounds and must NOT be blended"
            );
        }

        // Covered columns 1-4: each column samples its OWN original surface backdrop.
        // create_split_texture: col < 3 → left, col >= 3 → right (w=6, w/2=3).
        // col 1 → surface col 1 < 3 → backdrop = color_left (opaque red).
        // col 2 → surface col 2 < 3 → backdrop = color_left.
        // col 3 → surface col 3 >= 3 → backdrop = color_right (opaque blue).
        // col 4 → surface col 4 >= 3 → backdrop = color_right.
        let backdrop_at_col = |col: u32| {
            if col < SURF_W / 2 {
                color_left
            } else {
                color_right
            }
        };

        for row in 0..SURF_H {
            for col in 1..5u32 {
                let expected =
                    oracle_pixel(src_straight, backdrop_at_col(col), BlendMode::Multiply);
                let pix = pixels[(row * SURF_W + col) as usize];
                assert_pixel_close(
                    &format!("NonZeroOrigin Multiply col={col} row={row}"),
                    pix,
                    expected,
                    tolerance,
                );
            }
        }
    }

    /// Semi-transparent composite: Screen mode with α<255 inputs.
    ///
    /// The opaque tests above verify blend-formula correctness but exercise only the
    /// `αs=1, αb=1` path of the W3C composite equation.  This test uses semi-transparent
    /// src (α=180) and dst (α=200) to exercise the full αs/αb composite arithmetic.
    ///
    /// Tolerance is ±2 (not ±1) to absorb the premul→u8→unpremul quantization error
    /// that occurs when the GPU round-trips straight colors through premultiplied u8
    /// textures before the blend function.  The oracle (`Color::blend`) operates on
    /// pristine straight-channel values; at sub-255 alphas the u8 encode/decode
    /// introduces up to ~1 LSB per channel before the blend, which propagates to ±2
    /// in the output byte.
    #[test]
    fn translucent_composite_matches_oracle() {
        let (device, queue) = request_device_and_queue();
        let pipeline = AdvancedBlendPipeline::new(&device, TEST_FORMAT);
        let pool = TexturePool::new(Arc::clone(&device));
        let mut resources = GpuResources::new(Arc::clone(&device), Arc::clone(&queue));

        // Semi-transparent inputs — the composite path where αs·αb terms are non-trivial.
        let src_straight = Color::rgba(160, 80, 200, 180);
        let dst_straight = Color::rgba(50, 180, 60, 200);
        let src_pm = color_to_premul_f32(src_straight);
        let dst_pm = color_to_premul_f32(dst_straight);

        let surface = create_solid_texture(
            &device,
            &queue,
            TARGET_W,
            TARGET_H,
            TEST_FORMAT,
            dst_pm,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let surface_view = surface.create_view(&wgpu::TextureViewDescriptor::default());

        let fg_staging = create_solid_texture(
            &device,
            &queue,
            TARGET_W,
            TARGET_H,
            TEST_FORMAT,
            src_pm,
            wgpu::TextureUsages::COPY_SRC,
        );
        let fg_pooled = pool.acquire(TARGET_W, TARGET_H, TEST_FORMAT);
        {
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Translucent FG Upload"),
            });
            enc.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &fg_staging,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: fg_pooled.texture(),
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: TARGET_W,
                    height: TARGET_H,
                    depth_or_array_layers: 1,
                },
            );
            queue.submit(std::iter::once(enc.finish()));
        }

        let op = AdvancedBlendOp {
            foreground: fg_pooled,
            mode: BlendMode::Screen,
            device_bounds: Rect::from_xywh(
                Pixels(0.0),
                Pixels(0.0),
                Pixels(TARGET_W as f32),
                Pixels(TARGET_H as f32),
            ),
            opacity: 1.0,
            tint: [1.0, 1.0, 1.0],
            src_uv_min: [0.0, 0.0],
            src_uv_max: [1.0, 1.0],
        };

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Translucent Screen Encoder"),
        });
        flush_advanced_layer(
            op,
            &surface,
            &surface_view,
            TEST_FORMAT,
            (TARGET_W, TARGET_H),
            &pipeline,
            &mut resources,
            &device,
            &mut encoder,
        );
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_texture(&device, &queue, &surface, TARGET_W, TARGET_H);
        let expected = oracle_pixel(src_straight, dst_straight, BlendMode::Screen);
        // ±2: absorbs the premul→u8→unpremul quantization of the GPU's premultiplied
        // texture inputs, which the straight-color oracle does not model.
        // Formula correctness (±1) is covered by the opaque tests above.
        let tolerance = 2u8;
        for (i, &pixel) in pixels.iter().enumerate() {
            assert_pixel_close(
                &format!("Screen translucent pixel {i}"),
                pixel,
                expected,
                tolerance,
            );
        }
    }

    /// Non-separable Color mode with an achromatic (flat) backdrop (R=G=B).
    ///
    /// `set_sat` on a flat triple must return black (not NaN) because max==min.
    #[test]
    fn color_mode_with_achromatic_backdrop_does_not_nan() {
        let src_straight = Color::rgba(200, 50, 100, 180); // colourful source
        let dst_achromatic = Color::rgba(128, 128, 128, 200); // flat (R=G=B)

        // CPU oracle must not panic (regression guard).
        let result = src_straight.blend(dst_achromatic, BlendMode::Color);
        let [r, g, b, _a] = result.to_f32_array();
        // Result channels must be finite and in [0, 1].
        assert!(
            r.is_finite() && (0.0f32..=1.0).contains(&r),
            "Color mode R NaN/OOB: {r}"
        );
        assert!(
            g.is_finite() && (0.0f32..=1.0).contains(&g),
            "Color mode G NaN/OOB: {g}"
        );
        assert!(
            b.is_finite() && (0.0f32..=1.0).contains(&b),
            "Color mode B NaN/OOB: {b}"
        );
    }
}
