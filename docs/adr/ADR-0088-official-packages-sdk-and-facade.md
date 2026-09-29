# ADR-0088: Official packages live in this workspace, build on `flui-sdk`, and the facade names none of them

- **Status:** Proposed. First moves landed (2026-09-26): `flui-sdk` exists and
  `flui-foundation` carries the train guard `links = "flui_train"`; `flui-material` builds on
  `flui-sdk` alone from `packages/flui-material`, `flui-cupertino` builds on `flui-sdk` alone
  from `packages/flui-cupertino`, and the FLUI derives resolve through the SDK first. On
  2026-09-27 `flui-devtools` moved onto `flui-sdk` alone from `packages/flui-devtools`;
  `flui-hot-reload` stays in `crates/` until the runtime hook of ADR-0094 exists (move 4)
  ([migration plan](../plans/2026-09-25-architecture-migration-plan.md)).
- **Date:** 2026-09-25
- **Supersedes in part (on acceptance):** [ADR-0028](ADR-0028-design-system-decoupling-contract.md) — the
  placement of Material and Cupertino as core-workspace crates, and the exemption set
  `{flui-app, flui}` that may depend on them (it named `flui-localizations` too until
  ADR-0081 deleted that crate). Its decoupling rules (shared
  substrate below both, mechanism goes down, capability seams instead of platform branches, raw
  primitives, injected selection chrome, no god-widget entry point) stand.
- **Supersedes in part (on acceptance):** [ADR-0040](ADR-0040-tree-observation-seam.md) §8 —
  the devtools inspector's `flui-foundation`-only dependency and the observation-seam test's
  place in `flui-testing`. The inspector reaches the seam through `flui-sdk`, and the test
  lives in `packages/flui-devtools`; the seam itself, and the rule that no core crate names
  devtools, stand.
- **Amends (on acceptance):** the delivery-layer sentence of the product plan (the living plan linked from
  [`docs/ROADMAP.md`](../ROADMAP.md)), which puts official packages "in separate repositories,
  one release train"
- **Related:** [ADR-0040](ADR-0040-tree-observation-seam.md) (the devtools observation seam),
  [ADR-0041](ADR-0041-workspace-topology-contract.md) (the manifest-as-source rule, which stands;
  its layer table is superseded in part by ADR-0081), [ADR-0042](ADR-0042-theming-ownership.md) (`MaterialApp` stays in the design
  system), [ADR-0043](ADR-0043-presentation-bundled-trees-and-realm-globalkey-scope.md),
  [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md) (tiers, `tier-kind`, reach facts),
  [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md) (what a Stable signature may name),
  [ADR-0094](ADR-0094-hot-reload-through-subsecond.md) (removes the `flui-app → flui-hot-reload`
  edge)
- **Refs:** decisions D7 and D17 and owner decisions 1, 2 and 6 in
  [`design/decisions.md`](../../design/decisions.md); the panel record in
  [`report-decisions.ru.md` §1, §2, §6](../research/2026-09-25-architecture-review/report-decisions.ru.md)

## Context

### Where the design systems sit today

