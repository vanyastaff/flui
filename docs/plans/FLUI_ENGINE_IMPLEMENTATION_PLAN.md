# flui_engine Implementation Plan

**Crate:** `flui_engine`  
**Purpose:** Cross-platform GPU rendering engine (Metal, DirectX 12, Vulkan, WebGPU)  
**Priority:** ⭐⭐⭐⭐⭐ CRITICAL (Core rendering infrastructure)

---

## Overview

`flui_engine` is responsible for:
- GPU abstraction via **wgpu** (supports Metal, D3D12, Vulkan, WebGPU, WebGL)
- Rendering pipeline management
- Shader compilation and caching
- Texture and buffer management
- GPU-accelerated effects (blur, shadows, gradients)
- HDR rendering support
- Compute shader integration

**Architecture:**
```
flui_engine/
├── src/
│   ├── lib.rs              # Public API
│   ├── renderer.rs         # Main renderer
│   ├── backend/
│   │   ├── metal.rs        # Metal-specific (macOS/iOS)
│   │   ├── dx12.rs         # DirectX 12 (Windows)
│   │   ├── vulkan.rs       # Vulkan (Linux/Android)
│   │   └── webgpu.rs       # WebGPU (Web)
│   ├── pipeline.rs         # Render pipeline management
│   ├── shader.rs           # Shader loading/compilation
│   ├── texture.rs          # Texture management
│   ├── buffer.rs           # Buffer management
│   ├── compute.rs          # Compute shaders
│   └── effects/
│       ├── blur.rs         # Gaussian blur
│       ├── shadow.rs       # Drop shadows
│       └── gradient.rs     # Gradients
```

---

## Platform-Specific Backend Matrix

| Platform | Backend | API Version | Priority | Status |
|----------|---------|-------------|----------|--------|
| **macOS** | Metal 4 | Metal 3.2+ | ⭐⭐⭐⭐⭐ | Q1 2026 |
| **iOS** | Metal 4 | Metal 3.2+ | ⭐⭐⭐⭐⭐ | Q2 2026 |
| **Windows** | DirectX 12 | Agility SDK 1.614+ | ⭐⭐⭐⭐⭐ | Q1 2026 |
| **Linux** | Vulkan 1.4 | Mesa 25.x | ⭐⭐⭐⭐⭐ | Q1 2026 |
| **Android** | Vulkan 1.3 | NDK r26 | ⭐⭐⭐⭐⭐ | Q2 2026 |
| **Web** | WebGPU | WGSL | ⭐⭐⭐⭐⭐ | Q2 2026 |
| **Web Fallback** | WebGL 2 | GLSL ES 3.0 | ⭐⭐⭐ | Q3 2026 |

---

## Q1 2026: Core Rendering Infrastructure (Weeks 1-12)

### 1. wgpu Integration & Backend Abstraction ⭐⭐⭐⭐⭐
- **Effort:** 4 weeks
- **Priority:** CRITICAL (Foundation for all rendering)

**Tasks:**

