# FLUI App Architecture with Pure wgpu

**Status:** Adapted for pure wgpu backend (no egui)
**Date:** 2025-11-11

---

## Current Reality Check

You have:
- ✅ **flui_core** - Element tree, hooks (use_signal, etc.)
- ✅ **flui_rendering** - RenderObject trait, layout, paint → Layer tree
- ✅ **flui_engine** - EventRouter, Layer compositing
- ✅ **flui_gestures** - TapRecognizer, DragRecognizer, GestureDetector
- ✅ **flui_widgets** - Text, Container, Column, Row, etc.
- ✅ **Pure wgpu** - Your own GPU rendering (no egui!)

---

## Architecture with Pure wgpu

```
User App
   ↓
runApp(root_widget)
   ↓
WidgetsFlutterBinding::ensure_initialized()
   ├─ GestureBinding (EventRouter integration)
   ├─ SchedulerBinding (frame callbacks)
   ├─ RendererBinding (PipelineOwner integration)
   └─ WidgetsBinding (ElementTree management)
   ↓
WgpuEmbedder::new()
   ├─ Create winit window
   ├─ Initialize wgpu (device, queue, surface)
   └─ Setup render pipeline
   ↓
Event Loop (winit)
   ├─ Window events → GestureBinding
   ├─ Frame callback → SchedulerBinding
   ├─ Layout → flui_rendering
   ├─ Paint → Layer tree
   └─ Render layers → wgpu
```

---

## Simplified flui_app Structure

```
crates/flui_app/
├── src/
│   ├── lib.rs                    // runApp(), exports
│   │
│   ├── binding/
│   │   ├── mod.rs
│   │   ├── base.rs              // BindingBase trait
│   │   ├── gesture.rs           // Bridge to EventRouter
│   │   ├── scheduler.rs         // Frame callbacks
│   │   ├── renderer.rs          // Bridge to PipelineOwner
│   │   ├── widgets.rs           // Bridge to ElementTree
│   │   └── widgets_flutter_binding.rs  // Combined
│   │
│   ├── embedder/
│   │   ├── mod.rs               // PlatformEmbedder trait
│   │   └── wgpu.rs              // WgpuEmbedder (winit + wgpu)
│   │
│   ├── app.rs                    // App widget
│   └── window.rs                 // Window config
```

---

## Code Implementation

### 1. Minimal BindingBase

```rust
// crates/flui_app/src/binding/base.rs

/// Base trait for all bindings
pub trait BindingBase: Send + Sync {
    fn init(&mut self);
}
```

### 2. GestureBinding - Bridge to EventRouter

```rust
// crates/flui_app/src/binding/gesture.rs

use flui_engine::EventRouter;
use flui_types::events::{PointerEvent, KeyEvent, Event};
use std::sync::Arc;
use parking_lot::RwLock;

/// Gesture binding - bridges platform events to EventRouter
pub struct GestureBinding {
    event_router: Arc<RwLock<EventRouter>>,
}

impl GestureBinding {
    pub fn new() -> Self {
        Self {
            event_router: Arc::new(RwLock::new(EventRouter::new())),
        }
    }
    
    /// Handle pointer event from platform (winit)
    pub fn handle_pointer_event(&self, event: PointerEvent, root: &mut dyn Layer) {
        let mut router = self.event_router.write();
        router.route_event(root, &Event::Pointer(event));
    }
    
    /// Handle keyboard event from platform
    pub fn handle_key_event(&self, event: KeyEvent, root: &mut dyn Layer) {
        let mut router = self.event_router.write();
        router.route_event(root, &Event::Key(event));
    }
    
    pub fn event_router(&self) -> Arc<RwLock<EventRouter>> {
        self.event_router.clone()
    }
}

impl BindingBase for GestureBinding {
    fn init(&mut self) {
        tracing::debug!("GestureBinding initialized");
    }
}
```

### 3. SchedulerBinding - Frame Callbacks

