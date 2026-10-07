# ADR-0160: Focus traversal uses scoped reading direction

- **Status:** Accepted
- **Date:** 2026-10-07
- **Supersedes:** [ADR-0026](ADR-0026-focus-traversal-seam.md) §2's required
  traversal-policy signature only
- **Related:** [ADR-0079](ADR-0079-keyboard-activation-and-focus-for-assistive-technology.md),
  [ADR-0127](ADR-0127-exceptional-path-retention.md)

## Decision

Strict top-coordinate ordering splits visually aligned controls when their
bounds differ by a small vertical offset. A fixed left-to-right order also
ignores an inherited right-to-left reading direction. Traversal therefore uses
spatial rows and the direction of the enclosing focus scope.

`FocusScopeNode` owns a `TextDirection`, initially `Ltr`. The mounted
`FocusScope` reads its nearest inherited `Directionality` during `init_state`
and `did_change_dependencies`, and transfers that direction when replacing its
external scope node. An absent inherited direction and the presentation's root
scope use `Ltr`. Changing direction preserves the caller's custom traversal
policy and the existing focus nodes. Storing direction in a newly installed
default policy would overwrite a caller's policy on every inherited update.

The required policy method is
`order(&self, nodes: &mut [Rc<FocusNode>], direction: TextDirection)`. The scope
snapshots its policy and direction before invoking user code, outside the
policy cell's borrow. A replacement or direction change made during that call
applies to the next traversal. Policies must permute the supplied identities,
retaining each exactly once. A mutable slice fixes cardinality and avoids
returning a second owned candidate list; it does not prevent an implementation
from replacing or duplicating `Rc` handles. Permutation remains the policy's
contract, rather than a type-system guarantee. A failed policy call publishes
no order, preserves the first failure, and retires outgoing policy and candidate
ownership under ADR-0127.

`ReadingOrderPolicy` snapshots each candidate's rectangle once before sorting.
Rect providers are user callbacks, so comparisons use only the snapshot and
no owner borrow spans a provider call. A rectangle with non-finite edges or
nonpositive dimensions follows all spatial candidates in structural order.
Consequently, bounds that are still zero before layout retain deterministic
traversal instead of discarding an eligible node.

Valid rectangles form rows from top to bottom. Each row retains a strictly
nonempty shared vertical intersection, which shrinks as members join. A tall
rectangle cannot connect otherwise disjoint rows through a transitive overlap;
touching vertical edges start separate rows. Within a row, leading edges ascend
from the left in `Ltr` and descend from the right in `Rtl`. Exact ties retain
depth-first sibling attachment order. The initial descendant stack and nested
sibling pushes use the same reversal so popping visits siblings in that order.
This defines scope-level reading order; mixed-direction traversal groups and
directional-arrow traversal require separate decisions.

The rest of ADR-0026 §2 remains in force:
`FocusScopeNode::sorted_traversal_order(cursor)` is the single traversal
primitive, force-including a structurally live cursor even when it is disabled
or skipped. Next, previous, first focus and scope-edge resolution use that
order. Existing edge behavior and ADR-0079's partial supersession of the
Actions/Shortcuts nesting rule remain unchanged.

## Tests

The public widget table `focus_actions_and_shortcuts` drives real Tab and
Shift+Tab through mounted actions and committed layout. Its rows
`tab_groups_vertically_overlapping_widgets_into_one_reading_row`,
`tab_reads_an_rtl_scope_from_its_inherited_directionality`, and
`a_tall_widget_cannot_bridge_disjoint_reading_rows` pin spatial rows and RTL.
`a_directionality_update_changes_tab_order_without_replacing_focus_nodes`
pins inherited updates on the same external nodes.
`spatial_tab_preserves_geometric_ties_and_row_boundaries` asserts the complete
identity order as well as focus transitions, covering fractional overlap,
touching edges, unequal-width RTL leading edges, stable ties and zero-sized
fallback. `tab_traversal_preserves_failure_before_policy_and_candidate_retirement`
retains the custom-policy failure and ownership contract.
