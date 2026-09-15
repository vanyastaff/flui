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
matching batch into a local buffer before running any of it:

- `handle_begin_frame`'s transient-callback loop pops from the front of
  `CallbackState::transient` (now a `VecDeque`, not a `Vec`, to make the pop
  O(1) — the same reason `flush_microtasks` already used one) — but bounded
  to the queue's length AT ENTRY, read once before invoking anything, not
  "until empty": a callback that registers another transient callback of
  its own from inside itself must still have the new one deferred to the
  NEXT frame (issue #1058's reentrant-registration contract), and looping
  "until empty" would reach that newly-appended entry in the same call.
- [`TaskQueue::execute_until`](../../crates/flui-scheduler/src/task.rs) pops
  the top of the heap only while it still meets the priority threshold,
  one task at a time, instead of draining the whole matching run into a
  batch first. The atomic `len` mirror decrements per pop (inside the same
  critical section as the pop), matching `add_task`/`pop`'s existing
  ordering, so a panic mid-loop cannot leave it under- or over-reporting.
- `flush_microtasks` already had this shape and needed no change; it is the
  model the other two now follow.

The result: **the panicking entry itself is consumed; every entry still
queued behind it is preserved and runs on the next completed frame — not
retried, not silently dropped.** Persistent callbacks need no such
treatment at all: `handle_draw_frame` clones the registration list rather
than draining it (they "cannot be unregistered", per their own doc), so a
panic there costs nothing structurally — the same callback (including the
one that panicked) simply runs again next frame, same as always.

**The frame's post-frame callbacks do not run** for an aborted frame —
unchanged from before this issue; see `abort_frame`'s own doc. **Completion
waiters are woken with the aborted frame's timing** — an aborted frame is a
frame that finished, badly, and `end_of_frame()` must not hang forever
because of it. This is a **named limitation, not an oversight**: the
completion future's `Poll::Ready` value carries no success/failure
distinction, so a caller cannot tell an aborted frame apart from a
successful one purely by observing `end_of_frame()` resolve.

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
persistent/post-frame callback in its own `FlutterError`-reporting
boundary, so a throwing callback never unwinds Dart's call stack; their
`finally { _schedulerPhase = ... }` blocks exist for symmetry with
`handleDrawFrame`'s pipeline-exception handling, not because a callback
panic ever reaches them. FLUI does not isolate per callback — this issue
does not change that, and does not attempt to (see
`docs/PANIC-POLICY.md` and the port-check/doc note this crate already
carries on the topic) — a panic here still poisons and propagates the
whole frame. What this issue closes is narrower and Rust-specific: the
scheduler's OWN bookkeeping must never be left half-closed by an unwind it
did not choose to isolate.

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

**Trade-off accepted:** an aborted frame is indistinguishable from a
successful one through `end_of_frame()` alone (named above); a caller that
needs to tell them apart must catch the panic itself, not rely on the
completion future.

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
