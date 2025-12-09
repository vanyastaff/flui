//! Text rendering using glyphon
//!
//! This module provides a clean wrapper around glyphon for GPU-accelerated text rendering.
//! Follows KISS principle: simple API that handles batching and rendering internally.
//!
//! # Caching Strategy
//!
//! Text layout is expensive (shaping, line breaking, metrics calculation).
//! We cache `Buffer` objects keyed by (text, font_size) to avoid re-layout
//! when the same text is rendered in subsequent frames.

use flui_types::{styling::Color, Point};
use glyphon::{
    Attrs, Buffer, Cache, Color as GlyphonColor, Family, FontSystem, Metrics, Resolution, Shaping,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer as GlyphonRenderer, Viewport,
};
use std::collections::HashMap;

/// Cache key for text buffers
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TextCacheKey {
    text: String,
    font_size_bits: u32, // f32 as bits for hashing
}

impl TextCacheKey {
    fn new(text: &str, font_size: f32) -> Self {
        Self {
            text: text.to_string(),
            font_size_bits: font_size.to_bits(),
        }
    }
}

/// Cached text buffer with LRU tracking
struct CachedBuffer {
    buffer: Buffer,
    last_used_frame: u64,
}

/// Text rendering system using glyphon
///
/// Manages font loading, text layout, and GPU-accelerated glyph rasterization.
/// Batches text across the frame for efficient rendering.
///
/// # Caching
///
/// Text buffers are cached by (text, font_size) to avoid expensive re-layout.
/// Cache is automatically pruned to remove stale entries.
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

    /// Batched text buffers for current frame (references into cache)
    text_areas_data: Vec<(TextCacheKey, Point, GlyphonColor)>,

    /// Cache of text buffers (text + font_size -> Buffer)
    buffer_cache: HashMap<TextCacheKey, CachedBuffer>,

    /// Current frame number for LRU tracking
    current_frame: u64,

    /// Max cache size (number of entries)
    max_cache_size: usize,

    /// Cache statistics
    cache_hits: u64,
    cache_misses: u64,
}

impl TextRenderer {
    /// Initialize font system with smart fallback strategy
    ///
    /// Strategy:
    /// 1. Try to load system fonts (works on desktop platforms)
    /// 2. If system fonts unavailable or empty, load embedded fonts
    /// 3. Embedded fonts are always included as fallback for reliability
    fn initialize_font_system() -> FontSystem {
        let mut fs = FontSystem::new();

        // Check if system fonts were loaded successfully
        let system_fonts_available = fs.db().faces().count() > 0;

        if system_fonts_available {
            tracing::trace!("Loaded {} system fonts", fs.db().faces().count());
        } else {
            // No system fonts - load embedded fonts as primary
            tracing::warn!("No system fonts available, loading embedded fonts");
            Self::load_embedded_fonts(&mut fs);

            if fs.db().faces().count() == 0 {
                tracing::error!("Failed to load any fonts! Text rendering may fail.");
            } else {
                tracing::info!("Loaded {} embedded fonts", fs.db().faces().count());
            }
        }

        fs
    }

    /// Load embedded fonts into font system
    ///
    /// Includes Roboto-Regular as the primary fallback font.
    /// This ensures text rendering works on all platforms.
    fn load_embedded_fonts(fs: &mut FontSystem) {
        const ROBOTO_REGULAR: &[u8] = include_bytes!("../../assets/fonts/Roboto-Regular.ttf");

        // Note: load_font_data() returns (), not Result
        fs.db_mut().load_font_data(ROBOTO_REGULAR.to_vec());
        tracing::trace!("Loaded embedded Roboto-Regular font");

        // TODO: Add more embedded fonts if needed (Bold, Italic, etc.)
        // const ROBOTO_BOLD: &[u8] = include_bytes!("../../assets/fonts/Roboto-Bold.ttf");
        // fs.db_mut().load_font_data(ROBOTO_BOLD.to_vec());
    }

    /// Create a new text renderer
    ///
    /// # Arguments
    /// * `device` - wgpu device
    /// * `queue` - wgpu queue
    /// * `format` - Surface texture format
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        #[cfg(debug_assertions)]
        tracing::trace!("TextRenderer::new: format={:?}", format);

