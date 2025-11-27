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

## Build Commands

### Building Individual Crates

Always build crates in dependency order when making structural changes:

```bash
# Foundation Layer
cargo build -p flui_types
cargo build -p flui-foundation
cargo build -p flui-tree

# Framework Layer
cargo build -p flui-view
cargo build -p flui-pipeline
cargo build -p flui-reactivity
cargo build -p flui-scheduler
cargo build -p flui_core

# Rendering Layer
cargo build -p flui_painting
cargo build -p flui_engine
cargo build -p flui_rendering

# Widget & Application Layer
cargo build -p flui_widgets
cargo build -p flui_animation
cargo build -p flui_interaction
cargo build -p flui_app
cargo build -p flui_assets

# Development Tools
cargo build -p flui_devtools
cargo build -p flui_cli
cargo build -p flui_build

# Build all
cargo build --workspace
```

### Running Tests

```bash
# Run all tests
cargo test --workspace

# Test foundation layer
cargo test -p flui-foundation
cargo test -p flui-tree
cargo test -p flui-reactivity

# Test framework layer
cargo test -p flui_core
cargo test -p flui-pipeline

# Test specific crate
cargo test -p flui_widgets

# Run with logging
RUST_LOG=debug cargo test -p flui_core
```

### Running Examples

```bash
# Run basic examples
cargo run --example hello_world_view

# Run reactive examples
cargo run --example counter_reactive
cargo run --example todo_app

# Run pipeline examples
cargo run --example custom_pipeline
cargo run --example parallel_builds

# Run with tracing enabled
RUST_LOG=debug cargo run --example hello_world_view
```

### Benchmarks

```bash
# Run benchmarks for specific layers
cargo bench -p flui-reactivity   # Signal performance
cargo bench -p flui-pipeline     # Pipeline coordination
cargo bench -p flui_core         # Core framework
cargo bench -p flui_types        # Basic types
```

### Cross-Platform Builds

FLUI uses the **xtask** build system for cross-platform builds. See `BUILD.md` for complete documentation.

```bash
# Check environment and installed tools
cargo xtask info

# Build for Android (debug)
cargo xtask build android

# Build for Android (release)
cargo xtask build android --release

# Build for Web
cargo xtask build web --release

# Build for Desktop (Windows/Linux/macOS)
cargo xtask build desktop --release

# Clean build artifacts
cargo xtask clean --all

# Convenient aliases (defined in .cargo/config.toml)
cargo build-android-release
cargo build-web-release
cargo build-desktop-release
```

**Output locations:**
- Android: `target/flui-out/android/flui-{debug|release}.apk`
- Web: `target/flui-out/web/` (ready to serve)
- Desktop: `target/flui-out/desktop/flui_app[.exe]`

**Prerequisites:**
- **Android**: Android SDK, NDK, Java JDK 11+, `cargo install cargo-ndk`, `rustup target add aarch64-linux-android`
- **Web**: `cargo install wasm-pack`, `rustup target add wasm32-unknown-unknown`
- **Desktop**: Platform build tools (MSVC/Xcode/GCC)

See `BUILD.md` for detailed setup instructions and troubleshooting.

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
- Views implement traits from `flui-view` crate
- Single `build()` method returns `impl IntoElement`
- Views must be `'static` but NOT necessarily `Clone`
- Located in: `crates/flui-view/src/`

**Element Tree (Mutable):**
- Stored using tree abstractions from `flui-tree` crate
- Element identification and lifecycle managed by `flui-foundation`
- ElementId uses `NonZeroUsize` for niche optimization (Option<ElementId> = 8 bytes)
- Tree traversal uses visitor patterns from `flui-tree`
- Located in: `crates/flui_core/src/element/`

**Render Tree (Layout/Paint):**
- Three render traits based on child count: `LeafRender` (0), `SingleRender` (1), `MultiRender` (N)
- Uses GAT (Generic Associated Types) for type-safe metadata
- Pipeline coordination handled by `flui-pipeline` crate
- Located in: `crates/flui_rendering/src/objects/`

### Element Architecture (v0.7.0)

