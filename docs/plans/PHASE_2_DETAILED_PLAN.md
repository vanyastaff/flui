# Phase 2: Rendering Layer - Детальный План Реализации

> **Базируется на**: `docs/plans/2026-01-22-core-architecture-design.md`  
> **Предыдущая фаза**: `docs/plans/PHASE_1_DETAILED_PLAN.md` (должна быть завершена)  
> **Референсы**: `.gpui/src/platform/blade/`, `.gpui/src/scene.rs`, wgpu 25.x Learn Wgpu tutorial  
> **Цель**: Создать production-ready GPU rendering engine с scene graph и композитором

---

## Обзор Текущего Состояния

### ✅ Что Уже Есть

#### flui_engine
- ✅ Cargo.toml с правильными зависимостями (wgpu 25.x, lyon, glyphon, glam, bytemuck)
- ✅ Модульная структура: `wgpu/` backend, `traits.rs`, `commands.rs`, `utils/`
- ✅ Feature flags: `wgpu-backend`, `vulkan`, `metal`, `dx12`, `webgpu`, `gles`
- ✅ Shader files: WGSL shaders для shapes, gradients, effects, masks
- ✅ Базовые модули: `backend.rs`, `painter.rs`, `scene.rs`, `layer_render.rs`
- ✅ Утилиты: `buffer_pool.rs`, `texture_cache.rs`, `tessellator.rs`, `text.rs`
- ✅ Эффекты: `effects.rs`, `effects_pipeline.rs` для blur, shadows

#### flui-layer
- ✅ Scene abstraction: `Scene`, `SceneBuilder`, `LayerTree`
- ✅ Layer types: `CanvasLayer`, `ShaderMaskLayer`
- ✅ Compositor: `SceneCompositor`

#### flui_painting
- ✅ Paint abstraction для brush styles

### ❌ Что Нужно Доделать / Улучшить

#### Ядро Engine
1. **Scene Graph** - финализировать immutable scene design
2. **Primitive Types** - Quad, Path, Sprite, Text primitives
3. **Batching System** - автоматическая группировка примитивов
4. **Layer Compositing** - blend modes, opacity, transforms
5. **Render Pipeline** - настроить wgpu pipelines для каждого primitive type

#### wgpu Backend
1. **Backend Initialization** - Device, Queue, Surface setup
2. **Buffer Management** - vertex buffers, uniform buffers, staging buffers
3. **Texture Atlas** - для sprites/images/glyphs
4. **Shader Pipeline** - compile WGSL shaders, создать render pipelines
5. **Text Rendering** - интеграция glyphon для GPU text
6. **Path Rendering** - lyon tessellation → GPU buffers

#### Compositor
1. **Layer Blending** - правильные blend modes (normal, multiply, screen, etc.)
2. **Transform Stack** - матричные трансформы для layers
3. **Clipping** - stencil buffer для clip regions
4. **Effects** - blur, shadow через render-to-texture

---

## Детальный План Реализации

### Этап 2.1: Scene Graph & Primitives (Неделя 3, Дни 1-3)

#### День 1: Scene Graph Design

**Цель**: Финализировать immutable scene graph architecture

**Референсы**:
- `.gpui/src/scene.rs` - GPUI Scene implementation
- План `3.3.3 Scene Graph Design` - спецификация Scene as Data

**Задачи**:

1. **Обновить `wgpu/scene.rs` - Scene Struct**
   ```rust
   /// Immutable scene graph (built once, can be cached/diffed/replayed)
   #[derive(Clone, Debug)]
   pub struct Scene {
       /// All layers in the scene (ordered by z-index)
       layers: Vec<Layer>,
       
       /// Viewport size (in DevicePixels)
       viewport: Size<f32, DevicePixels>,
       
       /// Global clear color
       clear_color: Color,
   }
   
   impl Scene {
       pub fn builder(viewport: Size<f32, DevicePixels>) -> SceneBuilder {
           SceneBuilder::new(viewport)
       }
       
       pub fn layers(&self) -> &[Layer] {
           &self.layers
       }
       
       pub fn viewport(&self) -> Size<f32, DevicePixels> {
           self.viewport
       }
   }
   ```

2. **SceneBuilder (Fluent API)**
   ```rust
   pub struct SceneBuilder {
       layers: Vec<Layer>,
       current_layer: Option<LayerBuilder>,
       viewport: Size<f32, DevicePixels>,
       clear_color: Color,
   }
   
   impl SceneBuilder {
       pub fn new(viewport: Size<f32, DevicePixels>) -> Self {
           Self {
               layers: Vec::new(),
               current_layer: None,
               viewport,
               clear_color: Color::WHITE,
           }
       }
       
       pub fn clear_color(mut self, color: Color) -> Self {
           self.clear_color = color;
           self
       }
       
       pub fn push_layer(&mut self) -> &mut LayerBuilder {
           if let Some(layer) = self.current_layer.take() {
               self.layers.push(layer.build());
           }
           
           self.current_layer = Some(LayerBuilder::default());
           self.current_layer.as_mut().unwrap()
       }
       
       pub fn build(mut self) -> Scene {
           if let Some(layer) = self.current_layer.take() {
               self.layers.push(layer.build());
           }
           
           Scene {
               layers: self.layers,
               viewport: self.viewport,
               clear_color: self.clear_color,
           }
       }
   }
   ```

