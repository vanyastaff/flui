# FLUI target global architecture, from the "AI-native first" angle

*Scope: the whole architecture of FLUI at main `cab06137d` (2026-09-25): workspace and crate topology, runtime, extension points, API, performance, safety, and how it evolves through H0–H4. The lens is that one machine-readable contract (catalog, semantics, inspection, actions, diagnostics, replay) is the product's backbone. Evidence comes from the 12 codebase maps and 6 market surveys, cited as `path:line`. Anything I did not verify is labelled **hypothesis**. This is a read-only review: nothing in the repo was changed.*

---

## 0. Thesis in one paragraph

The owner's 2030 picture is "one catalog, three consumers": a developer with hot reload, a coding agent over MCP with golden tests and replay, and a model at runtime over A2UI (plan.md, "Взгляд на 2030"). Today there is **no single contract for those consumers to share**. What exists instead:

- **Four semantics vocabularies:** Flutter-shaped flags in flui-semantics, AccessKit output, a hand-copied role enum in `tools/desktop-mcp/src/a11y/role.rs:17`, and an unused `SemanticsSnapshot`.
- **Three inspection formats:** `DiagnosticsNode` dumps, `TreeObserver` events carrying only `TypeId`, and UIA outlines.
- **No widget catalog at all.**
- **A protocol trapped in a `publish = false` binary** (`tools/desktop-mcp/Cargo.toml:11`, no lib target). No crate is allowed to depend on it (`Cargo.toml:88-90`).

The render, runtime and platform problems the other maps find matter for the same reason. Each one is also a place where the contract has no stable identity or no owner:

- fresh `LayerId`s every frame (`flui-rendering/src/pipeline/owner/paint.rs:1383-1425`);
- `TypeId`-only elements (`flui-view/src/view/view.rs:460`);
- process-global ID counters and `FONT_SYSTEM` (`flui-painting/src/text_layout/layout.rs:124`);
- a frame transaction duplicated in `flui-testing/src/lib.rs:955-1079`.

My target architecture makes the **contract a layer of its own, low in the graph**. Every tree gets a stable, realm-scoped, generational identity, and every consumer becomes a thin client of that contract: devtools, `flui mcp`, `flui test`, A2UI, and the OS-level desktop driver.

---

## 1. Guiding principles, reconciled with the owner's seven

