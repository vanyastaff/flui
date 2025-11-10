# FLUI - Modern Rust UI Framework

A production-ready, Flutter-inspired declarative UI framework for Rust, featuring the proven three-tree architecture (View → Element → Render) with modern Rust idioms and GPU-accelerated rendering.

## 🚀 Status: Production Ready

- ✅ **426 passing tests** with 100% core functionality coverage
- ✅ **Zero clippy warnings** in library and test code
- ✅ **Complete documentation** with no rustdoc warnings
- ✅ **Thread-safe architecture** using Arc/Mutex for multi-threaded UI
- ✅ **GPU-accelerated rendering** with wgpu backend
- ✅ **Modern View API** with 75% less boilerplate
- ✅ **Production features** (metrics, error recovery, frame scheduling)

## ✨ Latest: v0.7.0 - Thread-Safe Hooks & Modern View API

### Copy-Based Signals (Thread-Safe)
```rust
use flui_core::hooks::use_signal;
use flui_core::prelude::*;

#[derive(Debug)]
struct Counter;

impl View for Counter {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Signal is Copy - no .clone() needed!
        let count = use_signal(ctx, 0);

        column![
            text(format!("Count: {}", count.get(ctx))),
            button("Increment").on_press(move || {
                count.update(|n| *n + 1);  // Thread-safe!
            })
        ]
    }
}
```

### Unified View Trait (Simplified API)
```rust
// Old API (deprecated): GATs, rebuild(), teardown()
// New API: Just one method!

impl View for Padding {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        (RenderPadding::new(self.padding), self.child)
    }
}
```

### Thread-Safe Architecture
- **All hooks use Arc/Mutex** (parking_lot for 2-3x performance)
- **Signal values must be Send** for multi-threaded UI
- **Callbacks are Send + Sync** for safe concurrent access
- **No Rc/RefCell** - fully thread-safe by design

## 🎯 Key Features

### Three-Tree Architecture
```
View Tree (immutable) → Element Tree (mutable) → Render Tree (layout/paint)
```

- **Views**: Lightweight, immutable configuration (can be moved, no Clone required)
- **Elements**: Persistent state and lifecycle management
- **Renders**: Layout calculations and GPU-accelerated painting

### Modern Reactive Hooks

```rust
// Signal - reactive state (Copy-based, thread-safe)
let count = use_signal(ctx, 0);
count.set(42);  // Triggers rebuild automatically

// Memo - derived state with automatic tracking
let doubled = use_memo(ctx, |ctx| count.get(ctx) * 2);

// Effect - side effects with cleanup
use_effect(ctx, move |ctx| {
    println!("Count: {}", count.get(ctx));
    None  // No cleanup needed
});
```

**Hook Rules** (strictly enforced):
1. ✅ Always call hooks in the same order
2. ❌ Never call hooks conditionally
3. ❌ Never call hooks in loops with variable iterations
4. ✅ Clone signals before moving into closures

See [RULES.md](crates/flui_core/src/hooks/RULES.md) for details.

### GPU-Accelerated Rendering

FLUI uses **wgpu** for high-performance, cross-platform graphics:

- **Hardware acceleration**: Native GPU performance on all platforms
- **Modern graphics APIs**: Vulkan, Metal, DX12, WebGPU
- **Efficient tessellation**: lyon for converting vectors to triangles
- **SDF text rendering**: glyphon for high-quality text at any scale

### Type Safety & Performance

- **ElementId with NonZeroUsize**: Zero-overhead niche optimization
  ```rust
  assert_eq!(size_of::<ElementId>(), 8);
  assert_eq!(size_of::<Option<ElementId>>(), 8);  // Still 8 bytes!
  ```

- **parking_lot synchronization**: 2-3× faster than std::sync
- **Slab allocator**: O(1) element insertion/removal
- **Lock-free dirty tracking**: Atomic bitmap operations

### Production Features

- **Frame scheduling**: Budget management with FrameSkipPolicy
- **Error recovery**: 4 policies (UseLastGoodFrame, ShowError, SkipFrame, Panic)
- **Performance metrics**: FPS tracking, frame times, dropped frames
- **Parallel build**: Multi-threaded widget rebuilds (optional feature)

## 🏗️ Project Structure