3. **Layer & LayerBuilder**
   ```rust
   #[derive(Clone, Debug)]
   pub struct Layer {
       primitives: Vec<Primitive>,
       transform: Matrix4,
       opacity: f32,
       blend_mode: BlendMode,
       clip: Option<Rect<f32, DevicePixels>>,
   }
   
   pub struct LayerBuilder {
       primitives: Vec<Primitive>,
       transform: Matrix4,
       opacity: f32,
       blend_mode: BlendMode,
       clip: Option<Rect<f32, DevicePixels>>,
   }
   
   impl LayerBuilder {
       pub fn add_rect(&mut self, rect: Rect<f32, DevicePixels>, color: Color) -> &mut Self {
           self.primitives.push(Primitive::Rect {
               rect,
               color,
               border_radius: 0.0,
           });
           self
       }
       
       pub fn add_text(&mut self, text: String, pos: Point<f32, DevicePixels>, style: TextStyle) -> &mut Self {
           self.primitives.push(Primitive::Text {
               text,
               position: pos,
               style,
               color: Color::BLACK,
           });
           self
       }
       
       pub fn transform(mut self, transform: Matrix4) -> Self {
           self.transform = transform;
           self
       }
       
       pub fn opacity(mut self, opacity: f32) -> Self {
           self.opacity = opacity;
           self
       }
       
       pub fn build(self) -> Layer {
           Layer {
               primitives: self.primitives,
               transform: self.transform,
               opacity: self.opacity,
               blend_mode: self.blend_mode,
               clip: self.clip,
           }
       }
   }
   ```

**Критерии завершения**:
- [ ] Scene is immutable after build
- [ ] SceneBuilder fluent API works
- [ ] Can serialize Scene (for debugging)
- [ ] 20+ scene graph tests

---

#### День 2: Primitive Types

**Цель**: Определить все rendering primitives

**Референсы**:
- `.gpui/src/scene.rs` - GPUI primitive types
- План `3.3.3` - Primitive enum

**Задачи**:

1. **Создать `wgpu/primitives.rs`**
   ```rust
   use flui_types::*;
   
   /// Rendering primitive (what to draw)
   #[derive(Clone, Debug)]
   pub enum Primitive {
       /// Solid or gradient-filled rectangle
       Rect {
           rect: Rect<f32, DevicePixels>,
           color: Color,
           border_radius: f32,
       },
       
       /// Text glyph run
       Text {
           text: String,
           position: Point<f32, DevicePixels>,
           style: TextStyle,
           color: Color,
       },
       
       /// Arbitrary path (filled or stroked)
       Path {
           path: Path,
           fill: Option<Color>,
           stroke: Option<(Color, f32)>,
       },
       
       /// Image/sprite from texture
       Image {
           texture_id: TextureId,
           src_rect: Rect<f32, DevicePixels>,
           dst_rect: Rect<f32, DevicePixels>,
       },
       
       /// Underline decoration
       Underline {
           start: Point<f32, DevicePixels>,
           end: Point<f32, DevicePixels>,
           thickness: f32,
           color: Color,
       },
       
       /// Shadow (gaussian blur)
       Shadow {
           rect: Rect<f32, DevicePixels>,
           blur_radius: f32,
           offset: Offset<f32, DevicePixels>,
           color: Color,
       },
   }
   
   impl Primitive {
       /// Get bounding box of primitive
       pub fn bounds(&self) -> Rect<f32, DevicePixels> {
           match self {
               Primitive::Rect { rect, .. } => *rect,
               Primitive::Text { position, style, .. } => {
                   // Approximate bounds (refined later with text layout)
                   Rect::from_origin_size(*position, Size::new(100.0, style.font_size))
               }
               Primitive::Path { path, .. } => path.bounds(),
               Primitive::Image { dst_rect, .. } => *dst_rect,
               Primitive::Underline { start, end, thickness, .. } => {
                   Rect::from_points(*start, *end).inflate(thickness / 2.0, thickness / 2.0)
               }
               Primitive::Shadow { rect, blur_radius, offset, .. } => {
                   rect.translate(*offset).inflate(blur_radius, blur_radius)
               }
           }
       }
   }
   
   #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
   pub struct TextureId(pub u32);
   
   /// 2D path for vector shapes
   #[derive(Clone, Debug)]
   pub struct Path {
       pub vertices: Vec<Point<f32, DevicePixels>>,
       pub closed: bool,
   }
   
   impl Path {
       pub fn bounds(&self) -> Rect<f32, DevicePixels> {
           if self.vertices.is_empty() {
               return Rect::zero();
           }
           
           let mut min_x = self.vertices[0].x;
           let mut max_x = min_x;
           let mut min_y = self.vertices[0].y;
           let mut max_y = min_y;
           
           for v in &self.vertices[1..] {
               min_x = min_x.min(v.x);
               max_x = max_x.max(v.x);
               min_y = min_y.min(v.y);
               max_y = max_y.max(v.y);
           }
           
           Rect::from_min_max(
               Point::new(min_x, min_y),
               Point::new(max_x, max_y),
           )
       }
   }
   ```

