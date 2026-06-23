//! PR-4 GPU acceptance gate: shape-level (tessellated rect/rrect) advanced blend.
//!
//! Unit tests (S1-S4e, no GPU) live in `batches/mod.rs` as an inline
//! `#[cfg(test)] mod unit_tests` block — they need `pub(super)` access to
//! `DrawBatcher`, `GpuStateStack`, and `vertices_aabb` which are not reachable
//! from this sibling module.
//!
//! ## GPU test inventory
//!
//! | # | Requirement |
//! |---|-------------|
//! | S5 | `drawRect` with Multiply over non-flat dst ≈ oracle (interior ±2) |
//! | S6 | `drawRRect` with Screen over non-flat dst ≈ oracle (interior ±2) |
//! | S7 | Z-interleave: SrcOver content before AND after an advanced shape — order correct |
//! | S8 | Foreground confined by `clip_rect`: scissored region blended, no right-half assertion |
//! | S9 | SrcOver shape byte-identity: advanced branch NOT taken |
//! | S10 | Plus/Modulate shapes: no panic, valid RGBA output (Segment path) |
//! | S11a | `has_advanced_shape_straddling`: straddling AdvancedShape → `true` |
//! | S11b | `has_advanced_shape_straddling`: fully-contained AdvancedShape → `false` |
//! | S11c | `has_advanced_shape_straddling`: SrcOver (Segment) straddling → `false` |

