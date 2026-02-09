# Paint System Architecture

## Overview

The Paint system is responsible for rendering visual output. Unlike Layout and HitTest, paint backends **truly vary** across platforms and use cases, making decomposition worthwhile.

## Why Paint is Different

### Layout vs HitTest vs Paint

```
Layout:
├─ Constraints/Geometry types vary by protocol ✅
└─ Sizing/Positioning logic varies by widget ✅
  → Simple LayoutCapability (3 types)

HitTest:
├─ Position/Result types vary by protocol ✅
└─ Testing logic varies by widget ✅
  → Simple HitTestCapability (4 types)

Paint:
├─ Backend varies by platform ✅✅✅
│   ├─ Desktop: Skia (CPU or GPU)
│   ├─ Web: Canvas2D, WebGL
│   ├─ Mobile: Metal, Vulkan
│   └─ Custom: wgpu, vello, tiny-skia
├─ Layering varies by strategy ✅✅
│   ├─ Immediate mode (simple)
│   ├─ Retained mode (display lists)
│   └─ Composited (GPU layers)
├─ Effects vary by capability ✅✅
│   ├─ CPU: Basic filters
│   └─ GPU: Custom shaders
└─ Caching varies by optimization ✅✅
    ├─ Repaint boundaries (CPU)
    └─ GPU texture cache
  → Decomposed PaintCapability (4 components)
```

**Key Difference**: Paint backends are **real architectural differences**, not just implementation choices.

## PaintCapability

### Decomposed Structure

```rust
pub trait PaintCapability: Send + Sync + 'static {
    /// Canvas backend (Skia, wgpu, Canvas2D, WebGL, etc.)
    type Canvas: CanvasApi;
    
    /// Layering strategy (simple, composited, retained)
    type Layering: LayeringStrategy;
    
    /// Effects support (filters, blend modes, shaders)
    type Effects: EffectsApi;
    
    /// Caching strategy (repaint boundaries, GPU cache)
    type Caching: CachingStrategy;
    
    /// Paint context type (GAT!)
    type Context<'ctx, A: Arity, P: ParentData>: PaintContextApi<'ctx, Self, A, P>
    where Self: 'ctx;
}
```

**Why 4 components?** Each component represents a **real backend choice**:

1. **Canvas**: What API to draw with (truly varies!)
2. **Layering**: How to organize operations (architecture choice)
3. **Effects**: What visual capabilities exist (platform-dependent)
4. **Caching**: How to optimize repaints (strategy choice)

### Standard Paint Configuration

```rust
pub struct StandardPaint;

impl PaintCapability for StandardPaint {
    type Canvas = SkiaCanvas;           // Skia backend
    type Layering = SimpleLayering;     // No layers
    type Effects = StandardEffects;     // Basic effects
    type Caching = RepaintBoundaries;   // Repaint optimization
    type Context<'ctx, A, P> = RenderContext<'ctx, StandardPaint, A, P>;
}
```

**Use case**: Default configuration for desktop/mobile using Skia.

### Web Paint Configuration

```rust
pub struct WebPaint;

impl PaintCapability for WebPaint {
    type Canvas = Canvas2DApi;          // HTML5 Canvas
    type Layering = SimpleLayering;     // Browser handles layers
    type Effects = StandardEffects;     // Limited effects
    type Caching = NoCaching;           // Browser caches
    type Context<'ctx, A, P> = RenderContext<'ctx, WebPaint, A, P>;
}
```

**Use case**: Web deployment using Canvas 2D API.

### GPU Paint Configuration

```rust
pub struct GPUPaint;

impl PaintCapability for GPUPaint {
    type Canvas = WgpuCanvas;           // wgpu backend
    type Layering = CompositedLayering; // GPU layers
    type Effects = ShaderEffects;       // Custom shaders
    type Caching = GPUCaching;          // GPU texture cache
    type Context<'ctx, A, P> = RenderContext<'ctx, GPUPaint, A, P>;
}
```

**Use case**: High-performance GPU-accelerated rendering.

## Canvas Component

### CanvasApi Trait

