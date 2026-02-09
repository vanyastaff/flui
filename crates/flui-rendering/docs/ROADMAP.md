# Protocol System Roadmap

## Current Status

The protocol system is in the design phase. Core architecture has been defined with composition-based capabilities.

## Phase 1: Foundation (Current)

### 1.1 Core Traits
- [ ] `LayoutCapability` trait with GAT Context
- [ ] `HitTestCapability` trait with GAT Context
- [ ] `PaintCapability` trait with decomposed components
- [ ] `Protocol` trait composing capabilities

### 1.2 Paint Components
- [ ] `CanvasApi` trait
- [ ] `LayeringStrategy` trait
- [ ] `EffectsApi` trait
- [ ] `CachingStrategy` trait

### 1.3 Box Protocol
- [ ] `BoxLayout` capability (BoxConstraints -> Size)
- [ ] `BoxHitTest` capability (Offset -> BoxHitTestResult)
- [ ] `BoxProtocol` composition
- [ ] `BoxLayoutCtx` context
- [ ] `BoxHitTestCtx` context

### 1.4 Shared Paint
- [ ] `StandardPaint` configuration
- [ ] `PaintCtx` shared context
- [ ] Basic `SkiaCanvas` implementation (stub)

## Phase 2: Sliver Protocol

### 2.1 Sliver Capabilities
- [ ] `SliverLayout` capability (SliverConstraints -> SliverGeometry)
- [ ] `SliverHitTest` capability (MainAxisPosition -> SliverHitTestResult)
- [ ] `SliverProtocol` composition

### 2.2 Sliver Contexts
- [ ] `SliverLayoutCtx`
- [ ] `SliverHitTestCtx`

## Phase 3: RenderObject Integration

### 3.1 Trait Updates
- [ ] Update `RenderBox` to use `BoxLayoutCtx`
- [ ] Update `RenderSliver` to use `SliverLayoutCtx`
- [ ] Migrate existing render objects

### 3.2 Children Access
- [ ] `ChildrenAccess<A, P, Phase>` with closure API
- [ ] `ChildHandle<P, Phase>` for phase-safe operations
- [ ] Integrate with contexts

## Phase 4: Canvas Backends

### 4.1 Skia Backend
- [ ] Full `SkiaCanvas` implementation
- [ ] Integration with `skia-safe` crate
- [ ] GPU acceleration support

### 4.2 wgpu Backend
- [ ] `WgpuCanvas` implementation
- [ ] Shader compilation pipeline
- [ ] Texture management

### 4.3 Web Backends
- [ ] `Canvas2DApi` for HTML5 Canvas
- [ ] `WebGLCanvas` for WebGL
- [ ] wasm-bindgen integration

## Phase 5: Advanced Features

### 5.1 Composited Layering
- [ ] `CompositedLayering` implementation
- [ ] GPU texture layers
- [ ] Layer compositing pipeline

### 5.2 Shader Effects
- [ ] `ShaderEffects` implementation
- [ ] WGSL shader support
- [ ] Common effects library (blur, shadows, gradients)

### 5.3 GPU Caching
- [ ] `GPUCaching` implementation
- [ ] Texture atlas management
- [ ] Cache invalidation strategies

## Phase 6: Optimization

### 6.1 Performance
- [ ] Benchmark suite
- [ ] Profile-guided optimization
- [ ] SIMD acceleration for geometry calculations

### 6.2 Memory
- [ ] Arena allocation for contexts
- [ ] Object pooling for paint operations
- [ ] Memory profiling tools

## Phase 7: Platform Integration

### 7.1 Desktop
- [ ] Windows integration (win32/WinRT)
- [ ] macOS integration (AppKit/Metal)
- [ ] Linux integration (X11/Wayland)

### 7.2 Mobile
- [ ] Android integration (NDK)
- [ ] iOS integration (UIKit/Metal)

### 7.3 Web
- [ ] wasm32 target optimization
- [ ] Web worker rendering
- [ ] Progressive enhancement

## Future Considerations

### Scene Graph
Higher-level abstraction for complex compositions:
```rust
trait SceneNode {
    fn build_scene(&self, builder: &mut SceneBuilder);
}
```

### Retained Mode
Display list recording for complex, static scenes:
```rust
trait DisplayList {
    fn record(&mut self, commands: impl Iterator<Item = DrawCommand>);
    fn replay(&self, canvas: &mut dyn CanvasApi);
}
```

### Async Painting
Background thread painting with texture upload:
```rust
trait AsyncPaint {
    async fn paint_async(&self) -> PaintResult;
    fn upload_texture(&self, result: PaintResult);
}
```

### Custom Protocols
Framework for defining application-specific protocols:
```rust
// Example: 3D protocol
struct Protocol3D;
impl Protocol for Protocol3D {
    type Layout = Layout3D;      // Constraints3D -> Bounds3D
    type HitTest = RaycastHitTest;  // Ray -> RaycastResult
    type Paint = GPU3DPaint;     // 3D rendering pipeline
}
```

## Timeline Estimates

| Phase | Complexity | Dependencies |
|-------|------------|--------------|
| Phase 1 | Medium | None |
| Phase 2 | Low | Phase 1 |
| Phase 3 | Medium | Phase 1, 2 |
| Phase 4 | High | Phase 1 |
| Phase 5 | High | Phase 4 |
| Phase 6 | Medium | Phase 1-5 |
| Phase 7 | High | Phase 4-6 |

## Success Metrics

1. **Type Safety**: Zero runtime protocol errors
2. **Performance**: < 1ms layout pass for 1000 widgets
3. **Memory**: < 100 bytes overhead per render object
4. **Flexibility**: Support 3+ canvas backends
5. **Ergonomics**: Clean API with minimal boilerplate
