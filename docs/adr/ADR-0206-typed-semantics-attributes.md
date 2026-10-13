# ADR-0206: Semantics are one typed attribute model with one AccessKit translation

- **Status:** Accepted (2026-10-12); implemented by the semantics-model works W2–W3
- **Date:** 2026-10-12
- **Replaces:** `crates/flui-semantics/ARCHITECTURE.md` mapping decisions 1 and 2 (the role
  cascade over flags), with the role precedence in decision 4 below.
- **Supersedes:** [ADR-0168](ADR-0168-descendant-reveal-through-scroll-ancestors.md), in part:
  its statement that `SemanticsSnapshot` data is the cross-thread handoff; and
  [ADR-0157](ADR-0157-native-scroll-range-actions.md), in part: the axis carried through the
  snapshot and node data.
- **Related:** [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md) (no upstream type
  in a stable signature), [ADR-0205](ADR-0205-semantics-relations-by-anchor.md),
  [ADR-0203](ADR-0203-semantics-vocabulary.md), [ADR-0204](ADR-0204-synthetic-semantics-nodes.md);
  the [semantics-model spec](../plans/specs/semantics-model/design.md).

## Context

One node's semantics exist in five shapes: `SemanticsProperties` (a public field per property),
`SemanticsConfiguration`, `SemanticsNodeData` (a copy with public fields),
`SemanticsNodeSnapshot` (another copy, with no consumer outside tests) and the AccessKit node.
A new property is threaded through all five, and nothing fails when one is missed. Role is
encoded twice, as role-bearing flags (`IsButton`, `IsTextField`, `HasCheckedState`, …) and as
`SemanticsRole`, and a cascade picks one. States are bits of a `u64` beside `Option<bool>`
fields, so invalid combinations are representable: `IsChecked` without `HasCheckedState`, an
assertive bit without a live region. Merging treats any overlapping bit as a conflict, with
per-field rules spread over one function.

AccessKit is FLUI's only output (spec D2), but it is `0.x` and breaks with each release; three
of the four Rust frameworks checked expose its types in their widget API and upgrade in lockstep,
while Slint keeps its own model and confines AccessKit to a backend.

## Decision

1. **One payload type.** `SemanticsAttributes` holds everything a node publishes, in groups:
   role, naming (label, description, value, placeholder, role description, tooltip, keyboard
   shortcut, access key, language, URL), states, numeric range, live region, collection, text
   content, scroll metrics, relations, traversal, custom actions, tags, platform view. The
   configuration a render object fills is assembly directives plus these attributes plus action
   handlers. The tree stores configurations; publication reads attributes through a borrowed
   view. `SemanticsProperties`, `SemanticsFlag`/`SemanticsFlags`, `SemanticsNodeData`,
   `SemanticsNodeSnapshot`/`SemanticsSnapshot` and `SemanticsEvent` are removed.
2. **One role.** A node has `role: Option<SemanticsRole>`; interactive kinds are roles
   ([ADR-0203](ADR-0203-semantics-vocabulary.md)). `None` publishes static text when the node is
   named and a generic container otherwise; the window root publishes `Window`.
3. **Typed states.** Each state is `Option<T>`: absent, or declared with a value, so a declared
   `false` is distinct from unset. Tri-state values are enums (`Toggled { Off, On, Mixed }`
   serves both checked and pressed, its meaning set by the role, as in AccessKit and Chromium).
   Each state type implements `StateMerge`: `conflicts` (declared on both sides) and `merge`
   (its documented precedence under a forced merge; `Mixed > On > Off` for `Toggled`).
4. **One merge table.** Per group: union (relations, tags, and custom actions, which the
   contributor's follow and each keep their own handler), direction-aware concatenation (label,
   description), first wins (other naming, traversal), conflict when both
   are present (role, each state through `StateMerge`, non-empty value, range, collection, text,
   scroll, live, platform view). A conflict forms separate nodes; a forced merge
   (`MergeSemantics`) resolves it: states by `StateMerge::merge`, the receiver otherwise, and
   role by specificity — an authored structural role (tab, list item, dialog, …) first, then
   checkable roles, then `Button`, then link, text inputs and slider, then image and heading;
   on a tie the receiver keeps its own. A node that itself merges its descendants is
   never absorbed by an outer merge; it stays a node of its own.
5. **One translation.** One module maps attributes to `accesskit::Node`; nothing else names
   AccessKit node types. A role, state or property ARIA 1.2 does not allow on the node's role is
   not published and is reported once per change with `tracing::warn!`; a name on a role that
   prohibits one becomes the description.
6. **The compiler enforces coverage.** Merge, conflict, validation and translation destructure
   `SemanticsAttributes` and each group exhaustively, without `..`, so a new field does not
   compile until all four handle it.

## Consequences

- Authors meet types instead of flags: `Semantics::heading(HeadingLevel)`,
  `checkbox(Toggled)`, `slider(NumericRange)` set a role and its data together. Existing widget
  call sites migrate mechanically.
- AccessKit upgrades touch one module and its tests; FLUI's public signatures do not change.
- The merge outcome changes where intended: a nested merger keeps its node. Other trees stay
  as they are: the step that introduces the types changes no test expectation, and the step
  that introduces the merge table compares every widget test's published tree before and after.
- A flag-word bitmask no longer travels; snapshots for other threads are the AccessKit update
  ([ADR-0082](ADR-0082-platform-api-contract-crate.md)).

## Verification

`compatibility_rules_for_modeled_fields` (the conflict rows), the `a_forced_merge_keeps_*` tests and
`state_merge_precedence` in `flui-semantics`; the coverage matrix (every role and attribute read
through `accesskit_consumer`); `merge_semantics_over_a_tile_and_radio_announces_as_one_radio_button`
and `role_precedence_settles_forced_merges` for the role precedence;
`nested_merge_semantics_keeps_its_own_node` in `crates/flui-rendering/tests/semantics_assembly.rs`.
