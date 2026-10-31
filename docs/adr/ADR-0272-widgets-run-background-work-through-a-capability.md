# ADR-0272: Widgets run background work through a capability

- **Status:** Accepted (2026-10-30)
- **Date:** 2026-10-30
- **Superseded-by:** [ADR-0281](ADR-0281-a-destructor-that-panics-during-an-unwind-aborts-as-in-std.md),
  §3's keeping of a panicking closure's captures and of a failed IO future, and §4's payload
  retention, only: both are dropped normally.
- **Amends:** [ADR-0047](ADR-0047-unified-execution-services.md) "One owner, no ambient reach":
  code below the host reaches the lanes through the background capability, whose handle holds
  an `Executor`, never `ExecutionServices`.
- **Closes:** [ADR-0049](ADR-0049-task-worker-service-lifecycles.md)'s deferred "widget-tier
  spawning capability".
- **Related:** [ADR-0152](ADR-0152-capability-seam-revised.md) (the capability seam and its
  owner-thread completions); [ADR-0078](ADR-0078-rules-live-in-types-and-lints.md)
  (capabilities are acquired in lifecycle hooks); [ADR-0243](ADR-0243-one-testing-kit-per-layer.md)
  (time and frames only from the kit)

## Context and problem statement

`ExecutionServices` (ADR-0047) owns two bounded lanes, compute and IO, with admission,
cancellation and a deadline-bounded shutdown. Only `flui-app` can reach it, and ADR-0049's task,
worker and service handles reach application code only through `ServiceContext`. A `TaskHandle`
is not a future, holds no waker, and its end does not wake the UI runtime.

So a widget that decodes an image, lays out a large document or reads a file has no sanctioned
place to run it. Code improvises: `flui-assets` starts a private Tokio runtime outside the
admission bounds, and other code spawns `std::thread`. That work is unbounded, unprioritised and
invisible to shutdown, and its answer reaches the widget by a channel the widget polls, or by a
lock. How does widget code run work off the owner thread, inside the host's bounds, and get its
answer back on the owner thread?

## Decision drivers

