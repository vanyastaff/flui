# Chapter 6: Render Backend

## 📋 Overview

Render Backend - это platform abstraction layer между FLUI's layer system и actual rendering (GPU or CPU). FLUI поддерживает multiple backends: **wgpu** (GPU-accelerated, primary), **software rasterizer** (CPU fallback), и **egui** (for dev tools).

## 🎯 Backend Abstraction

### RenderBackend Trait

```rust
/// RenderBackend - platform-independent rendering interface
pub trait RenderBackend: Send + Sync {
    /// Initialize backend
    fn init(&mut self, window: &Window) -> Result<(), BackendError>;
    
    /// Begin frame
    fn begin_frame(&mut self, window_size: Size);
    
    /// End frame and present
    fn end_frame(&mut self);
    
    /// Rasterize picture to texture
    fn rasterize_picture(
        &mut self,
        picture: &Picture,
        transform: Mat4,
    ) -> Arc<Texture>;
    
    /// Draw texture with transform
    fn draw_texture(&mut self, texture: Arc<Texture>, transform: Mat4);
    
    /// Clear screen
    fn clear(&mut self, color: Color);
    
    /// Get backend name
    fn name(&self) -> &'static str;
    
    /// Get capabilities
    fn capabilities(&self) -> BackendCapabilities;
    
    /// Resize surface
    fn resize(&mut self, new_size: Size);
    
    /// Take screenshot
    fn screenshot(&mut self) -> Result<Image, BackendError>;
}

/// Backend capabilities
#[derive(Debug, Clone)]
pub struct BackendCapabilities {
    /// Supports GPU acceleration
    pub gpu_accelerated: bool,
    
    /// Max texture size
    pub max_texture_size: u32,
    
    /// Supports compute shaders
    pub compute_shaders: bool,
    
    /// Supports MSAA
    pub msaa: bool,
    
    /// Max MSAA samples
    pub max_msaa_samples: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("Initialization failed: {0}")]
    InitFailed(String),
    
    #[error("Surface lost")]
    SurfaceLost,
    
    #[error("Out of memory")]
    OutOfMemory,
    
    #[error("Unsupported operation: {0}")]
    Unsupported(String),
}
```

### Texture

```rust
/// Texture - GPU/CPU texture handle
pub struct Texture {
    /// Backend-specific handle
    handle: TextureHandle,
    
    /// Texture size
    size: Size,
    
    /// Format
    format: TextureFormat,
}

pub enum TextureHandle {
    Wgpu(wgpu::Texture),
    Software(Arc<ImageBuffer<Rgba<u8>>>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureFormat {
    Rgba8,
    Bgra8,
    Rgb8,
    R8,
}
```

---

## 🖥️ wgpu Backend (Primary)

### Implementation

