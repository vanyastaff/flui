# Codebase map: engine_painting_text (flui-engine, flui-painting: DisplayList, wgpu backend, atlases, shaders, text stack, damage, CPU reference plans, renderer swappability, external GPU content)

_Raw output of the `map:engine_painting_text` agent (read-only review of main @ cab06137d, 2026-09-25). Unedited; claims are the agent's and were only partly re-verified._

## Summary

How a frame works today: render objects record through `flui_painting::Canvas` into an immutable `DisplayList` of `DrawCommand { transform: Matrix4, op: DrawOp }`. `DrawOp` is a closed enum with 28 variants (crates/flui-painting/src/display_list/command.rs:53-364). Picture layers carry that list in `Arc`s inside a `flui_layer::Scene`. flui-app hands each scene to one `flui_engine::Renderer` per window, through an inline "raster lane" (crates/flui-app/src/app/raster_lane.rs, which wraps `RasterOwner`). `Renderer::render_scene` walks the layer tree iteratively (layer_walk.rs). The walk goes `LayerRender` → `LayerDispatcher` → `WgpuPainter`/`DrawBatcher`, which records a GPU-lowered Command IR; `GpuReplay` encodes that into passes and one `queue.submit` (ADR-0006). Paths are tessellated with lyon and cached for 120 frames. Clip anti-aliasing uses SDF plus scissor, and path AA uses SSAA. Effects (blur, colour matrix, morphology, advanced blend) run as offscreen pipelines over pooled textures, and the WGSL (4.6k lines) is composed with naga_oil. Text is shaped in flui-painting by cosmic-text 0.19 against one process-global `FONT_SYSTEM: OnceLock<Arc<Mutex<FontState>>>` (text_layout/layout.rs:124). The shaped `Arc<TextLayout>` travels inside `DrawOp::Paragraph`. The engine's own glyph atlas (ADR-0067, one per `WgpuPainter`) rasterises misses through `SharedFontSystem::rasterize`, which takes that same global mutex.

Damage: the consuming half (the scissor in `render_scene`) is written, but no producer exists. Every path calls `mark_full_repaint` (flui-app/src/app/direct.rs:181, raster_lane.rs:486), and the lane stamps `DamageRegion::Full` (raster_lane.rs:354).

Why the engine is "74k LOC": `find src -name '*.rs' | xargs wc -l` gives 72,479 lines, but most of that is not engine code.
- **Tests:** 22,440 lines are readback/oracle suites named `*_tests.rs`/`*_oracle*.rs` that live in `src/`, and another 14,716 lines are inline `#[cfg(test)] mod` blocks. About 37.8k lines (52%) are tests.
- **Production:** about 35.1k lines, of which 13.7k are comments (these modules carry essay-length rationale) and 2.5k are blank.
- **Executable Rust:** about 18.8k lines, plus 4.6k lines of WGSL. The largest code modules are batches/images (966), replay/flush (895), layer_dispatcher (888), layer_offscreen (763), pipeline_cache (760), tessellator (754) and batches/shapes (648). raster_owner.rs is 4,365 lines, of which only about 600 are code.
- **Verdict:** the engine is not bloated with features. It is a mid-sized wgpu 2D renderer with a very large, valuable in-crate readback test suite and heavy prose.

Structurally, the engine openly declares itself "wgpu is the engine, not a backend behind one", with no second rasteriser and nothing pluggable (crates/flui-engine/ARCHITECTURE.md:8-14 and Mapping decision 1). That directly contradicts the owner's principle 2 ("the renderer may change"), E7 (the CPU reference for goldens), H2 (a software fallback) and the external-GPU-content extension point. Yet a backend-neutral lowering core already exists inside the crate as `pub(crate)` modules with zero wgpu references: `layer_walk`, `layer_render`, `dispatch`, `command_renderer` (the `CommandRenderer` trait), `damage`, `raster` and `raster_owner`. The pieces for a second backend are there but locked in the GPU crate. The 3 layer kinds that need offscreen work (BackdropFilter, ShaderMask, Follower) are handled only by `Renderer`, so `HeadlessRenderer` already paints them differently. Text is well encapsulated: cosmic-text appears only in flui-painting, apart from one engine test file. But the global font system, one lock shared by shaping and rasterisation, and process-lifetime `GlyphKey`s tie text to the process rather than the realm. That is the main thing B1/B7 must redesign together.

## Responsibilities and boundaries

**flui-painting (layer 2, 7.0k LOC)** is by its own admission "two things under one name" (ARCHITECTURE.md:3-7):
- **The paint recorder:** `Canvas`, `DisplayList`, `DrawCommand`/`DrawOp`, and the box-decoration and table-border painters.
- **The whole text stack:** font database and host discovery, family and weight resolution (font_resolve.rs is 1,858 lines), shaping, truncation, caret and hit-test queries, `TextPainter`, and glyph rasterisation for the engine (`SharedFontSystem::rasterize`).