```rust
// crates/flui_app/src/binding/scheduler.rs

use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;

pub type FrameCallback = Arc<dyn Fn(Duration) + Send + Sync>;

/// Scheduler binding - manages frame callbacks
pub struct SchedulerBinding {
    /// Persistent callbacks (called every frame)
    persistent_callbacks: Arc<Mutex<Vec<FrameCallback>>>,
    
    /// One-time post-frame callbacks
    post_frame_callbacks: Arc<Mutex<Vec<FrameCallback>>>,
}

impl SchedulerBinding {
    pub fn new() -> Self {
        Self {
            persistent_callbacks: Arc::new(Mutex::new(Vec::new())),
            post_frame_callbacks: Arc::new(Mutex::new(Vec::new())),
        }
    }
    
    /// Add persistent frame callback
    pub fn add_persistent_frame_callback(&self, callback: FrameCallback) {
        self.persistent_callbacks.lock().push(callback);
    }
    
    /// Add one-time post-frame callback
    pub fn add_post_frame_callback(&self, callback: FrameCallback) {
        self.post_frame_callbacks.lock().push(callback);
    }
    
    /// Called at start of frame
    pub fn handle_begin_frame(&self, timestamp: Duration) {
        let callbacks = self.persistent_callbacks.lock();
        for callback in callbacks.iter() {
            callback(timestamp);
        }
    }
    
    /// Called at end of frame
    pub fn handle_draw_frame(&self) {
        // Take all post-frame callbacks (consume)
        let callbacks = std::mem::take(&mut *self.post_frame_callbacks.lock());
        for callback in callbacks {
            callback(Duration::ZERO);
        }
    }
}

impl BindingBase for SchedulerBinding {
    fn init(&mut self) {
        tracing::debug!("SchedulerBinding initialized");
    }
}
```

### 4. RendererBinding - Bridge to PipelineOwner

```rust
// crates/flui_app/src/binding/renderer.rs

use flui_rendering::PipelineOwner;
use flui_engine::Scene;
use std::sync::Arc;
use parking_lot::RwLock;

/// Renderer binding - bridges to flui_rendering
pub struct RendererBinding {
    pipeline_owner: Arc<RwLock<PipelineOwner>>,
}

impl RendererBinding {
    pub fn new() -> Self {
        Self {
            pipeline_owner: Arc::new(RwLock::new(PipelineOwner::new())),
        }
    }
    
    /// Draw frame - flush pipeline and composite
    pub fn draw_frame(&self, root_layer: &mut dyn Layer) -> Scene {
        let mut pipeline = self.pipeline_owner.write();
        
        // 1. Flush layout
        pipeline.flush_layout();
        
        // 2. Flush compositing bits
        pipeline.flush_compositing_bits();
        
        // 3. Flush paint
        pipeline.flush_paint();
        
        // 4. Composite scene
        let mut scene = Scene::new();
        root_layer.composite(&mut scene);
        
        scene
    }
    
    pub fn pipeline_owner(&self) -> Arc<RwLock<PipelineOwner>> {
        self.pipeline_owner.clone()
    }
}

impl BindingBase for RendererBinding {
    fn init(&mut self) {
        tracing::debug!("RendererBinding initialized");
    }
}
```

### 5. WidgetsBinding - Bridge to ElementTree

```rust
// crates/flui_app/src/binding/widgets.rs

use flui_core::{ElementTree, ElementId, View, IntoElement};
use parking_lot::RwLock;
use std::sync::Arc;

/// Widgets binding - manages widget tree
pub struct WidgetsBinding {
    element_tree: Arc<RwLock<ElementTree>>,
    root_element: Arc<RwLock<Option<ElementId>>>,
}

impl WidgetsBinding {
    pub fn new() -> Self {
        Self {
            element_tree: Arc::new(RwLock::new(ElementTree::new())),
            root_element: Arc::new(RwLock::new(None)),
        }
    }
    
    /// Attach root widget
    pub fn attach_root_widget(&self, widget: impl View + 'static) {
        let element = widget.into_element();
        let mut tree = self.element_tree.write();
        
        // Inflate element tree
        let root_id = tree.inflate_element(element);
        *self.root_element.write() = Some(root_id);
        
        tracing::info!("Root widget attached: {:?}", root_id);
    }
    
    /// Handle build frame
    pub fn handle_build_frame(&self) {
        let mut tree = self.element_tree.write();
        tree.flush_build();
    }
    
    pub fn element_tree(&self) -> Arc<RwLock<ElementTree>> {
        self.element_tree.clone()
    }
    
    pub fn root_element(&self) -> Option<ElementId> {
        *self.root_element.read()
    }
}

impl BindingBase for WidgetsBinding {
    fn init(&mut self) {
        tracing::debug!("WidgetsBinding initialized");
    }
}
```

### 6. WidgetsFlutterBinding - Combined

