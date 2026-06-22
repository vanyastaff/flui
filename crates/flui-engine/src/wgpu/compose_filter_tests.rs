//! GPU readback acceptance gate for `ImageFilter::Compose` flatten + Chain execution.
//!
//! ## Test inventory
//!
//! | # | Gate | Requirement |
//! |---|------|-------------|
//! | C1 | GPU  | Order-matters discriminator: `[alpha→R, Blur]` vs `[Blur, alpha→R]` — readbacks DIFFER at a named halo pixel + ABSOLUTE interior assertion |
//! | C2 | CPU  | Flatten-nesting: nested Compose ASTs flatten to the correct flat pass sequence |
//! | C3 | GPU  | Deep-chain (6 passes, >4 SmallVec inline capacity): heap-spill + correct GPU readback vs hand-derived oracle |
//! | C4 | CPU  | Cumulative bounds: `[Blur(σ=4), Matrix, Blur(σ=4)]` grows by 2×kernel_radius(4); `[Matrix, Matrix]` grows by 0 |
//! | C5 | CPU  | Empty/degenerate: `Compose([])` → no FilterOp; `Compose([Matrix])` → one ColorMatrix pass, zero growth |
//!
//! ## Order convention (PINNED #4)
//!
//! `flatten_compose` in `backend.rs` maps `Compose(Vec)` left-to-right (index 0 = innermost
//! = applied first), faithfully matching Flutter `dl_compose_image_filter.cc:33–51`.
//! `restore_layer` applies `FilterOp::passes` in index order.
//!
//! ## C1 discriminator — why alpha→R and Blur do NOT commute
//!
//! `colormatrix ∘ blur ≠ blur ∘ colormatrix` when the matrix reads the alpha channel
//! AND the blur creates halo pixels with partial alpha.
//!
//! At a halo pixel with partial alpha `a_partial ∈ (0, 1)`:
//!
//! - **Order A** (alpha→R inner, Blur outer): source is opaque (A=1) inside the content
//!   rect.  The alpha→R matrix maps (R=1,G=1,B=1,A=1) → (R=A=1,G=0,B=0,A=1) — opaque red.
//!   Then Blur produces a partial-alpha red halo.  Premul halo: R = a_partial × 255.
//!
//! - **Order B** (Blur inner, alpha→R outer): source is opaque white.  Blur creates a
//!   partial-alpha white halo premul = (a_partial,a_partial,a_partial,a_partial)×255.
//!   Then alpha→R matrix: unpremul halo → straight (1,1,1,a_partial); matrix applies
//!   R_out=A_in=a_partial → repremul → premul R = a_partial² × 255.
//!
//! At halo pixels, a_partial ∈ (0,1), so a_partial > a_partial² → Order A R > Order B R.
//! The test asserts `halo_a.R > halo_b.R` at a named pixel inside the halo.

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod gpu_tests {
    use std::sync::Arc;

    use flui_painting::Paint;
    use flui_types::{Color, Rect, geometry::Pixels};
    use smallvec::smallvec;

    use crate::wgpu::{
        command_ir::{ImageFilterPass, ImageFilterSpec, MorphOp},
        effects::kernel_radius,
        painter::WgpuPainter,
        render_target::RenderTarget,
    };

    // ── Harness constants ─────────────────────────────────────────────────────

    const SURFACE_W: u32 = 64;
    const SURFACE_H: u32 = 64;
    const SURFACE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

    // ── Harness helpers ───────────────────────────────────────────────────────

    fn acquire_device_and_queue() -> (Arc<wgpu::Device>, Arc<wgpu::Queue>) {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .expect("GPU adapter must be available for compose_filter_tests");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("ComposeFilter Test Device"),
            ..Default::default()
        }))
        .expect("GPU device must be available for compose_filter_tests");
        (Arc::new(device), Arc::new(queue))
    }

    fn create_surface(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ComposeFilter Test Surface"),
            size: wgpu::Extent3d {
                width: SURFACE_W,
                height: SURFACE_H,
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
            label: Some("ComposeFilter Surface Clear"),
        });
        {
            let _clear_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ComposeFilter Clear Pass"),
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
        let unpadded_row_bytes = SURFACE_W * bytes_per_pixel;
        let row_alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_row_bytes = unpadded_row_bytes.div_ceil(row_alignment) * row_alignment;
        let staging_size = u64::from(padded_row_bytes * SURFACE_H);

        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ComposeFilter Readback Staging"),
            size: staging_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ComposeFilter Readback Encoder"),
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
                    rows_per_image: Some(SURFACE_H),
                },
            },
            wgpu::Extent3d {
                width: SURFACE_W,
                height: SURFACE_H,
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
        let pixel_count = (SURFACE_W * SURFACE_H) as usize;
        let mut pixels = Vec::with_capacity(pixel_count);
        for row in 0..SURFACE_H {
            let row_start = (row * padded_row_bytes) as usize;
            for col in 0..SURFACE_W {
                let byte_offset = row_start + col as usize * 4;
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
        WgpuPainter::with_shared_device(device, queue, SURFACE_FORMAT, (SURFACE_W, SURFACE_H))
    }

    fn px(v: f32) -> Pixels {
        Pixels(v)
    }

    fn full_surface_rect() -> Rect<Pixels> {
        Rect::from_xywh(px(0.0), px(0.0), px(SURFACE_W as f32), px(SURFACE_H as f32))
    }

    // ── CPU oracle helpers ────────────────────────────────────────────────────

    /// Apply a 5×4 color matrix to a straight-RGBA pixel and return the premul u8 output.
    ///
    /// Mirrors the WGSL shader in `color_matrix.wgsl`:
    /// 1. Treat input as straight-alpha.
    /// 2. `output = M × straight + offset`, clamped per-channel to `[0, 1]`.
    /// 3. Repremultiply: `(r×a, g×a, b×a, a)`.
    /// 4. Quantise via `round(x × 255)`.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "values are clamped to [0,1] and multiplied by 255 before rounding; fits u8"
    )]
    fn color_matrix_oracle(matrix: &[f32; 20], straight_rgba: [f32; 4]) -> [u8; 4] {
        let m = matrix;
        let [sr, sg, sb, sa] = straight_rgba;
        let out_r = (m[0] * sr + m[1] * sg + m[2] * sb + m[3] * sa + m[4]).clamp(0.0, 1.0);
        let out_g = (m[5] * sr + m[6] * sg + m[7] * sb + m[8] * sa + m[9]).clamp(0.0, 1.0);
        let out_b = (m[10] * sr + m[11] * sg + m[12] * sb + m[13] * sa + m[14]).clamp(0.0, 1.0);
        let out_a = (m[15] * sr + m[16] * sg + m[17] * sb + m[18] * sa + m[19]).clamp(0.0, 1.0);
        let to_u8 = |x: f32| (x * 255.0).round() as u8;
        [
            to_u8(out_r * out_a),
            to_u8(out_g * out_a),
            to_u8(out_b * out_a),
            to_u8(out_a),
        ]
    }

    // ── Shared filter matrices ────────────────────────────────────────────────

    /// R↔B channel-swap: rows 0/2 are transposed; G and A pass through.
    ///
    /// ```text
    /// row 0 (R_out = B_in): [0, 0, 1, 0, 0]
    /// row 1 (G_out = G_in): [0, 1, 0, 0, 0]
    /// row 2 (B_out = R_in): [1, 0, 0, 0, 0]
    /// row 3 (A_out = A_in): [0, 0, 0, 1, 0]
    /// ```
    #[rustfmt::skip]
    fn swap_rb_matrix() -> [f32; 20] {
        [
            0.0, 0.0, 1.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0, 0.0,
            1.0, 0.0, 0.0, 0.0, 0.0,
            0.0, 0.0, 0.0, 1.0, 0.0,
        ]
    }

    /// Identity: each output channel equals the corresponding input channel.
    #[rustfmt::skip]
    fn identity_matrix() -> [f32; 20] {
        [
            1.0, 0.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 0.0, 1.0, 0.0,
        ]
    }

    /// Copies alpha into the R channel; zeros G and B; passes A through.
    ///
    /// ```text
    /// row 0 (R_out = A_in): [0, 0, 0, 1, 0]
    /// row 1 (G_out = 0):    [0, 0, 0, 0, 0]
    /// row 2 (B_out = 0):    [0, 0, 0, 0, 0]
    /// row 3 (A_out = A_in): [0, 0, 0, 1, 0]
    /// ```
    ///
    /// Used as the C1 discriminating matrix because it reads alpha: when applied
    /// BEFORE a Gaussian blur, the halo pixel red value is `a_partial`, whereas
    /// when applied AFTER the blur (where halo pixels have `A = a_partial`), the
    /// unpremul step scales R by `1/a_partial`, so the matrix output R = `a_partial`
    /// repremultiplied becomes `a_partial²` — a strictly smaller value.
    #[rustfmt::skip]
    fn alpha_to_red_matrix() -> [f32; 20] {
        [
            0.0, 0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 0.0, 0.0,
            0.0, 0.0, 0.0, 0.0, 0.0,
            0.0, 0.0, 0.0, 1.0, 0.0,
        ]
    }

    // ── C1: Order-matters — discriminator + ABSOLUTE ──────────────────────────

    /// C1: The Compose pass order is faithfully preserved end-to-end on the GPU.
    ///
    /// A 20-px-margin content rect of opaque white is rendered through two chains:
    ///
    /// - **Order A** (inner=alpha→R, outer=Blur): alpha→R converts the opaque-white
    ///   interior to opaque red (A=1 → R=1 in straight). Blur then creates a partial-alpha
    ///   red halo. Halo premul R = `a_partial × 255`.
    ///
    /// - **Order B** (inner=Blur, outer=alpha→R): Blur creates a partial-alpha white halo
    ///   first. Then alpha→R unpremultiplies (straight R=1, A=a_partial), applies the matrix
    ///   (R_out = A_in = a_partial), repremultiplies → halo premul R = `a_partial² × 255`.
    ///
    /// Since `a_partial ∈ (0, 1)` at halo pixels, `a_partial > a_partial²`, so Order A
    /// produces a strictly higher R at the halo pixel.
    ///
    /// The absolute assertion checks that Order A interior is opaque red (R≈255, G≈0, A≈255).
    ///
    /// Fails if the chain passes are applied in the wrong order, or if the ColorMatrix
    /// pass is skipped.
    #[test]
    fn compose_order_matters_alpha_to_red_then_blur_vs_blur_then_alpha_to_red() {
        const CONTENT_MARGIN_PX: f32 = 20.0;
        // σ=4 → kernel_radius=7 px halo; halo pixel sampled 3 px outside the content rect.
        const BLUR_SIGMA: f32 = 4.0;

        let alpha_to_red = alpha_to_red_matrix();

        let (device, queue) = acquire_device_and_queue();
        let (tex_order_a, view_order_a) = create_surface(&device);
        let (tex_order_b, view_order_b) = create_surface(&device);
        let transparent_black = wgpu::Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        };
        clear_surface(&device, &queue, &view_order_a, transparent_black);
        clear_surface(&device, &queue, &view_order_b, transparent_black);

        let content_rect = Rect::from_xywh(
            px(CONTENT_MARGIN_PX),
            px(CONTENT_MARGIN_PX),
            px(SURFACE_W as f32 - 2.0 * CONTENT_MARGIN_PX),
            px(SURFACE_H as f32 - 2.0 * CONTENT_MARGIN_PX),
        );
        let opaque_white = Color::rgba(255, 255, 255, 255);

        // Order A: inner=alpha→R applied first; outer=Blur applied second.
        let mut encoder_a =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            painter.save_layer_with_image_filter(ImageFilterSpec::Chain(smallvec![
                ImageFilterPass::ColorMatrix(alpha_to_red),
                ImageFilterPass::Blur {
                    sigma_x: BLUR_SIGMA,
                    sigma_y: BLUR_SIGMA
                },
            ]));
            painter.rect(content_rect, &Paint::fill(opaque_white));
            painter.restore_layer();
            painter
                .render(
                    RenderTarget::sampleable(&view_order_a, &tex_order_a),
                    &mut encoder_a,
                )
                .expect("C1 Order-A (alpha→R inner, Blur outer) render must succeed");
        }
        queue.submit(std::iter::once(encoder_a.finish()));

        // Order B: inner=Blur applied first; outer=alpha→R applied second.
        let mut encoder_b =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            painter.save_layer_with_image_filter(ImageFilterSpec::Chain(smallvec![
                ImageFilterPass::Blur {
                    sigma_x: BLUR_SIGMA,
                    sigma_y: BLUR_SIGMA
                },
                ImageFilterPass::ColorMatrix(alpha_to_red),
            ]));
            painter.rect(content_rect, &Paint::fill(opaque_white));
            painter.restore_layer();
            painter
                .render(
                    RenderTarget::sampleable(&view_order_b, &tex_order_b),
                    &mut encoder_b,
                )
                .expect("C1 Order-B (Blur inner, alpha→R outer) render must succeed");
        }
        queue.submit(std::iter::once(encoder_b.finish()));

        let pixels_a = readback_pixels(&device, &queue, &tex_order_a);
        let pixels_b = readback_pixels(&device, &queue, &tex_order_b);

        let row_stride = SURFACE_W as usize;

        // Named halo pixel: 3 px outside the left edge of the content rect, at mid-height.
        // Content rect left = CONTENT_MARGIN_PX (=20 px). Halo pixel col = 20-3 = 17.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "CONTENT_MARGIN_PX=20 and offset=3 are small positive consts; fits usize"
        )]
        let halo_col = CONTENT_MARGIN_PX as usize - 3;
        let halo_row = (SURFACE_H / 2) as usize;
        let halo_pixel_idx = halo_row * row_stride + halo_col;

        let halo_a = pixels_a[halo_pixel_idx];
        let halo_b = pixels_b[halo_pixel_idx];

        // ── Relational discriminator ──────────────────────────────────────────
        // At this halo pixel, Order A premul-R = a_partial × 255,
        // Order B premul-R = a_partial² × 255. Since 0 < a_partial < 1 at the halo,
        // Order A R > Order B R.
        assert!(
            halo_a[0] > halo_b[0],
            "C1 DISCRIMINATOR: halo pixel [{halo_col},{halo_row}] — \
             Order-A R={ar} should be > Order-B R={br}. \
             Order A (alpha→R inner): halo R = a_partial×255. \
             Order B (Blur inner): halo R = a_partial²×255. \
             If equal or inverted, the pass order is not preserved (PINNED #4 violation).",
            ar = halo_a[0],
            br = halo_b[0],
        );

        // Sanity: both pixels must have some red (not transparent black) to prove the
        // halo is actually inside the Gaussian halo extent and was touched by both paths.
        assert!(
            halo_a[0] > 0,
            "C1: Order-A halo pixel R=0 — halo pixel is outside the blur extent; \
             adjust halo_col to be within kernel_radius({BLUR_SIGMA}) px of the content edge"
        );
        assert!(
            halo_b[0] > 0,
            "C1: Order-B halo pixel R=0 — halo pixel is outside the blur extent; \
             adjust halo_col to be within kernel_radius({BLUR_SIGMA}) px of the content edge"
        );

        // ── Absolute assertion (Order A interior centre) ──────────────────────
        // At the centre of the content rect (well inside, alpha=1 everywhere):
        // alpha→R maps (R=1,G=1,B=1,A=1) → (R=1,G=0,B=0,A=1) opaque red.
        // Blur does not change interior pixels far from the edge.
        let centre_pixel_idx = (SURFACE_H / 2) as usize * row_stride + (SURFACE_W / 2) as usize;
        let centre_a = pixels_a[centre_pixel_idx];
        assert!(
            centre_a[0] >= 230,
            "C1 ABSOLUTE: Order-A centre R={r} expected ≥230. \
             alpha→R matrix maps opaque-white interior to opaque red; Blur preserves interior.",
            r = centre_a[0]
        );
        assert!(
            centre_a[1] <= 10,
            "C1 ABSOLUTE: Order-A centre G={g} expected ≤10 (alpha→R zeroes G).",
            g = centre_a[1]
        );
        assert!(
            centre_a[3] >= 230,
            "C1 ABSOLUTE: Order-A centre A={a} expected ≥230 (opaque source stays opaque).",
            a = centre_a[3]
        );
    }

    // ── C2: Flatten-nesting — CPU structural ─────────────────────────────────

    /// C2: `filter_ops_for_test` returns the correctly flattened pass sequence for
    /// two nested `Compose` AST shapes.
    ///
    /// Exercises `flatten_compose` in `backend.rs` via the painter IR record path
    /// and reads back `FilterOp::passes` via `filter_ops_for_test`.
    ///
    /// No `painter.render()` call — purely inspects in-memory IR.
    #[test]
    fn flatten_nested_compose_produces_correct_pass_sequence() {
        use flui_painting::display_list::ImageFilter;
        use flui_types::painting::ColorMatrix;

        let (device, queue) = acquire_device_and_queue();

        let sigma_blur = 2.0_f32;
        let dilate_radius = 3.0_f32;
        let matrix_values = identity_matrix();

        let blur_pass = ImageFilterPass::Blur {
            sigma_x: sigma_blur,
            sigma_y: sigma_blur,
        };
        let dilate_pass = ImageFilterPass::Morph {
            radius: dilate_radius,
            op: MorphOp::Dilate,
        };
        let matrix_pass = ImageFilterPass::ColorMatrix(matrix_values);

        let blur_filter = ImageFilter::Blur {
            sigma_x: sigma_blur,
            sigma_y: sigma_blur,
        };
        let dilate_filter = ImageFilter::Dilate {
            radius: dilate_radius,
        };
        let matrix_filter = ImageFilter::Matrix(ColorMatrix {
            values: matrix_values,
        });

        // ── Case 1: Compose([Compose([Blur]), Dilate]) → [Blur, Dilate] ──────
        {
            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            painter.push_compose_for_test(&[
                ImageFilter::Compose(vec![blur_filter.clone()]),
                dilate_filter.clone(),
            ]);
            painter.rect(
                full_surface_rect(),
                &Paint::fill(Color::rgba(128, 128, 128, 255)),
            );
            painter.pop_compose_for_test();

            let recorded_ops = painter.filter_ops_for_test();
            assert_eq!(
                recorded_ops.len(),
                1,
                "C2 case-1: expected exactly 1 FilterOp, got {n}",
                n = recorded_ops.len()
            );
            let passes = &recorded_ops[0].passes;
            assert_eq!(
                passes.as_slice(),
                &[blur_pass.clone(), dilate_pass.clone()],
                "C2 case-1: Compose([Compose([Blur]), Dilate]) should flatten to [Blur, Dilate]"
            );
        }

        // ── Case 2: Compose([Blur, Compose([Dilate, Matrix])]) → [Blur, Dilate, Matrix] ──
        {
            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            painter.push_compose_for_test(&[
                blur_filter.clone(),
                ImageFilter::Compose(vec![dilate_filter.clone(), matrix_filter.clone()]),
            ]);
            painter.rect(
                full_surface_rect(),
                &Paint::fill(Color::rgba(128, 128, 128, 255)),
            );
            painter.pop_compose_for_test();

            let recorded_ops = painter.filter_ops_for_test();
            assert_eq!(
                recorded_ops.len(),
                1,
                "C2 case-2: expected exactly 1 FilterOp, got {n}",
                n = recorded_ops.len()
            );
            let passes = &recorded_ops[0].passes;
            assert_eq!(
                passes.as_slice(),
                &[blur_pass, dilate_pass, matrix_pass],
                "C2 case-2: Compose([Blur, Compose([Dilate, Matrix])]) should flatten to \
                 [Blur, Dilate, Matrix]"
            );
        }
    }

    // ── C3: Deep-chain (6 passes, >4 inline, GPU readback) ───────────────────

    /// C3: A 6-pass chain exceeds `SmallVec<[ImageFilterPass; 4]>` inline capacity,
    /// forcing heap allocation.  The chain still executes correctly on the GPU.
    ///
    /// Chain: `[swap_rb, noop, swap_rb, noop, swap_rb, noop]` applied to opaque red.
    ///
    /// ## Hand-derived oracle
    ///
    /// Source: opaque red (straight R=200/255, G=0, B=0, A=1).
    ///
    /// Pass 0 (swap_rb): (R=200/255,G=0,B=0,A=1) → (R=0,G=0,B=200/255,A=1) — opaque blue.
    /// Pass 1 (noop):    identity                 → (R=0,G=0,B=200/255,A=1) unchanged.
    /// Pass 2 (swap_rb): (R=0,G=0,B=200/255,A=1) → (R=200/255,G=0,B=0,A=1) — opaque red.
    /// Pass 3 (noop):    identity                 → (R=200/255,G=0,B=0,A=1) unchanged.
    /// Pass 4 (swap_rb): (R=200/255,G=0,B=0,A=1) → (R=0,G=0,B=200/255,A=1) — opaque blue.
    /// Pass 5 (noop):    identity                 → (R=0,G=0,B=200/255,A=1) unchanged.
    ///
    /// Final oracle: premul (R=0, G=0, B=200, A=255).
    ///
    /// Three swap_rb passes (odd count) ⟹ net effect is one swap: R→B and B→R.
    #[test]
    fn deep_six_pass_chain_heap_spill_produces_oracle_pixel_output() {
        const SOURCE_RED: u8 = 200;
        let swap_rb = swap_rb_matrix();
        let noop = identity_matrix();

        let (device, queue) = acquire_device_and_queue();
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

        let six_passes: smallvec::SmallVec<[ImageFilterPass; 4]> = smallvec![
            ImageFilterPass::ColorMatrix(swap_rb), // pass 0: red → blue (odd)
            ImageFilterPass::ColorMatrix(noop),    // pass 1: identity
            ImageFilterPass::ColorMatrix(swap_rb), // pass 2: blue → red (even)
            ImageFilterPass::ColorMatrix(noop),    // pass 3: identity
            ImageFilterPass::ColorMatrix(swap_rb), // pass 4: red → blue (odd)
            ImageFilterPass::ColorMatrix(noop),    // pass 5: identity
        ];

        // SmallVec inline capacity is 4; 6 passes must spill to the heap.
        assert!(
            six_passes.spilled(),
            "C3: SmallVec with 6 passes should have spilled to heap (inline capacity = 4)"
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save_layer_with_image_filter(ImageFilterSpec::Chain(six_passes));
        painter.rect(
            full_surface_rect(),
            &Paint::fill(Color::rgba(SOURCE_RED, 0, 0, 255)),
        );
        painter.restore_layer();

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_tex),
                &mut encoder,
            )
            .expect("C3 deep-chain render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_tex);
        let centre_idx = (SURFACE_H / 2) as usize * SURFACE_W as usize + (SURFACE_W / 2) as usize;
        let centre = pixels[centre_idx];

        // Hand-derived oracle: 3 swap_rb + 3 noop → net single swap → opaque blue.
        // Computed step-by-step via `color_matrix_oracle` for verification.
        let source_straight = [f32::from(SOURCE_RED) / 255.0, 0.0, 0.0, 1.0_f32];
        let after_pass0 = color_matrix_oracle(&swap_rb, source_straight);
        let after_pass1 = color_matrix_oracle(&noop, after_pass0.map(|v| f32::from(v) / 255.0));
        let after_pass2 = color_matrix_oracle(&swap_rb, after_pass1.map(|v| f32::from(v) / 255.0));
        let after_pass3 = color_matrix_oracle(&noop, after_pass2.map(|v| f32::from(v) / 255.0));
        let after_pass4 = color_matrix_oracle(&swap_rb, after_pass3.map(|v| f32::from(v) / 255.0));
        let oracle = color_matrix_oracle(&noop, after_pass4.map(|v| f32::from(v) / 255.0));
        // oracle == (0, 0, 200, 255) by the hand-trace above.

        for channel in 0..4 {
            let gpu_value = centre[channel];
            let oracle_value = oracle[channel];
            let diff = i16::from(gpu_value).abs_diff(i16::from(oracle_value));
            // ±3 LSB tolerance for GPU u8 quantisation across 6 ping-pong passes.
            assert!(
                diff <= 3,
                "C3 ORACLE: centre channel {channel} — GPU={gpu_value}, oracle={oracle_value}, \
                 diff={diff} > 3. Six-pass chain (3× swap_rb, 3× noop) on opaque red must \
                 produce opaque blue (net single swap); diff > 3 indicates heap-spill \
                 truncated passes or fold terminated early.",
            );
        }
    }

    // ── C4: Cumulative bounds — CPU structural ────────────────────────────────

    /// C4: `cumulative_growth` accumulates Blur kernel radii and treats ColorMatrix
    /// as bounds-PRESERVING (zero growth).
    ///
    /// a) `[Blur(σ=4), ColorMatrix, Blur(σ=4)]` → growth = 2 × kernel_radius(4).
    /// b) `[ColorMatrix, ColorMatrix]` → growth = 0.
    #[test]
    fn cumulative_growth_sums_blur_radii_and_ignores_color_matrix() {
        const SIGMA: f32 = 4.0;
        let expected_growth_px = (2 * kernel_radius(SIGMA)) as f32;

        // ── Case a: two Blur passes accumulate; one ColorMatrix contributes nothing ──
        let passes_two_blurs_one_matrix: &[ImageFilterPass] = &[
            ImageFilterPass::Blur {
                sigma_x: SIGMA,
                sigma_y: SIGMA,
            },
            ImageFilterPass::ColorMatrix(identity_matrix()),
            ImageFilterPass::Blur {
                sigma_x: SIGMA,
                sigma_y: SIGMA,
            },
        ];
        let growth_two_blurs =
            super::super::painter::cumulative_growth(passes_two_blurs_one_matrix);
        assert!(
            (growth_two_blurs - expected_growth_px).abs() < 0.5,
            "C4a: cumulative_growth([Blur(σ={SIGMA}), Matrix, Blur(σ={SIGMA})]) = {growth_two_blurs}, \
             expected {expected_growth_px} (= 2 × kernel_radius({SIGMA}) = 2 × {})",
            kernel_radius(SIGMA),
        );

        // ── Case b: two ColorMatrix passes contribute zero growth ─────────────
        let passes_two_matrices: &[ImageFilterPass] = &[
            ImageFilterPass::ColorMatrix(identity_matrix()),
            ImageFilterPass::ColorMatrix(swap_rb_matrix()),
        ];
        let growth_two_matrices = super::super::painter::cumulative_growth(passes_two_matrices);
        assert!(
            growth_two_matrices == 0.0,
            "C4b: cumulative_growth([ColorMatrix, ColorMatrix]) = {growth_two_matrices}, \
             expected 0.0 (ColorMatrix is bounds-PRESERVING)"
        );
    }

    // ── C5: Empty/degenerate — CPU structural ────────────────────────────────

    /// C5: Degenerate Compose cases:
    ///
    /// a) `Compose([])` opens a plain group layer → no `DrawItem::Filter` in the IR.
    ///
    /// b) `Compose([Matrix])` → one FilterOp with one ColorMatrix pass;
    ///    `grown_bounds == content_bounds` (zero growth).
    ///
    /// No `painter.render()` call — purely inspects in-memory IR.
    #[test]
    fn empty_and_single_pass_compose_produce_correct_ir() {
        use flui_painting::display_list::ImageFilter;
        use flui_types::painting::ColorMatrix;

        let (device, queue) = acquire_device_and_queue();
        let content_bounds = full_surface_rect();

        // ── Case a: Compose([]) → no FilterOp ────────────────────────────────
        {
            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            painter.push_compose_for_test(&[]);
            painter.rect(
                content_bounds,
                &Paint::fill(Color::rgba(100, 100, 100, 255)),
            );
            painter.pop_compose_for_test();

            let recorded_ops = painter.filter_ops_for_test();
            assert!(
                recorded_ops.is_empty(),
                "C5a: Compose([]) should emit no FilterOp (falls through to plain group layer); \
                 got {n} FilterOp(s)",
                n = recorded_ops.len()
            );
        }

        // ── Case b: Compose([Matrix]) → one FilterOp, one pass, zero growth ──
        {
            let matrix_values = identity_matrix();
            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            painter.push_compose_for_test(&[ImageFilter::Matrix(ColorMatrix {
                values: matrix_values,
            })]);
            painter.rect(
                content_bounds,
                &Paint::fill(Color::rgba(100, 100, 100, 255)),
            );
            painter.pop_compose_for_test();

            let recorded_ops = painter.filter_ops_for_test();
            assert_eq!(
                recorded_ops.len(),
                1,
                "C5b: Compose([Matrix]) should emit exactly 1 FilterOp; got {n}",
                n = recorded_ops.len()
            );
            let filter_op = &recorded_ops[0];
            assert_eq!(
                filter_op.passes.as_slice(),
                &[ImageFilterPass::ColorMatrix(matrix_values)],
                "C5b: single-pass Compose should produce one ColorMatrix pass"
            );
            // ColorMatrix grows bounds by 0 px (bounds-PRESERVING).
            assert_eq!(
                filter_op.grown_bounds, filter_op.content_bounds,
                "C5b: ColorMatrix is bounds-PRESERVING; grown_bounds should equal content_bounds"
            );
        }
    }
}