**flui-engine (layer 4)** owns everything below `Scene`:
- device, surface and recovery;
- layer lowering and the Command IR;
- pipelines, shaders, tessellation, offscreen effects, SSAA;
- the glyph atlas, image texture cache and atlas, and the external texture registry;
- damage accumulation;
- the raster mailbox protocol (`RasterOwner`).

Where the boundaries leak:
1. **Engine and text:** the engine reaches the process-global font system itself (painter/mod.rs:167 `flui_painting::shared_font_system()`) instead of receiving it from its owner, so the engine participates in ambient state.
2. **Recorder and shaper:** the DisplayList wire embeds a concrete shaper product (`DrawOp::Paragraph { layout: Arc<TextLayout> }`), which couples the recorder vocabulary to the shaper. Any consumer of display lists (another backend, record/replay, devtools) inherits cosmic-text.
3. **GPU-free code in the GPU crate:** the layer-lowering semantics that any backend needs (walk order, clip/opacity discipline, which layers are "diverted") live in the wgpu crate. The diverted handlers are methods on `Renderer` itself, so they are not a backend-neutral layer.
4. **Raster protocol placement:** the raster lane/mailbox protocol (`RasterOwner`, `SurfaceState`, `FrameStamp` checks) is runtime/scheduling topology, but it sits in the GPU crate as `pub mod raster_owner`. flui-app is its only consumer.
5. **Damage:** the decision (layer-tree diff) belongs between flui-layer and flui-rendering. The engine holds only the accumulator, and nobody produces damage.

What belongs elsewhere:
- a backend-neutral "scene lowering" core (walk, dispatch, CommandRenderer, damage, raster protocol) below the wgpu crate;
- text as a realm-owned service handed to the engine;
- an external-texture/GPU-content registration API on the app/realm side, acquired through `LifecycleContext` like other capabilities.

## Key types and contracts

- flui_painting::Canvas -> DisplayList (immutable, no serde per ADR-0066). DrawCommand { transform: Matrix4 (absolute CTM stamped per command), op: DrawOp }; DrawOp is a closed enum with no #[non_exhaustive], matched exhaustively by flui-engine dispatch.rs (painting ARCHITECTURE Mapping decision 1)
- DrawOp::Paragraph { layout: Arc<TextLayout>, offset, color }: the shaped layout crosses the display list by identity (ADR-0065), so the engine never shapes
- SharedFontSystem (painting text_layout/layout.rs): has 4 entry points, shape(|Shaper|), register_font (append-only, bumps generation), generation(), rasterize(GlyphKey) -> Option<GlyphImage>. All of them take the single global parking_lot Mutex
- GlyphKey(pub(super) cosmic_text::CacheKey) (glyphs.rs:23) is documented as valid for the life of the process because the font DB is append-only. That is a process-scoped identity
- PlacedGlyph { key, x: i32, y: i32, color } plus GlyphImage { Mask | Color }: the shaper-agnostic door into the engine atlas (ADR-0067)
- flui_layer::Scene / LayerTree / Layer (15-variant closed enum), DamageRegion (only Full), SceneSnapshot + FrameStamp
- flui_engine::Renderer: one per window. new(WindowTarget) creates its own wgpu::Instance/Adapter/Device (renderer.rs:1140-1168). render_scene -> Result<PresentDisposition, EngineError>. The type is Send + !Sync, pinned by static assertions
- RasterBackend trait (raster.rs): dyn-compatible and Send, with one production implementor. raster.rs:6 documents it as a test seam, not a plugin point, while lib.rs:510 calls it 'the frame-driver swap point'
- pub(crate) CommandRenderer (command_renderer.rs:31, about 30 render_*/clip_*/save_* methods) + pub(crate) LayerStateStack + LayerRender<R: CommandRenderer + ?Sized>: the de facto backend seam, and it is GPU-free
- Command IR (command_ir.rs DrawSegment/DrawItem) -> GpuReplay::submit: record/replay seam (ADR-0006), pinned by a deterministic replay test
- RasterOwner / RasterHandle / RasterAck / SurfaceState (pub mod raster_owner): mailbox protocol for ADR-0045 (status Proposed), pumped inline by flui-app
- HeadlessRenderer::render_layer_tree -> RGBA8: surface-less capture. It does not render BackdropFilter, ShaderMask or Follower the way Renderer does (headless.rs:15-27)
- ExternalTextureRegistry (crate-private module, pub struct) reachable only through WgpuPainter::external_texture_registry[_mut] (painter/mod.rs:700-712). TextureLayer (flui-layer) and DrawOp::Texture are the consumers
- pub use ::wgpu (lib.rs:229): the exact linked wgpu (30.x) is part of the engine's public API

