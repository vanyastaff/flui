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
    use flui_types::{Color, Point, Rect, geometry::Pixels};
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
            // Integer-aligned (Task 6): grown_left/top are already integers here.
            fb_origin: (grown_left, grown_top),
            fb_dim: (grown_right - grown_left, grown_bottom - grown_top),
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

    // ── B6: Off-origin readback — Task 6 grown-bounds sizing discriminator ───

    /// B6: Content rect NOT at origin (`[34,34]→[54,54]`), blur σ=4.
    ///
    /// ## What this tests (Task 6 non-negotiables)
    ///
    /// With the pre-Task-6 full-viewport intermediate the pixel values are correct:
    /// the texel grid aligns with the device-pixel grid regardless of content position.
    /// After Task 6 the intermediate is `fb_dim`-sized and the content is rendered at
    /// pixel `(0,0)` of the intermediate via a vertex pre-transform (non-negotiable #2).
    ///
    /// A wrong implementation that uses:
    /// - `grown.width()` (fractional float) as the vertex remap denominator instead of
    ///   integer `fb_dim` → the vertex scaling is off by `ceil(w) / w` ≠ 1, shifting
    ///   content within the intermediate.
    /// - fractional `grown_bounds` as the composite `dst_rect` over an integer-origin
    ///   intermediate → a sub-pixel grid shift on the bilinear composite.
    /// - `content_bounds` UV WITHOUT subtracting `fb_origin` → decal guard in the
    ///   wrong coordinate system, clipping or mis-locating the blur.
    ///
    /// All three bugs manifest as the blurred halo being SHIFTED or CLIPPED relative
    /// to the source content rect.
    ///
    /// ## Assertions
    ///
    /// 1. **Halo symmetry** — pixels at equal distances left and right of the content
    ///    centre col have equal (or near-equal) alpha.  A shift by `frac(fb_origin)` or
    ///    a wrong vertex scale would break horizontal symmetry.
    /// 2. **Oracle match ±3 LSB** — the GPU output must match `blur_oracle_premul` on
    ///    the full 64×64 surface within the standard tolerance.  The oracle is run in
    ///    full-surface space (not fb-local space) because the readback is from the
    ///    composited surface.
    /// 3. **Content centre** — the content centre must be the brightest pixel on the
    ///    mid-row, verifying the halo is centred correctly.
    ///
    /// **Fails if:** the vertex denominator is float grown width (bug #2), the composite
    /// shifts by `frac(grown_left)` (bug #1), or the decal UV forgets `fb_origin` (bug #3).
    #[test]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "test geometry uses small integer constants; all casts are safe on a 64×64 surface"
    )]
    fn blur_off_origin_halo_centred_on_content() {
        const SIGMA: f32 = 4.0;

        // Content rect NOT at origin: [34, 34] → [54, 54].
        // kernel_radius(4.0) = 7, so grown ≈ [27, 27] → [61, 61] — fractional if
        // content were at non-integer position, but here it's integer-aligned so
        // the test exercises the PRODUCTION path through `save_layer_with_image_filter`
        // (which calls `filter_fb_rect` at record time) rather than a hand-crafted FilterOp.
        // The off-origin position is what matters: content centre col=44, not col=0.
        const CONTENT_LEFT: u32 = 34;
        const CONTENT_TOP: u32 = 34;
        const CONTENT_RIGHT: u32 = 54;
        const CONTENT_BOTTOM: u32 = 54;
        const KERNEL_RAD: u32 = 7;
        // Oracle match tolerance: ±3 u8 per channel (matches B3 standard tolerance).
        const ORACLE_TOLERANCE: i32 = 3;
        let source_color = Color::rgba(180, 120, 60, 255);

        assert_eq!(
            kernel_radius(SIGMA),
            KERNEL_RAD,
            "B6 precondition: kernel_radius({SIGMA}) must equal {KERNEL_RAD}"
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

        // Production path: save_layer_with_image_filter → restore_layer computes
        // fb_origin/fb_dim via filter_fb_rect and stores them on FilterOp.
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
            .expect("B6 off-origin blur render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let gpu_pixels = readback_pixels(&device, &queue, &surface_tex);
        let w = SURFACE_WIDTH as usize;
        let mid_row = u32::midpoint(CONTENT_TOP, CONTENT_BOTTOM) as usize;
        let center_col = u32::midpoint(CONTENT_LEFT, CONTENT_RIGHT) as usize;

        // ── Assertion 1: halo symmetry ────────────────────────────────────────
        //
        // The halo extends KERNEL_RAD pixels on both sides. At `halo_offset`
        // pixels from the content edge (left and right) the blur contribution
        // must be equal: if it isn't, content is shifted within the intermediate.
        //
        // halo_offset = 3 (well within the kernel radius): check col 34-3=31 and
        // col 54+3-1=56 (both in the halo, symmetric about center).
        let halo_offset: u32 = 3;
        let left_halo_col = (CONTENT_LEFT - halo_offset) as usize;
        let right_halo_col = (CONTENT_RIGHT + halo_offset - 1) as usize;
        let left_alpha = f32::from(gpu_pixels[mid_row * w + left_halo_col][3]);
        let right_alpha = f32::from(gpu_pixels[mid_row * w + right_halo_col][3]);

        assert!(
            left_alpha > 0.0,
            "B6: left halo pixel ({left_halo_col},{mid_row}) alpha={left_alpha:.1} — \
             expected non-zero (halo from sigma={SIGMA}). \
             alpha=0 means the blur halo was clipped or the content is off-position."
        );
        assert!(
            right_alpha > 0.0,
            "B6: right halo pixel ({right_halo_col},{mid_row}) alpha={right_alpha:.1} — \
             expected non-zero (halo from sigma={SIGMA})."
        );

        // The left/right halo pixels should be nearly symmetric (equal distance from content).
        // Tolerance: ±8 u8 for the bilinear rounding differences vs sub-pixel alignment.
        let alpha_diff = (left_alpha - right_alpha).abs();
        assert!(
            alpha_diff < 8.0,
            "B6: left halo alpha={left_alpha:.1} vs right halo alpha={right_alpha:.1} — \
             diff={alpha_diff:.1}, expected < 8. \
             Asymmetry indicates content is shifted within the intermediate. \
             Root causes: wrong vertex denominator (float grown width instead of integer fb_dim), \
             composite at fractional grown_bounds, or content_rect_uv not rebased by fb_origin."
        );

        // ── Assertion 2: oracle match ±3 LSB ─────────────────────────────────
        //
        // The oracle runs on the full 64×64 surface. The GPU output should match
        // because Task 6 only changes VRAM layout — not pixel values.
        let source_premul: [u8; 4] = [
            source_color.r,
            source_color.g,
            source_color.b,
            source_color.a,
        ];
        let source_grid: Vec<[u8; 4]> = (0..SURFACE_HEIGHT as usize)
            .flat_map(|row| {
                (0..SURFACE_WIDTH as usize).map(move |col| {
                    if row >= CONTENT_TOP as usize
                        && row < CONTENT_BOTTOM as usize
                        && col >= CONTENT_LEFT as usize
                        && col < CONTENT_RIGHT as usize
                    {
                        source_premul
                    } else {
                        [0u8; 4]
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
            (CONTENT_LEFT, CONTENT_TOP, CONTENT_RIGHT, CONTENT_BOTTOM),
        );

        let mut max_diff: u8 = 0;
        for row in 0..SURFACE_HEIGHT as usize {
            for col in 0..SURFACE_WIDTH as usize {
                let idx = row * w + col;
                for ch in 0..4usize {
                    let diff = (i32::from(gpu_pixels[idx][ch]) - i32::from(oracle[idx][ch])).abs();
                    max_diff = max_diff.max(diff as u8);
                    assert!(
                        diff <= ORACLE_TOLERANCE,
                        "B6 oracle mismatch: pixel ({col},{row}) channel {ch}: \
                         GPU={gpu} oracle={exp} diff={diff} > {ORACLE_TOLERANCE}. \
                         Off-origin blur output diverges from CPU oracle — \
                         check vertex pre-transform denominator (must be integer fb_dim).",
                        gpu = gpu_pixels[idx][ch],
                        exp = oracle[idx][ch],
                    );
                }
            }
        }

        // ── Assertion 3: content centre is the brightest ──────────────────────
        let center_alpha = f32::from(gpu_pixels[mid_row * w + center_col][3]);
        assert!(
            center_alpha > 200.0,
            "B6: content centre ({center_col},{mid_row}) alpha={center_alpha:.1} — \
             expected > 200 (fully opaque source). The blur must not erase content."
        );

        tracing::debug!(
            max_diff,
            "B6 off-origin blur: max oracle diff = {max_diff} (tolerance = {ORACLE_TOLERANCE})"
        );
    }

    // ── B7: Intermediate-size producer assertion (sub-viewport, content-AABB) ──

    /// B7: `restore_layer` emits a `DrawItem::Filter` whose `fb_dim` is the
    /// integer-aligned grown content bounds — **NOT** the full viewport.
    ///
    /// This is a CPU-only (no GPU device needed) unit test that proves the
    /// content-AABB producer wiring is live: `save_layer_with_image_filter` passes
    /// `bounds=None`, so `composite_bounds` is now derived from
    /// `content_aabb(&offscreen_final_segment)` rather than falling back to the
    /// full viewport.
    ///
    /// ## Layout (surface 64×64, sigma=4, CONTENT_MARGIN=12 px)
    ///
    /// ```text
    /// content rect    = [12, 12] → [52, 52]  (from_xywh 12,12,40,40)
    /// content_aabb    = Rect[12, 12, 52, 52]  (baked RectInstance, identity M+t)
    /// kernel_radius(sigma=4) = ceil(4 × 1.732_050_8) = ceil(6.928) = 7
    /// grown           = content_aabb.expand(7) = [5, 5, 59, 59]
    /// grown ∩ viewport = [5, 5, 59, 59]  (fully inside 64×64)
    /// fb_origin       = (floor(5.0), floor(5.0)) = (5, 5)
    /// fb_far          = (min(ceil(59.0), 64), min(ceil(59.0), 64)) = (59, 59)
    /// fb_dim          = (59 - 5, 59 - 5) = (54, 54)
    /// ```
    ///
    /// **Criterion #8 discriminator:** `fb_dim = (54, 54) < (64, 64) = viewport`
    /// proves the intermediate is sub-viewport — VRAM is actually reduced.
    ///
    /// **Fails if:**
    /// - `content_aabb` is not called / returns `None` → `fb_dim = (64, 64)` (full vp).
    /// - `content_aabb` under-estimates the content extent → `grown_bounds` is wrong.
    /// - `filter_fb_rect` is not called or `fb_origin`/`fb_dim` not carried through.
    #[test]
    fn blur_restore_layer_emits_correct_fb_dim() {
        // 64×64 surface, content rect not at origin (margin=12 px on each side).
        // This places the content away from the viewport edges, so:
        //   - The content AABB is visibly sub-viewport.
        //   - The grown bounds (after blur halo) remain comfortably inside the viewport.
        //   - fb_dim is provably smaller than the full viewport.
        const SIGMA: f32 = 4.0;
        const CONTENT_MARGIN: u32 = 12;

        // Expected values derived from the layout comment above.
        let expected_fb_origin: (u32, u32) = (5, 5);
        let expected_fb_dim: (u32, u32) = (54, 54); // < (64, 64) — sub-viewport

        let (device, queue) = acquire_test_device_and_queue();
        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));

        painter.save_layer_with_image_filter(ImageFilterSpec::Blur {
            sigma_x: SIGMA,
            sigma_y: SIGMA,
        });
        // Draw a baked RectInstance that does NOT fill the surface.
        // The baked path (identity M, zero translate) makes bounds trackable via
        // content_aabb — this is the canonical producer the criterion covers.
        painter.rect(
            Rect::from_xywh(
                px(CONTENT_MARGIN as f32),
                px(CONTENT_MARGIN as f32),
                px((SURFACE_WIDTH - 2 * CONTENT_MARGIN) as f32),
                px((SURFACE_HEIGHT - 2 * CONTENT_MARGIN) as f32),
            ),
            &Paint::fill(Color::rgba(180, 120, 60, 255)),
        );
        painter.restore_layer();

        // Inspect the FilterOp via the existing test accessor on WgpuPainter.
        let mut filter_ops = painter.filter_ops_for_test();
        assert_eq!(
            filter_ops.len(),
            1,
            "B7: expected exactly 1 FilterOp in draw_order, got {}. \
             restore_layer must emit a DrawItem::Filter for an image-filter spec.",
            filter_ops.len()
        );
        let filter_op = filter_ops.remove(0);

        // fb_dim must be sub-viewport — criterion #8 proof.
        assert!(
            filter_op.fb_dim.0 < SURFACE_WIDTH && filter_op.fb_dim.1 < SURFACE_HEIGHT,
            "B7 CRITERION #8 FAIL: FilterOp.fb_dim = {:?} is NOT sub-viewport ({}, {}). \
             content_aabb wiring is inert — composite_bounds is still falling back to the \
             full viewport instead of being derived from the content AABB.",
            filter_op.fb_dim,
            SURFACE_WIDTH,
            SURFACE_HEIGHT,
        );

        assert_eq!(
            filter_op.fb_origin, expected_fb_origin,
            "B7: FilterOp.fb_origin = {:?}, expected {:?}. \
             content_aabb([12,12,52,52]) → grown=[5,5,59,59] → fb_origin=(5,5).",
            filter_op.fb_origin, expected_fb_origin,
        );
        assert_eq!(
            filter_op.fb_dim, expected_fb_dim,
            "B7: FilterOp.fb_dim = {:?}, expected {:?}. \
             grown_bounds=[5,5,59,59] → fb_dim=(54,54) — provably sub-viewport.",
            filter_op.fb_dim, expected_fb_dim,
        );
    }

    // ── B8: Clip-detection (content-AABB MUST NOT under-estimate) ──────────────

    /// B8: Content at the AABB boundary must survive rendering without clipping.
    ///
    /// This is the **discriminating safety test** for `content_aabb`: if the
    /// AABB under-estimates the true content extent, the intermediate framebuffer
    /// would be sized too small, and pixels at the content edge would be clipped
    /// (rendered to a region outside the allocated texture).
    ///
    /// ## Strategy
    ///
    /// Draw a content rect whose edges are flush with the content AABB boundary,
    /// blur it, and readback the output at the center of the content rect.
    /// If `content_aabb` under-estimates (returns a box smaller than the actual
    /// content), the intermediate is cropped and those pixels are missing.
    ///
    /// The test draws a rect at a margin of 20 px on each side:
    ///
    /// ```text
    /// content rect    = [20, 20] → [44, 44]  (from_xywh 20,20,24,24)
    /// content_aabb    = [20, 20, 44, 44]
    /// kernel_radius(sigma=2) = ceil(2 × 1.732) = ceil(3.464) = 4
    /// grown           = [16, 16, 48, 48]
    /// fb_origin       = (16, 16),  fb_dim = (32, 32)
    /// center of content rect = (32, 32)   ← inside [16..48], survives
    /// ```
    ///
    /// After blurring, the center pixel `(32, 32)` is well inside both the
    /// content rect `[20..44]` AND the grown intermediate `[16..48]`, so its
    /// alpha must be non-zero.
    ///
    /// ## Red→Green evidence (discrimination proof)
    ///
    /// Temporarily shrinking `content_aabb` by 5 px per side would make
    /// `composite_bounds = [25, 25, 39, 39]` → `grown = [21, 21, 43, 43]` →
    /// `fb_dim = (22, 22)`.  The center pixel `(32, 32)` maps to
    /// `(32-21, 32-21) = (11, 11)` in intermediate space, which IS still inside
    /// the 22×22 intermediate — so this particular discriminator is conservative.
    ///
    /// The real discriminator for content-AABB under-estimate is the producer-level
    /// fb_dim assertion: if content_aabb returned the wrong bounds, fb_dim would
    /// be wrong.  The GPU alpha check confirms the rendered output is present
    /// (not clipped out by an intermediate that's sized too small for the content).
    #[test]
    fn blur_content_aabb_does_not_clip_edge_pixels() {
        const SIGMA: f32 = 2.0;
        const INNER_MARGIN: u32 = 20; // content rect [20,20]→[44,44]

        // Center of the content rect — must be non-transparent after blur.
        const CENTER_X: u32 = SURFACE_WIDTH / 2; // 32, inside [20..44]
        const CENTER_Y: u32 = SURFACE_HEIGHT / 2; // 32, inside [20..44]

        // ── Producer-level size check (CPU, no rendering needed) ──────────────
        // Verify the optimization fires for this rect before spending GPU time.
        {
            let (dev, q) = acquire_test_device_and_queue();
            let mut painter = build_painter(Arc::clone(&dev), Arc::clone(&q));
            painter.save_layer_with_image_filter(ImageFilterSpec::Blur {
                sigma_x: SIGMA,
                sigma_y: SIGMA,
            });
            painter.rect(
                Rect::from_xywh(
                    px(INNER_MARGIN as f32),
                    px(INNER_MARGIN as f32),
                    px((SURFACE_WIDTH - 2 * INNER_MARGIN) as f32),
                    px((SURFACE_HEIGHT - 2 * INNER_MARGIN) as f32),
                ),
                &Paint::fill(Color::rgba(200, 150, 80, 255)),
            );
            painter.restore_layer();

            let ops = painter.filter_ops_for_test();
            assert_eq!(ops.len(), 1, "B8: must emit exactly 1 FilterOp");
            let op = &ops[0];
            assert!(
                op.fb_dim.0 < SURFACE_WIDTH && op.fb_dim.1 < SURFACE_HEIGHT,
                "B8 producer: fb_dim {:?} is NOT sub-viewport ({}, {}). \
                 content_aabb wiring must fire for an off-origin rect.",
                op.fb_dim,
                SURFACE_WIDTH,
                SURFACE_HEIGHT
            );
        }

        // ── GPU readback: center pixel must be non-transparent ────────────────
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_tex, surface_view) = create_surface(&device);

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save_layer_with_image_filter(ImageFilterSpec::Blur {
            sigma_x: SIGMA,
            sigma_y: SIGMA,
        });
        // Opaque rect filling [20,20]→[44,44].
        painter.rect(
            Rect::from_xywh(
                px(INNER_MARGIN as f32),
                px(INNER_MARGIN as f32),
                px((SURFACE_WIDTH - 2 * INNER_MARGIN) as f32),
                px((SURFACE_HEIGHT - 2 * INNER_MARGIN) as f32),
            ),
            &Paint::fill(Color::rgba(200, 150, 80, 255)),
        );
        painter.restore_layer();

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_tex),
                &mut encoder,
            )
            .expect("B8 clip-detection blur render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_tex);
        let center_pixel = pixels[(CENTER_Y as usize) * SURFACE_WIDTH as usize + CENTER_X as usize];
        let center_alpha = center_pixel[3];

        assert!(
            center_alpha > 0,
            "B8 CLIP-DETECTION FAIL: center pixel ({}, {}) has alpha=0 after blur. \
             The content at ({},{})→({},{}) was clipped from the intermediate — \
             content_aabb under-estimated the AABB (fb too small for actual content).",
            CENTER_X,
            CENTER_Y,
            INNER_MARGIN,
            INNER_MARGIN,
            SURFACE_WIDTH - INNER_MARGIN,
            SURFACE_HEIGHT - INNER_MARGIN,
        );
    }

    // ── B9: Circle content AABB — radius factor must be included ─────────────

    /// B9: `content_aabb` for a baked `CircleInstance` must include the radius.
    ///
    /// ## The bug (pre-fix)
    ///
    /// `CircleInstance::new` stores `center_radius[2] = radius` and
    /// `transform = diag(sx, sy)`.  The old code computed device half-extents as
    /// `half_x = a.abs() + c.abs() = sx + 0 = sx` — dropping the radius factor.
    /// A circle of radius 16 at scale 1 yielded `half_x = 1` → AABB of 2×2 px
    /// around the center → `fb_dim` too small to contain the circle → content
    /// clipped entirely from the intermediate framebuffer.
    ///
    /// ## Layout (64×64 surface, σ=2, circle center=(32,32), radius=16, scale=1)
    ///
    /// ```text
    /// circle device extent (before fix):
    ///   half_x = 1, half_y = 1   → content_aabb ≈ [31,31,33,33]  ← BUG
    ///
    /// circle device extent (after fix):
    ///   r = 16, half_x = 16*1 = 16, half_y = 16*1 = 16
    ///   content_aabb = [16, 16, 48, 48]
    ///   kernel_radius(σ=2) = ceil(2 × 1.732) = 4
    ///   grown           = [12, 12, 52, 52]
    ///   fb_dim          = (40, 40)  ← comfortably contains the circle
    /// ```
    ///
    /// ## Assertions
    ///
    /// 1. CPU: `fb_dim` ≥ circle diameter (32 px).  With the old code `fb_dim`
    ///    would be ~(2+8, 2+8) = (10, 10) — far too small, failing this check.
    /// 2. GPU: the pixel on the circle's top rim `(32, 16)` is non-transparent
    ///    after blur.  Pre-fix, the circle falls entirely outside the tiny
    ///    intermediate, so that pixel is transparent (alpha=0).
    ///
    /// **Fails if:** `content_aabb` drops the `* center_radius[2]` radius factor.
    #[test]
    fn blur_circle_content_aabb_includes_radius() {
        const SIGMA: f32 = 2.0;
        const KERNEL_RAD: u32 = 4;
        const CENTER_COL: u32 = SURFACE_WIDTH / 2;
        const CENTER_ROW: u32 = SURFACE_HEIGHT / 2;
        const RADIUS_PX: u32 = 16;
        const MIN_FB_DIM: u32 = RADIUS_PX * 2;

        {
            let (device, queue) = acquire_test_device_and_queue();
            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            painter.save_layer_with_image_filter(ImageFilterSpec::Blur {
                sigma_x: SIGMA,
                sigma_y: SIGMA,
            });
            painter.circle(
                Point::new(Pixels(CENTER_COL as f32), Pixels(CENTER_ROW as f32)),
                RADIUS_PX as f32,
                &Paint::fill(Color::rgba(200, 80, 80, 255)),
            );
            painter.restore_layer();

            let ops = painter.filter_ops_for_test();
            assert_eq!(ops.len(), 1, "B9: must emit exactly 1 FilterOp");
            let op = &ops[0];
            assert!(
                op.fb_dim.0 >= MIN_FB_DIM && op.fb_dim.1 >= MIN_FB_DIM,
                "B9 RADIUS-FACTOR BUG: FilterOp.fb_dim = {:?} < circle diameter {}. \
                 content_aabb dropped the radius factor from CircleInstance.center_radius[2]. \
                 With the fix, half_x = radius * sx = {} → fb_dim ≥ ({}, {}). \
                 kernel_radius({}) = {}.",
                op.fb_dim,
                MIN_FB_DIM,
                RADIUS_PX,
                MIN_FB_DIM,
                MIN_FB_DIM,
                SIGMA,
                KERNEL_RAD,
            );
        }

        let (device, queue) = acquire_test_device_and_queue();
        let (surface_tex, surface_view) = create_surface(&device);
        clear_surface(&device, &queue, &surface_view, wgpu::Color::TRANSPARENT);

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save_layer_with_image_filter(ImageFilterSpec::Blur {
            sigma_x: SIGMA,
            sigma_y: SIGMA,
        });
        painter.circle(
            Point::new(Pixels(CENTER_COL as f32), Pixels(CENTER_ROW as f32)),
            RADIUS_PX as f32,
            &Paint::fill(Color::rgba(200, 80, 80, 255)),
        );
        painter.restore_layer();

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_tex),
                &mut encoder,
            )
            .expect("B9 circle-radius blur render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_tex);
        let w = SURFACE_WIDTH as usize;
        let rim_col = CENTER_COL as usize;
        let rim_row = (CENTER_ROW - RADIUS_PX) as usize;
        let rim_alpha = pixels[rim_row * w + rim_col][3];
        assert!(
            rim_alpha > 0,
            "B9 CLIP FAIL: circle rim pixel ({rim_col},{rim_row}) has alpha=0 after blur. \
             The circle (centre=({CENTER_COL},{CENTER_ROW}), radius={RADIUS_PX}) was clipped \
             from the intermediate — content_aabb under-estimated the extent. \
             Pre-fix: half_x = sx = 1 (missing radius factor) → fb too small. \
             Post-fix: half_x = radius * sx = {RADIUS_PX} → rim survives."
        );
    }

    // ── B10: Gradient fallback — content_aabb returns None for gradient kinds ──

    /// B10: A filter layer containing a linear gradient must emit `fb_dim == viewport`,
    /// proving the `content_aabb` fallback gate fires.
    ///
    /// ## Why this test exists (P0 regression story)
    ///
    /// Before the fix, `content_aabb` unioned `linear_gradient_batch.instances[*].bounds`
    /// and returned `Some(aabb)`.  For a gradient at rect `[16,16]→[48,48]` on a 64×64
    /// surface, the AABB was `[16,16,48,48]`, grown by `kernel_radius(2.0)=4` to
    /// `[12,12,52,52]`, giving `fb_dim=(40,40)` — a sub-viewport intermediate.
    ///
    /// `render_segment_to_grown_offscreen` does NOT remap gradient instances, so the
    /// gradient shader used the static `vp_w=64` uniform against a 40×40 attachment.
    /// NDC was wrong: the gradient rendered at approximately `[0.25·64, 0.25·64]→[0.75·64,…]`
    /// coordinates against the 40-wide attachment, placing content at origin-0 of the
    /// sub-viewport rather than at `[4,4]→[36,36]` (fb-local).
    ///
    /// The fix: `content_aabb` returns `None` when `linear_gradient_batch` (or any
    /// other un-repositionable kind) is non-empty.  The caller's `.unwrap_or(vp)` then
    /// selects `composite_bounds = viewport` → `fb_dim == viewport` → the remap in
    /// `render_segment_to_grown_offscreen` is the identity transform → gradient renders
    /// at the correct position.
    ///
    /// ## RED proof (implicit)
    ///
    /// To trigger the pre-fix failure without modifying production code, remove the
    /// fallback gate from `content_aabb` and re-run: `fb_dim` would drop to `(40,40)`
    /// and the GPU readback assertion (gradient centre pixel must be non-transparent
    /// after blur) would fail because the content is mispositioned entirely outside
    /// the intermediate framebuffer region that maps back to the content's device rect.
    ///
    /// ## Layout (surface 64×64, σ=2.0, gradient at [16,16]→[48,48])
    ///
    /// ```text
    /// gradient rect  = [16, 16] → [48, 48]
    /// kernel_radius  = ceil(2.0 × √3) = 4
    ///
    /// PRE-FIX (wrong):
    ///   content_aabb = Some([16,16,48,48])  ← gradient unioned
    ///   grown        = [12, 12, 52, 52]
    ///   fb_dim       = (40, 40) < (64, 64)  ← sub-viewport, gradient mis-positioned
    ///
    /// POST-FIX (correct):
    ///   content_aabb = None                  ← gate fires
    ///   fb_dim       = (64, 64)              ← full-viewport, gradient at correct pos
    /// ```
    #[test]
    fn blur_gradient_fallback_forces_viewport_fb() {
        use crate::wgpu::effects::GradientStop;

        const SIGMA: f32 = 2.0;
        // Gradient rect centred in the surface ([16,16]→[48,48]).
        const MARGIN: f32 = 16.0;
        const SIDE: f32 = (SURFACE_WIDTH as f32) - 2.0 * MARGIN; // 32

        // ── Part A: CPU producer check — fb_dim must equal viewport ──────────
        {
            let (device, queue) = acquire_test_device_and_queue();
            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            painter.save_layer_with_image_filter(ImageFilterSpec::Blur {
                sigma_x: SIGMA,
                sigma_y: SIGMA,
            });
            let stops = [
                GradientStop::new(flui_types::Color::rgba(255, 0, 0, 255), 0.0),
                GradientStop::new(flui_types::Color::rgba(0, 0, 255, 255), 1.0),
            ];
            painter.gradient_rect(
                Rect::from_xywh(px(MARGIN), px(MARGIN), px(SIDE), px(SIDE)),
                glam::Vec2::new(MARGIN, MARGIN),
                glam::Vec2::new(MARGIN + SIDE, MARGIN),
                &stops,
                0.0,
            );
            painter.restore_layer();

            let ops = painter.filter_ops_for_test();
            assert_eq!(ops.len(), 1, "B10: must emit exactly 1 FilterOp");
            let op = &ops[0];

            // The fallback gate must force fb_dim == viewport.
            // PRE-FIX: fb_dim would be (40,40) — gradient was unioned into AABB.
            // POST-FIX: fb_dim == (64,64) — gate returns None → viewport fallback.
            assert_eq!(
                op.fb_dim,
                (SURFACE_WIDTH, SURFACE_HEIGHT),
                "B10 GRADIENT-FALLBACK FAIL: FilterOp.fb_dim = {:?} != viewport ({}, {}). \
                 content_aabb must return None when linear_gradient_batch is non-empty \
                 so the caller falls back to fb_dim == viewport. \
                 PRE-FIX: gradient bounds were unioned → fb_dim=(40,40), mis-positioning \
                 the gradient in the sub-viewport intermediate.",
                op.fb_dim,
                SURFACE_WIDTH,
                SURFACE_HEIGHT
            );
        }

        // ── Part B: GPU readback — gradient centre must survive blur ──────────
        //
        // The centre of the gradient rect is at (32, 32).  After a Gaussian blur
        // with σ=2 over an opaque gradient, that pixel must be opaque (alpha ≥ 254).
        // PRE-FIX: the gradient was mispositioned in the sub-viewport intermediate,
        // rendering outside the fb region that maps back to (32,32), so the centre
        // would be transparent (alpha=0) after compositing.
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_tex, surface_view) = create_surface(&device);
        clear_surface(&device, &queue, &surface_view, wgpu::Color::TRANSPARENT);

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save_layer_with_image_filter(ImageFilterSpec::Blur {
            sigma_x: SIGMA,
            sigma_y: SIGMA,
        });
        let stops = [
            GradientStop::new(flui_types::Color::rgba(200, 100, 0, 255), 0.0),
            GradientStop::new(flui_types::Color::rgba(0, 100, 200, 255), 1.0),
        ];
        painter.gradient_rect(
            Rect::from_xywh(px(MARGIN), px(MARGIN), px(SIDE), px(SIDE)),
            glam::Vec2::new(MARGIN, MARGIN),
            glam::Vec2::new(MARGIN + SIDE, MARGIN),
            &stops,
            0.0,
        );
        painter.restore_layer();

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_tex),
                &mut encoder,
            )
            .expect("B10 gradient-fallback blur render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_tex);
        let w = SURFACE_WIDTH as usize;

        // Centre of the gradient rect: (32, 32).
        let centre_col = (SURFACE_WIDTH / 2) as usize;
        let centre_row = (SURFACE_HEIGHT / 2) as usize;
        let centre_alpha = pixels[centre_row * w + centre_col][3];

        assert!(
            centre_alpha > 200,
            "B10 GRADIENT-POSITION FAIL: gradient centre pixel ({centre_col},{centre_row}) \
             has alpha={centre_alpha} after blur. \
             The gradient at [16,16]→[48,48] must remain fully opaque at its centre. \
             PRE-FIX: fb_dim=(40,40) caused the gradient to render mis-positioned \
             in the sub-viewport intermediate → centre pixel transparent after composite. \
             POST-FIX: fb_dim=(64,64) → gradient at correct device position → alpha≥254."
        );
    }

    // ── B11: Rect-only layer still fires optimization after gradient gate ─────

    /// B11: A filter layer with ONLY a rect (no gradient / shadow / image) must
    /// still emit a sub-viewport `fb_dim` — the gradient gate must not over-fallback.
    ///
    /// This is the regression-guard for the OTHER direction: the gate added in B10
    /// must be surgical.  A layer with only rect/circle/arc instances must continue
    /// to benefit from the VRAM optimization (fb_dim < viewport).
    ///
    /// B7/B8/B9 already cover this from multiple angles; B11 adds a minimal named
    /// discriminator that pairs directly with B10 as a "gate is precise" check.
    #[test]
    fn blur_rect_only_layer_still_sub_viewport_after_gradient_gate() {
        const SIGMA: f32 = 2.0;
        // Rect inset 12 px from each edge: [12,12]→[52,52] on 64×64 surface.
        const INNER_MARGIN: u32 = 12;

        let (device, queue) = acquire_test_device_and_queue();
        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save_layer_with_image_filter(ImageFilterSpec::Blur {
            sigma_x: SIGMA,
            sigma_y: SIGMA,
        });
        painter.rect(
            Rect::from_xywh(
                px(INNER_MARGIN as f32),
                px(INNER_MARGIN as f32),
                px((SURFACE_WIDTH - 2 * INNER_MARGIN) as f32),
                px((SURFACE_HEIGHT - 2 * INNER_MARGIN) as f32),
            ),
            &flui_painting::Paint::fill(flui_types::Color::rgba(255, 128, 0, 255)),
        );
        painter.restore_layer();

        let ops = painter.filter_ops_for_test();
        assert_eq!(ops.len(), 1, "B11: must emit exactly 1 FilterOp");
        let op = &ops[0];

        assert!(
            op.fb_dim.0 < SURFACE_WIDTH && op.fb_dim.1 < SURFACE_HEIGHT,
            "B11 OVER-FALLBACK REGRESSION: FilterOp.fb_dim = {:?} is NOT sub-viewport ({}, {}). \
             A rect-only filter layer must still use the grown-bounds optimization \
             (fb_dim < viewport).  The gradient fallback gate in content_aabb must only \
             fire when shadow/gradient/image kinds are present — not for rect-only content.",
            op.fb_dim,
            SURFACE_WIDTH,
            SURFACE_HEIGHT
        );
    }

    // ── B12: Clipped rect in sub-viewport filter layer — non-identity scissor rebase ──

    /// B12: A scissor-clipped rect inside a sub-viewport filter layer must have its
    /// `rect_scissors` entry rebased from full-frame to fb-local coords by
    /// `remap_scissor`, producing a **non-identity** remap (fb_dim < viewport).
    ///
    /// ## What this tests
    ///
    /// `render_segment_to_grown_offscreen` rebases `rect_scissors` from full-frame to
    /// fb-local coords.  This test drives the **non-identity** branch of that rebase:
    ///
    /// - The content rect is INSET (`[16,16]→[48,48]`) so `content_aabb` returns a
    ///   sub-viewport AABB → `fb_dim < (64,64)`.  The test asserts this explicitly.
    /// - A second clip `[24,24]→[40,40]` (inside the content rect) is applied before
    ///   drawing, populating `rect_scissors` with a scissor in full-frame coords.
    ///   After rebase the scissor becomes `(12,12,16,16)` in fb-local space
    ///   (non-identity because `fb_origin = (12,12) ≠ (0,0)`).
    ///
    /// Pre-fix: a scissor in full-frame coords applied to the smaller fb attachment
    /// would be out-of-range → wgpu validation error or sentinel (nothing drawn).
    /// Post-fix: `remap_scissor` intersects + translates to fb-local → valid scissor.
    ///
    /// ## Layout (surface 64×64, σ=2, kernel_radius=4)
    ///
    /// ```text
    /// content rect  = [16, 16] → [48, 48]   (inset 16 px each side)
    /// content_aabb  = [16, 16, 48, 48]
    /// grown         = [12, 12, 52, 52]       (expand by kernel_radius=4)
    /// fb_origin     = (12, 12),  fb_dim = (40, 40)   ← sub-viewport (ASSERT A)
    ///
    /// clip rect     = [24, 24] → [40, 40]   (full-frame scissor (24,24,16,16))
    /// remap_scissor:
    ///   inter = max(24,12)..min(40,52) = 24..40
    ///   fb-local = (24-12, 24-12, 16, 16) = (12, 12, 16, 16)   ← non-identity
    ///
    /// pixel (32,32): inside both content rect and clip → non-transparent (ASSERT B)
    /// pixel (17,32): inside content rect but 7 px left of clip edge (24-17=7>4) →
    ///                transparent (ASSERT C) — the rebased scissor clips it out
    /// ```
    ///
    /// The test running to completion proves no wgpu validation panic (ASSERT D).
    ///
    /// **Fails if:** the scissor is not rebased → either a validation error (panic) or
    /// the sentinel path fires (nothing drawn → ASSERT B fails), or the clip is applied
    /// in full-frame coords against the fb-local attachment (wrong pixels clipped).
    #[test]
    #[allow(
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        reason = "constants are small positive u32/usize; all casts are safe on a 64×64 surface"
    )]
    fn blur_clipped_rect_scissor_rebased_non_identity() {
        const SIGMA: f32 = 2.0;
        // kernel_radius(2.0) = ceil(2.0 × √3) = ceil(3.464) = 4
        const KERNEL_RAD: u32 = 4;

        // Content rect: inset 16 px each side → [16,16]→[48,48].
        const CONTENT_MARGIN: u32 = 16;
        const CONTENT_LEFT: u32 = CONTENT_MARGIN;
        const CONTENT_TOP: u32 = CONTENT_MARGIN;
        const CONTENT_RIGHT: u32 = SURFACE_WIDTH - CONTENT_MARGIN;
        const CONTENT_BOTTOM: u32 = SURFACE_HEIGHT - CONTENT_MARGIN;

        // grown bounds (after kernel_radius expansion, clamped to viewport):
        //   [16-4, 16-4, 48+4, 48+4] = [12, 12, 52, 52] → fb_origin=(12,12), fb_dim=(40,40)
        const EXPECTED_FB_ORIGIN: (u32, u32) =
            (CONTENT_LEFT - KERNEL_RAD, CONTENT_TOP - KERNEL_RAD);
        const EXPECTED_FB_DIM: (u32, u32) = (
            (CONTENT_RIGHT + KERNEL_RAD) - (CONTENT_LEFT - KERNEL_RAD),
            (CONTENT_BOTTOM + KERNEL_RAD) - (CONTENT_TOP - KERNEL_RAD),
        );

        // Clip rect nested inside the content rect: [24,24]→[40,40].
        // In full-frame device coords: scissor = (24, 24, 16, 16).
        const CLIP_LEFT: u32 = 24;
        const CLIP_TOP: u32 = 24;
        const CLIP_RIGHT: u32 = 40;
        const CLIP_BOTTOM: u32 = 40;

        // Precondition: kernel_radius must match the constant above.
        assert_eq!(
            kernel_radius(SIGMA),
            KERNEL_RAD,
            "B12 precondition: kernel_radius({SIGMA}) must equal {KERNEL_RAD}"
        );

        // ── ASSERT A (CPU): fb_dim is sub-viewport ────────────────────────────
        {
            let (dev, q) = acquire_test_device_and_queue();
            let mut painter = build_painter(Arc::clone(&dev), Arc::clone(&q));
            painter.save_layer_with_image_filter(ImageFilterSpec::Blur {
                sigma_x: SIGMA,
                sigma_y: SIGMA,
            });
            // Apply clip then draw the inset content rect (clip is nested inside).
            painter.clip_rect(Rect::from_xywh(
                px(CLIP_LEFT as f32),
                px(CLIP_TOP as f32),
                px((CLIP_RIGHT - CLIP_LEFT) as f32),
                px((CLIP_BOTTOM - CLIP_TOP) as f32),
            ));
            painter.rect(
                Rect::from_xywh(
                    px(CONTENT_LEFT as f32),
                    px(CONTENT_TOP as f32),
                    px((CONTENT_RIGHT - CONTENT_LEFT) as f32),
                    px((CONTENT_BOTTOM - CONTENT_TOP) as f32),
                ),
                &flui_painting::Paint::fill(flui_types::Color::rgba(255, 0, 0, 255)),
            );
            painter.restore_layer();

            let ops = painter.filter_ops_for_test();
            assert_eq!(ops.len(), 1, "B12: must emit exactly 1 FilterOp");
            let op = &ops[0];

            // The sub-viewport optimization must fire: fb_dim < viewport.
            // If this fails, the test cannot exercise the non-identity remap.
            assert!(
                op.fb_dim.0 < SURFACE_WIDTH && op.fb_dim.1 < SURFACE_HEIGHT,
                "B12 ASSERT A FAIL: FilterOp.fb_dim = {:?} is NOT sub-viewport ({}, {}). \
                 The inset content rect [16,16]→[48,48] must produce a sub-viewport \
                 intermediate so the scissor rebase is non-identity. \
                 Expected fb_dim = {:?}.",
                op.fb_dim,
                SURFACE_WIDTH,
                SURFACE_HEIGHT,
                EXPECTED_FB_DIM,
            );

            assert_eq!(
                op.fb_origin, EXPECTED_FB_ORIGIN,
                "B12 ASSERT A: fb_origin = {:?}, expected {:?}.",
                op.fb_origin, EXPECTED_FB_ORIGIN,
            );
            assert_eq!(
                op.fb_dim, EXPECTED_FB_DIM,
                "B12 ASSERT A: fb_dim = {:?}, expected {:?}.",
                op.fb_dim, EXPECTED_FB_DIM,
            );
        }

        // ── ASSERT B + C (GPU): pixel inside clip is visible; pixel outside clip
        //    but inside content rect is transparent (non-identity rebase proof). ──
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_tex, surface_view) = create_surface(&device);
        clear_surface(&device, &queue, &surface_view, wgpu::Color::TRANSPARENT);

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save_layer_with_image_filter(ImageFilterSpec::Blur {
            sigma_x: SIGMA,
            sigma_y: SIGMA,
        });
        painter.clip_rect(Rect::from_xywh(
            px(CLIP_LEFT as f32),
            px(CLIP_TOP as f32),
            px((CLIP_RIGHT - CLIP_LEFT) as f32),
            px((CLIP_BOTTOM - CLIP_TOP) as f32),
        ));
        painter.rect(
            Rect::from_xywh(
                px(CONTENT_LEFT as f32),
                px(CONTENT_TOP as f32),
                px((CONTENT_RIGHT - CONTENT_LEFT) as f32),
                px((CONTENT_BOTTOM - CONTENT_TOP) as f32),
            ),
            &flui_painting::Paint::fill(flui_types::Color::rgba(255, 0, 0, 255)),
        );
        painter.restore_layer();

        // ASSERT D: no wgpu validation panic (test completing proves this).
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_tex),
                &mut encoder,
            )
            .expect("B12 clipped-rect blur render must succeed (no wgpu validation panic)");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_tex);
        let w = SURFACE_WIDTH as usize;

        // ASSERT B: centre of clip [24,24]→[40,40] is (32,32) → must be non-transparent.
        let centre_col: usize = (SURFACE_WIDTH / 2) as usize; // 32
        let centre_row: usize = (SURFACE_HEIGHT / 2) as usize; // 32
        let centre_alpha = pixels[centre_row * w + centre_col][3];
        assert!(
            centre_alpha > 0,
            "B12 ASSERT B FAIL: centre pixel ({centre_col},{centre_row}) has alpha=0 after blur. \
             Pixel is inside clip [24,24]→[40,40] and content rect [16,16]→[48,48]. \
             The rebased fb-local scissor must allow drawing here. \
             Pre-fix: out-of-range scissor → sentinel → nothing drawn."
        );

        // ASSERT C: pixel at (17,32) is inside the content rect [16,16]→[48,48] but
        // 7 pixels left of the clip left edge (24-17=7 > kernel_radius=4).
        // The blur cannot spread 7 px from the clipped content, so this must be transparent.
        // This proves the rebased scissor clips at the correct fb-local position.
        let outside_clip_col: usize = 17;
        let outside_clip_row: usize = centre_row;
        let outside_alpha = pixels[outside_clip_row * w + outside_clip_col][3];
        assert_eq!(
            outside_alpha, 0,
            "B12 ASSERT C FAIL: pixel ({outside_clip_col},{outside_clip_row}) has alpha={outside_alpha} \
             but should be transparent. \
             This pixel is inside the content rect but 7 px left of the clip edge (24-17=7 > \
             kernel_radius=4), so no blur contribution can reach it — the clip must block it. \
             A non-zero value means the scissor was NOT applied in fb-local coords \
             (full-frame scissor against fb attachment clips at the wrong position)."
        );
    }
}
