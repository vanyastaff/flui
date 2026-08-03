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
Of the crate roots, `crates/flui-rendering/src/lib.rs` is by far the densest — budget accordingly.

## Build & Development Commands

This project uses **`justfile`** for build automation. Install [`just`](https://just.systems) and
run `just --list` for the full recipe set — every recipe is categorised and documented there, so
don't look for a duplicate list here.

**`just ci` is the gate to run before any commit.** It chains `fmt-check` → `inventory-check` →
`port-check` → `clippy` → `test` → `test-doc`; running the pieces individually is for narrowing a
failure, not a substitute.

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

- **CI runs nextest fully parallel.** flui-app's bindings are process-global singletons, but nextest gives each test its own process; the in-process race on `semantics_enabled` is serialized by `SEMANTICS_TEST_LOCK`. A new test that mutates shared binding state must take that lock.
- **`flui-platform` tests are excluded from CI** (STATUS_HEAP_CORRUPTION investigation in progress)
- **Render-object harness** — every concrete `RenderBox`/`RenderSliver` must have harness tests. See [`crates/flui-rendering/docs/TESTING.md`](crates/flui-rendering/docs/TESTING.md) for the `RenderTester`/`Probe` API and catalog rules. The catalog CI guard (`render_object_harness.rs`) verifies every exported type appears in `RENDER_OBJECT_TYPES` and has a matching `harness_*` test.
- **Coverage**: `just coverage` (requires `cargo-llvm-cov`)
- **Visual self-verification (no window needed)** — to *see* what a widget tree renders, capture it to a PNG instead of screenshotting a live window: `cargo run -p flui --example screenshot -- <demo> [width] [height] [out.png]` (`<demo>` = `material` \| `cupertino` \| `vertical-slice`), then open the PNG. It mounts the tree through `HeadlessBinding`, extracts the `LayerTree`, and rasterizes it offscreen via `flui_engine::wgpu::HeadlessRenderer` (`crates/flui-engine/src/wgpu/headless.rs`) — same GPU raster path as on-screen, so shadows/blends match. Add a `match` arm in `examples/screenshot.rs` to cover another tree. This exists because OS screenshot tools can't grab the live window under GNOME/Wayland+Mutter (the wgpu/Vulkan surface never lands in the X11 framebuffer, and `wlr-screencopy`/`grim` is unsupported) — a green harness test is necessary but "MVP reported as parity" hides in the pixels the test never looks at (see [Definition of Done](#definition-of-done-anti-cheating)).

## Flutter Parity

When changing render-tree, sliver, layout, paint, hit-test, semantics, scheduling, or parent-data behavior, **check `.flutter/` first**. Preserve behavioral contracts unless FLUI has an explicit documented divergence. The `.flutter/` and `.gpui/` directories are read-only architectural references — adapt patterns to FLUI idioms (Arity system, Ambassador delegation, no nullability).

**Read the reference for *what* and *why*, then write Rust from that understanding — do not transcribe.** Loyalty is to observable behavior (output, edge cases, ordering), not to Dart's structure, naming, or file layout. Confirm the match before reporting done — see [Definition of Done](#definition-of-done-anti-cheating).

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

- **cross-typecheck is the only gate on the Win32 and AppKit backends.** It type-checks
  `flui-platform` for `x86_64-pc-windows-msvc` and `aarch64-apple-darwin`, and `cargo check` does
  not link — no link, no tests, and `flui-platform` is excluded from the `test` job. Green means
  "compiles", nothing more. Before this job existed those backends were only ever compiled by
  whoever happened to develop on that OS, and the Windows one did not compile at all.
- **miri covers `pipeline::owner::subtree_arena` only** — but that now includes two real-`NodePtr`
  walks driving `layout_dirty_root` through every reborrow phase of `layout_subtree_borrowed_impl`
  (one straight pass, one cyclic edge exercising the baseline callback's in-flight gate — removing
  that gate fails miri). Sliver walks and intrinsics queries are still not interpreted. Advisory
  while stabilizing.
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

- **Toolchain:** development toolchain pinned in `rust-toolchain.toml` to `1.97.1` with `rustfmt` + `clippy` components. The pin is deliberately NOT the MSRV floor (`rust-version = "1.97"`), so a future MSRV freeze does not hold the developer back from stable diagnostics; only the `msrv` CI job exercises the floor
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
5. **Test flake (flui-app singleton)** → a test is mutating process-global binding state without taking `SEMANTICS_TEST_LOCK` (or an equivalent guard). Serialize it; do not reach for `--test-threads=1`.
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
- **No internal process-ID markers in code** — comments, doc-comments, file names, and function/test names must not encode private review/planning history (`Cycle N`, `audit T-N`/`R-N`/`E-N`, `PR #NNN review`, `Codex/Copilot P#`, bare `U##` step-citations, spec `SC-NNN` success-criteria numbers). State the invariant or rationale in plain English instead — a reader shouldn't need `docs/research/`, `docs/plans/`, or a spec doc that may not outlive the project to understand why the code is shaped this way. `FR-NNN` and `ADR-NNNN` stay exempt only where they're mechanically load-bearing (e.g. grepped by `scripts/port-check.sh` triggers FR-033/FR-036) — not as a blanket pass for any formal-looking ID. Workspace swept clean 2026-07-12; don't reintroduce the pattern.
