//! Surface colour selection and layer compositing, read back from real GPU
//! textures: the SDR pair a surface is configured with, followers resolved
//! across repaint boundaries, shader masks at the root and over external
//! textures, and (on Windows) tessellated clips through optimized FXC.
//!
//! Each case self-skips without an adapter, except the FXC case, which
//! requires a DX12 adapter where it is compiled.

use std::sync::Arc;

use crate::error::EngineError;
use crate::surface::color::OutputEncoding;

fn surface_caps(
    pairs: &[(wgpu::TextureFormat, wgpu::SurfaceColorSpaces)],
) -> wgpu::SurfaceCapabilities {
    wgpu::SurfaceCapabilities {
        formats: pairs.iter().map(|&(format, _)| format).collect(),
        format_capabilities: pairs
            .iter()
            .map(|&(format, color_spaces)| wgpu::SurfaceFormatCapabilities {
                format,
                color_spaces,
            })
            .collect(),
        ..Default::default()
    }
}

fn sdr_surface_selection_rejects_incompatible_pairs() {
    use wgpu::{SurfaceColorSpaces as Spaces, TextureFormat as Format};
    for pairs in [
        vec![],
        vec![(Format::Rgba16Float, Spaces::EXTENDED_SRGB_LINEAR)],
        vec![(Format::Bgra8Unorm, Spaces::EXTENDED_SRGB_LINEAR)],
    ] {
        let error = crate::surface::color::select_sdr(&surface_caps(&pairs))
            .expect_err("unsupported color contract");
        assert_eq!(
            error.recoverability(),
            crate::error::Recoverability::Unrecoverable
        );
        assert!(
            matches!(error, EngineError::UnsupportedSurfaceColorConfiguration { supported } if supported.len() == pairs.len())
        );
    }
    let caps = surface_caps(&[
        (Format::Bgra8Unorm, Spaces::EXTENDED_SRGB_LINEAR),
        (Format::Rgba8Unorm, Spaces::SRGB),
    ]);
    assert_eq!(
        crate::surface::color::select_sdr(&caps)
            .expect("RGBA alternative")
            .format,
        Format::Rgba8Unorm
    );
}

fn sdr_surface_selection_painter_readback_preserves_swatches_and_blending() {
    use crate::painter::WgpuPainter;
    use flui_foundation::geometry::Rect;
    use flui_painting::paint::Paint;
    use flui_painting::styling::Color;
    use wgpu::{SurfaceColorSpaces as Spaces, TextureFormat as Format};

    let (device, queue) = crate::test_support::test_device_and_queue("SDR transfer regression");
    for alternative in [Format::Bgra8Unorm, Format::Rgba8Unorm] {
        let caps = surface_caps(&[
            (Format::Rgba16Float, Spaces::EXTENDED_SRGB_LINEAR),
            (alternative, Spaces::SRGB),
        ]);
        let choice = crate::surface::color::select_sdr(&caps).expect("SDR pair");
        let format = choice.format;
        assert_eq!(choice.output, OutputEncoding::Identity);
        let size = 64;
        let (target, view) = crate::test_support::create_target(
            &device,
            "SDR swatches",
            size,
            size,
            format,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        crate::test_support::clear_target(&device, &queue, &view, wgpu::Color::BLACK);
        let mut painter = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            format,
            (size, size),
        );
        let colors = [
            Color::rgb(18, 18, 18),
            Color::rgb(24, 24, 24),
            Color::rgb(128, 128, 128),
            Color::rgb(229, 57, 53),
            Color::rgb(255, 0, 0),
            Color::rgba(128, 128, 128, 128),
        ];
        for (index, color) in colors.iter().enumerate() {
            painter.draw_rect(
                Rect::from_xywh(f64::from(index as f32 * 10.0), 0.0, 10.0, 64.0),
                &Paint::fill(*color),
            );
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        painter
            .render_to_view(&view, &mut encoder)
            .expect("paint swatches");
        queue.submit([encoder.finish()]);
        let bytes = crate::test_support::readback_bytes(&device, &queue, &target, size, size);
        for (index, expected) in [
            [18, 18, 18],
            [24, 24, 24],
            [128, 128, 128],
            [229, 57, 53],
            [255, 0, 0],
            [64, 64, 64],
        ]
        .iter()
        .enumerate()
        {
            let offset = (32 * size as usize + index * 10 + 5) * 4;
            let raw = &bytes[offset..offset + 4];
            let rgb = if format == Format::Bgra8Unorm {
                [raw[2], raw[1], raw[0]]
            } else {
                [raw[0], raw[1], raw[2]]
            };
            for channel in 0..3 {
                assert!(
                    (i32::from(rgb[channel]) - expected[channel]).abs() <= 1,
                    "{format:?} swatch {index}: {rgb:?}, expected {expected:?}"
                );
            }
            assert_eq!(raw[3], 255, "opaque black underlay");
        }
    }
}

/// Acquire a real device/queue for the readbacks below.
/// Returns `None` when no GPU adapter is available (CI without a GPU).
fn test_device_and_queue() -> Option<(Arc<wgpu::Device>, Arc<wgpu::Queue>)> {
    crate::test_support::try_test_device_and_queue("Compositing Readback Test Device")
}

/// Helper: GPU-copy the first four bytes of a texture into a host Vec.
/// Returns `None` when no GPU is available in this environment.
fn readback_rgba_pixel(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    x: u32,
    y: u32,
) -> Option<[u8; 4]> {
    // 256-byte-aligned staging buffer (wgpu requirement: bytes_per_row % 256 == 0)
    // For a single texel readback we only need 4 bytes of payload, but the
    // buffer must be at least 256 bytes to satisfy `MAP_READ` alignment.
    let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Readback Staging Buffer"),
        size: 256,
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
            origin: wgpu::Origin3d { x, y, z: 0 },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &staging_buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(std::iter::once(encoder.finish()));

    // Map synchronously: submit, poll-wait, then read.
    staging_buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, |_| {});
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .ok()?;

    // The `?`s above return `None` for "no usable device", which is what
    // this helper's callers read. A map that fails AFTER the poll-wait
    // succeeded is not that — it is a broken invariant, and folding it into
    // the same `None` would report a real failure as an absent GPU.
    let mapped = staging_buffer
        .slice(..4)
        .get_mapped_range()
        .expect("staging buffer must be mapped: the poll above waited for the map to complete");
    let bytes: [u8; 4] = mapped[..4].try_into().ok()?;
    Some(bytes)
}

