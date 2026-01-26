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

### Constitution Compliance

**CRITICAL:** All work must align with `.specify/memory/constitution.md` (v1.2.0). Key principles:

1. **Test-First for Public APIs** - Write tests BEFORE implementation, verify red state
2. **Type Safety First** - Foundation crates MAY use generics, Application crates MUST use concrete types
3. **Never use println!/eprintln!** - Always use `tracing` for logging
4. **On-demand rendering** - Use `ControlFlow::Wait`, not constant 60 FPS loops
5. **Coverage Requirements**: Core ≥80%, Platform ≥70%, Widget ≥85%
6. **ID Offset Pattern**: Slab uses 0-based, IDs use 1-based (NonZeroUsize)

### OpenSpec Workflow

**For large changes** (new features, breaking changes, architecture shifts):
1. Check if similar work exists in `specs/` or `openspec/changes/`
2. Create proposal with `/openspec:proposal` or `/speckit.plan`
3. Follow spec → plan → tasks → implementation workflow
4. See OpenSpec instructions at top of this file

### MCP Servers

**Context7** - Library documentation (wgpu, lyon, glam, etc.):
- `mcp__context7__resolve_library_id(libraryName)` - Get library ID
- `mcp__context7__get_library_docs(context7CompatibleLibraryID, topic, mode)` - Fetch docs
- Use proactively when external library is mentioned

**Filesystem MCP** - Advanced operations:
- `mcp__filesystem__read_multiple_files` - Batch read (preferred for multiple files)
- `mcp__filesystem__directory_tree` - JSON directory structure
- `mcp__filesystem__search_files` - Glob-based search

**Sequential Thinking** - Complex problem solving:
- `mcp__sequential_thinking__sequentialthinking` - Multi-step reasoning
- Use for architecture decisions, refactorings, debugging

## Project Overview

FLUI is a modular, Flutter-inspired declarative UI framework for Rust, featuring the proven three-tree architecture (View → Element → Render) with modern Rust idioms. Built with wgpu for high-performance GPU-accelerated rendering.

**Key Architecture:**
```
View Tree (immutable) → Element Tree (mutable) → Render Tree (layout/paint)
```

**Modular Design:** 20+ specialized crates organized in layers:
- **Foundation:** `flui_types`, `flui-foundation`, `flui-tree`, `flui-platform`
- **Framework:** `flui-view`, `flui-reactivity`, `flui-scheduler`, `flui_core`
- **Rendering:** `flui_painting`, `flui_engine`, `flui_rendering`
- **Widget:** `flui_widgets`, `flui_animation`, `flui_interaction`
- **Application:** `flui_app`, `flui_assets`, `flui_log`
- **Tools:** `flui_devtools`, `flui_cli`, `flui_build`
- **Layer System:** `flui-layer` (compositing), `flui-semantics` (accessibility)

### Flutter Reference Sources

`.flutter/` directory contains Flutter framework source code for reference:
- `.flutter/src/rendering/` - RenderObject implementations
- `.flutter/src/widgets/` - Widget and Element implementations
- `.flutter/rendering.dart`, `widgets.dart`, `animation.dart` - API overview

**Usage:** Check Flutter's approach before implementing new features, then adapt to Rust idioms (type-safe arity, Ambassador delegation, no nullability).

### Development Specs and Plans

`specs/` directory contains feature specifications and implementation plans:
- `specs/dev/` - Current active development (flui-platform MVP)
  - `spec.md` - Feature specification with user stories and requirements
  - `plan.md` - Implementation plan with phases and technical approach
  - `research.md` - Research findings and architectural decisions
  - `quickstart.md` - Developer quick start guide
  - `tasks.md` - Detailed task breakdown (125 tasks organized by phase)

**When to check specs:**
- Before implementing new features
- When modifying existing platform code
- To understand current priorities (P1 = MVP critical, P2 = important, P3 = nice-to-have)
- For task assignment and parallel work opportunities (marked with `[P]`)

