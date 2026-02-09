# Paint System Architecture

## Overview

The paint system in FLUI is decomposed into four orthogonal components, allowing maximum flexibility and backend independence. Each component can be swapped independently without affecting others.

## Component Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│                      PaintCapability                            │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────┐│
│  │   Canvas    │  │  Layering   │  │   Effects   │  │ Caching ││
│  │   (WHAT)    │  │   (HOW)     │  │  (ENHANCE)  │  │(OPTIMIZE││
│  ├─────────────┤  ├─────────────┤  ├─────────────┤  ├─────────┤│
│  │ draw_rect   │  │ push_layer  │  │ apply_blur  │  │ should_ ││
│  │ draw_path   │  │ pop_layer   │  │ apply_opacity│ │  cache  ││
│  │ draw_text   │  │ push_clip   │  │ apply_shader│  │invalidate│
│  │ draw_image  │  │ push_transform│ │            │  │         ││
│  └─────────────┘  └─────────────┘  └─────────────┘  └─────────┘│
│        │                │                │               │      │
│        ▼                ▼                ▼               ▼      │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────┐│
│  │SkiaCanvas   │  │SimpleLayering│ │StandardEffects│RepaintBoundaries│
│  │WgpuCanvas   │  │CompositedL. │  │ShaderEffects│  │GPUCaching││
│  │Canvas2DApi  │  │             │  │             │  │NoCaching ││
│  └─────────────┘  └─────────────┘  └─────────────┘  └─────────┘│
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

## Components

### 1. CanvasApi - Drawing Backend

The Canvas component abstracts the drawing backend. Different implementations target different platforms or rendering strategies.

```rust
pub trait CanvasApi: Send + Sync + Debug {
    // Basic drawing
    fn draw_rect(&mut self, rect: Rect, paint: &Paint);
    fn draw_rrect(&mut self, rrect: RRect, paint: &Paint);
    fn draw_circle(&mut self, center: Offset, radius: f32, paint: &Paint);
    fn draw_path(&mut self, path: &Path, paint: &Paint);
    fn draw_text(&mut self, text: &str, offset: Offset, paint: &Paint);
    fn draw_image(&mut self, image: &Image, offset: Offset, paint: &Paint);
    
    // State management
    fn save(&mut self) -> SaveLayerHandle;
    fn restore(&mut self);
    fn translate(&mut self, dx: f32, dy: f32);
    fn scale(&mut self, sx: f32, sy: f32);
    fn rotate(&mut self, radians: f32);
    fn transform(&mut self, matrix: &Matrix4);
    
    // Clipping
    fn clip_rect(&mut self, rect: Rect);
    fn clip_rrect(&mut self, rrect: RRect);
    fn clip_path(&mut self, path: &Path);
    
    // Backend info
    fn backend_type(&self) -> CanvasBackend;
    fn flush(&mut self);
}
```

#### Implementations

| Implementation | Platform | GPU | Use Case |
|----------------|----------|-----|----------|
| `SkiaCanvas` | Desktop/Mobile | CPU/GPU | Default, full-featured |
| `WgpuCanvas` | Cross-platform | GPU | High performance |
| `Canvas2DApi` | Web | CPU | Browser compatibility |
| `WebGLCanvas` | Web | GPU | Web performance |

### 2. LayeringStrategy - Paint Organization

Controls how paint operations are organized into layers for compositing.

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
```

#### Implementations

| Implementation | Description | Performance | Use Case |
|----------------|-------------|-------------|----------|
| `SimpleLayering` | No actual layers, uses canvas state | Fast | Simple UIs |
| `CompositedLayering` | Separate GPU textures per layer | Slower setup, fast composite | Complex UIs with animations |

### 3. EffectsApi - Visual Effects

Handles visual effects like blur, opacity, color filters, and custom shaders.

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

#### Implementations

| Implementation | Shaders | Filters | Use Case |
|----------------|---------|---------|----------|
| `StandardEffects` | No | Basic | Most applications |
| `ShaderEffects` | Yes | Full | Games, creative apps |
| `NoEffects` | No | No | Minimal rendering |

### 4. CachingStrategy - Repaint Optimization

Controls how render objects are cached to minimize repainting.

```rust
pub trait CachingStrategy: Send + Sync + Debug {
    fn should_cache(&self, id: RenderId) -> bool;
    fn mark_repaint_boundary(&mut self, id: RenderId);
    fn invalidate(&mut self, id: RenderId);
    fn invalidate_all(&mut self);
    fn cache_stats(&self) -> CacheStats;
}
```

#### Implementations

| Implementation | Strategy | Memory | Use Case |
|----------------|----------|--------|----------|
| `RepaintBoundaries` | Cache at boundaries | Medium | Standard apps |
| `GPUCaching` | Cache as GPU textures | High | Complex, static content |
| `NoCaching` | Always repaint | Low | Simple, dynamic content |

## Pre-configured Paint Capabilities

### StandardPaint
Default configuration for most applications.

```rust
pub struct StandardPaint;

impl PaintCapability for StandardPaint {
    type Canvas = SkiaCanvas;
    type Layering = SimpleLayering;
    type Effects = StandardEffects;
    type Caching = RepaintBoundaries;
}
```

### GPUPaint
High-performance GPU-accelerated rendering.

```rust
pub struct GPUPaint;

impl PaintCapability for GPUPaint {
    type Canvas = WgpuCanvas;
    type Layering = CompositedLayering;
    type Effects = ShaderEffects;
    type Caching = GPUCaching;
}
```

### WebPaint
Optimized for web browsers.

```rust
pub struct WebPaint;

impl PaintCapability for WebPaint {
    type Canvas = Canvas2DApi;
    type Layering = SimpleLayering;
    type Effects = StandardEffects;
    type Caching = NoCaching;  // Browser handles caching
}
```

## Custom Paint Capability

Create custom paint configurations by mixing components:

```rust
/// WebGL with simple layering and standard effects
pub struct WebGLPaint;

impl PaintCapability for WebGLPaint {
    type Canvas = WebGLCanvas;          // WebGL backend
    type Layering = SimpleLayering;     // No GPU layers
    type Effects = StandardEffects;     // Basic effects
    type Caching = RepaintBoundaries;   // Standard caching
    
    type Context<'ctx, A: Arity, P: ParentData> = 
        PaintCtx<'ctx, Self, A, P>
    where Self: 'ctx;
}
```

## PaintContext API

The paint context provides unified access to all paint components:

```rust
pub struct PaintCtx<'a, Paint: PaintCapability, A: Arity, P: ParentData> {
    canvas: &'a mut Paint::Canvas,
    layering: &'a mut Paint::Layering,
    effects: &'a mut Paint::Effects,
    caching: &'a mut Paint::Caching,
    children: ChildrenAccess<'a, A, P, PaintPhase>,
}

