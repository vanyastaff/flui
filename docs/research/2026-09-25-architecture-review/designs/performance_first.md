# FLUI target global architecture from a performance-first angle

Scope: this covers the whole workspace, crate topology, runtime model, extension seams and how they carry H0–H4. I grounded it in the codebase maps (main @ cab06137d) and the market surveys. Anything marked **Hypothesis** was not measured. I did not build the workspace; all evidence below comes from the maps' `cargo tree`/grep/path citations.

---

## 0. Verdict in one paragraph

The components FLUI's performance depends on are mostly already present:
- a single-writer realm;
- typestate pipeline phases over slab arenas;
- a B+-tree virtualizer (ADR-0003);
- composited-layer capture patching;
- a `Send + Sync` `Scene` (flui-layer/src/scene_snapshot.rs:76-78);
- field-masked inherited dependencies;
- a FrameClock that skips idle segments (flui-app/src/app/ui_realm/frame.rs:147-185).

What stops the H2 budgets from being *structural* is that frame output has no cross-frame identity, and several O(tree) steps sit inside frame work:
- every frame mints a fresh `LayerTree` (flui-rendering/src/pipeline/owner/paint.rs:86-189, 1383-1425);
- every frame is a full repaint (flui-app/src/app/raster_lane.rs:354, :486);
- any render insert triggers a global topology pass (flui-view/src/tree/element_tree.rs:1428-1520);
- each dirty root scans the whole slab (flui-rendering/src/storage/tree.rs:82-110);
- view configs are deep-cloned on every rebuild (flui-view/src/view/into_view.rs:178-184);
- UI and raster run serially and are additionally coupled through a process-global font mutex (flui-painting/src/text_layout/layout.rs:124, flui-engine/src/painter/mod.rs:167).

The crate topology makes the compile-side cost worse. One trait import (flui-interaction/src/text_input.rs:27) pulls winit, tokio and `windows` into the headless render stack, so a Win32 backend edit rebuilds 16 crates. The target architecture below makes "cost ∝ change" a checked invariant. It turns the budgets into deterministic per-PR counters and re-cuts crates along rebuild fan-out and heavy-dependency reach. It keeps the user's mental model and most already-taken decisions.

---

## 1. Guiding principles

### 1.1 Performance principles (this angle)

| # | Principle | Mechanism that makes it structural |
|---|---|---|
| PF1 | **Cost ∝ change, never ∝ tree.** Every stage from build to present is keyed by identity and dirtiness. An O(total) pass inside a frame is a defect unless it is a debug verifier. | Stable IDs through every tree, including layers; deterministic work counters per frame; per-PR gates on counters |
| PF2 | **Idle is zero.** One authority decides whether a presentation produces a frame. | A single per-presentation `DemandMask` in the frame clock; everything else calls `mark_demand(reason)` |
| PF3 | **Owner-local means lock-free.** Realm state is `!Send` and accessed through `&mut`/`RefCell`. `Send` exists only at lane boundaries (commands in, `Scene` out, IO results in). | `clippy::disallowed_types` for `Mutex`/`RwLock`/`DashMap` in the frame-path crates; `assert_not_impl_any!` tests |
| PF4 | **Budgets are gates, wall-clock is a trend.** Count-based metrics (elements built, nodes laid out, layers recomposed, bytes uploaded, damage area, allocations) are deterministic on the virtual clock and gate every PR. Timing stays nightly and per OS. | `cargo xtask perf` in `checks` |
| PF5 | **Crates are cut along rebuild fan-out and heavy-dependency reach.** The headless stack (values → widgets) links no OS, GPU, tokio or shaping-engine dependency it does not use. | "reach facts" in manifests, checked by `cargo xtask workspace` |
| PF6 | **Shared heavy resources are explicit infrastructure, never process globals.** Fonts, glyph atlas, GPU device, pipeline cache and image cache have one owner each, passed explicitly. | globals ratchet gate |
| PF7 | **The frame contract carries identity and damage.** A `Scene` without identity cannot be cached, diffed, presented partially or embedded. | retained layer IDs + `DamageRegion::Rects` in the lane protocol |

### 1.2 Reconciliation with the owner's 7 principles

1. **Mental model is sacred.** I agree. None of this changes View → Element → Render, keys or constraints. The performance model lives underneath it. One refinement: signals may bind to render-object properties so an animated value repaints without a rebuild. Compose does this with phase-scoped reads (developer.android.com/develop/ui/compose/phases). The user still writes declarative views.
2. **One language, one toolchain, one renderer.** I disagree with the literal wording. It should read "**one render contract**, several rasterisers." Several things need a second rasteriser: the H0 CPU golden reference (E7), the H2 software fallback, and wgpu backends that exist on only one API per OS (flui-engine/Cargo.toml:105-112: dx12/metal/vulkan, no GLES). That second rasteriser must implement the *same* lowering, or it drifts. It already drifts in-crate: HeadlessRenderer paints BackdropFilter, ShaderMask and Follower differently (flui-engine/src/headless.rs:15-27). The engine's own stance ("nothing here exists to make one pluggable", flui-engine/ARCHITECTURE.md:8-14) should be superseded by an ADR.
3. **No global state.** I agree, and this angle makes it a hard performance requirement. The global FONT_SYSTEM serialises shaping across realms and blocks raster-lane overlap. The ratchet file the roadmap cites (`runtime-contract.toml`) does not exist, and no xtask checks statics. Reinstate it as a gate.
4. **Everything machine-readable.** I agree. Per-frame work counters and `FrameBuildReport` belong in the agent/devtools protocol stream, so agents can run perf loops the way Chrome DevTools MCP does.
5. **Proof, not claims.** I agree. Today it fails for performance: the weekly bench job is `continue-on-error` (weekly.yml:8-12), and `bench-collect` skips every feature-gated bench, including the damage baseline ADR-0061 names (tools/xtask/src/bench.rs:36).
6. **Break explicitly.** I agree. §9 lists breaks that must precede H3, because they change `paint`'s output contract and trait supertraits.
7. **Market before decision.** I agree. The findings used most here: Compose phase reads, GPUI lease plus effect queue, gpui-fast (keeping layout nodes gave about 2×), Graphite/Impeller pipeline prewarm, Subduction/frameclock presenter plus demand classes, and fontique shared collections.

