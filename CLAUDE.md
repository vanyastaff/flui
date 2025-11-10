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

**Example workflow:**
```
User: "Add support for serde serialization"
Assistant: [Automatically calls Context7 to get serde docs, then provides implementation]
```

## Project Overview

FLUI is a production-ready, Flutter-inspired declarative UI framework for Rust, featuring the proven three-tree architecture (View → Element → Render) with modern Rust idioms. Built with wgpu for high-performance GPU-accelerated rendering.

**Key Architecture:**
```
View Tree (immutable) → Element Tree (mutable) → Render Tree (layout/paint)
```

**Thread-Safety:** FLUI is fully thread-safe and supports multi-threaded UI. All hooks use `Arc`/`Mutex` (parking_lot) instead of `Rc`/`RefCell`.

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
# Run simplified view example
cargo run --example simplified_view

# Run thread-safe hooks example
cargo run --example thread_safe_hooks

# Run with tracing enabled
RUST_LOG=debug cargo run --example simplified_view
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

# Format code
cargo fmt --all
```

## Code Architecture

### Three-Tree System

**View Tree (Immutable):**
- Views implement the unified `View` trait
- Single `build()` method returns `impl IntoElement`
- **NO GATs** - State/Element types removed in v0.6.0 migration
- **NO rebuild() method** - Framework handles this automatically
- Views must be `'static` but NOT necessarily `Clone`
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

### Modern View API (v0.6.0+)

The View API has been unified and simplified. **The old Component trait no longer exists.**

```rust
// Modern View trait (unified)
pub trait View: 'static {
    fn build(self, ctx: &BuildContext) -> impl IntoElement;
}

// Example usage
#[derive(Debug)]
pub struct Padding {
    pub padding: EdgeInsets,
    pub child: Option<AnyElement>,
}

impl View for Padding {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Returns tuple of (RenderObject, Option<child>)
        (RenderPadding::new(self.padding), self.child)
    }
}
```

**Key Changes:**
- ✅ Single unified `View` trait (no separate Component)
- ✅ No GAT State/Element types
- ✅ No rebuild() or teardown() methods
- ✅ Returns `impl IntoElement` (automatic tree insertion)
- ✅ Thread-local BuildContext for automatic setup
- ✅ 75% less boilerplate per widget

**IntoElement Types:**
- `(LeafRender, ())` → LeafRenderBuilder
- `(SingleRender, Option<child>)` → SingleRenderBuilder
- `(MultiRender, Vec<child>)` → MultiRenderBuilder
- `AnyElement` → For heterogeneous view storage

### State Management with Hooks

**CRITICAL:** FLUI is thread-safe. All hooks use `Arc`/`Mutex` (parking_lot).

Hooks provide React-like state management with automatic rebuild scheduling:

```rust
// Signal - reactive state
let count = use_signal(ctx, 0);

// Memo - derived state
let doubled = use_memo(ctx, |_| count.get() * 2);

// Effect - side effects
use_effect(ctx, move || {
    println!("Count changed: {}", count.get());
    None  // No cleanup
});
```

**Hook Rules (MUST follow):**
1. ✅ Always call hooks in the same order every build
2. ❌ Never call hooks conditionally
3. ❌ Never call hooks in loops with variable iterations
4. ✅ Only call hooks at component top level
5. ✅ Clone signals before moving into closures

**Breaking these rules causes PANICS!** See `crates/flui_core/src/hooks/RULES.md` for detailed explanation.

**Thread-Safety Requirements:**
- All signal values must implement `Send`
- All callbacks must be `Send + Sync`
- Uses `Arc<Mutex<T>>` instead of `Rc<RefCell<T>>`
- Uses `parking_lot::Mutex` (2-3x faster than std, no poisoning)

Located in: `crates/flui_core/src/hooks/`

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

### Creating a Simple View (New API)

```rust
#[derive(Debug)]
pub struct MyView {
    pub text: String,
}

impl View for MyView {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Return RenderObject + children
        // Framework handles tree insertion automatically
        (RenderText::new(self.text), ())
    }
}
```

**No need for:**
- ❌ GAT State/Element types
- ❌ rebuild() method
- ❌ teardown() method
- ❌ Manual tree insertion
- ❌ Clone derive (unless you need it)

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

### Using Hooks for State

