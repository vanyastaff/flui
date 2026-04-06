//! Render surface management (window surfaces and offscreen targets).
//!
//! [`RenderSurface`] is a per-window rendering surface that owns a wgpu
//! `Surface` and provides access to the current frame texture.

use std::sync::Arc;

use wgpu::util::DeviceExt;

use crate::context::gpu_device::GpuDevice;
use crate::error::{RenderError, RenderResult};
use crate::frame::encoder::FrameEncoder;
use crate::vertex::FrameUniforms;

/// Per-window rendering surface. One per window.
///
/// Owns the wgpu [`Surface`](wgpu::Surface) and its configuration, and
/// provides methods to resize and acquire frame textures for rendering.
pub struct RenderSurface {
    gpu: Arc<GpuDevice>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    width: u32,
    height: u32,
    scale_factor: f32,
    viewport_buffer: wgpu::Buffer,
    viewport_bind_group: wgpu::BindGroup,
}

impl RenderSurface {
    /// Create a surface for a window.
    ///
    /// # Safety
    ///
    /// The window handle must remain valid for the lifetime of the returned
    /// `RenderSurface`. Dropping or invalidating the window while the surface
    /// is still alive is undefined behavior.
    #[allow(unsafe_code)]
    pub unsafe fn new(
        gpu: Arc<GpuDevice>,
        instance: &wgpu::Instance,
        window: &(impl raw_window_handle::HasWindowHandle + raw_window_handle::HasDisplayHandle),
        width: u32,
        height: u32,
        scale_factor: f32,
    ) -> RenderResult<Self> {
        // SAFETY: caller guarantees window handle validity for the surface lifetime.
        let surface = instance
            .create_surface_unsafe(
                wgpu::SurfaceTargetUnsafe::from_window(window)
                    .map_err(|e| RenderError::SurfaceCreation(Box::new(e)))?,
            )
            .map_err(|e| RenderError::SurfaceCreation(Box::new(e)))?;

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: gpu.default_format(),
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(gpu.device(), &config);

        let clamped_width = width.max(1);
        let clamped_height = height.max(1);
        let (viewport_buffer, viewport_bind_group) =
            create_viewport_resources(&gpu, clamped_width, clamped_height, scale_factor);

        Ok(Self {
            gpu,
            surface,
            config,
            width: clamped_width,
            height: clamped_height,
            scale_factor,
            viewport_buffer,
            viewport_bind_group,
        })
    }

    /// Create a headless surface (renders to texture, no window).
    ///
    /// This is a placeholder that will be implemented alongside `FrameEncoder`
    /// to support off-screen rendering via textures.
    pub fn new_headless(_gpu: Arc<GpuDevice>, _width: u32, _height: u32) -> RenderResult<Self> {
        // For headless, we don't have a real surface. Actual headless rendering
        // will use offscreen textures created via FrameEncoder.
        // TODO: implement proper headless rendering via offscreen texture
        Err(RenderError::NotInitialized)
    }

    /// Resize the surface. Call when window resizes or DPI changes.
    pub fn resize(&mut self, width: u32, height: u32, scale_factor: f32) {
        self.width = width.max(1);
        self.height = height.max(1);
        self.scale_factor = scale_factor;
        self.config.width = self.width;
        self.config.height = self.height;
        self.surface.configure(self.gpu.device(), &self.config);

        let uniforms = FrameUniforms::new(self.width as f32, self.height as f32, self.scale_factor);
        self.gpu
            .queue()
            .write_buffer(&self.viewport_buffer, 0, bytemuck::bytes_of(&uniforms));
    }

    /// Get the current surface texture for rendering.
    ///
    /// Returns the next [`SurfaceTexture`](wgpu::SurfaceTexture) from the
    /// swap chain. The texture must be presented (dropped) after rendering
    /// to display the frame.
    pub fn get_current_texture(&self) -> RenderResult<wgpu::SurfaceTexture> {
        self.surface.get_current_texture().map_err(|e| match e {
            wgpu::SurfaceError::Lost => RenderError::SurfaceLost,
            wgpu::SurfaceError::Outdated => RenderError::SurfaceOutdated,
            wgpu::SurfaceError::Timeout => RenderError::Timeout,
            wgpu::SurfaceError::OutOfMemory => RenderError::OutOfMemory,
            _ => RenderError::SurfaceLost,
        })
    }

    /// Begin recording a new frame.
    ///
    /// Returns a [`FrameEncoder`] that can be used to record draw commands
    /// and then submitted via [`FrameEncoder::finish`].
    ///
    /// Returns `Err(SurfaceLost)` if the surface needs reconfiguration
    /// (call [`resize`](Self::resize) and retry).
    pub fn begin_frame(&self) -> RenderResult<FrameEncoder<'_>> {
        let surface_texture = self.get_current_texture()?;
        Ok(FrameEncoder::new(self, surface_texture))
    }

    /// The shared GPU device backing this surface.
    #[must_use]
    pub fn gpu(&self) -> &Arc<GpuDevice> {
        &self.gpu
    }

    /// Surface width in physical pixels.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Surface height in physical pixels.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Display scale factor (DPI scaling).
    #[must_use]
    pub fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    /// The texture format used by this surface.
    #[must_use]
    pub fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    /// The viewport uniform bind group (group 0) for all pipelines.
    #[must_use]
    pub fn viewport_bind_group(&self) -> &wgpu::BindGroup {
        &self.viewport_bind_group
    }

    /// The viewport uniform buffer containing [`FrameUniforms`].
    #[must_use]
    pub fn viewport_buffer(&self) -> &wgpu::Buffer {
        &self.viewport_buffer
    }
}

/// Create the viewport uniform buffer and its bind group.
fn create_viewport_resources(
    gpu: &GpuDevice,
    width: u32,
    height: u32,
    scale_factor: f32,
) -> (wgpu::Buffer, wgpu::BindGroup) {
    let uniforms = FrameUniforms::new(width as f32, height as f32, scale_factor);
    let buffer = gpu
        .device()
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("viewport_uniform"),
            contents: bytemuck::bytes_of(&uniforms),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
    let bind_group = gpu.device().create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("viewport_bind_group"),
        layout: gpu.bind_group_layout(),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    });
    (buffer, bind_group)
}

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod tests {
    // RenderSurface requires a window handle, tested in integration tests (Task 17).
}
