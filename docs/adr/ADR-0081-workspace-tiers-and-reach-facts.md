# ADR-0081: Workspace tiers, reach facts and stability kinds

- **Status:** Accepted in part (2026-09-26): §1 (tiers, `order`, the direction rule,
  `edge-exceptions`) and the `tier-kind` declarations of §3. §2 (reach), the kind rules of §3
  (core never names official, the forward allowlist), §4 and §5 remain Proposed.
- **Date:** 2026-09-25
- **Supersedes in part (on acceptance):** [ADR-0041](ADR-0041-workspace-topology-contract.md)
  (the numbered layer table, "a crate is a layer" as the only reason for a crate, and the
  "Localization direction is locked" paragraph). The manifest-as-source rule, the
  `allowed-dependents`/`allowed-dev-dependents` mechanism, the dev-edge exemption and the
  rejected alternatives stand. ADR-0041's "No `flui-runtime` without two consumers" paragraph
  is superseded separately by [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md).
- **Related:** [ADR-0028](ADR-0028-design-system-decoupling-contract.md) (the dependent
  restriction this generalizes), [ADR-0037](ADR-0037-presentation-ownership-domains.md) (the
  `interaction -> platform` edge), [ADR-0078](ADR-0078-rules-live-in-types-and-lints.md)
  (prefer a type or a lint to a scanner),
  [ADR-0082](ADR-0082-platform-api-contract-crate.md),
  [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md),
  [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md),
  [ADR-0097](ADR-0097-no-process-global-state-gate.md)
- **Refs:** decisions D8, D11 and D17 and owner decision 4 of the
  [architecture review](../research/2026-09-25-architecture-review/report-decisions.ru.md);
  index in [`design/decisions.md`](../../design/decisions.md)

## Context

Line citations in this section are to `c2ba3ae51`, before the tier gate landed.

ADR-0041 made the layer graph a claim the manifests make and `cargo xtask workspace` checks:
each crate declares `[package.metadata.flui] layer = N`, the root names eleven layers
(`Cargo.toml:91-103`), and a normal or build edge may point to the same layer or lower
(`tools/xtask/src/workspace.rs:1-8`). A crate may narrow its dependents with
`allowed-dependents`/`allowed-dev-dependents` (`tools/xtask/src/workspace.rs:9-14`, keys at
`:80-81`, read at `:146-147`, enforced at `:225-262`); the design systems use it for ADR-0028
(`crates/flui-material/Cargo.toml:87-88`, `crates/flui-cupertino/Cargo.toml:84-85`) and
`flui-log` for composition roots (`crates/flui-log/Cargo.toml:66`).

That gate checks direct edges only. It cannot say what must never be reachable, and the graph
already contains what it cannot see:

- `flui-interaction` depends on `flui-platform` (`crates/flui-interaction/Cargo.toml:31`) for one
  import (`crates/flui-interaction/src/text_input.rs:27`, `use flui_platform::traits::PlatformTextInput;`).
  `flui-platform`'s default feature `desktop = ["dep:winit"]`
  (`crates/flui-platform/Cargo.toml:300,303`) then puts winit, and on Windows the `windows`
  crate, into the normal graph of every crate above interaction. On this host
  `cargo tree -p flui-interaction -e normal -i winit --locked` prints
  `winit <- flui-platform <- flui-interaction`, and `cargo tree -p flui-view -e normal -i windows`
  prints `windows <- flui-platform`. Pruning `flui-platform` removes both.
- The widget harness adds a second direct edge under the `testing` feature
  (`crates/flui-widgets/Cargo.toml:94`).
- The only reach facts that exist are three hard-coded `cargo tree` probes about hot reload
  (`TREE_FACTS`, `tools/xtask/src/tasks/facade.rs:53-92`).

The layer numbers also carry no statement about stability. Every crate publishes at the
workspace version with an exact pin, so every public item of every crate is, by default, a
semver promise. The review found that the facade re-exports whole crates and that the Stable
surface is the transitive closure of what the Stable modules name, not a crate count.