impl<'a, Paint: PaintCapability, A: Arity, P: ParentData> PaintCtx<'a, Paint, A, P> {
    /// Access the canvas for drawing
    pub fn canvas(&mut self) -> &mut Paint::Canvas {
        self.canvas
    }
    
    /// Push an opacity layer
    pub fn push_opacity(&mut self, opacity: f32) -> LayerId {
        self.effects.apply_opacity(opacity);
        self.layering.push_opacity(opacity)
    }
    
    /// Push a clip layer
    pub fn push_clip(&mut self, clip: Rect) -> LayerId {
        self.canvas.clip_rect(clip);
        self.layering.push_clip(clip)
    }
    
    /// Paint all children
    pub fn paint_children(&mut self) {
        self.children.for_each(|child| {
            child.paint();
        });
    }
}
```

## Usage Example

```rust
impl RenderBox for RenderDecoratedBox {
    type Arity = Single;
    type ParentData = BoxParentData;
    
    fn paint(&self, mut ctx: PaintCtx<StandardPaint, Single>) {
        // Draw background
        ctx.canvas().draw_rrect(
            RRect::new(self.bounds(), self.border_radius),
            &self.background_paint,
        );
        
        // Apply shadow effect if needed
        if let Some(shadow) = &self.shadow {
            ctx.effects().apply_blur(shadow.blur_radius);
        }
        
        // Paint child with opacity
        if self.opacity < 1.0 {
            let layer = ctx.push_opacity(self.opacity);
            ctx.paint_children();
            ctx.layering().pop_layer();
        } else {
            ctx.paint_children();
        }
        
        // Draw border on top
        if self.border_width > 0.0 {
            ctx.canvas().draw_rrect(
                RRect::new(self.bounds(), self.border_radius),
                &self.border_paint,
            );
        }
    }
}
```

## Performance Considerations

### Canvas Selection
- **SkiaCanvas**: Best all-around choice, hardware acceleration on most platforms
- **WgpuCanvas**: Best for custom shaders and compute workloads
- **Canvas2DApi**: Fallback for browsers without WebGL

### Layering Strategy
- Use `SimpleLayering` for static content or simple animations
- Use `CompositedLayering` when:
  - Many opacity layers
  - Frequent animations that don't change content
  - Complex clip paths

### Caching Strategy
- `RepaintBoundaries`: Mark natural boundaries (list items, cards)
- `GPUCaching`: Best for complex, mostly-static content
- `NoCaching`: Use for highly dynamic content (games, visualizations)

## Future Improvements

1. **Retained Mode Rendering**: Display list recording for complex scenes
2. **Async Paint**: Paint on background thread with texture upload
3. **Vulkan Backend**: Direct Vulkan canvas for maximum performance
4. **Metal Backend**: Native Metal canvas for Apple platforms
5. **Scene Graph**: Higher-level abstraction for complex compositions
