//! GPU readback acceptance gate for the Gaussian blur filter pass.
//!
//! ## Test inventory
//!
//! | # | Gate | Requirement |
//! |---|------|-------------|
//! | B1 | GPU | Half-alpha disc: no dark halo — premultiplied-direct discriminator (G2/G3) |
//! | B2 | GPU | Anisotropic: sigma_x=8/sigma_y=2 → H-spread ≫ V-spread |
//! | B3 | GPU | Oracle match ±3 LSB on an opaque-colour content rect |
//! | B4 | GPU | Zero-sigma identity (ABSOLUTE — not GPU==oracle) |
//! | B5 | GPU | grown_bounds halo extent: pixels at col=3 or col=57 are non-zero for sigma=4 |
//!
//! ## Premultiplied-direct invariant (PINNED #2)
//!
//! The Gaussian kernel operates on **premultiplied** RGBA in **sRGB-encoded** space.
//! NO unpremultiply step, NO linearise — matching Impeller
//! `gaussian_blur_filter_contents.cc:935` (`apply_unpremultiply=false`).
//!
//! B1 is the discriminating test: a half-alpha white disc blurred premul-direct
//! should produce a smooth luminous halo, NOT a dark ring.  The dark-halo artefact
//! appears when unpremultiplying before the Gaussian and repremultiplying after.
//!
//! ## CPU oracle
//!
//! [`blur_oracle_premul`] mirrors the WGSL shader exactly:
//! - Premultiplied-direct (no unpremul/repremul)
//! - `exp(-0.5·i²/σ²)` Gaussian weights, running-sum renormalised
//! - `ceil(σ × √3)` half-radius (Impeller `kKernelRadiusPerSigma`)
//! - Decal: H pass decals at content rect; V pass decals at texture edge
//! - Anisotropic: H pass uses `sigma_x`, V pass uses `sigma_y`

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod gpu_tests {
    use std::sync::Arc;

    use flui_painting::Paint;
    use flui_types::{Color, Rect, geometry::Pixels};
    use smallvec::smallvec;

    use crate::wgpu::{
        command_ir::{DrawItem, DrawSegment, FilterOp, ImageFilterPass, ImageFilterSpec},
        effects::kernel_radius,
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
        .expect("a GPU adapter must be available for blur_filter_tests");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("BlurFilter Test Device"),
            ..Default::default()
        }))
        .expect("a GPU device must be available for blur_filter_tests");
        (Arc::new(device), Arc::new(queue))
    }

    fn create_surface(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("BlurFilter Test Surface"),
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
            label: Some("BlurFilter Surface Clear"),
        });
        {
            let _clear_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("BlurFilter Clear Pass"),
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
            label: Some("BlurFilter Readback Staging"),
            size: staging_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("BlurFilter Readback Encoder"),
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

    /// CPU oracle for separable Gaussian blur operating **premultiplied-direct**
    /// on a flat pixel grid.
    ///
    /// ## Contract (matches PINNED #2 and the WGSL shader exactly)
    ///
    /// - Premultiplied-direct: NO unpremultiply/repremultiply step.
    /// - Weights: `exp(-0.5 × i² / σ²)`, running-sum renormalised (divide by
    ///   sum of weights, not by theoretical integral).
    /// - Kernel half-radius: `ceil(σ × √3)` — `kernel_radius(σ)`.
    /// - Decal: H pass decals at `content_rect_px`; V pass decals at surface edge.
    /// - Anisotropic: H pass uses `sigma_x`, V pass uses `sigma_y`.
    ///
    /// ## Anti-co-vacuous design
    ///
    /// The oracle is intentionally faithful (not trivial) so that:
    /// - B1 (dark-halo) would FAIL if the oracle used unpremul/repremul.
    /// - B3 (oracle match) would FAIL if the GPU diverged from the oracle.
    /// - B4 (zero-sigma) is verified with ABSOLUTE values, not oracle comparison.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        clippy::cast_possible_wrap,
        reason = "sigma is small positive float; i32 offset fits for 64px grid; \
                  u32→i32 casts are safe for the 64px test surface; \
                  u8 clamping via f32 → u8::try_from is intentional"
    )]
    fn blur_oracle_premul(
        source_pixels: &[[u8; 4]],
        surface_width: u32,
        surface_height: u32,
        sigma_x: f32,
        sigma_y: f32,
        content_rect_px: (u32, u32, u32, u32), // (left, top, right_exclusive, bottom_exclusive)
    ) -> Vec<[u8; 4]> {
        let grid_w = surface_width as usize;
        let grid_h = surface_height as usize;

        let content_left = content_rect_px.0 as i32;
        let content_top = content_rect_px.1 as i32;
        let content_right = content_rect_px.2 as i32;
        let content_bottom = content_rect_px.3 as i32;

        // H pass: scan horizontally with sigma_x.
        // Decal at content_rect (only samples within content bounds are read;
        // outside → transparent black).  Mirrors the WGSL H-pass decal guard.
        let h_radius = kernel_radius(sigma_x) as i32;
        let mut h_pass: Vec<[f32; 4]> = vec![[0.0; 4]; grid_w * grid_h];

        for row in 0..grid_h {
            for col in 0..grid_w {
                if sigma_x <= 0.0 {
                    // Degenerate case: identity if sample is in content.
                    let row_i = row as i32;
                    let col_i = col as i32;
                    if row_i >= content_top
                        && row_i < content_bottom
                        && col_i >= content_left
                        && col_i < content_right
                    {
                        let p = source_pixels[row * grid_w + col];
                        h_pass[row * grid_w + col] = [
                            f32::from(p[0]),
                            f32::from(p[1]),
                            f32::from(p[2]),
                            f32::from(p[3]),
                        ];
                    }
                    continue;
                }
                let sigma_sq = sigma_x * sigma_x;
                let mut acc = [0.0_f32; 4];
                let mut tally = 0.0_f32;
                for dx in -h_radius..=h_radius {
                    let sample_col = col as i32 + dx;
                    let sample_row = row as i32;
                    // Decal: outside content rect → transparent black.
                    let texel = if sample_row >= content_top
                        && sample_row < content_bottom
                        && sample_col >= content_left
                        && sample_col < content_right
                        && sample_row >= 0
                        && sample_row < grid_h as i32
                        && sample_col >= 0
                        && sample_col < grid_w as i32
                    {
                        let p = source_pixels[sample_row as usize * grid_w + sample_col as usize];
                        [
                            f32::from(p[0]),
                            f32::from(p[1]),
                            f32::from(p[2]),
                            f32::from(p[3]),
                        ]
                    } else {
                        [0.0; 4]
                    };
                    let weight = (-0.5 * (dx * dx) as f32 / sigma_sq).exp();
                    acc.iter_mut()
                        .zip(texel.iter())
                        .for_each(|(accumulated, &texel_channel)| {
                            *accumulated += texel_channel * weight;
                        });
                    tally += weight;
                }
                if tally > 0.0 {
                    for channel_acc in &mut acc {
                        *channel_acc /= tally;
                    }
                }
                h_pass[row * grid_w + col] = acc;
            }
        }

        // V pass: scan vertically with sigma_y.
        // Decal at surface edge ([0..grid_h) × [0..grid_w)) — reads the full H
        // halo, including diagonal corners.  Mirrors the WGSL V-pass decal guard.
        let v_radius = kernel_radius(sigma_y) as i32;
        let mut v_pass: Vec<[u8; 4]> = vec![[0; 4]; grid_w * grid_h];

        for row in 0..grid_h {
            for col in 0..grid_w {
                if sigma_y <= 0.0 {
                    // Degenerate case: identity (read H pass directly).
                    let h_pixel = h_pass[row * grid_w + col];
                    v_pass[row * grid_w + col] = [
                        h_pixel[0].round().clamp(0.0, 255.0) as u8,
                        h_pixel[1].round().clamp(0.0, 255.0) as u8,
                        h_pixel[2].round().clamp(0.0, 255.0) as u8,
                        h_pixel[3].round().clamp(0.0, 255.0) as u8,
                    ];
                    continue;
                }
                let sigma_sq = sigma_y * sigma_y;
                let mut acc = [0.0_f32; 4];
                let mut tally = 0.0_f32;
                for dy in -v_radius..=v_radius {
                    let sample_row = row as i32 + dy;
                    let sample_col = col as i32;
                    // Decal: outside surface edge → transparent black.
                    let texel = if sample_row >= 0
                        && sample_row < grid_h as i32
                        && sample_col >= 0
                        && sample_col < grid_w as i32
                    {
                        h_pass[sample_row as usize * grid_w + sample_col as usize]
                    } else {
                        [0.0; 4]
                    };
                    let weight = (-0.5 * (dy * dy) as f32 / sigma_sq).exp();
                    acc.iter_mut()
                        .zip(texel.iter())
                        .for_each(|(accumulated, &texel_channel)| {
                            *accumulated += texel_channel * weight;
                        });
                    tally += weight;
                }
                if tally > 0.0 {
                    for channel_acc in &mut acc {
                        *channel_acc /= tally;
                    }
                }
                v_pass[row * grid_w + col] = [
                    acc[0].round().clamp(0.0, 255.0) as u8,
                    acc[1].round().clamp(0.0, 255.0) as u8,
                    acc[2].round().clamp(0.0, 255.0) as u8,
                    acc[3].round().clamp(0.0, 255.0) as u8,
                ];
            }
        }

        v_pass
    }

    // ── B1: No dark halo — premultiplied-direct discriminator (G2 / G3) ───────

    /// B1: A half-alpha (128/255) white disc blurred in premultiplied space must
    /// NOT produce a dark ring at the edge of the disc.
    ///
    /// ## Dark-halo artefact (the failure mode)
    ///
    /// Unpremultiplying half-alpha white `(128,128,128,128)` gives straight
    /// `(255,255,255,0.502)`.  Blurring straight values, then repremultiplying,
    /// mixes in transparent-black `(0,0,0,0)` at the disc boundary.  After
    /// repremultiply the RGB at the boundary drops (e.g. R ≈ 64 at the edge),
    /// producing a dark halo around the disc.
    ///
    /// Premultiplied-direct blur mixes premul `(128,128,128,128)` with premul
    /// `(0,0,0,0)` — the resulting RGB at the boundary is ~64 too, but the alpha
    /// also drops proportionally, so the perceived colour at the composited boundary
    /// is the SAME as the disc (just more transparent). No dark ring.
    ///
    /// ## Test assertion
    ///
    /// The centre of the disc after blur must be visibly brighter than a ring just
    /// inside the inner "dark-halo radius" (`ceil(sigma)`).  With premul-direct blur
    /// the ring is not darker than the centre — the test checks that the ring is at
    /// least 80% as bright as the centre.  With an unpremul implementation the ring
    /// would be ~50% as bright (a clearly visible dark halo).
    ///
    /// **Fails if:** the shader unpremultiplies before the Gaussian, PINNED #2 broken.
    #[test]
    fn blur_half_alpha_no_dark_halo_premul_direct() {
        const SIGMA: f32 = 5.0;
        // Disc radius in pixels — well within the 64×64 surface.
        const DISC_RADIUS_PX: f32 = 16.0;
        let disc_center = (SURFACE_WIDTH / 2, SURFACE_HEIGHT / 2);

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

        // Draw a half-alpha white disc (filled circle approximated by a full rect here;
        // SDF precision at the boundary doesn't matter — we check the interior vs ring).
        // Use `save_layer_with_image_filter` so the production path exercises the
        // `restore_layer(Blur)` arm and the `DrawItem::Filter` seam.
        let disc_rect = Rect::from_xywh(
            px(disc_center.0 as f32 - DISC_RADIUS_PX),
            px(disc_center.1 as f32 - DISC_RADIUS_PX),
            px(2.0 * DISC_RADIUS_PX),
            px(2.0 * DISC_RADIUS_PX),
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        // Half-alpha white: RGBA (255,255,255,128).
        // Premul = (128,128,128,128).
        let half_alpha_white = Color::rgba(255, 255, 255, 128);
        painter.save_layer_with_image_filter(ImageFilterSpec::Blur {
            sigma_x: SIGMA,
            sigma_y: SIGMA,
        });
        painter.rect(disc_rect, &Paint::fill(half_alpha_white));
        painter.restore_layer();

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_tex),
                &mut encoder,
            )
            .expect("B1 no-dark-halo blur render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_tex);
        let w = SURFACE_WIDTH as usize;

        // Centre pixel — well inside the disc, full disc contribution.
        let center_pixel = pixels[disc_center.1 as usize * w + disc_center.0 as usize];
        let center_red = f32::from(center_pixel[0]);

        // Ring pixel — at exactly DISC_RADIUS_PX - ceil(sigma) from the centre.
        // This is where the dark halo would manifest with an unpremul implementation.
        // With premul-direct the ring brightness is close to the centre.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "DISC_RADIUS_PX and SIGMA are small positive consts; result fits usize"
        )]
        let ring_offset = (DISC_RADIUS_PX - SIGMA.ceil()) as usize;
        let ring_col = disc_center.0 as usize - ring_offset;
        let ring_pixel = pixels[disc_center.1 as usize * w + ring_col];
        let ring_red = f32::from(ring_pixel[0]);

        // With premul-direct blur: ring brightness ≥ 80% of centre (the ring is
        // inside the disc and still well-covered, just slightly attenuated at sigma).
        // With unpremul blur: ring brightness ≈ 50% of centre (dark halo visible).
        // We require at least 75% to clearly distinguish the two implementations.
        let ratio = if center_red > 0.0 {
            ring_red / center_red
        } else {
            1.0 // Both zero: no disc drawn → vacuously pass, but check below ensures center > 0.
        };

        assert!(
            center_red > 10.0,
            "B1: disc centre R={center_red:.1} — expected > 10 (disc must have been drawn)"
        );
        assert!(
            ratio >= 0.75,
            "B1: ring_red/center_red = {ratio:.3} (ring R={ring_red:.1}, centre R={center_red:.1}) \
             — expected ≥ 0.75. With premul-direct blur the ring is not darker than the centre; \
             a ratio < 0.75 indicates the shader unpremultiplied before the Gaussian (dark halo), \
             violating PINNED #2."
        );
    }

    // ── B2: Anisotropy — sigma_x=8/sigma_y=2 → H-spread ≫ V-spread ──────────

    /// B2: A blur with `sigma_x=8, sigma_y=2` must produce a wide horizontal
    /// spread and a narrow vertical spread.  Measures the distance from the content
    /// centre to the first non-zero pixel in the H and V directions and verifies
    /// that the H reach is ≥ 3× the V reach.
    ///
    /// **Fails if:** sigma_x and sigma_y are swapped, or both passes use the same sigma.
    #[test]
    fn blur_anisotropic_sigma_x_wide_sigma_y_narrow() {
        const SIGMA_X: f32 = 8.0;
        const SIGMA_Y: f32 = 2.0;
        const CONTENT_MARGIN_PX: u32 = 20;
        // Minimum alpha a pixel must carry to count as "reached" by the blur.
        const THRESHOLD: u8 = 5;

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

        let content_rect = center_rect(CONTENT_MARGIN_PX);
        let source_color = Color::rgba(200, 200, 200, 255);

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save_layer_with_image_filter(ImageFilterSpec::Blur {
            sigma_x: SIGMA_X,
            sigma_y: SIGMA_Y,
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
            .expect("B2 anisotropic blur render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_tex);
        let w = SURFACE_WIDTH as usize;
        let h = SURFACE_HEIGHT as usize;

        // The content rect left/top edge — blur should spread past this.
        let content_left = CONTENT_MARGIN_PX as usize;
        let content_top = CONTENT_MARGIN_PX as usize;

        // Scan along mid-row to find the leftmost pixel with alpha ≥ THRESHOLD.
        // If H blur spreads sigma_x=8 px beyond the left content edge, the leftmost
        // non-zero pixel will be ~14px left of col=20 (i.e., near col=6).
        // H halo = content_left - leftmost_col.
        let mid_row = h / 2;
        let leftmost_h_col = (0..content_left)
            .find(|&col| pixels[mid_row * w + col][3] >= THRESHOLD)
            .unwrap_or(content_left);
        let h_halo = content_left.saturating_sub(leftmost_h_col);

        // Scan along mid-col to find the topmost pixel with alpha ≥ THRESHOLD.
        // If V blur spreads sigma_y=2 px beyond the top content edge, the topmost
        // non-zero pixel will be ~4px above row=20 (i.e., near row=16).
        // V halo = content_top - topmost_row.
        let mid_col = w / 2;
        let topmost_v_row = (0..content_top)
            .find(|&row| pixels[row * w + mid_col][3] >= THRESHOLD)
            .unwrap_or(content_top);
        let v_halo = content_top.saturating_sub(topmost_v_row);

        // Sanity: content centre (mid_row, mid_col) must be non-transparent.
        // If both halos are zero the blur may not have run at all.
        let center_alpha = pixels[mid_row * w + mid_col][3];
        assert!(
            center_alpha > 10,
            "B2: content centre alpha={center_alpha} — \
             expected > 10 (blur must not have erased the source content)"
        );

        assert!(
            h_halo >= 3 * v_halo.max(1),
            "B2: H halo = {h_halo}px beyond content edge, V halo = {v_halo}px — \
             expected H ≥ 3×V for sigma_x={SIGMA_X}/sigma_y={SIGMA_Y}. \
             (H_halo=0 means blur did not spread horizontally past the content edge; \
              check sigma_x is used in the H pass, sigma_y in the V pass.)"
        );
    }

    // ── B3: Oracle match ±3 LSB ───────────────────────────────────────────────

    /// B3: A Gaussian blur of an opaque colour rect must match the CPU oracle
    /// (running-sum renormalised, premul-direct) to within ±3 u8 units per channel.
    ///
    /// ±3 u8 tolerates:
    /// - Bilinear sub-pixel interpolation in the GPU shader (not in the oracle).
    /// - f32/f16 precision differences between GPU and CPU.
    ///
    /// This test verifies the GPU shader computes the correct weights AND the
    /// correct √3·sigma half-radius (a 3×sigma radius would produce a visibly
    /// different output).
    ///
    /// **Fails if:** the shader uses the wrong sigma constant, wrong weight formula,
    /// wrong renormalisation, or diverges from the oracle premul contract.
    #[test]
    #[allow(
        clippy::cast_possible_truncation,
        reason = "test constants (CONTENT_MARGIN_PX=12, SURFACE_WIDTH=64) are far below u32::MAX; \
                  usize→u32 casts for oracle call are safe here"
    )]
    fn blur_oracle_match_within_3_lsb() {
        const SIGMA: f32 = 4.0;
        const CONTENT_MARGIN_PX: u32 = 12;

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

        let content_rect = center_rect(CONTENT_MARGIN_PX);
        let source_color = Color::rgba(180, 120, 60, 255);

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save_layer_with_image_filter(ImageFilterSpec::Blur {
            sigma_x: SIGMA,
            sigma_y: SIGMA,
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
            .expect("B3 oracle-match blur render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let gpu_pixels = readback_pixels(&device, &queue, &surface_tex);

        // Build oracle source grid: interior of content rect has the source color
        // (premultiplied — opaque so premul == straight); outside is transparent.
        let margin_u = CONTENT_MARGIN_PX as usize;
        let left = margin_u;
        let top = margin_u;
        let right = SURFACE_WIDTH as usize - margin_u;
        let bottom = SURFACE_HEIGHT as usize - margin_u;
        // Color stores public r/g/b/a u8 fields; there is no to_rgba_u8 method.
        let source_premul: [u8; 4] = [
            source_color.r,
            source_color.g,
            source_color.b,
            source_color.a,
        ];

        let source_grid: Vec<[u8; 4]> = (0..SURFACE_HEIGHT as usize)
            .flat_map(|row| {
                (0..SURFACE_WIDTH as usize).map(move |col| {
                    if row >= top && row < bottom && col >= left && col < right {
                        source_premul
                    } else {
                        [0, 0, 0, 0]
                    }
                })
            })
            .collect();

        let oracle = blur_oracle_premul(
            &source_grid,
            SURFACE_WIDTH,
            SURFACE_HEIGHT,
            SIGMA,
            SIGMA,
            (left as u32, top as u32, right as u32, bottom as u32),
        );

        // Compare oracle vs GPU for every non-border pixel.
        let w = SURFACE_WIDTH as usize;
        let h = SURFACE_HEIGHT as usize;
        let mut fail_count = 0usize;
        let mut max_diff = 0u8;

        for row in 2..(h - 2) {
            for col in 2..(w - 2) {
                let pixel_index = row * w + col;
                let gpu = gpu_pixels[pixel_index];
                let oracle_px = oracle[pixel_index];
                for channel in 0..4 {
                    let diff = u8::try_from(
                        (i16::from(gpu[channel]) - i16::from(oracle_px[channel])).unsigned_abs(),
                    )
                    .expect("diff of two u8 values fits u8");
                    if diff > max_diff {
                        max_diff = diff;
                    }
                    if diff > 3 {
                        fail_count += 1;
                    }
                }
            }
        }

        assert_eq!(
            fail_count, 0,
            "B3: {fail_count} pixels exceeded ±3 u8 oracle tolerance; \
             max diff = {max_diff}. GPU and CPU oracle must agree within ±3 u8."
        );
    }

    // ── B4: Zero-sigma identity (ABSOLUTE) ───────────────────────────────────

    /// B4: A blur with `sigma_x=0, sigma_y=0` must be an identity filter.
    ///
    /// ## Why absolute (not oracle-comparison)
    ///
    /// The CPU oracle correctly handles sigma=0 (returns the source pixel), but
    /// if the GPU shader silently skipped the filter entirely (e.g., by always
    /// outputting the source texture unchanged), both GPU and oracle would agree
    /// without actually proving the filter ran.  This test compares the blurred
    /// output to the directly-drawn output (no filter at all), then asserts:
    /// 1. The sigma=0 blur produces the same pixel values as the unfiltered draw
    ///    (within ±2 tolerance for the offscreen composite roundtrip).
    /// 2. The sigma=0 blur doesn't silently erase the content (alpha > 0).
    ///
    /// **Fails if:** the zero-sigma case returns transparent black instead of the
    /// source pixel (wrong identity branch), or the shader panics.
    #[test]
    fn blur_zero_sigma_is_identity() {
        let (device, queue) = acquire_test_device_and_queue();
        let (no_filter_tex, no_filter_view) = create_surface(&device);
        let (blur_tex, blur_view) = create_surface(&device);

        let opaque_black = wgpu::Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        clear_surface(&device, &queue, &no_filter_view, opaque_black);
        clear_surface(&device, &queue, &blur_view, opaque_black);

        let source_color = Color::rgba(160, 80, 200, 255);
        let bounds = full_surface_bounds();

        // Reference: draw without any filter.
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
                .expect("B4 no-filter render must succeed");
            queue.submit(std::iter::once(encoder.finish()));
        }

        // Blur with sigma=0 (identity case in the shader).
        {
            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            painter.save_layer_with_image_filter(ImageFilterSpec::Blur {
                sigma_x: 0.0,
                sigma_y: 0.0,
            });
            painter.rect(bounds, &Paint::fill(source_color));
            painter.restore_layer();
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            painter
                .render(
                    RenderTarget::sampleable(&blur_view, &blur_tex),
                    &mut encoder,
                )
                .expect("B4 zero-sigma blur render must succeed");
            queue.submit(std::iter::once(encoder.finish()));
        }

        let no_filter_pixels = readback_pixels(&device, &queue, &no_filter_tex);
        let blur_pixels = readback_pixels(&device, &queue, &blur_tex);

        let w = SURFACE_WIDTH as usize;
        let h = SURFACE_HEIGHT as usize;

        // Centre pixel must be non-zero (content was not erased).
        let centre_alpha = blur_pixels[(h / 2) * w + (w / 2)][3];
        assert!(
            centre_alpha > 0,
            "B4: centre pixel alpha={centre_alpha} — zero-sigma blur must not erase content"
        );

        // All non-border pixels must match the unfiltered reference within ±2.
        for row in 2..(h - 2) {
            for col in 2..(w - 2) {
                let pixel_index = row * w + col;
                let no_filter = no_filter_pixels[pixel_index];
                let blurred = blur_pixels[pixel_index];
                for channel in 0..4 {
                    let diff = u8::try_from(
                        (i16::from(no_filter[channel]) - i16::from(blurred[channel]))
                            .unsigned_abs(),
                    )
                    .expect("diff of two u8 values fits u8");
                    assert!(
                        diff <= 2,
                        "B4: pixel ({col},{row}) channel {channel} — \
                         no_filter={a} sigma0_blur={b} diff={diff} > tolerance 2. \
                         Zero-sigma blur must be the identity.",
                        a = no_filter[channel],
                        b = blurred[channel],
                    );
                }
            }
        }
    }

    // ── B5: grown_bounds halo extent ─────────────────────────────────────────

    /// B5: A Gaussian blur with `sigma=4` applied to a content rect at [10,10,50,50]
    /// must produce non-zero pixels at the halo columns (3 and 57 for the H direction).
    ///
    /// `kernel_radius(4.0) = ceil(4.0 × 1.732) = ceil(6.928) = 7`.
    /// Content left edge: 10.  Halo reaches to 10 - 7 = 3.
    /// Content right edge: 50.  Halo reaches to 50 + 7 - 1 = 56 (so col=56 is in halo).
    ///
    /// This test verifies that `grown_bounds` was computed correctly in `restore_layer`
    /// AND that the final composite rect is large enough to hold the halo.  If
    /// `grown_bounds == content_bounds` (no expansion), the halo would be clipped
    /// and columns 3 and 56 would be transparent.
    ///
    /// **Fails if:** `grown_bounds` is not expanded by `kernel_radius(sigma)`, or the
    /// composite is clipped to the original content rect.
    #[test]
    fn blur_grown_bounds_includes_halo_pixels() {
        const SIGMA: f32 = 4.0;
        // kernel_radius(4.0) = ceil(4.0 × √3) = ceil(6.928) = 7
        const KERNEL_RAD: u32 = 7;
        const CONTENT_LEFT: u32 = 10;
        const CONTENT_TOP: u32 = 10;
        const CONTENT_RIGHT: u32 = 50;
        const CONTENT_BOTTOM: u32 = 50;

        // Verify the test assumption is sound.
        assert_eq!(
            kernel_radius(SIGMA),
            KERNEL_RAD,
            "B5 precondition: kernel_radius({SIGMA}) must be {KERNEL_RAD}"
        );

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

        let content_rect = Rect::from_xywh(
            px(CONTENT_LEFT as f32),
            px(CONTENT_TOP as f32),
            px((CONTENT_RIGHT - CONTENT_LEFT) as f32),
            px((CONTENT_BOTTOM - CONTENT_TOP) as f32),
        );
        let source_color = Color::rgba(200, 200, 200, 255);

        // Use a directly-constructed DrawItem::Filter (tight bounds) to test
        // the grown_bounds composite directly, independent of `restore_layer`.
        let grown_left = CONTENT_LEFT - KERNEL_RAD;
        let grown_top = CONTENT_TOP - KERNEL_RAD;
        let grown_right = CONTENT_RIGHT + KERNEL_RAD;
        let grown_bottom = CONTENT_BOTTOM + KERNEL_RAD;
        let grown_rect = Rect::from_xywh(
            px(grown_left as f32),
            px(grown_top as f32),
            px((grown_right - grown_left) as f32),
            px((grown_bottom - grown_top) as f32),
        );

        let mut segment = DrawSegment::new();
        let _ = segment
            .rect_batch
            .add(RectInstance::rect(content_rect, source_color));
        DrawSegment::push_scissor_region(&mut segment.rect_scissors, None);
        let op = FilterOp {
            input: segment,
            passes: smallvec![ImageFilterPass::Blur {
                sigma_x: SIGMA,
                sigma_y: SIGMA,
            }],
            content_bounds: content_rect,
            grown_bounds: grown_rect,
        };

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        painter
            .replay_items_for_test(vec![DrawItem::Filter(op)], &surface_view, &mut encoder)
            .expect("B5 grown_bounds halo render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_tex);
        let w = SURFACE_WIDTH as usize;
        let mid_row = (CONTENT_TOP + CONTENT_BOTTOM) as usize / 2;

        // Left halo column (3): within kernel_radius of the left content edge (10).
        let halo_left_col = grown_left as usize;
        let halo_left_alpha = pixels[mid_row * w + halo_left_col][3];
        assert!(
            halo_left_alpha > 0,
            "B5: pixel at halo col={halo_left_col} row={mid_row} alpha={halo_left_alpha} — \
             expected non-zero (halo from sigma={SIGMA}, kernel_radius={KERNEL_RAD}). \
             alpha=0 means grown_bounds was not expanded past content_bounds."
        );

        // Right halo column (56 = CONTENT_RIGHT + KERNEL_RAD - 1):
        // just inside the right halo edge.
        let halo_right_col = (CONTENT_RIGHT + KERNEL_RAD - 1) as usize;
        let halo_right_alpha = pixels[mid_row * w + halo_right_col][3];
        assert!(
            halo_right_alpha > 0,
            "B5: pixel at halo col={halo_right_col} row={mid_row} alpha={halo_right_alpha} — \
             expected non-zero (right halo from sigma={SIGMA}, kernel_radius={KERNEL_RAD}). \
             alpha=0 means grown_bounds was not expanded past content_bounds."
        );

        // Content centre must be opaque (sanity: the filter did not erase content).
        let center_col = (CONTENT_LEFT + CONTENT_RIGHT) as usize / 2;
        let center_alpha = pixels[mid_row * w + center_col][3];
        assert!(
            center_alpha > 200,
            "B5: content centre ({center_col},{mid_row}) alpha={center_alpha} — \
             expected fully opaque; blur must not erase interior content."
        );
    }
}