## Dependencies

**flui-engine (layer 4)**
- **Depends on:** flui-types, flui-foundation, flui-painting, flui-layer. External crates:
  - wgpu 30 (per-target backends: dx12, metal, vulkan, webgpu+gles with fragile-send-sync-non-atomic-wasm);
  - naga_oil 0.23; lyon 1.0; etagere 0.3; glam 0.33;
  - crossbeam-channel, parking_lot, rustc-hash, bytemuck, web-time, static_assertions;
  - optional wgpu-profiler;
  - build-time wgsl_bindgen and regex.
- **Consumed by (production):** only flui-app (its runners, raster_lane, device_recovery, surface_lifecycle).
- **Also used by:** the root facade (for features), examples/screenshot.rs (HeadlessRenderer) and examples/painting_demo (the imperative WgpuPainter API).
- **Not used by:** flui-testing does not depend on the engine, so widget tests never produce pixels.

**flui-painting (layer 2)**
- **Depends on:** flui-types, flui-foundation, cosmic-text 0.19, unicode-script (=0.5.8), unicode-segmentation, parking_lot, thiserror, tracing.
- **Consumed by:** flui-layer (3, so every crate above layer 3 compiles cosmic-text), flui-rendering, flui-objects, flui-widgets, flui-material, flui-testing (testing feature, for pin_font_faces), flui-app (installs the font system at realm install, runtime.rs:148), and the facade (src/painting.rs re-exports Canvas, DisplayList, DrawCommand, DrawOp).
- **cosmic-text containment:** outside flui-painting/src, cosmic-text is named only in flui-engine/src/paragraph_readback_tests.rs and tools/text-spike.

## Fit with the plan

**H0 (beta)**
- E1 damage producer: blocked on a layer-tree diff that does not exist. The prerequisite ADR-0061 called "missing" (layer identity) now partly exists: `LayerNode::with_render_id` in flui-rendering pipeline/owner/paint.rs:1258. So the remaining work is the diff plus the `DamageRegion::Partial` variant.
- E7 CPU reference renderer and G3 goldens: nothing exists. The crate doc says no second rasteriser is planned. The natural seam (`CommandRenderer`/`LayerStateStack`/`LayerRender`) is private to the GPU crate, and the effect layers live on `Renderer`.
- B1 per-realm FontSystem: blocked by `FONT_SYSTEM`, by the engine reaching the global directly, and by process-lifetime `GlyphKey`s shared across per-window atlases.
- B7 Parley: well prepared on the shaping side (cosmic-text is confined to about 5 files in painting). The rasteriser and key redesign are still open (ADR-0077 Proposed, whose own precondition names `GlyphKey = cosmic_text::CacheKey`). B1 and B7 touch the same seam and should be designed as one text-service change, not two sequential migrations.
- The "idle = 0 fps" and partial-repaint budgets are unmet: every frame is a full repaint.

**H1**
- WebGL2: the gles backend is compiled on wasm, and dual-source blending is capability-gated (ADR-0057), so the shape tolerates it.
- The system-compositor spike and external GPU content: TextureLayer, `DrawOp::Texture` and `ExternalTextureRegistry` exist and render, but no widget, render object or app API produces them. `Renderer` exposes no device or queue, so a third-party wgpu renderer cannot share a device.
- Mobile: the per-target backends are fine.

**H2**
- Raster lane: the protocol exists but is pumped inline; ADR-0045 is still Proposed.
- Layer caching, damage, mipmaps and stencil clip (E3): not started. `clip_path` is a bounding-box scissor (Mapping decision 6), and `ClipOp::Difference` is refused (decision 8).
- Software fallback for weak GPUs: excluded by the stated stance.
- "Multi-window as the norm": each window builds its own Instance, Device, pipelines, glyph atlas and texture cache, and `SharedRealm` with content is refused (flui-app secondary_window.rs:741).

**H3 (stability tiers)**
- The engine's public surface is ad hoc: `pub use ::wgpu`, `pub mod raster_owner`, the imperative `WgpuPainter` API, and `GpuCapabilities`. It ties FLUI's semver to wgpu's fast major cadence.
- Custom render objects and layers (H0 extension point) are fine at the render level. Custom paint vocabulary cannot be extended: `DrawOp` and `Layer` are closed enums, and there are no custom fragment shaders.

**H4 (Vello, 3D, video)**
- All of these need either the external-content seam or a swappable raster backend, and the crate doc rules out the latter.

**Delivery layers:** engine and painting are correctly core.

**Principle 3 (no global state):** violated by `FONT_SYSTEM`, which the crate itself documents as an ambient residual.

**Principle 5 (proof, not claims):** well served by the readback suites on WARP. They are merge-blocking per .github/workflows/ci.yml:778.

