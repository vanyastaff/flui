# ADR-0041: Workspace topology contract

- **Status:** Accepted
- **Date:** 2026-08-01
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

**The layer graph is a policy file, checked against Cargo.** `docs/workspace-layers.toml`
declares, for every active `crates/*` member and the `flui` facade, its layer and a note
explaining the placement, plus sanctioned same-layer edges, forbidden edges, projected future
edges and gated crates. `check-workspace-inventory.sh` enforces against `cargo metadata`:

1. Every governed member appears exactly once, with a valid layer.
2. Every in-workspace **normal** edge points at a strictly lower layer, or is a listed
   same-layer edge.
3. Forbidden edges fail with the contract text, even where the layer rule would allow them.
4. The normal-edge graph plus every projected edge stays acyclic.
5. Nothing under `crates/` depends on an example or tool member.
6. A gated planned crate may not exist as a workspace member.

**Normal edges only.** A dev-dependency is a testing convenience, not an architectural claim,
and Cargo tolerates dev-dependency cycles a normal edge never could. ADR-0028's design-system
rule deliberately stays broader (every dependency kind).

**Same-layer edges are ordered pairs.** Allowing `flui-objects -> flui-rendering` never allows
the reverse. That lets the render catalog share L4 with the render machine while staying
strictly below `flui-view`.

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

`docs/workspace-layers.toml` is the source of truth; this table is its reading at the time of
writing.

**Localization direction is locked.** `flui-material`/`flui-cupertino -> flui-localizations`
are forbidden edges; the reverse are projected edges, so acyclicity validates the future graph
before the edges exist.

**No `flui-runtime` without two consumers.** It is recorded as gated. Extraction needs a
managed entry point and an embedded/host-driven one both driving the same core, a measurable
dependency reduction for a consumer, and that boundary exercised by both. Until then
`flui-app` is the private composition root.

## Flutter divergence

Package topology is a leapfrog zone (ADR-0027). Flutter is the behavioral reference for
widget-tree semantics, not for how a Rust workspace is partitioned.

## Consequences

- A wrong dependency direction fails the inventory check naming both crates and layers.
- The policy file is a second place to edit when the graph legitimately changes — the point for
  a topology contract.
- Same-layer exemptions are a real loosening: `flui-objects` and `flui-rendering` sharing L4
  means only the directional exemption, not the layer number, expresses their order.
- Splitting a crate by feature is off the table; a large crate is organized by modules.

## Alternatives rejected

- **Renumber the layers so no same-layer edge is needed.** Shifts every layer above and
  falsifies existing layer citations; a directional exemption catches the same inversion.
- **Generate the layer assignment from Cargo.** It could never disagree with the real graph,
  so it would catch nothing; the policy must be an independent claim.
- **`cargo-deny` bans.** Pairwise bans cannot express a layered partial order with directional
  exemptions and projected edges.
- **Check dev-dependencies too.** Legal dev cycles and deliberate cross-layer test wiring would
  produce noise that trains reviewers to add exemptions.
- **Split `flui-widgets` into feature crates** (navigation, overlay, focus). Features inside
  the widget layer depend on each other freely; splitting them manufactures cross-crate edges
  and public surface with no layer to justify them.
