# AGENTS.md

> Compact guide for AI agents working in the FLUI repository. Every line answers: "Would an agent likely miss this without help?"

---

## Prime Directive

Three rules, in priority order. They override convenience, never each other.

1. **Port the core, loyal to behavior.** The three-tree model (View → Element → Render), lifecycle, the layout/paint/hit-test protocol, and reconciliation are ported 1:1 from `.flutter/`. *Structure* is Rust-native (Arity system, `NonZeroUsize` IDs, Slab arenas, `Result`/`thiserror`); *behavior* stays loyal. "Make the core better" reverts to Flutter semantics — see [`STRATEGY.md`](STRATEGY.md).
2. **Leapfrog the edges.** Where Flutter has *no strong contract* — animation curves, velocity prediction, color interpolation, input smoothing — propose the market-best abstraction now, not the Flutter one. Breaking changes are cheap today and ossify once consumers exist; do not defer a better shape to "later". (This never touches the widget-tree mental model rule #1 protects.) **Sanctioned leapfrog zones (ADR-0027):** multi-window ownership, runtime/scheduling topology, concurrency architecture, and presentation architecture — Flutter is the behavioral reference for widget-tree semantics, *not* for process/thread/window topology; a review must not reject `UiRealm`-model divergence (realm-scoped GlobalKey/focus, per-realm schedulers) as forbidden drift.
3. **Done means verified against the reference.** "Implemented" is not "done", and a green gate is necessary but not sufficient. Before claiming parity or completion, verify against `.flutter/` and the render harness — see [Definition of Done](#definition-of-done-anti-cheating).

---

## Quick Start for AI Agents

**Read this first.** Then read `crates/<crate>/AGENTS.md` for the crate you're working on.

### Decision Tree

```
You need to...
├── Understand the project → read this file + README.md
├── Work on a specific crate → read crates/<crate>/AGENTS.md
├── Find a symbol, or its callers → rust-analyzer LSP if available, else rg
├── Rename across files → LSP rename if available; else find every call site with rg first
├── Understand port methodology → read docs/PORT.md
├── Add a dependency → check workspace deps in root Cargo.toml
├── Run tests for one crate → `just test-crate <crate-name>`
├── Run full pre-PR gate → `just ci`
├── Check if code compiles → `just check`
└── Run port-check triggers → `just port-check-verbose`
```

### What to Read by Task

| Task | Read First | Then |
|------|-----------|------|
| Fix a bug in a crate | `crates/<crate>/AGENTS.md` | crate's `src/lib.rs`, relevant ARCHITECTURE.md |
| Add a new feature | `docs/ROADMAP.md` (is it planned?) | `crates/<crate>/AGENTS.md`, `docs/FOUNDATIONS.md` |
| Change render/layout/paint | `crates/flui-rendering/AGENTS.md` | `.flutter/` reference, `docs/PORT.md` |
| Understand error handling | `crates/flui-foundation/AGENTS.md` | `thiserror` in libs, `anyhow` in bins |
| Touch logging setup or a log backend | `crates/flui-log/AGENTS.md` | `docs/workspace-layers.toml` (only composition roots may depend on it) |
| Write or review Rust code | `STYLE.md` | Crate `AGENTS.md`, relevant architecture contract |
| Add a cross-crate dep | `docs/workspace-layers.toml` (the checked layer policy) | Root `Cargo.toml` `[workspace.dependencies]`, `docs/FOUNDATIONS.md` Part IV |
| Add a new crate | `docs/workspace-layers.toml` — classify it *first*; `[[planned]]` records gated extractions | `docs/crates.md` "Adding a New Crate", [ADR-0041](docs/adr/ADR-0041-workspace-topology-contract.md) |
| Understand GPU rendering | `crates/flui-engine/AGENTS.md` | `crates/flui-engine/ARCHITECTURE.md` |
| Create a PR | Run `just ci` first | Fix any failures before committing |

---

## Code Navigation

This repo declares exactly one MCP server in `.mcp.json` — **cratesio**, for crates.io package,
version, and docs.rs lookups. It answers questions about *external* crates only; it knows nothing
about this workspace. Everything else is local tooling:

- **A symbol's definition, its callers, or a rename** — the rust-analyzer LSP when its binary is on
  PATH; otherwise `rg` for the name, then `read` the hits. A rename without an LSP means finding
  every call site with `rg` first — never a blind search-and-replace.
- **String literals, log messages, comments, attributes** — `rg`, always. An LSP can't see them.
- **A file you can already name** — `read` it. Don't search for what you can open.

Individual developers may have extra servers configured at user scope (a code-graph server, a
notes vault); those are personal setup, not a repo contract — never assume one is present, and
never make a workflow here depend on it.

---

## Project Overview

FLUI is a Flutter-inspired declarative UI framework for Rust with a three-tree architecture (View → Element → Render) and a `wgpu`-backed GPU rendering engine. Foundation layers are stable; higher layers land incrementally. Phase status lives in [`docs/ROADMAP.md`](docs/ROADMAP.md); architecture contracts in [`docs/FOUNDATIONS.md`](docs/FOUNDATIONS.md).

## Tech Stack

Versions and the dependency set live in the root `Cargo.toml` (`[workspace.dependencies]`) — read
them there. What the manifest can't tell you:

- **Layering:** crates form a DAG, foundation → core → rendering → framework → app. Dependencies
  point one way down that DAG; see [`docs/FOUNDATIONS.md`](docs/FOUNDATIONS.md)
- **Platform:** native Win32, AppKit, and headless backends, with `winit` only as a fallback
- **Diagnostics:** `tracing` only — **no `println!`, `eprintln!`, or `dbg!` in shipped code** (CI enforces this in foundation/tree/macros crates via port-check trigger #15). *Emitting* is universal; *installing a subscriber* is a composition-root decision that lives in `flui-log` and never in a library
- **Errors:** `thiserror` (libraries), `anyhow` (applications); panics only per [`docs/PANIC-POLICY.md`](docs/PANIC-POLICY.md) — `expect("BUG: <invariant>")` for internal invariants, never bare `unwrap()` on production paths (`clippy::unwrap_used` gates this)
`crates/flui-rendering/src/lib.rs` itself is a thin 220-line root — density lives deeper in the tree: `pipeline/owner/subtree_arena.rs` (~2.5k lines), `protocol/sliver_protocol.rs` (~1.9k), `storage/tree.rs` (~1.8k), and `protocol/box_protocol.rs` (~1.8k) are the densest files; budget accordingly.

## Build & Development Commands

This project uses **`justfile`** for build automation. Install [`just`](https://just.systems) and
run `just --list` for the full recipe set — every recipe is categorised and documented there, so
don't look for a duplicate list here.

**`just ci` is the gate to run before any commit.** It chains `fmt-check` → `inventory-check` →
`runtime-conformance-check` → `port-check` → `clippy` → `test` → `test-doc`; running the pieces
individually is for narrowing a failure, not a substitute.

Two invocations `just --list` won't teach you:

```bash
cargo test -p flui-objects --test render_object_harness  # catalog guard for render objects
just port-check-verbose                                  # per-trigger pass/fail + marker totals
```

Additionally, CI gates on two checks with no `just` recipe:
- **`taplo fmt --check`** — TOML formatting (config: `.taplo.toml`)
- **`typos`** — spell checking (config: `typos.toml`)

## Architecture Constraints (port methodology)

These are enforced by `scripts/port-check.sh` in CI and locally via `just port-check`. Violating them will fail CI. See [`docs/PORT.md`](docs/PORT.md) for the full list of 22 refusal triggers plus FR-033.

| Rule | Why |
|------|-----|
| **ID offset pattern** — slab indices are 0-based; public IDs (`ViewId`, `ElementId`, `RenderId`, `LayerId`, `SemanticsId`) are 1-based `NonZeroUsize`. Insert: `slab_index + 1`; lookup: `id.get() - 1`. | Consistent across all crates |
| **No `RwLock<Box<dyn RenderObject>>`** in render/view/layer/painting/engine storage | Lock-or-interior-mutability problem |
| **No `async fn` in build/layout/paint/composite/render hot paths** | Sync pipeline per Flutter contract |
| **No `unimplemented!()`/`todo!()` in production code** (except platform-init stubs on linux/ios/android) | Triggers #8 |
| **No `Box<dyn View>` as struct fields** in element child collections | Recursive-box storage rejected |
| **No `From<f32>` for unit wrappers** in flui-geometry | Unit-barrier escape hatch guard |
| **Sanctioned `dyn` boundaries only** — see the allowlist in port-check.sh trigger #9 | FR-036 registry |
| **No locks in public API** (`pub fn -> MutexGuard`, `pub field: Mutex<...>`) | SP-6: locks behind private fields |
| **No dependency on `flui-log` outside `flui-app`, `flui-cli`, and the facade** | Libraries emit through `tracing` and must not reach the backend; `just inventory-check` enforces the `allowed_dependents` list in `docs/workspace-layers.toml` |
| **No `println!`/`eprintln!`/`dbg!`** in foundation/tree/macros crates | Use `tracing` macros |
| **No lifecycle-only presentation capability inside `build`/`perform_layout`/`paint`** — `rebuild_handle()` (ADR-0018), `post_frame_handle()` (ADR-0021), `text_input_handle()` (ADR-0030), and `focus_manager()` (ADR-0037) are acquired in `ViewState::init_state` / `did_change_dependencies` and used later | Trigger #22: mutation or scheduling from a frame phase can create an unbounded rebuild loop, re-enter the frame transaction, or leak ownership across presentations. Adding a capability to `BuildContext` means adding its token to `scripts/check-frame-capability-scope.sh` in the same change |

## Testing Quirks

- **CI runs nextest fully parallel; nextest gives each test its own process.** `AppBinding` is fully retired (deleted, not slimmed — its fields dissolved into `AppRuntime`/`UiRealm`/`PresentationState`), and `UpdateScheduler` (formerly `Scheduler`, renamed in #556) is no longer singleton-backed either: each `UiRealm` owns a fresh `UpdateScheduler` value (`RealmServices::construct()`, `crates/flui-app/src/app/runtime.rs`), built new for every realm a test installs. `RenderingFlutterBinding` and `SemanticsBinding` left the singleton graph the same way earlier in this retirement (`SemanticsBinding` is gone entirely, not slimmed). None of this family's old test-serialization locks exist any more — `SINGLETON_WINDOW_TEST_LOCK`, `SCHEDULER_PHASE_TEST_LOCK`, and `SEMANTICS_TEST_LOCK` are all deleted along with the process-wide state each one guarded, and there is nothing left in `AppBinding`/`UpdateScheduler`/`RenderingFlutterBinding`/`SemanticsBinding` for a parallel test run to race on. What genuinely remains process-global (not realm- or test-scoped) is named per-symbol in `docs/runtime-contract.toml`'s ambient-reach ratchet — `flui-assets`'s `Registry::global` and flui-painting's free-standing `FONT_SYSTEM`. A new test that mutates one of THOSE resources (not a realm or its scheduler, which a fresh construction already isolates per test) needs its own explicit serialization — a `Mutex`/lock scoped to that test module — rather than reaching for a shared lock from this now-deleted family.
- **Singleton retirement is complete (#553).** The transitional at-most-one-instance construction guard `UiRealm` used to enforce (`REALM_CLAIMED`, `UiRealmError::AlreadyExists`) and its own dedicated test lock are both deleted: any number of `UiRealm`s may be constructed and driven concurrently, on one thread or several — see `two_realms_coexist_same_thread` / `two_realms_two_threads_no_shared_state` / `dropping_realm_a_cannot_wake_realm_b` / `cross_realm_duplicate_global_key_mounts_succeed_in_both` in `crates/flui-app/src/app/ui_realm.rs`. The `impl_binding_singleton!` macro and the `HasInstance`/`BindingBase` trait pair that backed every one of these bindings are deleted entirely from `flui-foundation` — there is no ambient-singleton pattern left in the workspace for a new binding to reach for.
- **`flui-platform`'s Linux-runnable suite runs in CI, filtered by mechanism, not by test name.** The CI `test` job's dedicated flui-platform step runs `cargo nextest run -p flui-platform --all-features` (default features alone silently skip the entire winit backend — see `crates/flui-platform/AGENTS.md`) under two green-by-construction devices: `FLUI_HEADLESS=1` routes `current_platform()` to the `HeadlessPlatform` mock (fixes the tests that call `open_window` outside `Platform::run`'s `on_ready` callback — an ordering requirement of the winit event-loop model, not a display-server issue), and `xvfb-run` gives the 9 winit-internals unit tests that construct `WinitPlatform::new()` directly a real (if virtual) X11 connection for clipboard init. All 175 runnable tests pass this way (5x-verified stable locally); doctests need neither device and are not excluded from the `doc-test` job either. What remains excluded, and why: the Windows, macOS, and Android backends are never linked or executed anywhere — `STATUS_HEAP_CORRUPTION` (ROADMAP-TRACKER item H9) is a Windows-only crash that cannot reproduce on the ubuntu-latest runners this CI uses, so there is nothing to gate it on yet; Android's own build needs the NDK's cross-linker, which these runners do not have; `cross-typecheck` lints all three backends (clippy, no link, no tests) as the only coverage they get. `just test-ci` mirrors the CI step and needs `xvfb-run` locally (`apt install xvfb` on Debian/Ubuntu).
- **Multi-window / `WindowPolicy` testing (issue #555's final slice).** `flui_app::WindowPolicy` (`SeparateRealms` default, or `SharedRealm`) is the embedder-facing knob `flui_app::open_secondary_window` consults when opening a second top-level window; `flui_app::ExitPolicy` (default `OnLastWindowClosed`) governs when the platform loop exits once every hosted window has closed, and is now LIVE-WIRED into the real winit/headless backends via `flui_platform::traits::Platform::set_exit_policy_hook` (a new default-no-op trait method only the winit and headless backends override) — a backend deciding "every window I track just closed" no longer exits unconditionally; it consults this hook, which `runner.rs::install_exit_policy_hook` wires to `AppRuntime::should_exit`. Both policy modes are covered end-to-end under `HeadlessPlatform` in `crates/flui-app/src/app/runner.rs`'s `realm_dispatch_tests` module (`two_realms_via_separate_windows_policy_share_nothing`, `one_realm_two_windows_policy_routes_by_presentation`, the live-loop exit-policy probe, and the hot-restart probe) plus `crates/flui-platform/tests/headless.rs`'s own hook-level tests, driven entirely through the public `Platform`/`PlatformWindow` surface (`open_window`, `.close()`, `on_quit`, `set_exit_policy_hook`) — never internal `MockWindow` access. **Named gap, not silently assumed:** a window `open_secondary_window` opens carries no widget content and never renders a frame — see that function's own doc for the two reasons (rendering is one canonical frame-pump closure per BACKEND today, pinned by `crates/flui-app/tests/runner_frame_ordering.rs`'s own mechanical guards, not per-window; and `UiRealm::attach_root_widget` is wired to a realm's primary presentation only). A genuine live two-window winit smoke test was investigated and found infeasible under this workspace's standard test harness: winit refuses to construct an `EventLoop` outside the process's main thread (`Initializing the event loop outside of the main thread is a significant cross-platform compatibility hazard`), and every existing test in `platforms/winit/platform.rs` already works around this by never calling `EventLoop::builder().build()` at all — introducing `any_thread()` (unused anywhere else in this codebase) to force it through was judged a bigger, differently-risky change than this slice's own scope, not folded in under time pressure.
- **Live E2E smoke (`just live-smoke`, CI's `live-smoke` step)** — `tools/live-smoke` drives a REAL windowed demo with REAL X11 input (XTEST) under Xvfb and asserts on captured pixels and the exit code. It is the only executing coverage of the band ABOVE synthetic event dispatch — platform translation, the event-loop wake chain, window-close teardown — each of which shipped broken while every synthetic gesture test stayed green. It also verifies hidden-surface gating against a REAL occlusion signal (issue #623): an input-transparent cover window drives X11 `VisibilityFullyObscured` → winit `Occluded(true)`, and the check asserts zero GPU submissions mid-fling while covered (oracle: per-present `flui.gpu` trace lines in the captured log), input still serviced at the translation layer, and no-input frame resumption on uncover. The occlusion check's fling drags UPWARD on purpose — its arming wheel-ticks walk the offset toward the top, so a downward fling would be dragging into the room the arming just consumed and would die against the top clamp with delivery perfectly healthy (that exact premise inversion is what made this check fail on `main`); the direction is documented at the drag itself. Input/pixel checks are X11-only; the Wayland close-path teardown ordering has its own variant: **`just live-smoke-wayland`** (CI's `live-smoke-wayland` step) runs the demo under a headless weston compositor and self-closes it through the platform's `FLUI_SELF_CLOSE_AFTER_MS` hook — the same `CloseRequested` arm a compositor close takes, and the only way to drive a Wayland close from a harness at all (no protocol lets one client close another's toplevel). It exists because a wgpu surface torn down after its `wl_surface` segfaulted post-quit on Wayland (issue #713) while the X11 close check stayed green — Xlib tolerates the same out-of-order teardown. Skips with a message when `weston` is absent; CI installs weston explicitly so it can never silently skip there.
- **Render-object harness** — every concrete `RenderBox`/`RenderSliver` must have harness tests. See [`crates/flui-rendering/docs/TESTING.md`](crates/flui-rendering/docs/TESTING.md) for the `RenderTester`/`Probe` API and catalog rules. The catalog CI guard (`render_object_harness.rs`) verifies every exported type appears in `RENDER_OBJECT_TYPES` and has a matching `harness_*` test.
- **Coverage**: `just coverage` (requires `cargo-llvm-cov`)
- **Visual self-verification (no window needed)** — to *see* what a widget tree renders, capture it to a PNG instead of screenshotting a live window: `cargo run -p flui --example screenshot -- <demo> [width] [height] [out.png]` (`<demo>` = `material` \| `cupertino` \| `vertical-slice`), then open the PNG. It mounts the tree through `HeadlessBinding`, extracts the `LayerTree`, and rasterizes it offscreen via `flui_engine::wgpu::HeadlessRenderer` (`crates/flui-engine/src/wgpu/headless.rs`) — same GPU raster path as on-screen, so shadows/blends match. Add a `match` arm in `examples/screenshot.rs` to cover another tree. This exists because OS screenshot tools can't grab the live window under GNOME/Wayland+Mutter (the wgpu/Vulkan surface never lands in the X11 framebuffer, and `wlr-screencopy`/`grim` is unsupported) — a green harness test is necessary but "MVP reported as parity" hides in the pixels the test never looks at (see [Definition of Done](#definition-of-done-anti-cheating)).

## Flutter Parity

When changing render-tree, sliver, layout, paint, hit-test, semantics, scheduling, or parent-data behavior, **check `.flutter/` first**. Preserve behavioral contracts unless FLUI has an explicit documented divergence. The `.flutter/` and `.gpui/` directories are read-only architectural references — adapt patterns to FLUI idioms (Arity system, Ambassador delegation, no nullability).

**Read the reference for *what* and *why*, then write Rust from that understanding — do not transcribe.** Loyalty is to observable behavior (output, edge cases, ordering), not to Dart's structure, naming, or file layout. Confirm the match before reporting done — see [Definition of Done](#definition-of-done-anti-cheating).

**Both references are gitignored local clones, so either can be absent — check before citing one.** `ls .flutter` costs nothing and a missing reference has already produced hollow "verified against Flutter" claims here. Restore `.flutter/` with a sparse shallow clone (~62 MB):

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/flutter/flutter.git .flutter
cd .flutter && git sparse-checkout set packages/flutter/lib packages/flutter/test
```

`.gpui/` is the same kind of local clone (from the Zed repository) and is consulted far less often; restore it only when a task actually calls for it.

If a reference is unavailable, say so explicitly instead of reasoning from memory — an unverified parity claim is worse than a stated gap.

## Documentation

| Document | Path | When to read |
|----------|------|-------------|
| **Foundations** | `docs/FOUNDATIONS.md` | Architecture contract, locked contracts (C1–C9) |
| **Roadmap** | `docs/ROADMAP.md` | Current phase, dependency-ordered phases |
| **Port methodology** | `docs/PORT.md` | Translation rules, refusal triggers, type map |
| **Architecture** | `docs/architecture.md` | Three-tree pipeline overview |
| **Crates map** | `docs/crates.md` | Per-layer crate inventory |
| **Testing** | `docs/testing.md` | Build/test/coverage commands |
| **Panic policy** | `docs/PANIC-POLICY.md` | When `expect("BUG: …")` is allowed vs. `Result`; `clippy::unwrap_used` gate |
| **Runtime contract registry** | `docs/runtime-contract.toml` | Public shipped/planned runtime contracts, classified boundary families, and the checked root-export manifest. It deliberately does not depend on internal design records. Checked by `just runtime-conformance-check`; touching a monitored runtime export means updating it deliberately |
| **Render harness** | `crates/flui-rendering/docs/TESTING.md` | RenderTester API, catalog rules |
| **Logging ownership** | `crates/flui-log/AGENTS.md` | Subscriber policies, native sinks, who may depend on the backend |
| **Crate ARCHITECTURE.md** | `crates/flui-{foundation,rendering,engine,layer,painting}/ARCHITECTURE.md` | Per-crate deep architecture |

## AI Context Files

`AGENTS.md` (this file) is the cross-tool guide, shared by every agent runtime. `CLAUDE.md`,
`mimocode.jsonc`, and `.pi/settings.json` are thin per-runtime shims that point back here —
**keep the substance in this file**, or the runtimes drift apart. `STRATEGY.md` carries product
strategy and the port rules behind the Prime Directive.

## CI Pipeline

CI runs on PR + push to main (+ merge queue). All jobs are gated on the fast `checks` source gate and aggregate into a single **`ci`** job — branch protection requires that one check, so *renaming* a job cannot silently drop a gate (`needs` stops resolving). Two holes remain: a job **added** without editing the aggregator's `needs` list is ungated, and the aggregator counts `skipped` as green, so a job that later gains an `if:` becomes a no-op. All cargo invocations run `--locked`; actions are SHA-pinned (dependabot keeps them current, 7-day cooldown); workflow files are linted by actionlint + zizmor. A scheduled `weekly.yml` (Mondays + `workflow_dispatch`) re-checks RustSec advisories against the committed lockfile and builds/tests against a fresh `cargo update` — early warning, not a merge gate.

The job list and its exact commands live in `.github/workflows/ci.yml` — read them there. What the
workflow file does *not* tell you, and what you will misjudge without it:

- **cross-typecheck is the only gate on the Win32, AppKit, and Android backends.** It lints
  `flui-platform` (`cargo clippy`, not plain `cargo check` — the latter let ~80 deny-level
  violations accumulate unseen before this switched) for `x86_64-pc-windows-msvc`,
  `aarch64-apple-darwin`, and `aarch64-linux-android`; `cargo clippy` does not link — no link, no
  tests, and `flui-platform` is excluded from the `test` job. Green means "compiles clean under
  the workspace lints", nothing more. Before this job existed those backends were only ever
  compiled by whoever happened to develop on that OS, and the Windows one did not compile at all;
  Android joined the matrix later (#556's device-recovery wake fix) for the identical reason —
  it had never been built, type-checked, or linted anywhere in CI, carrying ~24 of its own
  unseen lint violations at the time.
- **miri covers `pipeline::owner` (widened from `pipeline::owner::subtree_arena`)** —
  this now runs every unit test under that module, including `cell.rs`'s `PipelineCell` checkout
  tests and two real-`NodePtr` walks driving `layout_dirty_root` through every reborrow phase of
  `layout_subtree_borrowed_impl` (one straight pass, one cyclic edge exercising the baseline
  callback's in-flight gate — removing that gate fails miri; both predate the widening and are
  unchanged by it). Also new: an owner-local traversal (a full `run_frame` over a real 3-node tree,
  driven through `PipelineCell::with_mut`) and a reentrant-layout walk (a Sliver child that issues
  a mid-layout child-build request against the checked-out owner). Deeper sliver walks and
  intrinsics queries are still not interpreted. Advisory while stabilizing.
- **feature-matrix exists because workspace feature unification hides broken per-crate wiring.** A
  crate whose features only resolve thanks to a sibling's dependency passes a normal build and
  fails here. The same job also runs `just facade-combos`, which compiles every supported `flui`
  facade feature combination (`material` / `cupertino` / `localizations` / `hot-reload` / `golden`)
  **on its own**, against the `flui` package alone — a `--workspace` build is not evidence about
  the facade surface — and asserts via `cargo tree` that `flui-hot-reload` is *absent* from
  `flui-app`'s default normal graph rather than merely unused by it.
- **wasm-check excludes 7 crates** — the mio/uuid CLI stack and the dlopen-based hot-reload path,
  none of which can work on wasm32.
- **gpu-test runs the readback suite on WARP** (windows-latest) and is merge-blocking. On an oracle
  mismatch the harness dumps the actual frame as a PNG to `FLUI_READBACK_DUMP_DIR` and uploads it
  as an artifact — fetch that before theorising about a pixel diff.
- **test failures upload insta `.snap.new` candidates as artifacts** — review them rather than
  regenerating snapshots blind.

## Important Config

- **Toolchain:** development toolchain pinned in `rust-toolchain.toml` to `1.98.0` with `rustfmt` + `clippy` components. The pin is deliberately NOT the MSRV floor (`rust-version = "1.97"`), so a future MSRV freeze does not hold the developer back from stable diagnostics; only the `msrv` CI job exercises the floor
- **Cargo profiles:** dev `opt-level = 1` (faster runtime) + `debug = "line-tables-only"` (backtrace file:line only — matches CI; variable/type DWARF was the bulk of `target/debug/deps`), deps `opt-level = 2` + `debug = false` (deps carry no debuginfo at all; raise it for one package to step into it — a global `-C debuginfo=` rustflag, from `RUSTFLAGS` or a user-level cargo config, overrides any `debug =` key silently and without error, since rustflags append after the profile flag); `dbg` profile (`inherits = "dev"`, `debug = "full"`) is the opt-in full-type-info build for a step-debugger; release `lto = "thin"`, `codegen-units = 1`, `strip = "debuginfo"` (the symbol table is retained so `perf`/flamegraph/minidumps can resolve frames — measured cost +935 KiB; DWARF from the std rlibs is still dropped, 23.5 MB → 4.19 MB). Local disk: `target/debug/deps` is the largest consumer on a 28-crate wgpu workspace (incremental is off via `[profile.dev] incremental = false` in the root `Cargo.toml` — `.cargo/config.toml`'s `[env] CARGO_INCREMENTAL = "0"` feeds sccache but does not itself reach cargo's profile resolution) — artifacts accumulate per RUSTFLAGS/feature/toolchain fingerprint with no size cap; run `just sweep` periodically (cargo-sweep: current-toolchain + 7-day prune). CI sets `CARGO_INCREMENTAL=0` + `CARGO_PROFILE_DEV_DEBUG=line-tables-only` and reclaims ~25 GB of runner bloat before building.
- **Build jobs:** 8 (set in `.cargo/config.toml`)
- **Android examples** require `cargo-ndk` + Android NDK (not in workspace default-members)
- **WASM examples** require `wasm-pack` (not in workspace default-members); use `just web-server` for the dev server

## Error Triage

When you hit a build/test error:

1. **Port-check violation** → check `docs/PORT.md` for the trigger ID. The pattern you introduced is banned by the architecture contract.
2. **Clippy warning** → run `just clippy` to see workspace-wide. Fix the warning, don't suppress it.
3. **`unimplemented!()`/`todo!()` in production** → implement or gate behind `cfg(test)` / platform-init exemption.
4. **Render-object harness failure** → every exported `RenderBox`/`RenderSliver` must appear in `RENDER_OBJECT_TYPES` with a matching `harness_*` test. See `crates/flui-rendering/docs/TESTING.md`.
5. **Test flake (flui-app shared state)** → `AppBinding`/`UpdateScheduler`/`SemanticsBinding` are retired, not singletons any more (each `UiRealm` owns its own fresh `UpdateScheduler`; `AppBinding`/`SemanticsBinding` are deleted types), so a flake in this family means a test is mutating a *genuinely* process-global resource — one of the named ambient residuals in `docs/runtime-contract.toml`'s ambient-reach ratchet (`Registry::global`, `FONT_SYSTEM`), not the realm/scheduler. Add an explicit lock scoped to that test module; do not reach for `--test-threads=1`, and do not look for `SINGLETON_WINDOW_TEST_LOCK`/`SCHEDULER_PHASE_TEST_LOCK` — both are deleted along with the state they used to guard.
6. **Type mismatch across crate boundary** → check if you're using the wrong ID type (1-based vs 0-based). See ID offset pattern above.

## Definition of Done (anti-cheating)

An agent reporting "done" makes a claim that later work is built on. A green gate is **necessary but not sufficient** — gates can be satisfied without implementing the behavior. The recurring failure mode in this repo is **"MVP reported as parity"**: a change passes the harness and port-check but silently diverges from Flutter on untested edges.

**Before reporting a render/layout/paint/lifecycle change done:**

1. **Verify against `.flutter/`.** Open the corresponding Flutter source and confirm edge-case behavior matches — or is a *documented* divergence. An audit finding without a `.flutter/` cross-check is a hypothesis, not a fact.
2. **No fake-passing.** Never satisfy a gate by:
   - special-casing the test/harness input instead of implementing the behavior;
   - returning a stub / `Size::ZERO` / empty value that happens to pass;
   - narrowing a test to only what the partial impl handles;
   - reporting intrinsics, baselines, or hit-test as working when they return defaults.

   If a behavior is not implemented, **say so explicitly** — do not paper over it.
3. **Harness evidence.** Every concrete `RenderBox`/`RenderSliver` carries harness tests (catalog CI guard). New behavior needs a test that would *fail* without the change.
4. **Report scope honestly.** "X done" from a prior session ≠ parity — re-verify. State what is implemented vs deferred and *why*; never imply completeness you did not check.

> Rationale: the same guardrails Git's own Rust reimplementation (GitButler's Grit) had to encode for its agents — *"you gotta be super explicit with the ground rules"* — because agents will pass through to the reference or fake a feature to make tests green unless it is explicitly forbidden.

## Agent Rules

- **Decompose chained shell commands** — run each step separately so failures are inspectable
- **Never run destructive git operations** without explicit user permission
- **Honor the architecture contract** — cross-check against `docs/FOUNDATIONS.md` and `docs/ROADMAP.md`
- **Logging via `tracing` only** — no `println!`, `eprintln!`, or `dbg!` in shipped code
- **Verify before committing** — for flui-rendering work: `cargo test -p flui-rendering`, `cargo fmt --package flui-rendering -- --check`, `cargo clippy -p flui-rendering --all-targets -- -D warnings`
- **Prefer behavior-first ports** — translate Flutter semantics into Rust-native structure, keep edge-case behavior loyal
- **No internal process-ID markers in code** — comments, doc-comments, file names, and function/test names must not encode private review/planning history (`Cycle N`, audit finding IDs, `PR #NNN review`, agent-pass IDs, bare `U##` step-citations, spec `SC-NNN` success-criteria numbers). State the invariant or rationale in plain English instead — a reader shouldn't need a planning artifact that may not outlive the project to understand why the code is shaped this way. A marker is acceptable only when its meaning is defined beside its use (for example, a test-case ID in the same file's legend) or mechanically load-bearing (`FR-NNN`/`ADR-NNNN` references grepped by a checker). Repository sweeps may exclude only archival/planning roots: `docs/{audits,brainstorms,ideation,plans,research,superpowers}`, `.rust-studio/specs`, `specs`, and `openspec`; shipped docs such as crate `ARCHITECTURE.md` files and `docs/ROADMAP-TRACKER.md` remain in scope. This defines the denominator, not a claim that every in-scope hit has already been removed; known residue is tracked in issue #644.
