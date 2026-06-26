# Animation heap-scheduling — listenable ticks reach the build heap

Status: **SHIPPED** (2026-06-26). Branch `core1-widgets-slice`.

## The blocker

`AnimatedView` (Flutter's `AnimatedWidget`) subscribes to a `Listenable` (an
animation) and is meant to rebuild when it changes. But the wiring was inert:
`AnimatedBehavior::on_mount` registered `ElementCore::create_mark_dirty_callback`,
which only did `dirty.store(true)` on an `Arc<AtomicBool>`. It never pushed the
element onto the heap `BuildOwner::build_scope` drains, so `notify_listeners`
flipped a flag that nothing read — animations never rebuilt.

The hard part: the listener callback fires from `notify_listeners` *between*
frames, with **no `&mut BuildOwner` in scope**, and in production `BuildOwner` is
a direct field inside the `WidgetsBinding` singleton (not `Arc<RwLock>`), so the
callback can't reach `schedule_build_for` directly.

## Mechanism (all additive)

- `BuildOwner.external_inbox: Arc<Mutex<HashSet<ElementId>>>` — a shared inbox of
  ids scheduled from outside a frame. A SET so repeated ticks dedup (bounded
  growth even if the frame driver stalls).
- `on_build_scheduled` changed `Box<dyn Fn>` → `Arc<dyn Fn + Send + Sync>` so it
  can be cloned into a scheduler handle as the frame-request hook.
- `ExternalBuildScheduler { inbox, request_frame }` with `schedule(id)` =
  insert into the set, and fire `request_frame` ONLY on the empty→present
  transition (one frame request per burst).
- `ElementCore.external_scheduler: Option<ExternalBuildScheduler>` captured at
  `mount` from the `ElementOwner`. `create_mark_dirty_callback` captures it +
  `self_id`; when fired: `dirty.store(true)` + `scheduler.schedule(self_id)`.
- `build_scope` drains the inbox onto its dirty heap at the very start, looking
  up each id's **tree depth** from the node (`tree.get(id).depth`) — NOT the
  callback-captured value, because `ElementCore::depth` is the sibling slot
  index, not `parent_depth + 1`.

Production path is fully wired: `request_frame` = the binding's
`on_build_scheduled` Arc → `handle_build_scheduled` → `on_need_frame` → platform
schedules a frame → `draw_frame` → `build_scope` drains the inbox → rebuild.
Headless tests pass `request_frame = None` and drive `build_scope` directly.

## Verification

- Red→green test `animation_notify_schedules_rebuild_through_build_scope`
  (view/animated.rs): mounts a `CountingAnimatedView`, asserts `notify_listeners`
  + `build_scope` advances the build count (was stuck before the wiring).
- `nested_animation_notify_reschedules_at_correct_tree_depth`: the same at tree
  depth >= 1, guarding the depth-lookup against a slot-capture regression.
- Workspace nextest 2931 passed / 2 skipped; fmt + clippy clean; port-check clean.

## Adversarial review (harsh findings, all addressed)

- **P1 (kieran)**: callback captured `ElementCore::depth` (= slot, not tree depth)
  → wrong dirty-heap order. FIXED by looking up `node.depth` at drain time.
- **HIGH (async)**: `Vec` inbox grew unbounded per tick. FIXED with a `HashSet`.
- **MEDIUM**: `request_frame` fired per tick. FIXED to fire on empty→present only.
- **MEDIUM**: scheduler snapshots `on_build_scheduled` Arc at mount (stale if
  re-set later). DOCUMENTED on `set_on_build_scheduled` (binding wires it once).
- **LOW**: `Debug` locked the inbox. FIXED with `try_lock`.

## Known pre-existing imperfection (NOT fixed here — separate change)

`ElementCore::depth = slot` (the sibling index) is also what `set_state_scheduled`
→ `schedule_self_build` feeds to the heap. That path is unchanged by this work and
mis-orders nested `setState` the same way. The animation path sidesteps it via the
drain-time depth lookup; a proper fix (make `ElementCore::depth` the tree depth, or
route `setState` through a node-depth lookup) is orthogonal and would ripple
through every `ElementBase::mount` signature.

## Next consumer (not yet built): FadeTransition

`FadeTransition { opacity: Arc<dyn Animation<f32>>, child }` as an `AnimatedView`
whose `ViewState::build` returns `Opacity::new(opacity.value()).child(child)`.
Requires: a `flui-animation` dep in `flui-widgets`; a headless-harness method to
run a frame driven by the external inbox (a tick) WITHOUT marking the root dirty,
so an end-to-end test can show the opacity change across a frame.