**Unified Element Struct:**
```rust
pub struct Element {
    // Tree position
    parent: Option<ElementId>,
    children: Vec<ElementId>,
    slot: Option<Slot>,
    
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

**ViewObject Trait:**
```rust
pub trait ViewObject: Send {
    // Core lifecycle
    fn mode(&self) -> ViewMode;
    fn build(&mut self, ctx: &BuildContext) -> Element;
    fn init(&mut self, ctx: &BuildContext) {}
    fn did_update(&mut self, new_view: &dyn Any, ctx: &BuildContext) {}
    fn dispose(&mut self, ctx: &BuildContext) {}
    
    // Render-specific (default: None)
    fn render_object(&self) -> Option<&dyn RenderObject> { None }
    fn render_state(&self) -> Option<&RenderState> { None }
    fn protocol(&self) -> Option<LayoutProtocol> { None }
    fn arity(&self) -> Option<RuntimeArity> { None }
    
    // Provider-specific (default: None)
    fn provided_value(&self) -> Option<&(dyn Any + Send + Sync)> { None }
    fn dependents(&self) -> Option<&[ElementId]> { None }
    
    // Downcasting
    fn as_any(&self) -> &dyn Any;
}
```

**Element Access Patterns:**
```rust
// Check element type
if element.is_render() {
    let render = element.render_object().unwrap();
    let state = element.render_state().unwrap();
}

if element.is_provider() {
    let value = element.provided_value().unwrap();
    let deps = element.dependents().unwrap();
}

// Children access (unified for all types)
element.children()
element.add_child(child_id)
element.remove_child(child_id)
```

### Pipeline Architecture

The rendering pipeline has three phases coordinated by abstract traits from `flui-pipeline`:

1. **Build Phase:** Implements `BuildPhase` trait for widget rebuilds
2. **Layout Phase:** Implements `LayoutPhase` trait for size computation  
3. **Paint Phase:** Implements `PaintPhase` trait for layer generation

**Key files:**
- `crates/flui-pipeline/src/traits/` - Abstract phase traits
- `crates/flui-pipeline/src/coordinator/` - Pipeline coordination
- `crates/flui_core/src/pipeline/` - Concrete implementations
- `crates/flui-scheduler/src/` - Frame scheduling and prioritization

**Architecture Benefits:**
- **Extensible:** Implement custom pipeline phases
- **Testable:** Abstract traits enable easy mocking
- **Flexible:** Different coordinators for different use cases
- **Thread-safe:** Built-in support for parallel processing

### Modern View API (v0.1.0+)

Views are defined using traits from the `flui-view` crate with reactive state from `flui-reactivity`:

```rust
use flui_view::View;
use flui_reactivity::{Signal, use_signal};

// Modern View trait
pub trait View: 'static {
    fn build(self, ctx: &BuildContext) -> impl IntoElement;
}

// Example with reactive state
#[derive(Debug)]
pub struct Counter;

impl View for Counter {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let count = use_signal(ctx, 0);
        
        // Returns element structure
        column![
            text(format!("Count: {}", count.get(ctx))),
            button("Increment").on_press(move || count.update(|n| *n + 1))
        ]
    }
}
```

**Key Features:**
- ✅ Reactive state with `flui-reactivity` hooks
- ✅ Abstract tree operations via `flui-tree` 
- ✅ Foundation types from `flui-foundation`
- ✅ Pipeline integration with `flui-pipeline`
- ✅ Thread-safe Copy-based signals
- ✅ Automatic change tracking and updates

**Element Creation:**
- Views create elements using tree abstractions
- Pipeline handles build/layout/paint coordination
- Reactive system manages state updates

### Reactive State Management

**CRITICAL:** FLUI uses `flui-reactivity` for thread-safe reactive state. All signals are Copy-based and use DashMap for lock-free access.

The reactive system provides automatic change tracking and updates:

```rust
use flui_reactivity::{Signal, use_signal, use_effect, batch};

// Signal - reactive state (Copy-based)
let count = Signal::new(0);
let count_copy = count; // Copy, not clone!

// Computed values
let doubled = count.derive(|&n| n * 2);

// Effects with cleanup
let cleanup = count.watch(|value| {
    println!("Count changed: {}", value);
});

