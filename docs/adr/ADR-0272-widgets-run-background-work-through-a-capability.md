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
A compute lane on Web workers is a gap ([No compute lane off the UI thread on the Web](../gaps.md#no-compute-lane-off-the-ui-thread-on-the-web)).

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

## Amendment (2026-11-02): a caller waits for room on a lane

### Context

A full lane refuses with `SpawnError::Saturated` (ADR-0047), and nothing told the caller when
the lane had room again. The asset registry's image loads read on the IO lane and decode on the
compute lane, so a decode refused for want of room ran where the load ran: on an IO worker,
holding it for the length of a CPU-bound decode, which is what the two lanes exist to keep
apart. A caller had no choice but to run the work in place, shed it, or poll on a timer the frame
path does not have.

### Options

1. **A queued spawn, `spawn_compute_queued`, that the executor holds until it has room.**
   - Good: one call for the caller.
   - Bad: the executor queues without bound, which ADR-0047 refuses; the bound would move to
     each caller, unseen by the host's admission.
2. **A poll per lane, `poll_compute_room(cx)` and `poll_io_room(cx)`.**
   - Good: no allocation; the executor keeps refusing.
   - Bad: a method per lane. A poll cannot tell that its caller gave up, so a waker registered
     by a wait that was dropped stays until the lane frees room; waits started and abandoned
     while a lane stays full pile up.
3. **One poll that names the lane, `poll_room(Lane, cx)`.**
   - Good: a new lane is a `Lane` variant, not a new method.
   - Bad: the same pile-up as option 2.
4. **One wait that names the lane, `room(Lane) -> RoomFuture`, a named future type.**
   - Good: a new lane is a `Lane` variant; the wait owns its registration, so dropping it lets go
     of its waker at once; a host pool's wait and the services' shutdown compose in one future.
   - Bad: a boxed future per wait, on a path taken only when a lane is full. A named type, not a
     bare `Pin<Box<dyn Future>>`, keeps how it is held free to change.
5. **A wait inside `flui-assets` only, on its own decode slots.**
   - Good: no API change.
   - Bad: a lane full of other work still sends the decode to the IO worker.

### Decision

Chosen option: 4.

```rust
#[non_exhaustive]
pub enum Lane { Compute, Io }

/// A named type, so how a wait is held can change without breaking callers.
#[must_use]
pub struct RoomFuture(/* Pin<Box<dyn Future<Output = Result<(), SpawnError>> + Send>> */);
impl RoomFuture {
    pub fn new(wait: impl Future<Output = Result<(), SpawnError>> + Send + 'static) -> Self;
    pub fn ready(answer: Result<(), SpawnError>) -> Self;
}
impl Future for RoomFuture { type Output = Result<(), SpawnError>; }

pub trait Executor: Send + Sync + 'static {
    fn spawn_compute(&self, job: ComputeJob) -> Result<(), SpawnError>;
    fn spawn_io(&self, future: IoFuture) -> Result<(), SpawnError>;
    fn room(&self, lane: Lane) -> RoomFuture;
}

// flui-runtime: a host pool answers for its own queue.
pub trait HostComputePool: Send + Sync {
    fn spawn_job(&self, job: ComputeJob) -> Result<(), SpawnError>;
    fn room(&self) -> RoomFuture;
}
pub trait HostIoPool: Send + Sync {
    fn spawn_future(&self, future: IoFuture) -> Result<(), SpawnError>;
    fn room(&self) -> RoomFuture;
}
```

1. `room` has no default body, on `Executor` and on both host pools: an implementation that
   refuses with `Saturated` says when it has room, or a caller waiting on it would wait forever.
2. `Ok(())` means room now, not reserved: a spawn that follows may find it taken and waits again.
   `Err(_)` means waiting cannot help: `ShuttingDown`, or `Unavailable` (workers that could not
   start, or a lane the executor does not run). Dropping the wait lets go of its waker.
3. `ExecutionServices::room` waits on its admission window, then on the host's pool, and hears
   shutdown at either. The window and the shutdown each keep one registration per wait (a repoll
   replaces its waker), removed when the wait resolves or is dropped. A unit of work that leaves
   the window wakes its waits outside the lock, past a waker that panics, raising the first panic
   once every one is woken (ADR-0281). Shutdown stops admission, then wakes every wait last, a
   wait on a host pool included, and each hears `ShuttingDown`.
4. The asset registry's decode waits for room on a `Saturated` compute lane and spawns again; it
   decodes where the load runs only on `Unavailable`. This amends
   [ADR-0279](ADR-0279-image-loads-run-on-the-lanes-their-caller-holds.md) §3.

`Background` (the widget capability) does not expose `room`: its refused spawn answers
`Refused`, and a widget that wants to wait is a follow-up with its first caller.

### Consequences

- Good, because CPU work refused by a full compute lane no longer lands on an IO worker.
- Good, because the executor still refuses rather than queues, and a wait holds a registration
  only while it is alive.
- Bad, because every `Executor` and host pool implements one more method, and a host pool that
  refuses must wake the waits on it.

### Confirmation

- `flui-runtime`'s `execution_lane_matrix`: `a_full_lane_wakes_its_waiters_when_work_leaves_it`
  (a repoll holds one place, a panicking waker does not cost the rest their wake),
  `a_dropped_wait_lets_go_of_its_waker`, `shutdown_wakes_a_waiter_for_room` (a wait on the
  window and a wait on a full host pool).
- `flui-assets`' `image_loads_run_on_their_lanes_and_survive_the_requests_that_leave`:
  `a_full_compute_lane_is_waited_for_off_the_io_lane` (the IO future yields, decoding nothing,
  until the compute lane has room), `a_lane_shutting_down_ends_the_wait_for_room`, and
  `saturated_lanes_leave_the_load_to_the_request`.
