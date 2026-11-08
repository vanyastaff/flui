# ADR-0088: Official packages live in this workspace, build on `flui-sdk`, and join the facade by feature

- **Status:** Accepted (2026-11-07)
- **Date:** 2026-09-25
- **Superseded-by:** [ADR-0315](ADR-0315-first-party-packages-remain-train-aligned-facade-features.md)
  for former §6 and move 5b only (facade package features and re-exports remain; their release and
  prelude rules are stated there)
- **Related:** [ADR-0028](ADR-0028-design-system-decoupling-contract.md) (design-system decoupling),
  [ADR-0040](ADR-0040-tree-observation-seam.md) (the devtools observation seam),
  [ADR-0041](ADR-0041-workspace-topology-contract.md) (the manifest-as-source rule),
  [ADR-0042](ADR-0042-theming-ownership.md) (`MaterialApp` stays in the design system),
  [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md) (tiers, kinds and reach facts),
  [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md) (the Stable signature closure),
  [ADR-0098](ADR-0098-owned-f64-geometry-values.md) (the deleted duplicate value crates),
  [ADR-0154](ADR-0154-capability-crates.md) (capability crates are a distinct package kind),
  [ADR-0311](ADR-0311-development-reload-is-a-hook-the-application-installs.md) (the reload hook and
  host boundary), [ADR-0315](ADR-0315-first-party-packages-remain-train-aligned-facade-features.md)
  (standing facade aggregation)
- **Sources:** decisions D7 and D17 and owner decisions 1, 2 and 6 in
  [`design/decisions.md`](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/design/decisions.md);
  the panel record in
  [`report-decisions.ru.md` §1, §2, §6](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/docs/research/2026-09-25-architecture-review/report-decisions.ru.md)

## Context and problem statement

Material and Cupertino began as workspace crates that named many internal crates directly. An
external package author would have to reproduce that manifest and track every core topology change.
The review also found that the catalog and core usually changed together, that a nested `packages/`
workspace compiled the core twice, and that facade-plus-package graphs without a train guard could
resolve two incompatible copies of internal crates.

Official packages therefore need a package-author surface smaller than the host-bearing facade, one
workspace and lockfile for first-party development, and one train identity in every resolved graph.
The facade's application-facing aggregation is a separate decision: ADR-0315 keeps it permanent and
defines its semver and release consequences.

## Decision drivers

- Official packages build against public extension points, not the core's changing crate topology.
- First-party packages and the core change together and use exact train pins.
- The package-author surface must not pull the application host, engine or wgpu into a package.
- Cargo must resolve one FLUI train in a graph, rather than failing later with incompatible types.
- The facade and SDK expose the same items at matching paths, never wrappers with distinct type
  identity.
- Facade aggregation is opt-in and train-aligned under ADR-0315; it is not a reason to reverse the
  package dependency direction.

## Considered options

1. Root-workspace official packages on `flui-sdk`, one train guard, and opt-in facade aggregation.
2. A nested `packages/` workspace built against the published train.
3. Separate package repositories from the start.
4. Packages depending on the facade.
5. The package-author surface inside the Stable facade.
6. Packages pinning internal crates directly.
7. Removing every facade package feature and requiring direct package dependencies.

## Decision

Chosen option: 1. It gives package authors a host-free SDK and first-party packages one train,
while ADR-0315 preserves the application facade's opt-in aggregation.

### 1. Official packages are members of the root workspace

- `packages/` contains members of the root workspace: one `Cargo.lock`, one `cargo metadata`, and
  path dependencies with exact train pins. It is not a nested workspace.
- An official package declares `[package.metadata.flui] tier-kind = "official"` under ADR-0081.
- A crate moves into `packages/` in the same change that ports it onto `flui-sdk`. The first
  official packages are `flui-material`, `flui-cupertino`, `flui-devtools` and `flui-hot-reload`.
  Capability crates follow ADR-0154 instead.

### 2. The dependency gate generalises `allowed-dependents`

`cargo xtask workspace` keys the rule by `tier-kind` and admits an otherwise refused edge through
the dependent's `edge-exceptions`; no parallel per-package allowlist is added.

- **Forward.** An official package's normal and build dependencies are only `flui-sdk`,
  `flui-platform` and `flui-protocol`, plus declared official-to-official edges. Other
  dev-dependencies are free. An edge to another official package, dev included, requires a declared
  exception. A member under `packages/` has no internal-crate `edge-exceptions`.
- **Reverse.** No core crate other than the facade names an official package in normal, optional,
  build or dev dependencies. The facade's optional, feature-gated aggregation edges are the
  standing exception of ADR-0315 §2, not temporary debt. Applications and tools retain the
  exemptions stated by ADR-0081.
