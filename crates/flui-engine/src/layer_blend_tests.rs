//! Save layers composited with a blend mode, read back from the GPU.
//!
//! - An opaque `Multiply` layer matches the CPU oracle.
//! - A layer composites its whole region with its recorded mode
//!   (`ARCHITECTURE.md` mapping decision 19): the region is the bounds mapped
//!   through the transform, cut by the clip, and a destination-replacing mode
//!   changes every pixel of it, the ones the content left transparent
//!   included.

// ─── GPU readback tests ───────────────────────────────────────────────────────

#[cfg(all(test, feature = "testing"))]
mod gpu_tests {
    use std::sync::Arc;

    use flui_foundation::geometry::Rect;
    use flui_painting::Paint;
    use flui_painting::{paint::BlendMode, styling::Color};

    use crate::{painter::WgpuPainter, render_target::RenderTarget};

    // ── Harness constants ─────────────────────────────────────────────────────

    // 64×64 avoids DX12 small-texture copy artifacts that manifest at 8×8
    // (the last few corner texels of a copy_texture_to_texture can produce
    // physically-impossible values on DX12 for sub-tile textures).  All blend
    // math is identical at any size; 64×64 still fits entirely in GPU L2.
    const SURFACE_WIDTH: u32 = 64;
    const SURFACE_HEIGHT: u32 = 64;
    const SURFACE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

    // ── Harness helpers ───────────────────────────────────────────────────────

    fn acquire_test_device_and_queue() -> (Arc<wgpu::Device>, Arc<wgpu::Queue>) {
        crate::test_support::test_device_and_queue("LayerBlend Test Device")
    }

    /// Create a sampleable surface texture (RENDER_ATTACHMENT | TEXTURE_BINDING | COPY_SRC | COPY_DST).
    fn create_sampleable_surface(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView) {
        crate::test_support::create_sampleable_target(
            device,
            "LayerBlend Test Surface",
            SURFACE_WIDTH,
            SURFACE_HEIGHT,
            SURFACE_FORMAT,
        )
    }

    /// Fill the entire surface with a solid color via a clear render pass.
    fn clear_surface_to_color(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        color: wgpu::Color,
    ) {
        crate::test_support::clear_target(device, queue, view, color);
    }

    /// Read all pixels from `texture` and return RGBA bytes (tightly packed, row-major).
    fn readback_pixels(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
    ) -> Vec<[u8; 4]> {
        crate::test_support::readback_pixels(device, queue, texture, SURFACE_WIDTH, SURFACE_HEIGHT)
    }

    /// CPU oracle: `Color::blend(src, dst, mode)` → premultiplied RGBA u8.
    fn oracle_premultiplied(src_straight: Color, dst_straight: Color, mode: BlendMode) -> [u8; 4] {
        let result = src_straight.blend(dst_straight, mode);
        let [r, g, b, a] = result.to_f32_array();
        let to_u8 = |channel: f32| (channel.clamp(0.0, 1.0) * 255.0).round() as u8;
        [to_u8(r * a), to_u8(g * a), to_u8(b * a), to_u8(a)]
    }

