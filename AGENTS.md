# AGENTS.md

> The single agent guide for FLUI — shared by every runtime, no per-crate shims. If a rule is
> enforced by tooling, this file says by what; if it's judgment, it says who decides. Pipeline:
> `View` (config) → `Element` (lifecycle) → `RenderObject` (layout/paint) → `Layer` (retained) →
> `flui-engine` → `wgpu`.

---

## Prime Directive

1. **Flutter is a reference, not a spec.** Start from its three-tree model, lifecycle, and
   layout/paint/hit-test protocol where good — structure and style target Rust as it is now
   (Arity system, `NonZeroUsize` IDs, Slab arenas, `thiserror`/`Result`). Following a Flutter
   contract: name it, prove it with a test. Improving on it: the test asserts the improvement,
   recorded as an ADR (protocol-level) or a `## Mapping decisions` entry in the crate's
   `ARCHITECTURE.md` (local). Losing a behavior by accident is never acceptable; dropping one on
   purpose is a recorded decision. **Leapfrog zones (ADR-0027):** multi-window ownership,
   runtime/scheduling topology, concurrency, presentation architecture aren't bound by Flutter's
   widget-tree semantics. `.flutter/`/`.gpui/` are gitignored optional-reading clones — cite the
   revision you actually read.
2. **Check the market before settling.** Before adopting a design — Flutter's or your own — check
   Compose, SwiftUI, and the Rust field (egui, Iced, Xilem/Masonry, Bevy UI, GPUI, Dioxus, Slint),
   for functionality, architecture, and style alike. Breaking changes are cheap now, expensive
   once consumers exist — don't defer a better shape. Where Flutter has no strong contract
   (animation curves, velocity prediction, color interpolation, input smoothing) it isn't even
   the baseline — propose the market-best shape directly.
