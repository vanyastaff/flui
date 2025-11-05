# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

FLUI is a production-ready, Flutter-inspired declarative UI framework for Rust, featuring the proven three-tree architecture (View → Element → Render) with modern Rust idioms. Built on egui 0.33 with support for both egui and wgpu backends.

**Key Architecture:**
```
View Tree (immutable) → Element Tree (mutable) → Render Tree (layout/paint)
```

## Build Commands

### Building Individual Crates

Always build crates in dependency order when making structural changes:

```bash
# Build in dependency order
cargo build -p flui_types
cargo build -p flui_painting
cargo build -p flui_engine
cargo build -p flui_core
cargo build -p flui_rendering
cargo build -p flui_widgets
cargo build -p flui_app

# Build all
cargo build --workspace
```

### Running Tests

```bash
# Run all tests
cargo test --workspace

# Test specific crate
cargo test -p flui_core

# Run with logging
RUST_LOG=debug cargo test -p flui_core
```

### Running Examples

```bash
# Run minimal text demo (useful for debugging rendering)
cargo run --example minimal_text_demo

# Run hello world
cargo run --example hello_world_view

# Run with tracing enabled
RUST_LOG=debug cargo run --example minimal_text_demo
```

### Benchmarks

```bash
# Run benchmarks for specific crate
cargo bench -p flui_core
cargo bench -p flui_types
```

### Linting

```bash
# Check for warnings
cargo clippy --workspace -- -D warnings

# Fix automatically
cargo clippy --workspace --fix
```

## Code Architecture

### Three-Tree System

**View Tree (Immutable):**
- Views implement the `View` trait with `Clone` bound
- Created fresh every frame - must be cheap to clone
- `build()` creates initial element and state
- `rebuild()` efficiently updates existing elements (override for performance)
- Located in: `crates/flui_core/src/view/`

**Element Tree (Mutable):**
- Stored in `Slab` arena at `crates/flui_core/src/element/element_tree.rs`
- Three variants: `Component`, `Render`, `Provider`
- ElementId uses `NonZeroUsize` for niche optimization (Option<ElementId> = 8 bytes)
- **CRITICAL:** Slab indices are 0-based but ElementId is 1-based (+1 offset in insert, -1 in get)
- Lifecycle: Initial → Active → Inactive → Defunct

**Render Tree (Layout/Paint):**
- Three render traits based on child count: `LeafRender` (0), `SingleRender` (1), `MultiRender` (N)
- Uses GAT (Generic Associated Types) for type-safe metadata
- `RenderNode` enum at `crates/flui_core/src/render/render_node.rs`
- Located in: `crates/flui_rendering/src/objects/`

### Pipeline Architecture

The rendering pipeline has three phases coordinated by `PipelineOwner`:

1. **Build Phase:** Rebuilds dirty components via `flush_build()`
2. **Layout Phase:** Computes sizes via `flush_layout(constraints)`
3. **Paint Phase:** Generates layers via `flush_paint()`

**Key files:**
- `crates/flui_core/src/pipeline/pipeline_owner.rs` - Main coordinator
- `crates/flui_core/src/pipeline/frame_coordinator.rs` - Phase management
- `crates/flui_core/src/pipeline/build_pipeline.rs` - Build phase
- `crates/flui_core/src/pipeline/layout_pipeline.rs` - Layout phase
- `crates/flui_core/src/pipeline/paint_pipeline.rs` - Paint phase

**CRITICAL BUG PATTERN:** When calling `request_layout()`, you must set BOTH:
1. Mark in dirty set via `coordinator.layout_mut().mark_dirty(node_id)`
2. Set RenderState flag via `render_state.mark_needs_layout()`

Failing to set both will cause layout to skip elements.

### State Management with Hooks

Hooks provide React-like state management with automatic rebuild scheduling:

```rust
// Signal - reactive state
let count = use_signal(ctx, 0);

// Memo - derived state
let doubled = use_memo(ctx, |_| count.get() * 2);

// Effect - side effects
use_effect_simple(ctx, || {
    println!("Count changed: {}", count.get());
});
```

**Hook Rules (MUST follow):**
1. Always call hooks in the same order every build
2. Never call hooks conditionally
3. Only call hooks at component top level
4. Clone signals before moving into closures

Located in: `crates/flui_core/src/hooks/`

### Widget Conversion to View API

The codebase is migrating from the old Widget API to the new View API. When converting widgets:

1. Change `Widget` trait to `View` trait
2. Update `build()` signature: `fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State)`
3. Implement `rebuild()` for performance (compare with previous view)
4. Use `Element` enum instead of `Box<dyn AnyWidget>`
5. Replace `RenderObject` with one of: `LeafRender`, `SingleRender`, or `MultiRender`

**Conversion examples:** See `crates/flui_widgets/src/` for converted widgets.

## Logging and Debugging

### Always Use Tracing

**IMPORTANT:** Always use `tracing` for logging, NEVER use `println!` or `eprintln!`.

```rust
// Initialize at program start
tracing_subscriber::fmt()
    .with_max_level(tracing::Level::DEBUG)
    .init();

// Use throughout code
#[cfg(debug_assertions)]
tracing::debug!("RenderParagraph::paint: text='{}', size={:?}", text, size);

#[cfg(debug_assertions)]
tracing::warn!("Layout phase skipped: needs_layout() returned false");
```

### Debugging Text Rendering Issues

If text doesn't appear on screen but all pipeline phases execute:

1. Check layout is actually running (not just scheduled)
2. Verify RenderState flags are set correctly
3. Add tracing to paint phase: `RenderParagraph::paint()`, `PictureLayer::paint()`, `EguiPainter::text_styled()`
4. Check coordinate system (text might be offscreen)
5. Verify egui shapes are being added to the correct painter

