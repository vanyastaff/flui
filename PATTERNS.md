# FLUI Development Patterns

This file contains common development patterns and best practices for FLUI development.

## Table of Contents

- [Creating Views](#creating-views)
- [Creating RenderObjects](#creating-renderobjects)
- [Using GAT Metadata](#using-gat-metadata)
- [RenderSliverProxy Pattern](#rendersliverproxy-pattern)
- [Superior Design Patterns](#superior-design-patterns)
- [Using Hooks for State](#using-hooks-for-state)
- [Using Transform API](#using-transform-api)
- [Advanced Visual Effects](#advanced-visual-effects)

## Creating Views

### Simple View (New API)

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

## Creating RenderObjects

Choose trait based on child count:

### No Children (LeafRender)

```rust
impl LeafRender for RenderText {
    type Metadata = ();

    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Compute size
    }

    fn paint(&self, offset: Offset) -> BoxedLayer {
        // Draw text
    }
}
```

### One Child (SingleRender)

```rust
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
```

### Multiple Children (MultiRender)

```rust
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

## Using GAT Metadata

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

## RenderSliverProxy Pattern

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

### When to use RenderSliverProxy

- ✅ Single-child sliver objects
- ✅ Layout constraints pass through unchanged
- ✅ Only paint/hit-test/semantics behavior differs
- ✅ Examples: opacity, ignore pointer, offstage, clipping

### When NOT to use RenderSliverProxy

- ❌ Layout needs modification (use SliverRender<Single> instead)
- ❌ Multiple children (use SliverRender<Variable>)
- ❌ Complex geometry transformations (implement full SliverRender)

### Built-in Proxy Objects

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

## Superior Design Patterns

FLUI demonstrates several design improvements over Flutter's architecture that eliminate code duplication and improve type safety.

### Generic Clip Pattern

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

### Optional Arity for Decorative Boxes

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

### Clipper Delegate Pattern with Closures

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

## Using Hooks for State

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

## Using Transform API

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

### Common Transform Patterns

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

**Common Shader Mask Patterns:**

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

### BackdropFilter Effects

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
- `examples/backdrop_filter_frosted.rs` - Frosted glass effects

**See Also:**

- OpenSpec proposal: `openspec/changes/add-compositor-layer-support/`
- ShaderMask implementation: `crates/flui_engine/src/layer/shader_mask.rs`
- BackdropFilter implementation: `crates/flui_engine/src/layer/backdrop_filter.rs`
- Shader compiler: `crates/flui_engine/src/layer/shader_compiler.rs`
- Gaussian blur shaders: `crates/flui_engine/src/layer/shaders/gaussian_blur_*.wgsl`
- Offscreen renderer: `crates/flui_engine/src/layer/offscreen_renderer.rs`
