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

The contract for the widget-facing behaviour is what a layer means and in what
order it paints; the reference for everything below the `Scene` is wgpu itself.
Cross-crate contracts carry an ADR; crate-local shapes are recorded under
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
| Offscreen effects | `effects_pipeline.rs`, `blur/`, `mode/`, `gamma/`, `color_matrix/`, `morphology/`, `advanced_blend/`, `ssaa.rs` | Ordered shader masks and backdrop filters, colour filters, dst-read blends and supersampled path AA over pooled textures |
| Presentation blit | `offscreen/` | Cached intermediate-to-surface copy, independent of layer effects |
| GPU resources | `texture_pool.rs`, `texture_cache.rs`, `buffer_pool.rs`, `uniform_pool.rs`, `path_cache.rs`, `external_texture_registry.rs`, `resources.rs`, `atlas.rs`, `glyph_atlas.rs`, `tessellator.rs` | Pooling, caching, the glyph atlas (rasterised-glyph pages the glyph pipeline samples), and the one adapter over an external crate (`lyon` for tessellation) |
| Raster protocol | `raster.rs`, `raster_owner.rs`, `frame_timing.rs` | `RasterBackend`; the mailbox/ack channel a threaded raster lane uses (ADR-0045); frame timers |
| Damage | `damage.rs`, `retained_target.rs`, `frame_protocol.rs` | The dirty-rect accumulator behind `render_scene`'s scissor (ADR-0061), `plan_frame` (where a frame renders), `begin_partial` (the scissored clear), the retained target a partial frame repaints into (ADR-0087 §4), and `FrameProtocol`, the plan-to-GPU sequence the renderer and the readback capture share |
| Test support | `test_support.rs`, `readback_dump.rs`, `fake_window_target.rs`, `blend_oracle.rs`, `*_tests.rs` | Device acquisition, staged readback, the CPU blender oracle, and the readback suites (`cfg(test)`, most under the `testing` feature) |

`wgsl_bindgen` generates the uniform-layout wrappers for the filter shaders
into `OUT_DIR` from `build.rs`; each `<filter>/generated.rs` is the committed
`include!` shim plus the `const` layout assertions that fail the build if the
generated struct drifts from the hand-written one.

`OffscreenRenderer` caches only the intermediate-to-surface presentation blit.
Shader masks and backdrop filters use ordered painter IR and the shared layer
recorder. The `offscreen_resource_cache` benchmark exercises public layer captures
(including GPU completion and RGBA readback), with pixel preconditions before timing.

---

## Embedder-owned frames

Public `WgpuPainter` embedders check `begin_frame`'s result before recording, may flush
with `render_to_view` several times, then call `finish_frame` once after the
final encoder submission (or after discarding an encoder on error). This makes
state reset and resource maintenance reachable without exposing the internal
renderer. The window renderer retains its existing private multi-pass calls.
The public frame owns the device-domain submission scope until `finish_frame`;
retiring intermediate submissions does not renew its cumulative work allowance.
Nested begin is rejected. Internal child painters reset recording inside their
owner's existing scope instead of opening another frame.
The default prior-work backlog window is checked before opening the scope. Its
64-submission threshold is independent of a frame's cumulative allowance, which
derives from prepared object and CPU metadata budgets. Submission bookkeeping is
charged through completion. Native admission first polls nonblockingly so rejected
frames cannot starve completion callbacks; browser progress uses its event loop.
Recoverable failure publishes reliable retry debt to the application and UI runtime
(ADR-0101), while an impossible frame footprint remains a terminal error.
`examples/embedded_gpu_scene.rs` exercises the public lifecycle with an external
GPU texture, a depth-tested producer pass and foreground 2D drawing on the same
device; its `--capture` mode asserts fixed-angle pixels after two frames.

## One frame

```text
Scene (flui-layer)                one tree per frame, frozen for the raster side
    │
    ▼
Renderer::render_frame            plans the frame from its damage (skip, direct,
    │                             or into the retained target), acquires the
    │                             surface texture, clears (in full, or inside the
    │                             damage scissor), walks the tree (layer_walk,
    │                             explicit stack); a retained frame is blitted
    │                             to the surface texture at the end
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
GpuReplay::submit (replay)        ordered Command IR → render passes
    │                             DeviceDomain owns each queue submission
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

Recording appends typed `DrawRun` ranges in painter order. Sealing consumes the
mutable segment into `SealedSegment`; nested filters, opacity, advanced shapes
and SSAA carry the same read-only boundary. Replay remaps an independently
charged scratch copy when an offscreen coordinate system requires it. Gradient
tables and viewport uniforms are immutable per encoded flush, and buffer-pool
slots stay distinct until the frame's encoders are submitted or discarded.

`RecordingBudget` bounds requested arena capacity and live element count for a
painter recording session, shared across sealed, isolated and remapped segments.
Admission precedes growth; the first failure stays latched until a new frame.
It excludes allocator slack, `DrawItem` container metadata, Lyon output, caches,
image decoding and separate painters. `DeviceDomain` separately admits the
listed prepared GPU payloads and retains submission charges until callbacks.
Neither quota is a whole-process memory or physical VRAM limit.

The Scene IR remains GPU-free and names logical `TextureId`s. At engine lowering,
an external draw captures an immutable allocation lease: view, validated
interpretation and weak device-domain owner. Replacing or unregistering that ID
cannot redirect an already recorded draw. A later lowering resolves the new
allocation. The lease does not snapshot texels: ordered producer writes to the
same allocation remain visible to subsequent GPU execution.

Command IR carries cloneable external leases but no `PooledTexture`; offscreen
pool slots are still acquired at replay and returned by their non-cloneable
owners. Recorded, remapped and submitted references retain imported allocations.
Managed submission transfers leases and their bookkeeping charges to completion;
CPU `finish_frame` is not evidence of GPU completion. The weak domain stamp avoids
a queue/callback/domain cycle and checks engine routing, not raw wgpu provenance.

`WgpuPainter` is a coordinator, not a recorder: it holds `GpuStateStack`
(transform / scissor / SDF-clip stacks), `LayerCompositor` (save-layer
state), `GpuResources`, `PipelineSet`, the open `DrawSegment` and
`draw_order`, and `GpuReplay`. Its `draw_*` methods field-split `self` and
hand `DrawBatcher` the narrowest disjoint borrows it needs (`&mut
DrawSegment`, `&GpuStateStack`, …) — never `&mut WgpuPainter` — so the
borrow checker is what keeps record logic out of the coordinator.

`GpuStateStack` stores transforms as `glam::Mat4`; the conversion from
`flui_foundation::geometry::Matrix4` happens once, at `LayerDispatcher`'s `CommandRenderer`
implementation, and `Matrix4` never appears in `batches/`, `pipeline_set.rs`,
or `replay/`. Direct `glam` is expected in the GPU modules and kept out of
the GPU-free ones (`raster*`, `dispatch`, `error`, `fonts`, `frame_timing`,
`layer_state_stack`, `superellipse`).

### Layer traversal is iterative

Both walkers over a `LayerTree` — `Renderer`'s and `HeadlessRenderer`'s —
share the explicit-stack traversal in `layer_walk.rs`, parameterised by a
two-step visitor (`enter` answers `Descend` or `SkipSubtree`; `exit` runs the
node's post-children cleanup). A Rust stack overflow is a process abort, not
a panic, so one frame per layer would take the render path down on a
deep-but-valid chain; `a_deep_chain_survives_a_small_stack` in
`layer_walk.rs` pins a 10 000-deep walk on a small stack.

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
share (`OffscreenRenderer`, `PathCache` re-exports). It is off
by default, not part of the public API, and the same name and meaning as
flui-layer's and flui-rendering's `testing`. The local `cargo xtask gpu-test`
command sets `FLUI_REQUIRE_GPU=1` for both the engine and facade readback
suites, so missing adapter/device initialization fails instead of silently
skipping. Linux-only CI does not supply the native GPU runtime coverage.

---

## Ownership and thread safety

`flui-engine` runs on the render thread. No `Arc<Mutex<_>>` guards any
engine subsystem, and none should come back; every
shared handle is a wgpu ref-count.

| Site | Type | Ownership |
|---|---|---|
| `Renderer::device` / `queue` | `Arc<wgpu::Device>` / `Arc<wgpu::Queue>` | wgpu's own ref-counted handles, shared with `WgpuPainter` and `OffscreenRenderer` at setup |
| `Renderer::lease` | `SurfaceLease<wgpu::Surface<'static>>` | Owned. The lease keeps the presentation target alive as long as the surface exists; `release()` hands back a `#[must_use] Released` token that only `replace_surface` consumes, so a recreate cannot skip the drop-order step and a released renderer cannot present |
| `Renderer::painter`, `Renderer::offscreen` | `WgpuPainter`, `OffscreenRenderer` | Owned outright; the painter records content and the offscreen renderer blits retained presentation |
| `Renderer::_single_mutator` | `PhantomData<Cell<()>>` | Makes `Renderer: !Sync` by declaration rather than by whichever field happens to be `!Sync`; pinned by `assert_impl_all!(Renderer: Send)` / `assert_not_impl_any!(Renderer: Sync)` |
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

