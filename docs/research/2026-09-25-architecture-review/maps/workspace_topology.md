# Codebase map: workspace_topology

_Raw output of the `map:workspace_topology` agent (read-only review of main @ cab06137d, 2026-09-25). Unedited; claims are the agent's and were only partly re-verified._

## Summary

The workspace has 27 library/binary crates under crates/, the `flui` facade at the root, 10 example packages and 4 tool packages as members. tools/text-spike is a detached workspace; tools/device-checks and decoy-face are Python/Swift scripts. The topology contract is ADR-0041: each crate declares `[package.metadata.flui] layer` (0..10, names in root Cargo.toml:92-104). `cargo xtask workspace` (tools/xtask/src/workspace.rs:189-287) checks four things against `cargo metadata`: every normal/build edge points to the same layer or lower, `allowed-dependents` holds (flui-log and the design systems), manifests inherit workspace keys and lints, and examples/tools set publish=false. Same-layer edges are legal and dev edges are unchecked.

The real graph, from cargo metadata, is acyclic and mostly clean at the bottom. geometry and types (L0), foundation and macros (L1) are leaves. The engine never depends on flui-rendering, so wgpu stays out of the headless stack: `cargo tree -p flui-widgets -i wgpu` finds nothing. Composition is concentrated in flui-app, which has 21 transitive workspace deps.

The main structural defect is one same-layer edge, `flui-interaction -> flui-platform`. It exists for a single trait import (crates/flui-interaction/src/text_input.rs:27, `PlatformTextInput`). Through it, flui-rendering, objects, view, widgets, material and testing all link the OS backend crate. That crate carries winit (switched on by a default feature with no code behind it), a tokio multi-thread runtime, `windows` and objc2. A Win32 backend edit therefore rebuilds 16 workspace crates.

The facade (src/lib.rs:126-152) re-exports whole crates (`pub use flui_app as app`, `flui_view as view`, `flui_widgets as widgets`, ...) plus curated `rendering`/`painting`/`interaction` modules. Its features are material (the default), cupertino, localizations, hot-reload, a11y, serde, signals and testing. `cargo xtask facade-combos` checks 15 of those combinations with clippy, plus three `cargo tree` facts, all about hot-reload.

Every one of the 28 crates, facade included, is `publish = ["crates-io"]`, version 0.2.0-dev, and every internal edge is pinned `=0.2.0-dev`. There is no publish dry-run and no semver-checks anywhere in `.github/workflows` or xtask. A valid publish order exists (log, geometry, cli, types, foundation, ..., material, cupertino, localizations, app, flui), but 51 versioned dev-dependencies point upward and constrain it.

Several boundaries are organisational rather than architectural:
- flui-localizations is 281 lines with a layer of its own.
- flui-devtools has zero production consumers.
- flui-testing sits beside flui-widgets but below it, because widgets depends on it through an optional normal edge. So the widget test harness lives inside widgets (`flui_widgets::testing`, 1.6k lines).
- flui-app holds the entire runtime (realm, runner, raster lane, execution), and the separate `flui-runtime` crate is gated by ADR-0041.

The owner's delivery layers (core / official packages in separate repos / community) are not reflected yet:
- Material, Cupertino, localizations, devtools and hot reload are in-repo crates.
- The facade depends on them optionally, with Material on by default.
- The agent protocol has no crate: its wire contract lives inside the unpublished 18k-line tools/desktop-mcp binary.
- No `PlatformCapability` or A2UI seam exists in code or docs (grep finds nothing).

## Responsibilities and boundaries

What the workspace layer owns: the crate graph and its direction rule (ADR-0041, manifests plus xtask), feature topology, publish units and the facade surface.

Where the boundaries leak:

1. **Platform contract vs platform backends.** flui-platform is both the capability-contract crate (traits/: PlatformTextInput, Clipboard, PlatformCapabilities, Platform with 31 methods) and the home of 7 OS backends (macos 9.5k, winit 8.3k, windows 6.5k lines, ...) plus a tokio executor. flui-interaction needs only the contract but gets everything.

2. **Spine vs catalog.** flui-view (L5 spine) depends on the concrete render catalog flui-objects (RenderLayoutBuilder, RenderSliverList/Grid, BuildDuringLayoutCell) and on flui-animation (AnimationController).

3. **Test support.** It is split. flui-testing (L6) cannot see widgets, because widgets has an optional normal edge to it. `flui_widgets::testing` holds the view-level harness used by material/cupertino tests.

4. **Runtime.** flui-app is the private composition root and the only runtime (ui_realm/ 15k lines, runner/ 18.7k lines, execution.rs, raster_lane.rs, runtime.rs). Its public `embedder` module is a 22-line stale doc stub. Execution ownership is split three ways: a tokio runtime in flui-platform/src/executor.rs:66, `async_driver.rs` in flui-scheduler (2.1k lines), and `app/execution.rs` (1.5k lines).

