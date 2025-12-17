# FLUI - Modern Rust UI Framework

A production-ready, Flutter-inspired declarative UI framework for Rust, featuring the proven three-tree architecture (View → Element → Render) with modern Rust idioms and GPU-accelerated rendering.

## 🚀 Status: Active Development

- ✅ **Modular architecture** with 20+ specialized crates
- ✅ **Thread-safe reactive system** with signals and hooks
- ✅ **GPU-accelerated rendering** with wgpu backend
- ✅ **Modern pipeline architecture** with abstract traits
- ✅ **Cross-platform support** (Desktop, Mobile, Web)
- ✅ **Production features** (metrics, error recovery, frame scheduling)

## ✨ Latest: v0.1.0 - Modular Architecture & New Pipeline System

### New Modular Architecture
FLUI has been restructured into focused, composable crates:

- **flui-foundation** - Core types and change notification
- **flui-tree** - Tree abstractions and visitor patterns
- **flui-view** - View traits, elements, and BuildContext
- **flui-reactivity** - Signals, hooks, and reactive state management
- **flui-scheduler** - Frame scheduling and task prioritization

### Thread-Safe Reactivity
```rust
use flui_reactivity::{Signal, use_signal, use_effect};
use flui_view::View;

#[derive(Debug)]
struct Counter;

impl View for Counter {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Signal is thread-safe and Copy
        let count = use_signal(ctx, 0);

        // Effects with automatic cleanup
        use_effect(ctx, move |ctx| {
            println!("Count changed: {}", count.get(ctx));
            None // No cleanup needed
        });

        column![
            text(format!("Count: {}", count.get(ctx))),
            button("Increment").on_press(move || {
                count.update(|n| *n + 1);  // Thread-safe!
            })
        ]
    }
}
```

## 🎯 Key Features

### Three-Tree Architecture
```
View Tree (immutable) → Element Tree (mutable) → Render Tree (layout/paint)
```

- **Views**: Lightweight, immutable configuration
- **Elements**: Persistent state and lifecycle management
- **Renders**: Layout calculations and GPU-accelerated painting

### Modern Reactive System

```rust
use flui_reactivity::prelude::*;

// Signal - reactive state (Copy-based, thread-safe)
let count = Signal::new(0);
count.set(42);  // Triggers reactive updates

// Computed - derived state with automatic tracking
let doubled = count.derive(|&n| n * 2);

// Effects - side effects with cleanup
let cleanup = count.watch(|value| {
    println!("Count: {}", value);
});
```

### GPU-Accelerated Rendering

FLUI uses **wgpu** for high-performance, cross-platform graphics:

- **Hardware acceleration**: Native GPU performance on all platforms
- **Modern graphics APIs**: Vulkan, Metal, DX12, WebGPU
- **Efficient tessellation**: lyon for converting vectors to triangles
- **SDF text rendering**: glyphon for high-quality text at any scale

### Production Features

- **Frame scheduling**: Budget management with priority queues
- **Error recovery**: Configurable policies (skip frame, show error, use last good)
- **Performance metrics**: FPS tracking, frame times, cache statistics
- **Lock-free operations**: Atomic dirty tracking, triple buffering
- **Parallel processing**: Multi-threaded builds with rayon

## 🏗️ Project Structure

```
flui/
├── crates/
│   # Foundation Layer
│   ├── flui_types/              # Basic geometry and math types
│   ├── flui-foundation/         # Core types, change notification, diagnostics
│   ├── flui-tree/              # Tree abstractions and visitor patterns
│   
│   # Framework Layer  
│   ├── flui-view/              # View traits, elements, and BuildContext
│   ├── flui-reactivity/        # Signals, hooks, reactive state
│   ├── flui-scheduler/         # Frame scheduling and task prioritization
│   ├── flui_core/              # Core framework implementation
│   
│   # Rendering Layer
│   ├── flui_painting/          # 2D graphics primitives
│   ├── flui_engine/            # wgpu rendering engine
│   ├── flui_rendering/         # RenderObject implementations
│   
│   # Widget Layer
│   ├── flui_widgets/           # Widget library (Text, Container, etc.)
│   ├── flui_animation/         # Animation system
│   ├── flui_interaction/       # Event handling and gestures
│   
│   # Application Layer
│   ├── flui_app/               # Application framework
│   ├── flui_assets/            # Asset management (images, fonts)
│   
│   # Development Tools
│   ├── flui_devtools/          # Development and debugging tools
│   ├── flui_cli/               # CLI for project management
│   ├── flui_build/             # Cross-platform build system
│   └── flui_log/               # Cross-platform logging
│   
├── examples/                   # Application examples
├── demos/                      # Demo applications
├── docs/                       # Documentation
└── platforms/                  # Platform-specific code
```

