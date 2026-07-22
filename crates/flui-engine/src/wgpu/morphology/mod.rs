//! Separable morphological filter: dilate (per-channel max) or erode (per-channel min).
//!
//! The only public entry point is `apply_morphology`, called by
//! `apply_image_filter_passes` when an `ImageFilterPass::Morph` is in the
//! pass chain.
//!
//! ## Two-sub-pass design
//!
//! A separable morphological filter is O(N) per pixel rather than O(N²):
//!
//! 1. **H pass** (`direction = 0`): read `source_tex`, write `h_tex` — horizontal scan.
//! 2. **V pass** (`direction = 1`): read `h_tex`,     write `v_tex` — vertical scan.
//!
//! At most **two** pooled textures are live simultaneously (source is caller-owned,
//! `h_tex` lives only between the two sub-passes and is dropped before returning).
//!
//! ## Premultiplied-direct invariant (PINNED #1)
//!
//! max/min operates directly on premultiplied RGBA — NO unpremultiply step.
//! The CPU oracle in `morphology_filter_tests.rs` follows the same contract.
//!
//! ## Decal semantics
//!
//! wgpu has no `AddressMode::Decal`.  The shader implements decal in-shader:
//! samples outside `content_bounds_uv` return the neutral element (transparent
//! black for dilate, opaque white for erode) rather than clamping the UV.

use std::sync::Arc;

use bytemuck::cast_slice;
use flui_types::{Rect, geometry::Pixels};

pub(crate) use pipeline::MorphologyPipeline;

use super::{
    command_ir::MorphOp, resources::GpuResources, texture_pool::PooledTexture,
    uniform_pool::UniformPool,
};

mod generated;
mod pipeline;

use generated::morphology;

/// Apply a morphological filter (dilate or erode) to `source_tex` via two
/// separable sub-passes (H then V), returning the filtered texture.
///
/// The caller must composite (or otherwise use) the returned texture before
/// dropping it.  Dropping without compositing returns the texture to the pool
/// silently — not a soundness error, but produces an invisible layer.
///
/// ## Parameters
///
/// - `radius` — kernel half-radius in physical pixels; the shader samples
///   `[-ceil(radius) ..= ceil(radius)]` texels in each direction.
/// - `morph_op` — `Dilate` (max) or `Erode` (min).
/// - `source_tex` — premultiplied RGBA offscreen from `render_segment_to_grown_offscreen`.
/// - `content_bounds` — the AABB of the actual content in **full-frame** physical pixels;
///   rebased to fb-local UV by subtracting `fb_origin` before dividing by `fb_dim`.
/// - `fb_origin` — integer-aligned top-left of the offscreen frame in device pixels.
///   Used to rebase `content_bounds` to fb-local UV (non-negotiable #3).
/// - `fb_dim` — integer dimensions `(width, height)` of the source texture and all
///   intermediate textures. The shader's `texture_size` uniform is set to `fb_dim`
///   (non-negotiable #2: denominator is integer fb_dim, not viewport_size).
/// - `surface_format` — texture format of the render target.
/// - `pipeline` — the morphology pipeline (render pipeline + generated bind-group helpers).
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
pub(crate) fn apply_morphology(
    radius: f32,
    morph_op: MorphOp,
    source_tex: &PooledTexture,
    content_bounds: Rect<Pixels>,
    fb_origin: (u32, u32),
    fb_dim: (u32, u32),
    surface_format: wgpu::TextureFormat,
    pipeline: &MorphologyPipeline,
    resources: &mut GpuResources,
    device: &Arc<wgpu::Device>,
    encoder: &mut wgpu::CommandEncoder,
) -> PooledTexture {
    let (fb_w, fb_h) = fb_dim;
    let (fb_origin_x, fb_origin_y) = fb_origin;

    // Rebase the content AABB to fb-local UV coordinates (non-negotiable #3).
    //
    // The source texture is fb_dim-sized with pixel (0,0) = fb_origin in device space.
    // The H-pass decal guard compares sample UV against content_rect_uv; without
    // subtracting fb_origin the UV is in viewport space, which would be wrong for an
    // off-origin grown-bounds texture (content UV would not fall in [0, fb_dim]).
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
    // The V pass reads the H-pass OUTPUT, whose content extent has already grown
    // (dilate) horizontally beyond `content_bounds` into the halo. Decaling the V
    // pass at the original content rect would clip that halo and drop diagonal /
    // corner growth, so the V pass decals only at the texture edge ([0,1]) — the
    // H output is already transparent outside its (grown) content.
    let content_rect_uv_v = [0.0_f32, 0.0, 1.0, 1.0];

    // Operation selector: 0.0 = dilate, 1.0 = erode.
    let op_selector = match morph_op {
        MorphOp::Dilate => 0.0_f32,
        MorphOp::Erode => 1.0_f32,
    };

    // ── H pass: source_tex → h_tex ──────────────────────────────────────────
    //
    // texture_size = fb_dim (non-negotiable #2): the shader uses texture_size to
    // convert kernel offsets to UV deltas. Using viewport_size for a smaller
    // fb_dim texture over-shrinks the UV step → effectively widens the kernel.
    #[allow(
        clippy::cast_precision_loss,
        reason = "fb_w/fb_h are u32 texture dims ≤ viewport; f32 precision is sufficient"
    )]
    let h_tex = resources
        .layer_texture_pool_mut()
        .acquire(fb_w, fb_h, surface_format);
    run_morph_sub_pass(
        morphology::MorphUniforms::new(
            [fb_w as f32, fb_h as f32],
            radius,
            0.0, // horizontal
            content_rect_uv_h,
            op_selector,
        ),
        source_tex.view(),
        h_tex.view(),
        pipeline,
        resources.uniform_pool_mut(),
        device,
        encoder,
        "Morphology H Pass",
    );

    // ── V pass: h_tex → v_tex ───────────────────────────────────────────────
    let v_tex = resources
        .layer_texture_pool_mut()
        .acquire(fb_w, fb_h, surface_format);
    run_morph_sub_pass(
        morphology::MorphUniforms::new(
            [fb_w as f32, fb_h as f32],
            radius,
            1.0, // vertical
            content_rect_uv_v,
            op_selector,
        ),
        h_tex.view(),
        v_tex.view(),
        pipeline,
        resources.uniform_pool_mut(),
        device,
        encoder,
        "Morphology V Pass",
    );
    // `h_tex` drops here, returning to the pool.  Only `v_tex` remains live.
    drop(h_tex);

    v_tex
}

