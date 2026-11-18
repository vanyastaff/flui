# FLUI Scheduler

Frame scheduling, task prioritization, and animation coordination for FLUI.

Standalone `rust` blocks in this document are compiled as doctests. Blocks
marked `rust,ignore` are integration sketches that depend on an external
render pipeline or event loop.

## Features

- **Frame Scheduling** - Complete owner turns and frame lifecycle management
- **Priority-based Task Queue** - Execute tasks in priority order (UserInput > Animation > Build > Idle)
- **Frame Telemetry** - Per-phase durations of every frame, and Idle-priority work bounded by the frame's deadline
- **Type-Safe IDs** - PhantomData markers prevent ID type mixing
- **Optional Serde Support** - Serialization for priorities and the frame and lifecycle enums

## Architecture

```text
Application
    ↓
UpdateScheduler (frame phases and owner turns)
    ├─ TaskQueue (priority-based execution)
    └─ FrameClock (per-phase frame telemetry)

Frame Timeline:
BeginFrame → Tasks (Build/Layout/Paint) → EndFrame → Present
```

`UpdateScheduler` makes no refresh-rate, display, or surface assumption of
its own — a caller supplies the frame's vsync timestamp and an Idle-slice
deadline to [`drive_frame`](#basic-frame-scheduling). Physical pacing
(vsync coordination) is a presentation-owned concern that lives outside
this crate.

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

### Migrating performance-mode requests

The scheduler no longer exposes `PerformanceMode`, `PerformanceModeRequestHandle`,
`request_performance_mode`, `current_performance_mode`, `set_performance_mode`,
`performance_mode_request_count`, or `debug_assert_no_pending_performance_mode_requests`.
Remove their imports, requests, handle disposal and mode bookkeeping from callers.
Persisted application configuration using the removed serde enum needs its own
application-owned representation if it still serves an application policy.

These requests counted handles and stored a label without changing scheduling,
polling, latency or power behavior. There is no replacement scheduling hint, and
removing the API does not claim a performance improvement. Physical presentation
pacing remains the presentation host's responsibility; task priority continues to
control logical execution order.

### Basic Frame Scheduling

```rust
use flui_scheduler::__runtime::DriveFrame as _;
use flui_scheduler::{OwnerFrame, Priority, UpdateScheduler};

let scheduler = UpdateScheduler::new();
// The owner thread's frame state (owner-local post-frame callbacks and
// async tasks); complete execution belongs to this owner.
let owner = OwnerFrame::try_new(&scheduler).expect("the scheduler has no live owner frame");

// Schedule a one-time frame callback (animation tick)
scheduler.schedule_frame_callback(Box::new(|vsync_time| {
    println!("Frame started at {:?}", vsync_time);
}));

// Add tasks with different priorities
scheduler.add_task(Priority::Animation, || {
    // Update animations
});

scheduler.add_task(Priority::Build, || {
    // Rebuild widgets
});

// Execute frame (called by event loop)
let now = flui_scheduler::Instant::now();
owner.drive_frame(now, flui_scheduler::IdleDeadline::far_future(now), || {}, || {})
    .expect("the owner accepts the frame");
```

### Migrating execution

`UpdateScheduler` registers work and requests delivery. Its owner executes
`OwnerFrame::pump_background(prepare)`, or, from a composition root, the frame drive
`flui_scheduler::__runtime::DriveFrame::drive_frame(timestamp, deadline, prepare, pipeline)`
(`#[doc(hidden)]`, sealed, no semver promise; the method is not inherent on `OwnerFrame`). Both return typed admission errors;
neither accepts another scheduler's owner. Preparation runs after admission,
before frame phases or ready-task polling.

Replace `execute_frame` and synchronous warm-up calls with a complete owner
frame, using a no-op pipeline only when no render pipeline is needed. Observe
frame identity and timing from `current_frame()` inside the pipeline or from
`end_of_frame()` afterward. Replace manual begin/draw/end/abort sequences and
`finish_async_pump` plus `poll_ready` with the corresponding complete operation.
Failures close started frame bookkeeping before propagation; callers cannot
manually abort an enclosing operation.

The unused idle-callback lane (`schedule_idle_callback`, `execute_idle_callbacks`,
`has_idle_callbacks`) was removed. `Priority::Idle` tasks remain supported and
run within the frame's supplied deadline. Direct access to the scheduler's
`TaskQueue` and manual timing-phase mutation were removed; standalone `TaskQueue`
is still available as a separate queue, without authority over an owner's frame.

### Priority-based Task Queue

```rust
use flui_scheduler::{TaskQueue, Priority};

let queue = TaskQueue::new();

// Add tasks in any order
queue.add(Priority::Idle, || println!("Background work"));
queue.add(Priority::UserInput, || println!("Handle mouse click"));
queue.add(Priority::Animation, || println!("Update animation"));

// Execute in priority order: UserInput > Animation > Idle
queue.execute_all();
```

## Safety Features

This crate uses small, explicit Rust types for correctness at API boundaries.

### Type-Safe IDs

Callback and task ids are distinct opaque types; only a mint issues one, and
none is built from a number:

```rust
use flui_scheduler::{Priority, Task};

let first = Task::new(Priority::Build, || {});
let second = Task::new(Priority::Build, || {});
// Ids of one domain are never issued twice; a `TaskId` and a
// `CallbackId` are different types and can't be mixed.
assert_ne!(first.id(), second.id());
```

### Builder Pattern

```rust
use flui_foundation::ManualClock;
use flui_scheduler::{ClockSource, SchedulerBuilder};

// A scheduler that reads time from a test's manual clock, so the Idle
// deadline is measured on the frames' own timeline.
let scheduler = SchedulerBuilder::new()
    .clock(ClockSource::Manual(ManualClock::new()))
    .build();
assert_eq!(scheduler.frame_count(), 0);
```

## Priority Levels

Tasks execute in strict priority order:

| Priority | Use Case | Examples |
|----------|----------|----------|
| **UserInput** | Immediate response | Mouse clicks, keyboard, touch |
| **Animation** | Time-sensitive UI work | Animation samples, transitions |
| **Build** | Widget rebuilds | State changes, layout |
| **Idle** | Background work | Deferred cleanup, telemetry, preloading |

## Integration with FLUI

### In flui-rendering's `PipelineOwner`

`flui-rendering::pipeline::owner::PipelineOwner` is the real consumer —
there is no `flui_core` crate in this workspace. The sketch below is
illustrative shape, not the real struct (the actual `PipelineOwner` is
typestate-generic over its pipeline phase and carries a `DirtyTracker`,
not a bare `UpdateScheduler` field — see `flui-rendering`'s own docs for the
real definition):

```rust,ignore
use flui_scheduler::{UpdateScheduler, FramePhase};

struct PipelineOwner {
    scheduler: UpdateScheduler,
}

impl PipelineOwner {
    pub fn build_frame(&mut self) {
        // Execute frame phases in order
        self.flush_build();
        self.flush_layout();
        self.flush_paint();
    }
}
```

### In Event Loop

```rust,ignore
use flui_scheduler::{OwnerFrame, Priority, UpdateScheduler};

let scheduler = UpdateScheduler::new();
let owner = OwnerFrame::try_new(&scheduler).expect("the scheduler has no live owner frame");

// In your event loop
match event {
    Event::MainEventsCleared => {
        if scheduler.is_frame_scheduled() {
            let now = flui_scheduler::Instant::now();
            owner.drive_frame(now, flui_scheduler::IdleDeadline::far_future(now), || {}, || {})
                .expect("the owner accepts the frame");
            window.request_redraw();
        }
    }
    Event::UserInput(input) => {
        scheduler.add_task(Priority::UserInput, move || {
            handle_input(input);
        });
        scheduler.request_frame();
    }
    _ => {}
}
```

## Performance

### Frame telemetry

`UpdateScheduler` makes no frame-rate assumption and does not skip work by
how long a frame took. The runtime labels frames with the refresh period of
the display they are produced for (`UpdateScheduler::set_display_period`); the
`frame` span carries it as `period_us`, and a profiler judges a frame late
against it. Per-phase durations of each frame come from the
`FrameSnapshot`s `FrameClock::frames_since` returns. The only thing that
ever defers work is the frame drive's own
`deadline` parameter, and it bounds `Priority::Idle` tasks alone;
`Priority::Animation` and `Priority::Build` always run to completion.

## Feature Flags

| Feature | Description |
|---------|-------------|
| `serde` | Enable serialization for priorities and the frame and lifecycle enums |

## Platform Support

Hosts provide display pacing and sampled timestamps. `web-time` supplies a
portable monotonic clock; it does not coordinate display refresh or prevent tearing.

`UpdateScheduler`, its callback queues and `OwnerFrame` belong to one UI owner.
Callbacks accept owner-local captures. Workers request frames through
`FrameWaker`; wake infrastructure does not retain UI callback storage (ADR-0175).

## Testing

```bash
# Run all tests
cargo test -p flui-scheduler

# Run with serde feature
cargo test -p flui-scheduler --features serde
```

## License

MIT OR Apache-2.0
