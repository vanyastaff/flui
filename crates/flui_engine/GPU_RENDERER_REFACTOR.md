# GPU Renderer Encapsulation - Architecture Proposal

**Date**: 2025-01-10
**Status**: 📋 PROPOSED
**Priority**: HIGH (Architecture improvement)

---

## 🎯 Problem

Currently `FluiApp` directly manages low-level GPU resources:

```rust
pub struct FluiApp {
    // High-level (correct)
    pipeline: PipelineOwner,
    root_view: Box<dyn AnyView>,

    // Low-level GPU details (WRONG LAYER!)
    surface: wgpu::Surface,        // ❌ Should be in flui_engine
    device: wgpu::Device,          // ❌ Should be in flui_engine
    queue: wgpu::Queue,            // ❌ Should be in flui_engine
    config: SurfaceConfiguration,  // ❌ Should be in flui_engine
    painter: Option<WgpuPainter>,  // ✅ OK but could be better
}
```

**Issues**:
1. **Separation of Concerns**: App layer knows GPU implementation details
2. **Tight Coupling**: Changes to GPU backend require changes in flui_app
3. **Testability**: Hard to mock GPU layer for testing
4. **Complexity**: FluiApp has too many responsibilities

---

## 🏗️ Proposed Solution

Create `GpuRenderer` in `flui_engine` to encapsulate ALL GPU concerns:

### New Architecture

```
flui_app::FluiApp
├─ pipeline: PipelineOwner (UI tree management)
├─ root_view: Box<dyn AnyView> (UI definition)
└─ renderer: GpuRenderer (GPU abstraction) ✨ NEW

flui_engine::GpuRenderer
├─ surface: wgpu::Surface
├─ device: wgpu::Device
├─ queue: wgpu::Queue
├─ config: SurfaceConfiguration
└─ painter: WgpuPainter
```

### Implementation Plan

#### 1. Create `GpuRenderer` in `flui_engine`

```rust
// flui_engine/src/gpu_renderer.rs

/// High-level GPU rendering abstraction
///
/// Encapsulates all wgpu resources and rendering logic.
/// Provides clean interface for flui_app without exposing GPU details.
pub struct GpuRenderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    painter: WgpuPainter,
}

impl GpuRenderer {
    /// Create new GPU renderer for a window
    pub fn new(window: Arc<Window>) -> Self {
        // All wgpu initialization happens here
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = instance
            .create_surface(Arc::clone(&window))
            .expect("Failed to create surface");

        let adapter = pollster::block_on(instance.request_adapter(
            &wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            }
        )).expect("Failed to find adapter");

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                label: None,
                memory_hints: wgpu::MemoryHints::default(),
                trace: Default::default(),
            }
        )).expect("Failed to create device");

        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface.get_capabilities(&adapter).formats[0],
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let painter = WgpuPainter::new(
            device.clone(),
            queue.clone(),
            config.format,
            (config.width, config.height),
        );

        Self {
            surface,
            device,
            queue,
            config,
            painter,
        }
    }

    /// Resize the rendering surface
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
            self.painter.resize(width, height);
        }
    }

    /// Render a layer to the surface
    pub fn render(&mut self, layer: &CanvasLayer) -> Result<(), RenderError> {
        // Get current frame
        let frame = self.surface.get_current_texture()?;
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Create command encoder
        let mut encoder = self.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            }
        );

        // Clear screen
        {
            let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Clear Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1, g: 0.1, b: 0.1, a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }

        // Render layer using command dispatcher
        let mut renderer = WgpuRenderer::new(self.painter.clone());
        layer.render(&mut renderer);
        self.painter = renderer.into_painter();

        // Render to GPU
        self.painter.render(&view, &mut encoder)?;

        // Submit and present
        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();

        Ok(())
    }

    /// Get current viewport size
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }
}

#[derive(Debug)]
pub enum RenderError {
    SurfaceLost,
    SurfaceOutdated,
    OutOfMemory,
    Timeout,
    PainterError(String),
}

impl From<wgpu::SurfaceError> for RenderError {
    fn from(err: wgpu::SurfaceError) -> Self {
        match err {
            wgpu::SurfaceError::Lost => RenderError::SurfaceLost,
            wgpu::SurfaceError::Outdated => RenderError::SurfaceOutdated,
            wgpu::SurfaceError::OutOfMemory => RenderError::OutOfMemory,
            wgpu::SurfaceError::Timeout => RenderError::Timeout,
        }
    }
}
```

#### 2. Simplify `FluiApp`