5. **Text.** Shaping lives in flui-painting (L2), with a process-global FONT_SYSTEM (text_layout/layout.rs:124) and a public re-export of `cosmic_text::fontdb::Family` (lib.rs:87). The Parley swap would leak into the public API.

6. **Images.** Two caches: moka in flui-assets and lru in flui-widgets. Image features exist on widgets but not on the facade.

7. **Agent/devtools protocol.** Split between flui-devtools (in-process, unwired) and tools/desktop-mcp (a bin holding the ADR-0080 wire types).

8. **Visibility by feature.** `flui-view/runtime-internals` is a public cargo feature that flui-app enables on its normal edge (crates/flui-app/Cargo.toml:90), so it is on in every production graph. There are also `testing` features on 8 library crates.

What belongs elsewhere:
- **Lower (contract crate):** platform capability traits and value types.
- **Above the catalogs, dev-only:** test support (WidgetTester, golden, replay).
- **Out of the facade's default graph and eventually out of repo:** Material, Cupertino, localizations, hot-reload, devtools.
- **Its own small crate:** a protocol crate shared by `flui mcp`, devtools and the desktop backend.

## Key types and contracts

- Root Cargo.toml:87-104 `[workspace.metadata.flui] layers` (11 names); each crate's `[package.metadata.flui] layer`, `allowed-dependents`, `allowed-dev-dependents`, `wasm`
- tools/xtask/src/workspace.rs `check_layers` (:189-287): the direction rule covers normal and build edges only; same-layer edges are allowed; dev edges are unchecked unless allowed-dev-dependents is set
- tools/xtask/src/tasks/facade.rs:17-35 COMBOS (15 facade feature sets); :49-90 TREE_FACTS (3 facts, all about hot-reload)
- ADR-0041 Workspace topology contract (a crate is a layer; flui-widgets stays one crate; no flui-runtime until there are two entry points)
- ADR-0028 design-system decoupling (allowed-dependents on flui-material/flui-cupertino)
- Facade src/lib.rs:120-152: whole-crate re-exports app/view/widgets/foundation/types/geometry/animation, plus curated rendering.rs/painting.rs/interaction.rs
- Facade [features] Cargo.toml:592-646: default=[material], cupertino, localizations, hot-reload, a11y, serde, signals, testing, gpu-readback-tests
- flui_platform::traits::Platform (31 fixed methods, traits/platform.rs:253); PlatformTextInput (traits/text_input.rs:30), the only reason interaction depends on platform
- Internal version pin: every internal edge is `version = "=0.2.0-dev"` (one lockstep release train)
- flui-view feature `runtime-internals` (27 cfg sites, e.g. binding.rs:52), described as 'not an application-facing API contract'

## Dependencies

Measured with `cargo metadata` and `cargo tree -e normal` on Windows, offline.

**Unique normal deps per crate:**

| Crate | Unique deps |
|---|---|
| geometry | 15 |
| types | 17 |
| foundation | 23 |
| tree | 38 |
| scheduler | 37 |
| platform | 67 |
| painting | 70 (cosmic-text) |
| interaction | 76 |
| layer | 83 |
| rendering | 135 |
| objects | 147 |
| view | 153 |
| widgets | 156 |
| material | 158 |
| engine | 169 |
| app | 245 |
| flui | 248 |
| cli | 87 (no workspace deps at all) |
| devtools | 59 |

**Unwanted reach into the headless stack.** `cargo tree -p flui-rendering -i winit`, `-i tokio` and `-i windows` all resolve through flui-platform <- flui-interaction <- flui-rendering. The flui-widgets graph contains winit, tokio, windows, accesskit and cosmic-text, but not wgpu.

**Workspace crates rebuilt when one crate changes:**

| Changed crate | Rebuilt |
|---|---|
| foundation | 26 |
| geometry | 25 |
| types | 24 |
| tree | 18 |
| painting | 17 |
| layer, platform, scheduler | 16 each |
| interaction, semantics, macros | 14 each |
| rendering | 13 |
| view | 11 |
| widgets | 6 |

**Inbound (who consumes the layer):**
- The facade and flui-app consume nearly every crate.
- Examples: web_counter uses the facade; android_* and desktop_scene use granular crates.
- The hot_reload_counter example is split into types, logic and host crates.

**Outbound:** crates.io (nothing published yet) and the CI workflows (feature-matrix runs the facade combos).

**Publish order (a valid topological order that honours versioned dev-deps):** log, geometry, cli, types, foundation, assets, tree, painting, platform, scheduler, macros, semantics, layer, interaction, animation, engine, rendering, objects, view, devtools, testing, widgets, hot-reload, material, cupertino, localizations, app, flui.

## Fit with the plan

