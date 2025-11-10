# Project Context

## Purpose

**FLUI** is a production-ready, Flutter-inspired declarative UI framework for Rust, featuring the proven three-tree architecture (View → Element → Render) with modern Rust idioms. Built for high-performance GPU-accelerated rendering using wgpu.

**Key Goals:**
- Provide a declarative, composable UI framework inspired by Flutter's architecture
- Enable thread-safe, multi-threaded UI with full `Send + Sync` support
- Deliver GPU-accelerated rendering with wgpu backend for native performance
- Support reactive state management with React-like hooks (signals, memos, effects)
- Maintain type-safe, zero-cost abstractions with minimal runtime overhead
- Target Windows (Tier 1), with potential for cross-platform support

## Tech Stack

### Core Technologies
- **Language:** Rust 1.91.0+ (stable channel)
- **Platform:** `x86_64-pc-windows-msvc` (Tier 1 support)
- **Rendering Backend:** wgpu (GPU-accelerated, Vulkan/Metal/DX12/WebGPU)
- **Text Rendering:** glyphon (GPU text with SDF - Signed Distance Field)
- **Path Tessellation:** lyon (converts vector graphics to triangles)

### Key Dependencies
- **tokio 1.43** - Async runtime
- **parking_lot 0.12** - High-performance RwLock/Mutex (2-3x faster than std, no poisoning)
- **glam 0.30** - Math and geometry (SIMD-optimized)
- **tracing** - Structured logging (NEVER use println!)
- **slab** - Arena allocator for element tree (O(1) insertion/removal)
- **rayon** - Parallel processing (when `parallel` feature enabled)
- **bon** - Builder pattern macro generation

### Workspace Structure
```
crates/
├── flui_types/       - Core types (Color, Size, Rect, EdgeInsets, etc.)
├── flui_painting/    - Painting primitives (Path, Shape, Border, Image)
├── flui_engine/      - Rendering engine (Painter, Layer system, wgpu backend)
├── flui_core/        - Core framework (View trait, Element tree, Pipeline, Hooks)
├── flui_rendering/   - RenderObject implementations (layout/paint logic)
├── flui_widgets/     - Widget library (Column, Row, Text, Container, etc.)
└── flui_app/         - Application framework (AppRunner, WindowManager)
```

## Project Conventions

### Code Style

**Formatting:**
- Use `cargo fmt --all` for consistent formatting
- Line length: 100 characters (default rustfmt)
- 4-space indentation
- Use trailing commas in multi-line expressions

**Naming Conventions:**
- **Types:** `PascalCase` (e.g., `RenderBox`, `ElementTree`, `BuildContext`)
- **Functions/Methods:** `snake_case` (e.g., `flush_build`, `request_layout`)
- **Constants:** `SCREAMING_SNAKE_CASE` (e.g., `DEFAULT_WINDOW_SIZE`)
- **Type Parameters:** Single uppercase letter or descriptive (e.g., `T`, `V: View`, `R: Render`)

**Documentation:**
- Public APIs MUST have doc comments (`///`)
- Use `# Examples` sections for non-trivial APIs
- Use `# Safety` sections for unsafe code
- Include doc tests for public interfaces

**Linting:**
- Run `cargo clippy --workspace -- -D warnings` before committing
- Fix all warnings (treat warnings as errors)
- Use `#[allow(clippy::...)]` sparingly with justification

### Architecture Patterns

#### Three-Tree Architecture (Core Design)

```
View Tree (Immutable) → Element Tree (Mutable) → Render Tree (Layout/Paint)
```

**View Tree (Immutable):**
- Views implement the unified `View` trait
- Single `build()` method returns `impl IntoElement`
- **NO GATs** - State/Element types removed in v0.6.0
- Views must be `'static` but NOT necessarily `Clone`
- Located in: `crates/flui_core/src/view/`

**Element Tree (Mutable):**
- Stored in `Slab` arena at `crates/flui_core/src/element/element_tree.rs`
- Three variants: `Component`, `Render`, `Provider`
- ElementId uses `NonZeroUsize` for niche optimization (Option<ElementId> = 8 bytes)
- **CRITICAL:** Slab indices are 0-based but ElementId is 1-based (+1 offset in insert, -1 in get)
- Lifecycle: Initial → Active → Inactive → Defunct

**Render Tree (Layout/Paint):**
- Three render traits based on child count:
  - `LeafRender` (0 children) - e.g., Text, Image
  - `SingleRender` (1 child) - e.g., Padding, Center
  - `MultiRender` (N children) - e.g., Column, Row, Stack
