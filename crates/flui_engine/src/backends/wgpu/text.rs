//! GPU text rendering using glyphon
//!
//! Provides high-performance text rendering using GPU acceleration.

use glam::Mat4;
use glyphon::{
    Attrs, Buffer, Color, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache, TextArea,
    TextAtlas, TextBounds, TextRenderer as GlyphonTextRenderer, Viewport,
};
use wgpu::{Device, MultisampleState, Queue};

use flui_types::Point;

/// Text alignment options
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

/// Text rendering command
#[derive(Debug, Clone)]
pub struct TextCommand {
    pub text: String,
    pub position: Point,
    pub font_size: f32,
    pub color: [f32; 4],
    pub max_width: Option<f32>,
    pub align: TextAlign,
    pub transform: Mat4,
}

/// GPU text renderer wrapper around glyphon
pub struct TextRenderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    atlas: TextAtlas,
    text_renderer: GlyphonTextRenderer,
    viewport: Viewport,
    viewport_width: u32,
    viewport_height: u32,
    // Store buffers to keep them alive during rendering
    buffers: Vec<Buffer>,
}

impl TextRenderer {
    /// Create a new text renderer
    pub fn new(
        device: &Device,
        queue: &Queue,
        surface_format: wgpu::TextureFormat,
        viewport_width: u32,
        viewport_height: u32,
    ) -> Self {
        Self::new_with_msaa(
            device,
            queue,
            surface_format,
            viewport_width,
            viewport_height,
            1,
        )
    }

    /// Create a new text renderer with MSAA
    pub fn new_with_msaa(
        device: &Device,
        queue: &Queue,
        surface_format: wgpu::TextureFormat,
        viewport_width: u32,
        viewport_height: u32,
        sample_count: u32,
    ) -> Self {
        let font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let cache = glyphon::Cache::new(device);
        let mut atlas = TextAtlas::new(device, queue, &cache, surface_format);

        let text_renderer = GlyphonTextRenderer::new(
            &mut atlas,
            device,
            MultisampleState {
                count: sample_count,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            None,
        );

        // Create viewport and update it with initial resolution
        let mut viewport = Viewport::new(device, &cache);
        viewport.update(
            queue,
            Resolution {
                width: viewport_width,
                height: viewport_height,
            },
        );

        Self {
            font_system,
            swash_cache,
            atlas,
            text_renderer,
            viewport,
            viewport_width,
            viewport_height,
            buffers: Vec::new(),
        }
    }

    /// Prepare text for rendering
    ///
    /// This clears previous buffers and prepares new ones for the current frame
    pub fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
        commands: &[TextCommand],
    ) -> Result<(), TextRenderError> {
        // Clear previous frame's buffers
        self.buffers.clear();
        self.buffers.reserve(commands.len());

        // Create buffers for each text command
        for cmd in commands {
            let mut buffer = Buffer::new(
                &mut self.font_system,
                Metrics::new(cmd.font_size, cmd.font_size * 1.2),
            );

            // Set text with default attributes
            let attrs = Attrs::new().family(Family::SansSerif);
            buffer.set_text(&mut self.font_system, &cmd.text, &attrs, Shaping::Advanced);

            // Set buffer dimensions
            if let Some(max_width) = cmd.max_width {
                buffer.set_size(&mut self.font_system, Some(max_width), None);
            }

            // Shape the buffer
            buffer.shape_until_scroll(&mut self.font_system, false);

            self.buffers.push(buffer);
        }

        // Create text areas referencing the stored buffers
        let text_areas: Vec<TextArea> = self
            .buffers
            .iter()
            .zip(commands.iter())
            .map(|(buffer, cmd)| {
                let color = Color::rgba(
                    (cmd.color[0] * 255.0) as u8,
                    (cmd.color[1] * 255.0) as u8,
                    (cmd.color[2] * 255.0) as u8,
                    (cmd.color[3] * 255.0) as u8,
                );

                TextArea {
                    buffer,
                    left: cmd.position.x,
                    top: cmd.position.y,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: 0,
                        top: 0,
                        right: self.viewport_width as i32,
                        bottom: self.viewport_height as i32,
                    },
                    default_color: color,
                    custom_glyphs: &[],
                }
            })
            .collect();

        // Prepare atlas with all text areas
        self.text_renderer
            .prepare(
                device,
                queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                text_areas,
                &mut self.swash_cache,
            )
            .map_err(|e| TextRenderError::PreparationFailed(e.to_string()))?;

        Ok(())
    }

    /// Render prepared text to the given render pass
    pub fn render<'rpass>(
        &'rpass self,
        pass: &mut wgpu::RenderPass<'rpass>,
    ) -> Result<(), TextRenderError> {
        self.text_renderer
            .render(&self.atlas, &self.viewport, pass)
            .map_err(|e| TextRenderError::RenderFailed(e.to_string()))?;

        Ok(())
    }

    /// Update viewport size
    pub fn resize(&mut self, queue: &Queue, width: u32, height: u32) {
        self.viewport_width = width;
        self.viewport_height = height;
        self.viewport.update(queue, Resolution { width, height });
    }

    /// Trim the atlas to free unused space
    pub fn trim_atlas(&mut self) {
        self.atlas.trim();
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TextRenderError {
    #[error("Text preparation failed: {0}")]
    PreparationFailed(String),

    #[error("Text rendering failed: {0}")]
    RenderFailed(String),
}
