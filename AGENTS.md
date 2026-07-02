# AGENTS.md

> Compact guide for AI agents working in the FLUI repository. Every line answers: "Would an agent likely miss this without help?"

---

## Prime Directive

Three rules, in priority order. They override convenience, never each other.

1. **Port the core, loyal to behavior.** The three-tree model (View → Element → Render), lifecycle, the layout/paint/hit-test protocol, and reconciliation are ported 1:1 from `.flutter/`. *Structure* is Rust-native (Arity system, `NonZeroUsize` IDs, Slab arenas, `Result`/`thiserror`); *behavior* stays loyal. "Make the core better" reverts to Flutter semantics — see [`STRATEGY.md`](STRATEGY.md).
2. **Leapfrog the edges.** Where Flutter has *no strong contract* — animation curves, velocity prediction, color interpolation, input smoothing — propose the market-best abstraction now, not the Flutter one. Breaking changes are cheap today and ossify once consumers exist; do not defer a better shape to "later". (This never touches the widget-tree mental model rule #1 protects.)
3. **Done means verified against the reference.** "Implemented" is not "done", and a green gate is necessary but not sufficient. Before claiming parity or completion, verify against `.flutter/` and the render harness — see [Definition of Done](#definition-of-done-anti-cheating).

---

## Quick Start for AI Agents

**Read this first.** Then read `crates/<crate>/AGENTS.md` for the crate you're working on.

### Decision Tree

```
You need to...
├── Understand the project → read this file + README.md
├── Work on a specific crate → read crates/<crate>/AGENTS.md
├── Find a symbol/function → use Serena (find_symbol, symbol_overview)
├── Find where something is called → use Serena (find_references)
├── Rename across files → use Serena (rename_symbol) — NOT grep+replace
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
| Add a cross-crate dep | Root `Cargo.toml` `[workspace.dependencies]` | `docs/FOUNDATIONS.md` layer rules |
| Understand GPU rendering | `crates/flui-engine/AGENTS.md` | `crates/flui-engine/ARCHITECTURE.md` |
| Create a PR | Run `just ci` first | Fix any failures before committing |

---

## MCP Servers — When to Use What

| Tool | Use When | Don't Use When |
|------|----------|----------------|
| **Serena find_symbol** | Looking for a struct/fn/trait definition | You already know the exact file:line |
| **Serena find_references** | Finding all callers of a function | You need to search for a string literal (use grep) |
| **Serena rename_symbol** | Renaming across the codebase | Renaming a local variable in one function (use edit) |
| **Serena symbol_overview** | Getting file structure/outline | You need to read the full file (use read) |
| **rust-analyzer-mcp** | Hover info, diagnostics, code actions | Symbol search (use Serena) |
| **rust-mcp-server** | cargo check/clippy/test for a crate | Symbol-level code navigation (use Serena) |
| **rust-docs** | Crate documentation, dependency trees | Local crate code (use Serena) |
| **cratesio** | Searching crates.io for packages | Local workspace queries |
| **grep** | Searching for string patterns, log messages | Finding symbol definitions (use Serena) |
| **read** | Reading a known file | Exploring unknown code structure (use Serena) |

**Rule of thumb:** If you're about to do 3+ grep/read calls to find something, use Serena instead.

---

## Project Overview

FLUI is a Flutter-inspired declarative UI framework for Rust with a three-tree architecture (View → Element → Render) and a `wgpu`-backed GPU rendering engine. Foundation layers are stable; higher layers land incrementally. Phase status lives in [`docs/ROADMAP.md`](docs/ROADMAP.md); architecture contracts in [`docs/FOUNDATIONS.md`](docs/FOUNDATIONS.md).

## Tech Stack

- **Rust 1.96**, edition 2024, workspace of ~20 crates (foundation → core → rendering → framework → app layers)
- **Graphics:** `wgpu` 29.x, `lyon`, `glyphon`, `cosmic-text`, `glam`
- **Platform:** native Win32, AppKit, headless backends + `winit` 0.30 fallback
- **Diagnostics:** `tracing` only — **no `println!`, `eprintln!`, or `dbg!` in shipped code** (CI enforces this in foundation/tree/macros crates via port-check trigger #15)
- **Errors:** `thiserror` (libraries), `anyhow` (applications)
- **Async runtime:** `tokio` 1.43 LTS

## Key Entry Points

| File | Purpose |
|------|---------|
| `Cargo.toml` | Workspace manifest with `[workspace.dependencies]` — all shared deps pinned here |
| `crates/flui-types/src/lib.rs` | Foundation types and unit system |
| `crates/flui-geometry/src/lib.rs` | Geometry primitives (Point, Rect, Size, transforms) |
| `crates/flui-engine/src/lib.rs` | GPU rendering engine entry |
| `crates/flui-rendering/src/lib.rs` | Render tree — the densest crate |
| `crates/flui-view/src/lib.rs` | View + Element tree |
| `crates/flui-app/src/lib.rs` | Application framework entry |
| `examples/hello_world.rs` | Minimal desktop bootstrap |

## Build & Development Commands

This project uses **`justfile`** for build automation. Install [`just`](https://just.systems) and run `just` for the full recipe list.

### Most-used recipes

| Recipe | What it does |
|--------|-------------|
| `just check` | Fast type-check (no codegen) |
| `just build` | Build the workspace |
| `just test` | Run all tests |
| `just clippy` | Lint gate: `cargo clippy --workspace --all-targets -- -D warnings` |
| `just fmt` | Format with rustfmt |
| `just fmt-check` | Format check (CI gate) |
| `just inventory-check` | Docs / justfile crate inventory drift guard |
| `just ci` | Full local CI: `fmt-check` → `inventory-check` → `port-check` → `clippy` → `test` |
| `just example-hello` | Platform smoke test |
| `just port-check` | Port-methodology refusal triggers |
| `just port-check-verbose` | Per-trigger pass/fail + marker totals |

### Single-crate and single-test commands

```bash
just test-crate flui-tree                    # Test one crate
just test-name flui-tree element_id          # Run one test with stdout
cargo test -p flui-rendering --test render_object_harness  # Run a specific integration test
```

### Format & lint (run before any commit)

```bash
just fmt-check    # rustfmt
just inventory-check
just port-check
just clippy       # clippy with -D warnings
```

Additionally, CI gates on:
- **`taplo fmt --check`** — TOML formatting (config: `.taplo.toml`)
- **`typos`** — spell checking (config: `typos.toml`)

## Architecture Constraints (port methodology)

These are enforced by `scripts/port-check.sh` in CI and locally via `just port-check`. Violating them will fail CI. See [`docs/PORT.md`](docs/PORT.md) for the full list of 21 refusal triggers plus FR-033.

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
| **No `println!`/`eprintln!`/`dbg!`** in foundation/tree/macros crates | Use `tracing` macros |

## Testing Quirks

- **CI runs nextest with `--test-threads=1`** due to a pre-existing flui-app singleton-state flake
- **`flui-platform` tests are excluded from CI** (STATUS_HEAP_CORRUPTION investigation in progress)
- **Render-object harness** — every concrete `RenderBox`/`RenderSliver` must have harness tests. See [`crates/flui-rendering/docs/TESTING.md`](crates/flui-rendering/docs/TESTING.md) for the `RenderTester`/`Probe` API and catalog rules. The catalog CI guard (`render_object_harness.rs`) verifies every exported type appears in `RENDER_OBJECT_TYPES` and has a matching `harness_*` test.
- **Coverage**: `just coverage` (requires `cargo-llvm-cov`)

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
| **Render harness** | `crates/flui-rendering/docs/TESTING.md` | RenderTester API, catalog rules |
| **Crate ARCHITECTURE.md** | `crates/flui-{foundation,rendering,engine,layer,painting}/ARCHITECTURE.md` | Per-crate deep architecture |

## AI Context Files

| File | Purpose |
|------|---------|
| `AGENTS.md` | This file — what every agent needs to know |
| `CLAUDE.md` | Thin shim that imports `@AGENTS.md` so Claude Code auto-loads this guide — keep substance here, not there |
| `.mcp.json` | MCP servers (Serena, rust-analyzer, cratesio, etc.) |
| `mimocode.jsonc` | MiMoCode runtime config |
| `.pi/settings.json` | Pi runtime settings |
| `STRATEGY.md` | Product strategy and port rules |
| `justfile` | All build/test/lint recipes |
| `.taplo.toml` | TOML formatter config |
| `typos.toml` | Spell-check config |
| `deny.toml` | cargo-deny (license, advisory, bans) |

## CI Pipeline

CI runs on PR + push to main. Jobs (in dependency order):

1. **checks** — `cargo fmt --check`, `taplo fmt --check`, `typos`, `scripts/check-workspace-inventory.sh`, `port-check.sh`
2. **clippy** — `cargo clippy --workspace --all-targets -- -D warnings` (needs: checks)
3. **test** — `cargo nextest run --workspace --exclude flui-platform --lib --test-threads 1` (needs: checks, Linux only)
4. **bench-compile** — `cargo bench -p flui-rendering --no-run` (needs: checks)
5. **doc** — `cargo doc --workspace --no-deps --document-private-items` with `RUSTDOCFLAGS="-D warnings"` (needs: checks)

## Important Config

- **Toolchain:** pinned in `rust-toolchain.toml` to `1.96.0` with `rustfmt` + `clippy` components
- **Cargo profiles:** dev `opt-level = 1` (faster runtime) + `debug = 1` ("limited" — backtraces + debugger vars, no DWARF/CodeView type info, the bulk of `target/debug/deps` size), deps `opt-level = 2`; `dbg` profile (`inherits = "dev"`, `debug = "full"`) is the opt-in full-type-info build for a step-debugger; release `lto = "thin"`, `codegen-units = 1`, `strip = "symbols"`. Local disk: the `target/debug/incremental` cache (no size cap) is the largest consumer on a 28-crate wgpu workspace — sweep it periodically. CI sets `CARGO_INCREMENTAL=0` + `CARGO_PROFILE_DEV_DEBUG=line-tables-only` and reclaims ~25 GB of runner bloat before building.
- **Build jobs:** 8 (set in `.cargo/config.toml`)
- **Android examples** require `cargo-ndk` + Android NDK (not in workspace default-members)
- **WASM examples** require `wasm-pack` (not in workspace default-members); use `just web-server` for the dev server

## Error Triage

When you hit a build/test error:

1. **Port-check violation** → check `docs/PORT.md` for the trigger ID. The pattern you introduced is banned by the architecture contract.
2. **Clippy warning** → run `just clippy` to see workspace-wide. Fix the warning, don't suppress it.
3. **`unimplemented!()`/`todo!()` in production** → implement or gate behind `cfg(test)` / platform-init exemption.
4. **Render-object harness failure** → every exported `RenderBox`/`RenderSliver` must appear in `RENDER_OBJECT_TYPES` with a matching `harness_*` test. See `crates/flui-rendering/docs/TESTING.md`.
5. **Test flake (flui-app singleton)** → CI uses `--test-threads=1`. If tests fail locally with parallelism, try single-threaded.
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
