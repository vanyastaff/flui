# Scroll jump during dispatch: what happens to `jump_to` called while a channel pass runs?

- **Date:** 2026-10-17, base `interaction-integration` @ `30bfcee6a`
- **Question (owner):** a position listener or a nested scroll participant calls
  `ScrollController::jump_to(x)` while a pass of the ADR-0209 channel is running. Today the jump
  is refused and lost. Which contract replaces that: (a) `jump_to` returns a `Result`, (b) the jump
  is queued and applied after the pass, (c) only the latest target is applied after the pass, or
  (d) something better? Should a programmatic jump cancel an in-flight fling or drag, and what
  cause and sequence does it report to participants?
- **Inputs:** [ADR-0209](../adr/ADR-0209-nested-scroll-connection.md) §1–§3, §7, §8,
  [ADR-0127](../adr/ADR-0127-exceptional-path-retention.md), `crates/flui-widgets/ARCHITECTURE.md`
  ("Every offset change is a step of the nested scroll channel", "Programmatic scroll replacement
  preserves the published velocity"), and the code below.
- Paths are from the repository root. External code is pinned to the commit read: Flutter
  `flutter/flutter@dd45f0c2`, Compose `androidx/androidx@9a589fa6`, Xilem/Masonry
  `linebender/xilem@271a27a6`, GPUI `zed-industries/zed@56934ec0`. Apple frameworks are closed
  source; their claims rest on the developer documentation only.
- Everything said about FLUI's current behaviour was read from the code, not run.

## Recommendation

**(d): a programmatic request made while a pass runs is deferred to the end of the outermost
pass on that chain, through the controller's existing one-slot command cell, so the latest request
wins; it is applied before the dispatch that ran the pass returns, not on the next frame.** It is
(c) with three things (c) leaves open settled: where the jump waits (the slot `animate_to` already
uses, so a jump and an animation supersede each other in call order), when it lands (when the
outermost pass returns, in the same input event or tick), and what triggers the deferral (any pass
on the owner's chain, not only a user connection's callout).

1. **No `Result`.** A deferred jump is not a failure: the caller cannot fix it, and every caller
   (a listener, a participant, a `PageController`) would have to rebuild the deferral itself.
   Flutter and UIKit apply a reentrant jump on the spot. Compose fails the programmatic scroll only
   when user input owns the state, and the caller there is a suspended coroutine. `jump_to` keeps
   returning `()`.
2. **No FIFO queue.** A jump is absolute, so only the last target decides where the offset ends.
   Replaying every target would emit a programmatic sequence per stale target, and a listener that
   jumps on each notification could grow the queue without bound. The existing slot already
   follows "a later command always supersedes an earlier, not-yet-serviced one"
   (`crates/flui-widgets/src/scroll/scroll_controller.rs:76-96`).
3. **Deferral covers listeners too.** Today only a user connection's callout raises the refusal
   (`RefuseDispatch` in `crates/flui-widgets/src/scroll/nested/chain.rs:728`, `:772-795`). A
   position listener that fires from the leaf's own publish runs while `refusing == 0`, so its jump
   runs *inside* the step, as a second sequence nested between the outer step's `pre_scroll` and
   `post_scroll` (see 1.2). Count a pass from dispatch entry to exit on every owner of the chain,
   and defer whenever the count is non-zero.
4. **What the jump cancels, decided when it lands, not when it is asked for.** It preempts the
   motion the owner *runs*: a fling, a settle, an `animate_to`, a residual run it accepted. Those
   sequences end `Preempted` before the jump's sequence starts. It never ends what live input
   *drives*: a contact still down, a held key, a drop autoscroll. The contact goes on from the new
   offset under its own sequence and latch. This is ADR-0209 §3's priority, a `UserInput` step
   preempts `Programmatic` and never the other way round, and it is Compose's `MutatePriority`
   order. Flutter cancels the drag instead (2.1); that is rejected (Trade-offs).
5. **What participants see.** The jump is its own sequence: a fresh `InputSequenceId` minted when
   the deferred jump lands, `ScrollCause::Programmatic`, `ScrollDevice::Programmatic`, then
   `sequence_started`, one step (`pre_scroll` root to leaf, `post_scroll` leaf to root), no
   `released`, and `sequence_ended(Released)`. That is what `ScrollOwner::jump` emits today
   (`crates/flui-widgets/src/scroll/scroll_owner.rs:1236-1253`). Participants keep opting in to
   `Programmatic` (§2). The order is fixed: the requesting step completes; a preempted run's
   sequence ends; the jump's sequence starts, steps and ends. A coalesced jump is one sequence.
6. **Failure and lifetime.** The flush runs from the pass guard's `Drop`, under the pass's
   `PanicRecovery`, so a participant that panics after asking for a jump still gets the jump
   applied, and the first failure stays the one that resumes (ADR-0127). If the owner closes on
   the same position mid-pass, the slot lives on the controller, so the successor services it
   (§8). If no owner serves the controller any more, the controller writes the clamped position
   itself, as `jump_to` already does when the jump hook declines
   (`scroll_controller.rs:283-292`). The target is clamped to the extents when it lands, not when
   it is asked for.
7. **No livelock.** A jump requested during the flush's own pass (a listener that jumps on every
   change) goes back into the slot and is flushed by the same loop, up to a fixed bound (for
   example 8). Past the bound the target stays in the slot with a `tracing::warn!`, and the
   command listener services it on the next notify or frame. The accepted target is kept, never
   dropped.
