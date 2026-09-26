# Codebase map: tooling_testing_docs: flui-cli, flui-devtools, flui-testing, flui-assets, flui-log, tools/ (xtask, desktop-mcp, device-checks, live-smoke, text-spike, web-server), CI shape, docs/ + ADR set, book/

_Raw output of the `map:tooling_testing_docs` agent (read-only review of main @ cab06137d, 2026-09-25). Unedited; claims are the agent's and were only partly re-verified._

## Summary

How it works today. There are two tooling planes, and they barely touch. The repository plane is `cargo xtask`: 13k lines of Rust with 33 subcommands (tools/xtask/src/main.rs:35-105). It covers the gates (checks/lint/gate/ci/ci-full), change-scope classification for CI's fast lane (`affected`, `ci-verify`), and dependency, wasm, miri, feature-matrix and font checks, plus `device`, which runs live platform checks. It is mature and actually is the single task runner for contributors. CI (.github/workflows/ci.yml, 1737 lines, 19 jobs behind one `ci` aggregator at :1650) runs those same xtask commands. It runs the full workspace test suite only on Linux (:638-643 "Windows temporarily dropped"), plus flui-platform on windows-latest and a macOS CLI job.

The application plane is the `flui` CLI (flui-cli, 18.6k lines, no framework dependency). Its commands are create/run/build/test/analyze/doctor/devices/emulators/clean/upgrade/platform/format/completions. It has a careful output policy: NDJSON with `--json`, a documented exit-code table, and a non-interactive mode (crates/flui-cli/ARCHITECTURE.md). Real work is concentrated in `run` (2512 lines, a dlopen hot-reload loop with an Android `--scene` mode) and in `build` (per-platform packaging). `test`, `analyze` and `format` are thin cargo wrappers (60-83 lines each). There is no `devtools` or `mcp` command and no golden support.

The agent plane exists only outside the process. tools/desktop-mcp (18k lines, a binary with `publish = false`) implements ADR-0080's wire contract over rmcp. Its only accessibility backend is Windows UI Automation. It overlaps with three other out-of-process drivers: `xtask device` (hand-rolled COM UIA and SendInput), tools/live-smoke (X11/Wayland) and tools/device-checks (11 Python scripts and 4 Swift files for macOS/iOS).

Nothing in production uses flui-devtools (profiler, timeline, inspector counters). flui-testing provides the non-singleton HeadlessBinding (virtual clock, replay, AccessKit-based a11y queries, log capture, font pinning). The widget-level tester lives instead in `flui_widgets::testing` behind a `testing` feature. Test support is spread across 12 documented tiers and 9 `testing` features on shipped crates (docs/testing.md:21-58). Pixel goldens were removed in favour of structural insta snapshots of the LayerTree. Determinism depends on pinning the process-global FONT_SYSTEM.

flui-assets is a layer-2 crate that owns its own tokio runtime and exposes a public process-global registry. flui-log is a clean composition-only tracing backend.

Docs: 61 ADRs with no index and free-text statuses; docs/ROADMAP.md defers status to a claude.ai artifact; BETA.md is an 1171-line narrative evidence log; the book is a 773-line skeleton whose 15 Rust fences are all `ignore`.

Verdict against the plan: "CLI is the single entry point" is false today. There are five user-facing executables, and the planned `flui devtools`, `flui mcp` and golden `flui test` do not exist. "Everything machine-readable" holds only in islands: ADR-0080's typed replies, CLI NDJSON events and `affected --format github`. Test results, analyzer diagnostics, platform evidence, the widget catalog and ADR status are all prose or discarded.

## Responsibilities and boundaries

Ownership today:
- **flui-cli** (layer 9, no framework dependency): scaffolding, per-platform build and packaging, the dev loop (watcher and the dlopen hot-reload driver on the host side), device discovery, the output policy.
- **xtask** (no layer): every repository gate, CI planning and live device checks for Windows.
- **flui-devtools** (layer 9): in-app profiling adapters over tracing, FrameSnapshot and the ADR-0040 TreeObserver seam. No transport, no tree access.
- **flui-testing** (layer 6): the headless frame driver and its test vocabulary.
- **flui_widgets::testing**: the widget harness. It is here because flui-testing may not depend on widgets.
- **flui-log** (layer 2, `allowed-dependents` = flui-app/flui-cli/flui): the subscriber assembly.
- **flui-assets** (layer 2): byte, image and network loading with a moka cache.
- **desktop-mcp**: the agent protocol, OS-level backend only.

