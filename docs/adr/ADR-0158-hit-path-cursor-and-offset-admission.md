# ADR-0158: Hit paths distinguish cursor deferral and refuse non-finite offsets

- **Status:** Accepted
- **Date:** 2026-10-07
- **Related:** [ADR-0113](ADR-0113-finite-inverse-hit-admission.md),
  [ADR-0127](ADR-0127-exceptional-path-retention.md)
- **Design source:** [Interaction patterns](../plans/specs/flui-interaction/patterns.md),
  [routing contract handoff](../plans/specs/flui-interaction/t6d-handoff.md)

## Decision

A hit entry contributes `CursorRequest::Defer` or `Icon(CursorIcon)`.
The first explicit icon in the leaf-first path wins, including
`Icon(CursorIcon::Default)`, which selects the platform arrow. A path containing
only deferrals resolves to that arrow. Treating `CursorIcon::Default` as a
deferral sentinel made an explicit child arrow indistinguishable from an absent
request, so an ancestor's text cursor could override it.

`MouseRegion` defaults to deferral; its existing `cursor(icon)` builder selects
an explicit icon. `RenderMouseRegion` stores that request, the render traits
return it, and both pipeline hit-test paths attach it to the canonical
`HitTestEntry`. Ordinary render objects defer. The request enum remains
exhaustive: absence and explicit choice are the complete contribution states;
additional platform icons belong to `CursorIcon`, not additional request modes.

`HitTestResult::with_paint_offset` returns `Option<R>` just like the transform
scope. A non-finite offset returns `None`, does not call its subtree, and leaves
the ancestor transform stack intact. The rendering walk interprets refusal as
no hit. Publishing an invalid entry and skipping pointer dispatch afterwards
would still admit its cursor and hover annotation, giving different consumers
inconsistent answers to the same hit test.

Every positional sample in a localized pointer move is transformed, including
coalesced and predicted samples. Scroll deltas are transformed without a
translation contribution while preserving their pixel, line or page unit.
The scroll claim walk uses the same local coordinate contract.

Hover state is tracked per device. A shared region's resolved annotation remains
available until the final device leaves; retiring one device must not erase
another device's exit. Ambient probes visit device identities in order and
preserve the previous state of a device whose probe fails. Every already
committed transition is delivered before the first failure resumes. Withdrawn
callback captures retire outside tracker borrows using ADR-0127: after a caught
failure, later opaque captures are retained and the next healthy operation can
progress.

The hosting window has one cursor output. Its latest physical source owns that
output; ambient probes refresh every device's regions but cannot transfer cursor
ownership by iteration order. An ambient result revalidates its device's exact
observation after the probe, preserving newer reentrant motion and re-admission.
Removing the owner selects the arrow with the retired source's real metadata.
In-flight cursor callbacks are distinct from successful publication: only a
successful current observation and hook acknowledge delivery. Failure, hook
replacement and same-state reentry retain newer debt for a subsequent operation.

## Evidence

The public integration contracts are `explicit_arrow_cursor_wins`,
`hit_test_offset_admission`, `transformed_entry_receives_local_samples_and_deltas`,
`shared_region_exit_per_device`, `ambient_refresh_contains_each_device`, and
`released_region_destructor_reenters_tracker`. Ambient coverage asserts the
first device's competing callback failure and recovery after a probe failure;
bounded retirement children cover single and competing capture failures,
retention of the remaining captures, and subsequent healthy enter/exit delivery.
These contracts make no native backend execution claim.

`mouse_tracking_ordering_and_cursor_deferral` additionally covers physical
source ownership, stale ambient probes, source retirement, callback replacement,
publication failure and recovery.