## Strengths

- A clean two-level IR. The Scene IR (DisplayList) is lowered to a GPU Command IR that holds no pooled textures, and replay is a pure function of the IR, proven by a deterministic replay test that renders one recording to two targets byte-for-byte (ADR-0006, engine ARCHITECTURE 'Record/replay boundary')
- Closed exhaustive enums (DrawOp, Layer, PresentDisposition) with no wildcard arms. Adding a paint op or a layer kind is a compile error until every consumer handles it
- No production unsafe in either crate: flui-engine has #![cfg_attr(not(test), deny(unsafe_code))] and flui-painting has #![forbid(unsafe_code)]. Surface ownership is typed (ADR-0063 SurfaceLease with a #[must_use] Released token and a compile-fail fixture)
- cosmic-text is well contained: outside flui-painting/src it appears only in one engine test file, and GlyphKey is opaque. The Parley migration surface on the shaping side is small, as the ADR-0077 analysis confirms
- The engine owns a shaper-agnostic glyph atlas (ADR-0067: two pages, Mask R8 and Color RGBA8, bucketed etagere packing, evict-then-grow). This removed glyphon and half of ADR-0059's objection to Parley
- A serious pixel-verification culture: about 38k lines of readback and oracle suites, including CPU oracles for blending, AA and the superellipse, run merge-blocking on WARP in CI (ci.yml:778-794). Mapping decisions each name the test that pins them
- An iterative layer walk that survives 10,000-deep trees on a small stack; frame-failure paths keep painter state balanced (Mapping decision 13)
- Capability-gated correctness: dual-source blending for coverage-correct clip AA, with a documented degraded mode on WebGPU (ADR-0057)
- Font resolution is hermetic and tested against generated fixture faces (tools/decoy-face), with licence and provenance inventory for bundled fonts
- The GPU-free lowering modules already exist as a separable set (layer_walk, layer_render, dispatch, command_renderer, damage, raster, raster_owner, superellipse all have 0 wgpu references), so extracting a backend-neutral core is a move, not a rewrite

## Problems

### The engine's stated stance ('wgpu is the engine, nothing pluggable') contradicts principle 2, E7, the H2 software fallback and the H4 Vello option

- **Kind:** plan_misfit · **Severity:** high
- **Evidence:** crates/flui-engine/ARCHITECTURE.md:8-14 says 'No other rasteriser (Vello, Skia, a software path) is planned, and nothing here exists to make one pluggable'. Mapping decision 1 says 'A trait or feature whose only justification is a backend swap is deleted, not kept warm'. raster.rs:6 says 'It is a test seam, not a plugin point: wgpu is the engine and no second backend is planned'. The plan says the renderer may change but is one on all platforms (principle 2), puts a CPU reference renderer for goldens in H0 (roadmap E7), and a software renderer for weak GPUs in H2. flui-geometry/src/bridges/mod.rs:6 even mentions 'the future Vello raster backend', and lib.rs:510 calls RasterBackend 'the frame-driver swap point'. The docs disagree with each other.
- **Impact:** A merge-blocking crate doc actively steers agents to delete any swap seam, so E7, G3 goldens and the H2 software fallback cannot be built without first reversing a recorded decision. Principle 2 becomes unenforceable in practice.
- **Direction:** Supersede this stance with an ADR, 'one renderer contract, several rasterisers'. The production path is still wgpu everywhere, but the scene-lowering contract is a named, backend-neutral seam that the CPU reference, and later a software or Vello backend, implement. Fix the contradictory comments in the same change.

### The backend-neutral lowering core is locked as pub(crate) inside the wgpu crate, and effect layers are special-cased on Renderer

- **Kind:** layering · **Severity:** high
- **Evidence:** The GPU-free modules (wgpu references counted with grep -c) are layer_walk.rs 0, layer_render.rs 0, dispatch.rs 0, command_renderer.rs 0, damage.rs 0, and layer_state_stack.rs with 2 (docs only). CommandRenderer (command_renderer.rs:31) and LayerStateStack (layer_state_stack.rs:41) are pub(crate). BackdropFilter, ShaderMask and Follower are 'diverted' to Renderer's own handlers (ARCHITECTURE 'One frame', Renderer::handle_shader_mask). layer_dispatcher.rs, which owns the clip and opacity discipline, has 19 wgpu and 18 WgpuPainter/OffscreenRenderer references.
- **Impact:** A CPU reference renderer (E7), a golden pipeline in flui-testing (G3) or a future backend must either live inside flui-engine or re-derive walk order, clip and opacity semantics and effect-layer semantics. The layer semantics (what a BackdropFilter means) are defined in GPU code rather than in a contract.
- **Direction:** Extract a backend-neutral scene-lowering module set: layer walk, LayerRender, CommandRenderer, LayerStateStack, the effect-layer decomposition (backdrop, mask and follower as neutral steps), damage, and the raster protocol. Put it in a lower layer, either flui-layer or a new crate below engine. Per the 'layers, not micro-crates' rule it is a genuine layer: backend contract below backends. flui-engine (wgpu) and a flui-engine-cpu or reference module then implement it.