### Current Development Focus

**IMPORTANT:** Workspace in platform integration phase. Many high-level crates temporarily disabled in `Cargo.toml`.

**Active crates (Phase 1-2):**
- Foundation: `flui_types`, `flui-foundation`, `flui-tree`
- Platform: `flui-platform` (MVP development - cross-platform support)
- Core: `flui-layer`, `flui-semantics`, `flui_interaction`, `flui_painting`
- Framework: `flui-scheduler`, `flui_engine`, `flui_log`, `flui_app`

**Temporarily disabled until integration complete:**
- `flui_rendering`, `flui-view`, `flui_animation`, `flui-reactivity`, `flui_widgets`, `flui_devtools`, `flui_cli`, `flui_build`

**Current Priority**: Complete flui-platform MVP (see `specs/dev/` for detailed plan and tasks)

## Essential Build Commands

```bash
# Quick commands
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all
cargo check -p flui_rendering

# Dependency order (Foundation → Core → Rendering)
cargo build -p flui_types
cargo build -p flui-foundation
cargo build -p flui-tree
cargo build -p flui_rendering

# Test specific crate
cargo test -p flui_rendering test_layout_constraints
cargo test -p flui_rendering -- --nocapture
RUST_LOG=debug cargo test -p flui_rendering
```

### Slash Commands

**FLUI-specific:**
```bash
/flui:build-crate <name>    # Build crate with deps
/flui:test-crate <name>     # Test specific crate
/flui:run-example <name>    # Run example with timeout
/flui:check-tree            # Verify three-tree architecture
/flui:deps                  # Analyze dependencies
/flui:lint                  # Lint workspace
/flui:new-widget <name>     # Create new widget
/flui:profile               # Profile performance
/flui:android               # Build for Android
```

**OpenSpec workflow (large changes):**
```bash
/openspec:proposal          # Create change proposal
/openspec:apply <name>      # Apply approved change
/openspec:archive <name>    # Archive deployed change
```

**Speckit workflow (feature planning):**
```bash
/speckit.constitution       # Create/update constitution
/speckit.plan               # Create implementation plan
/speckit.clarify            # Identify spec ambiguities
/speckit.tasks              # Generate task breakdown
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

**Arity System:**
- `Leaf` - No children (Text, Image)
- `Single` - Exactly one child (Center, Padding)
- `Optional` - Zero or one child (Container)
- `Variable` - N children (Row, Column, Stack)

**BoxChild Container:**
```rust
pub struct RenderPadding {
    child: BoxChild<Single>,  // Type-safe single child
}

pub struct RenderFlex {
    children: BoxChild<Variable>,  // Type-safe variable children
}
```

### Pipeline Architecture

Three phases: **Build** → **Layout** → **Paint**

- `BuildPhase` - Widget rebuilds via `BuildOwner`
- `LayoutPhase` - Size computation via `RenderTree`
- `PaintPhase` - Layer generation via `PaintContext`

### Platform Abstraction (flui-platform)

**CRITICAL for MVP**: Cross-platform window management, text systems, and event handling.

**Architecture:**
```rust
// Platform trait with lifecycle and abstractions
pub trait Platform {
    fn run(&self, ready: Box<dyn FnOnce()>);
    fn open_window(&self, options: WindowOptions) -> Result<Box<dyn PlatformWindow>>;
    fn text_system(&self) -> Arc<dyn PlatformTextSystem>;
    fn background_executor(&self) -> Arc<dyn PlatformExecutor>;
    fn clipboard(&self) -> Arc<dyn PlatformClipboard>;
}