2. **BlendMode Enum**
   ```rust
   #[derive(Copy, Clone, Debug, PartialEq, Eq)]
   pub enum BlendMode {
       Normal,
       Multiply,
       Screen,
       Overlay,
       Darken,
       Lighten,
       ColorDodge,
       ColorBurn,
       HardLight,
       SoftLight,
       Difference,
       Exclusion,
   }
   
   impl BlendMode {
       /// Convert to wgpu blend state
       pub fn to_wgpu_blend(&self) -> wgpu::BlendState {
           match self {
               BlendMode::Normal => wgpu::BlendState::ALPHA_BLENDING,
               // ... other modes
               _ => todo!("Implement blend mode: {:?}", self),
           }
       }
   }
   ```

**Критерии завершения**:
- [ ] All primitive types defined
- [ ] Bounds calculation works
- [ ] BlendMode conversions
- [ ] 15+ primitive tests

---

#### День 3: Batching System

**Цель**: Автоматическая группировка primitives для эффективного rendering

**Референсы**:
- `.gpui/src/scene.rs` - GPUI batching logic

**Задачи**:

1. **Primitive Batching**
   ```rust
   /// Batch of similar primitives (same pipeline, textures)
   #[derive(Clone, Debug)]
   pub struct PrimitiveBatch {
       pub primitive_type: PrimitiveType,
       pub primitives: Vec<Primitive>,
       pub texture_id: Option<TextureId>,
   }
   
   #[derive(Copy, Clone, Debug, PartialEq, Eq)]
   pub enum PrimitiveType {
       Rect,
       Text,
       Path,
       Image,
       Underline,
       Shadow,
   }
   
   impl Scene {
       /// Group primitives into batches for efficient rendering
       pub fn batch_primitives(&self) -> Vec<PrimitiveBatch> {
           let mut batches = Vec::new();
           
           for layer in &self.layers {
               let layer_batches = Self::batch_layer_primitives(&layer.primitives);
               batches.extend(layer_batches);
           }
           
           batches
       }
       
       fn batch_layer_primitives(primitives: &[Primitive]) -> Vec<PrimitiveBatch> {
           let mut batches: Vec<PrimitiveBatch> = Vec::new();
           
           for primitive in primitives {
               let prim_type = match primitive {
                   Primitive::Rect { .. } => PrimitiveType::Rect,
                   Primitive::Text { .. } => PrimitiveType::Text,
                   Primitive::Path { .. } => PrimitiveType::Path,
                   Primitive::Image { texture_id, .. } => {
                       // Try to batch with existing image batch
                       if let Some(batch) = batches.iter_mut()
                           .find(|b| b.primitive_type == PrimitiveType::Image 
                                  && b.texture_id == Some(*texture_id))
                       {
                           batch.primitives.push(primitive.clone());
                           continue;
                       }
                       PrimitiveType::Image
                   }
                   Primitive::Underline { .. } => PrimitiveType::Underline,
                   Primitive::Shadow { .. } => PrimitiveType::Shadow,
               };
               
               // Check if can batch with previous
               if let Some(last_batch) = batches.last_mut() {
                   if last_batch.primitive_type == prim_type && last_batch.texture_id.is_none() {
                       last_batch.primitives.push(primitive.clone());
                       continue;
                   }
               }
               
               // Create new batch
               batches.push(PrimitiveBatch {
                   primitive_type: prim_type,
                   primitives: vec![primitive.clone()],
                   texture_id: match primitive {
                       Primitive::Image { texture_id, .. } => Some(*texture_id),
                       _ => None,
                   },
               });
           }
           
           batches
       }
   }
   ```

**Критерии завершения**:
- [ ] Batching reduces draw calls
- [ ] Same-type primitives grouped
- [ ] Texture batching works
- [ ] 10+ batching tests

---

### Этап 2.2: wgpu Backend Setup (Неделя 3-4, Дни 4-7)

#### День 4: Device & Surface Initialization

**Цель**: Настроить wgpu Device, Queue, Surface

**Референсы**:
- `.gpui/src/platform/blade/blade_context.rs` - GPUI device setup
- Learn Wgpu tutorial - Device creation

**Задачи**:

1. **Обновить `wgpu/backend.rs`**
   ```rust
   use wgpu::*;
   use std::sync::Arc;
   
   /// wgpu rendering backend
   pub struct Backend {
       /// GPU device
       device: Arc<Device>,
       
       /// Command queue
       queue: Arc<Queue>,
       
       /// Surface configuration
       surface_config: SurfaceConfiguration,
       
       /// Surface (per window)
       surface: Surface<'static>,
   }
   
   impl Backend {
       /// Create backend from platform window
       pub async fn new(
           window: &dyn PlatformWindow,
           width: u32,
           height: u32,
       ) -> Result<Self, RenderError> {
           // Create wgpu instance
           let instance = Instance::new(InstanceDescriptor {
               backends: Backends::all(),
               ..Default::default()
           });
           
           // Create surface
           let surface = unsafe {
               instance.create_surface_unsafe(
                   SurfaceTargetUnsafe::from_window(window)?,
               )?
           };
           
           // Request adapter
           let adapter = instance.request_adapter(&RequestAdapterOptions {
               power_preference: PowerPreference::HighPerformance,
               compatible_surface: Some(&surface),
               force_fallback_adapter: false,
           })
           .await
           .ok_or(RenderError::NoAdapter)?;
           
           tracing::info!("GPU Adapter: {:?}", adapter.get_info());
           
           // Request device
           let (device, queue) = adapter.request_device(
               &DeviceDescriptor {
                   label: Some("FLUI wgpu Device"),
                   required_features: Features::empty(),
                   required_limits: Limits::default(),
                   memory_hints: MemoryHints::default(),
               },
               None,
           )
           .await?;
           
           let device = Arc::new(device);
           let queue = Arc::new(queue);
           
           // Configure surface
           let surface_caps = surface.get_capabilities(&adapter);
           let surface_format = surface_caps
               .formats
               .iter()
               .find(|f| f.is_srgb())
               .copied()
               .unwrap_or(surface_caps.formats[0]);
           
           let surface_config = SurfaceConfiguration {
               usage: TextureUsages::RENDER_ATTACHMENT,
               format: surface_format,
               width,
               height,
               present_mode: PresentMode::Fifo, // VSync
               alpha_mode: surface_caps.alpha_modes[0],
               view_formats: vec![],
               desired_maximum_frame_latency: 2,
           };
           
           surface.configure(&device, &surface_config);
           
           Ok(Self {
               device,
               queue,
               surface_config,
               surface,
           })
       }
       
       pub fn device(&self) -> &Device {
           &self.device
       }
       
       pub fn queue(&self) -> &Queue {
           &self.queue
       }
       
       pub fn resize(&mut self, new_width: u32, new_height: u32) {
           self.surface_config.width = new_width;
           self.surface_config.height = new_height;
           self.surface.configure(&self.device, &self.surface_config);
       }
   }
   ```

2. **Error Handling**
   ```rust
   // Update error.rs
   #[derive(Debug, thiserror::Error)]
   pub enum RenderError {
       #[error("No suitable GPU adapter found")]
       NoAdapter,
       
       #[error("Failed to request device: {0}")]
       DeviceRequest(#[from] wgpu::RequestDeviceError),
       
       #[error("Surface error: {0}")]
       Surface(#[from] wgpu::CreateSurfaceError),
       
       #[error("Platform error: {0}")]
       Platform(String),
   }
   ```

**Критерии завершения**:
- [ ] Backend creates device/queue
- [ ] Surface configuration works
- [ ] Resize handling correct
- [ ] Error handling comprehensive

---

#### День 5: Shader Pipeline Setup

**Цель**: Compile WGSL shaders и create render pipelines

**Референсы**:
- `.gpui/src/platform/blade/blade_renderer.rs` - GPUI shader pipelines
- Learn Wgpu - RenderPipeline creation
- Existing shaders in `wgpu/shaders/`

**Задачи**:

1. **Shader Compilation (обновить `wgpu/shader_compiler.rs`)**
   ```rust
   use std::borrow::Cow;
   
   /// Shader module cache
   pub struct ShaderCache {
       rect_shader: ShaderModule,
       text_shader: ShaderModule,
       path_shader: ShaderModule,
       image_shader: ShaderModule,
   }
   
   impl ShaderCache {
       pub fn new(device: &Device) -> Self {
           Self {
               rect_shader: Self::compile_shader(
                   device,
                   "Rect Shader",
                   include_str!("shaders/rect_instanced.wgsl"),
               ),
               text_shader: Self::compile_shader(
                   device,
                   "Text Shader",
                   include_str!("shaders/text.wgsl"),
               ),
               path_shader: Self::compile_shader(
                   device,
                   "Path Shader",
                   include_str!("shaders/fill.wgsl"),
               ),
               image_shader: Self::compile_shader(
                   device,
                   "Image Shader",
                   include_str!("shaders/texture_instanced.wgsl"),
               ),
           }
       }
       
       fn compile_shader(device: &Device, label: &str, source: &str) -> ShaderModule {
           device.create_shader_module(ShaderModuleDescriptor {
               label: Some(label),
               source: ShaderSource::Wgsl(Cow::Borrowed(source)),
           })
       }
   }
   ```