Features are used for things features cannot express:

- A feature with no effect: `flui-platform`'s `desktop` and `web`
  (`crates/flui-platform/Cargo.toml:303,324`) have zero `cfg(feature = ...)` sites in
  `crates/flui-platform/src`; `desktop` only adds winit to the graph, which the
  `winit-backend` feature already controls (`:314`). `flui-app` declares `desktop`, `android`,
  `ios`, `web`, `debug-overlay` and `performance-overlay` (`crates/flui-app/Cargo.toml:52-55,85-86`)
  with zero `cfg` sites each; `flui-geometry`'s `mint` (`crates/flui-geometry/Cargo.toml:49`)
  likewise.
- A feature as a visibility switch: `flui-view`'s `runtime-internals`
  (`crates/flui-view/Cargo.toml:116-118`, "Not an application-facing API contract") gates 27
  `cfg` sites in `crates/flui-view/src`, and is turned on by `flui-app`
  (`crates/flui-app/Cargo.toml:90`), `flui-testing` (`crates/flui-testing/Cargo.toml:51`) and
  `flui-hot-reload` (`crates/flui-hot-reload/Cargo.toml:27`). Feature unification enables it
  for every application that links `flui-app`, so the boundary it claims does not exist.

Finally, the B0 exit criterion is a count: "26 crates; no file over 3000 lines in `flui-app`
and `flui-widgets`" and "`cargo xtask ci` green on a clean Mac" (`docs/ROADMAP.md:11`). The
count coincides with the target by accident, and a count proves nothing about shape. The
in-process check list pinned by a test (`tools/xtask/src/tasks/checks.rs:100-123`, the pinning test at
`:149-172`) has no reach, globals, module-graph or file-length check, and only `wgsl` has a
`--self-test` (`:112`).

## Decision

### 1. Tiers replace layer numbers

Every workspace package declares, in `[package.metadata.flui]`:

- `tier` — one of `V`, `C`, `S`, `R`, `K`, `H`, `pkg`, named in the root
  `[workspace.metadata.flui] tiers` in that order;
- `tier-kind` — one of `stable`, `evolving`, `internal`, `official`, `tool` (§3);
- `order` — the crate's position inside its tier, an integer unique within the tier.

The rule for a **normal or build** edge between workspace packages: it points to a lower tier,
or to the same tier and a smaller `order`. Cargo's cycle check stays, but in-tier direction no
longer depends on it. **Dev edges** keep ADR-0041's rule: they may point anywhere unless the
target restricts them with `allowed-dev-dependents`. The four dev cycles in the graph stay legal
and no new restriction is added for them: `flui-view <-> flui-testing`
(`crates/flui-view/Cargo.toml:63-67`), `flui-interaction <-> flui-testing`,
`flui-scheduler <-> flui-testing` and `flui-rendering <-> flui-objects`. A library package
without a valid `tier`, `tier-kind` and `order` fails the gate. Examples and `tools/*` declare
only `tier-kind = "tool"`: they carry no `tier` or `order`, and stay exempt from the edge rule as
applications. No package with a tier has a normal or build dependency on a `tool` package.

An edge the rule refuses is legal only while the **dependent** lists it in its
`[package.metadata.flui] edge-exceptions`, an array of
`{ to = "<package>", exit = "ADR-NNNN", reason = "<text>" }`: `exit` names the ADR whose change
removes the edge, and its file must exist under `docs/adr`. An entry for an edge that does not
exist, or that the rule admits, is itself a finding, so the list only shrinks. The key exempts
the tier rule only; the layer rule of ADR-0041 is checked beside it, unchanged, until the `layer`
key is removed in a later change.

`pkg` is ordered after H, so an edge from a host to an official package points up. Such an edge
is legal only when it is one of the dated exceptions listed under "Core never names official"
in §3; each is an `edge-exceptions` entry on the host, which the kind rule of §3 reads as well,
and the gate reports any other H → `pkg` edge.

