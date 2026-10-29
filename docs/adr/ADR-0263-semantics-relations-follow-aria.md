# ADR-0263: Every ARIA relation with an AccessKit property is a `SemanticsRelation`

- **Status:** Accepted (2026-10-29)
- **Date:** 2026-10-29
- **Supersedes:** [ADR-0205](ADR-0205-semantics-relations-by-anchor.md) §3 only (the rule that a
  relation joins only once a shipped adapter reads it)
- **Related:** [ADR-0206](ADR-0206-typed-semantics-attributes.md) (the typed model relations
  belong to), [ADR-0203](ADR-0203-semantics-vocabulary.md) (the vocabulary follows ARIA 1.2);
  the [semantics-model spec](../plans/specs/semantics-model/design.md), (a).

## Context and problem statement

ADR-0205 §3 admitted a relation only when a shipped adapter read it, which left `LabelledBy`
and `Controls`. The published AccessKit tree has more readers than the three platform adapters:
the in-process agent wire (ADR-0080), `flui-testing`'s query-by-role tree and the coverage
matrix all read the AccessKit node, and AccessKit 0.25 has a property for five more ARIA
relations (`accesskit` 0.25.2, `node_id_vec_property_methods` and `node_id_property_methods`
in `lib.rs`):

| ARIA attribute | AccessKit 0.25 property | Targets |
|---|---|---|
| `aria-describedby` | `described_by` | many |
| `aria-details` | `details` | many |
| `aria-flowto` | `flow_to` | many |
| `aria-errormessage` | `error_message` | one |
| `aria-activedescendant` | `active_descendant` | one |
| `aria-owns` | none: AccessKit expresses ownership only as `children` | — |

An author who cannot declare a relation the platform model has writes it as prose in the
description, and a later adapter that reads the property never sees it. Which relations does
`SemanticsRelation` carry, and how does a single-target property choose among the nodes an anchor
names?

## Decision drivers

- The vocabulary follows ARIA 1.2 wherever AccessKit has a counterpart (ADR-0203), so the model
  is complete before every adapter catches up.
- A relation names nodes by anchor and resolves at publication (ADR-0205 §1, §2); the
  extension keeps that path, its incremental re-translation and its omission of unpublished
  targets.
- A published value must not depend on declaration order or index iteration order, or an
  unchanged relation diffs as changed.

## Considered options

1. Every ARIA relation with an AccessKit property, resolved as the existing two are.
2. Keep ADR-0205 §3: add a relation only when a shipped adapter reads it.
3. Option 1 plus `aria-owns`, published by reparenting the owned nodes under the owner.

## Decision

Chosen option: 1.

1. `SemanticsRelation` (`#[non_exhaustive]`) holds `LabelledBy`, `Controls`, `DescribedBy`,
   `Details`, `FlowTo`, `ErrorMessage` and `ActiveDescendant`. Each resolves through the anchor
   index as ADR-0205 §2 describes, and each publishes to its AccessKit property. Supplementary
   text a widget owns may still be the node's own description; `DescribedBy` is for text another
   node shows.
2. A many-target relation publishes every resolved node, sorted by identity and deduplicated. A
   single-target relation (`ErrorMessage`, `ActiveDescendant`) publishes the first resolved node
   in identity order; the others are not published.
3. `aria-owns` has no `SemanticsRelation`: ownership in AccessKit is the `children` list, which the
   render tree decides. A widget that needs ownership places the owned content in its subtree.

### Consequences

- Good, because an author can state every relation the platform model has, and the agent wire,
  tests and newer adapters read it without a second declaration.
- Good, because the extension adds no resolution path: the incremental cost stays
  O(dirty + anchors + relation sources).
- Bad, because the three shipped adapters still ignore `described_by`, `details`, `flow_to`,
  `error_message` and `active_descendant`, so a screen reader reaches that text only where the
  author also puts it in reading order.
- Neutral, because a single-target relation whose anchor names several nodes publishes one of
  them, chosen deterministically rather than by the author.

### Confirmation

`every_relation_resolves_to_its_accesskit_property` in
`crates/flui-semantics/tests/incremental_flush.rs` (each property, the identity-order choice of a
single target and a dropped unresolved anchor);
`authored_properties_and_relations_reach_the_published_node` in
`crates/flui-widgets/tests/semantics.rs` through the `Semantics` widget.

## Pros and cons of the options

### 1. Every ARIA relation with an AccessKit property

- Good, because the model matches ARIA and AccessKit, and the publication path is unchanged.
- Bad, because some relations reach no shipped screen reader yet.

### 2. Only what a shipped adapter reads

- Good, because nothing is published that no screen reader speaks.
- Bad, because the agent wire and tests, which read the AccessKit tree, cannot see relations the
  model refuses, and authors fall back to prose.

### 3. Also `aria-owns`, by reparenting

- Good, because ARIA's full relation set would be expressible.
- Bad, because semantics would then contradict the render tree's structure, which hit testing,
  bounds and traversal order follow; no AccessKit property records the ownership itself.

## More information

- AccessKit 0.25.2, `src/lib.rs`: `node_id_vec_property_methods!` (`DescribedBy`, `Details`,
  `FlowTo`, `Controls`, `LabelledBy`) and `node_id_property_methods!` (`ActiveDescendant`,
  `ErrorMessage`).
- WAI-ARIA 1.2, §6.6.4 "Relationship Attributes": https://www.w3.org/TR/wai-aria-1.2/#attrs_relationships
- Revisit when an AccessKit release adds an ownership property.