- `flui-app` reaches reload only through `DevReloadHook`; it has no edge to `flui-hot-reload`.
  `flui-hot-reload`'s remaining internal-crate edges exit through ADR-0311 §6 when that package
  completes its SDK move.

### 3. Parity with an external author is proven on every change

- An `xtask` command packages `flui-sdk` and every official package with `cargo package`, offline
  against a local registry, and builds each from its tarball against its neighbours' tarballs. It
  catches missing files, path-only features and wrong pins, and runs in `cargo xtask checks` or a
  merge-gating CI job.
- `crates/flui-sdk/tests/package_parity.rs` builds an out-of-tree fixture package against the
  unpacked SDK archive. The fixture has a version-only SDK dependency, opts into its test kit through
  the dev-dependency, and exercises a custom widget, a theme extension and a plugin through the
  public extension points. Cargo metadata must prove that the resolved SDK manifest is the unpacked
  archive, not the checkout.
- After first publication, `cargo-semver-checks` runs on `flui-sdk` in advisory mode. Facade semver
  checks include every feature so aggregated package APIs are covered under ADR-0315 §4.
- No job rewrites the next unpublished train's pins down to the previous release. The published
  combination is checked by `cargo publish` at release.

### 4. `flui-sdk` is the package-author surface

- `flui-sdk` is a tier-K, Evolving, host-free crate. Its normal dependency closure contains no
  `flui-app`, `flui-engine` or wgpu.
- It has its own `0.N` version, bumped on every train and published in the same run, patches
  included. It pins internal crates exactly. `cargo xtask workspace` refuses an Evolving crate that
  inherits the workspace version or has a major above zero.
- **Stable closure:** whole modules are re-exported at facade paths (`pub use flui_x as x`) without
  wrappers or newtypes, so `flui_sdk::m::T` and `flui::m::T` are the same type.
- **Evolving part:** the ordinary surface's Evolving modules are `pipeline` and `gpu`.
  Evolving painting and rendering items go in `pipeline`; there is no second Evolving `paint` module.
  Development hooks required by ADR-0311 may be introduced as Evolving surface when their actual
  producer and consumer land; no empty `hooks` module or scheduler snapshot bridge is kept ahead
  of them. The opt-in `testing` feature mirrors `flui::testing` and keeps its additional
  package-test helpers in Evolving test surface, absent from an ordinary dependency graph.
- The facade neither depends on nor re-exports `flui-sdk`. An experimental facade module, if ever
  needed, is gated by `--cfg flui_unstable`, not a unified Cargo feature.
- **Curated modules.** Where the facade curates `interaction`, `painting` or `rendering`, the SDK
  exposes the same items at the same paths for package extension tasks. An item packages need that
  has no Stable facade path goes into an Evolving module.
- **Surface ceiling.** The Evolving surface is measured from rustdoc JSON. Growth substantially
  beyond the small development-hook and extension surface triggers a review of whether the SDK is
  becoming a second facade. Development hooks needed by ADR-0311 remain outside that ceiling.
- **Graduation.** An Evolving item can move into a Stable facade module after surviving the agreed
  trains unchanged and gaining a second consumer; the SDK keeps its old path as a re-export.
- No documentation invites third-party authors to depend directly on internal crates.

### 5. One train per graph

`flui-foundation`, a bottom crate shared by the train, declares `links = "flui_train"` with a
minimal build script. Cargo refuses two trains in one graph: a mismatched application and package
resolve to one train or fail in the resolver, never later with an internal type mismatch.

The guard is on `flui-foundation`, not `flui-sdk`, because the facade does not depend on the SDK.
It covers the facade, SDK, platform contract and first-party packages through their shared normal
closure. `cargo xtask workspace` requires the key on `flui-foundation` and refuses it on another
member.

A third-party package can lag a train until it is rebuilt and released against the new exact pins.
A facade-aggregated first-party package cannot lag: ADR-0315 §4 requires it to ship with the train.

### 6. The facade aggregates first-party packages by opt-in feature

ADR-0315 owns the complete facade rule. In particular:

- `flui` has `default = []` and keeps `material`, `cupertino`, `devtools` and `hot-reload` as
  standing opt-in features and re-exports.
- Official capability crates use features and paths of the form `flui::services::<name>`.
- `flui::prelude` is feature-independent. Material owns both `flui_material::prelude` and
  `flui::material::prelude`; the compatibility feature `localizations` is removed.
- Every facade-aggregated package ships with the train and participates in facade semver. Before
  adopting independent cadence it leaves the facade under ADR-0315 §4.

This record's earlier removal of facade package features was not selected.

### 7. Repository and cadence changes follow the facade boundary

A package can move to its own repository when its maintenance and coupling justify independent
infrastructure. That move alone does not permit an independent version cadence while the facade
still re-exports it.