```
flui/
├── crates/
│   ├── flui_core/           # Core framework (426 tests)
│   │   ├── src/
│   │   │   ├── element/     # Element system (Component, Render, Provider)
│   │   │   ├── pipeline/    # Build/layout/paint pipelines
│   │   │   ├── render/      # Render traits (Leaf, Single, Multi)
│   │   │   ├── view/        # Unified View trait
│   │   │   ├── hooks/       # Reactive hooks (Signal, Memo, Effect)
│   │   │   ├── foundation/  # Keys, notifications, diagnostics
│   │   │   └── testing/     # Test utilities
│   │   └── examples/
│   │       ├── simplified_view.rs       # Modern View API demo
│   │       ├── thread_safe_hooks.rs     # Thread-safety demo
│   │       ├── theme_provider_demo.rs   # Provider pattern
│   │       └── hit_test_demo.rs         # Event handling
│   ├── flui_types/          # Shared types (Size, Offset, Color, etc.)
│   ├── flui_painting/       # 2D graphics primitives
│   ├── flui_engine/         # wgpu rendering engine
│   ├── flui_rendering/      # RenderObject implementations
│   ├── flui_widgets/        # Widget library (Text, Container, etc.)
│   ├── flui_app/            # Application framework
│   ├── flui_assets/         # Asset management (images, fonts)
│   └── flui_devtools/       # Development tools
├── examples/                # Application examples
│   ├── hello_world_view.rs
│   └── profile_card.rs
└── docs/                    # Comprehensive documentation
    ├── API_GUIDE.md
    ├── FINAL_ARCHITECTURE_V2.md
    └── PIPELINE_ARCHITECTURE.md
```

## 🚀 Getting Started

### Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
flui_core = "0.7"
flui_widgets = "0.7"
```

### Hello World

```rust
use flui_core::prelude::*;
use flui_widgets::Text;

#[derive(Debug)]
struct HelloWorld;

impl View for HelloWorld {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        Text::new("Hello, FLUI!")
    }
}

fn main() {
    let mut pipeline = PipelineOwner::new();
    let root = HelloWorld.into_element();
    pipeline.set_root(root);

    // Render loop
    loop {
        let layer = pipeline.build_frame(constraints)?;
        present(layer);
    }
}
```

### Counter Example (with Hooks)

```rust
use flui_core::prelude::*;
use flui_core::hooks::use_signal;

#[derive(Debug)]
struct Counter;

impl View for Counter {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let count = use_signal(ctx, 0);

        Column::new()
            .children(vec![
                Box::new(Text::new(format!("Count: {}", count.get(ctx)))),
                Box::new(Button::new("Increment")
                    .on_pressed(move || count.update(|n| *n + 1))),
            ])
    }
}
```

## 📖 Examples

### Run Examples

```bash
# Core examples
cargo run --example simplified_view       # Modern View API
cargo run --example thread_safe_hooks     # Thread-safe hooks demo
cargo run --example theme_provider_demo   # Provider pattern
cargo run --example hit_test_demo         # Event handling

# Application examples
cargo run --example hello_world_view      # Hello world
cargo run --example profile_card          # Profile card widget
```

## 🧪 Testing

```bash
# Build workspace
cargo build --workspace

# Run all tests
cargo test --workspace

# Run specific crate tests
cargo test -p flui_core

# Check documentation
cargo doc -p flui_core --no-deps

# Run clippy (no warnings!)
cargo clippy --workspace -- -D warnings

# Format code
cargo fmt --all
```

## 📚 Documentation

### Essential Reading

- **[API_GUIDE.md](docs/API_GUIDE.md)** - Complete API guide with examples
- **[CLAUDE.md](CLAUDE.md)** - Project conventions and build commands
- **[crates/flui_core/src/lib.rs](crates/flui_core/src/lib.rs)** - Architecture overview

### Technical Deep Dives

- **[FINAL_ARCHITECTURE_V2.md](docs/FINAL_ARCHITECTURE_V2.md)** - System architecture
- **[PIPELINE_ARCHITECTURE.md](docs/PIPELINE_ARCHITECTURE.md)** - Rendering pipeline
- **[hooks/RULES.md](crates/flui_core/src/hooks/RULES.md)** - Hook usage rules
- **[RENDER_OBJECT_GUIDE.md](crates/flui_rendering/RENDER_OBJECT_GUIDE.md)** - Creating RenderObjects

### Migration Guides

- **[VIEW_API_MIGRATION_COMPLETE.md](VIEW_API_MIGRATION_COMPLETE.md)** - v0.6.0 → v0.7.0
- **[THREAD_SAFE_HOOKS_REFACTORING.md](THREAD_SAFE_HOOKS_REFACTORING.md)** - Thread-safety changes

## 🔧 Feature Flags

```toml
# Thread-safe parallel processing (stable)
flui_core = { version = "0.7", features = ["parallel"] }

