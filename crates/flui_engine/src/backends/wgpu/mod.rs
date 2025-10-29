//! WGPU painter implementation
//!
//! GPU-accelerated rendering with wgpu

pub mod event_translator;
mod pipeline;
mod tesselator;
pub mod text;
mod vertex;
pub mod window;

use pipeline::SolidPipeline;
use tesselator::Tesselator;
pub use text::{TextAlign, TextCommand, TextRenderError, TextRenderer};
pub use vertex::{SolidVertex, ViewportUniforms};

use crate::painter::{Paint, Painter, RRect};
use flui_types::{Offset, Point, Rect, Size};
use glam::Mat4;
use parking_lot::Mutex;
use std::sync::Arc;
use wgpu::util::DeviceExt;

/// Internal GPU draw command
#[derive(Debug, Clone)]
enum GpuDrawCommand {
    Rect {
        rect: Rect,
        color: [f32; 4],
        transform: Mat4,
    },
    RRect {
        rrect: RRect,
        color: [f32; 4],
        transform: Mat4,
    },
    Circle {
        center: Point,
        radius: f32,
        color: [f32; 4],
        transform: Mat4,
    },
    Line {
        p1: Point,
        p2: Point,
        width: f32,
        color: [f32; 4],
        transform: Mat4,
    },
    Text(TextCommand),
}

/// Painter state for save/restore
#[derive(Debug, Clone)]
struct PainterState {
    transform: Mat4,
    opacity: f32,
    clip_rect: Option<Rect>,
}

impl Default for PainterState {
    fn default() -> Self {
        Self {
            transform: Mat4::IDENTITY,
            opacity: 1.0,
            clip_rect: None,
        }
    }
}

/// WGPU-backed painter
pub struct WgpuPainter {
    renderer: Arc<Mutex<WgpuRenderer>>,
    commands: Vec<GpuDrawCommand>,
    state_stack: Vec<PainterState>,
    current_state: PainterState,
}

impl WgpuPainter {
    pub fn new(renderer: Arc<Mutex<WgpuRenderer>>) -> Self {
        Self {
            renderer,
            commands: Vec::with_capacity(1024),
            state_stack: Vec::new(),
            current_state: PainterState::default(),
        }
    }

    pub fn begin_frame(&mut self) {
        self.commands.clear();
        self.state_stack.clear();
        self.current_state = PainterState::default();
    }

    pub fn end_frame(&mut self) -> Result<(), RenderError> {
        if self.commands.is_empty() {
            return Ok(());
        }
        let mut renderer = self.renderer.lock();
        renderer.render_frame(&self.commands)?;
        self.commands.clear();
        Ok(())
    }

    fn apply_opacity(&self, color: [f32; 4]) -> [f32; 4] {
        let [r, g, b, a] = color;
        [r, g, b, a * self.current_state.opacity]
    }
}

impl Painter for WgpuPainter {
    fn rect(&mut self, rect: Rect, paint: &Paint) {
        self.commands.push(GpuDrawCommand::Rect {
            rect,
            color: self.apply_opacity(paint.color),
            transform: self.current_state.transform,
        });
    }
    fn rrect(&mut self, rrect: RRect, paint: &Paint) {
        self.commands.push(GpuDrawCommand::RRect {
            rrect,
            color: self.apply_opacity(paint.color),
            transform: self.current_state.transform,
        });
    }
    fn circle(&mut self, center: Point, radius: f32, paint: &Paint) {
        self.commands.push(GpuDrawCommand::Circle {
            center,
            radius,
            color: self.apply_opacity(paint.color),
            transform: self.current_state.transform,
        });
    }
    fn line(&mut self, p1: Point, p2: Point, paint: &Paint) {
        self.commands.push(GpuDrawCommand::Line {
            p1,
            p2,
            width: paint.stroke_width.max(1.0),
            color: self.apply_opacity(paint.color),
            transform: self.current_state.transform,
        });
    }
    fn text(&mut self, text: &str, position: Point, font_size: f32, paint: &Paint) {
        self.commands.push(GpuDrawCommand::Text(TextCommand {
            text: text.to_string(),
            position,
            font_size,
            color: self.apply_opacity(paint.color),
            max_width: None,
            align: TextAlign::Left,
            transform: self.current_state.transform,
        }));
    }
    fn save(&mut self) {
        self.state_stack.push(self.current_state.clone());
    }
    fn restore(&mut self) {
        if let Some(s) = self.state_stack.pop() {
            self.current_state = s;
        }
    }
    fn translate(&mut self, offset: Offset) {
        self.current_state.transform = self.current_state.transform
            * Mat4::from_translation(glam::Vec3::new(offset.dx, offset.dy, 0.0));
    }
    fn rotate(&mut self, angle: f32) {
        self.current_state.transform = self.current_state.transform * Mat4::from_rotation_z(angle);
    }
    fn scale(&mut self, sx: f32, sy: f32) {
        self.current_state.transform =
            self.current_state.transform * Mat4::from_scale(glam::Vec3::new(sx, sy, 1.0));
    }
    fn clip_rect(&mut self, rect: Rect) {
        self.current_state.clip_rect = Some(if let Some(c) = self.current_state.clip_rect {
            c.intersection(&rect).unwrap_or(Rect::ZERO)
        } else {
            rect
        });
    }
    fn clip_rrect(&mut self, rrect: RRect) {
        self.clip_rect(rrect.rect);
    }
    fn set_opacity(&mut self, opacity: f32) {
        self.current_state.opacity *= opacity.clamp(0.0, 1.0);
    }
}

