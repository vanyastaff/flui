# Design: Compositor Layer Support Architecture

## Overview

This document describes the technical architecture for adding compositor-level layer support to FLUI, enabling advanced visual effects (ShaderMask and BackdropFilter) that cannot be implemented using the Canvas API alone.

## Current Architecture

### Layer System (As-Is)

```
flui_engine/src/layer/
├── mod.rs              # Exports CanvasLayer only
└── picture.rs          # CanvasLayer implementation
```

**CanvasLayer** is the only layer type:
- Contains a `Canvas` (from flui_painting)
- Canvas contains a `DisplayList` (vector of DrawCommands)
- Rendered via `CommandRenderer` (Visitor Pattern)

**Design Philosophy:**
> "All layer effects (Transform, Opacity, Clip, etc.) are implemented as RenderObjects in flui_rendering, not here."

This works because these effects can be expressed as Canvas drawing operations.

### Why Current Architecture is Insufficient

**Problem 1: ShaderMask requires offscreen rendering**
- Need to render child to intermediate texture
- Apply shader as mask to texture
- Composite masked result
- **Canvas API limitation:** Cannot render to texture, only to framebuffer

**Problem 2: BackdropFilter requires backdrop access**
- Need to capture framebuffer content BEFORE this widget
- Apply image filter to captured content
- Render filtered backdrop + child on top
- **Canvas API limitation:** Cannot access previous layer content

## Proposed Architecture

### Layer Hierarchy (New Design)

```
┌─────────────────────────────────────┐
│          Layer Trait                │  ← Abstract interface
│  - fn render(renderer)              │
│  - fn bounds() -> Rect              │
└─────────────────────────────────────┘
            △
            │ implements
            │
┌───────────┴──────────────────────────────────────────┐
│                                                       │
│ CanvasLayer        ShaderMaskLayer    BackdropFilterLayer
│ (existing)         (NEW)              (NEW)
│                                                       │
│ - Canvas           - child: Layer    - child: Layer  │
│                    - shader: Spec    - filter: Filter│
│                    - blend_mode      - blend_mode    │
└───────────────────────────────────────────────────────┘
```

### Layer Trait (Interface)

```rust
/// Abstract layer interface for compositor
pub trait Layer: Send + Sync {
    /// Render this layer using the provided renderer
    fn render(&self, renderer: &mut dyn CommandRenderer);

    /// Get the bounding rectangle of this layer
    fn bounds(&self) -> Rect;
}
```

**Design Decision:** Keep minimal interface
- ✅ Simple to implement
- ✅ Extensible for future layer types
- ✅ Compatible with existing Visitor Pattern (CommandRenderer)

## Component Designs

### 1. ShaderMaskLayer

#### Data Structure

```rust
/// Layer that applies a shader as a mask to its child
pub struct ShaderMaskLayer {
    /// Child layer to mask (required)
    pub child: Box<dyn Layer>,

    /// Shader specification (gradient, pattern, etc.)
    pub shader: ShaderSpec,

    /// Blend mode for compositing
    pub blend_mode: BlendMode,

    /// Bounds for rendering (pre-computed)
    pub bounds: Rect,
}
```

**Design Decisions:**
- **Owned child:** `Box<dyn Layer>` enables layer tree composition
- **ShaderSpec:** Enum for different shader types (matches existing code in flui_rendering)
- **Pre-computed bounds:** Avoid traversing tree during render

#### Rendering Algorithm

```
1. ALLOCATE offscreen texture T (RGBA8, bounds.size())
2. CREATE render pass with T as attachment
3. RENDER child layer to T
4. CREATE shader pipeline with mask shader
5. BIND texture T as input
6. DRAW fullscreen quad with shader (applies mask)
7. COMPOSITE result to main framebuffer with blend_mode
8. FREE or pool texture T
```

**GPU Operations:**
```wgsl
// Shader mask fragment shader (example: linear gradient)
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Sample child texture
    let color = textureSample(child_texture, sampler, in.uv);

    // Compute mask value from gradient
    let t = dot(in.uv - start, direction);
    let mask_alpha = mix(colors[0].a, colors[1].a, t);

    // Apply mask
    return vec4<f32>(color.rgb, color.a * mask_alpha);
}
```

#### Resource Management

**Texture Pooling Strategy:**
```rust
pub struct TexturePool {
    available: Vec<Texture>,
    in_use: HashMap<TextureId, Texture>,
}

impl TexturePool {
    /// Acquire texture of given size (reuse if available)
    pub fn acquire(&mut self, size: Size) -> TextureHandle;

    /// Release texture back to pool
    pub fn release(&mut self, handle: TextureHandle);
}
```

**Design Decision:** Pool textures to avoid allocation overhead
- ✅ Reuse textures across frames
- ✅ Configurable max pool size
- ⚠️ Monitor GPU memory usage

