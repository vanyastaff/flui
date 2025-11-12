# flui-scheduler Integration Guide

This guide explains how to integrate `flui-scheduler` into your FLUI application for optimal frame scheduling and animation coordination.

## Overview

`flui-scheduler` provides:
- **Frame scheduling** with VSync coordination
- **Priority-based task execution** (UserInput > Animation > Build > Idle)
- **Frame budget management** to maintain target FPS
- **Animation tickers** for frame-perfect timing
- **Thread-safe** Arc/Mutex based design

## Integration with flui_animation

### ✅ Current Status: Clean Integration Complete

**As of v0.1.0**, `flui_animation` has been fully migrated to use `flui-scheduler` types directly. The adapter pattern has been removed in favor of a clean, direct integration.

### Breaking Changes (v0.1.0)

**AnimationController API Change:**

```rust
// ❌ OLD API (deprecated)
use flui_core::foundation::{Ticker, TickerProvider, SimpleTickerProvider};

let ticker_provider = Arc::new(SimpleTickerProvider);
let controller = AnimationController::new(
    Duration::from_millis(300),
    ticker_provider,  // Arc<dyn TickerProvider>
);

// ✅ NEW API (v0.1.0+)
use flui_scheduler::Scheduler;

let scheduler = Arc::new(Scheduler::new());
let controller = AnimationController::new(
    Duration::from_millis(300),
    scheduler,  // Arc<Scheduler>
);
```

**Type Re-exports:**

All scheduler types are re-exported from `flui_animation` for convenience:

```rust
// Re-exported from flui_animation
use flui_animation::{
    Scheduler, Ticker, TickerProvider, TickerState,
    Priority, TaskQueue, FrameBudget, FramePhase,
};
```

### Migration Guide for Users

If you're using `flui_animation` in your application, here's how to migrate:

#### Step 1: Update Your Code

Replace `SimpleTickerProvider` with `Scheduler`:

```rust
// ❌ OLD CODE
use flui_core::foundation::SimpleTickerProvider;

let ticker_provider = Arc::new(SimpleTickerProvider);
let controller = AnimationController::new(
    Duration::from_millis(300),
    ticker_provider,
);

// ✅ NEW CODE (Option 1: Use flui_scheduler directly)
use flui_scheduler::Scheduler;

let scheduler = Arc::new(Scheduler::new());
let controller = AnimationController::new(
    Duration::from_millis(300),
    scheduler,
);

// ✅ NEW CODE (Option 2: Use re-exports from flui_animation)
use flui_animation::Scheduler;

let scheduler = Arc::new(Scheduler::new());
let controller = AnimationController::new(
    Duration::from_millis(300),
    scheduler,
);
```

#### Step 2: Update Test Code

All test modules need the same change:

```rust
// ❌ OLD TEST CODE
use flui_core::foundation::SimpleTickerProvider;

#[test]
fn test_animation() {
    let ticker_provider = Arc::new(SimpleTickerProvider);
    let controller = AnimationController::new(
        Duration::from_millis(100),
        ticker_provider,
    );
}

// ✅ NEW TEST CODE
use flui_scheduler::Scheduler;

#[test]
fn test_animation() {
    let scheduler = Arc::new(Scheduler::new());
    let controller = AnimationController::new(
        Duration::from_millis(100),
        scheduler,
    );
}
```

#### Step 3: Optional - Customize Scheduler

The new `Scheduler` provides more control:

```rust
// Create scheduler with custom target FPS
let scheduler = Arc::new(Scheduler::with_target_fps(120));

// Or use default (60 FPS)
let scheduler = Arc::new(Scheduler::new());
```

#### Step 4: Update Animation Loop

```rust
use flui_scheduler::{Scheduler, Priority};

let scheduler = Arc::new(Scheduler::with_target_fps(60));
let controller = AnimationController::new(
    Duration::from_millis(300),
    Arc::clone(&scheduler),
);

// Frame loop
loop {
    // Begin frame
    let frame_id = scheduler.begin_frame();

    // Execute animation tasks (high priority)
    scheduler.task_queue().execute_until(Priority::Animation);

    // Tick animations
    controller.tick();

    // Execute other tasks
    if !scheduler.is_over_budget() {
        scheduler.task_queue().execute_until(Priority::Build);
    }

    // End frame
    scheduler.end_frame();
}
```

**Pros:**
- ✅ Clean architecture - single source of truth for timing
- ✅ No adapter layer
- ✅ Direct access to scheduler features (priority, budget)
- ✅ Simpler mental model

**Cons:**
- ⚠️ Requires refactoring `AnimationController` and related types
- ⚠️ Breaking change for existing code

## Integration with flui_app

### Bindings Integration

Replace `SchedulerBinding` in `flui_app` with scheduler-based implementation:

```rust
use flui_scheduler::{Scheduler, Priority, SchedulerBinding};

pub struct AppBinding {
    scheduler: Arc<Scheduler>,
    // ... other fields
}

impl SchedulerBinding for AppBinding {
    fn scheduler(&self) -> &Scheduler {
        &self.scheduler
    }

    fn schedule_frame(&self) {
        self.scheduler.schedule_frame(Box::new(|timing| {
            // Frame callback
        }));
    }

    fn add_task(&self, priority: Priority, callback: impl FnOnce() + Send + 'static) {
        self.scheduler.add_task(priority, callback);
    }
}
```

### Event Loop Integration

```rust
use flui_scheduler::{Scheduler, Priority};
use winit::event_loop::EventLoop;

let scheduler = Arc::new(Scheduler::with_target_fps(60));

event_loop.run(move |event, _, control_flow| {
    match event {
        Event::MainEventsCleared => {
            if scheduler.is_frame_scheduled() {
                scheduler.execute_frame();
                window.request_redraw();
            }
        }
        Event::WindowEvent { event, .. } => {
            // Handle input with highest priority
            scheduler.add_task(Priority::UserInput, || {
                handle_input(event);
            });
            scheduler.schedule_frame(Box::new(|_| {}));
        }
        _ => {}
    }
});
```

## Integration with flui_core Pipeline

### PipelineOwner Integration

```rust
use flui_scheduler::{Scheduler, Priority, FramePhase};

impl PipelineOwner {
    pub fn build_frame(&mut self, scheduler: &Scheduler) {
        // Build phase
        scheduler.set_phase(FramePhase::Build);
        let build_start = Instant::now();
        self.flush_build();
        scheduler.budget().lock().record_build_time(
            build_start.elapsed().as_secs_f64() * 1000.0
        );

        // Layout phase
        scheduler.set_phase(FramePhase::Layout);
        let layout_start = Instant::now();
        self.flush_layout();
        scheduler.budget().lock().record_layout_time(
            layout_start.elapsed().as_secs_f64() * 1000.0
        );

        // Paint phase
        scheduler.set_phase(FramePhase::Paint);
        let paint_start = Instant::now();
        self.flush_paint();
        scheduler.budget().lock().record_paint_time(
            paint_start.elapsed().as_secs_f64() * 1000.0
        );
    }
}
```

## Best Practices

### 1. Frame Budget Management

Monitor and respect frame budgets:

```rust
if scheduler.is_over_budget() {
    // Skip non-critical work
    tracing::warn!("Frame over budget, skipping idle tasks");
} else if scheduler.remaining_budget_ms() > 5.0 {
    // Execute idle tasks with 5ms buffer
    scheduler.task_queue().execute_until(Priority::Idle);
}
```

### 2. Priority Guidelines

- **UserInput**: Mouse clicks, keyboard input, touch events
- **Animation**: Ticker callbacks, implicit animations, transitions
- **Build**: Widget tree rebuilds, state changes
- **Idle**: GC, telemetry, preloading, non-critical work

### 3. VSync Coordination

```rust
use flui_scheduler::{VsyncScheduler, VsyncMode};

let vsync = VsyncScheduler::new(60); // 60Hz display
vsync.set_mode(VsyncMode::On);

vsync.set_callback(|instant| {
    scheduler.begin_frame();
});
```

### 4. Frame Statistics

```rust
let budget = scheduler.budget().lock();

println!("Frame stats:");
println!("  Build: {:.2}ms ({:.1}%)",
    budget.build_stats().duration_ms,
    budget.build_stats().budget_percent);
println!("  Layout: {:.2}ms ({:.1}%)",
    budget.layout_stats().duration_ms,
    budget.layout_stats().budget_percent);
println!("  Avg FPS: {:.1}", budget.avg_fps());

if budget.is_janky() {
    tracing::warn!("Janky frame detected!");
}
```

## Migration Checklist

For full scheduler integration:

- [ ] Update `Cargo.toml` dependencies
- [ ] Replace `flui_core::Ticker` imports with `flui_scheduler::Ticker`
- [ ] Update `AnimationController` to accept `Arc<Scheduler>`
- [ ] Update all `TickerProvider` implementations
- [ ] Integrate with event loop (winit/platform)
- [ ] Add priority-based task scheduling
- [ ] Implement frame budget tracking
- [ ] Add VSync coordination (optional)
- [ ] Update examples and tests
- [ ] Update documentation

## Examples

See:
- `crates/flui_animation/examples/scheduler_animation.rs` - Animation integration
- `crates/flui-scheduler/examples/basic_scheduler.rs` - Basic scheduling
- `crates/flui-scheduler/README.md` - Full API documentation

## Performance Notes

- Scheduler uses `parking_lot::Mutex` for 2-3x better performance than std
- BinaryHeap provides O(log n) priority queue operations
- Frame budgets help maintain consistent FPS
- Priority-based execution ensures smooth animations

## Thread Safety

All scheduler components are thread-safe:
- `Scheduler` - Fully thread-safe with Arc/Mutex
- `TaskQueue` - Concurrent task submission
- `Ticker` - Thread-safe state management
- `FrameBudget` - Thread-safe statistics
