# ADR-0368: A frame polls its tasks, runs its pipeline, then its post-frame callbacks

- **Status:** Implemented (2026-11-18)
- **Date:** 2026-11-18
- **Supersedes:** [ADR-0347](ADR-0347-a-frame-request-carries-the-hosts-frame-times-and-reports-that-name-their-frame.md)
  decision 3 only (`target_present`, or the refresh grid of the newest presentation report,
  bounds idle work): no idle work is bounded, and the request's times stay data
- **Supersedes:** [ADR-0021](ADR-0021-hero-flight-seam.md) §3 the phase list of the frame contract
  only (transient callbacks, microtasks, persistent callbacks, the task queue); its rule that
  post-frame callbacks see this frame's layout stands
- **Supersedes:** [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md) §1 the scheduler
  phases inside the frame transaction only (begin frame with transient callbacks and the
  microtask flush, draw frame with persistent callbacks and the task queue)
- **Supersedes:** [ADR-0136](ADR-0136-owner-local-ui-surfaces.md) §2 the clause that a frame polls
  its tasks in `MidFrameMicrotasks` inside `handle_begin_frame` only
- **Supersedes:** [ADR-0182](ADR-0182-complete-owner-execution.md) the clause that preparation runs
  "before transient callbacks" only: it runs before the frame's tasks are polled
- **Supersedes:** [ADR-0039](ADR-0039-event-loop-affinity-capability.md) the phase names of the
  scoped-access rule only: `with_owner_platform` refuses to run during `SchedulerPhase::Frame`
- **Supersedes:** [ADR-0035](ADR-0035-lifecycle-consolidation-and-frames-enabled.md) §1 the
  sentence keeping `should_schedule_frame()` as an alias of `frames_enabled()` only
- **Supersedes:** [ADR-0266](ADR-0266-one-owner-thread-ui-runtimes-isolate.md) §3 the sentence on
  `TimingsCallback` only: the scheduler keeps no timings lane
- **Related:** [ADR-0175](ADR-0175-owner-local-scheduling-and-animation.md) (animation time is the runtime's
  `Vsync` controllers, not scheduler tickers), [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md)
  (one frame transaction), [ADR-0257](ADR-0257-frame-timing-and-latency.md) (where the idle bound
  came from)

## Context and problem statement

`UpdateScheduler` carried Flutter's `SchedulerBinding` lanes: one-shot ("transient") frame
callbacks, persistent frame callbacks, a microtask queue, a priority task queue with an idle
deadline, and a frame-timings callback lane. FLUI fills none of them. Animation time comes from the
runtime's `Vsync` controllers (ADR-0175), the pipeline is the closure the runtime hands
`drive_frame`, deferred work is an async task on the owner's `AsyncDriver`, and per-frame cost is
`FrameClock`'s `FrameSnapshot`. Outside the scheduler's own tests, no production path registered a
transient or persistent callback, queued a microtask or a priority task, or subscribed to timings.

Each lane still cost the frame path its machinery: five phases where two do the work, re-entry caps
and a run budget for queues nothing fills, an idle deadline every host computed (ADR-0347
decision 3) for idle tasks nothing queued, a cancellation set, and a phase each diagnostic had to
name. Each lane was also a second way to do what the runtime already does one way: a widget that
took a transient callback for animation would tick off the `MotionClock` every other animation
reads.

Which phases does a frame have, and which lanes does the scheduler keep?

## Decision drivers

- One way to do each thing: a second registration path for frame work is the next change's copy.
- The frame path is synchronous and does only work it has (ADR-0083).
- Animation reads one clock: the runtime's `Vsync` controllers and `MotionClock` (ADR-0175).
- Accepted work is delivered or refused on every exit; a lane nobody fills is a lane whose failure
  paths nobody exercises.
- Flutter's shape is the reference: its transient callbacks are tickers, its persistent callback
  is the pipeline and its task queue is `scheduleTask`. FLUI already has each of those as its own
  form; the departure is that the forms are not scheduler lanes.

## Considered options