| # | Owner's principle | Position | Refinement from this angle |
|---|---|---|---|
| 1 | The mental model is sacred (View→Element→Render, keys, constraints) | **Agree** | The model also has to be *machine-visible*. An element whose only identity is a `TypeId` (`view.rs:460`) is invisible to agents. Every View gets a stable catalog name (§5). |
| 2 | One language, one toolchain, **one renderer** | **Partly disagree** | Keep "one *production* renderer per platform (wgpu)". Change the invariant to **"one raster contract, several rasterisers"**. The CPU reference for golden tests (E7), the software fallback (H2) and headless capture all need the same lowering. Today `flui-engine/src/headless.rs:15-24` already renders BackdropFilter, ShaderMask and Follower differently from the windowed path. `ARCHITECTURE.md:8-14` ("nothing pluggable") has to be superseded, otherwise E7 cannot be built without contradicting a recorded decision. |
| 3 | No global state (ambient-reach = 0) | **Agree, and extend** | Extend it to **identity determinism**. About 20 process-wide `static Atomic*` ID counters (GlobalKey, Route, FocusNode, PipelineOwner…) make IDs depend on test order. Replay (G7) and semantic goldens (G3) need realm-scoped allocators. The ratchet file the roadmap relies on (`runtime-contract.toml`, roadmap.md:224/743) **does not exist** and no xtask gate replaces it. This must be restored first. |
| 4 | Everything machine-readable; DevTools is a client of the agent's protocol | **Agree, but disagree on layering** | plan.md says "the agent protocol over standards: MCP transport, AccessKit vocabulary, own spec only for what MCP lacks". I would make the **FLUI wire protocol primary and MCP a projection of it**, as egui does (`egui_inspection` with `egui_mcp` as a client) and Dart does (DTD with the MCP server as a client). Reasons: (a) the MCP 2026-07-28 spec removed sessions, Logging and Sampling, so building on MCP features is building on sand; (b) rmcp/JSON-RPC must not enter the framework's layer graph (the problem with Slint's in-app MCP); (c) tests and devtools need the same types without MCP. The AccessKit vocabulary still holds. |
| 5 | Proof, not claims | **Agree, and make it data** | BETA.md is 1171 lines of prose. Evidence should be structured records (`docs/evidence/*.toml`) produced by the same protocol runs that agents use. |
| 6 | Break explicitly | **Agree** | ADR front matter (a status enum, Supersedes symmetry) should be checked by xtask. Today there are 61 ADRs with free-text statuses and no index. |
| 7 | Market before decisions | **Agree** | The surveys change several decisions below: Stores for A8, per-phase reads for signals, Subduction/frameclock for presentation, and glifo for Parley rasterisation. |

Two principles of my own, for AI contributors:
- **P8: one concept, one home, found by a machine.** Duplicate types, the same type at four facade paths, and history-narrating comments are defects because agents replicate them. Examples of duplicates: `BoxConstraints` in `flui-types/src/layout/constraints.rs:37` and in `flui-rendering`; physics in two crates. `CursorIcon` is reachable by 4+ paths.
- **P9: rules are gates.** The module-direction check that replaced the widget crate split (2026-09-23) does not exist (`grep tools/xtask/src` finds nothing). The globals ratchet is gone. The BUG: panic convention is review-only. An agent-heavy codebase decays at the speed of the least-checked convention.

---

## 2. Workspace and crate topology

### 2.1 The shape

There are 11 numbered layers today (`Cargo.toml:86-104`). Three of them hold a single crate, and L2 hides the `interaction→platform` edge that drags winit, tokio and `windows` into `flui-rendering` (`cargo tree -p flui-rendering -i winit`). I propose **seven named tiers** with **reach facts**, not just direction:

```
T0 Values      geometry · types · foundation · macros(proc-macro, exempt)
T1 Contracts   flui-platform-api · flui-protocol · flui-state
T2 Substrate   tree · scheduler · painting(recorder+text service) · interaction · assets · log
T3 Composition layer(+raster contract) · semantics · animation
T4 Render      rendering(protocol) → objects(catalog) ; engine(wgpu) ‖ raster-cpu
T5 Spine+UI    view → widgets(Raw catalog + catalog registry) ; runtime ; testing
T6 Roots       platform(backends) · app(runners) · cli · flui(facade)
Packages       material · cupertino · mcp · a2ui · i18n     (own release train, `packages/` → own repos later)
```

**Reach facts** go in the manifests as `[package.metadata.flui] forbid-reach = [...]` and are checked by `cargo xtask workspace` with `cargo tree -e normal`. For every crate in T0–T5 except `runtime`, the rule is: no `winit`, `tokio`, `windows`, `objc2`, `android-activity`, `wgpu`, `rmcp`. This is what makes the contract crates something agents and plugins can depend on without taking in OS backends. Today the headless stack is pure only by accident (the maps: "headless/wasm purity is only accidental").

### 2.2 New crates, and why each one earns a crate

The test for a new crate is: *a distinct set of consumers, and a dependency footprint the consumers must not inherit*.

1. **`flui-protocol` (T1).** Serde plus accesskit, nothing else. It holds the ADR-0080 wire contract:
   - handles (`w3`/`e12`/`s2`) and error codes with retry/effect semantics;
   - AccessKit role and action names via `accesskit::Role`, not a copy;
   - the outline serializer;
   - JSON Schemas;
   - the `AccessibilityBackend` trait (now at `tools/desktop-mcp/src/a11y/mod.rs:498`);
   - diagnostic event codes (`layout.overflow`…);
   - the frame-telemetry envelope;
   - the replay log format;
   - the CLI NDJSON event envelope (the CLI now emits about 28 untyped `json!` events, `crates/flui-cli/src/ui.rs:180`).

   Its consumers are desktop-mcp, the in-process server, flui-testing's finders, flui-cli, and flui-a2ui. It has to be low in the graph because the rule forbids depending on a tool, and because tests at T5 need it.

2. **`flui-platform-api` (T1).** Capability traits (`PlatformTextInput`, `PlatformAccessibility`, `PlatformHaptics`, the new `PlatformCapability`), the input/IME vocabulary (re-exported `ui-events`), the owner-capability minting seam (#560), and the lifecycle state machine. It has no OS dependencies. `flui-interaction` depends on this crate instead of the 46k-line backend crate: today its only use of the backends is `text_input.rs:27`. Plugins depend on it too.

3. **`flui-state` (T1).** The realm-owned reactive graph core: arena, Clean/Check/Dirty push-pull, owner disposal, the **write journal**, and phase-typed subscribers. It moves down from `flui-view/src/reactive/mod.rs` so that `flui-rendering` and `flui-animation` can subscribe needs_layout/needs_paint readers (Compose's per-phase reads). That lets it retire the `Arc<Mutex>` `Listenable` repaint path (`flui-foundation/src/notifier_generic.rs:41-45`). The roadmap already names `flui-state` (roadmap.md:766), and ADR-0074 puts the graph in flui-view. One ADR should supersede the placement and fix `docs/FOUNDATIONS.md:89-91` and the `Cargo.toml:72-77` comment together.

4. **`flui-runtime` (T5, above view/widgets, below app).** It is extracted from flui-app and holds:
   - `Realm`, `Presentation`, and the one frame transaction (today `flui-app/src/app/ui_realm/frame.rs:74,500`, re-implemented in `flui-testing/src/lib.rs:955-1079`);
   - lanes, execution services and the capability registry;
   - Subsecond hooks;
   - the **inspector server** module (the in-process protocol backend, behind a feature that is off in release).

   ADR-0041 gates `flui-runtime` on "two entry points". **I challenge that gate:** the headless test binding *is* the second entry point, it exists today, and it is a divergent copy. Keeping the gate forces tests to verify a pipeline that production does not run.

5. **`flui-raster-cpu` (T4 sibling of engine).** The CPU reference rasteriser (a vello_cpu or tiny-skia backend) implementing the raster contract that moves into `flui-layer`. It stays `publish = false` until H2 promotes it to the software fallback.

### 2.3 Per-crate fate table (all 27)

| Crate | Fate | Reason (evidence) |
|---|---|---|
| flui-geometry | **Keep, trim** | Delete about 3.5k lines of unused GPUI vocabulary (`length.rs`, `transform2d.rs`, `bezier.rs`, `text_path.rs`, kurbo bridge, no-op `mint`). Pick one Rect (drop `Bounds`) and one Axis. Fix the `Pixels` Eq/Hash contract (`units.rs:91,575-596`), because it corrupts cache keys and snapshot dedupe. |
| flui-types | **Keep, shrink** | Delete physics (duplicated in `flui-animation/src/simulation.rs:31`), `BoxConstraints` (duplicated), `MaterialColors` (570 lines, design residue in L0), and the orphans. Move gesture details to interaction. Rule: an item must be consumed by at least two crates. Decide Color f32+colour-space before publish, since themes-as-data serialise it. |
| flui-foundation | **Keep, split internally** | Keep IDs, keys, callbacks, diagnostics vocabulary and observe. Move runtime protocol types (`FrameStamp`, `SurfaceGeneration`, `ClaimSlot`, `OwnerAffinity`) to `flui-runtime`/`flui-platform-api`; they sit in foundation only because of a CI gap (`affinity.rs:11-13`). Make `LayerId`/`SemanticsId` `GenId` and delete `ViewId`. Move `ChangeNotifier` onto `flui-state`. |
| flui-macros | **Keep, grow** | Add `#[derive(Catalog)]` (§5), `#[derive(RenderView)]` and `#[derive(Store)]`. Record the semantic edge to view/animation as an allowed proc-macro exception. |
| flui-tree | **Keep, give it a job** | The trait trio has no generic consumer (`grep` finds only impls). **Make `ElementTree` implement it and make the protocol's tree walker the consumer.** One inspector then walks the element, render, layer and semantics trees. Drop `bon`. |
| flui-scheduler | **Keep, shrink** | An owner-affine `!Send` core plus a `Send` waker. Drop the ~20-Mutex `SchedulerInner` (`scheduler.rs:743-855`). Move `AsyncDriver` to runtime. Move `TIME_DILATION` (`config.rs:43`) into the presentation clock. |
| flui-painting | **Keep** | Recorder plus a `TextService` trait. Remove `FONT_SYSTEM` together with the Parley migration (per-realm `LayoutContext` over a shared fontique `Collection { shared: true }`). Stop re-exporting `cosmic_text::fontdb::Family` (`lib.rs:87`). |
| flui-platform | **Split** → `flui-platform-api` (T1) + `flui-platform` (backends, T6) | See §2.2. Remove the no-op `desktop=[winit]` default (`Cargo.toml:300-303`), `LinuxPlatform`, `src/window.rs` duplicate `Window`, `PlatformEmbedder`, `PlatformCapabilities`. |
| flui-interaction | **Keep** | Depend on platform-api. Move `InteractionLane`'s generic closure registry (paint clip/shader targets, `interaction_lane.rs:158-190`) to runtime as an explicit owner-lane handle. Give intents stable names (a command registry the agent can list). |
| flui-assets | **Keep, de-runtime** | Take an injected IO spawner. Delete `AssetRegistry::global()` (`registry/mod.rs:83`) and the bridge runtime (`bridge.rs:42-66`). Absorb the widget decode cache. |
| flui-log | **Keep** | Clean already. It should also export `tracing` spans as protocol frame events (OTel-shaped). |
| flui-layer | **Keep, grow** | Becomes the home of the **raster contract**: layer walk, `LayerRender`, `CommandRenderer`, `LayerStateStack` (today `pub(crate)` in engine with 0 wgpu refs), damage diff, `DamageRegion::Partial`, and a conformance suite every backend runs. |
| flui-semantics | **Keep, re-vocabulary** | Store AccessKit roles, actions and states natively (the protocol vocabulary by construction). Keep Flutter flags only as builders. Action handlers become owner-lane target IDs, not `Arc<dyn Fn+Send+Sync>` (`action.rs:217`). Delete or adopt `SemanticsSnapshot`. |
| flui-animation | **Keep** | One clock (the presentation `FrameClock`). `AnimationController` becomes `!Send` (`controller.rs:177-180` records the pending flip). |
| flui-rendering | **Keep** | Owns topology through a transaction API. `render_tree_mut` becomes private (26 flui-view call sites). Move catalog parent data, delegates and `ScrollPosition` up. Ship `conformance::check_box/check_sliver` as a library, not a 15.9k-line test string list. |
| flui-objects | **Keep** | The catalog. Build-during-layout cells move down to rendering (`layout_constraints_cell.rs:31-35`). Keep the typed `RENDER_OBJECT_TYPES` registry as a machine-readable list. |
| flui-engine | **Keep, shrink surface** | wgpu backend of the raster contract. `GpuContext` shared across windows plus a `Presentation` per window. Stop `pub use ::wgpu` (`lib.rs:229`). Make `WgpuPainter` and `raster_owner` crate-private. External textures via a realm capability. |
| flui-view | **Keep, curate** | Hand the graph over to flui-state and the frame driver (`WidgetsBinding`) over to runtime. Open an element protocol, or move layout-builder, lazy-sliver and async builders to widgets, which drops the view→objects and view→animation edges. Split the authoring and `__runtime` surfaces. Delete `ElementBuildContext`. |
| flui-widgets | **Keep, one crate** | Honour the 2026-09-23 decision, but **implement the module-direction gate**. Add the Raw primitives (RawButton, RawToggle, Surface, ink). Gain the root-scope widgets flui-app now imports. Hosts the catalog registry module. Delete `__private`. |
| flui-material | **Package** (`packages/`, own train, later own repo) | It depends on 9 internal crates (`Cargo.toml` of flui-material). It may depend on `flui` (the SDK surface) only, which xtask should enforce. Remove it from the facade default and the prelude. |
| flui-cupertino | **Package** | Same. Rebuild on the Raw primitives, which also closes the keyboard/a11y parity gap (`button.rs:34-43`). |
| flui-localizations | **Delete (merge)** | 281 lines and a whole layer. Fold the RTL table into widgets' localization contract. Each design package owns its strings (Flutter 3.47 did this). ICU4X becomes the H1 `flui-i18n` package. |
| flui-testing | **Move above widgets** (T5, after runtime) | Drives the *real* frame transaction from runtime. Holds `WidgetTester` with finders over the protocol's query model (`by_role`, `by_label`, `by_catalog_name`), semantic and pixel goldens, and replay. widgets drops its normal edge to testing (`flui-widgets/Cargo.toml:89`). |
| flui-app | **Keep, shrink to runners** | About 40k lines today; target under 15k. It keeps runners, window/surface/device recovery and policy wiring. Realm, frame, lanes, services, semantics host and held input move to runtime or their subsystems. Delete the stub `embedder` and the empty features. |
| flui-cli | **Keep, independent version** | Gains `flui mcp`, `flui devtools` (client), `flui test --golden/--accept` with per-test NDJSON, `flui catalog`. Delete or make real `analyze`/`format`: they are thin wrappers, and `--json` drops their output (`runner.rs:166-168`). Absorbs `tools/web-server`. |
| flui-devtools | **Delete (merge)** | Zero production consumers. Its profiler and timeline become protocol streams served by the runtime inspector. A DevTools UI, if one is ever written, is a protocol client package. |
| flui-hot-reload | **Delete** after the Subsecond spike | It uses the dlopen/ABI design with documented residual unsoundness (`lib.rs:1-40`), plus 3 facade features and 5 workspace members. It is replaced by a `DevReloadHook` in runtime plus `flui run --hot`. |
| *tools/desktop-mcp* | **Promote to `packages/flui-mcp`** (lib + bin) | The OS-level backend (UIA now, AX/AT-SPI next). It also becomes the driver library that `xtask device` and live-smoke call, which retires the duplicate hand-rolled COM UIA in `tools/xtask/src/device/uia.rs` and the 11 Python and 4 Swift scripts. |

### 2.4 Module boundaries inside big crates

- **flui-widgets:** a declared DAG in manifest metadata (`[package.metadata.flui.modules]`): `base = {layout, paint, flex, stack}` → `interaction` → `{text, scroll}` → `overlay` → `navigator` → `app`. xtask parses `use crate::X`/`super::` edges. Today there is almost no coupling (1 cross-import among text/scroll/navigator/overlay/image), so it is cheap to lock now.
- **flui-engine:** after the raster contract moves down, only wgpu-specific modules remain. Move test suites under `src/tests/`: about 52% of its 72k lines are tests, so "74k non-test LOC" misreads it.
- **flui-view:** `authoring` (re-exported), `__runtime` (`#[doc(hidden)]`, for runtime and testing), `element` (internal).
- **flui-runtime:** `realm`, `frame`, `lanes`, `exec`, `capability`, `inspector` (feature), `reload` (feature).

### 2.5 Feature-flag policy

1. Features are additive. Every optional dependency is behind `dep:`. **No empty features.** Currently empty: flui-app `desktop/android/ios/web/debug-overlay/performance-overlay`, platform `web/wayland/x11`, geometry `mint`.
2. **One `unstable` feature** per published crate replaces `runtime-internals`, the 9 `testing` features and `experimental-delegates`. Internal composition goes through `#[doc(hidden)] __runtime` modules, not features. flui-app switches `runtime-internals` on in every production graph (`flui-app/Cargo.toml:90`), so "internal" is fiction today.
3. `inspector` and `hot` are dev-only features on runtime. They are on by default in `flui run` debug builds and compile-time absent in release, so they cannot be switched on in a release build (§7).
4. Platform backend choice happens at run time or by target, not through widget-crate features.
5. The facade feature matrix (`cargo xtask facade-combos`) is extended with the reach facts.

### 2.6 Facade and prelude

- `flui::prelude` is an **explicit, enumerated list**, not a glob of `flui_widgets::prelude::*` (which leaks `BuildOwner`, `ElementTree` and `tracing::info!` into user scope, `flui-view/src/lib.rs:252,278-285`). It is catalog-neutral: **no Material**. `flui_material::prelude` is globbed alongside by apps that want it.
- Curated modules replace `pub use flui_view as view` and friends (`src/lib.rs:120-152`): `view`, `widgets`, `rendering` (the Stable authoring tier, grown to cover slivers, `ViewportOffset` and `LayerLink`), `painting`, `state`, `platform` (capabilities), `testing`, and `sdk` (what a design system needs: `Surface`, `RenderView` authoring, post-frame handles).
- `cargo public-api` snapshot of the facade, committed, as a `checks` step now. `cargo-semver-checks` is advisory from H1 and gating from H3.
- `default = []`. The CLI template adds `material` explicitly.

### 2.7 Publish order and train

Core train, in `=` lockstep via `[workspace.dependencies]` (there are 172 hardcoded `=0.2.0-dev` pins today):

geometry → types → foundation → macros → platform-api → protocol → state → tree → scheduler → painting → interaction → assets → log → layer → semantics → animation → rendering → objects → engine → view → widgets → runtime → testing → platform → app → flui.

Packages (material, cupertino, mcp, a2ui, i18n) use caret requirements on `flui` and follow a faster cadence, as Flutter now does with weekly package releases against a quarterly core. Use `cargo publish --workspace` (Rust 1.90) behind a `cargo xtask release-check` dry run. flui-cli is versioned independently: it has no framework dependencies.

---

## 3. Runtime model

**Trees and identity.** View → Element → Render → Layer, plus Semantics. **Every node in every tree has a generational, realm-scoped ID** (`GenId<M>`): `ElementId` (bespoke today, `id.rs:1163`), `RenderId` (done), `LayerId` and `SemanticsId` (plain reusable slab indices today, which is ABA-unsafe for damage caches and agent handles). A single `NodeRef { realm, tree, id }` is the protocol's addressing unit. Agent handles (`e12`) map onto it through a backend-owned handle table, as MCP 2026-07-28 requires (no protocol session).

**Realms.** Keep ADR-0027's single-writer `!Send` realm. Record the truth: N realms are serialised on one owner thread (`runner/host.rs:25-47`, `thread_local! APP_RUNTIME`). Either amend the verdict or schedule owner threads before H2; do not keep citing concurrency. Everything mutable is a realm resource: signal graph, text layout context, image cache, GlobalKey scope, capability registry, error-view builder, time dilation, ID allocators. **Signals move to the realm, not the presentation.** Today `Reactive` lives in `BuildOwner`, one per presentation (`build_owner.rs:444,722`), so `SignalWrite` always targets the primary window (`ui_realm/commands.rs:450-455`).

**Frame transaction (one copy, in runtime):** input → effects (a flui-state phase, run-to-completion like GPUI) → build → layout (with the lazy-child fixpoint) → paint (retained layers with stable IDs) → semantics → **layer diff → damage** → `SceneSnapshot` → raster lane. Every phase emits protocol telemetry events. The headless binding is the same transaction with a `ManualClock` and a headless sink.

**Scheduling and lanes.** One demand authority per presentation (`FrameClock::mark_demand(reason)`), replacing the five demand mechanisms. The raster lane is mode-agnostic: Inline stays permanent for macOS and wasm, and Threaded is used on Win32 and Linux once text no longer shares a mutex with glyph rasterisation. There is one execution owner. A `Spawner` capability is vended via `LifecycleContext` with cancel-on-unmount, and tokio sits behind a feature of the default executor. flui-platform and flui-assets stop building runtimes; up to four exist today.

**State.** `flui-state` is the canonical layer: Copy handles, `Computed`/`Effect` (ADR-0075), `#[derive(Store)]` with path-keyed triggers tied to reconciler keys (A8, the Dioxus/Leptos Stores pattern), and a `MaybeSignal<T>` prop type for the catalog. The **write journal** (slot, writer element, frame) is exposed to the inspector. That brings back iced's replayable-message property without Elm ceremony (**hypothesis**: enough for time-travel of app state). setState, `StateCell` and `ValueNotifier` stay as documented low-level tools and leave the prelude.

**Rendering and text.** Layer tree with stable per-boundary identity (`LayerNode.render_id` already exists, `flui-layer/src/tree/layer_tree.rs:38`). ADR-0061's damage diff is implemented in the raster contract. Text: Parley per realm, a shared read-only font collection, a raster-owned glyph cache keyed by font-blob identity (not the process-scoped `GlyphKey`, `glyphs.rs:23`), and ICU4X as the single Unicode source.

**Platform.** The `platform-api` contract is used by `platform` backends, with **one backend per OS** recorded in an ADR. IME becomes a **pull-model text-store contract** (text in range, selection, rect for range, index for point), because the push-only 2-method trait (`traits/text_input.rs:24-45`) cannot serve TSF, NSTextInputClient, UITextInput or InputConnection. Windows gets TSF, not IMM32; the shipping Win32 backend has no IME today.

**Accessibility, which is also the agent substrate.** AccessKit Windows and macOS adapters become unconditional. Linux AT-SPI is default-on with an opt-out. Android and iOS adapters are H1 items. **Semantics can be enabled by the realm, not only by an OS assistive tool.** `SemanticsHandle`/`ensure_semantics` are `#[expect(dead_code)]` today (`semantics_host.rs:30-50`), so an in-process agent cannot see the tree unless Narrator is running. Use AccessKit `tree_id` (0.25.1) for multi-window and embedded foreign content.

---

## 4. Extension points and plugin model (H1–H4)

The design rule: **every extension point is registered as data, discoverable through the protocol, and absent-as-typed-`Unsupported`.**

1. **PlatformCapability (H1).** One generic method on the sealed `LifecycleContext` keeps ADR-0078's build/lifecycle split while making the set of capabilities open. Registration happens at app build time and is realm-scoped. Implementations follow the federated-plugin split: an interface crate over `flui-platform-api`, per-target implementation crates, a default implementation pulled in by Cargo feature, and an override hook from day one (Flutter still lacks one, flutter#80374). **Every registered capability appears in the protocol's `capabilities` listing,** so agents can ask "can this app pick a file?" Clipboard is the first built-in capability to go through the seam; its accessor is dead code today (`flui-app/src/app/runtime.rs:1636`).

2. **Third-party render objects (H0).** Authoring through `flui::rendering`, with a `#[derive(RenderView)]` instead of `impl_render_view!`, the published `conformance` kit, and a `harness` badge the community catalog can check mechanically (pub-points style, no committee).

3. **External GPU content (H1 spike, H2).** A `Presenter` abstraction shaped like Subduction's: the layer tree lowers to surfaces plus a native compositor tree, with the single-swapchain wgpu presenter as the fallback. A `TextureRegistry` capability feeds a `Texture` widget. Delete `PlatformViewLayer` until then; its render is a no-op (`layer_render.rs:381-384`).

4. **Themes as data (H1).** A serde token substrate below the design systems, `ThemeData::from_tokens`, and `WidgetStateProperty` in data form (a per-state map) with closures kept as the escape hatch. Tokens are catalog entries too, so an A2UI model can pick a theme.

5. **Agent protocol (H0, frozen in H3).** `flui-protocol` + runtime inspector + `flui-mcp`. MCP extensions (H4) are new protocol methods, not new vocabularies.

6. **A2UI (H1).** `packages/flui-a2ui` is a surface controller that interprets `updateComponents` against the **same catalog registry**, binds `updateDataModel` JSON Pointers onto flui-state Store paths, resolves named functions from the catalog, and exposes a transport adapter trait with no LLM client in core (Flutter GenUI had to undo exactly that coupling in May 2026). It is in the Evolving tier, because A2UI renamed core properties at v0.9 within five months.

---

## 5. Public API and DX sketches

**The catalog as data (G6, the A2UI catalog, llms.txt, the prompt text, preview registry):** generated on stable Rust from a derive. rustdoc JSON is nightly-only and changes format, so it is only an optional enrichment.

```rust
/// A labelled checkbox.
#[derive(Clone, StatelessView, Catalog)]
#[catalog(name = "flui.Checkbox", role = CheckBox, since = "0.2", tier = Stable)]
pub struct Checkbox {
    #[prop(doc = "Checked state")]            pub value: MaybeSignal<bool>,
    #[prop(action = "flui.checkbox.toggle")]  pub on_changed: Action<bool>, // id + optional closure
    #[prop(default)]                          pub label: Option<Text>,
}
// generated: impl CatalogEntry { const NAME; fn schema() -> JsonSchema; fn examples() -> &[Example];
//            fn from_value(&serde_json::Value, &FunctionRegistry) -> Result<BoxedView, CatalogError> }
```

`const NAME` is also the element's stable identity in inspector outlines (it replaces `TypeId`). `examples()` compile as tests and double as headless previews (the Xcode `RenderPreview` / Compose preview equivalent).

**Capabilities:**

```rust
pub trait PlatformCapability: 'static { type Handle: Clone; }
impl dyn LifecycleContext + '_ {
    pub fn capability<C: PlatformCapability>(&self) -> Result<C::Handle, Unsupported>;
}
App::new(Notes).capability(flui_clipboard::endorsed()).capability(my_camera::Camera).run();
```

**One query model for tests and agents:**

```rust
let mut t = flui::testing::WidgetTester::new(Notes::sample());
t.find(Query::role(Role::TextInput).label("Title")).set_text("Hi")?;   // semantic action, not a pointer
t.find(Query::catalog("flui.Checkbox").nth(0)).act(Action::Toggle)?;
t.pump();
t.assert_semantics_golden("notes_after_edit");   // deterministic outline, one node per line
t.assert_pixels_golden("notes_after_edit");      // CPU reference + bundled font, opt-in
```

`Query`, `Action` and the outline format are `flui-protocol` types. `flui mcp`'s `find`/`act`/`snapshot` tools serialise the same values, so **a failing agent scenario and a failing test are the same artifact**.

**The in-process inspector (the G1 server):**

```rust
// flui-runtime, feature "inspector" (absent in release)
pub trait RealmObserver { fn on_frame(&mut self, f: &FrameReport); fn on_journal(&mut self, w: &WriteEvent); }
pub enum Request { Tree { tree: TreeKind, scope: Option<NodeRef>, max_depth: u16, max_nodes: u32, format: Format },
                   Act { target: Handle, action: protocol::Action }, Catalog { filter: String },
                   Trace { frames: u32 }, Replay(ReplayLog), Capabilities, Diagnostics { since: FrameId } }
```

Every read is bounded (scope, depth, max nodes, `truncated`), following Anthropic's tool guidance. `DiagnosticsNode::to_string_deep` replies (tests/agent_workflow.rs) are unbounded and are retired. Every `Act` returns the post-action outline diff, as Playwright MCP does.

**Other DX changes:**
- `run_app(impl View)` for any view kind; today a `StatefulView` root needs a wrapper (`runner/mod.rs:214`).
- `View for Option<V>` and an `Either`.
- UI callbacks drop the `Send+Sync` bound. Today `PageView::on_page_changed` and the `Draggable` callbacks reject `StateCell`, which contradicts ADR-0027:106.

**For AI contributors in the repo:**
- a generated concept→module map in AGENTS.md;
- the generated `docs/crates.md` and llms.txt, which today teaches `flui_app::run_app` rather than the facade;
- ADR front matter plus a generated index;
- `flui create` ships an AGENTS.md with a compressed catalog index. Vercel measured passive context at 100% against 79% for on-demand skills.

---

## 6. Performance model

The contract has a cost, and it has to be budgeted:

- **Semantics always-on in debug.** Measure the `publish_cost` bench on the Notes app and on a 100k virtualised list before choosing the default. The published mirror keeps a full clone of every `accesskit::Node` (`owner.rs:185-270`); if memory matters, switch to hash-per-node diffing.
- **Telemetry is counts first.** Deterministic counters (elements built, layout roots, layers, damage area, bytes uploaded) gate every PR because they are noise-free. Wall time is nightly, per OS. `bench-collect` must stop skipping the `required-features` benches (`tools/xtask/src/bench.rs:36` skips the damage baseline).
- **Structural fixes the agent protocol also depends on:** stable layer IDs (a damage diff *and* stable agent handles into the layer tree); local topology commits instead of the global sync (`element_tree.rs:1428-1520`); disjoint arena indexing instead of whole-slab scans (`storage/tree.rs:82-110`); `Rc`-shared view configs instead of `dyn_clone` deep copies (`into_view.rs:178-184`); per-phase signal readers.
- **The protocol's hot path is off the frame path.** The inspector reads snapshots produced at the semantics and layer phase boundaries and never takes a lock inside `perform_layout` or `paint`.
- **Cold start.** Measure the phase split, then parallelise the font scan and adapter creation and use `wgpu::PipelineCache`. The H2 < 300 ms target has no baseline today.

---

## 7. Safety model

1. **The inspector is a remote-control surface.** It is compile-time absent from release builds (a Cargo feature *and* a `cfg(debug_assertions)` guard, not only an environment variable). Transport is a Windows named pipe or Unix socket with a per-launch token; `flui mcp` speaks stdio. This avoids egui_inspection's unauthenticated TCP port and the MCP SDK DNS-rebinding advisories.
2. **Globals ratchet** (`cargo xtask globals`, in `checks`): a checked-in allowlist of statics and thread-locals with reason and horizon. Burn down FONT_SYSTEM, the decode `CACHE` (`decode_cache.rs:96`), `ERROR_VIEW_BUILDER`, `TIME_DILATION`, `AssetRegistry::global`, the navigator, lane and GlobalKey thread-locals, and the ID counters that reach the protocol.
3. **Finish the !Send flip** before H3: `Listenable`, `Animation`, `CustomPainter`, the delegates, `ScrollPhysics`, `ViewKey`, `HitTestTarget`. That removes the `Arc<Mutex>` controllers, the per-node layout Mutexes and `unsafe impl Send for ObjectKey`.
4. **Unsafe:** per-backend ledgers with a ratchet, `undocumented_unsafe_blocks` enabled per module once annotated, Miri on the arena island (already done), and live-run evidence required for PRs that touch unexecuted backends. The protocol's desktop driver becomes the one live-run tool.
5. **Panics:** production `unwrap` is already 0. Add the BUG:-prefix lint and wrap hit-test, intrinsics and semantics calls in the same `guarded_call`, so a third-party render object cannot take down the agent's view of the tree.

---

## 8. Delete, merge, or replace with ecosystem crates

| Replace / delete | With | Why |
|---|---|---|
| dlopen hot reload (`flui-hot-reload`, 3-crate template, `run.rs` 2.5k lines) | Subsecond | Iced and Dioxus converged on it. The contract: logic edits keep state, State-type edits restart the realm. |
| cosmic-text, `FONT_SYSTEM`, `unicode-segmentation` | Parley/fontique/HarfRust/ICU4X; evaluate **glifo** for atlas rasterisation | The spike should test glifo/Skrifa, since Swash is no longer in Parley's path. |
| Hand-copied AccessKit roles in desktop-mcp | `accesskit::Role` via flui-protocol | Removes one of the four vocabularies. |
| Hand-rolled COM UIA in xtask, Python/Swift device checks | the `flui-mcp` driver library (uiautomation, objc2 AX, AT-SPI) | One driver, Rust only (principle 2 and the owner's tooling memory). |
| `tools/web-server` (axum + wasm-pack) | `flui run --device browser` | Two web servers today. |
| Bespoke frame-demand logic (5 carriers) | frameclock-style demand classes (or the crate itself) | Maps to #1172 and ADR-0058. |
| CPU reference renderer (build our own) | vello_cpu or tiny-skia behind the raster contract | The authors call vello_cpu mature; pin its SIMD level for deterministic goldens. |
| flui-devtools, flui-localizations | merged (§2.3) | No independent consumer. |
| egui_inspection-compatible wire? | **Consider co-designing**: same GetTree/HandleEvents/Screenshot shape over AccessKit | kittest-style generic inspectors would then work with FLUI unchanged (**hypothesis**; needs a compatibility read of its MessagePack schema). |

---

## 9. Breaking changes to make now (pre-publish, cheapest moment)

1. Split `flui-platform` into api and backends; drop the `interaction→platform` edge.
2. Create `flui-protocol` and move the ADR-0080 types into it. desktop-mcp becomes a client.
3. Create `flui-runtime` from flui-app; flui-testing drives it; supersede the ADR-0041 gate.
4. Move the signal graph to `flui-state`, owned by the realm; supersede ADR-0074's placement.
5. Generational `LayerId`/`SemanticsId`/`ElementId` as `GenId`; realm-scoped ID allocators; fix the AGENTS.md ID-offset row (it describes 1-based `NonZeroUsize`, which compile-fail doctests now forbid).
6. `#[derive(Catalog)]` with a stable `NAME` on every catalog View; `TreeObserver` events carry the name.
7. An explicit prelude, curated facade modules, no Material default, and a public-API snapshot gate.
8. Drop `Send+Sync` from UI-side traits and callbacks.
9. IME pull-model contract ADR (TSF-first on Windows) together with the Parley ADR.
10. Delete flui-devtools, flui-localizations, flui-hot-reload (after the spike), `__private`, `ElementBuildContext`, the duplicate types, the empty features, and `src/window.rs`.
11. Supersede the engine's "nothing pluggable" stance with "one raster contract, several rasterisers".
12. Restore the gates: globals ratchet, module direction, duplicate pub type names, ADR front matter, process markers (34+ remain, e.g. `Cargo.toml:8-38`).

---

## 10. Evolution H0 → H4 without rewrites

- **H0 (beta).**
  - Topology: protocol, platform-api, runtime and state extracted; testing moved up.
  - Contract: `flui mcp` and `flui test` share `Query`/`Action`/outline; realm-enabled semantics; raster contract in flui-layer with the CPU reference passing the conformance suite; catalog derive on Raw primitives; structured evidence; the globals ratchet at a fixed count.
  - Exit additions: an agent scenario runs in CI on windows-latest against both the in-process and the UIA backend with identical outlines. Today the protocol never executes in CI (`ci.yml:638-643`, Linux-only tests).
- **H1.** PlatformCapability registry (clipboard, haptics and dialogs ported first); `flui-a2ui` interprets the same catalog registry; token themes; AccessKit Android/iOS; Presenter spike. The capability and catalog listings appear in the protocol, so an A2UI model negotiates catalogs from the same data an agent reads. No new vocabulary is needed.
- **H2.** Threaded raster lane, damage producer (already enabled by stable layer IDs), a Store-backed 100k list, a shared `GpuContext`, and a software fallback that is the CPU reference promoted. Performance evidence is protocol telemetry, so agents can run perf loops the way Chrome DevTools MCP does.
- **H3.** Freeze tiers. Stable: prelude, `flui::rendering`, state, Router, catalog metadata, **protocol v1** (versioned apart from the AccessKit crate version). Evolving: packages, A2UI adapter. Experimental: `unstable`. Gate with semver-checks on the facade and protocol. Material and Cupertino move to their own repos by moving the directory, since they already build against `flui` only.
- **H4.** Community crates declare `Catalog` entries and capabilities, so they are automatically agent-visible and A2UI-usable. MCP extensions become protocol methods. Embedded and kiosk builds use runtime with a host-owned loop (the platform-api embedder seam).

Nothing here requires a second rewrite. The costly structural moves (contract crates, IDs, runtime extraction) all happen while there are zero consumers.

---

## 11. Risks and deliberate non-goals

**Risks**
- *Extraction churn on a bus-factor-1 project.* Mitigation: each extraction is one PR series with a reach fact that proves it and a WIP limit of 2. The protocol crate comes first because it unblocks H0's `flui mcp` exit.
- *Protocol ossification.* Freeze only the catalog metadata and the AccessKit-derived vocabulary. Keep A2UI, MCP extensions and the transport in the Evolving tier.
- *Semantics cost when always-on.* Measure before choosing the default; debug-only is the fallback.
- *Catalog derive burden across about 1.3k widget items.* Start with Raw primitives and Material components used by Notes; the remaining widgets stay in the registry.
- *Hypotheses I have not verified:* whether the Subsecond vtable behaviour needs element recreation; whether vello_cpu is deterministic across CPUs; whether egui_inspection wire compatibility is feasible; the GlobalKey deadlock under `draw_frame_impl`'s write lock (`binding.rs:1240`).
- *crates.io name availability for `flui-*`* is unverified (the cratesio MCP failed to connect).

**Non-goals**
- An in-app MCP server (the Slint shape): MCP stays in `flui mcp` and packages.
- A UI DSL or markup for AI generation (the Makepad Splash shape): typed catalog plus A2UI, measured by agent success rate rather than asserted.
- MCP Apps (an HTML iframe) as the generative runtime; at most a web-build interop.
- Screenshots as the primary agent channel; they are for visual checks only.
- Pixel parity across OSes in production; goldens are deterministic on the CPU reference only.
- Splitting flui-widgets into crates (the 2026-09-23 decision stands, with a real gate).
- Separate repos for packages before they have their own release cadence.

---

**Files relevant to this review (read, not modified):**
- `C:\Users\vanya\AppData\Local\Temp\claude\D--flui\bbb28042-f972-4a10-8e94-731819e26161\scratchpad\context.md`, `plan.md`, `roadmap.md`
- `D:\flui\tools\desktop-mcp\Cargo.toml`, `D:\flui\tools\desktop-mcp\src\a11y\{mod.rs,role.rs}`
- `D:\flui\crates\flui-semantics\Cargo.toml`, `D:\flui\crates\flui-devtools\Cargo.toml`
- `D:\flui\Cargo.toml`, `D:\flui\src\lib.rs`
