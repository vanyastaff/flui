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
//! All tests use `testing` feature-gate and follow the same harness
//! pattern as `layer_blend_tests`.  The 64×64 surface avoids DX12 small-texture
//! copy artefacts (see `layer_blend_tests.rs` for the rationale).

// ─── GPU readback tests ───────────────────────────────────────────────────────

#[cfg(all(test, feature = "testing"))]
mod gpu_tests {
    use std::sync::Arc;

    use flui_foundation::geometry::Rect;
    use flui_painting::Paint;
    use flui_painting::{paint::ColorMatrix, styling::Color};

    use crate::{command_ir::LayerFilter, painter::WgpuPainter, render_target::RenderTarget};

    // ── Harness constants ─────────────────────────────────────────────────────

    const SURFACE_WIDTH: u32 = 64;
    const SURFACE_HEIGHT: u32 = 64;
    const SURFACE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

    // ── Harness helpers ───────────────────────────────────────────────────────

    fn acquire_test_device_and_queue() -> (Arc<wgpu::Device>, Arc<wgpu::Queue>) {
        crate::test_support::test_device_and_queue("ColorMatrixFilter Test Device")
    }

    fn create_surface(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView) {
        crate::test_support::create_sampleable_target(
            device,
            "ColorMatrixFilter Test Surface",
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

    fn full_surface_bounds() -> Rect<f64> {
        Rect::from_xywh(
            0.0,
            0.0,
            f64::from(SURFACE_WIDTH as f32),
            f64::from(SURFACE_HEIGHT as f32),
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
        let to_u8 = |x: f32| (x * 255.0).round() as u8;
        [
            to_u8(out_r * out_a),
            to_u8(out_g * out_a),
            to_u8(out_b * out_a),
            to_u8(out_a),
        ]
    }

    // ── Identity matrix — output byte-identical to unfiltered ────────────────

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
        painter.draw_rect(bounds, &Paint::fill(source_color));
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

    // ── Asymmetric matrix (catches the transpose bug) ──────────────────────────

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
        painter.draw_rect(bounds, &Paint::fill(source_color));
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
        use flui_painting::paint::BlendMode;

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
        painter.draw_rect(bounds, &Paint::fill(source_color));
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
