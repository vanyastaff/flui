# Architecture migration plan

- **Status:** Proposed. Nothing here is scheduled until the owner accepts it. The owner's answers
  of 2026-09-25 to the open questions are applied below (§2).
- **Date:** 2026-09-25
- **Base:** `main` @ `cab06137d`. All `path:line` citations were re-checked in a worktree at that commit.
- **Target architecture:** [design/architecture.md](../../design/architecture.md)
- **Decisions and their ADRs:** [design/decisions.md](../../design/decisions.md) (ADR-0081 to ADR-0097, all Proposed except ADR-0081, ADR-0082, ADR-0083, ADR-0095 and ADR-0097, each accepted in part)
- **Owner decisions and what is still open:** [design/open-questions.md](../../design/open-questions.md)
- **Dynamic linking study:** [design/dynamic-linking.md](../../design/dynamic-linking.md)
- **Findings not yet acted on:** [design/research-findings.md](../../design/research-findings.md)
- **Raw research:** [docs/research/2026-09-25-architecture-review/](../research/2026-09-25-architecture-review/synthesis.md). Where [report-architecture.ru.md](../research/2026-09-25-architecture-review/report-architecture.ru.md) and [report-decisions.ru.md](../research/2026-09-25-architecture-review/report-decisions.ru.md) disagree, this plan follows the decisions report.

This document orders the work that takes the workspace from today's layout to the target
architecture. It says what lands in which wave, in which crates, how each step proves it is done,
and how each step is undone. It does not restate the design or the reasoning behind a decision;
those live in `design/` and in the ADRs.

---

## 1. How to read this plan

**Waves.** W0 is spikes and measurements that decide a design question before code is written.
W1 to W8 are the H0 waves. A wave closes when every step in it has met its acceptance command,
not on a date.

**WIP limit.** Two tracks and one platform slot run at a time, as the owner's plan already sets:

- **Track A:** structure (gates, facade, runtime, and most crate splits).
- **Track B:** capability (reactive core, text, protocol, hot reload). The `flui-platform-api`
  split (D1a) runs here, right after the reactive routing fix, because track A is busy with the
  gates in W1 and the split is what turns the reach gate green.
- **Slot P:** one platform backend at a time (TSF, live evidence, `Send` removal per backend), in
  the owner's order for live evidence: Windows, macOS, Linux, web.

At most one PR that adds a gate is open at a time. A track's first step in a wave is the gate
that later steps in that wave rely on.

**PR size.** S is at most 500 lines of logic, M at most 2k, L is above 2k or a series of PRs.
Moved files and generated code do not count.

**Every step lists:** PR size, crates touched, the acceptance command, the main risk, and the
rollback. A command in the form `cargo xtask <name>` marked **(new)** does not exist yet; the step
that adds it is named. The commands that exist today are the subcommands in
`tools/xtask/src/main.rs:35-105` (`checks`, `workspace`, `check-changed`, `facade-combos`,
`cross-typecheck`, `device`, `bench-collect`, and the rest).

**Definition of done** stays the one in [AGENTS.md](../../AGENTS.md): a test that fails without
the change, harness tests for every concrete render object, and each Flutter divergence recorded
and pinned.

**ADRs.** Each step names the ADR it implements. An ADR moves from Proposed to Accepted in the PR
that ships the first behavior it decides, not before. The back-links on the older ADRs
(`Amended by:`, `Superseded in part by:`) land in the same PR as the acceptance; see step W1-A10.
No gate checks back-link symmetry.

**Publication.** No FLUI crate is published to crates.io before the `!Send` flip and the event
callback signature change have landed (W5-A4, W5-A5; ADR-0091 §1). Every step that publishes,
including a train that would publish `flui-sdk` (W3-A1), waits for them.

**Workflow files.** No step edits `.github/workflows/` on its own. The CI redesign (W0-15, W1-A0)
takes every workflow need in §9 as an input and lands them as one design.

---

## 2. Before W1: owner inputs

These blocked a step below. The owner answered them on 2026-09-25
([design/open-questions.md](../../design/open-questions.md)); the plan follows the answers.

| Input | Blocks | Owner's answer |
|---|---|---|
| Accept the waves and the WIP limit as written | everything | pending with the plan as a whole |
| Is the one-transaction step (W2-A1..A4) part of B0 or B1? | the B0 exit, item [S]4 | B0 |
| The workflow-file changes in §9 | W1-A8 (perf step), W1-A12 (macOS job), W3-A5, W3-P1, W8-A1 | not piecemeal: one CI redesign (W0-15, W1-A0) takes them as inputs |
| `EventCx` shape and the rollback trigger (ADR-0086, panel confidence 0.62) | W5-A1..A4 | typed `&mut EventCx<'_>`, pilot on `flui-cupertino` and the `counter` and `todo` examples, rollback trigger kept |
| Deadline of the `!Send` flip (ADR-0091) | W5-A5, every publication | before the first crates.io publication, with the callback signature change |
| Escape modules for upstream types: ahead of time or on demand (ADR-0089) | W3-A2 | on demand, except `flui_sdk::gpu` |
| Third-party package lag under a lockstep `flui-sdk` (ADR-0088) | nothing in H0 | bump `0.N` on every train; accept the lag until H3 |
| Enable the ja-JP input language on the Windows dev host | W3-P2 | the owner enables it before the IME work |
| crates.io name availability (`flui-sdk`, `flui-reactive`, `flui-protocol`, `flui-platform-api`, `flui-runtime`, `flui-engine-cpu`) | W1-B2a, W2-A1a, W2-B1, W3-A1, W3-B3, W6-A3 | checked in the step that creates each crate; nothing reserved ahead (`flui-runtime`: free on crates.io on 2026-09-26) |
| Copy and paste in text fields: B1 or B2 | W4-A3, W6-B2 | B1, with Form |
| Web in the H0 exit | W7-P1 | rendering and pointer input only; web IME and DOM/ARIA are H1 items |
| Dynamic linking (ADR-0096) | nothing in H0 | deferred; the build-footprint study (W0-14) addresses the build cost instead |

---

## 3. W0: spikes and measurements

Spikes run in a scratch worktree and land nothing on `main` except the number or the decision
they produce, written into the ADR that asked for it. None of them uses a WIP slot on a track
except where marked.