#### 1.1 Base Renderer Setup
```rust
// src/renderer.rs
use wgpu;

pub struct Renderer {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: Option<wgpu::Surface>,
    config: wgpu::SurfaceConfiguration,
}

impl Renderer {
    pub async fn new(window: &impl raw_window_handle::HasRawWindowHandle) -> Result<Self> {
        // Create instance with appropriate backend
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: Self::select_backend(),
            ..Default::default()
        });

        // Request adapter
        let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: surface.as_ref(),
            force_fallback_adapter: false,
        }).await.ok_or_else(|| anyhow::anyhow!("No suitable GPU adapter"))?;

        // Request device
        let (device, queue) = adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("FLUI GPU Device"),
                required_features: Self::required_features(),
                required_limits: Self::required_limits(),
            },
            None
        ).await?;

        Ok(Self { instance, adapter, device, queue, surface, config })
    }

    fn select_backend() -> wgpu::Backends {
        #[cfg(target_os = "macos")]
        return wgpu::Backends::METAL;

        #[cfg(target_os = "windows")]
        return wgpu::Backends::DX12;

        #[cfg(target_os = "linux")]
        return wgpu::Backends::VULKAN;

        #[cfg(target_os = "android")]
        return wgpu::Backends::VULKAN;

        #[cfg(target_arch = "wasm32")]
        return wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL;

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "android",
            target_arch = "wasm32"
        )))]
        return wgpu::Backends::all();
    }

    fn required_features() -> wgpu::Features {
        wgpu::Features::empty()
            | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::PUSH_CONSTANTS
    }

    fn required_limits() -> wgpu::Limits {
        wgpu::Limits::default()
    }

    pub fn render(&mut self, display_list: &DisplayList) -> Result<()> {
        let surface_texture = self.surface
            .as_ref()
            .unwrap()
            .get_current_texture()?;

        let view = surface_texture.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("FLUI Render Encoder"),
        });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("FLUI Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });

            // Render display list
            self.render_display_list(&mut render_pass, display_list)?;
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        surface_texture.present();

        Ok(())
    }

    fn render_display_list(
        &self,
        render_pass: &mut wgpu::RenderPass,
        display_list: &DisplayList
    ) -> Result<()> {
        for command in &display_list.commands {
            match command {
                DrawCommand::Rectangle { rect, color } => {
                    // Render rectangle
                }
                DrawCommand::Text { text, position, font } => {
                    // Render text
                }
                DrawCommand::Image { image, rect } => {
                    // Render image
                }
            }
        }
        Ok(())
    }
}
```

#### 1.2 Backend Feature Detection
```rust
// src/backend/capabilities.rs

pub struct GpuCapabilities {
    pub backend: wgpu::Backend,
    pub supports_hdr: bool,
    pub supports_compute: bool,
    pub max_texture_size: u32,
    pub supports_bc_compression: bool,
    pub supports_astc_compression: bool,
}

impl GpuCapabilities {
    pub fn detect(adapter: &wgpu::Adapter) -> Self {
        let info = adapter.get_info();
        let features = adapter.features();
        let limits = adapter.limits();

        Self {
            backend: info.backend,
            supports_hdr: Self::check_hdr_support(adapter),
            supports_compute: features.contains(wgpu::Features::COMPUTE_SHADER),
            max_texture_size: limits.max_texture_dimension_2d,
            supports_bc_compression: features.contains(wgpu::Features::TEXTURE_COMPRESSION_BC),
            supports_astc_compression: features.contains(wgpu::Features::TEXTURE_COMPRESSION_ASTC),
        }
    }

    fn check_hdr_support(adapter: &wgpu::Adapter) -> bool {
        #[cfg(target_os = "windows")]
        {
            // Check for DirectX 12 HDR support
            true
        }

        #[cfg(target_os = "macos")]
        {
            // Check for EDR (Extended Dynamic Range) on macOS
            true
        }

        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            false
        }
    }
}
```

**Deliverables:**
- ✅ wgpu-based renderer with automatic backend selection
- ✅ GPU capability detection
- ✅ Basic render loop

---

### 2. Metal 4 Backend (macOS/iOS) ⭐⭐⭐⭐⭐
- **Effort:** 3 weeks
- **Priority:** CRITICAL

**Tasks:**

#### 2.1 Metal 4 Features Integration
```rust
// src/backend/metal.rs

#[cfg(target_os = "macos")]
pub struct MetalBackend {
    device: wgpu::Device,
    features: MetalFeatures,
}

pub struct MetalFeatures {
    pub metalfx_upscaling: bool,
    pub metalfx_temporal_aa: bool,
    pub ray_tracing: bool,
    pub mesh_shaders: bool,
}

impl MetalBackend {
    pub fn detect_metal_version() -> (u32, u32) {
        // Query Metal version
        // Returns (major, minor), e.g., (3, 2) for Metal 3.2
        (3, 2)
    }

    pub fn supports_metalfx(&self) -> bool {
        let (major, minor) = Self::detect_metal_version();
        major >= 3 && minor >= 2  // MetalFX requires Metal 3.2+
    }

    /// Apply MetalFX temporal upscaling
    pub fn apply_metalfx_upscaling(
        &self,
        input: &wgpu::Texture,
        output: &wgpu::Texture,
        motion_vectors: &wgpu::Texture,
    ) -> Result<()> {
        if !self.supports_metalfx() {
            return Err(anyhow::anyhow!("MetalFX not available"));
        }

        // Use MetalFX temporal upscaling
        // This is a wgpu abstraction - actual Metal code would use MTLFXTemporalScaler
        todo!("Implement MetalFX via wgpu extension or metal-rs")
    }
}
```

