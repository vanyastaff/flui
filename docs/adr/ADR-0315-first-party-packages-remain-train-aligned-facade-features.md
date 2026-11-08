# ADR-0315: First-party packages remain train-aligned facade features

- **Status:** Accepted (2026-11-07)
- **Date:** 2026-11-07
- **Supersedes:** [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md) §6 and move 5b only
  (the removal of facade package features and re-exports);
  [ADR-0154](ADR-0154-capability-crates.md) §8 only (the facade aggregation mechanism, not the
  declaration or registration behavior owned by ADR-0307);
  [ADR-0311](ADR-0311-development-reload-is-a-hook-the-application-installs.md) §6 only (the
  prohibition on the facade naming `flui-hot-reload` and the removal of its `hot-reload` feature);
  [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md) §1 and §3 only (the temporary facade
  edge exceptions and their exits; the other tier and kind rules stand)
- **Related:** [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md) (the Stable API closure
  includes facade re-exports); [ADR-0153](ADR-0153-release-train-and-capability-crates.md) (the
  capability release train); [ADR-0307](ADR-0307-platform-declarations-are-crate-metadata.md)
  (declaration and `flui add` behavior, unchanged); [ADR-0311](ADR-0311-development-reload-is-a-hook-the-application-installs.md)
  (the host/reload boundary, unchanged apart from the clause named above)

## Context and problem statement

The workspace already exposes Material, Cupertino, devtools and hot reload as opt-in facade
features, and exposes capability crates as `flui::services::<name>`. ADR-0088 §6 instead required
all of those edges to disappear, while ADR-0154 §8 adopted the same aggregation mechanism for
capabilities and ADR-0311 §6 repeated the prohibition for hot reload. The facade's manifest
exceptions were therefore recorded as temporary debt even though aggregation is the application
surface the project chooses to keep.

The choice affects more than dependency direction. A Stable facade re-export carries the
re-exported API into the facade's semver closure, and exact train pins already require first-party
packages to resolve with the matching framework train. How can the facade provide one opt-in
application surface without pretending that an aggregated package has an independent release
cadence?

## Decision drivers

- Application authors get one dependency and explicit, additive feature selection; `flui` enables
  no design system or development package by default.
- The facade is the composition-facing aggregation point, not a dependency channel back into the
  core. Core crates must not come to depend on official packages through it.
- Optional facade APIs participate in the facade's public API and semver checks with every feature
  enabled, whether or not an ordinary application enables them.
- Exact train pins and `flui-foundation`'s `links = "flui_train"` guard require compatible package
  and framework versions in one graph.
- Material names in the design-neutral `flui::prelude` make its meaning depend on a feature.
- ADR-0307 owns declaration metadata and `flui add`; ADR-0311 owns the reload hook and the rule
  that the host names no reload tool. Facade aggregation changes neither.

## Considered options

1. Keep first-party facade features permanently, with train-aligned packages and a standing facade
   aggregation exception.
2. Remove every facade package feature and require direct package dependencies.
3. Keep facade re-exports but let each package choose an independent release cadence.

## Decision

Chosen option: 1, because it preserves the opt-in application surface while making the semver and
release cost explicit. Option 2 is rejected; full removal of facade package features is not the
selected direction.

### 1. Facade aggregation is permanent and opt-in

`flui` keeps `default = []`. The `material`, `cupertino`, `devtools` and `hot-reload` features are
standing first-party package features, not migration compatibility names. Each enables its optional
package dependency and the corresponding public re-export: `flui::material`, `flui::cupertino`,
`flui::devtools` and `flui::hot_reload`.

