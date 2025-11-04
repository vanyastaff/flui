# FLUI - Modern Rust UI Framework

A production-ready, Flutter-inspired UI framework for Rust, featuring the proven three-tree architecture (Widget → Element → Render) with modern Rust idioms.

## 🚀 Status: Active Development

- ✅ **Type-safe ElementId** with zero-overhead niche optimization
- ✅ **Fluent PipelineBuilder** API for ergonomic configuration
- ✅ **Production features** (metrics, error recovery, batching)
- ✅ **Comprehensive documentation** and examples
- ✅ **0 compilation errors**, 49 warnings (down from 53)
- ✅ **100% backward compatible** API

## ✨ New in v0.6.0 (Week 3-4 Updates)

### ElementId with NonZeroUsize
```rust
use flui_core::ElementId;

// Type-safe: cannot create ElementId(0)
let id = ElementId::new(42);

// Zero overhead: Option<ElementId> is same size as ElementId!
assert_eq!(size_of::<ElementId>(), 8);
assert_eq!(size_of::<Option<ElementId>>(), 8);  // Still 8 bytes!

// No sentinel values needed
let child: Option<ElementId> = None;
if let Some(child_id) = child {
    // Has child
}
```

### PipelineBuilder Pattern
```rust
use flui_core::pipeline::PipelineBuilder;
use std::time::Duration;

// Production preset (metrics + error recovery + batching)
let owner = PipelineBuilder::production().build();

// Development preset (error widgets, minimal overhead)
let owner = PipelineBuilder::development().build();

// Custom configuration
let owner = PipelineBuilder::new()
    .with_metrics()
    .with_batching(Duration::from_millis(16))
    .with_error_recovery(RecoveryPolicy::UseLastGoodFrame)
    .with_build_callback(|| {
        println!("Frame requested!");
    })
    .build();
```

## 📚 Documentation

### Quick Start
- **[API_GUIDE.md](docs/API_GUIDE.md)** - Comprehensive API guide (400+ lines)
- **[SESSION_COMPLETE.md](SESSION_COMPLETE.md)** - Latest session summary
- **[WEEK3_WEEK4_COMPLETE.md](WEEK3_WEEK4_COMPLETE.md)** - Week 3-4 improvements

### Technical Details
- **[WEEK3_API_IMPROVEMENTS.md](crates/flui_core/WEEK3_API_IMPROVEMENTS.md)** - Implementation details
- **[FINAL_ARCHITECTURE_V2.md](docs/FINAL_ARCHITECTURE_V2.md)** - Architecture overview
- **[PIPELINE_ARCHITECTURE.md](docs/PIPELINE_ARCHITECTURE.md)** - Pipeline design

## 🎯 Key Features

### Three-Tree Architecture
```
Widget Tree (immutable) → Element Tree (mutable) → Render Tree (layout/paint)
```

### Type Safety
- **ElementId**: NonZeroUsize with niche optimization (zero overhead)
- **No sentinel values**: Idiomatic Option<T> usage
- **Compile-time safety**: Cannot create invalid IDs

### Performance
- **Niche optimization**: Option<ElementId> = 8 bytes (no overhead!)
- **parking_lot**: 2-3× faster than std::sync::RwLock
- **Build batching**: Deduplicate rapid setState() calls
- **Lock-free dirty tracking**: Atomic bitmap operations

### Developer Experience
- **Fluent builder API**: Clear configuration intent
- **4 presets**: production, development, testing, minimal
- **Working examples**: 2 comprehensive demos
- **Comprehensive docs**: 1,400+ lines of documentation

### Production Features
- **Metrics tracking**: FPS, frame times, cache hit rates
- **Error recovery**: Graceful degradation policies
- **Cancellation**: Timeout support for long operations
- **Triple buffer**: Lock-free frame exchange

## 🏗️ Project Structure

```
flui/
├── crates/
│   ├── flui_core/           # Core framework
│   │   ├── src/
│   │   │   ├── element/     # Element system
│   │   │   ├── pipeline/    # Build/layout/paint pipelines
│   │   │   ├── render/      # Render objects
│   │   │   ├── view/        # View system
│   │   │   └── hooks/       # Reactive hooks
│   │   └── benches/         # Benchmarks
│   ├── flui_types/          # Shared types
│   ├── flui_engine/         # Rendering engine
│   ├── flui_widgets/        # Widget library
│   └── flui_app/            # Application framework
├── examples/                # Example applications
│   ├── pipeline_builder_demo.rs
│   └── element_id_demo.rs
├── docs/                    # Documentation
│   ├── API_GUIDE.md
│   └── FINAL_ARCHITECTURE_V2.md
└── README.md               # This file
```

## 🚀 Getting Started

### Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
flui_core = "0.6.0"
```

### Basic Usage

```rust
use flui_core::pipeline::PipelineBuilder;
use flui_core::ElementId;
use flui_types::constraints::BoxConstraints;

fn main() {
    // Create pipeline with production settings
    let mut owner = PipelineBuilder::production().build();

    // Create and mount root element
    let root = create_my_app();
    let root_id = owner.set_root(root);

    // Render loop
    loop {
        // Build phase
        owner.build_scope(|o| o.flush_build());

        // Layout phase
        let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));
        let size = owner.flush_layout(constraints)?;

        // Paint phase
        let layer = owner.flush_paint()?;

        // Present to screen
        present(layer);
    }
}
```

## 📖 Examples

### Run Examples

```bash
# PipelineBuilder demo - shows all builder features
cargo run --example pipeline_builder_demo