#### 2.2 HDR Support (macOS)
```rust
// src/backend/metal/hdr.rs

#[cfg(target_os = "macos")]
pub struct HdrSupport {
    pub max_edr_headroom: f32,  // Extended Dynamic Range headroom
}

impl HdrSupport {
    pub fn detect() -> Self {
        // Query NSScreen for EDR headroom
        // macOS uses EDR (Extended Dynamic Range) instead of HDR10
        Self {
            max_edr_headroom: Self::query_edr_headroom(),
        }
    }

    fn query_edr_headroom() -> f32 {
        // Use NSScreen.maximumPotentialExtendedDynamicRangeColorComponentValue
        // Default SDR = 1.0, EDR can go up to 2.0+ on XDR displays
        1.0
    }

    pub fn configure_surface(
        &self,
        config: &mut wgpu::SurfaceConfiguration
    ) {
        // Set color space to extended linear sRGB for EDR
        config.format = wgpu::TextureFormat::Rgba16Float;
    }
}
```

**Deliverables:**
- ✅ Metal 4 backend with feature detection
- ✅ MetalFX integration (if available via wgpu)
- ✅ EDR (Extended Dynamic Range) support

---

### 3. DirectX 12 Backend (Windows) ⭐⭐⭐⭐⭐
- **Effort:** 3 weeks
- **Priority:** CRITICAL

**Tasks:**

#### 3.1 DirectX 12 Agility SDK Integration
```rust
// src/backend/dx12.rs

#[cfg(target_os = "windows")]
pub struct Dx12Backend {
    device: wgpu::Device,
    features: Dx12Features,
}

pub struct Dx12Features {
    pub work_graphs: bool,  // DX12 Work Graphs (2024)
    pub shader_execution_reordering: bool,  // SER (2025)
    pub cooperative_vectors: bool,  // ML acceleration (2025)
    pub auto_hdr: bool,
}

impl Dx12Backend {
    pub fn detect_agility_sdk_version() -> u32 {
        // Returns Agility SDK version, e.g., 614 for 1.614.0
        614
    }

    pub fn supports_work_graphs(&self) -> bool {
        // Work Graphs require Agility SDK 1.614+
        Self::detect_agility_sdk_version() >= 614
    }

    /// GPU-driven rendering with Work Graphs
    pub fn dispatch_work_graph(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        root_nodes: &[WorkNode],
    ) -> Result<()> {
        if !self.supports_work_graphs() {
            // Fallback to traditional dispatch
            return self.fallback_dispatch(encoder, root_nodes);
        }

        // Use Work Graphs for GPU-driven rendering
        // This eliminates CPU round-trips for spawning child tasks
        todo!("Implement Work Graphs via wgpu extension or windows-rs")
    }

    fn fallback_dispatch(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        root_nodes: &[WorkNode],
    ) -> Result<()> {
        // Traditional compute dispatch (CPU-driven)
        for node in root_nodes {
            let compute_pass = encoder.begin_compute_pass(&Default::default());
            // Dispatch compute shader
        }
        Ok(())
    }
}
```