2. **Pipeline Creation (обновить `wgpu/pipeline.rs`)**
   ```rust
   pub struct RenderPipelines {
       rect_pipeline: RenderPipeline,
       text_pipeline: RenderPipeline,
       path_pipeline: RenderPipeline,
       image_pipeline: RenderPipeline,
   }
   
   impl RenderPipelines {
       pub fn new(
           device: &Device,
           shader_cache: &ShaderCache,
           surface_format: TextureFormat,
       ) -> Self {
           // Rect pipeline
           let rect_pipeline = Self::create_rect_pipeline(
               device,
               &shader_cache.rect_shader,
               surface_format,
           );
           
           // Text pipeline (с glyphon)
           let text_pipeline = Self::create_text_pipeline(
               device,
               &shader_cache.text_shader,
               surface_format,
           );
           
           // Path pipeline (lyon tessellation)
           let path_pipeline = Self::create_path_pipeline(
               device,
               &shader_cache.path_shader,
               surface_format,
           );
           
           // Image/sprite pipeline
           let image_pipeline = Self::create_image_pipeline(
               device,
               &shader_cache.image_shader,
               surface_format,
           );
           
           Self {
               rect_pipeline,
               text_pipeline,
               path_pipeline,
               image_pipeline,
           }
       }
       
       fn create_rect_pipeline(
           device: &Device,
           shader: &ShaderModule,
           surface_format: TextureFormat,
       ) -> RenderPipeline {
           let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
               label: Some("Rect Pipeline Layout"),
               bind_group_layouts: &[],
               push_constant_ranges: &[],
           });
           
           device.create_render_pipeline(&RenderPipelineDescriptor {
               label: Some("Rect Pipeline"),
               layout: Some(&pipeline_layout),
               vertex: VertexState {
                   module: shader,
                   entry_point: Some("vs_main"),
                   buffers: &[RectVertex::desc()],
                   compilation_options: Default::default(),
               },
               fragment: Some(FragmentState {
                   module: shader,
                   entry_point: Some("fs_main"),
                   targets: &[Some(ColorTargetState {
                       format: surface_format,
                       blend: Some(BlendState::ALPHA_BLENDING),
                       write_mask: ColorWrites::ALL,
                   })],
                   compilation_options: Default::default(),
               }),
               primitive: PrimitiveState {
                   topology: PrimitiveTopology::TriangleList,
                   strip_index_format: None,
                   front_face: FrontFace::Ccw,
                   cull_mode: None,
                   polygon_mode: PolygonMode::Fill,
                   unclipped_depth: false,
                   conservative: false,
               },
               depth_stencil: None,
               multisample: MultisampleState {
                   count: 1,
                   mask: !0,
                   alpha_to_coverage_enabled: false,
               },
               multiview: None,
               cache: None,
           })
       }
       
       // Similar for other pipelines...
   }
   ```

**Критерии завершения**:
- [ ] All shaders compile without errors
- [ ] Pipelines created for all primitive types
- [ ] Blend states configured correctly
- [ ] Vertex buffer layouts match shaders

---

#### День 6: Buffer Management

**Цель**: Efficient vertex/uniform buffer handling

**Референсы**:
- `.gpui/src/platform/blade/blade_renderer.rs` - Buffer management
- Learn Wgpu - Buffers tutorial

**Задачи**:

1. **Vertex Types (обновить `wgpu/vertex.rs`)**
   ```rust
   use bytemuck::{Pod, Zeroable};
   
   /// Rect vertex (instanced rendering)
   #[repr(C)]
   #[derive(Copy, Clone, Debug, Pod, Zeroable)]
   pub struct RectVertex {
       pub position: [f32; 2],
       pub color: [f32; 4],
       pub border_radius: f32,
       pub _padding: [f32; 3],
   }
   
   impl RectVertex {
       pub fn desc() -> VertexBufferLayout<'static> {
           VertexBufferLayout {
               array_stride: std::mem::size_of::<RectVertex>() as BufferAddress,
               step_mode: VertexStepMode::Instance,
               attributes: &[
                   VertexAttribute {
                       offset: 0,
                       shader_location: 0,
                       format: VertexFormat::Float32x2,
                   },
                   VertexAttribute {
                       offset: std::mem::size_of::<[f32; 2]>() as BufferAddress,
                       shader_location: 1,
                       format: VertexFormat::Float32x4,
                   },
                   VertexAttribute {
                       offset: std::mem::size_of::<[f32; 6]>() as BufferAddress,
                       shader_location: 2,
                       format: VertexFormat::Float32,
                   },
               ],
           }
       }
   }
   
   /// Path vertex (tessellated)
   #[repr(C)]
   #[derive(Copy, Clone, Debug, Pod, Zeroable)]
   pub struct PathVertex {
       pub position: [f32; 2],
   }
   
   // Similar для Text, Image vertices...
   ```

