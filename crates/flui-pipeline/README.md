# FLUI Pipeline

[![Crates.io](https://img.shields.io/crates/v/flui-pipeline)](https://crates.io/crates/flui-pipeline)
[![Documentation](https://docs.rs/flui-pipeline/badge.svg)](https://docs.rs/flui-pipeline)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](https://github.com/flui-org/flui)

**Abstract pipeline traits and utilities for FLUI's rendering system.**

FLUI Pipeline provides the infrastructure for coordinating build, layout, and paint phases in UI rendering. It includes abstract traits for phase implementation, dirty tracking utilities, lock-free frame exchange, performance metrics, and error recovery strategies.

## Architecture

```text
┌─────────────────────────────────────────────────────────────────┐
│                        flui-pipeline                            │
│  (Abstract traits + utilities)                                  │
├─────────────────────────────────────────────────────────────────┤
│  traits/                                                        │
│    ├─ BuildPhase                    - Widget rebuild phase      │
│    ├─ LayoutPhase                   - Size computation phase    │
│    ├─ PaintPhase                    - Layer generation phase    │
│    ├─ PipelineCoordinator           - Phase orchestration       │
│    └─ SchedulerIntegration          - Scheduler bridge          │
├─────────────────────────────────────────────────────────────────┤
│  utilities/                                                     │
│    ├─ dirty (DirtySet, LockFreeDirtySet)                       │
│    ├─ buffer (TripleBuffer)                                     │
│    ├─ metrics (PipelineMetrics)                                │
│    ├─ recovery (ErrorRecovery)                                  │
│    └─ cancellation (CancellationToken)                         │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    flui_core/pipeline                           │
│  (Concrete implementations)                                     │
│    ├─ BuildPipeline   : impl BuildPhase                         │
│    ├─ LayoutPipeline  : impl LayoutPhase                        │
│    ├─ PaintPipeline   : impl PaintPhase                         │
│    └─ FrameCoordinator: impl PipelineCoordinator               │
└─────────────────────────────────────────────────────────────────┘
```

## Features

- **Phase Traits**: `BuildPhase`, `LayoutPhase`, `PaintPhase` for defining pipeline phases
- **Coordinator**: `PipelineCoordinator` for orchestrating phase execution
- **Dirty Tracking**: Lock-free bitmap (`LockFreeDirtySet`) and HashSet-based (`DirtySet`) implementations
- **Triple Buffer**: Lock-free frame exchange between producer and consumer threads
- **Performance Metrics**: FPS tracking, frame timing, cache statistics
- **Error Recovery**: Configurable policies (skip frame, use last good frame, show error)
- **Cancellation**: Thread-safe cancellation tokens with timeout support
- **Parallel Execution**: Optional rayon-based parallel processing

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
flui-pipeline = "0.1"
```

## Phase Traits

### BuildPhase

Rebuilds dirty widgets with depth-aware scheduling:

```rust
use flui_pipeline::{BuildPhase, PhaseContext, PhaseResult};
use flui_foundation::ElementId;

pub trait BuildPhase: Send {
    type Tree;
    
    /// Schedule an element for rebuild at a specific depth
    fn schedule(&mut self, element_id: ElementId, depth: usize);
    
    /// Rebuild all dirty elements, returns count of rebuilt
    fn rebuild_dirty(&mut self, tree: &Self::Tree) -> usize;
    
    /// Check if there are pending rebuilds
    fn has_pending(&self) -> bool;
    
    /// Clear all pending rebuilds
    fn clear(&mut self);
}
```

### LayoutPhase

Computes sizes with constraint propagation:

```rust
use flui_pipeline::{LayoutPhase, PhaseContext, PhaseResult, PipelineResult};
use flui_foundation::ElementId;

pub trait LayoutPhase: Send {
    type Tree;
    type Constraints;
    type Size;
    
    /// Compute layout for all dirty elements
    fn compute_layout(
        &mut self, 
        tree: &mut Self::Tree, 
        constraints: Self::Constraints
    ) -> PipelineResult<Vec<ElementId>>;
    
    /// Mark element as needing layout
    fn mark_dirty(&mut self, id: ElementId);
    
    /// Check if layout is needed
    fn needs_layout(&self) -> bool;
}
```

### PaintPhase

Generates paint layers:

```rust
use flui_pipeline::{PaintPhase, PhaseContext, PhaseResult, PipelineResult};

pub trait PaintPhase: Send {
    type Tree;
    
    /// Generate paint layers for dirty elements
    fn generate_layers(&mut self, tree: &mut Self::Tree) -> PipelineResult<usize>;
    
    /// Mark element as needing repaint
    fn mark_dirty(&mut self, id: ElementId);
    
    /// Check if paint is needed
    fn needs_paint(&self) -> bool;
}
```

## Pipeline Coordinator

Orchestrates phase execution:

```rust
use flui_pipeline::{PipelineCoordinator, CoordinatorConfig, FrameResult};

pub trait PipelineCoordinator: Send {
    type Tree;
    type Constraints;
    
    /// Execute a complete frame (build → layout → paint)
    fn run_frame(
        &mut self, 
        tree: &mut Self::Tree, 
        constraints: Self::Constraints
    ) -> FrameResult;
    
    /// Request a new frame
    fn request_frame(&mut self);
    
    /// Check if frame is needed
    fn needs_frame(&self) -> bool;
}

// Configuration
let config = CoordinatorConfig {
    target_fps: 60,
    budget_ms: 16.0,
    parallel_threshold: 100,
};
```

## Dirty Tracking

### LockFreeDirtySet (Bitmap-based)

High-performance atomic bitmap for large fixed-capacity sets:

```rust
use flui_pipeline::LockFreeDirtySet;
use flui_foundation::ElementId;

// Create with capacity for 10,000 elements
let dirty_set = LockFreeDirtySet::new(10_000);

// Mark element dirty (lock-free, ~2ns)
let id = ElementId::new(42);
dirty_set.mark_dirty(id);

// Check if dirty (lock-free, ~2ns)
assert!(dirty_set.is_dirty(id));

// Collect all dirty elements
let dirty_elements = dirty_set.collect_dirty();
assert!(dirty_elements.contains(&id));

// Clear dirty flag
dirty_set.clear_dirty(id);
assert!(!dirty_set.is_dirty(id));

// Bulk operations
dirty_set.mark_all_dirty();   // Global invalidation
dirty_set.clear_all();        // Reset all flags
let drained = dirty_set.drain(); // Take and clear

// Statistics
println!("Dirty count: {}", dirty_set.dirty_count());
println!("Has dirty: {}", dirty_set.has_dirty());
```

**Performance:**
- `mark_dirty`: ~2ns (single atomic OR)
- `is_dirty`: ~2ns (single atomic load)
- `clear_dirty`: ~2ns (single atomic AND)
- `collect_dirty`: O(capacity/64) bitmap scan

**Memory:** 8 bytes per 64 elements (extremely compact)

### DirtySet (HashSet-based)

Simple thread-safe dirty set for smaller dynamic sets:

```rust
use flui_pipeline::DirtySet;
use flui_foundation::ElementId;

let set = DirtySet::new();

// Mark dirty
set.mark(ElementId::new(1));
set.mark_many([ElementId::new(2), ElementId::new(3)]);

// Check and clear
if set.is_dirty(ElementId::new(1)) {
    set.clear(ElementId::new(1));
}

// Drain all dirty elements
let dirty: Vec<_> = set.drain();
```

## Triple Buffer

Lock-free frame exchange between producer (pipeline) and consumer (renderer):

```text
Producer (Pipeline)          Consumer (Renderer)
        │                           │
   ┌────▼────┐                 ┌────▼────┐
   │ Write   │                 │  Read   │
   │ Buffer  │                 │ Buffer  │
   └────┬────┘                 └────┬────┘
        │                           │
   ┌────▼────────────────────────────▼────┐
   │         Shared Buffer (atomic)        │
   └──────────────────────────────────────┘
```

```rust
use flui_pipeline::TripleBuffer;

// Create buffer with initial values
let buffer = TripleBuffer::new(
    Frame::default(),
    Frame::default(),
    Frame::default(),
);

// Producer: write new frame
buffer.write(new_frame);

// Consumer: read latest frame
let frame = buffer.read();

// Check for new data without consuming
if buffer.has_new_data() {
    let frame = buffer.read();
}

// Peek without swapping buffers
let current = buffer.peek();
```

**Thread Safety:**
- Write operations: exclusive (single producer)
- Read operations: exclusive (single consumer)
- Exchange: lock-free using atomic operations

## Performance Metrics

Track frame times, phase durations, cache statistics:

```rust
use flui_pipeline::PipelineMetrics;
use std::time::Duration;

let mut metrics = PipelineMetrics::new();

// Or with custom target FPS
let mut metrics = PipelineMetrics::with_target_fps(120);

// Record frame timing
metrics.record_frame(
    Duration::from_micros(2000),  // build time
    Duration::from_micros(1000),  // layout time
    Duration::from_micros(500),   // paint time
);

// Or use start/end for automatic timing
metrics.start_frame();
// ... frame work ...
metrics.end_frame();

// Record cache statistics
metrics.record_cache_hit();
metrics.record_cache_miss();

// Query metrics
println!("FPS: {:.1}", metrics.fps());
println!("Avg frame time: {:?}", metrics.avg_frame_time());
println!("Drop rate: {:.1}%", metrics.drop_rate());
println!("Cache hit rate: {:.1}%", metrics.cache_hit_rate());

// Per-phase averages
println!("Avg build time: {:?}", metrics.avg_build_time());
println!("Avg layout time: {:?}", metrics.avg_layout_time());
println!("Avg paint time: {:?}", metrics.avg_paint_time());

// Reset metrics
metrics.reset();
```

**Available Metrics:**
| Metric | Description |
|--------|-------------|
| `fps()` | Current FPS based on recent frames |
| `avg_frame_time()` | Average total frame time |
| `drop_rate()` | Percentage of dropped frames |
| `avg_build_time()` | Average build phase duration |
| `avg_layout_time()` | Average layout phase duration |
| `avg_paint_time()` | Average paint phase duration |
| `cache_hit_rate()` | Cache hit percentage |
| `total_frames` | Total frames processed |
| `dropped_frames` | Frames exceeding budget |

## Error Recovery

Configurable strategies for handling pipeline errors:

```rust
use flui_pipeline::{ErrorRecovery, RecoveryPolicy, RecoveryAction, PipelineError, PipelinePhase};
use flui_foundation::ElementId;

// Create recovery with policy
let recovery = ErrorRecovery::new(RecoveryPolicy::SkipFrame);

// Or with max errors before panic
let recovery = ErrorRecovery::with_max_errors(RecoveryPolicy::SkipFrame, 100);

// Handle an error
let error = PipelineError::layout_failed(ElementId::new(1), "invalid constraints");

match recovery.handle_error(error, PipelinePhase::Layout) {
    RecoveryAction::SkipFrame => {
        // Skip this frame, continue
    }
    RecoveryAction::UseLastFrame => {
        // Use last successfully rendered frame
    }
    RecoveryAction::ShowError(e) => {
        // Show error widget overlay (dev mode)
        render_error_overlay(&e);
    }
    RecoveryAction::Panic(e) => {
        // Fatal error, panic
        panic!("Pipeline error: {}", e);
    }
}

// Check error count
println!("Errors: {}", recovery.error_count());
```

**Recovery Policies:**
| Policy | Use Case | Behavior |
|--------|----------|----------|
| `UseLastGoodFrame` | Production | Show last successful frame |
| `ShowErrorWidget` | Development | Show error overlay |
| `SkipFrame` | General | Skip failed frame, continue |
| `Panic` | Testing | Fail fast |

## Cancellation Tokens

Thread-safe cancellation for long-running operations:

```rust
use flui_pipeline::CancellationToken;
use std::time::Duration;

// Create token with timeout
let token = CancellationToken::with_timeout(Duration::from_millis(16));

// Or create and set timeout later
let token = CancellationToken::new();
token.set_timeout(Duration::from_millis(16));

// In pipeline code - check for cancellation
fn process_elements(token: &CancellationToken) -> Result<(), PipelineError> {
    for element in elements {
        if token.is_cancelled() {
            return Err(PipelineError::cancelled("timeout"));
        }
        process(element);
    }
    Ok(())
}

// Manual cancellation
token.cancel();
assert!(token.is_cancelled());

// Query remaining time
if let Some(remaining) = token.remaining() {
    println!("Time left: {:?}", remaining);
}

// Reset for reuse
let mut token = CancellationToken::new();
token.cancel();
token.reset(); // Clears cancellation, resets timer
assert!(!token.is_cancelled());
```

## Pipeline Errors

Comprehensive error types for all phases:

```rust
use flui_pipeline::{PipelineError, PipelinePhase, PipelineResult};
use flui_foundation::ElementId;

// Create errors
let build_err = PipelineError::build_failed(ElementId::new(1), "widget not found");
let layout_err = PipelineError::layout_failed(ElementId::new(2), "invalid constraints");
let paint_err = PipelineError::paint_failed(ElementId::new(3), "canvas error");

// Other error types
let not_found = PipelineError::element_not_found(ElementId::new(4));
let invalid = PipelineError::invalid_state("unexpected phase order");
let cancelled = PipelineError::cancelled("timeout exceeded");

// Query error info
let phase = build_err.phase(); // PipelinePhase::Build
let element = build_err.element_id(); // Some(ElementId(1))

// Use in results
fn do_layout(id: ElementId) -> PipelineResult<()> {
    if !valid_constraints() {
        return Err(PipelineError::layout_failed(id, "constraints out of bounds"));
    }
    Ok(())
}
```

**Error Types:**
| Error | Phase | Description |
|-------|-------|-------------|
| `BuildFailed` | Build | Widget tree construction failed |
| `LayoutFailed` | Layout | Size computation failed |
| `PaintFailed` | Paint | Layer generation failed |
| `ElementNotFound` | Any | Element not in tree |
| `InvalidState` | Any | Unexpected state detected |
| `Cancelled` | Any | Operation cancelled/timeout |
| `NoRoot` | Any | No root element attached |
| `ConstraintViolation` | Layout | Constraint bounds exceeded |

## Scheduler Integration

Bridge to flui-scheduler for priority-based task scheduling:

```rust
use flui_pipeline::{SchedulerIntegration, Priority, FrameTiming};

pub trait SchedulerIntegration: Send + Sync {
    /// Schedule a callback at given priority
    fn schedule(&self, priority: Priority, callback: Box<dyn FnOnce() + Send>);
    
    /// Get current frame timing info
    fn frame_timing(&self) -> FrameTiming;
    
    /// Request vsync callback
    fn request_vsync(&self, callback: Box<dyn FnOnce() + Send>);
}

// Priority levels (highest to lowest)
// Priority::UserInput > Priority::Animation > Priority::Build > Priority::Idle
```

**Test Utilities:**
```rust
use flui_pipeline::{NoopScheduler, RecordingScheduler};

// NoopScheduler - does nothing (for testing)
let scheduler = NoopScheduler;

// RecordingScheduler - records scheduled tasks
let scheduler = RecordingScheduler::new();
scheduler.schedule(Priority::Build, Box::new(|| {}));
assert_eq!(scheduler.scheduled_count(), 1);
```

## Visitor Traits (Re-exported from flui-tree)

Abstract patterns for phase operations:

```rust
use flui_pipeline::{
    LayoutVisitable, LayoutVisitableExt,
    PaintVisitable, PaintVisitableExt,
    HitTestVisitable, HitTestVisitableExt,
    layout_with_callback, paint_with_callback, hit_test_with_callback,
};

// Layout visitor
impl LayoutVisitable for MyRenderObject {
    fn layout_visit(
        &mut self,
        tree: &impl TreeNav,
        constraints: BoxConstraints,
    ) -> Size {
        // Compute size
    }
}

// Paint visitor
impl PaintVisitable for MyRenderObject {
    fn paint_visit(
        &self,
        tree: &impl TreeNav,
        offset: Offset,
    ) -> BoxedLayer {
        // Generate paint commands
    }
}

// Hit test visitor
impl HitTestVisitable for MyRenderObject {
    fn hit_test_visit(
        &self,
        tree: &impl TreeNav,
        position: Point,
    ) -> bool {
        // Check if point is within bounds
    }
}
```

## Feature Flags

| Feature | Description | Dependencies |
|---------|-------------|--------------|
| `parallel` | Enable rayon-based parallel phase execution | `rayon` |
| `serde` | Serialization support for metrics and errors | `serde` |
| `full` | Enable all features | All |

```toml
[dependencies]
flui-pipeline = { version = "0.1", features = ["parallel", "serde"] }
```

## Module Structure

```
flui-pipeline/
├── src/
│   ├── lib.rs              # Main entry, re-exports
│   ├── traits/
│   │   ├── mod.rs          # Trait module
│   │   ├── phase.rs        # BuildPhase, LayoutPhase, PaintPhase
│   │   ├── coordinator.rs  # PipelineCoordinator, CoordinatorConfig
│   │   ├── scheduler_integration.rs # SchedulerIntegration
│   │   └── visitor.rs      # Re-exports from flui-tree
│   ├── dirty.rs            # LockFreeDirtySet, DirtySet
│   ├── triple_buffer.rs    # TripleBuffer
│   ├── metrics.rs          # PipelineMetrics
│   ├── error.rs            # PipelineError, PipelinePhase
│   ├── recovery.rs         # ErrorRecovery, RecoveryPolicy
│   ├── cancellation.rs     # CancellationToken
│   └── build.rs            # BuildPipeline, BuildBatcher
└── Cargo.toml
```

## Prelude

Import commonly used types:

```rust
use flui_pipeline::prelude::*;

// Includes:
// - Phase traits: BuildPhase, LayoutPhase, PaintPhase
// - Coordinator: PipelineCoordinator, CoordinatorConfig, FrameResult
// - Scheduler: SchedulerIntegration, Priority, FrameTiming
// - Utilities: DirtySet, LockFreeDirtySet, TripleBuffer
// - Metrics: PipelineMetrics
// - Errors: PipelineError, PipelinePhase, PipelineResult
// - Recovery: ErrorRecovery, RecoveryPolicy, RecoveryAction
// - Cancellation: CancellationToken
```

## Contributing

We welcome contributions! Please see [CONTRIBUTING.md](../../CONTRIBUTING.md).

```bash
# Run tests
cargo test -p flui-pipeline

# Run with all features
cargo test -p flui-pipeline --all-features

# Check documentation
cargo doc -p flui-pipeline --open
```

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))
- MIT License ([LICENSE-MIT](../../LICENSE-MIT))

at your option.

## Related Crates

- [`flui-foundation`](../flui-foundation) - Foundation types (ElementId, Key, etc.)
- [`flui-tree`](../flui-tree) - Tree abstraction traits
- [`flui-scheduler`](../flui-scheduler) - Task scheduling system
- [`flui_core`](../flui_core) - Core framework with concrete implementations

---

**FLUI Pipeline** - High-performance pipeline orchestration for UI frameworks.