```rust
pub trait CanvasApi: Send + Sync + Debug {
    // ========================================================================
    // Basic Drawing
    // ========================================================================
    
    fn draw_rect(&mut self, rect: Rect, paint: &Paint);
    fn draw_rrect(&mut self, rrect: RRect, paint: &Paint);
    fn draw_circle(&mut self, center: Offset, radius: f32, paint: &Paint);
    fn draw_path(&mut self, path: &Path, paint: &Paint);
    fn draw_image(&mut self, image: &Image, offset: Offset, paint: &Paint);
    fn draw_text(&mut self, text: &str, offset: Offset, paint: &Paint);
    
    // ========================================================================
    // State Management
    // ========================================================================
    
    fn save(&mut self) -> SaveLayerHandle;
    fn restore(&mut self);
    fn translate(&mut self, dx: f32, dy: f32);
    fn scale(&mut self, sx: f32, sy: f32);
    fn rotate(&mut self, radians: f32);
    fn transform(&mut self, matrix: &Matrix4);
    
    // ========================================================================
    // Clipping
    // ========================================================================
    
    fn clip_rect(&mut self, rect: Rect);
    fn clip_rrect(&mut self, rrect: RRect);
    fn clip_path(&mut self, path: &Path);
    
    // ========================================================================
    // Backend Info
    // ========================================================================
    
    fn backend_type(&self) -> CanvasBackend;
    fn flush(&mut self);
}

pub enum CanvasBackend {
    Skia,
    Wgpu,
    Canvas2D,
    WebGL,
    Custom(&'static str),
}
```

### SkiaCanvas Implementation

```rust
pub struct SkiaCanvas {
    canvas: skia::Canvas,
}

impl CanvasApi for SkiaCanvas {
    fn draw_rect(&mut self, rect: Rect, paint: &Paint) {
        let skia_rect = skia::Rect::from_xywh(
            rect.left(),
            rect.top(),
            rect.width(),
            rect.height(),
        );
        let skia_paint = convert_paint(paint);
        self.canvas.draw_rect(skia_rect, &skia_paint);
    }
    
    fn draw_circle(&mut self, center: Offset, radius: f32, paint: &Paint) {
        let skia_paint = convert_paint(paint);
        self.canvas.draw_circle(
            skia::Point::new(center.dx, center.dy),
            radius,
            &skia_paint,
        );
    }
    
    fn save(&mut self) -> SaveLayerHandle {
        let count = self.canvas.save();
        SaveLayerHandle(count)
    }
    
    fn restore(&mut self) {
        self.canvas.restore();
    }
    
    fn translate(&mut self, dx: f32, dy: f32) {
        self.canvas.translate((dx, dy));
    }
    
    fn backend_type(&self) -> CanvasBackend {
        CanvasBackend::Skia
    }
    
    fn flush(&mut self) {
        self.canvas.flush();
    }
}
```

### WgpuCanvas Implementation

```rust
pub struct WgpuCanvas {
    device: wgpu::Device,
    queue: wgpu::Queue,
    render_pass: wgpu::RenderPass,
    // ... GPU state
}

impl CanvasApi for WgpuCanvas {
    fn draw_rect(&mut self, rect: Rect, paint: &Paint) {
        // Create GPU buffers for rectangle
        let vertices = create_rect_vertices(rect);
        let vertex_buffer = self.device.create_buffer_init(&vertices);
        
        // Set up pipeline with paint settings
        let pipeline = self.get_or_create_pipeline(paint);
        
        // Record draw command
        self.render_pass.set_pipeline(pipeline);
        self.render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        self.render_pass.draw(0..6, 0..1);
    }
    
    fn backend_type(&self) -> CanvasBackend {
        CanvasBackend::Wgpu
    }
    
    fn flush(&mut self) {
        // Submit GPU commands
        self.queue.submit(Some(self.encoder.finish()));
    }
}
```

### Canvas2DApi Implementation

