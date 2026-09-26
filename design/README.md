# FLUI design

- **Status:** Proposed. The ADRs this folder relies on (ADR-0081 to ADR-0097) are `Proposed`,
  except four accepted in part on 2026-09-26. ADR-0081: its tiers, `order`, direction rule,
  `edge-exceptions` and `tier-kind` declarations are implemented and checked by
  `cargo xtask workspace`. Its reach facts (§2) are implemented and checked by
  `cargo xtask reach` but stay Proposed until the owner decides the three points ADR-0081's
  status names. ADR-0082: `flui-platform-api` exists and holds the capability traits and the
  window and input vocabulary, and only `flui-app` depends on `flui-platform`. ADR-0095: the
  `flui-protocol` crate exists and holds `SemanticsRole`, `SemanticsAction` and the ADR-0080
  wire vocabulary. ADR-0097: `cargo xtask globals` gates process-global state against the
  seeded `globals` entries in each manifest. The module-DAG gate for flui-widgets is
  implemented as `cargo xtask module-dag`. The phase counters and `cargo xtask perf` with its
  baseline are implemented, non-blocking ([architecture.md](architecture.md), budgets).
  Nothing else described here is implemented. The
  owner answered the open questions on 2026-09-25; the ADRs and this folder carry those answers.
- **Date:** 2026-09-25
- **Baseline:** `main` at `cab06137d`

This folder holds the target architecture of FLUI and the reasoning around it: where the
workspace, runtime and ecosystem should end up, which decision carries each part, what the owner
decided, and what the research found that nothing acts on yet. Together with the migration plan
it is the living plan and its source of truth. It is the forward-looking
counterpart of [`docs/architecture.md`](../docs/architecture.md), which describes the code as it
is and stays authoritative for the code until an ADR here is accepted and implemented.

The folder does not order the work (the
[migration plan](../docs/plans/2026-09-25-architecture-migration-plan.md) does), and it does not
replace the ADRs: each document names the ADR that records a decision, and the ADR is the place to
argue with it.

## Reading order

