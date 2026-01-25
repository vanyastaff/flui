//! Scene - Composited layer tree ready for rendering
//!
//! A Scene represents a fully composed layer tree that can be rendered
//! to the screen. It's created by `SceneBuilder::build()` and consumed
//! by the rendering engine.
//!
//! # Architecture
//!
//! ```text
//! RenderObject.paint()
//!     │
//!     ▼
//! SceneBuilder (push/pop layers)
//!     │
//!     ▼
//! Scene (owns LayerTree, ready to render)
//!     │
//!     ▼
//! Engine.render(&scene)
//! ```
//!
//! # Example
//!
//! ```rust
//! use flui_layer::{Scene, LayerTree, CanvasLayer, Layer};
//! use flui_types::Size;
//!
//! // Create scene from a single layer
//! let scene = Scene::from_layer(
//!     Size::new(800.0, 600.0),
//!     Layer::Canvas(CanvasLayer::new()),
//!     0,
//! );
//!
//! assert!(scene.has_content());
//! assert_eq!(scene.layer_count(), 1);
//! ```

use flui_foundation::LayerId;

use crate::layer::Layer;
use crate::link_registry::LinkRegistry;
use crate::tree::LayerTree;
use flui_types::{Pixels, Size};

// ============================================================================
// SCENE
// ============================================================================

/// An opaque object representing a composited scene.
///
/// Scene owns the `LayerTree` and contains all the information
/// needed to render the composed layer tree to the screen.
///
/// # Lifecycle
///
/// 1. Create layers with `SceneBuilder`
/// 2. Call `build_scene()` to get a Scene (takes ownership of LayerTree)
/// 3. Pass Scene to the rendering engine
/// 4. Scene is consumed/disposed after rendering
///
/// # Thread Safety
///
/// Scene is `Send` - it can be transferred to the render thread.
#[derive(Debug)]
pub struct Scene {
    /// Viewport size
    size: Size<Pixels>,

    /// Layer tree containing all layers
    layer_tree: LayerTree,

    /// Root layer ID (if scene has content)
    root: Option<LayerId>,

    /// Leader-follower link registry for this scene
    link_registry: LinkRegistry,

    /// Frame number for debugging
    frame_number: u64,
}

impl Scene {
    /// Creates a new empty Scene.
    pub fn empty(size: Size<Pixels>) -> Self {
        Self {
            size,
            layer_tree: LayerTree::new(),
            root: None,
            link_registry: LinkRegistry::new(),
            frame_number: 0,
        }
    }

    /// Creates a new Scene with LayerTree and root.
    pub fn new(
        size: Size<Pixels>,
        layer_tree: LayerTree,
        root: Option<LayerId>,
        frame_number: u64,
    ) -> Self {
        Self {
            size,
            layer_tree,
            root,
            link_registry: LinkRegistry::new(),
            frame_number,
        }
    }

    /// Creates a Scene with full metadata.
    pub fn with_links(
        size: Size<Pixels>,
        layer_tree: LayerTree,
        root: Option<LayerId>,
        link_registry: LinkRegistry,
        frame_number: u64,
    ) -> Self {
        Self {
            size,
            layer_tree,
            root,
            link_registry,
            frame_number,
        }
    }

    /// Creates a Scene from a single layer (convenience method).
    ///
    /// Creates a LayerTree with a single layer as root.
    pub fn from_layer(size: Size<Pixels>, layer: Layer, frame_number: u64) -> Self {
        let mut tree = LayerTree::new();
        let root_id = tree.insert(layer);
        Self {
            size,
            layer_tree: tree,
            root: Some(root_id),
            link_registry: LinkRegistry::new(),
            frame_number,
        }
    }

    /// Returns the viewport size.
    #[inline]
    pub fn size(&self) -> Size<Pixels> {
        self.size
    }

    /// Returns the layer tree.
    #[inline]
    pub fn layer_tree(&self) -> &LayerTree {
        &self.layer_tree
    }

    /// Returns mutable layer tree (for rendering traversal).
    #[inline]
    pub fn layer_tree_mut(&mut self) -> &mut LayerTree {
        &mut self.layer_tree
    }

    /// Returns the root layer ID of the scene.
    #[inline]
    pub fn root(&self) -> Option<LayerId> {
        self.root
    }

    /// Returns the root layer (if present).
    #[inline]
    pub fn root_layer(&self) -> Option<&Layer> {
        self.root.and_then(|id| self.layer_tree.get_layer(id))
    }

    /// Returns the link registry for leader-follower relationships.
    #[inline]
    pub fn link_registry(&self) -> &LinkRegistry {
        &self.link_registry
    }

    /// Returns the frame number.
    #[inline]
    pub fn frame_number(&self) -> u64 {
        self.frame_number
    }

    /// Returns true if the scene has content (has root layer).
    #[inline]
    pub fn has_content(&self) -> bool {
        self.root.is_some()
    }

