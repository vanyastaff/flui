//! GPU Painter implementation for wgpu
//!
//! This implements actual GPU rendering using wgpu render pipelines.

use flui_engine::painter::compat::{Paint, Painter, RRect};
use flui_types::{Offset, Point, Rect};
use glyphon::{
    Attrs, Buffer, Cache, Color as GlyphonColor, Family, FontSystem, Metrics, Resolution, Shaping,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};
use wgpu::util::DeviceExt;

/// Vertex for shape rendering
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct ShapeVertex {
    position: [f32; 2],
    color: [f32; 4],
}

impl ShapeVertex {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ShapeVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}

/// GPU painter implementation
pub struct WgpuPainter {
    /// wgpu device
    device: wgpu::Device,

    /// wgpu queue
    queue: wgpu::Queue,

    /// Surface format
    surface_format: wgpu::TextureFormat,

    /// Window size
    size: (u32, u32),

    /// Shape render pipeline
    shape_pipeline: wgpu::RenderPipeline,

    /// Collected vertices for shapes
    vertices: Vec<ShapeVertex>,

    /// Collected indices for shapes
    indices: Vec<u16>,

    /// Font system for text rendering
    font_system: FontSystem,

    /// Text renderer
    text_renderer: TextRenderer,

    /// Swash cache for glyph rasterization
    swash_cache: SwashCache,

    /// Glyph cache
    cache: Cache,

    /// Text atlas
    text_atlas: TextAtlas,

    /// Text buffers to render
    text_buffers: Vec<(Buffer, Point, glyphon::Color)>,

    /// Viewport for text rendering
    viewport: Viewport,
}

impl WgpuPainter {
    /// Create a new GPU painter
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        size: (u32, u32),
    ) -> Self {
        // Create shape shader
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shape Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/shape.wgsl").into()),
        });

        // Create shape pipeline
        let shape_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Shape Pipeline Layout"),
                bind_group_layouts: &[],
                push_constant_ranges: &[],
            });

        let shape_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Shape Pipeline"),
            layout: Some(&shape_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[ShapeVertex::desc()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });

        // Create text rendering system
        let mut font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let cache = Cache::new(&device);
        let mut text_atlas = TextAtlas::new(&device, &queue, &cache, surface_format);
        let text_renderer = TextRenderer::new(
            &mut text_atlas,
            &device,
            wgpu::MultisampleState::default(),
            None,
        );
        let viewport = Viewport::new(&device, &cache);

        Self {
            device,
            queue,
            surface_format,
            size,
            shape_pipeline,
            vertices: Vec::new(),
            indices: Vec::new(),
            font_system,
            text_renderer,
            swash_cache,
            cache,
            text_atlas,
            text_buffers: Vec::new(),
            viewport,
        }
    }

    /// Render all collected geometry to a texture view
    pub fn render(&mut self, view: &wgpu::TextureView, encoder: &mut wgpu::CommandEncoder) {
        #[cfg(debug_assertions)]
        tracing::debug!("WgpuPainter::render: vertices={}, indices={}, text_buffers={}",
            self.vertices.len(), self.indices.len(), self.text_buffers.len());

        // Upload vertices and indices
        if !self.vertices.is_empty() {
            let vertex_buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Shape Vertex Buffer"),
                    contents: bytemuck::cast_slice(&self.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });

            let index_buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Shape Index Buffer"),
                    contents: bytemuck::cast_slice(&self.indices),
                    usage: wgpu::BufferUsages::INDEX,
                });

            // Render shapes
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Shape Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load, // Don't clear, we already cleared
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(&self.shape_pipeline);
            render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            render_pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            render_pass.draw_indexed(0..self.indices.len() as u32, 0, 0..1);
        }

        // Render text
        if !self.text_buffers.is_empty() {
            // Update viewport with current resolution
            self.viewport.update(
                &self.queue,
                Resolution {
                    width: self.size.0,
                    height: self.size.1,
                },
            );

            let text_areas: Vec<TextArea> = self
                .text_buffers
                .iter()
                .map(|(buffer, position, color)| TextArea {
                    buffer,
                    left: position.x,
                    top: position.y,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: 0,
                        top: 0,
                        right: self.size.0 as i32,
                        bottom: self.size.1 as i32,
                    },
                    default_color: *color,
                    custom_glyphs: &[],
                })
                .collect();

            self.text_renderer
                .prepare(
                    &self.device,
                    &self.queue,
                    &mut self.font_system,
                    &mut self.text_atlas,
                    &self.viewport,
                    text_areas,
                    &mut self.swash_cache,
                )
                .expect("Failed to prepare text");

            let mut text_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Text Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            self.text_renderer
                .render(&self.text_atlas, &self.viewport, &mut text_pass)
                .expect("Failed to render text");
        }

        // Clear buffers for next frame
        self.vertices.clear();
        self.indices.clear();
        self.text_buffers.clear();
    }

    /// Add a rectangle to the batch
    fn add_rect(&mut self, rect: Rect, color: [f32; 4]) {
        let base_index = self.vertices.len() as u16;

        // Add vertices (two triangles forming a rectangle)
        let (x0, y0) = self.to_ndc(rect.min.x, rect.min.y);
        let (x1, y1) = self.to_ndc(rect.max.x, rect.max.y);

        self.vertices.extend_from_slice(&[
            ShapeVertex {
                position: [x0, y0],
                color,
            },
            ShapeVertex {
                position: [x1, y0],
                color,
            },
            ShapeVertex {
                position: [x1, y1],
                color,
            },
            ShapeVertex {
                position: [x0, y1],
                color,
            },
        ]);

        // Add indices (two triangles)
        self.indices.extend_from_slice(&[
            base_index,
            base_index + 1,
            base_index + 2,
            base_index,
            base_index + 2,
            base_index + 3,
        ]);
    }

    /// Add a rounded rectangle (approximated with multiple rectangles)
    fn add_rounded_rect(&mut self, rect: Rect, radius: f32, color: [f32; 4]) {
        // For simplicity, approximate rounded corners with a simple rect
        // TODO: Implement proper rounded corners with more triangles
        self.add_rect(rect, color);
    }

    /// Convert screen coordinates to NDC (Normalized Device Coordinates)
    fn to_ndc(&self, x: f32, y: f32) -> (f32, f32) {
        let ndc_x = (x / self.size.0 as f32) * 2.0 - 1.0;
        let ndc_y = 1.0 - (y / self.size.1 as f32) * 2.0; // Flip Y
        (ndc_x, ndc_y)
    }

    /// Convert RGBA u8 color to f32
    fn color_to_f32(r: u8, g: u8, b: u8, a: u8) -> [f32; 4] {
        [
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        ]
    }
}