# ElementId demo - shows niche optimization
cargo run --example element_id_demo
```

### PipelineBuilder Example

```rust
use flui_core::pipeline::{PipelineBuilder, RecoveryPolicy};
use std::time::Duration;

// Production: full features
let prod = PipelineBuilder::production().build();

// Development: error widgets
let dev = PipelineBuilder::development().build();

// Testing: fail fast
let test = PipelineBuilder::testing().build();

// Custom: your configuration
let custom = PipelineBuilder::new()
    .with_metrics()
    .with_batching(Duration::from_millis(8))  // 120fps
    .with_error_recovery(RecoveryPolicy::SkipFrame)
    .build();
```

### ElementId Example

```rust
use flui_core::ElementId;

// Create element IDs
let id = ElementId::new(42);

// Use with Option (no overhead!)
struct TreeNode {
    id: ElementId,
    parent: Option<ElementId>,  // 8 bytes (not 16!)
    children: Vec<ElementId>,
}

// Pattern matching
let node = TreeNode { id, parent: None, children: vec![] };
match node.parent {
    Some(parent_id) => println!("Has parent: {}", parent_id),
    None => println!("Root node"),
}
```

## 🧪 Testing

```bash
# Build library
cargo build -p flui_core

# Run tests
cargo test -p flui_core

# Run benchmarks
cargo bench -p flui_core

# Check for warnings
cargo clippy -p flui_core -- -D warnings
```

## 📊 Performance

### Memory Efficiency
```rust
// Before: 16 bytes with Option
Option<usize>  // 16 bytes (8 + 8 discriminant)

// After: 8 bytes with Option (niche optimization!)
Option<ElementId>  // 8 bytes (NonZeroUsize niche)
```

### Concurrency
- **parking_lot::RwLock**: 2-3× faster than std::sync
- **Lock-free dirty tracking**: Atomic bitmap
- **Triple buffer**: Concurrent renderer/compositor

### Build Performance
- **Batching**: Deduplicates rapid setState() calls
- **Incremental**: Only rebuilds dirty elements
- **Sorted by depth**: Parents before children

## 🛠️ API Overview

### Pipeline Configuration

```rust
// Presets
PipelineBuilder::production()   // Metrics + recovery + batching
PipelineBuilder::development()  // Error widgets
PipelineBuilder::testing()      // Fail fast
PipelineBuilder::minimal()      // No overhead

// Features
.with_metrics()                 // Performance tracking
.with_batching(duration)        // setState() deduplication
.with_error_recovery(policy)   // Graceful degradation
.with_cancellation()            // Timeout support
.with_frame_buffer(initial)    // Lock-free rendering
.with_build_callback(callback) // Frame notifications
```

### Element System

```rust
// ElementId (type-safe)
ElementId::new(42)              // Create (panics if 0)
ElementId::new_checked(0)       // Safe create (returns None)

// ComponentElement (for Views)
let elem = ComponentElement::new(view, state);
elem.child()                    // Option<ElementId>
elem.set_child(id)
elem.mark_dirty()

// RenderElement (for RenderObjects)
let elem = RenderElement::new(widget, render_obj);
let state = elem.render_state()
```

### Pipeline Phases

```rust
// Build phase
owner.schedule_build_for(id, depth);
owner.build_scope(|o| o.flush_build());

// Layout phase
let size = owner.flush_layout(constraints)?;

// Paint phase
let layer = owner.flush_paint()?;

// All-in-one
let layer = owner.build_frame(constraints)?;
```

## 🔧 Migration Guide

### From v0.5.x to v0.6.0

#### ElementId
```rust
// Old (still works)
type ElementId = usize;
const INVALID_ELEMENT_ID: ElementId = usize::MAX;

if child == INVALID_ELEMENT_ID {
    // No child
}

// New (recommended)
use flui_core::ElementId;

if let Some(child_id) = child {
    // Has child
}
```

#### PipelineOwner
```rust
// Old (still works)
let mut owner = PipelineOwner::new();
owner.enable_metrics();
owner.enable_batching(Duration::from_millis(16));

// New (recommended)
let owner = PipelineBuilder::production().build();
```

**Note**: Old API is 100% backward compatible!

## 🤝 Contributing

We welcome contributions! Areas for improvement:
- Fix remaining warnings (49 total)
- Add more examples
- Improve documentation
- Performance optimizations
- Additional widget implementations

## 📝 Changelog

### v0.6.0 (Week 3-4)
- ✨ ElementId with NonZeroUsize (zero overhead!)
- ✨ PipelineBuilder pattern (fluent API)
- 📚 Comprehensive documentation (1,400+ lines)
- 📖 Working examples (2 demos)
- 🧪 Benchmarks (performance testing)
- 🐛 Warning fixes (53 → 49)
- 🧹 Cleanup (31k lines removed)

### v0.5.0 (Previous)
- ✅ InheritedModel support
- ✅ O(N) multi-child reconciliation
- ✅ Complete test coverage

## 📄 License

MIT OR Apache-2.0

## 🙏 Acknowledgments

- **Flutter team** - For the proven three-tree architecture
- **Rust community** - For excellent tooling and ecosystem
- **parking_lot** - For high-performance synchronization primitives

---

**Built with ❤️ in Rust**