// =========================================================================
// Tier-2 Follower render-time resolution — GPU-level, end-to-end
// pixel-readback proof.
//
// Render-object-harness-level testing cannot check on-screen positioning
// (explicitly out of harness scope) — these
// tests exercise the REAL layer walk's (`layers::walk::record_scene`) Follower special case
// (not a hand-rolled stand-in) against a real GPU texture, reading back
// actual rendered pixels.
// =========================================================================

/// Clears `view` to `color`. Mirrors `SwapchainFrame::clear`'s body;
/// duplicated here because these tests build `WgpuPainter`/`LayerDispatcher`
/// manually rather than through a window slot, so
/// the swapchain frame's clear step isn't reachable.
/// `painter.render` uses `LoadOp::Load`, so every one of these tests must
/// pre-clear its target.
fn clear_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    view: &wgpu::TextureView,
    color: wgpu::Color,
) {
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Follower Test Clear Encoder"),
    });
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Follower Test Clear Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(color),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }
    queue.submit(std::iter::once(encoder.finish()));
}

/// (a) A `Layer::Follower` linked to a `Layer::Leader` under a DIFFERENT
/// `Layer::Offset` ancestor (the cross-repaint-boundary case that
/// motivated the whole render-time-resolution design) must
/// render its subtree at the LEADER's resolved position, not at its own
/// natural (pre-resolution) tree position.
fn follower_gpu_renders_at_resolved_position_across_repaint_boundaries() {
    use crate::frame::render_target::RenderTarget;
    use crate::layers::dispatcher::LayerDispatcher;
    use crate::painter::WgpuPainter;
    use flui_foundation::geometry::{Offset, Rect, Size};
    use flui_layer::{
        FollowerLayer, Layer, LayerLink, LayerTree, LeaderLayer, OffsetLayer, PictureLayer,
    };
    use flui_painting::styling::Color;
    use flui_painting::{Canvas, paint::Paint};

    let Some((device, queue)) = test_device_and_queue() else {
        return; // No GPU — skip gracefully.
    };

    let format = wgpu::TextureFormat::Rgba8Unorm;
    let width = 200u32;
    let height = 200u32;

    let link = LayerLink::new();
    let mut tree = LayerTree::new(Layer::Offset(OffsetLayer::zero()));

    let root_id = tree.root();

    // Leader lives under `branch_a`, offset (60,0) from root.
    let branch_a = tree.push_child(
        root_id,
        Layer::Offset(OffsetLayer::new(Offset::new(60.0, 0.0))),
    );
    let _leader_id = tree.push_child(
        branch_a,
        Layer::Leader(LeaderLayer::with_offset(
            link,
            Size::new(20.0, 20.0),
            Offset::new(5.0, 5.0),
        )),
    );

    // Follower lives under a DIFFERENT boundary, `branch_b`, offset
    // (0,90) from root.
    let branch_b = tree.push_child(
        root_id,
        Layer::Offset(OffsetLayer::new(Offset::new(0.0, 90.0))),
    );
    let follower_id = tree.push_child(
        branch_b,
        Layer::Follower(FollowerLayer::new(link).with_size(Size::new(10.0, 10.0))),
    );

    // The follower's child: a 10×10 opaque red rect at its own local origin.
    let mut canvas = Canvas::new();
    canvas.draw_rect(
        Rect::from_xywh(0.0, 0.0, 10.0, 10.0),
        &Paint::fill(Color::rgba(255, 0, 0, 255)),
    );
    let _child_id = tree.push_child(follower_id, Layer::from(PictureLayer::new(canvas.finish())));

    let render_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Follower GPU Test Texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let render_view = render_texture.create_view(&wgpu::TextureViewDescriptor::default());
    clear_texture(&device, &queue, &render_view, wgpu::Color::WHITE);

    let mut painter = WgpuPainter::with_shared_device(
        Arc::clone(&device),
        Arc::clone(&queue),
        format,
        (width, height),
    );
    let mut backend = LayerDispatcher::new(&mut painter);

    crate::layers::walk::record_scene(
        &flui_layer::SceneVersion::from_tree(&tree),
        None,
        crate::record::cache::CacheReads::On,
        &mut backend,
    )
    .expect("layer tree records");
    drop(backend);

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Follower GPU Test Render Encoder"),
    });
    let render_target = RenderTarget::sampleable(&render_view, &render_texture);
    painter
        .render(render_target, &mut encoder)
        .expect("painter.render must succeed on a GPU-enabled host");
    queue.submit(std::iter::once(encoder.finish()));

    // Resolved leader-relative position: branch_a (60,0) + leader local
    // (5,5) = (65,5). Sample a couple pixels inset to dodge edge AA.
    let resolved_pixel = readback_rgba_pixel(&device, &queue, &render_texture, 67, 7)
        .expect("resolved-position readback must succeed");
    assert!(
        resolved_pixel[0] > 200 && resolved_pixel[1] < 50,
        "follower content must render at the LEADER's resolved position \
         (65,5)-ish; expected opaque red, got {resolved_pixel:?}"
    );

    // The follower's own NATURAL (pre-resolution) tree position: branch_b
    // (0,90) + local (0,0). Must stay untouched white background,
    // proving the content actually MOVED to the leader's position rather
    // than painting at (or in addition to) its natural slot.
    let natural_pixel = readback_rgba_pixel(&device, &queue, &render_texture, 2, 92)
        .expect("natural-position readback must succeed");
    assert!(
        natural_pixel[1] > 200,
        "follower must NOT render at its own natural (pre-resolution) \
         tree position; expected untouched white background, got {natural_pixel:?}"
    );
}