        // Initialize font system with smart fallback
        let font_system = Self::initialize_font_system();

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
            text_areas_data: Vec::new(),
            buffer_cache: HashMap::new(),
            current_frame: 0,
            max_cache_size: 256, // Reasonable default for most UIs
            cache_hits: 0,
            cache_misses: 0,
        }
    }

    /// Get or create a cached buffer for text
    fn get_or_create_buffer(&mut self, key: &TextCacheKey) -> &Buffer {
        // Check if we have a cached buffer
        if self.buffer_cache.contains_key(key) {
            // Update LRU timestamp
            if let Some(cached) = self.buffer_cache.get_mut(key) {
                cached.last_used_frame = self.current_frame;
            }
            self.cache_hits += 1;
        } else {
            // Create new buffer
            let font_size = f32::from_bits(key.font_size_bits);
            let mut buffer = Buffer::new(&mut self.font_system, Metrics::new(font_size, font_size));

            // Set buffer size (large enough for most text)
            buffer.set_size(&mut self.font_system, Some(1000.0), Some(1000.0));

            // Set text with default font attributes
            let attrs = Attrs::new().family(Family::SansSerif);
            buffer.set_text(&mut self.font_system, &key.text, &attrs, Shaping::Advanced);

            // Shape the text (this is the expensive part!)
            buffer.shape_until_scroll(&mut self.font_system, false);

            self.buffer_cache.insert(
                key.clone(),
                CachedBuffer {
                    buffer,
                    last_used_frame: self.current_frame,
                },
            );
            self.cache_misses += 1;
        }

        &self.buffer_cache.get(key).unwrap().buffer
    }

    /// Add text to be rendered this frame
    ///
    /// Text is batched and rendered together for efficiency.
    /// Buffers are cached to avoid re-layout on subsequent frames.
    ///
    /// # Arguments
    /// * `text` - Text string to render
    /// * `position` - Screen position (top-left corner)
    /// * `font_size` - Font size in pixels
    /// * `color` - Text color
    pub fn add_text(&mut self, text: &str, position: Point, font_size: f32, color: Color) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "TextRenderer::add_text: text='{}', position={:?}, size={}, color={:?}",
            text,
            position,
            font_size,
            color
        );

        // Create cache key
        let key = TextCacheKey::new(text, font_size);

        // Ensure buffer exists in cache (creates if needed)
        let _ = self.get_or_create_buffer(&key);

        // Convert FLUI color to glyphon color
        let glyphon_color = GlyphonColor::rgba(color.r, color.g, color.b, color.a);

        // Add to batch (just store key reference, actual buffer is in cache)
        self.text_areas_data.push((key, position, glyphon_color));
    }

    /// Prune old cache entries (LRU eviction)
    fn prune_cache(&mut self) {
        if self.buffer_cache.len() <= self.max_cache_size {
            return;
        }

        // Remove entries not used in the last 60 frames (~1 second at 60fps)
        let threshold_frame = self.current_frame.saturating_sub(60);
        self.buffer_cache
            .retain(|_, cached| cached.last_used_frame >= threshold_frame);

        // If still too large, remove oldest entries
        while self.buffer_cache.len() > self.max_cache_size {
            // Find oldest entry
            if let Some(oldest_key) = self
                .buffer_cache
                .iter()
                .min_by_key(|(_, v)| v.last_used_frame)
                .map(|(k, _)| k.clone())
            {
                self.buffer_cache.remove(&oldest_key);
            } else {
                break;
            }
        }
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
        // Increment frame counter
        self.current_frame += 1;

        // Skip if no text to render
        if self.text_areas_data.is_empty() {
            return Ok(());
        }

        #[cfg(debug_assertions)]
        {
            let hit_rate = if self.cache_hits + self.cache_misses > 0 {
                (self.cache_hits as f64 / (self.cache_hits + self.cache_misses) as f64) * 100.0
            } else {
                0.0
            };
            tracing::trace!(
                "TextRenderer::render: {} text buffers, size=({}, {}), cache_hit_rate={:.1}%",
                self.text_areas_data.len(),
                size.0,
                size.1,
                hit_rate
            );
        }

        // Update viewport with current resolution
        self.viewport.update(
            queue,
            Resolution {
                width: size.0,
                height: size.1,
            },
        );

        // Create text areas from cached buffers
        let text_areas: Vec<TextArea<'_>> = self
            .text_areas_data
            .iter()
            .filter_map(|(key, position, color)| {
                self.buffer_cache.get(key).map(|cached| TextArea {
                    buffer: &cached.buffer,
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

        // Clear batch data for next frame (but keep cache!)
        self.text_areas_data.clear();

        // Periodically prune cache
        if self.current_frame % 60 == 0 {
            self.prune_cache();
        }

        Ok(())
    }

    /// Get number of batched text buffers
    #[inline]
    pub fn text_count(&self) -> usize {
        self.text_areas_data.len()
    }

    /// Get cache statistics (hits, misses, cache_size)
    #[allow(dead_code)]
    pub fn cache_stats(&self) -> (u64, u64, usize) {
        (self.cache_hits, self.cache_misses, self.buffer_cache.len())
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_text_batching() {
        // Note: Can't test without wgpu device, but we can test the API structure
        // This would need integration tests with a headless device
    }
}
