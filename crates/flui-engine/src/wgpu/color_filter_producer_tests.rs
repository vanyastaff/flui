//! Producer-path GPU readback acceptance gate for T1 of `gpu-filters-consumer-chain`.
//!
//! ## Purpose
//!
//! These tests prove that `ColorFilter::Mode{...}` and `ColorFilter::LinearToSrgbGamma`
//! are correctly routed through the **layer producer path**:
//!
//! ```text
//! ColorFilterLayer::render
//!   → Backend::push_color_filter(&ColorFilter::Mode/LinearToSrgbGamma/...)
//!   → WgpuPainter::save_layer_with_filter(LayerFilter::Mode{...} / LayerFilter::Gamma(...))
//!   → GPU mode/gamma filter shader
//! ```
//!
//! Before T1, `Backend::push_color_filter` only accepted `&ColorMatrix`; the
//! `LayerFilter::Mode` and `LayerFilter::Gamma` variants had no production caller
//! (only `#[cfg(test)]`-gated uses).  These tests would have **failed to compile**
//! on `main` because the old trait signature did not accept `&ColorFilter`.
//! They turn RED→GREEN exactly at T1.
//!
//! ## Test inventory
//!
//! | # | Gate | Requirement |
//! |---|------|-------------|
//! | P1 | GPU | Mode filter via producer path: oracle match (Multiply) |
//! | P2 | GPU | LinearToSrgbGamma via producer path: oracle match |
//! | P3 | GPU | SrgbToLinearGamma via producer path: oracle match |
//! | P4 | GPU | Matrix filter via producer path: matches direct-painter path byte-for-byte |
//!
//! ## Oracle discipline
//!
//! P1: calls `Color::blend(filter_color, dst_pixel, mode)` — the same function
//! the mode-filter shader mirrors.
//! P2/P3: applies the standard sRGB/linear transfer formula per channel (alpha unchanged).
//! P4: redundant compared to `color_matrix_filter_tests` but proves the producer path
//! for Matrix too.

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod gpu_tests {
    use std::sync::Arc;

    use flui_painting::Paint;
    use flui_types::{
        Color, Rect,
        geometry::{Matrix4, Pixels},
        painting::{BlendMode, ColorFilter},
    };

    use crate::{
        traits::{CommandRenderer, LayerStateStack},
        wgpu::{Backend, painter::WgpuPainter, render_target::RenderTarget},
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
        .expect("a GPU adapter must be available for color_filter_producer_tests");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("ColorFilterProducer Test Device"),
            ..Default::default()
        }))
        .expect("a GPU device must be available for color_filter_producer_tests");
        (Arc::new(device), Arc::new(queue))
    }

    fn create_surface(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ColorFilterProducer Test Surface"),
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
            label: Some("ColorFilterProducer Surface Clear"),
        });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ColorFilterProducer Clear Pass"),
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
            label: Some("ColorFilterProducer Readback Staging"),
            size: staging_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ColorFilterProducer Readback Encoder"),
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

    /// Assert every interior pixel (skip 1-pixel border to avoid SDF fwidth edge
    /// artefacts) is within `tolerance` of `expected` in all 4 channels.
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
            // Skip the 1-pixel border.
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

    /// Render using the **producer path**: `Backend::push_color_filter(&ColorFilter)`,
    /// draw a rect via `render_rect`, `Backend::pop_color_filter`, submit, readback.
    ///
    /// This is the code path exercised by `ColorFilterLayer::render` in production.
    /// Using `Backend` directly instead of `WgpuPainter::save_layer_with_filter`
    /// is the distinguishing property: it proves `Backend::push_color_filter`
    /// dispatches the right `LayerFilter` variant for each `ColorFilter` arm.
    ///
    /// Draw calls go through `CommandRenderer::render_rect` (which is what real
    /// display-list dispatch does); `WgpuPainter::rect` is a painter-internal
    /// method not exposed on `Backend`.
    fn render_via_producer_path(
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        surface_tex: &wgpu::Texture,
        surface_view: &wgpu::TextureView,
        filter: ColorFilter,
        layer_color: Color,
    ) -> Vec<[u8; 4]> {
        let bounds = full_surface_bounds();
        let painter = build_painter(Arc::clone(device), Arc::clone(queue));
        let mut backend = Backend::new(painter);

        // The same call that `ColorFilterLayer::render` issues (via LayerStateStack).
        backend.push_color_filter(&filter);
        // Draw a full-surface rect via CommandRenderer (the display-list dispatch path).
        backend.render_rect(bounds, &Paint::fill(layer_color), &Matrix4::IDENTITY);
        // The same call that `ColorFilterLayer::cleanup` issues.
        backend.pop_color_filter();

        // Extract the painter and submit.
        let mut painter = backend.into_painter();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ColorFilterProducer Render Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(surface_view, surface_tex),
                &mut encoder,
            )
            .expect("producer-path filter render must succeed");
        queue.submit(std::iter::once(encoder.finish()));
        readback_pixels(device, queue, surface_tex)
    }

    // ── CPU oracle helpers ────────────────────────────────────────────────────

    /// Mode oracle: mirrors the mode-filter GPU shader.
    /// Inputs are straight-alpha; output is premultiplied u8.
    fn mode_filter_oracle(filter_color: Color, dst_straight: Color, mode: BlendMode) -> [u8; 4] {
        // filter_color = SRC; dst_straight = DST (straight-alpha input from layer).
        let blended = filter_color.blend(dst_straight, mode);
        // `blended` is straight-alpha; convert to premultiplied u8.
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

    /// LinearToSrgb oracle: linear → sRGB per channel; alpha unchanged.
    /// Input is straight-alpha opaque; output is premultiplied u8 (== straight for
    /// opaque alpha).
    fn linear_to_srgb_oracle(linear_channel: u8) -> u8 {
        let linear = f32::from(linear_channel) / 255.0;
        let srgb = if linear <= 0.003_130_8 {
            linear * 12.92
        } else {
            1.055 * linear.powf(1.0 / 2.4) - 0.055
        };
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "value clamped to [0,1]*255 then rounded; truncation is safe"
        )]
        let result = (srgb.clamp(0.0, 1.0) * 255.0).round() as u8;
        result
    }

    /// SrgbToLinear oracle: sRGB → linear per channel; alpha unchanged.
    fn srgb_to_linear_oracle(srgb_channel: u8) -> u8 {
        let srgb = f32::from(srgb_channel) / 255.0;
        let linear = if srgb <= 0.04045 {
            srgb / 12.92
        } else {
            ((srgb + 0.055) / 1.055).powf(2.4)
        };
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "value clamped to [0,1]*255 then rounded; truncation is safe"
        )]
        let result = (linear.clamp(0.0, 1.0) * 255.0).round() as u8;
        result
    }

    // ── P1: Mode filter via producer path ─────────────────────────────────────

    /// P1: `ColorFilter::Mode { Multiply, half-opacity red }` dispatched through
    /// `Backend::push_color_filter` produces the same GPU output as the
    /// mode-oracle for the same input color.
    ///
    /// **Proves:**
    /// - `Backend::push_color_filter` now accepts `&ColorFilter` (not just `&ColorMatrix`).
    /// - The `ColorFilter::Mode` arm correctly translates to `LayerFilter::Mode`.
    /// - `color.to_f32_array()` produces the right channel order for the GPU shader.
    ///
    /// **Fails if:** `push_color_filter` ignores Mode (no-op layer), dispatches the
    /// wrong blend mode, or scrambles the color channels.
    ///
    /// **Red-before-green:** on `main`, this test would not compile because the old
    /// trait signature was `&ColorMatrix`, not `&ColorFilter`.
    #[test]
    fn p1_mode_filter_multiply_via_producer_path() {
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

        // Opaque red-green layer pixel; half-opacity blue filter color.
        let layer_color = Color::rgba(180, 120, 40, 255);
        let filter_color = Color::rgba(0, 0, 200, 128);
        let filter = ColorFilter::mode(filter_color, BlendMode::Multiply);

        let readback = render_via_producer_path(
            &device,
            &queue,
            &surface_tex,
            &surface_view,
            filter,
            layer_color,
        );

        // Oracle: mode filter blends filter_color (SRC) onto layer_color (DST).
        // For opaque input the layer pixel IS the straight-alpha value.
        let expected = mode_filter_oracle(filter_color, layer_color, BlendMode::Multiply);

        assert_interior_pixels_near("P1 Mode/Multiply producer path", &readback, expected, 3);
    }

    // ── P2: LinearToSrgbGamma via producer path ───────────────────────────────

    /// P2: `ColorFilter::LinearToSrgbGamma` dispatched through
    /// `Backend::push_color_filter` applies the linear→sRGB transfer per channel.
    ///
    /// **Proves:**
    /// - The `LinearToSrgbGamma` arm correctly translates to
    ///   `LayerFilter::Gamma(GammaDirection::LinearToSrgb)`.
    /// - The gamma shader applies the standard IEC 61966-2-1 transfer.
    ///
    /// **Discriminating:** the chosen `layer_color = rgba(50, 100, 200, 255)` has
    /// mid-range channel values where the nonlinear part of the IEC formula applies.
    /// A no-op (identity) would output `(50, 100, 200)` in straight-alpha space,
    /// while the oracle outputs significantly different values, so a wrong/missing
    /// dispatch is caught.
    ///
    /// **Red-before-green:** on `main`, the old `push_color_filter(&ColorMatrix)`
    /// signature could not accept `&ColorFilter::LinearToSrgbGamma`.
    #[test]
    fn p2_linear_to_srgb_gamma_via_producer_path() {
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

        // Choose a color where linear→sRGB is non-trivially different from identity.
        // At channel value 50/255 ≈ 0.196 (above the 0.003_130_8 threshold):
        //   sRGB = 1.055 * 0.196^(1/2.4) - 0.055 ≈ 0.471, i.e. ~120/255.
        let layer_color = Color::rgba(50, 100, 200, 255);
        let filter = ColorFilter::LinearToSrgbGamma;

        let readback = render_via_producer_path(
            &device,
            &queue,
            &surface_tex,
            &surface_view,
            filter,
            layer_color,
        );

        // GPU output is premultiplied; since alpha=255 (opaque), premul == straight.
        let expected = [
            linear_to_srgb_oracle(50),
            linear_to_srgb_oracle(100),
            linear_to_srgb_oracle(200),
            255,
        ];

        assert_interior_pixels_near("P2 LinearToSrgbGamma producer path", &readback, expected, 3);
    }

    // ── P3: SrgbToLinearGamma via producer path ───────────────────────────────

    /// P3: `ColorFilter::SrgbToLinearGamma` dispatched through
    /// `Backend::push_color_filter` applies the sRGB→linear transfer per channel.
    ///
    /// **Proves:**
    /// - The `SrgbToLinearGamma` arm correctly translates to
    ///   `LayerFilter::Gamma(GammaDirection::SrgbToLinear)`.
    ///
    /// **Discriminating:** `layer_color = rgba(180, 120, 60, 255)` — each channel
    /// is above the 0.04045 threshold, so the nonlinear formula fires and the oracle
    /// differs meaningfully from the input.
    #[test]
    fn p3_srgb_to_linear_gamma_via_producer_path() {
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

        let layer_color = Color::rgba(180, 120, 60, 255);
        let filter = ColorFilter::SrgbToLinearGamma;

        let readback = render_via_producer_path(
            &device,
            &queue,
            &surface_tex,
            &surface_view,
            filter,
            layer_color,
        );

        // GPU output is premultiplied; opaque alpha → premul == straight.
        let expected = [
            srgb_to_linear_oracle(180),
            srgb_to_linear_oracle(120),
            srgb_to_linear_oracle(60),
            255,
        ];

        assert_interior_pixels_near("P3 SrgbToLinearGamma producer path", &readback, expected, 3);
    }

    // ── P4: Matrix filter via producer path matches direct-painter path ───────

    /// P4: `ColorFilter::Matrix(grayscale)` through `Backend::push_color_filter`
    /// produces output byte-identical (tolerance=2) to the same filter applied
    /// via `WgpuPainter::save_layer_with_filter(LayerFilter::ColorMatrix(...))`.
    ///
    /// **Proves:**
    /// - The `ColorFilter::Matrix` arm correctly translates to `LayerFilter::ColorMatrix`.
    /// - The `ColorMatrix::values` field is passed through correctly.
    ///
    /// **Note:** This is redundant with `color_matrix_filter_tests` for the GPU shader
    /// correctness, but it closes the loop on the producer-path dispatch for the
    /// Matrix variant — confirming the `m.values` extraction works end-to-end.
    #[test]
    fn p4_matrix_filter_via_producer_path_matches_direct_painter() {
        use flui_types::painting::effects::ColorMatrix;

        use crate::wgpu::command_ir::LayerFilter;

        let (device, queue) = acquire_test_device_and_queue();
        let (producer_tex, producer_view) = create_surface(&device);
        let (direct_tex, direct_view) = create_surface(&device);

        let black = wgpu::Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        clear_surface(&device, &queue, &producer_view, black);
        clear_surface(&device, &queue, &direct_view, black);

        let layer_color = Color::rgba(180, 90, 40, 255);
        let grayscale = ColorMatrix::grayscale();
        let bounds = full_surface_bounds();

        // Producer path: Backend::push_color_filter(&ColorFilter::Matrix(grayscale)).
        {
            let painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            let mut backend = Backend::new(painter);
            backend.push_color_filter(&ColorFilter::Matrix(grayscale));
            backend.render_rect(bounds, &Paint::fill(layer_color), &Matrix4::IDENTITY);
            backend.pop_color_filter();
            let mut painter = backend.into_painter();
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("P4 Producer Encoder"),
            });
            painter
                .render(
                    RenderTarget::sampleable(&producer_view, &producer_tex),
                    &mut encoder,
                )
                .expect("P4 producer render must succeed");
            queue.submit(std::iter::once(encoder.finish()));
        }

        // Direct path: WgpuPainter::save_layer_with_filter(LayerFilter::ColorMatrix(grayscale.values)).
        {
            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            painter.save_layer_with_filter(None, LayerFilter::ColorMatrix(grayscale.values));
            painter.rect(bounds, &Paint::fill(layer_color));
            painter.restore_layer();
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("P4 Direct Encoder"),
            });
            painter
                .render(
                    RenderTarget::sampleable(&direct_view, &direct_tex),
                    &mut encoder,
                )
                .expect("P4 direct render must succeed");
            queue.submit(std::iter::once(encoder.finish()));
        }

        let producer_pixels = readback_pixels(&device, &queue, &producer_tex);
        let direct_pixels = readback_pixels(&device, &queue, &direct_tex);

        // Both paths must produce byte-identical output for every interior pixel.
        let width = SURFACE_WIDTH as usize;
        let height = SURFACE_HEIGHT as usize;
        for (pixel_index, (&prod, &direct)) in
            producer_pixels.iter().zip(direct_pixels.iter()).enumerate()
        {
            let row = pixel_index / width;
            let col = pixel_index % width;
            if row == 0 || row >= height - 1 || col == 0 || col >= width - 1 {
                continue;
            }
            for channel_index in 0..4 {
                let diff = u8::try_from(
                    (i16::from(prod[channel_index]) - i16::from(direct[channel_index]))
                        .unsigned_abs(),
                )
                .expect("diff of two u8 values always fits in u8");
                assert!(
                    diff <= 2,
                    "P4 Matrix/Grayscale producer vs direct: pixel {pixel_index} \
                     (row={row} col={col}) channel {channel_index} — \
                     producer={p} direct={d} diff={diff} > tolerance 2",
                    p = prod[channel_index],
                    d = direct[channel_index],
                );
            }
        }
    }
}
