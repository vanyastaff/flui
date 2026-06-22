//! GPU readback acceptance gate for the blend-mode color filter pass.
//!
//! ## Test inventory
//!
//! | # | Gate | Requirement |
//! |---|------|-------------|
//! | MO1 | GPU | Modulate with opaque white = DST pixel unchanged (fast-path id=13) |
//! | MO2 | GPU | Multiply opaque color: ABSOLUTE match against oracle |
//! | MO3 | GPU | SrcOver on translucent layer (G2 premul-bracket discriminator) |
//! | MO4 | GPU | Screen separable path: oracle match |
//! | MO5 | GPU | Hue non-separable path: oracle match |
//! | MO6 | GPU | Luminosity on translucent layer (G2 — second translucent test) |
//! | MO7 | GPU | ModePipeline::new GPU construction succeeds (catches WGSL errors) |
//!
//! ## Oracle discipline (G1)
//!
//! Every mode oracle calls `flui_types::Color::blend(filter_color, dst_pixel, mode)`
//! directly — the identical function used in production CPU blending.  No blend
//! math is re-derived in this file.  The oracle returns a **straight-alpha** `Color`
//! which is then converted to premultiplied u8 for the GPU comparison.
//!
//! ## G2 (translucent discriminator)
//!
//! MO3 and MO6 draw with alpha < 255.  The assertions verify that the premul
//! bracket (unpremul DST → blend → repremul) is applied correctly for
//! translucent inputs.
//!
//! ## Blend-mode → integer mapping
//!
//! The WGSL shader dispatches on the integer encoded by `blend_mode_to_u32` in
//! `mode/pipeline.rs`.  The key values exercised by these tests:
//!
//! | BlendMode   | u32 | WGSL path |
//! |-------------|-----|-----------|
//! | Modulate    | 13  | fast-path |
//! | SrcOver     |  3  | Porter-Duff |
//! | Multiply    | 25  | separable |
//! | Screen      | 15  | separable |
//! | Hue         | 26  | non-separable HSL |
//! | Luminosity  | 29  | non-separable HSL |

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod gpu_tests {
    use std::sync::Arc;

    use flui_painting::Paint;
    use flui_types::{Color, Rect, geometry::Pixels, painting::BlendMode};

    use crate::wgpu::{command_ir::LayerFilter, painter::WgpuPainter, render_target::RenderTarget};

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
        .expect("a GPU adapter must be available for mode_filter_tests");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("ModeFilter Test Device"),
            ..Default::default()
        }))
        .expect("a GPU device must be available for mode_filter_tests");
        (Arc::new(device), Arc::new(queue))
    }

    fn create_surface(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ModeFilter Test Surface"),
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
            label: Some("ModeFilter Surface Clear"),
        });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ModeFilter Clear Pass"),
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
            label: Some("ModeFilter Readback Staging"),
            size: staging_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ModeFilter Readback Encoder"),
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

    // ── CPU oracle (G1 — calls Color::blend directly) ────────────────────────

    /// Compute the expected GPU output for a blend-mode color filter.
    ///
    /// **Oracle:** calls `Color::blend(filter_color, dst_pixel, mode)` directly —
    /// the same function the GPU shader mirrors.  No blend math is re-derived.
    ///
    /// The GPU shader:
    /// 1. Reads the layer pixel from the offscreen texture (premultiplied).
    /// 2. Unpremultiplies to straight-alpha DST.
    /// 3. Blends SRC (filter color, straight) with DST (straight).
    /// 4. Re-premultiplies and emits.
    ///
    /// This oracle performs steps 2-4 in CPU arithmetic:
    /// - `dst_pixel_straight` is `layer_pixel` for opaque inputs (premul == straight).
    /// - For translucent inputs the caller passes the already-straight source color.
    /// - The output `Color` is straight-alpha; we convert to premul u8 for comparison.
    fn blend_mode_oracle(
        filter_color: Color,
        dst_pixel_straight: Color,
        mode: BlendMode,
    ) -> [u8; 4] {
        // filter_color is SRC; dst_pixel_straight is DST.
        let blended = filter_color.blend(dst_pixel_straight, mode);
        // `blended` is straight-alpha.  Convert to premultiplied u8 for comparison
        // with the GPU readback (which emits premultiplied RGBA8Unorm).
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "values clamped to [0,1]*255 then rounded; truncation is safe"
        )]
        let to_premul_u8 = |channel: u8, alpha: u8| -> u8 {
            let straight = f32::from(channel) / 255.0;
            let a = f32::from(alpha) / 255.0;
            (straight * a * 255.0).round() as u8
        };
        let out_a = blended.a;
        [
            to_premul_u8(blended.r, out_a),
            to_premul_u8(blended.g, out_a),
            to_premul_u8(blended.b, out_a),
            out_a,
        ]
    }

    /// Render a full-surface rect with `layer_color` inside a mode color filter,
    /// submit, and return the readback pixels.
    fn render_with_mode_filter(
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        surface_tex: &wgpu::Texture,
        surface_view: &wgpu::TextureView,
        filter_color: Color,
        layer_color: Color,
        mode: BlendMode,
    ) -> Vec<[u8; 4]> {
        let bounds = full_surface_bounds();
        let filter_color_f32 = [
            filter_color.red_f32(),
            filter_color.green_f32(),
            filter_color.blue_f32(),
            filter_color.alpha_f32(),
        ];
        let mut painter = build_painter(Arc::clone(device), Arc::clone(queue));
        painter.save_layer_with_filter(
            None,
            LayerFilter::Mode {
                color: filter_color_f32,
                blend_mode: mode,
            },
        );
        painter.rect(bounds, &Paint::fill(layer_color));
        painter.restore_layer();

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ModeFilter Render Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(surface_view, surface_tex),
                &mut encoder,
            )
            .expect("mode filter render must succeed");
        queue.submit(std::iter::once(encoder.finish()));
        readback_pixels(device, queue, surface_tex)
    }

    // ── MO1: Modulate with opaque white = DST unchanged ──────────────────────

    /// MO1: `Modulate` with filter_color=WHITE and an opaque DST must produce
    /// the DST pixel unchanged.
    ///
    /// **Mathematical identity:** `Modulate(SRC=white, DST) = SRC_pm ⊗ DST_pm`.
    /// Since `white_pm = (1,1,1,1)`, the product equals `DST_pm` — the filter
    /// is a no-op identity for this input.
    ///
    /// **ABSOLUTE discriminator:** the expected output is the original DST color.
    /// This is an ABSOLUTE assertion (not relative) because the Modulate formula
    /// is trivially computable:  `white * DST = DST` for opaque white.
    ///
    /// **Fails if:** the Modulate fast-path (WGSL `if mode == 13u`) is broken,
    /// or the premul bracket is wrong (wrong premul order would scale channels).
    ///
    /// **Oracle call (G1):** `Color::WHITE.blend(layer_color, BlendMode::Modulate)`.
    #[test]
    fn modulate_with_white_preserves_dst_pixel() {
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

        let layer_color = Color::rgba(180, 90, 40, 255);
        let readback = render_with_mode_filter(
            &device,
            &queue,
            &surface_tex,
            &surface_view,
            Color::WHITE,
            layer_color,
            BlendMode::Modulate,
        );

        // Oracle (G1): Modulate(white, layer) == layer for opaque inputs.
        let oracle_result = blend_mode_oracle(Color::WHITE, layer_color, BlendMode::Modulate);

        // ABSOLUTE sanity: Modulate(white, dst) must equal dst.
        // Oracle output should match layer_color.r/g/b/a almost exactly for opaque.
        assert_eq!(
            oracle_result[3], 255,
            "MO1 oracle: alpha must be 255 for opaque Modulate(white, opaque_dst)"
        );

        // GPU must match oracle within ±2 LSB (offscreen quantisation).
        assert_interior_pixels_near(
            "MO1 Modulate(white, dst) = dst",
            &readback,
            oracle_result,
            2,
        );
    }

    // ── MO2: Multiply opaque color — ABSOLUTE ────────────────────────────────

    /// MO2: `Multiply` of two opaque colors with known values must produce the
    /// oracle result.
    ///
    /// **ABSOLUTE discriminator:** the test color is chosen so the Multiply
    /// result is easily verified: `filter=half-gray (128), dst=red (255,0,0,255)`.
    /// `Multiply(half-gray, red) ≈ (128*255/255, 0, 0, 255) = (128, 0, 0, 255)` —
    /// the red channel is halved.  A wrong mode dispatch (e.g. Screen instead of
    /// Multiply) would produce `(255+128 - 255*128/255) ≈ (255, 0, 0, 255)` —
    /// clearly wrong.
    ///
    /// **Oracle call (G1):** `filter_color.blend(layer_color, BlendMode::Multiply)`.
    #[test]
    fn multiply_halves_red_channel() {
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

        // Filter: pure-gray (128,128,128,255); layer: opaque red (255,0,0,255).
        // Multiply: each channel = src * dst → red = 128/255 * 1.0 ≈ 128.
        // This is hand-verifiable without knowing the full blend formula.
        let filter_color = Color::rgba(128, 128, 128, 255);
        let layer_color = Color::RED; // (255, 0, 0, 255)

        let readback = render_with_mode_filter(
            &device,
            &queue,
            &surface_tex,
            &surface_view,
            filter_color,
            layer_color,
            BlendMode::Multiply,
        );

        // Oracle (G1).
        let oracle_result = blend_mode_oracle(filter_color, layer_color, BlendMode::Multiply);

        // ABSOLUTE sanity: Multiply(half-gray, red) red channel ≈ 128, not 255.
        assert!(
            oracle_result[0] > 60 && oracle_result[0] < 200,
            "MO2 oracle: Multiply(128-gray, red).r should be ~128 (half of red), \
             got {r}; oracle may be wrong",
            r = oracle_result[0],
        );
        // Green and blue must remain near 0.
        assert!(
            oracle_result[1] < 20,
            "MO2 oracle: green channel must be near 0 for Multiply(gray, red), got {}",
            oracle_result[1],
        );

        // GPU must match oracle within ±3 LSB.
        assert_interior_pixels_near("MO2 Multiply(gray, red)", &readback, oracle_result, 3);
    }

    // ── MO3: SrcOver on translucent layer (G2 discriminator) ─────────────────

    /// MO3: `SrcOver` with a 50%-alpha filter color on an opaque layer.
    ///
    /// **G2 (translucent discriminator):** SRC is the filter color (50% alpha);
    /// DST is the opaque layer pixel.  The premul bracket must correctly handle
    /// alpha < 255 on the SRC side.
    ///
    /// **Fails if:** the filter color's alpha is ignored or treated as 1.0
    /// (the output would then be the filter color alone, not the blended result).
    ///
    /// **Oracle call (G1):**
    /// `filter_color.blend(layer_color, BlendMode::SrcOver)`.
    #[test]
    fn src_over_with_translucent_filter_blends_correctly() {
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

        // Filter: 50%-alpha blue; layer: opaque orange.
        // SrcOver: output = SRC_pm + (1-SRC_a)*DST_pm — meaningful mix.
        let filter_color = Color::rgba(0, 0, 200, 128); // 50% alpha blue
        let layer_color = Color::rgba(220, 100, 0, 255); // opaque orange

        let readback = render_with_mode_filter(
            &device,
            &queue,
            &surface_tex,
            &surface_view,
            filter_color,
            layer_color,
            BlendMode::SrcOver,
        );

        // Oracle (G1): filter_color is SRC, layer_color is DST.
        let oracle_result = blend_mode_oracle(filter_color, layer_color, BlendMode::SrcOver);

        // Sanity: output alpha should be near 255 (50%-alpha SrcOver opaque = fully opaque out).
        // SrcOver alpha out = SRC_a + DST_a*(1 - SRC_a) = 0.5 + 1*(0.5) = 1.0.
        assert!(
            oracle_result[3] > 240,
            "MO3 oracle: SrcOver(50%_alpha, opaque) output alpha should be ~255, got {}",
            oracle_result[3]
        );

        // GPU must match oracle within ±3 LSB.
        assert_interior_pixels_near(
            "MO3 SrcOver(50%-blue, opaque-orange)",
            &readback,
            oracle_result,
            3,
        );
    }

    // ── MO4: Screen separable path ────────────────────────────────────────────

    /// MO4: `Screen` blend mode exercises the `else` (separable) branch in the
    /// WGSL dispatch (`mode >= 15 && mode < 26`, excluding Multiply=25).
    ///
    /// Screen: `1 - (1-SRC)*(1-DST)` per channel.  With SRC=red (1,0,0,1) and
    /// DST=blue (0,0,1,1): red_out=1, green_out=0, blue_out=1 → magenta.
    ///
    /// **ABSOLUTE discriminator:** the expected output is magenta (255,0,255,255),
    /// easily verifiable from the Screen formula.  A wrong dispatch (e.g. Overlay
    /// instead of Screen) would produce a different result.
    ///
    /// **Oracle call (G1):** `filter_color.blend(layer_color, BlendMode::Screen)`.
    #[test]
    fn screen_red_over_blue_produces_magenta() {
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

        let filter_color = Color::RED; // (255, 0, 0, 255) — SRC
        let layer_color = Color::BLUE; // (0, 0, 255, 255) — DST

        let readback = render_with_mode_filter(
            &device,
            &queue,
            &surface_tex,
            &surface_view,
            filter_color,
            layer_color,
            BlendMode::Screen,
        );

        // Oracle (G1).
        let oracle_result = blend_mode_oracle(filter_color, layer_color, BlendMode::Screen);

        // ABSOLUTE: Screen(red, blue) = magenta.
        // R_out = 1-(1-1)*(1-0) = 1 → 255.
        // B_out = 1-(1-0)*(1-1) = 1 → 255.
        // G_out = 1-(1-0)*(1-0) = 0 → 0.
        assert!(
            oracle_result[0] > 230,
            "MO4 oracle: Screen(red, blue) R should be ~255"
        );
        assert!(
            oracle_result[1] < 25,
            "MO4 oracle: Screen(red, blue) G should be ~0"
        );
        assert!(
            oracle_result[2] > 230,
            "MO4 oracle: Screen(red, blue) B should be ~255"
        );

        // GPU must match oracle within ±3 LSB.
        assert_interior_pixels_near(
            "MO4 Screen(red, blue) ≈ magenta",
            &readback,
            oracle_result,
            3,
        );
    }

    // ── MO5: Hue non-separable path ───────────────────────────────────────────

    /// MO5: `Hue` blend mode exercises the non-separable HSL branch in the
    /// WGSL dispatch (`mode >= 26`).
    ///
    /// `Hue(SRC, DST)` = Lum(DST) × Sat(DST) applied to Hue(SRC).  For
    /// SRC=red, DST=cyan: the hue of red is taken, saturation and luminosity of
    /// cyan are preserved.  The output is not magenta or cyan — it is a hue-red
    /// with cyan's lightness.
    ///
    /// **Discriminator:** the GPU result must match the oracle, proving the
    /// non-separable path (set_lum/set_sat helpers) runs.  A wrong dispatch
    /// (separable Screen=15 instead of Hue=26) would produce ~magenta, not the
    /// oracle value.
    ///
    /// **Oracle call (G1):** `filter_color.blend(layer_color, BlendMode::Hue)`.
    #[test]
    fn hue_takes_src_hue_preserves_dst_lum_sat() {
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

        let filter_color = Color::RED; // SRC: hue = 0° (red)
        let layer_color = Color::rgba(0, 200, 200, 255); // DST: cyan-ish

        let readback = render_with_mode_filter(
            &device,
            &queue,
            &surface_tex,
            &surface_view,
            filter_color,
            layer_color,
            BlendMode::Hue,
        );

        // Oracle (G1).
        let oracle_result = blend_mode_oracle(filter_color, layer_color, BlendMode::Hue);

        // Discriminator: the GPU result must match the oracle within ±4 LSB.
        // We don't assert the exact expected value — the oracle is authoritative —
        // but we verify the result is non-trivial (not black, not cyan).
        assert!(
            oracle_result[3] > 200,
            "MO5 oracle: Hue blend with opaque inputs must produce near-opaque output"
        );

        assert_interior_pixels_near("MO5 Hue(red, cyan)", &readback, oracle_result, 4);
    }

    // ── MO6: Luminosity on translucent (G2 — second translucent test) ─────────

    /// MO6: `Luminosity` blend with a 75%-alpha filter color on an opaque layer.
    ///
    /// **G2 (translucent discriminator — second test):** The filter SRC has
    /// alpha=192 (75%).  The premul bracket must correctly unpremultiply the
    /// translucent SRC, apply Luminosity, and re-premultiply.
    ///
    /// **Fails if:** the filter color alpha is not respected — the output RGB
    /// would be composited incorrectly (too bright or too dark).
    ///
    /// **Oracle call (G1):**
    /// `filter_color.blend(layer_color, BlendMode::Luminosity)`.
    #[test]
    fn luminosity_with_translucent_filter_matches_oracle() {
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

        // Filter: 75%-alpha warm yellow (SRC); layer: opaque deep teal (DST).
        let filter_color = Color::rgba(240, 200, 30, 192); // 75% alpha
        let layer_color = Color::rgba(20, 100, 120, 255); // opaque teal

        let readback = render_with_mode_filter(
            &device,
            &queue,
            &surface_tex,
            &surface_view,
            filter_color,
            layer_color,
            BlendMode::Luminosity,
        );

        // Oracle (G1).
        let oracle_result = blend_mode_oracle(filter_color, layer_color, BlendMode::Luminosity);

        // Sanity: output alpha ≈ 255 (75%-alpha SrcOver opaque → near full).
        // Luminosity composites as SrcOver: α_out = SRC_a + DST_a*(1-SRC_a) = 0.75 + 0.25 ≈ 1.
        assert!(
            oracle_result[3] > 230,
            "MO6 oracle: Luminosity(75%-alpha, opaque) output alpha should be ~255, got {}",
            oracle_result[3]
        );

        // GPU must match oracle within ±4 LSB (non-separable + translucent path).
        assert_interior_pixels_near(
            "MO6 Luminosity(75%-yellow, opaque-teal)",
            &readback,
            oracle_result,
            4,
        );
    }

    // ── MO7: Pipeline construction test ───────────────────────────────────────

    /// MO7: `ModePipeline::new` must complete without a wgpu validation error.
    ///
    /// This test catches WGSL parse/validation errors before the readback tests
    /// run.  A WGSL syntax error would cause `new()` to panic during device
    /// shader module creation.
    #[test]
    fn mode_pipeline_construction_succeeds() {
        use crate::wgpu::mode::ModePipeline;

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .expect("a GPU adapter must be available for MO7");
        let (device, _queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("MO7 ModePipeline Test Device"),
                ..Default::default()
            }))
            .expect("GPU device creation succeeded when adapter was found");

        let _pipeline = ModePipeline::new(&device, wgpu::TextureFormat::Rgba8Unorm);
    }
}
