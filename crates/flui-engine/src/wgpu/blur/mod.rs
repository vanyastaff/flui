//! Separable Gaussian blur filter: two sub-passes (H then V) over a
//! premultiplied RGBA offscreen.
//!
//! The only public entry point is `apply_blur`, called by
//! `apply_image_filter_passes` when an `ImageFilterPass::Blur` is in the
//! pass chain.
//!
//! ## Two-sub-pass design
//!
//! A separable Gaussian blur is O(N) per pixel rather than O(N²):
//!
//! 1. **H pass** (`direction = 0`): read `source_tex`, write `h_tex` — horizontal scan.
//! 2. **V pass** (`direction = 1`): read `h_tex`,     write `v_tex` — vertical scan.
//!
//! At most **two** pooled textures are live simultaneously (source is
//! caller-owned, `h_tex` lives only between the two sub-passes and is dropped
//! before returning).
//!
//! ## Premultiplied-direct invariant (PINNED #2)
//!
//! The Gaussian kernel operates directly on premultiplied RGBA in
//! sRGB-encoded space — NO unpremultiply step, NO sRGB→linear conversion.
//! This matches Impeller `gaussian_blur_filter_contents.cc:935`
//! (`apply_unpremultiply=false`).  The CPU oracle in `blur_filter_tests.rs`
//! follows the same contract.
//!
//! ## √3·sigma kernel extent
//!
//! The kernel half-radius is `kernel_radius(sigma) = ceil(sigma × √3)`, matching
//! Impeller's `kKernelRadiusPerSigma = √3` (`sigma.h:24`).  Running-sum
//! renormalisation in the shader compensates for the truncated tails.
//!
//! ## Decal semantics
//!
//! wgpu has no `AddressMode::Decal`.  The H pass decals in-shader:
//! samples outside `content_bounds` return `vec4(0.0)`.  The V pass decals
//! at the texture edge `[0,1]` so it reads the full H-pass halo — identical
//! to the morphology filter's V-pass strategy.
//!
//! ## Anisotropic
//!
//! `sigma_x` drives the H pass, `sigma_y` drives the V pass.  The passes
//! are independent, producing a true anisotropic Gaussian blur.

use std::sync::Arc;

use bytemuck::cast_slice;
use flui_types::{Rect, geometry::Pixels};
use wgpu::util::DeviceExt as _;

pub(crate) use pipeline::BlurPipeline;
use pipeline::BlurUniform;

use super::{resources::GpuResources, texture_pool::PooledTexture};

mod pipeline;

