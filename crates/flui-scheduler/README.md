# FLUI Scheduler

Frame scheduling, task prioritization, and animation coordination for FLUI.

## Features

- **Frame Scheduling** - VSync coordination and frame lifecycle management
- **Priority-based Task Queue** - Execute tasks in priority order (UserInput > Animation > Build > Idle)
- **Animation Tickers** - Frame-perfect animation timing with typestate safety
- **Frame Budget Management** - Enforce time limits to maintain target FPS
- **VSync Integration** - Coordinate with display refresh to avoid tearing
- **Type-Safe Durations** - Newtype wrappers prevent unit confusion
- **Type-Safe IDs** - PhantomData markers prevent ID type mixing
- **Optional Serde Support** - Serialization for all data types

## Architecture

```text
Application
    ↓
Scheduler (orchestrates frames)
    ├─ FrameScheduler (vsync coordination)
    ├─ TaskQueue (priority-based execution)
    ├─ TickerProvider (animation tickers)
    └─ FrameBudget (time management)

Frame Timeline:
VSync → BeginFrame → Tasks (Build/Layout/Paint) → EndFrame → Present
```

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
use flui_scheduler::{Scheduler, Priority};

let scheduler = Scheduler::new();

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
use flui_scheduler::Ticker;

let mut ticker = Ticker::new();

ticker.start(|elapsed| {
    let progress = (elapsed % 2.0) / 2.0; // 2-second loop
    println!("Animation progress: {:.2}", progress);
});

// In your render loop
ticker.tick();
```

### Frame Budget Management

```rust
use flui_scheduler::{FrameBudget, BudgetPolicy};

let mut budget = FrameBudget::new(60); // 16.67ms target
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
         build_stats.duration_ms, build_stats.budget_percent);
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

### VSync Coordination

```rust
use flui_scheduler::{VsyncScheduler, VsyncMode};

let vsync = VsyncScheduler::new(60); // 60Hz display
vsync.set_mode(VsyncMode::On);

vsync.set_callback(|instant| {
    println!("VSync signal at {:?}", instant);
    // Begin frame rendering
});

// Wait for next vsync (blocking)
let vsync_time = vsync.wait_for_vsync();
```

## Advanced Type System Features

This crate leverages Rust's advanced type system for zero-cost safety guarantees.

### Typestate Pattern for Tickers

Compile-time state machine prevents invalid operations:

```rust
use flui_scheduler::typestate::{TypestateTicker, Idle, Active};

// Create idle ticker
let ticker: TypestateTicker<Idle> = TypestateTicker::new();

// Start transitions to Active state
let ticker: TypestateTicker<Active> = ticker.start(|elapsed| {
    println!("Elapsed: {:.2}s", elapsed);
});

// tick() only available in Active state - compile error if called on Idle!
ticker.tick();

// Stop transitions back to Idle
let ticker: TypestateTicker<Idle> = ticker.stop();
```

### Type-Safe Duration Wrappers

Newtype pattern prevents unit mixing:

```rust
use flui_scheduler::duration::{Milliseconds, Seconds, FrameDuration};

let elapsed = Milliseconds::new(10.0);     // 10ms
let timeout = Seconds::new(1.5);           // 1.5s
let budget = FrameDuration::from_fps(60);  // ~16.67ms

// Type-safe comparisons
assert!(!budget.is_over_budget(elapsed));

// Conversions are explicit
let as_seconds: Seconds = elapsed.as_seconds();
```

### Type-Safe IDs

PhantomData markers prevent ID type confusion at compile time:

```rust
use flui_scheduler::{FrameId, TaskId, TickerId};

let frame_id = FrameId::new();
let task_id = TaskId::new();

// These are different types - can't be accidentally mixed!
// frame_id == task_id  // Compile error!
```

### Typed Tasks with Compile-Time Priority

```rust
use flui_scheduler::task::TypedTask;
use flui_scheduler::traits::{UserInputPriority, IdlePriority};

// Type encodes the priority
let input_task = TypedTask::<UserInputPriority>::new(|| {
    println!("High priority!");
});

// Function that only accepts user input tasks
fn process_urgent<F>(task: TypedTask<UserInputPriority>) {
    task.execute();
}

process_urgent(input_task); // OK

// let idle_task = TypedTask::<IdlePriority>::new(|| {});
// process_urgent(idle_task); // Compile error! Wrong priority type
```

### Builder Pattern

```rust
use flui_scheduler::{FrameBudgetBuilder, SchedulerBuilder};
use flui_scheduler::duration::FrameDuration;

// Fluent builder for FrameBudget
let budget = FrameBudgetBuilder::new()
    .with_target_fps(120)
    .build();

// Builder for Scheduler
let scheduler = SchedulerBuilder::new()
    .with_target_fps(60)
    .build();
```

## Preludes

Two prelude modules for convenient imports:

```rust
// Basic types
use flui_scheduler::prelude::*;

// Advanced types (typestate, typed IDs, etc.)
use flui_scheduler::prelude_advanced::*;
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

### In flui_core Pipeline

```rust
use flui_scheduler::{Scheduler, FramePhase};

pub struct PipelineOwner {
    scheduler: Scheduler,
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

```rust
use flui_scheduler::{Scheduler, Priority};

let scheduler = Scheduler::new();

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

### Frame Budget Enforcement

| Target FPS | Frame Budget |
|------------|--------------|
| 60 FPS | 16.67ms |
| 120 FPS | 8.33ms |
| 144 FPS | 6.94ms |

When over budget, low-priority work is automatically skipped.

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
