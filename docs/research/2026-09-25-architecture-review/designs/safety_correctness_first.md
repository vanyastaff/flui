# FLUI target global architecture: the safety and correctness view

*Written against main @ cab06137d, 2026-09-25. Every code claim cites path:line from the codebase maps. Market claims cite the survey sources. Anything marked **(hypothesis)** was not measured.*

## 0. Verdict

The inner mechanisms of FLUI are mostly sound. The typestate pipeline, generational `RenderId`/`ElementId`, the `!Send` realm, `LifecycleContext` capability splitting, per-node panic poisoning and zero production `.unwrap()` all hold up. The problem is at the boundaries, which is also where a safety-first architecture has to be enforced. Five of the owner's seven principles depend on review alone today:

- **Principle 3 (no global state)** has no gate. The ambient-reach ratchet file `runtime-contract.toml` cited at roadmap.md:224/743 is not in the tree. Meanwhile FONT_SYSTEM, the decode CACHE, ERROR_VIEW_BUILDER, TIME_DILATION, AssetRegistry::global and the APP_RUNTIME/NAVIGATOR/LANES/REGISTRY_STACK thread-locals are all live.
- **ADR-0027 (`!Send` UI)** covers the core trees only. Listenable, Animation, TickerProvider, CustomPainter, the delegates, ScrollPhysics, ViewKey and BuildDuringLayoutCell still require `Send + Sync`. The result is `Arc<Mutex>` in controllers, locks on the layout path (child_manager.rs:56, layout_constraints_cell.rs:96), and `unsafe impl Send` workarounds (object_key.rs:47-50).
- **The panic policy** is enforced for `unwrap` but not for the `BUG:` convention (about 255 `expect` calls without it, plus 51 non-literal ones).
- **Unsafe code** is concentrated in about 309 flui-platform sites that CI never executes. `undocumented_unsafe_blocks = "allow"` (Cargo.toml:398).
- **Behavioural contracts** have gaps. Tests reach production through a second frame transaction (`HeadlessBinding::pump_frame`, flui-testing/src/lib.rs:955-1079) and a second BuildContext (`ElementBuildContext`, element_build_context.rs:39). In the renderer, a second raster walker already diverges on three layer kinds (flui-engine/src/headless.rs:15-27).

The target architecture below turns each of these into a type, a lint, a manifest fact or a single implementation. The changes are cheapest now, before crates.io publication and the H3 freeze.

---

## 1. Guiding principles

### Reconciled with the owner's seven

| Owner principle | Keep / amend | Safety-first reading |
|---|---|---|
| P1 Mental model is sacred | Keep | Pin the model with conformance suites (render-object, element-lifecycle), not with doc prose. |
| P2 One language, one toolchain, one renderer | **Amend** | Change it to "one **raster contract**, one production rasteriser per platform". A CPU reference backend (E7) and a software fallback (H2) are required. That is only safe if every backend implements one contract and passes one conformance suite. Today flui-engine/ARCHITECTURE.md:8-14 forbids exactly that, and headless.rs already diverges. Python/Swift device checks (tools/device-checks) break "one toolchain". Port them to Rust. |
| P3 No global state | Keep, **add a gate** | "Ambient reach = 0" must be a `cargo xtask globals` check with a reasoned allowlist that can only shrink. |
| P4 Everything machine-readable | Keep, **add security** | A devtools/agent port is remote control of a real app. It is compiled out of release builds (a type or feature, not an env var) and uses a pipe or loopback with a per-launch token. MCP SDK DNS-rebinding advisories and CVE-2025-49596 show the failure mode. |
| P5 Proof, not claims | Keep | Evidence is a structured record written by `xtask device` (platform, command, commit, result), not prose in BETA.md. |
| P6 Break explicitly | Keep | Every item in section 9 gets an ADR with `Supersedes`. |
| P7 Market first | Keep | |

### Added safety principles

- **S1. One implementation per contract.** One frame transaction, one BuildContext, one raster lowering, one reactive graph, one agent-protocol schema. Tests drive production code paths, never re-implementations.
- **S2. Thread affinity is a type.** Everything realm-owned is `!Send` and lock-free. `Send` exists only on the values ADR-0027 §2 lists: `Scene`, lane mailboxes, `UiCommandSender`, IO results, capability *senders*. A `pub` signature that names `Mutex` or `RwLock` is a lint failure.
- **S3. Unsafe lives in named islands with budgets.** It is allowed in flui-platform backends, the `subtree_arena` layout island (Miri-covered, tasks.rs:474-480) and hot-reload/FFI glue. Each island has a counted budget, `undocumented_unsafe_blocks = warn`, and either a Miri target or a live-run proof.
- **S4. Reach is a manifest fact.** Headless crates declare what they must never link (`forbid-reach = ["winit","tokio","windows","objc2","wgpu"]`), and `cargo xtask workspace` checks it against `cargo tree`.
- **S5. Public surface is a reviewed diff.** A public-API snapshot and semver-checks run on the Stable surface from now on, not from H3.
- **S6. Determinism is a property of the runtime, not of test hygiene.** No process-global font, ID or time state reaches any output that tests, replay or the agent protocol observe.

Where I disagree with decisions already taken, the evidence is:

1. **ADR-0078's "one method per capability"** has to be superseded. `LifecycleContext` is sealed (build_context.rs:106), so plugins built outside the repo (the H1 exit) cannot exist under it.
2. **ADR-0041's `flui-runtime` gate ("two entry points")** has to be superseded. flui-testing is already the second entry point; it just re-implements the transaction.
3. **ADR-0074's placement** is wrong. The graph is per `BuildOwner`, so per presentation (build_owner.rs:444,722). `SignalWrite` targets the primary window (ui_realm/commands.rs:450-455). The graph belongs to the realm, with the core below rendering.
4. **I keep "flui-widgets is one crate"**, on the condition that its import-direction gate exists. It does not today (tools/xtask/src/tasks/checks.rs:100-120).

---