2. **Buffer Pool (финализировать `wgpu/buffer_pool.rs`)**
   ```rust
   use wgpu::util::DeviceExt;
   
   /// Reusable buffer pool для reducing allocations
   pub struct BufferPool {
       device: Arc<Device>,
       vertex_buffers: Vec<Buffer>,
       index_buffers: Vec<Buffer>,
       uniform_buffers: Vec<Buffer>,
   }
   
   impl BufferPool {
       pub fn new(device: Arc<Device>) -> Self {
           Self {
               device,
               vertex_buffers: Vec::new(),
               index_buffers: Vec::new(),
               uniform_buffers: Vec::new(),
           }
       }
       
       /// Get or create vertex buffer
       pub fn get_vertex_buffer(&mut self, size: u64) -> Buffer {
           // Try to reuse existing buffer
           if let Some(idx) = self.vertex_buffers.iter()
               .position(|b| b.size() >= size) 
           {
               return self.vertex_buffers.swap_remove(idx);
           }
           
           // Create new buffer
           self.device.create_buffer(&BufferDescriptor {
               label: Some("Vertex Buffer"),
               size,
               usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
               mapped_at_creation: false,
           })
       }
       
       /// Return buffer to pool for reuse
       pub fn return_vertex_buffer(&mut self, buffer: Buffer) {
           self.vertex_buffers.push(buffer);
       }
       
       /// Write data to buffer
       pub fn write_buffer<T: Pod>(
           &self,
           queue: &Queue,
           buffer: &Buffer,
           data: &[T],
       ) {
           queue.write_buffer(buffer, 0, bytemuck::cast_slice(data));
       }
   }
   ```

**Критерии завершения**:
- [ ] All vertex types Pod + Zeroable
- [ ] Buffer pool reduces allocations
- [ ] Write operations efficient
- [ ] Memory usage reasonable

---

#### День 7: Texture Atlas & Text Rendering

**Цель**: GPU texture management и glyphon integration

**Референсы**:
- `.gpui/src/platform/blade/blade_atlas.rs` - Texture atlas
- glyphon docs - GPU text rendering

**Задачи**:

1. **Texture Atlas (финализировать `wgpu/texture_cache.rs`)**
   ```rust
   /// Simple 2D texture packer (shelf packing)
   pub struct TextureAtlas {
       texture: Texture,
       texture_view: TextureView,
       width: u32,
       height: u32,
       
       /// Free rects (shelf packing)
       shelves: Vec<Shelf>,
   }
   
   struct Shelf {
       y: u32,
       height: u32,
       x: u32, // Current x position
   }
   
   impl TextureAtlas {
       pub fn new(device: &Device, width: u32, height: u32) -> Self {
           let texture = device.create_texture(&TextureDescriptor {
               label: Some("Texture Atlas"),
               size: Extent3d { width, height, depth_or_array_layers: 1 },
               mip_level_count: 1,
               sample_count: 1,
               dimension: TextureDimension::D2,
               format: TextureFormat::Rgba8UnormSrgb,
               usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
               view_formats: &[],
           });
           
           let texture_view = texture.create_view(&TextureViewDescriptor::default());
           
           Self {
               texture,
               texture_view,
               width,
               height,
               shelves: vec![Shelf { y: 0, height: 0, x: 0 }],
           }
       }
       
       /// Allocate region in atlas
       pub fn allocate(&mut self, width: u32, height: u32) -> Option<AtlasTile> {
           // Try to fit in existing shelf
           for shelf in &mut self.shelves {
               if shelf.height >= height && shelf.x + width <= self.width {
                   let tile = AtlasTile {
                       x: shelf.x,
                       y: shelf.y,
                       width,
                       height,
                   };
                   shelf.x += width;
                   return Some(tile);
               }
           }
           
           // Create new shelf
           let last_shelf = self.shelves.last().unwrap();
           let new_y = last_shelf.y + last_shelf.height;
           
           if new_y + height > self.height {
               return None; // Atlas full
           }
           
           let tile = AtlasTile {
               x: 0,
               y: new_y,
               width,
               height,
           };
           
           self.shelves.push(Shelf {
               y: new_y,
               height,
               x: width,
           });
           
           Some(tile)
       }
       
       /// Upload texture data to atlas
       pub fn upload(&self, queue: &Queue, tile: &AtlasTile, data: &[u8]) {
           queue.write_texture(
               ImageCopyTexture {
                   texture: &self.texture,
                   mip_level: 0,
                   origin: Origin3d {
                       x: tile.x,
                       y: tile.y,
                       z: 0,
                   },
                   aspect: TextureAspect::All,
               },
               data,
               ImageDataLayout {
                   offset: 0,
                   bytes_per_row: Some(tile.width * 4),
                   rows_per_image: Some(tile.height),
               },
               Extent3d {
                   width: tile.width,
                   height: tile.height,
                   depth_or_array_layers: 1,
               },
           );
       }
   }
   
   #[derive(Copy, Clone, Debug)]
   pub struct AtlasTile {
       pub x: u32,
       pub y: u32,
       pub width: u32,
       pub height: u32,
   }
   ```

