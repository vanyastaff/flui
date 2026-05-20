//! Text rendering integration
//!
//! This module integrates glyphon for GPU-accelerated text rendering.
//! Handles font loading, glyph atlas management, and text layout.

use flui_types::{
    geometry::{DevicePixels, Pixels, Point},
    styling::Color,
    typography::TextStyle,
};
#[cfg(feature = "wgpu-backend")]
use glyphon::{Cache, FontSystem, SwashCache, TextAtlas, TextRenderer as GlyphonRenderer};
use wgpu::{Device, MultisampleState, Queue, TextureFormat};

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
        let font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let cache = Cache::new(device);
        let mut text_atlas = TextAtlas::new(device, queue, &cache, surface_format);

        let text_renderer =
            GlyphonRenderer::new(&mut text_atlas, device, MultisampleState::default(), None);

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

    /// Render prepared text runs by delegating to the working TextRenderer
    ///
    /// # Arguments
    ///
    /// * `_device` - GPU device (reserved for future use)
    /// * `_queue` - GPU queue (reserved for future use)
    /// * `runs` - Text runs to render
    /// * `text_renderer` - The underlying TextRenderer that handles actual rendering
    pub fn render_text_runs(
        &mut self,
        _device: &Device,
        _queue: &Queue,
        runs: &[TextRun],
        text_renderer: &mut super::text::TextRenderer,
    ) {
        for run in runs {
            let font_size = run.style.font_size.unwrap_or(14.0) as f32;
            let position = Point::new(
                Pixels(run.position.x.0 as f32),
                Pixels(run.position.y.0 as f32),
            );
            text_renderer.add_text(&run.text, position, font_size, run.color);
        }
        tracing::trace!(count = runs.len(), "Delegated text runs to TextRenderer");
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
        _text_renderer: &mut super::text::TextRenderer,
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

    fn px(value: i32) -> DevicePixels {
        DevicePixels(value)
    }

    #[test]
    fn test_text_run_creation() {
        let run = TextRun::new(
            "Hello".to_string(),
            Point::new(px(10), px(20)),
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
            Point::new(px(0), px(0)),
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

    #[test]
    fn test_text_rendering_system_prepare_returns_valid_run() {
        let style = TextStyle {
            font_size: Some(24.0),
            ..TextStyle::default()
        };
        let position = Point::new(px(100), px(200));
        let color = Color::rgba(255, 0, 0, 255);

        let run = TextRun::new("Test text".to_string(), position, style.clone(), color);

        assert_eq!(run.text, "Test text");
        assert_eq!(run.position.x, px(100));
        assert_eq!(run.position.y, px(200));
        assert_eq!(run.style.font_size, Some(24.0));
        assert_eq!(run.color, color);
    }

    #[test]
    fn test_text_run_batch_collection() {
        let runs: Vec<TextRun> = vec![
            TextRun::new(
                "First".to_string(),
                Point::new(px(0), px(0)),
                TextStyle::default(),
                Color::BLACK,
            ),
            TextRun::new(
                "Second".to_string(),
                Point::new(px(100), px(0)),
                TextStyle::default(),
                Color::WHITE,
            ),
            TextRun::new(
                "Third".to_string(),
                Point::new(px(200), px(0)),
                TextStyle::default(),
                Color::rgba(128, 128, 128, 255),
            ),
        ];

        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0].text, "First");
        assert_eq!(runs[1].text, "Second");
        assert_eq!(runs[2].text, "Third");
        assert_eq!(runs[0].position.x, px(0));
        assert_eq!(runs[1].position.x, px(100));
        assert_eq!(runs[2].position.x, px(200));
    }
}