// Batch updates for performance
batch(|| {
    count.set(1);
    count.set(2); 
    count.set(3);
}); // Only one update notification
```

**Key Features:**
- ✅ **Copy-based signals** - No cloning needed
- ✅ **Lock-free storage** - DashMap for concurrent access  
- ✅ **Automatic cleanup** - Weak references prevent leaks
- ✅ **Thread-safe** - All operations work across threads
- ✅ **Performance optimized** - Batching and atomic operations

**Thread-Safety:**
- All signal values must implement `Send + Sync`
- Uses DashMap for lock-free concurrent HashMap
- parking_lot for synchronization (2-3x faster than std)

Located in: `crates/flui-reactivity/src/`

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

**Why tracing-forest over tracing-tree:**
- ✅ Automatic timing: `[ duration | percentage ]` out of the box
- ✅ Context-preserving: async operations stay grouped
- ✅ Percentage distribution: see where time is spent
- ✅ Future-proof: ready for async (animations, hot reload, resource loading)

### Debugging Text Rendering Issues

If text doesn't appear on screen but all pipeline phases execute:

1. Check layout is actually running (not just scheduled)
2. Verify RenderState flags are set correctly
3. Add tracing to paint phase: `RenderParagraph::paint()`, `PictureLayer::paint()`, `EguiPainter::text_styled()`
4. Check coordinate system (text might be offscreen)
5. Verify egui shapes are being added to the correct painter

**Common issue:** Layout dirty set and RenderState flags out of sync.

## Advanced Visual Effects

### Shader Mask Effects

FLUI provides GPU-accelerated shader mask effects for advanced visual styling through the `RenderShaderMask` render object. Shader masks apply gradient or solid color masks to child content, enabling effects like fades, vignettes, and spotlights.

**Architecture:**

```
Child Content → Offscreen Texture → Apply Shader Mask → Composite to Framebuffer
```

The implementation spans three layers:
- **`flui_rendering`**: `RenderShaderMask` render object (crates/flui_rendering/src/objects/effects/shader_mask.rs:122)
- **`flui_painting`**: Canvas API with `draw_shader_mask()` method
- **`flui_engine`**: GPU implementation via `ShaderMaskLayer`, offscreen rendering, and WGSL shaders

**Basic Usage:**

```rust
use flui_rendering::prelude::RenderShaderMask;
use flui_types::{
    painting::{BlendMode, ShaderSpec},
    styling::Color32,
};

// Linear gradient fade (left opaque → right transparent)
let fade = RenderShaderMask {
    shader: ShaderSpec::LinearGradient {
        start: (0.0, 0.5),  // Left center (normalized 0-1)
        end: (1.0, 0.5),    // Right center
        colors: vec![
            Color32::WHITE,        // Fully opaque
            Color32::TRANSPARENT,  // Fully transparent
        ],
    },
    blend_mode: BlendMode::SrcOver,
};

// Radial gradient vignette (bright center → dark edges)
let vignette = RenderShaderMask::radial_gradient(
    (0.5, 0.5),  // Center of viewport
    0.7,         // Radius (70% of viewport)
    vec![
        Color32::WHITE,  // Bright center
        Color32::from_rgba_unmultiplied(0, 0, 0, 200),  // Dark edges
    ],
)
.with_blend_mode(BlendMode::Multiply);

// Solid color mask (for testing)
let solid = RenderShaderMask::solid(Color32::WHITE);
```

**Coordinate System:**

- **Normalized coordinates (0.0 - 1.0)**: ShaderSpec uses relative positions
- **Absolute coordinates**: Converted during paint() based on child size
- **Example**: `(0.5, 0.5)` always points to the center regardless of actual size

**Blend Modes:**

- `SrcOver` - Standard alpha compositing (default)
- `Multiply` - Darkens content (perfect for vignettes)
- `Screen` - Lightens content
- Other Porter-Duff modes supported via `BlendMode` enum

**Common Patterns:**

```rust
// Pattern 1: Horizontal fade (text fade-out)
let horizontal = RenderShaderMask::linear_gradient(
    (0.0, 0.5), (1.0, 0.5),
    vec![Color32::WHITE, Color32::TRANSPARENT],
);

// Pattern 2: Vertical fade (scroll fade indicator)
let vertical = RenderShaderMask::linear_gradient(
    (0.5, 0.0), (0.5, 1.0),
    vec![Color32::TRANSPARENT, Color32::WHITE, Color32::TRANSPARENT],
);

