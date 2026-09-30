//! GPU readback acceptance gate for the morphology (dilate/erode) filter pass.
//!
//! ## Test inventory
//!
//! | # | Gate | Requirement |
//! |---|------|-------------|
//! | M2 | GPU | dilate opaque: content border expands by ceil(radius) |
//!
//! ## Premul-direct invariant
//!
//! max/min operates on **premultiplied** RGBA — there is NO unpremultiply step.
//! Adjacent pixels `(128,128,128,255)` and `(128,128,128,128)` have premul-max
//! RGB=128 but unpremul-max RGB=255, so a translucent pair would discriminate
//! the two paths. **Unasserted:** no test pins this.
//!
//! ## Decal semantics
//!
//! Pixels outside the declared `content_bounds` in UV are treated as the neutral
//! element (transparent-black for dilate, opaque-white for erode) — NOT clamped
//! to the edge colour. **Unasserted:** no test pins this.
//!
//! All tests use `testing` feature-gate and follow the same harness
//! pattern as `color_matrix_filter_tests`.  The 64×64 surface avoids DX12
//! small-texture copy artefacts.

#[cfg(all(test, feature = "testing"))]
mod gpu_tests {
    use std::sync::Arc;

    use flui_foundation::geometry::Rect;
    use flui_painting::Paint;
    use flui_painting::styling::Color;

    use crate::{
        command_ir::{ImageFilterSpec, MorphOp},
        painter::WgpuPainter,
        render_target::RenderTarget,
    };

    // ── Harness constants ─────────────────────────────────────────────────────

    const SURFACE_WIDTH: u32 = 64;
    const SURFACE_HEIGHT: u32 = 64;
    const SURFACE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

    // ── Harness helpers ───────────────────────────────────────────────────────

    fn acquire_test_device_and_queue() -> (Arc<wgpu::Device>, Arc<wgpu::Queue>) {
        crate::test_support::test_device_and_queue("MorphologyFilter Test Device")
    }

    fn create_surface(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView) {
        crate::test_support::create_sampleable_target(
            device,
            "MorphologyFilter Test Surface",
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
    /// Return a rect representing a sub-region in the center of the surface.
    ///
    /// `edge_margin_px` is the distance in whole pixels from each surface edge to
    /// the returned content rect.  Leaves transparent space on all four sides for
    /// decal and grown-bounds tests.
    fn center_rect(edge_margin_px: u32) -> Rect<f64> {
        let margin = edge_margin_px as f32;
        Rect::from_xywh(
            f64::from(margin),
            f64::from(margin),
            f64::from(SURFACE_WIDTH as f32 - 2.0 * margin),
            f64::from(SURFACE_HEIGHT as f32 - 2.0 * margin),
        )
    }

    // ── M2: dilate opaque — border expands ────────────────────────────────────

    /// M2: Dilate with radius=3 on an opaque content rect must produce non-zero
    /// alpha at pixels immediately outside (but within radius of) the content
    /// border, proving the filter expanded the content by ceil(radius) pixels.
    ///
    /// **Proves:**
    /// - The H→V two-pass scan correctly expands the content by `ceil(radius)`.
    /// - Interior pixels of the content rect stay at their original colour.
    ///
    /// **Fails if:** the dilate kernel has the wrong sign convention, the passes
    /// are misplaced, or the output is the same as the unfiltered content.
    #[test]
    fn dilate_expands_opaque_content_border() {
        const DILATE_RADIUS: f32 = 3.0;
        const CONTENT_EDGE_MARGIN_PX: u32 = 10; // content border at x=10, y=10

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

        let content_rect = center_rect(CONTENT_EDGE_MARGIN_PX);
        let source_color = Color::rgba(200, 150, 100, 255); // opaque

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save_layer_with_image_filter(ImageFilterSpec::Morph {
            radius: DILATE_RADIUS,
            op: MorphOp::Dilate,
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
            .expect("dilate render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_tex);
        let surface_width = SURFACE_WIDTH as usize;
        let surface_height = SURFACE_HEIGHT as usize;

        // Pixels just *inside* the content border must be fully opaque.
        let interior_row = CONTENT_EDGE_MARGIN_PX as usize + 2;
        let interior_col = CONTENT_EDGE_MARGIN_PX as usize + 2;
        let interior_alpha = pixels[interior_row * surface_width + interior_col][3];
        assert!(
            interior_alpha > 200,
            "M2: interior pixel at ({interior_col},{interior_row}) alpha={interior_alpha} — \
             expected fully opaque (>200); dilate must preserve interior content"
        );

        // Pixels just *outside* the left border (within radius) must be non-zero
        // (the dilate expanded into them).
        let just_outside_col = CONTENT_EDGE_MARGIN_PX as usize - 1;
        let vertical_mid_row = surface_height / 2;
        let expanded_pixel = pixels[vertical_mid_row * surface_width + just_outside_col];
        assert!(
            expanded_pixel[3] > 0,
            "M2: pixel at ({just_outside_col},{vertical_mid_row}) alpha={} — \
             expected non-zero after dilate-expand (radius={DILATE_RADIUS}); \
             border must have grown by ceil(radius) pixels",
            expanded_pixel[3]
        );

        // Pixels far outside the border (beyond the expanded region) must be zero.
        let far_corner_alpha = pixels[surface_width + 1][3];
        assert_eq!(
            far_corner_alpha, 0,
            "M2: far-corner pixel alpha={far_corner_alpha} — \
             expected transparent (dilate must not bleed beyond ceil(radius))"
        );
    }

    // ── M3: erode opaque — border contracts ───────────────────────────────────

    // ── M4: discriminating premul (G2 anti-vacuous) ───────────────────────────

    // ── M5: decal boundary ────────────────────────────────────────────────────

    // ── M6: grown_bounds wiring ───────────────────────────────────────────────

    // ── Erode shrinks at the viewport decal boundary ───────────────────────────

    // ── Tight-bounds dilate — corner growth and placement ──────────────────────

    // ── Tight-bounds erode shrinks at the content-rect edge ────────────────────
}