## 🚀 Getting Started

### Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
# Core framework
flui_core = "0.1"
flui_widgets = "0.1"

# Reactive state management
flui-reactivity = { version = "0.1", features = ["hooks"] }

# Optional: Asset management
flui_assets = { version = "0.1", features = ["images"] }
```

### Hello World

```rust
use flui_core::prelude::*;
use flui_widgets::Text;
use flui-view::View;

#[derive(Debug)]
struct HelloWorld;

impl View for HelloWorld {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        Text::new("Hello, FLUI!")
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut pipeline = PipelineOwner::new();
    let root = HelloWorld.into_element();
    pipeline.set_root(root);

    // Render loop
    loop {
        let layer = pipeline.build_frame(constraints)?;
        present(layer)?;
    }
}
```

### Counter with Reactive State

```rust
use flui_core::prelude::*;
use flui-reactivity::{use_signal, use_effect};
use flui_widgets::{Column, Text, Button};

#[derive(Debug)]
struct Counter;

impl View for Counter {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let count = use_signal(ctx, 0);

        // Side effect for logging
        use_effect(ctx, move |ctx| {
            println!("Count updated: {}", count.get(ctx));
            None // No cleanup
        });

        Column::new()
            .children(vec![
                Box::new(Text::new(format!("Count: {}", count.get(ctx)))),
                Box::new(Button::new("Increment")
                    .on_pressed(move || count.update(|n| *n + 1))),
                Box::new(Button::new("Decrement")
                    .on_pressed(move || count.update(|n| *n - 1))),
            ])
    }
}
```

## 📖 Examples

### Run Examples

```bash
# Core examples
cargo run --example hello_world_view      # Basic hello world
cargo run --example counter_reactive      # Reactive counter
cargo run --example todo_app              # Todo application

# Pipeline examples
cargo run --example custom_pipeline       # Custom pipeline implementation
cargo run --example parallel_builds       # Multi-threaded builds

# Rendering examples
cargo run --example custom_render         # Custom RenderObject
cargo run --example animation_demo        # Animation system
```

## 🧪 Testing

```bash
# Build workspace (dependency order matters)
cargo build --workspace

# Run all tests
cargo test --workspace

# Test specific layers
cargo test -p flui-foundation
cargo test -p flui-reactivity  
cargo test -p flui_core

# Check documentation
cargo doc --workspace --no-deps

# Run clippy
cargo clippy --workspace -- -D warnings

# Format code
cargo fmt --all
```

## 📚 Documentation

### Essential Reading

- **[CLAUDE.md](CLAUDE.md)** - Development guidelines and build commands
- **[docs/arch/README.md](docs/arch/README.md)** - Architecture overview

### Foundation Layer

- **[flui-foundation](crates/flui-foundation/README.md)** - Core types and change notification
- **[flui-tree](crates/flui-tree/README.md)** - Tree abstractions and visitor patterns
- **[flui_types](crates/flui_types/README.md)** - Basic geometry and math

### Framework Layer

- **[flui-view](crates/flui-view/README.md)** - View traits, elements, and BuildContext
- **[flui-reactivity](crates/flui-reactivity/README.md)** - Reactive state management
- **[flui_core](crates/flui_core/README.md)** - Core framework implementation

### Rendering & Widgets

- **[flui_engine](crates/flui_engine/README.md)** - wgpu rendering engine
- **[flui_widgets](crates/flui_widgets/README.md)** - Widget library
- **[flui_rendering](crates/flui_rendering/README.md)** - RenderObject implementations

### Development Tools

- **[flui_cli](crates/flui_cli/README.md)** - CLI tool for project management
- **[flui_devtools](crates/flui_devtools/README.md)** - Development and debugging
- **[flui_assets](crates/flui_assets/README.md)** - Asset management system

## 🔧 Feature Flags

```toml
# Reactive system with hooks
flui-reactivity = { version = "0.1", features = ["hooks", "async"] }

