# AGENTS.md

The one guide for every agent runtime and human contributor (`CLAUDE.md` just imports it). It
records what isn't derivable from the code: the project's design stance, the rules the compiler
and gates enforce, and the conventions they can't check.

---

## What FLUI is

A declarative UI framework for Rust: Flutter-shaped at the protocol level, Rust-shaped
everywhere else. Five trees — `View` (immutable config) → `Element` (lifecycle, reconciliation)
→ `RenderObject` (layout / paint / hit-test) → `Layer` (retained compositing), with `Semantics`
alongside for accessibility — then `flui-engine` → `wgpu`. Pre-1.0: breaking changes are cheap
now and expensive once consumers exist, so fix a bad shape instead of working around it.

## Design stance

- **Flutter is a reference, not a spec.** Its three-tree model, lifecycle ordering and
  layout/paint/hit-test protocol are a good starting point, and its tests are a useful floor for
  behavior. Structure, API and style are idiomatic Rust (compile-time child arity, `NonZeroUsize`
  IDs, slab arenas, `thiserror`/`Result`). Diverge whenever the result is better; record why —
  an ADR for a cross-crate contract, a `## Mapping decisions` entry in the crate's
  `ARCHITECTURE.md` for a local one — and let a test pin the behavior you ship. Multi-window
  ownership, runtime/scheduling topology, concurrency and presentation architecture aren't bound
  by Flutter at all (ADR-0027). `.flutter/` and `.gpui/` are optional gitignored reference clones.
- **Look around before settling.** Compose, SwiftUI and the Rust UI crates (egui, Iced,
  Xilem/Masonry, Bevy UI, GPUI, Dioxus, Slint) often have the better shape; where Flutter has no
  strong contract (animation curves, velocity prediction, color interpolation, input smoothing)
  it isn't the baseline at all. Prefer a mature crate over a hand-rolled one.
- **Make rules types, not reviews.** If the compiler can reject a mistake, encode it (arity
  types, sealed traits, `LifecycleContext`); if clippy can, turn the lint on; a comment or a
  grep is the last resort.
- **Frame path is synchronous.** No `async` in build/layout/paint; async lives at IO, scheduler
  and tooling edges, and delivers results to the next frame. Locks guard shared infrastructure
  only: a lock on per-node state touched inside `perform_layout`/`paint` puts contention (and a
  deadlock risk) on every frame, and a lock in a public signature makes callers part of the
  locking protocol.
- **Layers, not micro-crates.** A crate is a layer with one-way dependencies; a feature inside a
  layer is a module (`flui-widgets` stays one crate). Shared code moves down a layer, not
  sideways into a copy.

## Codebase map

28 crates under `crates/` plus the `flui` facade (`src/`), strictly layered. Each manifest
declares its tier and layer in `[package.metadata.flui]` (checked by `cargo xtask workspace`);
`docs/crates.md` is the readable version. Bottom to top:

- **Values & primitives** — `flui-geometry`, `flui-types`, `flui-foundation`, `flui-macros`
  (View derives), `flui-platform-api` (platform contracts: capability traits and window/input
  vocabulary, no OS code).
- **Substrate** — `flui-tree` (tree traits), `flui-platform` (the backends behind those
  contracts: windows, input, IME, clipboard; every `windows::*`/`objc2::*` type stays inside it;
  only `flui-app` depends on it), `flui-scheduler` (frame phases),
  `flui-painting` (records into a `DisplayList`), `flui-interaction` (event routing, gestures),
  `flui-assets`, `flui-log`.
- **Compositing** — `flui-layer`, `flui-semantics`, `flui-animation`.
- **Render machine** — `flui-rendering` (the `RenderBox`/`RenderSliver` protocols),
  `flui-objects` (the concrete render-object catalog), `flui-engine` (layers → `wgpu`).
- **Spine & catalog** — `flui-view` (View/Element, `BuildContext`/`LifecycleContext`,
  reconciliation, signals), `flui-widgets`, `flui-testing` (deterministic headless frame driver
  on a virtual clock), `flui-material`, `flui-cupertino`, `flui-localizations`.
- **Composition roots** — `flui-app` (per-window `UiRealm`s, the run loop), `flui-cli`,
  `flui-devtools`, `flui-hot-reload`, and the facade.

The non-obvious invariants live in the per-crate `ARCHITECTURE.md` files — read the one for the
crate you're changing before changing it.

## Working here

- **Isolate each task in its own worktree**; the shared checkout stays on `main`:
  `git worktree add -b <area>/<slug> ../flui-wt-<slug> origin/main`. Review someone else's PR
  from your own directory (`gh pr diff`/`checkout`), not inside their worktree.
