# Codebase map: plan_alignment — plan.md (principles, H0-H4, delivery layers, extension points, trajectory, governance) and roadmap.md (A1-A5, tracks A-H, B0-B4) compared with the code at main cab06137d

_Raw output of the `map:plan_alignment` agent (read-only review of main @ cab06137d, 2026-09-25). Unedited; claims are the agent's and were only partly re-verified._

## Summary

The plan is ahead of the code, but the gap is uneven. The inner runtime core has the seams the plan needs, and some of them are well built. Realms are single-writer (ADR-0027). The raster lane uses a mailbox protocol (crates/flui-app/src/app/raster_lane.rs), though it only runs inline. Realm-scoped signals exist behind a feature. Field-masked inherited dependencies landed (#1090). Semantics speak AccessKit end to end: flui-semantics, flui-platform and the flui-testing A11yTree all use it. Text is walled into flui-painting and flui-engine (cosmic_text appears in 6 files across 2 crates), so the Parley swap is cheap.

What the plan calls "extension points" and "delivery layers" has almost no seam in code.

1) PlatformCapability. Nothing by that name exists. Capabilities are a closed set of methods on `Platform` (crates/flui-platform/src/traits/platform.rs:253-571) and on the closed `LifecycleContext` trait (crates/flui-view/src/context/build_context.rs:360-~420). Adding one capability touches 5 core crates (platform, interaction, view, app, widgets). The only non-core precedent, haptics, has no production caller (crates/flui-app/src/app/presentation.rs:866-893).

2) Stable RenderBox/RenderSliver protocol for third parties. The trait itself is context-based and tidy (crates/flui-rendering/src/traits/render_box.rs:83). But flui-rendering pulls flui-platform through flui-interaction. `cargo tree -i flui-platform -p flui-rendering` shows the chain, and on Windows it brings in winit, windows, tokio and accesskit. The conformance harness is a 15,915-line test file inside flui-objects, not a library.

3) Stability tiers. There is no mechanism. The facade re-exports every crate wholesale (src/lib.rs:126-152). The `runtime-internals` Cargo feature is switched on by flui-app, so feature unification exposes it to every facade consumer. Nothing runs semver-checks and there is no `unstable` feature. Material imports 9 internal crates directly, so it cannot become a separate-repo "official package" until a curated stable surface exists.

4) Agent protocol. ADR-0080 fixes the wire contract, but it lives inside a binary tool (tools/desktop-mcp). The layer rule forbids crates from depending on tools. flui-devtools has no production consumer and, by its own docs, "opens no port". flui-cli has no mcp, devtools or golden commands.

5) Items that do not exist at all:
- A2UI and the G6 catalog: no view registry, no schema, and props carry closures.
- Themes as data: no serde in flui-material, and `WidgetStateProperty::Resolver` is an `Arc<dyn Fn>`.
- Router: the imperative Navigator is 27k lines, and the deep-link entry points have no production callers.
- CPU reference renderer: none.
- IO lane reachable from libraries: the execution services are app-private, so flui-assets and flui-widgets run their own runtimes and globals.
- External GPU content: TextureLayer and ExternalTextureRegistry exist, but no render object emits the layer and nothing outside the engine can reach the registry.

6) Principle 3 (no global state) lost its gate. The runtime-contract ratchet was deleted in cf46dfe20 with the note "those rules are types and clippy lints now", but no lint or type forbids a new `static`. Globals remain: FONT_SYSTEM, the image decode CACHE, AssetRegistry::global, and the Navigator thread_local.

7) Hot reload is a dlopen crate, while the plan decided on Subsecond.

8) The B0 structural exit is not met: crates/flui-app/src/app/runner/realm_dispatch.rs is 7,149 lines.

Overall: H0's developer loop is mostly blocked by missing features (Router, Form, multiline editing, the devtools protocol, the CPU renderer), not by bad structure. H1 (plugins, A2UI, themes as data) and H3 (freeze by tier, official packages in separate repos) are blocked structurally. They need a platform-API crate split out below the render machine, an open typed capability lookup, and a curated SDK surface. The cheapest time to build these is before the catalog grows further.

## Responsibilities and boundaries

Plan to code ownership as it is today:

- **Runtime (realm, lanes):** flui-app (L9) owns UiRealm, raster_lane.rs, ExecutionServices (execution.rs) and ServiceDefinition (lifecycle.rs:955). flui-scheduler owns AsyncDriver. The boundary leaks because the execution pools are app-private: "library crates cannot reach these pools" (execution.rs header). So L2 flui-assets builds its own tokio runtime (registry/mod.rs:52-115) and flui-widgets keeps a process-global decode CACHE (image/decode_cache.rs:96).
- **Text:** flui-painting owns shaping and FONT_SYSTEM (text_layout/layout.rs:124); flui-engine owns the glyph atlas (ADR-0067). The boundary is clean, and only `fontdb::Family` is re-exported (painting/lib.rs:87). The realm owns no text resource.
- **Platform capabilities:** the traits live in flui-platform (L2), the handles in flui-interaction (L2), the acquisition in flui-view LifecycleContext (L5), the wiring in flui-app (L9), and use in flui-widgets. One capability therefore spans 5 crates, and none of them is a plugin seam. flui-interaction depends on flui-platform only for `traits::PlatformTextInput` (3 uses), yet that single edge puts every OS backend under flui-rendering.
- **Render protocol:** flui-rendering owns RenderBox/RenderSliver, sealed Protocol and PipelinePhase, and the `testing::RenderTester` harness. flui-objects owns the catalog and the RENDER_OBJECT_TYPES registry, which lives in a test file. flui-view (the spine, L5) depends on flui-objects (the concrete catalog) for RenderSizedBox, the sliver lists and so on, so the spine is not catalog-neutral.
- **Design systems:** flui-material and flui-cupertino (L7) are in-repo, and the facade enables material by default (Cargo.toml `default = ["material"]`). ADR-0042 forbids a universal theme type, so no token layer exists below L7.
- **Agent protocol:** the wire contract lives in tools/desktop-mcp, a binary outside the layer system. flui-devtools (L9) is profiling and counters only. flui-testing owns the A11yTree query API over accesskit TreeUpdate. The in-process backend that ADR-0080 names has no home crate.
- **Navigation:** flui-widgets/src/navigator (27,457 lines) owns the imperative Navigator, Hero, modal routes and named routes (ADR-0024 Deprecated). flui-view/binding.rs owns RouteInformation and async handle_push_route/handle_pop_route, neither of which has a production caller.
- **Stability:** none of this is owned anywhere. The facade re-exports every crate, `__private` in flui-widgets serves the sibling crates (scrolling, navigation, text editing) that were rejected on 2026-09-23, and the `runtime-internals` feature is a Cargo feature that unification exposes to all consumers.

## Key types and contracts

- flui_platform::traits::Platform (platform.rs:253): closed trait; capability methods clipboard/data_transfer/open_url/prompt_for_paths/on_open_urls are fixed; adding camera/geo/notifications means editing it
- flui_platform::traits::PlatformCapabilities (capabilities.rs): boolean feature table (supports_touch, default_target_fps…); no production reader of Platform::capabilities(); name collides with the planned PlatformCapability
- flui_view::LifecycleContext (context/build_context.rs:360): closed set rebuild_handle/async_driver/post_frame_handle/text_input_handle/hit_test_handle/keep_alive_*/focus_manager/pipeline_owner; no typed `capability::<C>() -> Result<_, Unsupported>`
- flui_rendering::RenderBox (traits/render_box.rs:83): Arity + ParentData associated types, perform_layout(ctx)->Size, hit_test(ctx); Protocol and PipelinePhase sealed (protocol/protocol.rs:17, pipeline/phase.rs:101)
- flui_rendering::testing::RenderTester / LayoutRun / PaintRun / SemanticsRun (testing/harness.rs:108-908) behind feature `testing`: the only reusable third-party harness
- flui_engine::RasterBackend (raster.rs:100): Scene-level backend seam (render_scene, mark_dirty, mark_full_repaint); CommandRenderer is pub(crate) (command_renderer.rs:31)
- flui_layer::TextureLayer / PlatformViewLayer + flui_engine::ExternalTextureRegistry (external_texture_registry.rs:77), reachable only via WgpuPainter::external_texture_registry (painter/mod.rs:700)
- flui_app::app::ExecutionServices spawn_compute/spawn_io (lifecycle.rs:349,404), AppConfig::with_executors/with_service (config.rs:355,390): app-level, not reachable from L6
- flui_view::reactive::Signal<T> behind feature `signals` (off in facade default); FOUNDATIONS C1 still says setState is the 'sole canonical state model' (docs/FOUNDATIONS.md:91)
- flui_material::ThemeData (theme_data.rs:752): plain Rust struct, no serde; flui_widgets::WidgetStateProperty::Resolver(Arc<dyn Fn(&WidgetStates)->T>) (widget_state.rs:273)
- ADR-0080 wire contract: handles w3/e12/s2, AccessKit role names, typed replies, implemented only in tools/desktop-mcp
- [package.metadata.flui] layer + allowed-dependents checked by cargo xtask workspace (ADR-0041); layers 0..10 in root Cargo.toml

## Dependencies

Internal normal edges (from the manifests). Notable ones against the plan:

- rendering(L4) → interaction(L2) → platform(L2). Confirmed with `cargo tree -p flui-rendering -e normal -i flui-platform`. For the x86_64-pc-windows-msvc target the flui-rendering graph includes windows 0.62, winit 0.30, tokio 1.53 and accesskit.
- view(L5) → objects(L4): the spine depends on the concrete catalog.
- material(L7) → animation, foundation, interaction, objects, rendering, scheduler, types, view, widgets (9 crates).
- app(L9) → 15 flui crates, plus optional hot-reload.
- facade → re-exports animation, app, cupertino, foundation, geometry, hot_reload, localizations, material, types, view and widgets as whole modules.
- tokio is a non-optional native dependency in 3 library crates (flui-platform Cargo.toml:167, flui-app:136, flui-assets:31), each with its own runtime role.
- tools/desktop-mcp (rmcp, uiautomation, enigo, xcap) and tools/text-spike (not a workspace member; own Cargo.lock) sit outside the layer system, so nothing in crates/ can reuse them.
- Inbound dependency on the plan: every extension point (PlatformCapability, third-party render objects, external GPU content, themes as data, A2UI, the agent protocol) needs an edge from an out-of-repo crate into a small stable crate. Today the only such target is the whole facade or the 9-crate internal set.