Where the boundaries leak:
1. The agent-protocol contract (ADR-0080: handles, roles, error codes) is embedded in a tool binary. There is no library crate that both the OS backend and the planned in-process backend (`flui mcp`, G1/G2) could share, and the role vocabulary is a hand-copied enum rather than accesskit::Role, which flui-semantics and flui-testing use.
2. The devtools boundary has no protocol. Principle 4 ("DevTools is a client of the same protocol as the agent") has no crate to live in.
3. Test support is split three ways: flui-testing, flui_widgets::testing, and 9 `testing` features on production crates. This contradicts docs/testing.md's own rule that test-only APIs live in flui-testing.
4. Live-verification drivers are split across four tools in three languages. xtask duplicates desktop-mcp's UIA/input layer with its own unsafe COM code.
5. Web serving is duplicated: tools/web-server (axum + wasm-pack) and flui-cli/src/serve.rs (std + wasm-bindgen).
6. flui-assets owns runtime policy (its own multi-thread runtime) that the runtime model says belongs to a realm or AsyncDriver.
7. Plan misplacement: the plan puts devtools/MCP, hot reload and Material/Cupertino in official packages. Here they share one workspace with CI-only tools. tools/ members carry heavy dependencies (rmcp, uiautomation, xcap, axum) into the root lock and deny scope, and nothing gives them a layer.

What belongs elsewhere:
- a `flui-agent-protocol` (or devtools-protocol) library crate that holds the ADR-0080 types, used by desktop-mcp, an in-process realm backend, the CLI's `mcp`/`devtools` commands and flui-testing's semantic finders;
- a single Rust live-driver library (UIA/AX/AT-SPI plus input and capture) under desktop-mcp, consumed by xtask device and live-smoke;
- the widget tester behind one facade-level `flui::testing` surface with finders;
- asset IO executed on the realm's IO lane.

## Key types and contracts

- flui CLI output contract: `ui::emit(event, payload)` NDJSON on stdout under --json, 28 ad-hoc event names built with `json!`, no schema or version (crates/flui-cli/src/ui.rs:180); exit-code table in `CliError::exit_code` (crates/flui-cli/src/error.rs)
- Hot-reload contract CLI↔app: a pair of environment-variable names pinned by a dev-dependency test on flui-hot-reload (crates/flui-cli/Cargo.toml:109-113); dlopen host/worker/types 3-crate template (crates/flui-cli/src/templates/hot_reload.rs)
- ADR-0080 agent wire contract: session handles w/e/s, AccessKit role names, fixed error codes with retry/effect, typed replies with outputSchema. Implemented only in tools/desktop-mcp/src/{a11y/role.rs,error.rs,server.rs}; `trait AccessibilityBackend` (tools/desktop-mcp/src/a11y/mod.rs:498), one real impl, Uia (a11y/uia.rs:1481)
- flui_testing::HeadlessBinding (pump_frame, mount_root, replay(PointerScript), a11y_tree -> A11yTree re-exporting accesskit::Role), flui_testing::fonts::pin_font_faces (must run before any text is shaped in the process)
- flui_widgets::testing::{lay_out, LaidOut} (1656+467 lines): RenderId-addressed probes such as size/offset/opacity/render_property(id, &str); no finders by role/text/key
- flui_devtools::{Profiler, FrameTimingLayer, timeline::Timeline, inspector::InspectorCounters}: tracing layer plus ADR-0040 TreeObserver adapters
- flui_log::{setup, SubscriberPolicy::{Inherit,Auto,Install}, install_subscriber}: explicit ownership of the process-global subscriber slot
- flui_assets::AssetRegistry::global() (registry/mod.rs:83, LazyLock 100 MB) and BridgeRuntime owning a fallback multi-thread tokio runtime (registry/bridge.rs:42-66)
- Workspace topology: `[package.metadata.flui] layer`, `allowed-dependents`, `wasm = false`, checked by `cargo xtask workspace` (ADR-0041); tools/ have no layer
- CI: the `ci` aggregator needs 18 jobs and is verified by `cargo xtask ci-verify` with HEAVY_JOBS (.github/workflows/ci.yml:1647-1700); `cargo xtask affected --format github` is the change-scope contract
- Demo composition snapshots: an insta text serialization of the committed LayerTree (tests/demo_layer_snapshots.rs, docs/testing.md:703-772)

## Dependencies