// =========================================================================
// `Layer::ShaderMask` GPU readback: the layer's shader and blend mode must
// reach the pixels. The generic `LayerRender` dispatch (`layers/render.rs`)
// pushes an inert `save_layer`/`push_clip_rect` pair that never reads them,
// so `ShaderMask` has a special case of its own. These tests exercise the
// REAL `render_layer_recursive` special case (not a hand-rolled stand-in)
// against a real GPU texture, reading back actual rendered pixels, the same
// style as the Follower Tier-2 tests above.
// =========================================================================

fn shader_mask_layer_root_gpu_pixel_readback_reflects_mask() {
    use crate::frame::render_target::RenderTarget;
    use crate::layers::dispatcher::LayerDispatcher;
    use crate::painter::WgpuPainter;
    use flui_foundation::geometry::Rect;
    use flui_layer::{Layer, LayerTree, PictureLayer, ShaderMaskLayer};
    use flui_painting::{Canvas, paint::Paint, paint::Shader};
    use flui_painting::{paint::BlendMode, styling::Color};

    let Some((device, queue)) = test_device_and_queue() else {
        return; // No GPU — skip gracefully.
    };

    let format = wgpu::TextureFormat::Rgba8Unorm;
    let width = 64u32;
    let height = 64u32;

    let bounds = Rect::from_xywh(0.0, 0.0, 64.0, 64.0);
    let shader = Shader::solid(Color::rgba(10, 20, 30, 128));
    let mut tree = LayerTree::new(Layer::ShaderMask(ShaderMaskLayer::new(
        shader,
        BlendMode::SrcOver,
        bounds,
    )));
    let mask_id = tree.root();

    let mut canvas = Canvas::new();
    canvas.draw_rect(bounds, &Paint::fill(Color::rgba(255, 0, 0, 255)));
    let _child_id = tree.push_child(mask_id, Layer::from(PictureLayer::new(canvas.finish())));

    let render_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ShaderMask Root GPU Test Texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let render_view = render_texture.create_view(&wgpu::TextureViewDescriptor::default());
    clear_texture(&device, &queue, &render_view, wgpu::Color::WHITE);

    let mut painter = WgpuPainter::with_shared_device(
        Arc::clone(&device),
        Arc::clone(&queue),
        format,
        (width, height),
    );
    let mut backend = LayerDispatcher::new(&mut painter);

    crate::layers::walk::record_scene(
        &flui_layer::SceneVersion::from_tree(&tree),
        None,
        crate::record::cache::CacheReads::On,
        &mut backend,
    )
    .expect("layer tree records");
    drop(backend);

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("ShaderMask Root GPU Test Render Encoder"),
    });
    let render_target = RenderTarget::sampleable(&render_view, &render_texture);
    painter
        .render(render_target, &mut encoder)
        .expect("painter.render must succeed on a GPU-enabled host");
    queue.submit(std::iter::once(encoder.finish()));

    let pixel = readback_rgba_pixel(&device, &queue, &render_texture, 32, 32)
        .expect("center readback must succeed");
    // SrcOver combines the coloured shader with the opaque red child;
    // the result is opaque before it reaches the white parent.
    let alpha = 128.0_f64 / 255.0;
    let expected = [
        (10.0 * alpha + 255.0 * (1.0 - alpha)).round() as u8,
        (20.0 * alpha).round() as u8,
        (30.0 * alpha).round() as u8,
        255,
    ];
    assert!(
        pixel.iter().zip(expected).all(|(&a, b)| a.abs_diff(b) <= 2),
        "shader SrcOver child: {pixel:?}, expected {expected:?}"
    );
}