#### 3.2 Auto HDR Support
```rust
// src/backend/dx12/hdr.rs

#[cfg(target_os = "windows")]
pub struct AutoHdrSupport {
    pub enabled: bool,
    pub max_luminance: f32,  // nits
}

impl AutoHdrSupport {
    pub fn detect() -> Self {
        // Check if Auto HDR is available (Windows 11 24H2+)
        Self {
            enabled: Self::is_auto_hdr_available(),
            max_luminance: Self::query_max_luminance(),
        }
    }

    fn is_auto_hdr_available() -> bool {
        // Check Windows version (24H2+)
        true
    }

    fn query_max_luminance() -> f32 {
        // Query DXGI output for max luminance
        // Typical HDR10 = 1000 nits, HDR1000 = 1000 nits
        1000.0
    }

    pub fn configure_swapchain(
        &self,
        config: &mut wgpu::SurfaceConfiguration
    ) {
        if self.enabled {
            // Use HDR10 color space (Rec.2020 + PQ)
            config.format = wgpu::TextureFormat::Rgba16Float;
            // Set HDR metadata via wgpu extension or DXGI directly
        }
    }
}
```

**Deliverables:**
- ✅ DirectX 12 Agility SDK integration
- ✅ Work Graphs support (if available via wgpu)
- ✅ Auto HDR configuration

---

### 4. Vulkan 1.4 Backend (Linux/Android) ⭐⭐⭐⭐⭐
- **Effort:** 3 weeks
- **Priority:** CRITICAL

**Tasks:**

#### 4.1 Vulkan 1.4 Features
```rust
// src/backend/vulkan.rs

#[cfg(any(target_os = "linux", target_os = "android"))]
pub struct VulkanBackend {
    device: wgpu::Device,
    features: VulkanFeatures,
}

pub struct VulkanFeatures {
    pub dynamic_rendering: bool,  // VK 1.3+
    pub synchronization2: bool,  // VK 1.3+
    pub pipeline_binary: bool,  // VK 1.4 (Mesa 25.3+)
}

impl VulkanBackend {
    pub fn detect_vulkan_version() -> (u32, u32, u32) {
        // Returns (major, minor, patch), e.g., (1, 4, 0)
        (1, 4, 0)
    }

    pub fn detect_driver() -> VulkanDriver {
        // Detect Mesa RADV, ANV, NVK, or proprietary NVIDIA
        VulkanDriver::RADV
    }

    /// Use VK_KHR_pipeline_binary for faster shader loading
    pub fn load_pipeline_binary(
        &self,
        binary: &[u8],
    ) -> Result<wgpu::RenderPipeline> {
        if !self.features.pipeline_binary {
            // Fallback to regular pipeline creation
            return self.create_pipeline_from_spirv(binary);
        }

        // Use VK_KHR_pipeline_binary (Mesa 25.3+)
        // Significantly faster load times
        todo!("Implement pipeline binary caching")
    }
}

pub enum VulkanDriver {
    RADV,         // AMD Mesa (Rust-based, open-source)
    ANV,          // Intel Mesa
    NVK,          // NVIDIA Mesa (open-source)
    Proprietary,  // NVIDIA proprietary
}
```

#### 4.2 Linux-Specific Optimizations
```rust
// src/backend/vulkan/linux.rs

#[cfg(target_os = "linux")]
pub struct LinuxVulkanOptimizations {
    pub use_explicit_sync: bool,  // For NVIDIA Wayland
}

impl LinuxVulkanOptimizations {
    pub fn detect(platform: &Platform) -> Self {
        Self {
            use_explicit_sync: Self::should_use_explicit_sync(platform),
        }
    }

    fn should_use_explicit_sync(platform: &Platform) -> bool {
        // Check if Wayland + NVIDIA
        platform.is_wayland() && platform.gpu_vendor() == GpuVendor::Nvidia
    }

    pub fn configure_swapchain(
        &self,
        config: &mut wgpu::SurfaceConfiguration
    ) {
        if self.use_explicit_sync {
            // Enable VK_KHR_external_fence + linux_drm_syncobj
            // This is critical for NVIDIA Wayland stability
        }
    }
}
```

**Deliverables:**
- ✅ Vulkan 1.4 backend with Mesa 25.x support
- ✅ Pipeline binary caching
- ✅ NVIDIA explicit sync (Linux Wayland)

---

## Q2 2026: Advanced Rendering Features (Weeks 13-24)