```rust
// crates/flui_app/src/binding/widgets_flutter_binding.rs

use super::{BindingBase, GestureBinding, SchedulerBinding, RendererBinding, WidgetsBinding};
use std::sync::{Arc, OnceLock};

/// Combined Flutter-style binding
pub struct WidgetsFlutterBinding {
    pub gesture: GestureBinding,
    pub scheduler: SchedulerBinding,
    pub renderer: RendererBinding,
    pub widgets: WidgetsBinding,
}

impl WidgetsFlutterBinding {
    /// Ensure binding is initialized (idempotent)
    pub fn ensure_initialized() -> Arc<Self> {
        static INSTANCE: OnceLock<Arc<WidgetsFlutterBinding>> = OnceLock::new();
        
        INSTANCE.get_or_init(|| {
            let mut binding = Self {
                gesture: GestureBinding::new(),
                scheduler: SchedulerBinding::new(),
                renderer: RendererBinding::new(),
                widgets: WidgetsBinding::new(),
            };
            
            // Initialize all bindings
            binding.gesture.init();
            binding.scheduler.init();
            binding.renderer.init();
            binding.widgets.init();
            
            // Wire up frame callbacks
            binding.wire_up();
            
            tracing::info!("WidgetsFlutterBinding initialized");
            Arc::new(binding)
        }).clone()
    }
    
    fn wire_up(&self) {
        // Connect scheduler → widgets (build phase)
        self.scheduler.add_persistent_frame_callback(Arc::new({
            let widgets = self.widgets.clone();
            move |_| widgets.handle_build_frame()
        }));
        
        // Renderer draw happens explicitly in embedder
    }
}

impl Clone for WidgetsFlutterBinding {
    fn clone(&self) -> Self {
        // This is a singleton, so "cloning" returns a new reference
        Self::ensure_initialized();
        unreachable!("WidgetsFlutterBinding is a singleton")
    }
}
```

### 7. WgpuEmbedder - Platform Integration

```rust
// crates/flui_app/src/embedder/wgpu.rs

use crate::binding::WidgetsFlutterBinding;
use winit::{
    event::*,
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use std::sync::Arc;
use std::time::Instant;

pub struct WgpuEmbedder {
    binding: Arc<WidgetsFlutterBinding>,
    window: winit::window::Window,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface,
    config: wgpu::SurfaceConfiguration,
}

impl WgpuEmbedder {
    pub async fn new(
        binding: Arc<WidgetsFlutterBinding>,
        event_loop: &EventLoop<()>,
    ) -> Self {
        // 1. Create window
        let window = WindowBuilder::new()
            .with_title("FLUI App")
            .with_inner_size(winit::dpi::PhysicalSize::new(800, 600))
            .build(event_loop)
            .unwrap();
        
        // 2. Initialize wgpu
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        
        let surface = unsafe { instance.create_surface(&window) }.unwrap();
        
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();
        
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    features: wgpu::Features::empty(),
                    limits: wgpu::Limits::default(),
                    label: None,
                },
                None,
            )
            .await
            .unwrap();
        
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface.get_capabilities(&adapter).formats[0],
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        surface.configure(&device, &config);
        
        tracing::info!("wgpu initialized: {:?}", adapter.get_info());
        
        Self {
            binding,
            window,
            device,
            queue,
            surface,
            config,
        }
    }
    
    pub fn run(mut self, event_loop: EventLoop<()>) {
        let start_time = Instant::now();
        
        event_loop.run(move |event, _, control_flow| {
            *control_flow = ControlFlow::Poll;
            
            match event {
                Event::WindowEvent { event, .. } => match event {
                    WindowEvent::CloseRequested => {
                        *control_flow = ControlFlow::Exit;
                    }
                    
                    WindowEvent::Resized(size) => {
                        self.config.width = size.width;
                        self.config.height = size.height;
                        self.surface.configure(&self.device, &self.config);
                    }
                    
                    WindowEvent::CursorMoved { position, .. } => {
                        // Convert to PointerEvent
                        let event = PointerEvent::Move(PointerEventData {
                            position: Offset::new(position.x, position.y),
                            device_kind: PointerDeviceKind::Mouse,
                        });
                        
                        // Send to GestureBinding
                        self.binding.gesture().handle_pointer_event(event, root_layer);
                    }
                    
                    WindowEvent::MouseInput { state, button, .. } => {
                        // Convert to PointerEvent Down/Up
                        // ...
                    }
                    
                    WindowEvent::KeyboardInput { input, .. } => {
                        // Convert to KeyEvent
                        // ...
                    }
                    
                    _ => {}
                },
                
                Event::MainEventsCleared => {
                    // Request redraw
                    self.window.request_redraw();
                }
                
                Event::RedrawRequested(_) => {
                    // Render frame
                    self.render_frame(start_time.elapsed());
                }
                
                _ => {}
            }
        });
    }
    
    fn render_frame(&mut self, timestamp: Duration) {
        // 1. Begin frame (scheduler callbacks)
        self.binding.scheduler().handle_begin_frame(timestamp);
        
        // 2. Get root layer from element tree
        let root_id = match self.binding.widgets().root_element() {
            Some(id) => id,
            None => return,
        };
        
        let tree = self.binding.widgets().element_tree();
        let tree = tree.read();
        let root_layer = tree.get_root_layer(root_id);
        
        // 3. Draw frame (layout + paint)
        let scene = self.binding.renderer().draw_frame(root_layer);
        
        // 4. Render scene to wgpu
        let output = self.surface.get_current_texture().unwrap();
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());
        
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });
        
        {
            let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 1.0,
                            g: 1.0,
                            b: 1.0,
                            a: 1.0,
                        }),
                        store: true,
                    },
                })],
                depth_stencil_attachment: None,
            });
            
            // TODO: Render scene layers to wgpu
            // scene.layers.iter().for_each(|layer| {
            //     render_layer(layer, &mut render_pass);
            // });
        }
        
        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        
        // 5. Post-frame callbacks
        self.binding.scheduler().handle_draw_frame();
    }
}
```

