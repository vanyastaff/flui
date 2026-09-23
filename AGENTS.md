# AGENTS.md

The one guide for every agent runtime and human contributor (`CLAUDE.md` just imports it). It
records what isn't derivable from the code: the project's design stance, the rules the gates
enforce, and the conventions the gates can't check.

---

## What FLUI is

A declarative UI framework for Rust: Flutter-shaped at the protocol level, Rust-shaped
everywhere else. Five trees — `View` (immutable config) → `Element` (lifecycle, reconciliation)
→ `RenderObject` (layout / paint / hit-test) → `Layer` (retained compositing), with `Semantics`
alongside for accessibility — then `flui-engine` → `wgpu`. Pre-1.0: breaking changes are cheap
now and expensive once consumers exist, so fix a bad shape instead of working around it.

## Prime Directive

1. **Flutter is a reference, not a spec.** Its three-tree model, lifecycle ordering and
   layout/paint/hit-test protocol are the starting point; structure and style are idiomatic Rust
   (compile-time child arity, `NonZeroUsize` IDs, slab arenas, `thiserror`/`Result`). Matching a
   Flutter contract means naming it and proving it with a test. Diverging on purpose is fine —
   record it (an ADR when protocol-level, a `## Mapping decisions` entry in the crate's
   `ARCHITECTURE.md` when local) and have a test assert the shipped behavior. Losing a behavior
   by accident is never fine. **Leapfrog zones (ADR-0027)** — multi-window ownership,
   runtime/scheduling topology, concurrency, presentation architecture — aren't bound by
   Flutter's widget-tree semantics at all. `.flutter/` and `.gpui/` are optional gitignored
   reference clones; cite the revision you actually read.
2. **Look at the field before settling on a design** — Compose, SwiftUI, and the Rust UI crates
   (egui, Iced, Xilem/Masonry, Bevy UI, GPUI, Dioxus, Slint) — for API shape, architecture and
   style. Where Flutter has no strong contract (animation curves, velocity prediction, color
   interpolation, input smoothing) it isn't even the baseline; propose the best shape you find.
