# Hit Testing Guide - FLUI

Hit testing records which render objects are under a pointer and how to
transform the event into each target's local coordinate space. Ordinary pointer
delivery follows Flutter's `GestureBinding.dispatchEvent` semantics: dispatch is
leaf-first, synchronous, locally transformed per entry, and every hit target
receives the event.

## Current ADR-0027 shape

`HitTestEntry` is data-only:

- `target: RenderId`
- `transform: Option<Matrix4>`
- `pointer_target: Option<PointerTarget>`
- `scroll_handler: Option<ScrollEventHandler>`
- cursor and mouse-tracker annotation metadata

Executable pointer callbacks do not live in render storage or hit-test entries.
Widgets register owner-local handlers through `RenderObjectContext`, render
objects store the returned `PointerTarget`, and dispatch resolves those targets
through the active `InteractionLane`.

## Ordinary pointer dispatch

Ordinary pointer dispatch has no stop/continue result. A `HitTestResult` is
resolved into an owner-local route, invoked leaf-first, and then released for
one-shot dispatch. `GestureBinding` uses the same resolver/invoker but caches
the resolved route from Down through Up/Cancel:

1. Down: hit test, resolve route, invoke targets, close gesture arena.
2. Move: reuse the cached route.
3. Up: invoke cached route, sweep arena, release route.
4. Cancel: invoke cached route so recognizers reject themselves, release route;
   the binding does not sweep and force a winner.

This preserves Flutter's retained hit-target behavior while keeping render data
`Send + Sync`. If a target unmounts after Down, new hit tests will miss it, but
the active route keeps the owner-local handler cell alive until Up/Cancel.

Per-target panics are isolated: later targets still receive the event, cleanup
runs, then the first panic is resumed by the dispatch owner.

## Scroll / pointer-signal dispatch

`EventPropagation` is scroll-only. Pointer-signal/scroll handling remains a
separate claiming resolver where a scroll handler may return `Stop` to claim the
signal. Do not use `EventPropagation` for ordinary pointer delivery.

## Transform support

`HitTestResult` maintains a transform stack. It accumulates the
GLOBAL-TO-LOCAL mapping as the walk descends, so each level must push the
INVERSE of its own forward (paint-direction) offset/matrix — prefer the
scope helpers, which invert and pop for you:

```rust
use flui_interaction::prelude::*;
use flui_types::geometry::{Matrix4, Offset};

let mut result = HitTestResult::new();

// `with_paint_offset` takes the forward paint offset and pushes its
// inverse (negated) internally.
result.with_paint_offset(Offset::new(10.0.into(), 20.0.into()), |result| {
    child.hit_test(position, result);
});

// `with_paint_transform` takes the forward paint matrix and pushes its
// inverse internally (falling back to the singular forward matrix if the
// transform is not invertible — see its doc).
let rotation = Matrix4::rotation_z(std::f32::consts::PI / 4.0);
result.with_paint_transform(rotation, |result| {
    child.hit_test(position, result);
});
```

`push_offset`/`push_transform` are the raw primitives underneath — they push
exactly what they are given, no inversion, and the caller is responsible for
negating/inverting before calling them. Reach for them directly only when the
scope-helper's closure shape does not fit; `with_paint_offset`/
`with_paint_transform` are correct by construction and should be preferred.

Each entry captures the current (already-inverted) transform. During dispatch
the event is transformed into that entry's local coordinate space.
Non-invertible transforms compose to a singular matrix and are skipped at
delivery.

### Sliver child transforms

The sliver walk (`PipelineOwner::hit_test_subtree_impl`/
`hit_test_sliver_subtree_impl`, `crates/flui-rendering/src/pipeline/owner/accessors.rs`)
descends through both sliver→sliver and sliver→box edges. Each edge decides
independently whether to push a child offset onto the `HitTestResult`
transform stack, governed by one rule: **push iff the position handed to the
child was moved into the child's own frame.**

- Sliver→sliver, ordinary path: the position is converted from the parent's
  main-axis coordinates into the child's via
  `sliver_hit_position_minus_paint_offset`, so it pushes.
- Sliver→sliver, override path: an override caller (the sole supplier today
  is `RenderSliverOffstage::hit_test`, a transparent passthrough) already
  hands back a sliver-local position and never repositions its child, so
  nothing is pushed — asserted by a `debug_assert!` on the child's committed
  offset being `Offset::ZERO`, so a future supplier with a nonzero offset
  fails loudly instead of silently delivering a wrong position.