```rust
pub struct Canvas2DApi {
    context: web_sys::CanvasRenderingContext2d,
}

impl CanvasApi for Canvas2DApi {
    fn draw_rect(&mut self, rect: Rect, paint: &Paint) {
        self.context.set_fill_style(&paint.color.to_js_value());
        
        match paint.style {
            PaintStyle::Fill => {
                self.context.fill_rect(
                    rect.left() as f64,
                    rect.top() as f64,
                    rect.width() as f64,
                    rect.height() as f64,
                );
            }
            PaintStyle::Stroke => {
                self.context.set_line_width(paint.stroke_width as f64);
                self.context.stroke_rect(
                    rect.left() as f64,
                    rect.top() as f64,
                    rect.width() as f64,
                    rect.height() as f64,
                );
            }
        }
    }
    
    fn save(&mut self) -> SaveLayerHandle {
        self.context.save();
        SaveLayerHandle(0)  // Browser manages state
    }
    
    fn restore(&mut self) {
        self.context.restore();
    }
    
    fn backend_type(&self) -> CanvasBackend {
        CanvasBackend::Canvas2D
    }
    
    fn flush(&mut self) {
        // No-op - browser handles flushing
    }
}
```

## Layering Component

### LayeringStrategy Trait

```rust
pub trait LayeringStrategy: Send + Sync + Debug {
    fn push_layer(&mut self, bounds: Rect, opacity: f32) -> LayerId;
    fn pop_layer(&mut self);
    fn push_opacity(&mut self, opacity: f32) -> LayerId;
    fn push_clip(&mut self, clip: Rect) -> LayerId;
    fn push_transform(&mut self, offset: Offset) -> LayerId;
    fn supports_layers(&self) -> bool;
    fn layer_depth(&self) -> usize;
}

pub struct LayerId(pub usize);
```

### SimpleLayering

```rust
/// No actual layers - uses canvas state instead.
#[derive(Debug, Default)]
pub struct SimpleLayering {
    depth: usize,
}

impl LayeringStrategy for SimpleLayering {
    fn push_layer(&mut self, _bounds: Rect, _opacity: f32) -> LayerId {
        self.depth += 1;
        LayerId(self.depth)
    }
    
    fn pop_layer(&mut self) {
        if self.depth > 0 {
            self.depth -= 1;
        }
    }
    
    fn push_opacity(&mut self, _opacity: f32) -> LayerId {
        // Use canvas globalAlpha instead
        self.push_layer(Rect::ZERO, 1.0)
    }
    
    fn supports_layers(&self) -> bool {
        false  // Uses canvas state, not real layers
    }
    
    fn layer_depth(&self) -> usize {
        self.depth
    }
}
```

### CompositedLayering

```rust
/// Separate GPU textures for each layer.
#[derive(Debug)]
pub struct CompositedLayering {
    layers: Vec<Layer>,
    device: Arc<wgpu::Device>,
}

struct Layer {
    id: LayerId,
    texture: wgpu::Texture,
    bounds: Rect,
    opacity: f32,
}

impl LayeringStrategy for CompositedLayering {
    fn push_layer(&mut self, bounds: Rect, opacity: f32) -> LayerId {
        // Create GPU texture for this layer
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            size: wgpu::Extent3d {
                width: bounds.width() as u32,
                height: bounds.height() as u32,
                depth_or_array_layers: 1,
            },
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | 
                   wgpu::TextureUsages::TEXTURE_BINDING,
            // ...
        });
        
        let id = LayerId(self.layers.len());
        self.layers.push(Layer { id, texture, bounds, opacity });
        id
    }
    
    fn pop_layer(&mut self) {
        if let Some(layer) = self.layers.pop() {
            // Composite layer to parent
            self.composite_layer(layer);
        }
    }
    
    fn supports_layers(&self) -> bool {
        true
    }
}
```

## Effects Component

### EffectsApi Trait

```rust
pub trait EffectsApi: Send + Sync + Debug {
    fn apply_opacity(&mut self, opacity: f32);
    fn apply_color_filter(&mut self, filter: ColorFilter);
    fn apply_blur(&mut self, sigma: f32);
    fn apply_shader(&mut self, shader: &Shader);
    fn supports_shaders(&self) -> bool;
    fn supports_filters(&self) -> bool;
}
```

### StandardEffects