```rust
use wgpu::*;

/// wgpu backend - GPU-accelerated rendering
pub struct WgpuBackend {
    /// wgpu instance
    instance: Instance,
    
    /// Surface for rendering
    surface: Option<Surface>,
    
    /// Device
    device: Device,
    
    /// Queue
    queue: Queue,
    
    /// Surface config
    surface_config: Option<SurfaceConfiguration>,
    
    /// Render pipeline
    pipeline: RenderPipeline,
    
    /// Texture atlas for batching
    atlas: TextureAtlas,
    
    /// Vertex buffer
    vertex_buffer: Buffer,
    
    /// Index buffer
    index_buffer: Buffer,
    
    /// Staging belt for dynamic data
    staging_belt: StagingBelt,
}

impl WgpuBackend {
    pub async fn new() -> Result<Self, BackendError> {
        // Create instance
        let instance = Instance::new(InstanceDescriptor {
            backends: Backends::all(),
            dx12_shader_compiler: Default::default(),
        });
        
        // Request adapter
        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| BackendError::InitFailed("No adapter found".into()))?;
        
        // Request device
        let (device, queue) = adapter
            .request_device(
                &DeviceDescriptor {
                    label: Some("FLUI Device"),
                    features: Features::empty(),
                    limits: Limits::default(),
                },
                None,
            )
            .await
            .map_err(|e| BackendError::InitFailed(e.to_string()))?;
        
        // Create render pipeline
        let pipeline = Self::create_pipeline(&device);
        
        // Create buffers
        let vertex_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("Vertex Buffer"),
            size: 1024 * 1024, // 1MB
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        
        let index_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("Index Buffer"),
            size: 1024 * 1024, // 1MB
            usage: BufferUsages::INDEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        
        Ok(Self {
            instance,
            surface: None,
            device,
            queue,
            surface_config: None,
            pipeline,
            atlas: TextureAtlas::new(2048, 2048),
            vertex_buffer,
            index_buffer,
            staging_belt: StagingBelt::new(1024),
        })
    }
    
    fn create_pipeline(device: &Device) -> RenderPipeline {
        // Shader
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("FLUI Shader"),
            source: ShaderSource::Wgsl(include_str!("shaders/flui.wgsl").into()),
        });
        
        // Pipeline layout
        let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("FLUI Pipeline Layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });
        
        // Render pipeline
        device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("FLUI Pipeline"),
            layout: Some(&layout),
            vertex: VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[Vertex::descriptor()],
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(ColorTargetState {
                    format: TextureFormat::Bgra8UnormSrgb,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
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
        })
    }
}

impl RenderBackend for WgpuBackend {
    fn init(&mut self, window: &Window) -> Result<(), BackendError> {
        // Create surface
        let surface = unsafe {
            self.instance
                .create_surface(&window.raw_handle())
                .map_err(|e| BackendError::InitFailed(e.to_string()))?
        };
        
        // Configure surface
        let size = window.inner_size();
        let config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: TextureFormat::Bgra8UnormSrgb,
            width: size.width,
            height: size.height,
            present_mode: PresentMode::Fifo, // VSync
            alpha_mode: CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        
        surface.configure(&self.device, &config);
        
        self.surface = Some(surface);
        self.surface_config = Some(config);
        
        Ok(())
    }
    
    fn begin_frame(&mut self, window_size: Size) {
        // Update surface config if size changed
        if let Some(config) = &mut self.surface_config {
            if config.width as f32 != window_size.width
                || config.height as f32 != window_size.height
            {
                config.width = window_size.width as u32;
                config.height = window_size.height as u32;
                
                if let Some(surface) = &self.surface {
                    surface.configure(&self.device, config);
                }
            }
        }
    }
    
    fn end_frame(&mut self) {
        // Submit pending work
        self.staging_belt.finish();
        
        // Present frame
        // (actual rendering happens in draw_texture calls)
    }
    
    fn rasterize_picture(
        &mut self,
        picture: &Picture,
        transform: Mat4,
    ) -> Arc<Texture> {
        // Convert picture to vertices
        let vertices = self.tesselate_picture(picture, transform);
        
        // Create texture for rendering
        let size = picture.bounds().size();
        let texture = self.device.create_texture(&TextureDescriptor {
            label: Some("Rasterized Picture"),
            size: Extent3d {
                width: size.width as u32,
                height: size.height as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        
        // Render to texture
        let view = texture.create_view(&TextureViewDescriptor::default());
        
        let mut encoder = self.device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("Rasterize Encoder"),
        });
        
        {
            let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("Rasterize Pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: true,
                    },
                })],
                depth_stencil_attachment: None,
            });
            
            render_pass.set_pipeline(&self.pipeline);
            // ... set buffers and draw
        }
        
        self.queue.submit(Some(encoder.finish()));
        
        Arc::new(Texture {
            handle: TextureHandle::Wgpu(texture),
            size,
            format: TextureFormat::Rgba8,
        })
    }
    
    fn draw_texture(&mut self, texture: Arc<Texture>, transform: Mat4) {
        // Add texture to render queue
        // Actual rendering happens in present()
        todo!()
    }
    
    fn clear(&mut self, color: Color) {
        // Queue clear operation
    }
    
    fn name(&self) -> &'static str {
        "wgpu"
    }
    
    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            gpu_accelerated: true,
            max_texture_size: 16384,
            compute_shaders: true,
            msaa: true,
            max_msaa_samples: 4,
        }
    }
    
    fn resize(&mut self, new_size: Size) {
        if let Some(config) = &mut self.surface_config {
            config.width = new_size.width as u32;
            config.height = new_size.height as u32;
            
            if let Some(surface) = &self.surface {
                surface.configure(&self.device, config);
            }
        }
    }
    
    fn screenshot(&mut self) -> Result<Image, BackendError> {
        todo!()
    }
}

/// Vertex for wgpu rendering
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    tex_coords: [f32; 2],
    color: [f32; 4],
}

impl Vertex {
    fn descriptor<'a>() -> VertexBufferLayout<'a> {
        VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as BufferAddress,
            step_mode: VertexStepMode::Vertex,
            attributes: &[
                VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: VertexFormat::Float32x2,
                },
                VertexAttribute {
                    offset: std::mem::size_of::<[f32; 2]>() as BufferAddress,
                    shader_location: 1,
                    format: VertexFormat::Float32x2,
                },
                VertexAttribute {
                    offset: std::mem::size_of::<[f32; 4]>() as BufferAddress,
                    shader_location: 2,
                    format: VertexFormat::Float32x4,
                },
            ],
        }
    }
}
```