### Solid shader fills own the source RGBA

A `Shader::Solid` fill on a rectangle, rounded rectangle or circle takes its
whole RGBA from the shader and ignores `Paint::color`, alpha included, as every
gradient shader does. It is drawn as two identical gradient stops, so transforms,
captured clips and blend modes take the gradient routes, and save-layer opacity
stays a group composite.
`painter_solid_shaders_use_effective_colors_for_compositing` compares it with a
plain paint of the shader's color.

### Vertex colors and clip coverage own the source alpha

Supplied mesh colors replace `Paint::color`; an absent color array uses the paint
color. The shape fragment interpolates straight RGBA and then premultiplies it.
Every untextured `SrcOver` mesh uses alpha blending: clip coverage can make even
opaque vertex or paint colors translucent. Explicit fixed-function and advanced
blend modes retain their existing routing. Parent opacity remains a group
composite rather than mesh color baking.
`painter_vertices_use_effective_colors_for_compositing` reads back direct painter
and Canvas-recorded draws with uniform, mixed and zero alpha, opaque colors under
a contradictory paint, paint fallback, `Src`, `Clear`, `Plus`, `Multiply`, parent
opacity and ignored invalid input followed by a visible sibling. Its fractional
clip rows separately cover supplied opaque colors and opaque paint fallback,
requiring half-covered red over blue to preserve the destination's opaque alpha.
The optimized FXC row below exercises this same producer with analytic and mask
clips; its expected coverage depends on this blending contract.

### Shared clip helpers compile with optimized FXC

Shared clip coverage and rounded-superellipse distance helpers initialize their
results and use one return path. Fragment derivatives are evaluated before the
terminal discard. This preserves analytic membership and fractional coverage
while avoiding an optimized FXC fragment translation failure in the specialized
shape pipeline. The DEBUG instance flag skips FXC optimization and would hide
the failure, so the contract test runs without it.

`optimized_fxc_tessellated_clip_readback` runs inside
`renderer_surface_selection_and_layer_compositing_read_back_as_specified` on
Windows with an actual DX12 adapter, explicitly selected FXC and no DEBUG flag.
It records direct vertex meshes through the production scene and painter, checks
validation and internal error scopes, and reads pixels for unclipped, hard and
antialiased superellipse clips, nested masks, Src, Clear and reflection. The
helpers with early returns fail to compile under this configuration. Each case
creates its own painter, so the row does not cover painter state carried across
frames.

### Command transforms do not own clips

The dispatcher caches a command matrix separately from its ambient layer CTM.
Switching to another command matrix, identity, or a layer boundary restores
only the ambient CTM. A clip captures its geometry under the CTM at recording
and persists until its explicit state scope is restored; the transform cache
does not save or restore the clip stack. The public headless readback row
`command_transform_changes_preserve_captured_clips` in
`clip_layers_read_back_as_the_clip_contract_specifies` covers all current clip
shapes, same/different/identity command matrices, an ambient layer offset and
explicit save/restore. This fixes clip lifetime, not exact path clipping or
nested analytic intersections; those remain in the clip/effect implementation
plan.

Each display list dispatch opens and closes a state scope, so its local clips
and CTM do not escape into sibling pictures. Command SaveLayer owns a state
scope as well as compositor state; RestoreLayer closes both. The same readback
family's `display_list_and_save_layer_scopes_own_their_clips` pins sibling
isolation, restoration after a clipped group and group opacity. These scopes
assume a balanced recorded list; they do not introduce an unwind containment
boundary or validation of malformed manually assembled command streams.

### External texture interpretation and allocation identity

Registration admits only one-layer D2, single-sample, texture-bindable
`Rgba8Unorm` or `Bgra8Unorm` allocations. The view selects mip zero explicitly.
Dimensions come from wgpu metadata. Duplicate registration and incompatible
replacement return typed errors before view creation and preserve the old entry.
Replacement preserves the descriptor; rebinding requires unregister/register.
Raw imports remain trusted: wgpu does not expose a device-provenance or destroyed
state query, and an external alias can still destroy its allocation.

Explicit per-draw filtering selects nearest for `FilterQuality::None` and
bilinear mip-zero sampling for Low/Medium/High. Resource-sampling draws use the
registered default. Straight, premultiplied and opaque alpha have separate
interpretation; opacity scales premultiplied RGB and alpha together.
Straight-alpha linear path premultiplies four mip-zero texel loads before
interpolation, then uses premultiplied compositing and clip coverage. Filtering
unmultiplied RGB and alpha separately would darken translucent edges and leak
hidden colors. Nearest, opaque and premultiplied sources use hardware sampling.
The current color contract is encoded sRGB blended in encoded space. SRGB views, linear-light,
wide-gamut and HDR inputs are rejected rather than silently reinterpreted.

The painter readback family
`painter_images_and_offscreen_results_read_back_as_specified` pins sampling,
allocation replacement, alpha, validation, first-error preservation and recovery.
Allocation identity is pinned, while producer writes to that allocation remain
visible. The engine's behavior tests cover nearest/linear overrides, transparent
texels with hidden RGB, half opacity, RGBA/BGRA sources, update/rebind after
recording, incompatible updates, competing errors and the next valid frame.
Shader-mask captures borrow their parent's external registry only while lowering
the child subtree; their cached painter owns no copied registrations. Captured
leases follow the same domain and completion protocol as direct draws. The
`renderer_surface_selection_and_layer_compositing_read_back_as_specified`
family pins masked external draws, replacement with a reused painter, resize
and recovery after a failed lookup.
Bindings reuse the actual pipeline layout within a frame and retain their quota
charge through cache ownership and submitted work. Imported GPU allocation bytes
are excluded from prepared-resource quotas; a managed producer factory and the
whole-engine ledger remain follow-ups in the resource migration plan.

### Bounded gradient work and failed frame diagnostics

Linear, radial and sweep shaders currently scan stops linearly. Each draw accepts
at most 256 stops, independently of the recording memory budget; exceeding this
returns `PreparedResourceLimit` when rendering, never silent truncation. The
`gradients_read_back_as_specified` family covers nine-stop output, rejection of
100,000 stops for each kind, and the next valid frame. This bound limits one
fragment loop, not total frame work or a guaranteed GPU execution duration.

