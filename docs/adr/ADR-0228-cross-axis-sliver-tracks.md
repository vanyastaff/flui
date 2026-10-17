# ADR-0228: Cross-axis slivers use explicit track allocation

Status: Accepted

## Context

Parallel sliver tracks share a scroll coordinate but need independent cross-axis
allocation. A constrained child can claim a finite extent before flexible
siblings divide the remainder. These are separate policies, rather than special
numeric flex values. Placement must remain consistent across layout, paint,
hit-testing and accessible geometry.

## Decision

`SliverCrossAxisAllocation` distinguishes `Content` from `Flex(NonZeroU32)`.
`SliverCrossAxisParentData` carries this configuration; the render tree remains
the sole owner of committed positions. Ordinary children default to one share.
`SliverCrossAxisItem` authors nondefault policies through the normal parent-data
view seam, which rejects an incompatible render parent.

`SliverConstrainedCrossAxis` validates a finite, nonnegative limit and preserves
the child's origin and main-axis geometry. The parent track determines physical
placement. The render object reports the actual constrained cross-axis extent;
an invalid update leaves the previous limit authoritative. Construction and
updates return `InvalidSliverCrossAxisExtent` rather than accepting invalid
floating-point state.

`SliverCrossAxisGroup` lays out content tracks first and then flexible tracks.
A content track must report a finite extent within its available budget;
otherwise layout refuses the proposal. The group composes each main-axis
geometry component independently instead of borrowing the entire geometry of
the longest child. Cross-axis direction controls physical track order while
declarative order controls semantics. Reversed main axes align child paint to
the opposite edge. Pinned descendants are contained by the group's remaining
scroll extent.

Division precedes multiplication when allocating flexible shares. Physical
track positions and ends must remain representable. A nonrepresentable draft
is refused before committing positions; the existing degraded-layout contract
withholds scroll dimensions, retaining the previous accepted range. A later
representable proposal can recover. Scroll corrections propagate before
position publication.

## Alternatives

Adding optional flex fields to every sliver's physical parent data would permit
track configuration under unrelated render parents. A group-owned parallel
configuration vector would add a child-count synchronization obligation. A
dedicated parent-data type uses the existing reconciliation and compatibility
checks. Converting tracks to boxes would discard sliver cache, obstruction and
scroll-correction contracts.

## Evidence

`harness_sliver_cross_axis_group_allocates_tracks_in_both_physical_axes` exercises
content/flexible tracks, both physical axes, reversed directions, paint and hits.
`harness_sliver_cross_axis_group_refuses_unrepresentable_tracks_and_missing_content_extent`
checks retained accepted scroll dimensions and recovery through a real viewport.
`cross_axis_sliver_tracks_reconcile_weights_and_publish_child_semantics` changes
weights and limits on a mounted tree and reads the platform-facing geometry.
`harness_sliver_constrained_cross_axis_limits_actual_layout_and_refuses_invalid_updates`
and `constrained_sliver_updates_layout_without_replacing_the_child` pin the
standalone render and widget paths.