/// Surface selection and compositor readbacks, one row per feature: SDR pair
/// selection (rejections and swatch/blending readback), followers across
/// repaint boundaries, the shader-mask layer root and its external textures,
/// and the optimized FXC clip on Windows.
#[test]
fn surface_selection_and_layer_compositing_read_back_as_specified() {
    #[cfg(target_os = "windows")]
    optimized_fxc_tessellated_clip_readback();
    shader_mask_external_texture_registrations_follow_parent();
    sdr_surface_selection_rejects_incompatible_pairs();
    sdr_surface_selection_painter_readback_preserves_swatches_and_blending();
    follower_gpu_renders_at_resolved_position_across_repaint_boundaries();
    shader_mask_layer_root_gpu_pixel_readback_reflects_mask();
}

#[cfg(target_os = "windows")]
fn optimized_fxc_tessellated_clip_readback() {
    use flui_foundation::geometry::{Point, RSuperellipse, Radius, Rect};
    use flui_layer::{PictureLayer, SceneBuilder};
    use flui_painting::paint::{BlendMode, Clip, ClipOp};
    use flui_painting::{Canvas, paint::Paint, styling::Color};

    // Shader optimization is independent of the Rust build profile. In
    // particular, DEBUG would add D3DCOMPILE_SKIP_OPTIMIZATION and hide
    // release-only FXC failures even when the readback itself succeeds.
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12,
        flags: wgpu::InstanceFlags::VALIDATION | wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL,
        backend_options: wgpu::BackendOptions {
            dx12: wgpu::Dx12BackendOptions {
                shader_compiler: wgpu::Dx12Compiler::Fxc,
                ..Default::default()
            },
            ..Default::default()
        },
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = pollster::block_on(instance.request_adapter(
        &crate::device::adapter::trusted_adapter_options(
            wgpu::PowerPreference::HighPerformance,
            None,
        ),
    ))
    .expect("optimized FXC contract requires an actual DX12 adapter");
    assert_eq!(adapter.get_info().backend, wgpu::Backend::Dx12);
    let capabilities = crate::device::adapter::GpuCapabilities::detect(&adapter);
    let (device, queue) = pollster::block_on(crate::device::adapter::request_flui_device(
        &adapter,
        &capabilities,
        "optimized FXC clip contract",
    ))
    .expect("optimized FXC contract requires an actual device");
    let (device, queue) = (Arc::new(device), Arc::new(queue));
    let size = 64;
    let vertices = vec![
        Point::new(4.0, 4.0),
        Point::new(60.0, 4.0),
        Point::new(4.0, 60.0),
    ];
    let blue = [0, 0, 255, 255];
    let red = [255, 0, 0, 255];
    for (name, clip_case, mode, reflected, inside, edge) in [
        ("unclipped", 0, BlendMode::SrcOver, false, red, red),
        ("hard squircle", 1, BlendMode::SrcOver, false, red, red),
        (
            "AA squircle",
            2,
            BlendMode::SrcOver,
            false,
            red,
            [128, 0, 127, 255],
        ),
        (
            "nested mask",
            3,
            BlendMode::SrcOver,
            false,
            red,
            [128, 0, 127, 255],
        ),
        (
            "Src mask",
            3,
            BlendMode::Src,
            false,
            [128, 0, 0, 128],
            [64, 0, 127, 191],
        ),
        (
            "Clear mask",
            3,
            BlendMode::Clear,
            false,
            [0, 0, 0, 0],
            [0, 0, 127, 127],
        ),
        ("reflected", 0, BlendMode::SrcOver, true, red, blue),
        (
            "unclipped after masks",
            0,
            BlendMode::SrcOver,
            false,
            red,
            red,
        ),
    ] {
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
        let mut painter = crate::painter::WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            wgpu::TextureFormat::Rgba8Unorm,
            (size, size),
        );
        let (target, view) = crate::test_support::create_sampleable_target(
            &device,
            "optimized FXC tessellated clip",
            size,
            size,
            wgpu::TextureFormat::Rgba8Unorm,
        );
        crate::test_support::clear_target(&device, &queue, &view, wgpu::Color::BLUE);
        let mut canvas = Canvas::new();
        if clip_case != 0 {
            let left = if clip_case == 1 { 16.0 } else { 16.5 };
            canvas.clip_rsuperellipse_with(
                RSuperellipse::from_rect_and_radius(
                    Rect::from_xywh(left, 8.0, 32.0, 48.0),
                    Radius::circular(8.0),
                ),
                ClipOp::Intersect,
                if clip_case == 1 {
                    Clip::HardEdge
                } else {
                    Clip::AntiAlias
                },
            );
            if clip_case == 3 {
                canvas.clip_rect_with(
                    Rect::from_xywh(8.0, 0.0, 48.0, 64.0),
                    ClipOp::Intersect,
                    Clip::AntiAlias,
                );
            }
        }
        if reflected {
            canvas.translate(64.0, 0.0);
            canvas.scale(-1.0, 1.0);
        }
        let color = if mode == BlendMode::Src {
            Color::rgba(255, 0, 0, 128)
        } else {
            Color::RED
        };
        // A mesh reaches the failing Specialized Shape Pipeline directly;
        // a large filled Path instead detours through supersampled replay.
        canvas.draw_vertices(
            vertices.clone(),
            None,
            None,
            vec![0, 1, 2],
            &Paint::fill(color).with_blend_mode(mode),
        );
        let mut builder = SceneBuilder::new();
        builder.add(PictureLayer::new(canvas.finish()));
        let scene = builder.build();
        painter.begin_frame().expect("optimized FXC frame opens");
        let _recorded: crate::frame::protocol::RasterCounters =
            crate::frame::protocol::record_frame_content(
                &mut painter,
                &scene,
                None,
                crate::record::cache::CacheReads::On,
            )
            .expect("optimized FXC scene records through the window frame path");
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("optimized FXC clip encoder"),
        });
        painter
            .render_to_texture(&target, &mut encoder)
            .expect("optimized FXC scene encodes");
        painter
            .submit_encoder(encoder)
            .expect("optimized FXC scene submits");
        painter.finish_frame();
        let internal_error = pollster::block_on(internal.pop());
        let validation_error = pollster::block_on(validation.pop());
        assert!(internal_error.is_none(), "{name}: {internal_error:?}");
        assert!(validation_error.is_none(), "{name}: {validation_error:?}");
        let bytes = crate::test_support::readback_bytes(&device, &queue, &target, size, size);
        let outside_clip = if clip_case == 0 && !reflected {
            red
        } else {
            blue
        };
        let inside_x = if reflected { 40 } else { 24 };
        for (x, y, expected) in [
            (inside_x, 24, inside),
            (16, 24, edge),
            (15, 24, outside_clip),
            (60, 60, blue),
        ] {
            let offset = ((y * size + x) * 4) as usize;
            let actual = &bytes[offset..offset + 4];
            assert!(
                actual
                    .iter()
                    .zip(expected)
                    .all(|(&a, b)| a.abs_diff(b) <= 1),
                "{name} pixel ({x}, {y}): {actual:?}, expected {expected:?}"
            );
        }
    }
}

