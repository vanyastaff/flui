# Flutter Overlay/Follower Lessons for FLUI

Date: 2026-09-13

Scope: public Flutter issue comments plus pinned Flutter 3.44.0 source, used to
check whether FLUI's overlay/follower architecture is avoiding known framework
debt rather than merely porting the happy path.

## Sources Read

- Flutter #187162, open P1: `ReorderableListView` + `Tooltip` + `MenuAnchor`
  assertion after Tooltip moved to `OverlayPortal`.
- Flutter #178522, open P2: `CompositedTransformFollower` in `OverlayEntry`
  assertion/regression and comment thread about manual positioning and
  `OverlayPortal.overlayChildLayoutBuilder`.
- Flutter #140534, open P2: `OverlayPortal` in `ListView` excluded from widget
  tests by sliver onstage traversal when its layout height is zero.
- Flutter #127258, open P2: selection toolbar/local-to-global assertion during
  build/layout phase.
- Flutter 3.44.0 source:
  - `.flutter/packages/flutter/lib/src/rendering/layer.dart`
  - `.flutter/packages/flutter/lib/src/rendering/proxy_box.dart`
  - `.flutter/packages/flutter/test/widgets/widget_inspector_test.dart`
  - `.flutter/packages/flutter/test/semantics/semantics_update_test.dart`

## Lessons

### Overlay Positioning Must Be a Framework-Owned Composite Primitive

The comment threads around #178522 and #187162 repeatedly circle around
manual alternatives: compute global positions from render boxes, use
`OverlayPortal.overlayChildLayoutBuilder`, delay by a frame, or insert a
separate `Overlay` to break problematic nesting. The lesson for FLUI is not
that these exact APIs should be copied. It is that anchored overlays are too
central and phase-sensitive to leave to widget authors.

An anchored popover/tooltip/menu should be derived once from the retained layer
tree after paint/composition information exists, then reused consistently for
pixels, hit testing, and semantics.

FLUI status: ADR-0015's side-table design has landed. `PipelineOwner` keeps
per-frame follower side tables, `run_paint` resolves follower correlations
after the layer tree exists, the GPU path and hit-test path consume the same
resolved offset, and hidden unlinked followers are skipped consistently. This is
a strong direction and avoids the worst manual `localToGlobal` trap.

### Offset-Only Followers Are a Future Contract Ceiling

Flutter's `FollowerLayer` resolves a full matrix by collecting transform chains
from leader and follower to their common ancestor, caches the last transform,
pushes that matrix during `addToScene`, and uses the inverted matrix for
annotation/hit testing. Its widget-inspector tests include rotated
`CompositedTransformFollower` scenes.

FLUI's current `FollowerLayer` resolves only `Offset`. The code documents that
scale/rotation between leader and follower are not representable:
`crates/flui-layer/src/link_registry.rs:405`. The GPU path applies only
`backend.push_offset(resolved)`, and the hit-test path applies only the inverse
translation.

This is internally consistent for translation, but it risks becoming a public
semantic ceiling before higher-level widgets such as menus, selection handles,
tooltips, and portals are built on top.

Filed:

- <https://github.com/vanyastaff/flui/issues/1088> — `layer: make composited
  followers matrix-based, not offset-only`

### Overlay/Portal Semantics Need Independent Transform Coverage

Flutter has a semantics regression test for nested `OverlayPortal` traversal
transforms. FLUI already has separate semantics transform issues (#1069) and
overlay/follower hit-test work, but the matrix-based follower issue should also
cover semantics once the follower primitive can represent a full transform.

Do not treat visual correctness alone as enough for overlay primitives.

## Local Evidence

- Verified `.flutter` is pinned at `3.44.0`.
- Searched existing FLUI issues for follower/matrix/rotation/scale duplicates;
  no matching existing issue found before filing #1088.
- Source inspected:
  - `crates/flui-layer/src/layer/follower.rs`
  - `crates/flui-layer/src/link_registry.rs`
  - `crates/flui-engine/src/wgpu/renderer.rs`
  - `crates/flui-rendering/src/pipeline/owner/accessors.rs`
  - `crates/flui-rendering/src/pipeline/owner/paint.rs`
  - `crates/flui-objects/src/proxy/follower.rs`

## Follow-Up

- Add a direct FLUI test with leader/follower under different rotated ancestors.
- Add a scaled-ancestor hit-test test.
- Once semantics transform plumbing is matrix-capable, add nested follower or
  portal semantics traversal coverage.
- Continue mining Flutter overlay issues for phase/lifecycle lessons around
  Tooltip, MenuAnchor, SelectionOverlay, and retained offstage portals.

Status: ANSWERED for the offset-only follower architecture question; broader
overlay/portal lifecycle audit remains open.