2. **Glyphon Text Integration (обновить `wgpu/text.rs`)**
   ```rust
   use glyphon::{
       FontSystem, SwashCache, TextAtlas, TextRenderer,
       TextBounds, Buffer, Metrics, Family, Weight,
   };
   
   pub struct TextRenderSystem {
       font_system: FontSystem,
       swash_cache: SwashCache,
       text_atlas: TextAtlas,
       text_renderer: TextRenderer,
   }
   
   impl TextRenderSystem {
       pub fn new(device: &Device, queue: &Queue, surface_format: TextureFormat) -> Self {
           let mut font_system = FontSystem::new();
           let swash_cache = SwashCache::new();
           let mut text_atlas = TextAtlas::new(device, queue, surface_format);
           let text_renderer = TextRenderer::new(
               &mut text_atlas,
               device,
               MultisampleState::default(),
               None,
           );
           
           Self {
               font_system,
               swash_cache,
               text_atlas,
               text_renderer,
           }
       }
       
       pub fn prepare_text(
           &mut self,
           device: &Device,
           queue: &Queue,
           text: &str,
           position: Point<f32, DevicePixels>,
           style: &TextStyle,
       ) -> Result<(), RenderError> {
           // Create text buffer
           let mut buffer = Buffer::new(
               &mut self.font_system,
               Metrics::new(style.font_size, style.font_size * 1.2),
           );
           
           buffer.set_size(&mut self.font_system, f32::MAX, f32::MAX);
           buffer.set_text(
               &mut self.font_system,
               text,
               glyphon::Attrs::new().family(Family::SansSerif),
               glyphon::Shaping::Advanced,
           );
           
           // Prepare for rendering
           self.text_renderer.prepare(
               device,
               queue,
               &mut self.font_system,
               &mut self.text_atlas,
               &[buffer],
               &mut self.swash_cache,
           )?;
           
           Ok(())
       }
   }
   ```

**Критерии завершения**:
- [ ] Texture atlas packing works
- [ ] glyphon text renders correctly
- [ ] Atlas auto-grows when full
- [ ] Text metrics correct

---

### Этап 2.3: Compositor & Effects (Неделя 4, Дни 8-10)

#### День 8: Layer Compositing

**Цель**: Blend modes и transform stack

**Задачи**:

1. **Transform Stack**
   ```rust
   pub struct TransformStack {
       stack: Vec<Matrix4>,
   }
   
   impl TransformStack {
       pub fn new() -> Self {
           Self {
               stack: vec![Matrix4::identity()],
           }
       }
       
       pub fn push(&mut self, transform: Matrix4) {
           let current = self.current();
           self.stack.push(current * transform);
       }
       
       pub fn pop(&mut self) {
           if self.stack.len() > 1 {
               self.stack.pop();
           }
       }
       
       pub fn current(&self) -> Matrix4 {
           *self.stack.last().unwrap()
       }
   }
   ```

2. **Blend State Mapping**
   ```rust
   impl BlendMode {
       pub fn to_wgpu_blend(&self) -> BlendState {
           match self {
               BlendMode::Normal => BlendState::ALPHA_BLENDING,
               BlendMode::Multiply => BlendState {
                   color: BlendComponent {
                       src_factor: BlendFactor::Dst,
                       dst_factor: BlendFactor::OneMinusSrcAlpha,
                       operation: BlendOperation::Add,
                   },
                   alpha: BlendComponent::OVER,
               },
               // ... implement all blend modes
           }
       }
   }
   ```

**Критерии завершения**:
- [ ] Transform stack works
- [ ] All blend modes implemented
- [ ] Layer opacity correct

---

#### День 9: Effects (Blur, Shadow)

**Цель**: Render-to-texture effects

**Референсы**:
- Existing shaders: `shaders/effects/blur_*.wgsl`, `shaders/effects/shadow.wgsl`

**Задачи**:

1. **Blur Pipeline (финализировать `wgpu/effects_pipeline.rs`)**
   ```rust
   pub struct BlurPipeline {
       downsample_pipeline: RenderPipeline,
       horizontal_pipeline: RenderPipeline,
       vertical_pipeline: RenderPipeline,
       upsample_pipeline: RenderPipeline,
   }
   
   impl BlurPipeline {
       pub fn blur(
           &self,
           encoder: &mut CommandEncoder,
           device: &Device,
           input: &TextureView,
           output: &TextureView,
           blur_radius: f32,
       ) {
           // Multi-pass gaussian blur
           // 1. Downsample
           // 2. Horizontal blur
           // 3. Vertical blur  
           // 4. Upsample
       }
   }
   ```

**Критерии завершения**:
- [ ] Blur looks good
- [ ] Shadow rendering works
- [ ] Performance acceptable

---

#### День 10: Integration & Testing

**Цель**: End-to-end scene rendering

**Задачи**:

1. **SceneRenderer (финализировать `wgpu/scene.rs`)**
   ```rust
   pub struct SceneRenderer {
       backend: Backend,
       pipelines: RenderPipelines,
       buffer_pool: BufferPool,
       texture_atlas: TextureAtlas,
       text_system: TextRenderSystem,
   }
   
   impl SceneRenderer {
       pub fn render_scene(&mut self, scene: &Scene) -> Result<(), RenderError> {
           let frame = self.backend.surface.get_current_texture()?;
           let view = frame.texture.create_view(&Default::default());
           
           let mut encoder = self.backend.device.create_command_encoder(&Default::default());
           
           {
               let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                   label: Some("Main Render Pass"),
                   color_attachments: &[Some(RenderPassColorAttachment {
                       view: &view,
                       resolve_target: None,
                       ops: Operations {
                           load: LoadOp::Clear(scene.clear_color.into()),
                           store: StoreOp::Store,
                       },
                   })],
                   depth_stencil_attachment: None,
                   timestamp_writes: None,
                   occlusion_query_set: None,
               });
               
               // Render each layer
               for layer in scene.layers() {
                   self.render_layer(&mut render_pass, layer)?;
               }
           }
           
           self.backend.queue.submit(Some(encoder.finish()));
           frame.present();
           
           Ok(())
       }
       
       fn render_layer(
           &mut self,
           render_pass: &mut RenderPass,
           layer: &Layer,
       ) -> Result<(), RenderError> {
           // Apply layer transform
           // Set blend mode
           // Render primitives
           for primitive in &layer.primitives {
               self.render_primitive(render_pass, primitive)?;
           }
           Ok(())
       }
   }
   ```

