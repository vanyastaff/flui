# flui-scheduler Architecture

Per-crate ledger for architecture decisions that span more than one module in
this crate. Partial: this file exists for the `## Mapping
decisions` entries below; a full crate architecture writeup is deferred.

---

## Mapping decisions

### Exhausted task identities permanently refuse admission

Task identities (the process-wide `Task` allocator and each async driver's)
never wrap: the integer maximum is a permanent exhaustion state, and once it is
reached every admission panics, including after the panic is caught, tasks are
cancelled or another driver clone is used. An accepted task therefore keeps its
cancellation authority and its priority/FIFO position. Admission checks capacity
before polling a future or publishing a task, and the rejected callback or
future is retained rather than dropped (ADR-0127), so its destructor cannot
replace the capacity failure.

### Ticker cancellation precedes terminal callback retirement

Disposing, stopping or resetting a ticker withdraws its callback and cancels
its pending scheduler registration before dropping the callback, so a failing
callback destructor never leaves the registration live. While the thread is already
panicking the callback is retained instead of dropped
([ADR-0127](../../docs/adr/ADR-0127-exceptional-path-retention.md)).

### Post-frame panic preserves uninvoked work in its original queue

A post-frame callback panic stops the drain and propagates after frame completion
bookkeeping closes. The panicking entry is consumed, not retried; the uninvoked
tail returns to its original shared or owner-local queue with its original IDs.
The private `scheduler::post_frame_dispatch` module owns this snapshot and tail
recovery; the frame-close path retains phase and completion bookkeeping.
The next completed frame sorts those IDs with newer registrations, so surviving
work precedes work registered reentrantly by the failed callback. Cancellation
records remain intact on the failed drain, including records belonging to other
callback queues. No user callback or captured destructor runs under a queue guard.

After the shared and owner-local queues have been snapshotted, but before their
entries are sorted or invoked, the dispatcher emits one debug event with
`shared_callbacks`, `local_callbacks`, and `total_callbacks`. These are queue
work-item counts for diagnosing post-frame amplification; they are only a lower
bound on retained memory because an opaque callback's captured bytes cannot be
measured. Nested registrations therefore appear in the next frame's event, and
a panic-restored tail appears again in the retry frame's event. Observability
does not impose a capacity, coalesce commands, or alter delivery order.

This extends the existing pre-pipeline recovery invariant to the post-frame
snapshot. It does not catch-and-continue individual callbacks, request another
frame, or promise that a platform host survives an application panic. A direct
or headless caller that catches the propagated panic decides whether to resume.
The regression test
`post_frame_panic_preserves_uninvoked_mixed_tail_before_reentrant_work` pins
tail survival, mixed-queue FIFO, and consumption of the failed entry.

### One recovery boundary closes phase/completion state before any pre-pipeline panic propagates

**Rule:** a caller that catches a panic out of `drive_frame`/
`execute_frame` must find the scheduler's own
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
unresolved forever. `execute_frame` had the
identical gap: they called `handle_begin_frame`/`handle_draw_frame`/
`end_frame` directly, sharing no recovery boundary with `drive_frame` at
all.

**Choice:** `drive_frame_impl` now wraps `handle_begin_frame`,
`handle_draw_frame`, AND `pipeline` in ONE `catch_unwind`; a panic from any
of the three still runs `abort_frame` before the payload resumes.
`execute_frame` routes through the SAME
`drive_frame_impl` with a no-op pipeline (returning the `FrameId`
`handle_begin_frame` minted alongside the pipeline's own result, since
`drive_frame_impl` now returns `(FrameId, R)` internally — `drive_frame`
discards the id to keep its existing `-> R`
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
  later, before this flush call returns — nested-microtask
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
  `handle_begin_frame` to flush it. An uncapped queue would let a microtask
  that re-enqueues itself forever starve the frame; FLUI trades a one-frame
  deferral on the legitimate-but-deep case, plus a small chance of
  under-running a genuinely pathological one, for the guarantee that no
  single frame hangs.

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
**Superseded:** issue #1056's ready-index rewrite deleted this guard;
`PumpGuard`'s `in_flight` arm (see "`AsyncDriver` indexes ready tasks
instead of scanning every resident one" below) does this same slot-removal
half, plus what this guard never had to do — restore the unreached tail of
the pump's own ready batch.

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

**Scope:** FLUI does not isolate per callback (see
`docs/PANIC-POLICY.md` and the doc note this crate already carries on the
topic) — a panic here still poisons and propagates the
whole frame. What this closes is narrower and Rust-specific: the
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
- Isolating each callback family per-entry, so a panicking transient
  callback cannot take the rest of the frame down with it. Rejected: this
  crate's panic policy is that a panic poisons the whole operation
  (`docs/PANIC-POLICY.md`); per-callback isolation would be a policy change
  well beyond a bookkeeping fix.

**Trade-off accepted:** `TaskQueue::execute_until`'s count budget also accepts
that a reentrant HIGHER-priority task displaces a still-queued, lower-
priority sibling to the next call rather than deferring behind it the way
`handle_begin_frame`'s id watermark does for transient callbacks — named
above rather than left as an unstated difference between the two queues'
bounds. **Unasserted:** no test pins this.

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
place. Transient callbacks receive the vsync `Instant`, and a second,
"legacy" registration path carrying different timing-argument semantics
(a `&FrameTiming` argument) next to `schedule_frame_callback` has no
reason to exist. The retired API had no distinct
semantics to preserve: its only production caller
(`RenderingBinding::request_visual_update`) passed an empty
closure, and every other caller in the crate's own doctest/examples used it
identically to `schedule_frame_callback`. Per this crate's
active-development posture (no shims, no deprecated aliases), the fix is
the deletion, not a lock-scope patch that leaves a second,
narrower-purpose registration API standing.

**Every remaining callback family already followed, and continues to
follow, the no-lock-held rule.** A shared test oracle
(`scheduler/lock_discipline_tests.rs`'s `assert_no_scheduler_lock_held`,
`#[cfg(test)]`, exhaustively destructuring `FrameState`/`CallbackState`/
`BindingState` plus a `TaskQueue` probe so a new lock cannot
be added to any of them without this oracle noticing) `try_lock()`s every
mutex a callback could legally observe and asserts each is free. It is
invoked from inside a transient callback and a frame-completion waker
(`callbacks_run_with_no_scheduler_lock_held`). For the persistent, shared
post-frame, owner-local post-frame, idle, microtask, lifecycle-listener
and timings-callback families and the `on_frame_scheduled` platform-wake
hook, **Unasserted:** no test pins this. Three sibling sites shared a
related but distinct hazard: `Vec::retain` drops the *removed* element
while the collection's lock is still held, so a cancelled/removed
callback's `Drop` could deadlock the same way if it re-entered the
scheduler. All three were fixed the same release
(`cancel_frame_callback`, `remove_lifecycle_state_listener`, and, for
consistency, `remove_timings_callback`) by locating the match under the
lock without a full-`Vec` copy (`position`+`remove` for the two
`CallbackId`-addressed sites, `extract_if` for the `Arc::ptr_eq`-addressed
one) and dropping it only after the guard falls. **Unasserted:** no test
pins this.

**Alternatives considered:**

- Snapshot `FrameTiming` under the lock and release it before invoking the
  legacy callback, keeping the API. Rejected: it does not remove the
  question of why two registration paths exist with overlapping purpose,
  and the active-dev policy in `AGENTS.md` prefers reshaping
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

**Rule:** a caller of `UpdateScheduler::ensure_visual_update` must have its
request honor `frames_enabled`. It calls the equally
`frames_enabled`-gated frame request from `SchedulerPhase::Idle`/
`PostFrameCallbacks` (a call from `PostFrameCallbacks` requests the NEXT
frame, since the current frame's own pipeline has already run by that
phase), and no-ops during the three mid-frame phases
(`TransientCallbacks`, `MidFrameMicrotasks`, `PersistentCallbacks`) —
but only for the thread already driving the frame (see `frame_thread`'s
own doc; a caller on any other thread always requests, since a lost
cross-thread wake is worse than a surplus frame). Even for the driving
thread this is not a blanket guarantee that the in-flight frame observes
the call: it holds for `.transientCallbacks`/`.midFrameMicrotasks`, which
precede the pipeline in the frame's slot order, but not for
`.persistentCallbacks`, where the pipeline itself runs. What keeps a
same-thread caller's demand from being lost is not these no-op arms but
what the demand is for: a pipeline visual-update issued mid-frame is by
construction already inside the frame that will observe it (the pipeline
runs in `.persistentCallbacks`, and the other two mid-frame phases precede
it), while ticker/animation demand travels
`Ticker::schedule_tick_if_active` (`ticker.rs`) to `schedule_frame_callback`,
whose own registration ends in an ungated `self.request_frame()` call,
independent of this phase gate. `UpdateScheduler::ensure_visual_update`
implements the phase switch as a `match` over `phase()`, with every
`SchedulerPhase` variant spelled out and no wildcard arm: on the thread
driving the frame, only `Idle`/`PostFrameCallbacks` reach the
`frames_enabled`-gated `schedule_frame_if_enabled` call; on any other
thread, every phase reaches it.

**Conflict:** `RenderingBinding::request_visual_update`
(`flui-runtime`'s `renderer_binding.rs`) called the retired
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
**Unasserted:** no test pins this.

**Alternatives considered:**

- Gate inside `request_visual_update` itself (`if scheduler.frames_enabled()
  { scheduler.request_frame(); }`), duplicating `schedule_frame_if_enabled`'s
  body at the call site. Rejected: the scheduler already owns this exact
  check; a second copy in a consuming crate is the kind of drift this
  crate's own `should_schedule_frame`/`schedule_frame_if_enabled` pair
  exists to prevent.

**Phase gate plus driving-thread requirement:** the `match` above is the
phase-based dedup `request_frame_impl`'s `frame_scheduled` coalescing
alone cannot provide: that flag says "a frame is already scheduled," not
"a frame is already running and would observe this request anyway." It
carries a same-thread requirement: `UpdateScheduler` is `Send + Sync` and
documented as reachable from any thread, so the mid-frame no-op arms only
apply when `frame_thread` names the calling thread. Scoped to
`ensure_visual_update` alone: `schedule_frame_if_enabled` (shared with
`end_of_frame`) and `set_frames_enabled` keep their existing, unrelated
contracts.

**`request_frame` stays intentionally ungated:** it is the "always
schedule, don't ask" entry point every unconditional caller (tickers via
`schedule_frame_callback`, `schedule_forced_frame`, the
`set_frames_enabled` re-enable edge) already reaches for, and its raw,
unconditional contract is unchanged by this phase gate.

### Pipeline visual updates route through `ensure_visual_update`'s phase gate

**Mapping decision** (ADR-0027 leapfrog zone: runtime/scheduling topology
is a sanctioned divergence point): the pipeline carrier no longer bypasses
`UpdateScheduler`. `PipelineOwner::request_visual_update`
(`flui-rendering/src/pipeline/owner/accessors.rs`) still calls
`VisualUpdateNotifier::fire_need_visual_update`
(`flui-rendering/src/pipeline/notifier.rs`), which still invokes the
closure a presentation registers via `owner.set_on_need_visual_update`
(`flui-runtime/src/presentation.rs`) — but that closure no longer calls
the realm's shared `visual_wake()` and no longer pokes
`window.request_redraw()` unconditionally. It now captures a
`WeakUpdateScheduler` (as `RenderingBinding` holds its scheduler)
and calls `scheduler.ensure_visual_update()`; only when that
returns `true` (phase gate passed AND frames enabled) does it poke
`window.request_redraw()`.

The realm's `wake` was already registered as the scheduler's
`on_frame_scheduled` hook (`ui_realm/`), so the `frame_scheduled`
false→true edge fires the platform wake exactly as before — routing the
pipeline carrier through `ensure_visual_update` makes the scheduler's
`frame_scheduled` flag the single carrier, and the presentation closure
keeps only the per-window `request_redraw()` poke. The runner's
`wake_action` (`flui-app/src/app/runner/frame_pacing.rs`) still ORs its
two parameters, but `realm.needs_redraw()` (the old pipeline carrier) is
no longer set by this closure; the scheduler's `frame_scheduled` flag is
now the one latch both a pipeline mark and every other demand source flip.

**Why the bool is not the `frame_scheduled` false→true edge:**
`ensure_visual_update` returns `true` iff the phase gate passed AND frames
are enabled, not iff the demand flipped `frame_scheduled`. The per-window
poke must fire even when a frame is already scheduled — a multi-window
realm that dirties window B after window A, both from Idle, still expects
B's window poked, so the closure must not gate the poke on the coalescing
edge.

**Re-entrancy re-proven, not assumed:** the closure fires while the caller
holds the pipeline cell checked out, and now reaches
`ensure_visual_update`, which locks `frame_thread` (mid-frame arm) and, via
`request_frame_impl`, clones-and-drops `on_frame_scheduled` outside its
lock. Deadlock would require a reverse path — a scheduler lock held across
a call back into the pipeline — and none exists: `handle_begin_frame`
stores `frame_thread` under a temporary statement-scoped guard released
before the pipeline runs, and `drive_frame_impl` runs the lane closure
with no scheduler lock held. A mid-frame pipeline mark therefore locks a
free `frame_thread`, reads `Some(current)`, returns `false`, and pokes
nothing.

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
Ready(_)) && scheduled_callback_id.is_none()`) is the
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

**Why the ticker needs a slot protocol:** a ticker that held one callback
for its entire life would need only its registration id
(`scheduled_callback_id`) to answer "which run does this checked-out
closure belong to". FLUI's
`Ticker::start` accepts a fresh callback on every run (`TickerProvider`'s
factory shape plus ad hoc `start(closure)` call sites), so the SAME ticker
legitimately dispatches through a sequence of different closures over its
life — the slot state machine is what tracks which one a given dispatch is
allowed to restore.

**Recorded limitations, not closed by this fix:**

- **`start_inner` while `Muted` bypasses the `Idle`/`Stopped` contract.**
  A muted ticker is still active (muting gates ticking and scheduling,
  never activity), so `start` on it is a second start of an active ticker.
  `Ticker::start_inner`'s own `debug_assert!`/early-return
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
`restart_inside_auto_tick_preserves_new_callback_and_one_pending_tick` pins
the restart-inside-a-tick failure directly, and
`a_panicking_tick_callback_leaves_the_slot_restored` pins the panic-unwind
fix. The manual `Ticker::tick(&self, ...)` path cannot support the SAME
reentrant-restart probe: restarting needs `&mut Ticker` (`stop`/`start`),
which — since `tick` takes only `&self` — is only reachable by wrapping the
ticker in an outer lock the CALLER holds for `tick`'s entire duration,
including the callback; a reentrant call back through that same
non-reentrant lock self-deadlocks before it ever reaches `stop()`. For the
mute/unmute-inside-a-tick retention failure, `dispose()`/`reset()` inside a
tick leaving no pending registration, the superseded callback being dropped
outside the lock, the manual path's (non-reentrant) restore contract, and
the restart-inside-a-tick failure reached through `AnimationController`'s
production call chain (a status listener chaining the next run, then
`stop()` cancelling it fully), **Unasserted:** no test pins this.

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

### A ticker's elapsed time is wall-clock time, not the frame timestamp

**Rule:** `Ticker` reports `start_time.elapsed()`: the wall-clock time since
`start`, read when the tick runs. The frame's vsync timestamp, which the
scheduler hands every transient callback, is ignored (`_vsync_time` in both
auto-tick registrations).

**Consequence:** anchoring on the first frame's timestamp would give every
ticker in a frame the same instant and let a test's fake clock drive them.
Here a host that
drives frames on a virtual clock (`flui-runtime`'s `UiRealm::pump` with a
`ManualClock`) moves the frame timestamp, the realm's `Vsync` controllers and
the scheduler's frame timing, but not an `AnimationController` built on the
scheduler: that one advances only as real time passes.

**Choice:** kept for now, named. Moving the ticker onto the frame timestamp
changes `start`, `mute`/`unmute`'s elapsed rebasing and the manual
`Ticker::tick` path together, and belongs with the headless driver that needs
it, not with the pump that exposed it. **Unasserted:** no test pins this.
Moving the ticker onto the frame timestamp should delete this entry.

### `end_of_frame` registers before it demands, and the live registry is the memo

**Rule:** `UpdateScheduler::end_of_frame` pushes its waiter onto the completion
registry FIRST and issues the frame demand second, and it issues one only when
the registry held no live waiter before that push.

**Conflict:** demand-then-register is not equivalent once a frame can run
concurrently. A frame beginning on another thread can both start and drain the
registry in the window between the demand and the push, so the waiter misses
the very frame it paid for and silently buys a redundant next one. Registering
first makes the registration the linearization point. An entry is then either
inside the batch a drain took under the registry guard, and is served by that
frame, or it was pushed after that guard was released, in which case its own
predicate reads the post-drain registry and demands.

The per-waiter registry exists because the futures are independently
cancellable values rather than listeners on one shared future, which is also
what makes the registry the natural place to keep the demand memo.

**Choice:** the predicate is "no LIVE entry", evaluated on the vec already held
under the registry guard, and the demand call is `schedule_frame_if_enabled()`
rather than the ungated `request_frame()`. The guard is released before the demand,
because the demand reaches the `on_frame_scheduled` hook, which must find every
scheduler mutex, `completion_waiters` included, free. **Unasserted:** no test
pins this.

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
    since it reads `frames_enabled` false there and stays silent on purpose.
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
  241 probes. **Unasserted:** no test pins this.
- Gating the demand on `phase() == Idle`. Rejected: it goes silent in the post-drain window, where
  `notify_frame_completion` has already emptied the registry but the phase is
  still `PostFrameCallbacks`, so a waiter registered from a completion waker
  hangs. **Unasserted:** no test pins this.
- A per-frame "a frame is already open" flag, cleared when the frame ends.
  Rejected for the same defect one level down: every candidate clear point sits
  later than the drain it is meant to pair with, so the flag is still set
  during the post-drain window. Deleting the flag removes the clear point
  rather than moving it.
- Adopting an `event-listener` event, as the former ticker backend did.
  Rejected on a structural
  reason this crate has paid for once: `Event::notify` calls `task.wake()`
  inside the closure holding its own internal list mutex, which is the shape
  issue #1057 removed from `notify_frame_completion`.
  `notify_frame_completion_still_wakes_a_later_waiter_when_an_earlier_waker_panics`
  pins one half of the contract it breaks. The other half, an inline-polling
  waker finding free the lock its own future's `poll` takes: **Unasserted:** no
  test pins this.

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
resolves `Err(SchedulerClosed)` entry below for how. Frame demand and wake delivery are separate facts: a panicking or absent
`on_frame_scheduled` hook leaves delivery debt. A later identical `request_frame`
or a hook installation retries it. Task wakers likewise retry an unpaid wake even
when their task is already indexed as ready. Consuming a demand and clearing its
issuance latch are synchronized, so a concurrent new request cannot be erased.

The private `WakeDelivery` stores one receipt identity and tracks active hook
threads. Fresh work overlapping a hook replaces the identity, so an older success
cannot acknowledge a newer failure. Serial successful requests reuse the receipt
and thread-index allocation. No callback runs with a delivery lock held. Same-thread
reentry records demand for at most one compensating attempt, which rereads the
currently installed hook. A replacement hook therefore pays the debt even when the
displaced hook panics. Both owning envelopes remain live until the active entry is
removed; the first panic remains authoritative when compensation also fails. A
callback failure retains both envelopes, because self-uninstallation or replacement
can make an opaque capture bundle's
Drop the next failure. Opaque secondary panic payloads are also intentionally
retained: an aggregate whose two fields both panic in Drop aborts even inside a
catch boundary. These exceptional leaks keep the first failure authoritative;
they do not change normal hook ownership. On successful delivery, initial-envelope
retirement happens after active bookkeeping closes; if retirement panics, its error
resumes after retaining the compensation envelope. Two panicking field destructors
inside that first retirement remain Rust's unavoidable aggregate-abort boundary.
A hookless request never acknowledges
debt. `coalesced_wake_delivery_recovery` exercises repeated requests, cloned task
wakers, overlapping receipts, missing hooks, reentry, competing failures and the next
operation. Its `reentrant_hook_replacement_delivers_the_current_hook_after_failure`
row covers replacement on both scheduling and task wakes;
`initial_hook_retirement_retains_the_compensating_envelope_on_failure` covers
competing capture retirement and the next operation. Hooks must still only wake
the owner, never drive a frame inline.

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
`TickerFuture`'s own `poll_resolution` shape in this crate (`ticker.rs`)
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
waker is invoked with `wake_by_ref` after its state guard is released, while the
owning executor envelope remains outside the catch. A failed invocation retains
that envelope; an ordinary successful invocation retires it through a separate
catch. Once an earlier callback, pipeline, wake or retirement failure exists (or
teardown is already unwinding), remaining opaque envelopes are retained after
waking, since their aggregate destruction cannot safely run over that failure.
Telemetry runs in its own catch and never displaces the delivery failure or stops
the tail. Opaque secondary payloads are retained without running drop glue. The
first failure propagates after normal frame delivery finishes; scheduler teardown
retains it instead. Explicit frame abort preserves the same chronology; recovery
of an already failed pipeline also retains its earlier authoritative payload.

A cancelled pending `FrameCompletionFuture` can itself own the final executor
waker. Its state destructor retains that envelope during unrelated unwind and
performs ordinary destruction otherwise. Exceptional retention is deliberate:
two panicking fields in one opaque destructor can abort before an outer catch
returns. Ordinary successful retirement with such an aggregate remains outside
the containment guarantee, as with ticker executor envelopes.
`completion_wake_ownership_and_recovery`, a row of
`end_of_frame_demand_matrix`, isolates normal/aborted/failed-pipeline delivery,
post-frame failure, teardown and existing unwind, successful envelope retention,
ordinary retirement, competing opaque payloads and telemetry, pending cancellation
and a next independent frame through the consumer API.

**The teardown lifetime guarantee remains partial:** any live
strong `UpdateScheduler` handle defers `Drop for SchedulerInner`, the same as
any other `Arc`. A task on an external executor that owns a clone does not
hang — dropping the executor drops the task, the clone, then the scheduler.
A task on the realm's own `OwnerFrame` that captured a clone holds the
scheduler until the realm retires the owner frame at teardown; the owner
frame is not part of the scheduler, so that is no longer a self-cycle.

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

**Outcome and teardown:** a bare completion signal would carry no outcome,
successful or otherwise, and no teardown sentinel. FLUI's per-realm
`UpdateScheduler` can be dropped mid-flight, so `end_of_frame` needs an
answer for that case.

**Alternatives considered:** a flat `enum FrameCompletionOutcome { Completed,
Aborted, SchedulerClosed }` (the issue's own sketch) — rejected above for
conflating a frame outcome with "no frame will ever resolve this," which the
`Result` split keeps separate and composable. A sentinel `FrameTiming` value
(e.g. all-zero) for the closed case — rejected: it is indistinguishable from
a real frame's timing by construction, exactly the "cannot tell them apart"
defect this issue exists to close, just moved to a new field instead of
solved.

### A ticker future registers atomically with its terminal-state read

**Rule:** durable resolution is the source of truth. While pending, each polled
`TickerFuture` owns one slab registration under the same state lock that publishes
completion or cancellation. Repeat polls replace that slot's waker; clones have
independent slots. Dropping a pending future removes its slot, so registration
storage tracks live polled futures rather than historical polling or clone churn.
Old wakers are retired only after unlocking. Publication takes both callbacks and
the waitset, then delivery invokes every waker outside all locks. Terminal polls
need no subscription and can safely run inline from a wake.

**Conflict:** `event-listener` 5.4.2 calls a waker under its intrusive-list lock,
preventing inline poll/drop. Its notify loop advances the next entry and marks it
notified before waking, but increments the notified count afterward. A panicking
waker therefore leaves inconsistent accounting; catch-and-retry cannot repair the
listener's later removal. This is why ticker waiting now uses the durable-state
slab instead of that notification backend (ADR-0106).

**Proof:** the consumer `ticker_future_delivery_recovery` table covers repeated
poll replacement, independent clones, immediate resource release on waiter drop,
publication racing registration, inline wake polling and dropping, failed wake
and retirement followed by healthy waiters, and a subsequent independent run.
Abort- and deadlock-capable negatives run in bounded child processes.

### The ticker resolves nothing; the controller owns the one run future

**Rule:** `Ticker::start`/`stop`/`dispose`/`reset` are fire-and-forget. Nothing
in this crate creates a `TickerFuture` for a ticker's own run any more —
`TickerFuture::pending()` hands its caller a `TickerCompleter`/`TickerFuture`
pair, and that caller (never the ticker) decides when and how to resolve it.

**Conflict:** a ticker that owned its run future and resolved it in
`stop`/`dispose` was the earlier shape (through #1167), and it was a defect
waiting to happen: every `ticker.stop()`
`AnimationController` calls runs **under the controller's own `inner` lock**
(`reset`, `settle_at_target`, `tick_simulation`, both `tick_time_based`
completion arms, `dispose`, `restart_ticker`, `stop_running`). A future the
ticker resolved would run its continuations and wake its pollers — arbitrary
user code — from inside that non-reentrant mutex.

**Choice:** move resolution up to the layer that owns the lock and the
cancel/complete distinction. The ticker keeps exactly one thing: a "run in
progress" fact (`TickerState::is_running()`), used only to refuse a second
`start`. See
`docs/adr/ADR-0064-animation-completion-is-one-controller-resolved-future.md`
for the full accounting, including why this is a moved fact and not a
different one.

### A second resolution is ignored, not asserted

**Rule:** `TickerCompleter::publish` is once-only; a second `complete`/`cancel`
call (including the implicit one `Drop` performs) is a silent no-op and the
first outcome stands.

**Choice:** a no-op, not an assertion that the transition happens exactly once. `TickerCompleter::complete`/`cancel`
consume `self`, so only `Drop` can ever attempt a second transition (when the
completer was explicitly resolved and then falls out of scope) — a hard
failure there would fire on the ordinary, correct path, not a bug. The
invariant is load-bearing rather than cosmetic: it is what lets a re-read
after registering stand in for a notification that was never delivered.

### Publish then deliver: two phases, so the fan-out never runs under a caller's lock

**Rule:** resolving a run is two calls, not one. `TickerCompleter::complete`/
`cancel` **publish** — set the durable state and take every continuation
registered so far, in one locked step — and return a `TickerDelivery`, which
**delivers**: run each continuation (its own `catch_unwind`), then notify
pollers. A caller finishes publishing while still holding its own lock and
defers delivery until after that lock is released.

**Conflict:** FLUI's fan-out is synchronous, so a single-phase resolve would
either run under whatever lock the resolver holds (the exact hazard the
previous section moves resolution to avoid) or force every resolver to
manually stage a two-step unlock dance with no shared shape.

**Choice:** the split is the shared shape.
`AnimationController::finish` is the chokepoint every run-ending or
run-starting site funnels through: drop the controller lock, notify value
listeners if the run's value changed, fire status listeners, THEN deliver.
Continuations run **before** wakers within delivery. Each continuation is
`FnMut`, called exactly once through a borrow of its owning envelope; this keeps
captured values out of an invocation's unwind. The already-resolved registration
path uses the same ownership boundary and remains synchronous. Normal capture
retirement follows invocation and can raise the first failure; subsequent opaque
envelopes are retained once a failure has priority. Callback, capture retirement,
wake, waker retirement and reporting failures cannot starve the remaining fan-out.
The first payload resumes after delivery, or is retained when delivery runs during
an existing unwind. Secondary payloads are always retained through the shared
foundation helper. These exceptional leaks are deliberate: aggregate drop glue
cannot be safely executed while preserving another failure. Two panicking fields
inside an ordinary first retirement remain an unavoidable Rust abort boundary.

`Drop for TickerDelivery` delivers if explicit delivery was omitted, and
`Drop for TickerCompleter` publishes cancellation and delivers. No runtime, host
or global registry is introduced. `ticker_future_delivery_recovery` pins these
paths, chronological competition, hostile captured values, reporting failures and
next-operation progress. ADR-0106 supersedes ADR-0064's invocation/waker policy
while preserving controller ownership and two-phase publication.

**Review checkpoint, not a test-checkable one:** `TickerCompleter::publish`
taking one lock for both "set the durable state" and "take the continuation
`Vec`" has no test that can fail if it is ever split into two locks instead —
the window between them contains no user code, so nothing observable changes
from outside. A registration racing across that window (a `when_complete_or_cancel`
call from another thread, landing between the two hypothetical locks) would
still see a consistent state either way in every case this crate's own tests
can drive. Guard the one-lock shape at review time: a future edit that splits
`publish` into "set state" then "take continuations" as two separate
`self.inner.state.lock()` calls is the regression to catch by reading the
diff, not by a red test.

### A start while a run is already installed is refused, in every build

**Rule:** `Ticker::start`/`start_default` refuse when the ticker is `Active`
*or* `Muted` — muting pauses a run rather than ending it. A refused start
logs at `error!` and drops the caller's callback; it does not touch ticker
state.

**Reason:** an unguarded second `start()` would overwrite the run and
the displaced future would never complete. Before the
controller took over resolution, FLUI keyed this same refusal on
`active_future.is_some()` — the future's own presence was the durable fact
`mute()` never touched, so the refusal survived muting where a narrower
`state == Active` test did not (issue #1161). With no future left on the
ticker, `TickerState::is_running()` (`Active | Muted`) is that same fact
restated directly against state, since a live run and a live future were
always the same thing here.

**Choice:** refuse on `is_running()`; keep the existing `debug_assert!` on
`state == Active` unchanged — two different questions, not two guesses at
one: the assertion answers "is this a *started twice* programming
error?" and the refusal answers "is there a live run I must not silently
replace?". A start on a *muted* ticker is a supported, refused operation
rather than a thrown error.

**Lock discipline, because the refusal runs user code:** the decision is taken
under `Mutex<TickerInner>` and acted on after it. Both the `tracing` event
(whose subscriber is arbitrary user code) and the rejected callback's `Drop`
(likewise) would otherwise be able to re-enter a non-reentrant mutex.

**Cross-crate consequence:** `AnimationController::restart_ticker` guards its
pre-start `stop()` on `TickerState::is_running()` for the identical reason —
`mute()`-then-`restart` must end the muted run before installing a new one, or
the ticker's own refusal above silently drops the new callback instead of
starting it.

### `when_complete_or_cancel` never blocks; it registers a continuation

**Rule:** `TickerFuture::when_complete_or_cancel` runs its callback
immediately, on the calling thread, when the future is already resolved —
including in the window between a `TickerCompleter` publishing and its
`TickerDelivery` running registered continuations. On a still-pending future
it stores the callback and returns; whichever `complete`/`cancel` (or its
`Drop`) resolves the future runs it later, from inside `TickerDelivery::deliver`.

**Conflict:** the earlier implementation blocked the calling thread on a
still-pending future, which meant there was no non-blocking route to react to a resolution without
`async`/`await`, and its wasm path silently reported a completion that had
not happened.

**Choice:** register a continuation and return immediately; an already-resolved (or resolving) future runs the callback
**synchronously on the caller's thread**, not on a microtask, so a registrant
must be safe to re-enter from this call, and the relative order between two
different registrants racing a resolution is not a contract. This removes the
wasm special case entirely — there is no blocking path left to fail on a
target with no thread to park.

### The realm owns its async tasks: `OwnerFrame` holds them, `AsyncDriver` is `Weak`

**Rule:** a realm's async tasks and owner-local post-frame callbacks live in
its `OwnerFrame` (ADR-0136 §2), of which the realm (`UiRealm`) and the
headless binding are the only strong owners. Futures are not `Send`: they
are created, polled and dropped on the owner thread. Widgets reach the tasks
through `AsyncDriver`, a `Weak` handle, so a leaked handle keeps nothing alive
and spawning through a dead one drops the future at once and returns a
cancelled token. Only `Waker`s and `FrameWaker` cross threads, each through a
`Weak`. Every frame entry point takes the owner frame — there is no entry
that polls or drains nothing — and an owner frame made for another scheduler
is neither polled nor drained.

**Teardown order:** `OwnerFrame::retire` closes both admission lanes, detaches their
ownership and disables every task waker and the frame hook before user destruction.
It drops the post-frame queue, then the
tasks, each under its own catch, keeping the first panic; the realm calls it
after closing its presentations and before resuming any earlier failure.
During an existing unwind the values are retained instead, the same limit
`TaskToken`'s `Drop` states.

**Tests:** `owner_local_task_matrix` in
`crates/flui-testing/tests/async_driver.rs` (`owner_local_future_completes_after_a_worker_wake`,
`late_completion_after_realm_drop_drops_captures_on_the_owner`,
`a_leaked_async_driver_holds_no_task_after_the_realm`);
`retirement_drops_every_task_and_keeps_the_first_panic` and
`retirement_drops_queued_callbacks_and_closes_the_queue` here;
`async_driver_unwind_matrix` covers reentrant callback destruction, sibling wakes,
eager-poll retirement and foreign-owner rejection without consuming frame demand;
`frame_waker_wakes_the_realm_from_a_worker` in `flui-runtime`.

### `AsyncDriver` indexes ready tasks instead of scanning every resident one

**Rule:** `OwnerFrame::poll_ready`'s cost scales with **ready** work (`R`),
never with resident tasks (`N`). An idle driver holding 100,000 dormant tasks
touches none of them; a mid-pump panic must not lose a sibling task that pump
never reached; and a genuinely-ready-but-stale index entry (a cancelled or
already-processed id, or a self-wake landing on a task that then panics)
self-heals within one pump rather than needing to be scrubbed eagerly.

**Conflict:** issue #1056 found `poll_ready` filtering `inner.tasks`'s entire
`BTreeMap` on every pump to find the (possibly zero) ready ids — an O(N) scan
that measured 0.77–0.91 ms at N=100,000 with R=0, spent before a single future
runs. (That figure is the issue's own reproducer: an external scratch crate
built under cargo's default release profile, no LTO, 16 codegen units. The
208.18 µs "before" and 22.93 ns "after" in the table below are a different
measurement — this workspace's own `cargo bench`, under `[profile.release]`'s
`lto = "thin"`, `codegen-units = 1`, on the same CPU — so the two "before"
numbers are not directly comparable; the table's own before/after pair is,
since both sides share that one methodology.) Readiness was recorded per task
(an `AtomicBool`) but never indexed independently of storage, so discovering
it meant re-deriving it from every task, every time.

**Choice:** the owner-local `TaskStore` keeps `tasks: RefCell<BTreeMap<TaskId,
Task>>` beside an index of ready ids, `ready: Mutex<Vec<TaskId>>`, in the
store's cross-thread half (the only part a `Waker` reaches). Every path that
sets a task's `ready` flag `true` (`spawn_local`, `TaskWaker::wake_by_ref`'s
false→true edge once the task is admitted, `spawn_local_eager`'s post-poll
check) also pushes the id into `ready`, so `poll_ready` only ever drains that
`Vec` — an idle driver drains an empty one. `ready` is wake-arrival order,
sorted and deduplicated once per drain (the dedup exists because a
cross-thread wake racing an eager spawn's admission and the spawn itself can
each push the same id — a real race, not a hypothetical one).

**`PumpGuard` owns one pump's whole drained batch, not just the id being
polled.** The naive fix — take a future out of its slot, poll it with no lock
held (unchanged discipline), reinsert on the outcome — loses every unreached
sibling to a mid-pump panic: the ids after the panicking one were already
removed from `store.ready` by the initial drain and are gone if nothing
restores them. `PumpGuard` holds the pump's `remaining` ids and a `cursor`
(advanced *before* each poll, so it always means "ids consumed" regardless of
which of a poll's two distinct unwind sites panics — the poll itself, or a
completed/cancelled task's own destructor, which runs later with `in_flight`
already cleared); its `Drop` restores `remaining[cursor..]` into `store.ready`
on any unwind, and removes the in-flight slot only when `in_flight` was
actually `Some` (a mid-poll panic), never when a later destructor panics (that
slot's fate was already committed under lock). This is what an O(N)-rescan
design never needed — it re-derives readiness from ground truth every call and
cannot strand a sibling — and what an index-based one owes back in return for
not scanning.

**Owned future failure boundary:** both lazy and eager polling borrow the
future into `catch_unwind`, keeping its ownership outside the closure. On
poll failure the future is retained without invoking opaque destruction, its
waker is cancelled, and the original payload resumes. `PumpGuard` removes the
empty slot and restores unreached siblings. A token dropped during an existing
unwind likewise detaches its task and retains its future. Catching `drop` would
not contain two panicking fields in one future; exceptional retention includes
all captures and nested tokens, so it does not promise recursive cleanup.
Ordinary completion and explicit cancellation still run destructors outside
the map lock and propagate their first failure. Panics competing inside a
user's own `poll` locals, or multiple fields of an ordinary opaque destructor,
remain subject to Rust's abort behavior.

Spawn establishes its token before invoking the frame hook: a failing hook
therefore cancels the task whose token could not be returned. The public
`async_driver_unwind_matrix` runs twelve cases in child processes, covering
lazy/eager poll failures with zero, one and two hostile destructors, stale
self-wakes, sibling progress, unwind cancellation, retirement failure,
nested cancellation and both spawn-hook rollback paths.

**Stale index entries are tolerated, not prevented.** `store.ready` is never
proactively purged on cancel, nor scrubbed for a self-woken id whose task then
panics: both go stale for at most one pump and self-heal via `poll_ready`'s
existing "id not found in `tasks` ⇒ skip" arm — cheaper than an O(R) scan on
every cancel, and the residue is bounded (at most one entry per spawn or wake
since the last drain; nothing spawns or wakes without eventually requesting a
frame).

**`spare` (capacity donation, not just correctness):** draining `store.ready`
with a bare `mem::take` at the top of every pump would install a *cold*,
zero-capacity `Vec` in its place, and a self-waking task's mid-pump push (its
waker fires synchronously, inside `poll`) would land in exactly that cold
`Vec`, regrowing it from empty every single pump, forever, for any steady
`R > 0` workload. `TaskStore` instead carries a second, always-empty `spare`
buffer that `poll_ready`'s drain step `mem::swap`s with `ready` (not
`mem::take`s): `ready` receives whatever `spare` warmed up to two pumps ago,
and `spare` receives this pump's actual batch, taken out via `mem::take`
(cold is fine here, since `spare` isn't touched again until this pump's own
end). `recycle`, called on both `poll_ready`'s normal return and
`PumpGuard::drop`'s unwind path, clears the drained batch and stores it as
the *next* `spare`, leaving `store.ready` itself untouched — it already
correctly holds this pump's discovered-ready ids.

Every task retains one `Waker` from spawn through retirement; `poll_ready`
clones it instead of allocating a fresh `Arc<TaskWaker>` per poll. The waker
holds a `Weak<Inner>`, so its copies cannot retain the driver or its futures.
A completed or cancelled task's old wakers remain inert through the existing
live-slot and cancellation checks. The public `task_waker_lifecycle` table
covers eager and lazy spawn, repeated-poll identity, cloned-waker coalescing,
completion, cancellation, the next task, and destruction of a driver whose
pending future exported its waker.

**Allocation gate, not just a bench:** `cargo xtask ci` has no bench step, so
`tests/async_driver_ready_index_allocation.rs` (a dedicated counting-allocator
binary) asserts that warm R=0 at N in {0, 100,000} and steady R=64 self-waking
tasks both allocate nothing. This pins reuse of both task wakers and ready
buffers. An allocation count still
cannot discriminate an O(N) scan from an O(R) drain when R=0, since
collecting zero ready ids allocates nothing either way. That an idle pump
reads no dormant task's readiness flag (an O(N) filter-scan calls `.load()`
once per resident task per pump regardless of readiness): **Unasserted:** no
test pins this.

`benches/async_driver_pump.rs` (criterion) is evidence attached to the PR, not
a gate. CI's `clippy`/`feature-matrix` jobs pass `--all-targets`/`--benches`,
so this file is type-checked and lint-checked on every PR; what CI never
does is link or run it (`bench-compile` only `cargo bench -p flui-rendering
--no-run`s). Local measurement, `main` (`f2f1c4d9`) vs. this change, on a
13th Gen Intel Core i9-13900K (32 threads) running `rustc 1.98.1 (48a229cea
2026-09-01)`:

| Group | N/R | `main` (before) | this branch (after) |
|---|---:|---:|---:|
| `empty_pump` | 0 | 9.84 ns | 20.63 ns |
| `empty_pump` | 100,000 | 208.18 µs | 22.93 ns (**~9,080× faster**) |
| `ready_heavy` | 1,000 | 124.95 µs | 122.92 µs |
| `ready_heavy` | 10,000 | 1.5846 ms | 1.3314 ms |

`ready_heavy` (R=N, every resident task genuinely polled) is essentially
unchanged, as expected: it was never the O(N)-scan problem this issue fixes,
so there is no O(N)-vs-O(R) gap for it to close.

### `TaskQueue::clear` — deleted, not fixed

**Rule:** an API with zero production callers and no distinct semantics from
an existing one is deleted outright, not patched in place, per this crate's
active-development posture (no shims, no dead surface kept "just in case").

**Conflict:** a lock-drop discipline sweep found `TaskQueue::clear` dropping
its cleared tasks while `queue`'s lock was still held — a
statement-under-guard shape that runs arbitrary `Drop` code inside the
critical section. Re-tracing its callers first: the only ones were its own
definition and one test (`tests/integration_tests.rs`'s
`test_task_queue_clear`) that existed solely to exercise the method itself,
not any behavior a caller depended on.

**Choice:** delete `TaskQueue::clear` and its test rather than move the drop
outside the lock. This is the identical shape and identical choice as the
retired `schedule_frame`/`current_frame()` family documented above ("No
legacy, lock-tied frame-callback registration API"): an unused API surface
carrying a lock-discipline hazard is not worth preserving just to fix its
hazard — deleting it removes the hazard AND the dead surface in one motion.
`count_by_priority`, which shares this queue, keeps its lock but now scopes
it to the counting loop only (no hazard there — `PriorityCount` is a plain
`Copy` struct with no significant `Drop` — but a lock held longer than the
work it protects is still worth narrowing on its own merits). It stays,
unlike `clear`, though it has no caller EITHER today: it is a read-only
diagnostics accessor, and an unused QUERY costs nothing and commits an
owner to no distinct behavior, where `clear` was an unused MUTATION whose
only two observers — its own definition and a test built solely to
exercise it — is precisely the shape this crate deletes on sight.

**Trade-off accepted:** none. Nothing outside this crate observed `clear`'s
existence.

### Identity exhaustion refuses reuse until explicit reset

`IdGenerator` atomically checks its increment and reserves `usize::MAX` as
an exhaustion state. Once reached, every later allocation panics without
wrapping through zero and restarting at one. An explicit `reset` retains its
documented deterministic-test behavior. `update_scheduler_bounds_matrix`
includes repeated caught exhaustion, reset, and concurrent last-value admission
through the public API. The ordinary sequence starts at one and is unchanged.
