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

**Why, and why the probe survived the dispatch fix.** Pointer dispatch now
carries both spaces: a `Listener` callback receives a `PointerDispatch` whose
`global` half is the platform's own value, never re-derived from a transform.
That closed the general defect — but it did not reach this widget, because
`Draggable` does not consume pointer events directly. It feeds them to a
`MultiDragGestureRecognizer`, and the `GestureRecognizer` contract
(`add_pointer`, `handle_event`) still carries a *single* space, as does every
`Drag*Details` struct it produces. `DragSession`'s accumulated position, its
axis restriction, and `to_global` are all built on that one space being the
`Listener`'s. Handing the recognizer the global event instead would make
`global_position` truthful and `local_position` a lie — the same defect
relabelled — so the probe stays until the recognizers carry the pair.

**What the recognizers need, so the follow-up is not re-derived from scratch:**
Flutter's answer is `OffsetPair` (`gestures/events.dart`), which
`DragGestureRecognizer` threads through `_initialPosition` and `_lastPosition`
(`gestures/monodrag.dart:417`, `:686`) and hands to every detail struct; the
velocity tracker samples the *local* half (`:664`) and deltas are mapped to
global via `PointerEvent.transformDeltaViaPositions`. Porting that means the
trait signatures, `RecognizerBase`'s tracked position, and each of the ten
recognizers' internal position plumbing — a change of its own size, with its
own per-recognizer parity evidence.

**Alternatives rejected:**

- Reading `DragUpdateDetails::global_position` — that field is still fed the
  already-localized value, so it is a global position in name only. It is the
  named remaining half of issue #908.
- Assuming translation-only ancestors and adding a remembered origin — wrong
  under any `Transform`, and wrong silently.
- Stashing the `Listener`'s `dispatch.global` in a cell and having
  `DragSession::to_global` read it instead of converting — exact only under
  translation-only ancestors, because the session converts its *accumulated,
  axis-restricted* position rather than the raw event position. That trades
  exactness under scale and rotation for exactness across a mid-contact
  transform change, which is not a clear win, and it fixes only this widget
  while `GestureDetector`'s drag details keep lying.

**Consequences named rather than left to be discovered:**

- A drag carrying no data discovers nothing. The oracle's null-data drag enters
  every target; `ErasedDragData` erases a concrete value, not an `Option`, so
  that state has no representation here.
- `axis` restriction applies to deltas in the `Listener`'s space rather than the
  root's, which differs from the oracle only under a rotating ancestor.
- **A transform that changes mid-contact is still converted inconsistently.**
  Pointer dispatch localizes with the `HitTestEntry` transform captured in the
  route resolved at `PointerDown`, while `local_to_global` converts with the
  tree's *current* transform. A frame that moves or scales the `Listener`
  between two moves therefore has the drag convert a stale local point through
  a fresh matrix, and the probe lands off the pointer until the contact ends.
  The value that fixes this now exists and is proven correct at the dispatch
  boundary — `a_mid_contact_transform_change_does_not_move_the_reported_global_position`
  in `tests/parity/pointer_local_position_test.rs` pins it — but it stops at
  the `Listener`, one layer above where this widget reads its position. Not
  worked around.

**Replacement tests:** group 4 of `tests/parity/draggable_test.rs` drives real
pointer input across a tree where the draggable and the targets are at
different offsets, so a local-position implementation enters targets the
pointer was never over.
