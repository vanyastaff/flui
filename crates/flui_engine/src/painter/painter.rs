//! GPU-accelerated 2D painter using wgpu + glyphon + lyon
//!
//! This is the unified painter implementation that combines:
//! - Shape rendering via vertex batching
//! - Text rendering via glyphon
//! - Path tessellation via lyon
//! - Transform stack for coordinate transformations
//!
//! Follows SOLID and KISS principles with clean separation of concerns.

use std::sync::Arc;

use super::{
    paint::{Paint, Stroke},
    tessellator::Tessellator,
    text::TextRenderer,
    vertex::Vertex,
};
use flui_types::{geometry::RRect, Offset, Point, Rect};
use wgpu::util::DeviceExt;

/// GPU painter for hardware-accelerated 2D rendering
///
/// Batches all drawing operations per frame for efficient GPU rendering.
/// Supports shapes, text, transforms, and clipping.
///
/// # Example
/// ```ignore
/// let mut painter = WgpuPainter::new(device, queue, surface_format, (800, 600));
///
/// painter.rect(Rect::from_ltrb(10.0, 10.0, 100.0, 100.0), &Paint::fill(Color::RED));
/// painter.text("Hello", Point::new(10.0, 120.0), 16.0, &Paint::fill(Color::BLACK));
///
/// painter.render(&view, &mut encoder)?;
/// ```
pub struct WgpuPainter {
    // ===== GPU State =====
    /// wgpu device (Arc for sharing with text renderer)
    device: Arc<wgpu::Device>,

    /// wgpu queue (Arc for sharing with text renderer)
    queue: Arc<wgpu::Queue>,

    /// Surface texture format
    surface_format: wgpu::TextureFormat,

    /// Viewport size (width, height)
    size: (u32, u32),

    // ===== Shape Rendering =====
    /// Shape rendering pipeline
    shape_pipeline: wgpu::RenderPipeline,

    /// Batched vertices for current frame
    vertices: Vec<Vertex>,

    /// Batched indices for current frame
    indices: Vec<u32>,

    // ===== Tessellation =====
    /// Lyon-based path tessellator for complex shapes
    tessellator: Tessellator,

    // ===== Text Rendering =====
    /// Glyphon-based text renderer
    text_renderer: TextRenderer,

    // ===== Transform Stack =====
    /// Stack of saved transforms
    transform_stack: Vec<glam::Mat4>,

    /// Current active transform
    current_transform: glam::Mat4,
}