### HeadlessRenderer is a second walker that renders three layer kinds differently from the on-screen Renderer

- **Kind:** runtime_architecture · **Severity:** medium
- **Evidence:** crates/flui-engine/src/headless.rs:15-27: a BackdropFilter gets no blur, a ShaderMask is 'clipped to the mask bounds, with no mask applied', and a Follower gets 'no leader offset'. The readback suites capture through this renderer, so those three kinds are pinned only by renderer.rs's own tests. examples/screenshot.rs:68-109 uses it for screenshots.
- **Impact:** Any screenshot, golden or agent capture built on the headless path (G3, the G7 record/replay evidence, desktop-mcp alternatives) silently differs from production for glass, blur, mask and overlay-follower UIs. That breaks the 'determinism in tests' bet and principle 5.
- **Direction:** Put one scene-lowering path behind a render-target abstraction (surface texture vs caller texture), so headless differs from windowed only in where pixels land. Add a parity test that renders the same scene with both walkers.

### No golden or pixel path reaches the widget test tier, and E7's form is undecided

- **Kind:** testing · **Severity:** medium
- **Evidence:** flui-testing/Cargo.toml depends on flui-painting (testing feature) and not on flui-engine. HeadlessRenderer's only non-engine user is examples/screenshot.rs. GPU pixel tests exist only inside flui-engine (gpu-test job on WARP, ci.yml:778). grep for tiny-skia, CpuRenderer or 'reference renderer' in crates/ finds nothing.
- **Impact:** The G3 goldens and the H0 exit ('an agent walks a scenario via flui mcp' with golden evidence) have no substrate. A CPU renderer written separately would test a rasteriser that never ships (pixels differ from wgpu), unless it is framed as an oracle.
- **Direction:** Decide explicitly in an ADR between two options. (a) Goldens produced by the production wgpu path on a pinned software adapter (WARP, lavapipe or SwiftShader) with tolerance. (b) A CPU reference behind the extracted CommandRenderer seam, with bundled fonts, used as an oracle. Either way, flui-testing gains a capture API over the single lowering path.

### The process-global FONT_SYSTEM shares one mutex between shaping and rasterisation, the engine reaches it ambiently, and GlyphKey is process-scoped

- **Kind:** runtime_architecture · **Severity:** high
- **Evidence:** crates/flui-painting/src/text_layout/layout.rs:124 has `static FONT_SYSTEM: OnceLock<Arc<Mutex<FontState>>>`. shape() (:329-331), rasterize() (:348) and generation() (:374) all lock it. The engine grabs the global at painter construction (flui-engine/src/painter/mod.rs:167, glyph_atlas.rs:443). glyphs.rs documents that a key 'stays valid for the life of the process'. init_font_system_with_faces is irreversible per process (layout.rs:203-210), which forces the separate font_registration test binary (painting Cargo.toml).
- **Impact:** It violates principle 3 and blocks B1. Layout on N realms or windows and glyph-atlas misses serialise on one lock, and a future threaded raster lane (ADR-0045) would contend with UI-thread shaping on every glyph miss, which amounts to a lock on per-frame state. Per-realm font systems would make GlyphKeys from different realms collide in any shared atlas. Tests cannot pin different font sets in one process.
- **Direction:** Design B1 and B7 together as a 'text service' contract. It is owned by the realm (or a process-level font collection with per-realm views), passed explicitly to TextPainter and to Renderer::new, never fetched ambiently. The glyph key carries a font-collection or font-blob identity. Rasterisation uses its own scaler state outside the shaping lock, for example swash per raster thread. Add a gate that fails on new statics in painting and engine.

### The Parley migration (ADR-0077) is framed as a shaper swap, but the durable contract to settle is DisplayList::Paragraph plus the key and rasteriser seam

- **Kind:** extension_point · **Severity:** medium
- **Evidence:** DrawOp::Paragraph { layout: Arc<TextLayout> } (display_list/command.rs:167-174) embeds the concrete shaped product in the wire. GlyphKey = cosmic_text::CacheKey (glyphs.rs:23). rasterize goes through cosmic_text::SwashCache::get_image_uncached (layout.rs:348-366). ADR-0077 states that parley does not rasterise and that a new GlyphKey plus a rasteriser are unprototyped preconditions. flui-layer (layer 3) depends on flui-painting, so the text stack is in the closure of every crate at layer 3 and above.
- **Impact:** Each text-stack change (Parley, per-realm fonts, a CPU renderer that must rasterise glyphs identically, variable-font axes, rope editor in H2) ripples through the paint wire format and the engine atlas at once. The one-week spike budget covers only rasterisation, not this seam.
- **Direction:** Define the paragraph crossing as a backend-neutral 'shaped run' contract (placed glyphs keyed by font blob id + glyph id + size + variation coordinates + subpixel bin, plus a Rasterizer trait). Both the wgpu atlas and a CPU reference consume it. Make ADR-0077's acceptance criteria include this contract, not only 'glyphs pass oracle tests'.

