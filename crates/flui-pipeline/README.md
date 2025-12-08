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
│    ├─ BuildPhase<I>                 - Widget rebuild phase      │
│    ├─ LayoutPhase<I>                - Size computation phase    │
│    ├─ PaintPhase<I>                 - Layer generation phase    │
│    └─ PipelineCoordinator<I>        - Phase orchestration       │
├─────────────────────────────────────────────────────────────────┤
│  utilities/                                                     │
│    ├─ dirty (DirtySet<I>, LockFreeDirtySet<I>)                 │
│    ├─ buffer (TripleBuffer)                                     │
│    ├─ build (BuildPipeline<I>, BuildBatcher<I>)                │
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

- **Generic Types**: All core types are generic over `I: Identifier` for flexibility
- **Phase Traits**: `BuildPhase<I>`, `LayoutPhase<I>`, `PaintPhase<I>` for defining pipeline phases
- **Coordinator**: `PipelineCoordinator<I>` for orchestrating phase execution
- **Dirty Tracking**: Lock-free bitmap (`LockFreeDirtySet<I>`) and HashSet-based (`DirtySet<I>`) implementations
- **Triple Buffer**: Lock-free frame exchange between producer and consumer threads
- **Performance Metrics**: FPS tracking, frame timing, cache statistics with fixed-size ring buffer
- **Error Recovery**: Configurable policies with helper methods for introspection
- **Cancellation**: Thread-safe cancellation tokens with timeout support and status queries
- **Parallel Execution**: Optional rayon-based parallel processing

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
flui-pipeline = "0.1"
```

## Phase Traits

All phase traits are generic over `I: Identifier`, defaulting to `ElementId`:

### BuildPhase

Rebuilds dirty widgets with depth-aware scheduling:

```rust
use flui_pipeline::{BuildPhase, PhaseContext, PhaseResult};
use flui_tree::Identifier;

pub trait BuildPhase<I: Identifier = ElementId>: Send {
    type Tree;
    
    /// Schedule an element for rebuild at a specific depth
    fn schedule(&mut self, element_id: I, depth: usize);
    
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
use flui_tree::Identifier;

pub trait LayoutPhase<I: Identifier = ElementId>: Send {
    type Tree;
    type Constraints;
    type Size;
    
    /// Compute layout for all dirty elements
    fn compute_layout(
        &mut self, 
        tree: &mut Self::Tree, 
        constraints: Self::Constraints
    ) -> PipelineResult<Vec<I>>;
    
    /// Mark element as needing layout
    fn mark_dirty(&mut self, id: I);
    
    /// Check if layout is needed
    fn needs_layout(&self) -> bool;
}
```

### PaintPhase

Generates paint layers:

```rust
use flui_pipeline::{PaintPhase, PhaseContext, PhaseResult, PipelineResult};
use flui_tree::Identifier;

pub trait PaintPhase<I: Identifier = ElementId>: Send {
    type Tree;
    
    /// Generate paint layers for dirty elements
    fn generate_layers(&mut self, tree: &mut Self::Tree) -> PipelineResult<usize>;
    
    /// Mark element as needing repaint
    fn mark_dirty(&mut self, id: I);
    
