//! Cached layer - optimized layer caching for RepaintBoundary
//!
//! This layer provides efficient caching of painted content, allowing
//! the framework to skip repainting when only the parent changes.

use crate::renderer::CommandRenderer;
use std::sync::Arc;
use parking_lot::RwLock;

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
/// ```rust,ignore
/// use flui_engine::{CachedLayer, CanvasLayer, Layer};
///
/// let canvas = CanvasLayer::new();
/// let cached = CachedLayer::new(Layer::Canvas(canvas));
///
/// // First paint - renders child
/// cached.render(&mut renderer);
///
/// // Second paint - reuses cache (if not dirty)
/// cached.render(&mut renderer);
///
/// // Mark dirty to force repaint
/// cached.mark_dirty();
/// cached.render(&mut renderer); // Repaints child
/// ```
#[derive(Debug, Clone)]
pub struct CachedLayer {
    /// The wrapped layer to cache
    inner: Arc<RwLock<super::Layer>>,

    /// Whether the cache is dirty and needs repainting
    dirty: Arc<RwLock<bool>>,
}

impl CachedLayer {
    /// Create new cached layer wrapping another layer
    pub fn new(layer: super::Layer) -> Self {
        Self {
            inner: Arc::new(RwLock::new(layer)),
            dirty: Arc::new(RwLock::new(true)), // Start dirty to force initial render
        }
    }

    /// Mark the cache as dirty, forcing a repaint on next render
    pub fn mark_dirty(&self) {
        *self.dirty.write() = true;
    }

    /// Check if the cache is dirty
    pub fn is_dirty(&self) -> bool {
        *self.dirty.read()
    }

    /// Update the wrapped layer
    ///
    /// Automatically marks the cache as dirty.
    pub fn update_layer(&self, new_layer: super::Layer) {
        *self.inner.write() = new_layer;
        self.mark_dirty();
    }

    /// Render the cached layer
    ///
    /// If dirty, renders the wrapped layer. Otherwise, reuses cached result.
    ///
    /// Note: Current implementation always renders since we don't have
    /// GPU-level caching infrastructure yet. The dirty flag is tracked
    /// for future optimization.
    pub fn render(&self, renderer: &mut dyn CommandRenderer) {
        // Note: Full caching optimization requires:
        // - GPU texture cache for rendered content
        // - Render target pooling
        // - Texture atlas management
        //
        // For now, we always render but track dirty state for when
        // the infrastructure is available.

        let inner = self.inner.read();
        inner.render(renderer);

        // After rendering, mark as clean
        *self.dirty.write() = false;
    }

    /// Get a reference to the wrapped layer
    pub fn inner(&self) -> parking_lot::RwLockReadGuard<'_, super::Layer> {
        self.inner.read()
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

        // Clear dirty flag manually for testing
        *cached.dirty.write() = false;
        assert!(!cached.is_dirty());

        cached.mark_dirty();
        assert!(cached.is_dirty());
    }

    #[test]
    fn test_cached_layer_update_layer() {
        let canvas1 = CanvasLayer::new();
        let cached = CachedLayer::new(Layer::Canvas(canvas1));

        // Clear dirty flag
        *cached.dirty.write() = false;
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

        // Both should share same dirty flag
        cached1.mark_dirty();
        assert!(cached2.is_dirty());
    }
}