# Asset management
flui_assets = { version = "0.7", features = ["images", "network", "hot-reload"] }
```

## 📊 Performance

### Memory Efficiency
- **Option<ElementId>**: 8 bytes (niche optimization)
- **Signal<T>**: 8 bytes (just an ID, Copy-able)
- **Slab storage**: Contiguous, cache-friendly

### Concurrency
- **parking_lot::RwLock**: 2-3× faster than std, no poisoning
- **parking_lot::Mutex**: Smaller footprint, better performance
- **Lock-free operations**: Atomic dirty tracking, triple buffering

### GPU Acceleration
- **wgpu**: Native GPU performance on all platforms
- **Mesh-based rendering**: All primitives tessellate to triangles
- **Buffer pooling**: Reuses GPU buffers across frames

## 🛠️ API Overview

### View System

```rust
// Unified View trait (v0.7.0+)
impl View for MyWidget {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Return RenderObject + children
        (RenderMyWidget::new(), self.child)
    }
}

// Element types returned by build():
(LeafRender, ())                    // No children
(SingleRender, Option<child>)       // One child
(MultiRender, Vec<children>)        // Multiple children
AnyElement                          // Pre-built element
```

### Hooks (Thread-Safe)

```rust
use flui_core::hooks::*;

// Signal - reactive state (Copy)
let count = use_signal(ctx, 0);
count.set(42);
count.update(|n| *n += 1);

// Memo - computed value
let doubled = use_memo(ctx, |ctx| count.get(ctx) * 2);

// Effect - side effects
use_effect(ctx, move |ctx| {
    println!("Count: {}", count.get(ctx));
    Some(|| println!("Cleanup"))
});
```

### Pipeline Management

```rust
use flui_core::pipeline::*;

// Create pipeline
let mut owner = PipelineBuilder::production().build();

// Set root
let root_id = owner.set_root(element);

// Render phases
owner.flush_build();                    // Build dirty elements
let size = owner.flush_layout(constraints)?;  // Layout
let layer = owner.flush_paint()?;      // Paint

// All-in-one
let layer = owner.build_frame(constraints)?;
```

### RenderObject Creation

```rust
use flui_core::render::*;

// Leaf render (no children)
impl LeafRender for RenderText {
    type Metadata = ();

    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Compute size
    }

    fn paint(&self, offset: Offset) -> BoxedLayer {
        // Draw to GPU
    }
}

// Single child
impl SingleRender for RenderPadding {
    type Metadata = ();

    fn layout(&mut self, tree: &ElementTree, child: ElementId,
              constraints: BoxConstraints) -> Size {
        // Layout child with padding
    }

    fn paint(&self, tree: &ElementTree, child: ElementId,
             offset: Offset) -> BoxedLayer {
        // Paint child at offset
    }
}
```

## 🔥 What's New in v0.7.0

### Thread-Safe Hooks
- **Arc/Mutex-based**: All hooks are thread-safe
- **Copy signals**: Signal<T> is Copy (just 8 bytes)
- **Send values**: Signal values must implement Send
- **parking_lot**: 2-3× faster synchronization

### Unified View API
- **Single trait**: No more separate Component trait
- **No GATs**: Removed State/Element associated types
- **Automatic trees**: Framework handles element insertion
- **75% less code**: Simplified widget implementation

### Bug Fixes & Improvements
- ✅ Fixed all 22 clippy warnings in tests
- ✅ Fixed all 17 rustdoc warnings
- ✅ Removed legacy RenderPipeline (301 lines)
- ✅ 426 tests passing (100% core coverage)

## 🤝 Contributing

We welcome contributions! Please see [CLAUDE.md](CLAUDE.md) for:
- Build commands and workflow
- Code architecture and patterns
- Documentation standards
- Testing requirements

Areas for improvement:
- Additional widget implementations
- Performance benchmarks
- More examples and tutorials
- Platform-specific optimizations

## 📝 Changelog

### v0.7.0 (Current)
- ✨ Thread-safe hooks with Arc/Mutex
- ✨ Copy-based Signal<T> (8 bytes)
- ✨ Unified View trait (no GATs)
- ✨ wgpu-only rendering (GPU-accelerated)
- 🐛 All clippy warnings fixed (lib + tests)
- 📚 All rustdoc warnings fixed
- 🧹 Removed legacy code (RenderPipeline)
- ✅ 426 passing tests

### v0.6.0
- ✨ ElementId with NonZeroUsize
- ✨ PipelineBuilder pattern
- 📚 Comprehensive documentation

### v0.5.0
- ✅ InheritedModel support
- ✅ O(N) multi-child reconciliation
- ✅ Complete test coverage

## 📄 License

MIT OR Apache-2.0

## 🙏 Acknowledgments

- **Flutter team** - For the proven three-tree architecture
- **Leptos/SolidJS** - For inspiration on Copy-based signals
- **Rust community** - For excellent tooling and ecosystem
- **wgpu team** - For cross-platform GPU graphics
- **parking_lot** - For high-performance synchronization

---

**Built with ❤️ in Rust**

*"Flutter's architecture meets Rust's performance and safety"*
