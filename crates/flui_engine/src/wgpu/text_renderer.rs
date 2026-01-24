//! Text rendering integration
//!
//! This module integrates glyphon for GPU-accelerated text rendering.
//! Handles font loading, glyph atlas management, and text layout.

use std::sync::Arc;
use wgpu::{Device, MultisampleState, Queue, TextureFormat};

#[cfg(feature = "wgpu-backend")]
use glyphon::{FontSystem, SwashCache, TextAtlas, TextRenderer as GlyphonRenderer};

use flui_types::{
    geometry::Point,
    styling::Color,
    typography::TextStyle,
    units::DevicePixels,
};

/// Text rendering system
///
/// Manages font system, glyph atlas, and text rendering via glyphon.
/// Provides high-quality, GPU-accelerated text rendering.
pub struct TextRenderingSystem {
    #[cfg(feature = "wgpu-backend")]
    font_system: FontSystem,

    #[cfg(feature = "wgpu-backend")]
    swash_cache: SwashCache,

    #[cfg(feature = "wgpu-backend")]
    text_atlas: TextAtlas,

    #[cfg(feature = "wgpu-backend")]
    text_renderer: GlyphonRenderer,

    #[cfg(not(feature = "wgpu-backend"))]
    _phantom: std::marker::PhantomData<()>,
}

#[cfg(feature = "wgpu-backend")]
impl TextRenderingSystem {
    /// Create a new text rendering system
    ///
    /// # Arguments
    ///
    /// * `device` - GPU device
    /// * `queue` - GPU queue
    /// * `surface_format` - Target surface texture format
    pub fn new(device: &Device, queue: &Queue, surface_format: TextureFormat) -> Self {
        let mut font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let mut text_atlas = TextAtlas::new(device, queue, surface_format);

        let text_renderer = GlyphonRenderer::new(
            device,
            queue,
            MultisampleState::default(),
            surface_format,
        );

        Self {
            font_system,
            swash_cache,
            text_atlas,
            text_renderer,
        }
    }

    /// Prepare text for rendering
    ///
    /// # Arguments
    ///
    /// * `text` - Text to render
    /// * `position` - Position in device pixels
    /// * `style` - Text style
    /// * `color` - Text color
    pub fn prepare_text(
        &mut self,
        text: &str,
        position: Point<DevicePixels>,
        style: &TextStyle,
        color: Color,
    ) -> TextRun {
        TextRun {
            text: text.to_string(),
            position,
            style: style.clone(),
            color,
        }
    }

    /// Render prepared text runs
    ///
    /// # Arguments
    ///
    /// * `device` - GPU device
    /// * `queue` - GPU queue
    /// * `runs` - Text runs to render
    pub fn render_text_runs(
        &mut self,
        device: &Device,
        queue: &Queue,
        runs: &[TextRun],
    ) {
        // TODO: Implement actual glyphon rendering
        // This requires:
        // 1. Convert TextRun to glyphon::TextArea
        // 2. Shape text using font_system
        // 3. Upload glyphs to atlas
        // 4. Render using text_renderer

        tracing::trace!(count = runs.len(), "Rendering text runs");
    }

    /// Trim the glyph atlas to free unused space
    pub fn trim_atlas(&mut self) {
        self.text_atlas.trim();
    }
}

#[cfg(not(feature = "wgpu-backend"))]
impl TextRenderingSystem {
    /// Create a placeholder text rendering system (no wgpu backend)
    pub fn new(_device: &Device, _queue: &Queue, _surface_format: TextureFormat) -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }

    /// Prepare text for rendering (no-op without wgpu backend)
    pub fn prepare_text(
        &mut self,
        text: &str,
        position: Point<DevicePixels>,
        style: &TextStyle,
        color: Color,
    ) -> TextRun {
        TextRun {
            text: text.to_string(),
            position,
            style: style.clone(),
            color,
        }
    }

    /// Render text runs (no-op without wgpu backend)
    pub fn render_text_runs(
        &mut self,
        _device: &Device,
        _queue: &Queue,
        _runs: &[TextRun],
    ) {
        // No-op
    }

    /// Trim atlas (no-op without wgpu backend)
    pub fn trim_atlas(&mut self) {
        // No-op
    }
}

/// Prepared text run ready for rendering
///
/// Stores all information needed to render text at a specific position.
#[derive(Clone, Debug)]
pub struct TextRun {
    /// Text content
    pub text: String,

    /// Position in device pixels
    pub position: Point<DevicePixels>,

    /// Text style
    pub style: TextStyle,

    /// Text color
    pub color: Color,
}

impl TextRun {
    /// Create a new text run
    #[must_use]
    pub fn new(
        text: String,
        position: Point<DevicePixels>,
        style: TextStyle,
        color: Color,
    ) -> Self {
        Self {
            text,
            position,
            style,
            color,
        }
    }

    /// Get text length in characters
    #[must_use]
    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// Check if text run is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(value: f32) -> DevicePixels {
        DevicePixels(value as i32)
    }

    #[test]
    fn test_text_run_creation() {
        let run = TextRun::new(
            "Hello".to_string(),
            Point::new(px(10.0), px(20.0)),
            TextStyle::default(),
            Color::BLACK,
        );

        assert_eq!(run.text, "Hello");
        assert_eq!(run.len(), 5);
        assert!(!run.is_empty());
    }

    #[test]
    fn test_text_run_empty() {
        let run = TextRun::new(
            String::new(),
            Point::new(px(0.0), px(0.0)),
            TextStyle::default(),
            Color::BLACK,
        );

        assert!(run.is_empty());
        assert_eq!(run.len(), 0);
    }

    #[test]
    fn test_text_rendering_system_exists() {
        // Compile-time check
        let _ = std::marker::PhantomData::<TextRenderingSystem>;
    }
}
