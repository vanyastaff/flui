# flui-widgets architecture

The user-facing widget catalog: configuration objects over the `flui-objects`
render catalog, plus the stateful widgets that own gesture, focus, routing and
overlay behavior. Layer rules, dependency direction, and the crate's place in
the workspace DAG live in [`docs/FOUNDATIONS.md`](../../docs/FOUNDATIONS.md)
and [`docs/workspace-layers.toml`](../../docs/workspace-layers.toml); this file
records the per-widget decisions that diverge from the Flutter reference and
would otherwise read as drift.

Cross-crate protocol decisions belong in an ADR (`docs/adr/`). What belongs
here is a decision local to this crate: a widget's internal shape, a callback
bound, a payload type.

## Mapping decisions

### 1. `DragTarget` publishes a shared `DragTargetSlot`, not its `State`

**Rule:** [`AGENTS.md`](../../AGENTS.md) Prime Directive #1 — behavior is the
floor, structure is designed for Rust; every divergence names what is better,
replaces the oracle's test, and drops no edge case by accident.

**Oracle:** `widgets/drag_target.dart`. `_DragTargetState.build` wraps its
child in `MetaData(metaData: this)`, and `_DragAvatar._getDragTargets` walks
the hit path for a `RenderMetaData` whose `metaData` *is* a
`_DragTargetState`, then calls `didEnter`/`didMove`/`didLeave`/`didDrop` on
that object directly. Dart can do this because the payload is a GC'd reference
to the live `State`, and because a `State` can reach `widget` for the
callbacks.

**Choice:** the payload is an `Arc<DragTargetSlot>` — a non-generic object that
owns the entered list (keyed by `PointerId`), holds the current build's
callbacks type-erased behind a private `TargetCallbacks` trait, and carries the
target element's `RebuildHandle` so a transition can schedule the rebuild the
oracle gets from `setState`. `DragTargetState<T>` keeps only the slot and reads
its candidate/rejected lists back out of it; `build` refreshes the callbacks
into the slot so a rebuilt view's closures are the ones a later transition
invokes.

**Why the oracle's shape does not transcribe.** Two independent reasons, both
structural:

- A hit-test payload is `Arc<dyn Any + Send + Sync>` (`HitTestEntry::metadata`).
  A `DragTargetState` holding `Rc<dyn Fn …>` callbacks could not be one.
- FLUI's callbacks live on the *view*, which the state does not own. A payload
  reaching only the state could not invoke them at all.

**Consequences:**

- **`DragTarget`'s four transition callbacks change from `Rc<dyn Fn …>` to
  `Arc<dyn Fn … + Send + Sync>`** — a breaking public-API change. It makes
  `DragTarget` consistent with `Draggable`, whose callbacks already carried
  those bounds, and it puts the target's cross-thread contract in the type
  system instead of resting on GC. The *builder* stays `Rc`: it produces a
  `BoxedView`, which is owner-local by construction, and it is only ever called
  from `build`.
- The veto (`on_will_accept`) stays **synchronous**, which a deferred
  drain-on-next-build queue would have lost — a drag has to know at move time
  which targets are candidates.
- The slot outlives its element by `Arc`. A target that leaves the tree
  mid-drag is `retire`d in `dispose`, and every later transition is a no-op —
  the oracle's `if (!mounted) return;` in a form that cannot be forgotten at one
  call site. One deliberate improvement rides on this: `did_drop` on a retired
  slot returns `false`, so the drag reports the drop as *not* accepted, where
  the oracle's `finishDrag` records `wasAccepted = true` even though its
  `didDrop` returned early. Covered by
  `a_target_removed_mid_drag_receives_nothing_further_and_accepts_nothing`.
- `DragTarget` tags itself `HitTestBehavior::Translucent`, the oracle's own
  default `hitTestBehavior`. Making it configurable stays a named deferral.

**Replacement tests:** the whole `DragTargetSlot` protocol group in
`tests/parity/draggable_test.rs` (group 2), plus the live-discovery group
(group 4) which pins the enter/move/leave/drop ordering for nested and
overlapping targets against `_DragAvatar.updateDrag`'s own rules.

### 2. `DragTargetDetails` carries a target-local position as well as a global one

**Oracle:** `DragTargetDetails.offset` is a global position and nothing else. A
Dart target that wants a local one calls `globalToLocal` on its own render
object.

**Choice:** `offset` keeps the oracle's global meaning; `local_offset` adds the
same point mapped through the hit entry's own global-to-local transform.

**Why:** FLUI callback code cannot reach a render object, so the oracle's
escape hatch does not exist here — a target would have had *no* way to learn
where the drag is in its own space. Taking the value from
`HitTestEntry::transform` composes the entire ancestor chain, so it is exact
under scale and rotation, where subtracting a remembered origin is not.

**Replacement test:**
`a_transformed_target_is_told_the_drag_position_in_its_own_space`, whose
expected value is reachable only by composing the real transform.

### 3. `Draggable` recovers its global position through a private origin probe

**Oracle:** `_DragAvatar.updateDrag` hit-tests at `globalPosition +
feedbackOffset` on every move. Flutter's `PointerEvent` carries `position`
(global) *and* `localPosition`, so a widget always has both.

**Choice:** `Draggable` mounts a payload-free `DragOrigin` view as its
`Listener`'s direct child. That view's `find_render_object()` resolves to the
`Listener`'s own render node, and the drag converts its
`Listener`-local pointer positions to the root's space with
`PipelineOwner::local_to_global` before probing.

**Why:** FLUI's pointer events are `ui_events` types with room for exactly one
position, and dispatch rewrites it into the receiving entry's local space
(`HitTestResult`'s per-entry `transform_pointer_event`). A gesture callback
therefore knows only where the pointer is inside its own node, and a hit test
needs the root's space. `local_to_global` is the sanctioned conversion between
exactly those two spaces; the only missing input was the `RenderId` it keys
on, and a probe mounted under the `Listener` is the cheapest way to learn it.
The probe contributes no render node, so the mounted render tree keeps the same
shape.

**Alternatives rejected:**

- Reading `DragUpdateDetails::global_position` — that field is fed the
  already-localized value, so it is a global position in name only.
- Assuming translation-only ancestors and adding a remembered origin — wrong
  under any `Transform`, and wrong silently.
- Preserving the global position through dispatch (a second handler parameter,
  or an ambient per-entry dispatch context) — the right long-term fix and what
  the oracle effectively has, but a change to `flui-interaction`'s pointer
  handler contract that ripples through `flui-view`, `flui-objects` and every
  `Listener` consumer. It deserves its own design record rather than arriving
  as a side effect of wiring one widget.

**Consequences named rather than left to be discovered:** a drag carrying no
data discovers nothing (the oracle's null-data drag, which enters every target,
has no representation — `ErasedDragData` erases a concrete value, not an
`Option`); and `axis` restriction applies to deltas in the `Listener`'s space
rather than the root's, which differs from the oracle only under a rotating
ancestor.

**Replacement tests:** group 4 of `tests/parity/draggable_test.rs` drives real
pointer input across a tree where the draggable and the targets are at
different offsets, so a local-position implementation enters targets the
pointer was never over.