### Each window builds a full private GPU stack: its own Instance, Device, pipelines, glyph atlas and texture caches

- **Kind:** performance · **Severity:** medium
- **Evidence:** Renderer::new creates wgpu::Instance::new and request_adapter per call (renderer.rs:1140-1168). Every runner calls Renderer::new per window (flui-app desktop.rs:101, android.rs:191, ios.rs:267, web.rs:99). A WgpuPainter is 'nine pipelines and a glyph atlas' (ARCHITECTURE Mapping decision 11). secondary_window.rs:741 refuses SharedRealm with content because 'the realm's raster lane and content renderer are per-realm, not per-presentation'.
- **Impact:** The H2 goals (multi-window as the norm, memory and cold-start budgets under 300 ms) pay N times for pipeline and shader compilation and glyph rasterisation. Textures cannot move between windows (drag images, shared video or external content), and a shared-realm, multi-presentation design needs a redesign.
- **Direction:** Introduce a process- or app-level GpuContext (instance, adapter, device, queue, pipeline cache, glyph atlas, image cache) and a per-window Presentation (surface, damage, frame state). Honour the rule that 'the Instance and surface are created together' by creating the instance first and the surfaces from it. Record this as an ADR alongside ADR-0045 and ADR-0063.

### Damage is consumed but never produced; ADR-0061 is stale and no layer-tree diff exists

- **Kind:** missing_capability · **Severity:** high
- **Evidence:** The damage.rs accumulator is crate-private. Production callers only ever mark a full repaint: flui-app direct.rs:181 and raster_lane.rs:486, and raster_lane.rs:354 builds SceneSnapshot::new(stamp, DamageRegion::Full, scene). The DamageRegion enum has only Full (ADR-0061 Consequences). ADR-0061 says layer identity 'does not exist', yet flui-rendering pipeline/owner/paint.rs:1186/1258/1422 now populates LayerNode::with_render_id. The damage_scissor bench shows full vs damaged at 2901 µs vs 56 µs for 64 layers (ADR-0061).
- **Impact:** Every frame repaints the whole surface, which misses the H0 B2 'partial repaint' milestone and the H2 perf and battery budgets. A recorded ADR now misstates the code, which counts as a defect under the project's ADR policy.
- **Direction:** Amend ADR-0061: identity now exists for boundary layers. Build the producer as a render_id-keyed diff of consecutive LayerTrees (Arc::ptr_eq on PictureLayer display lists, plus transform, opacity and clip deltas) in the backend-neutral layer, emit DamageRegion::Partial, and gate it on the existing bench.

### The external GPU content extension point is implemented at the bottom but unreachable from any production path

- **Kind:** extension_point · **Severity:** medium
- **Evidence:** TextureLayer is defined in flui-layer (layer/texture.rs) and rendered in engine layer_render.rs:87/365. DrawOp::Texture is dispatched in dispatch.rs:74. `grep -rln TextureLayer` outside tests finds only flui-layer and flui-engine: no render object, widget or app code produces one. ExternalTextureRegistry sits in a private module (lib.rs:288), and its only accessors are WgpuPainter::external_texture_registry[_mut] (painter/mod.rs:700-712), returning an unnameable type. Renderer, which owns the painter privately, exposes no device, queue or registry.
- **Impact:** The plan's H1 compositor spike and H2 'wgpu texture or third-party renderer as a widget' have no reachable API, and video, camera, maps and 3D (H4) have no path. The surface is dead weight that looks like a feature (the AGENTS.md 'unwired surface' defect class).
- **Direction:** Design the seam top-down. Expose a typed capability acquired via LifecycleContext (ADR-0078) that returns a TextureHandle bound to the shared GpuContext device. Provide a Texture render object and widget that emit TextureLayer, with a frame-sync contract (who updates when, and fences). Pin it with a readback test through the real app path. Until then, delete or feature-gate the engine registry.

### The public engine API is not shaped for H3 stability tiers: it re-exports wgpu, has a public imperative painter and a public raster_owner module