```rust
#[derive(Debug, Default)]
pub struct StandardEffects {
    current_opacity: f32,
}

impl EffectsApi for StandardEffects {
    fn apply_opacity(&mut self, opacity: f32) {
        self.current_opacity = opacity;
    }
    
    fn apply_color_filter(&mut self, filter: ColorFilter) {
        // Basic color matrix multiplication
    }
    
    fn apply_blur(&mut self, sigma: f32) {
        // Software blur (slow)
    }
    
    fn apply_shader(&mut self, _shader: &Shader) {
        // Not supported
    }
    
    fn supports_shaders(&self) -> bool {
        false
    }
    
    fn supports_filters(&self) -> bool {
        true
    }
}
```

### ShaderEffects

```rust
#[derive(Debug)]
pub struct ShaderEffects {
    device: Arc<wgpu::Device>,
    shader_cache: HashMap<ShaderId, wgpu::ShaderModule>,
}

impl EffectsApi for ShaderEffects {
    fn apply_opacity(&mut self, opacity: f32) {
        // Use GPU blend state
    }
    
    fn apply_blur(&mut self, sigma: f32) {
        // Use GPU blur shader (fast!)
        let shader = self.get_blur_shader(sigma);
        self.apply_shader(&shader);
    }
    
    fn apply_shader(&mut self, shader: &Shader) {
        // Load and bind custom WGSL shader
        let module = self.get_or_compile_shader(shader);
        // Set up render pipeline with shader
    }
    
    fn supports_shaders(&self) -> bool {
        true
    }
    
    fn supports_filters(&self) -> bool {
        true
    }
}
```

## Caching Component

### CachingStrategy Trait

```rust
pub trait CachingStrategy: Send + Sync + Debug {
    fn should_cache(&self, id: RenderId) -> bool;
    fn mark_repaint_boundary(&mut self, id: RenderId);
    fn invalidate(&mut self, id: RenderId);
    fn invalidate_all(&mut self);
    fn cache_stats(&self) -> CacheStats;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CacheStats {
    pub total_boundaries: usize,
    pub dirty_boundaries: usize,
}
```

### RepaintBoundaries

```rust
#[derive(Debug, Default)]
pub struct RepaintBoundaries {
    boundaries: Vec<RenderId>,
    dirty: Vec<RenderId>,
}

impl CachingStrategy for RepaintBoundaries {
    fn should_cache(&self, id: RenderId) -> bool {
        self.boundaries.contains(&id)
    }
    
    fn mark_repaint_boundary(&mut self, id: RenderId) {
        if !self.boundaries.contains(&id) {
            self.boundaries.push(id);
        }
    }
    
    fn invalidate(&mut self, id: RenderId) {
        if !self.dirty.contains(&id) {
            self.dirty.push(id);
        }
    }
    
    fn invalidate_all(&mut self) {
        self.dirty.clear();
        self.dirty.extend(&self.boundaries);
    }
    
    fn cache_stats(&self) -> CacheStats {
        CacheStats {
            total_boundaries: self.boundaries.len(),
            dirty_boundaries: self.dirty.len(),
        }
    }
}
```

### GPUCaching

```rust
#[derive(Debug)]
pub struct GPUCaching {
    texture_cache: HashMap<RenderId, wgpu::Texture>,
    dirty: HashSet<RenderId>,
}

impl CachingStrategy for GPUCaching {
    fn should_cache(&self, id: RenderId) -> bool {
        self.texture_cache.contains_key(&id)
    }
    
    fn mark_repaint_boundary(&mut self, id: RenderId) {
        // Create GPU texture for caching
        if !self.texture_cache.contains_key(&id) {
            let texture = create_cache_texture();
            self.texture_cache.insert(id, texture);
        }
    }
    
    fn invalidate(&mut self, id: RenderId) {
        self.dirty.insert(id);
    }
    
    fn invalidate_all(&mut self) {
        self.dirty.extend(self.texture_cache.keys());
    }
    
    fn cache_stats(&self) -> CacheStats {
        CacheStats {
            total_boundaries: self.texture_cache.len(),
            dirty_boundaries: self.dirty.len(),
        }
    }
}
```