**Common issue:** Layout dirty set and RenderState flags out of sync.

## Common Patterns

### Creating a Simple View

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct MyView {
    pub text: String,
}

impl View for MyView {
    type Element = Element;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Build element tree
        let element = /* ... */;
        (element, ())
    }

    fn rebuild(self, prev: &Self, _state: &mut Self::State,
               element: &mut Self::Element) -> ChangeFlags {
        if self == *prev {
            return ChangeFlags::NONE;  // Skip rebuild!
        }
        element.mark_dirty();
        ChangeFlags::NEEDS_BUILD
    }
}
```

### Creating a RenderObject

Choose trait based on child count:

```rust
// No children
impl LeafRender for RenderText {
    type Metadata = ();

    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Compute size
    }

    fn paint(&self, offset: Offset) -> BoxedLayer {
        // Draw text
    }
}

// One child
impl SingleRender for RenderPadding {
    type Metadata = ();

    fn layout(&mut self, tree: &ElementTree, child_id: ElementId,
              constraints: BoxConstraints) -> Size {
        let child_size = tree.layout_child(child_id, constraints.deflate(&self.padding));
        Size::new(
            child_size.width + self.padding.horizontal_total(),
            child_size.height + self.padding.vertical_total(),
        )
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId,
             offset: Offset) -> BoxedLayer {
        tree.paint_child(child_id, offset + self.padding.top_left_offset())
    }
}

// Multiple children
impl MultiRender for RenderColumn {
    type Metadata = ();

    fn layout(&mut self, tree: &ElementTree, children: &[ElementId],
              constraints: BoxConstraints) -> Size {
        // Layout children vertically
    }

    fn paint(&self, tree: &ElementTree, children: &[ElementId],
             offset: Offset) -> BoxedLayer {
        // Paint children
    }
}
```

### Using GAT Metadata

For complex layouts that need per-child metadata:

```rust
#[derive(Debug, Clone, Copy)]
pub struct FlexItemMetadata {
    pub flex: i32,
    pub fit: FlexFit,
}

impl SingleRender for RenderFlexible {
    type Metadata = FlexItemMetadata;

    fn metadata(&self) -> Option<&dyn Any> {
        Some(&self.flex_metadata)
    }
}

// Parent accesses metadata:
impl MultiRender for RenderFlex {
    fn layout(&mut self, tree: &ElementTree, children: &[ElementId],
              constraints: BoxConstraints) -> Size {
        for &child_id in children {
            if let Some(metadata) = tree.get_metadata::<FlexItemMetadata>(child_id) {
                // Use metadata.flex and metadata.fit
            }
        }
    }
}
```

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

Views are cloned every frame, so implement efficient cloning:

```rust
// ✅ Good - cheap clone
#[derive(Clone)]
struct GoodView {
    text: String,  // String is cheap to clone
    data: Arc<Vec<i32>>,  // Arc for shared data
}

// ❌ Bad - expensive clone
#[derive(Clone)]
struct BadView {
    data: Vec<ComplexStruct>,  // Deep clone every frame!
}
```

### Override rebuild() for Performance

The default `rebuild()` always marks dirty - override it:

```rust
fn rebuild(self, prev: &Self, _state: &mut Self::State,
           element: &mut Self::Element) -> ChangeFlags {
    if self == *prev {
        return ChangeFlags::NONE;  // Massive optimization!
    }
    element.mark_dirty();
    ChangeFlags::NEEDS_BUILD
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

## Documentation

Comprehensive documentation is available in:

- `crates/flui_core/docs/ARCHITECTURE.md` - Three-tree architecture details
- `crates/flui_core/docs/VIEW_GUIDE.md` - Comprehensive View trait guide
- `crates/flui_core/docs/HOOKS_GUIDE.md` - State management with hooks
- `crates/flui_core/docs/QUICK_START.md` - Getting started guide
- `README.md` - Project overview and examples

## Git Workflow

### Commit Message Format

Use conventional commits with co-authorship:

```bash
git commit -m "$(cat <<'EOF'
feat: Add new widget for user profiles

- Implement ProfileCard view
- Add avatar support with image loading
- Add responsive layout for mobile/desktop

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

### Branch Structure

- `main` - Primary development branch
- Feature branches as needed

## Dependencies

Key dependencies and their purpose:

- **egui 0.33** - Immediate mode GUI backend
- **eframe 0.33** - Platform integration for egui
- **parking_lot 0.12** - High-performance RwLock (2-3x faster than std)
- **tokio 1.43** - Async runtime
- **glam 0.30** - Math and geometry
- **tracing** - Structured logging (always use this, never println!)
- **slab** - Arena allocator for element tree

## Performance Considerations

- Element enum is 3.75x faster than `Box<dyn>` trait objects
- Option<ElementId> has zero overhead due to niche optimization (8 bytes)
- parking_lot::RwLock is 2-3x faster than std::sync::RwLock
- Slab provides O(1) insertion/removal with cache-friendly contiguous storage
- Override `rebuild()` to avoid unnecessary work (check equality first)

## Known Issues

### Text Rendering Not Visible

If application runs but text doesn't appear:
1. Verify layout phase executes (check `needs_layout()` flag)
2. Ensure `request_layout()` sets both dirty set AND RenderState flag
3. Add tracing to paint pipeline to verify egui shapes are created
4. Check for coordinate system issues (text drawn offscreen)

### Layout Skipped Despite Dirty Elements

If `flush_layout` returns early:
1. Check `RenderState.needs_layout()` is true
2. Verify `request_layout()` sets both dirty set and flag
3. See `crates/flui_core/src/pipeline/pipeline_owner.rs:314-325` for correct pattern