// Get platform with automatic detection
let platform = current_platform(); // Windows, macOS, or Headless
```

**Implementations:**
- **WindowsPlatform** - Native Win32 API (100% complete)
- **MacOSPlatform** - Native AppKit/Cocoa (90% complete, needs hardware testing)
- **HeadlessPlatform** - Testing/CI without GPU (100% complete)
- **WinitPlatform** - Cross-platform fallback (in progress)

**Key Patterns:**
- **Callback registry** (GPUI-inspired) - `on_quit()`, `on_window_event()`, `on_reopen()`
- **Type erasure** - `Box<dyn PlatformWindow>`, `Arc<dyn PlatformTextSystem>`
- **W3C events** - Use `ui-events` crate for cross-platform consistency
- **Executor split** - Background (tokio) + Foreground (flume channel)

**Text System Integration:**
- Windows: DirectWrite for font loading, shaping, metrics
- macOS: Core Text equivalent
- Returns glyph positions for flui_painting Canvas API
- Critical blocker for MVP (1-2 weeks estimated)

See `specs/dev/` for detailed platform MVP plan and tasks.

## Logging and Debugging

**CRITICAL:** Always use `tracing`, NEVER `println!` or `eprintln!`.

```rust
// Initialize
use tracing_forest::ForestLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

tracing_subscriber::registry()
    .with(ForestLayer::default())
    .init();

// Use #[instrument] for automatic timing
#[tracing::instrument]
fn render_frame(num: u32) {
    tracing::info!("Starting frame");
}

// Manual logging
tracing::debug!(count = 1, "Layout complete");
tracing::warn!("Layout phase skipped");
```

## Important Codebase Conventions

### ID Offset Pattern

**CRITICAL:** Slab uses 0-based indices, IDs use 1-based (NonZeroUsize):

```rust
// Inserting into Slab:
let slab_index = self.nodes.insert(node);
let id = ElementId::new(slab_index + 1)  // +1 for NonZeroUsize

// Accessing from Slab:
self.nodes.get(element_id.get() - 1)  // -1 to get slab index
```

Applies to: `ViewId`, `ElementId`, `RenderId`, `LayerId`, `SemanticsId`.

### Ambassador Delegation Pattern

```rust
use ambassador::delegatable_trait;

#[delegatable_trait]
pub trait RenderObject {
    fn mark_needs_layout(&mut self);
}

#[derive(Delegate)]
#[delegate(RenderObject, target = "child")]
pub struct RenderPadding {
    child: BoxChild<Single>,  // Delegates through BoxChild
}
```

### RenderTree Lifecycle

**Setup:** Create → `attach(owner)` → Set `parent_data`
**Layout:** Check `needs_layout()` → `layout(constraints)` → `perform_layout()` → Clear flag
**Paint:** `paint(context)` → Generate layers → Compositor
**Teardown:** `detach()` → Drop

### Constraints and Sizing

```rust
// Tight constraints (exact size)
let tight = Constraints::tight(Size::new(100.0, 100.0));

// Loose constraints (max size)
let loose = Constraints::loose(Size::new(200.0, 200.0));

// Box constraints
let box_constraints = BoxConstraints::new(
    min_width: 0.0,
    max_width: 100.0,
    min_height: 0.0,
    max_height: 100.0,
);
```

## Common Development Patterns

### Creating a RenderObject

```rust
use flui_rendering::{RenderObject, RenderBox, BoxChild, Arity};
use flui_types::{Size, Constraints};

pub struct RenderCustom {
    child: BoxChild<Single>,
    size: Size,
    needs_layout: bool,
}

impl RenderObject for RenderCustom {
    fn attach(&mut self, owner: PipelineOwner) {
        self.child.attach(owner);
    }

    fn mark_needs_layout(&mut self) {
        self.needs_layout = true;
    }

    fn layout(&mut self, constraints: Constraints) {
        if !self.needs_layout { return; }
        self.perform_layout();
        self.needs_layout = false;
    }
}

impl RenderBox<Single> for RenderCustom {
    fn perform_layout(&mut self) {
        let child_size = self.child.layout(constraints);
        self.size = constraints.constrain(child_size);
    }
}
```

### Hit Testing

```rust
use flui_interaction::{HitTestResult, HitTestEntry};