- Work runs inside the host's admission bounds and is cancelled at its shutdown, whoever owns
  the threads (ADR-0047's host injection).
- The answer arrives on the owner thread at a turn boundary, never inline in the call that
  started the work, and wakes the UI runtime.
- Unowned work is the defect (ADR-0049): every unit ends when its owner lets go of it.
- A panic in background work is contained where it ran and answered as a value, not resumed on
  the owner thread.
- A test sees the loading state and the answer frame by frame, with no real threads.
- Dependencies point down the tiers: `flui-view` cannot name `flui-runtime`.

## Considered options

1. **Hand widgets `ServiceContext` and `TaskHandle`.**
   - Good: one surface for background work.
   - Bad: a `TaskHandle` answers off the owner thread and wakes nothing; a widget would poll it
     each frame. `ServiceContext` lives in `flui-runtime`, which `flui-view` cannot name.
2. **A `BuildContext` accessor that returns the lanes.**
   - Good: no registration.
   - Bad: it breaks ADR-0078 (capabilities are acquired in lifecycle hooks, never in `build`), and
     an application could not replace the lanes.
3. **A capability on the ADR-0152 seam whose handle holds an `Executor` and the owner's
   `CompletionSource`.**
   - Good: acquired like the clipboard and storage, replaceable by an application, answered
     through the seam's owner-thread `Completion`, which wakes the UI runtime and reports a
     retired owner.
   - Bad: the spawn types and an `Executor` trait move down to `flui-platform`.
4. **A second executor trait in `flui-view`.**
   - Good: `flui-platform` is untouched.
   - Bad: two traits for one contract; `flui-runtime` would implement both.

## Decision

Chosen option: 3, because it is the seam every other capability uses, so the answer, the wake,
the replacement and the retired owner all come from code that already holds them.

### The executor moves down

`flui_platform::executor` holds `ComputeJob`, `IoFuture`, `SpawnError` (moved from
`flui-runtime`, which re-exports them) and the trait the handle holds:

```rust
pub trait Executor: Send + Sync + 'static {
    fn spawn_compute(&self, job: ComputeJob) -> Result<(), SpawnError>;
    fn spawn_io(&self, future: IoFuture) -> Result<(), SpawnError>;
}
```

`ExecutionServices` implements it, so admission, host injection and shutdown cancellation apply
to every unit a widget starts.

### The capability

```rust
pub enum BackgroundCapability {}                     // NAME = "background"

#[derive(Clone)]
pub struct Background { /* Arc<dyn Executor>, CompletionSource */ }
impl Background {
    pub fn new(executor: Arc<dyn Executor>, completions: CompletionSource) -> Self;
    pub fn compute<T: Send + 'static>(&self, job: impl FnOnce() -> T + Send + 'static) -> BackgroundTask<T>;
    pub fn io<T: Send + 'static>(&self, future: impl Future<Output = T> + Send + 'static) -> BackgroundTask<T>;
}

#[must_use = "dropping a BackgroundTask cancels its work"]
pub struct BackgroundTask<T: 'static> { /* Completion<Result<T, BackgroundError>>, cancel */ }
impl<T> BackgroundTask<T> {
    pub fn cancel(&self);
    pub fn on_complete(self, f: impl FnMut(Result<T, BackgroundError>) + 'static);
    pub fn poll(&mut self) -> Option<Result<T, BackgroundError>>;
    pub fn into_future(self) -> impl Future<Output = Result<T, BackgroundError>>;
}

#[non_exhaustive]
pub enum BackgroundError { Refused(SpawnError), Cancelled, Panicked, OwnerRetired }
```

1. A widget acquires the handle in `init_state` or `did_change_dependencies` (ADR-0078) and
   keeps it.
2. The answer is the seam's `Completion`: it settles at an owner turn boundary, never inside
   `compute` or `io`, and a refused spawn settles the same way, as `Refused`.
3. Dropping the task, or the future `into_future` returns, cancels the work: a job not yet
   started is skipped, and a future is woken and dropped at its next poll. A job already running
   runs to its end, its value is dropped on the worker and the task answers `Cancelled`: a
   cancel wins over a value. `on_complete` hands the answer to its closure and keeps the work
   alive; to cancel, keep the task instead. The closure is an `FnMut` called once, as the seam's
   is, so a panic in it keeps its captures rather than dropping them mid-unwind, and a failed IO
   future is kept, not dropped. The compute job stays an `FnOnce`, since jobs consume their
   captures (`move || parse(bytes)`); a capture whose `Drop` panics during the job's unwind
   aborts, as it does anywhere in Rust.
4. A panic in a job or a poll is caught where it ran, logged once with its message, its payload
   kept off the unwinding path (`retain_opaque_payload`), and answered as `Panicked`.
5. Work the host discarded at shutdown answers `Cancelled`; a UI runtime that ended first
   answers `OwnerRetired`.

### Providers

- `flui-app` provides it over the run's `ExecutionServices`, held weakly: a spawn after the
  services are gone is refused with `ShuttingDown`, and before they start the capability answers
  `Unavailable`. An application replaces it like any other capability.
- `flui-testing`'s headless host provides it over `DeterministicExecutors`. Its `pump` runs the
  queued work before the frame, so an answer settles in the same pump. `hold_background` stops
  that, `release_background` resumes it, `run_background` runs the work now, held or not, and
  `pending_background` counts the work not finished; `pump_until_settled` counts it, and
  `time_to_next_frame` asks for a frame while work is runnable, not while a future is parked.
  `LaidOut` has the same four methods.

### wasm32

`ExecutionServices` is sequential there (ADR-0047): compute runs inline at the spawn site, on
the UI thread, so a long job blocks the frame. The answer still settles at the next owner turn.
A compute lane on Web workers is a gap ([`docs/gaps.md`](../gaps.md) entry 40).

### Consequences

- Good, because widget work runs inside the host's bounds, is cancelled at shutdown and ends
  when its widget lets go of it.
- Good, because the answer reaches the owner thread through the seam that wakes the UI runtime,
  so a widget needs no channel, lock or polling.
- Good, because a test steps the loading state and the answer with the kit's pump.
- Bad, because `flui-platform` carries the spawn types; `flui-runtime` re-exports them so its
  callers are unchanged.
- Neutral, because `TaskHandle`, workers and services stay the application tier's surface
  (ADR-0049); the capability is the widget tier's.
- Follow-ups: `Image::file` and `Image::memory` decode through `Background::compute` with a cache
  key; `flui-assets` drops its private Tokio runtime for `Background::io`.

### Confirmation

- The `background_work` table of `flui-testing`'s integration tests: an answer settles at the
  pump that runs the job (`a_compute_job_answers_at_the_pump_that_runs_it`), `on_complete` runs
  once, held work keeps the host unsettled until released, `run_background` runs held work, a
  dropped task skips its job and drops its parked future
  (`a_dropped_task_drops_its_parked_future_at_the_next_pump`), a cancelled task answers
  `Cancelled`, a panic answers `Panicked`, and a refused spawn answers `Refused` at the next
  pump.
- `cargo xtask workspace` keeps `flui-view` off `flui-runtime`: the handle names only
  `flui-platform`'s `Executor`.
