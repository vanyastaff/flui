# FLUI API Guide

## Table of Contents

1. [PipelineOwner Configuration](#pipelineowner-configuration)
2. [ElementId Type Safety](#elementid-type-safety)
3. [Element System](#element-system)
4. [Performance Optimizations](#performance-optimizations)

## PipelineOwner Configuration

The `PipelineOwner` is the heart of FLUI's rendering pipeline. Use the `PipelineBuilder` for fluent, type-safe configuration.

### Basic Usage

```rust
use flui_core::pipeline::PipelineBuilder;

// Minimal configuration (lowest overhead)
let owner = PipelineBuilder::minimal().build();

// Production configuration (recommended)
let owner = PipelineBuilder::production().build();
```

### Preset Configurations

#### Production

```rust
let owner = PipelineBuilder::production().build();
```

Enables:
- ✅ Performance metrics
- ✅ Error recovery (UseLastGoodFrame policy)
- ✅ Build batching (16ms window for 60fps)
- ✅ Cancellation support

Best for: **Production deployments**

#### Development

```rust
let owner = PipelineBuilder::development().build();
```

Enables:
- ✅ Error recovery (ShowErrorWidget policy)
- ❌ Metrics (minimal overhead for fast iteration)

Best for: **Development and debugging**

#### Testing

```rust
let owner = PipelineBuilder::testing().build();
```

Enables:
- ✅ Error recovery (Panic policy - fail fast)

Best for: **Unit tests and CI**

#### Minimal

```rust
let owner = PipelineBuilder::minimal().build();
```

Enables:
- ❌ All optional features disabled

Best for: **Maximum performance, no debugging**

### Custom Configuration

```rust
use flui_core::pipeline::{PipelineBuilder, RecoveryPolicy};
use std::time::Duration;

let owner = PipelineBuilder::new()
    .with_metrics()                          // Enable performance tracking
    .with_batching(Duration::from_millis(8)) // 120fps target
    .with_error_recovery(RecoveryPolicy::SkipFrame)
    .with_cancellation()                     // Enable timeout support
    .with_build_callback(|| {
        println!("Frame requested!");
    })
    .build();
```

### Extending Presets

```rust
// Start with production, add custom callback
let owner = PipelineBuilder::production()
    .with_build_callback(|| {
        // Trigger render on state change
        request_frame();
    })
    .build();
```

### Features

#### Metrics

```rust
let mut owner = PipelineBuilder::new()
    .with_metrics()
    .build();

// After rendering frames...
if let Some(metrics) = owner.metrics() {
    println!("FPS: {:.1}", metrics.fps());
    println!("Avg frame time: {:?}", metrics.avg_frame_time());
}
```

**Overhead**: ~1% CPU, 480 bytes memory

#### Build Batching

```rust
let mut owner = PipelineBuilder::new()
    .with_batching(Duration::from_millis(16))
    .build();

// Multiple setState() calls are batched
owner.schedule_build_for(id1, 0);
owner.schedule_build_for(id1, 0); // Deduplicated!
owner.schedule_build_for(id2, 1);

// Check if ready to flush
if owner.should_flush_batch() {
    owner.flush_batch();
    owner.build_scope(|o| o.flush_build());
}

// Check stats
let (batches, saved) = owner.batching_stats();
println!("Batches: {}, Builds saved: {}", batches, saved);
```

**Benefit**: Reduces redundant rebuilds during rapid state changes

#### Error Recovery

```rust
use flui_core::pipeline::{PipelineBuilder, RecoveryPolicy};

// Production: graceful degradation
let owner = PipelineBuilder::new()
    .with_error_recovery(RecoveryPolicy::UseLastGoodFrame)
    .build();

// Development: show errors
let owner = PipelineBuilder::new()
    .with_error_recovery(RecoveryPolicy::ShowErrorWidget)
    .build();

// Testing: fail fast
let owner = PipelineBuilder::new()
    .with_error_recovery(RecoveryPolicy::Panic)
    .build();
```

**Overhead**: ~40 bytes memory

#### Cancellation

```rust
use std::time::Duration;

let owner = PipelineBuilder::new()
    .with_cancellation()
    .build();

// Set timeout for frame rendering
if let Some(token) = owner.cancellation_token() {
    token.set_timeout(Duration::from_millis(16));
}
```

**Overhead**: ~24 bytes memory

#### Frame Buffer

```rust
use flui_engine::ContainerLayer;
use std::sync::Arc;

let initial = Arc::new(Box::new(ContainerLayer::new()) as crate::BoxedLayer);
let mut owner = PipelineBuilder::new()
    .with_frame_buffer(initial)
    .build();

// Renderer thread
if let Some(layer) = owner.build_frame(constraints)? {
    owner.publish_frame(layer); // Non-blocking!
}

// Compositor thread (concurrent!)
if let Some(buffer) = owner.frame_buffer() {
    let layer = buffer.read();
    compositor.present(&*layer);
}
```

**Benefit**: Lock-free concurrent rendering and compositing

---

## ElementId Type Safety

`ElementId` uses `NonZeroUsize` for type safety and niche optimization.

### Benefits

1. **Zero overhead**: `Option<ElementId>` is same size as `ElementId` (8 bytes)
2. **Type-safe**: Cannot create `ElementId(0)`, prevents invalid IDs
3. **No sentinel values**: Use `Option` instead of checking for special values

### Creating ElementIds

```rust
use flui_core::ElementId;

// Direct creation (panics if 0)
let id = ElementId::new(42);

// Safe creation (returns Option)
let maybe_id = ElementId::new_checked(0); // None
let valid_id = ElementId::new_checked(1); // Some(ElementId(1))

// Unsafe creation (caller must ensure non-zero)
let id = unsafe { ElementId::new_unchecked(42) };
```

### Using Option<ElementId>

```rust
struct TreeNode {
    id: ElementId,
    parent: Option<ElementId>,  // No overhead!
    left: Option<ElementId>,
    right: Option<ElementId>,
}

let root = TreeNode {
    id: ElementId::new(1),
    parent: None,              // Clean API
    left: Some(ElementId::new(2)),
    right: Some(ElementId::new(3)),
};

// Pattern matching
match root.parent {
    Some(parent_id) => println!("Has parent: {}", parent_id),
    None => println!("Root node"),
}

// Idiomatic checks
if root.left.is_some() {
    // Has left child
}
```

### Memory Layout

```rust
use std::mem::size_of;

assert_eq!(size_of::<ElementId>(), 8);
assert_eq!(size_of::<Option<ElementId>>(), 8);  // Same size!

// Compare with plain usize
assert_eq!(size_of::<usize>(), 8);
assert_eq!(size_of::<Option<usize>>(), 16);     // 8 bytes overhead!
```

### Operations

```rust
let id = ElementId::new(100);

// Comparisons
assert!(id == ElementId::new(100));
assert!(id < ElementId::new(200));

// Arithmetic (for bitmap indexing)
let next = id + 5;              // ElementId(105)
let offset = id - 50;           // 50 (usize)

// Conversion
let as_usize: usize = id.into();
let back = ElementId::new(as_usize);

// Display
println!("{}", id);             // "ElementId(100)"
println!("{:?}", id);           // "ElementId(100)"
```

### Collections

```rust
use std::collections::HashSet;

let mut seen = HashSet::new();
seen.insert(ElementId::new(1));
seen.insert(ElementId::new(2));
seen.insert(ElementId::new(1)); // Duplicate ignored

assert_eq!(seen.len(), 2);
```

---

## Element System

### ComponentElement

Manages View lifecycle for stateless and stateful widgets.

```rust
use flui_core::element::{ComponentElement, Element};

// Create from view
let component = ComponentElement::new(
    Box::new(my_view),
    Box::new(my_state),
);

// Check child
if let Some(child_id) = component.child() {
    // Has child
}

// Set child
component.set_child(child_id);

// Clear child
component.clear_child();
```

### RenderElement

Manages RenderObject for layout and paint.

```rust
use flui_core::element::{RenderElement, Element};

let render = RenderElement::new(
    Box::new(render_widget),
    Box::new(render_object),
);

// Access render state
let state = render.render_state();
let state_guard = state.read();
println!("Size: {:?}", state_guard.size());
```

### InheritedElement (Provider)

Manages data propagation and dependency tracking.

```rust
use flui_core::element::{InheritedElement, Element};

// Create provider
let provider = InheritedElement::new(
    Box::new(inherited_widget),
    Box::new(data),
);

// Add dependent
provider.add_dependent(dependent_id, None);

// Check dependents
println!("Dependents: {}", provider.dependents().count());
```

---

## Performance Optimizations

### parking_lot RwLock

FLUI uses `parking_lot::RwLock` for 2-3× better performance than `std::sync::RwLock`:

```rust
use parking_lot::RwLock;
use std::sync::Arc;

let tree = Arc::new(RwLock::new(ElementTree::new()));

// Read access (multiple readers)
let read_guard = tree.read();

// Write access (exclusive)
let write_guard = tree.write();
```

**Benefits**:
- Faster lock/unlock
- Fair scheduling (no writer starvation)
- No poisoning overhead
- Better cache locality

### Build Batching

Deduplicate rapid setState() calls:

```rust
let mut owner = PipelineBuilder::production().build();

// These get batched into single rebuild
owner.schedule_build_for(id, 0);
owner.schedule_build_for(id, 0); // Duplicate
owner.schedule_build_for(id, 0); // Duplicate

owner.flush_batch();
owner.build_scope(|o| o.flush_build());

// Stats show 2 builds saved
let (_, saved) = owner.batching_stats();
assert_eq!(saved, 2);
```

### Lock-Free Dirty Tracking

Atomic bitmap for dirty element tracking:

```rust
// No locks needed for marking dirty!
owner.schedule_build_for(id, depth); // Lock-free add to bitmap
```

### TripleBuffer

Lock-free frame exchange between renderer and compositor:

```rust
// Renderer writes
owner.publish_frame(layer);

// Compositor reads (concurrent!)
if let Some(buffer) = owner.frame_buffer() {
    let layer = buffer.read();
    present(&*layer);
}
```

---

## Examples

- **PipelineBuilder**: `cargo run --example pipeline_builder_demo`
- **ElementId**: `cargo run --example element_id_demo`

## Benchmarks

- **ElementId**: `cargo bench --bench element_id_bench`
