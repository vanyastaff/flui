# ADR-0041: Workspace topology contract

*The workspace layer graph is a machine-readable policy (`docs/workspace-layers.toml`) validated against Cargo's **normal** dependency edges, not a diagram in a document. It fixes `flui-objects`' and `flui-localizations`' documented placement, forbids the design-system → localizations direction that would cycle, classifies every active member for the Runtime.1 milestone, and gates the creation of a `flui-runtime` crate on two proven consumers.*

---

- **Status:** Accepted (2026-08-01)
- **Date:** 2026-08-01
- **Deciders:** @vanyastaff
- **Scope:** workspace dependency topology — `docs/workspace-layers.toml`, `scripts/check-workspace-inventory.sh`, `docs/FOUNDATIONS.md` Part IV, `docs/crates.md`, `docs/architecture.md`
- **Related:** [ADR-0028](ADR-0028-design-system-decoupling-contract.md) (design-system decoupling — the first dependency-graph contract, generalized here); [ADR-0037](ADR-0037-presentation-ownership-domains.md) (the `interaction -> platform` same-layer edge); [ADR-0027](ADR-0027-owner-affine-ui-realms.md) (package/dependency topology is a sanctioned leapfrog zone); [Workspace Boundary and Logging Review](../research/2026-08-01-workspace-boundary-and-logging-review.md); [Runtime Architecture Execution Plan](../research/2026-08-01-runtime-architecture-execution-plan.md)
- **Issue:** [#567](https://github.com/vanyastaff/flui/issues/567) — the first of three Runtime.1 pre-sprint structural tasks, ahead of [#568](https://github.com/vanyastaff/flui/issues/568) (diagnostics composition boundary) and [#569](https://github.com/vanyastaff/flui/issues/569) (public package surface cleanup)

---

## Context

`scripts/check-workspace-inventory.sh` already proved that workspace members, package metadata, and the human-facing inventories agree, and ADR-0028 added one dependency-graph rule (core never depends on a design system). Neither checks dependency *direction* in general. The gap is not hypothetical — it produced three live documentation defects that a green gate never caught:

1. **`flui-objects` is drawn in the wrong place.** `docs/FOUNDATIONS.md` Part IV places it in the same tier as `flui-widgets`, *above* `flui-view`, with `widgets --> objects` as the only inbound edge. The real production graph is `flui-rendering <- flui-objects <- flui-view <- flui-widgets`: `flui-view` names `flui_objects::RenderLayoutBuilder`, `RenderSliverList`, `RenderSliverGridLazy`, `LayoutConstraintsCell`, and `RenderSizedBox` in production for framework machinery whose element and render halves cooperate (`owner/layout_builder.rs`, `element/layout_builder.rs`, `element/sliver_adaptor.rs`, `element/child_manager.rs`). That is not a cycle and not a violation — the diagram is stale. `crates/flui-rendering/Cargo.toml` even carries a comment warning that the inverse edge would invert the layer order, which is the correct instinct pointed at a table that says otherwise.

2. **The documented localization direction is backwards.** Part IV draws `material --> l10n` and `cupertino --> l10n`. `flui-localizations` is the *implementation* package: it implements `flui-widgets`' `WidgetsLocalizations` today (the `l10n --> widgets` amendment of 2026-07-16 already recorded this), and it will implement `GlobalMaterialLocalizations` and `GlobalCupertinoLocalizations` next — so it must depend on Material and Cupertino, not the reverse. Following the documented direction would make the implementation package and its interface owners mutually dependent the moment global translations land. Nothing enforces the correct direction today; the crate simply has no design-system edge yet.

3. **`flui-devtools` and `flui-cli` are drawn below what they consume.** Part IV places `flui-devtools` beside `flui-view` with `devtools --> engine`; the real edges are `devtools --> foundation` and `devtools --> hot-reload`, which sits two documented tiers higher. `flui-cli` is drawn beside the widget catalog while it consumes `flui-devtools`.

The Runtime.1 execution plan freezes public runtime surfaces immediately after this preflight. A misplaced boundary corrected after the freeze costs a migration; corrected before it costs a documentation edit. Separately, the plan is explicit that a `flui-runtime` crate must **not** be guessed into existence before managed and embedded entry points prove its boundary — a rule with no mechanism is a rule that gets forgotten by the third contributor.

## Decision

**The layer graph is a policy file, checked against Cargo.**

`docs/workspace-layers.toml` declares, for every active `crates/*` member and the root `flui` facade: its layer rank, its Runtime.1 disposition, and a note explaining the placement. It additionally declares sanctioned same-layer edges, forbidden edges, projected future edges, and gated crates. `scripts/check-workspace-inventory.sh` (`just inventory-check`, part of `just ci` and the CI `checks` job) enforces six rules against `cargo metadata`:

1. Every governed member appears exactly once in the policy, with a valid layer and disposition.
2. Every in-workspace **normal** edge points at a strictly lower layer, or is listed as a `[[same_layer_edge]]`.
3. `[[forbidden_edge]]` pairs fail with the contract text in the error message, even where the layer rule alone would permit them.
4. The normal-edge graph **plus** every `[[projected_edge]]` stays acyclic.
5. Nothing under `crates/` depends on an example or tool member.
6. A `[[planned]]` crate with `status = "gated"` may not exist as a workspace member.

**Normal edges only.** The issue's own framing — "generate or maintain the authoritative normal-dependency graph separately from dev dependencies" — is the right call: a dev-dependency is a testing convenience, not an architectural claim, and Cargo tolerates dev-dependency cycles that a normal edge could never form. ADR-0028's design-system rule deliberately stays broader (every dependency kind), because an accidental Material dev-dependency in a core crate *is* the coupling smell that rule exists to catch.

**Same-layer edges are ordered pairs.** Allowing `flui-objects -> flui-rendering` never allows `flui-rendering -> flui-objects`. This is what lets the corrected `flui-objects` placement land without renumbering L5–L9: the render catalog joins `flui-engine`/`flui-rendering` in L4 with an explicit intra-tier edge, which keeps it strictly below `flui-view` (L5) while still failing the inversion that would actually break the layering. The mechanism is not new — `flui-types -> flui-geometry` (L0) and `flui-interaction -> flui-platform` (L2, ADR-0037) are the same shape, previously expressed only in prose.

**The corrected graph.**

| Layer | Crates |
|---|---|
| L0 — Foundation (value types) | `flui-geometry`, `flui-types` |
| L1 — Framework primitives | `flui-foundation`, `flui-macros` |
| L2 — Substrate | `flui-tree`, `flui-platform`, `flui-scheduler`, `flui-painting`, `flui-interaction`, `flui-assets` |
| L3 — Compositing / a11y / animation | `flui-semantics`, `flui-layer`, `flui-animation` |
| L4 — Render machine + render catalog | `flui-engine`, `flui-rendering`, `flui-objects` |
| L5 — Framework spine | `flui-view` |
| L6 — Widget catalog + DX tooling | `flui-widgets`, `flui-testing`, `flui-hot-reload`, `flui-build` |
| L7 — Design systems | `flui-material`, `flui-cupertino` |
| L8 — Global localizations | `flui-localizations` |
| L9 — Application / tooling | `flui-app`, `flui-devtools`, `flui-cli` |
| L10 — Facade | `flui` |

Three placements move relative to the previous Part IV table: `flui-objects` from the catalog tier down into L4; `flui-localizations` from the catalog tier up to L8, above the design systems it will implement; `flui-devtools` and `flui-cli` up to L9, above `flui-hot-reload`. Every other crate keeps its number, so ADR-0028's "design systems as L7, above the L6 widget catalog" and ADR-0009's "`flui-widgets` (L6)" remain literally correct.

**Localization direction is locked.** `flui-material -> flui-localizations` and `flui-cupertino -> flui-localizations` are `[[forbidden_edge]]` entries; `flui-localizations -> flui-material` and `-> flui-cupertino` are `[[projected_edge]]` entries. The acyclicity check therefore validates the *future* localization graph today, before the edges that would close a cycle exist. The facade may re-export the localizations package behind an optional feature without touching these internal directions.

**No `flui-runtime` without two consumers.** The policy records it as `status = "gated"`. Extraction requires all three of: (1) a managed entry point and a host-driven/embedded entry point that both drive the same proven core; (2) a measurable dependency reduction — a consumer that today needs all of `flui-app` would need only `flui-runtime`; (3) that boundary exercised by those two consumers, not inferred from a diagram. Until then `flui-app` is the private composition root. Creating `crates/flui-runtime` fails the gate with the gate text printed.

**Every active member is classified** as `keep`, `rename`, `narrow`, `optionalize`, or `deferred-extraction`, with the owning issue where the disposition implies work. The classification lives in the policy file, next to the layer rank, so it is reviewed whenever the topology is.

## Consequences

- **Positive.** A wrong dependency direction now fails `just inventory-check` naming both crates and both layers, instead of surviving as a stale diagram for months. The future localization cycle is refuted mechanically rather than argued in prose. Adding a crate to the workspace forces an explicit classification, which is what makes the `flui-runtime` gate enforceable rather than advisory.
- **Negative / accepted cost.** The policy file is a second place to edit when the graph legitimately changes — a `Cargo.toml` edit plus a `docs/workspace-layers.toml` edit. That friction is the point for a topology contract, and it is the same trade ADR-0028 already accepted for its hardcoded exemption set. Same-layer exemptions are also a genuine loosening: `flui-objects` and `flui-rendering` sharing L4 means the layer *number* no longer expresses their relative order — only the directional exemption does.
- **Neutral.** No crate's actual dependencies changed. The workspace already had the correct shape; three documents described it wrongly. This ADR is a contract for the next change, and a correction of the record for the last one.

## What is untouched

Prime Directive #1 is not amended. The three-tree model, lifecycle, layout/paint/hit-test protocol, and reconciliation stay ported 1:1 from `.flutter/`. This ADR moves nothing in `src/`; it constrains `Cargo.toml`. Package topology is the same sanctioned leapfrog category ADR-0027 opened and ADR-0028 first used — Flutter is the behavioral reference for widget-tree semantics, not for how a Rust workspace is partitioned.

The dispositions recorded here are classifications, not permission to act: the test driver's rename to `flui-testing`, `flui-hot-reload`'s and the facade's feature gating, and `flui-foundation`'s logging narrowing were all owned by [#568](https://github.com/vanyastaff/flui/issues/568) and [#569](https://github.com/vanyastaff/flui/issues/569), not by this change — they landed under those issues afterwards.

## Alternatives rejected

- **Renumber the layers so no same-layer edge is needed.** Giving `flui-objects` its own tier between the render machine and `flui-view` shifts every layer above it by one, which would falsify ADR-0028's and ADR-0009's layer citations and every "L6" in `docs/ROADMAP-TRACKER.md`. Rewriting dated decision records to keep a numbering scheme consistent is the wrong direction of fix. A directional same-layer exemption catches the exact inversion the numbering would have caught, at no cost to enforcement strength.
- **Generate the layer assignment from Cargo instead of declaring it.** A topological sort of the actual graph can never disagree with the actual graph, so it would have caught none of the three defects above. The policy has to be an independent *claim* about intended architecture for the comparison to mean anything.
- **Put the checks in `scripts/port-check.sh`.** Same reasoning as ADR-0028: port-check's triggers grep `.rs` sources for usage patterns. This is a declared-dependency-graph fact, and `check-workspace-inventory.sh` is the one script already parsing `cargo metadata`'s dependency lists.
- **Use `cargo-deny`'s `[bans]` list.** Rejected for the same reason ADR-0028 rejected it, now more strongly: `deny.toml` expresses "crate A must not depend on crate B", not a layered partial order with directional exemptions and projected future edges. Encoding eleven layers as pairwise bans would be unreadable and would still not express the acyclicity projection.
- **Check dev-dependencies against the layer rule too.** Rejected: dev-dependency cycles are legal in Cargo, and this workspace's test wiring already crosses layers deliberately — `flui-testing` (L6) dev-depends on `flui-devtools` (L9), `flui-macros` dev-depends on `flui-foundation` inside L1. Treating a test fixture as an architectural claim would produce noise that trains reviewers to add exemptions, which is how an enforced contract decays into a rubber stamp.
