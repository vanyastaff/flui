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
use wgpu::util::DeviceExt as _;

use pipeline::MorphUniform;
pub(crate) use pipeline::MorphologyPipeline;

use super::{command_ir::MorphOp, resources::GpuResources, texture_pool::PooledTexture};

mod pipeline;

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
/// - `source_tex` — premultiplied RGBA offscreen from `render_segment_to_offscreen`.
/// - `content_bounds` — the AABB of the actual content in physical pixels; used to
///   compute the decal UV rect so the filter does not bleed beyond content edges.
/// - `viewport_size` — `(width, height)` in physical pixels; the output and
///   intermediate textures share this size.
/// - `surface_format` — texture format of the render target.
/// - `pipeline` — the morphology pipeline (bind-group layout + render pipeline).
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
    viewport_size: (u32, u32),
    surface_format: wgpu::TextureFormat,
    pipeline: &MorphologyPipeline,
    resources: &mut GpuResources,
    device: &Arc<wgpu::Device>,
    encoder: &mut wgpu::CommandEncoder,
) -> PooledTexture {
    let (vp_w, vp_h) = viewport_size;

    // Normalise the content AABB to UV coordinates [0, 1] × [0, 1].
    // The H pass decals against the content rect (samples beyond it are
    // transparent black). Inside the texture but outside content, the source is
    // already transparent (cleared by `render_segment_to_offscreen`), so this is
    // equivalent to a texture-edge decal — but it keeps the content edge explicit.
    let content_rect_uv_h = [
        content_bounds.left().0 / vp_w as f32,
        content_bounds.top().0 / vp_h as f32,
        content_bounds.right().0 / vp_w as f32,
        content_bounds.bottom().0 / vp_h as f32,
    ];
    // The V pass reads the H-pass OUTPUT, whose content extent has already grown
    // (dilate) horizontally beyond `content_bounds` into the halo. Decaling the V
    // pass at the original content rect would clip that halo and drop diagonal /
    // corner growth, so the V pass decals only at the texture edge ([0,1]) — the
    // H output is already transparent outside its (grown) content, so this is the
    // correct read region. Matches Impeller's grown-target-per-pass decal.
    let content_rect_uv_v = [0.0_f32, 0.0, 1.0, 1.0];

    // Operation selector: 0.0 = dilate, 1.0 = erode.
    let op_selector = match morph_op {
        MorphOp::Dilate => 0.0_f32,
        MorphOp::Erode => 1.0_f32,
    };

    // ── H pass: source_tex → h_tex ──────────────────────────────────────────
    let h_tex = resources
        .layer_texture_pool_mut()
        .acquire(vp_w, vp_h, surface_format);
    run_morph_sub_pass(
        &MorphUniform {
            texture_size: [vp_w as f32, vp_h as f32],
            radius,
            direction: 0.0, // horizontal
            content_rect_uv: content_rect_uv_h,
            op: op_selector,
            _pad: [0.0; 3],
        },
        source_tex.view(),
        h_tex.view(),
        pipeline,
        device,
        encoder,
        "Morphology H Pass",
    );

    // ── V pass: h_tex → v_tex ───────────────────────────────────────────────
    let v_tex = resources
        .layer_texture_pool_mut()
        .acquire(vp_w, vp_h, surface_format);
    run_morph_sub_pass(
        &MorphUniform {
            texture_size: [vp_w as f32, vp_h as f32],
            radius,
            direction: 1.0, // vertical
            content_rect_uv: content_rect_uv_v,
            op: op_selector,
            _pad: [0.0; 3],
        },
        h_tex.view(),
        v_tex.view(),
        pipeline,
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
fn run_morph_sub_pass(
    uniform: &MorphUniform,
    src_view: &wgpu::TextureView,
    dst_view: &wgpu::TextureView,
    pipeline: &MorphologyPipeline,
    device: &Arc<wgpu::Device>,
    encoder: &mut wgpu::CommandEncoder,
    pass_label: &str,
) {
    // Per-pass uniform buffer (48 bytes — tiny allocation, new each pass so the
    // bind-group can reference its own buffer without a dynamic-offset scheme).
    let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(pass_label),
        contents: cast_slice(&[*uniform]),
        usage: wgpu::BufferUsages::UNIFORM,
    });

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