1. [architecture.md](architecture.md) — the target, section by section, each with its ADR.
2. [decisions.md](decisions.md) — the index from each decision (D1–D17, G, L, the eight panel
   decisions O1–O8 and the owner's decisions of 2026-09-25) to its ADR, with the alternatives
   rejected and what verification or the owner changed.
3. [open-questions.md](open-questions.md) — the questions the owner resolved, and what is still
   open (unverified claims, the sign-off of the CI design, the gaps of the build-footprint study).
4. The ADRs below, for the decision you care about.
5. The [migration plan](../docs/plans/2026-09-25-architecture-migration-plan.md), for the order of
   the work and how each step proves it is done.
6. [dynamic-linking.md](dynamic-linking.md) and [research-findings.md](research-findings.md), as
   needed.
7. [build-footprint.md](build-footprint.md) and [ci.md](ci.md), for the build cost and the CI
   lanes.

## The target in one paragraph

FLUI is positioned as a UI runtime trusted by people and agents; the Flutter model is the familiar
shape, not the headline promise. Numbered layers become tiers (values, contracts, substrate, render machine, spine and runtime,
hosts, official packages) with a direction rule and **reach facts**: a gate over the resolved
dependency graph proves that nothing below the hosts reaches an OS crate, winit or wgpu. The
platform contracts move into a small Stable `flui-platform-api` while the OS backends stay in
`flui-platform`. One frame transaction lives in a new `flui-runtime` above `flui-widgets`, and
the app runners and the test driver both run it, so tests exercise the product frame. Only three
crates carry a semver promise (`flui`, `flui-platform-api`, `flui-protocol`), measured as the
closure of what their signatures name, with no upstream type in it except raw-window-handle.
Official packages (Material, Cupertino, devtools, hot reload) live in `packages/` of this
workspace, build on an Evolving `flui-sdk`, and the facade names none of them. Platform
capabilities come in two classes behind one door, core-required backend methods and optional
plugins; signals stay realm-owned, are read
through `ReadScope` and written through `EventCx`; rendering gets one raster contract with wgpu and
CPU backends and damage from retained layer identity; text moves to Parley per realm; navigation
becomes Router-first; hot reload moves to Subsecond; process-global state is gated down to one
trampoline cell. Development-build dynamic linking was measured and deferred; a build-footprint
study and a CI redesign take up the build cost instead.

## Documents

| Document | What it holds |
|---|---|
| [architecture.md](architecture.md) | The target architecture: tiers and crates, feature policy, facade and SDK, runtime, state, rendering, platform, extension points, API sketches, performance and safety gates |
| [decisions.md](decisions.md) | Decision index D1–D17, G (process-global state), L (dynamic linking), panel decisions O1–O8 and the owner's decisions of 2026-09-25 (strategy, architecture, process), each linked to its record |
| [open-questions.md](open-questions.md) | The 22 owner questions, each resolved on 2026-09-25, and what is still open: unverified claims, ADR conflicts, the sign-off of the CI design ([ci.md](ci.md)) and the gaps of the build-footprint study ([build-footprint.md](build-footprint.md)) |
| [dynamic-linking.md](dynamic-linking.md) | The Bevy-style `dynamic_linking` study: how Bevy does it, what was measured on Windows, the export-count ceiling, and the shape if adopted |
| [build-footprint.md](build-footprint.md) | The build-footprint study: `target/` by artifact kind, test binaries, duplicate builds, peak memory and a warm edit on the Windows host, and each lever kept or rejected, with the ones not measured named |
| [ci.md](ci.md) | The CI design: today's jobs with measured durations, the target lanes, where each job and each workflow need of the migration plan runs, the levers adopted, costs, migration steps and the points awaiting the owner's sign-off |
| [research-findings.md](research-findings.md) | Findings from the raw research that no report or ADR acts on yet, re-checked claims, contradictions and unverified claims |
| [Migration plan](../docs/plans/2026-09-25-architecture-migration-plan.md) | Ordered steps with acceptance commands, risks and rollbacks, mapped to milestones B0–B4 and horizons H0–H4 |
| [Raw research](../docs/research/2026-09-25-architecture-review/synthesis.md) | The architecture review: codebase maps, market survey, design variants, the owner-decisions panel and its verification. Kept as a record; not edited |

## ADRs

All are `Proposed` and dated 2026-09-25. An ADR moves to `Accepted` in the change that ships the
first behaviour it decides; the older ADRs it amends or supersedes get their back-links then.

| ADR | Decision |
|---|---|
| [ADR-0081](../docs/adr/ADR-0081-workspace-tiers-and-reach-facts.md) | Workspace tiers, reach facts, stability kinds, feature policy and the B0 exit |
| [ADR-0082](../docs/adr/ADR-0082-platform-api-contract-crate.md) | `flui-platform-api` is the contract crate; OS backends stay in `flui-platform` (accepted in part: the first move; `PlatformWindow`, `Send` removal and deletions remain proposed) |
| [ADR-0083](../docs/adr/ADR-0083-one-frame-transaction-in-flui-runtime.md) | One frame transaction lives in `flui-runtime` above `flui-widgets` |
| [ADR-0084](../docs/adr/ADR-0084-open-capability-seam-and-plugins.md) | Platform capabilities are an open, typed set in two classes (core-required backend methods, optional plugins) behind one door |
| [ADR-0085](../docs/adr/ADR-0085-reactive-core-placement-and-phase-subscribers.md) | The reactive graph is realm-owned and stays in `flui-view`; reads go through a `ReadScope` contract in `flui-foundation` |
| [ADR-0086](../docs/adr/ADR-0086-signal-writes-through-event-context.md) | Signal writes go through `EventCx` opened by a `WriterSource` |
| [ADR-0087](../docs/adr/ADR-0087-raster-contract-and-cpu-backend.md) | One raster contract in `flui-layer` with wgpu and CPU backends; retained layer identity drives damage |
| [ADR-0088](../docs/adr/ADR-0088-official-packages-sdk-and-facade.md) | Official packages live in this workspace, build on `flui-sdk`, and the facade names none of them |
| [ADR-0089](../docs/adr/ADR-0089-upstream-types-in-stable-signatures.md) | Stable signatures carry no upstream type except raw-window-handle |
| [ADR-0090](../docs/adr/ADR-0090-ime-pull-text-store-contract.md) | IME talks to a pull text-store contract with edits and asynchronous locks |
| [ADR-0091](../docs/adr/ADR-0091-one-owner-thread-isolated-realms-raster-thread.md) | One owner thread hosts isolated realms; one raster thread per `GpuContext`; schedules the `!Send` flip before the first crates.io publication |
| [ADR-0092](../docs/adr/ADR-0092-per-realm-text-over-parley.md) | Text shapes per realm over Parley and crosses the display list as neutral shaped runs |
| [ADR-0093](../docs/adr/ADR-0093-router-is-the-primary-navigation-api.md) | Router is the primary navigation API |
| [ADR-0094](../docs/adr/ADR-0094-hot-reload-through-subsecond.md) | Hot reload goes through Subsecond behind a runtime hook |
| [ADR-0095](../docs/adr/ADR-0095-agent-protocol-schema-crate.md) | `flui-protocol` is the typed schema shared by tests, devtools and agents |
| [ADR-0096](../docs/adr/ADR-0096-dev-build-dynamic-linking.md) | Dynamic linking for development builds: not for framework or test builds; app-side deferred to H1 or later |
| [ADR-0097](../docs/adr/ADR-0097-no-process-global-state-gate.md) | Process-global state is gated: one trampoline cell, everything else realm-owned |

## How this relates to the other plans

- [`docs/ROADMAP.md`](../docs/ROADMAP.md) owns the milestones B0–B4 and their exit criteria. This
  folder does not change them; ADR-0081 §5 proposes a new B0 exit (gates, structural invariants,
  ratchets, hygiene instead of a crate count), and the migration plan maps every step to a
  milestone. The roadmap's text changes only when the owner accepts that proposal.
- [`docs/BETA.md`](../docs/BETA.md) owns the beta acceptance criteria and per-platform evidence.
  The design proposes machine-checked evidence records (`docs/evidence/<platform>.toml`, in the
  migration plan) but does not change what beta must demonstrate.
- The living plan is this folder and the migration plan; `docs/ROADMAP.md` names them as the
  source of truth ([question 18](open-questions.md#18-where-the-living-plan-lives)). The owner's
  own document outside the repository is a mirror and journal that points here. The migration
  plan's last sections list proposed edits to that document for the owner to apply.

## Proposing a change

- **To a decision:** edit the ADR. While it is `Proposed`, revise it in place and keep
  `decisions.md` and `architecture.md` in step. Once it is `Accepted`, write a new ADR that
  supersedes or amends it, as the ADR policy in [AGENTS.md](../AGENTS.md) requires.
- **To the target shape or an open question:** edit the document here, with a `path:line` for
  every claim about code, re-checked at the commit you name in the header.
- **To the order of work:** edit the migration plan; it is the only document here that uses wave
  and step labels.
- `docs/research/2026-09-25-architecture-review/` is a record of the review and is not edited;
  a finding that becomes actionable moves into [research-findings.md](research-findings.md) or an
  ADR.
- `cargo xtask checks` must stay green; it runs the link check over these files. `design/**` is
  in the docs-only list of `tools/xtask/src/change_scope/classify.rs`, so a change that touches
  only this folder compiles nothing in CI.
