//! Scene graph management
//!
//! A Scene represents a complete frame of rendering. It contains a root layer
//! and metadata about the scene that can be used for optimization and debugging.

use crate::layer::{Layer, ContainerLayer, BoxedLayer};
use crate::painter::Painter;
use flui_types::{Rect, Size};

/// A complete rendering scene
///
/// The Scene is the top-level container for a frame of rendering. It contains
/// a root layer (typically a ContainerLayer) and provides methods for building
/// and manipulating the scene graph.
///
/// # Example
///
/// ```rust,ignore
/// let mut scene = Scene::new(Size::new(800.0, 600.0));
///
/// // Add layers to the scene
/// scene.add_layer(Box::new(background_layer));
/// scene.add_layer(Box::new(content_layer));
/// scene.add_layer(Box::new(overlay_layer));
///
/// // Paint the scene
/// scene.paint(&mut painter);
/// ```
pub struct Scene {
    /// Root layer (typically a ContainerLayer)
    root: ContainerLayer,

    /// Viewport size
    viewport_size: Size,

    /// Scene metadata (for debugging and optimization)
    metadata: SceneMetadata,
}

/// Metadata about the scene
#[derive(Debug, Clone, Default)]
pub struct SceneMetadata {
    /// Total number of layers in the scene
    pub layer_count: usize,

    /// Scene bounds (union of all layer bounds)
    pub bounds: Rect,

    /// Whether the scene needs repaint
    pub needs_repaint: bool,

    /// Frame number (for debugging)
    pub frame_number: u64,
}

impl Scene {
    /// Create a new empty scene
    ///
    /// # Arguments
    /// * `viewport_size` - The size of the viewport to render into
    pub fn new(viewport_size: Size) -> Self {
        Self {
            root: ContainerLayer::new(),
            viewport_size,
            metadata: SceneMetadata::default(),
        }
    }

    /// Create a scene from an existing root layer
    ///
    /// This is primarily used by SceneBuilder to construct a scene
    /// from a built layer tree.
    ///
    /// # Arguments
    /// * `root` - The root container layer
    /// * `viewport_size` - The size of the viewport to render into
    pub fn from_root(root: ContainerLayer, viewport_size: Size) -> Self {
        let mut scene = Self {
            root,
            viewport_size,
            metadata: SceneMetadata::default(),
        };
        scene.update_metadata();
        scene
    }

    /// Add a layer to the scene
    ///
    /// Layers are added to the root container in the order they are added.
    /// Later layers are painted on top of earlier layers.
    pub fn add_layer(&mut self, layer: BoxedLayer) {
        self.root.add_child(layer);
        self.update_metadata();
    }

    /// Get the root layer
    pub fn root(&self) -> &ContainerLayer {
        &self.root
    }

    /// Get mutable access to the root layer
    pub fn root_mut(&mut self) -> &mut ContainerLayer {
        &mut self.root
    }

    /// Get the viewport size
    pub fn viewport_size(&self) -> Size {
        self.viewport_size
    }

    /// Set the viewport size
    pub fn set_viewport_size(&mut self, size: Size) {
        self.viewport_size = size;
    }

    /// Get scene metadata
    pub fn metadata(&self) -> &SceneMetadata {
        &self.metadata
    }

    /// Paint the entire scene
    ///
    /// This paints all layers in the scene to the given painter.
    pub fn paint(&self, painter: &mut dyn Painter) {
        self.root.paint(painter);
    }

    /// Clear all layers from the scene
    pub fn clear(&mut self) {
        self.root = ContainerLayer::new();
        self.update_metadata();
    }

    /// Update scene metadata based on current layer tree
    fn update_metadata(&mut self) {
        self.metadata.layer_count = self.count_layers();
        self.metadata.bounds = self.root.bounds();
        self.metadata.needs_repaint = true;
    }

    /// Count total number of layers in the scene (including nested layers)
    fn count_layers(&self) -> usize {
        // For now, just count top-level layers
        // A full implementation would require Layer to expose child count
        // or use a visitor pattern
        self.root.children().len()
    }

    /// Increment frame number
    pub fn next_frame(&mut self) {
        self.metadata.frame_number += 1;
        self.metadata.needs_repaint = false;
    }

    /// Get the bounds of all visible content in the scene
    pub fn content_bounds(&self) -> Rect {
        self.root.bounds()
    }

    /// Check if the scene is empty (has no layers)
    pub fn is_empty(&self) -> bool {
        self.root.children().is_empty()
    }

    /// Get the number of top-level layers
    pub fn layer_count(&self) -> usize {
        self.root.children().len()
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
    use crate::layer::PictureLayer;

    #[test]
    fn test_scene_creation() {
        let scene = Scene::new(Size::new(800.0, 600.0));
        assert_eq!(scene.viewport_size(), Size::new(800.0, 600.0));
        assert!(scene.is_empty());
    }

    #[test]
    fn test_add_layer() {
        let mut scene = Scene::new(Size::new(800.0, 600.0));
        let layer = Box::new(PictureLayer::new());

        scene.add_layer(layer);
        assert_eq!(scene.layer_count(), 1);
        assert!(!scene.is_empty());
    }

    #[test]
    fn test_clear_scene() {
        let mut scene = Scene::new(Size::new(800.0, 600.0));
        scene.add_layer(Box::new(PictureLayer::new()));

        scene.clear();
        assert!(scene.is_empty());
        assert_eq!(scene.layer_count(), 0);
    }
}
