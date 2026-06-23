//! SceneBuilder filter-chain GPU readback gate — T2′ of `gpu-filters-consumer-chain`.
//!
//! ## Purpose
//!
//! These tests prove that the **SceneBuilder producer path** closes end-to-end
//! to pixels for both image-filter (blur) and color-filter (Mode, Gamma, Matrix)
//! operations:
//!
//! ```text
//! SceneBuilder::push_{image,color}_filter
//!   → LayerTree containing {ImageFilterLayer, ColorFilterLayer}
//!   → LayerRender::render (same impl render_scene's render_layer_recursive calls)
//!   → Backend::{push_image_filter, push_color_filter}
//!   → WgpuPainter::save_layer_with_filter / save_layer_with_image_filter
//!   → GPU shader → pixels → readback
//! ```
//!
//! ## Relationship to existing tests
//!
//! `color_filter_producer_tests` (P1-P4) prove that `Backend::push_color_filter`
//! dispatches correctly when called **directly**.  These SC tests (`SC1`-`SC5`)
//! prove the same pixels appear when the call originates via
//! `SceneBuilder → LayerTree → LayerRender` — i.e., the **additional layer of
//! indirection** (SceneBuilder builds the tree; the tree walker calls
//! `LayerRender::render`) does not break the signal.
//!
//! ## Honest scope statement
//!
//! `Renderer::render_scene` requires a live wgpu swapchain surface and **cannot
//! run headlessly**.  These tests exercise the identical constituent operations
//! via the same code path the real frame loop uses:
//!
//! 1. `SceneBuilder` builds the `LayerTree` (same as production).
//! 2. A minimal recursive walk mirrors `render_layer_recursive` for the subset
//!    of layer types used here (no `BackdropFilter`, no offscreen OffscreenRenderer).
//! 3. `Backend` dispatches filter/canvas commands to `WgpuPainter`.
//! 4. `WgpuPainter::render(RenderTarget::sampleable(...))` submits to the GPU.
//! 5. GPU readback asserts oracle match.
//!
//! The only gap relative to `render_scene` is the swapchain acquire and surface
//! present steps — both are infrastructure that contains no filter logic.
//!
//! ## Test inventory
//!
//! | # | Gate | Requirement |
//! |---|------|-------------|
//! | SC1 | GPU | SceneBuilder→blur→pixels: blurred region is softer than sharp region |
//! | SC2 | GPU | SceneBuilder→Mode/Multiply→pixels: oracle match |
//! | SC3 | GPU | SceneBuilder→LinearToSrgbGamma→pixels: oracle match |
//! | SC4 | GPU | SceneBuilder→SrgbToLinearGamma→pixels: oracle match |
//! | SC5 | GPU | SceneBuilder→Matrix/grayscale→pixels: oracle match |

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod gpu_tests {
    use std::sync::Arc;

    use flui_foundation::LayerId;
    use flui_layer::{CanvasLayer, LayerTree, SceneBuilder};
    use flui_painting::Paint;
    use flui_types::{
        Color, Offset, Pixels, Rect,
        painting::{BlendMode, ColorFilter, ImageFilter},
    };

    use crate::wgpu::{
        Backend, layer_render::LayerRender, painter::WgpuPainter, render_target::RenderTarget,
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
        .expect("a GPU adapter must be available for scenebuilder_filter_chain_tests");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("SceneBuilderFilterChain Test Device"),
            ..Default::default()
        }))
        .expect("a GPU device must be available for scenebuilder_filter_chain_tests");
        (Arc::new(device), Arc::new(queue))
    }

    fn create_render_surface(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("SceneBuilderFilterChain Test Surface"),
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

    fn clear_to_black(device: &wgpu::Device, queue: &wgpu::Queue, view: &wgpu::TextureView) {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("SceneBuilderFilterChain Surface Clear"),
        });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("SceneBuilderFilterChain Clear Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
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

    /// Read every pixel back as `[r, g, b, a]` u8 quads.
    fn readback_all_pixels(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
    ) -> Vec<[u8; 4]> {
        let bytes_per_pixel = 4u32;
        let unpadded_row_bytes = SURFACE_WIDTH * bytes_per_pixel;
        let padded_row_bytes = unpadded_row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let staging_size = u64::from(padded_row_bytes * SURFACE_HEIGHT);

        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("SceneBuilderFilterChain Readback Staging"),
            size: staging_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("SceneBuilderFilterChain Readback Encoder"),
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

        let raw = staging.slice(..).get_mapped_range();
        let pixel_count = (SURFACE_WIDTH * SURFACE_HEIGHT) as usize;
        let mut pixels = Vec::with_capacity(pixel_count);
        for row_index in 0..SURFACE_HEIGHT {
            let row_start = (row_index * padded_row_bytes) as usize;
            for col_index in 0..SURFACE_WIDTH {
                let byte_offset = row_start + col_index as usize * 4;
                pixels.push([
                    raw[byte_offset],
                    raw[byte_offset + 1],
                    raw[byte_offset + 2],
                    raw[byte_offset + 3],
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

    fn full_surface_rect() -> Rect<Pixels> {
        Rect::from_xywh(
            Pixels(0.0),
            Pixels(0.0),
            Pixels(SURFACE_WIDTH as f32),
            Pixels(SURFACE_HEIGHT as f32),
        )
    }

    /// Assert every interior pixel (1-px border excluded to avoid SDF edge
    /// artefacts) is within `tolerance` of `expected` in all 4 channels.
    fn assert_interior_pixels_near(
        label: &str,
        pixels: &[[u8; 4]],
        expected: [u8; 4],
        tolerance: u8,
    ) {
        let width = SURFACE_WIDTH as usize;
        let height = SURFACE_HEIGHT as usize;
        for (pixel_index, &actual) in pixels.iter().enumerate() {
            let row = pixel_index / width;
            let col = pixel_index % width;
            if row == 0 || row >= height - 1 || col == 0 || col >= width - 1 {
                continue;
            }
            for channel in 0..4 {
                let channel_diff = u8::try_from(
                    (i16::from(actual[channel]) - i16::from(expected[channel])).unsigned_abs(),
                )
                .expect("diff of two u8 values always fits in u8");
                assert!(
                    channel_diff <= tolerance,
                    "{label}: pixel {pixel_index} (row={row} col={col}) \
                     channel {channel} — actual={a} expected={e} \
                     diff={channel_diff} > tolerance {tolerance}",
                    a = actual[channel],
                    e = expected[channel],
                );
            }
        }
    }

    // ── SceneBuilder layer-tree walk ──────────────────────────────────────────
    //
    // This mirrors `Renderer::render_layer_recursive` for the subset of layer
    // types used in these tests (canvas leaves, image-filter containers, and
    // color-filter containers).  Backdrop-filter special-casing is omitted
    // because none of the test scenes use it.

    fn walk_layer_tree(tree: &LayerTree, node_id: LayerId, backend: &mut Backend<'_>) {
        let Some(layer) = tree.get_layer(node_id) else {
            return;
        };
        layer.render(backend);

        let children: Vec<LayerId> = tree.children(node_id).unwrap_or_default().to_vec();
        for child_id in children {
            walk_layer_tree(tree, child_id, backend);
        }

        // Re-borrow after children walk.
        if let Some(layer) = tree.get_layer(node_id) {
            layer.cleanup(backend);
        }
    }

    /// Render a `LayerTree` built by `SceneBuilder` through `Backend` into
    /// `surface_view`/`surface_tex`, then readback all pixels.
    ///
    /// This is the headless equivalent of the per-frame segment of
    /// `Renderer::render_scene` that runs between surface acquire and present.
    fn render_scenebuilder_tree_and_readback(
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        surface_tex: &wgpu::Texture,
        surface_view: &wgpu::TextureView,
        tree: &LayerTree,
        root_id: Option<LayerId>,
    ) -> Vec<[u8; 4]> {
        let painter = build_painter(Arc::clone(device), Arc::clone(queue));
        let mut backend = Backend::new(painter);

        if let Some(root_id) = root_id {
            walk_layer_tree(tree, root_id, &mut backend);
        }

        let mut painter = backend.into_painter();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("SceneBuilderFilterChain Render Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(surface_view, surface_tex),
                &mut encoder,
            )
            .expect("SceneBuilder filter-chain render must succeed");
        queue.submit(std::iter::once(encoder.finish()));
        readback_all_pixels(device, queue, surface_tex)
    }

    // ── CPU oracle helpers (shared with color_filter_producer_tests) ──────────

    /// Mode oracle: `Color::blend(filter_color, dst_straight, mode)` → premul u8.
    fn mode_oracle(filter_color: Color, dst: Color, mode: BlendMode) -> [u8; 4] {
        let blended = filter_color.blend(dst, mode);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "values clamped to [0,1]*255 then rounded; truncation is safe"
        )]
        let to_premul = |ch: u8, alpha: u8| -> u8 {
            ((f32::from(ch) / 255.0) * (f32::from(alpha) / 255.0) * 255.0).round() as u8
        };
        [
            to_premul(blended.r, blended.a),
            to_premul(blended.g, blended.a),
            to_premul(blended.b, blended.a),
            blended.a,
        ]
    }

    fn linear_to_srgb_oracle(channel_linear: u8) -> u8 {
        let linear = f32::from(channel_linear) / 255.0;
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

    fn srgb_to_linear_oracle(channel_srgb: u8) -> u8 {
        let srgb = f32::from(channel_srgb) / 255.0;
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

    // ── SC1: SceneBuilder → blur → pixels ─────────────────────────────────────

    /// SC1: A `SceneBuilder::push_image_filter(Blur σ=4)` wrapping a solid-colour
    /// canvas produces pixels that are measurably different from an unfiltered
    /// solid fill — the blur decal-clamps at the surface edge, attenuating the
    /// RGB values of corner pixels while the centre stays bright.
    ///
    /// **Proves:**
    /// - `SceneBuilder::push_image_filter` builds an `ImageFilterLayer` in the tree.
    /// - `LayerRender::render` on `ImageFilterLayer` calls `Backend::push_image_filter`.
    /// - The blur shader fires and diffuses pixels, producing edge attenuation.
    ///
    /// **Discriminating assertions:**
    /// 1. Centre pixel is near-white (RGB ≥ 200): the blur interior is bright — the
    ///    filter ran and preserved most energy in the centre.
    /// 2. Corner pixel RGB < centre pixel RGB: the blur decal-clamps at the surface
    ///    boundary, losing energy at the corners.  A no-op (filter missing) would
    ///    leave all corners at the full white (255,255,255,255), so corner < centre
    ///    proves the blur fired.
    ///
    /// **Why not check alpha:** a full-surface white fill keeps alpha=255 everywhere
    /// after blur (no transparent source pixels to import at the boundary from
    /// _outside_ the logical surface); the energy attenuation shows in RGB.
    #[test]
    fn sc1_scenebuilder_blur_reaches_pixels() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_tex, surface_view) = create_render_surface(&device);
        clear_to_black(&device, &queue, &surface_view);

        // Build scene: push_image_filter(Blur σ=4) → add_canvas(white fill) → pop.
        let mut tree = LayerTree::new();
        let root_id = {
            let mut builder = SceneBuilder::new(&mut tree);
            // Zero-offset root so the first push becomes the tree root.
            builder.push_offset(Offset::ZERO);
            builder.push_image_filter(ImageFilter::blur(4.0));
            let mut content_canvas = CanvasLayer::new();
            content_canvas
                .canvas_mut()
                .draw_rect(full_surface_rect(), &Paint::fill(Color::WHITE));
            builder.add_canvas(content_canvas);
            builder.pop().expect("blur filter pop must not underflow");
            builder.pop().expect("root offset pop must not underflow");
            builder.build()
        };

        let pixels = render_scenebuilder_tree_and_readback(
            &device,
            &queue,
            &surface_tex,
            &surface_view,
            &tree,
            root_id,
        );

        let width = SURFACE_WIDTH as usize;

        // Assertion 1: centre pixel RGB must be near-white (blur interior is
        // bright; the filter ran and preserved most energy at the centre).
        let centre_index = (SURFACE_HEIGHT / 2) as usize * width + (SURFACE_WIDTH / 2) as usize;
        let centre_pixel = pixels[centre_index];
        assert!(
            centre_pixel[0] >= 200,
            "SC1: centre pixel R must be ≥ 200 after blur (interior stays bright); \
             got {centre_pixel:?}",
        );

        // Assertion 2: corner pixel RGB must be noticeably dimmer than the centre.
        // The Gaussian blur decal-clamps at the surface boundary: when the blur
        // kernel samples beyond the edge it reads transparent/black, attenuating
        // corner pixel values.  A no-op filter would leave corners at 255.
        let top_left_pixel = pixels[0];
        let corner_r = top_left_pixel[0];
        let centre_r = centre_pixel[0];
        assert!(
            corner_r < centre_r,
            "SC1: corner pixel R must be dimmer than centre after blur \
             (decal-clamp edge attenuation proves the blur shader fired); \
             corner_r={corner_r} centre_r={centre_r}",
        );
    }

    // ── SC2: SceneBuilder → Mode/Multiply → pixels ───────────────────────────

    /// SC2: `SceneBuilder::push_color_filter(Mode { Multiply, half-opacity-blue })`
    /// wrapping a coral canvas produces pixels matching the Mode oracle.
    ///
    /// **Proves:**
    /// - `SceneBuilder::push_color_filter` builds a `ColorFilterLayer` in the tree.
    /// - `LayerRender::render` on `ColorFilterLayer` calls `Backend::push_color_filter`.
    /// - The mode-filter GPU shader produces the correct Multiply result.
    ///
    /// **Red-before-green discriminator:** this would produce opaque coral (no
    /// filter) if `push_color_filter` were a no-op or if the tree walk bypassed
    /// `ColorFilterLayer::render`.
    #[test]
    fn sc2_scenebuilder_mode_filter_multiply_reaches_pixels() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_tex, surface_view) = create_render_surface(&device);
        clear_to_black(&device, &queue, &surface_view);

        let layer_color = Color::rgba(180, 120, 40, 255);
        let filter_color = Color::rgba(0, 0, 200, 128);
        let filter = ColorFilter::mode(filter_color, BlendMode::Multiply);

        let mut tree = LayerTree::new();
        let root_id = {
            let mut builder = SceneBuilder::new(&mut tree);
            builder.push_offset(Offset::ZERO);
            builder.push_color_filter(filter);
            let mut canvas = CanvasLayer::new();
            canvas
                .canvas_mut()
                .draw_rect(full_surface_rect(), &Paint::fill(layer_color));
            builder.add_canvas(canvas);
            builder.pop().expect("color filter pop must not underflow");
            builder.pop().expect("root offset pop must not underflow");
            builder.build()
        };

        let pixels = render_scenebuilder_tree_and_readback(
            &device,
            &queue,
            &surface_tex,
            &surface_view,
            &tree,
            root_id,
        );

        let expected = mode_oracle(filter_color, layer_color, BlendMode::Multiply);
        assert_interior_pixels_near("SC2 SceneBuilder/Mode/Multiply", &pixels, expected, 3);
    }

    // ── SC3: SceneBuilder → LinearToSrgbGamma → pixels ───────────────────────

    /// SC3: `SceneBuilder::push_color_filter(LinearToSrgbGamma)` wrapping a
    /// mid-range canvas produces pixels matching the linear→sRGB oracle.
    ///
    /// **Proves:** the `LinearToSrgbGamma` arm of `ColorFilterLayer::render` /
    /// `Backend::push_color_filter` fires the correct GPU gamma shader.
    #[test]
    fn sc3_scenebuilder_linear_to_srgb_gamma_reaches_pixels() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_tex, surface_view) = create_render_surface(&device);
        clear_to_black(&device, &queue, &surface_view);

        let layer_color = Color::rgba(50, 100, 200, 255);
        let filter = ColorFilter::LinearToSrgbGamma;

        let mut tree = LayerTree::new();
        let root_id = {
            let mut builder = SceneBuilder::new(&mut tree);
            builder.push_offset(Offset::ZERO);
            builder.push_color_filter(filter);
            let mut canvas = CanvasLayer::new();
            canvas
                .canvas_mut()
                .draw_rect(full_surface_rect(), &Paint::fill(layer_color));
            builder.add_canvas(canvas);
            builder.pop().expect("color filter pop must not underflow");
            builder.pop().expect("root offset pop must not underflow");
            builder.build()
        };

        let pixels = render_scenebuilder_tree_and_readback(
            &device,
            &queue,
            &surface_tex,
            &surface_view,
            &tree,
            root_id,
        );

        // Opaque alpha: premul == straight.
        let expected = [
            linear_to_srgb_oracle(50),
            linear_to_srgb_oracle(100),
            linear_to_srgb_oracle(200),
            255,
        ];
        assert_interior_pixels_near("SC3 SceneBuilder/LinearToSrgbGamma", &pixels, expected, 3);
    }

    // ── SC4: SceneBuilder → SrgbToLinearGamma → pixels ───────────────────────

    /// SC4: `SceneBuilder::push_color_filter(SrgbToLinearGamma)` wrapping a
    /// mid-range canvas produces pixels matching the sRGB→linear oracle.
    #[test]
    fn sc4_scenebuilder_srgb_to_linear_gamma_reaches_pixels() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_tex, surface_view) = create_render_surface(&device);
        clear_to_black(&device, &queue, &surface_view);

        let layer_color = Color::rgba(180, 120, 60, 255);
        let filter = ColorFilter::SrgbToLinearGamma;

        let mut tree = LayerTree::new();
        let root_id = {
            let mut builder = SceneBuilder::new(&mut tree);
            builder.push_offset(Offset::ZERO);
            builder.push_color_filter(filter);
            let mut canvas = CanvasLayer::new();
            canvas
                .canvas_mut()
                .draw_rect(full_surface_rect(), &Paint::fill(layer_color));
            builder.add_canvas(canvas);
            builder.pop().expect("color filter pop must not underflow");
            builder.pop().expect("root offset pop must not underflow");
            builder.build()
        };

        let pixels = render_scenebuilder_tree_and_readback(
            &device,
            &queue,
            &surface_tex,
            &surface_view,
            &tree,
            root_id,
        );

        let expected = [
            srgb_to_linear_oracle(180),
            srgb_to_linear_oracle(120),
            srgb_to_linear_oracle(60),
            255,
        ];
        assert_interior_pixels_near("SC4 SceneBuilder/SrgbToLinearGamma", &pixels, expected, 3);
    }

    // ── SC5: SceneBuilder → Matrix/grayscale → pixels ────────────────────────

    /// SC5: `SceneBuilder::push_color_filter(ColorFilter::grayscale())` wrapping
    /// an opaque coral canvas converts the coloured input to luminance-weighted
    /// grayscale.
    ///
    /// **Proves:** the `Matrix` arm of `ColorFilterLayer::render` /
    /// `Backend::push_color_filter` fires the colour-matrix GPU shader via
    /// `SceneBuilder`.
    ///
    /// **Discriminating:** a no-op would leave the coral pixel unchanged
    /// (r≠g≠b); grayscale forces r==g==b.  We assert all three channels are
    /// within tolerance of the luminance value and equal each other.
    #[test]
    fn sc5_scenebuilder_grayscale_matrix_reaches_pixels() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_tex, surface_view) = create_render_surface(&device);
        clear_to_black(&device, &queue, &surface_view);

        let layer_color = Color::rgba(180, 90, 40, 255);
        let filter = ColorFilter::grayscale();

        let mut tree = LayerTree::new();
        let root_id = {
            let mut builder = SceneBuilder::new(&mut tree);
            builder.push_offset(Offset::ZERO);
            builder.push_color_filter(filter);
            let mut canvas = CanvasLayer::new();
            canvas
                .canvas_mut()
                .draw_rect(full_surface_rect(), &Paint::fill(layer_color));
            builder.add_canvas(canvas);
            builder.pop().expect("color filter pop must not underflow");
            builder.pop().expect("root offset pop must not underflow");
            builder.build()
        };

        let pixels = render_scenebuilder_tree_and_readback(
            &device,
            &queue,
            &surface_tex,
            &surface_view,
            &tree,
            root_id,
        );

        // Luminance = 0.2126*R + 0.7152*G + 0.0722*B (ITU-R BT.709).
        // For opaque input, premul == straight.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "luminance in [0,1]*255; truncation is safe"
        )]
        let luminance = (0.2126 * f32::from(layer_color.r)
            + 0.7152 * f32::from(layer_color.g)
            + 0.0722 * f32::from(layer_color.b)) as u8;
        let expected = [luminance, luminance, luminance, 255];

        assert_interior_pixels_near(
            "SC5 SceneBuilder/grayscale-Matrix",
            &pixels,
            expected,
            4, // tolerance=4: grayscale matrix uses f32 shader math vs f32 CPU oracle
        );

        // Additional discriminator: all three RGB channels must be equal
        // (confirming we produced gray, not a colorful miss).
        let width = SURFACE_WIDTH as usize;
        let height = SURFACE_HEIGHT as usize;
        for (pixel_index, &px_rgba) in pixels.iter().enumerate() {
            let row = pixel_index / width;
            let col = pixel_index % width;
            if row == 0 || row >= height - 1 || col == 0 || col >= width - 1 {
                continue;
            }
            let r_g_diff =
                u8::try_from((i16::from(px_rgba[0]) - i16::from(px_rgba[1])).unsigned_abs())
                    .expect("diff of two u8 values always fits in u8");
            assert!(
                r_g_diff <= 4,
                "SC5: R and G channels must be equal in grayscale output; \
                 pixel {pixel_index} R={} G={} diff={}",
                px_rgba[0],
                px_rgba[1],
                r_g_diff,
            );
        }
    }
}