8. **Repair the half-refusal on the way.** Today a refused `jump_to` stops the fling through its
   `stop_hook` and drops a pending `animate_to` before the jump hook refuses
   (`scroll_controller.rs:267-305`, `scrollable.rs:736-750`), so the caller loses the jump *and*
   the motion. Under (d) both effects happen once, when the jump lands. An `animate_to` from inside
   a participant has the same defect today: `service_command` calls the fling controller while
   refused (`scroll_owner.rs:1270-1272`), the run carries no run tag, and `tick` drops its values
   (`scroll_owner.rs:709-722`). The same deferral fixes it. A reveal (`move_to`, `:1320`) is a
   programmatic request too and takes the same path. User-input dispatch from inside a participant
   (drag, wheel, key) stays refused, because a participant does not originate input.

### API shape

```rust
// crates/flui-widgets/src/scroll/scroll_controller.rs
impl ScrollController {
    /// Jump to `pixels`, clamped to the extents when the jump lands.
    ///
    /// Outside a scroll pass the jump lands before this returns. From inside one (a
    /// nested scroll participant, a position listener, a motion hook), it is recorded
    /// and lands when the outermost pass on this controller's chain returns, before
    /// the dispatch that ran it returns; until then `pixels()` reads the old offset.
    /// A later `jump_to` or `animate_to` replaces a jump that has not landed yet.
    ///
    /// When it lands it ends the run the scrollable drives (a fling, a settle, an
    /// animation) `Preempted`, never a contact or key still held, and moves the
    /// offset as one `Programmatic` step of a sequence of its own.
    pub fn jump_to(&self, pixels: f64);
}

/// One slot: the latest request wins.
pub(super) enum PendingScrollCommand {
    AnimateTo { target_pixels: f64, duration: Duration, curve: ArcCurve },
    /// A jump made while a pass ran; clamped when it lands.
    JumpTo { target_pixels: f64 },
    Cancel,
}

// crates/flui-widgets/src/scroll/scroll_owner.rs
pub(crate) struct ScrollOwner {
    /// Passes on this owner's chain in progress: nonzero defers programmatic
    /// requests and refuses user-input dispatch from a participant.
    passes: Cell<u32>,
    // `refusing` folds into `passes`, alongside the other fields.
}

/// Held for the length of one `ScrollSequence::dispatch`, `hand_off`, `release`
/// or `end`; dropping the outermost one services the slot (bounded, under the
/// pass's `PanicRecovery`).
struct PassScope<'a> { sequence: &'a ScrollSequence, recovery: &'a mut PanicRecovery }
```

`jump_to` checks the owner's pass count through the jump hook. When the count is zero it behaves as
today. When it is non-zero it writes `JumpTo` into the slot (with a fresh authority, as
`replace_pending_command` already mints) and returns. A non-finite target is dropped at the call
and does not replace a pending request. No new `pub` item is added.

### Test rows

These are rows of the `nested_scroll_channel` table (`crates/flui-widgets/tests/contracts.rs`),
each scripted for touch and mouse like the existing rows:

| Row | Pins | Fails today because |
|---|---|---|
| `a_jump_from_a_participant_lands_when_the_pass_returns` (replaces the jump half of `reentrant_dispatch_is_refused`) | A `pre_scroll`, `post_scroll`, `released`, `post_fling` or `sequence_ended` that calls `jump_to(500)`: right after that step, `pixels() == 500`, clamped; the contact's next delta moves from 500; the jump's `sequence_started` comes after the requesting step's last callout | The jump is refused: `pixels()` stays in 100..200 (`tests/nested_scroll.rs:1253-1282`) |
| `a_jump_from_a_position_listener_never_nests_inside_a_step` | A listener that jumps during the leaf's publish: no `Programmatic` callout between one step's `pre_scroll` and its `post_scroll`, and the outer step's `consumed_by_child` matches what the leaf took | The jump runs nested inside the leaf's consume closure |
| `requests_made_in_one_pass_keep_the_latest` | jump 300 then jump 700 → one programmatic sequence to 700; jump then `animate_to` → the animation runs; `animate_to` then jump → the jump lands and no run starts | Both are lost or half-applied |
| `a_landing_jump_preempts_the_run_and_keeps_live_input` | A jump asked from a fling tick: the fling's sequence ends `Preempted` before the jump's starts, and no residual is offered; asked during a drag or a held key: that sequence stays live and is not ended | The refused jump stops the fling through `stop_hook` and moves nothing |
| `a_jump_asked_before_a_participant_panics_still_lands` | Single panic, two competing, and the next sequence after containment: the jump lands, the first payload resumes, and the next sequence includes the participant again | The jump is refused |
| `a_jump_feedback_loop_is_bounded_and_keeps_its_target` | A listener that jumps to `pixels() + 1` on every change: the dispatch returns after the bound, a warning is logged, and the last target lands on the next notify | Not reachable today; under the new rule it would spin without the bound |
| `a_jump_pending_across_an_unmount_writes_the_position` | The leaf is unmounted in the requesting pass: the controller's position ends at the clamped target with no owner | — (the no-owner fallback, pinned so the slot never strands an accepted target) |