The tier assignment has six refused edges, each seeded as an `edge-exceptions` entry:
`flui-interaction -> flui-platform` and `flui-widgets -> flui-platform` (the widget harness,
optional under `testing`) exit with ADR-0082; `flui-app -> flui-hot-reload` and
`flui -> flui-hot-reload` exit with ADR-0094; `flui -> flui-material` and
`flui -> flui-cupertino` exit with ADR-0088. In K, `flui-widgets` names `flui-testing` as an
optional normal dependency (`crates/flui-widgets/Cargo.toml:89`), so `flui-testing` has the
smaller `order`; moving the harness above the runtime removes that edge first.

| Tier | Contains | Forbidden in the normal graph (reach fact) |
|---|---|---|
| **V** values | `flui-geometry`, `flui-types`, `flui-macros`, `flui-foundation` (`flui-reactive` if [ADR-0085](ADR-0085-reactive-core-placement-and-phase-subscribers.md) extracts it) | everything in S's set, plus `tokio` |
| **C** contracts | `flui-platform-api` ([ADR-0082](ADR-0082-platform-api-contract-crate.md)), `flui-protocol` ([ADR-0095](ADR-0095-agent-protocol-schema-crate.md)) | S's set |
| **S** substrate | `flui-log`, `flui-scheduler`, `flui-painting`, `flui-interaction`, `flui-semantics`, `flui-animation`, `flui-assets` | K's set |
| **R** render machine | `flui-layer`, `flui-rendering`, `flui-objects`, `flui-engine` (and a CPU backend, [ADR-0087](ADR-0087-raster-contract-and-cpu-backend.md)) | K's set minus `wgpu`, which only `flui-engine` may reach |
| **K** spine and runtime | `flui-view`, `flui-widgets`, `flui-runtime` ([ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md)), `flui-testing`, `flui-sdk` ([ADR-0088](ADR-0088-official-packages-sdk-and-facade.md)) | `flui-platform`, `winit`, `android-activity`, `ndk`, `windows`, `objc2-app-kit`, `objc2-ui-kit`, `wgpu`, `flui-engine`, `flui-app` |
| **H** hosts | `flui-platform` (OS backends), `flui-app` (runners), `flui-cli`, the `flui` facade | none |
| **pkg** official packages | `flui-material`, `flui-cupertino`, `flui-devtools`, `flui-hot-reload`, later packages | K's set, plus any other OS crate, with named exceptions |
| **deleted** | `flui-tree`, `flui-localizations` | — (transitional tier until removed; see below) |

The K set is the corrected list from owner decision 4. Generic FFI crates (`windows-sys`,
`jni`, bare `objc2`, `core-foundation`) are **not** forbidden in K: they arrive through
`reqwest -> rustls-platform-verifier -> jni` behind `network-images`
(`crates/flui-widgets/Cargo.toml:177`, `crates/flui-assets/Cargo.toml:69`) and say nothing
about a windowing backend. A package that must reach a forbidden crate lists it under
`reach-exceptions` with a reason; `flui-hot-reload`'s direct `windows` dependency
(`crates/flui-hot-reload/Cargo.toml:47`) is the first entry, with
[ADR-0094](ADR-0094-hot-reload-through-subsecond.md) as its exit.

`flui-tree` and `flui-localizations` are deleted; the owner confirmed both deletions on
2026-09-25. `flui-tree`'s tree traits have no generic consumer: its arity, slot and depth markers
fold into `flui-foundation`, and the read, navigation and write traits become inherent methods.
`flui-localizations` (a crate alone in its own layer) moves its RTL table into
`flui_widgets::localization` and its other tables into the packages. Until those removals land, both carry a transitional tier (`V` for `flui-tree`, `K` with kind `internal` for `flui-localizations`, which depends only on `flui-widgets` and `flui-types`), and their dependents are frozen with the existing `allowed-dependents` and
`allowed-dev-dependents` keys, under a comment that names this record: `flui-tree` admits the
six crates that name it today (`flui`, `flui-layer`, `flui-objects`, `flui-rendering`,
`flui-semantics`, `flui-view`) and no dev-dependent, `flui-localizations` admits only `flui`.
The lists only shrink. ADR-0041's "Localization direction is locked" paragraph no
longer applies: nothing in the catalog tiers depends on a localization crate.