# Asset management
flui_assets = { version = "0.1", features = ["images", "network", "hot-reload"] }

# Serialization support
flui-foundation = { version = "0.1", features = ["serde"] }

# Development tools
flui = { version = "0.1", features = ["devtools"] }
```

## 📊 Performance

### Memory Efficiency
- **ElementId**: 8 bytes with niche optimization
- **Signal<T>**: Copy-based, just an ID reference
- **Lock-free operations**: Atomic dirty tracking, triple buffering

### Concurrency  
- **parking_lot**: 2-3× faster than std sync primitives
- **DashMap**: Lock-free concurrent HashMap for signal storage
- **Rayon**: Optional parallel processing for builds

### GPU Acceleration
- **wgpu**: Cross-platform GPU API (Vulkan/Metal/DX12/WebGPU)
- **lyon**: Efficient tessellation to triangles
- **glyphon**: SDF-based text rendering

## 🛠️ Architecture Highlights

### Modular Design

Each crate has a specific responsibility:

- **Foundation**: Minimal dependencies, core abstractions
- **Tree**: Visitor patterns, tree traversal algorithms  
- **View**: View traits, elements, and BuildContext
- **Reactivity**: Signals, hooks, state management
- **Core**: Concrete implementations of abstractions

### Thread-Safe Reactivity

```rust
use flui_reactivity::{Signal, use_signal, batch};

// Signals are Copy and thread-safe
let signal = Signal::new(42);
let signal_copy = signal; // No .clone() needed

// Batch updates for performance
batch(|| {
    signal.set(1);
    signal.set(2); 
    signal.set(3);
}); // Only triggers one update
```

## 🔥 What's New in v0.1.0

### Major Architectural Changes
- ✨ **Modular crate structure** - 20+ focused crates
- ✨ **Abstract pipeline traits** - Extensible phase system
- ✨ **Copy-based signals** - Thread-safe reactive primitives
- ✨ **Foundation layer** - Minimal-dependency core types

### New Crates
- ✨ **flui-foundation** - Core types and diagnostics
- ✨ **flui-tree** - Tree abstractions and visitors
- ✨ **flui-view** - View traits, elements, and BuildContext
- ✨ **flui-reactivity** - Comprehensive reactive system
- ✨ **flui-scheduler** - Frame scheduling and prioritization

### Enhanced Developer Experience
- ✅ **Better separation of concerns** - Each crate has clear purpose
- ✅ **Flexible architecture** - Implement custom phases and coordinators
- ✅ **Comprehensive documentation** - Each crate fully documented
- ✅ **Testing utilities** - Built-in test harness for reactive components

## 🤝 Contributing

We welcome contributions! Please see [CLAUDE.md](CLAUDE.md) for:
- Build commands and development workflow
- Code architecture and design patterns
- Documentation standards
- Testing requirements

Areas for improvement:
- Widget library expansion
- Platform-specific optimizations
- Performance benchmarks
- More examples and tutorials

## 📝 Changelog

### v0.1.0 (Current)
- ✨ Complete architectural restructure into modular crates
- ✨ Abstract pipeline traits for extensibility
- ✨ Copy-based Signal<T> with thread safety
- ✨ Foundation layer with minimal dependencies
- ✨ Comprehensive reactive system with hooks
- ✨ Frame scheduling and task prioritization
- 📚 Complete documentation for all crates
- ✅ Modular testing with focused test suites

## 📄 License

MIT OR Apache-2.0

## 🙏 Acknowledgments

- **Flutter team** - For the proven three-tree architecture
- **Leptos/SolidJS** - For inspiration on Copy-based signals and fine-grained reactivity
- **React team** - For the hooks pattern and component lifecycle concepts
- **Rust community** - For excellent tooling and ecosystem
- **wgpu team** - For cross-platform GPU graphics
- **parking_lot** - For high-performance synchronization

---

**Built with ❤️ in Rust**

*"Modular architecture meets reactive performance"*