# ADR-0165: Focus policy groups and directional traversal

- **Status:** Accepted
- **Date:** 2026-10-07
- **Supersedes:** [ADR-0026](ADR-0026-focus-traversal-seam.md) §2's scope-only
  resolver and §3's scope-only parent retry; its widget and scope-history contracts remain
- **Supersedes:** [ADR-0160](ADR-0160-focus-reading-order.md)'s claim that the
  scope's sorted list is the only traversal primitive; its reading-order algorithm remains
- **Related:** [ADR-0079](ADR-0079-keyboard-activation-and-focus-for-assistive-technology.md),
  [ADR-0127](ADR-0127-exceptional-path-retention.md)

## Context

A focus scope owns autofocus and focused-child history. A custom ordering boundary
must not introduce those semantics. Linear reading order also cannot express geometric
arrow navigation: a right arrow should select a control to the right even when reading
order would visit a different row. Both paths must preserve the existing synchronous,
presentation-owned focus tree and focused-control-first keyboard dispatch.

## Decision

`FocusTraversalGroup` installs a policy on a plain, non-focusable focus node. It
introduces neither a scope nor focus history. Nested groups form contiguous blocks in
their containing policy order and inherit text direction. Groups default to
`ParentScope`; scopes retain their existing `ClosedLoop` default.
`FocusScope::edge_behavior` exposes the scope's existing edge contract in a widget builder.

Linear `TraversalDirection` and four-way `FocusDirection` are separate closed enums:
neither can accidentally select a mode from the other domain. Linear next/previous
overrides hold weak exact-node targets. Lookup checks attachment, exact manager ownership,
effective focusability, traversal eligibility and the same nearest group/scope boundary.
Invalid targets fall back to policy order. Generation-checked registrations prevent an
older cleanup token from clearing a newer policy or override; weak links do not keep
targets alive.

Linear traversal snapshots group membership, policy ownership and ancestor boundaries
before callbacks. Policies retain ADR-0160's permutation contract. Scope reading order
still supplies scope candidates, and nested group policies order their contiguous blocks.
`ParentScope` retries the actual containing group or scope with the same cursor. Each
group's order is cached for that input: replacing its policy inside a callback affects a
later traversal, without invoking the replacement during an ancestor retry. Unfocused
and scope-parked entry preserve the existing first-candidate behavior.

Directional traversal admits only rectangles with finite edges and finite positive
width and height. Missing geometry, zero fallback bounds, inverted bounds and overflowing
extents are untargetable. An unusable source leaves focus unchanged. Candidate centers
must lie strictly in the requested forward half-plane. Ranking prefers overlap with the
source's perpendicular beam, then the nonnegative primary edge gap, then perpendicular
center distance. Exact ties preserve depth-first sibling tree order.

Distance comparisons distinguish finite subtraction from overflow and compare
half-scaled magnitudes in the latter case. This preserves ordering of admitted finite
endpoints without squaring distances or publishing infinite geometry.
Source and candidate providers run at most once per directional step; lazy snapshots are
reused for wrapping and parent retries. Boundary edges are frozen before callbacks.

For both paths, `Stop` preserves primary focus, `LeaveView` releases it, and
`ParentScope` visits the enclosing actual boundary, retaining the existing root wrap
fallback. Directional `ClosedLoop` wraps toward the opposite geometric edge with the
same beam preference. Selected targets are rechecked against the owner and boundary
before applying the result. A provider or policy that changes primary focus reentrantly
invalidates the obsolete step.

Unmodified arrows reach `DirectionalFocusIntent` and `DirectionalFocusAction` through
the presentation's default root shortcuts. The action holds the lifecycle-acquired focus
manager. Focused editors and sliders receive and may consume the key first. Movement
returns `Handled`; an unavailable move or leave-view edge stops shortcut propagation
with `SkipRemainingHandlers`, leaving native default handling available.

Policy, provider and backing-node snapshots retire outside internal borrows through the
existing focus containment accumulator. A first failure prevents publication and stays
authoritative over competing capture destruction. Strong candidate, boundary and scope
ownership retires before propagation. Healthy retirement remains ordinary destruction;
ADR-0127's limits on aggregate destructors that double-panic before containment remain.

## Evidence

The mounted `focus_actions_and_shortcuts` table covers the production shortcut path:

- `traversal_groups_order_blocks_without_creating_focus_scopes` pins contiguous policy
  blocks, forward/reverse navigation and the absence of a new enclosing scope.
- `typed_focus_overrides_fall_back_after_target_invalidation` pins override admission
  and ordinary traversal after invalidation.
- `arrow_traversal_prefers_the_beam_and_respects_group_edges` pins geometric selection
  through mounted arrow actions.
- `widget_scope_edge_configuration_reaches_the_tab_path` and
  `nested_scope_edges_visit_the_containing_group_and_reuse_policy_order` pin widget
  configuration, actual boundary ancestry and one policy invocation across parent retries.

The public `caught_callback_failures_leave_captures_with_their_owner` table includes
`geometric_focus_navigation_pins_ranking_and_admission`,
`directional_edges_match_linear_scope_outcomes`,
`weak_traversal_links_revalidate_groups_and_registration_generations`,
`directional_provider_failure_preserves_first_failure_and_recovery`,
`group_policy_replacement_preserves_failure_and_future_traversal`, and
`directional_geometry_snapshots_run_once_and_respect_reentrant_focus`. These cover
fractional and extreme finite bounds, all four directions, weak identity, replacement,
single and competing failures, recovery and reentrant focus selection. The provider
failure row drives both linear and directional public navigation. Reverting provider
containment makes its linear failure case fail before recovery; restoring containment
passes the family. This is behavioral evidence for that repair, not proof of a live
native keyboard producer.

## Alternatives rejected

- Using a focus scope for a group would change autofocus and route restoration merely
  because a caller selected an ordering policy.
- A Boolean or one combined direction enum would conflate independent traversal domains.
- Strong explicit links would extend target lifetime; label lookup would lose exact identity.
- Calling rectangle providers from sort comparisons or parent retries would expose callback
  side effects repeatedly and make failure ownership depend on comparison count.
- Squared Euclidean distance would overflow for admitted finite coordinates and would ignore
  the beam preference that keeps arrow navigation within the visual row or column.