### 2. Reach facts are a gate over the resolved graph

A new command, `cargo xtask reach`, reads `cargo metadata --locked` without a target filter
(the equivalent of `--target all`), for the facade's default features, no features, all
features, and every single facade feature. For each package it computes the normal-edge
closure and fails when a package matching its tier's forbidden set, by name or glob, is in it
and is not a declared `reach-exceptions` entry. Matching is on package names, not
`cargo tree -i`, which fails on an absent package and on ambiguous specs (`objc2-app-kit` is
already ambiguous in this graph). The tier gate (§1) checks direct edges and `reach` checks
transitive absence; they do not overlap. These four feature sets are what "every facade feature
combination" means everywhere in these records.

A package may add names to its tier's forbidden set with `reach-forbid` in its own
`[package.metadata.flui]`, and may never remove one except through `reach-exceptions`. This is
how a single crate states a fact its tier cannot, such as "`wgpu` is absent from `flui-layer`"
(ADR-0087) or "`tokio` and `accesskit` are absent from `flui-platform-api`" (ADR-0082).

`TREE_FACTS` (`tools/xtask/src/tasks/facade.rs:53-92`) moves into `reach` as ordinary facts in
the same change. Two of its three probes are positive facts (a feature *brings in* a crate);
`reach` carries a `require` list for those.

Today the gate is red, and only through `flui-platform`, by two direct edges:
`flui-interaction → flui-platform` and `flui-widgets → flui-platform` under the widget harness's
`testing` feature (`crates/flui-widgets/Cargo.toml:94`), which the facade's `testing` feature
turns on. It turns green with the trait move of ADR-0082, not before.

### 3. Stability kinds

`tier-kind` states what a crate promises:

| Kind | Promise | Crates |
|---|---|---|
| `stable` | semver on the transitive closure of its public signatures; the only crates a 1.0 freezes | `flui`, `flui-platform-api`, `flui-protocol` |
| `evolving` | own `0.N` version, bumped on every release train; no promise across trains | `flui-sdk` |
| `internal` | none; exact-pinned by the crates above; not for direct use | every other core crate |
| `official` | a package built on `flui-sdk` and the two contract crates, released on the same train | the `pkg` tier |
| `tool` | an application; never a dependency | `flui-cli`, `tools/*`, examples |

"Three stable crates" is not the size of the promise. The size is **N items**: the transitive
closure of public types reachable from the three crates' public items, measured before the
first freeze and recorded here when measured. It has not been measured yet; the tool that
measures it is the closure gate of ADR-0089.

**Core never names official.** A package whose `tier-kind` is not `official` or `tool` must not
depend on an `official` package in any form — normal, build, dev or optional. This generalizes
ADR-0028's `allowed-dependents` by kind instead of by a hard-coded exemption set. It is strict
from the first day, with named, dated exceptions:

- `flui-testing` dev -> `flui-devtools` (`crates/flui-testing/Cargo.toml:106`): exits when the
  observation-seam test moves into `flui-devtools`;
- `flui-app` optional -> `flui-hot-reload` (`crates/flui-app/Cargo.toml:65,108`), the facade's
  optional edge and `hot-reload` feature (`Cargo.toml:553,664`) and the facade's dev-dependency
  (`Cargo.toml:588`): exit in the change that moves `flui-hot-reload` into the official packages
  (ADR-0094 §2);
- the facade's optional `material`/`cupertino` edges, features and default
  (`Cargo.toml:559-560,632,639-640`): exit with ADR-0088 §6, which sequences the facade change
  after the packages build on `flui-sdk`.