`painter_images_and_offscreen_results_read_back_as_specified` covers immutable
viewport bindings for resized offscreen-only flushes and cumulative public frame
submission limits after GPU retirement. Recording admission uses atomic counters
to preserve `Send` without a mutex in the successful recording path; exact-size
index batches charge once. The first failed admission remains authoritative.

`renderer_surface_selection_and_layer_compositing_read_back_as_specified` checks
that domain quarantine reaches the actual backend recovery predicate even without
the driver's callback. With `gpu-profiler`, its failure matrix discards incomplete
query frames on error or unwind, then checks that repeated successful frames
contain only their own scopes. An abort only marks profiler state; the next frame
replaces it before recording, without making GPU calls during unwind cleanup.

Headless capture serializes each renderer's complete render/readback operation
under a host-level mutex, so concurrent `&self` callers wait rather than collide
on the domain's active frame. `twin_renderers_tear_down_without_blocking` also
covers overlap and the next capture after a poisoned gate. `FrameAlreadyActive`
is a caller protocol error, distinct from transient GPU backpressure.
The same mutex owns an optional painter: successful captures retain its GPU
pipelines and caches, reset frame state and resize before the next recording.
The painter leaves the slot during capture and returns only after successful
readback; an error or unwind discards it. The same headless family reads changing
viewport sizes, clipped then unclipped frames, and a valid frame after invalid
geometry. These caches live as long as this renderer; the prepared-resource
quota does not claim to bound every legacy cache allocation.
An invalidated partial source returns recoverable `MissingRetainedSource` before
taking the spare target; the invalid-target family checks the retry and quota.

Adjacent tessellated geometry merges only when the last recorded run is tessellated
and pipeline, scissor, resolved clip and contiguous index ranges match. A solid
draw between two paths is an ordering barrier. The painter readback family checks
this under a bounded budget and with intervening solids and different clips.

Warm path-cache draws stream recoloured, transformed vertices directly into the
recording arena. The normal, advanced-blend and SSAA paths consume the same
iterator contract, with isolated bounds computed from their admitted geometry.
`warm_path_cpu_record` measures the public recording consumer rather than the
borrowed cache lookup alone. `warm_path_recording_has_no_per_draw_vertex_allocation`
uses the existing isolated allocator binary to count warmed public draws, allowing
geometric arena growth but rejecting a temporary allocation per path.
`cached_paths_reconstruct_colour_and_transform` reads back a cache hit with a new
colour and translation through normal, SSAA and advanced blend routes.

Dashed strokes consume Lyon's lazy, scale-aware flattened path events rather
than a list of lines that loses contour boundaries. A closed contour includes
its implicit last-to-first edge; an unfinished dash ends before a disconnected
contour begins. Dash phase continues across contours using travelled length,
without counting the spatial gap. On a closed contour, on-dash coverage on both
sides of the starting point forms a join rather than two caps. One uninterrupted
dash becomes a closed Lyon path; separate first/last dash fragments are merged
through the seam, keeping their other pattern boundaries capped. A gap at the
seam remains a gap. Painter rows `uninterrupted_closed_dash_uses_miter_join`,
`exact_perimeter_closed_dash_uses_miter_join` and `wrapped_closed_dash_uses_miter_join`
compare the miter corner with a solid stroke. `closed_dash_gap_keeps_seam_open`
excludes a join where the pattern is off at the seam.
Lyon's point-sampling walker cannot replace
this iterator: a stroke also needs the corners between dash boundaries. Kurbo's
dashing iterator restarts phase at each contour, so adopting it would change this
existing phase contract rather than repair contour handling.
Column and segment lengths use `hypot` to avoid intermediate square overflow;
`tiny_finite_circle_scale_remains_visible` reads interior and exterior pixels
of a large local circle under a finite `1e-23` scale. Its device radius is about
ten pixels; the previous squared norm underflowed and degenerated the instance.
Non-finite dashed segment lengths refuse the draw before walking it.
`invalid_dashed_contour_recovers` checks finite endpoints whose raster-space
difference overflows, then a valid stroke. Invalid input contributes no partial
geometry. Large finite dash intervals bound the broken walk; debug Lyon would
reject its generated non-finite point rather than loop indefinitely.
A dash step must also advance its raster-space offset: a positive interval can
round back to the current offset on a long contour. Failure rejects the whole
stroke before further geometry is built. The public painter row
`dashed_intervals_that_cannot_advance_refuse_the_whole_stroke` checks a bounded
cycle that loses one small step, discards its visible prefix and then renders
the next ordinary dashed draw. This is a progress guarantee, not a bound on
total tessellation work; tessellator output remains outside recording quotas.
These choices prioritize numerical range; no throughput improvement is claimed
for the native `hypot` implementation.
`dashed_closed_contour_has_its_closing_edge` and `dashed_contours_do_not_bridge`
read pixels that distinguish both contour defects. The same painter family row
`dashed_curves_and_phase_follow_contour_length` checks curve shape and equivalent
positive/negative phase across disconnected contours.

The offscreen texture pool retains the most recently returned idle allocations,
evicting the oldest when its bounded inventory fills. A resized effect working
set therefore replaces obsolete dimensions and warms up again. Returning an
allocation never mutates an outstanding texture or a submitted command's wgpu
reference. `offscreen_pool_reuses_a_resized_working_set` in the painter readback
family checks allocation reuse after the previous dimensions filled the pool.

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

`cancelling_renderer_new` exercises the production `probe_then_build` seam
with a CPU-only handle source and a builder that remains pending. After the
external owner is dropped, the target stays alive while construction is
pending and its destructor runs when that future is cancelled. This pins
construction ownership without initializing a GPU or requiring a window.

### 4. Clip coverage mixes the complete operator result with the destination

Coverage is independent of source alpha. Destination-destructive operators
must mix the fully evaluated operator result with the prior destination using
the final clip coverage; transparent source texels inside the group region
remain meaningful. The portable destination-read composite path provides this
mix without requiring dual-source blending. Capability-specific dual-source
shape pipelines remain an optimization, not the contract for group coverage.
Direct Tess/Gradient draws use independent unblended paint and coverage planes
when their accepted result cannot use fixed-function blending and dual-source
blending is unavailable. Fractional Plus always uses the portable path: clamp
the full operation before mixing coverage. Scratch textures are cropped to
conservative geometry bounds and the recorded scissor. An immutable crop mapping
preserves world coordinates and restores attachment coordinates for mask loads.
Crop origins preserve the attachment's 2×2 derivative grid; the original scissor
still bounds isolation and composite writes. Invisible SSAA paths are no-ops
before backdrop admission.
Each logical primitive composites before the next overlapping operation.
SSAA Plus resolves a separate geometry plane with the same downsample mapping.
[ADR-0103](../../docs/adr/ADR-0103-portable-independent-primitive-coverage.md)
replaces the former direct-draw capability decision.
`coverage_blend_reads_back_as_specified` checks the portable result, including
transparent Clear, gradient alpha and intrinsic edges, crop ordering and saturated
SSAA Plus. `WgpuPainter::render_to_texture` creates its own base-mip view from a
validated backing texture. `featureless_direct_aa_refusal_recovers` retains the
view-only refusal and next-frame contract; `limited_mrt_coverage_refusal_recovers`
and `invalid_texture_target_recovers` pin requested limits and target admission.
`grouped_clip_prefix_and_destructive_coverage` asserts Clear, Src and DstIn
on a fractional clip edge. Native readbacks pin the contract; browser and mobile
runtime execution remain separate evidence from native runs and cross-typechecks.

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

