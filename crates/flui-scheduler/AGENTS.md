# AGENTS.md — flui-scheduler

Frame scheduling, task prioritization, and animation coordination.

## What lives here

- `UpdateScheduler` — orchestrates *logical* time only: the phase machine
  (begin → tasks → end), callback queues, and the priority task queue. It
  makes no refresh-rate, display, or surface assumption of its own —
  `drive_frame(vsync_time, deadline, pipeline)` takes both timestamps from
  its caller. `deadline` bounds `Priority::Idle` work alone; `Animation` and
  `Build` always run to completion (physical pacing / vsync coordination is
  a presentation-owned concern that lives outside this crate).
- `TaskQueue` — priority-based execution (UserInput > Animation > Build > Idle)
- `Ticker` — drives animations with frame-perfect timing
- `LocalPostFrameLane` — owner-affine non-`Send` callback storage; runtime-internal, non-prelude
- `FrameBudget` — per-phase timing statistics (jank, over-budget) against a
  caller-chosen target framerate; stats only, does not gate anything
- Duration wrappers: `FrameDuration`, `Milliseconds`

## Key constraints

- Uses `web-time` (maintained replacement for `instant` crate) for cross-platform time
- Uses `dashmap` for lock-free concurrent collections
- Uses `event-listener` for async completion callbacks
- `serde` feature for serialization support
- Shared and local post-frame registration is linearized by one gate and one ID
  sequence. Never move local callbacks into `Arc`/`Mutex`; the lane is `Rc`-owned
  and only active inside its binding/realm owner scope.
