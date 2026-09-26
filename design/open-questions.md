# Owner questions and their resolutions

- **Status:** Resolved by the owner on 2026-09-25, except the items under
  [Still open](#still-open)
- **Date:** 2026-09-25
- **Snapshot:** `main` at `cab06137d`

This page listed the questions the design could not settle without the owner, each with a
recommended default. The owner answered all of them on 2026-09-25. Each item below keeps the
question, a short context, and the decision, and says where the decision differs from the default.
The decisions are indexed in [decisions.md](decisions.md#owner-decisions-of-2026-09-25); the order
of work is in the [migration plan](../docs/plans/2026-09-25-architecture-migration-plan.md). What
genuinely remains open is in [Still open](#still-open), with the two follow-up tasks the answers
created.

Sources: the owner-decisions report's list of what still needed the owner
([report-decisions.ru.md](../docs/research/2026-09-25-architecture-review/report-decisions.ru.md),
last section), the leftovers of the architecture report's §14
([report-architecture.ru.md](../docs/research/2026-09-25-architecture-review/report-architecture.ru.md)),
an audit of the existing ADRs against the seventeen planned ones, and a pass over the raw research
([research-findings.md](research-findings.md)). Every `path:line` was re-read at `cab06137d`.

## Summary

| # | Question | Decision | Against the default | Recorded in |
|---|---|---|---|---|
| 1 | Runtime extraction in B0 or B1 | B0 | same | ADR-0083, plan |
| 2 | P10 escape modules up front or on demand | On demand, except `flui_sdk::gpu` | same | ADR-0089 |
| 3 | Third-party package lag under a lockstep SDK | Bump `0.N` on every train; accept the lag, revisit at H3 | same | ADR-0088 |
| 4 | Shape of signal writes | Typed `EventCx` through `WriterSource`; pilot on `flui-cupertino`, `counter` and `todo`; rollback trigger kept | pilot widened | ADR-0086 |
| 5 | Workflow changes | Not piecemeal: a CI redesign task | **changed** | [Still open](#ci-redesign) |
| 6 | Enable ja-JP on the development host | The owner enables it before IME work | same | plan |
| 7 | crates.io names | Check before creating each crate; no reservation now | same | plan |
| 8 | Deadline of the `!Send` flip | Before the first crates.io publication, with the callback signature change | **changed** | ADR-0091 §1, ADR-0086 |
| 9 | `LayoutCallbackScope` against ADR-0017 | Three-day spike first; ADR-0017 stays until then | same | plan |
| 10 | Deleting `flui-tree` and `flui-localizations` | Delete both | same | ADR-0081 |
| 11 | Clipboard: required backend method or registry capability | A capability model: core-required backend methods and optional plugins behind one door | **generalised** | ADR-0084 §5 |
| 12 | New gates against the `cf46dfe20` deletions | Amend ADR-0078 explicitly in the first gate's change | same | plan |
| 13 | Back-links on the existing ADRs | Added only in the PR that accepts each new ADR; no symmetry gate | **changed** | plan |
| 14 | Is the signals precondition already met | Yes; remove the `signals` feature | same | ADR-0085 §5 |
| 15 | Pageless routes | Every push is URL-addressable; dialogs and overlays excluded | same | ADR-0093 §2 |
| 16 | Correctness findings the reports dropped | Copy and paste in B1 with Form; GlobalKey and `depth` with the runtime extraction (B0); path clips with the raster contract | **changed** (copy and paste earlier) | plan |
| 17 | Web IME and accessibility for the H0 exit | Web in H0 is rendering and pointer input only; text entry and the DOM/ARIA mirror are H1 items | same | plan, architecture |
| 18 | Where the living plan lives | The repository; the owner's document is a mirror | same | `docs/ROADMAP.md`, README |
| 19 | Windows test matrix in CI | An input to the CI redesign | **changed** | [Still open](#ci-redesign) |
| 20 | Dynamic linking | Not now; a build-footprint study instead | same, plus a study | ADR-0096 |
| 21 | dlopen hot-reload worker hazards | Run the Windows repro once; document both in the crate docs | same | plan |
| 22 | Unverified claims that ADRs must not state as fact | Unchanged: each checked before its ADR is accepted | same | [Still open](#22-unverified-claims-that-adrs-must-not-state-as-fact) |

The owner also set the product strategy in the same interview; it is recorded in
[decisions.md](decisions.md#strategy) and [architecture.md](architecture.md): FLUI is positioned
as a UI runtime trusted by people and agents, Notes is the beta hero application, and live
evidence comes platform by platform in the order Windows, macOS, Linux, web.

---

## From the owner-decisions panel

### 1. Runtime extraction in B0 or B1

The one-frame-transaction item of exit B0 holds only if extracting `flui-runtime` stays in B0.

- **Status:** Resolved by the owner on 2026-09-25.
- **Decision:** B0. The GlobalKey-under-lock and `ElementCore.depth` fixes (item 16) ship with it.

### 2. P10 escape modules up front or on demand

Two judges wanted versioned upstream escape modules in the SDK from the start; the owner judge
wanted them only with a consumer.

- **Status:** Resolved by the owner on 2026-09-25.
- **Decision:** on demand, except `flui_sdk::gpu`, which already has a consumer (external GPU
  content). [ADR-0089](../docs/adr/ADR-0089-upstream-types-in-stable-signatures.md) stands as
  written.

### 3. Third-party package lag under a lockstep SDK

With `flui-sdk` bumped on every train and exact internal pins, a minor `flui` upgrade waits for
every third-party package the application uses. The panel's verifier had suggested bumping only on
trains that change Evolving items.

- **Status:** Resolved by the owner on 2026-09-25.
- **Decision:** bump `0.N` on every train, as
  [ADR-0088](../docs/adr/ADR-0088-official-packages-sdk-and-facade.md) is written. The lag is
  accepted while no third-party packages exist and revisited at H3.

### 4. Shape of signal writes

The panel was unanimous for typed writes, but its verifier refuted two of the four arguments. The
question was one of taste: `move ||` ergonomics against compile-time errors.

- **Status:** Resolved by the owner on 2026-09-25.
- **Decision:** typed `&mut EventCx<'_>` on framework event callbacks, opened by a `WriterSource`,
  with a pilot on `flui-cupertino` and the `counter` and `todo` examples and the recorded rollback
  trigger to guard-only.
  [ADR-0086](../docs/adr/ADR-0086-signal-writes-through-event-context.md) records the choice and
  the lineage from the removed `flui-reactivity` crate.

### 5. Workflow changes

The decisions need several workflow edits: the `--features material` build at
`.github/workflows/ci.yml:924`, `cargo xtask ci` on `macos-latest`, a non-blocking perf job, a
Windows UI Automation step, and possibly `fetch-depth: 0` in `release.yml`. The default was two
PRs labelled `full-ci`.

- **Status:** Resolved by the owner on 2026-09-25.
- **Decision:** no piecemeal workflow edits. CI is rebuilt as one designed change: a fast PR lane,
  full runs only when needed, and no platform-specific heavy runs during active work. Each of the
  needs above becomes an input to that design; see [CI redesign](#ci-redesign).

### 6. Enable ja-JP on the development host

The host has en-US and ru only, and agents do not change system settings.

- **Status:** Resolved by the owner on 2026-09-25.
- **Decision:** the owner enables ja-JP before the IME work starts
  (`cargo xtask device windows-ime` and the live evidence for exit B1).

### 7. crates.io names

No name was checked: `flui-sdk`, `flui-reactive`, `flui-protocol`, `flui-platform-api`,
`flui-runtime`, `flui-engine-cpu`.

- **Status:** Resolved by the owner on 2026-09-25.
- **Decision:** check each name before the crate is created; nothing is reserved now.

---

## Raised by the ADR audit

### 8. Deadline of the `!Send` flip

[ADR-0027](../docs/adr/ADR-0027-owner-affine-ui-realms.md) §2 and §9 say UI callbacks and render
objects are not `Send + Sync`; the code says otherwise (`ListenerCallback`,
`crates/flui-foundation/src/notifier.rs:46`; `Listenable: Send + Sync`, `notifier.rs:78`;
`RenderView::RenderObject`, `crates/flui-view/src/view/render.rs:451`). The default kept
ADR-0091's deadline, "no later than the H3 freeze".

- **Status:** Resolved by the owner on 2026-09-25.
- **Decision:** the flip lands **before the first crates.io publication**, together with the
  callback signature change of ADR-0086, so public bounds and signatures break before anyone
  depends on them.
  [ADR-0091](../docs/adr/ADR-0091-one-owner-thread-isolated-realms-raster-thread.md) §1 carries the
  deadline.

### 9. `LayoutCallbackScope` against ADR-0017

The architecture report proposes Flutter's `invokeLayoutCallback` contract so that a lazy band
converges in one pass; [ADR-0017](../docs/adr/ADR-0017-build-during-layout-callback-seam.md) §3
says build never runs during layout.

- **Status:** Resolved by the owner on 2026-09-25.
- **Decision:** a three-day spike first, then a decision. ADR-0017 stays in force until then; if
  the spike converges in one pass without a double `RefCell` borrow, a separate ADR supersedes
  ADR-0017 §3.

### 10. Deleting `flui-tree` and `flui-localizations`

The tree traits have no generic consumer, and `flui-localizations` is a small crate in a layer of
its own.

- **Status:** Resolved by the owner on 2026-09-25.
- **Decision:** delete both.
  [ADR-0081](../docs/adr/ADR-0081-workspace-tiers-and-reach-facts.md) records the deletions in its
  tier table.

### 11. Clipboard: required backend method or registry capability

[ADR-0038](../docs/adr/ADR-0038-data-transfer-architecture.md) made `Platform::clipboard()` a
required method so no backend can forget it; the review named clipboard as a plugin capability,
which would reintroduce that risk.

- **Status:** Resolved by the owner on 2026-09-25.
- **Decision:** generalised into a capability model with two classes behind one door,
  `cx.capability::<C>()`. **Core-required** capabilities are methods of the backend traits, and a
  backend without them does not compile: clipboard and data transfer, text input and IME,
  accessibility, cursor, and window chrome basics. **Optional** capabilities are plugins in the
  registry with a typed `Unsupported`: haptics, camera, geolocation, notifications, and file
  dialogs unless they become core.
  [ADR-0084](../docs/adr/ADR-0084-open-capability-seam-and-plugins.md) §5 records the rule for
  which class a capability belongs to and how one moves between classes.

### 12. New gates against the `cf46dfe20` deletions

`cf46dfe20` (#1283) deleted the runtime-contract file, the panic allowlist and the publish dry-run,
stating the rules are types and lints now (ADR-0078). The new gates bring scanners back.

- **Status:** Resolved by the owner on 2026-09-25.
- **Decision:** the first gate's change amends
  [ADR-0078](../docs/adr/ADR-0078-rules-live-in-types-and-lints.md) explicitly, names what #1283
  removed, and says why no type or lint covers each new gate.

### 13. Back-links on the existing ADRs

The new ADRs supersede, amend or complement 25 existing ones, and nothing enforces symmetric
back-links.

- **Status:** Resolved by the owner on 2026-09-25.
- **Decision:** back-links are added only in the PR that accepts each new ADR. There is **no**
  symmetry gate. The lines that are already missing (ADR-0065 lacks `Amended by: ADR-0067`;
  ADR-0016 lacks `Amended by: ADR-0065, ADR-0067`) are fixed at acceptance time too; no older ADR
  is edited now.

---

## Raised by the research

### 14. Is the signals precondition already met

The manifest comments keep the `signals` feature opt-in until "the #1090 field-mask registry" and
"the go/no-go measurement" land. Both have: #1090 as `588251a1c`, the measurement in ADR-0074
§8.1.

- **Status:** Resolved by the owner on 2026-09-25.
- **Decision:** yes. The `signals` feature is removed;
  [ADR-0085](../docs/adr/ADR-0085-reactive-core-placement-and-phase-subscribers.md) §5 states the
  preconditions as met, with the evidence.

### 15. Pageless routes

The research synthesis had "every push produces a URL-addressable entry", Flutter's Navigator 1/2
lesson; the final decision dropped it without a reason.

- **Status:** Resolved by the owner on 2026-09-25.
- **Decision:** adopted. Every push is URL-addressable; dialogs and overlays are excluded.
  [ADR-0093](../docs/adr/ADR-0093-router-is-the-primary-navigation-api.md) §2 records it.

### 16. Correctness findings the reports dropped

Copy and paste are not wired in `EditableText`
(`crates/flui-widgets/src/text/editable_text.rs:322`); a path clip installs its bounding box
(`crates/flui-engine/ARCHITECTURE.md:289`); a GlobalKey lookup can take `inner.read()` while
`draw_frame` holds `inner.write()` (`crates/flui-view/src/binding.rs:1241,1297`); and
`ElementCore.depth` stores the sibling slot (`crates/flui-view/src/element/generic.rs:151`). The
default put copy and paste in B2.

- **Status:** Resolved by the owner on 2026-09-25.
- **Decision:** a failing test and a fix for each. Copy and paste in text fields lands in **B1,
  together with Form**, through the core-required clipboard of item 11. The GlobalKey lookup and
  `depth` are fixed with the runtime extraction in B0. Path clips enter the raster conformance
  scenes and are fixed with the raster contract.

### 17. Web IME and accessibility for the H0 exit

The web backend implements neither text input nor accessibility, and no item covered a
hidden-input IME bridge or a DOM/ARIA mirror.

- **Status:** Resolved by the owner on 2026-09-25.
- **Decision:** web in the H0 exit covers rendering and pointer input only, and the exit says so.
  Web text entry and IME, and the DOM/ARIA mirror, are H1 items of their own.

### 18. Where the living plan lives

`docs/ROADMAP.md` pointed to a private artifact that an outside contributor cannot read.

- **Status:** Resolved by the owner on 2026-09-25.
- **Decision:** the repository is the source of truth: [this folder](README.md) and
  `docs/plans/`. The owner's own document is a mirror and journal that points to the repository.
  `docs/ROADMAP.md` says so.

### 19. Windows test matrix in CI

The test matrix runs on `ubuntu-latest` only; the Windows entry was dropped "temporarily"
(`.github/workflows/ci.yml:638-643`).

- **Status:** Resolved by the owner on 2026-09-25.
- **Decision:** not a separate workflow edit; an input to the [CI redesign](#ci-redesign), under
  its rule that platform-specific heavy runs do not run during active work.

### 20. Dynamic linking

The study is in [dynamic-linking.md](dynamic-linking.md): an app-crate edit rebuilds about
0.3–1.5 s faster, a framework-crate edit gets slower, and the Windows dylib sits about 1,200
exports below the PE limit with the default features.

- **Status:** Resolved by the owner on 2026-09-25.
- **Decision:** not now.
  [ADR-0096](../docs/adr/ADR-0096-dev-build-dynamic-linking.md) stays Proposed as deferred. The
  owner's actual problem is test and build time, disk use and memory growth, which dynamic linking
  does not address; that goes to the [build-footprint study](#build-footprint-study).

### 21. dlopen hot-reload worker hazards

Two unrun hypotheses: the old image is unloaded before the realm drops its views
(`crates/flui-hot-reload/src/worker.rs:458`), and each worker reads its own `REQUEST_REBUILD`
(`crates/flui-hot-reload/src/dispatch.rs:24`).

- **Status:** Resolved by the owner on 2026-09-25.
- **Decision:** run `hot_reload_counter` on Windows once with a logic edit and record the result;
  document both hazards in `flui-hot-reload`'s crate docs until Subsecond replaces the path
  (ADR-0094). No shared-dylib worker.

---

## Still open

### 22. Unverified claims that ADRs must not state as fact

- **Status:** Unchanged by the owner on 2026-09-25: each claim is checked before the named ADR is
  accepted.

| Claim | Check | ADR |
|---|---|---|
| Subsecond works on Windows and Android, and patches reach code through `Box<dyn ElementBase>` vtables created before the patch | one-week spike | ADR-0094 |
| rustdoc-JSON, cargo-public-api and cargo-semver-checks need nightly | run each on the pinned toolchain | ADR-0089, ADR-0081 |
| The size N of the Stable closure | cargo-public-api spike before it is promised | ADR-0081 |
| Mismatched `=` pins through a real registry give a resolver error, not E0308 | registry test (only a directory-source probe ran) | ADR-0088 |
| "A plugin depends on about 30 crates", "a Win32 edit rebuilds 3 crates" | `cargo tree` and `cargo build --timings` | ADR-0082 |
| Stale pixels with a swapchain scissor; blit cost of a retained target on tile GPUs | readback on dx12 and vulkan; mobile measurement | ADR-0087 |
| Parley glyphs rasterize into the ADR-0067 atlas with a stable key | ADR-0077's precondition 1 prototype | ADR-0092 |
| vello_cpu is bit-deterministic across CPUs with pinned SIMD | conformance scenes on three OSes | ADR-0087 |
| Static ID counters reach semantics snapshots or the protocol | scan and snapshot test | ADR-0095, ADR-0097 |
| `KEYEVENTF_UNICODE` bypasses the IME; `windows-a11y` passes on hosted `windows-latest` | a hosted trial run, scheduled by the CI redesign | ADR-0090 |
| GameActivity is needed for the Android soft-keyboard IME (today `native-activity`, `crates/flui-platform/Cargo.toml:162`) | Android spike | ADR-0090 |
| The image GPU cache keyed by `Arc` pointer (`crates/flui-engine/src/texture_cache.rs:65-73`) can alias a freed and reallocated image | a test that drops and reallocates | ADR-0087 |
| Per-crate `testing` features build the upper stack more than once | count distinct `libflui_rendering-*` hashes | the build-footprint study (checked: one hash after a workspace build, more for every other package set; [build-footprint.md](build-footprint.md#duplicate-builds)) |

### ADR conflicts for the named ADR's author

The ADR audit found these; each is settled when the named ADR is accepted.

- ADR-0042 says there is no universal `ThemeData`; the review adds `flui_sdk::tokens` and
  `ThemeData::from_tokens`. ADR-0088 should state that tokens are data, not a theme type.
- ADR-0066 forbids serde on the display list; record/replay and devtools frame inspection want it.
  ADR-0095 should say whether frames cross the protocol.
- ADR-0080 advertises `expand`, `collapse` and `set_value`, while
  `crates/flui-semantics/src/accesskit_translation.rs` maps `SetValue` to `SetText` (`:311`) and
  has no expand/collapse action. ADR-0089's `ALL` test should cover ADR-0080's vocabulary.
- `crates/flui-engine/ARCHITECTURE.md:8-13` rules out a second rasteriser; ADR-0087 needs a
  `## Mapping decisions` note there.
- The AGENTS.md "ID offset" row says `LayerId` and `SemanticsId` are 1-based `NonZeroUsize`; the
  review reports reusable slab indices. A doc fix, not an ADR.

### CI redesign

A follow-up task created by items 5 and 19, done together with the build-footprint study and
related to the open issue #1279.

- **Status.** Designed in [ci.md](ci.md); still open until the owner signs off its open points
  (C1–C8, [ci.md §9](ci.md#9-open-points-for-the-owner)), and not implemented.

- **What it covers.** One design for the workflows instead of edits step by step: a fast PR lane
  that covers the changed crates and their dependents; full runs only when a change needs them
  (the `full-ci` label, `main`, nightly, release); and no platform-specific heavy runs while work
  is in progress on a branch. The result is a design the owner signs off, then workflow changes
  that implement it, each on the merge path through the `ci` aggregator.
- **Inputs.** Every workflow need the design and the migration plan found: `cargo xtask ci` on
  `macos-latest` (B0 hygiene); a non-blocking perf job that becomes blocking at the B1 exit; the
  Windows UI Automation step (`cargo xtask device windows-a11y`) and its hosted trial run; the
  `windows-latest` test matrix entry (item 19); the build of the facade without Material in place
  of `--features material` (`.github/workflows/ci.yml:924`); where `cargo xtask package-check`
  and `cargo xtask release-check` run; the advisory protocol comparison on `windows-latest`; a
  pinned nightly for rustdoc JSON if one is needed; `fetch-depth: 0` in `release.yml` only if the
  evidence freshness check runs there.
- **Why one task.** Each input alone is small, but each changes what every other PR must pass.
  Editing them one at a time would keep adding heavy jobs to a lane the owner wants lighter.

### Build-footprint study

A follow-up task created by item 20, done together with the CI redesign.

- **Status.** Measured in [build-footprint.md](build-footprint.md), with recommendations R1–R7.
  Some levers and measures below were not run (sccache, cargo-sweep, a shared target, peak
  memory per compiling job); the owner accepts or asks for them as C8 in
  [ci.md §9](ci.md#9-open-points-for-the-owner).

- **What it measures.** The size of `target/` by artifact kind (rlibs and rmeta, test binaries,
  incremental caches, debug info); the number of test binaries; duplicate builds of the upper stack
  caused by per-crate `testing` features (distinct `libflui_rendering-*` hashes); peak memory per
  compiling job; and the cost of a target directory per worktree against a shared one.
- **Levers to evaluate.** Consolidating test targets into fewer binaries; sccache; split debug
  info; nextest archives (build once, run in several jobs); cargo-sweep for stale artifacts; one
  shared target directory. Each lever is kept only with a before-and-after number.
- **Why dynamic linking is not the lever.** Dynamic linking shortens only the final link of an
  application crate. It does not reduce the number of compiled units, the rlibs and metadata
  under `target/`, the number or size of test binaries, or rustc's peak memory. It adds a 44 MB
  dylib to the output, makes a framework-crate edit slower because the dylib relinks every time,
  cannot serve test binaries and doctests without a `PATH` step for the std DLL, and gives nothing
  in CI, which builds without incremental compilation. The measurements are in
  [dynamic-linking.md](dynamic-linking.md).

### Owner actions

- Enable ja-JP on the Windows development host before the IME work (item 6).
