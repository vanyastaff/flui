//! GPU readback acceptance gate for the color-matrix filter pass.
//!
//! ## Test inventory
//!
//! | # | Gate | Requirement |
//! |---|------|-------------|
//! | 1 | GPU | Identity matrix: output equals unfiltered source |
//! | 2 | GPU | Swap-R↔B matrix: opaque red input → premul blue output |
//! | 3 | GPU | Identity on translucent: 50% alpha survives filter correctly |
//! | 4 | GPU | Asymmetric matrix (saturation=0) on mixed color: all channels equal (gray) — catches transpose bug |
//! | 5 | GPU | Brightness(+0.3) on translucent green: oracle match catches premul skip |
//! | 6 | GPU | Filter layer nested in opacity-0.5 layer: output alpha ≈ 128 (inherits parent opacity) |
//!
//! All tests use `enable-wgpu-tests` feature-gate and follow the same harness
//! pattern as `layer_blend_tests`.  The 64×64 surface avoids DX12 small-texture
//! copy artefacts (see `layer_blend_tests.rs` for the rationale).

// ─── GPU readback tests ───────────────────────────────────────────────────────

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod gpu_tests {
    use std::sync::Arc;

    use flui_painting::Paint;
    use flui_types::{Color, Rect, geometry::Pixels, painting::ColorMatrix};

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
        .expect("a GPU adapter must be available for color_matrix_filter_tests");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("ColorMatrixFilter Test Device"),
            ..Default::default()
        }))
        .expect("a GPU device must be available for color_matrix_filter_tests");
        (Arc::new(device), Arc::new(queue))
    }

    fn create_surface(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ColorMatrixFilter Test Surface"),
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
            label: Some("ColorMatrixFilter Surface Clear"),
        });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ColorMatrixFilter Clear Pass"),
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
            label: Some("ColorMatrixFilter Readback Staging"),
            size: staging_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ColorMatrixFilter Readback Encoder"),
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
            // Skip the 1-pixel border: SDF fwidth uses helper fragments outside
            // the primitive at the viewport edge, yielding partial alpha there.
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

    // ── CPU oracle helpers ────────────────────────────────────────────────────

    /// Apply `matrix` to a straight-alpha RGBA color and return the expected
    /// premultiplied `[r, g, b, a]` u8 quad, matching the shader's math:
    ///
    /// 1. Treat input as straight-alpha (opaque input → straight == itself).
    /// 2. `output = M * straight + offset`, clamped component-wise to `[0, 1]`.
    /// 3. Re-premultiply: `(r*a, g*a, b*a, a)` in the `[0,1]` domain.
    /// 4. Quantise to u8 via `round(x * 255)`.
    fn color_matrix_oracle(matrix: &ColorMatrix, straight_rgba: [f32; 4]) -> [u8; 4] {
        let v = &matrix.values;
        let [sr, sg, sb, sa] = straight_rgba;
        // The 5×4 matrix: row i = [v[5i], v[5i+1], v[5i+2], v[5i+3], v[5i+4]]
        // Output_i = row_i · [sr, sg, sb, sa, 1]
        let out_r = (v[0] * sr + v[1] * sg + v[2] * sb + v[3] * sa + v[4]).clamp(0.0, 1.0);
        let out_g = (v[5] * sr + v[6] * sg + v[7] * sb + v[8] * sa + v[9]).clamp(0.0, 1.0);
        let out_b = (v[10] * sr + v[11] * sg + v[12] * sb + v[13] * sa + v[14]).clamp(0.0, 1.0);
        let out_a = (v[15] * sr + v[16] * sg + v[17] * sb + v[18] * sa + v[19]).clamp(0.0, 1.0);
        // Re-premultiply.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "values clamped to [0,1]*255 then rounded; truncation is safe"
        )]
        let to_u8 = |x: f32| (x * 255.0).round() as u8;
        [
            to_u8(out_r * out_a),
            to_u8(out_g * out_a),
            to_u8(out_b * out_a),
            to_u8(out_a),
        ]
    }

    // ── Identity matrix — output byte-identical to unfiltered ────────────────

    /// An identity color-matrix filter must produce output bit-identical to
    /// drawing the same rect without any filter.
    ///
    /// **Proves:**
    /// - The filter pass runs (it must read and write to the ping-pong texture).
    /// - Identity `m * straight + 0 = straight`, so no channels are mutated.
    /// - `needs_composite` gate is set for filter layers even when opacity=1.
    ///
    /// **Fails if:** the filter pass is silently skipped (reintegrate fast-path
    /// wrongly fires when `filter.is_some()`).
    #[test]
    fn identity_filter_produces_same_output_as_no_filter() {
        let (device, queue) = acquire_test_device_and_queue();
        let (no_filter_tex, no_filter_view) = create_surface(&device);
        let (identity_filter_tex, identity_filter_view) = create_surface(&device);

        let black = wgpu::Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        clear_surface(&device, &queue, &no_filter_view, black);
        clear_surface(&device, &queue, &identity_filter_view, black);

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

        // With identity filter.
        {
            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            let identity = ColorMatrix::identity();
            painter.save_layer_with_filter(None, LayerFilter::ColorMatrix(identity.values));
            painter.rect(bounds, &Paint::fill(source_color));
            painter.restore_layer();
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            painter
                .render(
                    RenderTarget::sampleable(&identity_filter_view, &identity_filter_tex),
                    &mut encoder,
                )
                .expect("identity-filter render must succeed");
            queue.submit(std::iter::once(encoder.finish()));
        }

        let no_filter_pixels = readback_pixels(&device, &queue, &no_filter_tex);
        let identity_pixels = readback_pixels(&device, &queue, &identity_filter_tex);

        // Identity must be bit-exact — tolerance 0.  Both paths go through the
        // same rect draw; the only difference is the identity filter pass.
        // ±1 for u8 quantisation mismatch when the premul round-trip at the
        // offscreen boundary moves a texel by half an LSB.
        let quantization_tolerance = 1u8;
        let width = SURFACE_WIDTH as usize;
        let height = SURFACE_HEIGHT as usize;
        for (pixel_index, (&nf, &id)) in no_filter_pixels
            .iter()
            .zip(identity_pixels.iter())
            .enumerate()
        {
            let row = pixel_index / width;
            let col = pixel_index % width;
            if row == 0 || row >= height - 1 || col == 0 || col >= width - 1 {
                continue;
            }
            for channel_index in 0..4 {
                let channel_diff = u8::try_from(
                    (i16::from(nf[channel_index]) - i16::from(id[channel_index])).unsigned_abs(),
                )
                .expect("diff of two u8 values always fits in u8");
                assert!(
                    channel_diff <= quantization_tolerance,
                    "pixel {pixel_index} channel {channel_index} — \
                     no_filter={a} identity_filter={b} \
                     diff={channel_diff} > tolerance {quantization_tolerance}",
                    a = nf[channel_index],
                    b = id[channel_index],
                );
            }
        }
    }

    // ── Swap-R↔B matrix — opaque red → premul blue ───────────────────────────

    /// A channel-swap matrix (R↔B, pass-through G and A) applied to an
    /// opaque red layer must produce opaque blue output.
    ///
    /// **Proves:**
    /// - Per-pixel unpremultiply → matrix → clamp → repremultiply is correct.
    /// - The `from_values` row/column mapping correctly sends red-channel weight
    ///   to the blue output and vice versa.
    /// - Straight-alpha and premultiplied-alpha roundtrip is numerically correct
    ///   for opaque inputs (alpha=1 → premul==straight; no division hazard).
    ///
    /// **Fails if:** the WGSL mat4x4 column-major vs Rust row-major mapping is
    /// wrong (all four off-diagonal entries would be swapped, producing wrong
    /// channels — e.g. leaving red unchanged instead of swapping to blue).
    #[test]
    fn swap_rb_matrix_converts_red_to_blue() {
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

        // Source: opaque red.
        let source_color = Color::rgba(200, 0, 0, 255);

        // Swap-R↔B matrix: row-major 5×4 layout.
        //   R_out = B_in  →  row 0 = [0, 0, 1, 0, 0]
        //   G_out = G_in  →  row 1 = [0, 1, 0, 0, 0]
        //   B_out = R_in  →  row 2 = [1, 0, 0, 0, 0]
        //   A_out = A_in  →  row 3 = [0, 0, 0, 1, 0]
        #[rustfmt::skip]
        let swap_rb = ColorMatrix {
            values: [
                0.0, 0.0, 1.0, 0.0,   0.0,   // R_out = B_in
                0.0, 1.0, 0.0, 0.0,   0.0,   // G_out = G_in
                1.0, 0.0, 0.0, 0.0,   0.0,   // B_out = R_in
                0.0, 0.0, 0.0, 1.0,   0.0,   // A_out = A_in
            ],
        };

        let bounds = full_surface_bounds();
        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save_layer_with_filter(None, LayerFilter::ColorMatrix(swap_rb.values));
        painter.rect(bounds, &Paint::fill(source_color));
        painter.restore_layer();

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_tex),
                &mut encoder,
            )
            .expect("swap-R↔B filter render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let readback = readback_pixels(&device, &queue, &surface_tex);

        // Oracle: apply the CPU swap-R↔B matrix to the straight-alpha source.
        let expected = color_matrix_oracle(&swap_rb, [200.0 / 255.0, 0.0, 0.0, 1.0]);

        // ±2: GPU u8 quantization at offscreen boundary.
        assert_interior_pixels_near("swap-R↔B", &readback, expected, 2);
    }

    // ── Translucent premul roundtrip ──────────────────────────────────────────

    /// An identity matrix applied to a 50%-alpha source must preserve
    /// both the color channels and the alpha, proving the unpremultiply →
    /// (identity) matrix → clamp → repremultiply cycle is numerically stable
    /// for translucent inputs.
    ///
    /// **Proves:**
    /// - Divide-by-alpha guard in the shader handles `alpha ≠ 1` without NaN or
    ///   division by zero.
    /// - For `alpha = 0.5`, `straight = premul / 0.5` is exact in f32 for the
    ///   tested colors; identity matrix leaves straight unchanged; re-premul gives
    ///   back the original premul value within quantisation.
    ///
    /// **Fails if:** the shader skips the unpremultiply step (straight≠premul for
    /// alpha<1, so the matrix then operates on the wrong values and the output
    /// color channels are scaled incorrectly).
    #[test]
    fn identity_filter_preserves_translucent_premul_value() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_tex, surface_view) = create_surface(&device);

        // Transparent black backdrop — so compositing only sees the layer.
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

        // Source: 50% alpha green — chosen so straight = (0, 1, 0, 0.5),
        // premul = (0, 128, 0, 128) in u8.
        let source_color = Color::rgba(0, 255, 0, 128);

        let bounds = full_surface_bounds();
        let identity = ColorMatrix::identity();
        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save_layer_with_filter(None, LayerFilter::ColorMatrix(identity.values));
        painter.rect(bounds, &Paint::fill(source_color));
        painter.restore_layer();

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_tex),
                &mut encoder,
            )
            .expect("translucent filter render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let readback = readback_pixels(&device, &queue, &surface_tex);

        // Oracle: identity matrix on (0, 1, 0, 0.5) straight → same straight →
        // repremul → (0 * 0.5, 1 * 0.5, 0 * 0.5, 0.5) = (0, 0.5, 0, 0.5)
        // → u8: (0, 128, 0, 128).
        let expected = color_matrix_oracle(&identity, [0.0, 1.0, 0.0, 128.0 / 255.0]);

        // ±3: offscreen boundary premul quantization for translucent pixels
        // incurs one extra rounding step vs. opaque paths.
        assert_interior_pixels_near("translucent premul", &readback, expected, 3);
    }

    // ── Asymmetric matrix (catches the transpose bug) ──────────────────────────

    /// `ColorMatrix::saturation(0.0)` (grayscale) applied to opaque red must
    /// produce gray ≈ (45, 45, 45, 255), NOT greenish (54, 182, 18, 255).
    ///
    /// The saturation-0 matrix is severely asymmetric: off-diagonal weights differ
    /// dramatically above and below the diagonal (luminance coefficients
    /// 0.2126/0.7152/0.0722 are on the R-row, not the columns).
    ///
    /// **The transpose bug would produce:** applying the transposed matrix to red
    /// (1,0,0,1) reads column 0 of the transposed matrix = row 0 of M.  With
    /// saturation=0 the transposed column 0 = (0.2126, 0.2126, 0.2126, 0) which
    /// happens to give the correct R value — but columns 1,2,3 are wrong:
    /// column 1 = (0.7152, 0.7152, 0.7152, 0) → applies G-weight to every channel.
    /// Since source G=0 and B=0, only R contributes; with Mᵀ: the output for red
    /// would be (0.2126, 0.2126, 0.2126, 1) which is accidentally correct for this
    /// specific input.
    ///
    /// Opaque green is NOT differentiating (saturation(0) sends it to gray under both
    /// M and Mᵀ — column 1 of Mᵀ equals row 1 of M, the all-G-out row). The truly
    /// differentiating input is a mixed color, e.g. warm orange (R=200, G=80, B=0):
    ///
    /// ```text
    /// Correct    M·v:  R_out = 0.2126*R + 0.7152*G + 0.0722*B ≈ 0.391
    ///                  → all channels equal → gray (0.391, 0.391, 0.391, 1)
    /// Transposed Mᵀ·v: R_out = 0.2126*(R+G+B) ≈ 0.233, G_out = 0.7152*(R+G+B) ≈ 0.785
    ///                  → NOT gray, channels diverge (proves the transpose)
    /// ```
    ///
    /// **Fails if:** `from_values` packs rows instead of columns (transpose bug).
    #[test]
    fn saturation_zero_on_mixed_color_produces_gray() {
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

        // Mixed warm color: R=200, G=80, B=0, A=255.
        // With the correct matrix all RGB channels become equal (gray).
        // With the transposed matrix G would be ~3x brighter than R and B —
        // the assertion below catches that asymmetry.
        let source_color = Color::rgba(200, 80, 0, 255);
        let grayscale_matrix = ColorMatrix::saturation(0.0);

        let bounds = full_surface_bounds();
        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save_layer_with_filter(None, LayerFilter::ColorMatrix(grayscale_matrix.values));
        painter.rect(bounds, &Paint::fill(source_color));
        painter.restore_layer();

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_tex),
                &mut encoder,
            )
            .expect("saturation-0 filter render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let readback = readback_pixels(&device, &queue, &surface_tex);

        // CPU oracle: apply saturation(0) to the straight source.
        let expected =
            color_matrix_oracle(&grayscale_matrix, [200.0 / 255.0, 80.0 / 255.0, 0.0, 1.0]);

        // The three RGB channels of the oracle must be equal (grayscale invariant).
        assert_eq!(
            expected[0], expected[1],
            "oracle: grayscale must produce equal R and G channels"
        );
        assert_eq!(
            expected[1], expected[2],
            "oracle: grayscale must produce equal G and B channels"
        );

        // ±3: tolerates GPU u8 quantisation at the offscreen boundary.
        assert_interior_pixels_near("saturation(0) on warm color", &readback, expected, 3);
    }

    // ── Non-identity matrix on translucent input ───────────────────────────────

    /// `ColorMatrix::brightness(0.3)` applied to 50%-alpha green must produce
    /// the oracle value, not one that skipped the unpremultiply step.
    ///
    /// **Proves (combined):**
    /// - The unpremultiply → asymmetric-matrix → clamp → repremultiply cycle is
    ///   correct for translucent inputs.
    /// - The brightness matrix is NOT the identity, so this catches both the
    ///   premul error and any uniform-packing error simultaneously.
    ///
    /// **What the premul bug would produce (skipping unpremultiply):**
    /// Input premul = (0, 0.5, 0, 0.5).  Brightness adds +0.3 to each RGB channel.
    /// If the shader operates on premul instead of straight:
    ///   out = clamp((0,0.5,0,0.5) + (0.3,0.3,0.3,0)) = (0.3, 0.8, 0.3, 0.5)
    ///   repremul G = 0.8 * 0.5 = 0.4.
    /// Correct (operate on straight (0,1,0,0.5) then repremul):
    ///   out_straight = (0.3, 1.3→clamp→1.0, 0.3, 0.5)
    ///   repremul G = 1.0 * 0.5 = 0.5.
    /// The two values differ by 0.1 → ~25 u8 units, well above tolerance.
    ///
    /// **Fails if:** the shader applies the matrix to the premultiplied value
    /// instead of the straight-alpha value.
    #[test]
    fn brightness_filter_on_translucent_green_matches_oracle() {
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

        // Source: 50% alpha green.  Premul = (0, 128, 0, 128) in u8.
        let source_color = Color::rgba(0, 255, 0, 128);
        let brightness_matrix = ColorMatrix::brightness(0.3);

        let bounds = full_surface_bounds();
        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save_layer_with_filter(None, LayerFilter::ColorMatrix(brightness_matrix.values));
        painter.rect(bounds, &Paint::fill(source_color));
        painter.restore_layer();

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_tex),
                &mut encoder,
            )
            .expect("brightness-on-translucent render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let readback = readback_pixels(&device, &queue, &surface_tex);

        // Oracle: brightness on straight (0, 1, 0, 0.5).
        let expected = color_matrix_oracle(&brightness_matrix, [0.0, 1.0, 0.0, 128.0 / 255.0]);

        // ±4: translucent offscreen + clamping introduces one extra quantisation step.
        assert_interior_pixels_near(
            "brightness(+0.3) on translucent green",
            &readback,
            expected,
            4,
        );
    }

    // ── Nested filter inside opacity layer (opacity semantics) ─────────────────

    /// A filter layer nested inside an outer opacity-0.5 layer must composite
    /// at the parent's opacity (0.5), not at 1.0.
    ///
    /// **Proves:** `save_layer_with_filter` calls `effective_layer_opacity(1.0)`
    /// which multiplies 1.0 by the current ancestor opacity — correctly inheriting
    /// the parent's opacity rather than overriding it.
    ///
    /// **Fails if:** the filter layer ignores the parent opacity and composites at
    /// full opacity (output alpha would be ~255 instead of ~128 for opaque content).
    #[test]
    fn filter_layer_inherits_outer_opacity() {
        use flui_types::painting::BlendMode;

        let (device, queue) = acquire_test_device_and_queue();
        let (surface_tex, surface_view) = create_surface(&device);

        // Transparent black backdrop.
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

        // Draw: outer opacity-0.5 saveLayer → identity-filter saveLayer → opaque red rect.
        let bounds = full_surface_bounds();
        let source_color = Color::rgba(255, 0, 0, 255);
        let identity = ColorMatrix::identity();

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        let outer_paint = Paint::fill(Color::WHITE)
            .with_blend_mode(BlendMode::SrcOver)
            .with_opacity(0.5);
        painter.save_layer(Some(bounds), &outer_paint);
        painter.save_layer_with_filter(None, LayerFilter::ColorMatrix(identity.values));
        painter.rect(bounds, &Paint::fill(source_color));
        painter.restore_layer(); // pop filter layer
        painter.restore_layer(); // pop opacity layer

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_tex),
                &mut encoder,
            )
            .expect("nested filter+opacity render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let readback = readback_pixels(&device, &queue, &surface_tex);

        // The outer opacity layer composites at 0.5, so the output alpha should
        // be approximately 128 (50% of 255).  If the filter layer ignores parent
        // opacity, alpha would be ~255.
        let width = SURFACE_WIDTH as usize;
        let height = SURFACE_HEIGHT as usize;
        for (pixel_index, &pixel) in readback.iter().enumerate() {
            let row = pixel_index / width;
            let col = pixel_index % width;
            if row == 0 || row >= height - 1 || col == 0 || col >= width - 1 {
                continue;
            }
            let alpha = pixel[3];
            assert!(
                alpha < 200,
                "pixel {pixel_index} alpha={alpha} — filter layer must \
                 inherit outer opacity=0.5 (expected alpha ≈ 128, not full 255)"
            );
            assert!(
                alpha > 50,
                "pixel {pixel_index} alpha={alpha} — expected alpha ≈ 128, not 0"
            );
        }
    }
}