## Paint Context

### PaintContext Structure

```rust
pub struct RenderContext<'ctx, Paint: PaintCapability, A: Arity, P: ParentData> {
    phase_data: PhaseData::Paint {
        painting_context: &'ctx mut PaintingContext,
    },
    children: ChildrenAccess<'ctx, A, P, PaintPhase>,
}

impl<'ctx, Paint: PaintCapability, A: Arity, P: ParentData> 
    PaintPhaseContext<Paint> for RenderContext<'ctx, Paint, A, P>
{
    fn canvas(&mut self) -> &mut Paint::Canvas {
        // Returns canvas from PaintingContext
    }
    
    fn layering(&mut self) -> &mut Paint::Layering {
        // Returns layering manager
    }
    
    fn effects(&mut self) -> &mut Paint::Effects {
        // Returns effects manager
    }
    
    fn caching(&mut self) -> &mut Paint::Caching {
        // Returns caching manager
    }
}
```

### PaintingContext

```rust
pub struct PaintingContext {
    /// Canvas backend
    canvas: Box<dyn CanvasApi>,
    
    /// Current transform
    transform: Matrix4,
    
    /// Clip stack
    clips: Vec<Rect>,
    
    /// Estimated bounds being painted
    estimated_bounds: Rect,
}

impl PaintingContext {
    /// Pushes offset transform
    pub fn push_offset(&mut self, offset: Offset) {
        self.canvas.save();
        self.canvas.translate(offset.dx, offset.dy);
    }
    
    /// Pops transform
    pub fn pop(&mut self) {
        self.canvas.restore();
    }
    
    /// Pushes clip
    pub fn push_clip(&mut self, clip: Rect) {
        self.canvas.save();
        self.canvas.clip_rect(clip);
        self.clips.push(clip);
    }
    
    /// Pops clip
    pub fn pop_clip(&mut self) {
        self.canvas.restore();
        self.clips.pop();
    }
}
```

## Paint Examples

### Example 1: Simple Box

```rust
pub struct RenderColoredBox {
    color: Color,
    size: Size,
}

impl RenderBoxImpl for RenderColoredBox {
    type Arity = Optional;
    
    fn paint(&self, mut ctx: PaintContext<'_, Optional>) {
        // Draw rectangle
        let canvas = ctx.canvas();
        let paint = Paint::fill(self.color);
        canvas.draw_rect(self.size.as_rect(), &paint);
        
        // Paint child if any
        if let Some(child) = ctx.children().get() {
            child.paint(&mut ctx.painting_context);
        }
    }
}
```

### Example 2: Box with Transform

```rust
pub struct RenderTransform {
    transform: Matrix4,
    size: Size,
}

impl RenderBoxImpl for RenderTransform {
    type Arity = Single;
    
    fn paint(&self, mut ctx: PaintContext<'_, Single>) {
        let painting_ctx = ctx.painting_context();
        
        // Push transform
        painting_ctx.canvas.save();
        painting_ctx.canvas.transform(&self.transform);
        
        // Paint child
        let child = ctx.children().get();
        child.paint(painting_ctx);
        
        // Pop transform
        painting_ctx.canvas.restore();
    }
}
```

### Example 3: Opacity Layer

```rust
pub struct RenderOpacity {
    opacity: f32,
    size: Size,
}

impl RenderBoxImpl for RenderOpacity {
    type Arity = Single;
    
    fn paint(&self, mut ctx: PaintContext<'_, Single>) {
        if self.opacity >= 1.0 {
            // Fully opaque - just paint child
            let child = ctx.children().get();
            child.paint(&mut ctx.painting_context);
        } else if self.opacity > 0.0 {
            // Use layering + effects
            let layering = ctx.layering();
            let effects = ctx.effects();
            
            // Push opacity layer
            layering.push_opacity(self.opacity);
            effects.apply_opacity(self.opacity);
            
            // Paint child
            let child = ctx.children().get();
            child.paint(&mut ctx.painting_context);
            
            // Pop layer
            layering.pop_layer();
        }
        // else: invisible, don't paint
    }
}
```

