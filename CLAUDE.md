# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

FLUI is a modular, Flutter-inspired declarative UI framework for Rust, featuring the proven three-tree architecture (View → Element → Render) with modern Rust idioms. Built with wgpu for high-performance GPU-accelerated rendering.

**Key Architecture:**
```
View Tree (immutable) → Element Tree (mutable) → Render Tree (layout/paint)
```

**Modular Design:** 20+ specialized crates organized in layers:
- **Foundation:** `flui-types`, `flui-foundation`, `flui-tree`, `flui-platform`
- **Framework:** `flui-view`, `flui-reactivity`, `flui-scheduler`, `flui_core`
- **Rendering:** `flui-painting`, `flui-engine`, `flui-rendering`
- **Widget:** `flui-widgets`, `flui-animation`, `flui-interaction`
- **Application:** `flui-app`, `flui-assets`, `flui-log`
- **Tools:** `flui-devtools`, `flui-cli`, `flui-build`
- **Layer System:** `flui-layer` (compositing), `flui-semantics` (accessibility)

### Current Development Focus

**IMPORTANT:** Workspace in platform integration phase. Many high-level crates temporarily disabled in `Cargo.toml`.

**Active crates:**
- Foundation: `flui-types`, `flui-foundation`, `flui-tree`
- Platform: `flui-platform` (MVP development - cross-platform support)
- Core: `flui-layer`, `flui-semantics`, `flui-interaction`, `flui-painting`
- Framework: `flui-scheduler`, `flui-engine`, `flui-log`, `flui-app`
- Tools: `flui-build` (async PlatformBuilder)

**Temporarily disabled until integration complete:**
- `flui-rendering`, `flui-view`, `flui-animation`, `flui-reactivity`, `flui-devtools`, `flui-cli`

**Current Priority**: Complete flui-platform MVP

## Constitution Compliance

**CRITICAL:** All work must align with `.specify/memory/constitution.md` (v2.2.0). Key principles:

1. **Flutter as Reference, Not Copy** - Adapt Flutter patterns to Rust idioms, no Dart translation
2. **Strict Crate Dependency DAG** - Dependencies flow downward only, no circular deps
3. **Zero Unsafe in Widget/App Layer** - `unsafe` only in `flui-platform`, `flui-painting`, `flui-engine`
4. **Composition Over Inheritance** - Traits + generics + enum dispatch, not `dyn` by default
5. **Declarative API, Imperative Internals** - `build()` is pure, internals optimize freely
6. **No `unwrap()`/`println!`/`dbg!`** - Use `thiserror`/`anyhow` for errors, `tracing` for logging
7. **On-demand Rendering** - `ControlFlow::Wait`, render only when dirty, 60fps target
8. **Coverage Requirements** - Core ≥80%, Platform ≥70%, Widget ≥85%
9. **ID Offset Pattern** - Slab 0-based, IDs 1-based (NonZeroUsize)

## Essential Build Commands

```bash
# Workspace commands
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all

# Single crate
cargo check -p flui-types
cargo test -p flui-tree
cargo test -p flui-tree test_name -- --nocapture
RUST_LOG=debug cargo test -p flui-platform

# Dependency build order (Foundation → Core → Rendering)
cargo build -p flui-types
cargo build -p flui-foundation
cargo build -p flui-tree
cargo build -p flui-platform

# Clean build
cargo clean && cargo build --workspace

# Check workspace membership
cargo metadata --format-version 1 | grep -A 5 "workspace_members"
```

## Code Architecture

### Three-Tree System

**View Tree:** Immutable configuration, implements `View` trait, single `build()` method
**Element Tree:** Mutable state, stored in Slab, `ElementId` uses NonZeroUsize (Option<ElementId> = 8 bytes)
**Render Tree:** Layout/paint, arity-based type safety: `Leaf`, `Single`, `Optional`, `Variable`

### Render Tree Architecture

```rust
pub trait RenderObject {
    fn attach(&mut self, owner: PipelineOwner);
    fn detach(&mut self);
    fn mark_needs_layout(&mut self);
    fn layout(&mut self, constraints: Constraints);
    fn paint(&self, context: &mut PaintContext);
}

pub trait RenderBox<A: Arity>: RenderObject {
    fn perform_layout(&mut self);
    fn compute_intrinsic_size(&self, axis: Axis) -> f32;
}
```

**Arity System** (type-safe child count):
- `Leaf` - No children (Text, Image)
- `Single` - Exactly one child (Center, Padding)
- `Optional` - Zero or one child (Container)
- `Variable` - N children (Row, Column, Stack)

```rust
pub struct RenderPadding {
    child: BoxChild<Single>,     // Type-safe single child
}
pub struct RenderFlex {
    children: BoxChild<Variable>, // Type-safe variable children
}
```

### Pipeline Architecture

Three phases: **Build** → **Layout** → **Paint**

- `BuildPhase` - Widget rebuilds via `BuildOwner`
- `LayoutPhase` - Size computation via `RenderTree`
- `PaintPhase` - Layer generation via `PaintContext`

### RenderTree Lifecycle

**Setup:** Create → `attach(owner)` → Set `parent_data`
**Layout:** Check `needs_layout()` → `layout(constraints)` → `perform_layout()` → Clear flag
**Paint:** `paint(context)` → Generate layers → Compositor
**Teardown:** `detach()` → Drop

### Platform Abstraction (flui-platform)

Cross-platform window management, text systems, and event handling:

