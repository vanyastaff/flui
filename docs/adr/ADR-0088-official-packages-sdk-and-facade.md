# ADR-0088: Official packages live in this workspace, build on `flui-sdk`, and the facade names none of them

- **Status:** Proposed
- **Date:** 2026-09-25
- **Supersedes in part (on acceptance):** [ADR-0028](ADR-0028-design-system-decoupling-contract.md) — the
  placement of Material and Cupertino as core-workspace crates, and the exemption set
  `{flui-localizations, flui-app, flui}` that may depend on them. Its decoupling rules (shared
  substrate below both, mechanism goes down, capability seams instead of platform branches, raw
  primitives, injected selection chrome, no god-widget entry point) stand.
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
enforced by their own manifests: both list `allowed-dependents = ["flui-localizations",
"flui-app", "flui"]` and the same `allowed-dev-dependents`
(`crates/flui-material/Cargo.toml:83-88`, `crates/flui-cupertino/Cargo.toml:82-85`), checked by
`cargo xtask workspace` (`tools/xtask/src/workspace.rs:9-14`, `:146`, `:240-257`). In practice
only the facade depends on them: `flui-material` and `flui-cupertino` are optional facade
dependencies (`Cargo.toml:525-526`) behind `material` and `cupertino` features (`:605-606`), and
`default = ["material"]` (`:598`). `flui-app` and `flui-localizations` are exempted but declare
no such edge.

`flui-material` reaches into nine internal crates in its normal dependencies — widgets, view,
types, objects, rendering, foundation, animation, interaction, scheduler
(`crates/flui-material/Cargo.toml:25-49`) — each with an exact `=0.2.0-dev` pin
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

`cargo xtask workspace` keys its existing `allowed-dependents` mechanism by `tier-kind`; no
parallel rule is added.

- **Forward.** An official package depends only on `flui-sdk`, `flui-platform-api`,
  `flui-protocol` and declared official-to-official edges. Until `flui-sdk` exists the rule runs
  in allowlist mode, seeded with today's edges (Material's nine internal crates, Cupertino's,
  `flui-devtools`', and `flui-hot-reload`'s including `flui-view/runtime-internals`). The
  allowlist can only shrink; each package's entries are removed when it moves onto `flui-sdk`,
  and the rule becomes strict when the last one does.
- **Reverse, strict from the start.** No core crate names an official package in any form:
  normal, optional, build or dev. Named exceptions, each with its reason and exit:

  | Edge | Reason | Exit |
  |---|---|---|
  | `flui-testing` dev → `flui-devtools` | observation-seam test links both halves | the test moves into `flui-devtools` |
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
- **Evolving part:** only the named modules `paint`, `pipeline`, `hooks` and `gpu`. A package's
  exposure to them is `grep flui_sdk::(paint|pipeline|hooks|gpu)`.
- The facade neither depends on nor re-exports `flui-sdk`. An experimental facade module, if one
  is ever needed, is gated by `--cfg flui_unstable`, not by a Cargo feature, which feature
  unification would leak.
- **Surface ceiling.** When `flui-sdk` is created its Evolving surface is measured from rustdoc
  JSON. More than about 30 items beyond the hooks means the sdk is becoming a second facade, and
  this decision is revisited.
- **Graduation (after H3).** An Evolving item moves into a Stable facade module when it has
  survived N trains unchanged and has a second consumer; `flui-sdk` keeps re-exporting it at the
  old path. `gpu` and the development hooks may stay Evolving indefinitely.
- Until `flui-sdk` exists, no documentation invites third-party authors to depend on internal
  crates.

### 5. One train per graph

One bottom crate that everything on the train depends on (candidate: `flui-foundation`) declares
`links = "flui_train"` with a trivial build script. Cargo then refuses to put two trains in one
graph: a mismatched application and package resolve to one train or fail in the resolver, never
with E0308. The same guard protects facade-plus-sdk and facade-plus-Material pairs.

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
- The crates.io availability of the name `flui-sdk` is unchecked; if it is taken, the fallback
  is `flui-package-sdk`.
- Ordering relative to the other records is in
  [the migration plan](../plans/2026-09-25-architecture-migration-plan.md).

## Verification

None of these exist yet.

- `cargo xtask workspace` fails, in its own self-test, on a core crate that names an official
  package (normal, optional and dev), and on an official package that names an internal crate
  outside the allowlist.
- The `cargo package` parity command of §3 passes on every change once `flui-sdk` exists.
- The out-of-tree fixture builds against `flui-sdk` alone.
- A local-registry test: an application and a package on different trains resolve to one train
  or fail in the resolver, never with E0308 (§5).
- A type-identity test: `flui_sdk::m::T` and `flui::m::T` are the same type for each re-exported
  module.
- `cargo xtask reach` (ADR-0081 §2) holds the facts "`wgpu` and `flui-app` are absent from
  `flui-sdk`'s normal closure" and, after the move, "`wgpu` is absent from `flui-material`'s".
  They are reach facts, not `cargo tree -i` probes, because `cargo tree -i` errors on an absent
  package instead of printing nothing.
- A `flui-cli` test generates the counter template and runs `cargo check` on it with
  `flui-material` as a direct dependency.
- A doctest that `use flui::prelude::*; use flui_material::prelude::*;` resolves without
  ambiguity.