**Taken decisions I explicitly challenge:**
- **ADR-0041 "no flui-runtime until two entry points".** The second entry point already exists: flui-testing re-implements the frame transaction (flui-testing/src/lib.rs:955-1079). Extract the runtime now.
- **"Parallel layout inside a realm?" (open H2 spike).** Decide *no* now and record it. The types already foreclose it: `RenderObject` is not `Send` (flui-rendering/src/traits/render_object.rs:178) and `PipelineCell` is `Rc<RefCell>`. Leaving it open keeps `Arc<RwLock>` shapes alive in public API before the freeze. Parallelism goes *across* lanes (raster, IO, compute, shaping) and *across* realms, not inside layout.
- **"flui-widgets stays one crate."** I keep it. The graph is a deep chain, so splitting gains little unless siblings are independent (matklad; the Feldera result needs graph width). The module-direction gate that justified the decision does not exist (tools/xtask/src/tasks/checks.rs:100-120 has none), so add it.

---

## 2. Workspace and crate topology

### 2.1 Target tiers (collapsing 11 layers into 7)

The layer numbers today (Cargo.toml:92-104) are finer than they are useful. They also hide the only property that matters for compile time and wasm: *which crates are OS-free and GPU-free*. Layer 2 holds both the platform contract and 46k lines of backends. Target:

```
T0 values         flui-geometry  flui-types  flui-foundation  flui-macros
T1 contracts      flui-platform-api (new)   [reactive core lives in foundation]
T2 substrate      flui-painting  flui-text (new)  flui-interaction  flui-scheduler  flui-assets  flui-log
T3 compositing    flui-layer  flui-semantics  flui-animation
T4 render machine flui-rendering  →  flui-objects        flui-raster (new) → flui-engine
T5 spine+runtime  flui-view  flui-widgets  flui-runtime (new)  flui-testing
T6 composition    flui-platform (backends)  flui-app  flui (facade)
packages/         flui-material  flui-cupertino  flui-protocol (new)  flui-mcp  flui-a2ui (H1)
tools/            flui-cli  xtask  live-smoke
```

**Reach facts, checked by xtask and declared as manifest data (`[package.metadata.flui] forbid-reach`):**
- T0–T5 contain none of winit, tokio (rt-multi-thread), windows, objc2, android-activity, wgpu, zbus.
- T0–T3 and flui-rendering/objects/view/widgets contain no shaping engine (parley/harfrust/icu, cosmic-text).
- Only flui-engine and flui-app contain wgpu.

This replaces "direction only" checking (tools/xtask/src/workspace.rs:268-282) with the property that actually drives rebuild cost and wasm hygiene.

### 2.2 New crates and why each is a *layer*, not a micro-crate