```rust
// flui_app/src/app.rs

pub struct FluiApp {
    // High-level concerns only
    pipeline: PipelineOwner,
    root_view: Box<dyn AnyView>,
    root_id: Option<ElementId>,
    stats: FrameStats,
    last_size: Option<Size>,
    root_built: bool,
    window_state: WindowStateTracker,

    // Single GPU abstraction (no low-level details!)
    renderer: GpuRenderer,  // ✨ Clean interface!

    on_cleanup: Option<Box<dyn FnOnce() + Send>>,
    event_callbacks: WindowEventCallbacks,
}

impl FluiApp {
    pub fn new(root_view: Box<dyn AnyView>, window: Arc<Window>) -> Self {
        Self {
            pipeline: PipelineOwner::new(),
            root_view,
            root_id: None,
            stats: FrameStats::default(),
            last_size: None,
            root_built: false,
            window_state: WindowStateTracker::new(),
            renderer: GpuRenderer::new(window),  // ✨ One line!
            on_cleanup: None,
            event_callbacks: WindowEventCallbacks::new(),
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.renderer.resize(width, height);  // ✨ Delegate to renderer

        if let Some(root_id) = self.root_id {
            self.pipeline.request_layout(root_id);
        }
    }

    fn render(&mut self, layer: Box<CanvasLayer>) {
        // ✨ Clean delegation - no GPU details!
        match self.renderer.render(&layer) {
            Ok(()) => {},
            Err(RenderError::SurfaceLost | RenderError::SurfaceOutdated) => {
                tracing::warn!("Surface lost/outdated, will retry next frame");
            }
            Err(e) => {
                tracing::error!("Render error: {:?}", e);
            }
        }
    }
}
```

---

## ✅ Benefits

### 1. **Separation of Concerns**
- `FluiApp` focuses on application lifecycle
- `GpuRenderer` focuses on GPU rendering
- Clear boundaries between layers

### 2. **Encapsulation**
- GPU implementation details hidden from app layer
- Easy to swap rendering backends in future
- Clean API surface

### 3. **Testability**
```rust
// Easy to mock for testing
trait Renderer {
    fn render(&mut self, layer: &CanvasLayer) -> Result<(), RenderError>;
    fn resize(&mut self, width: u32, height: u32);
}

impl Renderer for GpuRenderer { /* ... */ }
impl Renderer for MockRenderer { /* ... */ }  // For tests!
```

### 4. **Reduced Complexity**
- `FluiApp` goes from 15+ fields to ~8 fields
- Clearer responsibilities
- Easier to understand and maintain

### 5. **Future-Proof**
- Easy to add Metal/Vulkan/DX12 specific optimizations
- Can add multiple rendering backends (2D canvas, software rasterizer)
- Simpler to add features like headless rendering

---

## 📋 Migration Checklist

- [ ] Create `flui_engine/src/gpu_renderer.rs`
- [ ] Implement `GpuRenderer::new()`
- [ ] Implement `GpuRenderer::resize()`
- [ ] Implement `GpuRenderer::render()`
- [ ] Add `RenderError` enum
- [ ] Update `flui_engine/src/lib.rs` exports
- [ ] Refactor `FluiApp` to use `GpuRenderer`
- [ ] Update WASM platform code (`flui_app/src/wasm.rs`)
- [ ] Test on all platforms (Windows, macOS, Linux, WASM)
- [ ] Update documentation
- [ ] Create migration guide for breaking changes

---

## 🚧 Breaking Changes

- `FluiApp::new()` signature unchanged (no breaking change!)
- `FluiApp::from_components()` removed (internal WASM API)
- Direct access to `device`/`queue`/`surface` removed
- New public API: `FluiApp::renderer()` or `FluiApp::renderer_mut()` if needed

**Migration**: Most users won't be affected - only those using internal APIs.

---

## 🎯 Priority

**HIGH** - This is a fundamental architecture improvement that:
1. Makes codebase cleaner and more maintainable
2. Prepares for future renderer backends
3. Improves testability significantly
4. Reduces coupling between layers

**Recommendation**: Implement in next sprint after current refactoring is complete.

---

## 📊 Estimated Effort

- **Implementation**: 2-4 hours
- **Testing**: 1-2 hours
- **Documentation**: 1 hour
- **Total**: 4-7 hours

**Risk**: LOW - Changes are well-scoped and backward compatible for most users.

---

**Generated**: 2025-01-10
**Author**: Architecture Review
**Status**: Ready for implementation