## Fit with the plan

Item by item (✓ has a seam, ~ partial, ✗ missing or blocked):

**Principles**
- P1 mental model ✓: View, Element, Render, keys, constraints.
- P2 one toolchain, one renderer ~: wgpu is the one renderer, but PlatformViewLayer exists as an embedder-composited escape hatch whose render is a no-op (layer_render.rs:381).
- P3 no global state ✗: the gate was removed in cf46dfe20 and globals remain.
- P4 machine-readable ~: semantics use AccessKit and tracing is structured, but there is no widget catalog and no devtools protocol.
- P5 proof, not claims ✓: device checks and live smoke exist.
- P6 break explicitly ~: ADR Supersedes is used, but the C1 text contradicts ADR-0074.

**Horizons**
- H0 ~: realms, signals, masks, AccessKit and replay (flui-testing/src/replay.rs) exist. Missing: Router (D1), Form, multiline editor (the 4.7k-line single-line EditableText), `flui mcp` (G2), the in-process devtools protocol (G1), the CPU reference renderer (E7), per-realm FontSystem (B1), and Subsecond (G4, the code is dlopen). The B0 exit "no file >3000 lines in flui-app" fails (realm_dispatch.rs is 7,149 lines).
- H1 ✗ structurally: there is no capability plugin seam, no A2UI registry or schema, no themes-as-data serialization, and no external-texture producer.
- H2 ~: the raster-lane protocol exists but runs inline (ADR-0045 Proposed). The IO pools exist but are app-private. No damage producer (only mark_full_repaint is called: app/raster_lane.rs:486, direct.rs:181). Parallel layout inside a realm is effectively foreclosed, since render objects are deliberately `!Send` (c281cd1fc); that part is a hypothesis.
- H3 ✗: no tier mechanism, no semver-checks, and an unscoped public surface (~4.7k pub items across rendering, view, widgets, objects and interaction by a rough grep).
- H4: follows from H1 and H3.

**Delivery layers:** core vs official vs community is not reflected. Material is the facade default and in-repo, hot-reload and localizations are in-repo, and official packages would need the internal crates as their API.

**Extension points:** PlatformCapability ✗; RenderBox protocol ~ (good trait shape, but a heavy dependency graph and no exported conformance suite); external GPU content ~ (layer and registry, no widget, no device sharing); themes as data ✗; agent protocol ~ (contract accepted, but in a binary tool); A2UI ✗.

**Trajectory table**
- Runtime ~: tokio's role is still implicit, with 3 runtimes.
- Text ✓: seam ready for Parley; global still present.
- Rendering ~.
- State ~: two models, signals off by default, 0 Signal inputs in the catalog against 59 notifier uses.
- Navigation ✗.
- Platforms ~.
- Semantics ~: AccessKit data model everywhere, but adapters off by default (facade `a11y` feature), while the plan wants them on by default for beta.

**Governance:** horizons, extension points and tiers are not in the repo. docs/ROADMAP.md is 21 lines and covers B0-B4 only, and no ADR records the delivery-layer or tier decisions.

## Strengths