- **Kind:** api_dx · **Severity:** medium
- **Evidence:** flui-engine/src/lib.rs:229 has `pub use ::wgpu`, with the workspace pinned to wgpu 30.0 (Cargo.toml:224). lib.rs:216 has `pub mod raster_owner` plus the root re-exports at :514. WgpuPainter's public draw_* API is used outside the crate only by examples/painting_demo. GpuCapabilities is public. The only production consumer of any of it is flui-app.
- **Impact:** Once FLUI 1.0 is frozen, every wgpu major becomes a FLUI breaking change, and wgpu majors ship several times a year. The imperative WgpuPainter is a second drawing API parallel to Canvas and DisplayList that users could bind to. It is hard to tell users which parts are Stable, Evolving or Experimental.
- **Direction:** Make the engine's public surface minimal and tiered: GpuContext, Presentation, capture, and an explicit `unstable-wgpu-interop` feature for device sharing and the wgpu re-export. Make WgpuPainter and raster_owner crate-private or #[doc(hidden)] behind `testing`. Add cargo-semver-checks on the engine in H3.

### The raster mailbox is heavyweight cross-thread machinery pumped synchronously on the UI thread while ADR-0045 is still Proposed

- **Kind:** tech_debt · **Severity:** medium
- **Evidence:** raster_owner.rs is 4,365 lines (about 600 code, about 2.7k tests) and uses parking_lot::Mutex, a condvar, two atomics and bounded crossbeam channels (ARCHITECTURE 'Ownership' table). flui-app raster_lane.rs:4-13 describes 'a RasterOwner whose pump runs synchronously on the owner (UI) thread'. desktop.rs:305-316 wraps it in Arc<Mutex<RasterLane>> with a try_lock that degrades reentrancy to a skipped frame. ARCHITECTURE Open items says 'tested but not wired' for the threaded lane. ADR-0045 status is Proposed.
- **Impact:** Every frame pays the protocol's complexity (generations, stamps, rejection paths) without the benefit of a real raster thread. The runtime-topology decision behind H2 raster/IO lanes stays open while code hardens around it, and a lock sits on the frame path in the app.
- **Direction:** Decide ADR-0045: either ship the threaded lane on Windows and Linux with measurements, or collapse the inline path to a direct call and keep the mailbox as the documented future shape. Move the raster protocol out of the GPU crate into the runtime and scheduler side, since it is topology, not rasterisation.

### The paint vocabulary has correctness gaps that widgets can hit, and no extensibility for custom shaders

- **Kind:** missing_capability · **Severity:** medium
- **Evidence:** Engine ARCHITECTURE Mapping decision 6: clip_path installs only the path's bounding-box scissor; content inside the box but outside the shape renders. Mapping decision 8: ClipOp::Difference is refused on all four clip shapes. The DrawOp variant list (command.rs:53-364) has no custom-shader or fragment-program variant, and flui-types Shader has only gradient variants. Both DrawOp and Layer are closed enums by design (painting Mapping decision 1). The roadmap schedules E3 (stencil path clip, mipmaps, custom fragment shaders).
- **Impact:** Arbitrary-shape ClipPath, and Material or Cupertino shapes that rely on path clips, render wrongly with no error, only a warn for Difference. The planned custom render objects and layers extension point (H0) cannot introduce new paint primitives or shaders, so third-party catalogs are limited to composing the fixed vocabulary.
- **Direction:** Add stencil or coverage-mask path clipping in E3 before beta, since it is visible correctness. Design one open extension variant (for example DrawOp::Custom with a registered, typed CustomPaintProgram carrying WGSL plus uniforms, and a CPU fallback for the reference renderer) rather than opening the enums.

### flui-engine's size is mostly in-crate tests and prose, and the audit metric misreads it

