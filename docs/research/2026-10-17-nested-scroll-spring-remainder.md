# Nested scroll: what a remainder does to an ancestor that is springing back

- **Date:** 2026-10-17, base `interaction-integration` @ `30bfcee6a`
- **Question (owner):** an ancestor with bouncing physics is past its edge and its settle spring
  runs. A descendant's sequence (the inner list's drag, or a new contact on it) sends a remainder
  up the chain. Today the remainder is added to the spring's travel, so the ancestor moves twice.
  What should happen instead? Who settles an overscrolled ancestor at release or sequence end,
  and how does a new contact catch a springing ancestor?
- **Inputs:** [ADR-0209](../adr/ADR-0209-nested-scroll-connection.md) (§1, §4–§7, §9),
  [ADR-0208](../adr/ADR-0208-gesture-ownership-table-and-composition.md) rule 4 (hold on Down),
  [ADR-0127](../adr/ADR-0127-exceptional-path-retention.md),
  `crates/flui-widgets/src/scroll/scroll_owner.rs`, `crates/flui-widgets/src/scroll/nested/chain.rs`,
  `crates/flui-widgets/src/scroll/scrollable.rs`, `crates/flui-widgets/src/scroll/anchored_drag.rs`,
  `crates/flui-widgets/src/scroll/interactive_viewer.rs`, `crates/flui-widgets/ARCHITECTURE.md`
  ("Scroll resistance…", the scroll channel section).
- Paths are from the repository root. External code is pinned to the commit read:
  Flutter `flutter/flutter@dd45f0c2`, AndroidX `androidx/androidx@9a589fa6`, AOSP
  `aosp-mirror/platform_frameworks_base@1cdfff55`. UIKit is closed source; only Apple's
  documentation was read.

## Recommendation