### 5. Shader System ⭐⭐⭐⭐⭐
- **Effort:** 4 weeks

**Tasks:**

#### 5.1 Shader Compilation Pipeline
```rust
// src/shader.rs

pub struct ShaderCompiler {
    cache: ShaderCache,
}

impl ShaderCompiler {
    pub fn compile_wgsl(&self, source: &str) -> Result<wgpu::ShaderModule> {
        // Compile WGSL to wgpu shader module
        // wgpu handles translation to Metal/DXIL/SPIR-V/WGSL
        let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("FLUI Shader"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });

        Ok(module)
    }

    pub fn load_from_cache(&self, shader_id: &str) -> Option<wgpu::ShaderModule> {
        self.cache.get(shader_id)
    }

    pub fn save_to_cache(&mut self, shader_id: &str, module: wgpu::ShaderModule) {
        self.cache.insert(shader_id, module);
    }
}

pub struct ShaderCache {
    // Platform-specific caching
    // macOS: ~/Library/Caches/com.flui.shaders/
    // Windows: %LOCALAPPDATA%\FLUI\ShaderCache\
    // Linux: ~/.cache/flui/shaders/
    cache_dir: PathBuf,
    modules: HashMap<String, wgpu::ShaderModule>,
}
```

#### 5.2 Standard Shaders
```wgsl
// shaders/rectangle.wgsl
struct VertexInput {
    @location(0) position: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.position = vec4<f32>(input.position, 0.0, 1.0);
    output.color = vec4<f32>(1.0, 0.0, 0.0, 1.0);  // Red
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}
```

**Deliverables:**
- ✅ WGSL shader compilation
- ✅ Shader caching system
- ✅ Standard shaders (rect, text, image, gradient)

---

### 6. GPU Effects ⭐⭐⭐⭐
- **Effort:** 3 weeks

**Tasks:**

#### 6.1 Gaussian Blur
```rust
// src/effects/blur.rs

pub struct GaussianBlur {
    pipeline: wgpu::ComputePipeline,
    kernel_size: u32,
}

impl GaussianBlur {
    pub fn apply(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        input: &wgpu::Texture,
        output: &wgpu::Texture,
        radius: f32,
    ) -> Result<()> {
        // Two-pass Gaussian blur (horizontal + vertical)
        let temp_texture = self.create_temp_texture(input.size());

        // Pass 1: Horizontal blur
        self.blur_pass(encoder, input, &temp_texture, BlurDirection::Horizontal, radius)?;

        // Pass 2: Vertical blur
        self.blur_pass(encoder, &temp_texture, output, BlurDirection::Vertical, radius)?;

        Ok(())
    }

    fn blur_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        input: &wgpu::Texture,
        output: &wgpu::Texture,
        direction: BlurDirection,
        radius: f32,
    ) -> Result<()> {
        // Dispatch compute shader for blur
        todo!()
    }
}

enum BlurDirection {
    Horizontal,
    Vertical,
}
```

#### 6.2 Drop Shadows
```rust
// src/effects/shadow.rs

pub struct DropShadow {
    blur: GaussianBlur,
}

impl DropShadow {
    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        content: &wgpu::Texture,
        offset: (f32, f32),
        blur_radius: f32,
        color: [f32; 4],
    ) -> Result<wgpu::Texture> {
        // 1. Render content to offscreen texture
        // 2. Apply blur
        // 3. Offset shadow
        // 4. Composite shadow + content
        todo!()
    }
}
```

**Deliverables:**
- ✅ Gaussian blur (compute shader)
- ✅ Drop shadows
- ✅ Linear/radial gradients

---

### 7. Texture Atlas & Management ⭐⭐⭐⭐
- **Effort:** 2 weeks

**Tasks:**