fn shader_mask_external_texture_registrations_follow_parent() {
    use flui_foundation::geometry::Rect;
    use flui_layer::{Layer, LayerTree, PictureLayer, Scene, ShaderMaskLayer};
    use flui_painting::paint::{FilterQuality, TextureId};
    use flui_painting::styling::Color;
    use flui_painting::{Canvas, paint::Paint, paint::Shader};

    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let mut capture = renderer
        .retained_capture((128, 128))
        .expect("capture target");
    let texture = TextureId::new(73);
    let scene = |extent: f64, source| {
        let mut background = Canvas::new();
        background.draw_rect(
            Rect::from_xywh(0.0, 0.0, 128.0, 128.0),
            &Paint::fill(Color::WHITE),
        );
        let mut tree = LayerTree::new(Layer::from(PictureLayer::new(background.finish())));
        let bounds = Rect::from_xywh(20.0, 20.0, extent, extent);
        let mask = tree.push_child(
            tree.root(),
            Layer::from(ShaderMaskLayer::new(
                Shader::solid(Color::rgba(255, 255, 255, 128)),
                flui_painting::paint::BlendMode::DstIn,
                bounds,
            )),
        );
        let mut canvas = Canvas::new();
        canvas.draw_texture(source, bounds, None, FilterQuality::None, 1.0);
        tree.push_child(mask, Layer::from(PictureLayer::new(canvas.finish())));
        Scene::new(tree)
    };
    // Equal extents reuse the child painter; a new extent rebuilds it.
    // Both must observe replacement on the parent's registry, not stale leases.
    for (extent, color, expected) in [
        (32.0, [255, 0, 0, 255], [255, 127, 127, 255]),
        (32.0, [0, 0, 255, 255], [127, 127, 255, 255]),
        (48.0, [0, 255, 0, 255], [127, 255, 127, 255]),
    ] {
        capture.set_solid_texture(texture, color);
        capture
            .render_unmanaged(&scene(extent, texture))
            .expect("masked external frame");
        let pixels = capture.read_rgba().expect("masked readback");
        let pixel = |x: usize, y: usize| {
            let index = (y * 128 + x) * 4;
            &pixels[index..index + 4]
        };
        for (&actual, expected) in pixel(28, 28).iter().zip(expected) {
            assert!(
                (i32::from(actual) - expected).abs() <= 2,
                "masked external pixel {:?}, expected {expected}",
                pixel(28, 28)
            );
        }
        assert_eq!(pixel(8, 8), &[255, 255, 255, 255], "outside mask unchanged");
    }
    let error = capture
        .render_unmanaged(&scene(48.0, TextureId::new(74)))
        .expect_err("missing external ID fails");
    assert!(matches!(
        error,
        crate::EngineError::ExternalTexture(crate::ExternalTextureError::UnknownTexture { id: 74 })
    ));
    capture
        .render_unmanaged(&scene(48.0, texture))
        .expect("mask painter recovers after failed lookup");
    let recovered = capture.read_rgba().expect("recovered masked readback");
    let offset = (28 * 128 + 28) * 4;
    for (&actual, expected) in recovered[offset..offset + 4]
        .iter()
        .zip([127, 255, 127, 255])
    {
        assert!(
            (i32::from(actual) - expected).abs() <= 2,
            "recovered frame must repaint the masked texture"
        );
    }
}