- Uses GAT (Generic Associated Types) for type-safe metadata
- Located in: `crates/flui_rendering/src/objects/`

#### Pipeline Architecture

Three phases coordinated by `PipelineOwner`:

1. **Build Phase:** `flush_build()` - Rebuilds dirty components
2. **Layout Phase:** `flush_layout(constraints)` - Computes sizes
3. **Paint Phase:** `flush_paint()` - Generates layers

**CRITICAL BUG PATTERN:** When calling `request_layout()`, set BOTH:
1. Mark in dirty set via `coordinator.layout_mut().mark_dirty(node_id)`
2. Set RenderState flag via `render_state.mark_needs_layout()`

#### Thread-Safety Design

- **All hooks use Arc/Mutex** (parking_lot) instead of Rc/RefCell
- Signal values must implement `Send`
- All callbacks must be `Send + Sync`
- Thread-local BuildContext via RAII guards
- Element enum is 3.75x faster than `Box<dyn>` trait objects

#### Modern View API (v0.6.0+)

```rust
// Unified View trait (no GATs, no rebuild)
pub trait View: 'static {
    fn build(self, ctx: &BuildContext) -> impl IntoElement;
}

// IntoElement types:
// - (LeafRender, ()) → LeafRenderBuilder
// - (SingleRender, Option<child>) → SingleRenderBuilder
// - (MultiRender, Vec<child>) → MultiRenderBuilder
// - AnyElement → For heterogeneous view storage
```

**Benefits:** 75% less boilerplate, automatic tree insertion, no manual rebuilds

#### State Management with Hooks

**Hook Rules (MUST follow - breaking causes PANICS):**
1. ✅ Always call hooks in the same order every build
2. ❌ Never call hooks conditionally
3. ❌ Never call hooks in loops with variable iterations
4. ✅ Only call hooks at component top level
5. ✅ Clone signals before moving into closures

**Available Hooks:**
- `use_signal(ctx, initial)` - Reactive state
- `use_memo(ctx, compute_fn)` - Derived state
- `use_effect(ctx, effect_fn)` - Side effects

### Testing Strategy

**Test Organization:**
- Unit tests: `#[cfg(test)] mod tests` at bottom of each file
- Integration tests: `tests/` directory in each crate
- Examples: `examples/` directory demonstrating real usage

**Running Tests:**
```bash
# All tests
cargo test --workspace

# Specific crate
cargo test -p flui_core

# With logging
RUST_LOG=debug cargo test -p flui_core
```

**Benchmarks:**
```bash
cargo bench -p flui_core
cargo bench -p flui_types
```

**Test Coverage Requirements:**
- Public APIs must have basic tests
- Critical paths (build/layout/paint pipeline) must have integration tests
- Hook usage must follow rules documented in `crates/flui_core/src/hooks/RULES.md`

### Git Workflow

**Branch Structure:**
- `main` - Primary development branch (stable)
- Feature branches as needed (short-lived)

**Commit Message Format:**
Use conventional commits with co-authorship:

```bash
<type>: <description>

<body (optional)>

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>
```

**Types:**
- `feat:` - New feature
- `fix:` - Bug fix
- `refactor:` - Code refactoring (no behavior change)
- `perf:` - Performance improvement
- `docs:` - Documentation only
- `test:` - Adding/updating tests
- `chore:` - Maintenance (dependencies, tooling)