// Pattern 3: Diagonal fade (creative effect)
let diagonal = RenderShaderMask::linear_gradient(
    (0.0, 0.0), (1.0, 1.0),
    vec![Color32::RED, Color32::BLUE],
).with_blend_mode(BlendMode::Multiply);

// Pattern 4: Spotlight (focused attention)
let spotlight = RenderShaderMask::radial_gradient(
    (0.5, 0.5), 0.5,
    vec![Color32::WHITE, Color32::BLACK],
);

// Pattern 5: Colored vignette (creative atmosphere)
let colored = RenderShaderMask::radial_gradient(
    (0.5, 0.5), 0.8,
    vec![Color32::WHITE, Color32::from_rgb(150, 100, 200)],
).with_blend_mode(BlendMode::Multiply);
```

**Performance Characteristics:**

- **Shader Compilation**: Cached per shader type (SolidMask, LinearGradientMask, RadialGradientMask)
- **Texture Pooling**: Offscreen textures reused via texture pool to minimize GPU allocations
- **GPU Execution**: All masking operations run on GPU via WGSL shaders
- **First Use Cost**: ~1-2ms for shader compilation (subsequent uses: < 0.1ms)

**Implementation Status:**

✅ Phase 1: ShaderMaskLayer (GPU infrastructure) - COMPLETE
✅ Phase 3: RenderObject Integration - COMPLETE
- Canvas API: `Canvas::draw_shader_mask()` (crates/flui_painting/src/canvas.rs)
- RenderShaderMask paint implementation (crates/flui_rendering/src/objects/effects/shader_mask.rs:122)
- Offscreen rendering with texture pooling
- WGSL shaders: solid, linear gradient, radial gradient
- Type consolidation: `ShaderSpec` in flui_types

✅ Phase 2: BackdropFilterLayer - COMPLETE

**BackdropFilterLayer** applies image filters (blur, color adjustments) to backdrop content:

- **Architecture**: Capture framebuffer → Apply GPU filter → Composite with child
- **Filters**: Gaussian blur, dilate, erode, matrix, color, compose
- **Blur Implementation**: Two-pass separable Gaussian (horizontal + vertical)
- **Shaders**: `gaussian_blur_horizontal.wgsl`, `gaussian_blur_vertical.wgsl`
- **Performance**: O(n) complexity per pixel (vs O(n²) for 2D blur)

**Usage Example:**

```rust
use flui_rendering::RenderBackdropFilter;
use flui_types::painting::ImageFilter;

// Frosted glass effect with 10px blur
let backdrop_filter = RenderBackdropFilter::blur(10.0);
```

**Canvas API:**

```rust
ctx.canvas().draw_backdrop_filter(
    bounds,
    ImageFilter::blur(5.0),
    BlendMode::SrcOver,
    Some(|canvas| {
        // Draw child content on top of filtered backdrop
        canvas.draw_rect(rect, paint);
    }),
);
```

**Examples:**

- `examples/shader_mask_gradient.rs` - Gradient fade effects (horizontal, vertical, diagonal)
- `examples/shader_mask_vignette.rs` - Vignette effects (classic, soft, spotlight, colored)
- ⏸️ `examples/backdrop_filter_frosted.rs` - Frosted glass (pending visual examples)

**See Also:**

- OpenSpec proposal: `openspec/changes/add-compositor-layer-support/`
- ShaderMask implementation: `crates/flui_engine/src/layer/shader_mask.rs`
- BackdropFilter implementation: `crates/flui_engine/src/layer/backdrop_filter.rs`
- Shader compiler: `crates/flui_engine/src/layer/shader_compiler.rs`
- Gaussian blur shaders: `crates/flui_engine/src/layer/shaders/gaussian_blur_*.wgsl`
- Offscreen renderer: `crates/flui_engine/src/layer/offscreen_renderer.rs`

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

### Using RenderSliverProxy Pattern

**IMPORTANT:** Use `RenderSliverProxy` for sliver objects that pass through layout unchanged but need to modify painting, hit testing, or semantics.

The RenderSliverProxy pattern is a zero-cost abstraction for implementing single-child sliver objects that act as lightweight wrappers around their child. Common examples include opacity, ignoring pointer events, and offstage rendering.

```rust
use flui_rendering::core::{RenderSliverProxy, PaintContext, PaintTree, Single};

