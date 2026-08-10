# FLUI Scheduler

Frame scheduling, task prioritization, and animation coordination for FLUI.

Standalone `rust` blocks in this document are compiled as doctests. Blocks
marked `rust,ignore` are integration sketches that depend on an external
render pipeline or event loop.

## Features

- **Frame Scheduling** - VSync coordination and frame lifecycle management
- **Priority-based Task Queue** - Execute tasks in priority order (UserInput > Animation > Build > Idle)
- **Animation Tickers** - Frame-perfect animation timing with explicit lifecycle futures
- **Frame Budget Management** - Enforce time limits to maintain target FPS
- **VSync Integration** - Coordinate with display refresh to avoid tearing
- **Type-Safe Durations** - Newtype wrappers prevent unit confusion
- **Type-Safe IDs** - PhantomData markers prevent ID type mixing
- **Optional Serde Support** - Serialization for all data types

## Architecture

```text
Application
    ↓
UpdateScheduler (orchestrates frames — logical time only)
    ├─ TaskQueue (priority-based execution)
    ├─ TickerProvider (animation tickers)
    └─ FrameBudget (phase-duration stats)

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
flui-scheduler = "0.1"

# With serialization support
flui-scheduler = { version = "0.1", features = ["serde"] }
```

## Usage

### Basic Frame Scheduling

```rust
use flui_scheduler::{UpdateScheduler, Priority};

let scheduler = UpdateScheduler::new();

// Schedule a frame callback
scheduler.schedule_frame(Box::new(|timing| {
    println!("Frame started");
}));

// Add tasks with different priorities
scheduler.add_task(Priority::Animation, || {
    // Update animations
});

scheduler.add_task(Priority::Build, || {
    // Rebuild widgets
});

// Execute frame (called by event loop)
scheduler.execute_frame();
```

### Animation Tickers

```rust
use std::sync::Arc;

use flui_scheduler::{UpdateScheduler, Ticker};

let scheduler = Arc::new(UpdateScheduler::new());
let mut ticker = Ticker::new_with_scheduler(&scheduler);

let future = ticker.start(|elapsed| {
    let progress = (elapsed % 2.0) / 2.0; // 2-second loop
    println!("Animation progress: {:.2}", progress);
});

// In your frame loop, scheduler transient callbacks drive the ticker.
scheduler.execute_frame();

// Stop completes the future normally. dispose()/drop cancels it.
ticker.stop();
assert!(future.is_complete());
```

### Frame Budget Management

```rust
use flui_scheduler::{FrameBudget, BudgetPolicy};

let mut budget = FrameBudget::new(60); // 60fps target duration
budget.set_policy(BudgetPolicy::SkipIdle);

budget.reset(); // Start new frame

// Record phase times
budget.record_build_time(5.0);
budget.record_layout_time(3.0);
budget.record_paint_time(4.0);

if budget.is_over_budget() {
    println!("Frame is over budget!");
}

// Get statistics
let build_stats = budget.build_stats();
println!("Build took {:.2}ms ({:.1}% of budget)", 
         build_stats.duration_ms(), build_stats.budget_percent);
```

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

### Ticker Lifecycle Futures

Ticker start methods return a `TickerFuture`. The future completes when the
ticker is stopped normally and is canceled when the ticker is disposed, dropped,
or reset.

```rust
use flui_scheduler::Ticker;

let mut ticker = Ticker::new();
let future = ticker.start(|_| {});

ticker.stop();
assert!(future.is_complete());
```

### Type-Safe Duration Wrappers

Newtype pattern prevents unit mixing:

```rust
use flui_scheduler::duration::{Milliseconds, Seconds, FrameDuration};

let elapsed = Milliseconds::new(10.0);        // 10ms
let timeout = Seconds::new(1.5);              // 1.5s
let budget = FrameDuration::try_from_fps(60)  // ~60fps
    .expect("fps > 0");

// Type-safe comparisons
assert!(!budget.is_over_budget(elapsed));

// Conversions are explicit
let as_seconds: Seconds = elapsed.to_seconds();
```