/// WGPU renderer state
pub struct WgpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: Option<wgpu::Surface<'static>>,
    surface_config: Option<wgpu::SurfaceConfiguration>,
    solid_pipeline: SolidPipeline,
    tesselator: Tesselator,
    text_renderer: Option<TextRenderer>,
    // MSAA for anti-aliasing
    msaa_texture: Option<wgpu::Texture>,
    sample_count: u32,
}

impl WgpuRenderer {
    pub async fn new(window: Option<Arc<winit::window::Window>>) -> Result<Self, RenderError> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        let surface = if let Some(w) = window.as_ref() {
            Some(
                instance
                    .create_surface(w.clone())
                    .map_err(|e| RenderError::SurfaceCreation(e.to_string()))?,
            )
        } else {
            None
        };
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: surface.as_ref(),
            })
            .await
            .map_err(|e| RenderError::AdapterRequest(e.to_string()))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Flui WGPU Device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            })
            .await
            .map_err(|e| RenderError::DeviceCreation(e.to_string()))?;
        let surface_config = if let (Some(s), Some(w)) = (surface.as_ref(), window) {
            let size = w.inner_size();
            let caps = s.get_capabilities(&adapter);
            let config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: caps.formats[0],
                width: size.width,
                height: size.height,
                present_mode: wgpu::PresentMode::Immediate, // VSync OFF - maximum FPS
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            };
            s.configure(&device, &config);
            Some(config)
        } else {
            None
        };
        let surface_format = surface_config
            .as_ref()
            .map(|c| c.format)
            .unwrap_or(wgpu::TextureFormat::Bgra8UnormSrgb);
        let viewport_size = surface_config
            .as_ref()
            .map(|c| (c.width as f32, c.height as f32))
            .unwrap_or((800.0, 600.0));

        // Use 4x MSAA for smooth anti-aliasing
        let sample_count = 4;
        let solid_pipeline =
            SolidPipeline::new_with_msaa(&device, surface_format, viewport_size, sample_count);

        // Create MSAA texture if we have a surface
        let msaa_texture = surface_config.as_ref().map(|config| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("MSAA Texture"),
                size: wgpu::Extent3d {
                    width: config.width,
                    height: config.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count,
                dimension: wgpu::TextureDimension::D2,
                format: surface_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
        });

        // Initialize text renderer with same MSAA as main renderer
        let text_renderer = surface_config.as_ref().map(|c| {
            TextRenderer::new_with_msaa(
                &device,
                &queue,
                surface_format,
                c.width,
                c.height,
                sample_count,
            )
        });

        Ok(Self {
            device,
            queue,
            surface,
            surface_config,
            solid_pipeline,
            tesselator: Tesselator::new(128), // Increased from 32 to 128 for smoother circles
            text_renderer,
            msaa_texture,
            sample_count,
        })
    }

    fn render_frame(&mut self, commands: &[GpuDrawCommand]) -> Result<(), RenderError> {
        // Separate text commands from shape commands
        let mut text_commands = Vec::new();

        self.tesselator.clear();
        for cmd in commands {
            match cmd {
                GpuDrawCommand::Rect {
                    rect,
                    color,
                    transform,
                } => self.tesselator.tesselate_rect(*rect, *color, *transform),
                GpuDrawCommand::RRect {
                    rrect,
                    color,
                    transform,
                } => self.tesselator.tesselate_rrect(*rrect, *color, *transform),
                GpuDrawCommand::Circle {
                    center,
                    radius,
                    color,
                    transform,
                } => self
                    .tesselator
                    .tesselate_circle(*center, *radius, *color, *transform),
                GpuDrawCommand::Line {
                    p1,
                    p2,
                    width,
                    color,
                    transform,
                } => self
                    .tesselator
                    .tesselate_line(*p1, *p2, *width, *color, *transform),
                GpuDrawCommand::Text(text_cmd) => {
                    text_commands.push(text_cmd.clone());
                }
            }
        }

        // Prepare text rendering if we have text commands
        if let Some(ref mut text_renderer) = self.text_renderer {
            if !text_commands.is_empty() {
                text_renderer
                    .prepare(&self.device, &self.queue, &text_commands)
                    .map_err(|e| RenderError::TextRenderingFailed(e.to_string()))?;
            }
        }
        if self.tesselator.vertex_count() == 0 {
            return Ok(());
        }
        let vbuf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Vertex Buffer"),
                contents: bytemuck::cast_slice(&self.tesselator.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let ibuf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Index Buffer"),
                contents: bytemuck::cast_slice(&self.tesselator.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        if let Some(surface) = &self.surface {
            let frame = surface.get_current_texture()?;
            let view = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());

            // Create MSAA view if available
            let msaa_view = self
                .msaa_texture
                .as_ref()
                .map(|tex| tex.create_view(&wgpu::TextureViewDescriptor::default()));

            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Render Encoder"),
                });
            {
                // Use MSAA texture as render target, resolve to screen
                let (render_view, resolve_target) = if let Some(ref msaa) = msaa_view {
                    (msaa, Some(&view))
                } else {
                    (&view, None)
                };

                let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Render Pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: render_view,
                        resolve_target,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.1,
                                g: 0.1,
                                b: 0.1,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                // Render shapes first
                if self.tesselator.vertex_count() > 0 {
                    rpass.set_pipeline(self.solid_pipeline.pipeline());
                    rpass.set_bind_group(0, self.solid_pipeline.bind_group(), &[]);
                    rpass.set_vertex_buffer(0, vbuf.slice(..));
                    rpass.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint32);
                    rpass.draw_indexed(0..self.tesselator.index_count() as u32, 0, 0..1);
                }

                // Render text on top of shapes
                if let Some(ref text_renderer) = self.text_renderer {
                    if !text_commands.is_empty() {
                        text_renderer
                            .render(&mut rpass)
                            .map_err(|e| RenderError::TextRenderingFailed(e.to_string()))?;
                    }
                }
            }
            self.queue.submit(Some(encoder.finish()));
            frame.present();
        }
        Ok(())
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if let (Some(s), Some(c)) = (self.surface.as_ref(), self.surface_config.as_mut()) {
            c.width = width;
            c.height = height;
            s.configure(&self.device, c);
            self.solid_pipeline
                .update_viewport(&self.queue, width as f32, height as f32);

            // Recreate MSAA texture with new size
            self.msaa_texture = Some(self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("MSAA Texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: self.sample_count,
                dimension: wgpu::TextureDimension::D2,
                format: c.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            }));

            // Also resize text renderer if present
            if let Some(ref mut text_renderer) = self.text_renderer {
                text_renderer.resize(&self.queue, width, height);
            }
        }
    }

    pub fn surface_size(&self) -> Option<Size> {
        self.surface_config
            .as_ref()
            .map(|c| Size::new(c.width as f32, c.height as f32))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("Failed to create surface: {0}")]
    SurfaceCreation(String),
    #[error("No suitable GPU adapter found")]
    AdapterNotFound,
    #[error("Failed to request adapter: {0}")]
    AdapterRequest(String),
    #[error("Failed to create device: {0}")]
    DeviceCreation(String),
    #[error("Surface error: {0}")]
    SurfaceError(#[from] wgpu::SurfaceError),
    #[error("Text rendering failed: {0}")]
    TextRenderingFailed(String),
}