**Example:**
```bash
git commit -m "$(cat <<'EOF'
feat: Add GPU-accelerated text rendering with glyphon

- Implement SDF text rendering pipeline
- Add text layout cache for performance
- Support custom fonts and fallback chains

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

## Domain Context

### Flutter-Inspired Architecture

FLUI closely follows Flutter's proven architecture patterns:

1. **Widget Tree → View Tree**
   - Immutable, declarative widget descriptions
   - Rebuild on state changes

2. **Element Tree → Element Tree**
   - Mutable, long-lived tree instances
   - Manages widget lifecycle and state

3. **RenderObject Tree → Render Tree**
   - Handles layout and painting
   - Optimized for performance

### Key Differences from Flutter

- **Thread-Safe by Default:** All state uses Arc/Mutex (not single-threaded like Dart)
- **No Garbage Collection:** Rust's ownership prevents memory leaks
- **Zero-Cost Abstractions:** No runtime overhead from framework
- **GPU-Only Rendering:** wgpu backend (no CPU fallback like Skia)
- **Static Typing:** Full type safety at compile time

### Rendering Concepts

**Tessellation:**
- All 2D shapes converted to triangles via lyon library
- GPU renders triangles efficiently
- No "Quad" or "Rectangle" primitives at GPU level

**Layer System:**
- Composable layers (Picture, Transform, Clip, etc.)
- Build paint command tree
- Submit to GPU in single pass

**Painter V2 Architecture (Approved for 0.7.0):**
- Three-level trait hierarchy: `PainterBackend → PainterShapes → Painter`
- Minimal core: Only 5 methods required for backends
- Auto-implemented high-level APIs
- Optimization hints for backend-specific performance

### Performance Characteristics

- **GPU-Accelerated:** wgpu provides native GPU performance
- **Buffer Pooling:** Reuses GPU buffers across frames
- **Element enum:** 3.75x faster than trait objects
- **Niche optimization:** Option<ElementId> has zero overhead (8 bytes)
- **parking_lot:** 2-3x faster than std::sync primitives

## Important Constraints

### Technical Constraints

1. **Thread-Safety Required:**
   - All types crossing thread boundaries must be `Send + Sync`
   - All hooks use Arc/Mutex (never Rc/RefCell)
   - BuildContext is read-only during build (enables parallel builds)

2. **GPU-Only Rendering:**
   - Must support wgpu backend (Vulkan/Metal/DX12/WebGPU)
   - No CPU-only fallback
   - Text rendering via glyphon (GPU SDF)

3. **Zero-Cost Abstractions:**
   - Framework overhead must be minimal
   - Prefer static dispatch over dynamic dispatch
   - Use arena allocation (Slab) for element tree

4. **Logging Policy:**
   - **ALWAYS use tracing** - NEVER println! or eprintln!
   - INFO level for user-facing messages
   - DEBUG level for development debugging
   - Controllable via RUST_LOG environment variable

5. **Const Where Possible:**
   - Mark constructors as const (Rust 1.91.0+ support)
   - Enable compile-time default values
   - Example: `Paint::BLACK`, `Paint::WHITE`

### Platform Constraints

- **Primary Platform:** Windows 11 (x86_64-pc-windows-msvc)
- **Target:** Tier 1 support with DirectX 12
- **Future:** Consider ARM64 support (aarch64-pc-windows-msvc is now Tier 1)

### Breaking Changes Policy

- Major architectural improvements are acceptable (even with breaking changes)
- Prioritize correctness and quality over backwards compatibility
- Document breaking changes in migration guides
- Current version: v0.6.0+ (New View API)
- Next major: v0.7.0 (Painter V2 architecture)

## External Dependencies

### Critical Dependencies

**wgpu** - GPU rendering abstraction
- Provides cross-platform GPU API
- Supports Vulkan, Metal, DirectX 12, WebGPU
- Used in: `crates/flui_engine/src/backends/wgpu/`

**lyon** - Path tessellation
- Production-ready triangle tessellation for 2D shapes
- Converts circles, rounded rectangles, paths to triangles
- Used in: `crates/flui_engine/src/painter/tessellator.rs`

**glyphon** - GPU text rendering
- SDF (Signed Distance Field) text rendering
- High-quality scaling and antialiasing
- Used in: `crates/flui_engine/src/painter/text.rs`

**parking_lot** - High-performance synchronization
- RwLock/Mutex 2-3x faster than std
- No poisoning (simpler error handling)
- Used throughout: hooks, pipeline, element tree

### Optional Dependencies

**rayon** - Parallel processing (feature: "parallel")
- Enables parallel build pipeline
- Thread-safe architecture required
- Status: ✅ Stable

### No External Services

- FLUI is a local framework (no network dependencies)
- No cloud services, APIs, or remote data sources
- All rendering happens locally via GPU

## Reference Documentation

### Key Documents

- **CLAUDE.md** - Main AI assistant guidelines
- **PAINTER_ARCHITECTURE_V2.md** - Approved Painter design (v0.7.0)
- **RUST_1.91.0_FEATURES.md** - Relevant Rust features for FLUI
- **rust-toolchain.toml** - Toolchain configuration
- **docs/PIPELINE_ARCHITECTURE.md** - Pipeline design
- **docs/FINAL_ARCHITECTURE_V2.md** - Overall architecture
- **crates/flui_core/src/hooks/RULES.md** - Hook usage rules (MUST READ)

### Examples

- **simplified_view.rs** - Modern View API demonstration
- **thread_safe_hooks.rs** - Thread-safety demonstration
- Located in: `crates/flui_core/examples/`