### Type-Safe IDs

Foundation IDs prevent ID type confusion at compile time:

```rust
use flui_scheduler::id::{IdGenerator, markers};

let frame_gen = IdGenerator::<markers::Frame>::new();
let task_gen = IdGenerator::<markers::Task>::new();

let frame_id = frame_gen.next();
let task_id = task_gen.next();

// These are different types - can't be accidentally mixed!
// frame_id == task_id  // Compile error!
```

### Builder Pattern

```rust
use flui_scheduler::{FrameBudgetBuilder, SchedulerBuilder};
use flui_scheduler::duration::FrameDuration;

// Fluent builder for FrameBudget
let budget = FrameBudgetBuilder::new()
    .target_fps(120)
    .build();

// Builder for UpdateScheduler. `target_fps` only labels the seeded
// `FrameBudget`'s stats (see `UpdateScheduler::new`'s doc) — it has no
// effect on frame gating, which `drive_frame`'s own `deadline` controls.
let scheduler = SchedulerBuilder::new()
    .target_fps(60)
    .build();
```

## Prelude

The prelude exports common scheduler types:

```rust
use flui_scheduler::prelude::*;
```

## Priority Levels

Tasks execute in strict priority order:

| Priority | Use Case | Examples |
|----------|----------|----------|
| **UserInput** | Immediate response | Mouse clicks, keyboard, touch |
| **Animation** | Smooth 60fps | Tickers, transitions |
| **Build** | Widget rebuilds | State changes, layout |
| **Idle** | Background work | GC, telemetry, preloading |

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
use flui_scheduler::{UpdateScheduler, Priority};

let scheduler = UpdateScheduler::new();

// In your event loop
match event {
    Event::MainEventsCleared => {
        if scheduler.is_frame_scheduled() {
            scheduler.execute_frame();
            window.request_redraw();
        }
    }
    Event::UserInput(input) => {
        scheduler.add_task(Priority::UserInput, move || {
            handle_input(input);
        });
        scheduler.schedule_frame(Box::new(|_| {}));
    }
    _ => {}
}
```

## Performance

### Frame Budget Statistics

| Target FPS | Frame Duration |
|------------|-----------------|
| 60 FPS | ~16.7ms |
| 120 FPS | ~8.3ms |
| 144 FPS | ~6.9ms |

`FrameBudget` reports timing statistics (jank, phase durations, over-budget)
against whichever target the caller chose — `UpdateScheduler` itself makes
no frame-rate assumption and does not act on these statistics to skip work.
The only thing that ever defers work is [`drive_frame`]'s own `deadline`
parameter, and it bounds `Priority::Idle` tasks alone; `Priority::Animation`
and `Priority::Build` always run to completion.

[`drive_frame`]: https://docs.rs/flui-scheduler/latest/flui_scheduler/scheduler/struct.UpdateScheduler.html#method.drive_frame

### Zero-Cost Abstractions

- Typestate pattern: No runtime overhead - states checked at compile time
- Newtype wrappers: Zero-cost - same as raw `f64`
- PhantomData markers: Zero-size - no memory overhead

## Feature Flags

| Feature | Description |
|---------|-------------|
| `serde` | Enable serialization for duration types, priorities, and statistics |

## Platform Support

| Platform | VSync Method |
|----------|--------------|
| Windows, macOS, Linux | Native vsync via `web-time` |
| WebAssembly | `performance.now()` |
| iOS/Android | Platform refresh rate |

All types are `Send + Sync` and safe for multi-threaded use.

## Testing

```bash
# Run all tests
cargo test -p flui-scheduler

# Run with serde feature
cargo test -p flui-scheduler --features serde
```

## License

MIT OR Apache-2.0