#### 7.1 Texture Atlas
```rust
// src/texture/atlas.rs

pub struct TextureAtlas {
    texture: wgpu::Texture,
    allocator: AtlasAllocator,
    size: (u32, u32),
}

impl TextureAtlas {
    pub fn new(device: &wgpu::Device, size: (u32, u32)) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Texture Atlas"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        Self {
            texture,
            allocator: AtlasAllocator::new(size),
            size,
        }
    }

    pub fn allocate(&mut self, width: u32, height: u32) -> Option<AtlasRect> {
        self.allocator.allocate(width, height)
    }

    pub fn upload(
        &self,
        queue: &wgpu::Queue,
        rect: AtlasRect,
        data: &[u8],
    ) {
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: rect.x,
                    y: rect.y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(rect.width * 4),
                rows_per_image: Some(rect.height),
            },
            wgpu::Extent3d {
                width: rect.width,
                height: rect.height,
                depth_or_array_layers: 1,
            },
        );
    }
}

pub struct AtlasRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
```

**Deliverables:**
- ✅ Texture atlas for glyphs/icons
- ✅ Texture compression (BC/ASTC)
- ✅ Mipmap generation

---

## Q3 2026: Platform-Specific Optimizations (Weeks 25-36)

### 8. Performance Optimization ⭐⭐⭐⭐
- **Effort:** 4 weeks

**Tasks:**

#### 8.1 GPU Profiling
```rust
// src/profiling.rs

pub struct GpuProfiler {
    query_sets: Vec<wgpu::QuerySet>,
}

impl GpuProfiler {
    pub fn begin_pass(&mut self, label: &str) {
        // Create timestamp query
    }

    pub fn end_pass(&mut self) -> Duration {
        // Resolve timestamp query, return duration
        Duration::from_millis(0)
    }

    pub fn report(&self) {
        tracing::info!("GPU Frame Profile:");
        tracing::info!("  Layout pass: {:?}", self.timings.layout);
        tracing::info!("  Paint pass: {:?}", self.timings.paint);
        tracing::info!("  Compute: {:?}", self.timings.compute);
    }
}
```

#### 8.2 Render Batching
```rust
// src/batching.rs

pub struct RenderBatcher {
    batches: Vec<DrawBatch>,
}

pub struct DrawBatch {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    instance_count: u32,
}

impl RenderBatcher {
    pub fn batch_rectangles(&mut self, rectangles: &[Rectangle]) {
        // Group rectangles by material/shader
        // Create instanced draw calls
    }

    pub fn flush(&self, render_pass: &mut wgpu::RenderPass) {
        for batch in &self.batches {
            render_pass.set_pipeline(&batch.pipeline);
            render_pass.set_vertex_buffer(0, batch.vertex_buffer.slice(..));
            render_pass.draw(0..6, 0..batch.instance_count);  // 6 vertices (2 triangles)
        }
    }
}
```

**Deliverables:**
- ✅ GPU profiling with timestamp queries
- ✅ Render batching/instancing
- ✅ Frustum culling
- ✅ Occlusion culling (future)

---

## Q4 2026: Advanced Features (Weeks 37-48)

### 9. Compute Shader Integration ⭐⭐⭐⭐
- **Effort:** 3 weeks

**Tasks:**

#### 9.1 Layout Computation on GPU
```rust
// src/compute/layout.rs

pub struct GpuLayoutEngine {
    pipeline: wgpu::ComputePipeline,
}

impl GpuLayoutEngine {
    pub fn compute_layout(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        constraints: &[Constraints],
    ) -> wgpu::Buffer {
        // Dispatch compute shader for parallel layout
        // Returns buffer with computed sizes/positions
        todo!()
    }
}
```

```wgsl
// shaders/layout.wgsl
@group(0) @binding(0) var<storage, read> constraints: array<Constraints>;
@group(0) @binding(1) var<storage, read_write> results: array<LayoutResult>;

@compute @workgroup_size(64)
fn compute_layout(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let idx = global_id.x;
    let constraint = constraints[idx];
    
    // Compute layout for this node
    results[idx] = layout_node(constraint);
}
```

**Deliverables:**
- ✅ GPU-accelerated layout (experimental)
- ✅ Parallel text shaping
- ✅ Image decoding on GPU

---

### 10. WebGPU Backend (Web) ⭐⭐⭐⭐⭐
- **Effort:** 3 weeks

