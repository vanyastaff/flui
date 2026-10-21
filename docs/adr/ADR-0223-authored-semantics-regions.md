# ADR-0223: Authored semantics regions retain presentation-owned identities

- **Status:** Accepted
- **Date:** 2026-10-16
- **Supersedes:** [ADR-0204](ADR-0204-synthetic-semantics-nodes.md), §1 for identity authority and authored-region action routing
- **Related:** [ADR-0206](ADR-0206-typed-semantics-attributes.md)

## Context

A custom-painted chart can expose several independently actionable parts without
inventing render children for them. One host configuration loses their separate
bounds, order and actions. A semantic region is accessible content within one
render object's custom content; it is distinct from a render boundary and from a
compositing layer.

Flutter's `CustomPainterSemantics` supplies independently keyed, transformed
rectangles. Its custom-paint assembly places background regions before actual
child semantics and foreground regions after them. The region lists themselves
are flat; normal render nesting supplies nested accessible structure. These are
useful behavioral cases, not reasons to copy its render-class hierarchy.
Primary references are pinned to Flutter commit
`abaf9c523780a608bd46686fd5e53740a07077f8`:
[painter regions](https://github.com/flutter/flutter/blob/abaf9c523780a608bd46686fd5e53740a07077f8/packages/flutter/lib/src/rendering/custom_paint.dart)
and [semantics assembly](https://github.com/flutter/flutter/blob/abaf9c523780a608bd46686fd5e53740a07077f8/packages/flutter/lib/src/rendering/object.dart).

## Decision

`CustomPainter::semantics_regions(Size)` returns data-valued regions: a required
logical key, local rectangle, local-to-host transform and existing typed
`SemanticsConfiguration`. The committed host size is the same size painting
receives. `RenderBox` and `RenderSliver` expose before-child and after-child
contributions through their normal protocol adapters. `RenderCustomPaint` wires
its two delegates into those contributions. The inert `SemanticsBuilder` shell
is removed; painters without accessible content use the default empty list.
The facade and SDK expose the region and key values so implementations need no
internal dependency.

A painter's repaint notification admits paint and semantics work atomically.
Both derive from the same authored state, and the host cannot infer that a
changed drawing left all region bounds and actions unchanged. No second notifier
or manual host invalidation is required. Replacement predicates still decide
paint and semantics impacts independently.

Identity is `(generational host RenderId, before/after namespace, logical key)`.
Labels, bounds and list positions do not define identity. Retained keys keep their
ID across reordering, geometry changes, full rebuilds, partial grafts and
accessibility disable/enable. A successfully rebuilt host retires absent keys;
reinsertion receives a new ID. Unvisited mounted scopes retain their keys, so
partial assembly and temporary ancestor exclusion cannot retire unrelated
content. Removed render generations are pruned at the next assembly pass.

The pipeline owns an opaque monotonic identity source throughout the
presentation's lifetime, independently of replaceable `SemanticsOwner` values.
Its reserved range is `1..=u32::MAX`, disjoint from nonzero render generations.
Batch reservation is all-or-nothing and never wraps. Duplicate keys in one
contribution list and exhausted reservations return a semantics frame error,
retain the existing tree and queued marks, and permit a later retry. Every graft
anchor is staged before the first arena mutation. An abandoned graft plan does
not allocate or retire identities. An incoming owner's synthetic tree cannot
import another allocator's authority: it is cleared and rebuilt using the
pipeline source. Hand-built render-boundary trees remain supported.

A duplicate-key or exhaustion error remains authoritative during rejection.
The pass pins registered standard and custom action handlers immediately after
each returned node description or region contribution, including blocked actions
and ancestor-context descriptions. Folding may overwrite the receiver's callback
or discard a clipped contributor before validation; neither operation may release
its last executable owner. Pins survive an abandoned graft proposal and its full
rebuild fallback without retaining another topology or copying configuration data.
Draft topology, geometry and attribute buffers are released. Configuration
retirement lends each action handler to the caller's existing recovery scope;
unproven callback owners are retained under ADR-0127. Rejection does not invoke their
destructors and therefore cannot replace the typed frame error with a capture
panic or strand its retryable marks. This is an exceptional-path policy;
healthy callback ownership retains its normal destruction behavior.
After a successful pass, temporary callback ownership retires only after publication,
queue completion and phase exit. Independent captures use the existing recovery
scope, preserving the first destruction failure and normal healthy destruction.

Regions enter the ordinary fragment assembly rules as authored boundaries.
Forced merge absorbs them under ADR-0206; exclusion omits the host subtree.
Transparent hosts still forward their contributions. Ancestor action blocking,
reveal ancestry and clips apply to each region, using the same checked root-space
projection as render nodes. The host's child clip applies to actual children;
regions inherit the host's ancestor clips. Published bounds are clipped while
reveal keeps uncut root-logical bounds. Unrepresentable projected regions are
omitted without retiring their still-authored keys.

An action resolves against the exact region node and its own callback, using
the existing current-tree and delivered-offer checks. It is not redirected to a
generic host callback. Callback invocation follows the existing detached action
snapshot contract. Arbitrary render description before it returns a configuration,
and destruction of callbacks owned by an old published tree, retain their existing
containment boundaries; the pass-local custody above does not cover them.

ADR-0204's previous status claimed an implemented synthetic allocator, but the
render-backed-only node representation had no such authority. This decision
supplies it for authored regions. Text-run generation, text selection and
announcements keep their separate contracts and require their own consumer
verification; this change makes no implementation claim for those features.

## Verification

The public render-object family runs
`harness_custom_paint_semantic_regions_keep_order_geometry_identity_and_actions`,
`harness_custom_paint_regions_obey_merge_exclude_and_action_blocking`,
`harness_custom_paint_region_failure_keeps_all_anchors_and_retries` and
`harness_custom_paint_regions_project_clip_and_rebind_owner`.
`synthetic_identity_exhaustion_never_wraps_or_partially_reserves` uses a private
counter seam because reaching the terminal value publicly is impractical.