```rust
#[derive(Debug)]
pub struct Counter;

impl View for Counter {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Hook at top level
        let count = use_signal(ctx, 0);

        // Clone before moving into closure
        let count_clone = count.clone();

        Column::new()
            .children(vec![
                Box::new(Text::new(format!("Count: {}", count.get()))),
                Box::new(Button::new("Increment")
                    .on_pressed(move || count_clone.update(|c| *c += 1))),
            ])
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

### Thread-Local BuildContext

The new View API uses thread-local BuildContext via RAII guards:

```rust
// Framework code (automatic)
pub fn with_build_context<F, R>(context: &BuildContext, f: F) -> R
where
    F: FnOnce() -> R,
{
    let _guard = BuildContextGuard::new(context);
    f()
}

// User code (you don't need to call this)
impl<V: View> IntoElement for V {
    fn into_element(self) -> Element {
        let ctx = current_build_context();  // ← Thread-local access
        let element_like = self.build(ctx);
        element_like.into_element()
    }
}
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

**Note:** Views don't need to be Clone anymore (only `'static`), but if you do derive Clone, make it cheap.

### ElementId Offset Pattern

**CRITICAL:** Slab uses 0-based indices but ElementId uses 1-based (NonZeroUsize):

```rust
// In element_tree.rs insert():
let id = self.nodes.insert(node);
ElementId::new(id + 1)  // +1 because ElementId uses NonZeroUsize

// In element_tree.rs get():
self.nodes.get(element_id.get() - 1).map(|node| &node.element)  // -1 to access slab
```

## Feature Flags

### Parallel Processing (Stable)

```toml
features = ["parallel"]
```

**Status:** ✅ Stable - Thread-safe parallel processing enabled

Enables rayon-based parallel processing for build pipeline. All thread-safety issues have been resolved through comprehensive Arc/Mutex refactoring.

### Rendering Backend

FLUI uses **wgpu** as its only rendering backend for GPU-accelerated graphics:

```toml
# wgpu backend (GPU-accelerated, production-ready)
flui = "0.1"
```

The previous dual-backend system (egui/wgpu) has been replaced with a unified wgpu-only architecture for better performance and maintainability.

## Documentation

Comprehensive documentation is available in:

**Recent Refactorings:**
- `THREAD_SAFE_HOOKS_REFACTORING.md` - Thread-safety migration (Arc/Mutex)
- `VIEW_API_MIGRATION_COMPLETE.md` - View API unification
- `VIEW_API_LOGIC_REVIEW.md` - View API design review

**Architecture:**
- `docs/PIPELINE_ARCHITECTURE.md` - Pipeline design and multi-threading
- `docs/FINAL_ARCHITECTURE_V2.md` - Overall architecture
- `docs/API_GUIDE.md` - Comprehensive API guide

**Hooks:**
- `crates/flui_core/src/hooks/RULES.md` - **MUST READ** - Hook usage rules
- `crates/flui_core/src/hooks/HOOK_REFACTORING.md` - Hook internals

**Widgets:**
- `crates/flui_widgets/flutter_widgets_full_guide.md` - Flutter widget reference
- `crates/flui_rendering/RENDER_OBJECT_GUIDE.md` - RenderObject guide

**Examples:**
- `crates/flui_core/examples/simplified_view.rs` - Modern View API example
- `crates/flui_core/examples/thread_safe_hooks.rs` - Thread-safety demonstration

## Asset Management (flui-assets)

FLUI provides a high-performance asset management system in the `flui-assets` crate for loading and caching images, fonts, and other resources.

### Architecture

The asset system uses a **Clean Architecture + Performance** approach:

- **Generic `Asset<T>` trait**: Type-safe, extensible system for any asset type
- **High-performance caching**: Moka-based cache with TinyLFU eviction (lock-free, async)
- **Interned keys**: 4-byte `AssetKey` using lasso for fast hashing and comparison
- **Arc-based handles**: Efficient shared ownership with weak references
- **Multiple loaders**: File, memory, and network sources (network requires feature flag)
- **Async I/O**: Non-blocking loading with tokio

### Basic Usage

```rust
use flui_assets::{AssetRegistry, ImageAsset, FontAsset};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get the global registry
    let registry = AssetRegistry::global();

    // Load an image (requires 'images' feature)
    let image = ImageAsset::file("assets/logo.png");
    let handle = registry.load(image).await?;
    println!("Loaded: {}x{}", handle.width(), handle.height());

    // Load a font
    let font = FontAsset::file("assets/Roboto-Regular.ttf");
    let font_handle = registry.load(font).await?;
    println!("Font loaded: {} bytes", font_handle.bytes.len());

    // Subsequent loads use the cache
    let image2 = ImageAsset::file("assets/logo.png");
    let handle2 = registry.load(image2).await?; // Cache hit!

    Ok(())
}
```

### Adding New Asset Types

To add a new asset type, implement the `Asset` trait:

```rust
use flui_assets::core::{Asset, AssetMetadata};
use flui_assets::types::AssetKey;
use flui_assets::error::AssetError;