Inbound (who uses these crates in production):
- flui-log: flui-app and the facade. flui-cli is allowed but does not depend on it.
- flui-assets: flui-widgets only, behind the `asset-images`/`network-images` features.
- flui-testing: the facade `testing` feature and flui-widgets `testing`. Everything else is dev edges.
- flui-devtools: nobody. Its only reference is a dev-dependency from flui-testing (crates/flui-testing/Cargo.toml:106).
- flui-cli: none (a binary).
- tools/*: none. Enforced: nothing layered may depend on a tool.

Outbound:
- flui-cli: zero FLUI crates, apart from a flui-hot-reload dev-dependency; external deps are clap, tokio, notify, cargo_metadata, dialoguer, zip.
- flui-devtools: flui-foundation and flui-scheduler (optional), tracing-subscriber.
- flui-testing: interaction, foundation, types, view (`runtime-internals` feature), rendering, painting (`testing`), animation, scheduler, semantics, accesskit.
- flui-assets: flui-types, tokio (fs + rt-multi-thread, unconditionally), moka, lasso, image (optional), reqwest (optional).
- desktop-mcp: no FLUI crate at all; rmcp, uiautomation, enigo, xcap, schemars.
- xtask: the windows crate for its own UIA/SendInput implementation.
- tools/web-server: axum, tower-http.

Missing edges the plan needs:
- CLI → a devtools/agent protocol client;
- app/realm → an in-process protocol server;
- desktop-mcp → a shared protocol-types crate;
- flui-assets → the realm IO lane.

## Fit with the plan

**H0 (beta: an agent passes a scenario through `flui mcp`; a clean consumer builds from crates.io; `flui test` with golden; docs site).** The tooling is shaped against the exit criteria.
- `flui mcp`/`flui devtools` have no home crate. The protocol types are locked inside a binary, and devtools has no transport.
- G3 golden has no CPU reference renderer and no finders. Font determinism depends on a process global.
- There is no framework publish pipeline: release.yml builds only CLI binaries, there is no semver-checks or cargo-package dry run, and 172 hardcoded `=0.2.0-dev` internal pins would each need an edit on a version bump.
- G4 names Subsecond, but the CLI's largest command is built around the dlopen 3-crate model the plan says to drop, and Subsecond appears nowhere in the code.
- H2 in the roadmap ("docs site, tutorial") has only a skeleton with untested fences.

**H1 (PlatformCapability plugins, store builds from CLI, A2UI over the G6 catalog).**
- Store packaging for Android/iOS in flui-cli is a real head start.
- G6 has no generated, machine-readable widget catalog (llms.txt is hand-written, and RENDER_OBJECT_TYPES is a `&[&str]` in a test file).
- There is no plugin scaffolding command.

**H2 (perf).** Benches are compile-only per PR and run weekly (weekly.yml). flui-devtools' Profiler/Timeline could feed perf budgets but is unwired. flui-assets' private runtime conflicts with the planned IO lane.

**H3 (freeze, stability tiers, external contributor without author).**
- The status source is off-repo: docs/ROADMAP.md links a private claude.ai artifact.
- BETA.md is narrative and not machine-checkable, and the ADR set has no index or status gate.
- CLI NDJSON has no schema or version, so it cannot be classified Stable or Evolving.

**H4 (MCP extensions, community crates).** These need the protocol crate and a community-facing testing surface, and neither exists.

**Delivery layers.** The plan's "official packages" (devtools/MCP, hot-reload, Material/Cupertino) all live in-repo, mixed with CI tools. That is fine pre-1.0, but no seam is prepared for the split: tools are workspace members with no layer and share root deps.

**Principles.**
- #2 (one toolchain, cargo only) is violated by the Python/Swift device checks and by wasm-pack in tools/web-server.
- #3 (no global state) is violated by `AssetRegistry::global()`, the FONT_SYSTEM pin, and the CLI's process-global policy (acceptable for a CLI).
- #4 (machine-readable) is met only partially.
- #5 (proof, not claim) is met in spirit (dated live evidence) but in prose.

## Strengths

- `cargo xtask` really is the one Rust task runner for contributors: 33 commands, and CI calls the same commands (checks, deps, feature-matrix, wasm-*, doc-strict), so local runs and CI cannot drift. `ci-verify` proves the aggregator gates every planned job.
- Workspace layering is a machine-checked contract (`[package.metadata.flui] layer`, `allowed-dependents`, `wasm = false`, test reachability, ADR-number uniqueness) rather than a review convention (ADR-0041).
- The flui-cli output policy is well designed: NDJSON on stdout, human text on stderr, --quiet/--non-interactive/CI detection, a documented exit-code table, and `cargo flui` as an exec shim so there is exactly one parser (crates/flui-cli/src/bin/cargo-flui.rs).
- The CLI has zero framework dependencies and the hot-reload contract is pinned by a test, so the installed CLI is decoupled from framework versions. binstall metadata matches release.yml.
- ADR-0080 is an unusually rigorous agent-facing contract: stable handles, a fixed error-code set with retry/effect semantics, typed replies with output schemas, AccessKit vocabulary. It is the right template for every machine-readable surface.
- flui-testing's HeadlessBinding is non-singleton and runs on a virtual clock. Replay preserves timing, and the a11y queries run against the same accesskit::TreeUpdate the platform adapters receive, so there is no second vocabulary.
- flui-log's explicit subscriber-ownership policy (Inherit/Auto/Install), plus the rule that only composition roots link it, is a clean embedding story.
- docs/testing.md's tier map ('pick the shallowest tier that can fail') and the evidence-first BETA.md culture (dated, command-backed, limitations published) put principle 5 into practice.
- Store/packaging pipelines (Android APK, iOS app/XCFramework, desktop, web) already exist in flui-cli/src/build, which is ahead of H1's 'store builds from CLI'.

## Problems

### The CLI is not the single entry point: five user-facing executables, and the planned commands are missing

- **Kind:** plan_misfit · **Severity:** high
- **Evidence:** The `flui` command set is create/run/build/test/analyze/doctor/devices/emulators/clean/upgrade/platform/format/completions (crates/flui-cli/src/main.rs:104-200). There is no `devtools` and no `mcp` subcommand (grep for devtools/mcp in main.rs matches nothing). Separate user-facing binaries: `flui-desktop-mcp` (tools/desktop-mcp/Cargo.toml [[bin]]), `flui-web-server` (tools/web-server), `flui-live-smoke`, and `cargo xtask device` for live checks. docs/getting-started.md:149 tells users to run `cargo run -p flui-web-server`. There are also two doctors (`flui doctor` and `cargo xtask doctor`) and two device notions (`flui devices` and `cargo xtask device`).
- **Impact:** H0 exit ('agent passes a scenario through `flui mcp`') and B3 have no CLI surface. The plan's tooling principle ('create, run, build, test, doctor, devices, devtools, mcp') is two commands short, and the agent path relies on a repo-internal tool that must be built from a clone.
- **Direction:** Define the CLI command surface as the product boundary. `flui mcp` becomes a thin client/launcher over a shared protocol crate (it can host the desktop backend when the app is external and the in-process backend when the app is launched by `flui run`). `flui run --device browser` replaces tools/web-server (then delete the tool). Keep `xtask` strictly for repository gates and name that split in AGENTS.md.

### The ADR-0080 agent-protocol contract is locked inside a publish=false tool binary, with its own role vocabulary

- **Kind:** extension_point · **Severity:** high
- **Evidence:** tools/desktop-mcp has only src/main.rs (no lib target) and no FLUI dependency. The error codes, handles and reply types live in src/error.rs, src/server.rs and src/params.rs (1579 lines). `trait AccessibilityBackend` (src/a11y/mod.rs:498) is implemented only by `Uia` (a11y/uia.rs:1481) and `Unsupported`, so macOS and Linux have no a11y backend. `Role` is a hand-written enum copying AccessKit names (a11y/role.rs:17), while flui-semantics and flui-testing re-export `accesskit::Role` directly (crates/flui-testing/src/a11y.rs:36).
- **Impact:** ADR-0080 says 'the in-process backend speaks the same one', but that backend cannot import the types. It will re-declare them and drift. The two AccessKit role vocabularies can diverge silently. G1/G2, H3's 'agent protocol over standards' and H4's MCP extensions all rest on this contract.
- **Direction:** Extract a `flui-agent-protocol` library crate (types, error codes, outline/json renderers, `AccessibilityBackend`, JSON schemas), layered low (serde + accesskit only). desktop-mcp, an in-process realm backend in flui-app/devtools, the CLI and flui-testing's finders all depend on it. Map roles via `accesskit::Role` instead of a copy. Add a contract test that runs the same scenario against both backends.

### flui-devtools is an unwired island with no protocol, so principle 4 has no home

- **Kind:** missing_capability · **Severity:** high
- **Evidence:** No production crate depends on flui-devtools. The only reference is a dev-dependency from flui-testing (crates/flui-testing/Cargo.toml:106) used by tests/tree_observer_inspector.rs and a bench. lib.rs:18-24 states it 'walks no tree, opens no port'. The CLI has no devtools command (docs/crates.md:95 'no edge to flui-devtools'). The roadmap marks G1 as 'Семантика + инспектор + тест-драйвер = один протокол (flui-devtools)'.
- **Impact:** Profiler, Timeline and InspectorCounters (2.8k lines) reach no user. H0 B3's devtools protocol, H2's perf budgets (the Timeline could feed them) and principle 4 ('DevTools is a client of the same protocol as the agent') all start from zero, and the crate's current shape (tracing adapters) is not the shape G1 needs.
- **Direction:** Redefine flui-devtools as the in-process server half of the agent protocol, per realm (tree/semantics/frame-event/diagnostic streams plus actions), built on the protocol crate and the ADR-0040 seam, installed by flui-app under a feature. Its profiler and timeline become protocol streams. Until that lands, mark the crate experimental and publish=false.

### Four overlapping out-of-process drivers in three languages, duplicating UIA/input code

- **Kind:** tech_debt · **Severity:** high
- **Evidence:** tools/xtask/src/device/uia.rs and device/windows_input.rs hand-roll COM UI Automation and SendInput with the raw `windows` crate (10 `unsafe` each). tools/desktop-mcp does the same through the `uiautomation`, `enigo` and `xcap` crates. tools/live-smoke drives X11/Wayland. tools/device-checks holds 11 Python scripts and 4 Swift files for macOS/iOS, which `xtask device` shells out to (tools/xtask/src/device.rs:52,400; device/plan.rs:17 `find_python`). tools/decoy-face/generate.py and tools/text-spike/scripts/aggregate.py are also Python.
- **Impact:** Each live platform check is written once per tool. Principle 2 (one language, one toolchain) and the owner's Rust-only tooling preference are violated. The F1-F3 'live protocol' per platform and G7 record/replay would each be built a fourth time, and bus factor 1 cannot maintain four drivers.
- **Direction:** Make desktop-mcp's backend a library (the OS-driver half of the protocol crate) with UIA, AX and AT-SPI backends, and have `xtask device` and live-smoke call it. The device checks then become scenario files run through the same driver. Port the macOS/iOS Python/Swift checks to Rust (objc2/AX via the driver; XCUITest only where Apple requires it) as they are next touched.

### The hot-reload tooling is built on the dlopen 3-crate model the plan replaced with Subsecond

- **Kind:** plan_misfit · **Severity:** high
- **Evidence:** flui-cli/src/commands/run.rs is 2512 lines: the host/worker rebuild loop plus Android `--scene` push (main.rs:190-204). The templates/hot_reload.rs template (474 lines) generates the host/worker/types split. flui-hot-reload is a dlopen DynLib driver (docs/crates.md:73, docs/hot-reload.md). `grep -ri subsecond` over crates/src/Cargo.toml finds nothing. Roadmap G4 says 'свой движок и ручной разрез на 3 крейта не делаем'.
- **Impact:** The largest and most complex CLI surface, plus a whole crate and several workspace example members (examples/hot_reload_counter/*, desktop_scene, hot_reload_lifecycle_fixture), encode a model marked for removal. Every CLI change until G4 lands maintains dead weight, and the `flui run --hot` exit criterion for B1 cannot be met on this base.
- **Direction:** Decide G4 before further CLI work on run.rs: add a Subsecond spike behind `flui run --hot`, then delete the 3-crate template, the `--scene` path and the dlopen driver, or explicitly supersede the roadmap decision in an ADR if the spike fails. Record the choice in flui-cli/ARCHITECTURE.md.

### `flui test` and `flui analyze` are cargo wrappers whose --json mode discards the results

- **Kind:** api_dx · **Severity:** medium
- **Evidence:** commands/test.rs (83 lines) and analyze.rs (65 lines) wrap `cargo test` and `cargo clippy --workspace -D warnings`. Under --json they use `OutputStyle::Captured`, which pipes and then drops stdout/stderr (crates/flui-cli/src/runner.rs:166-168). The only payload left is `test.done {ok, exit_code}` or `analyze.done {ok}` (test.rs:67-78, analyze.rs:40-48). format.rs is a 60-line `cargo fmt` wrapper.
- **Impact:** This violates the plan's rule that a command either does what cargo cannot or does not exist. An agent calling `flui test --json` learns pass/fail but not which test or diagnostic failed, so principle 4 fails exactly where G3 and agents need it most.
- **Direction:** Either remove test/analyze/format until they add value, or make them real: `flui test` drives nextest or libtest JSON and re-emits per-test NDJSON events, adds golden/semantic-snapshot accept and review (G3), and exposes failure diffs. `flui analyze` forwards `--message-format=json` diagnostics as events.

### Two machine-readable contracts with different rigor: the CLI's NDJSON has no schema or version

- **Kind:** api_dx · **Severity:** medium
- **Evidence:** The CLI emits about 28 distinct event names via `ui::emit(event, &json!{...})` (grep -o 'emit("..."' across crates/flui-cli/src gives 28 unique). There is no struct, schema, version field or test pinning shapes. ADR-0080's replies, by contrast, are typed with published outputSchema. `xtask device` prints ad-hoc JSON lines (tools/xtask/src/device.rs:168,196).
- **Impact:** The CLI's --json stream is already consumed by `xtask device` for the hot-reload loop check (device.rs:204), yet it cannot be given a stability tier (H3 Stable/Evolving) or be validated by clients. Agent skills written against it will break silently.
- **Direction:** Type every CLI event (serde structs with schemars), add a `schema_version` to a `hello` event, publish the schema (`flui --json-schema`), and pin shapes with snapshot tests. Put the shared event envelope in the protocol crate so the CLI, devtools and MCP share one envelope.

### Test support is split across 12 tiers and 9 production `testing` features; the widget tester has no finders

- **Kind:** api_dx · **Severity:** medium
- **Evidence:** docs/testing.md:28-41 lists 12 tiers. `testing = [...]` features exist on engine, interaction, layer, objects, painting, rendering, scheduler, semantics and widgets. The widget tester is `flui_widgets::testing` (1656+467 lines) and is addressed by RenderId (`size(id)`, `opacity(id)`, `render_property(id, &str)`, testing.rs:553-946), with no by_role/by_text/by_key finders. docs/crates.md:72 says 'WidgetTester ... belong[s] here' (in flui-testing), but docs/testing.md:44-48 admits layering forbids it. tests/agent_workflow.rs inspects structure by `DiagnosticsNode::to_string_deep` text.
- **Impact:** G3 ('WidgetTester equivalent, semantic finders by_role/by_label') has no natural home. App authors see three namespaces (`flui::testing`, `flui::testing::widgets`, `flui::testing::rendering`, src/testing.rs). Production crates carry test-only public API that becomes semver surface at H3. The in-process agent backend and the tester would implement finders twice.
- **Direction:** Move the widget harness into a crate above flui-widgets (for example flui-testing split into `flui-test-core` at L6 and a `flui-tester` at L7/L9, or the tester built into the facade `testing` module), with semantic finders built on the protocol crate's query model so agent and test share queries. Shrink the per-crate `testing` features to `#[doc(hidden)]` seams, and declare them non-Stable in the H3 tiering.

### Golden and determinism rest on a process-global font system; no CPU reference renderer exists

- **Kind:** missing_capability · **Severity:** high
- **Evidence:** crates/flui-testing/src/fonts.rs:22-35: `pin_font_faces` must run 'before any text is measured or shaped in the process' because FONT_SYSTEM is process-wide. The pixel-golden suite was removed (docs/testing.md:723-750); only insta LayerTree text snapshots remain (tests/demo_layer_snapshots.rs). flui-engine has only GPU readback/oracle suites on WARP (the gpu-test job, ci.yml:949). No software renderer crate or module exists.
- **Impact:** The H0 content E7 (CPU renderer for golden) and G3 (pixel and semantic golden, identical on 3 OSes) need a new renderer plus per-realm fonts (B7/Parley). Any test process that shapes text before pinning becomes nondeterministic, and nextest's process-per-test only hides this. Principle 3 is violated in the test path.
- **Direction:** Tie golden tooling to the text migration: per-realm font collections (Parley, ADR-0077) make pinning a HeadlessBinding option rather than a process ritual. Choose the CPU reference (for example tiny-skia or vello_cpu, rendering DisplayList/LayerTree) as an ADR before G3 and expose it through `flui test --golden`. Keep structural snapshots as the default tier.

### No framework publish pipeline: release builds only the CLI; 172 hardcoded internal version pins

- **Kind:** missing_capability · **Severity:** high
- **Evidence:** .github/workflows/release.yml (140 lines) builds and packages only flui/cargo-flui binaries and drafts a GitHub release. xtask has no publish, package dry-run or semver-checks command (grep 'cargo package' finds only fonts.rs:337). `grep -rh 'version = "=0.2.0-dev"'` over the manifests counts 172, versus 9 `flui-* = { workspace = true }`. The roadmap marks the framework as 'not published on crates.io'.
- **Impact:** The H0 exit ('clean consumer builds Notes from crates.io') and B4 have no mechanized path. H3's 'semver-checks green 3 minors in a row' has no tool. A version bump is a 172-line manual edit, and the delivery-layer plan (official packages on one release train) needs exactly this machinery.
- **Direction:** Move all internal deps to `[workspace.dependencies]` with the version in one place, enforced by `cargo xtask workspace`. Add `cargo xtask release-check` (cargo package --workspace --no-verify dry-run in dependency order, cargo-semver-checks against the last tag, publish order from layers) as a CI job. Consider release-plz for the train.

### Two web dev servers with different toolchains

- **Kind:** tech_debt · **Severity:** medium
- **Evidence:** tools/web-server/src/main.rs:1-12 uses axum, tower-http and wasm-pack on port 8080. crates/flui-cli/src/serve.rs:1-12 is std-only, uses wasm-bindgen directly (build/web.rs:2-7) and reloads by long-poll. docs/getting-started.md:149-156, docs/hot-reload.md:207 and examples/README.md:91 point users at the tool, not at `flui run --device browser:`.
- **Impact:** Users hit two web workflows. wasm-pack is a second build tool, against principle 2. The axum/tower-http dependencies enter the workspace lock and deny scope for a redundant binary.
- **Direction:** Delete tools/web-server, point the docs at `flui run --device browser:<name>` and `flui build web`, and let the examples' web crates build through the CLI.

### flui-assets (layer-2 substrate) owns a tokio runtime and a public process-global registry, and is not wasm-capable

- **Kind:** runtime_architecture · **Severity:** medium
- **Evidence:** crates/flui-assets/Cargo.toml: tokio with `rt-multi-thread` unconditionally, and `wasm = false` ('tokio's networking stack has no wasm32 support'). registry/bridge.rs:42-66 builds an owned multi-thread runtime as a fallback. registry/mod.rs:83-90 exposes `pub fn global()` over a 100 MB LazyLock, which flui-widgets deliberately avoids (image/asset_image.rs:19). Commented-out 'Coming Soon' features in the manifest: hot-reload, mmap-fonts, parallel-decode. A second image cache (lru) lives in flui-widgets (Cargo.toml:173).
- **Impact:** This violates principle 3 and the runtime trajectory (realm plus IO lane, tokio optional, roadmap #1063). Web cannot load asset or network images through the standard path. Two caches with different eviction policies share one concern. `global()` becomes a Stable-tier liability at H3.
- **Direction:** Make flui-assets runtime-agnostic: loaders are async fns or sources driven by an injected executor (the realm IO lane); remove `global()` and the owned runtime; make tokio and reqwest optional per target and add a wasm fetch loader. Move the decoded-image cache here as the roadmap intends, and drop the commented feature stubs.

### The project's status and plan live outside the repository and are stale inside it

- **Kind:** docs · **Severity:** medium
- **Evidence:** docs/ROADMAP.md:5 defers 'the living plan' to https://claude.ai/code/artifact/4e1d6ca0-... (private by default). The same file's B0 exit says '26 crates' while 27 exist. BETA.md (1171 lines) is an append-only narrative whose platform table heading reads 'candidate: this branch at `v0.1.0`' (BETA.md:110) while the workspace is 0.2.0-dev. The evidence rows are prose paragraphs, not records.
- **Impact:** The H3 exit ('an external contributor finishes a feature without the author') and principle 5 need status that a contributor or agent can read and check. Today it depends on an artifact only the owner sees and on a hand-edited prose log that drifts from the code.
- **Direction:** Keep the plan in the repo (for example docs/plan/*.md, generated from or mirrored to the artifact). Turn BETA.md evidence into structured records (docs/evidence/*.toml: platform, check, command, commit, date, result, limitations), written by `xtask device` and live-smoke runs and rendered into BETA.md by an xtask command, and gate staleness by age or commit distance.

### ADR set: 61 records, no index, free-text statuses, no gate on lifecycle

- **Kind:** docs · **Severity:** medium
- **Evidence:** ls docs/adr shows 61 files (the roadmap says 68), numbering gaps 0001-0002, 0004-0005, 0007-0008 and more, and no README/index. Statuses are free text: 'Accepted — to be superseded by ADR-0077' (0016, 0059), 'Accepted (parts 1 and 2 landed 2026-09-18)' (0065), a long Proposed clause (0045), 'Deprecated...' (0024). ADR-0077 (Parley) and ADR-0075 are Proposed while the roadmap treats them as decided. `cargo xtask workspace` checks only number uniqueness.
- **Impact:** The plan's governance ('ADR hygiene with index', 'break explicitly: Supersedes') is enforced only by review. Agents cannot tell which records are binding, and superseded-in-part chains (0030←0037, 0043←0050, 0018/21←0078, 0020←0064, 0023←0079) are discoverable only by reading every file.
- **Direction:** Add machine-readable front matter (status enum: Proposed, Accepted, Deprecated, Superseded; supersedes and superseded_by lists; date; crates). Extend `cargo xtask workspace` (or a new `xtask adr`) to validate the enum, the symmetry of Supersedes and Superseded-by, and links, and to generate docs/adr/README.md as the index. This follows AGENTS.md's 'make rules types, not reviews'.

### Book and docs drift: untested fences, missing signals, stale crate claims

- **Kind:** docs · **Severity:** medium
- **Evidence:** book/ totals 773 lines. All 15 Rust fences are ```rust,ignore, and docs.yml:10 deliberately runs no `mdbook test`. book/src/SUMMARY.md 'State: setState, InheritedView, ValueNotifier' does not cover signals although ADR-0074 is Accepted. book.toml says 'book skeleton (H2, part 1)' and cites 'AGENTS.md's no invented API rule', which AGENTS.md no longer contains (grep finds nothing). docs/crates.md:95 claims flui-cli 'depends on flui-hot-reload' (it is a dev-dependency only, crates/flui-cli/Cargo.toml:113). flui-log/src/lib.rs:14 names flui-cli as a dependent, and the CLI's ARCHITECTURE.md says it has none. The root Cargo.toml:70-75 comment says 'contract C1 locks the catalog to the setState/Inherited model', contradicting ADR-0074.
- **Impact:** The B3/H2 docs site would ship unverified code, and llms.txt/docs mislead agents, which the AI-native bet makes a first-class failure. Nothing gates the drift.
- **Direction:** Make book examples compile: include them from examples/ or doctested files via `{{#include}}` or `{{#rustdoc_include}}`, and run `mdbook test` or a skeptic-style xtask in CI. Generate the crate map (docs/crates.md) and llms.txt sections from `cargo metadata` and `[package.metadata.flui]` instead of hand-writing them.

### The process-marker rule is review-only, and dozens of markers remain

- **Kind:** tech_debt · **Severity:** low
- **Evidence:** At least 34 unambiguous markers remain in code and manifests, for example 'Ship bar (wave N)' in about 15 lib.rs files (flui-devtools/src/lib.rs:61, flui-app/src/lib.rs:42, flui-platform/src/lib.rs:165, ...), and 'ACTIVE — Core.1 slice', 'Catalog.1 slice', 'per Cross.A', 'Phase 1: Foundation Layer' in the root Cargo.toml member comments (lines 8-38). The Phase comments disagree with the declared layers: for example flui-log is listed under 'Phase 1' but is layer 2. `grep` finds no xtask check for markers.
- **Impact:** This is small by itself, but it shows the pattern AGENTS.md warns about: a convention without a gate decays. The member comments are the first thing a new contributor reads about topology and they contradict the layer table.
- **Direction:** Add a regex check in `cargo xtask checks` (with the archival-root exemption list AGENTS.md already defines), and replace the root members comments with the layer names, or drop them since the layer lives in each manifest.

### Only Linux executes; the agent protocol backend and Windows live checks never run in CI

- **Kind:** testing · **Severity:** medium
- **Evidence:** The `test` matrix is `os: [ubuntu-latest]` ('Windows temporarily dropped', ci.yml:638-643). Windows runs only flui-platform (platform-windows, :1049-1086) and gpu-test. desktop-mcp gets cross-target clippy only (ci.yml:478-482), and its only a11y backend is Windows. `xtask device windows-a11y` and `windows-input` are manual (BETA.md Windows row: 'a manual run rather than a CI job'). macOS runs only cli-macos.
- **Impact:** The protocol the H0 exit depends on is never executed by the merge path. Windows is the owner's dev host and the B1 exit platform (Narrator, IME), and its regressions are caught only by hand. Principle 5 evidence ages without re-verification.
- **Direction:** Add a windows-latest job that runs desktop-mcp's protocol tests and `xtask device windows-a11y`/`windows-input` against the generated counter (UIA works on hosted runners). Once a Linux AT-SPI backend exists, run the same scenario on Linux so the agent path has one CI-executed backend on each OS.

### tools/ is an unlayered bucket mixing CI internals, a product-grade agent server and spikes

- **Kind:** workspace_topology · **Severity:** medium
- **Evidence:** The root Cargo.toml members include tools/web-server, tools/live-smoke, tools/desktop-mcp and tools/xtask with no `[package.metadata.flui] layer` (the loop over manifests shows only `wasm = false`). tools/text-spike is a nested standalone workspace with its own Cargo.lock (tools/text-spike/Cargo.toml:10-14). tools/device-checks and tools/decoy-face are script folders. The plan puts 'devtools/MCP' among official packages with their own release train.
- **Impact:** The one tool that is a product (desktop-mcp, a future `flui mcp` backend, 18k lines) has the same status as CI glue: not published, no semver, no layer, no place in the H4 ecosystem story. Its dependencies (rmcp, uiautomation, xcap, enigo, schemars) sit in the framework's lock and deny scope.
- **Direction:** Split the category. Product tooling (the protocol crate, desktop-mcp as `flui-mcp`, the CLI, devtools) goes under crates/ with layers and publish intent, as the future 'official packages' seam. Repository-only tools (xtask, live-smoke, fixtures) stay in tools/, and spikes move to a branch or to docs/research once concluded.

## Unwired or dead surface

- flui-devtools, the whole crate: Profiler, FrameTimingLayer, timeline::Timeline, inspector::InspectorCounters. No production dependent; the only reference is a flui-testing dev edge (crates/flui-testing/Cargo.toml:106).
- flui_assets::AssetRegistry::global() (crates/flui-assets/src/registry/mod.rs:83): public process global that the only consumer (flui-widgets) deliberately never calls.
- flui-assets commented 'Future Features (Coming Soon)' stubs: hot-reload, mmap-fonts, parallel-decode (crates/flui-assets/Cargo.toml features section).
- tools/web-server: a second web dev server superseded by `flui run --device browser:` (crates/flui-cli/src/serve.rs), still referenced by docs/getting-started.md:149.
- desktop-mcp `AccessibilityBackend` trait (tools/desktop-mcp/src/a11y/mod.rs:498): an abstraction with one real implementation (Uia), inside a binary no other crate can link.
- `flui format` (60 lines) and `flui analyze` (65 lines): wrappers over cargo fmt/clippy that add nothing beyond styling, and whose --json output drops the diagnostics.
- The dlopen hot-reload surface slated for replacement by Subsecond: flui-hot-reload, the `--hot-reload` 3-crate template (templates/hot_reload.rs), `flui run --scene` (Android), examples/hot_reload_counter/*, examples/desktop_scene.
- flui_testing::replay::PointerScript and flui_testing::a11y: public, but reachable only from tests (by design test-only; listed because the in-process agent backend should reuse them and currently cannot through a protocol).
- RENDER_OBJECT_TYPES is a `&[&str]` inside a test file (crates/flui-objects/tests/render_object_harness.rs:151), not a machine-readable catalog; llms.txt is hand-written, so the G6 generated catalog does not exist.

## Open questions

- Where should the in-process protocol server live: flui-app (it owns realms, layer 9), a redesigned flui-devtools (layer 9, needs realm access through a seam), or a new crate between them? This decides whether `flui run` launches apps with the server compiled in by default in debug builds.
- Should desktop-mcp become the official `flui mcp` (published, and later moved to its own repo per the delivery-layer plan), or stay a repo-internal agent aid with `flui mcp` as a separate in-process-only product?
- Is the owner willing to delete `flui test`/`analyze`/`format` until they add value beyond cargo, or should they stay as convenience aliases, which conflicts with the plan's 'does what cargo cannot or does not exist'?
- Golden CPU reference: tiny-skia versus vello_cpu versus a DisplayList interpreter in flui-engine. This interacts with ADR-0077 (Parley glyph rasterization) and should probably be decided in the same spike (hypothesis).
- Does the Subsecond decision (G4) survive a spike on Windows and Android? If not, the dlopen CLI surface needs an ADR superseding the roadmap decision instead of silent persistence.
- Should BETA.md evidence become generated from structured records written by `xtask device` and live-smoke, and should the claude.ai roadmap artifact be mirrored into the repo so external contributors and agents can read the plan?
- The ADR count mismatch (61 files versus the '68 ADR' in the roadmap and journal): were ADRs deleted under the 'delete an ADR whose decision no longer exists' rule, or is the roadmap figure stale? Not verified; `git log --diff-filter=D -- docs/adr` would answer it.