- **Commits** `area: what changed`, one logical change each. **PRs** are one task each, with
  `cargo xtask check-changed` green first; CI is the proof. Before asking for review, review the
  branch against `main` yourself and list only what would block the merge: file and line, why it
  is wrong, how to show it fails. Risky PRs get the `full-ci` label (it runs the extended
  lane: every job, the nightly-only platform jobs included). Use
  `Refs #N`; `Closes`/`Fixes #N` only when merging should close it (GitHub's linker ignores
  negation around it).
- **Red main:** fix forward within the hour, or revert. A red CI run on main or nightly opens a
  "CI is red on main" issue; close it once main is green.
- **Leave these alone unless the task is about them:** `.github/workflows/` (it is the merge
  path, and a change there decides what every other PR must pass); `docs/archive/` (a
  historical record); `Cargo.lock` by hand (cargo regenerates it, a hand edit drifts from the
  manifests).
- **No internal process-ID markers** (`Cycle N`, `PR #NNN review`, `Phase B`, slice/wave labels)
  in code or docs — state the invariant, not the history that produced it. `ADR-NNNN` citations
  are fine. Archival roots are exempt (`docs/{audits,brainstorms,ideation,plans,research,superpowers}`,
  `.rust-studio/specs`, `specs`, `openspec`).
- **A new gate** is a `cargo xtask` command *and* a step in a CI job the `ci` aggregator gates,
  usually `checks` (a check folded into `cargo xtask checks` gets both) — a command alone never
  reaches the merge path. Prefer a lint or a type over a new check. A doc pulled in with
  `include_str!` is source: keep it out of `DOCS_ONLY` in
  `tools/xtask/src/change_scope/classify.rs`.

## Long runs

The maintainer usually hands over a whole task and comes back later.

- If a step doesn't need the maintainer's decision, keep going; put the status in the same
  message as the next action.
- Stop and ask only when you can't proceed without them, or before something irreversible or
  outward-facing: deleting data, force-pushing, merging, publishing, or changes outside your
  worktree.
- For a task of more than a few steps, keep a checklist in `TASKS.md` at the worktree root
  (git-ignored): tick what's done, append what you find. It survives context compaction and
  shows where the run is.
- Split large sweeps (an audit, a migration across many crates) between subagents with
  disjoint files; check each one's evidence before accepting it.
- End every run with three sections: **Waiting on you**, **Changed**, **Found** — with the
  command output behind each claim, and what you could not verify.

## Commands