### WGSL Shader

```wgsl
// flui.wgsl

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) tex_coords: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = vec4<f32>(input.position, 0.0, 1.0);
    output.tex_coords = input.tex_coords;
    output.color = input.color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // For now, just return vertex color
    // Later: sample texture
    return input.color;
}
```

---

## 💻 Software Backend (CPU Fallback)

### Implementation

```rust
use tiny_skia::*;

/// Software rasterizer backend - CPU rendering
pub struct SoftwareBackend {
    /// CPU framebuffer
    pixmap: Option<Pixmap>,
    
    /// Window size
    window_size: Size,
}

impl SoftwareBackend {
    pub fn new() -> Self {
        Self {
            pixmap: None,
            window_size: Size::ZERO,
        }
    }
}

impl RenderBackend for SoftwareBackend {
    fn init(&mut self, window: &Window) -> Result<(), BackendError> {
        let size = window.inner_size();
        self.window_size = Size::new(size.width as f32, size.height as f32);
        
        self.pixmap = Some(
            Pixmap::new(size.width, size.height)
                .ok_or_else(|| BackendError::InitFailed("Failed to create pixmap".into()))?
        );
        
        Ok(())
    }
    
    fn begin_frame(&mut self, window_size: Size) {
        if self.window_size != window_size {
            self.window_size = window_size;
            self.pixmap = Pixmap::new(
                window_size.width as u32,
                window_size.height as u32,
            );
        }
        
        // Clear framebuffer
        if let Some(pixmap) = &mut self.pixmap {
            pixmap.fill(tiny_skia::Color::TRANSPARENT);
        }
    }
    
    fn end_frame(&mut self) {
        // Copy pixmap to window
        // Platform-specific implementation
    }
    
    fn rasterize_picture(
        &mut self,
        picture: &Picture,
        transform: Mat4,
    ) -> Arc<Texture> {
        let bounds = picture.bounds();
        let size = bounds.size();
        
        // Create pixmap for picture
        let mut pixmap = Pixmap::new(size.width as u32, size.height as u32)
            .expect("Failed to create pixmap");
        
        // Rasterize each draw command
        for command in &picture.commands {
            self.rasterize_command(command, &mut pixmap);
        }
        
        // Convert pixmap to texture
        Arc::new(Texture {
            handle: TextureHandle::Software(Arc::new(pixmap.into())),
            size,
            format: TextureFormat::Rgba8,
        })
    }
    
    fn draw_texture(&mut self, texture: Arc<Texture>, transform: Mat4) {
        if let Some(pixmap) = &mut self.pixmap {
            // Composite texture onto framebuffer
            match &texture.handle {
                TextureHandle::Software(img) => {
                    // Blit with transform
                    self.blit_image(img, transform, pixmap);
                }
                _ => panic!("Invalid texture handle for software backend"),
            }
        }
    }
    
    fn clear(&mut self, color: Color) {
        if let Some(pixmap) = &mut self.pixmap {
            pixmap.fill(tiny_skia::Color::from_rgba8(
                color.r,
                color.g,
                color.b,
                color.a,
            ));
        }
    }
    
    fn name(&self) -> &'static str {
        "software"
    }
    
    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            gpu_accelerated: false,
            max_texture_size: 4096,
            compute_shaders: false,
            msaa: false,
            max_msaa_samples: 1,
        }
    }
    
    fn resize(&mut self, new_size: Size) {
        self.window_size = new_size;
        self.pixmap = Pixmap::new(
            new_size.width as u32,
            new_size.height as u32,
        );
    }
    
    fn screenshot(&mut self) -> Result<Image, BackendError> {
        if let Some(pixmap) = &self.pixmap {
            Ok(Image::from_pixmap(pixmap.clone()))
        } else {
            Err(BackendError::Unsupported("No pixmap available".into()))
        }
    }
}

impl SoftwareBackend {
    fn rasterize_command(&self, command: &DrawCommand, pixmap: &mut Pixmap) {
        match command {
            DrawCommand::DrawRect { rect, paint } => {
                let tiny_rect = tiny_skia::Rect::from_xywh(
                    rect.left,
                    rect.top,
                    rect.width(),
                    rect.height(),
                ).unwrap();
                
                let tiny_paint = self.convert_paint(paint);
                
                pixmap.fill_rect(
                    tiny_rect,
                    &tiny_paint,
                    Transform::identity(),
                    None,
                );
            }
            
            DrawCommand::DrawPath { path, paint } => {
                let tiny_path = self.convert_path(path);
                let tiny_paint = self.convert_paint(paint);
                
                pixmap.fill_path(
                    &tiny_path,
                    &tiny_paint,
                    FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
            
            // ... other commands
            _ => {}
        }
    }
    
    fn convert_paint(&self, paint: &Paint) -> tiny_skia::Paint {
        let mut tiny_paint = tiny_skia::Paint::default();
        tiny_paint.set_color(tiny_skia::Color::from_rgba8(
            paint.color.r,
            paint.color.g,
            paint.color.b,
            paint.color.a,
        ));
        tiny_paint.anti_alias = paint.anti_alias;
        tiny_paint
    }
    
    fn convert_path(&self, path: &Path) -> tiny_skia::Path {
        // Convert FLUI Path to tiny_skia Path
        todo!()
    }
    
    fn blit_image(
        &self,
        image: &ImageBuffer<Rgba<u8>>,
        transform: Mat4,
        dst: &mut Pixmap,
    ) {
        // Blit image to destination with transform
        todo!()
    }
}
```

