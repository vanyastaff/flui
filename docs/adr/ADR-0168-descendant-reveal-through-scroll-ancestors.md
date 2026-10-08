# ADR-0168: Descendant reveal through scroll ancestors

- **Status:** Accepted
- **Date:** 2026-10-07
- **Related:** [ADR-0043](ADR-0043-presentation-bundled-trees-and-realm-globalkey-scope.md),
  [ADR-0054](ADR-0054-the-viewport-commits-one-result.md),
  [ADR-0127](ADR-0127-exceptional-path-retention.md)

## Context

A native `ScrollIntoView` request addresses a published descendant, while its
scroll ancestors own the positions that can expose it. A materialized labelled
descendant previously disappeared beyond the viewport's semantics cache. Giving
every descendant a callback would duplicate ownership and make nested reveal a
widget-level routing protocol. Paint clipping also cannot serve as the test of
whether the descendant remains addressable for reveal.

Callbacks are reentrant. A position listener can request another descendant
before layout publishes the first movement. Adding a cached geometry delta to
the new position moves too far; refusing all such requests loses accepted work.
Resolved snapshots also own callback captures even when deliberately dropped
without invocation.

## Decision

### Addressability and geometry

The existing render semantics clip contract distinguishes `SemanticsClip::Bounds`
from `SemanticsClip::ScrollCache`. A cache limits ordinary publication; it does
not remove materialized descendants under an actual registered `ShowOnScreen`
ancestor. Explicit authored bounds, occlusion, hidden annotations and subtree
exclusion retain their meanings. Viewport paint clipping alone does not mark
these revealable descendants hidden in AccessKit.

Publication geometry remains clipped where paint bounds intersect it. Each
semantics node separately retains its source's unclipped root-space logical
rectangle for reveal. Geometry and the receiving ancestor's published
`scroll_position` come from the same semantics assembly. AccessKit's root
device-pixel-ratio transform remains the physical-coordinate boundary.

Assembly advertises `ShowOnScreen` on descendants of actual registered handlers
without installing per-descendant callbacks. This structural route does not
annotate otherwise transparent render nodes.

### Delivery

`SemanticsOwner::resolve_action` resolves a unique stable identity in its rooted
presentation tree and snapshots real ancestor handlers nearest first. An explicit
handler on the target retains ordinary direct action dispatch. Otherwise each
ancestor receives `ActionArgs::ShowOnScreen` containing the target rectangle,
its viewport rectangle and its published scroll-position basis. The preceding
viewport becomes the next outer ancestor's target, so the outer scrollable
reveals the inner viewport rather than only a narrow descendant fragment.

Scrollable rebases target edges by its actual movement since that published
basis, projects the nearest obscured edge onto its configured axis direction and
clamps the resulting position to its extents. An already visible target, or an
oversized target exposing both viewport edges, requires no displacement. Reveal
stops the active trajectory before writing, including a no-displacement request.
Same-target and sibling reentry use this geometry basis; there is no separate
admission ledger that suppresses another accepted target.

Invocation and snapshot retirement run after the pipeline owner borrow has been
released. Private owner-local weak membership identities bind geometry to the
target and its entire ancestor path. Removal, replacement, reparenting, root
changes and mutable geometry access revoke old snapshots. Node clones do not
copy membership authority. Invocation checks the path between ancestor calls;
stale snapshots deliver no old geometry.

### Failure and ownership

Every reveal callback and every outgoing callback snapshot has an individual
containment boundary. The first body or capture-retirement panic stays
authoritative. Failed callback ownership, remaining captures and later panic
payloads are retained according to ADR-0127. Still-live ancestor callbacks
continue after a body failure; the first panic resumes after retirement finishes.
Healthy ownership is destroyed normally.

The same retirement helper handles intentionally dropped, uninvoked snapshots.
It prevents a framework-owned vector from dropping a second hostile capture
while the first is unwinding. This cannot rescue a single user-owned aggregate
that already double-panics inside its own destructor before the boundary is
reached. Direct single-handler action behavior is unchanged.

## Consequences

The semantics tree and detached reveal invocations belong to the presentation's
owner thread; immutable `SemanticsSnapshot` data remains the cross-thread handoff.
No per-node lock or asynchronous frame operation is introduced. Reveal reaches
materialized descendants only: it does not create lazy children merely to find a
target that was never published.

Public widget rows `show_on_screen_reveals_offscreen_targets_on_both_axes_and_reverse`,
`show_on_screen_walks_nested_axes_and_replacement_uses_current_geometry`,
`show_on_screen_failure_continues_live_ancestors_and_fresh_requests_recover`,
`show_on_screen_same_pipeline_reentry_keeps_one_reveal_and_recovers` and
`show_on_screen_sibling_reentry_delivers_last_target_without_stale_motion`
exercise native AccessKit requests, publication, current owners and recovery.
The lower public family
`descendant_reveal_snapshots_preserve_identity_failure_and_recovery` covers stale
membership, body failures, healthy capture retirement and single/competing
retirement of uninvoked snapshots.

Reverting only the measured current-minus-published displacement makes the
unchanged same-target and sibling reentry rows overscroll. Removing only the
invocation's individual snapshot retirement makes the unchanged unused-snapshot
workers destroy a later capture after the first failure; competing capture
destructors abort the isolated worker. Restoring each production boundary makes
its public family pass again.