/// Sliver that applies opacity to its child
pub struct RenderSliverOpacity {
    pub opacity: f32,
}

impl RenderSliverOpacity {
    pub fn new(opacity: f32) -> Self {
        Self {
            opacity: opacity.clamp(0.0, 1.0),
        }
    }
}

impl RenderSliverProxy for RenderSliverOpacity {
    // Layout: Default implementation passes constraints through unchanged
    // No need to override unless you modify constraints

    // Paint: Custom implementation applies opacity
    fn proxy_paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: PaintTree,
    {
        // Apply opacity effect, then paint child
        // TODO: Apply opacity layer when canvas API supports it
        ctx.proxy();  // Paint child
    }
}
```

**When to use RenderSliverProxy:**
- ✅ Single-child sliver objects
- ✅ Layout constraints pass through unchanged
- ✅ Only paint/hit-test/semantics behavior differs
- ✅ Examples: opacity, ignore pointer, offstage, clipping

**When NOT to use RenderSliverProxy:**
- ❌ Layout needs modification (use SliverRender<Single> instead)
- ❌ Multiple children (use SliverRender<Variable>)
- ❌ Complex geometry transformations (implement full SliverRender)

**Built-in Proxy Objects:**

```rust
// Opacity - applies transparency
RenderSliverOpacity::new(0.5)

// Ignore pointer - blocks pointer events
RenderSliverIgnorePointer::new(true)

// Offstage - hides content (keeps in layout)
RenderSliverOffstage::new(true)

// Animated opacity - optimized for animations
RenderSliverAnimatedOpacity::new(1.0)

// Constrained cross-axis - limits cross-axis extent
RenderSliverConstrainedCrossAxis::new(200.0)
```

**Key Benefits:**
- ✅ One-line implementations for simple proxies
- ✅ Zero overhead - compiles to direct pass-through
- ✅ Automatic protocol compliance
- ✅ Type-safe child access via PaintContext
- ✅ Consistent with Flutter's RenderProxyBox pattern

**Implementation Details:**
- Default `proxy_layout()` passes constraints through unchanged
- Default `proxy_paint()` just calls `ctx.proxy()` to paint child
- Override only the methods you need to customize
- All proxy objects have `Single` arity (exactly one child)

**See also:**
- Implementation: `crates/flui_rendering/src/objects/sliver/sliver_opacity.rs`
- More examples: `crates/flui_rendering/src/objects/sliver/sliver_animated_opacity.rs`
- Proxy trait definition: `crates/flui_rendering/src/core/sliver_proxy.rs`

### Superior Design Patterns (Better than Flutter)

FLUI demonstrates several design improvements over Flutter's architecture that eliminate code duplication and improve type safety.

#### Generic Clip Pattern

**Problem in Flutter:** Four separate clip classes with ~400 lines of duplicated code
**FLUI Solution:** Single generic `RenderClip<S: ClipShape>` trait

```rust
use flui_rendering::core::{RenderClip, ClipShape};
use flui_types::{Canvas, Size, Offset, Rect, Path};

// Define the clip shape behavior
pub trait ClipShape: std::fmt::Debug + Send + Sync {
    fn apply_clip(&self, canvas: &mut Canvas, size: Size);
    fn contains_point(&self, position: Offset, size: Size) -> bool {
        // Default rectangular bounds check
        position.dx >= 0.0 && position.dy >= 0.0 &&
        position.dx <= size.width && position.dy <= size.height
    }
}

// Specific clip shapes
pub struct RectShape;
impl ClipShape for RectShape {
    fn apply_clip(&self, canvas: &mut Canvas, size: Size) {
        let clip_rect = Rect::from_xywh(0.0, 0.0, size.width, size.height);
        canvas.clip_rect(clip_rect);
    }
}