3. **Done means verified and recorded** — see [Definition of Done](#definition-of-done).

## Codebase map

27 crates under `crates/` plus the `flui` facade (`src/`), strictly layered. The checked
authority is `docs/workspace-layers.toml` (enforced by `inventory-check`); `docs/crates.md` is
the readable version. Bottom to top:

- **Values & primitives** — `flui-geometry`, `flui-types`, `flui-foundation`, `flui-macros`
  (View derives).
- **Substrate** — `flui-tree` (tree traits), `flui-platform` (windows, input, IME, clipboard;
  every `windows::*`/`objc2::*` type stays inside it), `flui-scheduler` (frame phases),
  `flui-painting` (records into a `DisplayList`), `flui-interaction` (event routing, gestures),
  `flui-assets`, `flui-log`.
- **Compositing** — `flui-layer`, `flui-semantics`, `flui-animation`.
- **Render machine** — `flui-rendering` (the `RenderBox`/`RenderSliver` protocols),
  `flui-objects` (the concrete render-object catalog), `flui-engine` (layers → `wgpu`).
- **Spine & catalog** — `flui-view` (View/Element, `BuildContext`, reconciliation),
  `flui-widgets`, `flui-testing` (deterministic headless frame driver on a virtual clock),
  `flui-material`, `flui-cupertino`, `flui-localizations`.
- **Composition roots** — `flui-app` (per-window `UiRealm`s, the run loop), `flui-cli`,
  `flui-devtools`, `flui-hot-reload`, and the facade.

The non-obvious invariants live in the per-crate `ARCHITECTURE.md` files (engine, foundation,
layer, painting, platform, rendering, scheduler, widgets) — read the one for the crate you're
changing before changing it.

## Working here

- **Isolate each task in its own worktree**; the shared checkout stays on `main`:
  `git worktree add -b <area>/<slug> ../flui-wt-<slug> origin/main`. Review someone else's PR
  from your own directory (`gh pr diff`/`checkout`), not inside their worktree.
- **Commits** `area: what changed`, one logical change each. **PRs** are one task each, with
  `just check-changed` green first; CI is the proof. Risky PRs get the `full-ci` label. Use
  `Refs #N`; `Closes`/`Fixes #N` only when merging should close it (GitHub's linker ignores
  negation around it).
- **Red main:** fix forward within the hour, or revert. A red heavy run on main or nightly opens a
  "CI is red on main" issue; close it once main is green.
- **Hands off unless the task is about them:** `.github/workflows/`,
  `docs/runtime-contract.toml`, `docs/workspace-layers.toml`, `docs/archive/`, and `Cargo.lock`
  edited by hand.
- **No internal process-ID markers** (`Cycle N`, `PR #NNN review`, `Phase B`) in code or docs —
  state the invariant, not the history that produced it. `U##`/`SC-NNN` are documented
  exceptions; `FR-NNN`/`ADR-NNNN` are fine because a checker greps them. Archival roots are
  exempt (`docs/{audits,brainstorms,ideation,plans,research,superpowers}`,
  `.rust-studio/specs`, `specs`, `openspec`).
- **A new gate** is a justfile recipe *and* a step in CI's `checks` job — a recipe alone never
  reaches the merge path. A doc pulled in with `include_str!` is source: keep it out of
  `DOCS_ONLY` in `scripts/lib/change_scope.py`.

## Commands

| Need | Run |
|------|-----|
| Before a PR | `just check-changed` — fmt + clippy + nextest over changed crates and their dependents (the same scope script as CI's fast lane) |
| Full local gate | `just ci` = `just gate` (fmt, text, inventory, runtime-conformance, panic-policy, toolchain, port-check, clippy, doc-strict) + tests + doctests; the pre-push hook runs `just gate` |
| CI heavy jobs locally | `just ci-full`; `just doctor full` names any missing tool; job table in `docs/testing.md` |
| One crate / one test | `cargo nextest run -p <crate>`, `just test-crate <crate>`, `just test-name <crate> <test>` |
| Other targets (no link) | `just cross-typecheck` — clippy for Win32 / AppKit / Android / iOS |
| Examples | `just example-hello`, `just example <name>`, `just example-list` |
| Render-object catalog | `cargo test -p flui-objects --test render_object_harness` |
| Port rules detail | `just port-check-verbose` |
| Toolchain | `rust-toolchain.toml` is the source of truth; pre-1.0 the MSRV tracks latest stable. `scripts/check-toolchain-consistency.sh` keeps every copy in sync |

Gotchas: nextest doesn't run doctests (`cargo test --doc`). A flaky test that isn't yours usually
mutates a genuinely process-global resource (`Registry::global`, `FONT_SYSTEM`) — scope a lock
to that test module rather than serializing the suite. The dev host is shared and
memory-limited: one compiling worker, a shared `CARGO_TARGET_DIR`; a docs-only change needs only
the script gates.

## Architecture Constraints

Enforced by `just port-check` / CI. The 24 refusal triggers + FR-033, with the reasoning behind
each, are in [`docs/PORT.md`](docs/PORT.md).

| Rule | Checked by |
|------|------------|
| **ID offset** — slab indices are 0-based; public IDs (`ViewId`, `ElementId`, `RenderId`, `LayerId`, `SemanticsId`) are 1-based `NonZeroUsize`: insert `slab_index + 1`, look up `id.get() - 1` | `port-check` |
| No `RwLock<Box<dyn RenderObject>>`; no `async fn` in build/layout/paint/composite/render | `port-check` |
| No `unimplemented!()`/`todo!()` in production code (linux/ios/android init stubs excepted); no `Box<dyn View>` child fields | `port-check` #8 |
| No `From<f32>` for flui-geometry unit wrappers; `dyn` only at sanctioned boundaries | `port-check` #9 (allowlist) |
| No locks in public API (`pub fn -> MutexGuard`); no `println!`/`eprintln!`/`dbg!` in foundation/tree/macros | `port-check` |
| `flui-log` is a dependency only of `flui-app`, `flui-cli` and the facade | `inventory-check` |
| Presentation capabilities (`rebuild_handle`, `post_frame_handle`, `text_input_handle`, `focus_manager`) are acquired in `init_state`/`did_change_dependencies`, never in `build`/`perform_layout`/`paint` | `port-check` #22, `check-frame-capability-scope.sh` |
| `thiserror` in libraries, `anyhow` in apps; `expect("BUG: <invariant>")` for internal invariants, no bare `unwrap()` in production | `clippy::unwrap_used`, [`docs/PANIC-POLICY.md`](docs/PANIC-POLICY.md) |

## ADR Policy

ADRs (`docs/adr/`) can be revised freely, but explicitly: a new ADR with
`Supersedes: ADR-XXXX`, and `Superseded-by: ADR-YYYY` added to the old one. Shipped code that
silently disagrees with an accepted ADR is a defect — either the code is wrong or the ADR needs
superseding.

## Extending FLUI

| Adding | What it takes |
|--------|---------------|
| **Render object** (`RenderBox`/`RenderSliver`) | Implement in `flui-objects` (protocol in `flui-rendering`) → register in `RENDER_OBJECT_TYPES` → `harness_*` tests in `render_object_harness` → Flutter equivalent, or its absence, in `## Mapping decisions` |
| **Widget** | `View`/`ViewState` in `flui-widgets` or the facade, backed by a render object → `SemanticsConfiguration` for assistive tech → a test that fails without it → divergences recorded |
| **Platform capability** (a new `BuildContext` handle) | Lifecycle-acquired (ADR-0018/0021/0030/0037) → backend in `flui-platform` with no platform types leaking out → token in `check-frame-capability-scope.sh` → a test that fails without it → ADR if protocol-level |
| **Crate** | `docs/crates.md` "Adding a New Crate", [ADR-0041](docs/adr/ADR-0041-workspace-topology-contract.md) |
| **Example using `material`/`cupertino`** | `[[example]] required-features = [...]` (`just facade-combos` relies on it) |

## Definition of Done

A green gate proves the gates pass, not that the behavior exists — the harness and port-check
can't see an uncovered path. So a change is done when:

- new behavior has a test that fails without the change, and every concrete
  `RenderBox`/`RenderSliver` has harness tests;
- each Flutter divergence is deliberate, recorded (ADR or `## Mapping decisions`), and asserted
  by a test — an unrecorded divergence counts as a regression.

## Where to read next

| Question | Read |
|----------|------|
| Is it planned? What changed recently? | `docs/ROADMAP.md`, `CHANGELOG.md` |
| Render / layout / paint change | `docs/PORT.md` (rules, triggers, type map), then `.flutter/` |
| Dependencies, layering, a new crate | `docs/workspace-layers.toml`, root `Cargo.toml` `[workspace.dependencies]`, `docs/crates.md` |
| Writing a frame-driving test | `docs/testing.md` (use the shallowest tier that can fail), `crates/flui-rendering/docs/TESTING.md` |
| Contracts, pipeline, panics | `docs/FOUNDATIONS.md`, `docs/architecture.md`, `docs/PANIC-POLICY.md` |
| Public runtime-contract surface | `docs/runtime-contract.toml` (`runtime-conformance-check`) |
| Planning a large change, git hygiene | [`CONTRIBUTING.md`](CONTRIBUTING.md) |