**H0 (crates.io beta; a clean consumer builds Notes on 3 desktops plus web; an agent passes a scenario through `flui mcp`).** Partly blocked:
- There is no publish pipeline or semver-checks, and 28 lockstep publish units.
- The `flui mcp` command and the protocol crate do not exist; flui-devtools is unwired.
- The facade does not expose image features.
- A consumer's build compiles winit and tokio even when using native backends.

**H1 (PlatformCapability plugins as ordinary crates; A2UI; mobile).**
- Plugins: the current topology forces a plugin to depend on flui-platform, a 46k-line crate with every OS backend. The Platform trait is a closed set of 31 methods, and FOUNDATIONS Part IV explicitly dissolves "services" into flui-platform traits. A small contract crate is a precondition.
- A2UI: needs the G6 catalog as data, and no crate or module is planned for it.
- Mobile: runs through flui-app's single runtime; there is no embedder entry point, only a stub.

**H2 (perf/scale).**
- The platform edge makes every backend edit rebuild 16 crates, which costs incremental compile time.
- Raster/IO lanes need a clear execution owner, but it is split across platform, scheduler and app.

**H3 (1.0 freeze with Stable/Evolving/Experimental tiers; semver-checks green for 3 minors).**
- The facade re-exports entire crates (~6k pub items across app/view/widgets/types/foundation/geometry/animation by a regex count).
- Internal seams are public cargo features (runtime-internals, testing, test-utils, experimental-delegates).
- There is no `unstable` convention, so a tiered freeze is not expressible today.

**Delivery layers (official packages in separate repos, one release train).**
- Material, Cupertino, localizations, hot-reload and devtools are in-repo.
- The facade depends on them optionally, with default=material, so moving Material out would create a flui -> flui-material -> flui cycle unless the facade stops re-exporting it.
- Material reaches below the facade (flui_objects::RenderPhysicalShape, flui_scheduler::LocalPostFrameHandle, flui_rendering::pipeline::Canvas).
- Exact `=` pins force lockstep versions on external repos.

**Principles.**
- Principle 3 (no global state) is contradicted at the topology level by FONT_SYSTEM in painting and the process-wide tokio runtime in platform.
- Principle 2 (one toolchain) holds: cargo only, xtask as the runner.
- The decided "widgets stays one crate with import-direction checks" is only half implemented: there is no such check in xtask.

## Strengths

- The layer policy lives in the manifests and is checked mechanically against `cargo metadata` (tools/xtask/src/workspace.rs), so there is no second registry to drift; unique ADR numbers and unreachable test files are checked too.
- `allowed-dependents` expresses the targeted restrictions that matter: flui-log is composition-only (crates/flui-log/Cargo.toml:66) and nothing below the catalogs can depend on Material or Cupertino (flui-material/Cargo.toml:87-88).
- The GPU is isolated: flui-engine depends only on layer/painting/types/foundation and not on flui-rendering; `cargo tree -p flui-widgets -i wgpu` finds no match. Headless tests (flui-testing) never link wgpu.
- The facade feature combinations are compiled in isolation (15 COMBOS with --all-targets), which catches wiring that workspace feature unification would hide; the hot-reload absence from the production graph is proven with `cargo tree` TREE_FACTS.
- The workspace-wide lints are strong (pedantic, unwrap_used, unexpected_cfgs=deny, unused_must_use=deny), resolver 3, and the dev/release profiles are tuned with measured rationale (Cargo.toml:780-842).
- The rendering-protocol vs render-catalog split (flui-rendering vs flui-objects) is a real boundary that supports the 'custom render objects for third-party catalogs' extension point; the facade exposes a curated authoring module (src/rendering.rs).
- The bottom of the graph is clean: geometry, types, foundation and macros have no upward reach; flui-log has zero workspace deps; flui-cli is a standalone binary with no workspace normal deps.
- A valid crates.io publish order exists today (no normal-edge or versioned-dev-edge cycles among the 28 publish units).

## Problems

### One trait import makes the whole headless stack depend on every OS backend

- **Kind:** layering · **Severity:** critical
- **Evidence:** crates/flui-interaction/Cargo.toml:31 normal dep on flui-platform; the only use is crates/flui-interaction/src/text_input.rs:27 `use flui_platform::traits::PlatformTextInput` (grep counts 1 site). `cargo tree -p flui-rendering -e normal -i winit|tokio|windows` all resolve via flui-platform <- flui-interaction <- flui-rendering. The flui-widgets graph contains winit 0.30.13, tokio 1.53.1 and windows 0.62.2. flui-platform has 16 rebuilt dependents (rendering, objects, view, widgets, material, cupertino, testing, app, ...). ADR-0041 blesses the edge as a same-layer exemption (ADR-0037).
- **Impact:** Every edit in a Win32/AppKit/winit backend rebuilds the render/view/widget stack, an H2 compile-time and agent-iteration cost. Headless and wasm purity is only accidental. H1 PlatformCapability plugins would have to depend on a 46k-line crate with 340 unsafe sites and every OS backend. The layer numbers hide this, because L2 contains both the contract and the backends.
- **Direction:** Split flui-platform into (a) a small contract crate (`flui-platform-api` or similar: capability traits, handles, value types, `Unsupported`), placed at L1/L2 with no OS deps, and (b) the backends (one crate or per-OS modules) consumed only by flui-app and the facade. flui-interaction depends on (a). Add a TREE_FACT that flui-rendering, flui-view and flui-widgets link none of winit/tokio/windows/objc2/wgpu.