- Sliver→box, both paths: `box_hit_offset_from_sliver_position` always
  decomposes the main-axis position into box-local coordinates, override or
  not, so both push.

This matches Flutter, which has no override-position concept for slivers at
all — every `hitTestChildren` override both repositions and pushes in the
same call: `RenderSliverHelpers::hitTestBoxChild` and
`RenderSliverPadding::hitTestChildren` (`rendering/sliver.dart`,
`rendering/sliver_padding.dart`) always route through
`SliverHitTestResult::addWithAxisOffset`, which unconditionally pushes
`paintOffset` when it is non-null.

## HitTestBehavior

`HitTestBehavior` controls whether a render object contributes itself to the hit
path and whether it blocks targets visually behind it:

- `DeferToChild`: contribute only if a child was hit.
- `Opaque`: contribute within bounds and block siblings behind it.
- `Translucent`: contribute within bounds without blocking siblings behind it.

Typical render-object hit testing still checks children before self so the path
is leaf-first.

## Ctx-level transform pushes (box protocol)

`BoxHitTestContext`'s `push_offset`/`push_transform`/`with_transform`/
`with_offset` (`crates/flui-rendering/src/context/hit_test.rs`) let a render
object record a FORWARD (paint-direction) transform before recursing into a
child via `hit_test_child`/`hit_test_child_at_layout_offset`. That push used
to feed a per-node `BoxHitTestCtx` stack that `hit_test_raw`
(`crates/flui-rendering/src/traits/render_box.rs`) discarded when the
context went out of scope, so `RenderFractionalTranslation`
(`crates/flui-objects/src/layout/fractional_translation.rs`) and
`RenderFlow` (`crates/flui-objects/src/layout/flow.rs`) delivered
un-localized positions.

The ctx now hands its accumulated forward transform to the driver's
`HitTestChildCallback` on every `hit_test_child`/`hit_test_child_at_layout_offset`
call (`crates/flui-rendering/src/protocol/box_protocol.rs`,
`BoxHitTestCtx::local_transform_for_driver`); the driver pushes its inverse
onto the SAME `HitTestResult` this document describes, scoped to that one
recursive call, via `with_paint_transform` — the identical mechanism the
walk already uses for `hit_test_transform` and resolved follower offsets
(`crates/flui-rendering/src/pipeline/owner/accessors.rs`). The sliver
protocol's ctx-level stack stays a permanent no-op (main-axis position
covers its needs); its own transform-stack gap (sliver→box edges skipping
the push) was fixed separately — see "Sliver child transforms" above.

Two more gaps in the same family, found by auditing every caller of every
`hit_test_child*` variant once the mechanism above started actually
reaching the driver:

- `HitTestContext::hit_test_child_at_offset` (the box-specific convenience
  wrapper that subtracts a caller-supplied offset before delegating to
  `hit_test_child`) computed the child-local position but never pushed the
  offset it consumed — a bug in the wrapper itself, distinct from the
  `BoxHitTestCtx`-discarded-on-scope-exit bug fixed above. Its only
  nonzero-offset caller, `RenderFractionallySizedBox::hit_test`
  (`crates/flui-objects/src/layout/fractionally_sized_box.rs`), delivered
  un-localized positions to a fractionally-sized, off-center-aligned child.
  Fixed by pushing the consumed offset unconditionally (mirroring
  `hit_test_child_at_layout_offset`): the method always moves the position
  into the child's frame, so per the rule above it must always push.
- `RenderFittedBox::hit_test` (`crates/flui-objects/src/layout/fitted_box.rs`)
  computed a child position through the inverse of its own scale/align
  matrix and called the raw `hit_test_child` directly, recording nothing —
  it has no `hit_test_transform` override (the driver-level mechanism
  `RenderTransform`/`RenderRotatedBox` use) and never pushed a ctx-level
  transform either (the mechanism `RenderFlow` uses). Fixed by wrapping the
  child call in `ctx.with_transform`, the same pattern `RenderFlow` already
  used.

## Tests

Useful focused checks:

```bash
cargo test -p flui-interaction hit_test
cargo test -p flui-interaction interaction_lane
cargo test -p flui-interaction down_caches_route_and_up_delivers_after_target_unregisters
```
