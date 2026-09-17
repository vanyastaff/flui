# #1180 — external schedules landing mid-drain are absorbed at the top of each heap pop — plan v2

(v1 re-drained after the heap emptied behind a `draining` flag; harsh-critic showed that keeps the redundant
build (the re-mark lands after `dirty_reasons.remove`, so the same element builds twice in one frame), cannot
satisfy its own shallow-before-deep test, and that the flag was redundant (the frame's dirty gate already
discards the extra wake) and wedgeable (stuck `true` after a caught build panic); concurrency-specialist showed a
standalone atomic read outside the inbox lock has a real cross-thread lost-wake window. v2 is the critic's ALT-1.)

Worktree: /mnt/data/dev/flui-worktrees/view-mid-drain (branch fix/1180-mid-drain-external-builds-join-the-scope, from origin/main 864b5798).
Issue: `gh issue view 1180`. Code: `crates/flui-view/src/owner/build_owner.rs` — `ExternalBuildScheduler::schedule`
(~L60-108), `build_scope` (~L1139), `drain_build_scope` (~L1263; inbox drained once at ~L1282, the heap loop from
~L1342, `dirty_reasons.remove` after each build ~L1555, `element_rebuilt` event ~L1568), `owner/layout_builder.rs`
`service_layout_builders`/`drive_fixpoint` (they reach `drain_build_scope` too, up to `MAX_LAYOUT_BUILD_PASSES = 10`
per frame), `tree/id_reconcile.rs` (~L408-419 schedules a dirty-by-update child onto the heap during phase 2),
`element/behavior_commons.rs` (`clear_dirty` after `build()`). Consumer pin to flip:
`crates/flui-widgets/tests/implicit_animations.rs` `zero_duration_retarget_lays_out_the_new_target_on_the_same_pump`.
Flutter at 3.44.0: `framework.dart` `BuildOwner.buildScope` re-sorts `_dirtyElements` every iteration and walks
`index` back when a schedule lands mid-loop; `markNeedsBuild` is absorbed by `if (dirty) return`; in DEBUG it asserts a
mid-build schedule targets a descendant of the element being built (`_debugCurrentBuildTarget`).

## Facts
- The redundant FRAME the issue describes comes from `has_pending_work()` → `has_dirty_elements()` seeing the
  non-empty inbox after the frame, not from the wake: `mark_rendered()` clears `needs_redraw`, and a leftover
  platform redraw hits `frame_is_dirty == false` → `WakeAction::Skip`. Emptying the inbox in-frame removes the frame.
- The heap path (`schedule_build_for` on every fresh child mount) already fires `on_build_scheduled` mid-drain
  (pinned by a test); suppressing only the inbox path changes nothing measurable.
- `ElementId` slots are reused immediately (a remove-then-insert yields the same id, pinned). A `RebuildHandle`
  for a child unmounted in phase 2 can therefore name the slot's new occupant — a pre-existing cross-frame hazard
  that same-drain absorption makes likelier.

## Decisions
1. **Absorb at the top of each heap pop.** In `drain_build_scope`'s `while let Some(dirty) = pop()` loop, before
   popping, absorb the inbox: `let landed = { lock; if empty { None } else { Some(drain().collect::<Vec<_>>()) } }`
   (lock released before anything else; never held across a build — `parking_lot::Mutex` is non-reentrant and a
   build may call `schedule` synchronously), then for each `(id, reasons)`: if `dirty_reasons` already has the id
   (Occupied — the element is on the heap or is the one being served), MERGE the reasons and do NOT re-mark or push;
   else `tree.mark_needs_build(id)`, push with the AUTHORITATIVE tree depth. That is Flutter's `if (dirty) return`
   absorption plus its per-iteration re-sort: a shallower mid-drain id builds next, a deeper one after the current
   pass's shallower entries, and the #1180 double build collapses to one (the inbox id merges into the still-live
   entry of the element the reconcile already scheduled).
2. **One per-FRAME absorb budget, shared by every drain.** `BuildOwner.mid_drain_absorbs_left: usize`, reset to
   `MAX_MID_DRAIN_ABSORBS` (16) at `build_scope` entry and decremented per non-empty absorb in ANY drain that frame
   (the layout-builder fixpoint's Global/scoped drains included). When it hits zero the inbox is left for the next
   frame (the existing `has_pending_work` gate schedules it) and a `tracing::warn!` fires ONCE PER STREAK
   (`mid_drain_cap_streak: bool`, re-armed when a frame completes under the cap) naming the leftover ids. Why 16:
   a downward listener chain (parent notifies a subscriber below it) costs ZERO absorbs beyond the first (it merges
   into a live entry); only upward/sideways re-entries consume one, and eight of those in a frame is already a
   pathological tree. The bound exists for a listener that reschedules itself on every build.
3. **No wake suppression.** `request_frame` stays as it is (fires on a newly queued id, any phase); the redundant
   frame is gone with the inbox, and the leftover wake is discarded by the dirty gate. Flutter's single
   `_scheduledFlushDirtyElements` latch across both paths is the shape to take IF a wake-count oracle ever shows a
   cost — recorded as the alternative, not built.
4. **Stale-id hazard documented at the absorb site** (a handle can name a reused slot; nothing in this change
   makes it worse than the cross-frame case, but same-drain absorption makes it likelier); a generation-carrying
   `ElementId` is out of scope — say so.
5. **Docs**: fix the false `drain_build_scope` comment ("Flutter defers mid-frame schedules" — Flutter defers the
   FRAME REQUEST via its latch, not the dirty mark); `ExternalBuildScheduler` doc; `## Mapping decisions` (flui-view
   ARCHITECTURE) entry: same-scope absorption = Flutter's `buildScope` re-sort, BUT FLUI builds any id (Flutter
   debug-asserts a mid-build schedule targets a descendant of the building element; FLUI has no such assert —
   record the divergence and why: the inbox is the only route for listener-driven rebuilds, which Flutter routes
   through `setState` under that assert); the per-frame budget; the stale-id note. CHANGELOG (flui-view)
   `### Changed`. Add a `tracing::debug!` field `absorbed_mid_drain = n` on the `build` span.