    /// Returns true if the scene is empty (no root layer).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    /// Returns the number of layers in the scene.
    #[inline]
    pub fn layer_count(&self) -> usize {
        self.layer_tree.len()
    }

    /// Disposes the scene, releasing any resources.
    ///
    /// After calling dispose, the scene should not be used.
    pub fn dispose(self) {
        // Scene is consumed, resources released via Drop
        drop(self);
    }
}

impl Default for Scene {
    fn default() -> Self {
        Self::empty(Size::ZERO)
    }
}

// Scene can be sent to render thread
unsafe impl Send for Scene {}

// ============================================================================
// SCENE BUILDER INTEGRATION
// ============================================================================

impl crate::compositor::SceneBuilder<'_> {
    /// Finishes building and returns a Scene ready for rendering.
    ///
    /// **Note:** This method only captures the root LayerId. The LayerTree
    /// must be passed separately or use `build_scene_owned()`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_layer::{LayerTree, SceneBuilder, CanvasLayer, Scene};
    /// use flui_types::Size;
    ///
    /// let mut tree = LayerTree::new();
    /// {
    ///     let mut builder = SceneBuilder::new(&mut tree);
    ///     let _ = builder.add_canvas(CanvasLayer::new());
    ///     // builder dropped here, tree still available
    /// }
    ///
    /// // Create scene with the tree
    /// let root_id = tree.root();
    /// let scene = Scene::new(Size::new(800.0, 600.0), tree, root_id, 0);
    /// assert!(scene.has_content());
    /// ```
    pub fn finish(self) -> Option<LayerId> {
        self.root()
    }

    /// Finishes building with frame number metadata.
    pub fn finish_with_frame(self, _frame_number: u64) -> Option<LayerId> {
        self.root()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CanvasLayer;
    use flui_types::Offset;

    #[test]
    fn test_scene_empty() {
        let scene = Scene::empty(Size::new(800.0, 600.0));
        assert!(scene.is_empty());
        assert!(!scene.has_content());
        assert!(scene.root().is_none());
        assert!(scene.root_layer().is_none());
        assert_eq!(scene.layer_count(), 0);
        assert_eq!(scene.size(), Size::new(800.0, 600.0));
    }

    #[test]
    fn test_scene_from_layer() {
        let scene = Scene::from_layer(
            Size::new(1920.0, 1080.0),
            Layer::Canvas(CanvasLayer::new()),
            42,
        );

        assert!(!scene.is_empty());
        assert!(scene.has_content());
        assert!(scene.root().is_some());
        assert!(scene.root_layer().is_some());
        assert_eq!(scene.layer_count(), 1);
        assert_eq!(scene.frame_number(), 42);
        assert_eq!(scene.size(), Size::new(1920.0, 1080.0));
    }

    #[test]
    fn test_scene_new_with_tree() {
        let mut tree = LayerTree::new();
        let root_id = tree.insert(Layer::Canvas(CanvasLayer::new()));

        let scene = Scene::new(Size::new(800.0, 600.0), tree, Some(root_id), 1);

        assert!(scene.has_content());
        assert_eq!(scene.root(), Some(root_id));
        assert_eq!(scene.layer_count(), 1);
    }

    #[test]
    fn test_scene_with_links() {
        let mut tree = LayerTree::new();
        let root_id = tree.insert(Layer::Canvas(CanvasLayer::new()));

        let mut registry = LinkRegistry::new();
        let link = crate::LayerLink::new();
        registry.register_leader(link, root_id, Offset::ZERO, Size::new(100.0, 50.0));

        let scene = Scene::with_links(Size::new(800.0, 600.0), tree, Some(root_id), registry, 123);

        assert_eq!(scene.link_registry().leader_count(), 1);
        assert_eq!(scene.frame_number(), 123);
    }

    #[test]
    fn test_scene_default() {
        let scene = Scene::default();
        assert!(scene.is_empty());
        assert_eq!(scene.size(), Size::ZERO);
    }

    #[test]
    fn test_scene_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Scene>();
    }

    #[test]
    fn test_scene_dispose() {
        let scene = Scene::from_layer(
            Size::new(800.0, 600.0),
            Layer::Canvas(CanvasLayer::new()),
            0,
        );
        scene.dispose();
        // Scene is consumed, no further use possible
    }

    #[test]
    fn test_scene_builder_finish() {
        let mut tree = LayerTree::new();
        let root_id = {
            let mut builder = crate::SceneBuilder::new(&mut tree);
            let _ = builder.add_canvas(CanvasLayer::new());
            builder.finish()
        };

        assert!(root_id.is_some());

        // Now create scene with the tree
        let scene = Scene::new(Size::new(800.0, 600.0), tree, root_id, 0);
        assert!(scene.has_content());
    }
}