- The layer DAG is machine-checked from the manifests (cargo xtask workspace, ADR-0041), and ADR-0028 forbids core → design-system edges via allowed-dependents. It is a good base for adding a platform-API layer or an SDK-surface rule.
- The text stack is well contained: cosmic_text appears in only 6 files across flui-painting and flui-engine, and the only public leak is `fontdb::Family` (flui-painting/src/lib.rs:87). ADR-0065 has painting own shaping, so Parley and a per-realm FontContext are local changes.
- AccessKit is already the semantics lingua franca: flui-semantics, flui-platform and flui-testing's A11yTree (find_by_label, find(Role), invoke_semantics_action) all use it. That is the right vocabulary for ADR-0080 and for a future in-process agent backend.
- The realm model (ADR-0027) and the raster-lane mailbox protocol (raster_lane.rs: SceneSnapshot, FrameStamp, SurfaceGeneration) are designed so that threading the lane 'changes who calls pump, not what a frame is'.
- Capability acquisition is enforced by types (LifecycleContext, ADR-0078), and the IME capability (ADR-0030) is a clean typed-handle precedent that a plugin model can generalize.
- RenderBox is context-based: perform_layout(ctx)->Size, and hit_test uses the ctx rather than self-state; child arity is a type. That is a stable-able protocol shape for third parties, and flui_rendering::testing::RenderTester already exists behind the `testing` feature.
- Execution services classify work by deadline (frame, compute, IO), support host executor injection (AppConfig::with_executors) and have named service owners (ADR-0047/0049). The IO lane is half there.
- Field-granular inherited dependencies (FieldMask, #1090) and realm-scoped signals with a build-time write guard are exactly the substrate that themes-as-data and A2UI data binding will need.
- View is object-safe (Downcast + DynClone, BoxedView), so a runtime view registry for A2UI is feasible without redesigning the tree.
- Material and Cupertino are already isolated above the widget catalog, and ADR-0042 keeps theme selection in the design system. Moving them out-of-repo is blocked by API stability, not by tangled dependencies.

## Problems

### No PlatformCapability plugin seam: capabilities are a closed set spread across 5 core crates

- **Kind:** extension_point · **Severity:** critical
- **Evidence:** `Platform` is a closed trait with fixed capability methods: clipboard, data_transfer, open_url, prompt_for_paths, on_open_urls, capabilities (crates/flui-platform/src/traits/platform.rs:253-571). `LifecycleContext` is a closed trait with fixed methods (crates/flui-view/src/context/build_context.rs:360-~420), and there is no typed lookup. The IME capability touches flui-platform (PlatformTextInput), flui-interaction (TextInputHandle, text_input.rs:285), flui-view (5 files), flui-app (2 files) and flui-widgets. The only non-core capability precedent, haptics (ADR-0031), has no production caller: `#[expect(dead_code, reason = "no production caller yet -- haptics through a ...")]` (crates/flui-app/src/app/presentation.rs:866-893). `grep PlatformCapability` finds nothing in code. The existing `PlatformCapabilities` boolean table has no production reader.
- **Impact:** Blocks the H1 exit '5 plugins built outside the repo': an out-of-repo crate cannot add a method to Platform or LifecycleContext. Every new OS capability (camera, files, geo, notifications, menus F8, dialogs) must be added to core in 5 places, which contradicts 'a plugin is an ordinary crate with #[cfg(target_os)]; absence is a typed Unsupported'.
- **Direction:** Before F8 lands, add a typed, open capability registry. A `trait Capability: 'static { type Handle; }` sits in a small low-layer crate. Plugins register a per-platform provider at app build (`AppConfig::with_capability::<C>(provider)`), and `LifecycleContext::capability::<C>() -> Result<C::Handle, Unsupported>` resolves it per realm. Port clipboard, haptics and file dialogs onto it as the first three plugins to prove the seam, and record it in an ADR superseding ADR-0031's deferral. Rename or remove the unused PlatformCapabilities table to avoid the name clash.

### The render machine transitively depends on the OS backends (rendering → interaction → platform)

- **Kind:** layering · **Severity:** high
- **Evidence:** `cargo tree -p flui-rendering -e normal -i flui-platform` shows flui-platform ← flui-interaction ← flui-rendering. For the x86_64-pc-windows-msvc target the flui-rendering graph contains windows 0.62.2, winit 0.30.13, tokio 1.53.1 and accesskit 0.25. flui-interaction uses flui_platform only for `traits::PlatformTextInput` (3 hits) and declares it non-optionally (crates/flui-interaction/Cargo.toml:31). Separately, the spine flui-view (L5) depends on the concrete catalog flui-objects (crates/flui-view/Cargo.toml:35; it uses RenderSizedBox ×16, RenderSliverList/Grid, RenderLayoutBuilder).
- **Impact:** A third-party render-object crate (plan: 'a stable public RenderBox/RenderSliver protocol for third-party catalogs', H0) inherits every OS backend, ~340 unsafe sites and tokio. That makes wasm and embedded (H4) builds and compile times worse, and it widens the semver surface a render-object author is exposed to. The spine depending on one catalog also makes flui-objects privileged over third-party catalogs.
- **Direction:** Split the platform traits and value types (PlatformTextInput, events, key codes, capability traits) into a thin L1/L2 `flui-platform-api` crate with no OS dependencies, and keep backends in flui-platform, depended on only by flui-app. Invert view → objects: move the handful of render objects the spine needs (sized box, the layout-builder cell) down into flui-rendering, or make them spine-private. Add a gate that no crate below L9 has flui-platform in its normal graph.

### No mechanism for stability tiers: the whole workspace is the public API

- **Kind:** api_dx · **Severity:** high
- **Evidence:** The facade re-exports whole crates: `pub use flui_view as view`, flui_widgets, flui_app, flui_foundation, flui_types, flui_animation, flui_geometry, flui_hot_reload, flui_material (src/lib.rs:126-152). The `runtime-internals` feature, documented as 'Not an application-facing API contract' (crates/flui-view/Cargo.toml:116-118), is enabled by flui-app (Cargo.toml:90), flui-testing and flui-hot-reload, so Cargo feature unification exposes it to every facade consumer. There is no `unstable` or `experimental` tier feature besides flui-rendering's `experimental-delegates`, and no cargo-semver-checks anywhere (tools/xtask, workflows). flui-material imports 9 internal crates directly. A rough `pub fn|struct|enum|trait|type|const` count gives rendering 1264, widgets 1266, interaction 840, objects 787 and view 585.
- **Impact:** Blocks H3 ('API freeze by tier; semver-checks green 3 minors in a row'): every change to any of 27 crates is a potential breaking change, so the Stable/Evolving/Experimental split cannot be expressed. It also blocks the delivery-layer plan, because Material and Cupertino in separate repos would have to depend on internals.
- **Direction:** Define the SDK surface now. The facade exposes curated modules (prelude, view, widgets, rendering-protocol, testing) instead of whole crates. Internal crates stay publishable but are documented as unstable, and the facade is the only semver promise. Replace the `runtime-internals` Cargo feature with `#[doc(hidden)]` plus a sealed token type or a separate `-internals` crate that only composition roots depend on. Add cargo-semver-checks on the facade and the protocol crates now, even if advisory. Record the tier policy in an ADR.

### The 'no global state' principle lost its enforcement, and globals remain

- **Kind:** safety · **Severity:** high
- **Evidence:** Commit cf46dfe20 deleted docs/runtime-contract.toml (4085 lines) and scripts/check-runtime-conformance.sh, stating the rules 'are types and clippy lints now (ADR-0078)'. The deleted AGENTS.md row was 'No new process-global singletons in the runtime crates | runtime-conformance-check'. No replacement exists: clippy.toml has no disallowed-* entries, and the workspace lints have none. Live production globals: FONT_SYSTEM (flui-painting/src/text_layout/layout.rs:124), the image decode CACHE LazyLock (flui-widgets/src/image/decode_cache.rs:96), AssetRegistry::global (flui-assets/src/registry/mod.rs:83), the NAVIGATOR_COMMAND_TARGETS thread_local plus a process AtomicU64 (flui-widgets/src/navigator/navigator.rs:88-91), and the hot-reload registries. About 35 files with static OnceLock/LazyLock/Mutex or thread_local outside tests.
- **Impact:** Principle 3 ('ratchet ambient-reach = 0') and roadmap B1 ('Ratchet ambient-reach = 0; two realms shape in parallel') have no gate, so regressions are invisible. Multi-window as the norm (H2), record/replay determinism (G7) and parallel agent test sessions all depend on it.
- **Direction:** Restore a ratchet as an xtask check in `cargo xtask checks`, as AGENTS.md prescribes for new gates. A syn-based scan for `static` items of interior-mutable or OnceLock/LazyLock type and thread_local! in crates/*/src, with an allowlist stating reasons, is enough. Alternatively use a dylint. Then burn down the list: FONT_SYSTEM moves to a realm with Parley (B1/B7), the decode cache and AssetRegistry move to realm or app resources, and Navigator command targets become realm-scoped.

### Background execution is app-private, so libraries grow their own runtimes and globals; the IO lane is not reachable where it is needed

- **Kind:** runtime_architecture · **Severity:** high
- **Evidence:** crates/flui-app/src/app/execution.rs header: 'Nothing here is ambient: there is no global accessor, and library crates cannot reach these pools'. flui-assets therefore resolves 'ambient runtime, then an owned one' (registry/mod.rs:52-115, BridgeRuntime) and has a global registry. flui-widgets keeps a global decode CACHE. tokio is a non-optional native dependency of flui-platform (Cargo.toml:167), flui-app (:136) and flui-assets (:31). Services (ServiceDefinition, lifecycle.rs:955) are registered in L9 and cannot be named from L6.
- **Impact:** Makes A5/E2 (the IO lane for asset decoding, and 'a 4k decode does not block the frame') and D3 (Task/Worker with cancel on unmount) expensive, because every library needing IO or compute invents a side channel. The trajectory goal 'explicit async model, tokio optional' cannot hold while three crates each embed tokio.
- **Direction:** Expose execution as a realm capability through the same typed-capability seam: `ctx.executor()` returning a Send spawner for compute and IO with unmount-scoped cancellation (tokio-util CancellationToken is already a dependency). Define the Executor trait below L6 so flui-assets and flui-widgets use it instead of owning runtimes. Make tokio a flui-app implementation detail behind a feature, with host executors injectable as today.

### The agent protocol's contract lives in a binary tool, and the in-process backend has no home

- **Kind:** missing_capability · **Severity:** high
- **Evidence:** ADR-0080 (Accepted) says 'the in-process backend speaks the same one'. The contract (handles, AccessKit roles, typed replies) exists only in tools/desktop-mcp/src (a11y/role.rs, server.rs, params.rs). The root Cargo.toml layer rule says crates never depend on tools. flui-devtools states 'It is not an inspector UI, not a DevTools server… opens no port' (crates/flui-devtools/src/lib.rs), and its only consumers are flui-testing's bench and tests. flui-cli/src/commands has analyze, build, clean, completions, create, devices, doctor, emulators, format, ios, platform, run, test and upgrade, but no mcp or devtools command. The Linux backend of desktop-mcp is disabled (journal 2026-09-24).
- **Impact:** The H0 exit 'an agent passes the scenario through `flui mcp`' and the B3 exit need G1 (the in-process protocol) plus G2. With the contract types in a binary, the second backend will copy them, which is exactly the drift ADR-0080 was written to prevent. It is also the base of the product bet 'one catalog, three consumers'.
- **Direction:** Extract the wire contract into a library crate: serde types, error codes, the AccessKit role mapping and handle semantics, with no OS dependencies, at a low layer. desktop-mcp, an in-process realm backend (in flui-app or flui-devtools, reading the realm's semantics TreeUpdate and driving input via flui-testing-style event constructors) and `flui mcp` in flui-cli all depend on it. Give flui-devtools a real job, the protocol server, or merge it into that crate.

### A2UI and the machine-readable widget catalog (G6) have no foundation: no registry, no schema, and closures in props

- **Kind:** missing_capability · **Severity:** medium
- **Evidence:** There are no a2ui or llms-catalog references in code; only llms.txt prose exists. The flui-macros derives are StatelessView, StatefulView, InheritedData, Diagnosticable and Animatable (flui-macros/src/lib.rs:107-281), and none emits a prop schema. Widget props include closures: WidgetStateProperty::Resolver(Arc<dyn Fn(&WidgetStates) -> T + Send + Sync>) (flui-widgets/src/widget_state.rs:273), and callbacks are Box/Arc dyn Fn. The catalog accepts no Signal<T>: 0 `Signal<` in flui-widgets and flui-material src, against 59 ValueNotifier/ChangeNotifier uses. `signals` is not a facade default.
- **Impact:** H1 exit 'the Notes form arrives from the model via A2UI' and G6 ('agent finds a widget and its signature without reading sources') need a data-describable catalog: name → props schema → constructor, action ids instead of closures, and data binding. Retrofitting schema derives across ~1.3k widget items later is costly, and the product bet 'one catalog, three consumers' depends on it.
- **Direction:** Hypothesis-level direction. Add a `#[derive(CatalogView)]`-style macro that emits a JSON schema and a registry entry (name, props, semantics role, example) for Raw primitives first. Model interactive props as data (an action id plus an optional closure) and state bindings as signals, and generate G6 from the same registry instead of rustdoc scraping. Decide now whether A2UI props bind to signals, which argues for making `signals` default-on before the catalog grows.

### Themes are not data: no token layer below Material, no serialization, closures in state properties

- **Kind:** extension_point · **Severity:** medium
- **Evidence:** flui_material::ThemeData is a plain struct of Option<XThemeData> (theme_data.rs:752-~830) with no serde feature in flui-material or flui-cupertino (their Cargo.toml files have no serde entry). theme_data.rs:737 says 'Deferred: iconTheme, extensions, platform'. ADR-0042 decides 'There is no universal ThemeData abstraction over Material and Cupertino'. State-dependent colors use Resolver closures (widget_state.rs:273).
- **Impact:** Blocks 'themes as data (H1)' and 'Figma-token import (H3)'. A theme cannot come from a file, a model (A2UI) or design tooling, and a third-party design system has no shared token vocabulary to build on.
- **Direction:** Keep ADR-0042's 'no universal ThemeData', but add a design-neutral token substrate at L6: typed color, typography, shape and motion token maps with serde, resolved through InheritedView plus FieldMask. Let Material and Cupertino ThemeData be derived from tokens (`ThemeData::from_tokens`), and make WidgetStateProperty serializable in its data forms (per-state map) while keeping the closure form as an escape hatch. Supersede or amend ADR-0042 explicitly.

### Router is absent while the imperative Navigator (27k lines) keeps absorbing Hero and overlay, and the deep-link path is unwired

- **Kind:** plan_misfit · **Severity:** medium
- **Evidence:** There is no Router, RouterDelegate or RouteInformationParser. crates/flui-widgets/src/navigator/ holds 33 files and 27,457 lines, including hero*, modal_route, named_route (ADR-0024 Deprecated) and pop_scope. RouteInformation and `async fn handle_push_route`/`handle_pop_route` exist in crates/flui-view/src/binding.rs:103,1600-1616 but have no production caller in flui-app or flui-widgets. Platform::on_open_urls (platform.rs:550) is not connected either.
- **Impact:** D1/D2 (the B1 exit) require Hero and overlay to become 'properties of Router, not Navigator'. Every navigator feature added before the Router ADR doubles migration cost. Web history (F4), mobile deep links (H1) and state restoration (D4) all need the URL → route path, which does not exist end to end.
- **Direction:** Write the Router ADR before any further navigator work: typed route enum, URL as source of truth, Navigator as a facade over the same stack. Wire Platform::on_open_urls → realm → handle_push_route as the first vertical slice with a test, then move Hero and overlay observers under the Router. Freeze new features in navigator/ until then.

### External GPU content has pieces but no path: nothing produces TextureLayer, and the registry is only reachable inside the engine

- **Kind:** extension_point · **Severity:** medium
- **Evidence:** TextureLayer and PlatformViewLayer exist (flui-layer/src/layer/mod.rs:49-86) and the engine renders TextureLayer via ExternalTextureRegistry (flui-engine/src/layer_render.rs:365-378; external_texture_registry.rs:23 says the registry is 'crate-private (embedders reach it through WgpuPainter::external_texture_registry)'). No render object, widget or flui-app code creates a TextureLayer (grep TextureLayer::new or Layer::Texture outside the engine and the snapshot test gives none). The WgpuPainter is owned by the raster lane in flui-app, and no API hands an app the wgpu Device or Queue. PlatformViewLayer::render is a no-op (layer_render.rs:381-384).
- **Impact:** Plan: 'external GPU content (compositor spike H1, embedding H2): a wgpu texture or third-party renderer as a widget'. Video, camera preview (a PlatformCapability), maps and 3D (H4) all need it. With the raster lane moving to its own thread (ADR-0045), texture registration must become a cross-thread protocol, and doing that later means redesigning the mailbox.
- **Direction:** Add a realm capability `TextureRegistry` handle (Send) that registers or updates textures through the raster mailbox, plus a `Texture` widget/RenderTexture in the catalog with a harness test. Define GPU device sharing (the app gets an Arc of the device and queue from the lane) in an addendum to ADR-0045 before threading the lane.

### Two state models with contradictory canonical claims; signals are off by default and absent from the catalog

- **Kind:** api_dx · **Severity:** medium
- **Evidence:** docs/FOUNDATIONS.md:91 says setState + InheritedWidget is 'the sole canonical state model. The catalog crates… never take a dependency on a signals crate', amended to 'first-class'. ADR-0074's title is 'Realm-scoped signals as the canonical application-state layer'. The root Cargo.toml members comment still says 'contract C1 locks the catalog to the setState/Inherited model'. The facade `signals` feature is opt-in (Cargo.toml features). There are 34 `cfg(feature = "signals")` sites in view, widgets and app, 0 Signal inputs in the catalog, and 59 notifier uses. ADR-0075 (Computed/Effect) is Proposed.
- **Impact:** A3, A8, D3, A2UI binding and themes-as-data all assume signals are the canonical app-state layer. As long as they are feature-gated, catalog widgets cannot accept Signal<T> unconditionally, every widget needs dual APIs, and the stable tier (H3 lists 'signals' as Stable) has no settled contract to freeze.
- **Direction:** Decide explicitly: make `signals` non-optional in flui-view before B1 closes, rewrite C1 so the two statements agree (signals for app state, setState and notifiers as low level), fix the stale Cargo.toml comment, and define which catalog inputs accept `impl Into<Reactive<T>>`. Settle ADR-0075 before A8.

### Hot reload is implemented as the design the plan rejected (dlopen), and it occupies workspace members, a facade feature and unsafe code

- **Kind:** plan_misfit · **Severity:** medium
- **Evidence:** The plan and G4 say 'Hot reload via Subsecond; we do not build our own engine'. The code has no subsecond dependency. crates/flui-hot-reload (L6, 2.9k LOC, about 90 lines containing `unsafe`) is a dlopen plugin host with ABI-token handshakes and documented residual UB risk (lib.rs header: 'the token handshake is a strong tripwire, not a proof of layout equality'). Workspace members include examples/desktop_scene, hot_reload_counter/{types,logic,host} and hot_reload_lifecycle_fixture, and the facade has a `hot-reload` feature re-exporting flui_hot_reload.
- **Impact:** The G4 and B1 exit ('flui run --hot preserves state on macOS and Windows') needs a subsecond::call point in the build cycle and element-state migration in flui-view or flui-app, which is a different seam from plugin dlopen. Keeping both doubles maintenance and the unsafe surface.
- **Direction:** Write an ADR superseding the dlopen design, add the Subsecond hook at the realm's build entry, then delete flui-hot-reload, its examples and the facade feature in the same series. That removes one crate from the topology.

### No seam for a CPU reference renderer outside flui-engine

- **Kind:** missing_capability · **Severity:** medium
- **Evidence:** There is no CPU rasterizer dependency (no tiny-skia, vello_cpu or softbuffer in any manifest). The engine's command-level interface `CommandRenderer` is pub(crate) (flui-engine/src/command_renderer.rs:31). The public seam is `RasterBackend::render_scene(&Scene)` (raster.rs:100), so a CPU backend must re-walk layers and re-interpret the display list itself. flui-engine is 74k lines, with effects (blur, blend, morphology, color matrix) implemented as wgpu pipelines.
- **Impact:** E7 and G3 (goldens identical across OSes, CI without a GPU) are part of the H0 exit and the plan's 'determinism in tests' bet. Without a shared lowering, a CPU backend will diverge from the GPU path effect by effect. That is hypothesis-level, since sizing depends on how much of command_ir is backend-neutral.
- **Direction:** Hypothesis: make command_ir (the lowered, backend-neutral command stream) the seam, expose a `CommandSink` trait from flui-engine, and implement the CPU reference as a second sink inside flui-engine behind a feature, reusing the layer walk (layer_walk.rs). Pin parity with the existing *_oracle tests.

### The owner's horizons, extension points, delivery layers and tiers are not recorded in the repo

- **Kind:** docs · **Severity:** medium
- **Evidence:** docs/ROADMAP.md is 21 lines and covers B0-B4 only. `grep -i router|capabilit|a2ui|stability|tier|parley|mcp|official` in docs/ROADMAP.md finds only B1 and B3 rows. No ADR covers PlatformCapability, SDK/stability tiers, or core vs official vs community packaging. ADR-0041 (topology) has no notion of out-of-repo official packages, and the facade's default = ["material"] contradicts 'Material as an official package'. The plan itself lives in an external Claude doc.
- **Impact:** Under AGENTS.md's rule ('code that silently disagrees with an accepted ADR is a defect'), architectural commitments that exist only outside the repo cannot be checked by review or gates. Agents working in worktrees (bus factor 1) cannot see them, so work keeps landing in shapes that H1 and H3 will have to break (Navigator, closed LifecycleContext, whole-crate re-exports).
- **Direction:** Add short ADRs for the extension-point contracts (capability registry, SDK surface and tiers, packaging and release train, external textures), each with its first enforcing check. Mirror the horizon table into docs/ROADMAP.md with links to those ADRs.

### Monolithic files survive the ui_realm split; the B0 structural exit is not met

- **Kind:** tech_debt · **Severity:** low
- **Evidence:** crates/flui-app/src/app/runner/realm_dispatch.rs is 7,149 lines of production code with no inline tests. Other large files: flui-view/src/owner/build_owner.rs 6,043, flui-view/src/tree/element_tree.rs 5,762, flui-scheduler/src/scheduler.rs 5,650, flui-engine/src/renderer.rs 5,009, flui-widgets/src/text/editable_text.rs 4,725. 40 src files exceed 2,000 lines. The B0 exit requires 'no file >3000 lines in flui-app and flui-widgets', and journal #1268 reports A2 as done.
- **Impact:** Hot files concentrate merge conflicts under the WIP limit and make B1 work (multiline editor, Subsecond hook in the build owner) harder. Moderate, but it contradicts a milestone marked closed.
- **Direction:** Split realm_dispatch.rs by event class (input, lifecycle, presentation, a11y actions) and editable_text.rs along the B4 multiline-editor seams. Add a soft file-size ratchet to cargo xtask checks if the owner wants it enforced.

## Unwired or dead surface

- Presentation::perform_haptic_feedback and UiRealm haptics: #[expect(dead_code)] 'no production caller yet' (crates/flui-app/src/app/presentation.rs:866-893). PlatformHaptics has no widget-side acquisition.
- flui_platform::traits::PlatformCapabilities / Platform::capabilities(): no production reader (only doc examples at flui-platform/src/lib.rs:139,329).
- flui_view::binding RouteInformation and async handle_push_route/handle_pop_route (binding.rs:103,1600-1616), plus Platform::on_open_urls (platform.rs:550): no production caller in flui-app or flui-widgets.
- flui_layer::TextureLayer and flui_engine::ExternalTextureRegistry: no producer outside the engine and tests. PlatformViewLayer::render is a no-op (flui-engine/src/layer_render.rs:381).
- RasterBackend::mark_dirty / DamageTracker: production code only calls mark_full_repaint (flui-app/src/app/raster_lane.rs:486, direct.rs:181).
- flui-devtools: only consumers are flui-testing's bench and tests (tree_observer_overhead.rs, tree_observer_inspector.rs).
- flui_widgets::__private: its rationale names sibling crates 'scrolling, navigation, text editing' that were rejected on 2026-09-23. Its only consumer outside the crate is tests/anchored_box.rs.
- flui-hot-reload (dlopen) plus 5 hot-reload example members and the facade `hot-reload` feature, which the plan replaces with Subsecond.
- tools/text-spike: not a workspace member (own Cargo.lock), so it is not checked by gates; the Parley rasterization spike (B7) lives outside the build.

## Open questions

- Is the 'no global state' ratchet removed in cf46dfe20 meant to come back as an xtask check, or is the owner accepting review-only enforcement of principle 3? The commit message claims types and lints replace it, but none do.
- Delivery layers: should Material, Cupertino, devtools and hot-reload move out-of-repo before H3's freeze, which would force an early stable SDK surface, or stay in-repo until 1.0? The facade's default = ["material"] implies the latter.
- Should the platform-API split (traits and values without OS dependencies) be the same crate that hosts the PlatformCapability registry, or should capabilities sit one layer higher next to flui-view's LifecycleContext?
- Is parallel layout inside a realm (the H2 open spike) still open, given render objects were made deliberately !Send (c281cd1fc)? Hypothesis: it is effectively decided against.
- For A2UI: do props bind to signals (which argues for signals non-optional before B1) or to a separate data model? This decides whether `signals` stays a feature.
- Where does the in-process agent backend live: flui-app (it owns the realm and semantics flush), flui-devtools, or a new protocol crate? ADR-0080 names it but assigns no crate.
- Is the H0 CPU reference renderer (E7) expected to share command_ir with the GPU path? This is not verified; it needs an engine-area reader.
- Does ADR-0061 (Accepted: damage needs layer identity) currently disagree with code, since no damage producer exists? If so, by AGENTS.md policy either the code or the ADR status is a defect. Not verified in depth.