impl Painter for WgpuPainter {
    fn rect(&mut self, rect: Rect, paint: &Paint) {
        #[cfg(debug_assertions)]
        tracing::debug!("WgpuPainter::rect: rect={:?}, color=({}, {}, {}, {})",
            rect, paint.color.r, paint.color.g, paint.color.b, paint.color.a);

        let color = Self::color_to_f32(
            paint.color.r,
            paint.color.g,
            paint.color.b,
            paint.color.a,
        );
        self.add_rect(rect, color);
    }

    fn rrect(&mut self, rrect: RRect, paint: &Paint) {
        let color = Self::color_to_f32(
            paint.color.r,
            paint.color.g,
            paint.color.b,
            paint.color.a,
        );
        // Use the average of all corner radii
        let radius = (rrect.top_left.x
            + rrect.top_left.y
            + rrect.top_right.x
            + rrect.top_right.y
            + rrect.bottom_left.x
            + rrect.bottom_left.y
            + rrect.bottom_right.x
            + rrect.bottom_right.y)
            / 8.0;
        self.add_rounded_rect(rrect.rect, radius, color);
    }

    fn circle(&mut self, center: Point, radius: f32, paint: &Paint) {
        // Approximate circle as a square for now
        let rect = Rect {
            min: Point {
                x: center.x - radius,
                y: center.y - radius,
            },
            max: Point {
                x: center.x + radius,
                y: center.y + radius,
            },
        };
        self.rect(rect, paint);
    }

    fn line(&mut self, _p1: Point, _p2: Point, _paint: &Paint) {
        // TODO: Implement line drawing
    }

    fn text(&mut self, text: &str, position: Point, font_size: f32, paint: &Paint) {
        #[cfg(debug_assertions)]
        tracing::debug!("WgpuPainter::text: text='{}', position={:?}, font_size={}, color=({}, {}, {}, {})",
            text, position, font_size, paint.color.r, paint.color.g, paint.color.b, paint.color.a);

        let mut buffer = Buffer::new(&mut self.font_system, Metrics::new(font_size, font_size));

        buffer.set_size(&mut self.font_system, Some(1000.0), Some(1000.0));
        let attrs = Attrs::new().family(Family::SansSerif);
        buffer.set_text(&mut self.font_system, text, &attrs, Shaping::Advanced);

        let color = GlyphonColor::rgba(
            paint.color.r,
            paint.color.g,
            paint.color.b,
            paint.color.a,
        );

        self.text_buffers.push((buffer, position, color));
    }

    fn save(&mut self) {
        // No-op for now
    }

    fn restore(&mut self) {
        // No-op for now
    }

    fn translate(&mut self, _offset: Offset) {
        // No-op for now - positions are already in screen space
    }

    fn scale(&mut self, _sx: f32, _sy: f32) {
        // No-op for now
    }

    fn rotate(&mut self, _angle: f32) {
        // No-op for now
    }

    fn clip_rect(&mut self, _rect: Rect) {
        // No-op for now
    }

    fn clip_rrect(&mut self, _rrect: RRect) {
        // No-op for now
    }
}
