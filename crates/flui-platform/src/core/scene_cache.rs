//! Type-safe scene caching for hit testing
//!
//! Maintains the most recent scene for hit testing without unsafe code.

use flui_engine::Scene;
use parking_lot::RwLock;
use std::sync::Arc;

/// Type-safe scene cache for hit testing
///
/// Maintains the most recent scene for hit testing without unsafe code.
/// Uses Arc for zero-copy sharing and RwLock for thread-safe access.
///
/// # Thread Safety
///
/// The cache is fully thread-safe:
/// - Multiple readers can access the scene concurrently
/// - Updates are atomic via write lock
/// - Arc clone is cheap (ref count increment)
///
/// # Example
///
/// ```rust,ignore
/// let cache = SceneCache::new();
///
/// // After rendering
/// cache.update(scene);
///
/// // For hit testing
/// if let Some(scene) = cache.get() {
///     // perform hit testing
/// }
/// ```
#[derive(Clone)]
pub struct SceneCache {
    /// Most recent scene (shared ownership)
    current: Arc<RwLock<Option<Scene>>>,

    /// Frame number of cached scene
    frame_number: Arc<RwLock<u64>>,
}

impl SceneCache {
    /// Create a new empty scene cache
    pub fn new() -> Self {
        Self {
            current: Arc::new(RwLock::new(None)),
            frame_number: Arc::new(RwLock::new(0)),
        }
    }

    /// Update the cached scene
    ///
    /// Called after each frame render to update hit testing cache.
    /// Uses Arc::clone for zero-copy sharing.
    pub fn update(&self, scene: Scene) {
        if scene.has_content() {
            let frame_num = scene.frame_number();

            *self.current.write() = Some(scene);
            *self.frame_number.write() = frame_num;

            tracing::trace!(frame = frame_num, "Scene cached for hit testing");
        }
    }

    /// Get the current scene for hit testing
    ///
    /// Returns `None` if no scene has been rendered yet.
    pub fn get(&self) -> Option<Scene> {
        self.current.read().clone()
    }

    /// Get the frame number of the cached scene
    pub fn frame_number(&self) -> u64 {
        *self.frame_number.read()
    }

    /// Check if cache has a scene
    pub fn has_scene(&self) -> bool {
        self.current.read().is_some()
    }

    /// Clear the cache
    ///
    /// Called on low memory warnings or when releasing resources.
    pub fn clear(&self) {
        *self.current.write() = None;
        *self.frame_number.write() = 0;
        tracing::debug!("Scene cache cleared");
    }
}

impl Default for SceneCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scene_cache_new() {
        let cache = SceneCache::new();
        assert!(!cache.has_scene());
        assert_eq!(cache.frame_number(), 0);
    }

    #[test]
    fn test_scene_cache_clear() {
        let cache = SceneCache::new();
        cache.clear();
        assert!(!cache.has_scene());
    }

    #[test]
    fn test_scene_cache_clone() {
        let cache1 = SceneCache::new();
        let cache2 = cache1.clone();

        // Both should share the same underlying data
        assert_eq!(cache1.frame_number(), cache2.frame_number());
    }
}