// ─── Test-only painter bridge ─────────────────────────────────────────────────

/// Painter helpers for recording `ImageFilter::Compose` IR in C2 and C5 structural tests.
///
/// `push_compose_for_test` calls the **production** `backend::flatten_compose` directly,
/// so C2 and C5 protect the real flatten against regressions (reversed iteration,
/// wrong variant mapping, dropped arms).  The other `ImageFilter` variants are not
/// wrapped here — they route through the `LayerFilter` seam in production and are
/// not inspectable as `FilterOp` passes from a bare `WgpuPainter`.
#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod painter_image_filter_bridge {
    use flui_painting::Paint;
    use flui_types::Color;
    use smallvec::SmallVec;

    use crate::wgpu::{
        command_ir::{ImageFilterPass, ImageFilterSpec},
        painter::WgpuPainter,
    };

    impl WgpuPainter {
        /// Record a `Compose(filters)` layer via the **production** `flatten_compose`,
        /// mirroring `backend.rs::push_image_filter`'s Compose arm exactly:
        /// flatten depth-first, then either a plain group layer (empty) or a `Chain`.
        ///
        /// Takes `&[ImageFilter]` (the inner slice of the Compose node — callers pass
        /// `&[...]` directly, unwrapping the outer `Compose` wrapper).
        ///
        /// Compose-only — the other `ImageFilter` variants route through the
        /// `LayerFilter` seam in production and are not inspectable as `FilterOp`
        /// passes from a bare `WgpuPainter`.
        pub(crate) fn push_compose_for_test(
            &mut self,
            filters: &[flui_painting::display_list::ImageFilter],
        ) {
            let mut passes: SmallVec<[ImageFilterPass; 4]> = SmallVec::new();
            crate::wgpu::backend::flatten_compose(filters, &mut passes);
            if passes.is_empty() {
                self.save_layer(None, &Paint::fill(Color::WHITE));
            } else {
                self.save_layer_with_image_filter(ImageFilterSpec::Chain(passes));
            }
        }

        /// Close the Compose layer opened by [`push_compose_for_test`].
        pub(crate) fn pop_compose_for_test(&mut self) {
            self.restore_layer();
        }
    }
}