## 2. Workspace and crate topology

### 2.1 Tiers, replacing 11 layer numbers

Six named tiers, with reach rules declared in each manifest. Same-tier edges stay legal, but they have an explicit intra-tier order in `[workspace.metadata.flui]`, so that the interaction → platform edge cannot hide inside "L2" again.

| Tier | Purpose | Reach rule (S4) |
|---|---|---|
| **T0 Values** | geometry, colours, ids, keys, diagnostics vocabulary, macros | std + serde/glam/smallvec only |
| **T1 Contracts** | capability traits, input/IME/a11y vocabulary, reactive core, protocol types | no OS crates, no tokio, no wgpu |
| **T2 Substrate** | painting/text, scheduler, interaction, semantics, layer/raster contract, animation, assets | same as T1 |
| **T3 Render machine** | protocol, catalog, wgpu backend, CPU backend | only backends may link wgpu or a CPU rasteriser |
| **T4 Spine and runtime core** | view/elements, realm/frame transaction | headless; no winit/tokio/windows/objc2/wgpu |
| **T5 Catalog and test support** | widgets (raw primitives, Router), testing | headless |
| **T6 Composition** | platform backends, app runners, facade, CLI | anything |
| **Packages** | material, cupertino, devtools, mcp, hot-reload, a2ui, i18n | depend on `flui` (sdk surface) only |

### 2.2 Target crate list

**Core (on the release train, semver):**

