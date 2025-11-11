# FLUI Engine Architecture

**Last Updated**: 2025-01-10
**Status**: ✅ Clean Architecture Enforced

## Dependency Architecture

### Strict Layering Rule

```
❌ FORBIDDEN: flui_app → wgpu (violates abstraction)
✅ CORRECT:   flui_app → flui_engine → wgpu (proper layering)
```

### Visual Architecture

```
┌─────────────────────────────────────────────────────────┐
│                      flui_app                           │
│  ┌───────────────────────────────────────────────────┐  │
│  │ Application Framework                             │  │
│  │  - Window management (winit)                      │  │
│  │  - Event loop & lifecycle                         │  │
│  │  - UI pipeline coordination                       │  │
│  │  - Event callbacks                                │  │
│  └───────────────────────────────────────────────────┘  │
│                                                          │
│  Dependencies:                                           │
│    ✅ flui_core, flui_types, flui_rendering             │
│    ✅ flui_engine (abstraction only!)                   │
│    ✅ winit (window management)                         │
│    ❌ NO wgpu, glyphon, bytemuck                        │
└──────────────────────┬───────────────────────────────────┘
                       │ depends on
                       ↓
┌─────────────────────────────────────────────────────────┐
│                   flui_engine                           │
│  ┌───────────────────────────────────────────────────┐  │
│  │ Rendering Engine                                  │  │
│  │  - GpuRenderer (wgpu wrapper)                     │  │
│  │  - WgpuPainter (GPU acceleration)                 │  │
│  │  - CanvasLayer (render tree)                      │  │
│  │  - CommandRenderer (clean architecture)           │  │
│  └───────────────────────────────────────────────────┘  │
│                                                          │
│  Dependencies:                                           │
│    ✅ flui_types, flui_painting                         │
│    ✅ wgpu, glyphon, bytemuck (GPU impl)                │
│    ✅ lyon (tessellation)                               │
│    ✅ cosmic-text (text rendering)                      │
└──────────────────────┬───────────────────────────────────┘
                       │ depends on
                       ↓
┌─────────────────────────────────────────────────────────┐
│                      wgpu                               │
│  Cross-platform GPU API (Vulkan/Metal/DX12/WebGPU)     │
└─────────────────────────────────────────────────────────┘
```

## Component Breakdown

### flui_app Components

**Purpose**: Application lifecycle and platform integration

```rust
pub struct FluiApp {
    // UI Pipeline
    pipeline: PipelineOwner,
    root_view: Box<dyn AnyView>,

    // GPU Rendering (abstracted!)
    renderer: GpuRenderer,  // ← ONLY GPU reference

    // Platform integration
    window_state: WindowStateTracker,
    event_callbacks: WindowEventCallbacks,
}
```

**Key Methods**:
- `new()` / `new_async()` - Create app (delegates to GpuRenderer)
- `update()` - Frame update (build → layout → paint)
- `resize()` - Window resize (delegates to GpuRenderer)
- `handle_window_event()` - Event dispatching

**NO GPU DETAILS!** All wgpu interaction through `GpuRenderer` abstraction.

### flui_engine Components

**Purpose**: GPU rendering implementation

```rust
pub struct GpuRenderer {
    // wgpu resources (encapsulated)
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    painter: Option<WgpuPainter>,
}
```

**Key Methods**:
- `new()` / `new_async()` - Initialize ALL wgpu resources
- `resize()` - Update surface & painter viewport
- `render()` - Execute rendering pipeline
- `size()` / `format()` - Query GPU state

**Encapsulated Details**:
- wgpu instance & adapter selection
- Surface configuration
- Device & queue management
- Painter lifecycle
- Error recovery (surface lost/outdated)

## Dependency Verification

### Check flui_app (should be clean)

```bash
cargo tree -p flui_app --depth 1 | grep -E "(wgpu|glyphon|bytemuck)"
# Expected: NO MATCHES
```

### Check flui_engine (should have GPU deps)

```bash
cargo tree -p flui_engine --depth 1 | grep -E "(wgpu|glyphon|bytemuck)"
# Expected output:
# ├── bytemuck v1.24.0
# ├── glyphon v0.9.0
# ├── wgpu v25.0.2
```

## Benefits of This Architecture

### 1. Backend Flexibility

Can replace wgpu with alternative backends:

```rust
// flui_engine/src/lib.rs
#[cfg(feature = "wgpu")]
pub use gpu_renderer_wgpu::GpuRenderer;

#[cfg(feature = "vulkan")]
pub use gpu_renderer_vulkan::GpuRenderer;

#[cfg(feature = "opengl")]
pub use gpu_renderer_opengl::GpuRenderer;
```

**flui_app doesn't change** - it only depends on the `GpuRenderer` trait!

### 2. Testability

Mock renderer for unit tests:

```rust
#[cfg(test)]
pub struct MockGpuRenderer {
    frames_rendered: usize,
}

impl MockGpuRenderer {
    pub fn new() -> Self { /* ... */ }

    pub fn render(&mut self, layer: &CanvasLayer) -> Result<()> {
        self.frames_rendered += 1;
        // Validate layer structure, no GPU needed
        Ok(())
    }
}
```

### 3. Platform Portability

Easy to port to platforms without wgpu:

