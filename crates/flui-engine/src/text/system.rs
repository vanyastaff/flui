//! Text rendering system coordinating font loading and glyph rasterization.
//!
//! Wraps glyphon 0.9 to provide GPU-accelerated text rendering with automatic
//! glyph atlas management. Integrates with [`ShapeCache`](super::cache::ShapeCache)
//! for buffer reuse across frames.

use glyphon::{
    Attrs, Buffer, Cache, Color as GlyphonColor, Family, FontSystem, Metrics, Resolution, Shaping,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer as GlyphonRenderer, Viewport, Weight,
};

use crate::batchers::text::PreparedTextRun;
use crate::text::cache::ShapeCache;

/// GPU-accelerated text rendering system backed by glyphon.
///
/// Manages the full text pipeline: font loading, text shaping, glyph
/// rasterization, atlas packing, and render-pass submission.
///
/// # Usage
///
/// Typically held behind a `parking_lot::Mutex` inside [`GpuDevice`] and
/// accessed once per frame to prepare and render batched text runs.
pub struct TextSystem {
    font_system: FontSystem,
    swash_cache: SwashCache,
    text_atlas: TextAtlas,
    text_renderer: GlyphonRenderer,
    viewport: Viewport,
    shape_cache: ShapeCache<Buffer>,
    current_frame: u64,
}

impl TextSystem {
    /// Create a new text system.
    ///
    /// Initialises the font system with platform fonts and an embedded Roboto
    /// fallback, then sets up the glyph atlas and renderer for the given
    /// texture format.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Self {
        let mut font_system = Self::initialize_font_system();

        // Load embedded fallback font
        Self::load_embedded_fonts(&mut font_system);

        let swash_cache = SwashCache::new();
        let cache = Cache::new(device);
        let mut text_atlas = TextAtlas::new(device, queue, &cache, format);

        let text_renderer = GlyphonRenderer::new(
            &mut text_atlas,
            device,
            wgpu::MultisampleState::default(),
            None,
        );

        let viewport = Viewport::new(device, &cache);

        Self {
            font_system,
            swash_cache,
            text_atlas,
            text_renderer,
            viewport,
            shape_cache: ShapeCache::new(256, 120),
            current_frame: 0,
        }
    }

    /// Prepare text runs for rendering.
    ///
    /// Shapes each [`PreparedTextRun`] into a glyphon `Buffer` (or retrieves
    /// it from the shape cache), builds the corresponding `TextArea` list,
    /// and uploads new glyphs to the atlas.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        runs: &[PreparedTextRun],
        width: u32,
        height: u32,
        scale: f32,
    ) {
        self.current_frame += 1;

        self.viewport.update(
            queue,
            Resolution {
                width,
                height,
            },
        );

        // Ensure all runs have cached buffers
        for run in runs {
            if self.shape_cache.get(&run.cache_key, self.current_frame).is_none() {
                let font_size = f32::from_bits(run.cache_key.font_size_bits);
                let line_height = font_size * 1.2;
                let mut buffer =
                    Buffer::new(&mut self.font_system, Metrics::new(font_size, line_height));

                buffer.set_size(&mut self.font_system, Some(width as f32), Some(height as f32));

                let family = if run.font_family.is_empty() {
                    Family::SansSerif
                } else {
                    Family::Name(&run.font_family)
                };
                let weight = Weight(run.cache_key.font_weight);
                let attrs = Attrs::new().family(family).weight(weight);
                buffer.set_text(&mut self.font_system, &run.text, &attrs, Shaping::Advanced);
                buffer.shape_until_scroll(&mut self.font_system, false);

                self.shape_cache
                    .insert(run.cache_key.clone(), buffer, self.current_frame);
            }
        }

        // Update LRU timestamps for all runs in one pass (mutable borrow).
        self.shape_cache.touch_keys(
            runs.iter().map(|r| &r.cache_key),
            self.current_frame,
        );

        // Build TextArea references using immutable borrows only.
        let mut text_areas: Vec<TextArea<'_>> = Vec::with_capacity(runs.len());
        for run in runs {
            if let Some(buffer) = self.shape_cache.get_ref(&run.cache_key) {
                let color = GlyphonColor::rgba(
                    (run.color[0] * 255.0) as u8,
                    (run.color[1] * 255.0) as u8,
                    (run.color[2] * 255.0) as u8,
                    (run.color[3] * 255.0) as u8,
                );

                let (bounds_left, bounds_top, bounds_right, bounds_bottom) =
                    if let Some(clip) = run.clip {
                        (
                            clip[0] as i32,
                            clip[1] as i32,
                            (clip[0] + clip[2]) as i32,
                            (clip[1] + clip[3]) as i32,
                        )
                    } else {
                        (0, 0, width as i32, height as i32)
                    };

                text_areas.push(TextArea {
                    buffer,
                    left: run.position[0],
                    top: run.position[1],
                    scale,
                    bounds: TextBounds {
                        left: bounds_left,
                        top: bounds_top,
                        right: bounds_right,
                        bottom: bounds_bottom,
                    },
                    default_color: color,
                    custom_glyphs: &[],
                });
            }
        }

        if let Err(e) = self.text_renderer.prepare(
            device,
            queue,
            &mut self.font_system,
            &mut self.text_atlas,
            &self.viewport,
            text_areas,
            &mut self.swash_cache,
        ) {
            tracing::error!("Failed to prepare text: {:?}", e);
        }
    }

    /// Submit text rendering commands into an existing render pass.
    pub fn render<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>) {
        if let Err(e) = self
            .text_renderer
            .render(&self.text_atlas, &self.viewport, pass)
        {
            tracing::error!("Failed to render text: {:?}", e);
        }
    }

    /// Trim the glyph atlas, evicting glyphs that were not used this frame.
    pub fn trim(&mut self) {
        self.text_atlas.trim();
    }

    // ── Private helpers ─────────────────────────────────────────────────

    /// Initialise the font system, preferring platform fonts.
    fn initialize_font_system() -> FontSystem {
        let fs = FontSystem::new();

        let face_count = fs.db().faces().count();
        if face_count > 0 {
            tracing::trace!("Loaded {} system fonts", face_count);
        } else {
            tracing::warn!("No system fonts available, relying on embedded fonts");
        }

        fs
    }

    /// Load the embedded Roboto-Regular fallback font.
    fn load_embedded_fonts(fs: &mut FontSystem) {
        const ROBOTO_REGULAR: &[u8] = include_bytes!("../../assets/fonts/Roboto-Regular.ttf");
        fs.db_mut().load_font_data(ROBOTO_REGULAR.to_vec());
        tracing::trace!("Loaded embedded Roboto-Regular font");
    }
}