| Crate | Tier | Responsibility | Public surface |
|---|---|---|---|
| `flui-geometry` | T0 | Unit-typed 2D math: one `Rect`, one `Axis`, Point/Offset | Stable |
| `flui-types` | T0 | Values shared by ≥2 tiers: Color (f32 + colour space), TextStyle, spans, layout enums, Path/Paint | Stable (trimmed) |
| `flui-foundation` | T0 | Generational `GenId<M>` for all ids, keys (value identity only), diagnostics field vocabulary, `observe`, arity/slot/depth (absorbed from flui-tree) | Stable; `runtime` module `#[doc(hidden)]` |
| `flui-macros` | T0 | View/Render/Inherited/Store/Catalog derives | Stable |
| **`flui-platform-api`** (new) | T1 | Capability traits + `Unsupported`, `PlatformCapability` registry types, owner capability + backend minting seam (#560), IME document protocol, `PlatformAccessibility`, ui-events re-export, lifecycle state machine | Stable for extension authors |
| **`flui-reactive`** (new) | T1 | Realm-owned signal graph core: push-pull Clean/Check/Dirty, typed subscriber kinds (element rebuild, layout, paint), write journal, Store paths | Stable (signals) |
| **`flui-protocol`** (new) | T1 | ADR-0080 wire types, error codes, AccessKit role mapping (via `accesskit::Role`, not a copy), outline serializer, JSON Schemas | Evolving |
| `flui-painting` | T2 | DisplayList recorder + text *service* (Parley, per-realm `FontContext` over a shared fontique `Collection`) | Stable (Canvas), Evolving (text) |
| `flui-scheduler` | T2 | Owner-affine `!Send` phase core + `Send` `SchedulerWaker`; the effects phase | Evolving |
| `flui-interaction` | T2 | Gestures, focus, key dispatch, hit-test values; depends on platform-api only | Stable (Focus/gesture authoring) |
| `flui-semantics` | T2 | Semantics model + AccessKit translation | Stable |
| `flui-layer` | T2 | Layer vocabulary, Scene, `DamageRegion::{Full, Partial}`, **raster contract** (layer walk, `CommandRenderer`, effect decomposition, damage diff), `Layer::External(ExternalContentId)` | Evolving |
| `flui-animation` | T2 | Curves, tweens, simulations (sole owner), `!Send` controller | Stable |
| `flui-assets` | T2 | Runtime-agnostic loaders, decoded-image cache as a realm resource, injected IO spawner | Evolving |
| `flui-rendering` | T3 | RenderBox/RenderSliver protocol, pipeline, topology transaction API, build-during-layout cell, virtualizer, conformance kit | Stable (authoring tier); pipeline `#[doc(hidden)]` |
| `flui-objects` | T3 | Concrete render catalog + catalog parent data/delegates, `ScrollPosition` | Evolving |
| `flui-engine` | T3 | wgpu backend of the raster contract, `GpuContext` (shared device) + per-window `Presentation` | minimal; wgpu interop behind `unstable-wgpu-interop` |
| **`flui-raster-cpu`** (new) | T3 | CPU backend of the raster contract (vello_cpu or tiny-skia), pinned SIMD in test mode | Evolving |
| `flui-view` | T4 | Authoring traits, elements, reconciliation, public element protocol | Stable authoring; `__runtime` hidden |
| **`flui-runtime`** (new, extracted from flui-app) | T4 | `Realm`, `Presentation`, the single frame transaction, lanes, execution services, capability host, `RealmObserver` | Evolving |
| `flui-widgets` | T5 | Raw primitives, Router, text editing, scroll, overlay; module DAG gate | Stable (base catalog) |
| `flui-testing` | T5 (above widgets) | Tester with semantic finders over `flui-protocol` queries, virtual clock, golden (CPU backend), replay | Evolving |
| `flui-platform` | T6 | OS backends only; consumed by flui-app | internal |
| `flui-app` | T6 | Runners (`PlatformHost` adapters), window/surface/device recovery | internal + `App` builder |
| `flui` (facade) | T6 | Curated modules, tiered, catalog-neutral prelude, `sdk` module for package authors | Stable |
| `flui-cli` | T6 | Independently versioned binary | CLI contract (typed NDJSON) |

**Official packages** (in-repo until each has its own cadence, then separate repos; depend on `flui` only): `flui-material`, `flui-cupertino`, `flui-devtools` (in-process protocol server), `flui-mcp` (promoted from tools/desktop-mcp; OS driver library + stdio MCP), `flui-hot-reload` (Subsecond, rewritten), and in H1 `flui-a2ui` and `flui-i18n`.

**Tools** (`publish = false`, no tier): `xtask`, `live-smoke` (becomes a scenario runner over the flui-mcp driver library), `decoy-face`. Delete `web-server` and, after the spike, `text-spike`. Port `device-checks` to Rust.

### 2.3 Fate of all 27 current crates

| Crate | Fate | Reason (evidence) |
|---|---|---|
| flui-geometry | **Keep, trim ~3.5k lines** | Unused GPUI vocabulary (length.rs, transform2d.rs, bezier.rs, kurbo bridge with a never-enabled feature). Fix the Pixels Eq/Hash/Ord inconsistency (units.rs:91,575-596). |
| flui-types | **Keep, trim** | Delete physics (duplicate of flui-animation/simulation.rs:31-950), `BoxConstraints` (duplicate of rendering), `MaterialColors`. Rule: an item needs ≥2 consuming crates. |
| flui-foundation | **Keep; absorb flui-tree's Arity/Slot/Depth** | Move `OwnerAffinity`/`ClaimSlot` back to the platform crates once platform tests run on the headless path (affinity.rs:11-13 names a CI gap as the reason they live here). Unify ids on `GenId`. |
| flui-macros | **Keep, extend** | Add `RenderView`, `Store` and `Catalog` derives; document the proc-macro semantic edge. |
| flui-tree | **Merge into foundation + rendering** | The trait trio has no generic consumer (tree_traits impls only), `ElementTree` does not implement it, and `bon` is used for one builder. |
| flui-platform | **Split**: contract → `flui-platform-api` (T1); backends stay (T6) | One trait import (interaction/text_input.rs:27) links winit/tokio/windows into rendering and everything above (`cargo tree -p flui-rendering -i winit`). |
| flui-scheduler | **Keep; make `!Send` core + waker; drop AsyncDriver's Send bound** | About 20 Mutex fields for single-writer state (scheduler.rs:743-855); global TIME_DILATION (config.rs:43). |
| flui-painting | **Keep; the text service becomes realm-owned** | Remove FONT_SYSTEM (layout.rs:124) and wrap the `fontdb::Family` re-export (lib.rs:87). |
| flui-interaction | **Keep; depend on platform-api** | Move `InteractionLane` TLS resolution into explicit context handles owned by the runtime. |
| flui-assets | **Keep; runtime-agnostic** | Delete `global()` (registry/mod.rs:83) and the owned tokio runtime (bridge.rs:42-66); absorb the widgets decode cache. |
| flui-log | **Keep** | Clean composition-only boundary. |
| flui-layer | **Keep; absorb the raster contract** | The GPU-free modules (layer_walk, layer_render, dispatch, command_renderer, damage) contain zero wgpu references. |
| flui-semantics | **Keep** | Extend `SemanticsAction` to cover AccessKit's action set 1:1. Owner-lane action targets replace `Send + Sync` handlers (action.rs:217). |
| flui-animation | **Keep; `!Send` controller** | Arc<Mutex> "scoped exception" (controller.rs:177-208). |
| flui-rendering | **Keep; tighten** | Make topology mutators `pub(crate)` (node.rs:297-343), add a topology transaction API, move catalog parent data/`ScrollPosition` out, add the conformance kit. |
| flui-objects | **Keep** | Owns catalog specifics; the build-during-layout cells move down into rendering. |
| flui-engine | **Keep; shrink to the wgpu backend** | The lowering core moves to flui-layer. `pub use ::wgpu` (lib.rs:229) goes behind a feature. `raster_owner` moves to flui-runtime. |
| flui-view | **Keep; open the element protocol, move catalog elements out** | Drop the view → objects/animation edges (sliver_adaptor.rs:58). Delete `ElementBuildContext`. Move the signal graph out. |
| flui-widgets | **Keep one crate + module DAG gate** | `__private` becomes `pub(crate)`. Root-scope widgets that runtime needs move to flui-view/runtime. |
| flui-testing | **Move above widgets** | widgets → testing normal edge (widgets/Cargo.toml:89) forced a 2.1k-line harness into widgets. |
| flui-hot-reload | **Rewrite on Subsecond; `publish = false` until then** | dlopen ABI-token design with documented residual UB risk (lib.rs header). |
| flui-material | **Package; depends on `flui` sdk only** | Currently reaches objects/scheduler/rendering (material.rs:83, scaffold_messenger.rs:212). |
| flui-cupertino | **Package; same** | Needs raw primitives; today it lacks focus/activation (button.rs:34-43). |
| flui-localizations | **Delete** | 281 lines; the RTL table moves to the widgets localization module; catalog l10n moves into each design package; ICU4X becomes `flui-i18n` in H1. |
| flui-app | **Split**: realm, frame and execution go to `flui-runtime`; runners stay | 44.5k lines. The god root blocks embedders and testing (ADR-0027 itself rejects a "god runtime object"). |
| flui-cli | **Keep; version independently** | No framework dependency; the typed NDJSON schema (ui.rs:180) is its contract. |
| flui-devtools | **Repurpose as the protocol server package** | Zero production consumers; "opens no port" (lib.rs:18-24). |

Net result: 27 → 26 core-plus-package crates. Four are added (platform-api, reactive, protocol, runtime, raster-cpu: four core, one promoted from tools); flui-tree and flui-localizations are removed; five become packages. The count of **semver-promised** surfaces drops to about 8: facade, platform-api, rendering authoring, view authoring, reactive, geometry, types, protocol. Everything else is documented as internal, following Slint's split between `internal/` and `api/` crates.

### 2.4 Module boundaries inside big crates (compiler-checked where possible)

- **flui-widgets:** a DAG `base (layout, paint, flex) → interaction → text | scroll | overlay → navigator/router → app`. Enforce it first with `pub(in crate::…)` visibility (the compiler checks it) and back it with an xtask `use crate::X` matrix in `checks`. It is cheapest to lock now, while cross-module imports amount to about one (navigator → overlay).
- **flui-rendering:** `protocol` (Stable), `pipeline` and `storage` (`#[doc(hidden)]`, used only through the runtime).
- **flui-view:** an `authoring` module re-exported by the facade and a `#[doc(hidden)] pub mod __runtime` for flui-runtime/testing/hot-reload. This replaces the `runtime-internals` Cargo feature, which feature unification turns on in every app (flui-app/Cargo.toml:90).
- **flui-platform backends:** one module per OS, with a shared `owner_loop` state machine (admission, deferred open, quit fence, exit policy, wake deadline) driven by a small `NativeLoop` trait. Today that logic is re-implemented 4-6 times (winit/control.rs, macos owner_lane/wake_pump/loop_control, windows/owner_control).

### 2.5 Feature-flag policy

1. **Features are additive and never change a production type's layout.** The 57 `cfg(any(test, feature = "testing"))` sites in flui-rendering, including fields on `PipelineOwner` (owner/mod.rs:251-257), are removed. Test hooks become a harness-installed registry. A `static_assertions` size check pins the layout.
2. **One naming rule:** `unstable-*` for anything not Stable, and no `*-internals` features.
3. **Library crates have no default features** except where a target needs one. Delete `flui-platform/desktop` (it pulls winit and gates no code, Cargo.toml:300-303) and the other no-op features (`web`, `wayland`, `x11`, `mint`, and flui-app's `desktop/android/ios/web/debug-overlay/performance-overlay`).
4. **Facade `default = []`.** Material is not a default feature. `a11y` becomes unconditional for Windows/macOS adapters, and only AT-SPI stays behind a default-on opt-out.
5. **`forbid-reach` facts in manifests**, checked alongside the existing TREE_FACTS (facade.rs:49-90).

### 2.6 Facade and prelude

- **Curated modules** replace crate aliases (src/lib.rs:120-152): `flui::{view, widgets, rendering, painting, interaction, animation, platform, testing, sdk}`. Each module is an explicit `pub use` list, like src/rendering.rs already is.
- **`flui::prelude`** is an enumerated, catalog-neutral list with no glob of layer preludes. That removes the `BuildOwner`/`ElementTree`/`tracing::info!` leaks (flui-view/src/lib.rs:252,278-285). A snapshot test pins the name list.
- **`flui::sdk`** is the package-author surface: RenderView authoring, `PostFrameHandle`, `Surface`, `PhysicalShape`, `TranslationFraction` and similar. Material and Cupertino compile against `flui` alone, and `cargo xtask workspace` checks it: a package's only normal in-repo dependency is `flui`.
- **`flui_material::prelude`** is globbed alongside by apps that use Material.

### 2.7 Publish order

Tier order: T0 (geometry, types, foundation, macros, log) → T1 (platform-api, reactive, protocol) → T2 → T3 → T4 → T5 → T6 (platform, app, flui) → packages (material, cupertino, devtools, mcp, hot-reload). Use `cargo publish --workspace` (Rust 1.90) and `cargo xtask release-check`, which runs `cargo package` in order plus semver-checks against the last tag. Internal pins move to `[workspace.dependencies]` with `=` only inside the core train (today there are 172 hard-coded pins). Packages use caret requirements on `flui`.

---

## 3. Runtime model

### 3.1 Trees and identity

- **Five trees stay:** View → Element → Render → Layer, with Semantics alongside.
- **All ids are generational `GenId<M>`.** ElementId stops being a bespoke packing (id.rs:1163), and LayerId/SemanticsId stop being reusable slab indices (id.rs:614-760). Delete ViewId. This must happen before damage (H2) keys caches on layer identity, and before the agent protocol exposes SemanticsId, which today can alias after reuse.
- **Topology is owned by the tree that stores it.** flui-rendering exposes `set_children(parent, &[RenderId])` / `move_subtree`, which enforce arity, depth, dirty-marking and capture eviction. flui-view submits per-parent diffs. The whole-forest `synchronize_render_children` (element_tree.rs:1428-1520) becomes a debug-only verifier.

### 3.2 Realms

```text
App (flui-app, T6)
 └─ Runtime host (flui-runtime, T4)   owner thread(s); no TLS registry
     └─ Realm  (!Send)               ReactiveGraph, GlobalKeyScope, CapabilityHost,
         │                            FocusCoordinator, SchedulerCore, ExecutionSpawner,
         │                            FontContext, ImageCache handle, RealmObserver
         └─ Presentation × N          ElementTree+BuildOwner, PipelineOwner, FrameClock,
                                      Vsync, SemanticsHost, → FrameSink(raster lane)
 └─ SharedEngineServices             GpuContext (one device), fontique Collection (shared,
                                      immutable after load), pipeline cache
```

- **Retire APP_RUNTIME TLS** (runner/host.rs:46). The host is an owned value passed into platform callbacks. ADR-0027:60 itself says TLS cannot express two realms on one thread.
- **Decide explicitly (ADR-0027 amendment):** one owner thread per process hosting N isolated realms is the model for H0-H2. Per-realm owner threads (Win32, Linux) are an H2 spike, not a claim. The ADR-0027 verdict "multiple realms may execute concurrently" is not implemented today, so leaving it in place is a correctness hazard in the docs.

### 3.3 Scheduling and demand

- **One demand authority per presentation:** `FrameClock::mark_demand(reason)`. Scheduler, AsyncDriver, Vsync, input and `request_visual_update` only mark demand. The loop wake is an edge "some clock is armed". This removes five carriers (frame_clock.rs, runtime.rs:727, scheduler hook, vsync.rs, #1172).
- **Named phases** in flui-scheduler: `input → build → effects → layout → compositing → paint → semantics → submit`. Effects (ADR-0075) run in the product frame, not only in the harness.
- **Clock:** one virtual-capable frame timestamp per presentation. Retire the controller's self-scheduling wall-clock `Ticker` (vsync.rs:1-20), and move time dilation into the presentation clock.

### 3.4 Lanes and async

- **Owner lane (default).** Futures here are `!Send`: `AsyncDriver` gets a local variant and drops the `Send` bound (async_driver.rs:102).
- **IO and compute lanes.** These are realm capabilities acquired in `init_state` and cancelled on unmount:

```rust
let task = cx.spawn_io(async move { fetch(url).await })       // Send future, off-thread
             .then_on_owner(|state: &mut NotesState, bytes| state.load(bytes)); // !Send delivery
// dropped with the element => CancellationToken fired
```

- **Tokio has one owner.** It is an implementation detail of flui-runtime's default executor behind a feature, and hosts can inject their own (ADR-0047 `HostExecutors`). Delete the platform `BackgroundExecutor` (executor.rs:66) and the assets bridge runtime. Up to four runtimes per process today (execution.rs:327,343; bridge.rs:66; executor.rs:66) becomes one.
- **Raster lane.** Accept ADR-0045 as a two-mode contract, with Inline as the permanent mode for macOS and wasm. Move `RasterOwner` from flui-engine to flui-runtime. Web moves onto the lane, and `DirectSink` is deleted (web.rs:85).

### 3.5 State and reactivity

- **`flui-reactive` (T1)** holds one realm-owned graph. Subscribers are typed as `Element(ElementId@presentation) | Layout(RenderId) | Paint(RenderId)`, which is Compose's per-phase read tracking. It answers the plan's "element-level or render-level" question with "both, one graph".
- **Push-pull Clean/Check/Dirty** propagation with slab-linked edges. This fixes the ADR-0075 prototype's diamond glitches. Writes are synchronous on the value and coalesced for invalidation per frame; record that choice explicitly.
- **Listenable/ChangeNotifier are re-based onto the graph** as `!Send` sources. `ListenerCallback = Arc<dyn Fn + Send + Sync>` (notifier.rs:46) disappears, and so do the locks on the paint path.
- **Write journal** (slot, writer, frame) exposed to the `RealmObserver`. This gives record/replay (G7) and agents Elm-like observability without Elm's ceremony (hypothesis, from iced 0.14's time travel).
- **`signals` is on by default**, and `Signal` joins the prelude. `StateCell`/`StateHandle` move to `flui::view::state` as the setState-level layer. Doing this before C1/C2 widen the catalog avoids a dual-idiom freeze.

### 3.6 Rendering, text, engine

- **Raster contract in flui-layer.** It covers layer walk semantics, effect decomposition (backdrop, shader mask and follower become neutral steps instead of `Renderer` methods), `CommandRenderer`, the damage diff and a conformance suite. flui-engine (wgpu) and flui-raster-cpu both implement it. HeadlessRenderer is deleted in favour of the wgpu backend rendering to a caller texture.
- **Damage.** Diff consecutive LayerTrees keyed on `render_id` boundaries (already stamped, layer_tree.rs:38; paint.rs:1258). Emit `DamageRegion::Partial`. Present through a retained target plus a blit, because wgpu has no buffer age or present regions (wgpu#682). Hypothesis: scissoring directly into a rotating swapchain image (renderer.rs:2290-2330) would leave stale pixels, and this must be proven before E1 lands.
- **Text service.** Parley with a per-realm `FontContext`/`LayoutContext` over a shared fontique `Collection` (`shared: true`). The glyph key carries font-blob identity. Rasterisation runs lock-free on the raster side. ICU4X from Parley is the single Unicode source; drop unicode-segmentation. The ADR-0077 acceptance criteria must include "no process-global font state".
- **GpuContext per app, Presentation per window.** Today every window builds its own Instance/Device/pipelines/atlas (renderer.rs:1140-1168).

### 3.7 Platform and accessibility

- **Contract/backend split** (section 2). Owner-owned native windows are `!Send`, and a `Send` `WindowHandle` proxy carries a closed verb set (redraw, close, title…) routed through a mandatory per-backend transport. This removes most of the 26 `unsafe impl Send/Sync` and the documented `request_redraw`-from-any-thread violation (#949, traits/window.rs:131-240).
- **IME is a document protocol.** The backend synchronously pulls text in a range, the selection, the rect for a range and the index for a point from the owner. TSF plus a UIA TextPattern on Windows is the B1 prerequisite. The push-only two-method trait (text_input.rs:24-45) cannot serve TSF, InputConnection or UITextInput. The Win32 backend has no IME code at all.
- **One backend per OS, recorded in an ADR:** native Win32/AppKit/UIKit/Android/web, and winit for Linux. Delete `LinuxPlatform`, whose methods all call `unimplemented!`.
- **AccessKit on by default.** accesskit_android and accesskit_ios in H1; `tree_id` for multi-window and embedded foreign trees. Semantics is enabled by a reference-counted `SemanticsHandle` shared by assistive technology, agent and devtools (semantics_host.rs:30-50 is dead today).

---

## 4. Extension points and plugin model

### 4.1 `PlatformCapability` (H1, designed now)

```rust
// flui-platform-api (T1): no OS deps
pub trait PlatformCapability: 'static {
    type Handle: Clone + 'static;          // !Send by default; Send only if it truly crosses lanes
    const ID: &'static str;                // stable name for diagnostics/protocol
}
#[derive(Debug, thiserror::Error)]
#[error("{capability} unsupported: {reason}")]
pub struct Unsupported { pub capability: &'static str, pub reason: UnsupportedReason }

pub trait CapabilityProvider<C: PlatformCapability>: 'static {
    fn attach(&self, native: &NativeContext<'_>) -> Result<C::Handle, Unsupported>;
}

// flui-view: the sealed trait stays closed; one erased method + typed ext
pub trait LifecycleContext: BuildContext + Sealed {
    #[doc(hidden)] fn capability_erased(&self, id: TypeId) -> Result<Rc<dyn Any>, Unsupported>;
    /* existing handles */
}
pub trait LifecycleContextExt: LifecycleContext {
    fn capability<C: PlatformCapability>(&self) -> Result<C::Handle, Unsupported> { /* downcast */ }
}

// app
flui::App::new(NotesApp)
    .capability::<flui_clipboard::Clipboard>(flui_clipboard::endorsed())
    .run();
```

- **`NativeContext`** exposes per-OS handles: HWND, NSWindow, JNI VM/Activity, UIViewController. It is available only on the owner thread.
- **Owner-thread execution** uses a typed `OwnerTask` trait object, never a closure channel, keeping ADR-0039 §3.
- **Endorsed implementations** are `[target.'cfg(..)'.dependencies]` of the capability crate, and an override hook exists from day one (Flutter has lacked one for years, flutter#80374).
- **First three built-ins:** clipboard (installed today but dead, runtime.rs:1636), haptics (dead, presentation.rs:866-893), file dialogs.
- **Rename** the unused `PlatformCapabilities` flag table.

### 4.2 Third-party render objects

The Stable tier is `flui::rendering`, grown to include the sliver authoring set, the viewport-offset trait, `LayerLink` and the virtualizer. Fixes:

- `RenderView::RenderObject` loses its `Send + Sync` bound (render.rs:451).
- `CustomPainter` drops `as_any`, using trait upcasting.
- `#[derive(RenderView)]` replaces `impl_render_view!`.

A public element protocol (a narrow `ElementOwner` facade with a layout callback and child-manager hooks) lets lazy and layout-builder elements live outside flui-view. The **conformance kit** is:

```rust
flui::testing::rendering::conformance::check_box(|| MyBox::new())?;   // dry==wet, intrinsics finite & monotone,
flui::testing::rendering::conformance::check_sliver(|| MySliver::new())?; // baseline in size, hit-test in bounds, idempotent relayout, semantics stable
```

Every first-party object runs it. This replaces the string-grep RENDER_OBJECT_TYPES check (render_object_harness.rs:14440-14485). A facade-only fixture adds a custom `RenderSliver`; there are zero today.

### 4.3 External GPU content

`Layer` stays a closed enum, plus one open variant `Layer::External { id: ExternalContentId, rect }` backed by a typed registry. That registry is reached through a `TextureRegistry` capability bound to the shared `GpuContext` device, with a fence contract across the raster mailbox (an ADR-0045 addendum). A `Texture` widget is the first producer, with a readback test through the real app path. `PlatformViewLayer`, whose render is a no-op (layer_render.rs:381-384), is removed until the presenter/compositor spike (H1: DirectComposition, CALayer, SurfaceControl, modelled on the Subduction `Presenter` + frameclock shape).

### 4.4 Themes as data

A design-neutral token substrate in flui-widgets: typed colour, typography, shape and motion maps with serde, resolved through InheritedView plus FieldMask. `ThemeData::from_tokens` sits in each package. `WidgetStateProperty` gets a serializable per-state-map form, with the closure kept as an escape hatch (widget_state.rs:273). ADR-0042 is amended, not reversed: there is still no universal ThemeData.

### 4.5 Agent protocol

`flui-protocol` (T1) is the single schema. Backends:

1. `flui-devtools`, in-process on `RealmObserver`: semantics snapshots, element/render/layer trees with stable names, frame telemetry, the write journal and diagnostics. Compiled only with the `devtools` feature, never in release, over a pipe with a per-launch token.
2. `flui-mcp` over UIA/AX/AT-SPI for live platform verification.

Every read is bounded (scope, depth, max nodes, concise|detailed). Every action returns the post-action outline. The same outline is the semantic golden file format (one node per line, deterministic ids). Views gain a derive-generated `TYPE_NAME`, because a `TypeId` is neither stable nor readable (view.rs:460).

### 4.6 A2UI

`flui-a2ui` is a package in the Evolving tier. It is a surface controller over the realm, a catalog read from `#[derive(Catalog)]` metadata (JSON Schema per widget, URI catalogId), data bound by JSON Pointer to `flui-reactive` Store paths, and named pure functions registered in the catalog. There is no LLM client in core; transport sits behind an adapter trait (GenUI's May 2026 lesson). The same derive emits G6, the `flui create` AGENTS.md index and llms.txt.

---

## 5. Public API and DX

```rust
use flui::prelude::*;          // catalog-neutral
use flui_material::prelude::*; // opt-in skin

#[derive(Store, Clone, Default)]
struct Notes { items: Vec<Note>, draft: String }

#[derive(StatefulView)]
struct NotesScreen;

struct NotesState { notes: Store<Notes> }   // Copy handle, realm-owned

impl StatefulView for NotesScreen {
    type State = NotesState;
    fn create_state(&self, cx: &dyn LifecycleContext) -> NotesState { NotesState { notes: cx.store(Notes::default()) } }
}
impl ViewState<NotesScreen> for NotesState {
    fn build(&self, _: &NotesScreen, cx: &dyn BuildContext) -> impl IntoView {
        let notes = self.notes;                             // Copy: no clone noise
        Column::new((
            TextField::bound(notes.draft()),                // signal-or-value prop type
            FilledButton::new("Add").on_pressed(move |w| notes.update(w, |n| n.commit_draft())),
            ListView::keyed(notes.items(), |n| n.id, |n| NoteRow::new(n)), // keys = store keys
        ))
    }
}

fn main() -> anyhow::Result<()> {
    flui::App::new(MaterialApp::router(routes!())).run()    // any View root; one entry on every target
}
```

The contract rules:

- **`run_app` accepts any View.** Today a StatefulView root is rejected (runner/mod.rs:214), which forces a wrapper type.
- **Writes happen outside build.** Their `w: &mut Writer` token is available only in callbacks and effects. This makes ADR-0074's run-time guard a type (S2 applied to state).
- **One signal-or-value prop type** (`impl Into<Bind<T>>`) exists before Material and community catalogs grow.
- **One unit rule at the widget boundary:** `impl Into<Pixels>`. Today 132 pub fns take `f32` against 4 that take `Pixels`.
- **Router is the primary navigation API.** Typed routes come from a derive, with the URL as the source of truth. `Navigator` becomes an implementation detail, and its about 30 push/pop variants leave the prelude.
- **The capability pattern** is shown in 4.1. Tests use the same public path:

```rust
#[flui::test]                         // seeded executor, virtual clock, CPU backend
async fn add_note(t: &mut Tester) {
    t.pump(NotesScreen).await;
    t.find(role(TextInput)).type_text("milk").await;
    t.find(role(Button).name("Add")).invoke().await;
    t.expect_semantics_golden("notes/add");      // outline file, same format as the agent reply
}
```

---

## 6. Performance model (stated as correctness contracts)

Performance regressions here are mostly structural. They should be pinned by **deterministic counts** as per-PR gates, with wall time as a trend only:

| Count | Scenario | Target |
|---|---|---|
| Elements rebuilt | one signal write in a 10k list | O(readers) (measured today at 20003 → 2 with signals) |
| Layout roots / layout passes | lazy fling over 100k | ≤ 2 passes/frame (today up to 6-10, layout_builder.rs:64-74) |
| Slab scanned nodes | D dirty roots | O(D), not O(D·N) (tree.rs:82-110) |
| Topology work | one row mount | O(children of parent), not O(forest) |
| Damage area | one text change | ≈ its box (today: full surface, raster_lane.rs:354) |
| Allocations | unchanged-boundary repaint | 0 steady-state |
| Frames | idle 10 s | 0 per presentation |
| Phases to first present | cold start | instrumented per OS (none today) |

View configs become shared (`Rc` children, or moved rather than deep-cloned, into_view.rs:178-184). Paint scratch buffers are reused. The `bench-collect` skip of required-features benches (bench.rs:36) is removed, which revives the damage baseline.

---

## 7. Safety model

| Invariant | Enforcement (target) | Today |
|---|---|---|
| No new process global | `cargo xtask globals` (syn: `static` of Mutex/RefCell/OnceLock/LazyLock types, `thread_local!`) + allowlist file that may only shrink; clippy `disallowed_macros` for `thread_local!` outside platform/app | Nothing; ratchet file gone |
| UI state is `!Send` | `assert_not_impl_any!` on realm types and controllers; clippy `disallowed_types` (`parking_lot::Mutex`, `std::sync::Mutex`, `RwLock`) in view/rendering/objects/widgets/animation, with an allowlist | 398 Mutex/RwLock in production; Send+Sync traits |
| No lock in public signatures | public-API snapshot filtered for Mutex/RwLock | `ElementBuildContext::tree()` (element_build_context.rs:128-135) |
| Unsafe confined and documented | `unsafe_code = warn` (kept) + `undocumented_unsafe_blocks = warn` per module + per-island budget counted by xtask; Miri on arena/key/surface-lease; scheduled macOS runner + Windows `xtask device` in CI; platform logic separated from FFI so it runs under Miri | ≈403 sites; platform FFI never executed |
| Panics classified | xtask lint: `expect`/`panic!` literal starts `BUG:` or names a `try_` twin; guarded calls (`guarded_call(node, phase, f)`) for hit-test, intrinsics, semantics too; repeated paint poison substitutes an error-box picture instead of stalling frames | Layout/paint only; stuck paint poison freezes UI |
| Eq/Hash/Ord consistency | property test `a == b ⇒ hash(a) == hash(b)`; canonicalise −0.0 or drop Eq from float units | Violated (units.rs:575-596) |
| Reach | `forbid-reach` manifest facts | Only hot-reload TREE_FACTS |
| One implementation per contract | `ElementBuildContext` deleted; `HeadlessBinding` = runtime core + manual clock; headless render = wgpu to texture; raster conformance suite across backends; protocol types in one crate | Two of each |
| Determinism | realm-scoped ids and counters for anything serialised; per-realm fonts; seeded executor in `#[flui::test]` | ~20 static ID counters; FONT_SYSTEM pin ritual (fonts.rs:22-35) |
| Stable surface | cargo-public-api snapshot + semver-checks on the Stable crates, advisory now, gating at H3 | None |
| Docs don't lie | ADR front matter (status enum, supersedes symmetry) validated by xtask; ARCHITECTURE symbol references resolved; no process markers (regex check) | Free text; AGENTS.md ID-offset row contradicts code (id.rs:1163) |
| Dev tooling isn't an attack surface | protocol server behind a `devtools` feature, off in release profile by `compile_error!` guard; pipe/loopback plus token | n/a (future) |

Also: set `deny.toml multiple-versions = "warn"` with a reasoned skip list (67 duplicate crate names today), and wrap `android_activity` in a FLUI newtype instead of re-exporting it from the facade (src/lib.rs:157).

---

## 8. Delete, merge, or replace with ecosystem crates

- **Delete:** flui-localizations; flui-tree's trait trio; `ElementBuildContext`; `src/window.rs` duplicate Window family; `PlatformEmbedder`; `BasicVelocityTracker`; `as_winit`; `LinuxPlatform`; no-op features; flui-types physics/BoxConstraints/MaterialColors/orphans; flui-geometry's unused vocabulary; `__private`; `ElementKind::RenderLeaf/Single/Optional` and the arity type parameter; `SemanticsSnapshot` (or make it the protocol outline); `SharedEngineServices`' empty seam (or populate it); `PlatformViewLayer` until compositor work; tools/web-server; the dlopen hot-reload path after Subsecond lands; `pub use ::wgpu`; public `WgpuPainter`.
- **Merge:** the three ID schemes into `GenId`; two change-notification systems into flui-reactive; two image caches into flui-assets; two web servers into `flui run --device browser`; four live drivers (xtask device UIA, desktop-mcp, live-smoke, Python/Swift) into one Rust driver library in flui-mcp.
- **Replace with ecosystem crates:** cosmic-text → Parley/fontique/HarfRust/ICU4X (evaluate glifo for glyph caching); hand-rolled CPU raster → vello_cpu or tiny-skia; dlopen → Subsecond; hand-copied role enum → `accesskit::Role`; frame demand → consider `frameclock` (no_std, owns no loop); publish ordering → `cargo publish --workspace`; hand-rolled COM UIA in xtask → the `uiautomation` crate already used by desktop-mcp. Wrap every pre-1.0 upstream type (ui-events, accesskit, kurbo, parley) behind FLUI types on the Stable surface, following the Bevy/glam lesson.

---

## 9. Breaking changes to make now, pre-1.0

Ordered by how much later work each one unblocks:

1. **Platform contract split + `forbid-reach`** (supersedes part of ADR-0037).
2. **`!Send` flip of all UI protocol traits** (Listenable, Animation, TickerProvider, CustomPainter, delegates, ScrollPhysics, ViewKey, HitTestTarget, cells, RenderView::RenderObject). Removing a supertrait after H3 breaks every implementor.
3. **`PlatformCapability` + typed lookup** (supersedes ADR-0078 clause 1).
4. **Extract `flui-runtime`; one frame transaction for app and tests** (supersedes the ADR-0041 gate). Retire APP_RUNTIME TLS.
5. **Realm-owned reactive graph core in `flui-reactive`**; signals on by default; ChangeNotifier re-based (amends ADR-0074 placement).
6. **Generational ids everywhere**; delete ViewId; fix AGENTS.md's ID row.
7. **Raster contract crate boundary + CPU backend seam**; delete HeadlessRenderer's walker (supersedes the engine "nothing pluggable" stance).
8. **Text service per realm** (with Parley); delete FONT_SYSTEM.
9. **IME document protocol** (supersedes ADR-0030 §1).
10. **Facade curation, catalog-neutral prelude, `sdk` module, Material off by default**; design packages depend on `flui` only (amends ADR-0028 with raw primitives: RawButton, FocusableActionDetector, Surface, toggles).
11. **Router ADR; freeze Navigator features**; replace `NavigatorCommand` in the runtime channel with navigation intents.
12. **Colour model decision** (f32 + colour space, or a recorded u8 divergence) before serde forms of themes exist.
13. **Delete the dead surface in section 8** before the first crates.io publish.

---

## 10. Evolution H0 → H4 without rewrites

- **H0 (beta).** Items 1-8 plus the gates in section 7. Exit evidence: Notes built from crates.io against a curated facade; `flui mcp` scenario over `flui-protocol` with the in-process backend; semantic goldens via the CPU backend; Windows TSF + Narrator; `globals` allowlist shrinking; a `release-check` job green.
- **H1.** Capability plugins built out-of-repo against `flui-platform-api` only; mobile via `PlatformHost` adapters sharing the runtime core; accesskit_android/ios; `flui-a2ui` and `flui-i18n` as packages; the compositor spike through a `Presenter` seam behind the raster contract. **Nothing in core changes shape.** Packages consume the Stable surfaces frozen informally in H0.
- **H2.** Damage producer on generational layer identity; threaded raster lane (text no longer blocks it); IO lane owned by the runtime; retained root capture; count gates become budgets; the per-realm owner-thread spike decided by ADR. The runtime model is designed so threading changes who calls `pump`, not what a frame is.
- **H3.** Flip semver-checks from advisory to gating on the ~8 Stable crates; tiers are already expressed as module visibility plus `unstable-*` features; Material and Cupertino move to their own repos mechanically (they already depend on `flui` only); a `flui migrate` data table ships per crate.
- **H4.** Embedders use the runtime core + `PlatformHost` + the minted-backend seam (#560); Vello GPU evaluated as a raster-contract backend; community badges computed by `flui` CLI checks (builds against current flui, conformance kit passes, semantics golden, unsafe budget declared).

---

## 11. Risks and deliberate non-goals

**Risks**

- **Sequencing risk with bus factor 1.** Items 1, 2 and 4 each touch 5+ crates. Mitigation: do the platform split and the `!Send` flip as mechanical, gate-first changes (land the check with an allowlist, then shrink it). Each fits the WIP limit of 2 tracks.
- **The `!Send` flip may conflict with the raster lane or parallel layout.** Mitigation: `Scene` stays `Send` (statically asserted, scene_snapshot.rs:76-78), and intra-realm parallel layout is explicitly *not* a goal until a measured H2 spike says otherwise.
- **Pre-1.0 upstream churn** (Parley, AccessKit, ui-events, winit 0.31, A2UI v0.x) leaking into Stable. Mitigation: wrapper types and version-cohort pins.
- **CPU/GPU golden divergence.** Mitigation: goldens are semantic by default; pixel goldens only on the CPU backend with pinned SIMD; GPU-only effects are excluded from pixel goldens until they have CPU implementations.
- **Over-tiering creating crate churn.** Adding 4-5 crates goes against "fewer publish units". Each new crate here is a real layer boundary (contract, graph core, protocol, runtime, alternate backend), consistent with "layers, not micro-crates".
- **Hypotheses still unmeasured:** swapchain-scissor staleness, cold-start split, lazy-band pass counts, the cost of the global topology pass, and the duplicate upper-stack builds from `testing` features.

**Deliberate non-goals**

- No intra-realm parallel layout before H2 evidence.
- No second rendering model (CSS/web engine, webview hybrid).
- No custom UI DSL for AI generation. Typed catalog + A2UI instead, with agent success rate measured against the DSL approach.
- No in-process MCP server inside core crates (Slint's model). MCP lives in `flui-mcp`/CLI over `flui-protocol`.
- No `Send` UI state "for flexibility".
- No cross-OS pixel parity in production. Determinism is promised only on the CPU reference.
- No MVCC/global-snapshot state (Compose); batched single-writer transactions instead.
- No separate repos for packages before they have an independent cadence (Flutter re-merged its engine in 2024).

---

Relevant input files: `C:\Users\vanya\AppData\Local\Temp\claude\D--flui\bbb28042-f972-4a10-8e94-731819e26161\scratchpad\context.md`, `...\plan.md`, `...\roadmap.md`.