### 2. BackdropFilterLayer

#### Data Structure

```rust
/// Layer that applies a filter to the backdrop (content behind)
pub struct BackdropFilterLayer {
    /// Child layer to render on top (optional)
    pub child: Option<Box<dyn Layer>>,

    /// Image filter to apply (blur, etc.)
    pub filter: ImageFilter,

    /// Blend mode for compositing
    pub blend_mode: BlendMode,

    /// Bounds for filtering (pre-computed)
    pub bounds: Rect,
}
```

**Design Decisions:**
- **Optional child:** Matches Flutter (can be pure backdrop filter with no child)
- **ImageFilter:** Reuse existing type from flui_types::painting
- **Bounds:** Define region of backdrop to capture and filter

#### Rendering Algorithm

```
1. CAPTURE current framebuffer region (bounds) to texture B
2. CREATE filter pipeline (e.g., Gaussian blur)
3. APPLY filter to texture B → filtered texture F
   (May require multi-pass for quality filters)
4. RENDER filtered texture F to framebuffer with blend_mode
5. IF child is Some:
   5a. RENDER child layer on top
6. FREE or pool textures B, F
```

**GPU Operations (Blur Example):**
```wgsl
// Two-pass Gaussian blur for quality

// Pass 1: Horizontal blur
@fragment
fn blur_horizontal(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel_size = 1.0 / texture_size;
    var color = vec4<f32>(0.0);

    for (var i = -radius; i <= radius; i++) {
        let offset = vec2<f32>(f32(i) * texel_size.x, 0.0);
        color += textureSample(input_texture, sampler, in.uv + offset) * weights[i + radius];
    }

    return color;
}

// Pass 2: Vertical blur
@fragment
fn blur_vertical(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel_size = 1.0 / texture_size;
    var color = vec4<f32>(0.0);

    for (var i = -radius; i <= radius; i++) {
        let offset = vec2<f32>(0.0, f32(i) * texel_size.y);
        color += textureSample(input_texture, sampler, in.uv + offset) * weights[i + radius];
    }

    return color;
}
```

#### Framebuffer Capture

**Challenge:** wgpu doesn't directly support "read current framebuffer"

**Solution:** Render order tracking
```rust
pub struct Compositor {
    /// Layers rendered so far this frame
    layer_stack: Vec<Box<dyn Layer>>,

    /// Current composite texture (accumulates rendering)
    composite_texture: Texture,
}

impl Compositor {
    /// Called by BackdropFilterLayer during render
    pub fn capture_backdrop(&self, bounds: Rect) -> Texture {
        // Copy region from composite_texture
        self.copy_texture_region(self.composite_texture, bounds)
    }
}
```

**Design Decision:** Compositor maintains composite texture
- ✅ Enables backdrop capture
- ✅ Natural for layer-based rendering
- ⚠️ Requires refactoring current rendering flow

### 3. PaintContext Extensions

#### API Design

