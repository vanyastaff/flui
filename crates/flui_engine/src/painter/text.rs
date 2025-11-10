//! Text rendering using glyphon
//!
//! This module provides a clean wrapper around glyphon for GPU-accelerated text rendering.
//! Follows KISS principle: simple API that handles batching and rendering internally.

use flui_types::{styling::Color, Point};
use glyphon::{
    Attrs, Buffer, Cache, Color as GlyphonColor, Family, FontSystem, Metrics, Resolution,
    Shaping, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer as GlyphonRenderer,
    Viewport,
};

/// Text rendering system using glyphon
///
/// Manages font loading, text layout, and GPU-accelerated glyph rasterization.
/// Batches text across the frame for efficient rendering.
///
/// # Example
/// ```ignore
/// let mut text_renderer = TextRenderer::new(&device, &queue, surface_format)?;
///
/// // Add text during frame
/// text_renderer.add_text("Hello, World!", Point::new(10.0, 10.0), 16.0, Color::BLACK);
///
/// // Render all text at end of frame
/// text_renderer.render(&device, &queue, &view, &mut encoder, (800, 600))?;
/// ```
pub struct TextRenderer {
    /// Font system (manages font loading and shaping)
    font_system: FontSystem,

    /// Swash cache (rasterizes glyphs)
    swash_cache: SwashCache,

    /// Text atlas (texture atlas for glyphs)
    text_atlas: TextAtlas,

    /// Glyphon renderer
    renderer: GlyphonRenderer,

    /// Viewport (manages resolution and transforms)
    viewport: Viewport,

    /// Batched text buffers for current frame
    text_buffers: Vec<(Buffer, Point, GlyphonColor)>,
}

impl TextRenderer {
    /// Create a new text renderer
    ///
    /// # Arguments
    /// * `device` - wgpu device
    /// * `queue` - wgpu queue
    /// * `format` - Surface texture format
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Self {
        #[cfg(debug_assertions)]
        tracing::debug!("TextRenderer::new: format={:?}", format);

        // Initialize font system
        let font_system = FontSystem::new();

        // Initialize glyph rasterization
        let swash_cache = SwashCache::new();

        // Create GPU glyph cache
        let cache = Cache::new(device);

        // Create text atlas (texture for glyphs)
        let mut text_atlas = TextAtlas::new(device, queue, &cache, format);

        // Create glyphon renderer
        let renderer = GlyphonRenderer::new(
            &mut text_atlas,
            device,
            wgpu::MultisampleState::default(),
            None,
        );

        // Create viewport
        let viewport = Viewport::new(device, &cache);

        Self {
            font_system,
            swash_cache,
            text_atlas,
            renderer,
            viewport,
            text_buffers: Vec::new(),
        }
    }

    /// Add text to be rendered this frame
    ///
    /// Text is batched and rendered together for efficiency.
    ///
    /// # Arguments
    /// * `text` - Text string to render
    /// * `position` - Screen position (top-left corner)
    /// * `font_size` - Font size in pixels
    /// * `color` - Text color
    pub fn add_text(&mut self, text: &str, position: Point, font_size: f32, color: Color) {
        #[cfg(debug_assertions)]
        tracing::debug!(
            "TextRenderer::add_text: text='{}', position={:?}, size={}, color={:?}",
            text,
            position,
            font_size,
            color
        );

        // Create text buffer with metrics
        let mut buffer = Buffer::new(&mut self.font_system, Metrics::new(font_size, font_size));

        // Set buffer size (large enough for most text)
        buffer.set_size(&mut self.font_system, Some(1000.0), Some(1000.0));

        // Set text with default font attributes
        let attrs = Attrs::new().family(Family::SansSerif);
        buffer.set_text(&mut self.font_system, text, &attrs, Shaping::Advanced);

        // Convert FLUI color to glyphon color
        let glyphon_color = GlyphonColor::rgba(color.r, color.g, color.b, color.a);

        // Add to batch
        self.text_buffers.push((buffer, position, glyphon_color));
    }

    /// Render all batched text to the GPU
    ///
    /// This should be called once per frame after all text has been added.
    ///
    /// # Arguments
    /// * `device` - wgpu device
    /// * `queue` - wgpu queue
    /// * `view` - Texture view to render to
    /// * `encoder` - Command encoder
    /// * `size` - Viewport size (width, height)
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
        size: (u32, u32),
    ) -> Result<(), String> {
        // Skip if no text to render
        if self.text_buffers.is_empty() {
            return Ok(());
        }

        #[cfg(debug_assertions)]
        tracing::debug!(
            "TextRenderer::render: {} text buffers, size=({}, {})",
            self.text_buffers.len(),
            size.0,
            size.1
        );

        // Update viewport with current resolution
        self.viewport.update(
            queue,
            Resolution {
                width: size.0,
                height: size.1,
            },
        );

        // Create text areas from batched buffers
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
                    right: size.0 as i32,
                    bottom: size.1 as i32,
                },
                default_color: *color,
                custom_glyphs: &[],
            })
            .collect();

        // Prepare glyphs (upload to GPU)
        self.renderer
            .prepare(
                device,
                queue,
                &mut self.font_system,
                &mut self.text_atlas,
                &self.viewport,
                text_areas,
                &mut self.swash_cache,
            )
            .map_err(|e| format!("Failed to prepare text: {:?}", e))?;

        // Render text
        let mut text_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Text Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load, // Don't clear - text is rendered on top
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        self.renderer
            .render(&self.text_atlas, &self.viewport, &mut text_pass)
            .map_err(|e| format!("Failed to render text: {:?}", e))?;

        // Clear buffers for next frame
        self.text_buffers.clear();

        Ok(())
    }

    /// Get number of batched text buffers
    #[inline]
    pub fn text_count(&self) -> usize {
        self.text_buffers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_batching() {
        // Note: Can't test without wgpu device, but we can test the API structure
        // This would need integration tests with a headless device
    }
}
