# flui-scheduler Architecture

Per-crate ledger for architecture decisions that span more than one module in
this crate, as required by [`docs/PORT.md`](../../docs/PORT.md) §Per-crate
`ARCHITECTURE.md` template. Partial: this file exists for the `## Mapping
decisions` entries below; a full crate architecture writeup is deferred.

---

## Mapping decisions

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
place. Flutter's `SchedulerBinding` has exactly one one-shot registration
path, `scheduleFrameCallback`/transient callbacks (`scheduler/binding.dart`
@ 3.44.0), with no second, "legacy" variant carrying different
timing-argument semantics (`&FrameTiming` instead of the vsync `Instant`
transient callbacks already receive). The retired API had no distinct
semantics to preserve: its only production caller
(`RenderingFlutterBinding::request_visual_update`) passed an empty
closure, and every other caller in the crate's own doctest/examples used it
identically to `schedule_frame_callback`. Per this crate's
active-development posture (no shims, no deprecated aliases), the fix is
the deletion, not a lock-scope patch that leaves a second,
narrower-purpose registration API standing.

**Every remaining callback family already followed, and continues to
follow, the no-lock-held rule.** This is proven by a shared test oracle
(`scheduler.rs`'s `assert_no_scheduler_lock_held`, `#[cfg(test)]`) that
`try_lock()`s every mutex a callback could legally observe and asserts each
is free, invoked from inside a callback registered in every family:
transient, persistent, shared post-frame, owner-local post-frame, idle,
microtask, lifecycle listener, timings callback, and the
`on_frame_scheduled` platform-wake hook. Three sibling sites shared a
related but distinct hazard: `Vec::retain` drops the *removed* element
while the collection's lock is still held, so a cancelled/removed
callback's `Drop` could deadlock the same way if it re-entered the
scheduler. All three were fixed the same release
(`cancel_frame_callback`, `remove_lifecycle_state_listener`, and, for
consistency, `remove_timings_callback`) by partitioning the match out
under the lock and dropping it only after the guard falls. Two of the
three carry a bounded reproduction
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

### `request_visual_update` routes through the gated pair, not the raw one

**Rule:** requesting a frame in response to pipeline work must honor
`frames_enabled`, matching Flutter's `ensureVisualUpdate`
(`scheduler/binding.dart` @ 3.44.0), which calls `scheduleFrame()` (itself
gated on `framesEnabled`) and returns early mid-frame.

**Conflict:** `RenderingFlutterBinding::request_visual_update`
(`flui-app`'s `bindings/renderer_binding.rs`) called the retired
`schedule_frame`, which pushed onto the (now-deleted) legacy queue and then
called the **ungated** `request_frame()` directly. A binding whose
scheduler had frames disabled (backgrounded app, `Hidden`/`Paused`/
`Detached` lifecycle state) would still schedule a frame on every pipeline
request.

**Choice:** `request_visual_update` now calls
`UpdateScheduler::ensure_visual_update()`, which already existed as the
gated pair (`ensure_visual_update` calls `schedule_frame_if_enabled`,
which calls `request_frame` only when `frames_enabled` is true) and needed
no new code, only a caller. `crates/flui-app/src/bindings/renderer_binding.rs`'s
test module pins both edges:
`request_visual_update_does_not_schedule_a_frame_while_frames_are_disabled`
and `request_visual_update_schedules_a_frame_while_frames_are_enabled`.

**Alternatives considered:**

- Gate inside `request_visual_update` itself (`if scheduler.frames_enabled()
  { scheduler.request_frame(); }`), duplicating `schedule_frame_if_enabled`'s
  body at the call site. Rejected: the scheduler already owns this exact
  check; a second copy in a consuming crate is the kind of drift this
  crate's own `should_schedule_frame`/`schedule_frame_if_enabled` pair
  exists to prevent.

**Trade-off accepted:** none. This is a pure bug fix (a previously ungated
path adopts an existing, already-tested gate) with no behavior this crate
intends to keep.
