# flui-scheduler Architecture

Per-crate ledger for architecture decisions that span more than one module in
this crate, as required by [`docs/PORT.md`](../../docs/PORT.md) §Per-crate
`ARCHITECTURE.md` template. Partial: this file exists for the `## Mapping
decisions` entries below; a full crate architecture writeup is deferred.

---

## Mapping decisions

### One recovery boundary closes phase/completion state before any pre-pipeline panic propagates

**Rule:** a caller that catches a panic out of `drive_frame`/`drive_frame_with_lane`/
`execute_frame`/`execute_frame_with_lane` must find the scheduler's own
bookkeeping — phase, `frame_scheduled`, frame count, and every registered
completion waiter — closed, regardless of which phase inside that call
raised the panic.

**Conflict:** issue #1057 found that `drive_frame_impl` only wrapped the
caller-supplied `pipeline` closure in `catch_unwind`; `handle_begin_frame`
and `handle_draw_frame` ran outside it. A panic from a transient callback, a
mid-frame async-driver poll, a persistent callback, or a `Priority::Build`/
`Animation`/`Idle` task — every one of which runs before the pipeline slot
ever opens — escaped straight past `abort_frame` and left the phase machine
stuck at whatever phase it reached (`TransientCallbacks`,
`MidFrameMicrotasks`, or `PersistentCallbacks`), `frame_scheduled` in
whatever state it happened to be, and every `end_of_frame()` waiter
unresolved forever. `execute_frame`/`execute_frame_with_lane` had the
identical gap: they called `handle_begin_frame`/`handle_draw_frame`/
`end_frame` directly, sharing no recovery boundary with `drive_frame` at
all.

**Choice:** `drive_frame_impl` now wraps `handle_begin_frame`,
`handle_draw_frame`, AND `pipeline` in ONE `catch_unwind`; a panic from any
of the three still runs `abort_frame` before the payload resumes.
`execute_frame`/`execute_frame_with_lane` route through the SAME
`drive_frame_impl` with a no-op pipeline (returning the `FrameId`
`handle_begin_frame` minted alongside the pipeline's own result, since
`drive_frame_impl` now returns `(FrameId, R)` internally — `drive_frame`/
`drive_frame_with_lane` discard the id to keep their existing `-> R`
signature) rather than hand-rolling a second, unguarded sequence that could
silently reopen the same gap later.

**Per-queue policy, so "closed" does not also mean "lossy":** every queue
this scheduler drains before the pipeline runs now pops one entry at a time
and invokes it OUTSIDE the queue's lock, rather than draining a whole
matching batch into a local buffer before running any of it. Each is bounded
at entry, before invoking anything — but by two DIFFERENT mechanisms, chosen
per queue rather than uniformly, after review found each one wrong on the
other's queue (see the two per-queue entries below for the measured
regressions each direction produced):