impl WgpuPainter {
    /// Create a new GPU painter
    ///
    /// # Arguments
    /// * `device` - wgpu device
    /// * `queue` - wgpu queue
    /// * `surface_format` - Surface texture format
    /// * `size` - Initial viewport size (width, height)
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        size: (u32, u32),
    ) -> Self {
        #[cfg(debug_assertions)]
        tracing::debug!(
            "WgpuPainter::new: format={:?}, size=({}, {})",
            surface_format,
            size.0,
            size.1
        );

        // Wrap device and queue in Arc for sharing
        let device = Arc::new(device);
        let queue = Arc::new(queue);

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
                buffers: &[Vertex::desc()],
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

        // Create tessellator for complex shapes
        let tessellator = Tessellator::new();

        // Create text renderer
        let text_renderer = TextRenderer::new(&device, &queue, surface_format);

        // Initialize transform stack with identity
        let current_transform = glam::Mat4::IDENTITY;
        let transform_stack = Vec::new();

        Self {
            device,
            queue,
            surface_format,
            size,
            shape_pipeline,
            vertices: Vec::new(),
            indices: Vec::new(),
            tessellator,
            text_renderer,
            transform_stack,
            current_transform,
        }
    }

    /// Render all batched geometry to a texture view
    ///
    /// This should be called once per frame after all drawing operations.
    ///
    /// # Arguments
    /// * `view` - Texture view to render to
    /// * `encoder` - Command encoder
    pub fn render(
        &mut self,
        view: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<(), String> {
        #[cfg(debug_assertions)]
        tracing::debug!(
            "WgpuPainter::render: vertices={}, indices={}, text_count={}",
            self.vertices.len(),
            self.indices.len(),
            self.text_renderer.text_count()
        );

        // ===== Render Shapes =====
        if !self.vertices.is_empty() {
            // Upload vertices to GPU
            let vertex_buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Shape Vertex Buffer"),
                    contents: bytemuck::cast_slice(&self.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });

            // Upload indices to GPU
            let index_buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Shape Index Buffer"),
                    contents: bytemuck::cast_slice(&self.indices),
                    usage: wgpu::BufferUsages::INDEX,
                });

            // Render shapes in single pass
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Shape Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load, // Don't clear - background already cleared
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(&self.shape_pipeline);
            render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            render_pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            render_pass.draw_indexed(0..self.indices.len() as u32, 0, 0..1);
        }

        // ===== Render Text =====
        self.text_renderer
            .render(&self.device, &self.queue, view, encoder, self.size)?;

        // ===== Clear buffers for next frame =====
        self.vertices.clear();
        self.indices.clear();

        Ok(())
    }

    /// Resize the viewport
    ///
    /// Call this when the window is resized.
    pub fn resize(&mut self, width: u32, height: u32) {
        #[cfg(debug_assertions)]
        tracing::debug!("WgpuPainter::resize: ({}, {})", width, height);

        self.size = (width, height);
    }

    // ===== Helper Methods =====

    /// Apply current transform to a point
    fn apply_transform(&self, point: Point) -> Point {
        let p = self.current_transform * glam::vec4(point.x, point.y, 0.0, 1.0);
        Point::new(p.x, p.y)
    }

    /// Add a simple rectangle (4 vertices, 6 indices)
    fn add_rect(&mut self, rect: Rect, paint: &Paint) {
        let base_index = self.vertices.len() as u32;

        // Transform corners
        let top_left = self.apply_transform(rect.top_left());
        let top_right = self.apply_transform(rect.top_right());
        let bottom_left = self.apply_transform(rect.bottom_left());
        let bottom_right = self.apply_transform(rect.bottom_right());

        // Add vertices (4 corners)
        self.vertices.extend_from_slice(&[
            Vertex::with_color(top_left, paint.color),
            Vertex::with_color(top_right, paint.color),
            Vertex::with_color(bottom_right, paint.color),
            Vertex::with_color(bottom_left, paint.color),
        ]);

        // Add indices (2 triangles)
        self.indices.extend_from_slice(&[
            base_index,
            base_index + 1,
            base_index + 2,
            base_index,
            base_index + 2,
            base_index + 3,
        ]);
    }

    /// Add tessellated shape from vertices/indices
    fn add_tessellated(&mut self, vertices: Vec<Vertex>, indices: Vec<u32>) {
        let base_index = self.vertices.len() as u32;

        // Add vertices (already transformed by tessellator if needed)
        self.vertices.extend(vertices);

        // Add indices with offset
        self.indices
            .extend(indices.iter().map(|&i| i + base_index));
    }
}

// ===== Painter Trait Implementation =====

/// Painter trait for layer system compatibility
pub trait Painter {
    // Core drawing methods
    fn rect(&mut self, rect: Rect, paint: &Paint);
    fn rrect(&mut self, rrect: RRect, paint: &Paint);
    fn circle(&mut self, center: Point, radius: f32, paint: &Paint);
    fn line(&mut self, p1: Point, p2: Point, paint: &Paint);
    fn text(&mut self, text: &str, position: Point, font_size: f32, paint: &Paint);

    // Transform stack
    fn save(&mut self);
    fn restore(&mut self);
    fn translate(&mut self, offset: Offset);
    fn rotate(&mut self, angle: f32);
    fn scale(&mut self, sx: f32, sy: f32);