---

## 🛠️ Backend Selection

### Automatic Selection

```rust
/// BackendSelector - chooses best backend for platform
pub struct BackendSelector;

impl BackendSelector {
    /// Select best available backend
    pub async fn select() -> Box<dyn RenderBackend> {
        // Try wgpu first (GPU-accelerated)
        if let Ok(backend) = WgpuBackend::new().await {
            println!("Using wgpu backend (GPU-accelerated)");
            return Box::new(backend);
        }
        
        // Fall back to software rasterizer
        println!("Falling back to software backend (CPU)");
        Box::new(SoftwareBackend::new())
    }
    
    /// Select backend by name
    pub async fn select_by_name(name: &str) -> Result<Box<dyn RenderBackend>, BackendError> {
        match name {
            "wgpu" => {
                Ok(Box::new(WgpuBackend::new().await?))
            }
            "software" | "soft" => {
                Ok(Box::new(SoftwareBackend::new()))
            }
            "egui" => {
                Ok(Box::new(EguiBackend::new()))
            }
            _ => Err(BackendError::Unsupported(format!("Unknown backend: {}", name))),
        }
    }
}

// Usage:
let backend = BackendSelector::select().await;
let compositor = Compositor::new(backend);
```

### Feature Flags

```toml
# Cargo.toml

[features]
default = ["wgpu-backend"]

# GPU backend (primary)
wgpu-backend = ["wgpu"]

# Software backend (fallback)
software-backend = ["tiny-skia"]

# Egui backend (dev tools)
egui-backend = ["egui", "egui-wgpu"]

# All backends
all-backends = ["wgpu-backend", "software-backend", "egui-backend"]
```

