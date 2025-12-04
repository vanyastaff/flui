<!-- OPENSPEC:START -->
# OpenSpec Instructions

These instructions are for AI assistants working in this project.

Always open `@/openspec/AGENTS.md` when the request:
- Mentions planning or proposals (words like proposal, spec, change, plan)
- Introduces new capabilities, breaking changes, architecture shifts, or big performance/security work
- Sounds ambiguous and you need the authoritative spec before coding

Use `@/openspec/AGENTS.md` to learn:
- How to create and apply change proposals
- Spec format and conventions
- Project structure and guidelines

Keep this managed block so 'openspec update' can refresh the instructions.

<!-- OPENSPEC:END -->

# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## AI Assistant Guidelines

### Context7 MCP Integration

**ALWAYS use Context7 MCP tools** when you need:
- Code generation involving external libraries
- Setup or configuration steps for dependencies
- Library or API documentation
- Understanding how to use a specific crate or API

**Automatically use Context7 tools to:**
1. Resolve library IDs using `mcp__context7__resolve_library_id`
2. Get library documentation using `mcp__context7__get_library_docs`

**You should do this proactively** without waiting for the user to explicitly ask. When the user mentions a library, crate, or API, immediately fetch its documentation.

## Project Overview

FLUI is a modular, Flutter-inspired declarative UI framework for Rust, featuring the proven three-tree architecture (View → Element → Render) with modern Rust idioms. Built with wgpu for high-performance GPU-accelerated rendering and structured as a collection of focused, composable crates.

**Key Architecture:**
```
View Tree (immutable) → Element Tree (mutable) → Render Tree (layout/paint)
```

**Modular Design:** FLUI is organized into 20+ specialized crates:
- **Foundation Layer:** `flui_types`, `flui-foundation`, `flui-tree`
- **Framework Layer:** `flui-view`, `flui-pipeline`, `flui-reactivity`, `flui-scheduler`, `flui_core`
- **Rendering Layer:** `flui_painting`, `flui_engine`, `flui_rendering`
- **Widget Layer:** `flui_widgets`, `flui_animation`, `flui_interaction`
- **Application Layer:** `flui_app`, `flui_assets`
- **Development Tools:** `flui_devtools`, `flui_cli`, `flui_build`

**Thread-Safety:** FLUI is fully thread-safe with reactive state management using Copy-based signals and parking_lot synchronization.

## Essential Build Commands

### Quick Commands

```bash
# Build entire workspace
cargo build --workspace

# Run tests
cargo test --workspace

# Run specific example
cargo run --example counter_reactive

# Lint and format
cargo clippy --workspace -- -D warnings
cargo fmt --all

# Cross-platform builds (Android, Web, Desktop)
cargo xtask build android --release
cargo xtask build web --release
cargo xtask build desktop --release
```

### Building Individual Crates (Dependency Order)

When making structural changes, build crates in dependency order:

```bash
# Foundation Layer (no dependencies)
cargo build -p flui_types
cargo build -p flui-foundation
cargo build -p flui-tree

# Framework Layer (depends on foundation)
cargo build -p flui-view
cargo build -p flui-pipeline
cargo build -p flui-reactivity
cargo build -p flui-scheduler
cargo build -p flui_core

# Rendering Layer (depends on framework)
cargo build -p flui_painting
cargo build -p flui_engine
cargo build -p flui_rendering

# Widget & Application Layer (depends on rendering)
cargo build -p flui_widgets
cargo build -p flui_app
```

### Running Tests

```bash
# Test all crates
cargo test --workspace

# Test specific crate
cargo test -p flui-foundation
cargo test -p flui-reactivity
cargo test -p flui_core

# Run a single test by name
cargo test -p flui_rendering test_layout_constraints

# Run tests with output (nocapture)
cargo test -p flui_core -- --nocapture

# Run with logging
RUST_LOG=debug cargo test -p flui_core
```

### Useful Cargo Aliases

Defined in `.cargo/config.toml`:

```bash
# Cross-platform builds
cargo build-android-release
cargo build-web-release
cargo build-desktop-release

# Development
cargo dev-android --logcat
cargo dev-web

# Quality
cargo lint                    # Run clippy
cargo lint-fix               # Auto-fix clippy warnings
cargo fmt-check              # Check formatting
```

For complete build instructions including cross-platform builds, see **[BUILD.md](BUILD.md)**.

## Code Architecture

### Three-Tree System

**View Tree (Immutable):**
- Views implement traits from `flui-view` crate
- Single `build()` method returns `impl IntoElement`
- Views must be `'static` but NOT necessarily `Clone`

