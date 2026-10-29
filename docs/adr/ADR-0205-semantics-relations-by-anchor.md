# ADR-0205: Semantics relations name their targets by anchor, resolved at publication

- **Status:** Accepted
- **Date:** 2026-10-12
- **Superseded-by:** [ADR-0263](ADR-0263-semantics-relations-follow-aria.md) for §3 only (every ARIA
  relation with an AccessKit property is a `SemanticsRelation`)
- **Related:** [ADR-0080](ADR-0080-agent-protocol-desktop-contract.md), the wire projection of the
  published AccessKit tree; `crates/flui-semantics/ARCHITECTURE.md`, "Failed incremental
  publication keeps delivery pending".

## Context

Some accessibility facts relate two nodes: a dialog is named by its title (`aria-labelledby`), a
button opens the panel it controls (`aria-controls`). AccessKit expresses these as node ids. A
FLUI widget cannot name an id: a semantics node's `AccessibilityNodeId` is the generational
identity of the render boundary that forms it, which does not exist when `build` runs, changes
when the target remounts, and may land on a merged ancestor node.

The publishing owner diffs dirty nodes against the last delivered tree, so a relation resolved
once would go stale when its target's identity changes and the source itself did not.

The shipped adapters (`accesskit_windows` 0.35, `accesskit_macos` 0.27, `accesskit_atspi_common`
0.21) consume `labelled_by`, through `accesskit_consumer`'s name computation, and `controls`.
None reads `described_by`, `error_message` or `active_descendant`.

## Decision

1. **A relation target is a `SemanticsAnchor`**, an opaque token whose identity is its
   allocation (`Arc` pointer equality), never a label or counter. A configuration declares the
   anchors it carries (`add_anchor`) and the relations it has (`add_relation(kind, anchor)`).
   Both merge as unions when a configuration is absorbed, so an anchor on a non-boundary
   `Semantics` names the node it merges into. A source holds its anchors, so an anchor's
   allocation cannot be reused while anything refers to it.
2. **Relations resolve at publication**, against the tree being published. An anchor resolves to
   every addressable node declaring it; a relation to an anchor no node declares is omitted.
   The full path resolves from the whole tree. The incremental path keeps, per owner, the
   anchor-to-node index and the set of relation sources, both updated from dirty nodes and
   pruned by liveness, and re-translates every relation source on each incremental publish; the
   mirror diff drops the unchanged ones. The cost is O(dirty + anchors + relation sources).
3. **`SemanticsRelation` is `#[non_exhaustive]` and holds what an adapter consumes:**
   `LabelledBy` and `Controls`. A relation joins when a shipped adapter reads it. Supplementary
   text a widget owns, such as a field's error or helper line, is the node's description, which
   every adapter reads.

## Consequences

- A widget that names another node creates the anchor once (in its state, or for both ends in
  one `build`) and hands it to both `Semantics` wrappers. An anchor recreated on each build
  makes both nodes differ from the published mirror, which costs a republish but stays correct.
- A relation reaches the platform only while its target is published; hiding the target drops
  the relation rather than pointing at a node the adapter does not have.
- Snapshots and the agent wire carry relations as the published AccessKit nodes carry them.

## Verification

`relations_resolve_through_anchors_across_publication` in
`crates/flui-semantics/tests/incremental_flush.rs` covers the full path, an unresolved anchor, a
target remounted under a clean source, and a withdrawn anchor;
`a_shown_dialog_hides_the_page_beneath_from_assistive_technology` in
`packages/flui-material/tests/show_dialog.rs` checks an `AlertDialog` named by its title.