| New crate | Tier | Responsibility | Public surface | Performance justification |
|---|---|---|---|---|
| `flui-platform-api` | T1 | Capability traits (`PlatformTextInput` redesigned as a text-store client, `PlatformAccessibility`, clipboard/dialog/haptics as `PlatformCapability`s), input/IME/lifecycle vocabulary, `Unsupported`, the owner/proxy capability types, a reusable backend-conformance kit. Deps: types, foundation, ui-events, accesskit, raw-window-handle | traits + value types | Removes winit/tokio/windows from rendering and everything above. Backend edits stop rebuilding 16 crates (the maps' fan-out table). Plugins depend on about 30 crates, not 198. |
| `flui-text` | T2 | Parley/fontique/ICU4X shaping and layout, per-realm `LayoutContext` over a shared immutable `Collection` (`shared: true`, docs.rs/fontique CollectionOptions), a glyph rasteriser (skrifa/zeno, or glifo after the spike), segmentation and BiDi for editing | `TextLayout` (opaque), `ShapedRun`, `FontCollection`, `GlyphRasterizer` | Takes cosmic-text out of flui-painting, which 17 crates reach today via flui-layer. The Parley swap and ICU data then rebuild only text consumers. Removes the global lock: shaping and rasterisation get separate owners. |
| `flui-raster` | T4 | Backend-neutral lowering extracted from flui-engine's GPU-free modules (layer_walk, layer_render, dispatch, command_renderer, layer_state_stack, damage: all 0 wgpu refs per the engine map). Adds effect-layer decomposition, the layer-tree **diff → damage** producer, the lane protocol (`RasterOwner`, moved out of engine), a conformance suite, and a CPU backend module (`cpu` feature: vello_cpu or tiny-skia, decided in the E7 ADR). | `SceneLowering`, `CommandSink`, `Presenter` trait, `DamageRegion`, `RasterLane` | One lowering serves the GPU, CPU and headless paths, which ends the headless.rs divergence. Damage is produced where identity lives, and the CPU fallback becomes a production backend. |
| `flui-runtime` | T5 | Realm, presentations, **the frame transaction**, demand authority, lanes (raster/IO/compute), execution services, `Spawner` capability, reactive-graph ownership, capability registry, `RealmObserver`. Platform-free, parameterised by `Clock` + `Presenter` + `PlatformHost`. | `Realm`, `App` builder internals, `RealmObserver`, `Spawner`, `CapabilityRegistry` | One frame transaction for production and tests, so perf counters measure what ships. flui-app shrinks to runners, and runtime edits stop relinking 245 deps. |
| `flui-protocol` (package) | T2-ish (serde + accesskit only) | ADR-0080 wire types (handles, roles as `accesskit::Role`, error codes, outline format), devtools streams (frame counters, traces, diagnostics), record/replay log | types + schemas | Perf telemetry becomes a protocol stream, so agents can measure, fix and re-measure. |

`flui-platform` stays as the **backends crate** (T6, consumed only by flui-app). Native backends are primary and winit is the Linux backend (see §3.9). Per-backend crates are rejected for now. That split would matter only for community backends (H4), and the #560 minting seam can live in `flui-platform-api` without it.

### 2.3 Fate of every current crate (27)

| Crate | Fate | Target contents / reason (evidence) |
|---|---|---|
| flui-geometry | **Keep, shrink** | Delete the unused GPUI vocabulary (~3.5k lines: length.rs, transform2d.rs, bezier.rs, text_path.rs, kurbo bridge, no-op `mint`). Make `Pixels` Eq/Hash consistent (units.rs:91, 575-596): today `±0.0` compare equal but hash differently. That breaks layout-cache keys, which H2 relies on. |
| flui-types | **Keep, shrink** | Delete physics (1.8k lines, duplicated in flui-animation/src/simulation.rs), `BoxConstraints` (duplicate), `MaterialColors`, and orphan types. Move gesture details to interaction and `Path/Paint/Shader` to painting. It is rebuilt by 24 crates, so every line removed lowers fan-out. Rule: an item needs ≥2 consuming crates. |
| flui-foundation | **Keep, re-cut** | Keep IDs, keys, callbacks, diagnostics vocabulary, observe seam, **reactive graph core** (moved down from flui-view, §3.5), and the arity/slot/depth markers (from flui-tree). Move runtime-protocol identities (FrameStamp, SurfaceGeneration, PresentationAddress, ClaimSlot, OwnerAffinity) to flui-runtime/flui-platform-api. Today they sit here only because platform tests didn't run in CI (affinity.rs:11-13). `ChangeNotifier` becomes `!Send` Rc-based (notifier.rs:46, notifier_generic.rs:41-45). |
| flui-macros | **Keep** | Add `#[derive(RenderView)]`, `#[derive(Store)]` (reactive collections), and catalog metadata emission (G6/A2UI). Document the proc-macro upward edge as an allowed exception. |
| flui-tree | **Merge into flui-foundation** (markers) + delete trait trio | TreeRead/Nav/Write have no generic consumer and ElementTree doesn't implement them (tree/element_tree.rs:394). Drops `bon`. One fewer publish unit. |
| flui-platform | **Split**: contract → `flui-platform-api`; backends stay | See §2.2. Also: delete the `desktop = ["dep:winit"]` default with no cfg sites (Cargo.toml:300-303), the second `Window` trait family (src/window.rs), `PlatformEmbedder`, the tokio `BackgroundExecutor` (executor.rs:66), `LinuxPlatform` stub, and the no-op wayland/x11/web features. |
| flui-scheduler | **Keep, slim** | Split into an owner-local `!Send` core (Cell/RefCell queues) plus a tiny `Send` waker. Today there are ~20 Mutex fields plus DashMap (scheduler.rs:743-855). Remove the global `TIME_DILATION` (config.rs:43). Move `AsyncDriver` to flui-runtime as a `!Send` local executor (async_driver.rs:102 requires `Send` for no reason). |
| flui-painting | **Keep, narrow** | Recorder only: Canvas, DisplayList, `DrawOp::Paragraph` carrying an opaque `Arc<TextLayout>` from flui-text via a trait object or `ShapedRun`. Remove `pub use cosmic_text::fontdb::Family` (lib.rs:87). |
| flui-interaction | **Keep** | Depend on flui-platform-api only. Arena and recognizers become `Rc`/`RefCell`: the crate-wide `arc_with_non_send_sync` expect (lib.rs:139-143) goes away along with the 17 lock sites in arena/mod.rs. Move `InteractionLane`'s generic owner-local closure registry (clip/shader-mask targets) to flui-runtime/rendering as an explicit context argument, replacing the TLS (interaction_lane.rs:738-742). |
| flui-assets | **Keep, de-runtime** | Loaders take an injected `Spawner`. Delete the bridge runtime (registry/bridge.rs:42-66) and `AssetRegistry::global()`. Move the decoded-image cache here from flui-widgets' static CACHE (image/decode_cache.rs:96) as a realm/app resource with a byte budget, keyed by `ImageId` (fixes the pointer-ABA hypothesis in texture_cache.rs:64-96). Add a wasm fetch loader. |
| flui-log | **Keep** | Unchanged. Add a hot-path log discipline gate: `info!` in element/behavior.rs:1059,1090 fires on every render-element build. |
| flui-layer | **Keep, extend** | **Retained** layer identity: `LayerId` stable per repaint boundary/effect node, derived from `RenderId` (already stamped: layer_tree.rs:38). `DamageRegion::Rects`. One open variant `Layer::External { id, rect }` for GPU content. Delete no-producer variants until wired (PlatformView no-op, layer_render.rs:381). |
| flui-semantics | **Keep** | Publish from the same retained identities. The action target becomes an owner-lane ID, not an `Arc<dyn Fn + Send + Sync>` (action.rs:217). Measure the mirror cost for 100k lists before a11y goes default-on. |
| flui-animation | **Keep** | `AnimationController` becomes `!Send` (controller.rs:177-208 records the exception). One clock per presentation; retire the wall-clock Ticker path. Values can bind to render properties (paint/layout phase subscribers). |
| flui-rendering | **Keep; own topology** | Add a topology transaction API (`set_children`, `move_subtree`). Make `render_tree_mut` and `RenderNode` link mutators `pub(crate)`: flui-view has 26 raw sites. Use disjoint slab indexing, not whole-slab scans (#1041). Coalesce invalidation by epoch (#1042). Keep a persistent per-owner layout arena. Retain the root capture. Move catalog parent data, delegates and `ScrollPosition` up. Unify the build-during-layout cell trait here. |
| flui-objects | **Keep** | Receives catalog parent data/delegates. Harness becomes the exported conformance kit (`check_box::<T>`) instead of a string grep (render_object_harness.rs:151, 14440-14450). |
| flui-engine | **Keep, re-scope** | wgpu backend of `flui-raster`: `GpuContext` (one per process/adapter: device, queue, `wgpu::PipelineCache`, pipeline set prewarmed from a closed effect catalog, glyph atlas, image cache) + per-window `Presentation`. Picture cache keyed by `Arc<DisplayList>` pointer identity plus generation. Drop `pub use ::wgpu` (lib.rs:229) from the Stable surface. The raster protocol moves to flui-raster. |
| flui-view | **Keep, trim** | Drop the flui-objects/flui-animation deps: sliver adaptor, layout builder and persistent headers move to flui-widgets behind a public element-extension protocol. Owned child moves instead of `dyn_clone` (dispatch.rs:145, behavior.rs:1069-1073). Local topology commits (#1039). No `Arc<RwLock>` in `WidgetsBinding` or public `ElementBuildContext` (element_build_context.rs:50-53, 128-135). The binding/frame driver moves to flui-runtime. |
| flui-widgets | **Keep one crate** | Add the module-direction gate. Receives lazy-list/layout-builder elements and root scopes. Raw primitives (RawButton, Surface, ink) come down from Material. Image cache goes out to assets. `__private` becomes `pub(crate)`. |
| flui-testing | **Move above widgets + runtime** | Drives the *same* frame transaction as production, plus the `perf` harness (§6.3), semantic goldens and replay. Widgets drops its normal edge to it (widgets/Cargo.toml:89). |
| flui-material | **Official package** (`packages/`, same repo until independent cadence) | Depends on `flui` facade/SDK only, with no reach into objects/scheduler/rendering (material.rs:83, scaffold_messenger.rs:212). Owns its localizations. Out of the facade default. |
| flui-cupertino | **Official package** | Same as Material; built on the same Raw primitives. |
| flui-localizations | **Delete** (fold) | 281 lines, a layer of its own. RTL table goes to the widgets localization module, strings to each design package. ICU4X i18n becomes an H1 package. |
| flui-app | **Keep, shrink to composition** | Runners (desktop/android/ios/web) as thin `PlatformHost` adapters over flui-runtime, plus window/surface/device recovery. Target <15k lines (from about 40k). The thread-local `APP_RUNTIME` (runner/host.rs:46) becomes an explicit owner-executor object. |
| flui-cli | **Tool, independent version** | Add `flui mcp`, `flui devtools`, `flui test --golden`/`--perf`. Replace tools/web-server. No framework deps (already the case). |
| flui-devtools | **Merge into flui-protocol + runtime observer** | Zero production consumers today. Its profiler/timeline become protocol streams served by the realm's `RealmObserver`. |
| flui-hot-reload | **Delete after Subsecond spike** | dlopen design with documented UB risk; the plan chose Subsecond. The Subsecond hook becomes a feature of flui-runtime (`build` entry via `HotFn`, reassemble command) plus CLI. Until the spike lands, set `publish = false`. |

**Net:** 27 → 22 framework and package crates (−tree, −localizations, −devtools, −hot-reload; +platform-api, +text, +raster, +runtime, +protocol; a2ui arrives in H1). The publish train drops to 17 core units plus packages on their own version.

### 2.4 Module boundaries inside big crates

- **flui-widgets:** declared DAG `base(layout, paint, flex, stack) → interaction → {text, scroll, overlay} → navigator/router → app`. Enforce it with a syn-based `use crate::X` matrix in xtask. Cross-imports are almost zero today (navigator→overlay only), so locking them now is cheap.
- **flui-rendering:** `protocol/` (Stable), `pipeline/` (`#[doc(hidden)] unstable`), `storage/` (`pub(crate)`), `virtualization/`.
- **flui-engine:** `gpu_context/`, `presentation/`, `backend/` (Command IR replay), `effects/` (closed, enumerable catalog for prewarm). Move in-src readback suites (22k lines) under `src/tests/`.
- **flui-app:** `runner/{desktop,android,ios,web}` only. realm_dispatch.rs (7,149 lines) dissolves into flui-runtime.

### 2.5 Feature-flag policy

1. Features are additive, every optional dep sits behind `dep:`, and **no feature exists without a cfg site**. Gate it in `cargo xtask workspace`: flui-app has six dead features and flui-platform has four.
2. **Test seams are not features.** Remove the 57 `cfg(any(test, feature="testing"))` sites in flui-rendering production types (owner/mod.rs:251-257). Today dependents' tests compile a differently shaped `PipelineOwner` than release, so perf and correctness tests measure the wrong binary. Test hooks go through an installable hook registry.
3. **One `unstable` feature** on the facade replaces `runtime-internals` and friends. flui-app currently turns `runtime-internals` on in every production graph (flui-app/Cargo.toml:90). Internals are reached through `#[doc(hidden)] pub mod __runtime` in the tier crates.
4. Signals are **not a feature**; they are the default state layer (§3.5).
5. Platform backend choice is a target cfg, never a user feature. a11y adapters for Windows/macOS become unconditional target deps; AT-SPI stays behind `a11y-linux`, default on.
6. Dev-only `dynamic-linking` on the facade (Bevy's biggest iteration lever) for dev builds.

### 2.6 Facade and prelude

- `flui` re-exports **curated modules**, not crates. Today src/lib.rs:120-152 aliases whole crates, which is ~6k pub items. Curated modules: `prelude`, `view`, `widgets`, `rendering` (the Stable render-object authoring tier, extended with sliver authoring, `ViewportOffset`, `LayerLink`), `painting`, `interaction`, `runtime` (App/config/capabilities/spawner), `testing`.
- The prelude is an explicit enumerated list: no glob of `flui_view::prelude` (which leaks `BuildOwner`, `ElementTree` and the `tracing` macros, lib.rs:252, 278-285), and catalog-neutral (no Material).
- cargo-public-api snapshot on the facade in `checks` from H0. cargo-semver-checks runs advisory from H1 and gating from H3.

### 2.7 Publish order

With `cargo publish --workspace` (Rust 1.90), the train is:

geometry → types → foundation → macros → platform-api → log → painting → text → scheduler → interaction → assets → layer → semantics → animation → rendering → objects → raster → engine → view → widgets → runtime → testing → platform → app → flui

Packages go on their own caret-versioned train against the facade: material, cupertino, protocol, mcp, a2ui. `flui-cli` is independent. Every internal edge becomes `workspace = true` with the version in one place (172 hard-coded `=0.2.0-dev` pins today). Upward dev-deps (51) become version-less path deps.

---

## 3. Runtime model

### 3.1 Threads and lanes (decided topology)

```
 Owner thread (platform main)          Raster thread (per GpuContext)     IO pool     Compute pool
 ├─ Realm A ─ presentations ─┐         ├─ lower Scene (flui-raster)       decode       shaping of large
 ├─ Realm B ─ presentations ─┼─Scene──►├─ picture/command cache           fetch        paragraphs (opt)
 └─ frame clock per pres.    │ (Send)  ├─ glyph raster (own FontRaster)   file IO      user compute
    single demand authority  │         └─ Presenter: swapchain or OS
    reactive graph per realm │            compositor (partial present)
             ◄── acks / FrameStamp / present feedback ──┘
```

- **One owner thread per process, N realms on it.** This is honest about AppKit/UIKit/wasm. ADR-0027's "realms may execute concurrently" is not implemented (host.rs:25-47); amend the verdict. Per-realm owner threads on Win32/Linux become a later, measured option behind an `OwnerExecutor` trait, not a promise.
- **Raster lane is the default overlap mechanism**, not intra-realm parallel layout. It is threaded on Win32/Linux/Android and inline on macOS (wgpu-hal pin, #653) and wasm, with the same protocol either way (raster_lane.rs mailbox). Precondition: the text split in §3.6, because today glyph misses on the raster side take the same mutex as UI shaping.
- **IO and compute** are pools owned by flui-runtime and vended through `ctx.spawner()`, with cancel-on-unmount. Tokio is an implementation detail behind a feature. Deleted: the platform runtime (executor.rs:66) and the assets bridge runtime. That removes 2 of the 4 multi-thread tokio runtimes.

### 3.2 Frame transaction (one, in flui-runtime)

`drive(presentation)`: 
1. input apply
2. effects phase (ADR-0075, compute part)
3. build drain (depth heap)
4. layout, with **reentrant lazy child build inside sliver layout** (ADR-0017/0003 seam) instead of the between-pass fixpoint of up to 10/6 passes (layout_builder.rs:64-74)
5. compositing
6. paint into the **retained layer tree**
7. semantics (incremental)
8. `Scene` + `DamageRegion` to the lane
9. post-frame

Production (flui-app) and tests (flui-testing) call the same function with a different `Clock` and `Presenter`.

### 3.3 Trees and identity

- Element arena: generational `ElementId`, unchanged. Topology commits are local: record the affected render parents during reconcile and resequence only those in O(children). The global `synchronize_render_children` becomes a debug verifier.
- Render arena: disjoint borrows by sorted indices and `split_at_mut`. `SubtreeArena` is persistent per owner and uses a flag bit, not a `HashMap<RenderId, NodePtr>` per pass (subtree_arena.rs:161, 523-543).
- **Layer tree: retained.** One `LayerNode` per boundary/effect node, `LayerId = f(RenderId, slot)`, mutated in place on repaint. A clean boundary is an `Arc` subtree, so a graft is O(1), not a clone of the whole capture (ARCHITECTURE.md:584-586). This is the single decision that unlocks damage, engine caching, external content and devtools layer diffs.
- `LayerId`/`SemanticsId` become `GenId` (ABA-safe). Today they are reusable slab indices (flui-foundation id.rs:614-760), so caches keyed on them are unsound under reuse.

### 3.4 Damage and presentation

- Producer (flui-raster): diff consecutive retained trees. It compares `Arc::ptr_eq` on picture content, transform/opacity/clip deltas and structure changes, then emits `DamageRegion::Rects`, falling back to `Full`.
- Consumer: over plain wgpu there is no buffer age or present region (wgpu #682 closed as not planned). So the presenter renders into a **persistent retained target** and blits to the swapchain, which saves shading and command recording. **Hypothesis:** today's scissor straight into a rotating swapchain image (renderer.rs:2290-2330) would leave stale pixels once a producer exists. Verify before E1.
- `Presenter` trait (Subduction shape) = swapchain presenter (default) | OS compositor presenter (DirectComposition/CALayer/SurfaceControl, H1 spike → H2). The OS presenter is what gets true OS-level partial present and zero-copy video.

### 3.5 State and reactivity

- The reactive graph core (arena, push-pull Clean/Check/Dirty propagation, owner disposal, write journal) moves to flui-foundation. It stays **realm-owned** (a process-unique graph id, never a static).
- **Subscribers are phase-typed**: `Element` (rebuild), `RenderLayout(RenderId)`, `RenderPaint(RenderId)`. A signal-driven colour on a painter marks `needs_paint` directly. That answers the plan's "element vs render granularity" with "both, one graph", the way Compose does.
- The graph moves from the `BuildOwner` (per presentation, build_owner.rs:444, 722) to the **realm**. Today `SignalWrite` targets the primary presentation (commands.rs:450-455) and cross-window reads fail with `ForeignGraph`.
- Fan-out is batched per write, with one registry for signal readers and inherited dependents (#1249, #1254). This removes "pick Signal if <50 readers" from user guidance (ADR-0074 §8.1).
- `Listenable`/`ChangeNotifier` become thin adapters over the graph. There are no longer two primitives with an `Arc<Mutex<HashMap>>` on the paint path.

### 3.6 Text

`FontCollection` (shared, immutable after load, host scan **async/lazy** with bundled faces available immediately) → per-realm `LayoutContext` (no lock) → `Arc<TextLayout>` into the DisplayList → the raster side rasterises glyphs through its own `GlyphRasterizer` + atlas owned by `GpuContext`. Glyph keys carry font-blob identity, not process-lifetime cosmic keys (glyphs.rs:23). This is written into ADR-0077's acceptance criteria. The same `ShapedRun`/rasteriser contract serves the CPU golden backend, so glyphs are identical by construction.

### 3.7 GPU

`GpuContext` is created once (instance → adapter → device), and surfaces are created from the same instance (the AGENTS rule). It holds:
- a `wgpu::PipelineCache` (none today);
- a closed effect catalog prewarmed on a warm-up task;
- a glyph atlas and image atlas shared across windows.

Today each window builds its own Instance/Device/pipelines/atlas (renderer.rs:1140-1168; per runner), and `SharedRealm` with content is refused (secondary_window.rs:741). Device loss is recovered per `GpuContext`.

### 3.8 Async

The `Spawner` capability (compute/io) supports `!Send` local futures on the owner lane by default and `Send` for pools. Results return as commands applied in the next frame's input phase. Cancellation is tied to element unmount.

### 3.9 Platform and a11y

One backend per OS, recorded in an ADR: Win32 (with TSF + UIA TextPattern; there is no IME path today, a grep for WM_IME finds nothing), AppKit, UIKit, Android, web, and winit on Linux (renamed from "fallback"). The lifecycle state machine and event stream live in flui-platform-api, so runners are thin. Windows are owner-owned `!Send`, with a `Send` handle proxy that has closed verbs (redraw, close, …). That removes most of the 26 `unsafe impl Send/Sync` and fixes the #949 cross-thread `request_redraw`. a11y adapters are on by default on Windows/macOS.

---

## 4. Extension points (H1–H4), performance-safe by construction

Each extension point must participate in identity, damage and budgets. Otherwise one plugin reintroduces full repaints.

| Extension | Seam | Perf rule |
|---|---|---|
| `PlatformCapability` (H1) | `flui-platform-api::PlatformCapability`, realm `CapabilityRegistry`, `LifecycleContext::capability::<C>()` (one generic method keeps the sealed trait closed while the set stays open; supersedes ADR-0078's "one method per capability") | Capabilities run on the owner lane or return futures through `Spawner`. No capability may block the frame. |
| Third-party render objects (H0) | `flui::rendering` Stable tier + `conformance::check_box/check_sliver` | Conformance includes perf invariants: relayout idempotence, `paint` recording counts, no paint outside the declared bounds, a stable `is_repaint_boundary`. |
| Third-party elements (lazy lists, A2UI hosts) | Public element-extension protocol (currently closed: ElementOwner fields pub(crate), element_owner.rs:115-150) | Lazy children use the in-layout build seam, never an extra pass. |
| External GPU content (H1 spike / H2) | `Layer::External { id }` + `ExternalContentRegistry` on `GpuContext`, handle vended as a capability; `Texture` widget | Updates mark damage for exactly the content's rect. Frame sync through the lane mailbox (fence per update). Compositor presenter for video/WebView. |
| Custom shaders (E3) | `DrawOp::Custom(ProgramId)` registered at app build with WGSL + uniforms + optional CPU impl | Registered programs join the prewarm set. No per-frame pipeline creation. |
| Themes as data (H1) | serde token maps at the widgets tier, resolved through InheritedView + FieldMask; `ThemeData::from_tokens` | Field masks keep theme edits O(readers of that field). `Theme::of` stops cloning whole ThemeData (29 sites). |
| Agent/devtools protocol (H0→H3) | `flui-protocol` + realm `RealmObserver`; `flui mcp` over stdio; loopback/pipe + per-launch token; compiled out of release | Streams per-frame counters and traces. Observation reads retained semantics, with no extra tree walk when unobserved. |
| A2UI (H1) | `flui-a2ui` package: catalog from macros metadata, JSON-pointer projection of realm signals, transport-adapter trait | Data-model updates are per-path Store triggers, so an update touches only bound widgets. |

---

## 5. Public API and DX sketches

```rust
use flui::prelude::*;

// Entry point: any view kind, one builder on every target.
fn main() -> flui::Result<()> {
    App::new(Notes)
        .capability::<flui_clipboard::Clipboard>()     // H1 plugin, typed
        .window(WindowConfig::titled("Notes"))
        .run()
}

// Signals default-on; Copy handles; the write resolves the realm through the handle.
#[derive(StatefulView)]
struct Counter;
struct CounterState { n: Signal<u32> }

impl ViewState<Counter> for CounterState {
    fn init_state(ctx: &dyn LifecycleContext) -> Self { Self { n: ctx.signal(0) } }
    fn build(&self, _: &Counter, cx: &dyn BuildContext) -> impl IntoView {
        let n = self.n;                                         // Copy, no clone
        Column::new((
            Text::new(format!("{}", n.get(cx))),
            RawButton::new("+").on_press(move |w| n.update(w, |v| *v += 1)),
        ))
    }
}

// Paint-phase binding: an animated value repaints without a rebuild.
#[derive(RenderView)]
struct Glow { #[render(paint)] intensity: Reactive<f32> }   // subscribes RenderPaint

// Background work: cancelled on unmount, result lands next frame.
fn init_state(ctx: &dyn LifecycleContext) -> Self {
    let img = ctx.signal(None);
    ctx.spawner().io(async move { decode("hero.png").await })
       .then(move |w, bytes| img.set(w, Some(bytes)));
    Self { img }
}

// Platform capability: typed, Unsupported instead of panic.
let clip = ctx.capability::<Clipboard>()?;          // Result<Rc<Clipboard>, Unsupported>

// External GPU content.
let tex = ctx.capability::<ExternalContent>()?.register(TextureDesc { .. });
Texture::new(tex.id())                                // emits Layer::External, damage-scoped

// Perf test: counts are deterministic and gate PRs.
#[flui::test]
fn one_row_change_is_local(t: &mut Tester) {
    t.mount(List10k::new());
    t.pump();
    t.write(|w| ROW_5.set(w, "x"));
    let f = t.pump_counted();
    assert!(f.elements_built <= 3 && f.layout_nodes <= 8);
    assert!(f.damage_area() <= t.bounds_of(by_key("row-5")).area());
}
```

DX commitments:
- one `App` entry point (today there are eight: run_app/run_app_with_config/run_direct/Application/…);
- `View for Option<V>` and `Either`, so branches don't box (39 `.boxed()` in examples);
- widget lengths as `impl Into<Pixels>`, one rule (132 f32 vs 4 Pixels signatures today);
- UI callbacks never `Send + Sync` (42 hits today contradict ADR-0027:106);
- `CustomPainter: Any`, dropping the required `as_any`.

---

## 6. Performance model

### 6.1 Invariants and today's violations

| Invariant (O(changed)) | Current violation | Fix (§ ref) |
|---|---|---|
| Rebuild cost ∝ dirty elements × their config size | Deep `dyn_clone` of configs per level (into_view.rs:178-184; behavior.rs:1069-1073; dispatch.rs:145) | Owned child moves / `Rc` configs (§2.3 view) |
| Topology commit ∝ changed parents' children | Global forest + arena pass per insert, quadratic `remove_child` (element_tree.rs:1107-1114, 1428-1520) | Local commits in rendering's topology API |
| Layout ∝ nodes relaid out | Whole-slab scan per dirty root (tree.rs:82-110); HashMap arena per pass × up to 10 passes; non-coalesced marks (scheduler.rs:191-261) | Disjoint indexing, persistent arena, epoch coalescing, in-layout lazy build |
| Paint ∝ dirty boundaries | Root never retained; inline content re-recorded; graft clones capture (ARCHITECTURE.md:287-293) | Retain root; `Arc` subtree graft |
| Raster ∝ damage | Always `Full` (raster_lane.rs:354, 486) | Diff producer + retained target |
| Idle = 0 frames | Five demand sources; loop-wide `needs_redraw` shared across realms (runtime.rs:727; #1172) | Single `DemandMask` authority |
| Event dispatch lock-free | Arena/recognizer Mutex/DashMap (arena/mod.rs 17 sites) | Rc/RefCell |
| No per-node locks in layout | ChildManager `Arc<Mutex<…>>` registry (child_manager.rs:56); constraint/header cells Mutex; ScrollPosition Mutex in viewport layout (viewport.rs:1057) | `!Send` flip |
| Steady-state frame allocs ≈ 0 | Fresh hash sets + LayerTree slab per paint (paint.rs:86-189) | Scratch buffers on owner; counting-allocator test |
| Glyph miss doesn't touch UI | Shared FONT_SYSTEM mutex (layout.rs:124; painter/mod.rs:167) | §3.6 |

### 6.2 Budgets (H0 gates → H2 exit)

| Scenario | Gated counter (per PR, virtual clock) | Trend (nightly, 3 OS) |
|---|---|---|
| Idle 10 s | frames produced = 0 per presentation | wakeups/s |
| Static 10k list scroll 1 screen | layout nodes ≤ visible + cache band; topology ops ∝ new rows; layout passes = 1 | p99 frame < panel period |
| Fling 100k list | rows built per frame ≤ band delta; passes = 1 | p99, max |
| One text change | elements built ≤ 3; damage area ≤ text box + AA margin | GPU time |
| Route push | new layers only for the route | first-frame time |
| Steady animation (opacity) | zero rebuild, zero relayout, 1 layer patch | — |
| Cold start → first present | phase counts; font scan off the critical path | < 300 ms (H2 exit), split across font/adapter/pipelines/first build |
| 1 MB editor keystroke | shaped runs touched ∝ edited paragraph | p99 |

### 6.3 Tooling

`cargo xtask perf` runs the scenarios above on flui-testing's `pump_counted()` (the same frame transaction as production) and fails on counter regressions. `bench-collect` enables required features instead of skipping them (bench.rs:36), so the `damage_scissor` baseline finally runs. A comparison harness against a mirrored Flutter suite is an explicit H2 deliverable; it does not exist today.

### 6.4 Compile-time performance

Target rebuild fan-out:
- a Win32 backend edit rebuilds flui-platform + app + facade only (16 today);
- a shaping-engine edit rebuilds text + consumers, not layer/engine-core;
- a flui-types edit drops from 24 as single-owner modules move out.

Measure with `cargo build --timings` before and after each cut (principle 5). **Hypothesis to measure:** per-crate `testing` features compile the upper stack several times per `cargo xtask test`. Count distinct `libflui_rendering-*.rlib` hashes.

---

## 7. Safety model (as it serves performance)

- **Globals ratchet** (`cargo xtask globals`, in `checks`): allowlist with reason and horizon. Fails on a new `static` holding Mutex/RefCell/Lazy/OnceLock or on `thread_local!` outside flui-platform and flui-app runners. Burn-down targets are FONT_SYSTEM, decode CACHE, ERROR_VIEW_BUILDER, TIME_DILATION, AssetRegistry::global, and the navigator/lane/GlobalKey TLS.
- **Frame-path lock ban:** `disallowed_types` for `parking_lot::{Mutex,RwLock}`, `std::sync::Mutex` and `DashMap` in foundation/scheduler/interaction/layer/rendering/objects/view/widgets/runtime, with a named allowlist for lane mailboxes.
- **`!Send` flip completed in one ADR before H3:** drop `Send + Sync` supertraits from Listenable, Animation, TickerProvider, CustomPainter, all layout delegates, ScrollPhysics, HitTestTarget, ViewKey and BuildDuringLayoutCell (flui-foundation/src/notifier.rs:78; flui-animation/src/animation.rs:68; …), and from `RenderView::RenderObject` (flui-view/src/view/render.rs:451). Removing supertraits after 1.0 is a breaking change, so this cannot wait.
- **Fault isolation everywhere third-party code runs:** one `guarded_call(node, phase, f)` for layout/paint/hit-test/intrinsics/semantics. Today hit-test, query intrinsics and semantics are unguarded (accessors.rs:710; query.rs:491; semantics.rs:941). A repeatedly poisoned paint node gets an error-box picture instead of stalling the frame.
- **unsafe:** the `subtree_arena.rs` island stays Miri-covered and becomes simpler with the persistent arena. Keep a per-backend unsafe ledger ratchet, and turn on `undocumented_unsafe_blocks` per backend module as annotation lands. ~309 platform sites never execute in CI, so add a scheduled macOS runner.

---

## 8. Delete, merge, or replace with ecosystem crates

| Area | Action |
|---|---|
| cosmic-text 0.19 + global FONT_SYSTEM | Replace with Parley/fontique/HarfRust/ICU4X in `flui-text`. Evaluate glifo for atlas rasterisation in the ADR-0077 spike. Drop separate `unicode-segmentation` (painting/widgets) for ICU4X from Parley. |
| CPU reference and software fallback | vello_cpu (authors call it the mature choice) or tiny-skia, as a `flui-raster` backend. Pin the SIMD level in golden mode (hypothesis: SIMD dispatch breaks cross-CPU determinism otherwise). |
| lyon | Keep. Vello GPU (sparse strips) is evaluated later behind the same contract, not as a rewrite. |
| Frame pacing and demand | Adopt frameclock's demand classes (INPUT/CONTINUOUS_INPUT/ANIMATION/BACKGROUND) and present feedback. Take the design or the no_std crate. |
| dashmap, bon, lru (widgets image cache), moka duplication | Remove dashmap (Rc/RefCell) and bon. One image cache in flui-assets. |
| tokio in platform/assets | Remove. Tokio only in runtime's default executor behind a feature. |
| winit on Win/macOS default graph | Remove (the `desktop` feature has no code). |
| tools/web-server, dlopen hot-reload, flui-devtools, flui-localizations, flui-tree | Delete or fold as in §2.3. |
| `deny.toml` `multiple-versions = "allow"` (67 dup names) | Switch to `warn` with a reasoned skip list and a ratchet. |

---

## 9. Breaking changes to make now (pre-1.0, ordered by how hard they get later)

1. **Retained layer identity + `DamageRegion::Rects`** in the lane protocol. Changes what `paint` produces, so it must land before the render protocol freezes (supersede ADR-0061's "until identity exists").
2. **The `!Send` flip** of UI-side traits and `RenderView::RenderObject`. Callbacks become `Rc`, notifier `Rc`.
3. **Platform contract split** (`flui-platform-api`) + redesign of IME as a text-store client (TSF, NSTextInputClient, InputConnection, UITextInput). The push-only 2-method trait (traits/text_input.rs:24-45) cannot serve mobile H1.
4. **Reactive graph to the realm + phase-typed subscribers + signals default-on.** The signals API surface freezes at H3, and Solid needed a major version to fix effect scheduling.
5. **Extract `flui-runtime`** and delete the parallel headless frame transaction. Supersedes ADR-0041's gate.
6. **Text split** (`flui-text`) with glyph keys carrying font identity. Remove the `fontdb::Family` re-export.
7. **Facade curation and a catalog-neutral prelude.** Material out of the default. `unstable` replaces `runtime-internals`.
8. **`GpuContext`/`Presentation` split** of the engine. No `pub use ::wgpu` on Stable.
9. **`Pixels` Eq/Hash consistency**, `GenId` for layer/semantics IDs, removal of duplicate types (physics, `BoxConstraints`).
10. **Public element extension protocol**; lazy/layout-builder elements move to widgets; flui-view drops the objects/animation deps.
11. **One `App` entry point** that accepts any view kind.

---

## 10. Evolution H0 → H4 without rewrites

| Horizon | Architecture work (additive on the target) | Proof |
|---|---|---|
| **H0** | Cuts 3/5/6/7/9 from §9. The `perf` harness gates counts. Damage producer over a retained target on the swapswapchain presenter. CPU backend in flui-raster for goldens. `flui-protocol` + `flui mcp`. Globals and lock gates. | Notes from crates.io; idle 0 fps; one-text-change damage gated; agent scenario via `flui mcp` |
| **H1** | Capability registry + 5 external plugins against `flui-platform-api`. Mobile runners as thin `PlatformHost`s. OS compositor presenter spike (`Presenter` impl #2). `Layer::External`. Themes-as-data tokens. `flui-a2ui` on Store triggers. | Plugins built outside the repo; video layer with rect-scoped damage |
| **H2** | Threaded raster lane (Win/Linux/Android), picture/raster cache for static boundaries, prewarmed pipeline catalog + PipelineCache, async font scan, 100k fling with one layout pass, OS partial present where the presenter owns the swapchain, software fallback enabled at runtime on adapter failure | Bench suite vs Flutter on 3 OS; cold start < 300 ms; decision "no intra-realm parallel layout" re-validated on 100k |
| **H3** | Freeze the tiers: `flui::rendering`, `flui::view`, prelude, Router, signals, protocol schema (Evolving). Semver-checks on the facade + platform-api + protocol. | 3 minors green |
| **H4** | Community render/element crates use the conformance kit; community backends through the #560 minting seam; Vello GPU as an optional `flui-raster` backend; embedding via a host-owned `OwnerExecutor` | 10 community crates |

Each horizon adds an implementation behind an existing seam (Presenter, raster backend, capability, Spawner, OwnerExecutor) rather than changing a contract.

---

## 11. Risks and deliberate non-goals

**Risks**
- *Retained layer tree complexity.* In-place mutation plus identity is where Flutter's DiffContext and the gpui-fast work spent their effort. Mitigation: keep the tree append-only per boundary subtree (an `Arc` subtree replaced whole on repaint), so the diff is pointer equality at boundary granularity first and finer later.
- *Raster-lane threading on macOS* is pinned inline upstream (#653). The protocol must stay mode-agnostic forever.
- *Parley rasterisation* (skrifa + rasteriser or glifo) is unproven in the ADR-0067 atlas. The spike exit must include "no process global, raster-side rasteriser".
- *Five new crates* raise the publish-unit count briefly. Offset by four deletions, and new crates are admitted only where a reach fact or fan-out measurement justifies them.
- *A bus-factor-1 project doing §9 in parallel with the H0 features.* Sequence it: 5 (runtime) and 3 (platform-api) first, because they unblock tests and plugins; then 1 (identity/damage); then 2/4.
- **Hypotheses not measured here:** swapchain stale-pixel risk under scissor; multi-copy upper-stack compilation from `testing` features; the size of the host font scan in cold start; extra frames from the loop-wide redraw flag; per-level clone magnitude.

**Non-goals**
- Parallel layout inside a realm (decided no; revisit only with H2 100k evidence).
- Per-realm owner threads before H2 measurements.
- A second GPU abstraction or per-OS native renderers. wgpu stays the GPU path; the CPU backend is the only other rasteriser.
- Cross-OS pixel parity in production. Pixel goldens run on the CPU reference; production uses OS hinting and semantic goldens.
- Splitting flui-widgets into feature crates.
- An in-app MCP server in release builds.
- Separate repositories for official packages before they have an independent release cadence. `packages/` in the monorepo with its own version train comes first, as Flutter's 2024 monorepo re-merge and Slint's in-repo Material suggest.
