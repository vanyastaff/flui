# ADR-0041: Workspace topology contract

- **Status:** Accepted
- **Date:** 2026-08-01
- **Amended:** 2026-09-23 — the layer graph moved from `docs/workspace-layers.toml` into the
  manifests (`[package.metadata.flui] layer`), checked by `cargo xtask workspace`. Layer order
  and Cargo's own cycle check replace the same-layer, forbidden and projected edge lists;
  `allowed-dependents` / `allowed-dev-dependents` carry the per-crate restrictions (`flui-log`,
  the design systems); review holds the planned-crate gate.
- **Superseded in part by:** [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md) — tiers
  are checked beside layers; the layer table, "a crate is a layer" as the only reason, and
  "Localization direction is locked" end when the `layer` key is removed.
- **Superseded in part by:** [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md) — the
  paragraph "No `flui-runtime` without two consumers"; `flui-runtime` exists.
- **Amended by:** [ADR-0082](ADR-0082-platform-api-contract-crate.md) (L1 gains
  `flui-platform-api`; the `interaction -> platform` edge becomes `interaction -> platform-api`,
  and `flui-platform` allows only `flui-app` as a dependent)
- **Amended by:** [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md) (2026-09-26) —
  `flui-tree` (L2) and `flui-localizations` are deleted, so L8 is empty and "Localization direction is locked" has
  nothing left to govern. The layer table below is a dated reading and stays as written.
- **Related:** ADR-0028 (the design-system rule this generalizes), ADR-0037 (the
  `interaction -> platform` same-layer edge)

## Context

The workspace had one enforced dependency-graph rule (ADR-0028: core never depends on a design
system) and a layer diagram nobody checked. The diagram had drifted: `flui-objects` was drawn
above `flui-view` although `flui-view` uses render objects from it in production;
`flui-localizations` was drawn below the design systems although it is the package that will
implement their global localizations, which would have made the two mutually dependent; and
`flui-devtools`/`flui-cli` were drawn below what they consume. None of that failed a gate.

## Decision

**A crate is a layer, and dependencies point one way.** Each crate exists because it is a
distinct layer in a one-way dependency graph, not because it groups a feature. A feature inside
a layer is a module of that layer's crate. In particular **`flui-widgets` stays one crate**:
navigation, overlays, focus, text editing and the rest of the widget catalog are modules of
L6, not `flui-navigation`/`flui-overlay` crates. A new crate needs a new layer (or a proven
second consumer of an existing boundary), not a new feature.

**The layer graph lives in the manifests, checked against Cargo.** Every active `crates/*`
member and the `flui` facade declares `[package.metadata.flui] layer = N`; the root manifest names
the layers in `[workspace.metadata.flui] layers`, and a crate may narrow who depends on it with
`allowed-dependents` (normal and build edges) and `allowed-dev-dependents` (`flui-log`, which
only composition roots link; the design systems, per ADR-0028). `cargo xtask workspace` enforces
against `cargo metadata`:

1. Every crate under `crates/` and the facade declares a valid layer.
2. Every in-workspace **normal** or build edge points to the same layer or lower.
3. A crate with `allowed-dependents` has no normal or build dependent outside that list, and one
   with `allowed-dev-dependents` no dev dependent outside it. Examples and tools are applications
   and are exempt.
4. Nothing layered depends on an example or tool member.

**Normal and build edges carry the layer rule.** A dev-dependency is a testing convenience, not
an architectural claim, and may point up (four do, all onto test support), unless the target
restricts them with `allowed-dev-dependents`, as the design systems do (ADR-0028).

**Same-layer edges are allowed; cycles are not.** With `flui-objects -> flui-rendering` in
place, the reverse closes a cycle Cargo rejects. That lets the render catalog share L4 with the
render machine while staying strictly below `flui-view`.

| Layer | Crates |
|---|---|
| L0 — Value types | `flui-geometry`, `flui-types` |
| L1 — Framework primitives | `flui-foundation`, `flui-macros` |
| L2 — Substrate | `flui-tree`, `flui-platform`, `flui-scheduler`, `flui-painting`, `flui-interaction`, `flui-assets`, `flui-log` |
| L3 — Compositing / a11y / animation | `flui-semantics`, `flui-layer`, `flui-animation` |
| L4 — Render machine + render catalog | `flui-engine`, `flui-rendering`, `flui-objects` |
| L5 — Framework spine | `flui-view` |
| L6 — Widget catalog + DX tooling | `flui-widgets`, `flui-testing`, `flui-hot-reload` |
| L7 — Design systems | `flui-material`, `flui-cupertino` |
| L8 — Global localizations | `flui-localizations` |
| L9 — Application / tooling | `flui-app`, `flui-devtools`, `flui-cli` |
| L10 — Facade | `flui` |

The manifests are the source of truth; this table is their reading at the time of writing.

**Localization direction is locked.** `flui-localizations` (L8) sits above the design systems
(L7), so `flui-material`/`flui-cupertino -> flui-localizations` would point up and fail; the
reverse edges point down when they land.

**No `flui-runtime` without two consumers.** (Superseded by ADR-0083.) It is recorded as gated. Extraction needs a
managed entry point and an embedded/host-driven one both driving the same core, a measurable
dependency reduction for a consumer, and that boundary exercised by both. Until then
`flui-app` is the private composition root. Review holds this gate; no tool refuses the
directory.

## Flutter divergence

Package topology is a leapfrog zone (ADR-0027). Flutter is the behavioral reference for
widget-tree semantics, not for how a Rust workspace is partitioned.

## Consequences

- A wrong dependency direction fails `cargo xtask workspace` naming both crates and layers.
- A new crate declares its layer in its own manifest; there is no second registry to edit.
- Same-layer edges are a real loosening: `flui-objects` and `flui-rendering` sharing L4
  means only the existing edge and Cargo's cycle rule, not the layer number, express their order.
- Splitting a crate by feature is off the table; a large crate is organized by modules.

## Alternatives rejected

- **Renumber the layers so no same-layer edge is needed.** Shifts every layer above and
  falsifies existing layer citations; Cargo's cycle rule catches the same inversion.
- **Generate the layer assignment from Cargo.** It could never disagree with the real graph,
  so it would catch nothing; the policy must be an independent claim.
- **`cargo-deny` bans.** Pairwise bans cannot express a layered partial order.
- **Check dev-dependencies too.** Legal dev cycles and deliberate cross-layer test wiring would
  produce noise that trains reviewers to add exemptions.
- **Split `flui-widgets` into feature crates** (navigation, overlay, focus). Features inside
  the widget layer depend on each other freely; splitting them manufactures cross-crate edges
  and public surface with no layer to justify them.
