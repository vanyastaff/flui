# Flutter Issue Comments: Phase and Tree Lessons for FLUI

Date: 2026-09-13

Scope: external issue evidence from Flutter's public tracker, focusing on
comments/proposals rather than only issue titles.

## Sources Read

- Flutter #61651, open P2/perf: excessive `RenderObject`/`Layer` rebuild and
  repaint scope. No comments; body itself is the architectural analysis.
- Flutter #126332, open P3/proposal: build-phase code touching layout-phase
  data can crash or read stale transforms. Comments include a prototype sanity
  check that catches a real production bug from existing tests, but also
  produces many false positives.
- Flutter #91114, open P2/framework: kept-alive `Element`s under a
  `LayoutBuilder` may not receive config updates while offscreen. No comments;
  body frames two possible solution directions: notify the widget or still call
  `Element.updateChild` while offscreen.

## Lessons

### Repaint Granularity

Flutter #61651 confirms the long-term cost of relying on app authors to place
manual repaint boundaries. A framework should track visual/damage dependencies
well enough that animating a local visual property does not default to repainting
unrelated retained content.

FLUI status: already has issues #1037, #1083, and #1084 for missing partial
damage identity, stack-safe layer walks, and display-list effect depth. A
separate scene/cache/resource pass found no new cache/reuse duplicate.

### Build Must Not Read Layout

Flutter #126332's comment thread is more useful than the title: a naive debug
assert catches real bugs, but also trips too broadly. That argues for a typed or
capability-scoped design rather than only a runtime assert.

FLUI status: `BuildContext::pipeline_owner()` explicitly documents the
`renderObject.size` / `getTransformTo` trap and marks the returned
`PipelineCell` as a frame capability. `scripts/check-frame-capability-scope.sh`
includes `pipeline_owner` and `hit_test_handle`; `just port-check-verbose`
passes trigger #22. Current production geometry readers in widgets capture the
capability in lifecycle hooks or store handles for later callbacks rather than
reading layout from `build`.

No new issue filed from this source pass.

### Kept-Alive Updates

Flutter #91114 shows a deep retained-tree failure mode: a kept-alive offscreen
subtree can be retained yet not updated because one layer's update depends on a
layout callback that is skipped while offscreen. The architectural lesson is
that retention and update propagation must be separate contracts. "Not laid out
this frame" must not automatically mean "does not receive a new widget
configuration."

FLUI status: existing #1073 covers a concrete reparent/inherited dependency
failure. A broader kept-alive + layout-builder offscreen update probe remains
unresolved and should be targeted before filing anything new.

## Commands / Local Evidence

- `gh issue view 61651 --repo flutter/flutter --json ...`
- `gh issue view 126332 --repo flutter/flutter --json ...`
- `gh issue view 91114 --repo flutter/flutter --json ...`
- `just port-check-verbose` passed all refusal triggers, including #22.
- Source inspected:
  - `crates/flui-view/src/context/build_context.rs`
  - `scripts/check-frame-capability-scope.sh`
  - `crates/flui-widgets/src/interaction/focus.rs`
  - selected `pipeline_owner()` call sites.

## Follow-Up

- Mine comments from Flutter's P1 overlay/layout mutation regression #187162
  and composited-transform issue #178522 next; these should inform FLUI's
  overlay/follower/composite phase contract.
- Build a direct FLUI probe for the #91114 class: kept-alive offscreen
  `LayoutBuilder` descendant receives config changes even when layout is not run.
