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

use super::{resources::GpuResources, texture_pool::PooledTexture};

mod generated;
mod pipeline;

use generated::blur;

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
/// - `source_tex` — premultiplied RGBA offscreen from `render_segment_to_grown_offscreen`.
/// - `content_bounds` — AABB of the content in **full-frame** physical pixels; rebased
///   to fb-local UV by subtracting `fb_origin` before dividing by `fb_dim`.
/// - `fb_origin` — integer-aligned top-left of the offscreen frame in device pixels
///   (computed in `painter::layer`). Used to rebase `content_bounds` to fb-local UV
///   (non-negotiable #3: `content_rect_uv = (content_bounds - fb_origin) / fb_dim`).
/// - `fb_dim` — integer dimensions `(width, height)` of the source texture and all
///   intermediate textures acquired here. The blur shader's `texture_size` uniform
///   MUST be `fb_dim`, not `viewport_size` (non-negotiable #2: denominator is integer
///   fb_dim to avoid the SSAA-tile-denominator bug class).
/// - `surface_format` — texture format of the render target.
/// - `pipeline` — the blur pipeline (render pipeline + generated bind-group helpers).
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
    fb_origin: (u32, u32),
    fb_dim: (u32, u32),
    surface_format: wgpu::TextureFormat,
    pipeline: &BlurPipeline,
    resources: &mut GpuResources,
    device: &Arc<wgpu::Device>,
    encoder: &mut wgpu::CommandEncoder,
) -> PooledTexture {
    let (fb_w, fb_h) = fb_dim;
    let (fb_origin_x, fb_origin_y) = fb_origin;

    // Rebase the content AABB to fb-local UV coordinates (non-negotiable #3).
    //
    // The source texture is fb_dim-sized, with pixel (0,0) = fb_origin in device
    // space. The H-pass decal guard compares sample UV against content_rect_uv;
    // without subtracting fb_origin the guard is in viewport UV space, which is
    // correct for a full-viewport source but WRONG for a fb_dim-sized source —
    // every content UV would be shifted by fb_origin/viewport, landing outside [0,1]
    // for off-origin content and clipping the blur decal to the wrong region.
    #[allow(
        clippy::cast_precision_loss,
        reason = "fb coords and content_bounds are ≤ viewport ≤ ~16 M px; f32 precision is sufficient"
    )]
    let content_rect_uv_h = [
        (content_bounds.left().0 - fb_origin_x as f32) / fb_w as f32,
        (content_bounds.top().0 - fb_origin_y as f32) / fb_h as f32,
        (content_bounds.right().0 - fb_origin_x as f32) / fb_w as f32,
        (content_bounds.bottom().0 - fb_origin_y as f32) / fb_h as f32,
    ];
    // The V pass reads the H-pass output whose content extent has already grown
    // horizontally into the halo. Decaling the V pass at the original content
    // rect would clip that halo and drop diagonal/corner blur, so the V pass
    // decals only at the texture edge ([0,1]) — the H output is already
    // transparent outside its (grown) content.  Matches the morphology V-pass
    // strategy and Impeller's grown-target-per-pass decal.
    let content_rect_uv_v = [0.0_f32, 0.0, 1.0, 1.0];

    // ── H pass: source_tex → h_tex ──────────────────────────────────────────
    //
    // texture_size = fb_dim, NOT viewport_size (non-negotiable #2):
    // The blur shader divides sample offsets by texture_size to get UV steps.
    // Using the full viewport size for a fb_dim-sized texture scales the kernel
    // offsets down by (fb/vp), effectively widening the blur to (sigma * vp/fb)
    // texels — incorrect. The shader must see the actual texture dimensions.
    #[allow(
        clippy::cast_precision_loss,
        reason = "fb_w/fb_h are u32 texture dims ≤ viewport; f32 precision is sufficient"
    )]
    let h_tex = resources
        .layer_texture_pool_mut()
        .acquire(fb_w, fb_h, surface_format);
    run_blur_sub_pass(
        blur::BlurUniforms::new(
            [fb_w as f32, fb_h as f32],
            sigma_x,
            0.0, // horizontal
            content_rect_uv_h,
        ),
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
        .acquire(fb_w, fb_h, surface_format);
    run_blur_sub_pass(
        blur::BlurUniforms::new(
            [fb_w as f32, fb_h as f32],
            sigma_y,
            1.0, // vertical
            content_rect_uv_v,
        ),
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
    uniform: blur::BlurUniforms,
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
        contents: cast_slice(&[uniform]),
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

    // Per-draw bind group via the generated typed helper.  `WgpuBindGroup0::from_bindings`
    // recreates the layout from the WGSL-derived descriptor, so no shared layout object
    // needs to be threaded through from the pipeline.
    let bind_group = blur::WgpuBindGroup0::from_bindings(
        device,
        blur::WgpuBindGroup0Entries::new(blur::WgpuBindGroup0EntriesParams {
            u: wgpu::BufferBinding {
                buffer: &uniform_buffer,
                offset: 0,
                size: None,
            },
            src_texture: src_view,
            src_sampler: &sampler,
        }),
    );

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
        bind_group.set(&mut render_pass);
        // 6 vertices synthesised in the VS — no vertex buffer.
        render_pass.draw(0..6, 0..1);
    }
}