// Type aliases for convenience
pub type RenderClipRect = RenderClip<RectShape>;
pub type RenderClipRRect = RenderClip<RRectShape>;
pub type RenderClipOval = RenderClip<OvalShape>;
pub type RenderClipPath = RenderClip<PathShape>;
```

**Benefits:**
- ✅ Eliminates ~400 lines of code duplication vs Flutter
- ✅ Type-safe: Compile-time guarantees for clip shapes
- ✅ Extensible: Add new clip shapes without modifying core
- ✅ Shared logic: Hit testing, bounds checking all in one place
- ✅ Zero-cost abstraction: Compiles to same performance as hand-written code

**Implementation:** `crates/flui_rendering/src/objects/effects/clip_base.rs`

#### Optional Arity for Decorative Boxes

**Problem in Flutter:** DecoratedBox requires a child even for pure decoration
**FLUI Solution:** Use `RenderBox<Optional>` arity for flexible child handling

```rust
use flui_rendering::core::{RenderBox, Optional, LayoutContext, PaintContext};

impl RenderBox<Optional> for RenderDecoratedBox {
    fn layout(&mut self, mut ctx: LayoutContext<Optional>) -> Size {
        if let Some(child_id) = ctx.children.get() {
            // Has child - use child size
            ctx.layout_child(child_id, ctx.constraints)
        } else {
            // No child - use constraints for decorative box
            Size::new(ctx.constraints.max_width, ctx.constraints.max_height)
        }
    }

    fn paint(&self, ctx: &mut PaintContext<Optional>) {
        // Paint background decoration
        if self.position == DecorationPosition::Background {
            self.paint_decoration(ctx.canvas(), rect);
        }

        // Paint child if present
        if let Some(child_id) = ctx.children.get() {
            ctx.paint_child(child_id, offset);
        }

        // Paint foreground decoration
        if self.position == DecorationPosition::Foreground {
            self.paint_decoration(ctx.canvas(), rect);
        }
    }
}
```

**Objects using Optional arity:**
- `RenderDecoratedBox` - Decorative backgrounds without child
- `RenderPhysicalModel` - Material elevation shadow only
- `RenderPhysicalShape` - Custom shape shadows only
- `RenderCustomPaint` - Custom painting without child

**Benefits:**
- ✅ More flexible API: Child is optional, not required
- ✅ Matches Flutter behavior: CustomPaint works without child
- ✅ Better semantics: Decorative use cases are explicit
- ✅ Type-safe: Compiler enforces Optional handling

**Fixed in:** Validation proposal `validate-effects-against-flutter`

#### Clipper Delegate Pattern with Closures

**Problem in Flutter:** Uses abstract `CustomClipper<T>` class requiring inheritance
**FLUI Solution:** Use Rust closures with `Send + Sync` bounds

```rust
use flui_types::{Size, Path};

// Type alias for clipper function
pub type ShapeClipper = Box<dyn Fn(Size) -> Path + Send + Sync>;

pub struct RenderPhysicalShape {
    clipper: ShapeClipper,
    elevation: f32,
    color: Color,
    // ...
}

impl RenderPhysicalShape {
    pub fn new(clipper: ShapeClipper, elevation: f32, color: Color) -> Self {
        Self { clipper, elevation, color, /* ... */ }
    }

    fn get_shape_path(&self) -> Path {
        (self.clipper)(self.size)  // Call closure
    }
}

// Usage: Create star shape with custom clipper
let star_clipper = Box::new(|size| {
    let mut path = Path::new();
    // ... create star shape using size
    path
});
let star = RenderPhysicalShape::new(star_clipper, 4.0, Color::YELLOW);
```

**Benefits:**
- ✅ Idiomatic Rust: Uses closures instead of inheritance
- ✅ Thread-safe: `Send + Sync` bounds ensure safety
- ✅ Zero-cost: Function pointer has no runtime overhead
- ✅ Flexible: Capture environment in closure
- ✅ Testable: Easy to create mock clippers

**Implementation:** `crates/flui_rendering/src/objects/effects/physical_shape.rs`

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

### Using Transform API for 2D Transformations

**IMPORTANT:** Use the high-level `Transform` enum instead of raw `Matrix4` for 2D transformations to improve code readability and reduce errors.

The Transform API is located in `flui_types::geometry::Transform` and provides type-safe, zero-cost abstractions for common 2D transformations.

```rust
use flui_types::geometry::Transform;
use std::f32::consts::PI;

