# flui-engine Architecture

`flui-engine` turns a [`flui_layer::Scene`] into wgpu draw calls. It is the
GPU end of FLUI's five-tree pipeline: everything above it (`Layer`, the
display list, the widget tree) is data; everything in here is the lowering of
that data onto one device, one queue, and — for a window — one surface.

wgpu is the engine, not a backend behind one. No other rasteriser (Vello,
Skia, a software path) is planned, and nothing here exists to make one
pluggable: the crate re-exports the exact `wgpu` it links against
(`flui_engine::wgpu`), its modules sit at the crate root, and the one trait
that abstracts over "a thing that renders a scene" ([`RasterBackend`]) is a
GPU-free test seam for the application frame loop, not a plugin point.

The reference for the widget-facing behaviour is Flutter's compositor
contract (what a layer means, in what order it paints); the reference for
everything below the `Scene` is wgpu itself. Protocol-level divergences from
Flutter carry an ADR; crate-local shapes are recorded under
[Mapping decisions](#mapping-decisions).

[`flui_layer::Scene`]: ../flui-layer/src/scene.rs
[`RasterBackend`]: src/raster.rs

---

## Module map

| Concern | Files | What lives there |
|---|---|---|
| Entry points | `renderer.rs`, `headless.rs` | `Renderer` (a window's device, queue, surface, recovery) and `HeadlessRenderer` (rasterise a tree to RGBA8 bytes) |
| Window ownership | `window_target.rs`, `surface_lease.rs`, `adapter.rs` | The owned `'static` handle source a surface is created over; the lease that pins surface-before-target drop order; adapter/device acquisition policy |
| Frame walk | `layer_walk.rs`, `layer_render.rs`, `layer_dispatcher.rs`, `layer_offscreen.rs`, `layer_compositor.rs` | Iterative layer traversal; per-variant lowering of `Layer`; `DrawCommand` → painter routing; offscreen layer rendering and compositing; save/restore layer state |
| Recording | `painter/`, `batches/`, `command_ir.rs`, `state_stack.rs` | `WgpuPainter` (per-frame coordinator), `DrawBatcher` (per-primitive record methods), the Command IR they write, the transform/clip stacks |
| Replay | `replay/`, `pipeline_cache.rs`, `pipeline_set.rs`, `instancing.rs`, `vertex.rs`, `shaders/` | Command IR → wgpu encoding; pipeline caching keyed by blend/coverage state; instance layouts; WGSL |
| Offscreen effects | `offscreen/`, `effects_pipeline.rs`, `blur/`, `mode/`, `gamma/`, `color_matrix/`, `morphology/`, `advanced_blend/`, `ssaa.rs` | Shader masks, backdrop filters, colour filters, dst-read blends, supersampled path AA — each a format-matched pipeline over pooled textures |
| GPU resources | `texture_pool.rs`, `texture_cache.rs`, `buffer_pool.rs`, `uniform_pool.rs`, `path_cache.rs`, `external_texture_registry.rs`, `resources.rs`, `atlas.rs`, `glyph_atlas.rs`, `tessellator.rs` | Pooling, caching, the glyph atlas (rasterised-glyph pages the glyph pipeline samples), and the one adapter over an external crate (`lyon` for tessellation) |
| Raster protocol | `raster.rs`, `raster_owner.rs`, `frame_timing.rs` | `RasterBackend`; the mailbox/ack channel a threaded raster lane uses (ADR-0045); frame timers |
| Damage | `damage.rs` | The per-frame dirty-rect accumulator behind `render_scene`'s scissor (ADR-0061) |
| Test support | `test_support.rs`, `readback_dump.rs`, `fake_window_target.rs`, `blend_oracle.rs`, `*_tests.rs` | Device acquisition, staged readback, the CPU blender oracle, and the readback suites (`cfg(test)`, most under the `testing` feature) |

`wgsl_bindgen` generates the uniform-layout wrappers for the filter shaders
into `OUT_DIR` from `build.rs`; each `<filter>/generated.rs` is the committed
`include!` shim plus the `const` layout assertions that fail the build if the
generated struct drifts from the hand-written one.

`OffscreenRenderer` retains mask and blur pipelines. Their cache misses create
shader modules directly from static WGSL; subsequent draws reuse the pipelines.

---

## One frame

```text
Scene (flui-layer)                one tree per frame, frozen for the raster side
    │
    ▼
Renderer::render_scene            acquires the surface texture, applies damage,
    │                             walks the tree (layer_walk, explicit stack)
    ▼
LayerRender for Layer             one arm per Layer variant; the three diverted
    │                             handlers (BackdropFilter, ShaderMask, Follower)
    │                             consume their subtree and answer SkipSubtree
    ▼
LayerDispatcher                   routes each DrawCommand { transform, op }
    │                             to the painter under that transform and
    │                             owns the clip/opacity discipline
    ▼
WgpuPainter (record)              DrawBatcher writes the Command IR:
    │                             DrawSegment + draw_order: Vec<DrawItem>
    ▼
GpuReplay::submit (replay)        Command IR → render passes, one queue.submit
    │
    ▼
present                           the pre-present hook runs only for a frame
                                  that will present, and before it does
```

`render_scene` is synchronous and single-threaded; the only `async` in the
crate is at the acquisition edges (`Renderer::new`, `HeadlessRenderer::new`,
`Renderer::recover`), because wgpu's adapter and device requests are.

### Record/replay boundary

Two IRs sit in a strict producer/consumer chain (governing decision:
[ADR-0006](../../docs/adr/ADR-0006-c-ir-record-replay-seam.md)).

| Level | Types | Written by | Read by |
|---|---|---|---|
| Scene IR | `flui_painting::DisplayList` / `DrawCommand { transform, op: DrawOp }` | the widget tree's `Canvas` | `LayerDispatcher` |
| Command IR | `command_ir::{DrawSegment, DrawItem}` | `DrawBatcher` (`batches/`) | `GpuReplay::submit` (`replay/`) |

The Command IR is GPU-lowered (baked instance arrays, pixel-space transforms,
opaque `TextureId`s) and holds no pooled texture: textures are acquired at
replay, never stored at record. `DrawSegment` derives `Clone` and the derive
is what bars a `PooledTexture` field (it is `!Clone`, returning its slot on
`Drop`). That is all the derive proves — wgpu 30's own handles are `Clone`
ref-counts — so "the IR holds no GPU handle" is a reading of `command_ir`'s
field types, not a compiler theorem. The deterministic-replay test records
one scene and replays it to two independent targets, asserting byte-identical
pixels; that is the runtime evidence that replay is a pure function of the IR.

`WgpuPainter` is a coordinator, not a recorder: it holds `GpuStateStack`
(transform / scissor / SDF-clip stacks), `LayerCompositor` (save-layer
state), `GpuResources`, `PipelineSet`, the open `DrawSegment` and
`draw_order`, and `GpuReplay`. Its `draw_*` methods field-split `self` and
hand `DrawBatcher` the narrowest disjoint borrows it needs (`&mut
DrawSegment`, `&GpuStateStack`, …) — never `&mut WgpuPainter` — so the
borrow checker is what keeps record logic out of the coordinator.

`GpuStateStack` stores transforms as `glam::Mat4`; the conversion from
`flui_types::Matrix4` happens once, at `LayerDispatcher`'s `CommandRenderer`
implementation, and `Matrix4` never appears in `batches/`, `pipeline_set.rs`,
or `replay/` (port-check trigger 19). Direct `glam` is expected in the GPU
modules and refused in the GPU-free ones (`raster*`, `dispatch`, `error`,
`fonts`, `frame_timing`, `layer_state_stack`, `superellipse`; port-check
`N-geom.U16`).

### Layer traversal is iterative

Both walkers over a `LayerTree` — `Renderer`'s and `HeadlessRenderer`'s —
share the explicit-stack traversal in `layer_walk.rs`, parameterised by a
two-step visitor (`enter` answers `Descend` or `SkipSubtree`; `exit` runs the
node's post-children cleanup). A Rust stack overflow is a process abort, not
a panic, so one frame per layer would take the render path down on a
deep-but-valid chain; `layer_walk.rs`'s own tests pin paint order, the
cleanup-after-subtree ordering, the no-descend/no-cleanup rule for
`SkipSubtree`, and a 10 000-deep walk on a small stack. The GPU-side
evidence is the readback suite plus
`a_deep_layer_chain_captures_without_overflowing_a_small_stack`.

---

## wgpu surface and backend features

| FLUI module | wgpu API surface |
|---|---|
| `renderer.rs` `Renderer` | `Instance`, `Adapter`, `Device`, `Queue`, `Surface<'static>`, `SurfaceConfiguration`, `TextureFormat`, `PresentMode`, `SurfaceError`, `CompositeAlphaMode` |
| `painter/` `WgpuPainter`, `replay/` `GpuReplay` | `Buffer`, `RenderPipeline`, `BindGroup`, `ShaderModule`, `Texture`, `TextureView`, `Sampler`, render-pass descriptors |
| `offscreen/`, the filter pipelines | `RenderPipeline`, `BindGroupLayout`, `BindGroup`, `Sampler` over pooled textures |
| `pipeline_cache.rs` / `pipeline_set.rs` | `RenderPipelineDescriptor`, `VertexBufferLayout`, `ColorTargetState`, `BlendState`; `Features::DUAL_SOURCE_BLENDING` gates the coverage-correct variants |
| `texture_pool.rs` | `TextureDescriptor`, `TextureUsages` |
| `tessellator.rs` | none — the adapter over `lyon` |
| `glyph_atlas.rs` | `Texture`, `TextureView`, `Sampler`, `BindGroup` for the two pages; `etagere` packs them |

The Cargo features `vulkan`, `metal`, `dx12`, `webgpu`, `gles` are **additive
pass-throughs to wgpu's own backend features, not selectors**. Nobody
normally names them: the per-target dependency entries in `Cargo.toml` unify
the right backend into every build (`dx12` on Windows, `metal` on macOS/iOS,
`vulkan` on Linux/Android, `webgpu` + `gles` on wasm32), and
`Renderer::select_backend()` picks at runtime per `target_os`. Enabling two
compiles both; no source in this crate cfg-gates on them. The two
`compile_error!` guards in `lib.rs` cover the invalid pairs: wasm32 with
atomics while `fragile-send-sync-non-atomic-wasm` is on, and wasm32 with
`gpu-profiler`. The per-target entries must stay non-optional — an
`optional = true` there leaves wgpu with no backend at all, and every
`Instance::new` panics; the ordinary test suite catches that, `cargo hack
check` does not.

`testing` gates the GPU readback/oracle suites and the bench scaffolding they
share (`OffscreenRenderer`, `TexturePool`, `PathCache` re-exports). It is off
by default, not part of the public API, and the same name and meaning as
flui-layer's and flui-rendering's `testing`. CI's `gpu-test` job runs the
suites on WARP with `FLUI_REQUIRE_GPU=1`, so a missing adapter fails there
instead of silently skipping.

---

## Ownership and thread safety

`flui-engine` runs on the render thread. No `Arc<Mutex<_>>` guards any
engine subsystem (port-check trigger 7 watches for one returning); every
shared handle is a wgpu ref-count.

| Site | Type | Ownership |
|---|---|---|
| `Renderer::device` / `queue` | `Arc<wgpu::Device>` / `Arc<wgpu::Queue>` | wgpu's own ref-counted handles, shared with `WgpuPainter` and `OffscreenRenderer` at setup |
| `Renderer::lease` | `SurfaceLease<wgpu::Surface<'static>>` | Owned. The lease keeps the presentation target alive as long as the surface exists; `release()` hands back a `#[must_use] Released` token that only `replace_surface` consumes, so a recreate cannot skip the drop-order step and a released renderer cannot present |
| `Renderer::painter`, `Renderer::offscreen` | `WgpuPainter`, `OffscreenRenderer` | Owned outright; borrowed disjointly per frame by `LayerDispatcher<'frame>` |
| `Renderer::_single_mutator` | `PhantomData<Cell<()>>` | Makes `Renderer: !Sync` by declaration rather than by whichever field happens to be `!Sync`; pinned by `assert_impl_all!(Renderer: Send)` / `assert_not_impl_any!(Renderer: Sync)` |
| `OffscreenRenderer::mask_painter` | `Option<WgpuPainter>` | The painter a shader mask renders its subtree with, cached across frames and rebuilt when the requested size changes |
| `TexturePool` | inventory + mpsc return channel | Single-mutator by construction; `Send` only (the receiver is `!Sync`) |
| `RasterOwner` mailbox | `parking_lot::Mutex` + condvar + two atomics + bounded crossbeam channels | The one module with its own memory model; its orderings are argued in `InFlightAccounting`'s doc |

**Unsafe.** There is no hand-written production `unsafe` in the crate:
`#![cfg_attr(not(test), deny(unsafe_code))]` in `lib.rs`. Surfaces come from
wgpu's safe `create_surface` over an owned `WindowTarget`
([ADR-0063](../../docs/adr/ADR-0063-the-renderer-owns-its-surface-target.md)),
and there is no manual `Send`/`Sync` impl. `deny` rather than `forbid` because
`wgsl_bindgen`'s generated wrappers declare an `unsafe fn from_raw` the crate
never calls and `allow` it locally. `fake_window_target.rs` uses test-only
raw-handle borrows for synthetic window and display handles.

**`raster_owner` under Miri and loom.** The mailbox's non-blocking tests run
under Miri (36 pass; the ten that block on the condvar are filtered out
because `parking_lot_core`'s futex call is outside Miri's model). Loom 0.7
models `std::sync` and an unbounded std-shaped `mpsc`, while this module's
trickiest invariants live in `crossbeam_channel::bounded`'s `try_send` /
`Full` path — a `cfg(loom)` shim would test approximations of exactly the
mechanisms under question, so it is declined. What stands in its place is
the threaded harness in the module (real OS threads, real primitives,
high-repetition races, a panic-unwind wake test); it exercises interleavings
by timing, not exhaustively. Reopen when `parking_lot`/`crossbeam` grow a
loom backend or the mailbox moves to `std::sync`.

---

## Mapping decisions

### Encoded sRGB surface presentation

The current shaders emit the encoded components supplied by `Color::to_f32_array`;
blending remains in that encoded space. Windowed rendering therefore selects only
an advertised `Bgra8Unorm` or `Rgba8Unorm` format paired with explicit `Srgb`
presentation, preferring BGRA when both work. Format and presentation color space
are one contract: an FP16 surface with `Auto` can select `ExtendedSrgbLinear`,
causing the compositor to brighten already encoded values. An sRGB texture format
would likewise encode those values a second time on storage.

This follows [wgpu's per-format surface capabilities](https://docs.rs/wgpu/30.0.1/wgpu/struct.SurfaceCapabilities.html)
and [explicit presentation color-space contract](https://docs.rs/wgpu/30.0.1/wgpu/enum.SurfaceColorSpace.html).
A backend name is not evidence of display HDR support. `GpuCapabilities::supports_hdr`
is removed; actual HDR requires a future end-to-end color-management contract.
There is no arbitrary format fallback: incompatible capabilities produce
`UnsupportedSurfaceColorConfiguration` before configuration or recreation commits.
The error is nonretryable; failed recreation retains the released surface lease.

Selector tests cover format order, both UNorm alternatives, and unsupported pairs.
GPU tests draw through the production painter/shaders and verify dark, midtone,
colored, saturated, and partial-alpha swatches in both byte layouts. These tests
preserve the existing encoded-space blending behavior; they do not claim HDR output.

Where this crate's shape is a deliberate choice rather than the obvious
transcription. Protocol-level contracts point at their ADR; the rest are
local. Each names the test that pins it.

### 1. wgpu is the engine; the crate is flat

No `wgpu` module, no `wgpu-backend` feature, no `RasterBackend` impl for a
boxed backend. `flui_engine::wgpu` names the linked wgpu crate so an
embedder that hands over a device or reads a surface format names the same
version without a second dependency line (the `egui-wgpu` / `iced_wgpu`
convention). `RasterBackend` survives as the test seam flui-app's frame loop
is driven through with a fake, and is documented as that. A trait or
feature whose only justification is a backend swap is deleted, not kept
warm.

### 2. Closed `LayerRender` static dispatch, not a `Box<dyn Backend>` plugin

`LayerRender<R: CommandRenderer + ?Sized>` has one arm per `Layer` variant
and is generic over the renderer, so adding a variant is a compile error in
both crates rather than a silently-ignored layer, and the hot path pays no
vtable. The one `dyn` on the frame path is the `PrePresentHook` closure.

### 3. `Renderer::new` owns its surface target — [ADR-0063](../../docs/adr/ADR-0063-the-renderer-owns-its-surface-target.md)

`WindowTarget` is an owned, `'static`, `Send + Sync` handle source; the
surface is created over an `Arc<dyn WindowTarget>` and `SurfaceLease`
guarantees the surface drops before the target it borrows. The compile-fail
fixture `renderer_new_rejects_borrowed_window` pins that a borrow cannot be
handed in; `surface_lease.rs`'s tests pin the drop order and that a released
lease cannot present.

### 4. Clip coverage on a second blend source, capability-gated — [ADR-0057](../../docs/adr/ADR-0057-coverage-correct-blending-is-capability-gated.md)

The tessellated shape shader and the three instanced gradients emit clip
coverage as `@blend_src(1)`, and the seven blend modes whose destination
factor cannot absorb `1 − coverage` (`Clear`, `Src`, `SrcIn`, `SrcOut`,
`Modulate`, `DstIn`, `DstATop`) take `dst_factor = OneMinusSrc1`.
`pipeline_cache::destination_alpha_scale_for` classifies the modes — a
property of the factor pair, the same partition `is_tile_safe_for_ssaa`
draws. Where `DUAL_SOURCE_BLENDING` is absent (WebGPU), `PipelineCache`
compiles only the folded assembly and those modes keep a hard anti-aliased
clip edge; both halves are pinned by the readback suite. The rect / circle /
texture instanced quads are not corrected because they are wired to `SrcOver`
only, which absorbs partial coverage already.

### 5. A gradient's blend mode is pipeline state, keyed per draw run

`GradientPipelines` caches one pipeline per `(GradientKind, BlendMode)` and a
gradient run (`command_ir::GradientRun`) carries the mode beside the scissor,
so a run ends when either changes. Before this the three gradient pipelines
were built with a hard-coded `ALPHA_BLENDING`, and every non-advanced mode a
caller set was accepted and discarded. Consequence: the gradient fragment
emits premultiplied colour and `SrcOver` moved to
`PREMULTIPLIED_ALPHA_BLENDING`, which is not byte-identical to the old
straight-alpha path (below one part in 255; the readback tolerances absorb
it). A gradient with more than eight stops is truncated and warned about
once per process, not silently.

### 6. A path clip installs the path's bounding box, not nothing

`WgpuPainter::clip_path` installs `Path::compute_bounds()` as a hardware
scissor, grown outward to whole pixels *after* the transform
(`GpuStateStack::clip_rect_enclosing`; growing in local space is re-fractioned
by any fractional translation and buys nothing). It does not clip to the
shape: a bounding box is a superset, so it can only remove what the exact
clip also removes, and what still renders — inside the box, outside the
shape — is the whole remaining gap, pinned by
`a_path_clip_lets_through_what_lies_inside_the_box_but_outside_the_shape`
so that a future stencil pass changes a named promise rather than closing
the gap unnoticed. An empty path clips everything (the one exact case,
`an_empty_clip_path_clips_everything`). Installing nothing, the previous
answer, is not a smaller approximation — it is the absence of a clip, and a
`draw_paint` inside a `Material`'s clip painted the whole window.

### 7. The squircle is an SDF only; the CPU generator is its oracle

`push_clip_rsuperellipse` clips through the superellipse SDF in the shape
shader (`shaders/common/clip.wgsl`). The tessellating route, its cache, and
the trait method that reached it are gone; `superellipse.rs`'s generator
stays because it is the only CPU statement of the same `n = 4` parametric
form, and the shader's correctness argument is tested against it. The scissor
behind any SDF clip is the AABB of the transformed box under a rotation
(`painter/transform_clip.rs`), so the approximation loosens, never tightens.

### 8. A clip op this backend cannot express is refused, never inverted

`ClipOp::Difference` keeps a shape's complement; a scissor is one rectangle
and the per-draw SDF slot evaluates the shape, not its inverse, so no
primitive here can honour it. `clip_op_is_expressible` refuses it on all four
clip shapes — no clip installed, one `tracing::warn!` naming the shape.
Three of the four used to bind the op as `_clip_op` and install an
*intersect*, erasing everything outside the hole the caller asked to punch:
refusing is permissive (extra content the caller can see), inverting was
destructive (content gone with nothing left to look at). Honouring it needs a
clip stack that can evaluate `1 − coverage`, and waits for that.

### 9. `Clip::AntiAliasWithSaveLayer` opens an offscreen bounded by the clip's own scissor, and declines it inside an image filter

Flutter clips anti-aliased and then `saveLayer`s with the clip's bounds. Here
`LayerDispatcher::opens_offscreen` grants the offscreen when the mode asks
and no enclosing layer routes through a bounds-growing image filter; the
layer's bounds are the scissor the clip just installed (already intersected
with every ancestor clip), which cannot cut content and keeps the composite
to one textured quad. Inside an image-filter layer the offscreen is declined:
those layers carry only their final `DrawSegment` into `FilterOp::input`, so
an opacity layer opened inside one would be discarded together with every
sibling already flushed — degrading to per-draw coverage loses an edge,
opening the layer loses the picture.
`a_clip_inside_an_image_filter_layer_keeps_its_content_and_its_siblings`
pins both halves. A rect clip under this mode is honoured by the scissor
alone; an offscreen would buy nothing there.

### 10. An offscreen result composites with the mode its producer recorded

`PendingOffscreenTexture` carries the producer's `BlendMode`;
`queue_offscreen_result` takes it; replay routes every mode through
`PipelineSet::ensure_texture_composite`'s per-mode pipeline. Shader-mask,
backdrop-filter, and opacity-layer results all go through it, so a mask layer
composited `Clear` erases instead of drawing.
`an_offscreen_result_composites_with_its_own_blend_mode` fails on the
`SrcOver`-always code. `OffscreenRenderer::render_masked` takes no blend
mode: it produces a premultiplied full-coverage offscreen, and the mode
belongs to the step that draws it back.

### 11. The shader-mask painter is cached across frames

A `WgpuPainter` is nine pipelines and a glyph atlas. The painter a shader
mask renders its subtree with used to live on the per-frame
`LayerDispatcher`, so every frame with a mask rebuilt it; it lives on
`OffscreenRenderer::mask_painter` now, rebuilt only when the requested size
changes, and `Renderer::handle_shader_mask` borrows the offscreen renderer
once for the whole capture.

### 12. A zero-sized resize mints nothing

`RasterOwner::resize(0, h)` returns `None` instead of a fresh
`SurfaceGeneration` that every subsequent submit would be rejected against;
the caller (`flui-app`'s raster lane) keeps its last generation. A window
minimised to zero is a pause, not a new surface epoch
(`a_zero_sized_resize_mints_nothing_and_queues_nothing`).

### 13. Frame failure does not leak painter state

`render_scene_content` returns `EngineResult`; on the error path the painter's
end-of-frame maintenance still runs before the error propagates, so the next
frame starts from balanced stacks rather than the failed frame's leftovers.
A `WakeGuard` in `raster_owner` does the same for the threaded lane: a panic
mid-pump still publishes the completion and retires the frame on unwind.

### 14. `GpuServices` and `RasterOptions`: deleted, not carried

Two public types whose every reader was their own test. `GpuServices`
(ADR-0045 decision 2) shipped only its offscreen half while the working
windowed entry point was hidden; `RasterOptions` (decision 6) was a DTO
`RasterOwner` stored and never read, advertising a range the capacity-one
mailbox could not reach. Both ADR decisions carry an implementation-status
paragraph; the pacing surface returns with a real consumer.

### 15. One gradient path: a shader on a fill paint

`CommandRenderer::render_gradient`/`render_gradient_rrect` and the painter's
`draw_gradient_rect` / `draw_radial_gradient_rect` / `draw_sweep_gradient_rect`
/ `draw_shadow_rect` are deleted with the display-list variants they served
([ADR-0066](../../docs/adr/ADR-0066-display-list-command-representation.md)).
A gradient arrives as `Paint::shader` on `Rect`/`RRect`/`Circle` and reaches
`DrawBatcher::dispatch_shader_rect`, which is the only lowering: it carries
the paint's blend mode (decision 5), its `anti_alias`, the painter's
transform, and the rounded rect's real `[tl, tr, br, bl]`. The deleted path
had none of those — it collapsed the corners to one radius and handed the
batcher untransformed bounds — so this is a fix as much as a deletion;
`gradient_rrect_keeps_per_corner_radii` is red against a uniform radius.
`GradientStop` and `ShadowParams` are crate-private batch payloads now, with
no root re-export.

### 16. Text is a glyph batch of the segment; the engine owns the atlas

glyphon is gone ([ADR-0067](../../docs/adr/ADR-0067-engine-owned-glyph-atlas.md)).
A paragraph is recorded by `DrawBatcher::draw_paragraph`: each glyph the
layout places (`TextLayout::placed_glyphs`, an opaque `GlyphKey` per glyph)
is looked up in `GlyphAtlas`, rasterised on first use through
`SharedFontSystem::rasterize`, and pushed as a `GlyphInstance` into
`DrawSegment::glyph_batch` under the same scissor run, SDF clip, and layer
opacity every other instance gets. `Phase::Glyph` is the last phase, and
`flush_segment` draws the batch either at the end of the instanced pass
(when no gradient/tessellated/image phase sits between) or in its own pass.
What that deleted: the per-segment glyph ranges (`text_start..text_end`),
the "claimed text" bookkeeping and the trailing gap passes that drew text
captured by a filter or advanced shape over everything, `seal_text_tail`,
one render pass per text-bearing segment, and the sRGB→linear colour
conversion glyphon applied to text on a gamma-space target
(`glyph_colour_lands_as_recorded`). A rotated or anisotropic CTM reaches
the glyphs: each quad carries the CTM's linear part over the raster scale
(`anisotropic_scale_squashes_glyphs_on_one_axis`). The engine names no
cosmic-text type
(`the_engine_does_not_shape`); `etagere` stays behind `glyph_atlas.rs` the
way `lyon` stays behind `tessellator.rs`.

---

## Open items

- **Threaded raster lane (ADR-0045, Proposed).** `raster_owner`'s mailbox,
  ack channel, and `InFlightAccounting` exist for a lane that flui-app does
  not yet drive; whether the lane ships or the direct path is the only path
  decides whether this family stays. Until decided, it is tested but not
  wired.
- **`catch_unwind` around `render_scene`.** A panic inside a layer's paint
  poisons the frame rather than isolating the layer, which is a deliberate
  divergence from Flutter's per-layer isolation; changing it is a contract
  change that needs its own ADR.