2. **Comprehensive Tests**
   ```rust
   #[test]
   fn test_render_simple_scene() {
       let scene = Scene::builder(Size::new(800.0, 600.0))
           .push_layer()
               .add_rect(Rect::new(100.0, 100.0, 200.0, 200.0), Color::RED)
               .add_text("Hello FLUI".to_string(), Point::new(150.0, 150.0), TextStyle::default())
           .build();
       
       let renderer = SceneRenderer::new(/* ... */);
       renderer.render_scene(&scene).unwrap();
   }
   ```

**Критерии завершения**:
- [ ] Can render complete scenes
- [ ] All primitive types work
- [ ] Performance is good (60fps for typical scenes)
- [ ] Memory usage reasonable

---

## Критерии Завершения Phase 2

### Обязательные Требования

- [ ] **flui_engine 0.1.0**
  - [ ] Scene graph immutable and cacheable
  - [ ] All primitive types (Rect, Text, Path, Image) render correctly
  - [ ] wgpu backend works на Windows/macOS/Linux
  - [ ] Text rendering с glyphon
  - [ ] Path rendering с lyon
  - [ ] Layer compositing с blend modes
  - [ ] 150+ rendering tests
  - [ ] 60fps для сцен с 1000+ primitives

### Бонусные Цели

- [ ] Effects (blur, shadow) реализованы
- [ ] Texture atlas auto-grow
- [ ] Multi-window support
- [ ] Scene diff/patch для incremental updates

---

## Примеры Использования

### Example 1: Simple Rectangle

```rust
use flui_engine::*;
use flui_types::*;

let scene = Scene::builder(Size::new(800.0, 600.0))
    .clear_color(Color::WHITE)
    .push_layer()
        .add_rect(
            Rect::new(100.0, 100.0, 200.0, 200.0),
            Color::from_rgb(1.0, 0.0, 0.0),
        )
    .build();

let mut renderer = SceneRenderer::new(surface, 800, 600).await?;
renderer.render_scene(&scene)?;
```

### Example 2: Text Rendering

```rust
let scene = Scene::builder(Size::new(800.0, 600.0))
    .push_layer()
        .add_text(
            "Hello, FLUI!".to_string(),
            Point::new(100.0, 100.0),
            TextStyle {
                font_size: 24.0,
                font_weight: FontWeight::Bold,
                color: Color::BLACK,
                ..Default::default()
            },
        )
    .build();
```

### Example 3: Layered Composition

```rust
let scene = Scene::builder(Size::new(800.0, 600.0))
    // Background layer
    .push_layer()
        .add_rect(Rect::from_size(Size::new(800.0, 600.0)), Color::WHITE)
    
    // Content layer with transform
    .push_layer()
        .transform(Matrix4::translate(100.0, 100.0))
        .opacity(0.8)
        .add_rect(Rect::from_size(Size::new(200.0, 200.0)), Color::RED)
    
    // Overlay layer with blend mode
    .push_layer()
        .blend_mode(BlendMode::Multiply)
        .add_rect(Rect::new(150.0, 150.0, 200.0, 200.0), Color::BLUE)
    
    .build();
```

---

## Troubleshooting Guide

### Issue: wgpu device creation fails

**Solution**:
```rust
// Проверьте доступные backends
let backends = Backends::PRIMARY; // Vulkan/Metal/DX12
// Или fallback:
let backends = Backends::all();
```

### Issue: Шейдеры не компилируются

**Solution**:
```rust
// Проверьте версию WGSL синтаксиса
// wgpu 25.x требует новый синтаксис:
@vertex
fn vs_main(...) -> @builtin(position) vec4<f32> { ... }
```

### Issue: Text не отображается

**Solution**:
```rust
// Убедитесь что glyphon text_atlas prepared:
text_system.prepare_text(device, queue, text, pos, style)?;
```

---

## Следующие Шаги (Phase 3 Preview)

После завершения Phase 2:

1. **flui_interaction** - event routing, hit testing, gestures
2. **flui_app** - application lifecycle, window management
3. Integration всех layers

---

**Статус**: 🟡 Ready for Implementation  
**Последнее обновление**: 2026-01-22  
**Автор**: Claude with executing-plans skill  
**Базируется на**: docs/plans/2026-01-22-core-architecture-design.md, PHASE_1_DETAILED_PLAN.md