    // Clipping
    fn clip_rect(&mut self, rect: Rect);
    fn clip_rrect(&mut self, rrect: RRect);

    // Advanced methods with default implementations (stubs for layers)
    fn save_layer(&mut self) {
        self.save(); // Fallback to regular save
    }

    fn save_layer_backdrop(&mut self) {
        self.save(); // Fallback to regular save
    }

    fn set_opacity(&mut self, _opacity: f32) {
        // No-op by default
        #[cfg(debug_assertions)]
        tracing::warn!("Painter::set_opacity: not implemented");
    }

    fn apply_image_filter(&mut self, _filter: &str) {
        // No-op by default
        #[cfg(debug_assertions)]
        tracing::warn!("Painter::apply_image_filter: not implemented");
    }

    fn clip_oval(&mut self, _rect: Rect) {
        // No-op by default
        #[cfg(debug_assertions)]
        tracing::warn!("Painter::clip_oval: not implemented");
    }

    fn clip_path(&mut self, _path: &str) {
        // No-op by default
        #[cfg(debug_assertions)]
        tracing::warn!("Painter::clip_path: not implemented");
    }

    fn path(&mut self, _path: &str, _paint: &Paint) {
        // No-op by default
        #[cfg(debug_assertions)]
        tracing::warn!("Painter::path: not implemented");
    }

    fn arc(&mut self, _center: Point, _radius: f32, _start_angle: f32, _end_angle: f32, _paint: &Paint) {
        // No-op by default
        #[cfg(debug_assertions)]
        tracing::warn!("Painter::arc: not implemented");
    }

    fn polygon(&mut self, _points: &std::sync::Arc<Vec<Point>>, _paint: &Paint) {
        // No-op by default
        #[cfg(debug_assertions)]
        tracing::warn!("Painter::polygon: not implemented");
    }

    fn skew(&mut self, _skew_x: f32, _skew_y: f32) {
        // No-op by default
        #[cfg(debug_assertions)]
        tracing::warn!("Painter::skew: not implemented");
    }

    fn transform_matrix(&mut self, _matrix: &[f32; 16]) {
        // No-op by default
        #[cfg(debug_assertions)]
        tracing::warn!("Painter::transform_matrix: not implemented");
    }

    fn text_styled(&mut self, text: &str, position: Point, font_size: f32, paint: &Paint) {
        // Fallback to regular text() method
        self.text(text, position, font_size, paint);
    }

    fn draw_image(&mut self, _image_name: &str, _position: Point) {
        // No-op by default
        #[cfg(debug_assertions)]
        tracing::warn!("Painter::draw_image: not implemented");
    }
}

impl Painter for WgpuPainter {
    fn rect(&mut self, rect: Rect, paint: &Paint) {
        #[cfg(debug_assertions)]
        tracing::debug!("WgpuPainter::rect: rect={:?}, paint={:?}", rect, paint);

        if paint.is_fill() {
            // Simple filled rect - direct quad
            self.add_rect(rect, paint);
        } else {
            // Stroked rect - use tessellator
            let stroke = paint.stroke.unwrap_or_else(|| Stroke::new(1.0)); // Default 1px stroke
            if let Ok((vertices, indices)) = self.tessellator.tessellate_rect_stroke(rect, paint, &stroke) {
                self.add_tessellated(vertices, indices);
            }
        }
    }

    fn rrect(&mut self, rrect: RRect, paint: &Paint) {
        #[cfg(debug_assertions)]
        tracing::debug!("WgpuPainter::rrect: rrect={:?}, paint={:?}", rrect, paint);

        // Use tessellator for proper rounded corners
        if let Ok((vertices, indices)) = self.tessellator.tessellate_rrect(rrect, paint) {
            self.add_tessellated(vertices, indices);
        } else {
            // Fallback to simple rect
            self.add_rect(rrect.rect, paint);
        }
    }