    /// Check if paint is needed
    fn needs_paint(&self) -> bool;
}
```

## Pipeline Coordinator

Orchestrates phase execution:

```rust
use flui_pipeline::{PipelineCoordinator, CoordinatorConfig, FrameResult};
use flui_tree::Identifier;

pub trait PipelineCoordinator<I: Identifier = ElementId>: Send {
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

// Configuration with builder pattern
let config = CoordinatorConfig::default()
    .with_target_fps(60)
    .with_budget_ms(16.0)
    .with_parallel_threshold(100);
```

## Dirty Tracking

### LockFreeDirtySet (Bitmap-based)

High-performance atomic bitmap for large fixed-capacity sets:

```rust
use flui_pipeline::LockFreeDirtySet;
use flui_foundation::ElementId;

// Create with capacity for 10,000 elements
let dirty_set: LockFreeDirtySet<ElementId> = LockFreeDirtySet::new(10_000);

// Mark element dirty (lock-free, ~2ns)
let id = ElementId::new(42).unwrap();
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

let set: DirtySet<ElementId> = DirtySet::new();

// Mark dirty
set.mark(ElementId::new(1).unwrap());
set.mark_many([ElementId::new(2).unwrap(), ElementId::new(3).unwrap()]);

// Check and clear
if set.is_dirty(ElementId::new(1).unwrap()) {
    set.clear(ElementId::new(1).unwrap());
}

// Drain all dirty elements
let dirty: Vec<_> = set.drain();
```

## Build Pipeline

Depth-aware build scheduling with optional batching:

```rust
use flui_pipeline::BuildPipeline;
use flui_foundation::ElementId;

// Create build pipeline
let mut pipeline: BuildPipeline<ElementId> = BuildPipeline::new();

// Schedule elements at their tree depths
pipeline.schedule(ElementId::new(1).unwrap(), 0);  // root
pipeline.schedule(ElementId::new(2).unwrap(), 1);  // child
pipeline.schedule(ElementId::new(3).unwrap(), 1);  // sibling

// Drain sorted by depth (parents before children)
let sorted = pipeline.drain_sorted();
assert_eq!(sorted[0].1, 0); // depth 0 first

// Enable batching for coalescing rapid updates
pipeline.enable_batching();
pipeline.schedule(ElementId::new(1).unwrap(), 0);
pipeline.schedule(ElementId::new(1).unwrap(), 0); // Deduplicated!

// Locking for thread-safe access
pipeline.with_lock(|inner| {
    inner.schedule(ElementId::new(4).unwrap(), 2);
});
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

## Performance Metrics

Track frame times, phase durations, cache statistics using a fixed-size ring buffer:

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

// Query metrics (all return values are #[must_use])
println!("FPS: {:.1}", metrics.fps());
println!("Avg frame time: {:?}", metrics.avg_frame_time());
println!("Drop rate: {:.1}%", metrics.drop_rate());
println!("Cache hit rate: {:.1}%", metrics.cache_hit_rate());

// Getter methods for all fields
println!("Total frames: {}", metrics.total_frames());
println!("Dropped frames: {}", metrics.dropped_frames());
println!("Target FPS: {}", metrics.target_fps());

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
| `total_frames()` | Total frames processed |
| `dropped_frames()` | Frames exceeding budget |

## Error Recovery

Configurable strategies for handling pipeline errors:

```rust
use flui_pipeline::{ErrorRecovery, RecoveryPolicy, RecoveryAction, PipelineError, PipelinePhase};

// Create recovery with policy
let recovery = ErrorRecovery::new(RecoveryPolicy::SkipFrame);

// Or with max errors before panic
let recovery = ErrorRecovery::with_max_errors(RecoveryPolicy::SkipFrame, 100);

// Handle an error (element_id is now usize for generic compatibility)
let error = PipelineError::layout_failed(1, "invalid constraints");

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

// Policy helper methods
assert!(RecoveryPolicy::SkipFrame.is_graceful());
assert!(RecoveryPolicy::ShowErrorWidget.shows_error());
println!("Policy: {}", RecoveryPolicy::SkipFrame.as_str());

// Action helper methods
let action = RecoveryAction::SkipFrame;
assert!(action.can_continue());
assert!(action.is_skip());
assert!(!action.is_panic());

// Error count tracking
println!("Errors: {}", recovery.error_count());
assert!(!recovery.has_errors()); // After reset
assert!(!recovery.is_at_limit());
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

// Query cancellation reason
if token.is_manually_cancelled() {
    println!("Cancelled by user");
} else if token.is_timed_out() {
    println!("Timed out");
}

// Query timing
println!("Elapsed: {:?}", token.elapsed());
if let Some(remaining) = token.remaining() {
    println!("Time left: {:?}", remaining);
}

// Manual cancellation
token.cancel();
assert!(token.is_cancelled());

// Reset for reuse
let mut token = CancellationToken::new();
token.cancel();
token.reset(); // Clears cancellation, resets timer
assert!(!token.is_cancelled());
```

## Pipeline Errors

Comprehensive error types for all phases (element IDs are `usize` for generic compatibility):

```rust
use flui_pipeline::{PipelineError, PipelinePhase, PipelineResult};

// Create errors (element_id is usize)
let build_err = PipelineError::build_failed(1, "widget not found");
let layout_err = PipelineError::layout_failed(2, "invalid constraints");
let paint_err = PipelineError::paint_failed(3, "canvas error");

// Other error types
let not_found = PipelineError::element_not_found(4);
let invalid = PipelineError::invalid_state("unexpected phase order");
let cancelled = PipelineError::cancelled("timeout exceeded");

// Query error info
let phase = build_err.phase(); // PipelinePhase::Build
let element = build_err.element_id(); // Some(1)

// Error predicates
assert!(build_err.is_build_error());
assert!(build_err.is_recoverable());

// Phase predicates and helpers
assert!(PipelinePhase::Build.is_build());
assert!(PipelinePhase::Layout.is_layout());
assert!(PipelinePhase::Paint.is_paint());
println!("Phase: {}", PipelinePhase::Build.as_str()); // "build"

// Use in results
fn do_layout(id: usize) -> PipelineResult<()> {
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
│   │   ├── phase.rs        # BuildPhase<I>, LayoutPhase<I>, PaintPhase<I>
│   │   └── coordinator.rs  # PipelineCoordinator<I>, CoordinatorConfig
│   ├── dirty.rs            # LockFreeDirtySet<I>, DirtySet<I>
│   ├── build.rs            # BuildPipeline<I>, BuildBatcher<I>
│   ├── triple_buffer.rs    # TripleBuffer
│   ├── metrics.rs          # PipelineMetrics
│   ├── error.rs            # PipelineError, PipelinePhase
│   ├── recovery.rs         # ErrorRecovery, RecoveryPolicy
│   └── cancellation.rs     # CancellationToken
└── Cargo.toml
```

## Prelude

Import commonly used types:

```rust
use flui_pipeline::prelude::*;

// Includes:
// - Phase traits: BuildPhase, LayoutPhase, PaintPhase
// - Coordinator: PipelineCoordinator, CoordinatorConfig, FrameResult
// - Tree traits: TreeNav, TreeRead
// - Utilities: DirtySet, LockFreeDirtySet, TripleBuffer
// - Build: BuildPipeline, BuildBatcher
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
- [`flui-tree`](../flui-tree) - Tree abstraction traits (Identifier)
- [`flui_core`](../flui_core) - Core framework with concrete implementations

---

**FLUI Pipeline** - High-performance pipeline orchestration for UI frameworks.