### 6. Path clips use bounded geometric membership

Path commands lower through Lyon's mature path representation and curve
flattening into a bounded edge tape. The GPU evaluates the admitted fill rule;
the transformed bounding box only limits work and does not replace membership.
An empty intersection path clips everything. The historical test name
`a_path_clip_lets_through_what_lies_inside_the_box_but_outside_the_shape` is
preserved for existing references, but its assertion now requires the triangle
exterior to retain the backdrop; it no longer promises bounding-box leakage.
`an_empty_clip_path_clips_everything` retains the empty-input contract.

### 7. Superellipse clips use analytic sample membership

The mask shader evaluates the n=4 superellipse at the common membership samples,
with independent elliptical corner radii. This is analytic Boolean membership,
not the old single SDF slot and not tessellation of the rounded clip itself.
`the_squircle_sdf_agrees_with_the_cpu_path_across_the_whole_boundary` keeps its
historical name and independent geometric oracle. New elliptical and nested
witnesses are in `nested_exact_clip_geometry_and_coverage`.

### 8. Difference is the exact complement within the inherited clip

Clip expressions preserve ordered Intersect and Difference operations. A
Difference subtracts its shape from the inherited membership; it is neither
ignored nor installed as Intersect. The historically named
`every_canvas_clip_shape_refuses_difference_rather_than_inverting` now asserts
both retained exterior and removed interior for rect, rrect, superellipse and
path. Invalid geometry returns a typed error instead of silently installing a
different operation.

### 9. Clip offscreens and nested image filters preserve ordered content

`Clip::AntiAliasWithSaveLayer` opens an offscreen bounded by the clip's own
scissor, including inside image filters. Coverage applies once to the finished
group. Rect AA also resolves geometric membership; a hard rectangular scissor remains a work bound.

`FilterOp` retains every ordered draw item plus the final segment. Source support,
backwards required input, evolving intermediate support and the final composite
clip have separate roles. Each Blur axis uses the shader's
`ceil(sigma * 1.7320508)` support; Morph uses `ceil(radius)`. The finite input
working domain is bounded by desired output plus accumulated per-axis support,
including enclosing filters' input demand. Distant scene geometry cannot enlarge
it. Antialiased primitive bounds include the shader's fringe before pixel cover.

Flat and nested input share a signed root-to-attachment mapping. Immutable
viewport bindings carry the attachment origin; primitive geometry, gradient
coordinates and analytic clips remain in root space. Scissors clamp only after
mapping to their actual attachment, and membership masks compose the same origin
into their attachment-to-root mapping. An integer texel grid keeps the final
composite aligned while cropping it independently of the input working domain.
Replay restores the previous attachment state on both success and refusal.
The frame boundary resets it after a contained unwind.

Dimensions, cumulative sampling work and prepared resource charges are admitted
before foreground texture acquisition. Existing submission/completion ownership
retains these charges; this is not a budget for every resident GPU allocation.
Nonfinite/negative radii and sigma that cannot preserve valid shader arithmetic
produce a typed geometry error. Zero sigma on either axis is identity on that
axis and preserves the source's antialias coverage.

`foreground_filter_viewport_crop_contract` compares direct replay and recorded
scenes with the corresponding crop of a larger rendering. The independent
`foreground_filter_chains_match_independent_nested_layers` compares Compose with
separately nested filters, preventing both crop renders from sharing an
intermediate-support defect. `image_filters_keep_nested_opacity_and_both_siblings`
and `a_clip_inside_an_image_filter_layer_keeps_its_content_and_its_siblings`
retain the earlier ordering and group-coverage witnesses.
`foreground_filter_invalid_parameters_refuse_and_recover` and
`foreground_filter_prepared_quota_refusal_keeps_next_frame_deliverable` pin
parameter admission and the next healthy frame after resource refusal.

### 10. Group and backdrop composites preserve their recorded operator

Save layers and ordered backdrop items composite through the shared group-texture
path, with their recorded blend mode and captured clip expression. Shader masks
apply the requested operator between shader source and child destination inside
their isolated group; the finished result composites SrcOver into its parent
(ADR-0099 §4). A Clear mask therefore makes its group transparent while preserving
the parent backdrop. `layer_effects_capture_as_specified` pins source/child order,
transparent masks, empty-child Src and trailing siblings.

### 11. Shader masks reuse the containing painter and replay resources

A mask records its children and terminal shader into the existing painter's
ordered IR. It does not create another painter, glyph atlas, pipeline set or
submission owner. Explicit isolation prevents ordinary opaque-layer reintegration
from applying the mask operator directly to the parent. Nested groups use the
same replay executor and DeviceDomain; effect nesting is admitted before replay.
### 12. A zero-sized resize mints nothing

`RasterOwner::resize(0, h)` returns `None` instead of a fresh
`SurfaceGeneration` that every subsequent submit would be rejected against;
the caller (`flui-app`'s raster lane) keeps its last generation. A window
minimised to zero is a pause, not a new surface epoch.

### 13. Frame failure does not leak painter state

The swapchain frame's content step (`FrameSteps::content`) returns
`EngineResult`; on the error path the painter's
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
A paragraph arrives as the `ShapedParagraph` its recorder measured
(ADR-0092 §4) and is recorded by `DrawBatcher::draw_paragraph`: each run's
face goes into the atlas rasterizer's registry (`FontRegistry::prepare_run`;
a run whose blob holds no face is warned and skipped), each glyph the run
places (`ShapedRun::placed_glyphs`, a `GlyphKey` per glyph) is looked up in
the atlas, rasterised on first use through `SwashRasterizer`, and pushed as a
`GlyphInstance` into
`DrawSegment::glyph_batch` under the same scissor run, immutable clip expression, and layer
opacity every other instance gets. `DrawRun::Glyph` preserves its recorded
position among the other primitive families; adjacent compatible runs may share
a render pass, without moving a glyph across an intervening draw.
What that deleted: the per-segment glyph ranges (`text_start..text_end`),
the "claimed text" bookkeeping and the trailing gap passes that drew text
captured by a filter or advanced shape over everything, `seal_text_tail`,
one render pass per text-bearing segment, and the sRGB→linear colour
conversion glyphon applied to text on a gamma-space target
(`glyph_colour_lands_as_recorded`). A rotated or anisotropic CTM reaches
the glyphs: each quad carries the CTM's linear part over the raster scale.
The engine names no shaper and depends on none: no cosmic-text, Parley,
fontique, skrifa or swash type or crate, and no text context, font collection
or paragraph spec either (`the_engine_does_not_shape`). The performance
overlay's labels arrive shaped: the layer carries a display list recorded
through the UI runtime's text context, which the engine clips to the overlay's
bounds and replays (`performance_overlay_labels_read_back`). `etagere` stays
behind `glyph_atlas.rs` the way `lyon` stays behind `tessellator.rs`.

`GlyphAtlas<R: GlyphRasterizer>` is generic over where bitmaps come from: it
hashes `R::Key` and owns `R`, taking it by `&mut` on a miss and on a grow. The
painter's is a `TextAtlas`, `GlyphAtlas<SwashRasterizer>`: the rasterizer owns
the registry of every face a paragraph drawn through it named, so a key stays
valid while the atlas lives, and rasterization takes no lock and shares no
font state with any UI runtime. `Renderer::render_plugin_scene` selects a plugin
font source; the first scene from a hook and every successful image reload
request a reset. Switching ordinary ↔ plugin sources also replaces the complete
`TextAtlas` between frames, including its registry and bitmap entries. Blob ids
are local to the image that created them, so retaining either cache across an
image transition could draw a previous image's face under the same glyph key.
Ordinary managed `render_frame` selects the ordinary source too. The shared
`FrameProtocol` selector forces full repaint before damage planning; an unchanged
producer diff cannot leave pixels from the previous namespace on screen.
Previously submitted GPU work owns its resources, so replacing the atlas does
not require waiting for the device.

