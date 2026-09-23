//! cosmic-text 0.19.0 shaping/layout, pinned to the exact version
//! `flui-painting` depends on (see `crates/flui-painting/src/text_layout/layout.rs`
//! for the production usage this mirrors -- `Buffer::new` + `set_text` here
//! instead of `Buffer::new_empty` + `set_rich_text`, since a single-style
//! spike shape doesn't need the rich-text/per-span path production takes).
//!
//! Uses `FontSystem::new()`'s default (unfiltered) fallback, not flui's
//! `EmojiForbiddenFallback` wrapper -- the spike measures cosmic-text's
//! *own* default fallback quality, which is the fair baseline for a
//! stack-vs-stack comparison.

use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache};

use crate::ShapeResult;

pub struct CosmicBackend {
    font_system: FontSystem,
    // Held so a "warm cache" run can rasterize into an already-populated
    // glyph cache; unused by shape-only measurements but keeps backend
    // construction cost (font system + cache init) comparable to parley's.
    _swash_cache: SwashCache,
}

impl CosmicBackend {
    pub fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
            _swash_cache: SwashCache::new(),
        }
    }

    pub fn shape(&mut self, text: &str, max_width: Option<f32>) -> ShapeResult {
        let metrics = Metrics::new(16.0, 20.0);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        // `set_size`/`set_text` are plain `&mut self` methods -- only
        // `shape_until_scroll` needs the `FontSystem` (confirmed against
        // the vendored cosmic-text-0.19.0 source after `cargo check`
        // rejected the `&mut FontSystem` argument these took in the
        // `Buffer::new_empty`/`set_rich_text` path flui-painting uses).
        buffer.set_size(max_width, None);
        buffer.set_text(
            text,
            &Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut self.font_system, false);

        let mut line_count = 0usize;
        let mut glyph_count = 0usize;
        for run in buffer.layout_runs() {
            line_count += 1;
            glyph_count += run.glyphs.len();
        }
        ShapeResult {
            line_count,
            glyph_count,
        }
    }
}
