# FLUI Scheduler Integration Guide

This guide explains how to integrate `flui-scheduler` into the FLUI workspace.

## Step 1: Add to Workspace

Edit the root `Cargo.toml` to include the scheduler crate:

```toml
[workspace]
members = [
    "crates/flui_types",
    "crates/flui_painting",
    "crates/flui_core",
    "crates/flui_engine",
    "crates/flui_rendering",
    "crates/flui_widgets",
    "crates/flui_app",
    "crates/flui_assets",
    "crates/flui_devtools",
    "crates/flui_scheduler",  # <-- ADD THIS
]

[workspace.package]
version = "0.7.0"
edition = "2021"
rust-version = "1.75"
# ... rest of workspace config
```

## Step 2: Move Crate to Workspace

```bash
# From your FLUI project root
mv /path/to/flui-scheduler crates/flui_scheduler
```

## Step 3: Add Dependencies

### In `flui_core/Cargo.toml`

```toml
[dependencies]
# ... existing dependencies
flui_scheduler = { path = "../flui_scheduler", version = "0.7" }
```

### In `flui_app/Cargo.toml`

```toml
[dependencies]
# ... existing dependencies
flui_scheduler = { path = "../flui_scheduler", version = "0.7" }
```

## Step 4: Integrate with PipelineOwner

Update `crates/flui_core/src/pipeline/owner.rs`:

```rust
use flui_scheduler::{Scheduler, FramePhase, Priority};

pub struct PipelineOwner {
    // ... existing fields
    
    /// Frame scheduler
    scheduler: Scheduler,
}

impl PipelineOwner {
    pub fn new() -> Self {
        Self {
            // ... existing initialization
            scheduler: Scheduler::new(),
        }
    }

    /// Get scheduler reference
    pub fn scheduler(&self) -> &Scheduler {
        &self.scheduler
    }

    /// Get mutable scheduler reference
    pub fn scheduler_mut(&mut self) -> &mut Scheduler {
        &mut self.scheduler
    }

    /// Build a complete frame with scheduling
    pub fn build_frame(&mut self, constraints: BoxConstraints) -> Result<BoxedLayer> {
        // Begin frame
        let frame_id = self.scheduler.begin_frame();
        
        // Build phase
        self.scheduler.set_phase(FramePhase::Build);
        self.flush_build();
        
        // Layout phase
        self.scheduler.set_phase(FramePhase::Layout);
        let size = self.flush_layout(constraints)?;
        
        // Paint phase
        self.scheduler.set_phase(FramePhase::Paint);
        let layer = self.flush_paint()?;
        
        // End frame
        self.scheduler.end_frame();
        
        Ok(layer)
    }
}
```

## Step 5: Integrate with Event Loop

Update your application's event loop (typically in `flui_app`):

```rust
use flui_scheduler::{Scheduler, Priority};
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};

pub fn run_app(mut app: impl Application) {
    let event_loop = EventLoop::new();
    let mut scheduler = Scheduler::new();
    scheduler.set_target_fps(60);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            }

            Event::WindowEvent {
                event: WindowEvent::CursorMoved { position, .. },
                ..
            } => {
                // Handle input with highest priority
                scheduler.add_task(Priority::UserInput, move || {
                    app.handle_mouse_move(position);
                });
                scheduler.schedule_frame(Box::new(|_| {}));
            }

            Event::MainEventsCleared => {
                // Execute frame if scheduled
                if scheduler.is_frame_scheduled() {
                    scheduler.execute_frame();
                    window.request_redraw();
                }
            }

            Event::RedrawRequested(_) => {
                // Render
                app.render();
            }

            _ => {}
        }
    });
}
```

## Step 6: Add Animation Support

Create `crates/flui_core/src/animation/controller.rs`:

```rust
use flui_scheduler::{Ticker, TickerProvider};

pub struct AnimationController {
    ticker: Ticker,
    duration: f64,
    value: f64,
}

impl AnimationController {
    pub fn new(duration_secs: f64) -> Self {
        Self {
            ticker: Ticker::new(),
            duration: duration_secs,
            value: 0.0,
        }
    }

    pub fn forward(&mut self) {
        self.ticker.start(|elapsed| {
            self.value = (elapsed / self.duration).min(1.0);
        });
    }

    pub fn value(&self) -> f64 {
        self.value
    }

    pub fn tick(&self) {
        self.ticker.tick();
    }
}
```

## Step 7: Update Build Configuration

Ensure all crates build correctly:

```bash
# Build entire workspace
cargo build --workspace

# Run tests
cargo test --workspace

# Run scheduler examples
cargo run -p flui_scheduler --example basic_scheduler
cargo run -p flui_scheduler --example animation_ticker
```

## Usage Examples

### Example 1: Basic Frame Scheduling

```rust
use flui_scheduler::{Scheduler, Priority};

let mut scheduler = Scheduler::new();

// Schedule frame callback
scheduler.schedule_frame(Box::new(|timing| {
    println!("Frame {} started", timing.id);
}));

// Add tasks
scheduler.add_task(Priority::Animation, || {
    // Update animations
});

// Execute frame
scheduler.execute_frame();
```

### Example 2: Animation Controller

```rust
use flui_scheduler::{Scheduler, Ticker};

let scheduler = Scheduler::new();
let mut ticker = Ticker::new();

ticker.start(|elapsed| {
    let progress = elapsed / 2.0; // 2-second animation
    println!("Progress: {:.2}%", progress * 100.0);
});

// In render loop
ticker.tick();
```

### Example 3: Budget Management

```rust
let budget = scheduler.budget();
let budget_lock = budget.lock();

if budget_lock.is_over_budget() {
    println!("⚠️  Frame over budget by {:.2}ms", 
             -budget_lock.remaining_ms());
}
```

## Benefits

1. **Clean Separation** - Scheduling logic is now isolated in its own crate
2. **Testability** - Easy to test scheduling behavior independently
3. **Reusability** - Can be used in other Rust UI frameworks
4. **Performance** - Priority-based task execution ensures smooth UX
5. **Budget Control** - Automatic frame budget management prevents jank

## Next Steps

1. Integrate with `flui_devtools` for profiling
2. Add async task support (with `async` feature)
3. Implement web platform support (requestAnimationFrame)
4. Add more animation easing functions
5. Create AnimationController and Tween abstractions

## Questions?

See the main README.md for detailed API documentation and examples.