```rust
pub trait Platform {
    fn run(&self, ready: Box<dyn FnOnce()>);
    fn open_window(&self, options: WindowOptions) -> Result<Box<dyn PlatformWindow>>;
    fn text_system(&self) -> Arc<dyn PlatformTextSystem>;
    fn background_executor(&self) -> Arc<dyn PlatformExecutor>;
    fn clipboard(&self) -> Arc<dyn PlatformClipboard>;
}

let platform = current_platform(); // Windows, macOS, or Headless
```

**Implementations:**
- **WindowsPlatform** - Native Win32 API
- **MacOSPlatform** - Native AppKit/Cocoa
- **HeadlessPlatform** - Testing/CI without GPU
- **WinitPlatform** - Cross-platform fallback (in progress)

**Key patterns:** Callback registry (`on_quit()`, `on_window_event()`), type erasure (`Box<dyn PlatformWindow>`), interior mutability (`Arc<Mutex<T>>`), W3C events via `ui-events` crate.

### Build System (flui-build)

Async cross-platform build pipeline with sealed `PlatformBuilder` trait:

```rust
#[allow(async_fn_in_trait)]
pub trait PlatformBuilder: private::Sealed + Send + Sync {
    async fn build_rust(&self, ctx: &BuilderContext) -> BuildResult<BuildArtifacts>;
    async fn build_platform(&self, ctx: &BuilderContext, artifacts: &BuildArtifacts) -> BuildResult<FinalArtifacts>;
    async fn clean(&self, ctx: &BuilderContext) -> BuildResult<()>;
}
```

Builders: `AndroidBuilder`, `IosBuilder`, `DesktopBuilder`, `WebBuilder`. Uses typestate pattern (`BuilderContextBuilder<P, Pr>`) for compile-time validation.

## Important Codebase Conventions

### ID Offset Pattern

**CRITICAL:** Slab uses 0-based indices, IDs use 1-based (NonZeroUsize):

```rust
let slab_index = self.nodes.insert(node);
let id = ElementId::new(slab_index + 1)  // +1 for NonZeroUsize
self.nodes.get(element_id.get() - 1)     // -1 to get slab index
```

Applies to: `ViewId`, `ElementId`, `RenderId`, `LayerId`, `SemanticsId`.

### Ambassador Delegation Pattern

```rust
#[delegatable_trait]
pub trait RenderObject { fn mark_needs_layout(&mut self); }

#[derive(Delegate)]
#[delegate(RenderObject, target = "child")]
pub struct RenderPadding {
    child: BoxChild<Single>,  // Delegates through BoxChild
}
```

### Logging

**CRITICAL:** Always use `tracing`, NEVER `println!` or `eprintln!`.

```rust
tracing_subscriber::registry()
    .with(ForestLayer::default())
    .init();

#[tracing::instrument]
fn render_frame(num: u32) {
    tracing::info!("Starting frame");
}
```

## Reference Sources

**Flutter (`.flutter/`)** - UI framework architecture reference:
- `.flutter/src/rendering/` - RenderObject implementations
- `.flutter/src/widgets/` - Widget and Element implementations
- **Usage:** Three-tree architecture, widget patterns, layout algorithms

**GPUI (`.gpui/`)** - Rust UI framework and platform patterns reference:
- `.gpui/src/platform/` - Platform abstraction (Windows, macOS, Linux)
- `.gpui/src/platform.rs` - Platform trait design
- `.gpui/src/window.rs` - Window management, `.gpui/src/executor.rs` - Async executor
- **Usage:** Platform trait patterns, callback registry, type erasure, interior mutability

**When to reference:**
- Check Flutter for UI architecture and widget patterns
- Check GPUI for platform abstraction and Rust-specific patterns
- Adapt both to FLUI's type-safe idioms (Arity system, Ambassador delegation, no nullability)

## Skills (`.claude/skills/`)

These are Rust-focused audit and optimization skills (user-invocable via `/skill-name`):

- `/rust` - Performance optimization (memory, ownership, iterators, async)
- `/rust-idioms` - Idiomatic patterns (types, error handling, traits, modules, conversions)
- `/rust-best-practices` - Apollo GraphQL Rust best practices (borrowing, errors, tests)
- `/rust-async-patterns` - Async programming with Tokio, concurrent patterns
- `/rust-pro` - Production-ready Rust 1.75+ (modern async, type system, systems programming)
- `/rust-systems` - Systems programming patterns (Cargo workspaces, naming, builders)

## Key Dependencies

**Core:** wgpu 25.x (stay on 25.x, 26.0+ broken), parking_lot 0.12, tokio 1.43, tracing, ambassador 0.4.2, slab, bon 3.8

**Platform:** winit 0.30.12, arboard 3.4, windows 0.52 (Win32), cocoa 0.26.0 (AppKit), ui-events (W3C events), keyboard-types

**Engine-only:** glam 0.30, lyon, glyphon, cosmic-text, guillotiere

**Minimum Rust version:** 1.91 (set in workspace `rust-version`)

## Git Workflow

Conventional commits: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`

```bash
git commit -m "feat: Add new widget
- Implementation details"
```

**NEVER do `git checkout`, `git reset`, `git stash`, or any destructive git operations without explicit user permission.**

## Speckit Workflow

**For large changes** (new features, breaking changes, architecture shifts):
1. Check if similar work exists in `specs/` directory
2. Create specification with `/speckit.plan` command
3. Follow spec → plan → tasks → implementation workflow

## Troubleshooting

- "package not found" → Check `Cargo.toml` `[workspace.members]` (many crates disabled)
- "trait not in scope" → Check prelude: `use flui_rendering::prelude::*;`
- "mismatched types" with Arity → Verify `BoxChild<Single>` matches `impl RenderBox<Single>`
- **wgpu issues:** Stay on 25.x (see https://github.com/gfx-rs/wgpu/issues/7915)