**Element Tree (Mutable):**
- Stored using tree abstractions from `flui-tree` crate
- Element identification and lifecycle managed by `flui-foundation`
- ElementId uses `NonZeroUsize` for niche optimization (Option<ElementId> = 8 bytes)

**Render Tree (Layout/Paint):**
- Arity system for compile-time child count validation: `Leaf` (0), `Single` (1), `Optional` (0-1), `Variable` (N)
- `RenderBox<A>` trait parameterized by arity type for type-safe child access
- Uses GAT (Generic Associated Types) for type-safe metadata
- Pipeline coordination handled by `flui-pipeline` crate

### Element Architecture (v0.7.0)

**Unified Element Struct:**
```rust
pub struct Element {
    // Tree position
    parent: Option<ElementId>,
    children: Vec<ElementId>,

    // Lifecycle
    lifecycle: ElementLifecycle,

    // Type-erased behavior
    view_object: Box<dyn ViewObject>,
}
```

**Key Benefits:**
- ✅ Single struct instead of enum - no dispatch overhead
- ✅ All type-specific behavior in ViewObject trait
- ✅ Extensible - add new view types without changing Element
- ✅ Flutter-like architecture with Rust idioms

### Pipeline Architecture

The rendering pipeline has three phases coordinated by abstract traits from `flui-pipeline`:

1. **Build Phase:** Implements `BuildPhase` trait for widget rebuilds
2. **Layout Phase:** Implements `LayoutPhase` trait for size computation
3. **Paint Phase:** Implements `PaintPhase` trait for layer generation

**Architecture Benefits:**
- **Extensible:** Implement custom pipeline phases
- **Testable:** Abstract traits enable easy mocking
- **Flexible:** Different coordinators for different use cases
- **Thread-safe:** Built-in support for parallel processing

For detailed architecture documentation, see **[docs/arch/](docs/arch/)**.

## Logging and Debugging

### Always Use Tracing

**IMPORTANT:** Always use `tracing` for logging, NEVER use `println!` or `eprintln!`.

FLUI uses **tracing-forest** for hierarchical logging with automatic timing:

```rust
// Initialize at program start (Development mode)
use flui_core::logging::{init_logging, LogConfig, LogMode};

init_logging(LogConfig::new(LogMode::Development));

// Use throughout code with #[instrument] for automatic timing
#[tracing::instrument]
fn render_frame(num: u32) {
    tracing::info!("Starting frame");
    // Spans automatically show [ duration | percentage ]
}

// Or use manual logging
tracing::debug!(count = 1, "Layout complete");
tracing::warn!("Layout phase skipped: needs_layout() returned false");
```

**Output example:**
```
INFO    render_frame [ 11.2ms | 100.00% ] num: 0
INFO    ┝━ build_phase [ 5.18ms | 46.26% ]
DEBUG   │  ┕━ Build complete count: 1
INFO    ┝━ layout_phase [ 3.49ms | 31.16% ]
DEBUG   │  ┕━ Layout complete count: 1
INFO    ┕━ paint_phase [ 2.51ms | 22.40% ]
```

### Cross-Platform Logging (flui_log)

FLUI provides platform-specific logging through the `flui_log` crate:

```rust
use flui_log::{init_logger, LogConfig};

// Initialize platform-specific logger
init_logger(LogConfig::default().with_app_name("MyApp"));

// Use standard tracing macros
tracing::info!("Application started");
```

**Platform Support:**
- **Android**: Redirects to logcat
- **iOS**: Redirects to NSLog/OSLog
- **Desktop/Web**: Uses tracing-subscriber

## Important Codebase Conventions

### BuildContext is Read-Only

`BuildContext` is intentionally read-only during build to enable parallel builds:

```rust
// ✅ Correct - Signal handles rebuild scheduling internally
let signal = use_signal(ctx, 0);
signal.set(42);  // Triggers rebuild via callback

// ❌ Wrong - Don't schedule rebuilds during build
// ctx.schedule_rebuild();  // This method doesn't exist!
```

### Clone is Cheap

Views should be cheap to work with:

```rust
// ✅ Good - cheap to use
struct GoodView {
    text: String,  // String is cheap to move
    data: Arc<Vec<i32>>,  // Arc for shared data
}

// ❌ Bad - expensive clone if derived
#[derive(Clone)]
struct BadView {
    data: Vec<ComplexStruct>,  // Deep clone every frame!
}
```

### ElementId Offset Pattern

**CRITICAL:** Slab uses 0-based indices but ElementId uses 1-based (NonZeroUsize):

```rust
// In element_tree.rs insert():
let id = self.nodes.insert(node);
ElementId::new(id + 1)  // +1 because ElementId uses NonZeroUsize

// In element_tree.rs get():
self.nodes.get(element_id.get() - 1).map(|node| &node.element)  // -1 to access slab
```