- **Kind:** tech_debt · **Severity:** low
- **Evidence:** 72,479 total lines in src. 22,440 are in 21 *_tests.rs/*_oracle*.rs files declared as cfg(test) modules in lib.rs:402-502, 14,716 are inline #[cfg(test)] mod blocks, and 677 are test support. Production is about 35.1k lines = 18.8k code + 13.7k comment lines + 2.5k blank (counted with an awk brace-depth pass over non-test modules), plus 4.6k lines of WGSL. The context audit lists 'engine 74.0k non-test LOC'.
- **Impact:** Architecture decisions such as 'split the engine' would be made on a 4x overstated number. In-src suites recompile with every unit-test build of the crate. The prose-heavy comments (more than two comment lines per three code lines) raise the cost of keeping docs truthful, and some are already stale (ADR-0061, raster.rs vs lib.rs).
- **Direction:** Fix the LOC metric in xtask to exclude cfg(test) and comments. Readback suites that need pub(crate) access stay in-crate but move under a src/tests/ directory. Consider moving rationale from comments into ARCHITECTURE Mapping decisions, where it is already partly duplicated.

### Image GPU cache identity uses raw Arc data-pointer addresses (hypothesis: ABA stale texture)

- **Kind:** safety · **Severity:** low
- **Evidence:** flui-engine/src/texture_cache.rs:64-96 has TextureKey::Pointer(usize) from `Arc::as_ptr()`. batches/images.rs:118/307/1092 key draw_image by `image.data_ptr()`. CachedTexture holds a TextureView, not the Arc (texture_cache.rs:100-120), so the allocation can be freed and its address reused while the entry survives until end_frame_maintenance evicts it.
- **Impact:** Hypothesis, not reproduced: an image dropped and replaced by a new decode landing at the same address within the retention window would draw the old texture. Structurally, flui-types::Image has no stable content or identity id, so every cache (engine, a future CPU renderer, devtools) must invent one.
- **Direction:** Give flui_types::painting::Image a stable ImageId (a monotonic counter or content hash at decode) and key GPU caches on it. Alternatively, hold a Weak or strong reference in the cache entry. Add a test that drops and reallocates to demonstrate or refute the ABA.

### Registering a font does not invalidate text layout; the realm-level broadcast is missing

- **Kind:** missing_capability · **Severity:** low
- **Evidence:** flui-painting ARCHITECTURE Open items: 'Nothing marks text render objects dirty on register_font... the layout is not requested by the registration'. Caches key on the global generation() counter, which locks the global mutex on every check (layout.rs:374).
- **Impact:** Web fonts, downloaded fonts and asset fonts loaded after first frame appear only on the next unrelated relayout. It is a small DX trap that also shows the text service has no realm owner to broadcast from.
- **Direction:** Fold this into the realm-owned text service. Font-collection changes become a realm event that marks text render objects needing layout, the same way Flutter's systemFonts listener works but realm-scoped.

## Unwired or dead surface

- ExternalTextureRegistry + WgpuPainter::external_texture_registry[_mut] (flui-engine painter/mod.rs:700-712) + flui_layer TextureLayer + DrawOp::Texture: rendered by the engine but produced by no render object, widget or app API, and unreachable because Renderer keeps its painter private
- Renderer::mark_dirty / RasterBackend::mark_dirty: no production caller. DamageRegion has only Full, and the damage.rs accumulator's partial path is never exercised in production
- HeadlessRenderer: the only non-engine user is examples/screenshot.rs; no flui-testing or devtools path
- WgpuPainter's public imperative draw_* API and WgpuPainter::with_shared_device: outside the crate, used only by examples/painting_demo
- The threaded mode of RasterOwner (run_until_shutdown, condvar and ack channel across threads): only the inline pump is used (flui-app raster_lane.rs)
- pub use ::wgpu (lib.rs:229): no in-repo consumer names flui_engine::wgpu, based on the grep of flui_engine:: uses outside the crate
- The GpuCapabilities public accessors (renderer.rs:469-509): consumed only inside the engine, per the grep of flui_engine:: uses
- LayerNode::render_id: populated by flui-rendering paint.rs:1258 but not read by any damage or diff consumer (the other reads are diagnostics in paint.rs:1364)

## Open questions

- E7: should goldens come from the production wgpu path on a pinned software adapter (WARP, lavapipe or SwiftShader), which tests what ships, or from a separate CPU reference renderer (tiny-skia-like) behind the CommandRenderer seam, which is deterministic across operating systems but a second rasteriser? The owner's 'golden on the CPU reference with a bundled font' implies the latter. It conflicts with the engine's stated stance and needs an ADR.
- Font ownership: should the font collection be per-realm, or process-level and immutable with per-realm views (cheaper, and preserves a shared glyph atlas across windows)? Principle 3 says per-realm, while memory and atlas sharing argue for a shared immutable collection with explicit handles.
- Will the threaded raster lane (ADR-0045) ship before 1.0? If not, should the mailbox protocol be simplified now? macOS is pinned inline by a wgpu-hal constraint (issue #653) and wasm is always inline.
- Should one wgpu Device be shared across all windows of an app, with a surface per window created from the same Instance? What does that do to device-loss recovery, which today is per-Renderer?
- Where should the external texture capability live? As a LifecycleContext capability in flui-view (ADR-0078), as a flui-platform capability, or as part of the future PlatformCapability plugin model in H1? And what is the synchronisation contract with third-party wgpu renderers?
- Does the backend-neutral lowering core belong in flui-layer (layer 3, which already defines Scene and DamageRegion), or in a new crate between layer and engine? This interacts with the 'layers, not micro-crates' decision.
- The roadmap cites docs/runtime-contract.toml and an ambient-reach ratchet for FONT_SYSTEM, but no such file or xtask check exists in the checkout (grep of docs/ and tools/xtask finds none). Is the principle 3 ratchet unenforced?
- Is DisplayList serialisation needed for G7 record/replay or devtools frame inspection? ADR-0066 forbids serde on the wire, and Arc<TextLayout> is not serialisable.

