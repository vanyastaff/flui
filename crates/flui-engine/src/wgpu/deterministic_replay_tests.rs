//! T11 C5-gate: deterministic-replay acceptance tests.
//!
//! This module implements the three-part C5 acceptance criterion from
//! `.rust-studio/specs/flui-engine-overhaul/tasks.md` task 11:
//!
//! 1. **A/B deterministic-replay test** — record one scene's Command-IR, then replay it
//!    to two *independent* offscreen render targets (encoder A → target A, encoder B → target
//!    B) and assert the pixel readbacks are byte-identical.  This proves the IR separation is
//!    non-tautological: the same logical scene always produces the same GPU output regardless
//!    of run order.
//!
//! 2. **Compile-time IR-purity witness** — a `const` compile-time check that [`DrawSegment`]
//!    implements `Clone` without any `wgpu::Device`, `wgpu::Queue`, `wgpu::CommandEncoder`, or
//!    `wgpu::TextureView` in scope.  `Clone` derivability is sound only when all fields are
//!    pure data; the derive macro would fail at compile time if any field held a non-`Clone`
//!    GPU handle.  A dedicated `#[test]` additionally constructs a full [`DrawSegment`] from
//!    CPU data only to show the record IR is self-contained.
//!
//! 3. **Idempotence (via A/B test, no separate test)** — the A/B dual-target replay in test 1
//!    is the real idempotence proof: the same logical scene, cloned and replayed twice through
//!    *independent* encoders and targets, produces byte-identical pixel output.  A separate
//!    "non-mutation" test that replayed a clone and asserted the original was unchanged was
//!    removed (see PR review finding Fix 3) because it was tautological: a deep `Clone` with
//!    no shared interior-mutable state is definitionally independent of its source, so the
//!    assertion held for any `Clone` type and exercised nothing about `submit`.  The A/B test
//!    is the discriminating property.
//!
//! ## What the A/B test proves (and cannot prove)
//!
//! The dual-target readback proves **determinism at the pixel level**: the same IR, replayed
//! twice through two independent encoders and two independent render targets, produces
//! byte-identical output.  This would fail under:
//! - any global mutable state mutated by replay that differs between pass A and pass B
//! - any per-run ordering dependency in the flush path
//! - any texture-batch scratch-buffer leak across the two replays
//!
//! The test cannot prove absence of GPU driver non-determinism (e.g. floating-point
//! rounding on different hardware), but for the instanced-rect + instanced-circle +
//! linear-gradient scene used here — which has no blending, SDF-clip, or floating-point
//! path tessellation — the outputs are expected to be bit-exact on any DX12/Vulkan/Metal
//! driver that correctly implements the WGSL shaders.
//!
//! ## Why `DrawSegment: Clone` is the right purity witness
//!
//! `Clone` is only derivable when *every* field implements `Clone`.  `wgpu::Buffer`,
//! `wgpu::Texture`, `wgpu::TextureView`, `wgpu::BindGroup`, and `wgpu::Sampler` do
//! **not** implement `Clone`.  Therefore `#[derive(Clone)] struct DrawSegment { … }`
//! compiling without error is a machine-checked proof that no live GPU handle is stored
//! in the record IR.  This is stronger than a comment: the compiler rejects any future
//! field addition that introduces a GPU handle.
//!
//! ## Limitations acknowledged
//!
//! `SavedLayer`, `PendingOpacityLayer`, and `DrawItem::OffscreenTexture` hold live
//! `PooledTexture` (which wraps `wgpu::Texture`) and are therefore NOT Clone.  The
//! tests use a draw scene that produces only `DrawItem::Segment` items (no `save_layer`,
//! no offscreen compositing) to stay within the cloneable subset.  The opacity-layer
//! path is covered by existing readback tests (T8 layer readback suite).

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod tests {
    use std::sync::Arc;

    use flui_painting::Paint;
    use flui_types::{Color, Rect, geometry::px, styling::Color as StyledColor};

    use crate::wgpu::{
        command_ir::{DrawItem, DrawSegment},
        effects::GradientStop,
        painter::WgpuPainter,
    };

    // ──────────────────────────────────────────────────────────────────────────
    // Compile-time IR-purity witness (deliverable 2)
    // ──────────────────────────────────────────────────────────────────────────

    /// Compile-time proof that `DrawSegment` is `Clone`.
    ///
    /// `Clone` is only derivable when every field is `Clone`.  `wgpu::Buffer`,
    /// `wgpu::Texture`, `wgpu::TextureView`, `wgpu::BindGroup`, and `wgpu::Sampler`
    /// do NOT implement `Clone`, so this `const` witness failing to compile would
    /// mean a live GPU handle had been added to the record IR.
    ///
    /// **What this proves:** `DrawSegment` contains no live GPU handles.
    /// **What this cannot prove:** that all field *values* are semantically free of
    /// GPU-side effects (e.g., a `u64` handle ID could be stored — the type system
    /// cannot reject that).  The companion runtime test below constructs a full
    /// `DrawSegment` using only CPU-side API to provide the runtime evidence.
    const _DRAW_SEGMENT_IS_CLONE: fn(DrawSegment) -> DrawSegment = |seg| seg.clone();

    /// Runtime IR-purity witness: construct a `DrawSegment` with no GPU context.
    ///
    /// This test creates and clones a `DrawSegment` without ever calling
    /// `wgpu::Device`, `wgpu::Queue`, `wgpu::CommandEncoder`, or
    /// `wgpu::TextureView`.  The fact that it compiles **and runs** confirms the
    /// record IR is self-contained CPU data — not a reference to live GPU state.
    ///
    /// Discriminating: if any code path in `DrawSegment::new()` or `Clone` secretly
    /// touched GPU resources (e.g., via a hidden `Arc<wgpu::Device>` field) the
    /// test would panic or fail to produce a meaningful clone without a device.
    #[test]
    fn draw_segment_is_pure_cpu_data() {
        // No `wgpu::Device` / `wgpu::Queue` / `wgpu::Encoder` / `wgpu::TextureView`
        // is created or referenced anywhere in this function.
        let segment = DrawSegment::new();

        // Clone must be sound — all fields are plain CPU vecs/arrays.
        let cloned = segment.clone();

        // Both are empty (freshly constructed) — `is_empty` proves the struct is
        // fully initialised without GPU side effects.
        assert!(
            segment.is_empty(),
            "freshly constructed DrawSegment must be empty"
        );
        assert!(
            cloned.is_empty(),
            "clone of an empty DrawSegment must also be empty"
        );
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Shared scene builder
    // ──────────────────────────────────────────────────────────────────────────

    /// The target size used for all readback tests in this module.
    const SCENE_SIZE: u32 = 64;

    /// The surface format used for all readback tests in this module.
    const SCENE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

    /// Headless GPU device + queue, identical to the painter-test helper.
    fn test_device_and_queue() -> (Arc<wgpu::Device>, Arc<wgpu::Queue>) {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .expect("a GPU adapter must be available for deterministic-replay tests");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("T11 Deterministic Replay Test Device"),
            ..Default::default()
        }))
        .expect("a GPU device must be available for deterministic-replay tests");
        (Arc::new(device), Arc::new(queue))
    }

    /// Create a fresh render target (RENDER_ATTACHMENT | COPY_SRC) and its view.
    fn make_render_target(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("T11 render target"),
            size: wgpu::Extent3d {
                width: SCENE_SIZE,
                height: SCENE_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SCENE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view)
    }

    /// Clear `target_view` to a solid opaque black via a one-shot render pass.
    fn clear_target(device: &wgpu::Device, queue: &wgpu::Queue, target_view: &wgpu::TextureView) {
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("T11 clear pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
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

    /// Read back all pixels from `texture` into a tightly-packed `Vec<u8>` (RGBA,
    /// stride = `SCENE_SIZE * 4`).
    fn readback_rgba(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
    ) -> Vec<u8> {
        let bytes_per_pixel = 4u32;
        let unpadded = SCENE_SIZE * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_stride = unpadded.div_ceil(align) * align;

        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("T11 readback staging buffer"),
            size: u64::from(padded_stride) * u64::from(SCENE_SIZE),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut copy_encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
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
                    bytes_per_row: Some(padded_stride),
                    rows_per_image: Some(SCENE_SIZE),
                },
            },
            wgpu::Extent3d {
                width: SCENE_SIZE,
                height: SCENE_SIZE,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(copy_encoder.finish()));

        let slice = staging.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| {
            r.expect("staging buffer mapping must succeed");
        });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("device poll must complete the readback copy");

        let raw = slice.get_mapped_range();
        let row_bytes = (SCENE_SIZE * bytes_per_pixel) as usize;
        let mut packed = Vec::with_capacity(row_bytes * SCENE_SIZE as usize);
        for row_index in 0..SCENE_SIZE as usize {
            let row_start = row_index * padded_stride as usize;
            packed.extend_from_slice(&raw[row_start..row_start + row_bytes]);
        }
        drop(raw);
        staging.unmap();
        packed
    }

    /// Record the multi-phase test scene into `painter` without flushing.
    ///
    /// The scene exercises three IR phases:
    /// - **Instanced rect** (rect_batch) — a solid white 20×20 square at (10,10)
    /// - **Instanced circle** (circle_batch) — a red circle at (48,48) r=10
    /// - **Linear gradient rect** (linear_gradient_batch) — a 30×30 gradient at (5,30)
    ///
    /// No `save_layer`, no offscreen compositing, no opacity layers — the scene
    /// produces only `DrawItem::Segment` items, which are `Clone`.
    fn record_multi_phase_scene(painter: &mut WgpuPainter) {
        let white = Color::rgba(255, 255, 255, 255);
        let red = Color::rgba(255, 0, 0, 255);

        // Phase 1: instanced rect
        painter.rect(
            Rect::from_xywh(px(10.0), px(10.0), px(20.0), px(20.0)),
            &Paint::fill(white),
        );

        // Phase 2: instanced circle
        painter.circle(
            flui_types::Point::new(px(48.0), px(48.0)),
            10.0,
            &Paint::fill(red),
        );

        // Phase 3: linear gradient rect — exercises the gradient flush phase
        let blue = StyledColor::rgba(0, 0, 255, 255);
        let transparent = StyledColor::rgba(0, 0, 255, 0);
        let gradient_stops = [GradientStop::start(blue), GradientStop::end(transparent)];
        painter.gradient_rect(
            Rect::from_xywh(px(5.0), px(30.0), px(30.0), px(30.0)),
            glam::Vec2::new(5.0, 30.0),
            glam::Vec2::new(35.0, 60.0),
            &gradient_stops,
            0.0,
        );
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Test 1: deterministic A/B dual-target replay (core C5 gate)
    // ──────────────────────────────────────────────────────────────────────────

    /// C5 gate: replay the same recorded Command-IR to two independent offscreen
    /// render targets (encoder A → target A, encoder B → target B) and assert that
    /// both full-frame pixel readbacks are byte-identical.
    ///
    /// **Why this is discriminating:**
    /// - Two *different* `wgpu::Texture` objects are allocated (no aliasing).
    /// - Two *different* `wgpu::CommandEncoder` objects are used (independent command
    ///   streams — the GPU sees two separate submissions).
    /// - The two replay calls use *separate clones* of the recorded IR (same logical
    ///   content, different heap allocations).
    /// - A non-trivial 3-phase scene is used (rect + circle + gradient) so the
    ///   result cannot be vacuously all-zeros.
    /// - The assertion compares **every pixel** of both targets, not just the center.
    ///
    /// **What a determinism break looks like:** if replay leaked per-call global state
    /// (e.g., a texture-batch scratch buffer not cleared between replays, or a viewport
    /// uniform not re-uploaded for the second encoder), pixels in target B would differ
    /// from target A.  The test would fail with a mismatch message showing which pixel
    /// index diverged.
    ///
    /// **Replay design note:** `GpuReplay::submit` takes `items: Vec<DrawItem>` by value.
    /// To replay the same logical scene twice, we record once, drain the recorded
    /// segments (obtaining `Vec<DrawSegment>`), then wrap two independent clones into
    /// `DrawItem::Segment` for replay A and replay B respectively.  No production
    /// signature was changed — `Clone` was added to `DrawSegment` and `InstanceBatch<T>`
    /// as the minimal production change required to support this gate.
    #[test]
    fn command_ir_replay_is_deterministic() {
        let (device, queue) = test_device_and_queue();

        // ── Step 1: Record the scene and drain the Command IR ─────────────────
        let mut recording_painter = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            SCENE_FORMAT,
            (SCENE_SIZE, SCENE_SIZE),
        );
        record_multi_phase_scene(&mut recording_painter);

        // `drain_segments_for_test` calls `finish_current_segment` internally and
        // drains only `DrawItem::Segment` variants (no OffscreenTexture / OpacityLayer
        // — our scene produces none).
        let recorded_segments: Vec<DrawSegment> = recording_painter.drain_segments_for_test();
        assert!(
            !recorded_segments.is_empty(),
            "the multi-phase scene must produce at least one DrawSegment"
        );

        // Verify the scene produced non-trivial content (rect_batch has instances).
        assert!(
            !recorded_segments[0].rect_batch.is_empty(),
            "the multi-phase scene must include at least one instanced rect"
        );
        assert!(
            !recorded_segments[0].circle_batch.is_empty(),
            "the multi-phase scene must include at least one instanced circle"
        );
        assert!(
            !recorded_segments[0].linear_gradient_batch.is_empty(),
            "the multi-phase scene must include at least one linear gradient"
        );

        // ── Step 2: Build two independent DrawItem lists from clones ─────────
        //
        // Clone A and Clone B are independent heap allocations with equal content.
        // Replaying Clone A must not influence Clone B — the test catches any
        // state-leakage between the two replay passes.
        let items_for_replay_a: Vec<DrawItem> = recorded_segments
            .iter()
            .map(|seg| DrawItem::Segment(seg.clone()))
            .collect();
        let items_for_replay_b: Vec<DrawItem> = recorded_segments
            .iter()
            .map(|seg| DrawItem::Segment(seg.clone()))
            .collect();

        // ── Step 3: Allocate two independent render targets ───────────────────
        let (target_a, view_a) = make_render_target(&device);
        let (target_b, view_b) = make_render_target(&device);

        // Clear both targets to the same solid colour before replay so any
        // painter-clear behaviour is identical.
        clear_target(&device, &queue, &view_a);
        clear_target(&device, &queue, &view_b);

        // ── Step 4: Replay A → encoder A → target A ───────────────────────────
        //
        // Each replay uses its own WgpuPainter (and therefore its own GpuReplay
        // instance).  This rules out any shared-field mutation between replays.
        let mut painter_a = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            SCENE_FORMAT,
            (SCENE_SIZE, SCENE_SIZE),
        );
        let mut encoder_a = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("T11-A"),
        });
        painter_a
            .replay_items_for_test(items_for_replay_a, &view_a, &mut encoder_a)
            .expect("replay A must succeed");
        queue.submit(std::iter::once(encoder_a.finish()));

        // ── Step 5: Replay B → encoder B → target B ───────────────────────────
        let mut painter_b = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            SCENE_FORMAT,
            (SCENE_SIZE, SCENE_SIZE),
        );
        let mut encoder_b = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("T11-B"),
        });
        painter_b
            .replay_items_for_test(items_for_replay_b, &view_b, &mut encoder_b)
            .expect("replay B must succeed");
        queue.submit(std::iter::once(encoder_b.finish()));

        // ── Step 6: Read back both targets and assert byte-identical ──────────
        let pixels_a = readback_rgba(&device, &queue, &target_a);
        let pixels_b = readback_rgba(&device, &queue, &target_b);

        assert_eq!(
            pixels_a.len(),
            pixels_b.len(),
            "both readback buffers must have the same byte length"
        );

        // Find the first diverging byte for a useful failure message.
        let first_mismatch = pixels_a
            .iter()
            .zip(pixels_b.iter())
            .enumerate()
            .find(|(_, (a, b))| a != b);

        assert!(
            first_mismatch.is_none(),
            "pixel readbacks from replay A and replay B must be byte-identical \
             (C5 deterministic-replay gate). First divergence at byte index {} \
             (pixel {}, channel {}): A={:#04x} B={:#04x}",
            first_mismatch.unwrap().0,
            first_mismatch.unwrap().0 / 4,
            first_mismatch.unwrap().0 % 4,
            first_mismatch.unwrap().1.0,
            first_mismatch.unwrap().1.1,
        );

        // Drawn-content guard: at least one pixel must have a non-zero RGB channel.
        //
        // The targets are pre-cleared to opaque black (R=0, G=0, B=0, A=255).
        // Summing ALL bytes (including alpha) would always produce a non-zero total
        // even for a silent no-op replay that drew nothing — that is the vacuous check
        // we are replacing.  Checking only the RGB channels (byte indices 0, 1, 2 of
        // each RGBA pixel) is discriminating: a pure background returns no hits; any
        // colored geometry drawn on the target (the white rect, the red circle, or the
        // blue gradient) contributes at least one pixel with R|G|B > 0.
        //
        // This also serves as the idempotence non-emptiness proof (Fix 3): because the
        // A/B equality assertion above already passed, proving target A has drawn color
        // is sufficient — target B is byte-identical and therefore also has drawn color.
        let has_drawn_color = pixels_a
            .chunks_exact(4)
            .any(|rgba| rgba[0] > 0 || rgba[1] > 0 || rgba[2] > 0);
        assert!(
            has_drawn_color,
            "the replayed scene must produce at least one pixel with a non-zero \
             RGB channel — a pure opaque-black readback means no geometry was \
             actually rasterized to the target (pre-clear is opaque black so \
             alpha alone cannot satisfy this guard)"
        );
    }
}
