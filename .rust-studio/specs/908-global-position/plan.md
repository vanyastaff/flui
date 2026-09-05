# #908 — carry the global pointer position through dispatch

## The defect

All four drag-detail structs in `crates/flui-interaction/src/recognizers/drag.rs`
assign the **same** value to `global_position` and `local_position`
(lines 403-404, 458-459, 496-497, 512-513), and that value is the **local**
one. `ResolvedHitRoute::invoke` (`routing/interaction_lane.rs:497`) rewrites
each event into the receiving entry's space via `transform_pointer_event`
before calling the handler, and `PointerEvent` is a `ui_events` type with room
for exactly one position — unlike Flutter's, which carries `position` *and*
`localPosition`.

So `global_position` is correct only under an identity ancestor chain and
silently wrong by the composed transform everywhere else. ~10 read sites.

Measured: draggable at (150, 150), pointer at global (160, 160), callback
receives `Offset { dx: 10px, dy: 10px }`.

## Why now

Two things already in `main` are shaped around the absence:

- `Draggable` mounts a payload-free `DragOrigin` probe purely to learn a
  `RenderId` so it can call `PipelineOwner::local_to_global`
  (`flui-widgets/ARCHITECTURE.md` mapping decision 3). That workaround exists
  only because dispatch discards the global position.
- A deferred correctness gap recorded under the same decision: a frame that
  changes the `Listener`'s transform mid-contact leaves dispatch localizing
  with the transform captured at `Down` while the probe converts with the
  tree's *current* transform, so the recovered global point is shifted.

Carrying the raw platform position fixes the second outright — the value is
never re-derived from a transform that may have moved — and removes the need
for the first.

## Blast radius (verified, not estimated)

| Layer | What changes |
|---|---|
| `flui-interaction/routing/interaction_lane.rs:287` | private `type PointerHandler = Rc<dyn Fn(&PointerEvent)>` |
| same, `:943` and `:960` | two registration fns taking `impl Fn(&PointerEvent)` |
| same, `:497` `ResolvedHitRoute::invoke` | the one dispatch call site |
| `flui-widgets/interaction/listener.rs` | `Listener`'s six public `on_pointer_*` callbacks + `handler()` |
| `flui-interaction/recognizers/drag.rs` | the four detail structs get a truthful `global_position` |
| `flui-widgets/interaction/draggable.rs` | drop `DragOrigin` and `to_global` |

**Not affected:** the render layer. `RenderPointerListener` only carries a
data-only `PointerTarget` token (ADR-0027) and never sees an event, so
`flui-objects` and `flui-rendering` need no change. Confirmed by reading
`flui-objects/src/interaction/listener.rs` — no `PointerEvent` in the file.

## Design

Change the handler argument from `&PointerEvent` to a borrowed context:

```rust
pub struct PointerDispatch<'a> {
    /// The event in the receiving node's local space — what the handler
    /// previously received, unchanged.
    pub local: &'a PointerEvent,
    /// The same event as the platform delivered it, untransformed.
    pub global: &'a PointerEvent,
}
```

Both borrowed from values `invoke` already owns, so no clone is added to the
dispatch path. `LocalEventTransform::Global` entries pass the same reference
twice; `NonInvertible` entries are skipped as they are today.

**Why a struct rather than a second parameter:** it leaves room for the entry
transform without another breaking change, and a named field at each call
site is harder to mix up than two same-typed positional arguments.

Breaking, and permitted. Prefer the reshape over an additive
`on_pointer_down_with_global` sibling — a shim would leave the lying field in
place, which is the actual defect.

## Decide, don't assume

**How recognizers receive events has NOT been established.** `drag.rs` builds
its details from an event it gets from somewhere — the gesture arena or the
pointer router, which may be a different path from `PointerHandler`. Read that
path first. If recognizers do not go through `ResolvedHitRoute::invoke`, the
detail-struct fix needs its own route for the global position and that changes
the shape of this work. Do not guess.

`GlobalPointerHandler` (`routing/pointer_router.rs:49`) is a separate,
already-global type — check whether it should stay as it is.

## Acceptance

1. `global_position` differs from `local_position` under a non-identity
   ancestor, and equals the platform value. Red test: a `Listener` under a
   `Transform`, asserting the two fields are *different* and naming which is
   which — a test at the origin cannot tell them apart.
2. A transform change mid-contact does not shift the reported global position
   (the deferred gap from #909).
3. `Draggable` keeps every behavior its parity suite pins, with `DragOrigin`
   removed. The whole group-4 discovery suite is the oracle.
4. `just gate` passes — including `doc-strict`, which clippy and tests do not
   cover.