/// Apply a Gaussian blur to `source_tex` via two separable sub-passes
/// (H then V), returning the filtered texture.
///
/// ## Premultiplied-direct (PINNED #2)
///
/// The Gaussian kernel operates on premultiplied sRGB-encoded RGBA — no
/// unpremultiply step, no linearisation.  Matching Impeller.
///
/// ## Parameters
///
/// - `sigma_x` — Gaussian sigma for the horizontal pass.
/// - `sigma_y` — Gaussian sigma for the vertical pass.
/// - `source_tex` — premultiplied RGBA offscreen from `render_segment_to_offscreen`.
/// - `content_bounds` — AABB of the content in physical pixels; used to compute
///   the H-pass decal UV rect (samples beyond content → `vec4(0.0)`).
/// - `viewport_size` — `(width, height)` in physical pixels; the output and
///   intermediate textures share this size.
/// - `surface_format` — texture format of the render target.
/// - `pipeline` — the blur pipeline (bind-group layout + render pipeline).
/// - `resources` — mutable GPU resource manager (texture pool).
/// - `device` — wgpu device for buffer and bind-group creation.
/// - `encoder` — command encoder that both sub-passes are recorded into.
///
/// ## Pool discipline (≤ 2 live textures)
///
/// - `h_tex` is acquired before the H pass and dropped immediately after the V
///   pass starts reading it, so only `h_tex` + `v_tex` are simultaneously live.
/// - `source_tex` is a caller-owned borrow — not counted against this limit.
#[allow(
    clippy::too_many_arguments,
    reason = "GPU pass functions require device/encoder/pipeline/resources plus the operation inputs"
)]
pub(crate) fn apply_blur(
    sigma_x: f32,
    sigma_y: f32,
    source_tex: &PooledTexture,
    content_bounds: Rect<Pixels>,
    viewport_size: (u32, u32),
    surface_format: wgpu::TextureFormat,
    pipeline: &BlurPipeline,
    resources: &mut GpuResources,
    device: &Arc<wgpu::Device>,
    encoder: &mut wgpu::CommandEncoder,
) -> PooledTexture {
    let (viewport_w, viewport_h) = viewport_size;

    // Normalise the content AABB to UV coordinates [0, 1] × [0, 1].
    // The H pass decals against the content rect (samples beyond it are
    // transparent black). Inside the texture but outside content, the source is
    // already transparent (cleared by `render_segment_to_offscreen`), so this is
    // equivalent to a texture-edge decal — but it keeps the content edge explicit.
    let content_rect_uv_h = [
        content_bounds.left().0 / viewport_w as f32,
        content_bounds.top().0 / viewport_h as f32,
        content_bounds.right().0 / viewport_w as f32,
        content_bounds.bottom().0 / viewport_h as f32,
    ];
    // The V pass reads the H-pass output whose content extent has already grown
    // horizontally into the halo. Decaling the V pass at the original content
    // rect would clip that halo and drop diagonal/corner blur, so the V pass
    // decals only at the texture edge ([0,1]) — the H output is already
    // transparent outside its (grown) content.  Matches the morphology V-pass
    // strategy and Impeller's grown-target-per-pass decal.
    let content_rect_uv_v = [0.0_f32, 0.0, 1.0, 1.0];

    // ── H pass: source_tex → h_tex ──────────────────────────────────────────
    let h_tex = resources
        .layer_texture_pool_mut()
        .acquire(viewport_w, viewport_h, surface_format);
    run_blur_sub_pass(
        &BlurUniform {
            texture_size: [viewport_w as f32, viewport_h as f32],
            sigma: sigma_x,
            direction: 0.0, // horizontal
            content_rect_uv: content_rect_uv_h,
        },
        source_tex.view(),
        h_tex.view(),
        pipeline,
        device,
        encoder,
        "Blur H Pass",
    );

    // ── V pass: h_tex → v_tex ───────────────────────────────────────────────
    let v_tex = resources
        .layer_texture_pool_mut()
        .acquire(viewport_w, viewport_h, surface_format);
    run_blur_sub_pass(
        &BlurUniform {
            texture_size: [viewport_w as f32, viewport_h as f32],
            sigma: sigma_y,
            direction: 1.0, // vertical
            content_rect_uv: content_rect_uv_v,
        },
        h_tex.view(),
        v_tex.view(),
        pipeline,
        device,
        encoder,
        "Blur V Pass",
    );
    // `h_tex` drops here, returning to the pool.  Only `v_tex` remains live.
    drop(h_tex);

    v_tex
}

// ── Sub-pass helper ───────────────────────────────────────────────────────────

/// Record one H or V Gaussian blur render pass into `encoder`.
///
/// Writes into `dst_view` using `LoadOp::Clear(TRANSPARENT)` so pixels outside
/// the viewport are transparent (R3 invariant). `REPLACE` blend prevents the
/// GPU from re-blending the premultiplied output.
fn run_blur_sub_pass(
    uniform: &BlurUniform,
    src_view: &wgpu::TextureView,
    dst_view: &wgpu::TextureView,
    pipeline: &BlurPipeline,
    device: &Arc<wgpu::Device>,
    encoder: &mut wgpu::CommandEncoder,
    pass_label: &str,
) {
    // Per-pass uniform buffer (32 bytes — tiny allocation, new each pass so the
    // bind-group can reference its own buffer without a dynamic-offset scheme).
    let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(pass_label),
        contents: cast_slice(&[*uniform]),
        usage: wgpu::BufferUsages::UNIFORM,
    });

    // Linear-filter sampler: bilinear interpolation + ClampToEdge.
    // Bilinear is valid for Gaussian; the continuous kernel naturally composes
    // with bilinear without artefact.  The sampler type must match the texture's
    // `filterable: true` and the bind-group layout's `SamplerBindingType::Filtering`.
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some(pass_label),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(pass_label),
        layout: &pipeline.bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(src_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });

    {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(pass_label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: dst_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    // R3: clear to TRANSPARENT so pixels outside the content
                    // area are transparent — matching the offscreen invariant.
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        render_pass.set_pipeline(&pipeline.pipeline);
        render_pass.set_bind_group(0, &bind_group, &[]);
        // 6 vertices synthesised in the VS — no vertex buffer.
        render_pass.draw(0..6, 0..1);
    }
}