```rust
impl PaintContext<'_, T, A> {
    /// Push a shader mask layer
    ///
    /// Renders child using the provided closure, then applies shader mask.
    ///
    /// # Example
    /// ```rust
    /// ctx.push_shader_mask(
    ///     ShaderSpec::LinearGradient { ... },
    ///     BlendMode::SrcOver,
    ///     |ctx| {
    ///         ctx.paint_child(child_id, offset);
    ///     },
    /// );
    /// ```
    pub fn push_shader_mask<F>(
        &mut self,
        shader: ShaderSpec,
        blend_mode: BlendMode,
        paint_child: F,
    ) where
        F: FnOnce(&mut PaintContext<'_, T, A>),
    {
        // 1. Create new paint context for child
        let mut child_ctx = self.create_child_context();

        // 2. Paint child (callback)
        paint_child(&mut child_ctx);

        // 3. Extract child layer
        let child_layer = child_ctx.into_layer();

        // 4. Create ShaderMaskLayer
        let mask_layer = ShaderMaskLayer {
            child: Box::new(child_layer),
            shader,
            blend_mode,
            bounds: child_layer.bounds(),
        };

        // 5. Add to current layer stack
        self.add_layer(Box::new(mask_layer));
    }

    /// Push a backdrop filter layer
    ///
    /// Filters the backdrop, then renders child using the provided closure.
    ///
    /// # Example
    /// ```rust
    /// ctx.push_backdrop_filter(
    ///     ImageFilter::blur(10.0),
    ///     BlendMode::SrcOver,
    ///     |ctx| {
    ///         ctx.paint_child(child_id, offset);
    ///     },
    /// );
    /// ```
    pub fn push_backdrop_filter<F>(
        &mut self,
        filter: ImageFilter,
        blend_mode: BlendMode,
        paint_child: F,
    ) where
        F: FnOnce(&mut PaintContext<'_, T, A>),
    {
        // Similar to push_shader_mask but creates BackdropFilterLayer
        // ...
    }
}
```

**Design Decisions:**
- **Closure API:** Matches Flutter's pattern (`context.pushLayer(..., super.paint)`)
- **Automatic layer creation:** RenderObjects don't need to know about layer internals
- **Type-safe:** Compiler ensures correct usage

### 4. CommandRenderer Integration

#### Current Visitor Pattern

```rust
pub trait CommandRenderer {
    fn draw_rect(&mut self, rect: Rect, paint: &Paint);
    fn draw_circle(&mut self, center: Point, radius: f32, paint: &Paint);
    // ... other drawing commands
}

pub fn dispatch_commands(commands: &[DrawCommand], renderer: &mut dyn CommandRenderer) {
    for command in commands {
        match command {
            DrawCommand::Rect { rect, paint } => renderer.draw_rect(*rect, paint),
            DrawCommand::Circle { center, radius, paint } => renderer.draw_circle(*center, *radius, paint),
            // ... other commands
        }
    }
}
```

#### Extended for Layers

**Option A: Add layer rendering to CommandRenderer (REJECTED)**
```rust
// ❌ Breaks single responsibility - CommandRenderer is for draw commands
trait CommandRenderer {
    fn render_shader_mask_layer(&mut self, layer: &ShaderMaskLayer);
    fn render_backdrop_filter_layer(&mut self, layer: &BackdropFilterLayer);
}
```

**Option B: Separate LayerRenderer trait (CHOSEN)**
```rust
/// Renderer specifically for layers (compositor operations)
pub trait LayerRenderer {
    fn render_canvas_layer(&mut self, layer: &CanvasLayer);
    fn render_shader_mask_layer(&mut self, layer: &ShaderMaskLayer);
    fn render_backdrop_filter_layer(&mut self, layer: &BackdropFilterLayer);
}

impl LayerRenderer for WgpuRenderer {
    fn render_shader_mask_layer(&mut self, layer: &ShaderMaskLayer) {
        // Allocate offscreen texture
        let texture = self.texture_pool.acquire(layer.bounds.size());

        // Render child to texture
        let render_pass = self.create_render_pass(&texture);
        layer.child.render(&mut render_pass);

        // Apply shader mask
        self.apply_shader_mask(&texture, &layer.shader);

        // Composite to main framebuffer
        self.composite(&texture, layer.blend_mode);

        // Release texture
        self.texture_pool.release(texture);
    }
}
```

**Design Decision:** Separate concerns
- ✅ CommandRenderer for draw commands (Canvas operations)
- ✅ LayerRenderer for compositor operations (layer composition)
- ✅ Single Responsibility Principle maintained

## Integration with Existing Systems

### Paint Pipeline Flow

**Before (Current):**
```
RenderObject.paint()
    ↓
PaintContext (accumulates Canvas commands)
    ↓
CanvasLayer (contains DisplayList)
    ↓
CommandRenderer.dispatch_commands()
    ↓
WgpuRenderer (GPU rendering)
```

**After (With Layers):**
```
RenderObject.paint()
    ↓
PaintContext (can push layers OR canvas commands)
    ├─→ Canvas commands → CanvasLayer
    └─→ push_shader_mask/push_backdrop_filter → ShaderMaskLayer/BackdropFilterLayer
    ↓
Layer tree (can be nested)
    ↓
LayerRenderer (renders layer tree)
    ├─→ CanvasLayer → CommandRenderer.dispatch_commands() → WgpuRenderer
    ├─→ ShaderMaskLayer → WgpuRenderer.render_shader_mask()
    └─→ BackdropFilterLayer → WgpuRenderer.render_backdrop_filter()
```

### Thread Safety

**Requirement:** All layer types must be `Send + Sync` (project constraint)

**Implementation:**
```rust
// Safe: Contains only owned data
pub struct ShaderMaskLayer {
    child: Box<dyn Layer>,  // ✅ Box is Send if Layer is Send
    shader: ShaderSpec,     // ✅ Enum with Send types
    blend_mode: BlendMode,  // ✅ Copy type
    bounds: Rect,           // ✅ Copy type
}

unsafe impl Send for ShaderMaskLayer {}
unsafe impl Sync for ShaderMaskLayer {}
```

**Design Decision:** No shared mutable state in layers
- ✅ Layers are immutable after creation
- ✅ Rendering is stateless (reads layer data, writes to GPU)
- ✅ Thread-safe by construction

## Performance Considerations

### Memory Usage

**ShaderMaskLayer:**
- Offscreen texture: `width × height × 4 bytes` (RGBA8)
- Example: 1920×1080 = **8.3 MB per layer**
- Mitigation: Texture pooling, limit nesting depth

**BackdropFilterLayer:**
- Backdrop texture: Same as ShaderMask
- Filter intermediate textures: 1-2× input size (multi-pass filters)
- Example: 1920×1080 blur = **16-24 MB per layer**
- Mitigation: Texture pooling, warn in docs about performance

### GPU Performance

**ShaderMask:**
- Render pass: ~0.5-1 ms (1080p, modern GPU)
- Shader application: ~0.2-0.5 ms
- **Total: ~1-2 ms per layer**

**BackdropFilter:**
- Framebuffer capture: ~0.5-1 ms
- Blur (two-pass): ~2-4 ms (radius-dependent)
- Composition: ~0.2-0.5 ms
- **Total: ~3-6 ms per layer**

**Design Decision:** Document as expensive operations
- ✅ Add performance warnings in API docs
- ✅ Recommend RepaintBoundary to cache layers
- ✅ Provide benchmarks in examples

### Optimization Opportunities

**Layer Caching (Future Work):**
```rust
pub struct CachedLayer {
    layer: Box<dyn Layer>,
    cached_texture: Option<Texture>,
    cache_valid: bool,
}
```
- Avoid re-rendering unchanged layers
- Implement via RepaintBoundary (Phase 2)

**Shader Pre-compilation:**
```rust
pub struct ShaderCache {
    compiled: HashMap<ShaderKey, wgpu::ShaderModule>,
}
```
- Compile shaders at startup, not first use
- Reduce frame time spikes

## Testing Strategy

### Unit Tests

**Layer Creation:**
```rust
#[test]
fn test_shader_mask_layer_bounds() {
    let child = CanvasLayer::new();
    let layer = ShaderMaskLayer {
        child: Box::new(child),
        shader: ShaderSpec::Solid(Color32::WHITE),
        blend_mode: BlendMode::SrcOver,
        bounds: Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
    };

    assert_eq!(layer.bounds(), Rect::from_xywh(0.0, 0.0, 100.0, 100.0));
}
```

**Rendering (Integration):**
```rust
#[test]
fn test_shader_mask_renders_gradient() {
    let mut renderer = WgpuRenderer::new_headless();
    let layer = ShaderMaskLayer::linear_gradient(...);

    layer.render(&mut renderer);

    // Verify texture was allocated and freed
    assert_eq!(renderer.texture_pool.in_use_count(), 0);
}
```

### Visual Testing

**Golden Image Tests:**
```rust
#[test]
fn test_shader_mask_matches_expected() {
    let output = render_to_image(shader_mask_scene);
    let expected = load_golden_image("shader_mask_gradient.png");

    assert_images_similar(output, expected, tolerance = 0.01);
}
```

**Manual Testing:**
- Run examples visually
- Compare side-by-side with Flutter equivalents
- Verify on different GPUs (Intel, NVIDIA, AMD)

## Migration Path

### Phase 1: ShaderMask (Lower Risk)

1. Implement ShaderMaskLayer
2. Add texture pooling
3. Update RenderShaderMask
4. Test and validate

**Risk:** Moderate (offscreen rendering is well-understood)

### Phase 2: BackdropFilter (Higher Risk)

1. Refactor compositor to track composite texture
2. Implement BackdropFilterLayer
3. Implement blur filter
4. Update RenderBackdropFilter
5. Test and validate

**Risk:** High (requires compositor refactoring)

**Mitigation:** Thorough testing, incremental rollout

## Open Questions

1. **Async readback for BackdropFilter?**
   - Current: Synchronous (blocks GPU)
   - Future: Async with `wgpu::Buffer::map_async`
   - Decision: Start synchronous, optimize later

2. **Layer caching strategy?**
   - RepaintBoundary integration needed
   - Invalidation on property changes
   - Decision: Document for future work (out of scope)

3. **WGSL vs GLSL shaders?**
   - WGSL is wgpu native
   - Better tooling and validation
   - Decision: Use WGSL exclusively

## References

### External Documentation

- [wgpu Examples - Render to Texture](https://github.com/gfx-rs/wgpu/tree/trunk/examples/render-to-texture)
- [wgpu Tutorial - Textures](https://sotrh.github.io/learn-wgpu/beginner/tutorial5-textures/)
- [Flutter Layer Compositing](https://api.flutter.dev/flutter/rendering/Layer-class.html)
- [Gaussian Blur Implementation](https://en.wikipedia.org/wiki/Gaussian_blur)

### Internal Documentation

- Current layer system: `crates/flui_engine/src/layer/picture.rs`
- CommandRenderer: `crates/flui_engine/src/renderer/mod.rs`
- Paint pipeline: `crates/flui_core/src/pipeline/paint.rs`
- Validation report: `openspec/changes/validate-effects-against-flutter/validation-report.md`