`reentrant_dispatch_is_refused` keeps the half that is still a refusal (a participant that
dispatches a drag, wheel or key step into its own chain), and is renamed to say so. `rg` the old
name in ADR-0209 and `ARCHITECTURE.md` before renaming it.

## 1. What FLUI does today

### 1.1 The refusal

- `ScrollSequence::call` wraps every user-connection callout in `RefuseDispatch::over(self)`. That
  raises `refusing` on every scrollable of the snapshot for the length of the callout
  (`crates/flui-widgets/src/scroll/nested/chain.rs:711-748`, `:772-795`).
- `ScrollOwner::refused()` logs `tracing::error!` and returns `true`
  (`crates/flui-widgets/src/scroll/scroll_owner.rs:326-336`). `jump` then returns `true`, which
  reads as "handled", so the controller does not write the position either (`:1236-1243`,
  `scroll_controller.rs:283-292`).
- By the time the refusal happens, `ScrollController::jump_to` has already replaced the pending
  command with `Cancel` and run `stop_hook` (`fling.stop()`), so a pending `animate_to` is dropped
  and a fling in flight is stopped (`scroll_controller.rs:267-282`, `scrollable.rs:736-750`). The
  refusal is partial: the jump is lost, but its side effects have already happened.
- The pinned row is `reentrant_dispatch_is_refused`: a `pre_scroll` that calls `jump_to(500)` must
  leave the drag's own offset (`crates/flui-widgets/tests/nested_scroll.rs:1253-1282`).

### 1.2 The listener path is not refused

`ScrollOwner::publish` writes through `ScrollController::set_pixels`
(`scroll_owner.rs:283-295`), and `ScrollPosition::set_pixels_with_recovery` notifies its listeners
synchronously (`crates/flui-rendering/src/view/scroll_position.rs:705-722`). That runs inside the
leaf's `consume` closure of `ScrollSequence::dispatch` (`chain.rs:465-570`), where no connection
callout is on the stack, so `refusing` is zero. A listener's `jump_to` therefore goes through: it
mints a programmatic sequence (`begin_programmatic`, `scroll_owner.rs:603-614`) and dispatches a
whole step through the same chain, between the outer step's `pre_scroll` and `post_scroll`. When
no contact is down, `drive` ends the outer contactless sequence before the outer pass has finished
(`:481-505`). The outer closure then reports a consumption computed from the pre-jump offset. The
two reentry paths disagree: one is refused with side effects, the other nests. Neither is a
contract.

## 2. Prior art

### 2.1 Flutter