`ordinary_to_plugin_repaints_with_the_new_font`,
`a_reloaded_plugin_repaints_with_the_new_font` and
`plugin_to_ordinary_repaints_with_the_new_font` in `parley_runs_read_back`
read back two distinct fonts sharing a glyph key, against independent fresh
captures. Each transition starts with `NoDamage`. The private retained capture
uses the production source selector and frame protocol; painting's
`testing::paragraph_with_font_ids` models image-local counters restarting, which
cannot be injected through the production paragraph constructors. Plugin atlases
use `SwashRasterizer::with_owned_fonts` to copy a newly admitted font into
host-owned bytes (painting's decision 18); ordinary atlases retain shared sources
to preserve their weak source-cache identity. Switching sources
also allocates a fresh atlas, while frames within one source retain it.
Recovery and a surface-format change also replace the painter. Their shared
format-consumer factory constructs its empty atlas using the current
`FrameProtocol` source: a warm plugin frame can then omit a reset without
retaining image-dependent font storage. The new atlas starts an empty font
namespace; ordinary replacements continue to retain shared sources.
`plugin_fonts_survive_format_replacement`,
`plugin_fonts_survive_domain_replacement` and
`ordinary_fonts_retain_sources_after_replacement` in
`painter_images_and_offscreen_results_read_back_as_specified` exercise the
actual factory and atlas with source retirement probes, uncached glyph bitmap
comparison and atlas upload. This private seam is needed because native surface
replacement requires a live window; it does not claim a native recovery run.

Bitmap bearings and dimensions are widened to `i64` before forming a glyph quad
and testing the scissor. An admitted `i32` origin can have ink outside that range;
clipping must not first overflow the bitmap offset. The public painter calls in
`extreme_glyph_bearings_do_not_overflow_before_clipping` draw real glyph bearings
at both lower coordinate endpoints and compare subsequent visible ink with a
reference readback. This preserves integer placement through clipping, without
promising extreme-coordinate GPU floating-point precision.


`parley_runs_read_back` reads back what paint now
draws: hard breaks, synthetic bold, host fallback faces, right alignment and
the device baseline. `GlyphImage` validates CPU byte storage at construction
(ADR-0122); the atlas does not repeat that invariant. A valid bitmap can still
exceed the device's texture limit, so allocation refuses it before eviction or
page growth and converts dimensions to the packer's signed representation with
checked conversions. A grow re-uploads a re-rasterized glyph only if it has the
size and content kind its slot was given. A glyph that fails the grow
check is dropped from the cache, as after a `None`, so its next use asks
again; its allocation is freed at the end of the frame if the frame already
drew from it, so no other glyph is packed into a region a recorded draw
samples. `swash_glyphs_land_and_equal_keys_share_a_slot` pins the placement
path a real rasterizer takes.
`failed_glyph_replay_retries_without_reusing_recorded_regions` models a missing
bitmap, valid bitmaps with changed size or content, and competing failures on independent keys through the private
rasterizer seam. Failed replay removes the cache entry even when the rasterizer
returns `None`; the replacement texture contains no uploaded bitmap for that key.
The next use retries, healthy keys remain cached, and retry allocations cannot
reuse regions already referenced by the current frame. This checks recovery and
allocation ownership, without claiming that a failed bitmap can still be drawn
in the frame which first requested it.
The same family produces a valid CPU bitmap beyond a deliberately small device
limit, proves refusal without replaying healthy glyphs or growing the page, and
then admits a valid bitmap for the same key. Width and height are separate rows.
CPU byte-count overflow and malformed storage are painting constructor tests.

### 17. One rounding rule per purpose: hard edges snap, bounds cover — [ADR-0098 §6](../../docs/adr/ADR-0098-owned-f64-geometry-values.md)

Every place that turns a device-space float into a whole pixel goes through
`flui_foundation::geometry::{snap_edges, cover}`, and which one is decided by
what the rectangle is for:

- **A hard rect clip snaps its edges** under a translation plus a positive
  axis-aligned scale (`GpuStateStack::is_translate_scale`): a pixel is kept
  exactly when its centre is inside, Skia's non-antialiased clip, with ties
  toward +∞. Under rotation, skew or a reflection it covers its device
  bounding box. Here the scissor *is* the clip.
- **Bounds cover** (floor of the minimum, ceiling of the maximum): the
  scissor in front of a rounded or squircle clip's SDF, the damage scissor,
  a path clip's bounding box, backdrop-filter and advanced-blend copy
  regions, filter offscreens and SSAA tiles. None of them may lose a partly
  covered pixel: behind an SDF that pixel carries the feathered fringe
  (`modes_that_cannot_absorb_coverage_feather_their_partially_covered_edge`
samples it).

This replaced three rules that disagreed: the identity-transform scissor
truncated every edge (keeping column 0 of a clip starting at 0.75 and
dropping column 10 of one ending at 10.75), the transformed scissor floored
the origin and ceiled the extent, and the backdrop copies rounded half away
from zero. One rule now covers every call site. Content quads are not yet snapped; see Open items.
Locked by `a_hard_rect_clip_keeps_the_pixels_whose_centres_are_inside`
(`src/state_stack.rs`).

### 18. Partial frames render into a retained target and blit — [ADR-0087 §4](../../docs/adr/ADR-0087-raster-contract-and-cpu-backend.md)

Damage arrives as `flui_layer::DamageRegion` on each `SceneSnapshot`, from the
host's `LayerDiffer`; `RasterOwner::pump` hands it to the backend before the
freshness checks, so a frame rejected on a stale generation still leaves its
debt, a superseded frame's region is folded into the frame that replaced it,
and a failed render marks the next frame full. The tracker forgets its debt
only after a present.

wgpu does not expose a swapchain image's age, so pixels outside a scissor on
a freshly acquired image come from an arbitrary older frame. A partial frame
therefore never renders into the swapchain: `damage::plan_frame` sends it
into `RetainedTarget`, a surface-format texture holding the last frame, which
is blitted whole onto the swapchain. A full frame renders directly and
leaves the target invalid; the first partial frame after it renders in full
into the target (the warm-up), and only later ones are scissored. On a
surface without `COPY_SRC` every frame goes through the target, which
replaces the pooled intermediate that path used to take. The target is
allocated by the first frame that needs it. A replacement uses a distinct
candidate ([ADR-0100](../../docs/adr/ADR-0100-prepared-gpu-work-and-retained-frame-commit.md));
a partial candidate first copies the committed pixels, then repaints the damage.
Only a successful submission sequence promotes it. A failed candidate preserves
the previous committed pixels and forces a full retry. Two BGRA8 targets require
16.6 MB at 1920×1080, excluding driver overhead. Resize, reconfigure and surface
recreation invalidate the retained content; `release_surface` and device recovery
drop the targets. Submitted uses keep their resource charges until completion.
No named Flutter test is replaced by this wgpu-specific ownership decision:
swapchain buffer-age assertions do not apply to this API. FLUI's
`damage_readback_tests::an_invalid_target_promotes_to_full`, including its
`bounded_reused_targets_preserve_pixels` row, pins failed-candidate preservation,
two-slot reuse and resize recovery.

A partial frame clears its damage with an opaque fill inside the scissor
(`damage::begin_partial`) before the content, not with the full clear pass,
which would wipe the retained pixels. The advanced-shape straddle self-heal
stays: with the correct previous frame outside the damage, it bounds a
straddling shape's out-of-damage slice to one frame. The full clear and the
partial clear paint one constant, `frame_protocol::BACKGROUND`.

Only frames a `RasterOwner` retires render damage: `RasterBackend::render_scene`
calls `Renderer::render_frame`. The public `Renderer::render_scene` is the
entry point for a frame no producer accounted for (direct mode, a hot-reload
plugin's scene): it renders in full, invalidates the target and makes the
next frame full, because the owner's differ compares against scenes it
submitted and would otherwise scissor over pixels it never saw.

FLUI has no per-buffer damage accumulation (buffer age), so it keeps a committed
retained image and prepares a candidate; it rounds damage outward with a 1 px anti-aliasing
margin (`DamageRect::covering`) rather than aligning to tiles; and a frame with
nothing damaged does not present (`PresentDisposition::NoDamage`).

Before candidate isolation, `render_throughput`'s `damage_retained_target` group (1920×1080,
translucent full-surface layers, 128 px damage, one desktop adapter): full
direct 343 µs / 1.07 ms / 3.39 ms at 4 / 16 / 64 layers against partial plus
blit 340 µs / 262 µs / 301 µs; the blit alone 144 µs. The blit's bandwidth on
tile-based mobile GPUs is not measured. Those figures exclude candidate allocation
and copy; see the [measurement record](../../docs/research/engine-foundation-measurements.ru.md)
for the current comparison and its limits. Locked by `damage_readback_tests.rs`
(through the crate-private `RetainedCapture`, which runs the renderer's own
`FrameProtocol` and `record_frame_content`; only the clear, the content
submission and the blit are its own), `damage::tests::plan_frame_table` and
the `raster_owner` damage tests. The windowed path itself runs only on a
developer machine: CI has no surface.

### 19. A layer composites its whole region with its recorded mode

A save layer and an opacity layer composite their whole region with the
blend mode they record ([ADR-0099](../../docs/adr/ADR-0099-save-layer-region-and-blend.md)
states the cross-crate contract). The region is the layer's bounds
mapped through the transform current at `save_layer`, cut by the clip in
force there (ancestor clip rects, a rounded clip, and a partial frame's
damage scissor); an unbounded layer's region is the clip itself. The
pixels the content left transparent are part of the layer, so a mode whose
`keeps_destination_under_transparent_source()` is false (`Clear`, `Src`,
`SrcIn`, `DstIn`, `SrcOut`, `DstATop`, `Modulate`) changes every pixel of the
region, and an empty layer in such a mode still composites. The modes that
keep the destination (`SrcOver`, `DstOver`, `Xor`, `Plus`, the advanced ones)
look the same outside the content either way, but are now honoured as
recorded rather than collapsed to `SrcOver`.

Why this and not "replace only under the content":

- `save_layer(bounds, Src)` reads as "this region becomes exactly the layer".
  Replacing only under the content would make `Src` depend on how far the
  content happens to reach, and make an empty `Clear` layer do nothing.
- It is what a paint-level mode already does: a `Src` rect with a partly
  transparent shader replaces every pixel of the rect. Restoring a layer is
  a draw of its buffer over its region.
- The region is the bounds the author wrote, in their own space, moved by
  the same transform as the content, so it lands where the content does.
  Bounds left unmapped composited a translated or scaled translucent layer
  over the wrong rectangle and lost its content outside it.
- Clips still clip: a layer never changes a pixel its clip excludes, the one
  limit the author cannot reach from inside the layer.
- It is the extent the damage producer already reports (`DrawOp`'s damage
  extent for a bounded save layer is its mapped bounds; `LayerDiffer` takes an
  opacity layer in such a mode as the viewport), so a partial frame repaints
  everything the composite changes.

Edge cases:

- The region bounds the content as well as the mode: a composited layer
  shows nothing it holds outside its region, whatever its mode. Under a
  transform that keeps axes aligned (translation, scale, flips, quarter
  turns) the region is exactly the mapped rectangle, which is the composite
  quad. Under a rotation or skew it is the exact quad: the composite runs
  over its bounding box with the local bounds as a hard clip, for every mode,
  so content reaching past the bounds never shows in the bounding box's
  corners. The clip also routes an opaque `SrcOver` layer through the
  composite instead of splicing its content into the parent, and the
  advanced-blend composite carries it as a hard rectangle in the bounds'
  local space. Ambient and transformed bounds membership combine in the
  immutable composite expression instead of competing for one clip slot.
  Captured transforms are validated affine geometry; unsupported projective
  input is a typed refusal. The conservative bounding extent still bounds damage.
- Composite quads are not anti-aliased: a pixel is in the region when its
  centre is. Damage rounds outward by a pixel (`DamageRect::covering`), so
  every changed pixel stays covered.
- The composite pipelines of the destination-replacing modes write
  transparent texels instead of discarding them (`replaces_destination` in
  `texture_instanced.wgsl`); only a fragment the clip excludes is dropped.
- A shader mask's and a backdrop filter's offscreen is drawn outside the
  ancestor clips, so their composite carries the scissor and complete inherited expression
  in force when they were queued, whatever their mode, at the top level and
  inside an opacity layer's offscreen alike.
- A shader mask is not such a layer. Its mode combines its shader with its
  child, which the mask pass does, and its result composites `SrcOver`:
  applied again at the composite, the default `Modulate` (or the gradient-text
  `SrcIn`) would multiply the child by the backdrop and erase the backdrop
  around it. The requested operator runs inside the isolated child group.

Locked by `layer_blend_tests::gpu_tests::a_layer_composites_its_whole_region_with_its_mode`
(mapped, scaled and rotated bounds, an empty `Clear` layer, rect and rounded
clips, a translucent translated layer, rotated translucent, opaque and
`Multiply` layers whose content overruns their bounds, an opaque `DstOver`
layer),
`damage_readback_tests::an_effect_layer_composites_its_whole_region_with_its_mode`
(opacity layers, a clipped `Modulate` shader mask that keeps the backdrop
around its child, and a clipped mask inside an opacity layer), and the damage
rows `a_removed_translated_src_save_layer_leaves_nothing_behind` (a rotated
opaque layer among them, its full frame against its partial one),
`a_removed_destination_affecting_layer_leaves_nothing_behind` and
`a_change_beside_a_viewport_compositing_layer_matches_a_full_frame`.

---

## Open items

- **Threaded raster lane (ADR-0045, Proposed).** `raster_owner`'s mailbox,
  ack channel, and `InFlightAccounting` exist for a lane that flui-app does
  not yet drive; whether the lane ships or the direct path is the only path
  decides whether this family stays. Until decided, it is tested but not
  wired.
- **An axis-aligned opaque `SrcOver` save layer is spliced, not composited
  (decision 19).** With nothing to apply at the composite, its content goes
  straight into the parent's draw order, so content drawn past its bounds is
  cut only by the clip. Cutting it means compositing such a layer whenever its
  content may overrun its bounds, which the splice exists to avoid.
- **`catch_unwind` around `render_scene`.** A panic inside a layer's paint
  poisons the frame rather than isolating the layer; changing that is a
  contract change that needs its own ADR.
- **Content snapping (ADR-0098 §6).** Solid, gradient and image quads are
  still drawn at their fractional device positions, antialiased; §6 has them
  snap their edges under a translation plus a positive scale, with animated
  layers composited unsnapped. That needs the engine to know which layers
  animate, and it moves every existing readback, so it lands on its own with
  readbacks that tell a snapped edge from an antialiased one. Border and
  stroke widths resolved to whole device pixels in layout
  (`geometry::resolve_stroke_width`) and a text run's baseline snap belong to
  the same change.

### 20. Immutable clip expressions resolve Boolean membership before coverage

Clip geometry captures the validated f64 affine transform at record time.
Singular Intersect is empty; singular Difference leaves prior membership intact.
Non-finite geometry, negative extents/radii, unsupported transforms and GPU
coordinates outside the admitted magnitude (2^20) return typed errors; values
are not silently clamped into another shape. `clip_failures_and_singular_membership_recover`
asserts refusal and the next valid capture.

A persistent expression lowers to portable fixed uniforms: at most 64 nodes and
512 edges. Boolean membership runs on a common 8x8 sample grid before one R8
coverage resolve. An entirely HardEdge expression checks the pixel center once,
with the work allowance charged for that single Boolean fold. Repeating a shape
is idempotent; multiplying already-resolved
R8 masks is not the expression algebra. Each encoded target gets frozen mapping,
mask and consumer uniforms. A scaled SSAA/offscreen attachment resamples geometry
in its attachment sample domain rather than resizing a root-resolution R8 mask.
Conservative regional crop limits allocation, while cumulative membership work
is admitted across the frame with a 1 billion unit ceiling. Both lowering CPU
scratch and GPU allocations enter existing admission/completion ownership.

SaveLayer separates inherited composite membership from membership introduced
inside its input. A prefix belongs to one group boundary and must not attenuate
both child pixels and their final composite. `grouped_clip_prefix_and_destructive_coverage`
pins the repeated-prefix case. `nested_exact_clip_geometry_and_coverage` pins
nested rounded geometry, repeated AA, independent rx/ry and fractional rects.
The native Windows GPU suite passes with `FLUI_REQUIRE_GPU=1` (59 tests, no
skips). This is execution evidence for this host, not every target/backend.

Future optimizations include bounded mask atlases, reuse keyed by immutable
expression plus target mapping, and workload benchmarks. These require measured
benefit and completion-safe resource ownership. The clip quotas do not constitute
a whole-engine memory cap: DrawItem allocation metadata and other previously
excluded recording payloads remain known admission limits to address.

### 21. Layer effects record once and replay against their containing target

Problem: window traversal intercepted masks and backdrop filters, while public
headless capture silently used incomplete generic handlers. A temporary mask
backend omitted nested effect support. Flushing a backdrop during recording
could read the root attachment while its preceding siblings belonged to an
isolated group. These are observable ordering errors, not backend preferences.

Alternatives: giving every recursive capture another renderer preserves duplicate
traversal, submission and ownership paths. A dedicated shader-mask pipeline also
duplicates shader evaluation and blend operators. Instead, traversal records one
ordered stream. A shader mask is an explicitly isolated group whose final draw
combines the shader source with the child destination; restoring that group uses
SrcOver. Backdrop filtering is an ordered item that reads the active replay target
before its children. Window and public headless capture use the same visitor,
including linked-follower resolution.

Contract: inherited clip membership belongs to the group composite once. Mask
bounds are captured with their affine transform. The terminal shader geometry
covers the attachment independently of those bounds, preserving shader coordinates
without introducing a second antialiasing edge. Unsupported shader or filter input
is a typed error, never a successful fallback to different pixels. Singular bounds
have empty output. Nested effects must be admitted before recursive replay so a
layer tree cannot cause an unbounded effect call stack.

Backdrop desired output, required input and write clip are distinct. Blur reads
its actual kernel halo from the containing target, including pixels outside the
output clip, and writes only the bounded output under the inherited clip. Outside
the attachment the source is transparent (decal). During a managed surface resize
transient, output and input reads are bounded by the intersection of the recorded
viewport and the acquired attachment. The composite uses the actual attachment
extent so a copied texel is written in the same device coordinate system. The
public texture-target API still requires matching extents. Sigma remains two-dimensional;
a transform must either preserve the implemented kernel semantics or be refused.
Partial-frame damage may constrain writes but must not truncate input dependencies.
Allocations and captured records remain owned by the existing DeviceDomain and
frame admission/completion protocol; there is no nested submission owner.

Reference check: Skia's `getInputBounds` and `getOutputBounds` deliberately answer
different questions; Impeller's entity pass reads its current pass texture before
rendering backdrop-filter children. These support separate footprints and ordered
execution, without adopting their APIs or scene models. Sources and limits are in
[the effect research](../../docs/research/engine-effect-footprints-research.ru.md).
`layer_effects_capture_as_specified` reads back nested masks and backdrop groups,
shader source/child destination order, followers, fractional DPR, affine linear/
radial/sweep gradients, both blur axes, attachment-edge decal and whole-target
partial/full equivalence. Its failure matrix preserves the first error and renders
the next valid frame after unsupported shaders, missing textures, excessive depth
and excessive sampling work. Disabling backdrop dependency expansion makes pixel
(28,25) stale in the partial/full witness; the unchanged test fails on the pixels.

Gradient instances retain local extents plus an affine matrix; the local origin
is transformed in f64 before packing, preserving small shapes at large offsets.
Cropped replay composes the attachment rebase with that matrix. Rounded coverage
uses derivatives without a fixed local-distance cutoff. Nonrepresentable matrix
packing is refused. Sweep gradients wrap angular position while retaining the full span,
so a complete turn remains a gradient. The phase is reduced modulo TAU in f64
before packing. Linear gradients carry a local affine parameter `t = a*x + b*y + c`,
computed in f64, instead of subtracting distant f32 endpoints per fragment.
`large_sweep_phase_preserves_pixels` and `distant_linear_projection_preserves_pixels`
compare direct and masked ordinary/advanced draws against small-coordinate references.
Mask shader admission currently supports
solid and Clamp linear/radial/sweep gradients without radial focal parameters;
unsupported tiling, nonfinite parameters and invalid effective stops are errors.
Ordinary and advanced gradient draws also validate rebased numeric payloads and
effective stop order before admitting stop storage. Finite inputs whose packed shader
intermediates overflow, or whose nonzero sweep span collapses, are refused too.
Device vertex input uses
13 attributes, including the quad, by grouping adjacent geometry and stop fields.

Before partial clear, input-dependency closure expands repaint conservatively.
It may overestimate clips and isolated groups; after 64 expansion scans it chooses
a full repaint rather than unbounded planning work. Effect nesting is limited to
64 and cumulative backdrop sampling to one billion taps per frame. These are
engine work limits, not a driver VRAM guarantee. Backdrop supports axis-preserving
Gaussian transforms and encoded SDR UNorm targets. The axis test admits only
per-column f64 roundoff (eight machine epsilons) so public quarter-turn rotations
work; `quarter_turn_backdrops_match_baked_axes` compares their anisotropic output
with baked device-space references and rejects a meaningful non-axis rotation.
Directional affine blur,
foreground input outside the viewport and backdrop filter chains remain open.

The [scene renderer example](../../examples/scene_render.rs) accepts `--effects`
for a native six-panel gallery and `--capture-effects <PNG path>` for the same
scene through headless capture. With feature `testing` and
`FLUI_READBACK_DUMP_DIR` set, `layer_effects_capture_as_specified` also writes
named direct/mask, full/partial, scaled/rebased and follower-state PNG witnesses.
Screenshots complement the numerical readbacks; they do not replace them.


### Reflected and scaled primitive bounds remain local

The baked rectangle fast path requires a positive axis-aligned scale. Reflections
keep positive local extents and the full affine transform. Rounded rectangles
bake only pure translation: scaling must transform the radii together with the
shape, and reflection must preserve each corner's identity. The affine path
rebases the local origin in f64 before packing the positive extents into f32.
`reflected_shapes_and_scaled_radii_match_baked` compares reflections, asymmetric
corners, scaled radii and a distant reflected origin against independently baked
shapes. This does not implement elliptical local corner radii, which still use
the existing per-corner maximum-radius approximation.

## Clip membership compilation

The clip mask retains its common 8x8 AA sample grid. Hard leaves evaluate the
pixel center; an all-hard chain uses one sample. The sample count is supplied
in the immutable mapping uniform rather than fixed nested shader loops.

Membership pipelines specialize only on geometry: rectangles, curves with
rectangles, or the general path/mixed case. These three lazy entries share the
same bind-group layout and shader module; pipeline constants remove unreachable
path and corner evaluation before driver compilation. No cache key contains
coordinates, node count, edge count or sample count. Each newly materialized
pipeline is admitted as a prepared object before creation.

`clip_layers_read_back_as_the_clip_contract_specifies` pins hard/AA and mixed
clips, curved membership, fill rules, transformed paths, subtraction, invalid
payload recovery and full-HD hard clipping. Its private quota row also refuses
mask and first-pipeline preparation under competing occupancy, completes a
valid clip on the same owner after refusal, and admits repeated use without
charging pipeline creation again. The existing
`offscreen_resource_cache` benchmark includes `clip_first_use_prepare_submit_wait`
for rectangles, curves, paths and mixed tapes. `FLUI_BENCH_FALLBACK=1` requires a
software adapter for that group and prints its identity. Timing includes first
clip preparation, submission and completion; painter construction is outside
the timed interval. It is not a steady-frame throughput measurement.

### Repeated images crop natural tiles and cannot stall recording

Repeated axes retain the image's natural pixel extent in logical coordinates.
The final tile crops the source UV extent, including atlas remapping, instead
of squeezing the whole source image into a smaller destination. Ordinary and
advanced blend routes consume the same tile bounds and UVs; advanced repeats
remain one isolated shape so all tiles blend against the same backdrop.
`NoRepeat` keeps the single-image route.

A repeated draw with nonfinite or reversed destination edges, an overflowing
extent, or an edge that cannot advance in `f64` is omitted as a whole. This is
a primitive omission, not a sticky frame error. Tiles accumulate in an empty
sibling of the live recording segment until traversal completes. A later
stalled edge therefore discards its finite prefix while retaining earlier and
following draws. Dropping that sibling releases its charged arena capacity and
live elements; the texture cache may still retain the loaded image, as caches
are outside recording admission. Successful ordinary repeats publish a segment
in painter order; advanced repeats publish one `AdvancedShape`.

A recording quota failure is different: it remains the existing sticky frame
error, and traversal stops immediately after refusal. The shared recording
budget bounds appended tile work; there is no separate tile-count cap or
floating-point-to-integer count conversion. A new frame recovers the budget.

`painter_images_and_offscreen_results_read_back_as_specified` includes named
X, Y and two-axis crop and late-stall rows for SrcOver and Multiply. The crop
rows render into an actual readable texture, check the terminal source prefix,
full-tile suffix, prior content outside the destination and a later overlapping
draw. Stall rows rebase two representable local edges into visible pixels
before the third stalls at `2^53`, then check that the prefix did not escape and
a healthy sibling draws. Ordinary and advanced quota and nonfinite rows cover
sticky next-frame recovery and omission respectively.

Potentially nonterminating counterfactuals reexecute the existing painter test
binary in a child. Device and target preparation signal readiness before the
public recording call; recording has a five-second deadline, while preparation
and subsequent GPU readback have separate deadlines. The parent kills and reaps
a hung child and the family continues to later named rows. No adapter skip is
added. A private zero-capacity recording seam is necessary for the quota rows;
the rendering and next-frame recovery operations remain the public painter API.


### Image regions preserve affine placement, Paint and clip coverage

ADR-0115 carries the source texel rectangle separately from logical destination
and optional fitted repeat placement. The image batcher maps all four corners
through a finite affine matrix, narrows the origin and basis vectors once, and
uses the resulting quad for GPU placement and replay bounds. Sprite transforms
compose with the ambient matrix instead of extracting only their translation.
Existing offscreen and SSAA rectangle constructors supply axis-aligned bases.
Attachment rebasing retains its separate attachment-to-root map.

Cached image runs record their fixed blend mode. An advanced image operation
records SrcOver internally and applies its operator once to the completed group.
Filters process decoded straight channels before tinting; decoded linear taps
are premultiplied before interpolation. Optional Paint multiplies RGB and alpha.
UV crops map through the actual cache atlas region without a half-texel inset.

Under fractional clips, destination-sensitive image modes use the existing
portable compositor even when the adapter exposes dual-source blending. The
image isolation pipeline writes sampled premultiplied color and geometric
coverage to separate attachments. Transparent source pixels carry clip coverage
and therefore replace the destination where Src requires it; pixels outside the
coverage preserve the destination. No additional image raster backend exists.

The existing `painter_images_and_offscreen_results_read_back_as_specified`
family contains `ambient_shear`, `rotated_uv`, `tint_and_alpha`,
`src_transparency` and `feathered_src_transparency`. The feathered row measures
coverage with an opaque SrcOver image on a transparent target, then checks a
transparent Src image against the retained destination at partial-coverage
pixels, together with untouched outside content and a later draw. These GPU
rows use the private readable-target seam because a public window surface
cannot be sampled by an integration consumer; recording is the public painter
API and the isolation is the actual production replay path.


`decoration_cover`, `decoration_filter_opacity`, `decoration_repeat_phase` and
`canvas_image_paint` invoke the public painting producers and replay their
recorded commands. `render_image_scaled_cover` mounts a real RenderImage through
RenderTester and captures its actual layer tree; `render_image_natural_crop`
also pins an oversized natural-scale crop. Both visible crop color and
pixels outside the allocated box distinguish the producer defect. Engine dev
edges on objects and rendering's testing feature exist for this consumer path.
`standalone_source_crop`, `atlas_affine_and_paint`, `advanced_affine` and
`degenerate_image_quad_keeps_sibling` cover source region/texture layout, sprite and
advanced placement, and deliberate zero-area image-quad omission with a healthy
next draw. They are rows of the same GPU family, rather than separate targets.

The texture instance also carries original-image UV bounds at location 11.
Decoded atlas images set these to their full loaded image region; external,
offscreen and SSAA constructors keep full-texture bounds. Manual straight taps
clamp at the original-image texel edge, not the crop edge, so adjacent texels
inside an image remain part of an internal fractional crop's filter footprint.
`packed_original_edge` and `standalone_original_edge` draw equivalent opaque
source crops at the same scale and check original edge samples and outside
pixels. Removing the original bounds affects the packed row while leaving the
standalone row healthy. This changes sampling admission, not atlas allocation.

### Radial gradients interpolate two circles

ADR-0116 defines the focal/initial and outer circles, greatest admissible root,
transparent no-solution coverage and existing tiling modes. Both ordinary and
advanced recording use one validated packed-circle admission. Radial vertex
inputs use 13 actual attributes (quad location 0 plus instance locations 2..13);
linear and sweep use 12. Mask gradients keep their explicit Clamp/no-focal
support. `layer_effects_capture_as_specified` includes the focal, initial-radius,
concentric/linear/repeated-root/cone, tiling, decoration, affine and refusal/recovery
readback rows. CPU geometry normalization and computed shader roots retain their
documented numeric limits; exact conical geometry at every floating range is
not promised.