### 8. runApp() - Main Entry Point

```rust
// crates/flui_app/src/lib.rs

use crate::binding::WidgetsFlutterBinding;
use crate::embedder::wgpu::WgpuEmbedder;
use flui_core::View;
use winit::event_loop::EventLoop;

pub mod binding;
pub mod embedder;
pub mod app;
pub mod window;

/// Run a FLUI app
///
/// # Example
///
/// ```rust,ignore
/// use flui_app::runApp;
/// use flui_widgets::*;
///
/// fn main() {
///     runApp(MyApp::new());
/// }
/// ```
pub fn runApp(app: impl View + 'static) {
    // Initialize tracing
    tracing_subscriber::fmt::init();
    
    // 1. Initialize bindings
    let binding = WidgetsFlutterBinding::ensure_initialized();
    
    // 2. Attach root widget
    binding.widgets.attach_root_widget(app);
    
    // 3. Create event loop
    let event_loop = EventLoop::new();
    
    // 4. Create wgpu embedder
    let embedder = pollster::block_on(WgpuEmbedder::new(binding, &event_loop));
    
    // 5. Run event loop (blocks)
    embedder.run(event_loop);
}
```

---

## Usage Example

```rust
// examples/hello_world.rs

use flui_app::runApp;
use flui_widgets::*;
use flui_core::*;

#[derive(Clone)]
struct Counter {
    initial: i32,
}

impl View for Counter {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let count = use_signal(ctx, self.initial);
        
        Column::new()
            .main_axis_alignment(MainAxisAlignment::Center)
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .children(vec![
                Text::new(format!("Count: {}", count.get())).into(),
                
                GestureDetector::builder()
                    .on_tap(move || count.update(|n| n + 1))
                    .child(
                        Container::new()
                            .padding(EdgeInsets::all(16.0))
                            .color(Color::blue())
                            .child(Text::new("Increment"))
                    )
                    .build()
                    .into(),
            ])
    }
}

fn main() {
    runApp(Counter { initial: 0 });
}
```

---

## Key Differences from Original Plan

| Original (with egui) | Current (pure wgpu) |
|---------------------|-------------------|
| egui handles window | winit creates window |
| egui handles events | winit events → GestureBinding |
| egui renders | wgpu renders Scene |
| ServicesBinding for plugins | Not needed yet |
| Complex embedders | Single WgpuEmbedder |
| Platform channels | Skip for now |

---

## Next Steps

1. **Scene → wgpu renderer** (~500 LOC)
   - Convert Layer tree to wgpu primitives
   - Text rendering (wgpu_glyph or cosmic-text)
   - Shape rendering (rectangles, rounded corners)

2. **Input handling** (~200 LOC)
   - winit events → PointerEvent/KeyEvent
   - Mouse, touch, keyboard mapping

3. **Window management** (~100 LOC)
   - Resize handling
   - DPI scaling

**Total: ~800 LOC core + embedder**

Much simpler than the original 3,500 LOC plan!

---

## Summary

**flui_app is now a THIN INTEGRATION LAYER:**
- ✅ Bindings bridge platform ↔ framework
- ✅ WgpuEmbedder handles winit + wgpu
- ✅ All logic stays in existing crates
- ✅ runApp() is simple entry point

**Most work is in wgpu renderer** (Scene → GPU primitives), not in flui_app itself!
