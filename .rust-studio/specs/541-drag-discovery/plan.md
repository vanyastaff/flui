# #541 — live drag-target discovery (the second half)

The pointer-kind slop half of #541 merged. This is the remaining half:
"Dragging across nested and overlapping targets emits Flutter-equivalent
transitions."

## What already exists (verified in-tree, all merged)

| Piece | Location | State |
|---|---|---|
| `RenderObject::metadata()` + `HitTestEntry.metadata` | `flui-interaction/src/routing/hit_test.rs:140` | live payload channel on every hit entry |
| `RenderMetaData` publishing through it | `flui-objects/src/interaction/meta_data.rs:204` | live |
| `MetaData` widget | `flui-widgets/src/interaction/meta_data.rs` | live |
| `BuildContext::hit_test_handle()` | `flui-view/src/context/build_context.rs:172` | live, wired at `flui-app/src/app/presentation.rs:472` |
| `PipelineHitTestProbe` (fresh test at a global position) | `flui-rendering/src/pipeline/hit_test_probe.rs` | live, realm- and liveness-checked |
| frame-capability scope guard covers `hit_test_handle` | `scripts/check-frame-capability-scope.sh:102` | live |
| `did_enter`/`did_move`/`did_leave`/`did_drop` state machine | `flui-widgets/src/interaction/drag_target.rs:209-311` | implemented, tested via `tests/parity/draggable_test.rs` |

Everything below the wiring layer is built and tested.

## The gap

Two edges are missing, and they are the only two:

- **A.** `DragTargetState::build` (drag_target.rs:348) returns `(view.builder)(..)`
  bare. It never wraps that child in a `MetaData`, so a hit test can never
  discover the target — nothing publishes a payload.
- **B.** `Draggable` never calls `hit_test_handle()`. Its module doc
  (divergence #2, draggable.rs:27-45) states no such capability is reachable
  from widget code. **That doc is now stale** — the capability landed with the
  probe. The doc must be rewritten in the same change, not left contradicting
  the code.

Consequence today: the transition methods are real and correct but no
production path reaches them. This is the repo's dominant defect class
(`shipped-seams-never-wired`), here in its documented form.

## Design decision — the slot, and the bound that forces it

Flutter's `_getDragTargets` walks `result.path` for `RenderMetaData` whose
`metaData is _DragTargetState`, then calls `didEnter` on that state object
directly. Dart can do this because the payload is a GC'd reference to the live
State, and because `_DragTargetState` holds its own widget.

FLUI cannot transcribe that, for two independent reasons found while scoping:

1. `MetaDataPayload = Arc<dyn Any + Send + Sync>`, but `DragTarget`'s
   callbacks are `Rc<dyn Fn ...>` (drag_target.rs:113-153) — **not**
   `Send + Sync`. A payload carrying the state cannot carry the callbacks.
2. The transitions take `&DragTarget<T>` — the *view* — as a parameter,
   because the callbacks live on the view, not the state. A payload reaching
   only the state cannot invoke them at all.

**Chosen shape: a shared `DragTargetSlot`.** The target publishes
`Arc<DragTargetSlot>` as its metadata payload. The slot owns the `entered`
list behind a `Mutex` and holds the callbacks; the element refreshes the
callbacks into the slot on each build, so a rebuilt view's callbacks are the
ones a later transition invokes. Transitions move from
`(&mut state, &view)` onto the slot, and `DragTargetState` reads its
candidate/rejected lists from the slot when it builds.

This requires **changing `DragTarget`'s callbacks from `Rc<dyn Fn>` to
`Arc<dyn Fn + Send + Sync>`** — a breaking public-API change, permitted, and
it makes `DragTarget` consistent with `Draggable`, whose callbacks are already
`Send + Sync + 'static` (draggable.rs:273).

Why this is better than the oracle's shape, not merely different: the veto
(`on_will_accept`) stays **synchronous**, which a deferred
drain-on-next-build queue would lose — `Draggable` must know at move time
which targets are candidates. And the slot makes the target's cross-thread
contract explicit in the type system instead of resting on GC.

Owed under Prime Directive rule 1: a `## Mapping decisions` entry in
`crates/flui-widgets/ARCHITECTURE.md` recording the slot and the `Arc`/`Send`
bound change, with the replacement tests below.

## Acceptance criteria (from the issue, mapped to tests)

Each needs a test that fails with the wiring reverted.

1. Nested and overlapping targets emit Flutter-equivalent transitions —
   leaf-first path order; enter/leave fire at the boundary crossing, once.
2. Target removal mid-drag is safe — the slot outlives the element via `Arc`;
   a dropped target receives no further transitions and produces no panic.
3. Multiple simultaneous drags stay isolated — keyed by `PointerId`, which
   the `entered` list already is.
4. Transform- and clip-aware targets get correct local positions — use
   `HitTestEntry.transform`, never the raw global offset.
5. Stale/cross-realm probe use is refused — already covered by the probe's own
   `OwnerGone`/`TreeBusy` tests; assert `Draggable` treats `TreeBusy` as
   "no change", NOT as an empty path (an empty path reads as "over nothing"
   and would fire a spurious leave on every target).

## Out of scope

`dragAnchorStrategy`, `rootOverlay`, `ignoringFeedback*`, and the
feedback global-origin term (draggable.rs divergences #1 and #4) stay open —
they are positioning gaps, independent of discovery.
