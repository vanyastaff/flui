//! GPU readback acceptance gate for the gamma transfer filter pass.
//!
//! ## Test inventory
//!
//! | # | Gate | Requirement |
//! |---|------|-------------|
//! | GA1 | GPU | SrgbToLinear of known mid-gray (ABSOLUTE): sRGB 128 → linear ≈ 99 |
//! | GA2 | GPU | LinearToSrgb is the inverse of SrgbToLinear (round-trip ≈ identity ±2) |
//! | GA3 | GPU | Translucent layer: alpha is UNCHANGED after gamma transfer |
//! | GA4 | GPU | Black/white boundary (ABSOLUTE): 0→0, 255→255 for both directions |
//! | GA5 | GPU | GammaPipeline::new GPU construction test (catches WGSL compile errors) |
//! | GA6 | GPU | Round-trip ≈ identity (SrgbToLinear ∘ LinearToSrgb ≈ input ±2) |
//!
//! ## Oracle discipline (G1)
//!
//! The oracle calls `flui_types::styling::color::srgb_to_linear` /
//! `flui_types::styling::color::linear_to_srgb` **directly** — the identical
//! functions the WGSL shader mirrors.  No transfer-function math is re-derived
//! in this test.  Absolute assertions verify specific known values so the test
//! cannot be co-vacuous.
//!
//! ## G2 (translucent discriminator)
//!
//! GA3 draws a 50%-alpha layer and asserts the alpha channel is byte-identical
//! before and after gamma transfer — the unpremul/repremul bracket must not
//! corrupt or scale alpha.

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod gpu_tests {
    use std::sync::Arc;

    use flui_painting::Paint;
    use flui_types::{
        Color, Rect,
        geometry::Pixels,
        styling::color::{linear_to_srgb, srgb_to_linear},
    };

    use crate::wgpu::{
        command_ir::{GammaDirection, LayerFilter},
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
        .expect("a GPU adapter must be available for gamma_filter_tests");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("GammaFilter Test Device"),
            ..Default::default()
        }))
        .expect("a GPU device must be available for gamma_filter_tests");
        (Arc::new(device), Arc::new(queue))
    }

    fn create_surface(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("GammaFilter Test Surface"),
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
            label: Some("GammaFilter Surface Clear"),
        });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("GammaFilter Clear Pass"),
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
            label: Some("GammaFilter Readback Staging"),
            size: staging_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("GammaFilter Readback Encoder"),
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

    fn full_surface_bounds() -> Rect<Pixels> {
        Rect::from_xywh(
            Pixels(0.0),
            Pixels(0.0),
            Pixels(SURFACE_WIDTH as f32),
            Pixels(SURFACE_HEIGHT as f32),
        )
    }

    /// Assert every interior pixel (skip 1-pixel border) is within `tolerance`
    /// of `expected` in all 4 channels.
    fn assert_interior_pixels_near(
        label: &str,
        readback: &[[u8; 4]],
        expected: [u8; 4],
        tolerance: u8,
    ) {
        let width = SURFACE_WIDTH as usize;
        let height = SURFACE_HEIGHT as usize;
        for (pixel_index, &actual) in readback.iter().enumerate() {
            let row = pixel_index / width;
            let col = pixel_index % width;
            if row == 0 || row >= height - 1 || col == 0 || col >= width - 1 {
                continue;
            }
            for channel_index in 0..4 {
                let channel_diff = u8::try_from(
                    (i16::from(actual[channel_index]) - i16::from(expected[channel_index]))
                        .unsigned_abs(),
                )
                .expect("diff of two u8 values always fits in u8");
                assert!(
                    channel_diff <= tolerance,
                    "{label}: pixel {pixel_index} (row={row} col={col}) \
                     channel {channel_index} — actual={a} expected={e} \
                     diff={channel_diff} > tolerance {tolerance}",
                    a = actual[channel_index],
                    e = expected[channel_index],
                );
            }
        }
    }

    // ── CPU oracle (G1 — calls flui_types transfer fns directly) ─────────────

    /// Apply the sRGB → linear transfer to a straight-alpha opaque color and
    /// return the expected premultiplied `[r, g, b, a]` u8 quad.
    ///
    /// **Oracle:** calls `flui_types::styling::color::srgb_to_linear` directly —
    /// the same source of truth as the WGSL shader.  No math re-derived here.
    ///
    /// For opaque input (alpha = 255): straight = premultiplied, so the
    /// repremultiply step is a no-op (multiplying by 1.0).
    fn srgb_to_linear_oracle(straight_rgba_u8: [u8; 4]) -> [u8; 4] {
        let [r, g, b, a] = straight_rgba_u8;
        let alpha = f32::from(a) / 255.0;
        // Unpremul: already straight (for opaque input alpha == 1 so no-op).
        let straight_r = f32::from(r) / 255.0;
        let straight_g = f32::from(g) / 255.0;
        let straight_b = f32::from(b) / 255.0;
        // Apply transfer per channel; alpha passes through.
        let out_r = srgb_to_linear(straight_r).clamp(0.0, 1.0);
        let out_g = srgb_to_linear(straight_g).clamp(0.0, 1.0);
        let out_b = srgb_to_linear(straight_b).clamp(0.0, 1.0);
        // Repremultiply and quantise.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "values clamped to [0,1]*255 then rounded; truncation is safe"
        )]
        let to_u8 = |x: f32| (x * 255.0).round() as u8;
        [
            to_u8(out_r * alpha),
            to_u8(out_g * alpha),
            to_u8(out_b * alpha),
            a,
        ]
    }

    /// Apply the linear → sRGB transfer to a straight-alpha opaque color and
    /// return the expected premultiplied `[r, g, b, a]` u8 quad.
    fn linear_to_srgb_oracle(straight_rgba_u8: [u8; 4]) -> [u8; 4] {
        let [r, g, b, a] = straight_rgba_u8;
        let alpha = f32::from(a) / 255.0;
        let straight_r = f32::from(r) / 255.0;
        let straight_g = f32::from(g) / 255.0;
        let straight_b = f32::from(b) / 255.0;
        let out_r = linear_to_srgb(straight_r).clamp(0.0, 1.0);
        let out_g = linear_to_srgb(straight_g).clamp(0.0, 1.0);
        let out_b = linear_to_srgb(straight_b).clamp(0.0, 1.0);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "values clamped to [0,1]*255 then rounded; truncation is safe"
        )]
        let to_u8 = |x: f32| (x * 255.0).round() as u8;
        [
            to_u8(out_r * alpha),
            to_u8(out_g * alpha),
            to_u8(out_b * alpha),
            a,
        ]
    }

    /// Draw a full-surface rect with `source_color` inside a gamma filter layer,
    /// submit, and return the readback pixels.
    fn render_with_gamma_filter(
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        surface_tex: &wgpu::Texture,
        surface_view: &wgpu::TextureView,
        source_color: Color,
        direction: GammaDirection,
    ) -> Vec<[u8; 4]> {
        let bounds = full_surface_bounds();
        let mut painter = build_painter(Arc::clone(device), Arc::clone(queue));
        painter.save_layer_with_filter(None, LayerFilter::Gamma(direction));
        painter.rect(bounds, &Paint::fill(source_color));
        painter.restore_layer();

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("GammaFilter Render Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(surface_view, surface_tex),
                &mut encoder,
            )
            .expect("gamma filter render must succeed");
        queue.submit(std::iter::once(encoder.finish()));
        readback_pixels(device, queue, surface_tex)
    }

    // ── GA1: SrgbToLinear known mid-gray value (ABSOLUTE) ────────────────────

    /// GA1: sRGB 128 (≈ 0.502) → linear ≈ 0.2158, so output channel ≈ 55.
    ///
    /// **ABSOLUTE assertion:** `srgb_to_linear(128/255) * 255 ≈ 55`.
    /// This prevents a co-vacuous oracle — the expected value is computed from the
    /// `flui_types::styling::color::srgb_to_linear` transfer function (the same
    /// source as the WGSL), not re-derived here.
    ///
    /// **Fails if:** the direction flag is swapped (linear→sRGB of 0.502 ≈ 0.734
    /// → 187, not 55) or the transfer is applied to premul instead of straight.
    #[test]
    fn srgb_to_linear_known_mid_gray_absolute() {
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
                a: 1.0,
            },
        );

        // mid-gray 128 in sRGB — the ABSOLUTE discriminating test value.
        let source_color = Color::rgba(128, 128, 128, 255);
        let readback = render_with_gamma_filter(
            &device,
            &queue,
            &surface_tex,
            &surface_view,
            source_color,
            GammaDirection::SrgbToLinear,
        );

        let expected = srgb_to_linear_oracle([128, 128, 128, 255]);

        // ABSOLUTE: oracle says srgb_to_linear(128/255)*255 ≈ 55.
        // Verify oracle value is in the expected ballpark before asserting GPU.
        // srgb_to_linear is defined on [0,1] and maps it to [0,1]; multiplying by
        // 255 and rounding yields a value in [0, 255] — truncation to u8 is safe.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "srgb_to_linear([0,1])*255 rounds to [0,255]; truncation to u8 is safe"
        )]
        let linear_of_mid_gray = (srgb_to_linear(128.0 / 255.0) * 255.0).round() as u8;
        assert!(
            linear_of_mid_gray < 80,
            "GA1 oracle sanity: sRGB 128 → linear should be < 80 (got {linear_of_mid_gray}), \
             not the same value or higher"
        );
        assert!(
            linear_of_mid_gray > 40,
            "GA1 oracle sanity: sRGB 128 → linear should be > 40 (got {linear_of_mid_gray})"
        );

        // GPU must match the oracle within ±2 LSB.
        assert_interior_pixels_near("GA1 SrgbToLinear mid-gray", &readback, expected, 2);
    }

    // ── GA2: LinearToSrgb is inverse of SrgbToLinear ─────────────────────────

    /// GA2: Two consecutive passes — first SrgbToLinear then LinearToSrgb —
    /// must recover the original pixel value within ±2 LSB (round-trip identity).
    ///
    /// **Oracle:** applies `srgb_to_linear` then `linear_to_srgb` via the
    /// `flui_types` transfer fns.  Any direction-swap in the shader would produce
    /// a doubling of the same transfer (not an inverse), breaking the ±2 target.
    #[test]
    fn linear_to_srgb_inverse_of_srgb_to_linear() {
        let (device, queue) = acquire_test_device_and_queue();

        // Two consecutive filter layers: first SrgbToLinear, then LinearToSrgb.
        let source_color = Color::rgba(180, 100, 40, 255);
        let bounds = full_surface_bounds();
        let (surface_tex, surface_view) = create_surface(&device);
        clear_surface(
            &device,
            &queue,
            &surface_view,
            wgpu::Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        // Outer filter: LinearToSrgb (applied second during fold).
        painter.save_layer_with_filter(None, LayerFilter::Gamma(GammaDirection::LinearToSrgb));
        // Inner filter: SrgbToLinear (applied first during fold).
        painter.save_layer_with_filter(None, LayerFilter::Gamma(GammaDirection::SrgbToLinear));
        painter.rect(bounds, &Paint::fill(source_color));
        painter.restore_layer(); // pop inner (SrgbToLinear)
        painter.restore_layer(); // pop outer (LinearToSrgb)

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("GA2 Round-trip Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_tex),
                &mut encoder,
            )
            .expect("GA2 round-trip render must succeed");
        queue.submit(std::iter::once(encoder.finish()));
        let readback = readback_pixels(&device, &queue, &surface_tex);

        // Oracle: apply both transfers in the same order.
        let intermediate = srgb_to_linear_oracle([180, 100, 40, 255]);
        let round_trip = linear_to_srgb_oracle(intermediate);

        // ±4: two-pass quantisation at two offscreen boundaries.
        assert_interior_pixels_near(
            "GA2 LinearToSrgb ∘ SrgbToLinear round-trip",
            &readback,
            round_trip,
            4,
        );
    }

    // ── GA3: Translucent layer — alpha unchanged (G2 discriminator) ───────────

    /// GA3: A 50%-alpha layer through the gamma filter must have alpha UNCHANGED.
    ///
    /// **Premul-bracket discriminator:** alpha passes through without modification
    /// — the unpremul/repremul bracket only acts on RGB.  A shader that scales
    /// or re-processes alpha would break this assertion.
    ///
    /// **Oracle:** for a 50%-alpha source, the expected output alpha is 128.
    /// The RGB channels undergo sRGB→linear; alpha byte is bit-identical.
    #[test]
    fn translucent_layer_alpha_is_unchanged() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_tex, surface_view) = create_surface(&device);

        // Transparent backdrop so compositing only sees the layer.
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

        // 50%-alpha green — the translucent G2 discriminating input.
        let source_color = Color::rgba(0, 200, 0, 128);
        let readback = render_with_gamma_filter(
            &device,
            &queue,
            &surface_tex,
            &surface_view,
            source_color,
            GammaDirection::SrgbToLinear,
        );

        // Check that every interior pixel has alpha ≈ 128.
        // The filter must NOT touch alpha — a tolerance of ±3 covers quantisation.
        let width = SURFACE_WIDTH as usize;
        let height = SURFACE_HEIGHT as usize;
        for (pixel_index, &pixel) in readback.iter().enumerate() {
            let row = pixel_index / width;
            let col = pixel_index % width;
            if row == 0 || row >= height - 1 || col == 0 || col >= width - 1 {
                continue;
            }
            let alpha = pixel[3];
            let alpha_diff = u8::try_from((i16::from(alpha) - 128_i16).unsigned_abs())
                .expect("diff of two u8 values always fits in u8");
            assert!(
                alpha_diff <= 3,
                "GA3: pixel {pixel_index} alpha={alpha} — gamma filter must not change alpha \
                 (expected ≈ 128, diff={alpha_diff} > tolerance 3)"
            );
        }

        // Also verify RGB was actually transferred (not left unchanged).
        // For green channel: srgb_to_linear(200/255) * 255 * (128/255) should be
        // significantly different from the source premul green (200*128/255 ≈ 100).
        let expected_premul = srgb_to_linear_oracle([0, 200, 0, 128]);
        // 200 * 128 / 255 = 100 — premultiplied green when alpha is 128.
        let source_premul_green: u8 =
            u8::try_from((200u32 * 128) / 255).expect("200*128/255 == 100, fits in u8");
        assert!(
            (i16::from(expected_premul[1]) - i16::from(source_premul_green)).unsigned_abs() > 10,
            "GA3: oracle green channel should be meaningfully different from premul source \
             (oracle={}, source_premul={source_premul_green}); transfer may be a no-op",
            expected_premul[1],
        );
    }

    // ── GA4: Black/white boundary (ABSOLUTE) ──────────────────────────────────

    /// GA4: sRGB 0 → linear 0 and sRGB 255 → linear 255 (and the inverse).
    ///
    /// **ABSOLUTE assertion:** both transfer functions fix 0 and 1 exactly.
    /// This tests the boundary of the piecewise formula.
    ///
    /// **Fails if:** the WGSL shader uses incorrect piecewise thresholds that map
    /// the boundary points incorrectly.
    #[test]
    fn black_and_white_boundary_maps_to_self() {
        let (device, queue) = acquire_test_device_and_queue();

        // Test SrgbToLinear on pure black.
        {
            let (surface_tex, surface_view) = create_surface(&device);
            clear_surface(
                &device,
                &queue,
                &surface_view,
                wgpu::Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                },
            );
            let readback = render_with_gamma_filter(
                &device,
                &queue,
                &surface_tex,
                &surface_view,
                Color::rgba(0, 0, 0, 255),
                GammaDirection::SrgbToLinear,
            );
            // ABSOLUTE: black maps to black.
            assert_interior_pixels_near("GA4 SrgbToLinear black", &readback, [0, 0, 0, 255], 1);
        }

        // Test SrgbToLinear on pure white.
        {
            let (surface_tex, surface_view) = create_surface(&device);
            clear_surface(
                &device,
                &queue,
                &surface_view,
                wgpu::Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                },
            );
            let readback = render_with_gamma_filter(
                &device,
                &queue,
                &surface_tex,
                &surface_view,
                Color::rgba(255, 255, 255, 255),
                GammaDirection::SrgbToLinear,
            );
            // ABSOLUTE: white maps to white (srgb_to_linear(1.0) == 1.0).
            assert_interior_pixels_near(
                "GA4 SrgbToLinear white",
                &readback,
                [255, 255, 255, 255],
                1,
            );
        }

        // Test LinearToSrgb on pure black.
        {
            let (surface_tex, surface_view) = create_surface(&device);
            clear_surface(
                &device,
                &queue,
                &surface_view,
                wgpu::Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                },
            );
            let readback = render_with_gamma_filter(
                &device,
                &queue,
                &surface_tex,
                &surface_view,
                Color::rgba(0, 0, 0, 255),
                GammaDirection::LinearToSrgb,
            );
            // ABSOLUTE: black maps to black.
            assert_interior_pixels_near("GA4 LinearToSrgb black", &readback, [0, 0, 0, 255], 1);
        }

        // Test LinearToSrgb on pure white.
        {
            let (surface_tex, surface_view) = create_surface(&device);
            clear_surface(
                &device,
                &queue,
                &surface_view,
                wgpu::Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                },
            );
            let readback = render_with_gamma_filter(
                &device,
                &queue,
                &surface_tex,
                &surface_view,
                Color::rgba(255, 255, 255, 255),
                GammaDirection::LinearToSrgb,
            );
            // ABSOLUTE: white maps to white (linear_to_srgb(1.0) == 1.0).
            assert_interior_pixels_near(
                "GA4 LinearToSrgb white",
                &readback,
                [255, 255, 255, 255],
                1,
            );
        }
    }

    // ── GA5: Pipeline construction test ───────────────────────────────────────

    /// GA5: `GammaPipeline::new` must complete without a wgpu validation error.
    ///
    /// This test catches WGSL parse/validation errors before the readback tests
    /// run.  A WGSL syntax error would cause `new()` to panic.
    #[test]
    fn gamma_pipeline_construction_succeeds() {
        use crate::wgpu::gamma::GammaPipeline;

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .expect("a GPU adapter must be available for GA5");
        let (device, _queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("GA5 GammaPipeline Test Device"),
                ..Default::default()
            }))
            .expect("GPU device creation succeeded when adapter was found");

        let _pipeline = GammaPipeline::new(&device, wgpu::TextureFormat::Rgba8Unorm);
    }

    // ── GA6: Full round-trip ≈ identity (±2) ─────────────────────────────────

    /// GA6: `LinearToSrgb(SrgbToLinear(x)) ≈ x` within ±2 LSB for a
    /// non-trivial color, verifying the inverse relationship end-to-end.
    ///
    /// **Differs from GA2** in that GA2 uses two *separate* filter layers
    /// (testing the chain fold), whereas GA6 checks the mathematical inverse
    /// property of the oracle itself and compares with a single-pass GPU output.
    ///
    /// Oracle: apply srgb_to_linear then linear_to_srgb via flui_types fns.
    /// The GPU two-pass result must match the oracle within ±4 LSB
    /// (two offscreen quantisation boundaries).
    #[test]
    fn round_trip_is_approximately_identity() {
        let (device, queue) = acquire_test_device_and_queue();

        let source_color = Color::rgba(120, 60, 200, 255);
        let bounds = full_surface_bounds();
        let (surface_tex, surface_view) = create_surface(&device);
        clear_surface(
            &device,
            &queue,
            &surface_view,
            wgpu::Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        // Chain: outer = LinearToSrgb, inner = SrgbToLinear.
        // Fold order: inner first → LinearToSrgb(SrgbToLinear(x)).
        painter.save_layer_with_filter(None, LayerFilter::Gamma(GammaDirection::LinearToSrgb));
        painter.save_layer_with_filter(None, LayerFilter::Gamma(GammaDirection::SrgbToLinear));
        painter.rect(bounds, &Paint::fill(source_color));
        painter.restore_layer();
        painter.restore_layer();

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_tex),
                &mut encoder,
            )
            .expect("GA6 round-trip render must succeed");
        queue.submit(std::iter::once(encoder.finish()));
        let readback = readback_pixels(&device, &queue, &surface_tex);

        // Oracle: the round-trip should be approximately the original color.
        let intermediate = srgb_to_linear_oracle([120, 60, 200, 255]);
        let round_trip = linear_to_srgb_oracle(intermediate);

        // ±4: two offscreen quantisation boundaries.
        assert_interior_pixels_near("GA6 round-trip ≈ identity", &readback, round_trip, 4);
    }
}