```rust
// Platform-specific GpuRenderer implementations
#[cfg(target_os = "ios")]
pub use gpu_renderer_metal::GpuRenderer;

#[cfg(target_arch = "wasm32")]
pub use gpu_renderer_webgpu::GpuRenderer;

#[cfg(not(any(target_os = "ios", target_arch = "wasm32")))]
pub use gpu_renderer_wgpu::GpuRenderer;
```

### 4. Clear Responsibilities

| Crate | Responsibility | GPU Knowledge |
|-------|----------------|---------------|
| `flui_app` | Application framework, event loop, lifecycle | ❌ None |
| `flui_engine` | Rendering implementation, GPU management | ✅ Full |
| `flui_core` | UI pipeline, element tree, build/layout/paint | ❌ None |
| `flui_rendering` | RenderObjects (layout algorithms) | ❌ None |
| `flui_painting` | Canvas API, display lists | ❌ None |

**Only flui_engine knows about GPU!**

## Migration Impact

### Before Refactoring

```toml
# flui_app/Cargo.toml (WRONG!)
[dependencies]
flui_engine = { path = "../flui_engine" }
wgpu.workspace = true         # ❌ Leaking implementation!
glyphon.workspace = true      # ❌ Leaking implementation!
bytemuck = { workspace = true } # ❌ Leaking implementation!
```

```rust
// flui_app/src/app.rs (WRONG!)
pub struct FluiApp {
    surface: wgpu::Surface,      // ❌ GPU details leaked!
    device: wgpu::Device,        // ❌ GPU details leaked!
    queue: wgpu::Queue,          // ❌ GPU details leaked!
    config: wgpu::SurfaceConfiguration,  // ❌ GPU details leaked!
    painter: Option<WgpuPainter>, // ❌ GPU details leaked!
}
```

### After Refactoring

```toml
# flui_app/Cargo.toml (CORRECT!)
[dependencies]
flui_engine = { path = "../flui_engine" }
winit.workspace = true        # ✅ Window management only
# NO wgpu dependencies!
```

```rust
// flui_app/src/app.rs (CORRECT!)
pub struct FluiApp {
    renderer: GpuRenderer,  // ✅ Clean abstraction!
    // No GPU details!
}

impl FluiApp {
    pub fn new(root_view: Box<dyn AnyView>, window: Arc<Window>) -> Self {
        let renderer = GpuRenderer::new(window);  // ✅ Delegates to engine
        Self { /* ... */ renderer, /* ... */ }
    }
}
```

## Enforcement Strategy

### 1. Code Review Checklist

- [ ] No `use wgpu::*` in flui_app
- [ ] No `use glyphon::*` in flui_app
- [ ] No `use bytemuck::*` in flui_app
- [ ] GpuRenderer is only GPU reference in FluiApp
- [ ] All GPU operations delegated to GpuRenderer

### 2. Automated Checks

```bash
# Check for wgpu imports in flui_app
grep -r "use wgpu" crates/flui_app/src/
# Expected: No matches

# Check for GPU dependencies in Cargo.toml
grep -E "(wgpu|glyphon|bytemuck)" crates/flui_app/Cargo.toml
# Expected: No matches
```

### 3. CI/CD Integration

```yaml
# .github/workflows/architecture-check.yml
- name: Verify architecture boundaries
  run: |
    # Ensure flui_app has no GPU dependencies
    ! grep -E "(wgpu|glyphon|bytemuck)" crates/flui_app/Cargo.toml
    ! grep -r "use wgpu" crates/flui_app/src/
```

## Future Enhancements

### Multiple Backend Support

```rust
// flui_engine/src/lib.rs
pub trait GpuBackend {
    fn new(window: Arc<Window>) -> Self;
    fn resize(&mut self, width: u32, height: u32);
    fn render(&mut self, layer: &CanvasLayer) -> Result<()>;
}

// Implementations
pub struct WgpuBackend { /* wgpu resources */ }
pub struct VulkanBackend { /* vulkan resources */ }
pub struct MetalBackend { /* metal resources */ }

// Runtime selection
pub enum GpuRenderer {
    Wgpu(WgpuBackend),
    Vulkan(VulkanBackend),
    Metal(MetalBackend),
}
```

### Backend Auto-Detection

```rust
impl GpuRenderer {
    pub fn auto_detect(window: Arc<Window>) -> Self {
        #[cfg(target_os = "macos")]
        return Self::Metal(MetalBackend::new(window));

        #[cfg(target_os = "linux")]
        return Self::Vulkan(VulkanBackend::new(window));

        #[cfg(target_arch = "wasm32")]
        return Self::Wgpu(WgpuBackend::new(window));

        Self::Wgpu(WgpuBackend::new(window))
    }
}
```

## Conclusion

The refactored architecture achieves **perfect separation of concerns**:

✅ **Application layer (flui_app)** has NO knowledge of GPU implementation
✅ **Rendering engine (flui_engine)** encapsulates ALL GPU details
✅ **Clean abstraction boundary** via GpuRenderer interface
✅ **Future-proof** for multiple backends and platform-specific optimizations
✅ **Testable** via mock renderers
✅ **Maintainable** with clear responsibilities

This is **production-ready** architecture following industry best practices!