| # | Spike | Time box | Success metric | Feeds | If it fails |
|---|---|---|---|---|---|
| W0-1 | Stable closure of the facade through rustdoc JSON (the public-api measurement) | 2 days | a deterministic, reviewable listing; the item count N of the Stable closure recorded | ADR-0081, ADR-0089, W8-A2 | ADR-0081 records per-module listings instead of one closure |
| W0-2 | `ReadScope` and the move of `Signal` behind it | 2 days | `count.get(cx)` compiles unchanged at call sites; `cargo check -p flui-view -p flui-rendering` on the prototype branch | ADR-0085, W3-B1 | an extension trait in the prelude instead of the upcast. **Outcome (2026-09-26):** prototyped on `spike/readscope`; the contract goes to `flui_foundation::read_scope`, reads take a generic `&S: ReadScope`, and the `flui-reactive` extraction is withdrawn (warm edit, `cargo check -p flui-app`: 15 crates / 5.74 s in foundation against 3 / 3.07 s in `flui-view`; a separate crate below rendering and animation would re-check about 7, inferred from `cargo tree -i`); see ADR-0085 Context and §6 |
| W0-3 | Open capability seam | 1 day | the 136 `&dyn LifecycleContext` sites still compile; a crate outside the workspace registers a capability and receives it; `Unsupported` is typed | ADR-0084, W6-B1 | keep the closed `LifecycleContext` method list and record why. **Outcome (2026-09-26):** prototyped on `spike/capability_seam`; the seam shape holds, and ADR-0084 adopts the corrections (an `&Arc` window in providers, a per-realm registry, conflicts at `Application::run`, no widget capability for cursor, text input or accessibility); see ADR-0084 Context |
| W0-4 | Parley glyph to `GlyphImage` into the existing ADR-0067 atlas, with a stable key (glifo against skrifa) | 1 week | glyph oracle tests green; no process-global font state; the rasterizer runs outside the shaping lock | ADR-0092 (carries ADR-0077 precondition 1), W4-B1 | ADR-0092 stays Proposed; text stays on cosmic-text with `FONT_SYSTEM` on the globals allowlist |
| W0-5 | Subsecond on Windows, macOS and Android | 1 week | a logic edit keeps state; an edit to a `State` type restarts the realm; no stale TLS or static breaks after a patch; patches reach code behind `Box<dyn ElementBase>` vtables created before the patch | ADR-0094, W5-B1 | keep the dlopen path (and its documented hazards, W0-11) until a later spike |
| W0-6 | `LayoutCallbackScope` for lazy sliver children | 3 days | a lazy band converges in one pass without a double `RefCell` borrow | a new ADR that supersedes ADR-0017 §3 and the ADR-0003 fixpoint | nothing changes; ADR-0083's phase order keeps today's layout↔build fixpoint |
| W0-7 | Retained layer identity and a differ | 1 week | damage for one text change is at most the box plus a margin; blit cost and memory per window recorded | ADR-0087, W6-A1 | `DamageRegion::Full` stays, and ADR-0087 records the measured cost |
| W0-8 | CPU raster backend (vello_cpu against tiny-skia) | 1 week | conformance scenes bit-identical on three OSes with pinned SIMD; the list of effects excluded from pixel goldens | ADR-0087, W6-A3 | goldens stay semantic-only through B3 |
| W0-9 | crates.io names | 1 hour per crate | the name checked on crates.io in the step that creates the crate, a fallback chosen if it is taken; nothing is reserved ahead (owner decision) | each crate-creating step | the fallback name |
| W0-10 | ADR-0074 go/no-go restated | done | ADR-0085 §5 states the go/no-go as met, citing ADR-0074 §8.1 and the landed field masks (commit `588251a1c`, PR #1260); the owner confirmed removing the feature on 2026-09-25 | W3-B1 | — |
| W0-11 | The two hot-reload worker hazards, reproduced on Windows once | 1 day | a recorded run of `hot_reload_counter` on Windows with a logic edit; either an access violation after unload or a clean reassemble; either a working `on_tap` rebuild or the "called before host registered a hook" log; **both hazards documented in `flui-hot-reload`'s crate docs whatever the result**, until Subsecond replaces the path | `crates/flui-hot-reload` crate docs, ADR-0094 | document both as hypotheses in the crate docs |
| W0-12 | Duplicate builds from per-crate `testing` features | folded into W0-14 | — | — | — |
| W0-13 | One-off `cargo xtask device windows-a11y` on a `windows-latest` branch run, with a pre-check that separates a host limit from a regression | 1 day, scheduled by W0-15 | PASS, or CANNOT_VERIFY with the pre-check's reason | W0-15, W3-P1 | the step stays on the maintainer's host |
| W0-14 | **Build-footprint study** (owner decision): `target/` size by artifact kind (rlibs and rmeta, test binaries, incremental caches, debug info); test binary count; duplicate builds of the upper stack from per-crate `testing` features (distinct `libflui_rendering-*` hashes); peak memory per compiling job; a target directory per worktree against a shared one. Levers measured before and after: test-target consolidation, sccache, split debug info, nextest archives, cargo-sweep, one shared target | 1 week | a table of the numbers with the commands that produced them, and each lever kept or rejected on its measured effect | W0-15, §5 feature policy in [design/architecture.md](../../design/architecture.md) | record the numbers; no lever lands without a measured gain |
| W0-15 | **CI redesign** (owner decision): one design for the workflows instead of the piecemeal edits of §9. A fast PR lane over the changed crates and their dependents; full runs only when needed (`full-ci` label, `main`, nightly, release); no platform-specific heavy runs during active work. Inputs: every row of §9, W0-13, W0-14's numbers, open issue #1279 | 1 week (design), then W1-A0 | a written design the owner signs off, listing each §9 row and where it runs | W1-A0 and every step that needed a workflow edit | the current workflows stay as they are |

**Why W0-10 is done, not a gate.** The signals feature comment still says it stays opt-in
"until the #1090 field-mask registry lands" (`Cargo.toml:676-678`), and the view crate's comment
says off "until the go/no-go measurement in the ADR lands" (`crates/flui-view/Cargo.toml:119-123`).
Both preconditions are met: the measurement is ADR-0074 §8.1, and #1090 shipped as `588251a1c`.
The decisions report made reactive step 1 wait on a gate that has already passed. ADR-0085 §5 now
says so, and the owner confirmed the removal, so step 1 is not blocked by a stale comment.

**Why W0-14 and W0-15 come first.** The owner's cost problem is test and build time, disk use and
memory growth, not the final link that dynamic linking would shorten. Every later wave adds gates
and jobs; designing the lanes and measuring the footprint before W1 keeps each new gate on a lane
chosen once, instead of a workflow edit per step.

**Dynamic linking is already measured.** Its numbers are in
[design/dynamic-linking.md](../../design/dynamic-linking.md). It is not a W0 spike and not an H0
step; see §7.

---

## 4. Waves at a glance (H0)

| Wave | Track A | Track B | Slot P | Milestone |
|---|---|---|---|---|
| W1 | The CI redesign (W1-A0); then gates: tiers and `tier-kind`, reach, markers, file-length, globals, module DAG, "core names no official crate", each with `--self-test`; perf counters and baseline | Reactive routing fix (a signal write reaches the realm that owns the signal); then D1a, the mechanical split of `flui-platform-api` | TSF spike | B0 |
| W2 | `flui-runtime` extraction: one frame transaction, defined by type; `flui-testing` above the runtime; widgets' inline test modules move to `tests/` | `flui-protocol` extraction; `tools/desktop-mcp` becomes the `flui-mcp` library | TSF implementation; D1b Win32 step one (owner-only callback storage, off-owner registration refused; signatures keep `Send`) | B0 |
| W3 | `flui-sdk` with the train guard; packages move to `packages/` with `git mv` in the same PR as their switch to the sdk; `cargo package` parity check; facade `default = []`; delete `flui-tree` and `flui-localizations`; delete the `flui-app → flui-hot-reload` edge | Reactive step 1, step 2a, step 2b | Windows live evidence (IME and Narrator) | B1 |
| W4 | Router, Form with copy and paste in text fields, `#[flui::main]`, `App::new(any View)`, `#[derive(RenderView)]` | Parley, per-realm text, shaped runs; IME text-store contract and its conformance kit | D1b AppKit | B1 |
| W5 | `EventCx` in four slices with a pilot and a rollback trigger; the `!Send` flip of UI traits | Subsecond hot reload, after the globals shrink | Linux live evidence; D1b winit | B1 / B2 |
| W6 | Retained layer identity, damage, the raster contract in `flui-layer`, `flui-engine-cpu` | Open capability seam (core-required and optional classes); haptics and file dialogs as the first plugins | Dialogs per OS; D1b UIKit and Android | B2 |
| W7 | Value cleanup (`GenId` for `LayerId`/`SemanticsId`, `Color`, `Rect`/`Axis`, `Pixels` equality); dead-surface removal | devtools server, `flui mcp`, semantic and CPU goldens | Web: rendering and pointer input (IME and a11y are H1, §8) | B3 |
| W8 | `release-check`, public-API gating on the three Stable crates, generated docs | Notes built from crates.io outside the repository | none | B4 |

The hard chains across tracks: dropping the `signals` feature (W3-B1) and the `EventCx` slices
(W5-A1..A4) both depend on W0-10, which is done; and no crates.io publication (W3-A1's first
train, W8-B1) happens before W5-A4 and W5-A5 have landed. Everything else follows from crate
dependencies.

---

## 5. Steps

### W1 (B0)

Order inside track A: the CI redesign (W1-A0) first, so every gate below lands on a lane chosen
once; then, from the decisions report, tier gate and reach, then markers and file-length, then
globals and module DAG. One gate PR open at a time.

| Step | What | PR | Crates | Acceptance | Risk | Rollback | ADR |
|---|---|---|---|---|---|---|---|
| W1-A0 | **CI redesign implemented**, as W0-15's signed-off design says: the fast PR lane, the conditions for full runs, and a place for every §9 row (the macOS `cargo xtask ci` run, the perf job, the Windows UI Automation step, the `windows-latest` matrix entry, the facade build without Material, `package-check`, `release-check`); the build-footprint levers W0-14 kept. Labelled `full-ci` | M (series) | `.github/workflows/`, `tools/xtask` | the `ci` aggregator gates every new job; a PR touching one crate runs only the fast lane; W0-14's numbers re-measured after the change | a heavy job lost from the merge path | revert the workflow PR; the old workflows stay in git | none |
| W1-A1 | `tier = V\|C\|S\|R\|K\|H\|pkg`, `tier-kind` and in-tier order in every `[package.metadata.flui]`; the `workspace` check enforces direction by tier; dev edges stay free (ADR-0081 §1), so the four existing dev cycles (`flui-view`, `flui-interaction` and `flui-scheduler` with `flui-testing`, `flui-rendering` with `flui-objects`) are legal; the six refused edges are seeded as `edge-exceptions` on the dependent: `flui-interaction` and `flui-widgets` → `flui-platform` (ADR-0082), `flui-app` and `flui` → `flui-hot-reload` (ADR-0094), `flui` → `flui-material`/`flui-cupertino` (ADR-0088) | M | `tools/xtask`, every manifest, `docs/adr` (the ADR-0078 amendment, W1-A10) | `cargo xtask workspace` 0 findings; `cargo xtask workspace --self-test` **(new flag)** fails on a planted upward edge | the layer model and the tier model disagree during the switch | keep `layer` beside `tier` for one wave, check both | ADR-0081 |
| W1-A2 | "Core names no official crate": `allowed-dependents` generalised by `tier-kind`, reverse direction strict from day one with named, dated exceptions: `flui-testing` dev → `flui-devtools` (`crates/flui-testing/Cargo.toml:106`), `flui-app` optional → `flui-hot-reload` (`crates/flui-app/Cargo.toml:65,108`), the facade's optional and dev edges to `flui-hot-reload` (`Cargo.toml:553,588`) and its optional `flui-material`/`flui-cupertino` edges (`Cargo.toml:559-560`), which are also the only H → `pkg` exceptions to the direction rule (ADR-0081 §1); `flui-cli` (kind `tool`) needs none; forward direction in allowlist mode (Material's core edges and `flui-view/runtime-internals`, the latter removed by W3-A8) | S | `tools/xtask` (`tools/xtask/src/workspace.rs:119,267,416`) | `cargo xtask workspace` green; self-test fails on a planted `flui-widgets → flui-material` optional edge | an exception without an owner lingers | every entry carries its exit step; the allowlist can only shrink | ADR-0081, ADR-0088 |
| W1-A3 | `cargo xtask reach` **(new)**, resolving each root build itself from `cargo metadata --locked --all-features` (Cargo's own resolution unifies features across the workspace, ADR-0081 §2), no target filter, every facade feature combination; K forbid set {flui-platform, winit, android-activity, ndk, windows, objc2-app-kit, objc2-ui-kit, wgpu, flui-engine, flui-app}; `jni`/`windows-sys`/`core-foundation`/bare `objc2` allowlisted with reasons; per-crate `reach-forbid` additions; the three hot-reload `TREE_FACTS` of the facade feature-matrix task move into it as facts | M | `tools/xtask` | `cargo xtask reach --self-test` fails on a planted edge; `cargo xtask reach` green **with three seeded `reach-exceptions` entries**: `flui-hot-reload → windows` and `→ android_log-sys` (exit ADR-0094) and `flui-engine → wgpu` (a grant from ADR-0081). W1-B2a landed first, so no entry names `flui-platform`: `flui-interaction` and the widget harness already depend on `flui-platform-api` instead | false positives through optional FFI | allowlist entries with reasons | ADR-0081 |
| W1-A4 | `markers` **(new)**: no process markers outside the archival roots; `file-length` **(new)**: at most 3000 production lines per file, allowlist seeded by a scan | S + S | `tools/xtask` | each `--self-test` fails on a planted file; `cargo xtask checks` green | the marker regex catches legitimate words | narrow the pattern, never a blanket skip | none |
| W1-A5 | Split the production `impl UpdateScheduler` (`crates/flui-scheduler/src/scheduler.rs:1093-3527`) into submodules by phase, so the file drops off the `file-length` allowlist (3569 production lines when W1-A4 landed, the only entry); in line with `design/architecture.md` "Keep, lighten" for the scheduler. `crates/flui-app/src/app/runner/realm_dispatch.rs` needs nothing: its test module starts at :1691 under `#[cfg(all(test, ...))]`, so it has about 1690 production lines | M | `flui-scheduler` | the entry is gone from `tools/xtask/allowlists/file-length.toml`; `cargo xtask file-length` green; `cargo nextest run -p flui-scheduler` green with the same test count | a moved method changes visibility | revert the split | none |
| W1-A6 | `globals` **(new)**: syn scan of every `static` (`OnceLock`, `LazyLock` and atomics included) and `thread_local!` entry outside `#[cfg(test)]`; allowlist seeded by the scan in the same PR (regex estimates do not seed it). Known entries: `APP_RUNTIME` (`crates/flui-app/src/app/runner/host.rs:46`), `AssetRegistry::global` (`crates/flui-assets/src/registry/mod.rs:83`), `TIME_DILATION` (`crates/flui-scheduler/src/config.rs:43`), `REQUEST_REBUILD` (`crates/flui-hot-reload/src/dispatch.rs:24`), `REGISTRY_STACK` (`crates/flui-view/src/key/registry.rs:204`), `NAVIGATOR_COMMAND_TARGETS` (`crates/flui-widgets/src/navigator/navigator.rs:91`), `FONT_SYSTEM` (`crates/flui-painting/src/text_layout/layout.rs:124`); each entry names its exit ADR (the step is reached through the ADR), or a permanent grant under ADR-0097 with its class | M | `tools/xtask` | `cargo xtask globals --self-test` fails on a planted static; `cargo xtask globals` green | the scan is blind to environment-variable selection such as `FLUI_HEADLESS` | record that limit in ADR-0097; do not claim it | ADR-0097 |
| W1-A7 | `module-dag -p flui-widgets` **(new)**: declared import direction between widgets modules | M | `tools/xtask`, `flui-widgets` | `cargo xtask module-dag --self-test` fails on a planted import; `cargo xtask module-dag -p flui-widgets` green on main | the declared DAG needs a few moves first | allowlist the edges, dated | none |
| W1-A8 | Phase counters and `cargo xtask perf` **(new)**: writes a baseline, non-blocking in B0, blocking at the B1 exit; its CI job runs where W1-A0 placed it. **Baseline recorded** (`crates/flui-widgets/perf/baseline.toml`); `perf --self-test` runs in `checks`; the advisory `perf` job W1-A0 placed (`design/ci.md`) is not in the workflows yet; the scenarios live in `flui-widgets` (a `flui-testing` → `flui-widgets` dev edge widened every change's CI scope) | M | `flui-rendering`, `flui-view`, `flui-testing`, `flui-widgets`, `tools/xtask` | `cargo xtask perf` writes the baseline; `cargo nextest run -p flui-widgets perf_` | counters on the hot path | atomic-free fields behind `cfg(debug_assertions)` | none |
| W1-A9 | Pin the in-process check list: the test in `tools/xtask/src/tasks/checks.rs:149-172` lists every new gate, so a gate cannot silently drop out of `cargo xtask checks` | S | `tools/xtask` | `cargo nextest run -p xtask` | none | none | none |
| W1-A10 | ADR hygiene: back-link lines on the older ADRs for every accepted amendment (the list is in [design/decisions.md](../../design/decisions.md)), each only in the PR that accepts the new ADR, including the lines already missing on ADR-0065 and ADR-0016; **no symmetry gate** (owner decision); the explicit amendment to ADR-0078 (why the new scanners come back after the deletions in `cf46dfe20` (#1283), naming the removed panic allowlist and publish dry-run) lands earlier, in W1-A1, the first gate's change | S | `docs/adr` | `cargo xtask docs-links` green | none | none | ADR-0078 amendment |
| W1-A11 | Upstream types: `#[non_exhaustive]` on `SemanticsRole`/`SemanticsAction`, a generated `ALL` and a test that every role but `None` maps to an AccessKit role; exhaustive matches without `_` only on the incoming action path | S | `flui-semantics` | `cargo nextest run -p flui-semantics` | none | none | ADR-0089 |
| W1-A12 | B0 hygiene: `cargo xtask ci` on `macos-latest` (§9 row 3, placed by W1-A0) and the workspace version bump from `0.2.0-dev` to `0.2.0` (`Cargo.toml:106` and every exact pin) | S + S | workflow, every manifest | the macOS job green; `cargo xtask workspace`; `cargo xtask deps` | the version bump touches every pin | revert the bump | none |
| W1-A13 | Debt ratchets: an undocumented-`unsafe` allowlist seeded by a scan (`clippy::undocumented_unsafe_blocks` switched on module by module) and a multiple-versions ratchet in place of `multiple-versions = "allow"` (`deny.toml:103`), both only shrinking | S + S | `tools/xtask`, `deny.toml`, `flui-platform` | each ratchet fails on a planted new entry; `cargo xtask deps` green | skips without a reason | every entry carries a reason | none |
| W1-A14 | Burn down the `markers` allowlist (`tools/xtask/allowlists/markers.toml`, 91 entries when W1-A4 landed): rewrite each recorded marker as the invariant it stands for, crate by crate, lowering or removing its entry in the same PR | S (series) | every crate with an entry, `tools/xtask` | `cargo xtask markers` green with an empty allowlist | a rewrite loses the reason the label pointed at | revert that crate's PR; its entry comes back at the old count | ADR-0078 |
| W1-B1 | **Reactive routing fix, first.** `UiCommand::SignalWrite` applies to the primary presentation (`crates/flui-app/src/app/ui_realm/commands.rs:450-455` → `presentations.rs:357-358`); route it by `SignalSlot.graph` (`crates/flui-view/src/reactive/mod.rs:78-82`) to the graph that owns the slot. `ForeignGraph` (`mod.rs:267-273`) already detects the mismatch | S | `flui-app`, `flui-view` | a two-window test that writes a signal owned by window B, fails on `main` with `ForeignGraph`, passes after; `cargo xtask check-changed` | the `widgets()` accessor is cfg'd out on Android, iOS and wasm (`presentations.rs:353-356`) | revert; the write keeps failing loudly, never silently | ADR-0085 (conformance with ADR-0074) |
| W1-B2a | D1a: `flui-platform-api` as a mechanical move of the capability traits (`PlatformTextInput`, `PlatformHaptics`, `PlatformDisplay`, `Clipboard`, the `data_transfer` module) and the window and input vocabulary, with re-exports at the old paths from `flui-platform`; the one production import below `flui-app` (`crates/flui-interaction/src/text_input.rs:27`) and the widget harness switch over. `PlatformWindow` waits for W1-B2b: no crate below `flui-app` names it, so moving it changes no reach | M | `flui-platform`, new `flui-platform-api`, `flui-interaction`, `flui-widgets` (test harness) | `flui-platform`'s `allowed-dependents = ["flui-app"]` with `cargo xtask workspace` at 0 findings (2 on `main`); `cargo tree --locked --target all -e normal` of `flui-interaction`, `flui-widgets --features testing` and `flui-platform-api` names none of `flui-platform`, `winit`, `windows`, `objc2-app-kit`, `android-activity`, `ndk`, `tokio`, until `cargo xtask reach` states it (W1-A3 landed after this step, so its reach facts hold with no `flui-platform` entry); `cargo xtask check-changed`; `cargo xtask cross-typecheck` | `#[non_exhaustive]` types now cross a crate boundary | the re-exports stay | ADR-0082 |
| W1-B2b | `PlatformWindow` moves to `flui-platform-api` without `as_winit` (removed; nothing calls it) and without `accessibility()`, which moves to a host-side `HostWindow: PlatformWindow` subtrait returned by `Platform::open_window`, `WindowOpen::Ready` and `PendingWindow` | M | `flui-platform`, `flui-app`, `flui-platform-api` | `cargo xtask cross-typecheck`; a recorded live run on Windows (`cargo xtask device windows-input`) | about 50 `dyn PlatformWindow` sites in `flui-app`, across four backends, two of them clippy-only | revert; W1-B2a stands alone | ADR-0082 |
| W1-P1 | TSF spike (2 weeks, slot P) | spike | `flui-platform` (scratch) | Japanese composition, reconversion, Win+H dictation; Narrator reads the value; result recorded | Win32 has no IME code today | the spike result decides W2-P1's scope | ADR-0090 |

### W2 (B0)

| Step | What | PR | Crates | Acceptance | Risk | Rollback | ADR |
|---|---|---|---|---|---|---|---|
| W2-A1a | Create `flui-runtime` (tier K, `internal`, order 5, layer 6) and move the platform-free presentation lanes into it: the held-input lane (`app/held_input.rs`), the semantics host (`app/semantics_host.rs`) and the commit epoch (`app/epoch.rs`); ADR-0083 accepted in part (§1 placement; the ordering before `flui-testing` waits for W2-A3). Fixes the two §8 defects first, each with a failing test: a GlobalKey lookup under the binding's own frame lock returns (`RegistryBusy`, recorded as a Mapping decision in `flui-view`), and element depth is the tree depth stamped by `ElementTree` | S + S logic, M moved | `flui-view`, `flui-app`, new `flui-runtime` | `global_key_lookup_from_build_during_draw_frame_returns_instead_of_deadlocking`, `global_key_in_a_sibling_binding_resolves_during_this_bindings_frame`, `global_key_lookup_from_dispose_during_detach_returns_instead_of_deadlocking`, the two `global_key_lookup_during_frame` realm tests, `element_depth_is_the_tree_depth_not_the_sibling_slot`, `globalkey_retake_restamps_element_depth_for_the_moved_subtree`; `cargo xtask workspace`; `cargo xtask check-changed`; `cargo xtask reach` green with `flui-runtime` in tier K | a GlobalKey read inside its own presentation's frame now resolves to nothing (it deadlocked before) | revert the series PR; nothing outside `flui-app` imports the moved items | ADR-0083 |
| W2-A1b (done) | The frame-sink seam: `FrameSink` and `SubmitVerdict` (formerly `raster_lane.rs:75-131`, engine-free) move to `flui_runtime::sink`, and `PerformanceStats` to `flui_runtime::performance_stats` (it is fed while the layer tree is built, not at submit); `RasterLane<B>` and `DirectSink` stay in `flui-app` and implement the trait | S | `flui-app`, `flui-runtime` | `cargo nextest run -p flui-runtime -p flui-app` with the same test count | none | revert the move | ADR-0083 |
| W2-A1c (done) | `ExecutionServices` (formerly `app/execution.rs`, ADR-0047) moves to `flui_runtime::execution`, and the runtime gains `allowed-dependents = ["flui-app"]`; `flui-app` keeps `pub use flui_runtime::execution::{ComputeJob, DeterministicExecutors, HostComputePool, HostExecutors, HostIoPool, IoFuture, SpawnError}` so public paths are unchanged | M moved | `flui-app`, `flui-runtime` | `cargo nextest run -p flui-runtime -p flui-app` with the same test count; the wasm32 `Backend::Sequential` lib tests | tokio enters the runtime's normal graph (native only) | revert the move | ADR-0083, ADR-0047 |
| W2-A1d | The realm core: `ui_realm/*`, `presentation.rs`, `presentation_forest.rs`, `lifecycle_state.rs`, `frame_failure.rs` and `media_query_root.rs` minus `from_window`; the ADR-0048 `catch_unwind` in `draw_frame_entered` moves unchanged, the realm tests move with a headless `FrameSink`; `UiRealm::enter_for_close` is deleted (redundant after W2-A1a) | L | `flui-app`, `flui-runtime`, `flui-widgets` | `cargo nextest run -p flui-runtime -p flui-app` with the same test count; `cargo xtask cross-typecheck` | `PresentationState`/`UiRealm` name `PlatformWindow`; blocked on W1-B2b, W2-A1b and W2-A1c | revert the move | ADR-0083 |
| W2-A1e | `Realm::pump(&mut self, clock, sink) -> FrameOutcome` absorbs the runner-side `drive_frame_with_lane` calls; `OwnerHost` replaces `AppRuntime.realms` and is the one trampoline cell (`APP_RUNTIME`, ADR-0097); the production part of `realm_dispatch.rs` (1-1690) moves | L | `flui-app`, `flui-runtime` | the two ADR-0083 verification tests (a post-frame callback sees committed layout; an animation advances between two pumps on a manual clock); `cargo xtask test` | phase-order regressions | a `legacy-frame-driver` cargo feature on `flui-app` for one minor (a feature, not an environment variable, which the globals scan cannot see) | ADR-0083 |
| W2-A2 | One frame transaction, **defined by type**: the phase entry points (`drive_frame_with_lane`, `crates/flui-scheduler/src/scheduler.rs:1874`; the scheduler's begin/draw frame handlers; the view frame entry) become unreachable outside `flui-runtime` through `pub(crate)`, a sealed token or a capability type; a syn scan with self-test only if no type works. The earlier check "no `pub fn pump_frame`" is dropped: a rename defeats it | M | `flui-scheduler`, `flui-view`, `flui-runtime` | a `compile_fail` doctest calling a phase entry from outside the runtime; `cargo xtask checks` | a test helper needs a private entry | a `#[doc(hidden)] __runtime` path, dated | ADR-0083 |
| W2-A3 | `flui-testing` moves above the runtime and drives the real transaction; `HeadlessBinding::pump_frame` (`crates/flui-testing/src/lib.rs:955`, which today calls `drive_frame_with_lane` at :1017) becomes a thin driver over it; absorbs `flui_widgets::testing`; the `flui-widgets → flui-testing` edge (`crates/flui-widgets/Cargo.toml:89`) is deleted | L | `flui-testing`, `flui-widgets`, `flui-runtime` | `cargo xtask test` green with the same test count; the frame-driving code is gone from `flui-testing` | the dev cycle between widgets and testing produces duplicate `TypeId`s (mechanism known, not compiled); `flui-runtime`'s dev edge to `flui-testing` (only `log_capture::disarm_interest_cache`, no shared types) becomes a dev cycle like `flui-view`'s and `flui-interaction`'s, legal under ADR-0081 §1, or is dropped here if a shared type ever crosses it | keep the old driver behind a flag for one wave | ADR-0083, ADR-0044 amendment |
| W2-A4 | The 22 inline test modules of `flui-widgets` move to `crates/flui-widgets/tests/` (separate PR, before W2-A3 lands) | M (moved lines) | `flui-widgets` | same test count before and after; `cargo nextest run -p flui-widgets` | tests reach private items | `__test_access`, doc-hidden, its list pinned by a unit test | ADR-0083 §4 |
| W2-B1 | `flui-protocol`: the typed schema shared by tests, devtools and agents; the role enum copied into `tools/desktop-mcp` is replaced by the protocol type; `SemanticsRole`/`SemanticsAction` (made `#[non_exhaustive]` in W1-A11) move from `flui-semantics` into it (ADR-0089 §3); `tools/desktop-mcp` becomes the `flui-mcp` library | M | new `flui-protocol`, `flui-semantics`, `tools/desktop-mcp` | a mapping test that covers ADR-0080's advertised actions (`expand`, `collapse`, `set_value`), not only that a mapping exists; an agent scenario on `windows-latest`: the normalised outline projection matches between the UIA backend and the in-process backend (advisory job first, on the lane W1-A0 gives it) | the projection is unstable | keep the job advisory | ADR-0095 (amends ADR-0080; wire contract unchanged) |
| W2-P1 | TSF implementation on Win32, sized by W1-P1 | L | `flui-platform` | `cargo xtask cross-typecheck`; a recorded run of `cargo xtask device windows-input` | clippy-only in CI | stays behind the Win32 backend; no public API change | ADR-0090 |
| W2-P2 | D1b Win32, step one of ADR-0082 §4: callback storage becomes owner-thread-only and a registration from any other thread is refused; the signatures keep `+ Send` (`crates/flui-platform/src/traits/platform.rs:158` and the registrations after it). Dropping `+ Send` is step two: one change after every backend, headless included, has done step one and registration goes through owner-proof types | M | `flui-platform`, `flui-app` | `cargo xtask cross-typecheck`; a recorded live run on Windows | not executed in CI | a per-backend adapter that keeps `Send` | ADR-0082 |

### W3 (B1)

Track A's order is fixed by the decisions report: the sdk and the guard first, then each package
moves and switches to the sdk in one PR, then the facade loses its Material feature.

| Step | What | PR | Crates | Acceptance | Risk | Rollback | ADR |
|---|---|---|---|---|---|---|---|
| W3-A1 | `flui-sdk`: a separate Evolving crate, host-free, `0.N` bumped on every train, published in the same run; Stable part as whole-module re-exports under the facade's paths, Evolving part only in named modules (`paint`, `pipeline`, `hooks`, `gpu`); **train guard** `links = "flui_train"` with a trivial `build.rs` in one bottom crate (candidate `flui-foundation`) | M | new `flui-sdk`, `flui-foundation` | a type-identity test (`flui_sdk::<m>::T` is `flui::<m>::T`); a local-registry test that mismatched trains resolve to one train or a resolver error, never E0308; surface measured through rustdoc JSON, reviewed if above ~30 Evolving items | the Evolving surface grows into a second facade | the ceiling in ADR-0088 triggers a review | ADR-0088 |
| W3-A2 | `pub use ::wgpu` (`crates/flui-engine/src/lib.rs:229`) leaves every Stable-reachable path; wgpu interop moves to `flui_sdk::gpu` behind `wgpu-NN`; `android_activity` leaves the app's public surface; `flui-testing`'s `pub use accesskit::{…}` (`crates/flui-testing/src/a11y.rs:36`), which the facade's `testing` module re-exports, moves to an internal path and its helpers take FLUI's own role and action types; the legacy `Window` family with its `raw_window_handle(&self) -> RawWindowHandle` (`crates/flui-platform/src/window.rs:53,196`) is deleted (ADR-0082 §5) | S + S | `flui-engine`, `flui-app`, `flui-sdk`, `flui-testing`, `flui-platform` | the W0-1 listing no longer contains `wgpu::`, `android_activity::` or `accesskit::` on a Stable path | an example uses the re-export | point the example at `flui_sdk::gpu` | ADR-0089 |
| W3-A3 | `cargo xtask package-check` **(new, working name)**: `cargo package` of `flui-sdk` and every official package, each built from its tarball against its neighbours' tarballs in a temporary local registry | M | `tools/xtask` | the command catches a planted missing `include` and a path-only feature | build time | run it in the heavy lane instead of `checks` (§9) | ADR-0088 |
| W3-A4 | **Packages move with `git mv` in the same PR as their switch to the sdk**, one package per PR: `flui-material`, `flui-cupertino`, `flui-devtools`, `flui-hot-reload` into `packages/` as members of the root workspace (one lock, one `cargo metadata`); `tier-kind = "official"`; each package's allowlist entries from W1-A2 removed in its PR; Material examples move to `packages/flui-material/examples`; an out-of-tree fixture package (outside `members`, depends only on `flui-sdk` through `[patch]`) uses a custom widget, a theme extension and a plugin | L (4 PRs) | the four packages, `flui-sdk`, `tools/xtask` | `cargo xtask reach` states `wgpu` absent from `flui-material`'s normal closure; the `flui-hot-reload` PR also carries W3-A6; `cargo xtask package-check` green; `cargo xtask workspace` with the forward allowlist shorter by that package | the sdk surface turns out larger than measured | the sdk item stays Evolving; the package keeps a dated allowlist entry | ADR-0088 (supersedes ADR-0028 in part) |
| W3-A5 | **Facade `default = []`** (today `Cargo.toml:632`), no `material`/`cupertino` features (`Cargo.toml:639-640`; the `hot-reload` feature at `:664` already went with W3-A6); `pub use flui_material as material` (`src/lib.rs:148-149`) and the Material half of the prelude removed; a `flui_material::prelude`; the `flui create` counter template adds `flui-material` explicitly, with a CLI test that generates it and runs `cargo check`; `cargo xtask facade-combos` rewritten; the document list comes from `rg 'flui::(material\|cupertino)\|features.*(material\|cupertino)'` over the repository minus `docs/archive`; CHANGELOG migration note. **Only after W3-A4 lands for Material**, so H0 authors keep a path to Material | M | `flui` facade, `flui-material`, `flui-cli`, `tools/xtask`, docs; the workflow's Material build (§9 rows 1-2) already moved by W1-A0 | `cargo xtask facade-combos`; the generated counter builds; `cargo xtask docs-links` | a document still says `flui::material` | the `rg` list is the checklist | ADR-0088 |
| W3-A6 | **In the same PR as W3-A4's `flui-hot-reload` move** (ADR-0094 §2): delete the `flui-app → flui-hot-reload` edge, the facade's optional and dev edges and its `hot-reload` feature (`Cargo.toml:553,588,664`); the runtime exposes a `DevReloadHook`; the hot-reload exceptions from W1-A2 expire. The Subsecond implementation itself is W5-B1 | S | `flui-app`, `flui-runtime`, `flui-hot-reload`, facade | `cargo xtask workspace` with no hot-reload exception; `cargo xtask reach` | `flui run --hot` loses its driver | the hook lands before the edge goes | ADR-0094 |
| W3-A7 | Delete `flui-tree` (Arity/Slot/Depth into `flui-foundation`, the TreeRead/Nav/Write trio becomes inherent methods) and `flui-localizations` (the RTL table into `flui_widgets::localization`) | M + S | `flui-tree`, `flui-localizations`, `flui-foundation`, their users | `cargo xtask workspace`; `cargo xtask check-changed` | none known | revert | ADR-0081 records the crate fate |
| W3-A8 | D11: `flui-view`'s `runtime-internals` feature (`crates/flui-view/Cargo.toml:118`, enabled by `crates/flui-app/Cargo.toml:90`, `crates/flui-testing/Cargo.toml:51` and `crates/flui-hot-reload/Cargo.toml:27`) becomes an always-compiled `#[doc(hidden)] pub mod __runtime`; its allowlist entry from W1-A2 is removed | M | `flui-view`, `flui-app`, `flui-testing`, `flui-hot-reload` | `cargo xtask workspace` with the entry gone; `cargo doc -p flui-view` shows no `__runtime` page | an application named the feature directly | none in this workspace; a CHANGELOG note | ADR-0081 §4 |
| W3-B1 | **Reactive step 1**: the read contract in `flui_foundation::read_scope` (ADR-0085 §2); `Signal::get/with/try_*` take a generic `&S where S: ReadScope + ?Sized`, `BuildContext: ReadScope`; `ReadScope` is read-only and never returns `Reactive`; subscription only through private `ReaderSink`s that `flui-view` mints (`ElementReads`, from `make_build_ctx` and the `ElementBuildContext` test seam), the graph staying in `flui-view`; a wrong-type handle is `SignalError::TypeMismatch` on read and write, and a refused write marks no reader; writes through the sealed `SignalWriteExt` in `flui_view::prelude`; **the `signals` feature is removed**, with default-build `signals_rebuilds` and idle-frame numbers recorded (depends on W0-10). The drivers, `RebuildSink`, `ScopeRef::detached` and `SignalError::NoGraph` are W3-B2's, because no production caller would reach them in this step. The four crate `signals` features stay as empty shims until the CI `test-features` step that names them is removed, together with the matching `cargo xtask test-features` step. Default-build numbers are in ADR-0085 §5 | M | `flui-foundation`, `flui-view`, the facade, `flui-widgets`, `flui-app`, `flui-testing` manifests | ADR-0085's Verification list: every context shape compiles; a read in `build` through a `flui-testing` mount rebuilds the reader and not its parent; `TypeMismatch` on every path, and a wrong-type write rebuilds no mounted reader; `Reactive` is not a `ReaderSink` (`static_assertions`); `ScopeRef` exposes no graph (trybuild) | upcast ergonomics at call sites | `&dyn ReadScope` with blanket impls for `&S` and `Box<S>` | ADR-0085 |
| W3-B2 | **Reactive step 2a**, inside `flui-view`: readers become `Element(ElementId) \| Layout(RenderId) \| Paint(RenderId)` (hard-wired to `ElementId` today); phase guards reject or defer writes during layout and paint, modelled on `WrittenDuringBuild`; the two non-`Clone` drivers (`ElementDriver`, `RenderDriver`) that mint the sinks, `RebuildSink` in place of `ExternalBuildScheduler`, and `ScopeRef::detached`/`SignalError::NoGraph` (moved here from W3-B1) | M | `flui-view` | a test per guard; `compile_fail`: no driver is `Clone`, `ReadScope` reaches no driver hook | the graph's element-specific paths | revert; step 2b waits | ADR-0085 |
| W3-B3 | **Reactive step 2b**: the first render subscriber, with no move: a render-object field read in paint through `PaintCx: ReadScope`, the scope minted by `RenderDriver` (the first production caller of `PaintCx::with_read_scope`), written outside frame phases. `ScrollPosition` is not the first consumer (it is written in `perform_layout`) | M | `flui-view`, `flui-rendering`, `flui-objects` | a test that fails without the change and shows the repaint itself, not only the subscription | no render consumer by the end of W3 | remove `Reader::Layout`/`Paint`, `RenderDriver` and `PaintCx: ReadScope`; the foundation contract stays | ADR-0085 |
| W3-P1 | Windows automated line: after W0-13 passes, the `windows-a11y` step runs where W1-A0 placed it (a heavy lane, not during active work); `cargo xtask device windows-ime` **(new)** on the maintainer's host or a Hyper-V VM, once the owner has enabled ja-JP | S | `tools/xtask` | the step runs on `windows-latest`; CANNOT_VERIFY is distinct from FAIL | the hosted runner lacks an interactive desktop | keep it on the maintainer's host | none |
| W3-P2 | Windows live session: Japanese IME composition ("toukyou" → 東京) and Narrator reading the label and the field; written to `docs/evidence/windows.toml` (§8) | S | `docs/evidence`, `docs/BETA.md` | the record exists, and `cargo xtask release-check` **(new, W8-A1; freshness part lands here)** accepts it | ja-JP not installed on the host (§2) | Windows stays experimental with an explicit "no IME" | none |

### W4 (B1)

| Step | What | PR | Crates | Acceptance | Risk | Rollback | ADR |
|---|---|---|---|---|---|---|---|
| W4-A1 | Router as the primary navigation API: derive routes, URL as the source of truth, a handle from `init_state` targeting the nearest ancestor; Navigator frozen; `NAVIGATOR_COMMAND_TARGETS` removed from the globals allowlist; `NavigatorCommand` in the command vocabulary becomes a design-neutral navigation intent | L | `flui-widgets`, `flui-macros`, `flui-runtime` | deep link and restore tests; `cargo xtask globals` allowlist shorter by one | the controller pattern for `GlobalKey::with_current_state` (only `&T` today) | keep Navigator primary one more wave | ADR-0093 |
| W4-A2 | Form, `#[flui::main]`, `App::new(any View)`, `#[derive(RenderView)]` with trybuild tests through the facade | L | `flui-widgets`, `flui-macros`, `flui-app`, facade | trybuild suites green; the counter example uses `#[flui::main]` | macro surface grows | macros stay `#[doc(hidden)]` until B3 | none |
| W4-A3 | **Copy and paste in text fields, with Form** (owner decision: B1, not B2): a failing test first; the core-required `Platform::clipboard()` wired from the runtime to `EditableText` (the accessor is dead code today, `crates/flui-app/src/app/runtime.rs:1632-1638`) through the built-in clipboard provider of ADR-0084 §5, which W6-B1 later exposes through `cx.capability::<Clipboard>()` without changing the widget's behaviour; copy, cut and paste in `EditableText` (`crates/flui-widgets/src/text/editable_text.rs:322` documents them as not wired) | M | `flui-widgets`, `flui-app` or `flui-runtime`, `flui-platform` | a copy and paste test through the headless fake clipboard, failing on `main`; a recorded live run on Windows | the built-in provider lands before the open seam | a runtime-internal route, replaced by the capability in W6-B1 | ADR-0084, ADR-0038 |
| W4-B1 | Parley with per-realm `FontContext` over a shared collection; text crosses the display list as neutral shaped runs; rasterization on the raster side; `FONT_SYSTEM` leaves the globals allowlist | L | `flui-painting`, `flui-engine`, `flui-runtime` | glyph oracle tests; `cargo xtask globals` allowlist shorter by one; caret and selection tests over glyph byte ranges | Parley drops glyph byte ranges (reported in the market survey) | cosmic-text path behind a flag for one wave | ADR-0092 (supersedes ADR-0077; ADR-0016 and ADR-0059 on acceptance) |
| W4-B2 | IME pull text-store contract (read, edit, asynchronous lock in the `TS_S_ASYNC` sense) and its **headless conformance kit**, a public, versioned test-support API that external crates run on their own widgets; the built-in text field passes the same kit | M | `flui-platform-api`, `flui-widgets`, `flui-testing` | the kit runs in CI and covers surrogates, graphemes and composition ranges; with TSF, unit tests against a mock `ITextStoreACP` | the contract misses a TSF verb | extend the contract before it is Stable | ADR-0090 |
| W4-P1 | D1b AppKit, step one of ADR-0082 §4 (owner-only storage, off-owner registration refused; signatures keep `+ Send`) | M | `flui-platform`, `flui-app` | `cargo xtask cross-typecheck`; a recorded live run on macOS | clippy-only in CI | per-backend `Send` adapter | ADR-0082 |

### W5 (B1 / B2)

| Step | What | PR | Crates | Acceptance | Risk | Rollback | ADR |
|---|---|---|---|---|---|---|---|
| W5-A0 | Callbacks a widget invokes from `build` (for example `AnimatedSize::build` calling `on_end`) move to a status listener or post-frame, each with a test that fails if the callback still runs in build | S | `flui-widgets` | the new tests | none | none | ADR-0086 |
| W5-A1 | `EventCx` slice 1: types, the `callback(\|cx\| ..)` helper (needed for the HRTB error the probe hit), `WriterSource` on `LifecycleContext` | M | `flui-view` | `compile_fail` doctests: `&mut Writer` does not escape to `'static`; `build` has no path to a `Writer` | helper ergonomics | stop here; nothing public changed yet | ADR-0086 |
| W5-A2 | Slice 2: `Signal::set/update` signatures change; `BuildContext::reactive()` (`crates/flui-view/src/context/build_context.rs:132`) and the public `BuildOwner::reactive()` (`crates/flui-view/src/owner/build_owner.rs:973`) are deleted | M | `flui-view`, `flui-widgets`, `flui-testing` | trybuild: `set` in `build` does not compile; `cargo xtask check-changed` | none known | revert slice 2 | ADR-0086 |
| W5-A3 | Slice 3, **pilot**: a `flui migrate` codemod converts the event setters of `flui-cupertino` (3 sites: `bottom_tab_bar.rs:227`, `button.rs:306,316` under `crates/flui-cupertino/src/`) and the call sites in the `counter` and `todo` examples (`examples/counter.rs`, `examples/todo.rs`) to `&mut EventCx<'_>`, so the pilot covers catalog and application code, with the 92-setter classification table and its grep command in ADR-0086 | M | `flui-cupertino`, examples, `flui-cli` | `cargo xtask check-changed` green; the pilot report records friction against the probe | **rollback trigger**: if the pilot shows HRTB friction or boilerplate clearly worse than the probe, switch to the runtime-guard-only option before 1.0 and supersede ADR-0086 | revert the pilot; ADR-0086 records the switch | ADR-0086 |
| W5-A4 | Slice 4: the codemod crate by crate (`flui-widgets`, then `flui-material`, then examples), `check-changed` green each time; the gesture arena and its `Rc<dyn Fn(Details)>` aliases stay unchanged, widgets wrap recognizer callbacks through `WriterSource` | L (series) | `flui-widgets`, packages, examples | the grep `pub fn on_[a-z_]+` over the catalogs (92 today) shows every event setter classified and migrated | scale of the break; it must land before the first crates.io publication (ADR-0091 §1) | revert per crate; an unmigrated crate keeps the old setter signature | ADR-0086 |
| W5-A5 | `!Send` flip of UI traits: `ListenerCallback`, `Listenable`, render-object metadata, animation-status and post-frame callbacks lose `Send + Sync`; listener and post-frame callbacks get `cx` in the same slice; the `Arc` notifier in `flui-foundation` is deleted; `CustomPainter` gets its signal API | L (series, one callback family per PR) | every UI crate, packages | `assert_not_impl_any!` pins; a gate on `Rc` capture for each `pub fn on_*`; `flui migrate` applied to examples | the break is public; ADR-0091 §1 requires it before the first crates.io publication, together with W5-A4 (owner decision) | revert the family's PR; the family keeps `Send` and its `SignalSender` write path until it is retried | ADR-0091 (schedules it), ADR-0086 §5 (the event context per family) |
| W5-B1 | Subsecond hot reload behind the runtime `DevReloadHook`, after `REQUEST_REBUILD` and `REGISTRY_STACK` leave the globals allowlist; the dlopen path and its three-crate template are deleted only after W0-5 passed | L | `flui-hot-reload`, `flui-runtime`, `flui-cli` | `flui run --hot` keeps state on Windows and macOS (B1 exit); `cargo xtask globals` allowlist shorter by two | Subsecond on Windows unverified until W0-5 | keep the dlopen path, with W0-11's hazards documented | ADR-0094 |
| W5-P1 | Linux live evidence; D1b winit, step one of ADR-0082 §4 (owner-only storage, off-owner registration refused; signatures keep `+ Send`) | M | `flui-platform` | `docs/evidence/linux.toml`; `cargo xtask cross-typecheck` | none | per-backend adapter | ADR-0082 |

### W6 (B2)

| Step | What | PR | Crates | Acceptance | Risk | Rollback | ADR |
|---|---|---|---|---|---|---|---|
| W6-A1 | Retained layer identity keyed by `RenderId` (paint already stamps it) and a differ producing `DamageRegion::Partial` (today only `Full`, `crates/flui-layer/src/scene_snapshot.rs:18-21`, and the lane always sends `Full`, `crates/flui-app/src/app/raster_lane.rs:291`); damage has an off switch that removes its cost | L | `flui-layer`, `flui-engine`, `flui-runtime` | a readback test whose sample points tell the fixed code from the broken code; the ADR-0061 bench in `cargo xtask bench-collect` | stale pixels with a swapchain scissor (hypothesis) | `Full` fallback behind a flag | ADR-0087 (amends ADR-0061) |
| W6-A2 | The raster contract in `flui-layer`: the GPU-free lowering, `RasterBackend`, `PresentDisposition` and a wgpu-free `RasterError`; `RasterOwner` stays in `flui-engine` in H0 | M | `flui-layer`, `flui-engine` | `wgpu` absent from `flui-layer`'s normal closure, which tier R's reach fact states (only `flui-engine` holds the grant); the conformance scenes include a rounded-rectangle and a path clip, so today's bounding-box path clip (`crates/flui-engine/ARCHITECTURE.md:289`) shows up as a named gap or a failure | none known | revert | ADR-0087 |
| W6-A3 | `flui-engine-cpu` (`publish = false` until B3) passes the raster conformance scenes | L | new `flui-engine-cpu` | conformance scenes bit-identical on three OSes | complex filter graphs (reported panic in vello_cpu) | a named fallback list in ADR-0087 | ADR-0087 |
| W6-B1 | Open capability seam: typed registry behind a sealed, object-safe method on `LifecycleContext`; explicit `Application::plugin` registration (no `inventory`/`linkme`); an override hook; the two classes of ADR-0084 §5: core-required capabilities (clipboard and data transfer, text input, accessibility, cursor, window chrome basics) are backend-trait methods, so `PlatformWindow::text_input()` returns `Arc<dyn PlatformTextInput>` and `accessibility()`, on the backend extension trait of ADR-0082 §3, returns `Arc<dyn PlatformAccessibility>`, neither with a default (`crates/flui-platform/src/traits/window.rs:333,352`); `InertTextInput`/`InertAccessibility` serve every backend or build without the service (non-`a11y` Windows, winit and macOS; text input on Win32, Android, iOS and web); only clipboard and data transfer have built-in widget providers, and the built-in clipboard provider from W4-A3 becomes reachable as `cx.capability::<Clipboard>()`; the registry is a parameter of each realm's construction, and `Application::run` validates plugins before the platform starts, returning `AppRunError::CapabilityConflict`; the [AGENTS.md](../../AGENTS.md) "Extending FLUI" row changes in the same PR and states the classification rule | M | `flui-view`, `flui-runtime`, `flui-app`, `flui-platform-api`, `flui-platform` | ADR-0084's Verification table: a fixture on `flui-platform-api` and `flui-sdk` only; a conflict fails `run` before any window opens; every runner-built realm resolves the clipboard; a `compile_fail` pair that a backend without `text_input` or `accessibility` does not compile; `cargo xtask cross-typecheck` for macOS with and without `a11y` | a backend that returns `Unsupported` for a core capability | revert; core capabilities keep their `Platform` methods either way | ADR-0084 |
| W6-B2 | Haptics and file dialogs as the first optional plugins (copy and paste already landed in W4-A3) | M × 2 | `flui-widgets`, `flui-platform`, packages | a headless test per plugin, including `NotRegistered` without it; recorded live runs per OS | clippy-only backends | per-OS behind the capability | ADR-0084 |
| W6-P1 | Clipboard live runs on macOS and Linux, dialogs per OS; D1b UIKit and Android, step one of ADR-0082 §4 each | M × 2 | `flui-platform` | `cargo xtask cross-typecheck`; recorded runs | not executed in CI | per-backend adapter | ADR-0082 |

### W7 (B3)

| Step | What | PR | Crates | Acceptance | Risk | Rollback | ADR |
|---|---|---|---|---|---|---|---|
| W7-A1 | Value cleanup: `GenId` for `LayerId` and `SemanticsId`, `Color` as f32 with a colour space, `Rect`/`Axis`, `Pixels` `Eq`/`Hash` agreement; the [AGENTS.md](../../AGENTS.md) "ID offset" row corrected to match | M × 4 | `flui-geometry`, `flui-types`, `flui-layer`, `flui-semantics` | per-type tests; `cargo xtask check-changed` | churn across crates | one type per PR | none |
| W7-A2 | Dead-surface removal: `ElementBuildContext`, unused geometry vocabulary, `__private`, features with zero `cfg` sites (the `workspace` check fails on them) | M | several | `cargo xtask workspace`; `cargo xtask deps` | none | revert | ADR-0081 |
| W7-B1 | devtools server in-process over `flui-protocol`, `flui mcp`, semantic goldens and CPU goldens (`flui test --golden --accept`) | L | `flui-devtools`, `flui-cli`, `flui-testing` | an agent creates a screen, runs a scenario and asserts the result without a human | none known | advisory until B3 closes | ADR-0095 |
| W7-P1 | Web: the H0 exit states "Notes on the web" for rendering and pointer input only, as the owner decided; the hidden-input IME bridge and the DOM/ARIA mirror are H1 items of their own (§6) | S | `docs/BETA.md`, `flui-platform` docs | a recorded browser run of Notes with pointer input; the BETA page and the web evidence record name the IME and a11y gap | a reader takes "Notes on the web" as full support | the exit wording names the scope | none yet |

### W8 (B4)

| Step | What | PR | Crates | Acceptance | Risk | Rollback | ADR |
|---|---|---|---|---|---|---|---|
| W8-A1 | `cargo xtask release-check` **(new)**: `cargo package` dry-run in publish order, semver-checks against the last tag, evidence freshness (§8); its CI job where W1-A0 placed it (§9) | M | `tools/xtask` | green on a release branch; fails on a planted stale evidence record | shallow history on the runner | run the freshness part locally before tagging | none |
| W8-A2 | `cargo xtask api-closure` **(new, working name)** over rustdoc JSON: denylist of upstream paths, allowlist `raw_window_handle`, `serde`, `cursor_icon`; scope `flui`, `flui-platform-api`, `flui-protocol`; first proved on a planted `pub fn f() -> accesskit::Role` | M | `tools/xtask` | the planted violation fails; green on main | rustdoc JSON may need nightly (hypothesis) | one pinned nightly shared with semver-checks and the sdk measurement, installed through `cargo xtask doctor` | ADR-0089 |
| W8-A3 | Generated docs, the published book tested (`mdbook test`; all fences are `rust,ignore` today) | M | `book`, `tools/xtask` | the book builds and its code runs | none | none | none |
| W8-B1 | Notes built from crates.io by a clean consumer outside the repository | M | none in the workspace | the build log recorded as evidence | a crate name unavailable | fallback names from W0-9 | none |

---

## 6. Milestones and horizons

### Mapping

| Milestone | Waves | Exit, as proposed below |
|---|---|---|
| B0 Reset | W1, W2 | [G] gates wired and able to fail; [S] structure green; [R] debt frozen; [P] hygiene |
| B1 App loop | W3, W4, W5 (A and B tracks) | unchanged from the roadmap, plus copy and paste in text fields with Form; `perf --check` blocking |
| B2 Platforms | W5-P1, W6 | unchanged, plus copy and paste live on three desktops |
| B3 Agents and docs | W7 | unchanged |
| B4 Beta release | W8 | unchanged; `release-check` green |

The B0 exit, as the decisions report wrote it:

- **[G]** The pinned `checks` list contains workspace-tiers, reach, globals, module-dag, markers,
  file-length and core-names-no-official, each with `--self-test`. A gate without a self-test and
  without an entry in the pinned list does not count.
- **[S]** `cargo xtask workspace` (tiers) has zero findings. `cargo xtask reach` is green with the
  K forbid set over every facade feature combination, which needs W1-B2a. `cargo xtask module-dag
  -p flui-widgets` is green. The frame-phase entry points are reachable only from `flui-runtime`
  (W2 is in B0, as the owner decided).
- **[R]** The globals, file-length, undocumented-unsafe and multiple-versions allowlists are seeded
  by a scan and only shrink. A perf baseline is recorded (not blocking); baseline recorded in
  `crates/flui-widgets/perf/baseline.toml`.
- **[P]** `cargo xtask ci` is green on `macos-latest`. `cargo build --workspace` works from the
  README without lld. The workspace version is `0.2.0` (today `Cargo.toml:106` says `0.2.0-dev`).
  The crate count is a reported fact of the tier table, not a target.

### H0 exit

A clean consumer builds Notes (signals, Router, Form) from `flui` and `flui-material` on crates.io
on three desktops and the web; an agent completes a scenario through `flui mcp`; the same finders
pass in `flui test`; the normalised projection matches on two backends; the globals allowlist is
fixed; `release-check` is green. On the web, the exit covers rendering and pointer input; text
entry and accessibility on the web are H1 items (W7-P1).

### After H0

| Horizon | What this plan hands over | First steps |
|---|---|---|
| H1 (0.3) | the capability seam (W6-B1) with three plugins; platform-api with no platform types; the IME kit (W4-B2); Subsecond | mobile hosts as runners over the same runtime (the iOS path does not host `UiRealm` today; see [design/research-findings.md](../../design/research-findings.md)); `flui-a2ui` taking catalogs as values; tokens as data; the web IME bridge and DOM/ARIA mirror; **optional dev dynamic linking, if §7's conditions hold** |
| H2 (0.4) | retained layer identity and damage (W6-A1); the CPU backend as a software fallback | the threaded raster lane per `GpuContext` (ADR-0091); per-realm owner threads spike; layer caches; 100k-row benches |
| H3 (1.0) | the three Stable crates with a measured closure (W0-1, W8-A2); `flui-sdk` Evolving | semver-checks from advisory to gating; the sdk graduation rule; revisit third-party package lag |
| H4 | `flui verify` badges from the conformance kits | community catalogs and backends |

---

## 7. Dynamic linking (conditional, H1 at the earliest)

The study in [design/dynamic-linking.md](../../design/dynamic-linking.md) and ADR-0096 decide
this; the plan only places it. The short version of the measurements on the Windows dev host
(MSVC `link.exe`, toolchain 1.98.1, the workspace dev profile):

- An edit in the app crate rebuilds in 1.0 to 1.3 s dynamic against 1.4 to 2.9 s static,
  depending on the linker and the package shape. The saving is the final link of a 19.7 MB
  executable.
- An edit in `flui-widgets` gets **slower**: about 5.6 to 7.0 s dynamic against 3.5 to 3.9 s
  static, because the 44 MB dylib is rebuilt and relinked every time.
- The dylib exports 64,336 symbols with the default facade features, 1,199 short of the 65,535
  limit of a Windows export table. With every facade feature on, both `link.exe` (LNK1189) and
  `rust-lld` refuse to link it (67,387 exports).
- The export count comes from shared generics, which rustc produces at opt-level 0 and 1. The
  workspace crates build at opt-level 1 in dev (`Cargo.toml:779-780`); dependencies already build
  at 3 (`Cargo.toml:807,813`). Building the workspace crates at opt-level 2 inside the dylib drops
  the count to 25,616. `-Zshare-generics=n` also works but needs nightly, and the toolchain is
  pinned stable.
- Dynamic linking does not replace Subsecond and does not need it; hot reload stays on Subsecond
  (ADR-0094).

**Nothing lands in H0.** The owner deferred dynamic linking on 2026-09-25; ADR-0096 stays
Proposed. The steps below are added to the H1 backlog only if ADR-0096 is accepted, and not at
all if the framework-edit penalty outweighs the app-edit gain. The build cost the owner wants
reduced (test and build time, disk use, memory) is W0-14's subject, not this section's: dynamic
linking shortens only an application's final link.

| Step | What | PR | Crates | Acceptance | Risk | Rollback |
|---|---|---|---|---|---|---|
| H1-DL0 | Measure on Linux and macOS (no 16-bit export limit there) and measure the framework-edit cost of building the workspace crates at opt-level 2; record both in ADR-0096 | spike | scratch | numbers recorded with the commands that produced them | the gain is too small to keep | ADR-0096 moves to Rejected |
| H1-DL1 | A `flui-dylib` crate in the Bevy shape: `crate-type = ["dylib"]`, depends on the core crates directly and **never on the facade** (a facade feature pulling a crate that depends on the facade would be a Cargo cycle); the facade gets a dev-only `dynamic-linking` feature on desktop targets only, plus `#[cfg(feature = "dynamic-linking")] use flui_dylib as _;`; `publish = false`; tier H with kind `internal` as ADR-0096 proposes (ADR-0081 has no dev-only kind, and `tool` means "never a dependency") | M | new `flui-dylib`, facade | with a profile that builds the dylib's crates at opt-level 2 or higher, `cargo run --example counter --features dynamic-linking` renders on Windows, Linux and macOS; `cargo xtask reach` proves no release graph reaches `flui-dylib` | duplicated statics if a package links a second copy of a core crate | delete the crate and the feature |
| H1-DL2 | `cargo xtask dylib-exports` **(new)**: builds `flui-dylib` on Windows and fails once the export count passes a budget (55,000 proposed); a step in the Windows lane the CI redesign defines (§9) | S | `tools/xtask`, workflow | fails on a planted budget of 1; green on main | CI time on Windows | run it in the nightly heavy lane only |

Separately from dynamic linking, W0-11 reproduces the two hazards the study found in today's
dlopen worker (the old image unloads before the realm reassembles; the worker `cdylib` carries
its own copy of `REQUEST_REBUILD`, `crates/flui-hot-reload/src/dispatch.rs:24`). They belong to
the hot-reload track whichever way ADR-0096 goes.

---

## 8. Evidence records

Platform status changes only with a dated record, as the owner's plan already requires. This plan
makes the record machine-checked:

- **Format:** `docs/evidence/<platform>.toml` with commit, OS, IME version, command, result and an
  artefact path. `docs/BETA.md` renders from it. A UI Automation client does not count as half of
  "Narrator".
- **Freshness:** a record is fresh when its commit is an ancestor of the release commit and no
  trigger path changed between them. Trigger paths for Windows: `crates/flui-platform/src/platforms/windows/**`,
  the text-input and text-store traits, the semantics-to-UIA bridge, and the text layout paths
  that affect caret ranges. The evidence files themselves are not trigger paths.
- **Check:** `cargo xtask release-check` (W8-A1; the freshness part lands with W3-P2) runs it
  locally before a tag, where the full history exists. A recorded commit missing from history is
  an error, not a pass.
- **No calendar ceiling** and no speech capture until drift is shown.

Research findings that change a step's scope, carried from
[design/research-findings.md](../../design/research-findings.md), with the step that owns them:

| Finding | Evidence | Owner step |
|---|---|---|
| GlobalKey lookup can deadlock during a frame: the binding holds `inner.write()` across `build_scope` | `crates/flui-view/src/binding.rs:1241,1297` | W2-A1a (a `RegistryBusy` result, with tests) |
| `ElementCore.depth` stores the sibling slot | `crates/flui-view/src/element/generic.rs:151` | W2-A1a |
| EditableText branches on the compile-time platform | `crates/flui-widgets/src/text/editable_text.rs:1554` | W4-B2 |
| Copy and paste are missing, and the clipboard accessor is dead code | `crates/flui-app/src/app/runtime.rs:1632-1638` | W4-A3 (B1, with Form, as the owner decided) |
| Image GPU cache keyed by `Arc` pointer (possible ABA) | `crates/flui-engine/src/texture_cache.rs:65-73` | W6-A1 |
| `flui-hot-reload` depends on `windows` directly, which the package forbid set would reject | `crates/flui-hot-reload/Cargo.toml:47` | W5-B1 (the Subsecond rewrite removes it); a dated reach exception until then |
| The web backend has no IME and no a11y | canvas only | W7-P1 (exit wording), H1 (the bridges) |
| A path clip installs its bounding box; `ClipOp::Difference` is refused | `crates/flui-engine/ARCHITECTURE.md:289,317` | W6-A2 (conformance scene), fixed with the raster contract |
| Android uses NativeActivity; GameActivity may be needed for soft-keyboard IME (hypothesis) | `crates/flui-platform/Cargo.toml:162` | H1 |

The paint-poison error box, device loss on a
shared `GpuContext`, and the AI-surface items dropped from the synthesis are listed in
[design/research-findings.md](../../design/research-findings.md) and are not scheduled here.

---

## 9. Workflow needs: inputs to the CI redesign

[AGENTS.md](../../AGENTS.md) keeps `.github/workflows/` out of task PRs unless the task is about
them. These are the workflow needs this plan found. The owner decided on 2026-09-25 that they do
not land piecemeal: they are inputs to the CI redesign (W0-15 designs it, W1-A0 implements it),
which decides where and when each runs under three rules: a fast PR lane, full runs only when
needed, and no platform-specific heavy runs during active work. The "For step" column names the
step that needs the result, not a step that edits the workflow.

| # | File and line | Change | For step | Notes |
|---|---|---|---|---|
| 1 | `.github/workflows/ci.yml:924` | `cargo build -p flui --features material --example sliver_demo` stops working when the facade loses `material`; build the example from `packages/flui-material` or make it catalog-neutral | W3-A5 | in place before W3-A5 lands |
| 2 | `.github/workflows/ci.yml:1012-1019` | the comment and the `--no-default-features` flag on the Windows readback step become redundant with `default = []` | W3-A5 | follows W3-A5 |
| 3 | new job, or an extension of `cli-macos` (`ci.yml:1491`) | `cargo xtask ci` on `macos-latest` (B0 exit [P]); a new job goes into the `ci` aggregator's `needs` (`ci.yml:1650`) and into `HEAVY_JOBS` (`ci.yml:1692`) | W1-A12, B0 exit | job name equals its key |
| 4 | a step in `checks` or a heavy job | `cargo xtask perf` non-blocking, then blocking at the B1 exit | W1-A8 | the change from non-blocking to blocking is a second edit |
| 5 | `gpu-test` (`ci.yml:949`) or a new heavy job | `cargo xtask device windows-a11y` with the pre-check, after the one-off run (W0-13) passes | W3-P1 | not `platform-windows`: a release build of `a11y_probe` with the facade changes that job's profile |
| 6 | a step in an existing job | `cargo xtask package-check` if it is too slow for `checks` | W3-A3 | only if it leaves `checks` |
| 7 | the advisory protocol job on `windows-latest` | the normalised-outline comparison between the UIA and in-process backends | W2-B1 | advisory until B3 |
| 8 | toolchain install step | one pinned nightly for rustdoc JSON (api-closure, sdk surface, semver-checks) | W3-A1, W8-A2 | only if the nightly requirement is confirmed |
| 9 | new job | `cargo xtask release-check` | W8-A1 | heavy; in `needs` and `HEAVY_JOBS` |
| 10 | `.github/workflows/release.yml` checkout | `fetch-depth: 0`, only if the owner prefers the freshness check on the runner over the local check | W8-A1 | default: keep it local |
| 11 | a Windows job | `cargo xtask dylib-exports` | H1-DL2 | only if ADR-0096 is accepted |
| 12 | `.github/workflows/ci.yml:638-643` | re-add `windows-latest` to the test matrix, which was dropped "temporarily" | not required by any step | the owner made it an input to the CI redesign; a heavy lane, not during active work |
| 13 | where the build-footprint levers apply | the levers W0-14 keeps (for example nextest archives, sccache, split debug info, one shared target) | W1-A0 | only with a measured gain |

Every new job: actions pinned to a full SHA, `--locked` on every cargo call, caches saved only on
`main`, name equal to key.

---

## 10. Proposed edits to the owner's roadmap and plan

The owner keeps a roadmap and plan document outside the repository. Since 2026-09-25 it is a
mirror and journal of this plan and `design/`, which are the source of truth; `docs/ROADMAP.md`
names them. These are edits for the owner to apply to that mirror so it stops contradicting the
repository; the line numbers refer to the copies read on 2026-09-25. The
proposed wording is given in English and is meant to be applied in the owner document's language.

**Roadmap, milestone table, B0 exit (line 617).** Today: "just ci green on a clean Mac without
workarounds; cargo build --workspace from the README without lld; 26 crates; no file > 3000
lines in flui-app and flui-widgets". Proposed:

> **[G]** the pinned `checks` list contains workspace-tiers, reach, globals, module-dag, markers,
> file-length and core-names-no-official, each with `--self-test`. **[S]** `cargo xtask workspace`
> (tiers) has 0 findings; `cargo xtask reach` is green with the K set {flui-platform, winit,
> android-activity, ndk, windows, objc2-app-kit, objc2-ui-kit, wgpu, flui-engine, flui-app} over
> every facade feature combination (needs the platform-api split); `cargo xtask module-dag -p
> flui-widgets` is green; [if the one-transaction wave stays in B0] frame-phase entry points are
> reachable only from flui-runtime. **[R]** the globals, file-length, undocumented-unsafe and
> multiple-versions allowlists are seeded by a scan and only shrink; a perf baseline is recorded
> (not blocking). **[P]** `cargo xtask ci` green on macos-latest; `cargo build --workspace` from
> the README without lld; version 0.2.0.

**Roadmap, track A (line 304).** Today: "26 crates instead of 28". Proposed:

> crates match the tier table (the number is not a target); derive macros are either real or removed

**Roadmap, track H (line 718).** Today: "just ci green". Proposed:

> `cargo xtask ci` green; no broken link (`cargo xtask docs-links`)

**Roadmap, track A (line 312).** Today: "1143 unwrap() outside tests → allowlist". Production
code has no bare `unwrap()` left (clippy `unwrap_used` enforces it). The `file-length` scan
(syn, test-only items excluded) finds 14 files above 2000 production lines and one above 3000,
`crates/flui-scheduler/src/scheduler.rs` at 3569; the nearest below the limit are
`flui-view`'s `element_tree.rs` (2894) and `build_owner.rs` (2887). Proposed:

> A7 Ratchet unsafe: `undocumented_unsafe` allowlist seeded by a scan; unsafe in flui-platform under a per-backend SAFETY audit

**Roadmap, verdict (line 20).** Replace the stale counts ("28 crates", "46 files >2000 lines",
"1143 unwrap()") with the numbers from the tier table and the `file-length` scan once W1-A4 lands.

**Roadmap, lines 224-225 and 743-745.** They cite `docs/runtime-contract.toml` and its ratchet,
which `cf46dfe20` (#1283) deleted. Proposed for both: "the `cargo xtask globals` allowlist
(ADR-0097)" in place of "runtime-contract ratchet"; "FONT_SYSTEM leaves the globals allowlist
with the Parley migration (ADR-0092)" in place of "update the ambient-reach ratchet in
runtime-contract.toml".

**Roadmap, line 766.** Today it plans a new `flui-state` crate. Proposed:

> the reactive graph stays in flui-view; the signal read contract lives in flui-foundation, and there is no separate signals crate (ADR-0085)

**Roadmap, line 814.** Replace "docs/workspace-layers.toml, docs/runtime-contract.toml, 68 ADR"
with "`[package.metadata.flui]` in each manifest (checked by `cargo xtask workspace`), the gates in
`cargo xtask checks`, and the ADRs in docs/adr/".

**Roadmap, line 183.** It lists a nested-scroll sliver protocol; the workspace has no
`NestedScroll`. Proposed: mark it "planned" rather than present.

**Roadmap, lines 351 and 489.** Append:

> recorded in `docs/evidence/<platform>.toml`; freshness by trigger paths, checked by `cargo xtask release-check`

**Plan, delivery layers (line 31).** Today: official packages "in separate repositories, one
release train". Proposed:

> Official packages (`flui-*` in `packages/` of this repository, one workspace and one release train; a separate repository only on a trigger recorded in ADR-0088, with lockstep versions): …

**Plan, governance (line 57).** Proposed:

> Evolving (`flui-sdk` for package authors, Material/Cupertino details, the devtools protocol, plugins)

**Plan, metrics (line 60).** Append after "platforms with fresh evidence (1 → 4 → 6)":

> ; fresh means the recorded commit is an ancestor of the release and the platform's trigger paths have not changed

**Plan, review tab (line 63).** Keep Windows, Narrator and Japanese IME in the B1 exit. Append:

> ; the H0 gate for IME is the text-store conformance kit, not a live session

**Plan, architectural trajectory, "Runtime" row.** Add to the "by 1.0" column: "one frame
transaction in flui-runtime, driven by the app and by the tests (ADR-0083)".

---

## 11. Unverified inputs

These are used above and must be checked before anyone relies on them; each has a W0 step or a
named owner step.

- crates.io availability of every new crate name (W0-9).
- Subsecond on Windows and Android, and patch reach through vtables created before a patch (W0-5).
- Whether rustdoc JSON, cargo-public-api and cargo-semver-checks need nightly (W0-1, W8-A2).
- `windows-a11y` and `windows-input` on a hosted `windows-latest` runner (W0-13); the claim that
  the `KEYEVENTF_UNICODE` path in `tools/desktop-mcp` bypasses the IME.
- The GlobalKey deadlock: code shape confirmed, never executed.
- The resolver error text for conflicting `=` pins through a real registry (only a
  directory-source probe ran; W3-A1's registry test settles it).
- The two dlopen worker hazards (W0-11).
- Dynamic linking on Linux and macOS, and the framework-edit cost at opt-level 2 (H1-DL0).