**Option (d): a `UserInput` remainder the ancestor takes *catches* it, and the descendant's
sequence owes it a settle.** This is option (b) with the settle obligation held by the
dispatcher (the leaf's `ScrollSequence`), not by each leaf widget. The obligation is discharged at
release with the release velocity, or at sequence end from rest. The pieces mostly exist:
`settle_from_inside` already stops an owner's run and starts a settle under the descendant's
sequence id (`scroll_owner.rs:1009-1035`). `settle_latched_ancestors` already settles latched
scrollables at release and tells the leaf which axes it may still run motion on
(`chain.rs:615-651`). `AnchoredDrag`'s ancestor role already follows the same pattern by hand: it
takes `UserInput` only, records that it moved, and settles on `released` (`anchored_drag.rs:769-800`).
The changes are:

1. **Catch.** When `take_remainder` takes a non-zero `UserInput` (or `Momentum`) step on an owner
   that drives a run of its own (a settle spring, a fling, a snap), it first stops that run. The
   run's own sequence ends `Preempted`, the same reason ADR-0209 §4 gives a handed-off run that a
   new `UserInput` sequence cancels. The owner then moves by exactly what `take_within_extents`
   admits. The direction rule that already landed is unchanged: from past an extent, a remainder
   moves the owner only toward its range, never farther than the step and never farther out.
2. **No catch without a take.** A remainder the owner cannot take (outward from past an edge,
   or zero on its axis) leaves the spring running. A `SideEffect` remainder (a connection's
   motion, a drop autoscroll) never catches: while the owner runs its own motion it takes
   nothing from below. `Programmatic` stays refused, as today (`scroll_owner.rs:765-770`). Only
   the user's own motion takes the ancestor out of its spring.
3. **Debt.** Every scrollable link that took a non-zero part of the sequence is marked in the
   snapshot, one `Cell<bool>` per link, set before the owner publishes. The mark is delivery
   debt in the "accepted work must remain deliverable" sense. A participant panic, a link closing
   or the leaf retiring does not erase it.
4. **Release settles the latch holders.** `ScrollSequence::release` (`chain.rs:430-439`), not the
   leaf widget, settles every marked scrollable link through `settle_from_inside`. A link gets the
   release velocity on the axes it holds the latch for, and zero on the others. The leaf runs
   motion only on the axes it holds itself (`leaf_holds`). Today only `InteractiveViewer` calls
   `settle_latched_ancestors` (`interactive_viewer.rs:799,811`). `Scrollable::drag_ended` and
   `hold_ended` (`scroll_owner.rs:1473-1537`) do not, so a caught ancestor would be stranded past
   its edge. Moving the call into the sequence covers every leaf and every device at once.
5. **End without release settles from rest.** `ScrollSequence::end` for any reason (`Cancelled`,
   `ArenaLost`, `Preempted`, leaf unmount or retirement, a wheel or key sequence's idle end)
   settles each marked link that the release did not settle, at zero velocity, before it
   delivers `sequence_ended`. After containment (ADR-0127), the settle runs after the
   bookkeeping, and the first failure stays authoritative.
6. **The ancestor's own contact wins.** `settle_from_inside` already refuses while the owner has
   a live contact of its own (`scroll_owner.rs:1016`). A finger placed directly on the ancestor
   takes it over, and that contact's release settles it.
7. **A new contact catches by hold.** This needs no new mechanism. The ancestor's hold member
   joins only while it moves (`scrollable.rs:1270-1283`), and its Down stops the spring
   (`scroll_owner.rs:1458-1466`, ADR-0208 rule 4). When the inner drag takes the contact, the hold
   yields and the ancestor settles from rest (`hold_ended`, `scroll_owner.rs:1473-1496`). This is
   what Flutter does: a hold is cancelled with `goBallistic(0.0)`. The first remainder of the inner
   drag then catches that settle (rule 1), so the gap is the frames between slop and the first
   delta past the leaf's edge.
8. **Fling residuals are unchanged.** `accept_fling` on a springing or caught owner replaces its
   run with the residual's (`scroll_owner.rs:966-1003`).

### Behavior table

A is a bouncing scrollable in the chain of leaf L. "Toward" means toward A's range. Every row
holds for touch and mouse, and for both remainder presets: the sequence-long latch and the
preset that re-latches at the edge (ADR-0209 §6).

| A's state | Input | Outcome |
|---|---|---|
| Past edge, own spring running | L's `UserInput` remainder toward | Spring stopped, A's spring sequence ends `Preempted`; A moves by exactly the taken part, once; link marked |
| Past edge, own spring running | L's `UserInput` remainder outward | Not taken; spring keeps running; remainder continues up |
| Past edge, own spring running | `SideEffect` remainder | Not taken; spring keeps running |
| Past edge, own spring running | `Programmatic` step from L | Not taken (unchanged) |
| Past edge, own spring running | Down on L (A moving) | A's hold admitted, spring stopped at Down |
| Held by that Down | L's drag wins the contact (`Yielded`) | A settles from rest (spring restarts from where it was held) |
| Caught (spring stopped, link marked) | Further `UserInput` toward | A follows by the taken part; crossing into range keeps scrolling within extents |
| Caught | `UserInput` outward | Not taken; A stays where it is; nothing pulls it farther out |
| Caught | L released with velocity v, A latched on its axis | A settles via `settle_from_inside` with v; L runs no motion on that axis |
| Caught | L released, A marked but not latched on that axis | A settles from rest |
| Caught | L's sequence ends `Cancelled` / `ArenaLost` / `Preempted`, or L unmounts | A settles from rest before `sequence_ended`; that sequence ends once |
| Caught | A's own contact begins (a second finger on A) | A's contact keeps the offset; L's settle of A is refused; A's release settles it |
| Caught | A retired on the same position | The successor inherits the debt through the snapshot's link (see trade-offs) |
| Caught | A participant panics during the pass | Debt survives; A settles at release or end; the first panic resumes after |
| Past edge at rest (no run) | L's `UserInput` remainder toward | Taken, link marked; settled at release or end |
| In range, idle or flinging | L's `UserInput` remainder | Taken within extents (stops a fling by rule 1); at release A runs its own physics or snap with v |
| Caught | Fling residual offered by `post_fling` | `accept_fling` replaces the run (unchanged) |

### Test rows (`nested_scroll_channel`, driven by `PointerScript`)

Every row runs under `DEVICES` and both remainder presets.

- `a_remainder_catches_a_springing_ancestor_and_moves_it_once`: A at −100 with its spring
  running, L at its end. Each frame, A's pixel change equals the remainder L reports, not that
  plus the spring's delta. Under (a) this fails because the track moves farther than the finger.
- `an_outward_remainder_leaves_the_spring_running`: the spring reaches A's edge on its own
  schedule (pin it against the spring's solo track).
- `a_side_effect_remainder_never_catches_a_springing_ancestor`.
- `a_caught_ancestor_settles_at_release_with_the_latched_velocity`: A rests on its edge. A
  `Scrollable` leaf (not only `InteractiveViewer`) proves the release path. A pager ancestor lands
  on a page.
- `a_caught_ancestor_settles_when_the_sequence_ends_without_release`: cancel, arena lost, leaf
  unmount mid-drag, and a wheel sequence's idle end. A rests on its edge each time, and
  `sequence_ended` reaches each participant once.
- `a_new_contact_holds_a_springing_ancestor_until_the_inner_drag_takes_it`: A is still from Down
  to slop, and the pixel track has no jump at the first remainder.
- `a_caught_ancestor_touched_directly_follows_its_own_contact`.
- `a_caught_ancestor_retired_mid_drag_is_settled_by_its_successor`.
- `participant_panic_consumes_all_and_next_sequence_recovers` gains a row: a panic in a
  connection above A after A was caught still settles A, and the first panic is the one resumed.
- `a_remainder_moves_an_overscrolled_ancestor_only_toward_its_range` stays as it is, as the
  static-overscroll row.

Revert check (AGENTS.md): with rule 1 reverted, the first row must fail on the per-frame
equality. With rule 4 reverted, the release row must fail because A stays past its edge.

## 1. FLUI today

- **Double motion.** `take_within_extents` publishes `pixels + delta` clamped between
  `min(min_extent, pixels)` and `max(max_extent, pixels)` (`scroll_owner.rs:1212-1231`). It does
  not touch the owner's run. On the next tick the run sees that its leaf has moved
  (`pixels != last`) and adds its own delta relative to the moved position
  (`scroll_owner.rs:744-751`). The ancestor therefore moves by the remainder plus the spring, and
  rests short of or past the spring's target by the sum of the remainders.
- **Why stopping the spring alone strands it.** At release, `Scrollable` calls
  `drag_ended`/`hold_ended`, which start a run on the leaf only (`scroll_owner.rs:1517-1537`).
  Nothing settles a latched ancestor. Only `InteractiveViewer` calls `settle_latched_ancestors`
  (`interactive_viewer.rs:791-812`), and `AnchoredDrag`'s ancestor role settles itself from
  `released` (`anchored_drag.rs:785-800`).
- **When a remainder reaches A.** Under the default latch the leaf keeps the axis for the whole
  sequence, and ancestors are offered nothing (`chain.rs:683-706`). Remainders reach A in two
  cases: when L is at its edge on the first delta (edge-at-start latch), and under the preset that
  re-latches at the edge. In both cases `latch_mask` then gives L nothing on that axis
  (`chain.rs:674-680`), so L's own fling deltas also reach A through `take_remainder`. Rule 4 makes
  A settle with its own physics instead. That is better: a latched pager snaps by its own policy.
  It also changes the edge-at-start fling path, so `edge_at_start_latches_ancestor_on_every_preset`
  and `fling_handoff_inherits_sequence_across_three_levels` must be re-checked.
- **A new contact on L while A springs.** A's hold member joins only while A animates
  (`scrollable.rs:1270-1283`), and `hold` stops the run (`scroll_owner.rs:1458-1466`). On
  `Yielded`, `hold_ended` calls `start_run(…, 0.0, …)` (`scroll_owner.rs:1473-1496`), and the
  bouncing physics springs back from the held position.

## 2. Per-framework findings

### Flutter

- **One activity per position.** `beginActivity` disposes the previous activity
  ([`scroll_position_with_single_context.dart` L114-127](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scroll_position_with_single_context.dart#L114-L127)).
  A `BallisticScrollActivity` writes the simulation's *absolute* value on each tick
  (`applyMoveTo(_controller.value)` → `setPixels`,
  [`scroll_activity.dart` L583-661](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scroll_activity.dart#L583-L661)).
  An external offset change during a bounce is not added to; the next tick overwrites it. Flutter
  has no "spring plus remainder" state.
- **Hold and drag replace the ballistic activity.** `hold()` begins a `HoldScrollActivity`, and
  `drag()` begins a `DragScrollActivity`
  ([L253-277](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scroll_position_with_single_context.dart#L253-L277)).
  `Scrollable._handleDragDown` holds at Down
  ([`scrollable.dart` L864-868](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scrollable.dart#L864-L868)).
  The drag recognizer reports Down when the pointer is added, before any arena decision
  ([`monodrag.dart` L412-423](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/gestures/monodrag.dart#L412-L423)).
  If another recognizer wins, `_handleDragCancel` cancels the hold
  ([L896-910](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scrollable.dart#L896-L910)),
  and `HoldScrollActivity.cancel` calls `delegate.goBallistic(0.0)`
  ([`scroll_activity.dart` L239-241](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scroll_activity.dart#L239-L241)).
  The outer list springs again from the held position, which is the same as FLUI's `Yielded` path.
- **Children are hit-testable during a bounce-back.** `shouldIgnorePointer` is
  `!outOfRange && …`. Its documentation says that when the position is overscrolled, "children of
  the scroll view will receive pointer events as the scroll view settles back"
  ([`scroll_position.dart` L284-291](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scroll_position.dart#L284-L291)).
  A new contact on the inner list during a spring-back is therefore a supported case.
- **Plain nested `Scrollable`s share no deltas.** The open case does not exist for them.
- **`BouncingScrollPhysics`** returns `0.0` from `applyBoundaryConditions`. Its
  `applyPhysicsToUserOffset` lets easing motion through with less friction than tensioning motion
  ([`scroll_physics.dart` L835-876](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scroll_physics.dart#L835-L876)).
  `createBallisticSimulation` springs from out of range even at zero velocity
  ([L879-899](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scroll_physics.dart#L879-L899)).
- **`NestedScrollView` is (b) with a single owner.** `_NestedScrollCoordinator.hold` and `drag`
  begin the new activity on the outer position *and on every inner position*. `goBallistic` gives
  every position its own ballistic activity at release
  ([`nested_scroll_view.dart` L702-737](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/nested_scroll_view.dart#L702-L737),
  [L1040-1051](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/nested_scroll_view.dart#L1040-L1051)).
  A touch stops every spring, the drag moves each position directly, and the release settles each
  one, including an inner position still out of range
  ([`_NestedScrollPosition.goBallistic` L1391-1402](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/nested_scroll_view.dart#L1391-L1402)).
  `applyUserOffset` resolves an inner overscroll first, then the outer view
  ([L1069-1124](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/nested_scroll_view.dart#L1069-L1124)).
  `applyClampedDragUpdate` states FLUI's landed rule: "we cannot, via applyClampedDragUpdate,
  _enter_ an overscroll situation"
  ([L1266-1314](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/nested_scroll_view.dart#L1266-L1314)).

### Jetpack Compose

- **Order.** For each delta, `scrollByWithOverscroll` wraps the *whole* nested pass in the
  dispatching node's own `OverscrollEffect.applyToScroll`. Inside it, `performScroll` runs
  `dispatchPreScroll` → self → `dispatchPostScroll`
  ([`Scrollable.kt` L626-670](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/gestures/Scrollable.kt#L626-L670)).
  A fling is wrapped the same way. `applyToFling` encloses `dispatchPreFling` → fling animation →
  `dispatchPostFling` ([L697-722](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/gestures/Scrollable.kt#L697-L722)),
  and the fling's frames are `SideEffect` deltas
  ([L724-768](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/gestures/Scrollable.kt#L724-L768)).
  The overscroll a remainder produces is shown at the *dispatching* leaf, after its ancestors
  declined it. The `NestedScrollConnection` contract is the pre/post pair for deltas and
  velocities ([`NestedScrollModifier.kt` L41-95](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/input/nestedscroll/NestedScrollModifier.kt#L41-L95)).
- **An ancestor scrollable never overscrolls from a child's remainder.** Its connection's
  `onPostScroll` calls `performRawScroll`, which dispatches a raw delta into the state, not
  through its overscroll effect. It returns `Offset.Zero` while that ancestor's own
  `isScrollInProgress` is true
  ([`AbstractScrollableNode.kt` L206-236](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/gestures/AbstractScrollableNode.kt#L206-L236),
  [`Scrollable.kt` L682-695](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/gestures/Scrollable.kt#L682-L695)).
  In effect, Compose chooses (c) for a moving ancestor: while it moves, the child's remainder is
  dropped. A parent that is already flinging also declines `onPostFling` (`isFlinging`).
- **A moving parent takes the new contact.** `startDragImmediately()` is
  `shouldScrollImmediately()`, which is `isScrollInProgress || overscrollEffect.isInProgress`
  ([`AbstractScrollableNode.kt` L201-203](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/gestures/AbstractScrollableNode.kt#L201-L203),
  [`Scrollable.kt` L770-772](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/gestures/Scrollable.kt#L770-L772)).
  The drag node then consumes the Down in the `Initial` pass, which reaches ancestors first,
  without waiting for slop
  ([`Draggable.kt` L846-872](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/gestures/Draggable.kt#L846-L872)).
  Touching a child while the parent stretches or flings drives the parent. The child never
  drags while the parent's effect releases, so the open case does not arise.
- **Catching the stretch.** At the start of a scroll cycle, `AndroidEdgeEffectOverscrollEffect`
  pulls each stretched edge by zero "to 'catch' it at its current point so that the user
  continues to manipulate the stretch with this new scroll, instead of letting the old stretch
  fade away underneath the user's input". Motion toward the edge relaxes the stretch first, only
  `UserInput` deltas pull new stretch, and once content consumes again the stretch is released
  ([`AndroidOverscroll.android.kt` L539-685](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/androidMain/kotlin/androidx/compose/foundation/AndroidOverscroll.android.kt#L539-L685)).
  `applyToFling` is the end signal. It absorbs leftover velocity and calls
  `animateToReleaseIfNeeded()`, so every cycle that caught a stretch releases it
  ([L687-753](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/androidMain/kotlin/androidx/compose/foundation/AndroidOverscroll.android.kt#L687-L753)).
  The `OverscrollEffect` documentation requires callers to call `applyToFling` for that reason
  ([`Overscroll.kt` L58-120](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/Overscroll.kt#L58-L120)).
  The owner that caught the effect is the one that settles it, which is FLUI's rule 4 in
  miniature.

### Android View (AndroidX core, RecyclerView, AOSP)

- **`NestedScrollingParent3.onNestedScroll`** receives the consumed and unconsumed parts and
  adds what it takes to the `consumed` out-array. The contract says that array also carries what
  parents further up consumed
  ([`NestedScrollingParent3.java` L35-79](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/core/core/src/main/java/androidx/core/view/NestedScrollingParent3.java#L35-L79)).
  `NestedScrollView.onNestedScrollInternal` takes the remainder with a clamped `scrollBy`, so a
  parent never overscrolls or glows from a child's remainder, and then passes the rest on
  ([`NestedScrollView.java` L392-404](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/core/core/src/main/java/androidx/core/widget/NestedScrollView.java#L392-L404)).
- **The edge effect belongs to the dispatching child.** `RecyclerView.scrollByInternal` scrolls
  itself, calls `dispatchNestedScroll`, and only then calls `pullGlows` with what is still
  unconsumed
  ([`RecyclerView.java` L2419-2475](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/recyclerview/recyclerview/src/main/java/androidx/recyclerview/widget/RecyclerView.java#L2419-L2475)).
- **A releasing edge effect or a running fling takes the new contact.** On `ACTION_DOWN`,
  `NestedScrollView.onInterceptTouchEvent` sets `mIsBeingDragged = stopGlowAnimations(ev) ||
  !mScroller.isFinished()`, with the comment "We also want to catch the edge glow and start
  dragging if one is being animated"
  ([L881-915](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/core/core/src/main/java/androidx/core/widget/NestedScrollView.java#L881-L915)).
  `stopGlowAnimations` catches the effect with `onPullDistance(0, …)`
  ([L1389-1400](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/core/core/src/main/java/androidx/core/widget/NestedScrollView.java#L1389-L1400)).
  `RecyclerView` does the same: `stopGlowAnimations(e) || mScrollState == SCROLL_STATE_SETTLING`
  disallows the parent's intercept and enters `DRAGGING`
  ([L4106-4122](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/recyclerview/recyclerview/src/main/java/androidx/recyclerview/widget/RecyclerView.java#L4106-L4122)).
  Android, like Compose, gives the contact to the view whose effect is releasing. A nested child
  scrolling while that view's edge effect releases does not happen in these views.
- **Catch, not add.** `EdgeEffect.onPull` restores the pull distance "to the fraction it is
  currently showing — we want to 'catch' the current stretch value". The release animation is
  replaced, and the new input continues from the shown value
  ([`EdgeEffect.java` L341-385](https://github.com/aosp-mirror/platform_frameworks_base/blob/1cdfff555f4a21f71ccc978290e2e212e2f8b168/core/java/android/widget/EdgeEffect.java#L341-L385)).
  `onPullDistance` returns the part of the delta it consumed and can reverse a pull, `getDistance`
  is "the pull distance that must be released", and `onRelease` begins the decay
  ([L425-470](https://github.com/aosp-mirror/platform_frameworks_base/blob/1cdfff555f4a21f71ccc978290e2e212e2f8b168/core/java/android/widget/EdgeEffect.java#L425-L470)).
  Motion toward the edge is consumed by the stretch first (`releaseVerticalGlow`,
  [`NestedScrollView.java` L2150-2168](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/core/core/src/main/java/androidx/core/widget/NestedScrollView.java#L2150-L2168)),
  and a fling destretches before it scrolls (`consumeFlingInVerticalStretch`, L1322-1342).

### iOS UIKit

- UIKit's source is not public. Apple's guide says same-direction nesting "is supported, and
  there is a distinct behavior defined for the functionality, however that behavior is subject
  to change in future versions of iOS". It does not describe bounce or edge handoff
  ([Scroll View Programming Guide, "Nesting Scroll Views"](https://developer.apple.com/library/archive/documentation/WindowsViews/Conceptual/UIScrollView_pg/NestedScrollViews/NestedScrollViews.html)).
  `UIScrollView` documents that a scroll view bounces at its content boundary by default
  ([`bounces`](https://developer.apple.com/documentation/uikit/uiscrollview/bounces)). It decides
  between scrolling and tracking a subview with a touch-down timer, and cancels the subview's
  tracking once it scrolls itself
  ([`UIScrollView` overview](https://developer.apple.com/documentation/uikit/uiscrollview)).
  `isDecelerating` covers motion after lift
  ([`isDecelerating`](https://developer.apple.com/documentation/uikit/uiscrollview/isdecelerating)).
  UIKit has no delta-sharing API between scroll views: each owns its own pan recognizer and
  bounce.
- **Not determined from primary sources:** what an inner `UIScrollView` drag does to an outer
  one that is bouncing back. The iOS preset's behavior must come from a device recording, as
  ADR-0209 already requires for the iOS latch rule ("run only on native hosts").

## 3. Trade-offs

| Option | For | Against |
|---|---|---|
| (a) Add to the spring | No new state | Double motion; the rest position is the spring's target shifted by every remainder; no other framework does it (Flutter's activities write absolute values) |
| (b) Stop the spring, move directly, settle at end | Matches Flutter's `NestedScrollView` and the "catch" of `EdgeEffect`/Compose stretch; one motion source at a time | Needs a settle owner at release and at every end reason; stranding is the failure mode |
| (c) Route back to the child or drop | No settle owner; Compose's choice for a moving parent (`performRawScroll` returns zero) | The child is at its edge, which is why it sent the remainder, so it has nothing to take. Under FLUI's arena (the innermost owns the contact, ADR-0208) the finger would move nothing until the spring ends. Compose and Android avoid that dead finger only because the moving parent takes the contact at Down |
| Moving ancestor takes the new contact (Compose, Android) | The open case disappears | Contradicts ADR-0208's table and the channel's latch; a press on an inner list would scroll the outer one while it moves; it solves only the new-contact case, not a remainder arriving mid-sequence |
| (d) = (b), with the debt in the sequence | One owner (the dispatcher) for every leaf and device; reuses `settle_from_inside`/`settle_latched_ancestors`; ends the hand-written `released` settles; a latched pager snaps by its own policy | Changes the edge-at-start fling path: L's run no longer feeds a latched A, which settles with its own physics instead, so existing fling rows must be re-checked. The debt must follow owner retirement: `retire_into` moves the owner's own sequences, but a snapshot's link to a retired ancestor is a `Weak` to the old owner, so the debt needs to resolve through the chain node's current owner (as `rebase_leaf` does for the leaf, `chain.rs:755`) |

Smaller choices inside (d):

- **Catch only on a take.** An outward remainder that cannot move A should not stop its spring.
  Otherwise A freezes past its edge for the rest of the drag while the user is not moving it.
- **`SideEffect` never catches.** This keeps the cause rule of ADR-0209 §2 (decide by cause, not
  by size) and matches `AnchoredDrag`'s ancestor and Compose's "pull only on `UserInput`".
- **Settle from rest on `Yielded` instead of keeping A held until the inner drag's first
  remainder.** This is what Flutter does, and the catch absorbs it within a frame or two. Keeping
  the hold would need A to know that the winner is in its subtree.

## 4. What the ADR-0209 amendment should record

> **§6a (Amended 2026-10-17). A remainder catches; the sequence settles what it moved.** A
> scrollable that takes a non-zero `UserInput` or `Momentum` remainder first stops the run it
> drives (a settle spring, a fling, a snap), whose sequence ends `Preempted`. It then moves only
> by what it takes, from past an extent only toward its range. A remainder it cannot take, and
> any `SideEffect` remainder, leave its run alone. Every scrollable that a sequence moved through
> the channel is owed a settle by that sequence. At `released`, each one settles under the same
> sequence id, with the release velocity on the axes whose latch it holds and from rest on the
> others, and the leaf runs motion only on the axes it holds. A sequence that ends without a
> release settles them from rest before `sequence_ended`, whatever the reason. The debt survives
> a participant panic and follows the owner's retirement to its successor. A scrollable with a
> live contact of its own is settled by that contact. A new contact catches a moving ancestor by
> the hold on Down (ADR-0208 rule 4); when the hold yields to a descendant's drag, the ancestor
> settles from rest and the first remainder catches it. Rows:
> `a_remainder_catches_a_springing_ancestor_and_moves_it_once`,
> `a_caught_ancestor_settles_at_release_with_the_latched_velocity`,
> `a_caught_ancestor_settles_when_the_sequence_ends_without_release`.

Also record in the amendment that a participant connection still settles itself from `released`
(§9's `AnchoredDrag` ancestor), and that the iOS preset's behavior for this case waits on a
device recording.

## Sources

Flutter (`flutter/flutter@dd45f0c2f98adca9d327649a61076d030c1eb426`):
- https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scroll_physics.dart#L835-L899
- https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scroll_activity.dart#L221-L248
- https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scroll_activity.dart#L583-L661
- https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scroll_position_with_single_context.dart#L114-L157
- https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scroll_position_with_single_context.dart#L253-L277
- https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scroll_position.dart#L284-L291
- https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scrollable.dart#L864-L910
- https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/gestures/monodrag.dart#L412-L423
- https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/nested_scroll_view.dart#L702-L737
- https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/nested_scroll_view.dart#L1040-L1124
- https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/nested_scroll_view.dart#L1266-L1402

Jetpack Compose (`androidx/androidx@9a589fa61f8661e0cbef9123dcc98d3c1f655fc7`):
- https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/gestures/Scrollable.kt#L626-L772
- https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/gestures/AbstractScrollableNode.kt#L201-L236
- https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/gestures/Draggable.kt#L846-L872
- https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/androidMain/kotlin/androidx/compose/foundation/AndroidOverscroll.android.kt#L539-L762
- https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/Overscroll.kt#L36-L130
- https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/input/nestedscroll/NestedScrollModifier.kt#L41-L95

Android View:
- https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/core/core/src/main/java/androidx/core/view/NestedScrollingParent3.java#L35-L79
- https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/core/core/src/main/java/androidx/core/widget/NestedScrollView.java#L392-L477
- https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/core/core/src/main/java/androidx/core/widget/NestedScrollView.java#L881-L915
- https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/core/core/src/main/java/androidx/core/widget/NestedScrollView.java#L1322-L1400
- https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/core/core/src/main/java/androidx/core/widget/NestedScrollView.java#L2150-L2168
- https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/recyclerview/recyclerview/src/main/java/androidx/recyclerview/widget/RecyclerView.java#L2419-L2475
- https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/recyclerview/recyclerview/src/main/java/androidx/recyclerview/widget/RecyclerView.java#L4106-L4122
- https://github.com/aosp-mirror/platform_frameworks_base/blob/1cdfff555f4a21f71ccc978290e2e212e2f8b168/core/java/android/widget/EdgeEffect.java#L341-L470

Apple:
- https://developer.apple.com/library/archive/documentation/WindowsViews/Conceptual/UIScrollView_pg/NestedScrollViews/NestedScrollViews.html
- https://developer.apple.com/documentation/uikit/uiscrollview
- https://developer.apple.com/documentation/uikit/uiscrollview/bounces
- https://developer.apple.com/documentation/uikit/uiscrollview/isdecelerating