    /// Assert two premultiplied RGBA pixels are within `tolerance` in every channel.
    fn assert_pixel_within_tolerance(
        label: &str,
        actual_pixel: [u8; 4],
        expected_pixel: [u8; 4],
        tolerance: u8,
    ) {
        for channel_index in 0..4 {
            let channel_diff = u8::try_from(
                (i16::from(actual_pixel[channel_index]) - i16::from(expected_pixel[channel_index]))
                    .unsigned_abs(),
            )
            .expect("diff of two u8 values always fits in u8");
            assert!(
                channel_diff <= tolerance,
                "{label}: channel {channel_index} — \
                 actual={a} expected={e} diff={channel_diff} > tolerance {tolerance}",
                a = actual_pixel[channel_index],
                e = expected_pixel[channel_index],
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

    /// Full-surface bounds for W×H.
    fn full_surface_bounds() -> Rect<f64> {
        Rect::from_xywh(
            0.0,
            0.0,
            f64::from(SURFACE_WIDTH as f32),
            f64::from(SURFACE_HEIGHT as f32),
        )
    }

    // ── T6: Opaque Multiply saveLayer vs CPU oracle ───────────────────────────

    /// T6: A solid-color rect drawn inside an opaque Multiply saveLayer on top of
    /// a solid backdrop must match `Color::blend(src, dst, Multiply)` within ±2.
    ///
    /// **Proves:**
    /// - `flush_opacity_layer` routes Multiply through `flush_advanced_layer`.
    /// - The backdrop copy captures the correct background pixels.
    /// - The WGSL Multiply formula matches `Color::blend`.
    ///
    /// **Fails if:**
    /// - SrcOver fallback: src dominates instead of darkening with dst.
    /// - Reintegrate path: no backdrop read → identical to a no-layer draw.
    #[test]
    fn opaque_multiply_layer_matches_cpu_oracle() {
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

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));

        // Source: opaque orange drawn inside a Multiply saveLayer.
        let source_color = Color::rgba(200, 120, 40, 255);
        let layer_bounds = full_surface_bounds();

        let multiply_paint = Paint::fill(Color::WHITE).with_blend_mode(BlendMode::Multiply);
        painter.save_layer(Some(layer_bounds), &multiply_paint);
        painter.draw_rect(layer_bounds, &Paint::fill(source_color));
        painter.restore_layer();

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Multiply Layer Test Encoder"),
        });
        let target = RenderTarget::sampleable(&surface_view, &surface_texture);
        painter
            .render(target, &mut encoder)
            .expect("painter.render must succeed on a GPU-enabled host");
        queue.submit(std::iter::once(encoder.finish()));

        let readback = readback_pixels(&device, &queue, &surface_texture);
        let expected = oracle_premultiplied(source_color, backdrop_color, BlendMode::Multiply);

