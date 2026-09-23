//! `shape_retained` exists for a fair timing comparison against parley
//! (a second Codex review finding, after the memory-retention one):
//! `shape` builds a local `Buffer` and drops it before returning, so its
//! destruction happens INSIDE the timed window the caller measures with
//! `Instant::elapsed()`. `ParleyBackend::shape_per_paragraph_retained`
//! returns its `Layout`s to the caller instead, so their destruction
//! happens AFTER both the timing and RSS samples. Comparing the two
//! without accounting for this let cosmic-text's `elapsed_ms` silently
//! include deallocating hundreds of MB while parley's did not.
//! `shape_retained` mirrors parley's shape: it returns the `Buffer`
//! alongside the result so `main.rs` can hold it past both samples too.

use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache};

use crate::ShapeResult;

pub struct CosmicBackend {
    font_system: FontSystem,
    _swash_cache: SwashCache,
}

impl CosmicBackend {
    pub fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
            _swash_cache: SwashCache::new(),
        }
    }

    /// Shapes and drops the `Buffer` before returning -- NOT the call to
    /// use when timing/RSS are being compared against parley's retained
    /// path; see `shape_retained` and the module docs.
    pub fn shape(&mut self, text: &str, max_width: Option<f32>) -> ShapeResult {
        self.shape_retained(text, max_width).0
    }

    /// Same shaping work as `shape`, but returns the `Buffer` instead of
    /// dropping it -- the caller must keep it alive until after sampling
    /// both `elapsed_ms` and peak RSS for the comparison against parley's
    /// `shape_per_paragraph_retained` to be fair on both axes.
    pub fn shape_retained(&mut self, text: &str, max_width: Option<f32>) -> (ShapeResult, Buffer) {
        let metrics = Metrics::new(16.0, 20.0);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
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
        (
            ShapeResult {
                line_count,
                glyph_count,
            },
            buffer,
        )
    }
}