1. Delete the lanes; a frame is `Idle → Frame → PostFrame`.
2. Keep the lanes as public API for application authors.
3. Deprecate the lanes and keep them working until a later release.

## Decision

Option 1.

1. **A frame has three phases.** `SchedulerPhase` is `Idle`, `Frame` and `PostFrame`
   (`#[non_exhaustive]`, `repr(u8)`). `Frame` polls the owner's ready async tasks once, then runs
   the pipeline closure; `PostFrame` runs the post-frame callbacks and resolves the `end_of_frame`
   waiters; the frame then returns to `Idle`. A panic in `Frame` aborts the frame: its post-frame
   callbacks do not run, its completion waiters resolve `Aborted`, and the phase returns to `Idle`
   without passing through the phase machine.
2. **The scheduler keeps no other lane.** There is no one-shot or persistent frame callback, no
   microtask queue, no priority task queue and no timings callback. Work for a later frame is an
   async task (`AsyncDriver::spawn_local`), a dirty mark on the tree, a `Vsync` controller of the
   runtime, or a post-frame callback; the lifecycle listeners and the completion futures stay.
3. **The frame drive takes no deadline.** `__runtime::DriveFrame::drive_frame(timestamp, prepare,
   pipeline)`. `FrameRequest::target_present` and the presentation reports stay on the request and
   in `HostInput::Frame`, so a recording keeps what the platform said; no step of the frame reads
   them, and `FrameClock` keeps no refresh grid of its own.
4. **The lifecycle alone enables frames.** `frames_enabled` is written only by
   `handle_app_lifecycle_state_change`, which requests a frame on the disabled→enabled edge; there
   is no public setter. `ensure_visual_update` honours it, `request_frame` does not.

### Consequences

- Good, because the frame path runs only work it has, and every phase a diagnostic names is one a
  frame passes through.
- Good, because an application author has one form for each need, and animation cannot tick off a
  second clock.
- Good, because a host no longer computes an idle deadline per frame.
- Bad, because a port of Flutter code that called `scheduleFrameCallback` or `scheduleTask` is
  rewritten against a `Vsync` controller or an async task.
- Neutral, because `target_present` and the presentation reports have no consumer in the frame
  path until frame pacing reads them; they are recorded.

### Confirmation

- The compiler: the removed methods and types do not exist, and `api/flui.txt` lists none of them.
- `callbacks_share_owner_state_and_allow_registration_reentry` and
  `a_post_frame_callback_runs_once_after_the_pipeline` (flui-scheduler) pin the order: tasks, the
  pipeline, then post-frame callbacks, and a callback registered from a post-frame callback waits
  for the next frame.
- `async_future_poll_panic_closes_the_frame` pins the abort of a frame whose task panics before the
  pipeline; `a_panicking_pipeline_aborts_the_frame_and_runs_no_post_frame_callbacks` the abort from
  the pipeline.
- `lifecycle_reenable_edge_schedules_exactly_one_frame` pins decision 4.
- `callbacks_run_with_no_scheduler_lock_held` destructures the scheduler's state with no trailing
  `..`, so a lane added back fails to compile there until it is classified.

## Pros and cons of the options

### 1. Delete the lanes

- Good, because nothing in the frame path is there for work that never arrives.
- Bad, because the lanes were public, and a caller outside the workspace breaks.

### 2. Keep the lanes as public API

- Good, because Flutter code ports line by line.
- Bad, because each lane is a second form of a need the runtime already meets, and its failure
  paths (re-entry caps, run budgets, cancellation) stay untested by any production caller.

### 3. Deprecate and keep them working

- Good, because a caller gets a release to move.
- Bad, because pre-1.0 the lanes have no caller to move, and the frame path keeps their cost.

## More information

Flutter's lanes: `flutter/flutter@ab2912035e packages/flutter/lib/src/scheduler/binding.dart`
(`scheduleFrameCallback`, `addPersistentFrameCallback`, `addPostFrameCallback`, `scheduleTask`,
`SchedulerPhase`). Revisit decision 3 when frame pacing needs the platform's target present time:
it reads it from the request, under its own ADR.