- `ScrollPositionWithSingleContext.jumpTo` is synchronous: `goIdle()`, `forcePixels(value)`, a
  start/update/end notification triplet, then `goBallistic(0.0)`
  ([`scroll_position_with_single_context.dart#L197-L207`](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scroll_position_with_single_context.dart#L197-L207)).
  The contract says any active animation is cancelled and, if the user is scrolling, that action
  is cancelled
  ([`scroll_position.dart#L931-L941`](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scroll_position.dart#L931-L941)).
- **The drag is cancelled.** `goIdle` calls `beginActivity(IdleScrollActivity)`, which disposes
  the old activity and `_currentDrag`
  ([`scroll_position_with_single_context.dart#L114-L137`](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scroll_position_with_single_context.dart#L114-L137)).
  `ScrollDragController.dispose` calls `onDragCanceled`
  ([`scroll_activity.dart#L448-L454`](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scroll_activity.dart#L448-L454)),
  which is `Scrollable`'s `_disposeDrag`, so every later update of the same pointer is ignored
  (`_drag?.update`,
  [`scrollable.dart#L883-L918`](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scrollable.dart#L883-L918)).
  The finger stays down but the list no longer follows it.
- **Reentry is allowed and nests.** `setPixels` calls `notifyListeners()` and then
  `didUpdateScrollPositionBy`
  ([`scroll_position.dart#L366-L400`](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scroll_position.dart#L366-L400)),
  which dispatches a `ScrollUpdateNotification` up the tree synchronously
  ([`scroll_position.dart#L1045-L1062`](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scroll_position.dart#L1045-L1062),
  [`scroll_activity.dart#L104-L115`](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scroll_activity.dart#L104-L115)).
  A listener or `NotificationListener` that calls `jumpTo` there disposes the running activity from
  inside its own tick. `BallisticScrollActivity._tick` calls `applyMoveTo` → `setPixels`
  ([`scroll_activity.dart#L617-L635`](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scroll_activity.dart#L617-L635))
  and its `dispose` disposes the `AnimationController`
  ([`#L666-L670`](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scroll_activity.dart#L666-L670)).
  The jump's start/update/end notifications nest inside the outer update notification, and the
  rest of the outer `setPixels` (`didOverscrollBy`) runs against the new activity.
- **The only phase guard** is an assert that pixels do not change during build, layout and paint
  (`setPixels`, `#L369-L372` in the same file). There is no guard during notification dispatch.
- `NestedScrollView`'s coordinator does the same across its positions: `goIdle`, a local jump per
  position, `goBallistic(0.0)`
  ([`nested_scroll_view.dart#L947-L954`](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/nested_scroll_view.dart#L947-L954)),
  and its `beginActivity` disposes its drag too (`#L702-L718`).
- What it reports: notifications carry metrics and `scrollDelta`, plus a `dragDetails` field that
  is null for a jump. The activity type is the only "cause". There is no sequence identity.

### 2.2 Jetpack Compose

- **Programmatic scrolls go through `scroll {}` and a `MutatorMutex`.** `ScrollState.scrollTo` is
  `suspend` and calls `scrollBy`
  ([`Scroll.kt#L210-L220`](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/Scroll.kt#L210-L220)),
  which runs inside `scroll { }` at the default priority
  ([`ScrollExtensions.kt#L81-L85`](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/gestures/ScrollExtensions.kt#L81-L85)).
  `DefaultScrollableState.scroll` is `scrollMutex.mutateWith(scrollScope, scrollPriority)`
  ([`ScrollableState.kt#L198-L220`](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/gestures/ScrollableState.kt#L198-L220)).
- **Priority decides who is cancelled.** `MutatePriority` is `Default < UserInput <
  PreventUserInput`, and `Default` is documented for programmatic changes that should not
  interrupt user input
  ([`MutatorMutex.kt#L27-L54`](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/MutatorMutex.kt#L27-L54)).
  `tryMutateOrCancel` cancels the current mutator when the new one has equal or higher priority.
  Otherwise the *new* caller throws `CancellationException("Current mutation had a higher
  priority")`
  ([`#L90-L100`](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/MutatorMutex.kt#L90-L100)).
  A drag runs at `UserInput`
  ([`Scrollable.kt#L340-L357`](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/gestures/Scrollable.kt#L340-L357))
  and a fling at `Default` (`#L728` in the same file). So `scrollTo` during a drag fails in the
  caller and leaves the drag alone, while `scrollTo` during a fling cancels the fling. This is
  Compose's form of option (a), and it is the closest match to FLUI's "input preempts programmatic,
  never the reverse".
- **A connection cannot reenter synchronously.** `onPreScroll` and `onPostScroll` are plain
  functions; only the fling hooks are `suspend`
  ([`NestedScrollModifier.kt#L52-L92`](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/input/nestedscroll/NestedScrollModifier.kt#L52-L92)).
  Calling the `suspend fun scrollTo` from `onPreScroll` needs a `launch`, which defers it past the
  pass, where it then meets the mutex. The type system does the deferral.
- **The escape hatch is unsynchronised.** `dispatchRawDelta` bypasses nested scroll, does not stop
  an ongoing scroll and ignores priority
  ([`ScrollableState.kt#L58-L73`](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/gestures/ScrollableState.kt#L58-L73)).
  A parent scrollable, as a nested connection, takes a child's remainder through
  `performRawScroll`, which returns zero while its own state `isScrollInProgress`
  ([`Scrollable.kt#L651-L688`](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/gestures/Scrollable.kt#L651-L688),
  [`AbstractScrollableNode.kt#L211-L220`](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/gestures/AbstractScrollableNode.kt#L211-L220)).
  A parent that is being scrolled programmatically refuses the child's deltas rather than
  interleaving.
- **The non-suspend jump is deferred and coalesced.** `LazyListState.requestScrollToItem`
  records the requested position, invalidates measurement and applies it "during the next
  remeasure", cancelling a scroll in progress by launching an empty `scroll {}`
  ([`LazyListState.kt#L454-L506`](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/lazy/LazyListState.kt#L454-L506)).
  `requestPositionAndForgetLastKnownKey` overwrites one stored position, so the latest request
  wins. The cancelling `scroll {}` runs at `Default`, so by the mutex rule above it cannot cancel
  a `UserInput` drag (inferred from the code; not run). This is option (c), with the boundary at
  the next measure rather than the end of a pass.
- What it reports: `NestedScrollSource` (`UserInput`, `SideEffect`, ...) on each callback. There
  is no sequence identity. A programmatic `scrollBy` inside `scroll {}` dispatches nothing to
  `NestedScrollConnection`: it calls the state's delta function directly (`Scroll.kt#L146-L157`).

### 2.3 UIKit

- `setContentOffset(_:animated:)` with `animated == false` makes the transition immediate
  ([doc](https://developer.apple.com/documentation/uikit/uiscrollview/setcontentoffset(_:animated:))).
  `scrollViewDidScroll(_:)` is documented only as telling the delegate that the user scrolled
  ([doc](https://developer.apple.com/documentation/uikit/uiscrollviewdelegate/scrollviewdidscroll(_:))).
  Apple documents neither whether a `setContentOffset` inside `scrollViewDidScroll` re-enters the
  delegate, nor whether it ends `isTracking`/`isDragging` or deceleration. **Unverified here; it
  needs a device run, not a citation.**
- Stopping motion is a separate call: `stopScrollingAndZooming()` stops scroll and zoom
  animations, and leaves the offset where it is unless the view is bouncing or paging
  ([doc](https://developer.apple.com/documentation/uikit/uiscrollview/stopscrollingandzooming())).
- **The sanctioned in-callback redirect is a write-back, not a move.**
  `scrollViewWillEndDragging(_:withVelocity:targetContentOffset:)` gives the delegate an inout
  target that it may change to adjust where the deceleration ends
  ([doc](https://developer.apple.com/documentation/uikit/uiscrollviewdelegate/scrollviewwillenddragging(_:withvelocity:targetcontentoffset:))).
  The scroll view reads the last value written once the callback returns. That is (c) at the
  callback boundary.
- `scrollViewDidEndScrollingAnimation` fires at the end of `setContentOffset(_:animated:)` only
  for animated requests
  ([doc](https://developer.apple.com/documentation/uikit/uiscrollviewdelegate/scrollviewdidendscrollinganimation(_:))),
  so an immediate jump reports no begin or end, only the did-scroll.

### 2.4 SwiftUI

- `ScrollViewReader`'s proxy must not be used while the content builder runs; doing so is a
  runtime error. It is used from actions such as gesture handlers and `onChange`
  ([doc](https://developer.apple.com/documentation/swiftui/scrollviewreader)). That is a refusal
  by phase, and it is loud.
- `ScrollViewProxy.scrollTo(_:anchor:)` scrolls to the first scroll view holding the identified
  child ([doc](https://developer.apple.com/documentation/swiftui/scrollviewproxy/scrollto(_:anchor:))).
  `ScrollPosition` is a value bound with `.scrollPosition(_:anchor:)`: writing it requests a
  position, which the framework applies on its next update, and the framework writes it back as
  the user scrolls
  ([doc](https://developer.apple.com/documentation/swiftui/scrollposition),
  [modifier](https://developer.apple.com/documentation/swiftui/view/scrollposition(_:anchor:))).
  A state write is coalesced by construction: the last value before the update is what applies.
  `scrollTo(point:)` is clamped to the content
  ([doc](https://developer.apple.com/documentation/swiftui/scrollposition/scrollto(point:))).
- **Cause is reported as a flag.** `isPositionedByUser` says whether the user positioned the
  view, and an app may write it
  ([doc](https://developer.apple.com/documentation/swiftui/scrollposition/ispositionedbyuser)).
  This is the analogue of `ScrollCause::Programmatic` versus `UserInput`, with no sequence.
- Whether a programmatic `scrollTo` interrupts a finger that is still down is not documented.

### 2.5 Masonry (Xilem) and GPUI

- **Masonry: the borrow checker refuses, and a queue defers.** `Portal::set_viewport_pos` takes
  `&mut WidgetMut<'_, Self>`
  ([`portal.rs#L358-L378`](https://github.com/linebender/xilem/blob/271a27a6d4a930f7878d404f9014e3c50a3a9b88/masonry/src/widgets/portal.rs#L358-L378)),
  which only the owner of the `RenderRoot` or the mutate pass holds. From inside a pass, code calls
  `mutate_later`, which pushes onto a FIFO that runs "in the order they were submitted during the
  mutate pass"
  ([`contexts.rs#L1754-L1768`](https://github.com/linebender/xilem/blob/271a27a6d4a930f7878d404f9014e3c50a3a9b88/masonry_core/src/core/contexts.rs#L1754-L1768),
  [`mutate.rs#L66-L78`](https://github.com/linebender/xilem/blob/271a27a6d4a930f7878d404f9014e3c50a3a9b88/masonry_core/src/passes/mutate.rs#L66-L78)).
  Callbacks whose target has gone are skipped. The docs call that pass an escape hatch
  ([`pass_system.md#L84-L97`](https://github.com/linebender/xilem/blob/271a27a6d4a930f7878d404f9014e3c50a3a9b88/masonry_core/src/doc/pass_system.md#L84-L97)).
  This is option (b), generic rather than scroll-specific. Nesting is whole-event: a child sets
  the event handled to prevent double scrolling (`portal.rs#L42-L44`), and there is no partial
  delta, no fling and no cause.
- **GPUI: one slot, applied at prepaint.** `ScrollHandle::scroll_to_item` and
  `scroll_to_top_of_item` overwrite a single `active_item`, and `scroll_to_bottom` sets a flag
  ([`div.rs#L4221-L4230`, `#L4321-L4395`](https://github.com/zed-industries/zed/blob/56934ec0560bce8929025497411de7f5322fb6ca/crates/gpui/src/elements/div.rs#L4321-L4395)).
  Both are consumed during prepaint (`div.rs#L2016`, `#L2454`), and an item not laid out yet stays
  pending. `UniformListScrollHandle` keeps one `deferred_scroll_to_item` and `take()`s it in
  prepaint
  ([`uniform_list.rs#L117`, `#L370-L379`](https://github.com/zed-industries/zed/blob/56934ec0560bce8929025497411de7f5322fb6ca/crates/gpui/src/elements/uniform_list.rs#L370-L379)).
  That is option (c) at the frame boundary. `ScrollHandle::set_offset` writes the shared offset
  immediately, with no notification or cancellation (`div.rs#L4397-L4403`).
  `ListState::scroll_to` writes the logical top and rebases a pending remeasure adjustment onto
  it, so the newer request wins over the older correction
  ([`list.rs#L659-L674`](https://github.com/zed-industries/zed/blob/56934ec0560bce8929025497411de7f5322fb6ca/crates/gpui/src/elements/list.rs#L659-L674),
  `#L844-L855`). GPUI reports no cause or sequence.

### 2.6 Summary

| | Reentrant programmatic jump | Lands | Coalesced | Cancels a fling | Cancels a held drag | Reports |
|---|---|---|---|---|---|---|
| Flutter `jumpTo` | applied, nested | now | — | yes | **yes**, rest of gesture ignored | notification triplet, no cause |
| Compose `scrollTo` | cannot be synchronous (`suspend`) | after the pass, via a coroutine | no (mutex) | yes (`Default` ≥ `Default`) | **no**, the caller is cancelled | `NestedScrollSource`, not to connections |
| Compose `requestScrollToItem` | recorded | next remeasure | yes | yes | no (inferred) | — |
| UIKit `setContentOffset` | undocumented | now | — | undocumented | undocumented | `scrollViewDidScroll` |
| UIKit `targetContentOffset` | write-back in callback | after the callback | yes | — | — | — |
| SwiftUI `ScrollPosition` | state write; proxy refused in builder | next update | yes | undocumented | undocumented | `isPositionedByUser` |
| Masonry | refused by `&mut`; `mutate_later` | mutate pass | no (FIFO) | n/a | n/a | — |
| GPUI handles | recorded | prepaint | yes | n/a | n/a | — |
| **FLUI (d)** | recorded | end of outermost pass, same dispatch | yes, shared with `animate_to` | yes, `Preempted` | **no** | own sequence, `Programmatic` |

## 3. Trade-offs

- **(a) `Result`.** It is honest and local, and Compose's mutex amounts to it. But the refusal is
  a property of *when* the caller runs, not of its arguments, so every caller needs a retry path.
  A listener has none without a post-frame handle, which `BuildContext` code cannot acquire
  (ADR-0078). It also turns a frequent and legitimate pattern into an error branch, and a
  `#[must_use]` there would be noise; without it, the jump is lost silently again. Rejected.
- **(b) FIFO.** It delivers everything, but the intermediate targets are meaningless for an
  absolute jump. Each would emit a sequence that participants (refresh, collapsing header) react
  to. A listener that jumps on every change would grow the queue, and it disagrees with the slot
  `animate_to` already uses. Masonry needs a FIFO because `mutate_later` is generic. Rejected for
  jumps.
- **(c) latest-wins, with the boundary unspecified.** It is right on what to keep. Left open, the
  boundary drifts to "next frame" (GPUI, Compose's remeasure): a frame paints the stale offset, and
  the next input event's step runs against a position the app already replaced. FLUI's pass is
  synchronous and short, so the end of the outermost pass is a boundary it can name exactly.
- **Applying in place (Flutter).** Nothing is lost and there is no deferral to explain. But the
  jump runs a second sequence through participants in the middle of a step, and the outer step's
  bookkeeping (consumption, latch, edge, run lag) is computed from a stale offset. That is the
  reentrant-user-code defect class AGENTS.md names, and ADR-0209 §7 already rejects it for
  participants. Rejected.
- **Cancelling the drag (Flutter) or keeping it (Compose, FLUI).** Flutter's choice leaves a
  finger down on a list that no longer follows it until the next touch. Keeping the contact means a
  "scroll to top" fired during a drag can be dragged away from again, which matches what the user is
  doing and ADR-0209 §3's priority. An app that wants to stop the user needs a separate, explicit
  hold (out of scope here).
- **Cost of (d).** One `Cell<u32>` per owner, one `PassScope` per pass (no allocation: the slot
  already exists, so `steady_state_dispatch_allocates_nothing` holds), and one documented
  surprise: `pixels()` read inside the pass right after `jump_to` still shows the old offset.
  Compose's `requestScrollToItem` and SwiftUI's `ScrollPosition` share that surprise.
- **Platform momentum.** A jump cannot stop OS inertia (`ScrollCause::Momentum`). The next momentum
  packet continues its own sequence from the new offset. Whether that feels right on macOS and
  Windows precision touchpads needs a native run; it does not change the contract.

## 4. What the ADR-0209 amendment should record

Amend §3 (one offset primitive) and §7 (participant contract). The text:

> **Programmatic requests during a pass.** A pass is one dispatch through a chain, from entry to
> return. A `ScrollController::jump_to`, `animate_to` or reveal requested while a pass runs on the
> requesting scrollable's chain (from a participant, a position listener or a motion hook) is not
> refused: it is recorded in the controller's one command slot, where the latest request replaces
> any not yet applied, and it is applied when the outermost pass returns, before the dispatch that
> ran it returns. The target is clamped to the extents then. A request made while that application
> runs is applied by the same loop, up to a fixed bound; past it, the last target waits for the
> next service with a diagnostic and is never dropped. A panic in the pass does not drop the
> request: it is applied under the pass's containment, and the first failure stays authoritative
> (ADR-0127). If the owner closes on the same position, the successor applies it (§8). With no
> owner, the controller writes the clamped position.
>
> **What a jump cancels and reports.** When a jump is applied, it ends the run the owner drives (a
> fling, a settle, an animation, an accepted residual) `Preempted`, and that run offers no residual.
> It never ends a contact or key still held or a drop autoscroll; those continue from the new
> offset under their own sequence and latch. The jump is a sequence of its own: a freshly minted
> `InputSequenceId`, `ScrollCause::Programmatic`, `ScrollDevice::Programmatic`, `sequence_started`,
> one step, no `released`, `sequence_ended(Released)`. A preempted sequence's `sequence_ended`
> comes before the jump's `sequence_started`.
>
> **Still refused.** A user-input step (drag, wheel, key) dispatched from inside a participant into
> its own chain is refused with a diagnostic.

Row list: replace `reentrant_dispatch_is_refused` with the rows in the table above, and keep the
user-input half under a name that says it refuses input.

## 5. Open, unverified on this host

- UIKit's behaviour for `setContentOffset` inside `scrollViewDidScroll` (delegate reentry,
  tracking, deceleration) and SwiftUI's for `scrollTo` while a finger is down are undocumented.
  Both need a device run if parity is ever claimed.
- Compose's `requestScrollToItem` not cancelling a `UserInput` drag is inferred from
  `MutatorMutex` priorities, not run.
- FLUI's current listener-reentry path (1.2) was traced by reading. The second row of the table
  is the test that makes it observable.

## Sources

FLUI (`interaction-integration` @ `30bfcee6a`):
`docs/adr/ADR-0209-nested-scroll-connection.md`; `crates/flui-widgets/src/scroll/nested/chain.rs`
(`dispatch` 465–570, `call` 711–748, `RefuseDispatch` 772–795);
`crates/flui-widgets/src/scroll/scroll_owner.rs` (`refuse_dispatch`/`refused` 317–336, `drive`
481–505, `begin_programmatic` 603–614, `tick` 709–722, `jump` 1236–1253, `service_command`
1265–1294, `move_to` 1314–1323); `crates/flui-widgets/src/scroll/scroll_controller.rs`
(`PendingScrollCommand` 76–96, `jump_to` 267–305); `crates/flui-widgets/src/scroll/scrollable.rs`
(`install_stop_hook` 736–750, command listener 801–809);
`crates/flui-rendering/src/view/scroll_position.rs` (`set_pixels_with_recovery` 705–722);
`crates/flui-widgets/tests/nested_scroll.rs` (`reentrant_dispatch_is_refused` 1253–1282);
`crates/flui-widgets/ARCHITECTURE.md`, scroll sections.

Flutter (`flutter/flutter@dd45f0c2`, `packages/flutter/lib/src/widgets/`):
[`scroll_position.dart`](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scroll_position.dart);
[`scroll_position_with_single_context.dart`](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scroll_position_with_single_context.dart);
[`scroll_activity.dart`](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scroll_activity.dart);
[`scrollable.dart`](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scrollable.dart);
[`scroll_controller.dart`](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/scroll_controller.dart);
[`nested_scroll_view.dart`](https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/packages/flutter/lib/src/widgets/nested_scroll_view.dart).

Compose (`androidx/androidx@9a589fa6`, `compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/`):
[`MutatorMutex.kt`](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/MutatorMutex.kt);
[`Scroll.kt`](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/Scroll.kt);
[`gestures/ScrollableState.kt`](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/gestures/ScrollableState.kt);
[`gestures/ScrollExtensions.kt`](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/gestures/ScrollExtensions.kt);
[`gestures/Scrollable.kt`](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/gestures/Scrollable.kt);
[`gestures/AbstractScrollableNode.kt`](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/gestures/AbstractScrollableNode.kt);
[`lazy/LazyListState.kt`](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/lazy/LazyListState.kt);
[`ui/input/nestedscroll/NestedScrollModifier.kt`](https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/input/nestedscroll/NestedScrollModifier.kt).

Apple developer documentation:
[`UIScrollView`](https://developer.apple.com/documentation/uikit/uiscrollview);
[`setContentOffset(_:animated:)`](https://developer.apple.com/documentation/uikit/uiscrollview/setcontentoffset(_:animated:));
[`stopScrollingAndZooming()`](https://developer.apple.com/documentation/uikit/uiscrollview/stopscrollingandzooming());
[`isScrollAnimating`](https://developer.apple.com/documentation/uikit/uiscrollview/isscrollanimating);
[`scrollViewDidScroll(_:)`](https://developer.apple.com/documentation/uikit/uiscrollviewdelegate/scrollviewdidscroll(_:));
[`scrollViewWillEndDragging(_:withVelocity:targetContentOffset:)`](https://developer.apple.com/documentation/uikit/uiscrollviewdelegate/scrollviewwillenddragging(_:withvelocity:targetcontentoffset:));
[`scrollViewDidEndScrollingAnimation(_:)`](https://developer.apple.com/documentation/uikit/uiscrollviewdelegate/scrollviewdidendscrollinganimation(_:));
[`ScrollViewReader`](https://developer.apple.com/documentation/swiftui/scrollviewreader);
[`ScrollViewProxy.scrollTo(_:anchor:)`](https://developer.apple.com/documentation/swiftui/scrollviewproxy/scrollto(_:anchor:));
[`ScrollPosition`](https://developer.apple.com/documentation/swiftui/scrollposition);
[`scrollPosition(_:anchor:)`](https://developer.apple.com/documentation/swiftui/view/scrollposition(_:anchor:));
[`ScrollPosition.scrollTo(point:)`](https://developer.apple.com/documentation/swiftui/scrollposition/scrollto(point:));
[`ScrollPosition.isPositionedByUser`](https://developer.apple.com/documentation/swiftui/scrollposition/ispositionedbyuser).

Masonry (`linebender/xilem@271a27a6`):
[`masonry/src/widgets/portal.rs`](https://github.com/linebender/xilem/blob/271a27a6d4a930f7878d404f9014e3c50a3a9b88/masonry/src/widgets/portal.rs);
[`masonry_core/src/core/contexts.rs`](https://github.com/linebender/xilem/blob/271a27a6d4a930f7878d404f9014e3c50a3a9b88/masonry_core/src/core/contexts.rs);
[`masonry_core/src/passes/mutate.rs`](https://github.com/linebender/xilem/blob/271a27a6d4a930f7878d404f9014e3c50a3a9b88/masonry_core/src/passes/mutate.rs);
[`masonry_core/src/doc/pass_system.md`](https://github.com/linebender/xilem/blob/271a27a6d4a930f7878d404f9014e3c50a3a9b88/masonry_core/src/doc/pass_system.md).

GPUI (`zed-industries/zed@56934ec0`, `crates/gpui/src/elements/`):
[`div.rs`](https://github.com/zed-industries/zed/blob/56934ec0560bce8929025497411de7f5322fb6ca/crates/gpui/src/elements/div.rs);
[`uniform_list.rs`](https://github.com/zed-industries/zed/blob/56934ec0560bce8929025497411de7f5322fb6ca/crates/gpui/src/elements/uniform_list.rs);
[`list.rs`](https://github.com/zed-industries/zed/blob/56934ec0560bce8929025497411de7f5322fb6ca/crates/gpui/src/elements/list.rs).