pub struct AudioAsset {
    path: String,
}

impl Asset for AudioAsset {
    type Data = AudioData;  // Your audio data type
    type Key = AssetKey;
    type Error = AssetError;

    fn key(&self) -> AssetKey {
        AssetKey::new(&self.path)
    }

    async fn load(&self) -> Result<AudioData, AssetError> {
        // Load and decode audio file
        let bytes = tokio::fs::read(&self.path).await?;
        Ok(AudioData::from_bytes(bytes)?)
    }

    fn metadata(&self) -> Option<AssetMetadata> {
        Some(AssetMetadata {
            format: Some("MP3".to_string()),
            ..Default::default()
        })
    }
}
```

The cache, registry, and loaders automatically work with your new asset type!

### Feature Flags

```toml
[dependencies]
flui-assets = { version = "0.1", features = ["images", "network"] }
```

Available features:
- `images` - Enable image loading (PNG, JPEG, GIF, WebP, etc.)
- `bundles` - Asset bundling and manifest support
- `network` - Network-based asset loading via HTTP
- `hot-reload` - File watching for development
- `mmap-fonts` - Memory-mapped font loading (performance optimization)
- `parallel-decode` - Parallel image/video decoding with rayon

### Loaders

**FileLoader** - Load from filesystem:
```rust
use flui_assets::BytesFileLoader;

let loader = BytesFileLoader::new("assets");
let bytes = loader.load_bytes("logo.png").await?;
let text = loader.load_string("config.json").await?;
```

**MemoryLoader** - Load from in-memory storage:
```rust
use flui_assets::MemoryLoader;

let loader = MemoryLoader::new();
loader.insert(AssetKey::new("data"), vec![1, 2, 3, 4, 5]);

let data = loader.load(&key).await?;
```

### Examples

- `crates/flui_assets/examples/basic_usage.rs` - Demonstrates core features

### Performance Notes

- **Cache**: Uses Moka's TinyLFU algorithm for better hit rates than LRU
- **Keys**: Interned strings reduce memory and speed up comparisons (4 bytes vs 24+)
- **Handles**: Arc-based for cheap cloning, weak references prevent cache bloat
- **Async**: All I/O is non-blocking for maximum concurrency

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

- **wgpu** - Cross-platform GPU API (Vulkan/Metal/DX12/WebGPU)
- **lyon** - Path tessellation (converts vector graphics to triangles)
- **glyphon** - GPU text rendering with SDF (Signed Distance Field)
- **parking_lot 0.12** - High-performance RwLock/Mutex (2-3x faster than std, no poisoning)
- **tokio 1.43** - Async runtime
- **glam 0.30** - Math and geometry
- **tracing** - Structured logging (always use this, never println!)
- **slab** - Arena allocator for element tree
- **rayon** - Parallel processing (when `parallel` feature enabled)

## Performance Considerations

- **GPU-Accelerated Rendering**: wgpu provides native GPU performance on all platforms
- **Buffer Pooling**: Reuses GPU buffers across frames for minimal allocation overhead
- **Mesh-Based Rendering**: All primitives tessellate to triangles for efficient GPU processing
- Element enum is 3.75x faster than `Box<dyn>` trait objects
- Option<ElementId> has zero overhead due to niche optimization (8 bytes)
- parking_lot::Mutex is 2-3x faster than std::sync::Mutex (no poisoning, smaller footprint)
- parking_lot::RwLock is 2-3x faster than std::sync::RwLock
- Slab provides O(1) insertion/removal with cache-friendly contiguous storage
- New View API reduces boilerplate by 75% with no performance cost
- Thread-safe hooks enable parallel UI updates

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

### Hook Panics

If you get "Hook state type mismatch" panics:
1. Check that hooks are called in the same order every render
2. Never call hooks conditionally (no `if` around hooks)
3. Never call hooks in loops with variable iterations
4. See `crates/flui_core/src/hooks/RULES.md` for complete rules

## Migration Guides

### Migrating to New View API

**Old API (deprecated):**
```rust
impl View for MyWidget {
    type Element = Element;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Manual tree management...
        (element, ())
    }

    fn rebuild(self, prev: &Self, state: &mut Self::State,
               element: &mut Self::Element) -> ChangeFlags {
        // Manual rebuild logic...
    }
}
```

**New API (recommended):**
```rust
impl View for MyWidget {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Just return RenderObject + children
        (RenderMyWidget::new(), self.child)
    }
}
```

**Benefits:** 75% less code, automatic tree management, no GATs, no manual rebuilds.
