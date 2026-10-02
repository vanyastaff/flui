//! Destination-reading composite driver: backdrop copy + coverage selection.
//!
//! The internal entry points are:
//!
//! - `copy_backdrop_region` — copies a device-space rect from the surface
//!   texture into a pooled offscreen texture so the compositor shader can
//!   sample it without racing the render target write.
//!
//! - `flush_advanced_layer` — runs the advanced-blend composite pass for one
//!   `AdvancedBlendOp`: copies the backdrop, builds the bind group, and
//!   executes one render pass over the op's device-space bounds.
//!
//! The shader supports advanced and Porter-Duff modes, applying clip coverage
//! to the finished composite rather than replacing geometric coverage with
//! source opacity. The synthetic GPU family checks full and fractional coverage.

use bytemuck::cast_slice;
use flui_foundation::geometry::Rect;
use flui_painting::paint::BlendMode;

pub(crate) use pipeline::AdvancedBlendPipeline;
pub(crate) use pipeline::mode_to_u32;

use generated::advanced_blend;

use crate::{resources::GpuResources, texture_pool::PooledTexture};

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
    pub(crate) device_bounds: Rect<f64>,
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
    /// A hard rectangular clip, in its own local space, that the composite
    /// leaves the destination alone outside: the bounds of a rotated or
    /// skewed layer, whose `device_bounds` are their bounding box. Its corner
    /// radii and its `kind` lanes are not read, so only the clip
    /// `WgpuPainter::composite_clip` builds from a layer's bounds belongs
    /// here. `None` for no clip.
    pub(crate) clip: Option<crate::state_stack::ResolvedClip>,
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
    device_rect: Rect<f64>,
    surface_format: wgpu::TextureFormat,
    resources: &mut GpuResources,
    encoder: &mut wgpu::CommandEncoder,
) -> Option<BackdropSample> {
    let surface_size = surface_texture.size();
    let surface_w = surface_size.width;
    let surface_h = surface_size.height;

    // The copy region covers every pixel the device rect touches, clamped to
    // the surface (ADR-0098 §6; the rule the backdrop filter uses too).
    let covered = flui_foundation::geometry::cover(device_rect);
    let x = covered.left().clamp(0.0, f64::from(surface_w)) as u32;
    let y = covered.top().clamp(0.0, f64::from(surface_h)) as u32;
    let right = covered.right().clamp(0.0, f64::from(surface_w)) as u32;
    let bottom = covered.bottom().clamp(0.0, f64::from(surface_h)) as u32;

    // Entirely off-screen after clamping → no copy possible.
    if right <= x || bottom <= y {
        tracing::warn!(
            bounds_l = device_rect.left(),
            bounds_t = device_rect.top(),
            bounds_r = device_rect.right(),
            bounds_b = device_rect.bottom(),
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
#[expect(clippy::too_many_arguments)]
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
    mask_binding: Option<&wgpu::BindGroup>,
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
    // A zero-size rectangle is the shader's "no clip".
    let (clip_rect, clip_inv, clip_origin) =
        op.clip
            .map_or(([0.0; 4], [1.0, 0.0, 0.0, 1.0], [0.0; 2]), |clip| {
                let [left, top, width, height, ..] = clip.rrect;
                let [m0, m1, m2, m3, tx, ty] = clip.device_to_local;
                ([left, top, width, height], [m0, m1, m2, m3], [tx, ty])
            });

    // The generated `BlendUniforms::new` zero-fills the WGSL alignment padding
    // (`_pad0`); fields are passed in WGSL declaration order minus the pad.
    let uniforms = advanced_blend::BlendUniforms::new(
        [
            (op.device_bounds.left() as f32),
            (op.device_bounds.top() as f32),
            (op.device_bounds.width() as f32),
            (op.device_bounds.height() as f32),
        ],
        [vp_w as f32, vp_h as f32],
        [copy_origin_x as f32, copy_origin_y as f32],
        [copy_extent_w as f32, copy_extent_h as f32],
        op.opacity,
        op.tint,
        mode_to_u32(op.mode),
        op.src_uv_min,
        op.src_uv_max,
        clip_rect,
        clip_inv,
        clip_origin,
    );

    let uniform_buffer = resources.uniform_pool_mut().alloc(cast_slice(&[uniforms]));

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
                buffer: uniform_buffer,
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
        render_pass.set_bind_group(
            1,
            mask_binding
                .expect("BUG: composite pass requires its enabled or disabled clip binding"),
            &[],
        );
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

#[cfg(all(test, feature = "testing"))]
mod synthetic_op_tests {
    use std::sync::Arc;

    use flui_foundation::geometry::Rect;
    use flui_painting::{paint::BlendMode, styling::Color};
    use wgpu::util::DeviceExt as _;

    use super::{AdvancedBlendOp, AdvancedBlendPipeline, flush_advanced_layer};
    use crate::{resources::GpuResources, texture_pool::TexturePool};

    // ── GPU test harness ──────────────────────────────────────────────────────

    fn request_device_and_queue() -> (Arc<wgpu::Device>, Arc<wgpu::Queue>) {
        crate::test_support::test_device_and_queue("AdvancedBlend Synthetic Test Device")
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
    #[expect(clippy::too_many_arguments)]
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
        crate::test_support::readback_pixels(device, queue, texture, w, h)
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
        let f32_to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let r_pm = f32_to_u8(r * a);
        let g_pm = f32_to_u8(g * a);
        let b_pm = f32_to_u8(b * a);
        let a_u8 = f32_to_u8(a);
        [r_pm, g_pm, b_pm, a_u8]
    }

    fn clip_fixture_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        let uniform = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            count: None,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
        };
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Composite clip fixture"),
            entries: &[
                uniform(0, wgpu::ShaderStages::VERTEX),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    count: None,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                },
                uniform(2, wgpu::ShaderStages::FRAGMENT),
            ],
        })
    }

    fn clip_fixture(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        coverage: Option<u8>,
    ) -> wgpu::BindGroup {
        let viewport = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Composite fixture viewport"),
            contents: &[0; 16],
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let consumer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Composite fixture consumer"),
            contents: bytemuck::cast_slice(&[0_i32, 0, i32::from(coverage.is_some()), 0]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Composite fixture mask"),
            size: wgpu::Extent3d {
                width: TARGET_W,
                height: TARGET_H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            &vec![coverage.unwrap_or(255); (TARGET_W * TARGET_H) as usize],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(TARGET_W),
                rows_per_image: Some(TARGET_H),
            },
            texture.size(),
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Composite fixture binding"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: viewport.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: consumer.as_entire_binding(),
                },
            ],
        })
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
            BlendMode::Clear,
            BlendMode::Src,
            BlendMode::Dst,
            BlendMode::SrcOver,
            BlendMode::DstOver,
            BlendMode::SrcIn,
            BlendMode::DstIn,
            BlendMode::SrcOut,
            BlendMode::DstOut,
            BlendMode::SrcATop,
            BlendMode::DstATop,
            BlendMode::Xor,
            BlendMode::Plus,
            BlendMode::Modulate,
        ];

        let mask_layout = clip_fixture_layout(&device);
        let disabled_mask = clip_fixture(&device, &queue, &mask_layout, None);
        let half_mask = clip_fixture(&device, &queue, &mask_layout, Some(128));
        let pipeline = AdvancedBlendPipeline::new(&device, TEST_FORMAT, &mask_layout);
        let mut pool = TexturePool::new(Arc::clone(&device));
        let mut resources = GpuResources::new(crate::device_domain::DeviceDomain::new(
            Arc::clone(&device),
            Arc::clone(&queue),
        ));

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

        for (coverage, mask_binding) in [(None, &disabled_mask), (Some(128_u8), &half_mask)] {
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
                let surface_view =
                    surface_texture.create_view(&wgpu::TextureViewDescriptor::default());

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
                        0.0,
                        0.0,
                        f64::from(TARGET_W as f32),
                        f64::from(TARGET_H as f32),
                    ),
                    opacity: 1.0,
                    tint: [1.0, 1.0, 1.0],
                    // Full-viewport foreground: identity UV remap.
                    src_uv_min: [0.0, 0.0],
                    src_uv_max: [1.0, 1.0],
                    clip: None,
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
                    Some(mask_binding),
                );

                queue.submit(std::iter::once(encoder.finish()));

                // Read back result.
                let pixels =
                    readback_texture(&device, &queue, &surface_texture, TARGET_W, TARGET_H);

                // Compute oracles.
                let covered = |destination: Color| {
                    let result = oracle_pixel(src_straight, destination, mode);
                    let Some(coverage) = coverage else {
                        return result;
                    };
                    let destination = oracle_pixel(destination, destination, BlendMode::Src);
                    let c = f32::from(coverage) / 255.0;
                    std::array::from_fn(|i| {
                        (f32::from(destination[i]) * (1.0 - c) + f32::from(result[i]) * c).round()
                            as u8
                    })
                };
                let expected_left = covered(dst_left_straight);
                let expected_right = covered(dst_right_straight);

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
        }

        // Drop fg_pooled after all modes — it was only used as the staging source.
        drop(fg_pooled);
    }

    // ── Edge-case tests ───────────────────────────────────────────────────────
}
