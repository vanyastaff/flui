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

`HitTestResult` maintains a transform stack:

```rust
use flui_interaction::prelude::*;
use flui_types::geometry::{Matrix4, Offset};

let mut result = HitTestResult::new();

result.push_offset(Offset::new(10.0.into(), 20.0.into()));
child.hit_test(position, &mut result);
result.pop_transform();

let rotation = Matrix4::rotation_z(std::f32::consts::PI / 4.0);
result.push_transform(rotation);
child.hit_test(position, &mut result);
result.pop_transform();
```

Each entry captures the current transform. During dispatch the event is
transformed into that entry's local coordinate space. Non-invertible transforms
skip that entry.

## HitTestBehavior

`HitTestBehavior` controls whether a render object contributes itself to the hit
path and whether it blocks targets visually behind it:

- `DeferToChild`: contribute only if a child was hit.
- `Opaque`: contribute within bounds and block siblings behind it.
- `Translucent`: contribute within bounds without blocking siblings behind it.

Typical render-object hit testing still checks children before self so the path
is leaf-first.

## Tests

Useful focused checks:

```bash
cargo test -p flui-interaction hit_test
cargo test -p flui-interaction interaction_lane
cargo test -p flui-interaction down_caches_route_and_up_delivers_after_target_unregisters
```