// ── Sub-pass helper ───────────────────────────────────────────────────────────

/// Record one H or V morphology render pass into `encoder`.
///
/// Writes into `dst_view` using `LoadOp::Clear(TRANSPARENT)` so pixels outside
/// the viewport are transparent (R3 invariant). `REPLACE` blend prevents the
/// GPU from re-blending the premultiplied output.
#[allow(
    clippy::too_many_arguments,
    reason = "GPU sub-pass needs the uniform, src/dst views, pipeline, pool, device, encoder, and label"
)]
fn run_morph_sub_pass(
    uniform: morphology::MorphUniforms,
    src_view: &wgpu::TextureView,
    dst_view: &wgpu::TextureView,
    pipeline: &MorphologyPipeline,
    uniform_pool: &mut UniformPool,
    device: &Arc<wgpu::Device>,
    encoder: &mut wgpu::CommandEncoder,
    pass_label: &str,
) {
    // Uniform (48 bytes) written into a frame-distinct buffer from the reusable
    // pool — no per-pass allocation; each pass still binds its own buffer.
    let uniform_buffer = uniform_pool.alloc(cast_slice(&[uniform]));

    // Nearest-clamp sampler: source texels are pixel-aligned with the output;
    // bilinear filtering would introduce colour error between morphology steps.
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some(pass_label),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });

    // Per-draw bind group via the generated typed helper.  `WgpuBindGroup0::from_bindings`
    // recreates the layout from the WGSL-derived descriptor, so no shared layout object
    // needs to be threaded through from the pipeline.
    let bind_group = morphology::WgpuBindGroup0::from_bindings(
        device,
        morphology::WgpuBindGroup0Entries::new(morphology::WgpuBindGroup0EntriesParams {
            u: wgpu::BufferBinding {
                buffer: uniform_buffer,
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