---

## 🎯 Performance Optimizations

### 1. Texture Atlas

```rust
/// TextureAtlas - packs multiple textures into one for batching
pub struct TextureAtlas {
    width: u32,
    height: u32,
    allocator: GuillotineAllocator,
    texture: Option<wgpu::Texture>,
}

impl TextureAtlas {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            allocator: GuillotineAllocator::new(width, height),
            texture: None,
        }
    }
    
    /// Allocate space in atlas
    pub fn allocate(&mut self, size: Size) -> Option<AtlasRegion> {
        self.allocator.allocate(size.width as u32, size.height as u32)
            .map(|rect| AtlasRegion {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
            })
    }
    
    /// Upload texture data to region
    pub fn upload(&mut self, region: AtlasRegion, data: &[u8], queue: &wgpu::Queue) {
        if let Some(texture) = &self.texture {
            queue.write_texture(
                ImageCopyTexture {
                    texture,
                    mip_level: 0,
                    origin: Origin3d {
                        x: region.x,
                        y: region.y,
                        z: 0,
                    },
                    aspect: TextureAspect::All,
                },
                data,
                ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(region.width * 4),
                    rows_per_image: Some(region.height),
                },
                Extent3d {
                    width: region.width,
                    height: region.height,
                    depth_or_array_layers: 1,
                },
            );
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AtlasRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
```

### 2. Instanced Rendering

```rust
/// Batch similar draw calls for instanced rendering
pub struct DrawBatch {
    texture: Arc<Texture>,
    instances: Vec<Instance>,
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Instance {
    transform: [[f32; 4]; 4],  // Mat4
    color: [f32; 4],
}

impl WgpuBackend {
    fn batch_draws(&mut self, draws: Vec<Draw>) -> Vec<DrawBatch> {
        let mut batches: HashMap<TextureId, DrawBatch> = HashMap::new();
        
        for draw in draws {
            batches.entry(draw.texture.id())
                .or_insert_with(|| DrawBatch {
                    texture: draw.texture.clone(),
                    instances: Vec::new(),
                })
                .instances.push(Instance {
                    transform: draw.transform.to_array(),
                    color: draw.color.to_array(),
                });
        }
        
        batches.into_values().collect()
    }
}
```

### 3. Command Buffer Pooling

```rust
/// Pool of reusable command buffers
pub struct CommandBufferPool {
    available: Vec<CommandBuffer>,
    in_use: Vec<CommandBuffer>,
}

impl CommandBufferPool {
    pub fn acquire(&mut self, device: &Device) -> CommandEncoder {
        if let Some(buffer) = self.available.pop() {
            buffer.reset()
        } else {
            device.create_command_encoder(&Default::default())
        }
    }
    
    pub fn release(&mut self, buffer: CommandBuffer) {
        self.available.push(buffer);
    }
}
```

---

## 🔗 Cross-References

- **Previous:** [Chapter 5: Layers & Compositing](05_layers_and_painters.md)
- **Next:** [Chapter 7: Input & Events](07_input_and_events.md)
- **Related:** [Appendix C: Performance Guide](appendix_c_performance.md)

---

**Key Takeaway:** FLUI's backend abstraction enables multiple rendering strategies (GPU-accelerated wgpu, CPU fallback) while maintaining a unified API. The wgpu backend provides industry-leading performance with modern GPU features!