**Tasks:**

#### 10.1 WebGPU Implementation
```rust
// src/backend/webgpu.rs

#[cfg(target_arch = "wasm32")]
pub struct WebGpuBackend {
    device: wgpu::Device,
}

impl WebGpuBackend {
    pub async fn new(canvas: web_sys::HtmlCanvasElement) -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU,
            ..Default::default()
        });

        let surface = instance.create_surface_from_canvas(&canvas)?;

        // Same API as desktop!
        let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }).await.unwrap();

        let (device, queue) = adapter.request_device(
            &wgpu::DeviceDescriptor {
                required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                ..Default::default()
            },
            None
        ).await?;

        Ok(Self { device })
    }
}
```

**Key Advantage:** Same rendering code works on desktop and web!

**Deliverables:**
- ✅ WebGPU backend (Chrome, Firefox, Safari)
- ✅ WebGL 2 fallback
- ✅ WASM binary optimization (< 500KB)

---

## Testing Strategy

### Unit Tests
```rust
// tests/renderer_tests.rs

#[test]
fn test_backend_selection() {
    let backend = Renderer::select_backend();
    
    #[cfg(target_os = "macos")]
    assert_eq!(backend, wgpu::Backends::METAL);
    
    #[cfg(target_os = "windows")]
    assert_eq!(backend, wgpu::Backends::DX12);
}

#[tokio::test]
async fn test_renderer_creation() {
    let renderer = Renderer::new_offscreen().await.unwrap();
    assert!(renderer.device.limits().max_texture_dimension_2d >= 4096);
}
```

### Integration Tests
- Render 1000 rectangles, measure FPS
- GPU memory usage profiling
- Shader compilation time benchmarks
- Texture atlas allocation stress test

### Platform-Specific Tests
- **macOS:** Metal validation layer enabled
- **Windows:** PIX GPU capture integration
- **Linux:** RenderDoc integration
- **Web:** WebGPU conformance tests

---

## Dependencies

```toml
[dependencies]
wgpu = "25.0"  # Stay on 25.x (26.0+ has issues)
bytemuck = { version = "1.13", features = ["derive"] }
parking_lot = "0.12"
tracing = "0.1"
anyhow = "1.0"

# Platform-specific
[target.'cfg(target_os = "macos")'.dependencies]
metal = "0.29"  # For Metal-specific features

[target.'cfg(target_os = "windows")'.dependencies]
windows = { version = "0.58", features = ["Win32_Graphics_Direct3D12"] }

[target.'cfg(target_arch = "wasm32")'.dependencies]
wasm-bindgen = "0.2"
web-sys = { version = "0.3", features = ["HtmlCanvasElement", "Gpu"] }

[dev-dependencies]
pollster = "0.3"  # Block on async in tests
image = "0.25"  # For texture loading tests
```

---

## Milestones & Timeline

| Quarter | Milestone | Deliverables |
|---------|-----------|--------------|
| **Q1 2026** | Core Infrastructure | wgpu integration, Metal 4, DX12, Vulkan 1.4 backends |
| **Q2 2026** | Advanced Rendering | Shaders, effects (blur, shadow), texture atlas |
| **Q3 2026** | Optimization | GPU profiling, batching, culling |
| **Q4 2026** | Compute & Web | GPU layout, WebGPU backend, WASM optimization |

---

## Success Metrics

- ✅ 60 FPS @ 4K resolution with 1000+ widgets
- ✅ < 16ms frame time (60 FPS)
- ✅ GPU memory usage < 100MB for typical app
- ✅ Shader compilation < 100ms
- ✅ Supports HDR on macOS (EDR) and Windows (Auto HDR)
- ✅ WebGPU bundle < 500KB (WASM)
- ✅ Zero validation errors (Metal Validation, D3D12 Debug Layer)

---

**Next Steps:**
1. Review and approve this plan
2. Set up wgpu integration
3. Implement Metal 4 backend (macOS priority)
4. Begin DX12 and Vulkan backends in parallel
5. Create shader compilation pipeline