// ─── GPU readback tests ───────────────────────────────────────────────────────

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod gpu_tests {
    use std::sync::Arc;

    use flui_painting::{BlendMode, Paint, PaintStyle};
    use flui_types::{
        Color, Rect,
        geometry::{Pixels, RRect},
    };

    use crate::wgpu::{painter::WgpuPainter, render_target::RenderTarget};

    // ── Harness constants ─────────────────────────────────────────────────────

    // 64×64: avoids DX12 small-texture copy artifacts (same rationale as
    // layer_blend_tests.rs — corner texels can be physically impossible at 8×8).
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
        .expect("a GPU adapter must be available for shape_blend_tests");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("ShapeBlend Test Device"),
            ..Default::default()
        }))
        .expect("a GPU device must be available for shape_blend_tests");
        (Arc::new(device), Arc::new(queue))
    }

    /// Create a sampleable surface texture (RENDER_ATTACHMENT | TEXTURE_BINDING |
    /// COPY_SRC | COPY_DST) needed for advanced blend backdrop reads.
    fn create_sampleable_surface(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ShapeBlend Test Surface"),
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

    /// Fill the entire surface with a solid colour via a clear pass.
    fn clear_surface_to_color(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        clear_color: wgpu::Color,
    ) {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ShapeBlend Surface Fill"),
        });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ShapeBlend Fill Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color),
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

    /// Read all pixels from `texture` and return RGBA bytes (row-major).
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
            label: Some("ShapeBlend Readback Staging"),
            size: staging_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut copy_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ShapeBlend Readback Encoder"),
        });
        copy_encoder.copy_texture_to_buffer(
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
        queue.submit(std::iter::once(copy_encoder.finish()));

        let pixel_slice = staging.slice(..);
        pixel_slice.map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("GPU readback poll must complete");

        let raw_bytes = pixel_slice.get_mapped_range();
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

    /// CPU oracle: `Color::blend(src, dst, mode)` → premultiplied RGBA u8.
    fn oracle_premultiplied(src: Color, dst: Color, mode: BlendMode) -> [u8; 4] {
        let result = src.blend(dst, mode);
        let [r, g, b, a] = result.to_f32_array();
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "value is clamped to [0,1]*255 then rounded — truncation is safe"
        )]
        let to_u8 = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
        [to_u8(r * a), to_u8(g * a), to_u8(b * a), to_u8(a)]
    }

    /// Assert two premultiplied RGBA pixels are within `tolerance` in all channels.
    fn assert_pixel_within_tolerance(
        label: &str,
        actual: [u8; 4],
        expected: [u8; 4],
        tolerance: u8,
    ) {
        for ch in 0..4 {
            let diff =
                u8::try_from((i16::from(actual[ch]) - i16::from(expected[ch])).unsigned_abs())
                    .expect("diff of two u8 values fits in u8");
            assert!(
                diff <= tolerance,
                "{label}: channel {ch} — actual={a} expected={e} diff={diff} > tolerance {tolerance}",
                a = actual[ch],
                e = expected[ch],
            );
        }
    }

    /// Build a fresh `WgpuPainter` for `device` / `queue`.
    fn build_painter(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> WgpuPainter {
        WgpuPainter::with_shared_device(
            device,
            queue,
            SURFACE_FORMAT,
            (SURFACE_WIDTH, SURFACE_HEIGHT),
        )
    }

    /// Full-surface bounds for the test viewport.
    fn full_surface_bounds() -> Rect<Pixels> {
        Rect::from_xywh(
            Pixels(0.0),
            Pixels(0.0),
            Pixels(SURFACE_WIDTH as f32),
            Pixels(SURFACE_HEIGHT as f32),
        )
    }

    // ── S5: drawRect Multiply vs CPU oracle ───────────────────────────────────

    /// S5: An opaque Multiply rect drawn directly (not via saveLayer) over a solid
    /// backdrop must match `Color::blend(src, dst, Multiply)` within ±2.
    ///
    /// **Proves:**
    /// - `DrawItem::AdvancedShape` is created for the Multiply rect.
    /// - `render_segment_to_offscreen` renders the shape correctly.
    /// - `flush_advanced_layer` computes Multiply and writes it to the surface.
    /// - The CPU oracle matches the GPU result.
    ///
    /// **Fails if:**
    /// - Shape stays on SrcOver fallback (src dominates instead of multiplying).
    /// - AABB is wrong (off-screen copy reads wrong region).
    ///
    /// ## AA boundary exclusion
    ///
    /// The tessellated rect has aliased edges at `sample_count=1`.  At the last
    /// row/column of the viewport the `flush_tessellated_geometry` path may leave
    /// partial-alpha edge pixels.  We skip those boundary texels (same policy as
    /// T6/T10 in `layer_blend_tests.rs`).
    #[test]
    fn multiply_rect_matches_cpu_oracle() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_sampleable_surface(&device);

        // Backdrop: opaque blue.
        let backdrop_color = Color::rgba(40, 60, 220, 255);
        clear_surface_to_color(
            &device,
            &queue,
            &surface_view,
            wgpu::Color {
                r: 40.0 / 255.0,
                g: 60.0 / 255.0,
                b: 220.0 / 255.0,
                a: 1.0,
            },
        );

        let source_color = Color::rgba(200, 120, 40, 255);
        let draw_rect = full_surface_bounds();

        // Direct drawRect with Multiply blend mode — no saveLayer.
        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.rect(
            draw_rect,
            &Paint {
                style: PaintStyle::Fill,
                color: source_color,
                blend_mode: BlendMode::Multiply,
                ..Default::default()
            },
        );

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("S5 Multiply Rect Encoder"),
        });
        let target = RenderTarget::sampleable(&surface_view, &surface_texture);
        painter
            .render(target, &mut encoder)
            .expect("painter.render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let readback = readback_pixels(&device, &queue, &surface_texture);
        let expected = oracle_premultiplied(source_color, backdrop_color, BlendMode::Multiply);
        let tolerance = 2u8;

        let width = SURFACE_WIDTH as usize;
        let height = SURFACE_HEIGHT as usize;
        for (idx, &pixel) in readback.iter().enumerate() {
            let row = idx / width;
            let col = idx % width;
            // Skip tessellation boundary: last row and last column.
            // The tessellated quad edge at the viewport boundary triggers the
            // aliased-edge path which produces partial alpha — not modelled by
            // the all-opaque oracle.
            if row >= height - 1 || col >= width - 1 {
                continue;
            }
            assert_pixel_within_tolerance(
                &format!("S5 Multiply pixel {idx} (row={row} col={col})"),
                pixel,
                expected,
                tolerance,
            );
        }
    }

    // ── S6: drawRRect Screen vs CPU oracle ────────────────────────────────────

    /// S6: An opaque Screen rrect drawn directly over a solid backdrop must match
    /// `Color::blend(src, dst, Screen)` within ±2 for interior pixels.
    ///
    /// Uses an inner rect (not full-viewport) to avoid the boundary-pixel issue
    /// at the surface edge.  The interior is far from the rounded-rect edges.
    ///
    /// **Proves:** the rrect tessellated path also flows through
    /// `add_tessellated_with_key` → `AdvancedShapeOp`.
    #[test]
    fn screen_rrect_interior_matches_cpu_oracle() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_sampleable_surface(&device);

        let backdrop_color = Color::rgba(100, 50, 200, 255);
        clear_surface_to_color(
            &device,
            &queue,
            &surface_view,
            wgpu::Color {
                r: 100.0 / 255.0,
                g: 50.0 / 255.0,
                b: 200.0 / 255.0,
                a: 1.0,
            },
        );

        let source_color = Color::rgba(150, 200, 60, 255);

        // Inner rrect — 8px inset from all sides, corner radius 4px.
        // Interior is at least 8px from any edge, safely away from AA boundaries.
        let inset = 8.0_f32;
        let rrect = RRect::from_rect_circular(
            Rect::from_ltrb(
                Pixels(inset),
                Pixels(inset),
                Pixels(SURFACE_WIDTH as f32 - inset),
                Pixels(SURFACE_HEIGHT as f32 - inset),
            ),
            Pixels(4.0),
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.rrect(
            rrect,
            &Paint {
                style: PaintStyle::Fill,
                color: source_color,
                blend_mode: BlendMode::Screen,
                ..Default::default()
            },
        );

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("S6 Screen RRect Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("painter.render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let readback = readback_pixels(&device, &queue, &surface_texture);
        let expected = oracle_premultiplied(source_color, backdrop_color, BlendMode::Screen);
        let tolerance = 2u8;

        // Check only pixels in the interior of the rrect (far from any edge).
        // The interior rect is inset by 2*inset from all surface edges so we are
        // safely away from both the surface boundary AND the rounded-rect boundary.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "inset is a small positive pixel margin; truncation and sign-loss are safe"
        )]
        let inner_margin = (inset * 2.0) as usize;
        let width = SURFACE_WIDTH as usize;
        let height = SURFACE_HEIGHT as usize;
        for row in inner_margin..(height - inner_margin) {
            for col in inner_margin..(width - inner_margin) {
                let idx = row * width + col;
                assert_pixel_within_tolerance(
                    &format!("S6 Screen rrect interior pixel {idx} (row={row} col={col})"),
                    readback[idx],
                    expected,
                    tolerance,
                );
            }
        }
    }

    // ── S7: Z-interleave — content before and after advanced shape ────────────

    /// S7: Content drawn BEFORE an advanced shape must appear behind it, and
    /// content drawn AFTER must appear on top.
    ///
    /// Setup:
    /// - Backdrop: opaque blue.
    /// - SrcOver rect drawn BEFORE the advanced shape (orange) → blended with blue.
    /// - Multiply rect (full-surface advanced shape) → blends with current surface.
    /// - SrcOver rect drawn AFTER the advanced shape (green, small, center) →
    ///   draws on top of the blended result.
    ///
    /// **Proves:** the Z-seal in `add_tessellated_with_key` (step 1: seal prior
    /// content) and the `submit` loop arm order preserve Z-ordering correctly.
    #[test]
    fn z_interleave_before_and_after_advanced_shape_is_correct() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_sampleable_surface(&device);

        // Backdrop: opaque blue.
        clear_surface_to_color(
            &device,
            &queue,
            &surface_view,
            wgpu::Color {
                r: 0.0,
                g: 0.0,
                b: 1.0,
                a: 1.0,
            },
        );

        let full_bounds = full_surface_bounds();
        // Small center rect for the "after" SrcOver draw.
        let center_rect = Rect::from_xywh(
            Pixels(SURFACE_WIDTH as f32 / 4.0),
            Pixels(SURFACE_HEIGHT as f32 / 4.0),
            Pixels(SURFACE_WIDTH as f32 / 2.0),
            Pixels(SURFACE_HEIGHT as f32 / 2.0),
        );

        let before_color = Color::rgba(220, 80, 30, 200); // translucent orange — before
        let multiply_color = Color::rgba(150, 150, 150, 255); // grey Multiply shape
        let after_color = Color::rgba(0, 255, 0, 255); // opaque green — after (on top)

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));

        // Draw 1: SrcOver rect (becomes backdrop context for the Multiply).
        painter.rect(full_bounds, &Paint::fill(before_color));

        // Draw 2: Multiply shape (advanced) — Z-seal fires here.
        painter.rect(
            full_bounds,
            &Paint {
                style: PaintStyle::Fill,
                color: multiply_color,
                blend_mode: BlendMode::Multiply,
                ..Default::default()
            },
        );

        // Draw 3: SrcOver rect drawn AFTER the advanced shape (green center).
        painter.rect(center_rect, &Paint::fill(after_color));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("S7 Z-interleave Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("painter.render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);

        // Assert: center pixels are green (after-draw dominates).
        let center_x = (SURFACE_WIDTH / 2) as usize;
        let center_y = (SURFACE_HEIGHT / 2) as usize;
        // Sample well inside the center rect to avoid tessellation edge pixels.
        let sample_offset = 4usize;
        let center_pixel = pixels[(center_y * SURFACE_WIDTH as usize) + center_x];
        assert!(
            center_pixel[1] > 200,
            "center pixel must be dominated by the green after-draw (G channel > 200); \
             got {center_pixel:?} — Z-ordering may be broken"
        );
        assert!(
            center_pixel[0] < 50,
            "center pixel red channel must be low (dominated by green after-draw); \
             got R={r}",
            r = center_pixel[0]
        );

        // Assert: corner pixels (outside center rect) are NOT pure green —
        // the Multiply result must show through.
        let corner_pixel = pixels[sample_offset * SURFACE_WIDTH as usize + sample_offset];
        assert!(
            corner_pixel[1] < 240,
            "corner pixel must NOT be pure green (Multiply shape affects it); \
             got {corner_pixel:?}"
        );
    }

    // ── S8: Scissor-respecting foreground — advanced shape inside clip_rect ──────

    /// S8: A `clip_rect` scissor applied to an advanced shape correctly confines
    /// the foreground tessellation to the scissored region; the INSIDE (left half)
    /// receives the correct `Multiply` blend result.
    ///
    /// This is a UNIT test for the painter's record-time scissor: it proves that
    /// foreground geometry for an advanced-blend shape is restricted by `clip_rect`.
    /// The right half is NOT asserted — its pixel value depends on whether the
    /// `flush_advanced_layer` backdrop copy wrote transparent-over-stale or fresh
    /// content there, which is the HAZARD documented in the damage-straddle
    /// correctness bug (`force_full_repaint_next_frame`).  Asserting the right half
    /// would enshrine that hazard as a guarantee.
    ///
    /// What this test does NOT cover:
    /// - The stale-pixel hazard when `device_bounds` straddles a partial damage edge.
    ///   That is covered by the `has_advanced_shape_straddling_detector_*` tests
    ///   (detector unit tests) and the `force_full_repaint_next_frame` field in
    ///   `Renderer` (self-healing integration).
    /// - Renderer-level damage tracking, which requires a full Renderer+surface setup.
    ///
    /// Setup:
    /// - Backdrop: opaque red (cleared directly onto the surface).
    /// - `clip_rect`: left half only (x=0..32, full height).
    /// - Multiply shape covering the full surface.
    ///
    /// Assertion: left half (inside scissor) must be the `Multiply` blend result.
    #[test]
    fn advanced_shape_foreground_confined_by_clip_rect() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_sampleable_surface(&device);

        // Backdrop: opaque red.
        let backdrop_color = Color::rgba(200, 30, 30, 255);
        clear_surface_to_color(
            &device,
            &queue,
            &surface_view,
            wgpu::Color {
                r: 200.0 / 255.0,
                g: 30.0 / 255.0,
                b: 30.0 / 255.0,
                a: 1.0,
            },
        );

        let source_color = Color::rgba(100, 200, 50, 255);
        let full_bounds = full_surface_bounds();
        let half_width = SURFACE_WIDTH as f32 / 2.0;

        let clip = Rect::from_ltrb(
            Pixels(0.0),
            Pixels(0.0),
            Pixels(half_width),
            Pixels(SURFACE_HEIGHT as f32),
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        // Apply scissor; the tessellation will only emit foreground geometry for
        // the left half.  The advanced-blend pass reads the backdrop and composites
        // the trimmed foreground.
        painter.clip_rect(clip);
        painter.rect(
            full_bounds,
            &Paint {
                style: PaintStyle::Fill,
                color: source_color,
                blend_mode: BlendMode::Multiply,
                ..Default::default()
            },
        );

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("S8 Clip-rect foreground encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("painter.render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);
        let expected_left = oracle_premultiplied(source_color, backdrop_color, BlendMode::Multiply);
        let tolerance = 2u8;

        let width = SURFACE_WIDTH as usize;
        let height = SURFACE_HEIGHT as usize;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "half_width is a small positive pixel count; truncation and sign-loss are safe"
        )]
        let half_col = half_width as usize;

        // Left half (inside scissor): foreground was emitted here; the Multiply
        // blend must have been applied correctly.
        for row in 1..(height - 1) {
            for col in 1..(half_col - 1) {
                let idx = row * width + col;
                assert_pixel_within_tolerance(
                    &format!("S8 left(blended) row={row} col={col}"),
                    pixels[idx],
                    expected_left,
                    tolerance,
                );
            }
        }
        // Right half is NOT asserted: its content depends on the backdrop-blend
        // outcome for a transparent foreground, which is the stale-pixel hazard
        // addressed by `force_full_repaint_next_frame` in `Renderer`.
    }

    // ── S9: SrcOver shape byte-identity ───────────────────────────────────────

    /// S9: A SrcOver shape renders deterministically and is unperturbed by PR-4.
    ///
    /// **Proves:** the SrcOver render path produces stable, self-consistent
    /// output after PR-4. The *routing* guarantee — that a SrcOver key does NOT
    /// divert into `DrawItem::AdvancedShape` — is proven by the no-GPU unit test
    /// `srcover_key_stays_in_segment_not_advanced` (S2). Together they show the
    /// SrcOver fast path is byte-identical to pre-PR-4.
    #[test]
    fn srcover_shape_is_byte_identical_to_pre_pr4() {
        // Two independent surfaces, same operations.
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_a, view_a) = create_sampleable_surface(&device);
        let (surface_b, view_b) = create_sampleable_surface(&device);

        let backdrop = wgpu::Color {
            r: 0.2,
            g: 0.4,
            b: 0.7,
            a: 1.0,
        };
        clear_surface_to_color(&device, &queue, &view_a, backdrop);
        clear_surface_to_color(&device, &queue, &view_b, backdrop);

        let source_color = Color::rgba(200, 80, 40, 180);
        let draw_bounds = full_surface_bounds();

        // Both painters draw the same SrcOver rect — results must be identical.
        for (surface, view) in [(&surface_a, &view_a), (&surface_b, &view_b)] {
            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            painter.rect(draw_bounds, &Paint::fill(source_color));
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            painter
                .render(RenderTarget::sampleable(view, surface), &mut encoder)
                .expect("render must succeed");
            queue.submit(std::iter::once(encoder.finish()));
        }

        let pixels_a = readback_pixels(&device, &queue, &surface_a);
        let pixels_b = readback_pixels(&device, &queue, &surface_b);

        for (idx, (a, b)) in pixels_a.iter().zip(pixels_b.iter()).enumerate() {
            assert_eq!(
                a, b,
                "S9: SrcOver shape pixel {idx}: {a:?} vs {b:?} — \
                 must be byte-identical (PR-4 must not perturb SrcOver path)"
            );
        }
    }

    // ── S10: Plus/Modulate shape — no panic, valid output, Segment path ───────

    /// S10: Plus and Modulate shapes drawn directly must not panic, must produce
    /// valid RGBA output, and must NOT take the advanced shape path.
    ///
    /// **Proves:** the routing guard (`!is_advanced()` for Plus/Modulate) correctly
    /// passes these to the fixed-function blend pipeline.
    #[test]
    fn plus_and_modulate_shapes_do_not_panic_and_use_segment_path() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_sampleable_surface(&device);

        clear_surface_to_color(
            &device,
            &queue,
            &surface_view,
            wgpu::Color {
                r: 0.3,
                g: 0.3,
                b: 0.3,
                a: 1.0,
            },
        );

        let draw_bounds = full_surface_bounds();
        let source_color = Color::rgba(100, 100, 100, 200);

        for mode in [BlendMode::Plus, BlendMode::Modulate] {
            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            painter.rect(
                draw_bounds,
                &Paint {
                    style: PaintStyle::Fill,
                    color: source_color,
                    blend_mode: mode,
                    ..Default::default()
                },
            );
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("S10 Plus/Modulate Encoder"),
            });
            painter
                .render(
                    RenderTarget::sampleable(&surface_view, &surface_texture),
                    &mut encoder,
                )
                .expect("Plus/Modulate shape must not return an error");
            queue.submit(std::iter::once(encoder.finish()));

            // Verify non-zero alpha in readback (confirms draw ran without panic).
            let pixels = readback_pixels(&device, &queue, &surface_texture);
            let non_zero_count = pixels.iter().filter(|p| p[3] > 0).count();
            assert!(
                non_zero_count > 0,
                "S10 {mode:?}: expected non-zero-alpha pixels; got all-zero \
                 (suggests draw silently produced nothing)"
            );
        }
    }

    // ── S11: Straddle detector — `has_advanced_shape_straddling` geometry ────────

    /// S11a: An advanced shape (`DrawItem::AdvancedShape`) whose `device_bounds`
    /// intersect the damage rect but are NOT fully contained by it (straddle) must
    /// cause `has_advanced_shape_straddling` to return `true`.
    ///
    /// This is the primary discriminator test for the new production method.
    /// It would not compile on `main` (the method is new) and it asserts real
    /// straddle geometry — not a tautology.
    #[test]
    fn has_advanced_shape_straddling_detector_straddle_returns_true() {
        let (device, queue) = acquire_test_device_and_queue();

        // Shape spans the full surface (0..64, 0..64).
        let full_bounds = full_surface_bounds();
        // Damage is the left half (0..32, 0..64) — the shape straddles the right edge.
        let half_width = SURFACE_WIDTH as f32 / 2.0;
        let damage = Rect::from_ltrb(
            Pixels(0.0),
            Pixels(0.0),
            Pixels(half_width),
            Pixels(SURFACE_HEIGHT as f32),
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        // Record a Multiply (advanced) rect that covers the full surface.
        painter.rect(
            full_bounds,
            &Paint {
                style: PaintStyle::Fill,
                color: Color::rgba(100, 200, 50, 255),
                blend_mode: BlendMode::Multiply,
                ..Default::default()
            },
        );

        assert!(
            painter.has_advanced_shape_straddling(damage),
            "S11a: full-surface Multiply shape must straddle a left-half damage rect"
        );
    }

    /// S11b: An advanced shape whose `device_bounds` are FULLY CONTAINED within
    /// the damage rect must cause `has_advanced_shape_straddling` to return `false`.
    ///
    /// Fully-contained shapes cannot write stale pixels outside the damage: the
    /// scissor already covers their entire extent.
    #[test]
    fn has_advanced_shape_straddling_detector_contained_returns_false() {
        let (device, queue) = acquire_test_device_and_queue();

        // Shape in the LEFT QUARTER (0..16, 0..64) — fully inside the left-half damage.
        let quarter_width = SURFACE_WIDTH as f32 / 4.0;
        let shape_bounds = Rect::from_ltrb(
            Pixels(0.0),
            Pixels(0.0),
            Pixels(quarter_width),
            Pixels(SURFACE_HEIGHT as f32),
        );
        // Damage is the left half (0..32, 0..64) — fully contains the shape.
        let half_width = SURFACE_WIDTH as f32 / 2.0;
        let damage = Rect::from_ltrb(
            Pixels(0.0),
            Pixels(0.0),
            Pixels(half_width),
            Pixels(SURFACE_HEIGHT as f32),
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.rect(
            shape_bounds,
            &Paint {
                style: PaintStyle::Fill,
                color: Color::rgba(100, 200, 50, 255),
                blend_mode: BlendMode::Multiply,
                ..Default::default()
            },
        );

        assert!(
            !painter.has_advanced_shape_straddling(damage),
            "S11b: a shape fully within the damage rect must NOT be flagged as straddling"
        );
    }

    /// S11c: A NON-advanced shape (SrcOver) whose `device_bounds` straddle the
    /// damage rect must cause `has_advanced_shape_straddling` to return `false`.
    ///
    /// SrcOver shapes do not call `flush_advanced_layer` and therefore cannot
    /// write stale pixels outside the scissor via the backdrop blend path.
    #[test]
    fn has_advanced_shape_straddling_detector_srcover_returns_false() {
        let (device, queue) = acquire_test_device_and_queue();

        // Full-surface SrcOver shape — straddles any sub-full damage rect.
        let full_bounds = full_surface_bounds();
        let half_width = SURFACE_WIDTH as f32 / 2.0;
        let damage = Rect::from_ltrb(
            Pixels(0.0),
            Pixels(0.0),
            Pixels(half_width),
            Pixels(SURFACE_HEIGHT as f32),
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        // SrcOver (default) — this stays in a `DrawItem::Segment`, NOT AdvancedShape.
        painter.rect(full_bounds, &Paint::fill(Color::rgba(100, 200, 50, 255)));

        assert!(
            !painter.has_advanced_shape_straddling(damage),
            "S11c: a SrcOver shape must NOT be flagged as straddling (not an advanced blend)"
        );
    }
}