Before independent cadence, the package first leaves the facade under ADR-0315 §4 and satisfies the
contract-version conditions of ADR-0153. Material and Cupertino move last because their APIs and
release compatibility are closely coupled to the core catalog.

## Migration

The move names preserve the order constraints; current package-surface behavior is the rule above,
not a pending removal of facade features.

| Move | Required change | Waits on |
|---|---|---|
| 1. SDK and guard | Create `flui-sdk`, the Evolving-version gate and `flui-foundation`'s train guard | — |
| 2. Material | Move Material onto `flui-sdk` and into `packages/` together | move 1 and the package parity check |
| 3. Cupertino | Move Cupertino onto `flui-sdk` and into `packages/` together | move 1 |
| 4. Devtools and hot reload | Move devtools' observation tests into the package. Complete hot reload's SDK move through ADR-0311's Evolving hooks without adding a host edge; the facade keeps its opt-in re-export | move 1; hot reload also waits on ADR-0311 §6 |
| 5a. Facade defaults | Keep `default = []`; every package feature is explicit | moves 2 and 3 for the catalogs |
| 5b. Facade package surface | Keep the standing package features and re-exports under ADR-0315; split Material's prelude from the feature-independent facade prelude and remove `localizations` | move 5a |

## Pros and cons of the options

### 1. Root-workspace packages on the SDK (chosen)

- Good, because package authors use a bounded public surface and first-party development keeps one
  lockfile and train.
- Good, because facade aggregation remains simple for applications without reversing package
  dependencies.
- Bad, because every train releases the SDK and every facade-aggregated first-party package.

### 2. A nested workspace against the published train

- Good, because package development would resemble an external consumer's workspace.
- Bad, because before publication it adds path patches, duplicate core builds, prerelease-pin
  handling and another CI topology without proving more than the tarball fixture.

### 3. Separate repositories from the start

- Good, because package infrastructure can be independent.
- Bad, because tightly coupled first-party changes would need coordinated pins and rollouts across
  repositories; independent cadence would still require leaving the facade first.

### 4. Packages depending on the facade

- Good, because all facade items would already be available.
- Bad, because every package would pull the host, engine and wgpu, and optional facade features
  naming those packages would form a Cargo cycle.

### 5. The package surface inside the Stable facade

- Good, because there would be one crate to depend on.
- Bad, because it would freeze unproven internals and retain the host-bearing dependency closure.

### 6. Packages pinning internal crates directly

- Good, because no SDK abstraction is needed.
- Bad, because every core topology change becomes a breaking change for package manifests and
  imports.

### 7. Removing every facade package feature

- Good, because packages could version independently without first leaving the facade.
- Bad, because applications lose the selected opt-in dependency model and capability namespace;
  ADR-0315 rejects this option.

## Consequences

- Official packages use `flui_sdk::…`, while applications can enable the corresponding facade
  features and use `flui::material`, `flui::cupertino`, `flui::devtools`, `flui::hot_reload` and
  `flui::services::<name>`.
- Material glob users import `flui::material::prelude::*` or `flui_material::prelude::*` beside
  `flui::prelude::*`; enabling Material does not alter the design-neutral facade prelude.
- The train guard also ties platform plugins to one compatible contract train. It does not claim to
  reject a duplicate crate outside `flui-foundation`'s dependency closure.
- Remaining implementation work for accepted sections is tracked in `docs/gaps.md`, not by changing
  this record to Implemented.

### Confirmation

- **One train per graph.** `two_trains_refuse_to_resolve` in `tools/xtask` builds a graph with two
  foundation copies and requires the shared `links` value to make resolution fail; the graph
  without the guard resolves both.
- **Type identity.** `crates/flui-sdk/tests/surface.rs` pins the re-export list and checks that
  representative `flui_sdk::m::T` and `flui::m::T` paths name the same types.
- **Host-free SDK.** `cargo xtask reach` checks the SDK at defaults and with all features against
  tier K's host, engine and GPU forbid set.
- **Package direction.** `cargo xtask workspace` checks official-package dependencies, rejects a
  non-facade core edge to an official package, and implements ADR-0315's standing facade exception.
- **No default package.** `the_facade_turns_no_feature_on_by_default` and the facade consumer graph
  checks require no catalog in an ordinary default graph.
- **Feature combinations.** `cargo xtask facade-combos` builds each supported facade combination in
  isolation.
- **Packaged-SDK external parity.** `flui-sdk::package_parity` resolves an external consumer to the
  unpacked SDK archive and runs its custom theme extension and capability plugin, including the
  opt-in package-test surface. The all-official-package tarball command and rustdoc-JSON surface
  measurement remain required confirmations and are entries of `docs/gaps.md` until built.
