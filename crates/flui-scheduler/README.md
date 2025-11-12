# FLUI Scheduler

Frame scheduling, task prioritization, and animation coordination for FLUI.

## Features

- **Frame Scheduling** - VSync coordination and frame lifecycle management
- **Priority-based Task Queue** - Execute tasks in priority order (UserInput > Animation > Build > Idle)
- **Animation Tickers** - Frame-perfect animation timing
- **Frame Budget Management** - Enforce time limits to maintain target FPS
- **VSync Integration** - Coordinate with display refresh to avoid tearing

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

## Usage

### Basic Frame Scheduling

```rust
use flui_scheduler::{Scheduler, Priority};

let mut scheduler = Scheduler::new();
scheduler.set_target_fps(60);

// Schedule a frame callback
scheduler.schedule_frame(Box::new(|timing| {
    println!("Frame {} started at {:?}", timing.id, timing.start_time);
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
    println!("Frame is over budget by {:.2}ms", -budget.remaining_ms());
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

## Priority Levels

Tasks are executed in strict priority order:

| Priority | Use Case | Examples |
|----------|----------|----------|
| **UserInput** | Immediate response to user actions | Mouse clicks, keyboard input, touch events |
| **Animation** | Smooth 60fps animations | Ticker callbacks, transitions, implicit animations |
| **Build** | Widget tree rebuilds | State changes, layout updates |
| **Idle** | Background work | Garbage collection, telemetry, preloading |

## Integration with FLUI

### In flui_core Pipeline

```rust
use flui_scheduler::{Scheduler, Priority};

pub struct PipelineOwner {
    scheduler: Scheduler,
    // ...
}

impl PipelineOwner {
    pub fn build_frame(&mut self) {
        self.scheduler.set_phase(FramePhase::Build);
        self.flush_build();
        
        self.scheduler.set_phase(FramePhase::Layout);
        self.flush_layout();
        
        self.scheduler.set_phase(FramePhase::Paint);
        self.flush_paint();
    }
}
```

### In Event Loop

```rust
use flui_scheduler::Scheduler;
use winit::event_loop::EventLoop;

let mut scheduler = Scheduler::new();

event_loop.run(move |event, _, control_flow| {
    match event {
        Event::MainEventsCleared => {
            // Execute scheduled frame
            if scheduler.is_frame_scheduled() {
                scheduler.execute_frame();
                window.request_redraw();
            }
        }
        Event::UserInput(input) => {
            // Handle input with highest priority
            scheduler.add_task(Priority::UserInput, move || {
                handle_input(input);
            });
            scheduler.schedule_frame(Box::new(|_| {}));
        }
        _ => {}
    }
});
```

## Performance

### Frame Budget Enforcement

The scheduler automatically manages frame budgets:

- **60 FPS** = 16.67ms per frame
- **120 FPS** = 8.33ms per frame
- **144 FPS** = 6.94ms per frame

When over budget, low-priority work is skipped:

```rust
scheduler.execute_frame(); // Automatically skips Idle tasks if over budget
```

### Priority-based Execution

Tasks execute in strict priority order, ensuring:

- User input is always processed immediately
- Animations remain smooth at 60fps
- Build phase can be interrupted if over budget
- Background work only runs when there's spare time

## Testing

Run tests:

```bash
cargo test -p flui_scheduler
```

Run with profiling:

```bash
cargo test -p flui_scheduler --features profiling
```

## Examples

See `examples/` directory for complete examples:

- `basic_scheduler.rs` - Basic frame scheduling
- `animation_ticker.rs` - Animation with tickers
- `budget_management.rs` - Frame budget tracking
- `priority_tasks.rs` - Task priority demonstration

## Platform Support

- **Native** (Windows, macOS, Linux) - Full support with native vsync
- **Web** (WebAssembly) - Uses `requestAnimationFrame` for vsync
- **iOS/Android** - Integrates with platform refresh rate

## License

MIT OR Apache-2.0