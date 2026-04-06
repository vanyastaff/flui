//! Text batching for glyph rendering.

use crate::text::TextCacheKey;

// ─── PreparedTextRun ─────────────────────────────────────────────────────────

/// A prepared text run ready for GPU submission.
#[derive(Debug, Clone)]
pub struct PreparedTextRun {
    /// Cache key identifying the shaped text run.
    pub cache_key: TextCacheKey,
    /// Original text content (needed for shaping on cache miss).
    pub text: String,
    /// Font family name (e.g. "sans-serif", "Roboto").
    pub font_family: String,
    /// Position in logical pixels (x, y).
    pub position: [f32; 2],
    /// RGBA color.
    pub color: [f32; 4],
    /// Optional clip rectangle (x, y, w, h).
    pub clip: Option<[f32; 4]>,
}

// ─── TextBatcher ─────────────────────────────────────────────────────────────

/// Collects text runs into a batch for efficient rendering.
pub struct TextBatcher {
    runs: Vec<PreparedTextRun>,
}

impl TextBatcher {
    /// Create an empty text batcher.
    pub fn new() -> Self {
        Self { runs: Vec::new() }
    }

    /// Add a text run to the batch.
    pub fn add_run(
        &mut self,
        key: TextCacheKey,
        text: String,
        font_family: String,
        position: [f32; 2],
        color: [f32; 4],
        clip: Option<[f32; 4]>,
    ) {
        self.runs.push(PreparedTextRun {
            cache_key: key,
            text,
            font_family,
            position,
            color,
            clip,
        });
    }

    /// Return a slice of all accumulated text runs.
    pub fn runs(&self) -> &[PreparedTextRun] {
        &self.runs
    }

    /// Return the number of runs in the batch.
    pub fn run_count(&self) -> usize {
        self.runs.len()
    }

    /// Check if the batcher has no runs.
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    /// Remove all runs from the batch.
    pub fn clear(&mut self) {
        self.runs.clear();
    }
}

impl Default for TextBatcher {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_key(text: &str) -> TextCacheKey {
        TextCacheKey::new(text, 16.0, "Arial", 400)
    }

    #[test]
    fn empty_text_batcher() {
        let batcher = TextBatcher::new();
        assert!(batcher.is_empty());
        assert_eq!(batcher.run_count(), 0);
    }

    #[test]
    fn add_run_accumulates() {
        let mut batcher = TextBatcher::new();
        batcher.add_run(make_key("hello"), "hello".into(), "Arial".into(), [0.0, 0.0], [1.0; 4], None);
        batcher.add_run(
            make_key("world"),
            "world".into(),
            "Arial".into(),
            [10.0, 20.0],
            [0.0, 0.0, 0.0, 1.0],
            Some([0.0, 0.0, 100.0, 100.0]),
        );
        assert_eq!(batcher.run_count(), 2);
        assert!(!batcher.is_empty());
    }

    #[test]
    fn clear_resets() {
        let mut batcher = TextBatcher::new();
        batcher.add_run(make_key("hello"), "hello".into(), "Arial".into(), [0.0, 0.0], [1.0; 4], None);
        assert!(!batcher.is_empty());
        batcher.clear();
        assert!(batcher.is_empty());
        assert_eq!(batcher.run_count(), 0);
    }

    #[test]
    fn text_cache_key_equality() {
        let key1 = TextCacheKey::new("hello", 16.0, "Arial", 400);
        let key2 = TextCacheKey::new("hello", 16.0, "Arial", 400);
        assert_eq!(key1, key2);
    }

    #[test]
    fn text_cache_key_different_size() {
        let key1 = TextCacheKey::new("hello", 16.0, "Arial", 400);
        let key2 = TextCacheKey::new("hello", 24.0, "Arial", 400);
        assert_ne!(key1, key2);
    }
}