`flui-cli`'s dev-dependency on `flui-hot-reload` (`crates/flui-cli/Cargo.toml:114`) needs no
entry: `flui-cli` has kind `tool`. The partial supersession of ADR-0028's exemption set is
ADR-0088's; this record supplies the kind rule it uses.

The forward direction (an `official` package depends only on `flui-sdk`, `flui-platform-api`,
`flui-protocol` and declared `official` edges) is ADR-0088's rule; until `flui-sdk` exists it
runs as an allowlist that can only shrink.

A new crate still needs a reason, as ADR-0041 required: a tier, a kind, and an ADR that names
its second consumer or the compile or semver seam it buys.

### 4. Feature policy

1. **A feature must do something the crate can see.** Every feature is named by at least one
   `cfg(feature = "...")` in the crate's own sources, or forwards to a feature of a dependency
   (`dep/feat`, `dep?/feat`). A feature that only activates an optional dependency (`dep:x`) must
   also be named by a `cfg`. `cargo xtask workspace` fails otherwise. Today this removes
   `flui-platform`'s `desktop` and `web`, `flui-app`'s six empty features and `flui-geometry`'s
   `mint` (with `flui-types`'s forwarding `mint`).
2. **A feature is not a visibility switch.** Cross-crate internals that composition roots need
   live in `#[doc(hidden)] pub mod __runtime` in the owning crate, always compiled, with a
   module doc stating that it carries no semver promise. `runtime-internals` is removed and its
   27 `cfg` sites become `__runtime` items. No gate can tell a visibility feature from a real
   one, so this rule is a review rule; the one instance is removed.
3. Features stay additive, and every optional dependency sits behind `dep:` (unchanged).

### 5. The B0 exit is gates, invariants and ratchets

The B0 exit criterion in `docs/ROADMAP.md:11` is replaced by four classes, as owner decision 4
recorded:

- **[G] Gates exist and can fail.** The in-process list pinned in `tools/xtask/src/tasks/checks.rs`
  contains `workspace` (tiers), `reach`, `globals` (ADR-0097), `module-dag`, `markers`,
  `file-length` and the core-never-names-official check. Each has a `--self-test` that runs it on
  a planted violation, as `wgsl --self-test` does. A gate without both does not count.
- **[S] Structural invariants are green.** `cargo xtask workspace` reports zero findings under
  tiers; `cargo xtask reach` is green for the four feature sets of §2 (this requires
  ADR-0082's trait move); `cargo xtask module-dag -p flui-widgets` is green; and, if the owner
  keeps the runtime extraction in B0, the frame-phase entry points are reachable only from
  `flui-runtime` (ADR-0083).
- **[R] Debt is frozen.** The `globals`, `file-length` (3000 lines), undocumented-`unsafe` and
  duplicate-versions allowlists are seeded by the scan itself in the same change as their gate
  and can only shrink.
- **[P] Hygiene.** `cargo xtask ci` is green on `macos-latest`; `cargo build --workspace` works
  from the README without lld; the workspace version is `0.2.0`. The crate count is a fact of
  the tier table, not a target.

## Alternatives considered

- **Keep numbered layers and add `forbid-reach` per crate.** Eleven numbers already encode
  accidents (`flui-platform` at layer 2 under everything, `flui-localizations` alone at 8). A
  per-crate forbid list repeats the same set on every K crate and drifts; a per-tier set is
  stated once. Per-crate `reach-forbid` (§2) only adds to the tier's set.
- **Derive stability from the facade's re-exports.** A crate re-exported by `flui` would be
  Stable by accident, which is exactly how the surface grew. The kind must be an independent
  claim, for the same reason ADR-0041 rejected generating layers from Cargo.
- **Roughly eight Stable crates, or one.** Eight multiplies the semver-checked surface for
  crates no application names; one forces platform plugins to depend on the whole facade.
  Three matches who consumes what: applications, plugin authors, tools.
- **Check reach with `cargo tree -i`.** It errors on an absent package and on a name with two
  versions, so a green result would be indistinguishable from a broken probe.
- **`cargo-deny` bans for reach.** Bans are global or per-dependent-crate; they cannot say
  "forbidden in K, allowed in H".
- **Keep `runtime-internals` and add an `unstable` feature for the rest.** A feature that
  changes visibility is unified on by any crate in the graph, so it never hides anything from
  an application; `#[doc(hidden)]` states the same intent without pretending.
- **Keep the crate count in the B0 exit.** It would be met or missed by merges and splits that
  change nothing about direction or reach.

## Consequences

- Every manifest changes: `layer` becomes `tier`, `tier-kind` and `order`. The root
  `layers` array becomes `tiers` with the forbidden sets. `flui-platform` moves from the
  bottom of the graph to H, which is the point: nothing below the hosts may name it.
- ADR-0028's hard-coded exemption set (`flui-localizations`, `flui-app`, `flui`) is replaced by
  ADR-0088 using this record's kind rule and its dated exceptions. ADR-0028's decoupling rules
  are unchanged.
- The reach gate is red until ADR-0082's trait move lands, so B0 closes only with it.
- Applications that enabled `flui-view/runtime-internals` directly (none in this workspace
  besides the three composition roots) lose the feature; the items stay reachable under
  `__runtime`.
- AGENTS.md's "Crate layering" rows and the "Crate" row of "Extending FLUI" and `docs/crates.md`
  changed with the tier gate; `docs/ROADMAP.md:11` changes with §5. ADR-0041 carries the
  `Superseded in part by: ADR-0081` back-link.
- Migration: one change added the three keys and `edge-exceptions` next to `layer` and taught
  the gate both; a second removes `layer`. `reach` lands with its allowlist seeded from its own
  first run.

## Verification

For the accepted part:

- `cargo xtask workspace` checks the tier rule on every manifest and reports zero findings;
  removing the `flui-interaction` `edge-exceptions` entry makes it fail with
  `flui-interaction (tier S, order 4) depends on flui-platform (tier H, order 1)`.
- `cargo xtask workspace --self-test` runs the rule over a built-in graph that plants an upward
  cross-tier edge, an in-tier edge against `order`, an H → `pkg` edge without an exception, a
  stale exception, a missing `tier-kind`, a duplicate `order` and an edge onto a `tool`; it
  fails unless exactly those are reported. `cargo xtask checks` runs it before `workspace`.
- `cargo nextest run -p xtask workspace`: `an_upward_tier_edge_is_refused`,
  `an_in_tier_edge_to_a_larger_order_is_refused`, `an_in_tier_edge_to_a_smaller_order_is_allowed`,
  `a_dev_edge_may_point_up_a_tier`, `a_dev_cycle_inside_a_tier_is_allowed`,
  `a_crate_without_tier_order_or_kind_is_reported`, `an_unknown_tier_or_kind_is_reported`,
  `two_crates_sharing_an_order_in_a_tier_are_reported`, `an_example_declares_only_the_tool_kind`,
  `nothing_depends_on_a_tool_kind_crate`, `an_edge_exception_admits_one_upward_edge`,
  `a_stale_edge_exception_is_reported`, `an_edge_exception_citing_a_missing_adr_is_reported`,
  `the_self_test_reports_exactly_the_planted_findings`, and
  `the_tiers_match_the_adr_0081_table`, which pins the table above and the six seeded
  exceptions against the real manifests.

Still to come, with the Proposed parts:

- `cargo xtask workspace --self-test` also plants a core crate with an optional dependency on an
  `official` crate (§3) and a feature with no `cfg` site (§4).
- `cargo xtask reach --self-test`: plants a K crate depending on `winit`; must fail. The first
  real run must fail on `flui-interaction -> flui-platform` and `flui-widgets -> flui-platform`
  (under `testing`) before ADR-0082's trait move and pass after it.
- The pinned list test in `tools/xtask/src/tasks/checks.rs` names the new gates, so removing
  one from `checks` fails a unit test.
- A doc-hidden check: `cargo doc -p flui-view` output contains no `__runtime` page.