3. **Done = verified and recorded.** A test fails without the change; a chosen Flutter divergence
   has an ADR or `## Mapping decisions` entry. "Better than Flutter" with no accounting is as
   unverified as "same as Flutter". See [Definition of Done](#definition-of-done-anti-cheating).

---

## How to Work

- **One git worktree per task, never the shared checkout:** `git worktree add -b <area>/<slug>
  ../flui-wt-<slug> origin/main` (e.g. `platform/win32-ime`). Reviewing another PR: `gh pr
  diff`/`checkout` in your own directory, never `checkout`/`branch` in their worktree.
- **A task is:** goal, crates/files in scope, acceptance criterion, what counts as proof — missing
  one, ask before guessing scope.
- **A report is:** actual command output behind each claim, plus what you could *not* verify and
  why. "Should work" isn't a report.
- **Commits:** `area: what changed`, one logical change per commit. **PRs:** one task, one PR,
  `just ci` green first. `Refs #N` by default; `Closes #N`/`Fixes #N` only when the merge should
  close it (GitHub's linker ignores surrounding negation).
- **Don't touch without an explicit task saying so:** `.github/workflows/`,
  `docs/runtime-contract.toml`, `docs/workspace-layers.toml`, `Cargo.lock` by hand, `docs/archive/`.
- **No internal process-ID markers** (`Cycle N`, `PR #NNN review`, `Phase B`) in code or docs.
  `U##`/`SC-NNN` are documented repo exceptions; `FR-NNN`/`ADR-NNNN` are fine (checker-grepped).
  Archival roots excluded (`docs/{audits,brainstorms,ideation,plans,research,superpowers}`,
  `.rust-studio/specs`, `specs`, `openspec`); residue tracked in issue #644.
- **A new gate is a justfile recipe + a `checks`-job step** — a recipe alone isn't on the merge
  path. A doc pulled in via `include_str!` is source, not docs: list it in `ci.yml`'s paths-filter.

## Commands

| Need | Run |
|------|-----|
| Local gate | `just ci` (fmt, text-check, inventory/runtime-conformance/panic-policy/port checks, clippy, doc-strict, tests, doctests) |
| CI parity | `just ci` is the fast local gate; `just ci-full` also runs every other CI job this host can (feature-matrix, wasm, cross-typecheck, msrv, miri, deny, gpu-test, ...). `just doctor full` names any tool it needs; what stays CI-only, and why: `docs/testing.md` |
| One crate | `cargo nextest run -p <crate>`, or `just test-crate <crate>` / `test-name <crate> <test>` |
| One target (no link/exec) | `just cross-typecheck` — clippies Win32/AppKit/Android/iOS |
| Run an example | `just example-hello` / `example <name>` / `example-list` |
| Render-object catalog guard | `cargo test -p flui-objects --test render_object_harness` |
| Port-check detail | `just port-check-verbose` (per-trigger pass/fail) |
| MSRV check | `bash scripts/check-toolchain-consistency.sh` (in `just gate`) — pre-1.0 tracks latest stable, bumped within a week of release; post-1.0, N-2. Verifies `Cargo.toml`, `clippy.toml`, the `msrv` CI job, all five `flui-cli` templates, the README badge, and `llms.txt` against `rust-toolchain.toml`'s `channel` (source of truth) |
| Flaky test that isn't yours | a genuinely process-global resource (`Registry::global`, `FONT_SYSTEM`) is mutated, not a realm/scheduler — scope a lock to that test module |
| This machine (shared, memory-limited) | one compiling worker, shared `CARGO_TARGET_DIR`, `CARGO_BUILD_JOBS` by RAM; docs-only PR = script gates |

## Architecture Constraints (port methodology)

Enforced by `just port-check`/CI; 24 refusal triggers + FR-033 in [`docs/PORT.md`](docs/PORT.md).

| Rule | Checked by |
|------|------------|
| **ID offset** — slab indices 0-based; public IDs (`ViewId`, `ElementId`, `RenderId`, `LayerId`, `SemanticsId`) are 1-based `NonZeroUsize`: insert `slab_index + 1`, lookup `id.get() - 1` | `port-check` |
| No `RwLock<Box<dyn RenderObject>>`; no `async fn` in build/layout/paint/composite/render | `port-check` |
| No `unimplemented!()`/`todo!()` in prod code (except linux/ios/android init stubs); no `Box<dyn View>` child fields | `port-check` #8 |
| No `From<f32>` for flui-geometry unit wrappers; `dyn` only at sanctioned boundaries (allowlist #9) | `port-check` #9 |
| No locks in public API (`pub fn -> MutexGuard`); no `println!`/`eprintln!`/`dbg!` in foundation/tree/macros | `port-check` |
| No dependency on `flui-log` outside `flui-app`, `flui-cli`, the facade | `inventory-check` (`workspace-layers.toml`) |
| No presentation capability (`rebuild_handle`, `post_frame_handle`, `text_input_handle`, `focus_manager`) acquired inside `build`/`perform_layout`/`paint`, only `init_state`/`did_change_dependencies` | `port-check` #22, `check-frame-capability-scope.sh` |
| `thiserror` (libs), `anyhow` (apps); `expect("BUG: <invariant>")` for internal invariants, never bare `unwrap()` in production | `clippy::unwrap_used`, `PANIC-POLICY.md` |

## ADR Policy

Existing ADRs are revised freely, but **only explicitly**: open a new ADR with
`Supersedes: ADR-XXXX`, add `Superseded-by: ADR-YYYY` to the old one. A silent mismatch between
shipped code and an accepted ADR is a defect — the code is wrong, or the ADR needs a superseding
entry; never an implicit gap.

## Extending FLUI

| Add | Steps |
|-----|-------|
| **Render object** (`RenderBox`/`RenderSliver`) | Implement the trait (`flui-rendering`/`flui-objects`) → register in `RENDER_OBJECT_TYPES` → `harness_*` test (`render_object_harness`) → no `RwLock<Box<dyn>>`/`async` in `perform_layout`/`paint` → Flutter equivalent (or absence) in `## Mapping decisions` |
| **Widget** | `View`/`ViewState` (facade or `flui-widgets`) → back with a render object (row above) → wire `SemanticsConfiguration` for AT → test that fails without the change → `## Mapping decisions` for any divergence |
| **Platform capability** (new `BuildContext` handle) | Lifecycle-acquired (ADR-0018/0021/0030/0037), never usable inside `build`/`perform_layout`/`paint` → backend under `flui-platform`, no `windows::*`/`cocoa::*`/`objc2::*` leaking out → token in `check-frame-capability-scope.sh` → test that fails without it → ADR if protocol-level |
| **Example using `material`/`cupertino`** | `[[example]] required-features = [...]` (`just facade-combos` needs it) |

## Definition of Done (anti-cheating)

A green gate is **necessary, not sufficient** — it can pass without the behavior existing. The
recurring failure, **"MVP reported as done"**: harness and port-check pass, but a behavior on an
uncovered path silently regressed. Its mirror, **"MVP reported as an improvement"**, calls an
accidental divergence "better" with no ADR and no test for it.

1. **Verified and recorded.** Every case is matched or *deliberately* different, the difference
   recorded (ADR / `## Mapping decisions`) and covered by a test asserting the shipped contract.
   An unrecorded divergence is a regression until proven otherwise.
2. **Every concrete `RenderBox`/`RenderSliver` carries harness tests**; new behavior needs a test
   that would *fail* without the change.
3. **Scope reported honestly.** "X done" from a prior session isn't done — re-verify. State
   implemented vs. deferred and why; never imply completeness you didn't check.

## Documentation Map

| Need | Read |
|------|------|
| Feature planned? Recent changes? | `docs/ROADMAP.md`; `CHANGELOG.md` |
| Render/layout/paint change | `docs/PORT.md` (rules, triggers, type map), then `.flutter/` |
| Cross-crate dep, new crate, logging backend | `docs/workspace-layers.toml`, root `Cargo.toml` `[workspace.dependencies]`; new crate: `docs/crates.md` "Adding a New Crate", [ADR-0041](docs/adr/ADR-0041-workspace-topology-contract.md) |
| Writing a frame-driving test | `docs/testing.md` (shallowest tier that can fail), `crates/flui-rendering/docs/TESTING.md` |
| Foundations, contracts C1–C9, pipeline, panic policy | `docs/FOUNDATIONS.md`, `docs/architecture.md`, `docs/PANIC-POLICY.md` |
| Public runtime-contract surface | `docs/runtime-contract.toml` (`runtime-conformance-check`) |
| Per-crate deep architecture (GPU rendering: flui-engine) | `crates/flui-{engine,foundation,layer,painting,platform,rendering,scheduler,widgets}/ARCHITECTURE.md` |
| Contributor workflow, commit/PR conventions | [`CONTRIBUTING.md`](CONTRIBUTING.md) |