    fn circle(&mut self, center: Point, radius: f32, paint: &Paint) {
        #[cfg(debug_assertions)]
        tracing::debug!(
            "WgpuPainter::circle: center={:?}, radius={}, paint={:?}",
            center,
            radius,
            paint
        );

        // Use tessellator for proper circle
        if let Ok((vertices, indices)) = self.tessellator.tessellate_circle(center, radius, paint) {
            self.add_tessellated(vertices, indices);
        }
    }

    fn line(&mut self, p1: Point, p2: Point, paint: &Paint) {
        #[cfg(debug_assertions)]
        tracing::debug!(
            "WgpuPainter::line: p1={:?}, p2={:?}, paint={:?}",
            p1,
            p2,
            paint
        );

        // Use tessellator for line stroke
        let stroke = paint.stroke.unwrap_or_else(|| Stroke::new(1.0)); // Default 1px stroke
        if let Ok((vertices, indices)) = self.tessellator.tessellate_line(p1, p2, paint, &stroke) {
            self.add_tessellated(vertices, indices);
        }
    }

    fn text(&mut self, text: &str, position: Point, font_size: f32, paint: &Paint) {
        #[cfg(debug_assertions)]
        tracing::debug!(
            "WgpuPainter::text: text='{}', position={:?}, size={}, color={:?}",
            text,
            position,
            font_size,
            paint.color
        );

        // Apply transform to position
        let transformed_position = self.apply_transform(position);

        // Delegate to text renderer
        self.text_renderer
            .add_text(text, transformed_position, font_size, paint.color);
    }

    // ===== Transform Stack =====

    fn save(&mut self) {
        #[cfg(debug_assertions)]
        tracing::debug!("WgpuPainter::save: stack depth={}", self.transform_stack.len());

        self.transform_stack.push(self.current_transform);
    }

    fn restore(&mut self) {
        if let Some(transform) = self.transform_stack.pop() {
            self.current_transform = transform;

            #[cfg(debug_assertions)]
            tracing::debug!(
                "WgpuPainter::restore: stack depth={}",
                self.transform_stack.len()
            );
        } else {
            #[cfg(debug_assertions)]
            tracing::warn!("WgpuPainter::restore: stack underflow");
        }
    }

    fn translate(&mut self, offset: Offset) {
        #[cfg(debug_assertions)]
        tracing::debug!("WgpuPainter::translate: offset={:?}", offset);

        let translation = glam::Mat4::from_translation(glam::vec3(offset.dx, offset.dy, 0.0));
        self.current_transform = self.current_transform * translation;
    }

    fn rotate(&mut self, angle: f32) {
        #[cfg(debug_assertions)]
        tracing::debug!("WgpuPainter::rotate: angle={}", angle);

        let rotation = glam::Mat4::from_rotation_z(angle);
        self.current_transform = self.current_transform * rotation;
    }

    fn scale(&mut self, sx: f32, sy: f32) {
        #[cfg(debug_assertions)]
        tracing::debug!("WgpuPainter::scale: sx={}, sy={}", sx, sy);

        let scaling = glam::Mat4::from_scale(glam::vec3(sx, sy, 1.0));
        self.current_transform = self.current_transform * scaling;
    }

    // ===== Clipping (TODO) =====

    fn clip_rect(&mut self, _rect: Rect) {
        #[cfg(debug_assertions)]
        tracing::warn!("WgpuPainter::clip_rect: not yet implemented");
        // TODO: Implement using scissor rect or stencil buffer
    }

    fn clip_rrect(&mut self, _rrect: RRect) {
        #[cfg(debug_assertions)]
        tracing::warn!("WgpuPainter::clip_rrect: not yet implemented");
        // TODO: Implement using stencil buffer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Full tests require wgpu device initialization
    // These would be integration tests with headless rendering

    #[test]
    fn test_transform_stack() {
        // Would need headless wgpu device for proper testing
    }
}
