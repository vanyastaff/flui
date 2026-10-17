# ADR-0224: Semantics child selection receives committed geometry

- **Status:** Accepted
- **Date:** 2026-10-16
- **Related:** [ADR-0223](ADR-0223-authored-semantics-regions.md),
  [ADR-0190](ADR-0190-owner-local-render-invalidation.md)

## Context

A flow delegate can present only some of its laid-out children and move them
according to its allocated size. Child layout participation alone cannot tell
the semantics walk which children are presented. A selection hook without size
would force the object to duplicate geometry owned by the rendering pipeline.
Paint clipping and accessible clipping must also describe the same parent box.

## Decision

`visits_child_for_semantics(child_slot, size)` receives the committed box size
or the sliver's absolute paint size, just as the existing clip and region hooks
do. The canonical render tree still owns topology and child slots. This hook
selects participation; it does not introduce another child list or own geometry.
Ordinary objects retain their existing size-independent selection policies.

Flow replays its pure presentation delegate with committed size and child sizes
for selection, transforms and hit testing. The shared replay helper owns only
temporary query results. It neither paints nor caches a second geometry state.
Unpresented children retain layout but contribute no accessibility nodes or
actions. A physical clip bounds the accessible geometry of presented children.

Per-child opacity uses the existing validated `PaintOpacity` value and a group
scope in the ordinary paint-fragment recorder. A zero-opacity paint call still
occupies its slot but selects no presented child. Partial opacity preserves
geometry, hit-testing and semantics; the compositor retains continuous alpha.
`harness_flow_child_opacity_reaches_compositing_and_input` covers both inline and
repaint-boundary children. The mounted repaint/replacement row also exercises
transparent, partial and opaque accessibility transitions.

Flow and its render object share the delegate through `Rc`. The delegate's
repaint source and callback state are owner-local, so atomic shared ownership
would impose cost without granting cross-thread use.

Logical reading order follows declarative child order. Delegate paint order
controls stacking and hit priority, not reading order: moving or restacking
controls should not silently reorder their spoken content. Authored semantics
ordering remains available independently.

A delegate repaint can change semantic participation and geometry. The local
invalidation handle admits paint and semantics marks together before requesting
a frame. A paint-only delegate replacement reports both impacts; a relayout
continues through the pipeline's existing layout-to-semantics propagation.

## Alternatives and consequences

Copying allocated size into each render object duplicates authoritative state.
Using an invalid transform to signal absence conflates geometry with selection.
Adding a separate public traversal abstraction is unnecessary for this policy.
Pure replay uses the same contract already required for flow hit testing and
coordinate conversion; it retains their linear replay cost for each query.

External box/sliver implementations of the selection hook add a `Size` argument;
size-independent implementations may name it `_size`. Defaults are unchanged.

## Evidence

The public objects harness rows
`harness_flow_accessible_bounds_follow_clip_without_layout` and
`harness_flow_only_announces_presented_children_after_repaint_and_replacement`
observe clipping, initial omission, repaint-driven selection, stale action
refusal, delegate replacement and declarative reading order under reversed
paint order through the actual rendering pipeline.
