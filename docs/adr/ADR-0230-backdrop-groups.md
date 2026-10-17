# ADR-0230: Grouped backdrop filters share one capture per surface

- **Status:** Accepted
- **Date:** 2026-10-17
- **Related:** ADR-0115 (image source regions)

## Context

Every backdrop filter reads the attachment at its position in the ordered
command stream, so a later glass pane filters the content an earlier pane
painted, and each pane copies its own backdrop region. Toolbars and sidebars
built from several adjacent panes want the opposite: all panes sample the same
background, and the backdrop is read once. No widget exposed a backdrop filter
at all.

## Decision

`flui_layer::BackdropGroupKey` is an identity (an `Arc` compared by pointer):
clones are equal, separately created keys never are, and no label or counter
stands in for it. `BackdropFilterLayer`, `RenderBackdropFilter` and the
`PaintCx::with_backdrop_filter` scope carry an optional key.

The engine records the key on each backdrop operation. While replaying one
ordered item list — one attachment, the root surface or one offscreen layer —
the first operation of a group copies the readable attachment into a pooled
snapshot texture; every operation of that group, the first included, reads its
region from the snapshot instead of the live attachment. The snapshots are
local to that list and dropped with it, so a group never reads another
surface's pixels or a later reuse of the same pooled texture. Ungrouped
filters keep reading the live attachment.

Content painted after the group's first filter is therefore not part of what
later grouped filters sample, including an earlier pane's own child: that is
the contract, not an artifact.

`flui-widgets` adds `BackdropFilter` (filter, blend mode, enabled, and
`grouped()`), and `BackdropGroup`, a stateful widget whose key lives in its
state, so rebuilding it keeps the identity and does not repaint its filters.
A grouped filter with no group above it reads the live backdrop.

## Verification

`grouped_backdrops_share_one_capture`, a row of the engine's
`layer_effects_capture_as_specified` GPU family, reads back an overlapping
pane that is tinted by the first pane's child when ungrouped or in different
groups, untinted in one group, and tinted again when the two members sit on
different surfaces. `harness_backdrop_filter_carries_its_group_to_the_layer`
pins the render object, and
`backdrop_filters_in_one_group_share_a_stable_identity` pins the widgets,
including identity across a rebuild.