impl RenderCustom {
    pub fn hit_test(&self, result: &mut HitTestResult, position: Offset) -> bool {
        if !self.size.contains(position) { return false; }

        // Test children first (reverse paint order)
        if let Some(child) = &self.child {
            if child.hit_test(result, position) { return true; }
        }

        result.add(HitTestEntry::new(self.id()));
        true
    }
}
```

## Key Dependencies

**Core dependencies:**
- **wgpu 25.x** - GPU API (stay on 25.x, 26.0+ has codespan-reporting issues - see https://github.com/gfx-rs/wgpu/issues/7915)
- **parking_lot 0.12** - High-performance sync primitives (2-3x faster than std)
- **tokio 1.43** - Async runtime (LTS until March 2026)
- **tracing** - Structured logging (MANDATORY, never println!)
- **ambassador 0.4.2** - Trait delegation for RenderObject traits
- **slab** - Tree node storage with O(1) insert/remove
- **bon 3.8** - Builder pattern for complex widgets

**Platform dependencies:**
- **winit 0.30.12** - Cross-platform windowing (fallback implementation)
- **arboard 3.4** - Cross-platform clipboard
- **windows 0.52** - Win32 API bindings (WindowsPlatform)
- **cocoa 0.26.0** - AppKit bindings (MacOSPlatform)
- **ui-events** - W3C-standard event types
- **keyboard-types** - Platform-agnostic keyboard events

**Engine-only dependencies:**
- **glam 0.30** - GPU math (vectors, matrices)
- **lyon** - Path tessellation to triangles
- **glyphon** - SDF text rendering
- **cosmic-text** - Text layout and shaping
- **guillotiere** - Texture atlas packing

## Git Workflow

Use conventional commits:
```bash
git commit -m "feat: Add new widget
- Implementation details
- Additional changes"
```

Types: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`

### CRITICAL Git Rules

**NEVER do `git checkout`, `git reset`, `git stash`, or any destructive git operations without explicit user permission.** Always ask first. Lost uncommitted work is unrecoverable.

## Troubleshooting

```bash
# Clean build
cargo clean && cargo build --workspace

# Check workspace
cargo metadata --format-version 1 | grep -A 5 "workspace_members"

# Verify crate
cargo check -p flui_rendering
cargo clippy -p flui_rendering -- -D warnings
```

**Common errors:**
- "package not found" → Check `Cargo.toml` `[workspace.members]`
- "trait not in scope" → Check prelude: `use flui_rendering::prelude::*;`
- "mismatched types" with Arity → Verify `BoxChild<Single>` matches `impl RenderBox<Single>`

**wgpu issues:** Stay on 25.x (see https://github.com/gfx-rs/wgpu/issues/7915)

## Development Workflow Tips

1. **Read constitution first** - `.specify/memory/constitution.md` governs all development
2. **Check workspace state** - Many crates disabled in `Cargo.toml`, verify before building
3. **Test-first for public APIs** - Write failing tests BEFORE implementation
4. **Use tracing, NEVER println** - Constitution requirement, essential for debugging
5. **Check current specs** - See `specs/dev/` for active development plans
6. **Enable logging** - `RUST_LOG=debug` or `RUST_LOG=trace` catches issues early
7. **Build in dependency order** - Foundation → Platform → Core → Rendering → Widget → App
8. **Reference Flutter first** - Check `.flutter/` directory before implementing new features
9. **Use Context7 proactively** - Fetch docs for external libraries (wgpu, winit, etc.)
10. **Batch file operations** - `read_multiple_files` for efficiency
11. **Check OpenSpec** - Large changes (new features, breaking changes) need proposals
12. **Follow ID Offset Pattern** - Slab index + 1 = ID, ID - 1 = Slab index (all ID types)
