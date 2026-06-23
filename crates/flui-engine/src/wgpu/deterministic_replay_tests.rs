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

    /// Companion witness: `AdvancedShapeOp` (the shape-level advanced-blend record
    /// item) is `Clone` + handle-free. Its fields are `DrawSegment` (witnessed
    /// above), `BlendMode` (`Copy`), and `Rect<Pixels>` (`Copy`). A future field
    /// holding a live GPU handle (`Texture`/`TextureView`/`BindGroup`/`Sampler`)
    /// would make this `const` fail to compile — guarding T11 IR-purity for the
    /// new variant.
    const _ADVANCED_SHAPE_OP_IS_CLONE: fn(
        crate::wgpu::command_ir::AdvancedShapeOp,
    ) -> crate::wgpu::command_ir::AdvancedShapeOp = |op| op.clone();

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

    // ──────────────────────────────────────────────────────────────────────────
    // Test 2: filter-layer A/B deterministic replay (Task 0 / G3 identity gate)
    // ──────────────────────────────────────────────────────────────────────────

    /// Build a `DrawItem::Filter(Identity)` wrapping a rect geometry segment
    /// and record it into a `Vec<DrawItem>` (no painter flush needed).
    fn build_filter_scene_items() -> Vec<DrawItem> {
        use crate::wgpu::command_ir::{FilterOp, ImageFilterPass};
        use smallvec::smallvec;

        // Build a geometry segment with a white 20×20 rect — the same geometry
        // the baseline `DrawItem::Segment` will also draw, enabling G3 comparison.
        let mut seg = DrawSegment::new();
        let instance = crate::wgpu::instancing::RectInstance::rect(
            Rect::from_ltrb(px(10.0), px(10.0), px(30.0), px(30.0)),
            Color::rgba(255, 255, 255, 255),
        );
        let _ = seg.rect_batch.add(instance);
        // Each rect instance must have a corresponding scissor region entry
        // (start+count for the draw call). Without this the flush loop at
        // replay/flush.rs `for region in &segment.rect_scissors` issues zero draws.
        DrawSegment::push_scissor_region(&mut seg.rect_scissors, None);

        let content_bounds = Rect::from_ltrb(px(10.0), px(10.0), px(30.0), px(30.0));

        let op = FilterOp {
            input: seg,
            passes: smallvec![ImageFilterPass::Identity],
            content_bounds,
            grown_bounds: content_bounds, // Identity grows by 0
            fb_origin: (10, 10),          // floor(grown.left/top) — integer-aligned (Task 6)
            fb_dim: (20, 20),             // ceil(grown.right/bottom) - fb_origin (Task 6)
        };

        vec![DrawItem::Filter(op)]
    }

    /// Build a plain `DrawItem::Segment` drawing the same geometry as
    /// `build_filter_scene_items` — used for the G3 identity-fidelity check.
    ///
    /// G3 (orchestrator guardrail): `DrawItem::Filter(Identity)` must composite
    /// identically to `DrawItem::Segment` passing through the same
    /// render→offscreen→premul-composite round-trip.
    ///
    /// Note: the `OffscreenTexture` path (not the bare `Segment` path) is the
    /// correct oracle because the Filter arm routes through
    /// `render_segment_to_offscreen` + `flush_texture_batch_premultiplied`,
    /// exactly matching the `OffscreenTexture` arm. Comparing to a bare
    /// `DrawItem::Segment` would introduce a 1-LSB difference from the extra
    /// offscreen round-trip's premultiplied composite.
    ///
    /// For Task 0 we compare against another Filter(Identity) replay (A vs B),
    /// not against a raw Segment, so this note is informational only; the
    /// identity-fidelity is proved by A == B being byte-exact over real geometry.
    fn build_baseline_segment_items() -> Vec<DrawItem> {
        let mut seg = DrawSegment::new();
        let instance = crate::wgpu::instancing::RectInstance::rect(
            Rect::from_ltrb(px(10.0), px(10.0), px(30.0), px(30.0)),
            Color::rgba(255, 255, 255, 255),
        );
        let _ = seg.rect_batch.add(instance);
        // Each rect instance needs a scissor region entry (see build_filter_scene_items).
        DrawSegment::push_scissor_region(&mut seg.rect_scissors, None);
        vec![DrawItem::Segment(seg)]
    }

    /// C5 extension: deterministic A/B replay of a `DrawItem::Filter(Identity)`
    /// scene, proving:
    /// 1. `FilterOp` is `Clone` + handle-free (the `op.clone()` below won't
    ///    compile without the Task 0 seam — the "red→green" structural gate).
    /// 2. Two independent replays of the same filter scene produce byte-identical
    ///    pixel output (determinism).
    /// 3. The replayed output has at least one non-zero RGB pixel (non-vacuous).
    /// 4. G3 identity-fidelity: the filter output is byte-identical to the same
    ///    geometry drawn via a plain `DrawItem::Segment` path (the Identity pass
    ///    must not corrupt or lose pixels vs a direct composite).
    ///
    ///    Implementation note on G3: Filter(Identity) and Segment are NOT
    ///    byte-identical because the filter routes through a full-viewport offscreen
    ///    (clear → draw → composite-to-grown_bounds) while Segment draws directly.
    ///    The correct byte-identical oracle is `DrawItem::OffscreenTexture` (same
    ///    composite path). For Task 0 we verify content presence at the composite
    ///    area, not byte-equality. Byte-exact oracle deferred to Slice 1.
    #[test]
    fn filter_layer_identity_replay_is_deterministic_and_faithful() {
        use crate::wgpu::command_ir::DrawItem;

        let (device, queue) = test_device_and_queue();

        // ── Step 1: Build the filter scene items ───────────────────────────────
        let filter_items_source = build_filter_scene_items();

        // Extract the FilterOp to clone it for the two replay passes.
        // This is the "red→green" gate: `op.clone()` requires FilterOp: Clone,
        // which requires DrawItem::Filter to exist as a variant.
        let op_clone_a = match &filter_items_source[0] {
            DrawItem::Filter(op) => op.clone(),
            _ => panic!("expected DrawItem::Filter as first item"),
        };
        let op_clone_b = op_clone_a.clone();

        let items_a: Vec<DrawItem> = vec![DrawItem::Filter(op_clone_a)];
        let items_b: Vec<DrawItem> = vec![DrawItem::Filter(op_clone_b)];

        // ── Step 2: Allocate three independent render targets ─────────────────
        // Target A and B: for A/B filter determinism.
        // Target C: for G3 identity-fidelity (plain Segment composite).
        let (target_a, view_a) = make_render_target(&device);
        let (target_b, view_b) = make_render_target(&device);
        let (target_c, view_c) = make_render_target(&device);
        clear_target(&device, &queue, &view_a);
        clear_target(&device, &queue, &view_b);
        clear_target(&device, &queue, &view_c);

        // ── Step 3: Replay A (Filter) ──────────────────────────────────────────
        let mut painter_a = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            SCENE_FORMAT,
            (SCENE_SIZE, SCENE_SIZE),
        );
        let mut encoder_a = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("T11-Filter-A"),
        });
        painter_a
            .replay_items_for_test(items_a, &view_a, &mut encoder_a)
            .expect("filter replay A must succeed");
        queue.submit(std::iter::once(encoder_a.finish()));

        // ── Step 4: Replay B (Filter) ──────────────────────────────────────────
        let mut painter_b = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            SCENE_FORMAT,
            (SCENE_SIZE, SCENE_SIZE),
        );
        let mut encoder_b = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("T11-Filter-B"),
        });
        painter_b
            .replay_items_for_test(items_b, &view_b, &mut encoder_b)
            .expect("filter replay B must succeed");
        queue.submit(std::iter::once(encoder_b.finish()));

        // ── Step 5: Replay C (plain Segment — G3 oracle) ──────────────────────
        let baseline_items = build_baseline_segment_items();
        let mut painter_c = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            SCENE_FORMAT,
            (SCENE_SIZE, SCENE_SIZE),
        );
        let mut encoder_c = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("T11-Filter-C-baseline"),
        });
        painter_c
            .replay_items_for_test(baseline_items, &view_c, &mut encoder_c)
            .expect("baseline segment replay C must succeed");
        queue.submit(std::iter::once(encoder_c.finish()));

        // ── Step 6: Read back all three targets ───────────────────────────────
        let pixels_a = readback_rgba(&device, &queue, &target_a);
        let pixels_b = readback_rgba(&device, &queue, &target_b);
        let pixels_c = readback_rgba(&device, &queue, &target_c);

        // ── Step 7: A/B determinism assertion ─────────────────────────────────
        assert_eq!(
            pixels_a.len(),
            pixels_b.len(),
            "readback buffers must have equal length"
        );
        let first_ab_mismatch = pixels_a
            .iter()
            .zip(pixels_b.iter())
            .enumerate()
            .find(|(_, (a, b))| a != b);
        assert!(
            first_ab_mismatch.is_none(),
            "filter-layer replay A and B must be byte-identical \
             (determinism gate). First divergence at byte {} (pixel {}, ch {}): \
             A={:#04x} B={:#04x}",
            first_ab_mismatch.unwrap().0,
            first_ab_mismatch.unwrap().0 / 4,
            first_ab_mismatch.unwrap().0 % 4,
            first_ab_mismatch.unwrap().1.0,
            first_ab_mismatch.unwrap().1.1,
        );

        // ── Step 8: Non-vacuous content guard ─────────────────────────────────
        let has_drawn_color = pixels_a
            .chunks_exact(4)
            .any(|rgba| rgba[0] > 0 || rgba[1] > 0 || rgba[2] > 0);
        assert!(
            has_drawn_color,
            "identity filter replay must produce at least one non-zero RGB pixel \
             (pre-clear is opaque black; a silent no-op would fail this guard)"
        );

        // ── Step 9: G3 identity-fidelity — Filter(Identity) has visible content ─
        //
        // The Filter(Identity) and Segment paths are NOT byte-identical because the
        // filter routes through a full-viewport offscreen texture (clear → draw → composite)
        // while Segment draws directly to the target. The offscreen composite maps
        // the full 64×64 texture to `grown_bounds` [10,10]-[30,30], so the white rect
        // (at offscreen UVs [0.156, 0.156]-[0.469, 0.469]) maps to a sub-region of
        // the 20×20 composite area: roughly screen pixels [13,13]-[19,19].
        //
        // G3 (orchestrator guardrail): Identity must preserve content — the filter
        // must not corrupt or eliminate pixels. We verify:
        //   (a) the filter output has non-zero RGB at the composite area [10,10]-[30,30],
        //   (b) the baseline Segment also has non-zero RGB at [10,10]-[30,30].
        //
        // Note: the correct byte-identical oracle for a filter is `DrawItem::OffscreenTexture`
        // (which also composites a full-viewport texture to a dst_rect), not a bare
        // `DrawItem::Segment`. That oracle is deferred to Slice 1 when the integration
        // test framework can construct a `PooledTexture` without a painter round-trip.
        let filter_has_content_in_composite_area =
            pixels_a
                .chunks_exact(4)
                .enumerate()
                .any(|(pixel_idx, rgba)| {
                    let col = pixel_idx % SCENE_SIZE as usize;
                    let row = pixel_idx / SCENE_SIZE as usize;
                    // Check the composite area (grown_bounds = [10,10]-[30,30])
                    (10..30).contains(&row)
                        && (10..30).contains(&col)
                        && (rgba[0] > 0 || rgba[1] > 0 || rgba[2] > 0)
                });
        assert!(
            filter_has_content_in_composite_area,
            "G3 identity-fidelity: Filter(Identity) must produce non-zero RGB content \
             within the composite area [10,10]-[30,30]. A silent identity pass that zeroed \
             all pixels would fail this guard (Identity must preserve, not corrupt)."
        );

        let segment_has_content_in_rect_area =
            pixels_c
                .chunks_exact(4)
                .enumerate()
                .any(|(pixel_idx, rgba)| {
                    let col = pixel_idx % SCENE_SIZE as usize;
                    let row = pixel_idx / SCENE_SIZE as usize;
                    (10..30).contains(&row)
                        && (10..30).contains(&col)
                        && (rgba[0] > 0 || rgba[1] > 0 || rgba[2] > 0)
                });
        assert!(
            segment_has_content_in_rect_area,
            "G3 baseline: Segment must produce non-zero RGB content within [10,10]-[30,30] \
             (oracle sanity check — if this fails, the test geometry is wrong)."
        );

        // ── Step 10: G3 byte-exact no-op oracle — Identity pass ≡ empty fold ───
        //
        // The content-presence checks above prove the round-trip composites visible
        // pixels, but NOT that the Identity *pass* preserves them byte-for-byte. The
        // tightest oracle constructible without Slice-1 harness work is a Filter with
        // an EMPTY pass chain: it runs the SAME render→offscreen→composite round-trip
        // but skips the fold loop body entirely. `Filter([Identity])` MUST therefore
        // be byte-identical to `Filter([])` — proving `ImageFilterPass::Identity` is a
        // true no-op (this would catch a regression where Identity acquired a texture
        // or altered pixels). It also covers the empty-fold branch of
        // `apply_image_filter_passes`, which `Filter([Identity])` alone never exercises.
        // (The full OffscreenTexture fidelity oracle is deferred to Slice 1, which adds
        // harness plumbing to build a content-filled `PooledTexture` directly in the IR.)
        let empty_pass_op = {
            use crate::wgpu::command_ir::FilterOp;
            let mut seg = DrawSegment::new();
            let instance = crate::wgpu::instancing::RectInstance::rect(
                Rect::from_ltrb(px(10.0), px(10.0), px(30.0), px(30.0)),
                Color::rgba(255, 255, 255, 255),
            );
            let _ = seg.rect_batch.add(instance);
            DrawSegment::push_scissor_region(&mut seg.rect_scissors, None);
            let content_bounds = Rect::from_ltrb(px(10.0), px(10.0), px(30.0), px(30.0));
            FilterOp {
                input: seg,
                passes: smallvec::SmallVec::new(), // empty fold — same round-trip, zero passes
                content_bounds,
                grown_bounds: content_bounds,
                fb_origin: (10, 10), // floor(grown.left/top) — integer-aligned (Task 6)
                fb_dim: (20, 20),    // ceil(grown.right/bottom) - fb_origin (Task 6)
            }
        };
        let (target_d, view_d) = make_render_target(&device);
        clear_target(&device, &queue, &view_d);
        let mut painter_d = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            SCENE_FORMAT,
            (SCENE_SIZE, SCENE_SIZE),
        );
        let mut encoder_d = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("T11-Filter-D-empty-passes"),
        });
        painter_d
            .replay_items_for_test(
                vec![DrawItem::Filter(empty_pass_op)],
                &view_d,
                &mut encoder_d,
            )
            .expect("empty-pass filter replay D must succeed");
        queue.submit(std::iter::once(encoder_d.finish()));
        let pixels_d = readback_rgba(&device, &queue, &target_d);

        let first_identity_mismatch = pixels_a
            .iter()
            .zip(pixels_d.iter())
            .enumerate()
            .find(|(_, (a, d))| a != d);
        assert!(
            first_identity_mismatch.is_none(),
            "G3 identity-fidelity: Filter([Identity]) must be byte-identical to Filter([]) \
             (empty fold) — the Identity pass must be a true no-op. First divergence at \
             byte {} (pixel {}, ch {}): Identity={:#04x} empty={:#04x}",
            first_identity_mismatch.unwrap().0,
            first_identity_mismatch.unwrap().0 / 4,
            first_identity_mismatch.unwrap().0 % 4,
            first_identity_mismatch.unwrap().1.0,
            first_identity_mismatch.unwrap().1.1,
        );
    }
}
