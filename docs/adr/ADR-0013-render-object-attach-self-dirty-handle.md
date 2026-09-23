# ADR-0013: Render objects that drive their own pipeline work get a self-dirty handle on attach

- **Status:** Accepted
- **Date:** 2026-07-01

## Context

Some render objects must mark **themselves** dirty outside a rebuild: `RenderAnimatedSize`
drives its own animation and relays out on every tick; `RenderFlow` and `RenderCustomPaint`
repaint when their delegate's `Listenable` notifies. In Flutter each node holds its
`PipelineOwner` (`attach(owner)`), receives a `vsync:` at construction, and calls
`markNeedsLayout()` from a controller listener.

FLUI had no attach/detach hook on render objects, and a node's `RenderId` does not exist until
it is inserted, so a render object could not be given a way to mark itself. The pieces around
that seam already existed: `AnimationController` (`flui-animation`) owns and drives its own
ticker and is a `Listenable`; `RenderInvalidationHandle` (`flui-rendering`) is a
least-privilege capability that marks one node dirty through a bounded channel drained at the
start of the next frame.

## Decision

**D1 — A defaulted `attach`/`detach` pair.** `RenderBox` and `RenderSliver` have

```rust
fn attach(&mut self, handle: RenderInvalidationHandle) { let _ = handle; }
fn detach(&mut self) {}
```

forwarded through the blanket `RenderObject` impls. The owner calls `attach` right after a
node's id is assigned and its links are wired, handing a handle scoped to that attachment
interval. It calls `detach` for every node in a removed or relocated subtree. Relocation keeps
node ids but closes each attachment interval; the owner's detached-subtree token must be
consumed exactly once, either by reattaching (fresh epochs) or by releasing for unmount.
Neither method runs per frame.

**D2 — The render object never sees a ticker.** `attach` carries only the self-dirty handle —
never a scheduler, ticker provider or vsync. The owning view creates the `AnimationController`
(where every other animated widget reaches its vsync) and passes it into the render object's
constructor, mirroring Flutter's `vsync:`-at-construction. The render object treats it as a
`Listenable`. `flui-rendering` takes no dependency on `flui-scheduler` or `flui-animation`.

**D3 — A tick becomes a next-frame mark through the existing dirty channel.** The controller
ticks → notifies the listener registered in `attach` → `handle.mark_needs_layout()` (or
`mark_needs_paint()`) stamps the request with the node's attachment epoch and wakes the
platform → the next frame's `drain_pending_dirty` accepts it only if the stable id and the
current attachment epoch both match → layout reads `controller.value()`. The mark is buffered,
never pushed into an in-flight walk, so a tick cannot corrupt a running phase; a full channel
surfaces as `SendError::ChannelFull`. Requests sent before detach, while detached, or through a
handle from a previous attachment are inert.

**D4 — One mechanism for owned and external notifiers.** An owned animation
(`RenderAnimatedSize`: subscribe to its controller, mark layout) and an external notifier
(`RenderFlow`/`RenderCustomPaint`: subscribe to the delegate's `Listenable`, mark paint) are the
same shape — `add_listener` in `attach`, self-mark on notify, `remove_listener` in `detach`.
They differ only in who owns the `Listenable` and which mark they send.

`RenderInvalidationHandle` is the only public invalidation capability; the raw sender and the
request protocol stay private.

## Flutter divergence

Flutter nodes hold a `PipelineOwner` back-pointer. FLUI hands a least-privilege, epoch-scoped
handle that can mark only its own node, so there is no owner back-pointer, no lock on a node,
and no way for a stale handle to reach a re-attached node.

## Consequences

- Non-animated objects pay nothing (no-op defaults).
- A running controller self-marks every frame until it settles or its owning state disposes it
  — the same cost Flutter pays; the frame driver quiesces once all controllers settle.
- Disposing the controller is the owning state's job; `detach` only stops the subscription.

## Alternatives rejected

| Option | Why rejected |
|---|---|
| Separate mechanisms for owned animation and external notifiers | The controller already owns ticking; once it is a `Listenable`, both cases are the same code. |
| `attach` hands a scheduler/ticker provider so the node builds its own controller | Drags ticker concepts into the render layer and invites ambient scheduler access. |
| A `PipelineOwner` back-pointer on every node (Flutter's shape) | Breaks single-owner mutation and puts an owner handle on every node. |
| A per-frame animation-callback list on `PipelineOwner` | Re-invents the scheduler's frame callbacks and the dirty channel inside the render layer. |
| Hand the handle at construction | The `RenderId` does not exist until insert; the lifecycle hook is the irreducible part. |