| Need | Run |
|------|-----|
| Every task | `cargo xtask --help` (crate `tools/xtask`; the alias is in `.cargo/config.toml`). Anything else is a plain `cargo` command |
| Before a PR | `cargo xtask check-changed` — fmt + clippy + nextest over changed crates and their dependents (the same classification as CI's fast lane) |
| Full local gate | `cargo xtask ci` = `cargo xtask gate` (`checks`: fmt, typos, taplo, docs-links, workspace, toolchain, wgsl, …; `lint`; `doc-strict`) + `cargo xtask test` + doctests |
| CI heavy jobs locally | `cargo xtask ci-full`; `cargo xtask doctor full` names any missing tool; job table in `docs/testing.md` |
| One crate / one test | `cargo nextest run -p <crate>`, `cargo nextest run -p <crate> <test> --no-capture` |
| Other targets (no link) | `cargo xtask cross-typecheck` — clippy for Win32 / AppKit / Android / iOS |
| Dependencies | `cargo xtask deps` — cargo-deny (bans, licenses, sources, advisories) over every member, and cargo-shear (`cargo shear --fix` applies its fixes) |
| Examples | `cargo run --example counter`, `cargo run --example <name>` (without a name, cargo lists them) |
| Render-object catalog | `cargo test -p flui-objects --test render_object_harness` |
| Toolchain | `rust-toolchain.toml` is the source of truth; pre-1.0 the MSRV tracks latest stable. `cargo xtask toolchain` keeps every copy in sync |

Gotchas: nextest doesn't run doctests (`cargo test --doc`). A flaky test that isn't yours usually
mutates a genuinely process-global resource (`Registry::global`, `FONT_SYSTEM`) — scope a lock
to that test module rather than serializing the suite. The dev host is shared and
memory-limited: one compiling worker, a shared `CARGO_TARGET_DIR`; a docs-only change needs only
`cargo xtask checks`, which builds xtask and not the workspace.

## What the compiler and gates enforce

| Rule | Enforced by |
|------|-------------|
| Presentation capabilities (`rebuild_handle`, `post_frame_handle`, `focus_manager`, `text_input_handle`, `keep_alive_*`, `pipeline_owner`, `async_driver`, …) are acquired only in `init_state`/`did_change_dependencies` | type system: they live on `LifecycleContext`, which only those hooks receive (ADR-0078) |
| Signals are read in `build`, never written or created there | run-time guard in `flui-view::reactive` (ADR-0074) |
| **ID offset** — slab indices are 0-based; public IDs (`ViewId`, `ElementId`, `RenderId`, `LayerId`, `SemanticsId`) are 1-based `NonZeroUsize`: insert `slab_index + 1`, look up `id.get() - 1` | `NonZeroUsize` + ID newtypes |
| No lock guard held across an `if let`/`match` arm | `clippy::significant_drop_in_scrutinee` |
| No `todo!`/`unimplemented!`/`dbg!` in production (linux/ios/android init stubs carry an `#[expect]`) | clippy `todo`/`unimplemented`/`dbg_macro` |
| No `println!`/`eprintln!` in `flui-foundation`/`flui-tree`/`flui-macros` | clippy `print_stdout`/`print_stderr` |
| No `From<f32>` for `flui-geometry` unit wrappers | `compile_fail` doctests in `flui-geometry` |
| No bare `unwrap()` in production; by convention `expect("BUG: <invariant>")` for internal invariants, `thiserror` in libraries, `anyhow` in apps ([`docs/PANIC-POLICY.md`](docs/PANIC-POLICY.md)) | `clippy::unwrap_used`; the conventions are review |
| Crate layering (a normal or build dependency points to a lower tier, or a smaller `order` in the same tier, unless the dependent lists it in `edge-exceptions` with the ADR that removes it; and, until `layer` is removed, to the same layer or lower — ADR-0081); no framework crate but `flui-app`, `flui-cli` and the facade links `flui-log`; none but `flui-app` depends on `flui-platform` (ADR-0082); none but `flui-localizations`, `flui-app` and the facade depends on Material or Cupertino, in any form (ADR-0028); manifests inherit the workspace keys and lints; no unreachable test file; unique ADR numbers | `cargo xtask workspace` (`[package.metadata.flui]` in each manifest) |
| Import direction between a crate's top-level modules (flui-widgets): non-test code names only modules in lower layers, through re-exports too; `#[cfg(test)]` code is exempt; a refused edge needs a dated `exceptions` entry naming the ADR that removes it | `cargo xtask module-dag` (`[package.metadata.flui.modules]`) |
| No dependency that no code uses, no test-only dependency in `[dependencies]`, no `[workspace.dependencies]` entry nothing inherits (an optional dependency, or one a feature names, is only warned about); licenses, sources and banned crates per `deny.toml`, including crates std now replaces (`once_cell`, `cfg-if`, …); RustSec advisories | `cargo xtask deps` (cargo-shear, cargo-deny; CI's `deps` job) |
| Links from the non-archival markdown into the checkout resolve without climbing out of it: files, `#heading` anchors, and this repository's own `main` URLs | `cargo xtask docs-links` (lychee, offline), part of `cargo xtask checks` |

## ADR Policy

ADRs (`docs/adr/`) record cross-crate decisions the code still follows. Revise them freely but
explicitly: a new ADR with `Supersedes: ADR-XXXX`, and `Superseded-by: ADR-YYYY` added to the old
one. Code that silently disagrees with an accepted ADR is a defect — either the code is wrong or
the ADR needs superseding. Delete an ADR whose decision no longer exists in the code; git keeps
the history.

## Extending FLUI

| Adding | What it takes |
|--------|---------------|
| **Render object** (`RenderBox`/`RenderSliver`) | Implement in `flui-objects` (protocol in `flui-rendering`) → register in `RENDER_OBJECT_TYPES` → `harness_*` tests in `render_object_harness` → note a Flutter divergence in `## Mapping decisions` |
| **Widget** | `View`/`ViewState` in `flui-widgets` or the facade, backed by a render object → `SemanticsConfiguration` for assistive tech → a test that fails without it |
| **Platform capability** (a new handle) | Trait in `flui-platform-api`, backend in `flui-platform` with no platform types leaking out → a method on `LifecycleContext`, not `BuildContext`, so `build` cannot reach it → a test that fails without it → ADR if it changes a cross-crate contract |
| **Crate** | A workspace `members` entry and `[package.metadata.flui]` `tier`, `tier-kind`, `order` and `layer = N` (names: root `[workspace.metadata.flui] tiers` and `layers`); `cargo xtask workspace` checks the rest. Why a crate must be a layer: `docs/crates.md` "Adding a New Crate", [ADR-0041](docs/adr/ADR-0041-workspace-topology-contract.md) |
| **Example using `material`/`cupertino`** | `[[example]] required-features = [...]` (`cargo xtask facade-combos` relies on it) |

## Definition of Done

A green gate proves the gates pass, not that the behavior exists. So a change is done when:

- new behavior has a test that fails without the change, and every concrete
  `RenderBox`/`RenderSliver` has harness tests;
- each Flutter divergence is deliberate, recorded (ADR or `## Mapping decisions`), and asserted
  by a test — an unrecorded divergence counts as a regression.

## Where to read next

| Question | Read |
|----------|------|
| Is it planned? What changed recently? | `docs/ROADMAP.md`, `CHANGELOG.md` |
| Dependencies, layering, a new crate | root `Cargo.toml` (`[workspace.metadata.flui] tiers` and `layers`, `[workspace.dependencies]`), `docs/crates.md` |
| Writing a frame-driving test | `docs/testing.md` (use the shallowest tier that can fail), `crates/flui-rendering/docs/TESTING.md` |
| Contracts, pipeline, panics | `docs/FOUNDATIONS.md`, `docs/architecture.md`, `docs/PANIC-POLICY.md` |
| Planning a large change, git hygiene | [`CONTRIBUTING.md`](CONTRIBUTING.md) |

## Review guidelines

Pull requests are reviewed by Codex, which reads this section; a human reviewer can use it the
same way. fmt, clippy (pedantic, `unwrap_used`, the lints in the table above), rustdoc and the
script gates already run in CI, so style and anything they catch is not worth a comment.

- **What to report:** defects that would make a maintainer block the merge. Each finding names
  the defect and a concrete failure scenario — the input or sequence that produces the wrong
  result. If you can't construct one, label it a hypothesis. No praise, no restating the diff.
- **Tests:** for each behavior change, find the test that covers it and ask whether it would fail
  with the production hunk reverted. Tests here have passed both ways by reimplementing the
  predicate they pin, asserting that a widget exists rather than that it was laid out or
  painted, pinning a `Send` bound with a type that already satisfies it, counting rebuilds
  through a harness helper that dirties the root itself, or narrowing an assertion to what a
  partial implementation handles. A regenerated `*.snap` is a claim the new output is correct —
  the PR must say what changed and why. A test that mutates genuinely process-global state
  (`Registry::global`, `FONT_SYSTEM`) needs a module-scoped lock, because nextest runs one
  process per test in parallel.
- **Unwired surface:** a new `pub` item that no production path reaches (test, example and
  bench callers don't count) is this repository's most common defect. Flag it unless the PR
  names the follow-up that wires it.
- **Flutter behavior:** a change to render, layout, paint, hit-test, semantics, scheduling or
  reconciliation either keeps Flutter's observable contract (output, edge cases, ordering) or
  records the divergence (ADR or `## Mapping decisions`) with a test for the new behavior. A
  Dart-shaped design is not an improvement by itself.
- **Rendering specifics:** `SliverGeometry { ..SliverGeometry::ZERO }` drops the constructor's
  derived defaults (`layout_extent`, `visible`) and has caused real header bugs; a layout that
  publishes geometry from a stand-in value (ADR-0054); intrinsics, baselines or hit-testing left
  returning defaults while the PR calls the object done; a concrete render object missing from
  `RENDER_OBJECT_TYPES` or its `harness_*` test.
- **Engine:** a `wgpu::Instance` and the surface it must be compatible with are created
  together. A pixel claim needs a readback whose sample points distinguish the fixed code from
  the broken code — rotation about the centre, SSAA area gates and framebuffer rebases have each
  produced tests that passed both ways.
- **Runtime and platform:** state belongs to a realm (scheduler, focus, GlobalKeys), never to the
  process. Only the Linux/headless platform path executes in CI; Win32, AppKit, Android and iOS
  are clippy-only, so a change there is unverified unless the PR shows a run. Event-translation
  changes need the live smoke path, not a synthetic gesture test.
- **`unsafe`:** each block's `SAFETY:` comment names an invariant this code establishes, not a
  restatement of the operation; say so if a safe API would do.
- **Manifests and workflows:** shared dependencies go through `[workspace.dependencies]`;
  features stay additive and every optional dependency sits behind a `dep:` feature; a new crate
  declares its `[package.metadata.flui]` `tier`, `tier-kind`, `order` and `layer`, and
  `wasm = false` if it cannot build for wasm32. In workflows: actions pinned to a full SHA,
  `--locked` on every cargo call, caches saved only on `main`, a job's name equals its key, and a
  new job is listed in the `ci` aggregator's `needs` (a lane-gated one also in `HEAVY_JOBS`,
  `FULL_JOBS` or `EXTENDED_JOBS`, matching its `if:`).
- **Registries and exemptions** (`RENDER_OBJECT_TYPES`, `docs/ROADMAP.md`, a `deny.toml` skip, a
  `typos.toml` word, an `#[expect]`): check that each entry matches the code in the same PR and
  that a new exemption states its reason.
- **Docs:** no process markers (see "Working here"); no hand-maintained completeness claims ("all
  call sites now use X") without the command that showed it; no Flutter-parity claim without the
  reference it was checked against.