### Example 4: Custom Shader

```rust
pub struct RenderShader {
    shader: Shader,
    size: Size,
}

impl RenderBoxImpl for RenderShader {
    type Arity = Single;
    
    fn paint(&self, mut ctx: PaintContext<'_, Single>) {
        let effects = ctx.effects();
        
        if effects.supports_shaders() {
            // Apply custom shader
            effects.apply_shader(&self.shader);
            
            // Paint child with shader
            let child = ctx.children().get();
            child.paint(&mut ctx.painting_context);
        } else {
            // Fallback: paint without shader
            let child = ctx.children().get();
            child.paint(&mut ctx.painting_context);
        }
    }
}
```

### Example 5: Repaint Boundary

```rust
pub struct RenderRepaintBoundary {
    is_repaint_boundary: bool,
    size: Size,
}

impl RenderBoxImpl for RenderRepaintBoundary {
    type Arity = Single;
    
    fn paint(&self, mut ctx: PaintContext<'_, Single>) {
        if self.is_repaint_boundary {
            let caching = ctx.caching();
            let render_id = self.id();
            
            // Mark as repaint boundary
            caching.mark_repaint_boundary(render_id);
            
            if caching.should_cache(render_id) {
                // Use cached layer if available
                // Otherwise paint to new layer
            }
        }
        
        // Paint child
        let child = ctx.children().get();
        child.paint(&mut ctx.painting_context);
    }
    
    fn is_repaint_boundary(&self) -> bool {
        self.is_repaint_boundary
    }
}
```

## Paint Best Practices

### 1. Always Save/Restore Canvas State

```rust
// ✅ Good
canvas.save();
canvas.translate(dx, dy);
// ... paint operations ...
canvas.restore();

// ❌ Bad - leaks state
canvas.translate(dx, dy);
// ... paint operations ...
// Forgot to restore!
```

### 2. Use Helper Methods

```rust
// ✅ Good
painting_ctx.push_offset(child.offset());
child.paint(painting_ctx);
painting_ctx.pop();

// ❌ Bad - manual state management
painting_ctx.canvas.save();
painting_ctx.canvas.translate(child.offset().dx, child.offset().dy);
child.paint(painting_ctx);
painting_ctx.canvas.restore();
```

### 3. Check Backend Capabilities

```rust
// ✅ Good
if effects.supports_shaders() {
    effects.apply_shader(&shader);
} else {
    // Fallback for non-shader backends
}

// ❌ Bad - assumes all backends support shaders
effects.apply_shader(&shader);  // Might panic!
```

### 4. Optimize with Repaint Boundaries

```rust
// ✅ Good - mark expensive widgets
impl RenderBoxImpl for ExpensiveWidget {
    fn is_repaint_boundary(&self) -> bool {
        true  // Cache this widget
    }
}

// ❌ Bad - no boundaries, always repaint everything
```

### 5. Paint in Correct Order

```rust
// ✅ Good - background, content, foreground
self.paint_background(canvas);
child.paint(painting_ctx);
self.paint_foreground(canvas);

// ❌ Bad - foreground behind content
self.paint_foreground(canvas);
child.paint(painting_ctx);
self.paint_background(canvas);
```

## Summary

**Paint is the ONLY capability that should be decomposed.**

| Component | Why Decomposed | Examples |
|-----------|----------------|----------|
| **Canvas** | Backend truly varies | Skia, wgpu, Canvas2D, WebGL |
| **Layering** | Strategy truly varies | Immediate, retained, composited |
| **Effects** | Capability truly varies | CPU filters, GPU shaders |
| **Caching** | Strategy truly varies | Repaint boundaries, GPU cache |

**Key Principle**: Decompose when backends/strategies **genuinely differ architecturally**, not just in implementation details.

Layout and HitTest don't need decomposition because:
- Layout: Constraints/geometry types vary (protocol level), but sizing/positioning logic is widget-specific
- HitTest: Position/result types vary (protocol level), but testing logic is widget-specific
- Paint: Canvas/layering/effects/caching ALL vary at architectural level

**Paint is special.** 🎨
