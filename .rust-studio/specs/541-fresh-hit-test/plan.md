# 541 (second half) — a fresh hit test for drag-target discovery

The slop half shipped in #892. This is the other one: "DragTarget discovery needs
a fresh hit test at the current global pointer position on every move,
independent of the pointer-down route. Widgets cannot request that today."

## Why nothing works today

`crates/flui-widgets/src/interaction/draggable.rs`'s module doc already states
the gap precisely, and it is worth not re-deriving:

> FLUI's pointer dispatch — both the production path and the widget test
> harness's arena-scoped helper — resolves the hit-test path **once, at
> `PointerDown`**, and replays that *cached* route for every subsequent
> `Move`/`Up`. There is no capability reachable from widget or gesture-callback
> code to run a fresh, arbitrary-position hit test later.

Consequence today: `Draggable`'s gesture lifecycle is real, but **no target is
ever discovered**, so every drag ends in `on_draggable_canceled`, never
`on_drag_completed`, and `DraggableDetails::was_accepted` is always `false`.
`DragTarget`'s accept/candidate/reject/leave protocol is implemented and tested
against its own state machine, never wired to a live drag.

The reference does this in `_DragAvatar.updateDrag`: an ad hoc
`WidgetsBinding.instance.hitTestInView` at the pointer's *current* global
position on every move, then a walk of the result for `RenderMetaData`-tagged
targets.

## The layering constraint that shapes the design

`HitTestResult` lives in **flui-interaction**, whose only FLUI dependencies are
flui-types, flui-foundation, and flui-platform. It cannot see
`PipelineOwner::hit_test` (flui-rendering, one layer up), and the dependency
must not be inverted to give it one.

So the capability is defined low and **installed from above** — the shape
`TextInputHandle` already uses (`TextInputOwner` is presentation-owned; flui-app
installs a weak handle into the build owner during realm construction).

## Design

**Reuse `InteractionDispatchHandle`'s validation, do not invent a second set.**
That handle already rejects `WrongThread`, `WrongRealm`, `InactiveRealm`, and
`OwnerGone` against a `LaneTicket` — which is exactly the issue's "reject
cross-realm and stale-presentation use". A new capability with its own staleness
rules would be a second, subtly different answer to a question already answered.

1. **flui-interaction** — a narrow installable probe:
   - `trait HitTestProbe { fn probe(&self, position, result: &mut HitTestResult); }`
   - the lane holds `Option<Rc<dyn HitTestProbe>>`;
   - `HitTestSnapshot` — an **owned** `Vec<HitTestEntry>` plus the position it
     was taken at. Owned is the contract the issue asks for ("valid for
     immediate synchronous dispatch only"): nothing borrows the render tree, so
     holding one across a frame is possible but useless, not unsound.
2. **flui-view** — `BuildContext::hit_test_handle() -> Option<HitTestHandle>`.
   A **narrow newtype**, not `InteractionDispatchHandle` itself: widgets need
   `hit_test_at`, and handing them the whole registration lane (register/
   unregister pointer, mouse region, scroll, pan-zoom, path clipper, shader
   mask) would be a much larger surface than the capability requires.
3. **flui-app** — the presentation installs the probe over its
   `root_pipeline_owner`, alongside where it already calls
   `set_text_input_handle`.
4. **The guard, in the same change.** `hit_test_handle` joins the nine tokens in
   `scripts/check-frame-capability-scope.sh` (`capabilities=` and the
   rejected-fixture loop), per AGENTS.md's rule that adding a capability means
   adding its token in the same commit. Acquisition is `init_state` /
   `did_change_dependencies`; a hit test from inside `build`/layout/paint is the
   trigger-#22 hazard, since discovery mid-frame reads a tree the frame is still
   mutating.

## Slice B — wire it

`_DragAvatar.updateDrag` equivalent on the live `Draggable` session: fresh probe
per move, walk for targets, enter/move/leave/drop. Acceptance criteria from the
issue that land here: nested/overlapping transitions, target removal mid-drag,
multiple simultaneous drags isolated, transform- and clip-aware local positions.

This slice can also close the **`_lastOffset` divergence** the draggable module
doc names: the reported offset is currently displacement-since-drag-start, where
the reference reports `globalOrigin + displacement`. That needs `local_to_global`
from the same binding-internal layer this capability opens up, so it belongs
here rather than staying an open Cross.H item. `draggable_test.rs`'s
`reported_offset_is_displacement_not_global_position` pins the *current*
behaviour and must be rewritten, not deleted, when the base changes.

## Not in scope

`LongPressDraggable` (needs `DelayedMultiDragGestureRecognizer`, which does not
exist), `dragAnchorStrategy` selection, `affinity`, `hitTestBehavior`,
`rootOverlay`, `ignoringFeedback*`. All are named deferrals in the draggable
module doc and none is blocked on this capability.

---

## Slice B scope, surveyed

The discovery mechanism is mostly ported already; what is missing is the wiring
(the dominant defect shape in this repo).

**Present:** `flui_objects::RenderMetaData` — a single-child proxy carrying
`MetaDataPayload = Arc<dyn Any + Send + Sync>`, with `metadata_as::<T>()` and a
`HitTestBehavior`. That is Flutter's `RenderMetaData`, the exact mechanism
`_DragAvatar.updateDrag` walks the hit path looking for.

**Absent:**

1. **No `MetaData` widget.** `RenderMetaData` has no widget wrapper anywhere in
   `flui-widgets` — nothing in the framework can produce one. `DragTarget` needs
   to wrap its child in one carrying a handle to its own state.
2. **No payload in the snapshot.** `HitTestSnapshot` carries `HitTestEntry`,
   whose `target` is a bare `RenderId`. Resolving that back to a render object
   needs the `PipelineOwner` — which widget code reaches only through
   `pipeline_owner()`, itself a guarded frame capability.

   The fix belongs in the probe, not a second lookup: it already holds the tree
   borrowed while walking, so it can collect the metadata payloads it passes and
   hand them back in the snapshot. That keeps the capability's contract intact
   (owned, synchronous, no borrow escaping) and avoids widgets needing the
   pipeline at all.
3. **The `_lastOffset` divergence.** `DraggableDetails.offset` reports
   displacement-since-drag-start where the reference reports
   `globalOrigin + displacement`. `local_to_global` is on `PipelineOwner`
   (`accessors.rs:304`), so this becomes reachable through the same probe seam.
   `draggable_test.rs`'s `reported_offset_is_displacement_not_global_position`
   pins the CURRENT behaviour and must be rewritten, not deleted.

**Order:** (1) `MetaData` widget + payloads in the snapshot; (2) `DragTarget`
tags itself, `DragSession::update` probes per move and drives
enter/move/leave; (3) `end` resolves the drop and finally lets
`on_drag_completed` / `was_accepted` be true; (4) the offset base, with its
pinning test rewritten.

**Acceptance criteria this closes** (from the issue): nested/overlapping target
transitions, target removal mid-drag, multiple simultaneous drags isolated, and
transform/clip-aware local positions — the last coming free from `HitTestEntry`'s
existing per-entry transform.