## Tests (RED first unless pin)
- flui-view unit: an element whose build notifies a `Listenable` that a DESCENDANT subscribes to via a
  `RebuildHandle` (the inbox path) → the descendant is built exactly ONCE in the same `build_scope` (oracle:
  `element_rebuilt` count / a build counter), `pending_external_builds() == 0`, `has_dirty_elements() == false`.
  Red today: two frames; red under v1's after-heap re-drain: two builds.
- Shallow-before-deep: a mid-drain schedule of a SHALLOWER element than the one building, with a deeper entry
  still pending → build order is current, shallow, deep.
- Merge: a mid-drain schedule for an element already on the heap → one build, merged reasons.
- Budget: a self-rescheduling element (schedules itself through the inbox from its own build) → the drain stops
  after `MAX_MID_DRAIN_ABSORBS`, the id is still in the inbox, one warn, the frame completes, the next
  `build_scope` serves it again; the warn re-arms only after a clean frame.
- Budget is per frame across drains: with a live layout builder (`register_layout_builder_for_test`) the fixpoint's
  drains share the budget (total absorbs across the frame ≤ 16).
- Reentrancy: a build calling `schedule` synchronously on the owner thread does not deadlock (the inbox lock is
  never held across a build).
- No wake change: a schedule OUTSIDE a drain still calls `on_build_scheduled` (pin); a schedule inside a drain also
  calls it (pin the current behavior explicitly so a later "optimization" is a deliberate change).
- flui-widgets: flip `zero_duration_retarget_lays_out_the_new_target_on_the_same_pump` to `pending == 0`,
  `has_dirty_elements() == false` after the retargeting pump, and assert the `AnimatedBuilder` built once.
- Stale-id: a keyed-child swap in phase 2 whose old child's `RebuildHandle` fires mid-drain — document the observed
  behavior in the test (which element builds) rather than asserting a policy; it is the pin for the recorded hazard.

## Gates
`cargo nextest run -p flui-view -p flui-widgets -p flui-material -p flui-app -p flui -p flui-testing`, clippy/fmt/doc,
`just ci` once. Review: rust-reviewer + one outside lens; concurrency-specialist only if a reviewer reopens the wake question.

## v3 deltas (harsh-critic's recheck of v2 — ACCEPTABLE once these are folded; all are part of the contract)
1. **The budget charges RE-ENTRIES only, never first-time absorbs.** The #1180 sequence itself is a first-time
   absorb (the notify fires during the PARENT's phase-2 reconcile, before `AnimatedBuilder` is in `dirty_reasons`,
   so it is pushed as Vacant and the container's later reconcile merges into it — one build). A page whose N
   independent parents each retarget an implicit animation in one frame therefore performs N first-time absorbs
   at N pops, all legitimate. Keep `built_this_frame: HashSet<ElementId>` (cleared at `build_scope` entry, an id
   inserted after each build); an absorbed Vacant id that is ALREADY in it is a re-entry and costs one unit of
   `MAX_MID_DRAIN_ABSORBS` (16); a first-time absorb is free (finite: each id builds once before it can count).
   Test: > 16 independent listener notifications across separate pops all build same-frame, no warn.
2. Wording: at the top of a pop nothing is "being served"; Occupied means "on the heap or in a deferred scope
   bucket" (merge keeps a deferred id in its bucket). The re-mark-after-`dirty_reasons.remove` path is exactly the
   self-rescheduler (its entry is gone at the next pop, so it builds again) — the case the budget bounds.
   Record the second divergence: Flutter DROPS a self-`setState` issued during the element's own build entirely
   (`if (dirty) return`, the flag is still set mid-build); FLUI rebuilds it once more per re-entry up to the budget,
   because a cross-thread `schedule` cannot tell "during my build" from "after". Say so in the mapping entry.
3. Streak re-arm has no frame-complete hook in `BuildOwner`: at the next `build_scope` entry, if the previous
   frame ended with budget left (`mid_drain_absorbs_left > 0`), clear the streak flag; then reset the budget.
4. Scoped-target tests: a Global id landing during a `LayoutBuilder(scope)` drain is deferred to the root bucket
   and built by the next pass's Global drain, NOT inside the scoped drain; the mirror (a scope-A id during a
   Global drain lands in `isolated[A]`).
5. Docs that still say "drained at frame start": `rebuild_handle.rs` module doc (~L20-21) and `schedule` doc
   (~L109-118); `build_owner.rs` `external_inbox` field doc (~L342-349), `ExternalBuildScheduler` type doc
   (~L51-52), `has_dirty_elements` comment (~L1111-1116); `flui-widgets/src/testing.rs` `LaidOut::tick`/`pump_for`
   docs (~L699-704, ~L722-725).
6. Test oracles: `element_rebuilt` fires only with a `tree_observer` installed — install one or count builds in
   the view; the budget test asserts the exact build count (1 + budget), `has_dirty_elements() == true` after the
   capped frame (the gate leg that schedules the next one), and the leftover id still in the inbox.
7. Panic posture stays clean: budget/streak/`built_this_frame` are plain fields reset at `build_scope` entry, so a
   caught build panic leaves no stuck state; the absorb's lock is scoped to the take.
