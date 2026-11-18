# FLUI Scheduler

Owner-local frame execution, frame demand, post-frame callbacks, lifecycle state
and async tasks for FLUI.

Standalone `rust` blocks in this document are compiled as doctests. Blocks
marked `rust,ignore` are integration sketches that depend on an external
render pipeline or event loop.

## Features

- **Frame execution** - one complete owner turn per frame: the frame's async
  tasks, the driver's pipeline, then post-frame callbacks and `end_of_frame`
  waiters
- **Frame demand** - `request_frame` from the owner, `FrameWaker` from any
  thread, one platform wake per false-to-true edge
- **Frame telemetry** - per-phase durations of every frame (`FrameClock`,
  `FrameSnapshot`)
- **Optional Serde support** - serialization for the frame and lifecycle enums

## Architecture

```text
Application
    ↓
OwnerFrame (executes complete frames and background turns)
    └─ UpdateScheduler (admits work and observes logical frame state)

Frame timeline:
Idle → Frame (async tasks, pipeline) → PostFrame (post-frame callbacks) → Idle
```

`UpdateScheduler` makes no refresh-rate, display or surface assumption of its
own: a caller supplies the frame's vsync timestamp to the frame drive. Physical
pacing is a presentation-owned concern that lives outside this crate.

## Installation

```toml
[dependencies]
flui-scheduler = { git = "https://github.com/vanyastaff/flui" }

# With serialization support
flui-scheduler = { git = "https://github.com/vanyastaff/flui", features = ["serde"] }
```

This crate isn't published to crates.io; depend on it via git or path — see
the [flui facade's README](../../README.md).

## Usage

### Driving a frame

```rust
use flui_scheduler::__runtime::DriveFrame as _;
use flui_scheduler::{OwnerFrame, UpdateScheduler};

let scheduler = UpdateScheduler::new();
// The owner thread's frame state (owner-local post-frame callbacks and
// async tasks); complete execution belongs to this owner.
let owner = OwnerFrame::try_new(&scheduler).expect("the scheduler has no live owner frame");

scheduler.add_post_frame_callback(Box::new(|timing| {
    println!("frame {} committed", timing.id);
}));

// Execute a frame (called by the event loop when a frame is scheduled).
let now = flui_scheduler::Instant::now();
owner
    .drive_frame(now, || {}, || { /* build, layout, paint */ })
    .expect("the owner accepts the frame");
```

`OwnerFrame::pump_background(prepare)` runs a turn without a frame: the ready
async tasks only. The frame drive
`flui_scheduler::__runtime::DriveFrame::drive_frame(timestamp, prepare, pipeline)`
is `#[doc(hidden)]`, sealed and carries no semver promise; a composition root
calls it. Both return typed admission errors, and neither accepts another
scheduler's owner. Preparation runs after admission, before the frame's tasks
are polled. A failure closes the frame's bookkeeping before it propagates.

### Builder

```rust
use flui_foundation::ManualClock;
use flui_scheduler::{ClockSource, SchedulerBuilder};

// A scheduler that reads time from a test's manual clock, so timers and the
// frame telemetry are measured on the frames' own timeline.
let scheduler = SchedulerBuilder::new()
    .clock(ClockSource::Manual(ManualClock::new()))
    .build();
assert_eq!(scheduler.frame_count(), 0);
```

### In an event loop

```rust,ignore
use flui_scheduler::__runtime::DriveFrame as _;
use flui_scheduler::{OwnerFrame, UpdateScheduler};

let scheduler = UpdateScheduler::new();
let owner = OwnerFrame::try_new(&scheduler).expect("the scheduler has no live owner frame");

match event {
    Event::MainEventsCleared => {
        if scheduler.is_frame_scheduled() {
            let now = flui_scheduler::Instant::now();
            owner
                .drive_frame(now, || {}, || pipeline.run())
                .expect("the owner accepts the frame");
            window.request_redraw();
        }
    }
    Event::UserInput(input) => {
        handle_input(input);
        scheduler.request_frame();
    }
    _ => {}
}
```

## Frame telemetry

`UpdateScheduler` makes no frame-rate assumption and does not skip work by how
long a frame took. The runtime labels frames with the refresh period of the
display they are produced for (`UpdateScheduler::set_display_period`); the
`frame` span carries it as `period_us`, and a profiler judges a frame late
against it. Per-phase durations of each frame come from the `FrameSnapshot`s
`FrameClock::frames_since` returns.

## Feature Flags

| Feature | Description |
|---------|-------------|
| `serde` | Enable serialization for the frame and lifecycle enums |

## Platform Support

Hosts provide display pacing and sampled timestamps. `web-time` supplies a
portable monotonic clock; it does not coordinate display refresh or prevent tearing.

`UpdateScheduler`, its callback storage and `OwnerFrame` belong to one UI owner.
Callbacks accept owner-local captures. Workers request frames through
`FrameWaker`; wake infrastructure does not retain UI callback storage (ADR-0175).

## Testing

```bash
cargo test -p flui-scheduler
cargo test -p flui-scheduler --features serde
```

## License

MIT OR Apache-2.0