Material and Cupertino are workspace members (`Cargo.toml:33-34`) at layer 7. ADR-0028's rule is
enforced by their own manifests: both list `allowed-dependents = ["flui-app", "flui"]` and
the same `allowed-dev-dependents`
(the design systems' manifests before moves 2 and 3), checked by
`cargo xtask workspace` (`tools/xtask/src/workspace.rs:9-14`, `:146`, `:240-257`). In practice
only the facade depends on them: `flui-material` and `flui-cupertino` are optional facade
dependencies (`Cargo.toml:525-526`) behind `material` and `cupertino` features (`:605-606`), and
`default = ["material"]` (`:598`). `flui-app` is exempted but declares no such edge.

`flui-material` reaches into nine internal crates in its normal dependencies — widgets, view,
types, objects, rendering, foundation, animation, interaction, scheduler
(its manifest before move 2) — each with an exact `=0.2.0-dev` pin
(`Cargo.toml:106` sets the workspace version). A package author outside this repository would
have to copy that list, and any change to the core's crate topology would break their manifest.
The panel counted 172 such exact pins across the workspace (145 in `crates/*/Cargo.toml`, 27 in
the root manifest), 14 of them in `flui-material`.

The facade publicly re-exports Material: `pub use flui_material as material` behind the feature
(`src/lib.rs:146-149`) and a Material half of the prelude (`src/lib.rs:267-276`). The facade
also carries a `hot-reload` feature that pulls `flui-hot-reload` and `flui-app/hot-reload`
(`Cargo.toml:630`), and `flui-app` has its own optional edge to `flui-hot-reload`
(`crates/flui-app/Cargo.toml:65`, `:108`). `flui-material` has no `prelude` module.

Other edges from core crates to what the review classifies as official packages:

- `flui-testing` has a dev-dependency on `flui-devtools` for the observation-seam test
  (`crates/flui-testing/Cargo.toml:104-106`);
- `flui-cli` has a path-only dev-dependency on `flui-hot-reload`
  (`crates/flui-cli/Cargo.toml:112-114`);
- `flui-hot-reload` enables `flui-view/runtime-internals` (`crates/flui-hot-reload/Cargo.toml:27`).

`crates/flui-widgets/Cargo.toml:85` and `:182` mention the design systems only in comments; the
edge there runs from the design systems to `flui-widgets`.

### What the panel measured

The panel record (`report-decisions.ru.md` §1, §2, §6) reports, from `git log` and from probes in
a scratch workspace:

- 68 of the 123 commits that touched the catalog crates also touched another crate under
  `crates/`, so the catalog and the core still change together;
- no `flui-*` crate is published yet, so there is no train to build packages against;
- a nested `packages/` workspace compiled the shared core twice (two fingerprints), `[patch]`
  works only at a workspace root, and `"0.2"` does not match a `0.2.0-dev` prerelease;
- `cargo tree` with deduplication counts 191 crates under the facade against 127 under
  `flui-material`, because the facade pulls `flui-app`, `flui-engine`, wgpu and naga;
- without a one-train guard, an application on `flui = "1"` and a package on `flui-sdk = "0.1"`
  resolved two copies of the internal crates and failed with E0308; with `links` on a shared
  bottom crate the resolver chose one train;
- the facade with `default = ["material"]` still built once Material depended on an sdk crate
  instead of the facade; the Cargo cycle appears only when a package depends on the facade.

These are probe results, not repository tests; §Verification lists the tests that replace them.

## Decision

### 1. Official packages are members of the root workspace

- `packages/` is a directory of members of the **root** workspace: one `Cargo.lock`, one
  `cargo metadata`, path dependencies with exact train pins. It is not a nested workspace.
- A package declares `[package.metadata.flui] tier-kind = "official"` (the key is defined by
  ADR-0081).
- A crate moves into `packages/` in the same change that ports it onto `flui-sdk`, never in a
  change of its own. The first official packages are `flui-material`, `flui-cupertino`,
  `flui-devtools` and `flui-hot-reload`.

### 2. The dependency gate generalises `allowed-dependents`

`cargo xtask workspace` keys the rule by `tier-kind` (the kind rule of ADR-0081 §3) and admits
a refused edge through the dependent's `edge-exceptions` entry, the list the direction rule
already reads; no parallel allowlist is added, and an entry that admits nothing is stale.

- **Forward.** An official package's normal and build dependencies are only `flui-sdk`,
  `flui-platform-api` and `flui-protocol`, plus declared official-to-official edges; its other
  dev-dependencies are free. An edge to another official package, in any kind (dev included),
  is refused unless the dependent's `edge-exceptions` declares it, so ADR-0028's "Material and
  Cupertino do not depend on each other" holds without per-package lists. Until a package
  moves onto `flui-sdk` its internal-crate edges are `edge-exceptions` entries in its manifest
  (`flui-hot-reload`'s four; `flui-devtools`' two went when it moved); the list only shrinks,
  each package's entries go when it moves, and a member under `packages/` may list none.
- **Reverse, strict from the start.** No core crate names an official package in any form:
  normal, optional, build or dev. Named exceptions, each with its reason and exit:

  | Edge | Reason | Exit |
  |---|---|---|
  | `flui-testing` dev → `flui-devtools` | observation-seam test links both halves | expired: the test moved into `packages/flui-devtools` with move 4 |
  | `flui-app` optional → `flui-hot-reload` | reload driver | the change that moves `flui-hot-reload` into `packages/` ([ADR-0094](ADR-0094-hot-reload-through-subsecond.md) §2) |
  | `flui` optional → `flui-hot-reload` (`hot-reload` feature) | facade feature | the same change (ADR-0094 §2) |
  | `flui` dev → `flui-hot-reload` (`Cargo.toml:554`) | a root-package example and test that name it (`examples/scene_render.rs`, `tests/facade_consumer.rs`) | the same change; the example moves with the package |
  | `flui` optional → `flui-material`, `flui-cupertino` | facade features | removed by §6 |

  `flui-cli`'s dev-dependency on `flui-hot-reload` (`crates/flui-cli/Cargo.toml:114`) is not an
  exception: `flui-cli` has kind `tool`, which the rule exempts (ADR-0081 §3).

### 3. Parity with an external author is proven on every change

- An `xtask` command packages `flui-sdk` and every official package with `cargo package`
  (offline, local registry) and builds each from its tarball against its neighbours' tarballs.
  It catches a missing `include`, a feature that exists only through a path dependency, and a
  wrong pin. It runs in `cargo xtask checks` or in a merge-gating CI job.
- An out-of-tree fixture package, outside `members`, depends only on
  `flui-sdk = "=<train>"` through `[patch]` to the path, and uses the public extension points: a
  custom widget, a theme extension, a plugin.
- After the first publication, `cargo-semver-checks` runs on `flui-sdk` in advisory mode.
- There is no job that strips path dependencies and builds against the last published train:
  `main` pins the next, unpublished version, and rewriting the pins down builds a combination no
  user receives. The published pair is checked by `cargo publish` at release.

### 4. `flui-sdk` is the package-author surface

- A separate crate in tier K (ADR-0081), stability kind **Evolving**, host-free: no
  `flui-app`, `flui-engine` or wgpu in its normal dependency closure.
- Version `0.N`, bumped on every train, published in the same run as the train, patches
  included. It pins the internal crates exactly.
- **Stable closure:** whole modules re-exported at the facade's paths
  (`pub use flui_x as x`), with no wrappers or newtypes, so `flui_sdk::m::T` and `flui::m::T` are
  the same type.
- **Evolving part:** only the named modules `pipeline`, `hooks` and `gpu`. Evolving painting and
  rendering items go in `pipeline`; there is no Evolving `paint` module, because `painting` is the
  Stable path the facade already uses. A package's exposure to them is
  `grep -E 'flui_sdk::(pipeline|hooks|gpu)::'`.
- The facade neither depends on nor re-exports `flui-sdk`. An experimental facade module, if one
  is ever needed, is gated by `--cfg flui_unstable`, not by a Cargo feature, which feature
  unification would leak.
- **Curated facade modules.** Where the facade curates a module instead of re-exporting a crate
  (`interaction`, `painting`, `rendering`), the SDK has the same module at the same path, holding
  the subset of its items that packages use, as the same items. An item packages use that no
  facade module exposes goes into an Evolving module.
- **Surface ceiling.** When `flui-sdk` is created its Evolving surface is measured from rustdoc
  JSON. More than about 30 items beyond the hooks means the sdk is becoming a second facade, and
  this decision is revisited. At creation `pipeline` holds three items (`PathClipConfiguration`,
  `RenderPhysicalShape`, `TranslationFraction`), counted from source, because the rustdoc JSON
  tooling is not in place yet; the measurement is in `crates/flui-sdk/ARCHITECTURE.md`. Move 4
  adds `hooks` with one item (`FrameSnapshot`), outside the ceiling.
- **Version.** The crate's own `version = "0.1.0-dev"`, not the workspace's; `cargo xtask
  workspace` refuses a `tier-kind = "evolving"` crate that inherits the version or has a major
  above 0.
- **Graduation (after H3).** An Evolving item moves into a Stable facade module when it has
  survived N trains unchanged and has a second consumer; `flui-sdk` keeps re-exporting it at the
  old path. `gpu` and the development hooks may stay Evolving indefinitely.
- Until `flui-sdk` exists, no documentation invites third-party authors to depend on internal
  crates.

### 5. One train per graph

One bottom crate that everything on the train depends on, `flui-foundation`, declares
`links = "flui_train"` with a trivial build script. Cargo then refuses to put two trains in one
graph: a mismatched application and package resolve to one train or fail in the resolver, never
with E0308. The same guard protects facade-plus-sdk and facade-plus-Material pairs.

The guard is on `flui-foundation` and not on `flui-sdk` because the facade does not depend on the
SDK: with `links` on the SDK alone, an application on `flui` train 2 and a package on `flui-sdk`
train 1 would hold one SDK and two copies of every internal crate, and fail with E0308 as before.
`flui-foundation` is in the normal dependency closure of the SDK, the facade, `flui-platform-api`
and every crate from tier S up except `flui-log`, `flui-assets` and the `flui-cli` tool. `cargo xtask workspace` requires the key on `flui-foundation` and refuses
it on any other member.

The cost is recorded, not hidden: a minor `flui` upgrade in an application waits for its
third-party packages to be re-released on the new train. First-party packages do not lag,
because they are published in the same run. Third-party lag is accepted until H3 and revisited
there.

### 6. The facade names no official package

- The facade gets `default = []` and no `material`, `cupertino`, `devtools` or `hot-reload`
  features. `pub use flui_material as material` and the Material half of the prelude go.
- `flui create` adds `flui-material` to a new project explicitly, and `flui-material` gains a
  `prelude` module with the names the facade prelude carries today, so an application imports
  `flui::prelude::*` and `flui_material::prelude::*`.
- **The reason is semver and train order, not a Cargo cycle.** A Stable facade that publicly
  re-exports an Evolving package makes every major of that package a major of `flui`; publishing
  `flui` must not wait for a package; and the reverse gate of §2 then has no exceptions. The cycle
  exists only if a package depends on the facade, which §2 forbids.
- **Order.** The facade loses the feature only after `flui-material` can be depended on directly
  from a project outside the workspace, that is after its move onto `flui-sdk`.
- **Cadence.** There is no independent cadence for official packages: with exact pins a new
  `flui` and an old `flui-material` do not resolve together, so Material is released in the same
  run as the train.
- A meta-crate bundling the facade with Material is deferred until the two-line install is shown
  to get in the way.

### 7. When a package leaves this repository

A package moves to its own repository only when one of these holds, checked at each train:

- its release cadence has diverged from the train for two consecutive trains;
- it has a maintainer outside the core team;
- fewer than 20% of its commits over the trailing six months also touch core crates (today, for
  the catalog, 55%). The threshold and window are this record's proposal; the method is the
  `git log` count the panel used.

A split repository versions in lockstep with the train. OS plugins and the A2UI renderer are the
first candidates; Material and Cupertino move last.

## Migration

Five moves, each of which leaves `main` green and merges on its own. The
[migration plan](../plans/2026-09-25-architecture-migration-plan.md) tracks them.

| Move | What changes | Waits on |
|---|---|---|
| 1. SDK and guard (in place) | `crates/flui-sdk` is created: tier K, `tier-kind = "evolving"`, `order = 6`, layer 6, `version = "0.1.0-dev"`, with the measured surface of §4 and no consumer yet. `flui-foundation` declares `links = "flui_train"` with a build script that does nothing else (§5). `cargo xtask workspace` requires an evolving crate's own `0.N` version and the guard on `flui-foundation` alone; `cargo xtask reach` states that `flui-foundation` is in the SDK's and the facade's builds | — |
| 2. Material (in place) | `flui-material`'s nine internal normal dependencies become `flui-sdk` (plus `tracing`), its imports move to SDK paths, and it moves to `packages/flui-material` in the same change, with its dev-dependency paths rewritten. Done when no `flui_(widgets\|view\|types\|objects\|rendering\|foundation\|animation\|interaction\|scheduler\|painting)::` path is left in its `src`. Its examples stay with the facade until move 5. **Outcome:** done, with no change to the SDK's surface; the view, inherited and animation derives resolve through `flui-sdk` first, so a package on the SDK alone can use them (the `Diagnosticable` derive has no SDK path yet); the kind rule of §2 is checked, and Material's `allowed-dependents` lists are replaced by it. It landed ahead of the parity command of §3, which still waits | move 1; the `cargo package` parity command of §3 |
| 3. Cupertino (in place) | The same for `flui-cupertino`, which moves to `packages/flui-cupertino`. **Outcome:** done, with no change to the SDK's surface: its normal dependencies are `flui-sdk` and `tracing`, and its six seeded internal-crate exceptions and `allowed-dependents` lists are gone | move 1; independent of move 2 |
| 4. Devtools and hot reload | `flui-devtools` moves onto the SDK and the observation-seam test moves into it, removing `flui-testing`'s dev edge. `flui-hot-reload` moves together with the `DevReloadHook` of ADR-0094 §2, which deletes the `flui-app` edge and the facade's `hot-reload` feature. **Outcome for devtools:** done. Its normal dependencies are `flui-sdk` plus six third-party crates, and its two internal-crate exceptions are gone. The SDK gained `hooks` with one item, `FrameSnapshot`, which the timeline records; the facade has no scheduler module, so the item is Evolving. No public API produces a `FrameSnapshot` yet (only `flui-app`'s crate-private presentation calls `FrameClock::frames_since`), so the timeline bridge is reachable from tests only; a public snapshot source, a frame-telemetry capability on the realm or on `LifecycleContext`, is the follow-up, recorded in `docs/plans/2026-09-25-architecture-migration-plan.md`, and lands after the realm moves into `flui-runtime`. The observation seam (ADR-0040) needed no new item: `foundation::observe` and `foundation::RebuildReason` are Stable paths inside the whole `foundation` re-export, and putting them in `hooks` would break the facade-path rule of §4. The seam test and the observer-overhead bench moved from `flui-testing` to `packages/flui-devtools`. **Hot reload stays in `crates/`:** `flui-app` names it (its `hot-reload` feature, the presentation's `apply_hot_reload`, and the runner code that installs its drivers), and so does the facade; deleting those edges needs the runtime hook of ADR-0094 §1, which as specified cannot host the dlopen worker or the Android scene plugin without amending ADR-0094, and whose Subsecond path is blocked by ADR-0094 §5. Its imports (`flui_layer::Scene` in the plugin ABI, `PipelineOwner`, `WidgetsBinding`, `flui-view/runtime-internals`) are not package-author items: SDK re-exports would add Evolving surface for a path ADR-0094 deletes, and enabling `runtime-internals` from the SDK would leak it to every package. Its two `reach-exceptions` and six `globals` entries, all exiting through ADR-0094, are more than a member under `packages/` may carry | move 1; for hot reload also `flui-view`'s `runtime-internals` feature replaced by a hidden module, and the runtime hook of ADR-0094 |
| 5. Facade | §6: `default = []`, no `material`/`cupertino` features, dependencies, `edge-exceptions`, re-exports or Material prelude half; `flui_material::prelude`; `flui create` adds `flui-material`, with a CLI test that checks the generated project; Material examples move to `packages/flui-material/examples`; `cargo xtask facade-combos` and the documents from the `rg` list of the Consequences are updated; the CI Material example build changes with the owner's sign-off | moves 2 and 3 |

Material's and Cupertino's `allowed-dependents` lists were replaced in moves 2 and 3 by the kind
rule of §2, which refuses every edge they refused: a core crate names either design system only
through a named exception (the facade's edges are such exceptions until move 5), and another
official package names one, in any dependency kind, only through a declared exception, none of
which exists.

## Alternatives considered

- **A nested `packages/` workspace built against the published train.** Rejected: no train is
  published, so until then it is path dependencies with extra cost — the core compiled twice,
  `[patch]` only at a root, prerelease pins, a change classifier that loads one root
  (`tools/xtask/src/change_scope/classify.rs:325-330`), a new CI job.
- **Separate repositories now.** Rejected: two-way pins and a roller bot, as Flutter ran before
  consolidating its own repositories; 55% of catalog commits touch the core.
- **Packages depend on the facade.** Rejected: every package would pull the host, engine and
  wgpu (191 against 127 crates), and optional facade features naming packages would form a Cargo
  cycle.
- **The package surface inside the Stable facade.** Rejected: it freezes about a dozen unproven
  internals (`DrawOp`, `Canvas`, `RenderUpdateImpact`, `LocalPostFrameHandle`, …) and still pulls
  the host into every package.
- **An Evolving module in the facade behind an `unstable` feature.** Rejected: feature
  unification leaks it to every crate in the graph.
- **Status quo: packages pin internal crates exactly.** Rejected: the core's crate topology
  becomes part of every external manifest, and each topology change breaks all of them.
- **Keep `default = ["material"]`.** Rejected: cheap now and a semver break after the first
  Stable release, and it needs an exception in the reverse gate.

## Consequences

- ADR-0028's placement and exemption set are replaced; `flui-material` and `flui-cupertino`
  manifests drop `allowed-dependents` in favour of `tier-kind`. This record owns that partial
  supersession of ADR-0028; ADR-0081 supplies the kind rule it uses and owns the partial
  supersession of ADR-0041's layer table.
- **Breaks for applications.** `flui::material::…` becomes `flui_material::…`;
  `features = ["material"]` / `["cupertino"]` / `["hot-reload"]` on `flui` disappear; an
  application adds `flui-material` (and development tools) as its own dependencies. The CHANGELOG
  carries the migration note.
- **Breaks for this repository.** Material examples move to `packages/flui-material/examples`;
  `cargo xtask facade-combos` and the AGENTS.md "Extending FLUI" row about
  `[[example]] required-features` are rewritten; `tests/facade_smoke.rs`, README, the crate docs
  and the book are updated from the list
  `rg 'flui::(material|cupertino)|features.*(material|cupertino)'` produces (excluding
  `docs/archive/`). `.github/workflows/ci.yml:924` builds `--features material --example
  sliver_demo`; that line changes, which needs the owner's sign-off and the `full-ci` label.
- The name `flui-sdk` is free on crates.io (checked 2026-09-26, when the crate was created); the
  fallback `flui-package-sdk` is not needed.
- **The guard also ties platform plugins to the train.** `flui-platform-api` depends on
  `flui-foundation` (ADR-0082), so a plugin built on one train and an application on another
  now fail in the resolver instead of with E0308.
- **The guard does not cover a lone duplicate outside its closure.** Two copies of `flui-geometry`,
  `flui-types`, `flui-macros`, `flui-protocol`, `flui-log` or `flui-assets` with one
  `flui-foundation` are not refused; only a crate that depends on those internal crates directly,
  which is not supported for third parties, can produce that graph.
- Ordering relative to the other records is in
  [the migration plan](../plans/2026-09-25-architecture-migration-plan.md).

## Verification

In place with move 1:

- **One train per graph (§5).** `two_trains_refuse_to_resolve` in `tools/xtask` reads
  `flui-foundation`'s `links` from this repository's `cargo metadata`, builds an application
  that depends on an SDK over one copy of `flui-foundation` and a facade over another, and
  requires `cargo metadata --offline` to fail on the shared `links` value; the same graph
  without `links` resolves both copies, which is where E0308 came from. It uses path packages
  rather than a local registry: the resolver's `links` rule is the same for both sources.
  `cargo xtask workspace` requires the key on `flui-foundation` and refuses it on any other
  member (its self-test plants a second guard).
- **Type identity (§4).** `crates/flui-sdk/tests/surface.rs` checks, for each curated item and
  one type of each whole-module re-export, that `flui_sdk::m::T` is `flui::m::T`; it fails to
  build on a wrapper or newtype. The same file pins the list of re-exports and names every
  measured item through its SDK path.
- **Host-free (§4).** `cargo xtask reach` checks `flui-sdk` at its defaults and with all
  features against tier K's forbid set, which contains `wgpu`, `flui-engine` and `flui-app`, so
  no separate absence fact is needed; its facts state that `flui-foundation` is in the SDK's
  build and in the facade's `--no-default-features` build. After move 2, "`wgpu` is absent from
  `flui-material`'s closure" follows from the `pkg` tier's forbid set. These are reach facts,
  not `cargo tree -i` probes, because `cargo tree -i` errors on an absent package instead of
  printing nothing.
- **Own version (§4).** `cargo xtask workspace` refuses an evolving crate that inherits the
  workspace version or has a major above 0.

In place with moves 2 and 3:

- **The kind rule (§2).** `cargo xtask workspace` fails, in its own self-test, on a core crate
  that names an official package (normal and dev planted), on an official package whose normal
  edge leaves the SDK, on an official package's dev edge to another official package, and on a member under `packages/` that is not official or lists
  `edge-exceptions`; excepted edges and a `tool` crate's edge stay silent.
- **The design systems on the SDK alone.** The manifests are
  `packages/flui-material/Cargo.toml` and `packages/flui-cupertino/Cargo.toml`, and each one's
  normal and build dependencies are `flui-sdk` and `tracing`; the kind rule above fails
  `cargo xtask workspace` on any other framework edge, and neither keeps an
  `allowed-dependents` list.
- **Derives through the SDK.** `sdk_consumers_derive_through_the_sdk_even_beside_the_facade` in
  `tests/facade_consumer.rs` builds a consumer on `flui-sdk` alone (plain and renamed) and one
  with the facade as a dev-dependency, each using the FLUI derives.

In place with move 4 (devtools):

- **Devtools on the SDK alone.** The manifest is `packages/flui-devtools/Cargo.toml`, it lists
  no `edge-exceptions`, and its only framework normal dependency is `flui-sdk` (the rest are
  `parking_lot`, `serde`, `serde_json`, `tracing`, `tracing-subscriber` and `web-time`), held
  by the kind rule. The seeded `edge-exceptions` no longer hold devtools' or
  `flui-testing`'s entries, and `crates/flui-sdk/tests/surface.rs` pins `hooks::FrameSnapshot`
  and names the `foundation::observe` items.

Not yet in place:

- The `cargo package` parity command of §3 passes on every change.
- The out-of-tree fixture builds against `flui-sdk` alone.
- The Evolving surface counted from rustdoc JSON instead of from source.
- A `flui-cli` test generates the counter template and runs `cargo check` on it with
  `flui-material` as a direct dependency.
- A doctest that `use flui::prelude::*; use flui_material::prelude::*;` resolves without
  ambiguity.