## Common Development Patterns

For common development patterns including:
- Creating Views and RenderObjects
- Using reactive state with hooks
- Working with the Transform API
- Advanced visual effects (ShaderMask, BackdropFilter)
- Generic patterns that improve on Flutter's design

See **[docs/arch/PATTERNS.md](docs/arch/PATTERNS.md)**.

## Feature Flags

### Reactive System

```toml
flui-reactivity = { version = "0.1", features = ["hooks", "async"] }
```

**Features:**
- `hooks` - Enable React-style hooks (use_signal, use_effect, etc.)
- `async` - Enable async utilities and resources

### Pipeline System

```toml
flui-pipeline = { version = "0.1", features = ["parallel"] }
```

**Features:**
- `parallel` - Enable rayon-based parallel processing

### Rendering Backend

FLUI uses **wgpu** as its only rendering backend for GPU-accelerated graphics:

```toml
flui = { version = "0.1", features = ["devtools"] }
```

**Features:**
- `devtools` - Enable development and debugging tools
- `full` - Enable all stable features

## Documentation Structure

### Foundation Layer
- **[flui-foundation/README.md](crates/flui-foundation/README.md)** - Core types and change notification
- **[flui-tree/README.md](crates/flui-tree/README.md)** - Tree abstractions and visitor patterns
- **[flui_types/README.md](crates/flui_types/README.md)** - Basic geometry and math

### Framework Layer
- **[flui-view/README.md](crates/flui-view/README.md)** - View traits and abstractions
- **[flui-pipeline/README.md](crates/flui-pipeline/README.md)** - Pipeline coordination system
- **[flui-reactivity/README.md](crates/flui-reactivity/README.md)** - Reactive state management
- **[flui_core/README.md](crates/flui_core/README.md)** - Core framework implementation

### Architecture Documentation
- **[docs/arch/README.md](docs/arch/README.md)** - Overall architecture overview
- **[docs/arch/CORE_ARCHITECTURE.md](docs/arch/CORE_ARCHITECTURE.md)** - Core framework design
- **[docs/arch/RENDERING_ARCHITECTURE.md](docs/arch/RENDERING_ARCHITECTURE.md)** - Rendering system

### Development Tools
- **[crates/flui_cli/README.md](crates/flui_cli/README.md)** - CLI tool documentation
- **[crates/flui_assets/README.md](crates/flui_assets/README.md)** - Asset management system

## Git Workflow

### Commit Message Format

Use conventional commits:

```bash
git commit -m "feat: Add new widget for user profiles

- Implement ProfileCard view
- Add avatar support with image loading
- Add responsive layout for mobile/desktop"
```

### Branch Structure

- `main` - Primary development branch
- Feature branches as needed

## Key Dependencies

- **wgpu 25.x** - Cross-platform GPU API (Vulkan/Metal/DX12/WebGPU). **Note:** Stay on 25.x; wgpu 26.0+ has compilation issues with codespan-reporting
- **parking_lot 0.12** - High-performance RwLock/Mutex (2-3x faster than std)
- **tokio 1.43** - Async runtime (LTS until March 2026)
- **tracing** - Structured logging (always use this, never println!)
- **glam 0.30** - Math and geometry
- **lyon** - Path tessellation
- **glyphon** - GPU text rendering

## Performance Considerations

- **GPU-Accelerated Rendering**: wgpu provides native GPU performance on all platforms
- **Copy-based Signals**: Zero-cost reactive primitives with DashMap for lock-free access
- **Niche Optimization**: Option<ElementId> = 8 bytes (NonZeroUsize)
- **parking_lot**: 2-3x faster than std sync primitives
- **Slab**: O(1) insertion/removal with cache-friendly contiguous storage

## Known Issues

### Text Rendering Not Visible

If application runs but text doesn't appear:
1. Verify layout phase executes (check `needs_layout()` flag)
2. Ensure `request_layout()` sets both dirty set AND RenderState flag
3. Add tracing to paint pipeline to verify egui shapes are created
4. Check for coordinate system issues (text drawn offscreen)

### Hook Panics

If you get "Hook state type mismatch" panics:
1. Check that hooks are called in the same order every render
2. Never call hooks conditionally (no `if` around hooks)
3. Never call hooks in loops with variable iterations

## Troubleshooting

### wgpu Compilation Issues

If you see errors related to `codespan-reporting` or feature flags when upgrading wgpu:
- Stay on wgpu 25.x (configured in workspace Cargo.toml)
- See: https://github.com/gfx-rs/wgpu/issues/7915

### Build Order Issues

If you encounter confusing type errors after making changes, rebuild crates in dependency order (Foundation → Framework → Rendering → Widget layers) as shown in "Building Individual Crates" section.