// Basic transforms
let translate = Transform::translate(50.0, 100.0);
let rotate = Transform::rotate(PI / 4.0);  // 45 degrees
let scale = Transform::scale(2.0);  // Uniform scale
let scale_xy = Transform::scale_xy(2.0, 3.0);  // Non-uniform

// Skew transforms (for italic text, perspective effects)
let italic = Transform::skew(0.2, 0.0);  // Horizontal shear ~11.3°
let perspective = Transform::skew(0.3, 0.3);  // Both axes

// Pivot point transforms
let rotate_around_center = Transform::rotate_around(
    PI / 2.0,  // 90 degrees
    button_center_x,
    button_center_y,
);

// Fluent composition API
let composed = Transform::translate(50.0, 50.0)
    .then(Transform::rotate(PI / 4.0))
    .then(Transform::scale(2.0));

// Convert to Matrix4 (idiomatic Rust)
let matrix: Matrix4 = transform.into();

// Query transform properties
if transform.has_rotation() {
    // ...
}

// Inverse transforms (for hit testing, animations)
let inverse = transform.inverse().unwrap();
```

**Key Benefits:**
- ✅ Type-safe and self-documenting code
- ✅ Automatic composition flattening and identity optimization
- ✅ First-class skew support for italic text and perspective
- ✅ Built-in pivot point transforms
- ✅ Zero-cost abstraction (compiles to same code as Matrix4)
- ✅ Idiomatic From/Into trait conversions

**When to use Transform vs Matrix4:**
- ✅ Use `Transform` for: UI layouts, animations, simple 2D effects, composing transforms
- ⚠️ Use `Matrix4` for: 3D transformations, GPU shader inputs, arbitrary affine matrices

**Common Patterns:**

```rust
// Pattern 1: UI Container with zoom
let container = Transform::translate(100.0, 100.0)
    .then(Transform::scale(1.5));

// Pattern 2: Button rotation animation
let angle = lerp(0.0, PI * 2.0, animation_t);
let rotation = Transform::rotate_around(angle, center_x, center_y);

// Pattern 3: Italic text rendering
let italic = Transform::skew(0.2, 0.0);
canvas.save();
canvas.transform(italic);
canvas.draw_text("Italic Text", position, style);
canvas.restore();

// Pattern 4: Card flip with perspective
let card_flip = Transform::rotate(PI)
    .then(Transform::skew(0.2, 0.0))
    .then(Transform::translate(0.0, 10.0));
```

**See also:**
- Full API documentation: `cargo doc -p flui_types --open`
- Usage examples: `examples/transform_demo.rs`
- OpenSpec proposal: `openspec/changes/add-transform-api/`

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

### Foundation

```toml
flui-foundation = { version = "0.1", features = ["serde", "async"] }
```

**Features:**
- `serde` - Enable serialization support
- `async` - Enable async change notification

### Rendering Backend

FLUI uses **wgpu** as its only rendering backend for GPU-accelerated graphics:

```toml
flui = { version = "0.1", features = ["devtools"] }
```

**Features:**
- `devtools` - Enable development and debugging tools
- `full` - Enable all stable features

## Documentation

Comprehensive documentation is available in each crate:

**Foundation Layer:**
- `crates/flui-foundation/README.md` - Core types and change notification
- `crates/flui-tree/README.md` - Tree abstractions and visitor patterns
- `crates/flui_types/README.md` - Basic geometry and math

**Framework Layer:**
- `crates/flui-view/README.md` - View traits and abstractions  
- `crates/flui-pipeline/README.md` - Pipeline coordination system
- `crates/flui-reactivity/README.md` - Reactive state management
- `crates/flui-scheduler/README.md` - Frame scheduling
- `crates/flui_core/README.md` - Core framework implementation

**Architecture:**
- `docs/arch/README.md` - Overall architecture overview
- `docs/arch/CORE_ARCHITECTURE.md` - Core framework design
- `docs/arch/RENDERING_ARCHITECTURE.md` - Rendering system

**Development:**
- `crates/flui_cli/README.md` - CLI tool documentation
- `crates/flui_devtools/README.md` - Development tools
- `crates/flui_build/README.md` - Build system

**Examples:**
- `examples/` - Application examples
- `demos/` - Demo applications

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

## Recent Completed Work (2025-11-26)