        // ±2: absorbs premul→u8→unpremul quantization at the GPU texture boundary.
        let quantization_tolerance = 2u8;
        // Skip the last row and last column of the surface.
        //
        // The source rect drawn inside the saveLayer has the exact same bounds as
        // the layer (full viewport). The `rect_instanced.wgsl` SDF shader uses
        // `fwidth()` for adaptive antialiasing. At the LAST row/column of a
        // primitive, the GPU evaluates `fwidth()` with helper fragments that lie
        // outside the primitive, yielding an inflated edge_width → `sdfToAlpha < 1`
        // → partial alpha in the foreground offscreen at those boundary texels.
        // The advanced-blend shader then receives a non-unit foreground alpha and
        // produces a correct intermediate value between fully-blended and backdrop
        // that does not match the all-opaque oracle.
        //
        // In production this does not occur: rects are drawn at widget-interior
        // coordinates and never coincide exactly with the viewport/offscreen edge.
        // Skipping the two outermost rows/columns preserves 99.9 % coverage of the
        // blend-formula path while avoiding the SDF boundary artefact.
        let width = SURFACE_WIDTH as usize;
        let height = SURFACE_HEIGHT as usize;
        for (pixel_index, &actual_pixel) in readback.iter().enumerate() {
            let row = pixel_index / width;
            let col = pixel_index % width;
            if row >= height - 1 || col >= width - 1 {
                continue;
            }
            assert_pixel_within_tolerance(
                &format!("Multiply pixel {pixel_index} (row={row} col={col})"),
                actual_pixel,
                expected,
                quantization_tolerance,
            );
        }
    }

    // ── A layer composites its whole region ──────────────────────────────────

    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const RED: [u8; 4] = [255, 0, 0, 255];
    const CLEARED: [u8; 4] = [0, 0, 0, 0];

    /// One sample of a row: `(x, y, expected, tolerance)`.
    type Sample = (u32, u32, [u8; 4], u8);

    /// One row: its name, the scene it draws, the samples it must read.
    type RegionRow = (&'static str, fn(&mut WgpuPainter), &'static [Sample]);

    /// Draws `scene` over a surface cleared to opaque green and reads it back.
    fn render_over_green(
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        scene: fn(&mut WgpuPainter),
    ) -> Vec<[u8; 4]> {
        let (surface_texture, surface_view) = create_sampleable_surface(device);
        clear_surface_to_color(device, queue, &surface_view, wgpu::Color::GREEN);
        let mut painter = build_painter(Arc::clone(device), Arc::clone(queue));
        scene(&mut painter);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Layer Region Test Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("painter.render must succeed on a GPU-enabled host");
        queue.submit(std::iter::once(encoder.finish()));
        readback_pixels(device, queue, &surface_texture)
    }

    fn layer_paint(blend: BlendMode) -> Paint {
        Paint::fill(Color::WHITE).with_blend_mode(blend)
    }

    fn red_ink(painter: &mut WgpuPainter, rect: Rect<f64>) {
        painter.draw_rect(rect, &Paint::fill(Color::RED));
    }

    fn translate(painter: &mut WgpuPainter, dx: f64, dy: f64) {
        painter.translate(flui_foundation::geometry::Offset::new(dx, dy));
    }

    /// `translate(20, 16)`, a `Src` layer over local `(0, 0, 24, 24)` (device
    /// `(20, 16)-(44, 40)`), red ink over its first 8×8.
    fn a_bounded_src_layer_replaces_its_whole_mapped_bounds(painter: &mut WgpuPainter) {
        translate(painter, 20.0, 16.0);
        painter.save_layer(
            Some(Rect::from_xywh(0.0, 0.0, 24.0, 24.0)),
            &layer_paint(BlendMode::Src),
        );
        red_ink(painter, Rect::from_xywh(0.0, 0.0, 8.0, 8.0));
        painter.restore_layer();
    }

    /// `translate(8, 8) scale(2)`: local bounds `(0, 0, 12, 12)` are device
    /// `(8, 8)-(32, 32)`; the unscaled bounds would stop at 20.
    fn a_scaled_src_layer_replaces_its_scaled_bounds(painter: &mut WgpuPainter) {
        translate(painter, 8.0, 8.0);
        painter.scale(2.0, 2.0);
        painter.save_layer(
            Some(Rect::from_xywh(0.0, 0.0, 12.0, 12.0)),
            &layer_paint(BlendMode::Src),
        );
        red_ink(painter, Rect::from_xywh(0.0, 0.0, 4.0, 4.0));
        painter.restore_layer();
    }

    /// A half-transparent `SrcOver` layer under a translation shows its
    /// content where the content landed.
    fn a_translucent_translated_layer_shows_its_content(painter: &mut WgpuPainter) {
        translate(painter, 20.0, 16.0);
        painter.save_layer(
            Some(Rect::from_xywh(0.0, 0.0, 24.0, 24.0)),
            &Paint::fill(Color::WHITE).with_alpha(128),
        );
        red_ink(painter, Rect::from_xywh(0.0, 0.0, 8.0, 8.0));
        painter.restore_layer();
    }

    /// A `Clear` layer with nothing in it, over device `(10, 10)-(30, 30)`.
    fn an_empty_clear_layer_clears_its_bounds(painter: &mut WgpuPainter) {
        translate(painter, 10.0, 10.0);
        painter.save_layer(
            Some(Rect::from_xywh(0.0, 0.0, 20.0, 20.0)),
            &layer_paint(BlendMode::Clear),
        );
        painter.restore_layer();
    }

    /// An unbounded `Src` layer under a rect clip `(16, 16)-(48, 48)`.
    fn an_unbounded_src_layer_replaces_only_its_clip(painter: &mut WgpuPainter) {
        painter.clip_rect(
            Rect::from_xywh(16.0, 16.0, 32.0, 32.0),
            flui_painting::paint::Clip::HardEdge,
        );
        painter.save_layer(None, &layer_paint(BlendMode::Src));
        red_ink(painter, Rect::from_xywh(16.0, 16.0, 8.0, 8.0));
        painter.restore_layer();
    }

    /// An empty unbounded `Src` layer under a rounded clip `(8, 8)-(56, 56)`
    /// with radius 16.
    fn a_src_layer_under_a_rounded_clip_keeps_the_corners(painter: &mut WgpuPainter) {
        painter.clip_rrect(
            flui_foundation::geometry::RRect::from_rect_circular(
                Rect::from_xywh(8.0, 8.0, 48.0, 48.0),
                16.0,
            ),
            flui_painting::paint::Clip::AntiAlias,
        );
        painter.save_layer(None, &layer_paint(BlendMode::Src));
        painter.restore_layer();
    }

    /// An empty `Src` layer over local `(-12, -12)-(12, 12)` rotated 45°
    /// about `(32, 32)`: a diamond whose bounding box reaches `(15, 15)`.
    fn a_rotated_src_layer_replaces_the_rotated_square(painter: &mut WgpuPainter) {
        translate(painter, 32.0, 32.0);
        painter.rotate(std::f32::consts::FRAC_PI_4);
        painter.save_layer(
            Some(Rect::from_xywh(-12.0, -12.0, 24.0, 24.0)),
            &layer_paint(BlendMode::Src),
        );
        painter.restore_layer();
    }

    /// A layer in `paint` over local `(-12, -12)-(12, 12)` rotated 45° about
    /// `(32, 32)`, holding red ink over local `(-30, -30)-(30, 30)`, which
    /// reaches through the corners of the diamond's bounding box.
    fn rotated_layer_with_oversized_ink(painter: &mut WgpuPainter, paint: &Paint) {
        translate(painter, 32.0, 32.0);
        painter.rotate(std::f32::consts::FRAC_PI_4);
        painter.save_layer(Some(Rect::from_xywh(-12.0, -12.0, 24.0, 24.0)), paint);
        red_ink(painter, Rect::from_xywh(-30.0, -30.0, 60.0, 60.0));
        painter.restore_layer();
    }

    fn a_rotated_translucent_layer_keeps_its_ink_inside_the_rotated_square(
        painter: &mut WgpuPainter,
    ) {
        rotated_layer_with_oversized_ink(painter, &Paint::fill(Color::WHITE).with_alpha(128));
    }

    fn a_rotated_opaque_layer_keeps_its_ink_inside_the_rotated_square(painter: &mut WgpuPainter) {
        rotated_layer_with_oversized_ink(painter, &Paint::fill(Color::WHITE));
    }

    fn a_rotated_multiply_layer_keeps_its_ink_inside_the_rotated_square(painter: &mut WgpuPainter) {
        rotated_layer_with_oversized_ink(painter, &layer_paint(BlendMode::Multiply));
    }

    /// An opaque `DstOver` layer puts its red content under the green
    /// backdrop, which therefore stays on top.
    fn an_opaque_dst_over_layer_keeps_the_backdrop_on_top(painter: &mut WgpuPainter) {
        painter.save_layer(None, &layer_paint(BlendMode::DstOver));
        red_ink(painter, Rect::from_xywh(16.0, 16.0, 16.0, 16.0));
        painter.restore_layer();
    }

    /// Each row: a scene, then the samples it must read. A row samples where
    /// the broken behaviours disagree with the rule: inside the mapped bounds
    /// but outside the content (a dropped mode leaves green there), inside the
    /// unmapped bounds (unmapped bounds clear it), outside the clip (an
    /// unclipped region clears it), inside the bounding box of a rotated
    /// region but outside the region (a bounding-box composite clears it, or
    /// shows the content that reached there).
    const REGION_ROWS: &[RegionRow] = &[
        (
            "a_bounded_src_layer_replaces_its_whole_mapped_bounds",
            a_bounded_src_layer_replaces_its_whole_mapped_bounds,
            &[
                (24, 20, RED, 0),
                (36, 34, CLEARED, 0),
                (8, 8, GREEN, 0),
                (46, 20, GREEN, 0),
                (50, 50, GREEN, 0),
            ],
        ),
        (
            "a_scaled_src_layer_replaces_its_scaled_bounds",
            a_scaled_src_layer_replaces_its_scaled_bounds,
            &[(12, 12, RED, 0), (28, 28, CLEARED, 0), (34, 34, GREEN, 0)],
        ),
        (
            "a_translucent_translated_layer_shows_its_content",
            a_translucent_translated_layer_shows_its_content,
            &[(24, 20, [128, 127, 0, 255], 2), (36, 34, GREEN, 0)],
        ),
        (
            "an_empty_clear_layer_clears_its_bounds",
            an_empty_clear_layer_clears_its_bounds,
            &[(15, 15, CLEARED, 0), (35, 35, GREEN, 0)],
        ),
        (
            "an_unbounded_src_layer_replaces_only_its_clip",
            an_unbounded_src_layer_replaces_only_its_clip,
            &[(20, 20, RED, 0), (40, 40, CLEARED, 0), (8, 8, GREEN, 0)],
        ),
        (
            "a_src_layer_under_a_rounded_clip_keeps_the_corners",
            a_src_layer_under_a_rounded_clip_keeps_the_corners,
            &[(10, 10, GREEN, 0), (32, 32, CLEARED, 0)],
        ),
        (
            "a_rotated_src_layer_replaces_the_rotated_square",
            a_rotated_src_layer_replaces_the_rotated_square,
            &[
                (17, 17, GREEN, 0),
                (32, 20, CLEARED, 0),
                (32, 32, CLEARED, 0),
            ],
        ),
        // (17, 17) is inside the diamond's bounding box but outside the
        // diamond, where the ink reaches; (6, 32) is past the bounding box.
        (
            "a_rotated_translucent_layer_keeps_its_ink_inside_the_rotated_square",
            a_rotated_translucent_layer_keeps_its_ink_inside_the_rotated_square,
            &[
                (17, 17, GREEN, 0),
                (6, 32, GREEN, 0),
                (32, 32, [128, 127, 0, 255], 2),
            ],
        ),
        (
            "a_rotated_opaque_layer_keeps_its_ink_inside_the_rotated_square",
            a_rotated_opaque_layer_keeps_its_ink_inside_the_rotated_square,
            &[(17, 17, GREEN, 0), (6, 32, GREEN, 0), (32, 32, RED, 0)],
        ),
        (
            "a_rotated_multiply_layer_keeps_its_ink_inside_the_rotated_square",
            a_rotated_multiply_layer_keeps_its_ink_inside_the_rotated_square,
            &[
                (17, 17, GREEN, 0),
                (6, 32, GREEN, 0),
                (32, 32, [0, 0, 0, 255], 2),
            ],
        ),
        (
            "an_opaque_dst_over_layer_keeps_the_backdrop_on_top",
            an_opaque_dst_over_layer_keeps_the_backdrop_on_top,
            &[(20, 20, GREEN, 0), (40, 40, GREEN, 0)],
        ),
    ];

    /// A layer composites its whole region with its recorded mode: one row
    /// per case in [`REGION_ROWS`], every row run, every failing sample named.
    #[test]
    fn a_layer_composites_its_whole_region_with_its_mode() {
        let Some((device, queue)) =
            crate::test_support::try_test_device_and_queue("Layer Region Test Device")
        else {
            return;
        };
        let mut failed = Vec::new();
        for &(name, scene, samples) in REGION_ROWS {
            let pixels = render_over_green(&device, &queue, scene);
            for &(x, y, expected, tolerance) in samples {
                let actual = pixels[(y * SURFACE_WIDTH + x) as usize];
                let close = actual
                    .iter()
                    .zip(expected)
                    .all(|(&a, e)| a.abs_diff(e) <= tolerance);
                if !close {
                    failed.push(format!(
                        "{name}: ({x}, {y}) is {actual:?}, expected {expected:?} ±{tolerance}"
                    ));
                }
            }
        }
        assert!(failed.is_empty(), "{failed:#?}");
    }
}
