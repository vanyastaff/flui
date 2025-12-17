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
- **Foundation:** `flui_types`, `flui-foundation`, `flui-tree`
- **Framework:** `flui-view`, `flui-reactivity`, `flui-scheduler`, `flui_core`
- **Rendering:** `flui_painting`, `flui_engine`, `flui_rendering`
- **Widget:** `flui_widgets`, `flui_animation`, `flui_interaction`
- **Application:** `flui_app`, `flui_assets`
- **Tools:** `flui_devtools`, `flui_cli`, `flui_build`

### Flutter Reference Sources

`.flutter/` directory contains Flutter framework source code for reference:
- `.flutter/src/rendering/` - RenderObject implementations
- `.flutter/src/widgets/` - Widget and Element implementations
- `.flutter/rendering.dart`, `widgets.dart`, `animation.dart` - API overview

**Usage:** Check Flutter's approach before implementing new features, then adapt to Rust idioms (type-safe arity, Ambassador delegation, no nullability).

### Current Development Focus

**IMPORTANT:** Workspace focused on `flui_rendering` development. Many crates temporarily disabled in `Cargo.toml`.

**Active crates:**
- Foundation: `flui_types`, `flui-foundation`, `flui-tree`
- Core: `flui-layer`, `flui-semantics`, `flui_interaction`, `flui_painting`
- Target: `flui_rendering` (ACTIVE DEVELOPMENT)

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

/openspec:proposal          # Create change proposal
/openspec:apply <name>      # Apply approved change
/openspec:archive <name>    # Archive deployed change
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

- **wgpu 25.x** - GPU API (stay on 25.x, 26.0+ has codespan-reporting issues)
- **parking_lot 0.12** - High-performance sync primitives (2-3x faster than std)
- **tokio 1.43** - Async runtime (LTS until March 2026)
- **tracing** - Structured logging (required, never println!)
- **ambassador 0.4.2** - Trait delegation
- **slab** - Tree node storage

**Engine only:** glam 0.30, lyon, glyphon, cosmic-text

## Git Workflow

Use conventional commits:
```bash
git commit -m "feat: Add new widget
- Implementation details
- Additional changes"
```

Types: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`

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

1. **Check workspace state** - Use `cargo metadata` or check `Cargo.toml`
2. **Use slash commands** - Faster than cargo commands
3. **Enable logging** - `RUST_LOG=debug` catches issues early
4. **Build in dependency order** - Foundation → Core → Rendering
5. **Use tracing, not println** - Essential for debugging
6. **Check OpenSpec** - Large changes need proposals
7. **Reference Flutter first** - Check `.flutter/` before implementing
8. **Use Context7 proactively** - Fetch docs for external libraries
9. **Batch file operations** - `read_multiple_files` for efficiency
