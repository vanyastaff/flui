# ADR-0275: Owner-local tasks sleep on the UI runtime's clock, through deadlines the host's wait honours

- **Status:** Accepted (2026-10-30)
- **Date:** 2026-10-30
- **Related:** [ADR-0035](ADR-0035-lifecycle-consolidation-and-frames-enabled.md) (frames enabled
  per lifecycle state, which decides the turn a due deadline gets),
  [ADR-0262](ADR-0262-persisted-documents-over-a-byte-storage-capability.md) (a document retries
  `Busy` on this clock), [ADR-0078](ADR-0078-rules-live-in-types-and-lints.md) (capabilities are
  acquired in lifecycle hooks)

## Context and problem statement

An owner-local task (`AsyncDriver::spawn_local`) can wait for a waker but not for time. Work that
must wait a while before trying again had no way to do it inside FLUI: a persisted document's
`Busy` retry (50 to 800 ms), a debounce, a delayed tooltip. Spawning a thread that sleeps reads the
wall clock, which a test cannot advance, and wakes nothing the host's wait knows about, so a host
idle in `WaitUntil` would not run the turn that resumes the task. The UI runtime already folds its
own deadlines (gesture arena, dropped-frame retry, haptics) into `UiRuntime::next_wake`, each on
the runtime's clock. How does an owner-local task wait for an instant on that clock?

## Decision drivers

- One timeline: a sleep reads the clock the runtime's frames and gestures read, the testing kit's
  `ManualClock` in a test, so a test steps it exactly (49 ms is not 50 ms).
- The host's idle wait wakes for a sleep's deadline; no thread sleeps.
- The frame path stays synchronous: firing a deadline wakes a task, it never runs one inline.
- Teardown drops what it owns: no waker outlives the owner, and none is woken during teardown.
- One general facility, not one timer per feature.

## Considered options

1. **Deadlines in the owner frame's task store, read from the scheduler's clock, folded into
   `next_wake`.**
2. **A timer thread per UI runtime** that sleeps and wakes the task.
3. **Each feature keeps its own deadline in the UI runtime** (as gesture and haptic deadlines do).

## Decision

Chosen option: 1, because it puts a task's wait on the runtime's own timeline and the host's own
wait, at the cost of one store beside the tasks.

```rust
// flui-scheduler, re-exported by flui-view
impl AsyncDriver {
    pub fn timer(&self) -> TimerHandle;
}
pub struct TimerHandle { /* Weak to the owner's deadlines; !Send; Clone */ }
impl TimerHandle {
    pub fn now(&self) -> Instant;
    pub fn sleep(&self, duration: Duration) -> Sleep;
    pub fn sleep_until(&self, deadline: Instant) -> Sleep;
}
pub struct Sleep { /* !Send */ }
impl Future for Sleep { type Output = (); }
impl OwnerFrame {
    pub fn next_deadline(&self) -> Option<Instant>;
}
// flui-view
trait LifecycleContext {
    fn timer(&self) -> Option<TimerHandle> { self.async_driver().map(|d| d.timer()) }
}
```

- **Where.** The deadlines live in the task store the `OwnerFrame` owns, and read the clock its
  `UpdateScheduler` was built with. `TimerHandle` is weak, so a leaked handle keeps nothing alive.
- **When `timer()` is `None`.** Exactly when `async_driver()` is `None`: a tree bound to no UI
  runtime has no clock to sleep on. A widget acquires it in a lifecycle hook, like the driver.
- **Arming.** A `Sleep` polled before its deadline stores its waker under `(deadline, id)`, ids
  minted from a counter that panics as a bug rather than wrap. Dropping a `Sleep` removes its entry.
  A deadline past what `Instant` can hold never resolves.
- **Firing.** Every owner turn that polls tasks (`drive_frame`, `pump_background`) first fires the
  deadlines due by the clock's now, earliest first. Each entry is removed and the borrow released
  before its waker is woken, so a waker that panics loses no later entry; the woken tasks are
  polled in the same batch.
- **Waking the host.** `UiRuntime::next_wake` folds in `OwnerFrame::next_deadline`, and
  `UiRuntime::has_pending_work` is true once that deadline is due, so the turn the host's wake runs
  is not skipped.
- **Teardown.** `OwnerFrame::retire` closes the deadlines with the tasks: their wakers are dropped
  under the retirement's failure policy, never woken, and nothing is armed again: the tasks they
  belong to go with the same owner. A `Sleep` polled after that stays pending and keeps no waker;
  one made after it never resolves.

### Consequences

- Good, because a task waits for time with `timer.sleep(d).await`, on the clock a test steps.
- Good, because the host's wait already honours `next_wake`; no new wake path.
- Bad, because a deadline is fired only by an owner turn: while frames are disabled the host runs
  one only for background work it already pumps, so a sleep in a hidden application may resolve
  late. A document's `Busy` retry therefore also retries at once on `Hidden`/`Paused`
  (ADR-0262 §3).
- Neutral, because the resolution is the host wait's, not a hard real-time timer.

### Confirmation

- `a_sleep_resolves_at_its_deadline_on_the_runtime_clock` (`flui-scheduler`'s `scheduler_it`): 49
  ms leaves the task pending, 50 ms fires and polls it in the same turn, and `next_deadline`
  reports the deadline meanwhile.
- `a_dropped_sleep_disarms_only_its_deadline` (same target).
- `a_sleep_outliving_its_owner_stays_pending_without_a_waker` (same target): teardown drops the
  stored waker without waking it, and a later poll keeps none.
- `a_task_sleep_follows_the_kit_clock` and `teardown_drops_a_sleeping_task`
  (`flui-testing`'s `flui_testing_it`): through `LifecycleContext::timer` and the kit's
  `pump_for`.

## Pros and cons of the options

### 1. Deadlines in the owner frame's task store

- Good, because the clock, the wait and the poll are the ones the runtime already has.
- Good, because tasks and their deadlines share one owner and one teardown.
- Bad, because firing waits for an owner turn.

### 2. A timer thread per UI runtime

- Good, because it fires on time with no owner turn.
- Bad, because it reads the wall clock, so a test cannot step it, and its wake reaches the task but
  not the host's wait accounting.
- Bad, because it is a thread per runtime for work the owner loop already schedules.

### 3. A deadline per feature in the UI runtime

- Good, because each feature's wake is explicit in `next_wake`.
- Bad, because every feature that waits needs its own field and fold; a widget outside the runtime
  crate cannot add one.

## More information

The deadlines a UI runtime already keeps (gesture arena, dropped-frame retry, haptics, text
preparation) are listed in `UiRuntime::next_wake_excluding_agent_waits`; this adds the owner's
task deadlines beside them.
