//! Scene - Immutable rendering snapshot
//!
//! Represents a complete frame ready for GPU rendering.
//! Created by the rendering pipeline and consumed by platform embedders.
//!
//! # Architecture
//!
//! ```text
//! PipelineOwner::build_frame()
//!     ↓ (creates)
//! Scene (immutable snapshot)
//!     ├─ Size (viewport dimensions)
//!     ├─ Arc<CanvasLayer> (shared for hit testing)
//!     └─ frame_number (for debugging)
//!     ↓ (consumed by)
//! WgpuEmbedder::render_frame()
//!     ↓ (calls)
//! Scene::render(renderer, view, encoder)
//! ```
//!
//! # Ownership Model
//!
//! - Scene owns the layer tree via `Arc<CanvasLayer>`
//! - Arc enables zero-copy sharing between rendering and hit testing
//! - Scene is immutable after creation (no interior mutability)
//! - Thread-safe for multi-threaded rendering

use crate::layer::CanvasLayer;
use flui_types::Size;
use std::sync::Arc;

/// Scene - Immutable rendering snapshot
///
/// Represents a complete frame ready for GPU rendering.
/// Contains the root layer tree and viewport dimensions.
///
/// # Example
///
/// ```rust,ignore
/// use flui_engine::Scene;
/// use flui_types::Size;
///
/// // Create scene from pipeline
/// let scene = Scene::with_layer(
///     Size::new(800.0, 600.0),
///     Arc::new(canvas_layer),
///     frame_number,
/// );
///
/// // Render to GPU
/// let mut renderer = WgpuRenderer::new(painter);
/// scene.render(&mut renderer, &view, &mut encoder)?;
///
/// // Share layer for hit testing (Arc clone is cheap!)
/// if let Some(layer) = scene.root_layer() {
///     hit_test(position, layer);
/// }
/// ```
#[derive(Clone)]
pub struct Scene {
    /// Scene size (viewport dimensions)
    size: Size,

    /// Root layer (shared via Arc for hit testing)
    ///
    /// Arc enables:
    /// - Zero-copy sharing between rendering and hit testing
    /// - Thread-safe access from multiple threads
    /// - Automatic cleanup when all references dropped
    root_layer: Option<Arc<CanvasLayer>>,

    /// Frame number (for debugging and profiling)
    frame_number: u64,
}

impl Scene {
    /// Create a new empty scene
    ///
    /// Used when there's no content to render (e.g., no root element).
    ///
    /// # Arguments
    ///
    /// * `size` - Viewport dimensions
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let scene = Scene::new(Size::new(800.0, 600.0));
    /// assert!(scene.root_layer().is_none());
    /// ```
    pub fn new(size: Size) -> Self {
        Self {
            size,
            root_layer: None,
            frame_number: 0,
        }
    }

    /// Create scene with root layer
    ///
    /// This is the primary constructor used by the rendering pipeline.
    ///
    /// # Arguments
    ///
    /// * `size` - Viewport dimensions
    /// * `layer` - Root canvas layer (Arc-wrapped for sharing)
    /// * `frame_number` - Frame counter for debugging
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let layer = Arc::new(CanvasLayer::from_canvas(canvas));
    /// let scene = Scene::with_layer(
    ///     Size::new(800.0, 600.0),
    ///     layer,
    ///     42,
    /// );
    /// ```
    pub fn with_layer(size: Size, layer: Arc<CanvasLayer>, frame_number: u64) -> Self {
        Self {
            size,
            root_layer: Some(layer),
            frame_number,
        }
    }

    /// Get scene size (viewport dimensions)
    ///
    /// # Returns
    ///
    /// The viewport size this scene was rendered for
    #[must_use]
    pub fn size(&self) -> Size {
        self.size
    }

    /// Get root layer (shared reference for hit testing)
    ///
    /// Returns a reference to the Arc-wrapped layer, allowing:
    /// - Zero-copy sharing via Arc::clone()
    /// - Concurrent access from rendering and hit testing
    /// - Safe lifetime management
    ///
    /// # Returns
    ///
    /// `Some(&Arc<CanvasLayer>)` if scene has content, `None` otherwise
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(layer) = scene.root_layer() {
    ///     // Clone Arc for hit testing (cheap!)
    ///     let layer_for_hit_test = Arc::clone(layer);
    ///
    ///     // Or use reference directly
    ///     layer.render(&mut renderer);
    /// }
    /// ```
    #[must_use]
    pub fn root_layer(&self) -> Option<&Arc<CanvasLayer>> {
        self.root_layer.as_ref()
    }

    /// Get frame number
    ///
    /// Useful for debugging, profiling, and frame skipping detection.
    ///
    /// # Returns
    ///
    /// The frame number when this scene was created
    #[must_use]
    pub fn frame_number(&self) -> u64 {
        self.frame_number
    }

    /// Check if scene has content to render
    ///
    /// # Returns
    ///
    /// `true` if scene has a root layer, `false` otherwise
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if scene.has_content() {
    ///     // Render scene
    /// } else {
    ///     // Just clear screen
    /// }
    /// ```
    #[must_use]
    pub fn has_content(&self) -> bool {
        self.root_layer.is_some()
    }

    /// Take the root layer out of the scene
    ///
    /// This is useful when you need ownership of the layer
    /// (e.g., for caching or triple buffering).
    ///
    /// After calling this, the scene will have no content.
    ///
    /// # Returns
    ///
    /// `Some(Arc<CanvasLayer>)` if scene had content, `None` otherwise
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut scene = Scene::with_layer(size, layer, 1);
    /// let layer = scene.take_layer();
    /// assert!(!scene.has_content());
    /// ```
    pub fn take_layer(&mut self) -> Option<Arc<CanvasLayer>> {
        self.root_layer.take()
    }
}

impl Default for Scene {
    fn default() -> Self {
        Self::new(Size::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::CanvasLayer;
    use flui_types::Size;

    #[test]
    fn test_empty_scene() {
        let scene = Scene::new(Size::new(800.0, 600.0));
        assert_eq!(scene.size(), Size::new(800.0, 600.0));
        assert!(!scene.has_content());
        assert!(scene.root_layer().is_none());
    }

    #[test]
    fn test_scene_with_layer() {
        let layer = Arc::new(CanvasLayer::new());
        let scene = Scene::with_layer(Size::new(1920.0, 1080.0), layer.clone(), 42);

        assert_eq!(scene.size(), Size::new(1920.0, 1080.0));
        assert!(scene.has_content());
        assert_eq!(scene.frame_number(), 42);
        assert!(scene.root_layer().is_some());
    }

    #[test]
    fn test_layer_sharing() {
        let layer = Arc::new(CanvasLayer::new());
        let scene = Scene::with_layer(Size::new(800.0, 600.0), layer.clone(), 1);

        // Arc::clone is cheap (just increments refcount)
        if let Some(layer_ref) = scene.root_layer() {
            let _cloned = Arc::clone(layer_ref);
            // Both references point to same layer
            assert_eq!(Arc::strong_count(layer_ref), 3); // original + scene + cloned
        }
    }

    #[test]
    fn test_take_layer() {
        let layer = Arc::new(CanvasLayer::new());
        let mut scene = Scene::with_layer(Size::new(800.0, 600.0), layer, 1);

        assert!(scene.has_content());
        let taken = scene.take_layer();
        assert!(taken.is_some());
        assert!(!scene.has_content());
    }

    #[test]
    fn test_default() {
        let scene = Scene::default();
        assert_eq!(scene.size(), Size::ZERO);
        assert!(!scene.has_content());
    }
}
