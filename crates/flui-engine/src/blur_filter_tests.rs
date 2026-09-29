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

#[cfg(all(test, feature = "testing"))]
mod gpu_tests {
    use std::sync::Arc;

    use flui_foundation::geometry::Rect;
    use flui_painting::Paint;
    use flui_painting::styling::Color;

    use crate::{
        command_ir::ImageFilterSpec, effects::kernel_radius, painter::WgpuPainter,
        render_target::RenderTarget,
    };

    // ── Harness constants ─────────────────────────────────────────────────────

    const SURFACE_WIDTH: u32 = 64;
    const SURFACE_HEIGHT: u32 = 64;
    const SURFACE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

    // ── Harness helpers ───────────────────────────────────────────────────────

    fn acquire_test_device_and_queue() -> (Arc<wgpu::Device>, Arc<wgpu::Queue>) {
        crate::test_support::test_device_and_queue("BlurFilter Test Device")
    }

    fn create_surface(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView) {
        crate::test_support::create_sampleable_target(
            device,
            "BlurFilter Test Surface",
            SURFACE_WIDTH,
            SURFACE_HEIGHT,
            SURFACE_FORMAT,
        )
    }

    fn clear_surface(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        color: wgpu::Color,
    ) {
        crate::test_support::clear_target(device, queue, view, color);
    }

    /// Read all pixels back from `texture` as `[r, g, b, a]` u8 quads.
    fn readback_pixels(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
    ) -> Vec<[u8; 4]> {
        crate::test_support::readback_pixels(device, queue, texture, SURFACE_WIDTH, SURFACE_HEIGHT)
    }

    fn build_painter(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> WgpuPainter {
        WgpuPainter::with_shared_device(
            device,
            queue,
            SURFACE_FORMAT,
            (SURFACE_WIDTH, SURFACE_HEIGHT),
        )
    }
    fn center_rect(edge_margin_px: u32) -> Rect<f64> {
        let margin = edge_margin_px as f32;
        Rect::from_xywh(
            f64::from(margin),
            f64::from(margin),
            f64::from(SURFACE_WIDTH as f32 - 2.0 * margin),
            f64::from(SURFACE_HEIGHT as f32 - 2.0 * margin),
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

    // ── B2: Anisotropy — sigma_x=8/sigma_y=2 → H-spread ≫ V-spread ──────────

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
        painter.draw_rect(content_rect, &Paint::fill(source_color));
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

    // ── B5: grown_bounds halo extent ─────────────────────────────────────────

    // ── B6: Off-origin readback — grown-bounds sizing discriminator ─────────

    // ── B7: Intermediate-size producer assertion (sub-viewport, content-AABB) ──

    // ── B8: Clip-detection (content-AABB MUST NOT under-estimate) ──────────────

    // ── B9: Circle content AABB — radius factor must be included ─────────────

    // ── B10: Gradient fallback — content_aabb returns None for gradient kinds ──

    // ── B11: Rect-only layer still fires optimization after gradient gate ─────

    // ── B12: Clipped rect in sub-viewport filter layer — non-identity scissor rebase ──
}
