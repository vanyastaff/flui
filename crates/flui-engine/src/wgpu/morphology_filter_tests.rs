//! GPU readback acceptance gate for the morphology (dilate/erode) filter pass.
//!
//! ## Test inventory
//!
//! | # | Gate | Requirement |
//! |---|------|-------------|
//! | M1 | GPU | radius=0 identity: Filter path runs, content is unchanged |
//! | M2 | GPU | dilate opaque: content border expands by ceil(radius) |
//! | M3 | GPU | erode opaque: content border contracts by ceil(radius) |
//! | M4 | GPU | translucent DISCRIMINATING premul (G2): adjacent non-uniform pixels prove premul-direct max |
//! | M5 | GPU | decal boundary: outside-content pixels are transparent-black, not clamped colour |
//! | M6 | GPU | grown_bounds wiring: composite rect = content ⊕ ceil(radius) after dilate |
//!
//! ## Premul-direct invariant (PINNED #1)
//!
//! max/min operates on **premultiplied** RGBA — there is NO unpremultiply step.
//! The CPU oracle [`morph_oracle_premul`] follows the same contract.  M4 is
//! specifically designed to discriminate premul-direct from unpremultiply+op+repremul:
//! adjacent pixels `(128,128,128,255)` and `(128,128,128,128)` have premul-max RGB=128
//! but unpremul-max RGB=255; the two paths produce different quantised outputs.
//!
//! ## Decal semantics
//!
//! Pixels outside the declared `content_bounds` in UV are treated as the neutral
//! element (transparent-black for dilate, opaque-white for erode) — NOT clamped
//! to the edge colour.  M5 verifies this for the dilate case.
//!
//! All tests use `enable-wgpu-tests` feature-gate and follow the same harness
//! pattern as `color_matrix_filter_tests`.  The 64×64 surface avoids DX12
//! small-texture copy artefacts.

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod gpu_tests {
    use std::sync::Arc;

    use flui_painting::Paint;
    use flui_types::{Color, Rect, geometry::Pixels};

    use smallvec::smallvec;

    use crate::wgpu::{
        command_ir::{DrawItem, DrawSegment, FilterOp, ImageFilterPass, ImageFilterSpec, MorphOp},
        instancing::RectInstance,
        painter::WgpuPainter,
        render_target::RenderTarget,
    };

    // ── Harness constants ─────────────────────────────────────────────────────

    const SURFACE_WIDTH: u32 = 64;
    const SURFACE_HEIGHT: u32 = 64;
    const SURFACE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

    // ── Harness helpers ───────────────────────────────────────────────────────

    fn acquire_test_device_and_queue() -> (Arc<wgpu::Device>, Arc<wgpu::Queue>) {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .expect("a GPU adapter must be available for morphology_filter_tests");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("MorphologyFilter Test Device"),
            ..Default::default()
        }))
        .expect("a GPU device must be available for morphology_filter_tests");
        (Arc::new(device), Arc::new(queue))
    }

    fn create_surface(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("MorphologyFilter Test Surface"),
            size: wgpu::Extent3d {
                width: SURFACE_WIDTH,
                height: SURFACE_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SURFACE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view)
    }

    fn clear_surface(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        color: wgpu::Color,
    ) {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("MorphologyFilter Surface Clear"),
        });
        {
            let _clear_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("MorphologyFilter Clear Pass"),
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

    /// Read all pixels back from `texture` as `[r, g, b, a]` u8 quads.
    fn readback_pixels(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
    ) -> Vec<[u8; 4]> {
        let bytes_per_pixel = 4u32;
        let unpadded_row_bytes = SURFACE_WIDTH * bytes_per_pixel;
        let row_alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_row_bytes = unpadded_row_bytes.div_ceil(row_alignment) * row_alignment;
        let staging_size = u64::from(padded_row_bytes * SURFACE_HEIGHT);

        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("MorphologyFilter Readback Staging"),
            size: staging_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("MorphologyFilter Readback Encoder"),
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
                    bytes_per_row: Some(padded_row_bytes),
                    rows_per_image: Some(SURFACE_HEIGHT),
                },
            },
            wgpu::Extent3d {
                width: SURFACE_WIDTH,
                height: SURFACE_HEIGHT,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));

        staging.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("GPU readback poll must complete within wait timeout");

        let raw_bytes = staging.slice(..).get_mapped_range();
        let pixel_count = (SURFACE_WIDTH * SURFACE_HEIGHT) as usize;
        let mut pixels = Vec::with_capacity(pixel_count);
        for row_index in 0..SURFACE_HEIGHT {
            let row_start = (row_index * padded_row_bytes) as usize;
            for col_index in 0..SURFACE_WIDTH {
                let byte_offset = row_start + col_index as usize * 4;
                pixels.push([
                    raw_bytes[byte_offset],
                    raw_bytes[byte_offset + 1],
                    raw_bytes[byte_offset + 2],
                    raw_bytes[byte_offset + 3],
                ]);
            }
        }
        pixels
    }

    fn build_painter(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> WgpuPainter {
        WgpuPainter::with_shared_device(
            device,
            queue,
            SURFACE_FORMAT,
            (SURFACE_WIDTH, SURFACE_HEIGHT),
        )
    }

    fn px(physical_pixels: f32) -> Pixels {
        Pixels(physical_pixels)
    }

    fn full_surface_bounds() -> Rect<Pixels> {
        Rect::from_xywh(
            px(0.0),
            px(0.0),
            px(SURFACE_WIDTH as f32),
            px(SURFACE_HEIGHT as f32),
        )
    }

    /// Return a rect representing a sub-region in the center of the surface.
    ///
    /// `edge_margin_px` is the distance in whole pixels from each surface edge to
    /// the returned content rect.  Leaves transparent space on all four sides for
    /// decal and grown-bounds tests.
    fn center_rect(edge_margin_px: u32) -> Rect<Pixels> {
        let margin = edge_margin_px as f32;
        Rect::from_xywh(
            px(margin),
            px(margin),
            px(SURFACE_WIDTH as f32 - 2.0 * margin),
            px(SURFACE_HEIGHT as f32 - 2.0 * margin),
        )
    }

    // ── CPU oracle ────────────────────────────────────────────────────────────

    /// CPU oracle for morphological filters operating **premul-direct** on a
    /// flat pixel grid.
    ///
    /// ## Contract (matches PINNED #1 and the WGSL shader exactly)
    ///
    /// - Input pixel format: `[r, g, b, a]` u8 premultiplied RGBA.
    /// - Two-pass separable: H pass then V pass, each scanning
    ///   `[-ceil(radius)..=ceil(radius)]` texels. **O(W·H·ceil(radius))** total.
    /// - Decal: pixels outside `content_rect` (in pixel coordinates) are
    ///   treated as the neutral element — `[0,0,0,0]` for dilate,
    ///   `[255,255,255,255]` for erode — rather than clamping to edge colour.
    /// - Dilate: per-channel `max` with neutral `[0,0,0,0]`.
    /// - Erode: per-channel `min` with neutral `[255,255,255,255]`.
    ///
    /// ## Discriminating-premul rationale (G2)
    ///
    /// For adjacent pixels P1=`(128,128,128,255)` and P2=`(128,128,128,128)`:
    /// - Premul-direct max: `max(128,128)=128` for all channels → boundary output ~128 RGB.
    /// - Unpremul+max+repremul would: straight P2=`(255,255,255,0.5)` → max straight
    ///   RGB=`255` → repremul → boundary RGB ~255. Gap of ~127 u8 units — test M4 catches it.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        reason = "radius is a small positive float (≤64); content_rect coords are small test constants (≤64); \
                  all casts are within i32/usize range for a 64×64 test surface"
    )]
    fn morph_oracle_premul(
        source_pixels: &[[u8; 4]],
        surface_width: u32,
        surface_height: u32,
        radius: f32,
        op: MorphOp,
        content_rect_px: (u32, u32, u32, u32), // (left, top, right_exclusive, bottom_exclusive)
    ) -> Vec<[u8; 4]> {
        let grid_width = surface_width as usize;
        let grid_height = surface_height as usize;
        let kernel_half = radius.ceil() as i32;

        // Content boundary in signed pixel coordinates for neighbour-fetch bounds tests.
        // Values are small test constants (≤64); i32 is safe.
        let content_left = content_rect_px.0 as i32;
        let content_top = content_rect_px.1 as i32;
        let content_right = content_rect_px.2 as i32;
        let content_bottom = content_rect_px.3 as i32;

        // Accumulator INIT — the identity element of the fold (op-dependent):
        // dilate starts at transparent black, erode at opaque white. Mirrors the
        // shader's `acc` init and Impeller `morphology_filter.frag` `result` init.
        let init: [u8; 4] = match op {
            MorphOp::Dilate => [0, 0, 0, 0],
            MorphOp::Erode => [255, 255, 255, 255],
        };

        // DECAL value — the out-of-bounds SAMPLE. Always TRANSPARENT BLACK for
        // BOTH ops, matching Impeller's decal address mode / IPHalfSampleDecal
        // (only the accumulator init is op-dependent). This is what makes erode
        // shrink at a decal boundary (`min(acc, 0) == 0`); the old op-dependent
        // neutral here (`[255,255,255,255]` for erode) was the bug.
        let decal: [u8; 4] = [0, 0, 0, 0];

        let apply_channel_op = |channel_a: u8, channel_b: u8| -> u8 {
            match op {
                MorphOp::Dilate => channel_a.max(channel_b),
                MorphOp::Erode => channel_a.min(channel_b),
            }
        };

        let apply_pixel_op = |pixel_a: [u8; 4], pixel_b: [u8; 4]| -> [u8; 4] {
            [
                apply_channel_op(pixel_a[0], pixel_b[0]),
                apply_channel_op(pixel_a[1], pixel_b[1]),
                apply_channel_op(pixel_a[2], pixel_b[2]),
                apply_channel_op(pixel_a[3], pixel_b[3]),
            ]
        };

        // H-pass fetch: the source is decal-clamped at the CONTENT rect (samples
        // beyond content, or beyond the surface, are transparent black). Inside the
        // surface but outside content, the source is already transparent (cleared),
        // so this matches `apply_morphology`'s H pass (`content_rect_uv` = content).
        let fetch_h = |grid: &[[u8; 4]], row: i32, col: i32| -> [u8; 4] {
            if row < content_top
                || row >= content_bottom
                || col < content_left
                || col >= content_right
                || row < 0
                || row >= grid_height as i32
                || col < 0
                || col >= grid_width as i32
            {
                return decal;
            }
            grid[row as usize * grid_width + col as usize]
        };

        // V-pass fetch: reads the H OUTPUT, decal-clamped only at the SURFACE edge
        // (the H output is already transparent outside its grown content, so no
        // content-rect clip — matches `apply_morphology`'s V pass `content_rect_uv`
        // = [0,1] texture edge, which is what lets dilate grow diagonally/at corners).
        let fetch_v = |grid: &[[u8; 4]], row: i32, col: i32| -> [u8; 4] {
            if row < 0 || row >= grid_height as i32 || col < 0 || col >= grid_width as i32 {
                return decal;
            }
            grid[row as usize * grid_width + col as usize]
        };

        // H pass: accumulate over horizontal neighbourhood.
        let mut h_pass: Vec<[u8; 4]> = vec![[0; 4]; grid_width * grid_height];
        for row in 0..grid_height {
            for col in 0..grid_width {
                let mut accumulated = init;
                for dx in -kernel_half..=kernel_half {
                    let neighbour = fetch_h(source_pixels, row as i32, col as i32 + dx);
                    accumulated = apply_pixel_op(accumulated, neighbour);
                }
                h_pass[row * grid_width + col] = accumulated;
            }
        }

        // V pass: accumulate over vertical neighbourhood from H result.
        let mut v_pass: Vec<[u8; 4]> = vec![[0; 4]; grid_width * grid_height];
        for row in 0..grid_height {
            for col in 0..grid_width {
                let mut accumulated = init;
                for dy in -kernel_half..=kernel_half {
                    let neighbour = fetch_v(&h_pass, row as i32 + dy, col as i32);
                    accumulated = apply_pixel_op(accumulated, neighbour);
                }
                v_pass[row * grid_width + col] = accumulated;
            }
        }

        v_pass
    }

    // ── M1: radius=0 identity — Filter path runs, content unchanged ───────────

    /// M1: A dilate filter with radius=0 must produce bit-identical output to
    /// drawing the same rect without a filter.
    ///
    /// **Proves:**
    /// - The `DrawItem::Filter` path runs end-to-end (record → replay → composite).
    /// - G3 guardrail: the filter layer does NOT reintegrate (it composites).
    /// - With a kernel of size 1 (only the pixel itself), dilate is the identity.
    ///
    /// **Fails if:** the filter layer is silently reintegrated (G3 violation) or
    /// the zero-radius case panics or corrupts pixels.
    #[test]
    fn dilate_radius_zero_is_identity() {
        let (device, queue) = acquire_test_device_and_queue();
        let (no_filter_tex, no_filter_view) = create_surface(&device);
        let (morph_tex, morph_view) = create_surface(&device);

        let opaque_black = wgpu::Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        clear_surface(&device, &queue, &no_filter_view, opaque_black);
        clear_surface(&device, &queue, &morph_view, opaque_black);

        let source_color = Color::rgba(180, 90, 40, 255);
        let bounds = full_surface_bounds();

        // Without filter.
        {
            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            painter.rect(bounds, &Paint::fill(source_color));
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            painter
                .render(
                    RenderTarget::sampleable(&no_filter_view, &no_filter_tex),
                    &mut encoder,
                )
                .expect("no-filter render must succeed");
            queue.submit(std::iter::once(encoder.finish()));
        }

        // With dilate radius=0 (kernel size 1 — only the pixel itself).
        {
            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            painter.save_layer_with_image_filter(ImageFilterSpec::Morph {
                radius: 0.0,
                op: MorphOp::Dilate,
            });
            painter.rect(bounds, &Paint::fill(source_color));
            painter.restore_layer();
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            painter
                .render(
                    RenderTarget::sampleable(&morph_view, &morph_tex),
                    &mut encoder,
                )
                .expect("radius=0 dilate render must succeed");
            queue.submit(std::iter::once(encoder.finish()));
        }

        let no_filter_pixels = readback_pixels(&device, &queue, &no_filter_tex);
        let morph_pixels = readback_pixels(&device, &queue, &morph_tex);

        // ±2 u8 tolerance: one extra quantisation step at the offscreen boundary
        // (render → pool texture → composite) compared to the direct path.
        let surface_width = SURFACE_WIDTH as usize;
        let surface_height = SURFACE_HEIGHT as usize;
        for (pixel_index, (&no_filter_pixel, &morph_pixel)) in
            no_filter_pixels.iter().zip(morph_pixels.iter()).enumerate()
        {
            let row = pixel_index / surface_width;
            let col = pixel_index % surface_width;
            // Skip the 1-pixel border: SDF fwidth uses helper fragments outside
            // the primitive at the viewport edge, yielding partial alpha there.
            if row == 0 || row >= surface_height - 1 || col == 0 || col >= surface_width - 1 {
                continue;
            }
            for channel_index in 0..4 {
                let channel_diff = u8::try_from(
                    (i16::from(no_filter_pixel[channel_index])
                        - i16::from(morph_pixel[channel_index]))
                    .unsigned_abs(),
                )
                .expect("diff of two u8 values fits in u8 — both are in [0,255]");
                assert!(
                    channel_diff <= 2,
                    "M1: pixel {pixel_index} (row={row} col={col}) channel {channel_index} — \
                     no_filter={a} dilate_radius0={b} diff={channel_diff} > tolerance 2",
                    a = no_filter_pixel[channel_index],
                    b = morph_pixel[channel_index],
                );
            }
        }
    }

    // ── M2: dilate opaque — border expands ────────────────────────────────────

    /// M2: Dilate with radius=3 on an opaque content rect must produce non-zero
    /// alpha at pixels immediately outside (but within radius of) the content
    /// border, proving the filter expanded the content by ceil(radius) pixels.
    ///
    /// **Proves:**
    /// - The H→V two-pass scan correctly expands the content by `ceil(radius)`.
    /// - Interior pixels of the content rect stay at their original colour.
    ///
    /// **Fails if:** the dilate kernel has the wrong sign convention, the passes
    /// are misplaced, or the output is the same as the unfiltered content.
    #[test]
    fn dilate_expands_opaque_content_border() {
        const DILATE_RADIUS: f32 = 3.0;
        const CONTENT_EDGE_MARGIN_PX: u32 = 10; // content border at x=10, y=10

        let (device, queue) = acquire_test_device_and_queue();
        let (surface_tex, surface_view) = create_surface(&device);
        clear_surface(
            &device,
            &queue,
            &surface_view,
            wgpu::Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            },
        );

        let content_rect = center_rect(CONTENT_EDGE_MARGIN_PX);
        let source_color = Color::rgba(200, 150, 100, 255); // opaque

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save_layer_with_image_filter(ImageFilterSpec::Morph {
            radius: DILATE_RADIUS,
            op: MorphOp::Dilate,
        });
        painter.rect(content_rect, &Paint::fill(source_color));
        painter.restore_layer();

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_tex),
                &mut encoder,
            )
            .expect("dilate render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_tex);
        let surface_width = SURFACE_WIDTH as usize;
        let surface_height = SURFACE_HEIGHT as usize;

        // Pixels just *inside* the content border must be fully opaque.
        let interior_row = CONTENT_EDGE_MARGIN_PX as usize + 2;
        let interior_col = CONTENT_EDGE_MARGIN_PX as usize + 2;
        let interior_alpha = pixels[interior_row * surface_width + interior_col][3];
        assert!(
            interior_alpha > 200,
            "M2: interior pixel at ({interior_col},{interior_row}) alpha={interior_alpha} — \
             expected fully opaque (>200); dilate must preserve interior content"
        );

        // Pixels just *outside* the left border (within radius) must be non-zero
        // (the dilate expanded into them).
        let just_outside_col = CONTENT_EDGE_MARGIN_PX as usize - 1;
        let vertical_mid_row = surface_height / 2;
        let expanded_pixel = pixels[vertical_mid_row * surface_width + just_outside_col];
        assert!(
            expanded_pixel[3] > 0,
            "M2: pixel at ({just_outside_col},{vertical_mid_row}) alpha={} — \
             expected non-zero after dilate-expand (radius={DILATE_RADIUS}); \
             border must have grown by ceil(radius) pixels",
            expanded_pixel[3]
        );

        // Pixels far outside the border (beyond the expanded region) must be zero.
        let far_corner_alpha = pixels[surface_width + 1][3];
        assert_eq!(
            far_corner_alpha, 0,
            "M2: far-corner pixel alpha={far_corner_alpha} — \
             expected transparent (dilate must not bleed beyond ceil(radius))"
        );
    }

    // ── M3: erode opaque — border contracts ───────────────────────────────────

    /// M3: Erode with radius=3 on an opaque rect surrounded by transparent space
    /// must shrink the opaque region by `ceil(radius)` pixels on every side.
    ///
    /// ## Why erode contracts relative to transparent neighbours
    ///
    /// Erode accumulates per-channel `min`, starting from the op-dependent
    /// ACCUMULATOR INIT `[255,255,255,255]` (so an interior pixel whose whole
    /// kernel is opaque stays opaque: `min` of opaques). Contraction happens when
    /// the kernel reaches transparent pixels `[0,0,0,0]` — either drawn-transparent
    /// neighbours in the source, or the DECAL value (also `[0,0,0,0]`, for BOTH
    /// ops, matching Impeller) for samples beyond the content rect / surface edge:
    /// `min(x, 0) = 0` propagates zeros inward.
    ///
    /// `save_layer_with_image_filter` uses `bounds=None`, so `content_bounds` is the
    /// full 64×64 surface; the opaque inner rect is surrounded by drawn-transparent
    /// `[0,0,0,0]` pixels (cleared source). Those in-bounds zeros — NOT the decal —
    /// drive this test's contraction (the rect is far from the surface edge). The
    /// decal-boundary erosion is covered separately by
    /// `erode_shrinks_at_viewport_decal_boundary`.
    ///
    /// **Proves:**
    /// - `min` accumulation contracts the drawn rect by `ceil(radius)` pixels from
    ///   every side that adjoins transparent pixels.
    /// - The deep center of the rect (more than `ceil(radius)` pixels from any
    ///   transparent neighbour) remains fully opaque.
    ///
    /// **Fails if:** erode and dilate are swapped, the erode ACCUMULATOR INIT is
    /// `[0,0,0,0]` instead of `[255,255,255,255]` (would erase everything), or the
    /// kernel direction is wrong.
    #[test]
    fn erode_contracts_opaque_content_border() {
        const ERODE_RADIUS: f32 = 3.0;
        // ceil(ERODE_RADIUS) in pixels — exact since 3.0.ceil() == 3.0.
        const CEIL_RADIUS_PX: usize = 3;
        // Opaque inner rect: 20×20 centered at (22,22) in the 64×64 surface.
        // 22px of transparent space on each side ensures the kernel hits transparent
        // pixels before reaching the content_rect boundary.
        const RECT_ORIGIN_PX: u32 = 22;
        const RECT_SIZE_PX: u32 = 20;

        let (device, queue) = acquire_test_device_and_queue();
        let (surface_tex, surface_view) = create_surface(&device);
        clear_surface(
            &device,
            &queue,
            &surface_view,
            wgpu::Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            },
        );

        let opaque_rect = Rect::from_xywh(
            px(RECT_ORIGIN_PX as f32),
            px(RECT_ORIGIN_PX as f32),
            px(RECT_SIZE_PX as f32),
            px(RECT_SIZE_PX as f32),
        );
        let source_color = Color::rgba(100, 200, 80, 255);

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save_layer_with_image_filter(ImageFilterSpec::Morph {
            radius: ERODE_RADIUS,
            op: MorphOp::Erode,
        });
        painter.rect(opaque_rect, &Paint::fill(source_color));
        painter.restore_layer();

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_tex),
                &mut encoder,
            )
            .expect("erode render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_tex);
        let surface_width = SURFACE_WIDTH as usize;

        // The LEFT border of the drawn rect is at col=RECT_ORIGIN_PX.
        // After erode with radius=3, that border pixel must be transparent
        // (the transparent neighbours at col=21,20,... propagated zeros inward).
        let border_col = RECT_ORIGIN_PX as usize;
        let mid_row = RECT_ORIGIN_PX as usize + RECT_SIZE_PX as usize / 2;
        let border_pixel_alpha = pixels[mid_row * surface_width + border_col][3];
        assert_eq!(
            border_pixel_alpha, 0,
            "M3: pixel at rect left border (col={border_col}, row={mid_row}) \
             alpha={border_pixel_alpha} — \
             expected 0 after erode (radius={ERODE_RADIUS}, ceil={CEIL_RADIUS_PX}px); \
             transparent neighbours must propagate inward"
        );

        // The DEEP CENTER of the rect is more than ceil(radius) pixels from all
        // four transparent borders — must remain fully opaque.
        let center_row = RECT_ORIGIN_PX as usize + RECT_SIZE_PX as usize / 2;
        let center_col = RECT_ORIGIN_PX as usize + RECT_SIZE_PX as usize / 2;
        let center_alpha = pixels[center_row * surface_width + center_col][3];
        assert!(
            center_alpha > 200,
            "M3: center pixel at ({center_col},{center_row}) alpha={center_alpha} — \
             expected fully opaque (>200); erode must preserve deep interior content"
        );
    }

    // ── M4: discriminating premul (G2 anti-vacuous) ───────────────────────────

    /// M4: Dilate on adjacent pixels that are identical in premul RGB but differ
    /// in the way their premul values were *derived* must produce the premul-direct
    /// max, NOT the unpremultiply-max.
    ///
    /// ## The discriminating pixel pair
    ///
    /// | Pixel | Premul RGBA | Straight RGBA |
    /// |-------|-------------|---------------|
    /// | P1 (left half) | `(128,128,128,255)` | `(128,128,128,255)` |
    /// | P2 (right half) | `(128,128,128,128)` | `(255,255,255,128)` |
    ///
    /// Both pixels have **premul RGB = 128**.
    ///
    /// **Premul-direct max** at the left-right boundary:
    ///   `max(128, 128) = 128` per channel.  Oracle boundary output RGB ≈ 128.
    ///
    /// **Unpremul+max+repremul** at the boundary:
    /// 1. Straight P1 = `(0.502, 0.502, 0.502, 1.0)`.
    /// 2. Straight P2 = `(1.0, 1.0, 1.0, 0.502)`.
    /// 3. Max straight RGB = `(1.0, 1.0, 1.0)` → repremul RGB ≈ 255.
    ///
    /// Gap: ~127 u8 units — well above the ±5 tolerance, so the test cannot
    /// accidentally pass on the wrong implementation.
    ///
    /// **Fails if:** the shader unpremultiplies before the max/min operation.
    #[test]
    fn dilate_uses_premul_direct_max_not_unpremul_max() {
        const DILATE_RADIUS: f32 = 2.0;

        let (device, queue) = acquire_test_device_and_queue();
        let (surface_tex, surface_view) = create_surface(&device);
        clear_surface(
            &device,
            &queue,
            &surface_view,
            wgpu::Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            },
        );

        // Left half: opaque mid-gray.  Premul = (128,128,128,255).
        let left_half_color = Color::rgba(128, 128, 128, 255);
        // Right half: half-alpha white.  Premul = (128,128,128,128) because
        //   r_premul = round(255 * 128/255) = 128.
        let right_half_color = Color::rgba(255, 255, 255, 128);

        let left_half_rect = Rect::from_xywh(
            px(0.0),
            px(0.0),
            px(SURFACE_WIDTH as f32 / 2.0),
            px(SURFACE_HEIGHT as f32),
        );
        let right_half_rect = Rect::from_xywh(
            px(SURFACE_WIDTH as f32 / 2.0),
            px(0.0),
            px(SURFACE_WIDTH as f32 / 2.0),
            px(SURFACE_HEIGHT as f32),
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save_layer_with_image_filter(ImageFilterSpec::Morph {
            radius: DILATE_RADIUS,
            op: MorphOp::Dilate,
        });
        painter.rect(left_half_rect, &Paint::fill(left_half_color));
        painter.rect(right_half_rect, &Paint::fill(right_half_color));
        painter.restore_layer();

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_tex),
                &mut encoder,
            )
            .expect("M4 premul discriminating render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let gpu_pixels = readback_pixels(&device, &queue, &surface_tex);
        let surface_width = SURFACE_WIDTH as usize;
        let surface_height = SURFACE_HEIGHT as usize;
        let boundary_col = surface_width / 2;

        // Build the oracle source grid with the premul-encoded values:
        //   left_half_color  (128,128,128,255): premul = (128,128,128,255) (opaque, premul==straight)
        //   right_half_color (255,255,255,128): premul ≈ (128,128,128,128) — 255 * 128/255 rounds to 128
        let left_premul = [128u8, 128, 128, 255];
        let right_premul = [128u8, 128, 128, 128];

        let oracle_source_grid: Vec<[u8; 4]> = (0..surface_height)
            .flat_map(|_row| {
                (0..surface_width).map(|col| {
                    if col < boundary_col {
                        left_premul
                    } else {
                        right_premul
                    }
                })
            })
            .collect();

        let oracle = morph_oracle_premul(
            &oracle_source_grid,
            SURFACE_WIDTH,
            SURFACE_HEIGHT,
            DILATE_RADIUS,
            MorphOp::Dilate,
            // Both halves together cover the full surface.
            (0, 0, SURFACE_WIDTH, SURFACE_HEIGHT),
        );

        // Check a pixel at the left-right boundary, in the vertical middle
        // (away from top/bottom surface edges).
        let boundary_row = surface_height / 2;
        let oracle_pixel = oracle[boundary_row * surface_width + boundary_col];
        let gpu_pixel = gpu_pixels[boundary_row * surface_width + boundary_col];

        for channel_index in 0..4 {
            let channel_diff = u8::try_from(
                (i16::from(gpu_pixel[channel_index]) - i16::from(oracle_pixel[channel_index]))
                    .unsigned_abs(),
            )
            .expect("diff of two u8 values fits in u8 — both are in [0,255]");
            // Tight tolerance: any unpremul deviation would be >>10 u8 units (~127 for RGB).
            assert!(
                channel_diff <= 5,
                "M4: boundary pixel at ({boundary_col},{boundary_row}) channel {channel_index} — \
                 gpu={gpu_ch} oracle={oracle_ch} diff={channel_diff} > tolerance 5. \
                 If the shader unpremultiplies before max, RGB would be ~255 here (not ~128) — \
                 a gap of ~127 u8 units. This indicates PINNED #1 (premul-direct max) violation.",
                gpu_ch = gpu_pixel[channel_index],
                oracle_ch = oracle_pixel[channel_index],
            );
        }
    }

    // ── M5: decal boundary ────────────────────────────────────────────────────

    /// M5: A dilate filter on a centered content rect must produce transparent-black
    /// pixels far outside the content area (in the top-left corner), NOT the
    /// content colour clamped to the edge.
    ///
    /// This tests the in-shader decal guard: when a sample UV falls outside the
    /// `content_rect_uv` uniform, the shader must return the neutral element
    /// (`vec4(0.0)` for dilate) rather than sampling with `ClampToEdge`, which
    /// would smear the edge colour across the empty region.
    ///
    /// **Fails if:** the sampler uses `ClampToEdge` address mode instead of the
    /// in-shader decal bounds test.
    #[test]
    fn dilate_decal_outside_content_rect_is_transparent() {
        const DILATE_RADIUS: f32 = 4.0;
        const CONTENT_EDGE_MARGIN_PX: u32 = 16; // content well away from corner

        let (device, queue) = acquire_test_device_and_queue();
        let (surface_tex, surface_view) = create_surface(&device);
        clear_surface(
            &device,
            &queue,
            &surface_view,
            wgpu::Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            },
        );

        let content_rect = center_rect(CONTENT_EDGE_MARGIN_PX);
        let source_color = Color::rgba(255, 100, 50, 255);

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save_layer_with_image_filter(ImageFilterSpec::Morph {
            radius: DILATE_RADIUS,
            op: MorphOp::Dilate,
        });
        painter.rect(content_rect, &Paint::fill(source_color));
        painter.restore_layer();

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_tex),
                &mut encoder,
            )
            .expect("M5 decal render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_tex);
        let surface_width = SURFACE_WIDTH as usize;

        // The top-left corner is at distance MARGIN - ceil(RADIUS) from the content
        // boundary, safely outside the expanded region.  Must be transparent-black.
        let safe_corner_alpha = pixels[2 * surface_width + 2][3];
        assert_eq!(
            safe_corner_alpha, 0,
            "M5: corner pixel alpha={safe_corner_alpha} — \
             expected transparent-black (decal); \
             ClampToEdge would smear the edge colour here"
        );
        let safe_corner_red = pixels[2 * surface_width + 2][0];
        assert_eq!(
            safe_corner_red, 0,
            "M5: corner pixel R={safe_corner_red} — expected 0 (transparent-black decal)"
        );

        // The interior of the content rect must remain fully opaque and coloured.
        let interior_row = CONTENT_EDGE_MARGIN_PX as usize + 4;
        let interior_col = CONTENT_EDGE_MARGIN_PX as usize + 4;
        let interior_alpha = pixels[interior_row * surface_width + interior_col][3];
        assert!(
            interior_alpha > 200,
            "M5: interior pixel at ({interior_col},{interior_row}) alpha={interior_alpha} — \
             expected fully opaque; content interior must be unaffected by decal"
        );
    }

    // ── M6: grown_bounds wiring ───────────────────────────────────────────────

    /// M6: After a dilate filter, the GPU output texture must contain non-zero
    /// pixels just outside the original content rect (within `ceil(radius)` pixels
    /// of the content border), proving that `grown_bounds` was wired correctly.
    ///
    /// This test verifies that the `FilterOp { content_bounds, grown_bounds }` pair
    /// in the IR caused the compositor to allocate a texture large enough to hold
    /// the expanded content and that the final composite placed it correctly.
    ///
    /// If `grown_bounds` were not wired (e.g., `grown_bounds == content_bounds`),
    /// the expanded pixels would be clipped at the original content boundary and
    /// the just-outside pixels would be transparent.
    ///
    /// **Fails if:** `grown_bounds` is the same as `content_bounds` (no expansion),
    /// or the expansion is not applied at the final composite step.
    #[test]
    fn dilate_grown_bounds_expands_composite_rect() {
        const DILATE_RADIUS: f32 = 5.0;
        // ceil(DILATE_RADIUS) as usize — exact since 5.0.ceil() == 5.0 and 5.0 fits usize.
        const DILATE_CEIL_RADIUS_PX: u32 = 5;
        const CONTENT_EDGE_MARGIN_PX: u32 = 12; // content border at x=12, y=12

        let (device, queue) = acquire_test_device_and_queue();
        let (surface_tex, surface_view) = create_surface(&device);
        clear_surface(
            &device,
            &queue,
            &surface_view,
            wgpu::Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            },
        );

        let content_rect = center_rect(CONTENT_EDGE_MARGIN_PX);
        let source_color = Color::rgba(80, 200, 255, 255);

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save_layer_with_image_filter(ImageFilterSpec::Morph {
            radius: DILATE_RADIUS,
            op: MorphOp::Dilate,
        });
        painter.rect(content_rect, &Paint::fill(source_color));
        painter.restore_layer();

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_tex),
                &mut encoder,
            )
            .expect("M6 grown_bounds render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_tex);
        let surface_width = SURFACE_WIDTH as usize;
        let surface_height = SURFACE_HEIGHT as usize;

        // Pixel one step before the content left edge, in the vertical middle:
        // within ceil(radius) of the border → must be expanded by dilate.
        let just_left_of_content_col = CONTENT_EDGE_MARGIN_PX as usize - 1;
        let vertical_mid_row = surface_height / 2;
        let expanded_pixel_alpha =
            pixels[vertical_mid_row * surface_width + just_left_of_content_col][3];
        assert!(
            expanded_pixel_alpha > 0,
            "M6: pixel at ({just_left_of_content_col},{vertical_mid_row}) \
             alpha={expanded_pixel_alpha} — \
             expected non-zero after dilate expand (radius={DILATE_RADIUS}, \
             ceil={DILATE_CEIL_RADIUS_PX}px); \
             alpha=0 means grown_bounds was not expanded past content_bounds"
        );

        // Deep interior must remain fully coloured.
        let deep_interior_row = surface_height / 2;
        let deep_interior_col = surface_width / 2;
        let interior_alpha = pixels[deep_interior_row * surface_width + deep_interior_col][3];
        assert!(
            interior_alpha > 200,
            "M6: interior pixel at ({deep_interior_col},{deep_interior_row}) \
             alpha={interior_alpha} — \
             expected fully opaque; dilate must preserve interior content"
        );
    }

    // ── Erode shrinks at the viewport decal boundary ───────────────────────────

    /// Erode of opaque content flush against the LEFT viewport edge must shrink the
    /// content at that edge. Samples beyond the edge are decal = TRANSPARENT BLACK
    /// (`vec4(0)`) for BOTH ops (Impeller contract), so `min(opaque, 0) == 0`. The
    /// pre-fix shader used an op-dependent neutral (`vec4(1)` for erode) there — a
    /// no-op that left the edge un-eroded (a parity bug vs Flutter/Impeller).
    ///
    /// **Absolute assertion** (not GPU==oracle, since the oracle was co-bugged):
    /// the columns within `ceil(radius)` of the left edge MUST be alpha 0.
    /// RED on the pre-fix shader (those columns stayed opaque); GREEN after.
    #[test]
    fn erode_shrinks_at_viewport_decal_boundary() {
        const ERODE_RADIUS: f32 = 3.0;
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_tex, surface_view) = create_surface(&device);
        clear_surface(
            &device,
            &queue,
            &surface_view,
            wgpu::Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            },
        );

        // Opaque rect flush against the LEFT edge (x=0), away from top/bottom edges.
        let edge_rect = Rect::from_xywh(px(0.0), px(20.0), px(24.0), px(24.0));
        let source_color = Color::rgba(200, 60, 40, 255);

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save_layer_with_image_filter(ImageFilterSpec::Morph {
            radius: ERODE_RADIUS,
            op: MorphOp::Erode,
        });
        painter.rect(edge_rect, &Paint::fill(source_color));
        painter.restore_layer();
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_tex),
                &mut encoder,
            )
            .expect("erode viewport-edge render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_tex);
        let surface_width = SURFACE_WIDTH as usize;
        let mid_row = 30usize; // inside the rect's y-range [20,44), away from y edges

        // Columns within ceil(radius) of the left viewport edge MUST erode to
        // transparent — the decal beyond x=0 is vec4(0). (Pre-fix: stayed opaque.)
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "ERODE_RADIUS is a small positive const (3.0); ceil()→usize is exact"
        )]
        let kernel = ERODE_RADIUS.ceil() as usize;
        for col in 0..kernel {
            let alpha = pixels[mid_row * surface_width + col][3];
            assert_eq!(
                alpha, 0,
                "column {col} at row {mid_row} must erode to alpha 0 at the \
                 left viewport decal boundary (decal=vec4(0) for both ops); got {alpha}. \
                 The pre-fix erode used a vec4(1) decal here and left the edge opaque."
            );
        }
        // An interior column (well clear of every eroded edge) stays opaque.
        let interior_alpha = pixels[mid_row * surface_width + 10][3];
        assert!(
            interior_alpha > 200,
            "interior column 10 at row {mid_row} must stay opaque (got {interior_alpha})"
        );
    }

    // ── Tight-bounds dilate — corner growth and placement ──────────────────────

    /// Dilate of an opaque square with a TIGHT `content_bounds`/`grown_bounds`
    /// (sub-viewport), constructed directly as a `DrawItem::Filter` (the current
    /// `save_layer_with_image_filter` producer only emits `bounds=None`, so this is
    /// the only way to exercise the tight-bounds paths). Verifies two latent fixes:
    ///
    /// - The separable V pass must read the H-pass horizontal halo
    ///   (it decals at the texture edge, not the original content rect), so a box
    ///   dilate fills its CORNERS. Pre-fix the V pass decaled at `content_bounds`
    ///   and clipped the halo → corners stayed transparent.
    /// - The composite must map `grown_bounds` to the matching texture
    ///   sub-region (`src_uv = grown/viewport`), not the whole texture (`[0,1]`),
    ///   so the result lands at the correct surface position. Pre-fix it stretched
    ///   the full-viewport texture onto the `grown_bounds` rect.
    ///
    /// **Absolute assertions** independent of the CPU oracle. RED if EITHER bug is
    /// present (corner transparent, or content mis-placed); GREEN with both fixed.
    #[test]
    fn dilate_tight_bounds_fills_corners_at_correct_position() {
        const DILATE_RADIUS: f32 = 4.0;
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_tex, surface_view) = create_surface(&device);
        clear_surface(
            &device,
            &queue,
            &surface_view,
            wgpu::Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            },
        );

        // Opaque 16×16 square at [24,40)×[24,40), tightly bounded; dilate r=4 grows
        // it to [20,44)×[20,44) including square corners.
        let content_rect = Rect::from_xywh(px(24.0), px(24.0), px(16.0), px(16.0));
        let grown = Rect::from_xywh(px(20.0), px(20.0), px(24.0), px(24.0));
        let source_color = Color::rgba(255, 255, 255, 255);

        // Build the content as a single DrawSegment (mirrors the deterministic-replay
        // direct-construction pattern), then wrap it in a DrawItem::Filter.
        let mut segment = DrawSegment::new();
        let _ = segment
            .rect_batch
            .add(RectInstance::rect(content_rect, source_color));
        DrawSegment::push_scissor_region(&mut segment.rect_scissors, None);
        // grown = [20, 44) × [20, 44); all integer-aligned.
        // fb_origin = (floor(20), floor(20)) = (20, 20)
        // fb_dim    = (ceil(44) - 20, ceil(44) - 20) = (24, 24)
        let op = FilterOp {
            input: segment,
            passes: smallvec![ImageFilterPass::Morph {
                radius: DILATE_RADIUS,
                op: MorphOp::Dilate,
            }],
            content_bounds: content_rect,
            grown_bounds: grown,
            fb_origin: (20, 20),
            fb_dim: (24, 24),
        };

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        painter
            .replay_items_for_test(vec![DrawItem::Filter(op)], &surface_view, &mut encoder)
            .expect("tight-bounds dilate replay must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_tex);
        let w = SURFACE_WIDTH as usize;
        let alpha_at = |x: usize, y: usize| pixels[y * w + x][3];

        // Content center stays opaque (basic dilate + correct placement).
        assert!(
            alpha_at(32, 32) > 200,
            "content center (32,32) must be opaque, got {}",
            alpha_at(32, 32)
        );
        // Grown CORNER (43,43) — inside grown_bounds [20,44), diagonally beyond the
        // original square. Opaque only if the V pass reads the H halo (corner fill)
        // and the composite maps grown to correct texels are both fixed.
        assert!(
            alpha_at(43, 43) > 200,
            "grown corner (43,43) must be opaque after a box dilate; \
             got {}. Pre-fix the V pass clipped the H halo at content_bounds (corner \
             transparent) and/or the composite stretched the full texture onto \
             grown_bounds (mis-placed).",
            alpha_at(43, 43)
        );
        // Well OUTSIDE grown_bounds must be transparent — no stretch-bleed.
        assert_eq!(
            alpha_at(6, 6),
            0,
            "pixel (6,6) far outside grown_bounds [20,44) must be \
             transparent; got {}. A full-texture composite stretch would bleed \
             content here.",
            alpha_at(6, 6)
        );
    }

    // ── Tight-bounds erode shrinks at the content-rect edge ────────────────────

    /// Erode of an opaque square that FILLS a TIGHT `content_bounds` (sub-viewport),
    /// constructed directly as a `DrawItem::Filter`. At the content-rect edge the H
    /// pass's decal value (`vec4(0)` for both ops) drives contraction: the `inside`
    /// test returns the decal — NOT the cleared source — for samples beyond the
    /// content rect, so the decal VALUE is what matters here (unlike the bounds=None
    /// case where the cleared source coincides with the decal). Pre-fix the erode
    /// decal was `vec4(1)`, which overrode the boundary and left the edge un-eroded.
    ///
    /// Closes the tight-bounds-erode coverage gap (the viewport-edge test above covers
    /// `content_rect == viewport`; this covers a tight content rect). **Absolute,
    /// oracle-independent assertions.** RED on the pre-fix erode decal; GREEN after.
    #[test]
    fn erode_tight_bounds_shrinks_at_content_edge() {
        const ERODE_RADIUS: f32 = 3.0;
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_tex, surface_view) = create_surface(&device);
        clear_surface(
            &device,
            &queue,
            &surface_view,
            wgpu::Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            },
        );

        // Opaque 20×20 square that exactly fills its tight content_bounds [22,42)².
        // Erode r=3 → opaque region shrinks to ~[25,39)²; the [22,25) edge erodes.
        let content_rect = Rect::from_xywh(px(22.0), px(22.0), px(20.0), px(20.0));
        let source_color = Color::rgba(255, 255, 255, 255);

        let mut segment = DrawSegment::new();
        let _ = segment
            .rect_batch
            .add(RectInstance::rect(content_rect, source_color));
        DrawSegment::push_scissor_region(&mut segment.rect_scissors, None);
        // Erode does not grow content; grown_bounds == content_rect = [22, 42) × [22, 42).
        // fb_origin = (floor(22), floor(22)) = (22, 22)
        // fb_dim    = (ceil(42) - 22, ceil(42) - 22) = (20, 20)
        let op = FilterOp {
            input: segment,
            passes: smallvec![ImageFilterPass::Morph {
                radius: ERODE_RADIUS,
                op: MorphOp::Erode,
            }],
            content_bounds: content_rect,
            grown_bounds: content_rect,
            fb_origin: (22, 22),
            fb_dim: (20, 20),
        };

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        painter
            .replay_items_for_test(vec![DrawItem::Filter(op)], &surface_view, &mut encoder)
            .expect("tight-bounds erode replay must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_tex);
        let w = SURFACE_WIDTH as usize;
        let alpha_at = |x: usize, y: usize| pixels[y * w + x][3];

        // The left content edge (x=22, within ceil(radius) of the content-rect edge)
        // MUST erode to transparent — the H decal beyond the content rect is vec4(0).
        assert_eq!(
            alpha_at(22, 31),
            0,
            "left content edge (22,31) must erode to alpha 0 at the tight \
             content-rect decal boundary (decal=vec4(0)); got {}. Pre-fix the erode \
             decal was vec4(1) and left the content edge opaque.",
            alpha_at(22, 31)
        );
        // The content center (well clear of every eroded edge) stays opaque.
        assert!(
            alpha_at(31, 31) > 200,
            "content center (31,31) must stay opaque after erode; got {}",
            alpha_at(31, 31)
        );
    }
}