### flui-platform's default feature compiles winit into every graph with no code behind it

- **Kind:** performance · **Severity:** high
- **Evidence:** crates/flui-platform/Cargo.toml:300 `default = ["desktop"]`, :303 `desktop = ["dep:winit"]`; `grep feature = "desktop"` in crates/flui-platform/src finds 0 sites. The real fallback is `winit-backend` (:314). flui-interaction takes flui-platform with default features, so winit appears under flui-rendering on Windows, where the native Win32 backend is used.
- **Impact:** winit and its deps are compiled and linked into every crate above interaction and into user apps on Windows and macOS, where it is unused. The dependency surface (cargo-deny, advisories) and build times grow for nothing. cargo-shear cannot see it because the dep is optional.
- **Direction:** Make `desktop` an empty feature (or remove it), default to no winit, and enable `winit-backend` only for linux in flui-app (already done at crates/flui-app/Cargo.toml:159). Add a TREE_FACT that winit is absent from the Windows/macOS default graph.

### Facade shape contradicts the plan's 'official packages in separate repos'

- **Kind:** plan_misfit · **Severity:** high
- **Evidence:** Cargo.toml:526-528 optional normal deps on flui-material, flui-cupertino and flui-localizations; :599 `default = ["material"]`; src/lib.rs:131,143,149 re-export them as flui::material/cupertino/localizations, and the prelude includes Material (src/lib.rs:267). flui-material normal-depends on 9 workspace crates and reaches below the facade (src/material.rs:83 `flui_objects::RenderPhysicalShape`, scaffold_messenger.rs:212 `flui_scheduler::LocalPostFrameHandle`, checkbox.rs:95 `flui_rendering::pipeline::Canvas`). All internal edges are pinned `=0.2.0-dev`.
- **Impact:** Moving Material or Cupertino to their own repos (the plan's delivery layers; Flutter 3.47 did the same) would create a flui -> flui-material -> flui cycle, or force Material to pin exact versions of 9 core crates. The default feature teaches every consumer the Material-coupled facade, which H3 would then have to break.
- **Direction:** Decide now: the core facade `flui` has no design-system deps and a neutral prelude; `flui-material` and `flui-cupertino` depend on `flui` (or a documented 'framework author' surface) and are added by users directly. Whatever Material needs from objects/scheduler/rendering is exposed through the facade's curated modules. Replace `=` pins with caret requirements for crates meant to live outside the train, and keep `=` only inside the core train.

### The public API surface is untiered: whole-crate re-exports plus internal seams exposed as cargo features

- **Kind:** api_dx · **Severity:** high
- **Evidence:** src/lib.rs:126-152 `pub use flui_app as app`, `flui_view as view`, `flui_widgets as widgets`, `flui_foundation`, `flui_types`, `flui_geometry`, `flui_animation`. crates/flui-app/src/lib.rs:46-48 `pub mod app; pub mod bindings; pub mod embedder;` (so `flui::app::app::...`). A regex count of pub items (includes cfg(test) code): widgets 1498, rendering 1419, types 1349, interaction 951, geometry 875, objects 869, platform 866, view 686. Public features that are internal seams: flui-view `runtime-internals` (Cargo.toml:118, 'Not an application-facing API contract', yet enabled on flui-app's normal edge at crates/flui-app/Cargo.toml:90), plus `testing` on 8 crates, `test-utils` and `experimental-delegates`.
- **Impact:** H3's 'API freeze by tiers' plus semver-checks for 3 minors cannot be expressed: the Stable surface equals the entire pub surface of 7+ crates, and any consumer can switch on the internals features. Every refactor before then is a potential semver break once published.
- **Direction:** Curate the facade: explicit `pub use` lists per module (as rendering.rs already does), no whole-crate re-exports, and granular crates documented as 'framework-author, Evolving'. Introduce one naming convention (`unstable-*` or a single `unstable` feature) for seams, and make flui-app reach view internals through a sealed/doc(hidden) path instead of a feature. Run cargo-semver-checks on the facade first.

### No publish pipeline for 28 lockstep publish units

- **Kind:** missing_capability · **Severity:** high
- **Evidence:** All 27 crates and the facade declare `publish = ["crates-io"]` (grep of crates/*/Cargo.toml). `grep -rn 'publish|semver|cargo package' .github/workflows tools/xtask/src` finds only release.yml:17 'Publishing to crates.io is not done here'; no dry-run or semver-checks job exists (the journal says a weekly publish-dry-run landed in #1236, but it is absent on main at cab06137d). There are 51 dev-dependencies with a version requirement pointing at other publish units (e.g. flui-widgets -> flui-testing =0.2.0-dev), which constrain the order. crates.io name availability for `flui` and `flui-*` was not verified (the cratesio MCP failed to connect).
- **Impact:** This blocks the H0 exit ('a clean consumer builds Notes from crates.io'). Every framework change means republishing up to 28 crates in a strict order; one crate failing `cargo package` verification blocks everything above it.
- **Direction:** Add `cargo xtask publish --dry-run` (topological order from cargo metadata, `cargo package` per crate, version-less dev-deps for upward test edges) as a CI step; add cargo-semver-checks for the facade. Reduce the unit count first (merge localizations; move devtools/hot-reload out of the train; make cli independent).

### Test support sits below the widget catalog, so WidgetTester, golden and replay have no home

- **Kind:** workspace_topology · **Severity:** high
- **Evidence:** crates/flui-widgets/Cargo.toml:89 `flui-testing = { workspace = true, optional = true }` (normal edge, feature `testing` at :188), so flui-testing (L6) can never depend on flui-widgets without a cycle. The view-level harness therefore lives in widgets: crates/flui-widgets/src/testing.rs (1656 lines) plus testing/ (488), `#[cfg(any(test, feature = "testing"))] pub mod testing` (lib.rs:96-97), and material/cupertino use it through the `flui-widgets/testing` dev feature. flui-testing's own manifest description promises WidgetTester, golden helpers and replay.
- **Impact:** The G-track (golden on the CPU reference renderer, semantic golden, record/replay, agent test API) needs to know widgets, Material and the app runtime; with today's topology it must be spread across widgets' `testing` feature and flui-testing. A test-only surface also ships inside a published catalog crate.
- **Direction:** Invert: flui-testing moves above the catalogs (depending on widgets and view; dev-only edges from everything else), and widgets drops the normal edge to it. Keep the frame-driver core (virtual clock, pump_frame) low if rendering/view tests need it, e.g. as a module of flui-scheduler or a tiny `flui-test-clock`.

### flui-app is a god composition root and the only runtime; the embedder seam is a stub

- **Kind:** runtime_architecture · **Severity:** high
- **Evidence:** flui-app is 52k lines including tests (the context puts it at 44.5k non-test): ui_realm/ 15.0k, runner/ 18.7k, runtime.rs 2.3k, execution.rs 1.5k, raster_lane, presentation_forest, window_registry. It has 21 transitive workspace deps (every framework crate but the catalogs). crates/flui-app/src/embedder/mod.rs is 22 lines of doc listing Android/iOS/Web as '(future)', although those runners exist. ADR-0041 gates `flui-runtime` on 'two entry points'.
- **Impact:** H1 (mobile, host-driven embedding, the system-compositor spike), H2 (raster/IO lanes) and the H4 embedded/kiosk bet all need a runtime usable without flui-app's managed loop; the gate can never open because no second entry point can be written against a crate that owns everything. Change amplification: any runtime edit relinks the full 245-dep graph.
- **Direction:** Treat ADR-0041's gate as satisfied by a plan instead of a second consumer: write the embedder/host-driven entry point as the first client of an extracted `flui-runtime` (realm, frame transaction, lanes, execution services), with flui-app keeping only platform wiring and `run_app`. Delete the stub `embedder` module until then.

### Execution and async ownership is spread over three crates, with tokio baked into the platform layer

- **Kind:** runtime_architecture · **Severity:** medium
- **Evidence:** crates/flui-platform/src/executor.rs:66 builds a `tokio::runtime::Builder::new_multi_thread()` and task.rs:67 exposes `tokio::task::JoinHandle`; the Platform trait requires `fn background_executor(&self) -> Arc<dyn PlatformExecutor>` (traits/platform.rs:259). flui-scheduler/src/async_driver.rs has 2119 lines, and flui-app/src/app/execution.rs 1504. tokio is a non-optional normal dep of flui-platform (Cargo.toml:167), flui-app (:136) and flui-assets (:31). docs/crates.md itself says platform 'loses BackgroundExecutor ... when host-injected runtime execution lands'.
- **Impact:** This conflicts with the plan's runtime target ('explicit async model (Task/Worker), tokio optional'). A process-wide tokio runtime inside a platform object violates principle 3 (realm ownership). Through the interaction edge, tokio is compiled into the headless stack.
- **Direction:** One owner for execution services (the future flui-runtime / flui-app), host-injectable, with tokio behind a feature. flui-platform exposes only a main-thread wake/dispatch capability; flui-scheduler keeps phase ordering and tickers and drops the async driver.

### The agent/devtools protocol has no crate: flui-devtools is unwired and the wire contract sits inside a tool binary

- **Kind:** extension_point · **Severity:** medium
- **Evidence:** No production crate depends on flui-devtools (grep for `flui_devtools` outside the crate hits only crates/flui-testing/benches/tree_observer_overhead.rs). tools/desktop-mcp (17.9k lines, publish=false, bin only) holds the ADR-0080 wire types (params.rs, error.rs). ADR-0080 says 'the in-process backend speaks the same one'. flui-cli has no `mcp` command (crates/flui-cli/src/commands: analyze, build, clean, completions, create, devices, doctor, emulators, format, ios, platform, run, test, upgrade).
- **Impact:** The H0 exit ('an agent passes a scenario through `flui mcp`') and principle 4 (DevTools is a client of the same protocol as the agent) have no place to land. A second backend would have to copy the schema out of a binary, or depend on it.
- **Direction:** Create one protocol crate (schema, handles, error codes, AccessKit vocabulary; serde only) that desktop-mcp, an in-process realm backend and `flui mcp` in the CLI all depend on. Retire or fold flui-devtools into it, or into the in-process backend. Plan it as an official package on the release train.

### No open seam for the planned PlatformCapability plugins; capabilities are a closed trait in the backend crate

- **Kind:** extension_point · **Severity:** medium
- **Evidence:** The `pub trait Platform` (crates/flui-platform/src/traits/platform.rs:253) has 31 fixed methods (clipboard :423, data_transfer :435, displays, background_executor, ...); a new capability means editing this trait and all 7 backends. docs/FOUNDATIONS.md Part IV: 'No flui-services — ... becomes capability traits on flui-platform'. `grep PlatformCapability|a2ui` across crates, src and docs finds nothing.
- **Impact:** H1's exit ('5 plugins built outside the repo') is impossible without a registration seam and a small crate plugins can depend on. Retrofitting after the 1.0 freeze would break the Platform trait.
- **Direction:** Together with the platform-api split, define a typed capability registry (TypeId- or key-typed, realm-scoped, with `Unsupported` as a typed result), acquired through LifecycleContext (ADR-0078). Migrate haptics, clipboard and text input to it as the first built-in 'plugins' to prove the seam before H1.

### The layer model is too fine to guide and too weak to enforce reach

- **Kind:** workspace_topology · **Severity:** medium
- **Evidence:** 11 layers for 27 crates; L5 (view), L8 (localizations, 281 lines) and L10 (facade) hold a single crate each. L2 'Substrate' mixes log, tree, platform, scheduler, painting, interaction and assets, and ADR-0041 allows same-layer edges, so L2's internal order (interaction -> platform) is invisible to the gate. The layer checker verifies only direction (workspace.rs:268-282); facade.rs TREE_FACTS check only hot-reload absence. flui-macros is declared L1 but generates code against flui-view/flui-animation (crates/flui-macros/src/runtime_path.rs names "flui-view", "flui-animation"), and those edges are invisible to the policy.
- **Impact:** The one property that matters most for H0-H2, which crates are headless, OS-free and wasm-clean, is not expressed or checked. Layer numbers are cited in docs and ADRs, so renumbering costs grow over time.
- **Direction:** Collapse to about 6 named tiers (values, substrate-contracts, render machine, spine+catalog, runtime/backends, facade/packages) and add 'reach' facts: for a list of headless crates, `cargo tree -e normal` must not contain winit/tokio/windows/objc2/wgpu/android-activity. Make the facts data in the manifests (`[package.metadata.flui] forbid-reach = [...]`).

### The 'widgets stays one crate with import-direction checks' decision has no check

- **Kind:** workspace_topology · **Severity:** medium
- **Evidence:** ADR-0041 and the 2026-09-23 decision in context.md/roadmap.md:68 require import-direction checks between flui-widgets modules (text/, scroll/, navigator/ see the base, not each other). `grep -rn -i 'import.direction|module_boundar' tools/xtask/src` finds nothing. Current module sizes: navigator 27.5k, interaction 12.8k, text 8.6k, scroll 7.7k lines. Cross-imports among text/scroll/navigator/overlay/image today amount to just 1 (navigator -> overlay).
- **Impact:** The rationale for rejecting the A1 split depends on a gate that does not exist; with ~83k lines and agents writing code, coupling between modules will grow unobserved. It is cheapest to lock now, while cross-imports are almost zero.
- **Direction:** Add a `cargo xtask` module-direction check (a syn-based or grep-based `use crate::X` matrix with an allowed-edges table in the flui-widgets manifest metadata) and wire it into `checks`.

### The framework spine depends on the concrete render catalog

- **Kind:** layering · **Severity:** medium
- **Evidence:** crates/flui-view/Cargo.toml:34-35 normal deps on flui-animation and flui-objects; uses flui_objects::{RenderLayoutBuilder, LayoutConstraintsCell, RenderSliverList, RenderSliverGrid, RenderSliverFixedExtentList, BuildDuringLayoutCell} and flui_animation::AnimationController. flui-objects has 12 rebuilt dependents.
- **Impact:** Any change to the render catalog rebuilds the spine and everything above it. It also blurs the H0 extension point 'custom render objects for third-party catalogs': the spine privileges first-party objects, and build-during-layout and lazy slivers cannot be implemented by a third-party catalog through the same seam.
- **Direction:** Move the element/render cooperating pairs (LayoutBuilder, lazy sliver lists/grids) up into flui-widgets, or define the cooperation seam (BuildDuringLayoutCell-like traits) in flui-rendering so flui-view depends only on the protocol.

### The text stack leaks a third-party type and a process global through a low-layer crate

- **Kind:** tech_debt · **Severity:** medium
- **Evidence:** crates/flui-painting/src/lib.rs:87 `pub use cosmic_text::fontdb::Family;` crates/flui-painting/src/text_layout/layout.rs:124 `static FONT_SYSTEM: OnceLock<Arc<Mutex<FontState>>>`. cosmic-text is reachable from 17 crates via painting (`cargo tree -i cosmic-text` -> flui-painting <- flui-layer ...).
- **Impact:** The decided Parley migration (ADR-0077) becomes a public API break across painting and every re-exporter, and the global font state contradicts principle 3 and the per-realm text target. Text shaping at L2 means a shaping-engine swap rebuilds 17 crates.
- **Direction:** Wrap `Family` in a FLUI type now, and put shaping behind a trait/module owned by painting with realm-injected font state, so the Parley swap is internal. Consider whether shaping belongs in the engine/text subsystem (A2) instead of the recording crate.

### The dlopen hot reload shapes user workspaces and spans four crates' features, while the plan moves to Subsecond

- **Kind:** plan_misfit · **Severity:** medium
- **Evidence:** examples/hot_reload_counter/{types,logic,host} (a 3-crate split for state-preserving reload; roadmap.md:40 'requires a manual cut into 3 crates'). Supporting surface: flui-hot-reload (L6, published, `app-plugin` feature enabling flui-view/runtime-internals), flui-app `hot-reload`, the facade `hot-reload` (Cargo.toml:631), 3 TREE_FACTS in xtask facade.rs, examples/desktop_scene and hot_reload_lifecycle_fixture as workspace members.
- **Impact:** When Subsecond lands, this becomes dead surface spread over a published crate, 3 features and 5 workspace members; publishing it in H0 would freeze an API the plan has already abandoned.
- **Direction:** Mark flui-hot-reload `publish = false` (or move it out of the release train) until the Subsecond decision; plan its removal together with the examples and TREE_FACTS as one change; keep `flui run` as the only public hot-reload entry point.

### Modules in disguise and unwired crates inflate the publish train

- **Kind:** workspace_topology · **Severity:** low
- **Evidence:** flui-localizations has 281 lines in 2 files, 7 pub items, and a layer of its own (L8). flui-devtools has 2.5k lines and zero production consumers. flui-cli has no workspace normal deps but shares the lockstep version. flui-app/src/embedder/mod.rs is a 22-line stub.
- **Impact:** Each is a separate crates.io unit to version, document and semver-check for H3; they add layers and cognitive load without an independent consumer.
- **Direction:** Fold flui-localizations into flui-widgets' localization module, or make it the planned i18n (ICU4X) official package outside the core train. Move devtools into the protocol crate. Version flui-cli independently.

### Dead or empty features, and feature surface the facade cannot reach

- **Kind:** tech_debt · **Severity:** low
- **Evidence:** flui-app features desktop/android/ios/web/debug-overlay/performance-overlay each have 0 `cfg(feature=...)` sites in crates/flui-app/src; flui-platform `web` 0 and `desktop` 0 (the latter only pulls winit); flui-geometry `mint` 0. flui-widgets `images`/`asset-images`/`network-images` (Cargo.toml:173-177) have no facade feature, so `flui` users cannot enable network images without adding flui-widgets directly.
- **Impact:** The feature matrix grows with no behaviour behind it, and consumers see misleading switches; image loading, a table-stakes feature, is unwired from the product surface.
- **Direction:** Delete the empty features; add `images`/`network-images` to the facade; extend the facade-combos with them.

### Shared dependencies bypass [workspace.dependencies] and the gate does not notice

- **Kind:** tech_debt · **Severity:** low
- **Evidence:** raw-window-handle is declared directly in flui-platform (Cargo.toml:74) and flui-engine (:60) although the workspace has it (root :234). bitflags is "2.6" in flui-platform (:77) vs workspace 2.10. android_log-sys is direct in flui-hot-reload (:41) though it is a workspace dep. flui-cli declares serde/toml directly (:83,:86). dpi appears in both interaction "0.1.2" and platform "0.1". wasm-bindgen-test =0.3.77 is pinned separately in app, engine and foundation.
- **Impact:** Version drift and duplicate resolution, against the AGENTS.md review rule 'shared dependencies go through [workspace.dependencies]', which no gate enforces.
- **Direction:** Have `cargo xtask workspace` flag any external dep used by 2+ members that is not `workspace = true`.

### The topology docs are stale and carry process markers; no forward target graph for H1-H4

- **Kind:** docs · **Severity:** low
- **Evidence:** docs/FOUNDATIONS.md:134 onwards, 'Part IV target crate decomposition', lists as 'still ahead' items that have landed (facade, design-system crates, localizations), and its target table equals the current layers. Nothing is listed for platform-api, runtime, protocol, a2ui or plugins. The root Cargo.toml member comments carry 'Phase 1/2/3', 'Core.1 slice', 'Catalog.1 slice', 'Cross.A' (Cargo.toml:9-39); docs/crates.md uses 'Catalog.1', 'Runtime.1' and 'Cross.P' and says '28 crates plus the flui facade' (there are 27) and that flui-cli 'depends on flui-hot-reload' (dev-only edge). There are Python and Swift device scripts in tools/device-checks.
- **Impact:** Agents read these as the architecture; the absence of a written target topology for H1-H4 means crate decisions will be made one PR at a time. Process markers violate the AGENTS.md rule.
- **Direction:** Replace Part IV with a dated target graph that includes the platform-api split, runtime extraction, protocol crate, test-support placement and the out-of-repo packages; strip the process markers; generate the crates table from the manifests.

### Feature slicing multiplies full-graph builds (hypothesis)

- **Kind:** performance · **Severity:** low
- **Evidence:** `testing`-style features on 8 library crates are enabled on dev edges (e.g. crates/flui-material/Cargo.toml:57-74, flui-objects:43-48, flui-view:72); facade-combos runs 15 separate clippy builds of `-p flui`. The journal's own open hypothesis: several feature slices leave multiple copies of wgpu/naga/cosmic-text in target/debug (not measured here).
- **Impact:** Test builds of different crates resolve flui-rendering/objects/widgets with different feature sets, so the upper stack is compiled more than once. This would explain part of the 13-20 GB target and the CI time, which bears on the H2 metrics and the memory-limited dev host.
- **Direction:** Measure: count distinct `libflui_rendering-*.rlib` hashes in target/debug/deps after `cargo xtask test`. If confirmed, move test hooks behind `cfg(test)` plus one workspace-unified `unstable-testing` feature, or into the test-support crate.

## Unwired or dead surface

- flui-devtools: the whole crate (2.5k lines, 67 pub items). No production dependent; used only by a flui-testing bench and test (dev edge).
- flui-app `pub mod embedder` (crates/flui-app/src/lib.rs:48; embedder/mod.rs is 22 lines of doc only, and stale).
- flui-app features desktop, android, ios, web, debug-overlay, performance-overlay: zero cfg sites.
- flui-platform features `desktop` (only pulls winit, zero cfg sites) and `web` (a no-op, per its own manifest comment).
- flui-geometry feature `mint`: zero cfg sites.
- flui-widgets `images`/`asset-images`/`network-images` features: not reachable through the `flui` facade.
- examples/android_app, android_demo and android_scene: excluded from the workspace and not built by any CI job or xtask command (only clippy of the platform/app android runner in cross-typecheck).
- flui-hot-reload dlopen path: slated for replacement by Subsecond in the plan, yet published and wired through three features.
- flui-painting `pub use cosmic_text::fontdb::Family` (lib.rs:87): a third-party type in the public API, due to break with ADR-0077.

## Open questions

- Can `flui`, `flui-*` (27 names) be registered on crates.io? The cratesio MCP failed to connect, so availability is unverified and could force a rename before H0.
- Was the weekly publish-dry-run from PR #1236 (journal) intentionally removed? No trace exists in .github/workflows or xtask on main cab06137d.
- Should the platform contract crate sit at L1 (beside foundation) or L2? This depends on whether capability traits need flui-types values (probably yes: ime.rs and haptics.rs already live in flui-types).
- Is flui-rendering's dependency on flui-interaction (for hit-testing and pointer types) itself necessary, or could hit-test/pointer value types move to flui-types so rendering no longer depends on the gesture/focus machinery?
- How many copies of the upper stack does a full `cargo xtask test` compile because of per-crate `testing` features? Unmeasured; see the last problem.
- For the out-of-repo packages (Material, Cupertino): will the core keep an exact-pin lockstep train with them ('one release train' in plan.md), or will packages use caret ranges against a Stable facade? This decides whether `=0.2.0-dev` pins survive.
- Does the ADR-0041 `flui-runtime` gate need superseding now, since the embedder entry point is a precondition for H1 mobile/embedding rather than a later nice-to-have?
- Where does the G6 widget catalog-as-data (A2UI source of truth) live: flui-widgets metadata, a derive in flui-macros, or a separate catalog crate?