- `handle_begin_frame`'s transient-callback loop pops from the front of
  `CallbackState::transient` (now a `VecDeque`, not a `Vec`, to make the pop
  O(1) — the same reason `flush_microtasks` already used one), bounded by
  an id WATERMARK: the id of the LAST (`back()`) entry queued at entry, read
  once before invoking anything. `CallbackId`s are minted from this
  scheduler's own monotonic `id_gen` in registration order, so `back().id`
  at entry is exactly the highest id already queued, in O(1) — no scan
  needed, since a `VecDeque` already keeps registration order — but ONLY
  because `schedule_frame_callback` mints that id INSIDE the same
  `transient` lock acquisition as the push, making id order and push order
  the same fact rather than two facts that could disagree. Minting the id
  BEFORE acquiring the lock (tried first, and wrong) let two threads racing
  this method push `[id6, id5]`: `back().id` (5) then sits BELOW
  `front().id` (6), so the watermark loop's very first check fails and
  NEITHER callback ever runs, every frame, until a later registration
  happens to raise the watermark — a permanent stall from two lone tickers
  registered concurrently, measured via a repeated two-thread race (a
  single race is not reliably reproducible on a timer). A callback that
  registers another transient callback of its own from inside itself must
  still have the new one deferred to the NEXT frame (issue #1058's
  reentrant-registration contract): the fresh entry always gets a strictly
  greater id and is excluded by `front().id <= watermark` regardless of
  where it lands. A remaining-COUNT bound (tried first, and wrong, for a
  different reason) does not survive cancellation: a callback that cancels
  a not-yet-run sibling (`cancel_frame_callback` removes it from the live
  queue directly, #1156) and registers a fresh one in its place leaves the
  queue's LENGTH unchanged from that callback's own perspective, so a count
  budget still reaches the fresh entry in the SAME call — measured: it ran
  in the same frame it was registered in.
- [`TaskQueue::execute_until`](src/task.rs) pops the top of the heap only
  while it still meets the priority threshold, one task at a time, bounded
  by a COUNT: `queue.len()` at entry, read once under the first lock
  acquisition, decremented once per pop, the loop stopping the moment
  EITHER the budget is spent or the top's priority fails the threshold. A
  count is sound here specifically because the heap has no mid-scan removal
  API the way the transient deque's `cancel_frame_callback` does — nothing
  can shrink the heap's contents out from under a budget read at entry
  except this same call's own pops, so nothing can make a stale count lie.
  An id watermark (tried first, and wrong) needed a side buffer to let a
  reentrant task of the SAME or a HIGHER priority than something
  still-eligible be popped, set aside, and skipped past without ending the
  scan early — and that buffer is a plain local `Vec`, dropped on unwind
  before it is ever re-queued: a LATER task in the same pass panicking lost
  every task the scan had already set aside, measured (`A` enqueues `X`,
  `B` panics → the heap is empty afterward, `X` never runs) as a NEW
  regression the pre-#1057 batch drain never had, since nothing there ever
  left the heap except what was already executing. The count budget has no
  buffer to lose anything from, at the cost of a different, accepted
  trade-off: a reentrant task of a HIGHER priority than a still-queued
  sibling does not defer behind it the way an id watermark would — it
  DISPLACES it, consuming the budget slot the sibling would otherwise have
  used, pushing the sibling to the NEXT call rather than losing it. Simply
  re-peeking the live heap with no bound at all (tried before either of
  these) does not survive genuine reentrancy: a self-re-enqueuing task runs
  without limit inside ONE call — measured: 500+ executions and no return,
  hanging the frame, because `handle_draw_frame`'s own
  `MAX_BUILD_REENTRY_PASSES` cap only ever fires BETWEEN calls to this
  method and never got the chance.
- `flush_microtasks` already had a pop-one-under-lock shape (the model the
  other two now follow for that part). Before issue #1159 it carried no
  bound at all beyond "until the queue is empty" — an unaudited state, not
  silently assumed correct, since no reentrant-registration contract was
  documented for microtasks the way issue #1058 documents one for transient
  callbacks. Issue #1159 closed that gap: `flush_microtasks` is now an outer
  loop over `flush_microtasks_pass`, a private helper with the SAME
  count-budget shape as `TaskQueue::execute_until` (`queue.len()` read once,
  under the first lock acquisition, decremented once per pop), capped at
  `MAX_MICROTASK_REENTRY_PASSES` (32) outer passes with a `tracing::warn!`
  fired once per call on the cap with work still queued, not a
  process-wide latch. A microtask
  enqueued reentrantly BY another one in the same pass still runs, one pass
  later, before this flush call returns — Dart's own nested-microtask
  semantics are preserved — **but only up to the cap.** A FINITE chain
  deeper than 32 passes (33 levels of nesting, no genuinely unbounded
  re-enqueuing anywhere in it) is deferred exactly like an unbounded one:
  whatever the cap leaves queued waits for the NEXT `flush_microtasks` call
  (the following frame's `handle_begin_frame`), not this one. The cap
  cannot tell a genuinely unbounded, self-re-enqueuing chain apart from a
  merely deep finite one — both trip the SAME warning at the SAME depth,
  and only the unbounded case would otherwise hang the frame. This bounds
  reentrancy *depth*, not *width* — the same caveat `MAX_BUILD_REENTRY_PASSES`
  states for the Build/Animation drain: a pass whose every popped microtask
  enqueues two more still finishes in 32 passes, not because the fan-out is
  bounded, but because passes are. Unlike the Build cap, there is no
  trailing, unconditional second sweep here (`handle_draw_frame`'s own
  `execute_until(Priority::Idle)` has no microtask analogue), so a capped
  chain's observed run count is exactly the cap, not `+ 1`. A capped flush's
  unrun leftovers stay queued and request nothing of their own —
  `schedule_microtask` has never called `request_frame` the way `TaskQueue::add`
  does for Build-or-higher priority, and this cap does not change that
  pre-existing contract; a leftover simply waits for the next frame's
  `handle_begin_frame` to flush it. Divergence from Dart, recorded rather
  than silently improved: Dart's own microtask queue has no such outer
  cap at all — a browser or VM microtask that re-enqueues itself forever
  genuinely starves that isolate, and even a merely deep FINITE chain runs
  to completion in the SAME turn there; FLUI trades a one-frame deferral on
  that legitimate-but-deep case, plus a small chance of under-running a
  genuinely pathological one, for the guarantee that no single frame hangs.

The result: **the panicking entry itself is consumed; every entry still
queued behind it is preserved and runs on the next completed frame — not
retried, not silently dropped.** Persistent callbacks need no such
treatment at all: `handle_draw_frame` clones the registration list rather
than draining it (they "cannot be unregistered", per their own doc), so a
panic there costs nothing structurally — the same callback (including the
one that panicked) simply runs again next frame, same as always.

**The frame's post-frame callbacks do not run** for an aborted frame —
unchanged from before this issue; see `abort_frame`'s own doc. **Completion
waiters are woken with the aborted frame's outcome** — an aborted frame is a
frame that finished, badly, and `end_of_frame()` must not hang forever
because of it. It resolves `Ok(FrameOutcome::Aborted { timing })`, distinct
from a successful frame's `Ok(FrameOutcome::Completed { timing })`: see this
file's own `end_of_frame` resolves an outcome, and a dropped scheduler
resolves `Err(SchedulerClosed)` entry below for the full contract and why
issue #1162 shaped it this way.

**The `frame_scheduled` contract for a catcher:** by the time any callback
this recovery could be catching a panic from has even run,
`handle_begin_frame` has already cleared `frame_scheduled` back to `false`
unconditionally (before the transient-callback loop), so it reads `false`
once `abort_frame` returns — the same as after any other completed frame.
`abort_frame` deliberately does **not** re-arm it: doing so would hot-loop
a caller that keeps re-driving a deterministically panicking frame. A
caller that catches the resumed panic and decides to keep going owns
calling `request_frame()` itself, exactly as it would after any other frame
that produced no visible work. See `drive_frame`/`abort_frame`'s own docs.

**The async zombie-slot fix (`AsyncDriver::poll_ready`):** a panic inside
`future.poll` used to leave that task's slot in `Inner::tasks` holding
`future: None` forever — nothing else on the normal path ever revisits a
slot that is not `ready` and was never explicitly cancelled, so
`pending_task_count` counted a task that could neither be polled again nor
be cancelled by a caller whose `TaskToken` had already been dropped or was
never checked. Fixed with a small `Drop`-based guard, armed only for the
duration of the `poll` call and disarmed immediately on a normal return:
on unwind it removes the slot. Taking the map lock in that `Drop` is safe
specifically because `poll_ready` never holds it across a poll, and the
removed `Task` already has `future: None` — dropping it runs no user code.

**`notify_frame_completion` no longer holds a waiter's lock across
`wake()`:** the previous implementation locked `notifier.state` and called
`waker.wake()` while still holding that guard. A waker whose `wake()`
immediately re-polls the SAME `FrameCompletionFuture` — an inline-polling
executor, not merely one that schedules a later poll — locks that identical
`Arc<Mutex<FrameCompletionState>>` from inside `poll()`; held across the
call, that self-relock on the non-reentrant `parking_lot::Mutex` never
returns. The waker is now taken out from under the lock and called only
after it is released. A waker that panics is caught and traced
(`tracing::error!` with `payload_text`) rather than aborting the
notification loop — every OTHER waiter still gets its `completed` value set
and its own waker called — and the first such panic is re-raised only once
every waiter has been notified, matching `end_frame_impl`'s own
catch-then-resume shape for a panicking post-frame callback.

**Honest Flutter comparison:** `.flutter/packages/flutter/lib/src/scheduler/binding.dart`
@ 3.44.0 cannot unwind out of either `handleBeginFrame` or `handleDrawFrame`
at all — `_invokeFrameCallback` wraps every individual transient/
persistent/post-frame callback in its own `FlutterError`-reporting boundary,
and `drawFrame()` — the pipeline's own equivalent — is itself registered and
invoked as a persistent callback through that SAME boundary
(`rendering/binding.dart:61`, `:557-558`), so no callback's exception,
`drawFrame`'s included, ever unwinds Dart's call stack far enough to reach
either `finally { _schedulerPhase = ... }` at all. Per-callback isolation is
what the source actually shows holding there; each `finally` covers whatever
else could still escape past it — the source does not say what that is, and
neither does this entry. FLUI does not isolate per callback — this issue
does not change that, and does not attempt to (see
`docs/PANIC-POLICY.md` and the port-check/doc note this crate already
carries on the topic) — a panic here still poisons and propagates the
whole frame. What this issue closes is narrower and Rust-specific: the
scheduler's OWN bookkeeping must never be left half-closed by an unwind it
did not choose to isolate.

**A secondary panic during recovery never displaces the original one.**
Two distinct sites can themselves panic while THIS issue's own recovery
runs, both discovered by review after the first pass shipped:

- `drive_frame_impl`'s `Err` arm calls `abort_frame`, which calls
  `notify_frame_completion` — and a panicking waker there is a second panic
  on top of the pipeline's (or `handle_begin_frame`/`handle_draw_frame`'s)
  own. `abort_frame` is now called inside its OWN `catch_unwind`; a
  secondary panic is traced at `error` level and the ORIGINAL payload is
  what `resume_unwind`s, always — measured before the fix: the escaped
  panic was the waker's, not the frame's, silently losing whichever failure
  the frame was actually reporting.
- `end_frame_impl`'s CLEAN path (`drive_frame`'s `Ok` arm) calls
  `notify_frame_completion` OUTSIDE the post-frame callback's own
  `catch_unwind`, so a panicking waker there used to escape straight past
  the phase reset at the end of that function, leaving the scheduler stuck
  at `PostFrameCallbacks` with `current_vsync_time` still set — the exact
  class of bug this issue exists to close, just reachable from the success
  path instead of the failure one. `notify_frame_completion` is now caught
  there too, and folded into the SAME `callback_result`-or-`notify_result`
  check that already ran the phase reset for a panicking post-frame
  callback: whichever of the two panicked, the phase still closes before
  either one resumes (the post-frame callback's payload wins if both did,
  since it panicked earlier in this frame's own order).

The general rule, in both places: **the panic that caused THIS frame to
need recovery is never displaced by a panic the recovery itself raises
while running.** A secondary panic is contained, logged, and never allowed
to become the one the caller observes.

**Alternatives considered:**

- A `Drop`-based guard around the whole `handle_begin_frame`/
  `handle_draw_frame`/`pipeline` sequence, instead of `catch_unwind` +
  explicit `abort_frame`. Rejected for the same reason `abort_frame`'s own
  doc already gives for the pipeline-only boundary: forcing
  `PersistentCallbacks -> Idle` (or `TransientCallbacks -> Idle`, or
  `MidFrameMicrotasks -> Idle`) trips the phase validator's `debug_assert!`
  *while already panicking* — a double panic, i.e. `abort` — and running
  user-callback-adjacent cleanup during an unwind is a second hazard for no
  benefit.
- Isolating each callback family per-entry (Flutter's own shape), so a
  panicking transient callback cannot take the rest of the frame down with
  it. Rejected: this crate's panic policy is that a panic poisons the whole
  operation (`docs/PANIC-POLICY.md`); adopting Flutter's per-callback
  isolation here would be a policy change well beyond this issue's scope,
  not a bookkeeping fix.

**Trade-off accepted:** `TaskQueue::execute_until`'s count budget also accepts
that a reentrant HIGHER-priority task displaces a still-queued, lower-
priority sibling to the next call rather than deferring behind it the way
`handle_begin_frame`'s id watermark does for transient callbacks — named
above, and covered by its own test
(`execute_until_lets_a_higher_priority_reentrant_task_displace_a_lower_priority_sibling`)
rather than left as an unstated difference between the two queues' bounds.

### No legacy, lock-tied frame-callback registration API

**Rule:** every callback family this scheduler owns must invoke user code
with **no scheduler-owned storage lock held**. A callback must be free to
call back into the scheduler, including a read-only accessor such as
`current_frame()`.

**Conflict:** issue #1058 found one family that violated this: the retired
`schedule_frame`/`FrameCallback` registration, paired with
`handle_begin_frame`'s dispatch loop
(`if let Some(timing) = self.inner.frame.current_frame.lock().as_ref() {
callback(timing); }`), kept the `current_frame` guard alive for the whole
call — the `if let` scrutinee's temporary lives through the block even
under edition 2024's if-let rescoping. A registered callback that called
`current_frame()` deadlocked on itself; a bounded reproduction timed out at
5 seconds.

**Choice:** delete the API outright rather than fix the lock scope in
place. Flutter's `SchedulerBinding` has no second, "legacy" transient
registration path carrying different timing-argument semantics next to
`scheduleFrameCallback` (`scheduler/binding.dart` @ 3.44.0): its transient
callbacks receive the vsync `Instant`, and there is no `&FrameTiming`-argument
sibling of that same family. The retired API had no distinct
semantics to preserve: its only production caller
(`RenderingFlutterBinding::request_visual_update`) passed an empty
closure, and every other caller in the crate's own doctest/examples used it
identically to `schedule_frame_callback`. Per this crate's
active-development posture (no shims, no deprecated aliases), the fix is
the deletion, not a lock-scope patch that leaves a second,
narrower-purpose registration API standing.

**Every remaining callback family already followed, and continues to
follow, the no-lock-held rule.** This is proven by a shared test oracle
(`scheduler/lock_discipline_tests.rs`'s `assert_no_scheduler_lock_held`,
`#[cfg(test)]`, exhaustively destructuring `FrameState`/`CallbackState`/
`BindingState` plus a `TaskQueue`/`AsyncDriver` probe so a new lock cannot
be added to any of them without this oracle noticing) that `try_lock()`s
every mutex a callback could legally observe and asserts each is free,
invoked from inside a callback registered in every family: transient,
persistent, shared post-frame, owner-local post-frame, idle, microtask,
lifecycle listener, timings callback, and the `on_frame_scheduled`
platform-wake hook. Three sibling sites shared a
related but distinct hazard: `Vec::retain` drops the *removed* element
while the collection's lock is still held, so a cancelled/removed
callback's `Drop` could deadlock the same way if it re-entered the
scheduler. All three were fixed the same release
(`cancel_frame_callback`, `remove_lifecycle_state_listener`, and, for
consistency, `remove_timings_callback`) by locating the match under the
lock without a full-`Vec` copy (`position`+`remove` for the two
`CallbackId`-addressed sites, `extract_if` for the `Arc::ptr_eq`-addressed
one) and dropping it only after the guard falls. Two of the three carry a
bounded reproduction
(`cancel_frame_callback_drops_the_cancelled_callback_outside_the_lock`,
`remove_lifecycle_state_listener_drops_the_removed_listener_outside_the_lock`);
`remove_timings_callback`'s hazard is not reachable through its own public
signature (see that test's own doc comment for why), so its test is a
plain removal-correctness regression instead of a deadlock reproduction.

**Alternatives considered:**

- Snapshot `FrameTiming` under the lock and release it before invoking the
  legacy callback, keeping the API. Rejected: it does not remove the
  question of why two registration paths exist with overlapping purpose,
  and the active-dev policy in `docs/PORT.md`/`AGENTS.md` prefers reshaping
  a wrong-shaped API over patching around it when nothing depends on its
  distinct behavior.
- Make `current_frame`'s mutex reentrant (e.g. `parking_lot::ReentrantMutex`).
  Rejected per the issue's own "possible direction" note: it would mask the
  same class of bug at every other call site instead of enforcing the
  no-lock-held discipline structurally.

**Trade-off accepted:** none observed. The deleted API had exactly one
production call site, itself replaced by the gated `ensure_visual_update`
call described below, and no external consumer of this crate (pre-1.0,
unpublished) depends on it.

### `request_visual_update` routes through the `frames_enabled` gate, not the raw one

**Rule:** requesting a frame in response to pipeline work must honor
`frames_enabled`. This adopts *part* of Flutter's `ensureVisualUpdate`
(`scheduler/binding.dart` @ 3.44.0): it calls the equally
`framesEnabled`-gated `scheduleFrame()`. It does **not** adopt
`ensureVisualUpdate`'s other early-return: Flutter also no-ops while the
scheduler is already mid-frame, inside `SchedulerPhase.transientCallbacks`,
`.midFrameMicrotasks`, or `.persistentCallbacks`. `UpdateScheduler::ensure_visual_update`
checks `frames_enabled` only, with no phase check at all — this is a named
gap, not a hidden one (see **Recorded gap** below); the phase list is
`scheduler/binding.dart::ensureVisualUpdate` at the pinned tag 3.44.0.

**Conflict:** `RenderingFlutterBinding::request_visual_update`
(`flui-app`'s `bindings/renderer_binding.rs`) called the retired
`schedule_frame`, which pushed onto the (now-deleted) legacy queue and then
called the **ungated** `request_frame()` directly. A binding whose
scheduler had frames disabled (backgrounded app, `Hidden`/`Paused`/
`Detached` lifecycle state) would still schedule a frame on every pipeline
request.

**Choice:** `request_visual_update` now calls
`UpdateScheduler::ensure_visual_update()`, which already existed as the
`frames_enabled` gate (`ensure_visual_update` calls
`schedule_frame_if_enabled`, which calls `request_frame` only when
`frames_enabled` is true) and needed no new code, only a caller.
`crates/flui-app/src/bindings/renderer_binding.rs`'s test module pins both
edges:
`request_visual_update_does_not_schedule_a_frame_while_frames_are_disabled`
and `request_visual_update_schedules_a_frame_while_frames_are_enabled`.

**Alternatives considered:**

- Gate inside `request_visual_update` itself (`if scheduler.frames_enabled()
  { scheduler.request_frame(); }`), duplicating `schedule_frame_if_enabled`'s
  body at the call site. Rejected: the scheduler already owns this exact
  check; a second copy in a consuming crate is the kind of drift this
  crate's own `should_schedule_frame`/`schedule_frame_if_enabled` pair
  exists to prevent.

**Recorded gap, deliberately not closed here:** Flutter's `ensureVisualUpdate`
skips scheduling a *new* frame while one is already running
(`transientCallbacks`/`midFrameMicrotasks`/`persistentCallbacks`), because a
pipeline request arriving mid-frame is already going to be served by the
frame in progress. `UpdateScheduler` has no such phase-based dedup for
`request_frame`/`ensure_visual_update` today; it only coalesces on the
`frame_scheduled` flag (see `request_frame_impl`). Implementing the phase
check is a pacing/scheduling-topology change, out of scope for this fix
(which closes the deadlock and the `frames_enabled` gap only) and is left
as a follow-up rather than silently assumed done.

**Trade-off accepted:** the `frames_enabled` fix ships now; the mid-frame
phase check above does not, and is named rather than implied.

### The ticker callback slot is a state machine, leased across user code

**Rule:** a ticker dispatch (`Ticker::tick`, `Ticker::tick_and_reschedule_static`)
must invoke the user callback with no lock held, and whatever that callback
does to the SAME ticker — `stop`/`start`/`mute`/`unmute`/`dispose`/`reset`,
called reentrantly — must be resolved against the run's actual outcome, not
against a state re-read after the fact that cannot tell which run it belongs
to.

**Conflict:** issue #1059 found `TickerInner::callback: Option<TickerCallback>`
conflated two different reasons for being `None` — "never installed, or the
run stopped" and "checked out for an in-flight dispatch" — and that the
auto-scheduling dispatch (`tick_and_reschedule_static`) resolved a checkout
by re-reading `state == Active` alone, an ABA problem: that check cannot
distinguish "still my run" from "a different run the callback itself just
started". Two concrete failures followed, both with production call
chains through `AnimationController::restart_ticker` (a status listener
that calls `forward()`/`reverse()` again from inside the run it is reacting
to is the ordinary "chain the next animation" idiom, not an edge case):

- **Restart inside a tick.** A callback that calls `stop()` then
  `start(new)` had `new` overwritten by the dispatch tail's own blind
  `guard.callback = callback` (the OLD, checked-out closure) restore, and
  the tail then registered ANOTHER transient callback on top of the one
  `start()`'s own `schedule_tick_if_active()` had just registered — measured
  before the fix: `(pending_after_restart, old_calls, new_calls) = (2, 3, 0)`
  instead of `(1, 1, 1)`.
- **Mute then unmute inside a tick.** `mute()` promised to retain the
  callback, but during the dispatch the callback was held OUTSIDE the inner
  slot entirely (checked out into a bare local, not a tracked state); the
  old dispatch tail's restore condition (`state == Active`) never fires for
  a ticker that ends the callback Muted, so `unmute()`'s own re-registration
  was the only one — except a reentrant mute-then-unmute in the SAME tick
  raced it against the tail's blind re-registration, orphaning one of the
  two live ids (issue #1059's own reentrancy trace).

**Choice:** replace the `Option` with an explicit three-state
`CallbackSlot` (`Vacant` | `Ready(TickerCallback)` | `CheckedOut`), and check
a `Ready` callback out into an RAII `TickerLease` for the duration of the
dispatch — the same checkout/restore-or-discard shape as `flui-platform`'s
`CallbackLease` (`crates/flui-platform/src/shared/handlers.rs`), adapted
for a run-completion condition instead of a "window torn down" one. The
lease's `Drop` (which runs on a panicking callback's unwind too, closing a
latent bug where a panic left the slot checked out — i.e. empty — forever)
restores the callback to `Ready` only if the slot is STILL exactly
`CheckedOut` (nothing reentrant already replaced it with a fresh
`Ready(new)`) AND the ticker is still running (`TickerState::is_running()`:
`Active` or `Muted`); otherwise it drops the checked-out callback, always
OUTSIDE the inner lock (a callback's own `Drop` — an `Arc`/`Box` capture's
destructor — is user code that may call back into this same, non-reentrant
ticker; see the workspace memory note `a-statement-lock-drops-its-guard-last`
and the sibling fix in `flui-platform`'s `CallbackLease::drop`). This
resolves both failures structurally rather than by re-checking more state
after the fact:

- A reentrant `start()` overwrites `CheckedOut` with `Ready(new)` directly,
  so the lease finds the slot no longer `CheckedOut` when it drops and
  discards the superseded callback instead of restoring it — an old run can
  no longer overwrite a newer one.
- A reentrant `mute()` leaves the slot `CheckedOut` untouched (mute never
  touched the slot, before or after this fix — only `state`), so the lease
  restores the SAME callback because `Muted` is still `is_running()`. The
  callback is never held outside a tracked state at all.

**One scheduling predicate, not one check per site:**
`TickerInner::should_schedule_tick` (`state == Active && matches!(slot,
Ready(_)) && scheduled_callback_id.is_none()`) is Flutter parity —
[`ticker.dart:270`](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/ticker.dart)
`shouldScheduleTick = !muted && isActive && !scheduled` — and is now the
ONLY scheduling check, shared by `start_inner`, `unmute` (via
`schedule_tick_if_active`), and the auto-tick tail. Checking the SLOT, not
just `state`, is what closes the duplicate-registration failure above:
while a callback is checked out, `slot` is `CheckedOut`, not `Ready`, so
`should_schedule_tick` is false for the WHOLE reentrant window a
mute()-then-unmute() runs inside — `unmute()`'s own scheduling attempt
during that window is a correctly-refused no-op, leaving the dispatch
tail's own (post-restore) attempt as the only one that can succeed. Every
site that still registers a callback id after computing this predicate
also refuses to overwrite an existing `Some` id — a residual defense (not
required to make either measured failure disappear, since the predicate
above already prevents both) against a caller registering between the
predicate check and the id being stored, both of which run with no lock
held; a redundant registration is traced and cancelled rather than
orphaned.

**Five sites hardened to drop a displaced callback outside the lock, none
independently reproducible today (`stop`/`dispose`/`reset` already
released the lock before this fix; `start_inner`'s explicit-callback
overwrite and `set_pending_callback` were the two genuine
statement-scoped-guard instances — memory note
`a-statement-lock-drops-its-guard-last`):** `stop`, `dispose`, `reset`
(`CallbackSlot::clear_if_ready` extracts a `Ready` callback and leaves a
`CheckedOut` one for the dispatching lease to resolve, so a reentrant
stop/dispose/reset never fights the lease over the same callback), and
`start_inner`'s explicit-callback branch and `set_pending_callback` (both
now `mem::replace` the slot and bind the displaced value out of the lock's
block before dropping it).

**Why FLUI needs a slot protocol Flutter does not:** Flutter's `Ticker`
holds one `_onTick` for its entire life, assigned once at construction
(`Ticker(this._onTick, ...)`); there is no `start(callback)` that installs
a NEW callback per run, so Dart's own `_tick` has no "which run does this
checked-out closure belong to" question to answer — `_animationId` alone
(this crate's `scheduled_callback_id`) is Flutter's whole story. FLUI's
`Ticker::start` accepts a fresh callback on every run (`TickerProvider`'s
factory shape plus ad hoc `start(closure)` call sites), so the SAME ticker
legitimately dispatches through a sequence of different closures over its
life — the slot state machine is what tracks which one a given dispatch is
allowed to restore.

**Recorded divergences and limitations, not closed by this fix:**

- **`start_inner` while `Muted` bypasses the `Idle`/`Stopped` contract.**
  Flutter's `Ticker.isActive` is `_future != null` and its doc states that
  a muted ticker "can be active" — muting gates `isTicking` and
  `shouldScheduleTick`, never `isActive` — so `start` on a muted Flutter
  ticker hits `'A ticker that is already active cannot be started again'`
  and is REJECTED. `Ticker::start_inner`'s own `debug_assert!`/early-return
  rejects only `TickerState::Active`, so FLUI accepts the call, silently
  overwriting the muted run's callback and future and re-anchoring its
  start time. Named here as a known gap; closing it is a `start_inner`
  contract change outside this fix's scope.
- **A panicking tick callback leaves the ticker unscheduled.** The lease
  restores the callback on unwind, so the slot is `Ready` and the state is
  still `Active` — but the registration id was cleared at dispatch entry
  and the tail that would re-register never runs, so the ticker stays
  active and idle until something external (a `mute()`/`unmute()` cycle, a
  `stop()`+`start()`) re-arms scheduling. Pinned by
  `a_panicking_tick_callback_leaves_the_slot_restored`, which asserts the
  slot's contents, not that ticking resumes.
- **Register-outside-lock / store-id-under-lock is still a genuine
  cross-thread TOCTOU window; the concrete consequence it produced is
  closed (issue #1166).** `schedule_tick_if_active` and the auto-tick
  tail both check eligibility, then upgrade the scheduler and register,
  then re-lock ONLY to decide whether to keep the id — three separate
  lock acquisitions with no lock held across any of them, and that window
  itself remains open. Both tails decide with
  `TickerInner::may_record_registration` (`state == Active &&
  scheduled_callback_id.is_none()`, re-read under the SAME lock as the
  write, deliberately without `should_schedule_tick`'s slot term — see that
  method's own doc for why a checked-out slot must not retract a
  registration here), and self-cancel the id they just minted via
  `UpdateScheduler::cancel_frame_callback` whenever a `stop`/`dispose`/
  `reset`/`mute` raced them in that window and left the ticker anything
  other than `Active` with no registration id on record. A concurrent
  cross-thread `mute()` immediately followed by `unmute()` racing this
  window is traced and cancelled the same way — it is NEITHER a starvation
  hazard (the losing tail's self-cancel blocks nothing) NOR an orphan
  hazard (nothing survives live and uncancelled); whichever tail's
  registration lands in the re-lock first wins outright, and the other
  cancels its own. Two residual gaps this fix does not touch, both
  structural rather than part of #1166's scope: (a) the transient loop's
  `cancelled` check (`handle_begin_frame`) is read outside the `transient`
  lock, so a callback popped for execution in the same instant it is
  cancelled can still run once — inert, since the tick path's own
  top-of-dispatch `state` re-read is what makes it harmless; (b) the
  top-of-tick unconditional `scheduled_callback_id = None` in
  `tick_and_reschedule_static` assumes the firing closure is the one
  currently on record — a stale closure firing concurrently with a fresh
  registration's record on another thread could clobber that fresh id
  instead of its own stale one. Both need two frames racing on two threads
  to manifest.
- **The `AnimationController` ↔ `Ticker` strong-clone reference cycle is
  unchanged and undocumented as a NEW risk by this fix.**
  `AnimationController::restart_ticker` captures `let controller =
  self.clone();` into the ticker's callback closure — `Ticker` holds
  `TickerCallback = Box<dyn FnMut(f64) + Send>`, so the controller's
  `Arc<Mutex<AnimationControllerInner>>` is kept alive by its OWN ticker's
  installed callback for as long as that callback is installed. This is
  safe today only because every mutator that could otherwise deadlock or
  leak reaches the controller through a borrowed `&self` (never taking a
  second strong clone that would need dropping to break the cycle) and
  `dispose()`/`stop()`/`reset()` all clear the ticker's callback (directly,
  or via `Ticker::dispose`/`Ticker::stop`), which drops the closure and
  with it the controller's self-reference. The invariant — every
  `AnimationController` method that runs while its own ticker's callback
  could still be installed must reach `self` through a borrow, never
  through a second owned strong clone the callback itself would need to
  outlive — is recorded here rather than changed.

**Tests:** `flui-scheduler`'s `ticker::tests` module —
`restart_inside_auto_tick_preserves_new_callback_and_one_pending_tick` and
`mute_then_unmute_inside_tick_delivers_next_frame_once` pin the two measured
failures directly; `restart_between_ticks_preserves_new_callback` is a
non-reentrant control; `dispose_inside_tick_drops_the_callback_and_does_not_reschedule`
and `reset_inside_tick_drops_the_callback_and_does_not_reschedule` are
controls too — both already passed before this fix (`dispose` cleared the
registration id and the tail returned at its `disposed` check; `reset` left
the state `Idle`, so the tail's restore and reschedule were already
skipped) and guard against a regression rather than pinning one of the
defects; `a_panicking_tick_callback_leaves_the_slot_restored` pins the
panic-unwind fix; `stale_callback_is_dropped_outside_the_lock` proves the
outside-the-lock drop with a non-blocking `try_lock` probe rather than a
test whose failure mode would be a hang. The manual `Ticker::tick(&self,
...)` path cannot support the SAME reentrant-restart probe: restarting
needs `&mut Ticker` (`stop`/`start`), which — since `tick` takes only
`&self` — is only reachable by wrapping the ticker in an outer lock the
CALLER holds for `tick`'s entire duration, including the callback; a
reentrant call back through that same non-reentrant lock self-deadlocks
before it ever reaches `stop()`. `stop_between_two_manual_ticks_does_not_reinvoke_callback`
pins the manual path's (non-reentrant) restore contract instead.
`flui-animation`'s `controller::tests::status_listener_chaining_forward_ticks_once_per_frame_and_stop_fully_stops_it`
reproduces the auto-scheduling "restart inside tick" failure through the
real production call chain (a status listener chaining the next run) rather
than a ticker-level probe, and pins that `stop()` afterward cancels the
run fully rather than one half of a duplicated pair.

**Alternatives considered:**

- Keep `Option<TickerCallback>` and add a generation/epoch counter
  (the issue's own "possible direction") checked alongside `state`.
  Rejected: an epoch still answers "is this the same run", but does nothing
  about the SECOND failure (mute/unmute retention while checked out) or the
  panic-unwind leak, both of which are a missing STATE — "checked out" is
  not representable in `Option` at all — rather than a missing identity
  check. The slot state machine subsumes what an epoch would have bought
  and closes the other two failures the same shape closes.
- Hold the ticker's inner lock across the callback invocation, so no
  reentrant call could observe an inconsistent slot. Rejected per the
  issue's own explicit instruction and this crate's existing
  no-lock-held-during-a-callback discipline (see this file's own "No
  legacy, lock-tied frame-callback registration API" entry above): it would
  prohibit the legitimate reentrancy (`AnimationController::restart_ticker`
  IS a real, common call path) and reintroduce exactly the self-deadlock
  class issue #1058 removed.

**Trade-off accepted:** the two named limitations above (the `Muted`
start-contract gap and the cross-thread register/store TOCTOU) ship
unfixed, named rather than silently assumed closed; both predate this fix
and are not measured to have widened under it.

### `end_of_frame` registers before it demands, and the live registry is the memo

**Rule:** `UpdateScheduler::end_of_frame` pushes its waiter onto the completion
registry FIRST and issues the frame demand second, and it issues one only when
the registry held no live waiter before that push. Flutter's `endOfFrame`
(`scheduler/binding.dart` @ 3.44.0) orders it the other way: it calls
`scheduleFrame()`, then hands back the shared `_nextFrameCompleter`'s future.

**Conflict:** demand-then-register is not equivalent once a frame can run
concurrently. A frame beginning on another thread can both start and drain the
registry in the window between the demand and the push, so the waiter misses
the very frame it paid for and silently buys a redundant next one. Registering
first makes the registration the linearization point. An entry is then either
inside the batch a drain took under the registry guard, and is served by that
frame, or it was pushed after that guard was released, in which case its own
predicate reads the post-drain registry and demands. Flutter never faces this
ordering question: `SchedulerBinding` is confined to one isolate, there is no
per-waiter registry to race against at all, and every caller within one frame
coalesces onto a single completer the binding owns.

FLUI keeps the per-waiter registry because its futures are independently
cancellable values rather than listeners on one shared `Future`, which is also
what makes the registry the natural place to keep the demand memo.

**Choice:** the predicate is "no LIVE entry", evaluated on the vec already held
under the registry guard, and the demand call is `schedule_frame_if_enabled()`
rather than the ungated `request_frame()` (Dart's `scheduleFrame()` carries the
same enablement check internally). The guard is released before the demand,
because the demand reaches the `on_frame_scheduled` hook and
`frame_scheduled_hook_runs_with_no_scheduler_lock_held` asserts every scheduler
mutex, `completion_waiters` included, is free inside it.

That predicate has two halves, and only the first belongs to the registry:

- **Issuance.** After a drain the vec is empty, so the first push demands.
  Every later push either observes a live entry whose demand postdates that
  drain, by induction, or demands itself. The predicate is a pure function of
  the vec's contents at push time, so there is no bit written at one time and
  read at another.
- **Survival.** No registry predicate can decide whether an issued demand
  still stands. A demand is revoked without any drain when `frame_scheduled`
  is cleared with a live waiter still registered, and it has a second
  clearer besides `handle_begin_frame`: the public `finish_async_pump`. If
  nothing else acted, a later push would then see that live entry, stay
  silent, and both wait forever. Two independent mechanisms recover it,
  covering the two ways the revoke is reached:

  - **`finish_async_pump` re-issues the demand itself (issue #1162)**, at
    the exact point that would otherwise drop it: once the latch is clear,
    it checks `frames_enabled` and the registry's own `has_live_waiter()`
    and calls `request_frame()` if both hold. This is the leg that matters
    when frames are ALREADY enabled at the moment the pump runs — no
    disable→enable edge ever happens for such a waiter to ride, so nothing
    but the pump's own re-check can recover it. Not reachable off a
    runner's own decide-then-pump order, though: every `flui-app` runner
    calls `finish_async_pump` only from `wake_action`'s `PumpAsync` arm,
    chosen iff `!frames_enabled` at that same read, immediately followed by
    this call on the same thread with nothing in between — so `frames_enabled`
    still reads false when the runner's own pump reaches the re-check. What
    DOES reach this leg with `frames_enabled` true: `finish_async_pump`
    called directly, out of `wake_action`'s order (an embedder or a test
    bypassing it), or a cross-thread enable landing in the store-buffering
    window this doc's own **Ordering:** note names below. See the
    `end_of_frame` resolves an outcome, and a dropped scheduler resolves
    `Err(SchedulerClosed)` entry's own **Ordering:** note for why this needs
    `SeqCst`, not `Acquire`/`Release`.
  - **A frames-enabled edge re-issues a demand the pump left for dead while
    frames were disabled** — the case the pump's own re-check cannot reach,
    since it reads `frames_enabled` false there and stays silent on purpose
    (Dart's `scheduleFrame()` carries the identical enablement gate).
    **The production carrier is `handle_app_lifecycle_state_change`**, whose
    `if !frames_were_enabled && should_render { self.request_frame(); }` leg
    predates issue #1162 and is pinned by
    `lifecycle_reenable_edge_schedules_exactly_one_frame`. That is the edge a
    real app crosses, and the sequence is reachable rather than theoretical:
    frames enabled, a demand issued, lifecycle goes `Hidden`, a `PumpAsync`
    tick revokes the latch through `finish_async_pump` with no drain, later
    registrations stay silent behind the still-live waiter, and the resume
    edge is what recovers them. `set_frames_enabled(true)` gained the same
    re-request so the public setter mirrors the lifecycle path rather than
    being a second way to reach the stranded state. It has **zero production
    callers** today (the only non-test call in the workspace passes `false`,
    and it is itself inside a `#[cfg(test)]` module), so do not read its
    caller count as a measure of whether this argument holds.

  Before issue #1162, only the second leg existed, which is why a live
  waiter stranded while frames stayed enabled the whole time (no disable, no
  re-enable, just a `PumpAsync` cycle) had no recovery path at all — named
  as a reachable, not theoretical, gap on `finish_async_pump`'s own doc and
  closed by the first leg above.

**Alternatives considered:**

- `waiters.is_empty()` as the predicate, which is also sound against today's
  code: a tombstone can only exist since the last drain, and the push that
  created it already demanded. Rejected because that soundness spans three
  pieces of state (registry population, the `frame_scheduled` latch, and
  `frames_enabled` plus the lifecycle edge), none of them asserted anywhere,
  and one leg of it is a public method any embedder may call. The live scan
  reads only the vec it already holds.

  `is_empty()` is O(1) and the live scan is not, which is a real cost and was
  a real defect: the first version walked the whole vec on every registration,
  so a run of tombstones in front of one live entry was re-walked per push
  (measured: 14,641 probes for 121 registrations, under the registry mutex).
  The registry now carries a cursor that retires the tombstones it walks past,
  which makes the scan amortized O(1) and brought the same 121 registrations to
  241 probes. `the_demand_scan_does_not_rewalk_a_tombstone_prefix` pins it, at
  both ends: an upper bound alone is satisfied by a predicate that does no
  scanning at all.
- Gating the demand on `phase() == Idle`, the closest reading of Flutter's own
  `endOfFrame`. Rejected: it goes silent in the post-drain window, where
  `notify_frame_completion` has already emptied the registry but the phase is
  still `PostFrameCallbacks`, so a waiter registered from a completion waker
  hangs. `crates/flui-scheduler/tests/end_of_frame_lifecycle.rs`'s
  `a_registration_from_inside_a_completion_waker_demands_the_next_frame` is the
  oracle for exactly that window.
- A per-frame "a frame is already open" flag, cleared when the frame ends.
  Rejected for the same defect one level down: every candidate clear point sits
  later than the drain it is meant to pair with, so the flag is still set
  during the post-drain window. Deleting the flag removes the clear point
  rather than moving it.
- Adopting `event-listener`, already a dependency of this crate and already
  used by `TickerFuture` for the sibling problem. Rejected on a structural
  reason this crate has paid for once: `Event::notify` calls `task.wake()`
  inside the closure holding its own internal list mutex, which is the shape
  issue #1057 removed from `notify_frame_completion`, and two currently-green
  tests pin the contract it breaks
  (`notify_frame_completion_tolerates_an_inline_polling_waker` and
  `notify_frame_completion_still_wakes_a_later_waiter_when_an_earlier_waker_panics`).

**Trade-off accepted:** a registration landing mid-frame while no other waiter
is live demands a frame the in-flight drain would have served anyway. That
costs one surplus frame per registration burst, and no more within a frame,
because `request_frame` fires the wake hook only on the `frame_scheduled`
false to true edge. Jetpack Compose, `Choreographer`, `requestAnimationFrame`,
and Unity's `Awaitable.NextFrameAsync` all make registration itself the demand
and all pay the same price; Compose states the rule as the zero to one
transition of the awaiter set.

**What that coalescing does NOT bound, stated because the obvious reading
overstates it:** it bounds requests *within* one frame, not across frames. A
caller that registers on every frame sustains the frame loop indefinitely, and
nothing here damps it: `handle_begin_frame` clears the latch at the top of each
frame, the registration re-demands, and that frame's own drain removes the
entry so the next registration re-demands too. A persistent callback that drops
an `end_of_frame()` each frame therefore keeps the scheduler awake forever. That
is the same standing demand an animation ticker creates, and it is what asking
for a frame every frame means rather than a runaway; the point is that "one
surplus frame" describes a burst, not a repeating caller.

**Closed by issue #1162, previously recorded here as an open gap:** a
`FrameCompletionFuture` whose scheduler was dropped while it was pending used
to never resolve, because only a frame resolved it and only the scheduler ran
frames, and `Output` being a bare `FrameTiming` left no sentinel to resolve
with. See the `end_of_frame` resolves an outcome, and a dropped scheduler
resolves `Err(SchedulerClosed)` entry below for how. **Still open:** a
panicking `on_frame_scheduled` hook loses its demand permanently, since
`request_frame` sets the latch before firing the hook, and FLUI defines no
recovery transition for that (Compose does: a throwing `onNewAwaiters`
permanently fails the clock and resumes every current and future awaiter with
the error). That note is on `end_of_frame`, which is the call that can reach
the hook.

### `end_of_frame` resolves an outcome, and a dropped scheduler resolves `Err(SchedulerClosed)`

**Rule:** [`FrameCompletionFuture::Output`](src/scheduler.rs) is
`Result<FrameOutcome, SchedulerClosed>`, never a bare `FrameTiming`. A frame
that closed through `end_frame_impl` resolves `Ok(FrameOutcome::Completed { timing })`;
one that closed through `abort_frame` resolves `Ok(FrameOutcome::Aborted { timing })`;
and every waiter still registered when the scheduler's last strong handle
drops resolves `Err(SchedulerClosed)`. Polling again after `Ready` repeats the
same value rather than hanging.

**Conflict:** the two gaps the entries above named as open — "a caller cannot
tell an aborted frame apart from a successful one" and "a
`FrameCompletionFuture` whose scheduler is dropped never resolves" — both
trace to the same root cause: `Output` carried no room for anything but a
successful frame's timing.

**Choice, and why `Result<FrameOutcome, SchedulerClosed>` rather than the
issue's own flat three-variant enum**
(`Completed`/`Aborted`/`SchedulerClosed` in one type): `SchedulerClosed` is
categorically different from the other two — it is "this future will never
be resolved by a frame at all," not "a frame resolved it, badly." Splitting
it into the `Result` error channel lets ordinary `?`/`.await?` composition
work the way it would for any other fallible async operation, and keeps
`FrameOutcome` free to grow more *frame* outcomes later (it is
`#[non_exhaustive]` for exactly that) without also having to reason about
where a teardown sentinel sits among them. `Completed`/`Aborted` are `Debug,
Clone, Copy` only — `FrameTiming` itself has no `PartialEq`/`Eq`. Both are
struct-shaped (`{ timing: FrameTiming }`) and each carries its own
variant-level `#[non_exhaustive]`, so outcomes are constructed only by the
scheduler and matched outside it with `{ .. }`: `abort_frame` records nothing
else about how a frame ended, so `Aborted`'s field is the only surface an
aborted frame's data reaches, and a reason or phase is a plausible additive
field later. A plain tuple `Completed(FrameTiming)` would have let external
code fabricate an outcome, and `#[non_exhaustive]` on a *tuple* variant is no
middle ground — it makes the variant fully opaque cross-crate, not even
matchable as `Completed(..)`; the struct shape is the one form that is both
sealed for construction and open for matching. `SchedulerClosed` is a plain
unit struct, not `#[non_exhaustive]`, mirroring this crate's closest sibling,
`ticker::TickerCanceled`.

**Fused for free, not by a new mechanism:** `poll` used to `take()` the
resolved timing, so a second poll after `Ready` found `None` again and hung
forever — a real "polling it again after it resolved is a silent hang" trap,
named on the future's own doc before this issue. `Result<FrameOutcome,
SchedulerClosed>` is `Copy` (both arms are), so `poll` now peeks the stored
value by copy instead, and a second poll simply repeats it. This mirrors
`TickerFutureOrCancel`'s own `poll_resolution` shape in this crate (`ticker.rs`)
rather than introducing a second fusing mechanism.

**Teardown mechanism:** `impl Drop for SchedulerInner` drains the completion
registry with `Mutex::get_mut` — no lock, sound because `Drop::drop` runs
only once the `Arc`'s strong count reaches zero, and `Arc`'s own
release/acquire ordering means the destructor observes every prior mutation
through any dropped clone. Every drained entry's `completed` is unconditionally
written `Some(Err(SchedulerClosed))` with no `is_none()` guard: `drain()`
performs `mem::take`, so an entry reaching this loop was, by construction,
never reached by `notify_frame_completion` first (a delivered completion
already left the registry through that same `drain`) — the guard would be
dead code testing a fact the type already proves, not a real defense. Every
waker's `wake()`, and a panicking wake payload's own possibly-panicking
`Drop`, is caught and traced via `tracing::error!`; teardown never
`resume_unwind`s — a panic from a destructor while already unwinding aborts
the process with no diagnostic, and `abort_frame`'s own doc already states
this rule for this crate. The same `discard_panic_payload` helper introduced
for this contains a second (or later) waker's panic during an ordinary,
non-teardown `notify_frame_completion` drain, and the symmetric case one
level up in `end_frame_impl`'s own `callback_result`/`notify_result` merge —
previously an uncontained `Option::or`-selected `drop`.

**The teardown guarantee is partial, and this does not widen it:** any live
strong `UpdateScheduler` handle defers `Drop for SchedulerInner`, the same as
any other `Arc`. A task on an external executor that owns a clone does not
hang — dropping the executor drops the task, the clone, then the scheduler —
but the scheduler's own async driver holding a task future that captured a
clone is a true self-cycle no `Drop` impl here can break.

**Ordering: the same issue also closed a `finish_async_pump` wake-loss hazard,
and it needed `SeqCst`, not `Acquire`/`Release`.** A live `end_of_frame`
waiter whose demand survives a disable→enable edge with `frame_scheduled`
already latched `true` fires no NEW wake on that edge (the false→true
transition `request_frame_impl` needs never happens, since the latch was
never cleared by disabling frames), so a LATER `finish_async_pump` cycle that
unconditionally clears the same latch would silently drop the demand with no
mechanism left to re-issue it. The fix is `finish_async_pump`
re-checking `frames_enabled && completion_waiters.has_live_waiter()` after
clearing the latch and re-issuing the demand itself — see the "Survival"
paragraph above for the full mechanism and its sibling (the frames-enabled
edge covers the case this can't reach). That re-check races a concurrent
frames-enabled edge's own writes with **no reads-from edge between the two
threads** — a store-buffering (Dekker/SB) litmus pair: pump thread's
`frame_scheduled.swap(false, _)` then `frames_enabled.load(_)`, against edge
thread's `frames_enabled.swap(true, _)` then (via `request_frame_impl`)
`frame_scheduled.swap(true, _)`. Under `Acquire`/`Release` alone, "the pump's
load still sees the OLD `frames_enabled` AND the edge's swap still sees the
OLD `frame_scheduled`" is a legal outcome and observable on real hardware
(x86's store buffering: a plain store can sit buffered while a later plain
load on the SAME thread already executes) — not merely a theoretical
interleaving. `SeqCst` on all four accesses this race instance touches — the
pump's `frame_scheduled` swap AND its `frames_enabled` load; the edge's
`frames_enabled` swap AND (via `request_frame_impl`) its `frame_scheduled`
swap — puts them on one global total order, which rules that outcome out.
Marked at five code sites in total, since `frames_enabled`'s swap has two
production carriers (`set_frames_enabled` and
`handle_app_lifecycle_state_change`) even though only one of them fires per
race instance. No single-threaded test can redden a regression from
`SeqCst` back to `Acquire`/`Release` here; only a `loom` model could prove
it, and none exists in this crate yet.

**Divergence from Flutter, recorded rather than silently improved:**
`SchedulerBinding.endOfFrame` (`scheduler/binding.dart` @ 3.44.0) resolves a
bare `Future<void>` — Dart has no outcome signal here at all, successful or
otherwise, and no teardown sentinel either (`SchedulerBinding` is a
process-lifetime singleton in Dart; there is no analogue of dropping it).
FLUI's per-realm `UpdateScheduler` can be dropped mid-flight, so it needs an
answer Dart's own binding never had to give.

**Alternatives considered:** a flat `enum FrameCompletionOutcome { Completed,
Aborted, SchedulerClosed }` (the issue's own sketch) — rejected above for
conflating a frame outcome with "no frame will ever resolve this," which the
`Result` split keeps separate and composable. A sentinel `FrameTiming` value
(e.g. all-zero) for the closed case — rejected: it is indistinguishable from
a real frame's timing by construction, exactly the "cannot tell them apart"
defect this issue exists to close, just moved to a new field instead of
solved.

### A ticker future's poll registers before the read that decides to park

**Rule:** the durable resolution state is the source of truth and the
notification is only a hint to re-read it. Every polling path through
`TickerFuture` / `TickerFutureOrCancel` must have a listener linked *before* it
takes the state read that decides to return `Poll::Pending`.

**Conflict:** Flutter's `TickerFuture` is built on a `Completer`, so this class
of bug does not exist there — there is no listener to register and therefore no
window between observing the state and subscribing to a change. FLUI models a
once-only, monotone transition (a *level* fact) with `event_listener::Event` (an
*edge* primitive) whose own documentation says a notification sent with no
listener registered "simply gets lost". Both `poll` impls read the state, dropped
the guard, and only then called `listen()`; a `set_complete`/`set_canceled`
landing in that window notified zero listeners, and because the transition is
guarded by `if *state == Pending` nothing ever re-announced it. The future then
parked on an event that could never fire again — a permanent hang, not a delay.
The correct order was already present one screen away, in the blocking
`when_complete_or_cancel`, with a comment naming the hazard.

**Choice:** one private `poll_resolution` helper, shared by both `Future` impls,
running `read → register → read → park`. The second read is the fix; the
listener latch (a notification landing on a registered-but-unpolled entry marks
it `Notified`, and the next `register` reports that) is a redundant second net
that closes only the `listen()`→poll half and cannot touch the window that is
the defect. `resolve` publishes the durable state before it notifies, which is
what makes the re-read sufficient, and
`the_resolution_is_published_before_the_notification` pins that ordering
independently. The helper is also what makes the two impls provably one state
machine rather than two copies that drift; both are exercised by their own
ordering test for that reason.

**Precondition this makes reachable, stated rather than fixed:** `Event::notify`
calls `task.wake()` while holding `event-listener`'s own internal list mutex,
which both registering and dropping a listener re-take. A waker that re-polls or
drops the future from inside `wake()` deadlocks inside the dependency. That was
always true, but a bug that never woke anyone kept it unreachable; it is now a
documented precondition on both `Future` impls. Standard parker-based executors
satisfy it. A panicking waker is likewise left uncontained and propagates out of
`stop`/`dispose`/`reset`: `Inner::notify` increments its notified counter *after*
`task.wake()`, so an unwind through the waker skips the increment and the next
listener removal underflows inside the dependency's own `Drop` — measured, and a
`catch_unwind`-and-retry around `notify` plants that abort rather than avoiding
it. The residual is that a task whose only wake source is this ticker hangs if
its waker panics; a task with any other wake source self-heals, because the poll
reads the durable state first.

### A second resolution is ignored where Flutter asserts

**Rule:** `TickerFuture::resolve` is once-only; a second `set_complete` or
`set_canceled` is a silent no-op and the first outcome stands.

**Conflict:** Flutter's `_complete()`/`_cancel()` open with
`assert(_completed == null)` — a debug assertion that the transition happens
exactly once, stripped in release.

**Choice:** keep the no-op, do not assert. FLUI reaches these drivers from
`impl Drop for Ticker` as well as from explicit teardown, and a hard failure on
an idempotent teardown call is worse than the no-op; a panic raised from a `Drop`
running during an unwind aborts the process. The invariant is load-bearing rather
than cosmetic — it is what lets a re-read after registering stand in for a
notification that was never delivered — so it is pinned by
`a_second_resolution_is_ignored_and_the_first_outcome_stands` in place of the
assertion the reference relies on.

### The base ticker future parks on cancellation with no waker registered

**Rule:** `TickerFuture` (the base future) resolves only on `Complete`. On
`Canceled` it returns `Poll::Pending` holding no listener and no waker;
`or_cancel()` is the route that observes cancellation.

**Conflict:** returning `Pending` without registering a waker is a literal
violation of the `Future` contract, which requires a poll that returns `Pending`
to have arranged for the task to be woken. Flutter has no such contract to
violate: `_cancel` completes only the secondary completer and the primary future
simply never completes.

**Choice:** match Flutter's semantics and take the contract violation
deliberately, because the alternative is worse in a way the contract exists to
prevent. The previous code registered a *fresh* listener on every poll of a
canceled future — a subscription to an event that provably can never fire again,
pinning the executor's waker for the lifetime of the future. Holding nothing is
the honest encoding of "nothing can wake this", and it is what
`a_canceled_base_future_holds_no_waker_and_no_listener` asserts. The obligation
this carries is documentation: the `Future` impl says so at the point a caller
reads it.

### A start that would orphan a live ticker future is refused, in every build

**Rule:** `Ticker::start`/`start_default` refuse when a previous run's
`TickerFuture` is still installed, keyed on `active_future.is_some()` — the
durable fact — and never on a `TickerState` variant list. A refused start logs at
`error!`, drops the caller's callback, and returns the live future, so an
existing awaiter stays valid.

**Conflict:** Flutter guards this with `assert(!isActive)`, where
`isActive => _future != null`; stripped in release, a second `start()` overwrites
`_future` and the displaced future never completes. FLUI already did better than
that — it refused in release and returned the live future — but keyed the refusal
on `state == Active`, which is *narrower* than the fact it was protecting.
`Ticker::mute()` pauses a run without touching `active_future`, so `mute()` then
`start()` walked straight past the guard, overwrote the field, and orphaned a
pending future permanently. The enum collapse is what opened the hole, which is
why the predicate is now the future itself.

**Choice:** refuse on `active_future.is_some()`; keep the existing
`debug_assert!` on `state == Active` unchanged. These are two different
questions, not two guesses at one: the assertion answers "is this Flutter's
*started twice* programming error?" and the refusal answers "is there a live
future I must not orphan?". Widening the assertion to the same predicate was
considered and rejected — it would make the ordinary `mute(); start()` sequence a
debug panic, which would leave the refusal itself release-only and therefore
untested by a suite that runs in debug. The divergence from Flutter is that a
start on a *muted* ticker is a supported, refused operation here rather than a
thrown error.

**Lock discipline, because the refusal runs user code:** the decision is taken
under `Mutex<TickerInner>` and acted on after it. Both the `tracing` event (whose
subscriber is arbitrary user code) and the rejected callback's `Drop` (likewise)
would otherwise be able to re-enter a non-reentrant mutex. Rust's drop order
already saves the callback — a function's body-scope locals, the guard included,
drop before its parameters — so the subscriber is the half that genuinely needed
hoisting, and it is the half
`a_refused_start_logs_and_drops_its_callback_with_the_inner_lock_free` actually
pins. That test's callback oracle stays green with the explicit `drop(callback)`
deleted, for the same drop-order reason, so the `drop` documents intent and
nothing more; it must not be cited as test-defended.

**The same rule, applied to the two sites in this file that were breaking it.**
`start_inner`'s vacant-slot arm emitted `tracing::warn!` and returned from inside
the guard scope — thirty lines below the comment declaring the rule — and
`TickerLease::drop` emitted a `tracing::trace!` whose text says "outside the
inner lock" from inside it, a claim that was true of the callback drop it
describes and false of the event carrying it. Both now decide under the lock and
report after it.

**Cross-crate consequence, closed in the same change:**
`AnimationController::restart_ticker` guarded its pre-start `stop()` on
`TickerState::can_tick()` (Active only), so from `Muted` it skipped the stop —
and with the refusal in place it would have received the *old* future back and
silently failed to restart the animation. The guard is now `is_running()`.

It stays a guard rather than becoming an unconditional `stop()`, because **`stop`
and `reset` are not no-ops on a ticker that is not running**: both call
`CallbackSlot::clear_if_ready`, so a callback pre-loaded by
`TickerProvider::create_ticker` and never started is discarded by a bare
`stop()`, and the next `start_default()` then warns and hands back an
already-complete future. `restart_ticker` is unaffected either way — it always
passes an explicit callback — but the claim is on `Ticker::start`'s public doc,
where a reader who acts on it loses a callback, so it is stated there too.

### `when_complete_or_cancel` blocks, and refuses rather than lying on wasm

**Rule:** `TickerFuture::when_complete_or_cancel` invokes its callback
immediately when the future is already resolved. On a still-pending future it
parks the calling thread; on a wasm target, where there is no thread to park, it
`debug_assert!`s, logs at `error!`, and returns **without** invoking the
callback.

**Conflict:** Flutter's `whenCompleteOrCancel` is
`orCancel.then(thunk, onError: thunk)` — it registers a continuation and returns
immediately, never blocking. FLUI's blocks, and its wasm path used to drop the
registration and invoke the callback right away, reporting a completion that had
not happened. That third behaviour was recorded in
`docs/audits/2026-07-25-upgrade-pack-audit.md` and never fixed.

**Choice:** keep the blocking shape (the `async` route, `or_cancel().await`, is
the non-blocking equivalent and is the supported route on wasm) and remove the
lie rather than the method. Reporting nothing is strictly better than reporting a
completion that has not happened, and the debug-assert follows
`Ticker::assert_not_disposed`'s in-crate precedent for "a call that cannot honour
its contract". The `cfg` predicate is `target_family = "wasm"`, mirroring the one
`event_listener::Listener::wait` is gated on upstream, so the two cannot skew.
**Nothing here executes on wasm32:** `flui-scheduler` declares no wasm32
`wasm-bindgen-test` dev-dependency, so `just wasm-test` does not discover it and
this branch is proven by `just wasm-check` compiling and by nothing else.

**Where the uniform panicking-`Drop` question is decided:** a `Drop` impl that
runs user code during teardown needs
`if std::thread::panicking() { report } else { resume_unwind }` as its
discriminator, because a caller can invoke the teardown from their own `Drop`
during an unwind — something a per-call-site split cannot see.
`impl Drop for Ticker` → `dispose` → `notify` → a panicking waker is exactly that
shape. Issue #1162 carries the decision for `SchedulerInner`; whichever way it
lands applies here for the same reason.
