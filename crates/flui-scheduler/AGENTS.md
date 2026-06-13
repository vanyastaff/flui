# AGENTS.md — flui-scheduler

Frame scheduling, task prioritization, and animation coordination.

## What lives here

- `Scheduler` — orchestrates frames (vsync → begin → tasks → end → present)
- `FrameScheduler` — vsync coordination
- `TaskQueue` — priority-based execution (UserInput > Animation > Build > Idle)
- `Ticker` — drives animations with frame-perfect timing
- `FrameBudget` — enforces frame time limits (16.67ms for 60fps)
- Duration wrappers: `FrameDuration`, `Milliseconds`

## Key constraints

- Uses `web-time` (maintained replacement for `instant` crate) for cross-platform time
- Uses `dashmap` for lock-free concurrent collections
- Uses `event-listener` for async completion callbacks
- `serde` feature for serialization support