Each official capability crate likewise has an off-by-default facade feature and is re-exported as
`flui::services::<name>`. For example, `system` exposes `flui::services::system`. The crates remain
separate, so an application links only the packages and capabilities it selects. ADR-0307's `flui
add` behavior and declaration metadata remain the mechanism that enables and registers a
capability.

### 2. The facade is the standing aggregation exception

The tier and kind gates admit the facade's optional, feature-gated edges to official and capability
packages as a standing exception associated with this record. Those edges have no removal exit.
Every other core crate, including `flui-app`, `flui-runtime` and `flui-native`, names no official
package in any dependency kind. The existing exemptions for applications and tools, and the forward
rules that official packages build on `flui-sdk` and the contract crates, are unchanged.

This exception permits aggregation only. It does not let a core crate consume package
functionality through the facade, and it does not weaken ADR-0311's host boundary: `flui-app`
continues to reach reload only through an installed hook.

### 3. Preludes follow package ownership, not feature selection

`flui::prelude` is feature-independent and design-system-neutral. Enabling `material` adds names
only under Material-owned paths; it never changes what an existing `flui::prelude` name resolves
to.

`flui-material` owns `flui_material::prelude`. The facade's `flui::material::prelude` is the same
Material-owned module through the package re-export, not a second curated list in `flui::prelude`.
An application that wants Material's glob imports it explicitly:

```rust
use flui::material::prelude::*;
use flui::prelude::*;
```

The empty compatibility feature `localizations` is removed rather than retained as a deprecated
alias. Global widgets localizations remain in `flui::widgets` and need no facade feature.

### 4. Aggregation ties release cadence and semver to the train

While the facade re-exports a first-party package, that package is released with the framework
train, including patch releases, and its public API participates in the facade's semver closure.
After the Stable freeze, a breaking change in such a re-exported API is also a breaking facade
change; optional features do not create an exemption.

A package may adopt an independent cadence only after it leaves the facade. Leaving means removing
the facade's optional dependency, feature and public re-export, including any facade path to its
prelude, in a change whose semver impact on `flui` is treated accordingly. Direct use of the package
can then version independently under the rules of ADR-0153; keeping the re-export while claiming
independent cadence is not allowed.

### Consequences

- Good, because applications keep one framework dependency and explicit opt-in package features,
  while the core's dependency direction remains strict.
- Good, because release tooling and reviewers have one rule: facade aggregation means train
  alignment and facade semver participation.
- Good, because `flui::prelude` is additive and stable across feature combinations, and Material's
  glob has one owner.
- Bad, because a train release includes every facade-aggregated package, and a breaking package API
  can require a facade major after the Stable freeze.
- Bad, because moving a package to independent cadence first requires a facade-visible removal.
- Neutral, because direct package dependencies remain possible where ADR-0307 already supports
  them, but they do not create a second aggregation namespace.

### Confirmation

- `cargo xtask workspace` accepts the facade's optional aggregation edges as the standing exception
  of §2 and rejects an official-package edge from any other core crate, including dev and optional
  edges.
- `cargo xtask facade-combos` builds every supported feature combination in isolation. The existing
  `the_facade_turns_no_feature_on_by_default` check holds `default = []`.
- Facade consumer and surface tests pin the four package re-exports and
  `flui::services::<name>` behind their features. A compile check pins `flui::prelude` to the same
  exports with and without `material`, and a doctest imports `flui::prelude::*` beside
  `flui::material::prelude::*`.
- `cargo xtask api-closure` runs on the facade with every feature, so aggregated package APIs remain
  inside the Stable closure of ADR-0089. Release checks run semver comparison with every facade
  feature enabled.
- The release packaging check of ADR-0088 §3 packages the facade and every aggregated first-party
  package at one train version. An independent package version is accepted by the workspace gate
  only after no facade feature or re-export names that package.
- Accepted work not yet built is listed in `docs/gaps.md`; this record does not mark itself
  Implemented.

## Pros and cons of the options

### 1. Permanent train-aligned facade aggregation

- Good, because it preserves the simple application dependency model and the existing capability
  namespace without weakening core dependency direction.
- Good, because semver and release cadence match the actual public API graph.
- Bad, because first-party packages cannot release independently while aggregated, and the facade
  carries their API stability cost.

### 2. Remove facade package features

- Good, because each package could version independently without first changing the facade.
- Bad, because applications need separate dependencies and imports, ADR-0154's capability namespace
  loses its mechanism, and the already built opt-in surface is removed without a required
  dependency-direction benefit.
- Bad, because it mistakes a manageable semver commitment for a Cargo cycle; packages still build
  on `flui-sdk`, not on the facade.

### 3. Facade re-exports with independent package cadence

- Good, because package maintainers could release on separate schedules.
- Bad, because exact train pins do not admit that version matrix, and a Stable facade cannot
  delegate the compatibility of its own re-exported API to an independent package cadence.
- Bad, because consumers could not tell whether `flui::material` or `flui::services::<name>` belongs
  to the facade train they selected.

## More information

Revisit §4 only for a package whose contract can sustain independent versioning under ADR-0153.
The first step is still removal from the facade, not a second version policy for an aggregated
package. ADR-0307's declaration behavior and ADR-0311's hook/host contract are outside this
record's supersession.
