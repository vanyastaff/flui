//! Cached layer - optimized layer caching for RepaintBoundary
//!
//! This layer provides efficient caching of painted content, allowing
//! the framework to skip repainting when only the parent changes.

use parking_lot::RwLock;
use std::sync::Arc;

use flui_types::geometry::Rect;

/// Cached layer content
///
/// Wraps another layer and caches its rendering results for performance.
/// Used by RenderRepaintBoundary to avoid unnecessary repaints.
///
/// # Architecture
///
/// ```text
/// RenderRepaintBoundary
///   ↓
/// CachedLayer (with dirty flag)
///   ↓
/// Wrapped Layer (Canvas/ShaderMask/etc.)
/// ```
///
/// # Lifecycle
///
/// 1. **First paint**: Render wrapped layer, cache result
/// 2. **Parent changes**: Reuse cached result (no repaint)
/// 3. **Child changes**: Mark dirty, repaint on next frame
///
/// # Example
///
/// ```rust
/// use flui_layer::{CachedLayer, CanvasLayer, Layer};
///
/// let canvas = CanvasLayer::new();
/// let cached = CachedLayer::new(Layer::Canvas(canvas));
///
/// // Check if needs repaint
/// assert!(cached.is_dirty()); // Starts dirty
///
/// // Mark clean after rendering
/// cached.mark_clean();
/// assert!(!cached.is_dirty());
///
/// // Mark dirty to force repaint
/// cached.mark_dirty();
/// assert!(cached.is_dirty());
/// ```
#[derive(Debug, Clone)]
pub struct CachedLayer {
    /// The wrapped layer to cache
    inner: Arc<RwLock<super::Layer>>,

    /// Whether the cache is dirty and needs repainting
    dirty: Arc<RwLock<bool>>,
}

impl CachedLayer {
    /// Create new cached layer wrapping another layer.
    pub fn new(layer: super::Layer) -> Self {
        Self {
            inner: Arc::new(RwLock::new(layer)),
            dirty: Arc::new(RwLock::new(true)), // Start dirty to force initial render
        }
    }

    /// Mark the cache as dirty, forcing a repaint on next render.
    pub fn mark_dirty(&self) {
        *self.dirty.write() = true;
    }

    /// Mark the cache as clean (after rendering).
    pub fn mark_clean(&self) {
        *self.dirty.write() = false;
    }

    /// Check if the cache is dirty.
    pub fn is_dirty(&self) -> bool {
        *self.dirty.read()
    }

    /// Update the wrapped layer.
    ///
    /// Automatically marks the cache as dirty.
    pub fn update_layer(&self, new_layer: super::Layer) {
        *self.inner.write() = new_layer;
        self.mark_dirty();
    }

    /// Get a read lock on the wrapped layer.
    pub fn inner(&self) -> parking_lot::RwLockReadGuard<'_, super::Layer> {
        self.inner.read()
    }

    /// Get a write lock on the wrapped layer.
    ///
    /// Automatically marks the cache as dirty.
    pub fn inner_mut(&self) -> parking_lot::RwLockWriteGuard<'_, super::Layer> {
        self.mark_dirty();
        self.inner.write()
    }

    /// Returns the bounds of the wrapped layer.
    pub fn bounds(&self) -> Option<Rect> {
        self.inner.read().bounds()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::{CanvasLayer, Layer};

    #[test]
    fn test_cached_layer_new() {
        let canvas = CanvasLayer::new();
        let cached = CachedLayer::new(Layer::Canvas(canvas));

        assert!(cached.is_dirty()); // Should start dirty
    }

    #[test]
    fn test_cached_layer_mark_dirty() {
        let canvas = CanvasLayer::new();
        let cached = CachedLayer::new(Layer::Canvas(canvas));

        // Clear dirty flag for testing
        cached.mark_clean();
        assert!(!cached.is_dirty());

        cached.mark_dirty();
        assert!(cached.is_dirty());
    }

    #[test]
    fn test_cached_layer_mark_clean() {
        let canvas = CanvasLayer::new();
        let cached = CachedLayer::new(Layer::Canvas(canvas));

        assert!(cached.is_dirty());
        cached.mark_clean();
        assert!(!cached.is_dirty());
    }

    #[test]
    fn test_cached_layer_update_layer() {
        let canvas1 = CanvasLayer::new();
        let cached = CachedLayer::new(Layer::Canvas(canvas1));

        // Clear dirty flag
        cached.mark_clean();
        assert!(!cached.is_dirty());

        // Update layer should mark dirty
        let canvas2 = CanvasLayer::new();
        cached.update_layer(Layer::Canvas(canvas2));
        assert!(cached.is_dirty());
    }

    #[test]
    fn test_cached_layer_clone() {
        let canvas = CanvasLayer::new();
        let cached1 = CachedLayer::new(Layer::Canvas(canvas));
        let cached2 = cached1.clone();

        // Both should share same dirty flag (Arc)
        cached1.mark_clean();
        assert!(!cached2.is_dirty());

        cached1.mark_dirty();
        assert!(cached2.is_dirty());
    }

    #[test]
    fn test_cached_layer_inner_mut_marks_dirty() {
        let canvas = CanvasLayer::new();
        let cached = CachedLayer::new(Layer::Canvas(canvas));

        cached.mark_clean();
        assert!(!cached.is_dirty());

        // Accessing inner_mut should mark dirty
        let _guard = cached.inner_mut();
        assert!(cached.is_dirty());
    }

    #[test]
    fn test_cached_layer_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<CachedLayer>();
        assert_sync::<CachedLayer>();
    }
}
