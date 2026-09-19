//! `Scene` — a [`LayerTree`] frozen for the raster side.

use flui_foundation::LayerId;

use crate::tree::LayerTree;

/// A composited layer tree the raster side may read but never mutate.
///
/// The paint side builds a [`LayerTree`] and freezes it here; from this point
/// there is no `&mut` path back to the tree, which is the one fact this type
/// adds over the tree itself. It is moved by value across the raster boundary
/// (inside a [`SceneSnapshot`](crate::SceneSnapshot)) and through
/// `flui-hot-reload`'s plugin ABI, whose compatibility token folds in this
/// type's size and alignment.
///
/// ```rust
/// use flui_layer::{Layer, LayerTree, OffsetLayer, Scene};
///
/// let tree = LayerTree::new(Layer::from(OffsetLayer::zero()));
/// let scene = Scene::new(tree);
///
/// assert_eq!(scene.root(), scene.tree().root());
/// assert_eq!(scene.tree().len(), 1);
/// ```
#[derive(Debug, Default)]
pub struct Scene {
    tree: LayerTree,
}

impl Scene {
    /// Freezes `tree`.
    pub fn new(tree: LayerTree) -> Self {
        Self { tree }
    }

    /// The frozen tree.
    #[inline]
    pub fn tree(&self) -> &LayerTree {
        &self.tree
    }

    /// The tree's root.
    #[inline]
    pub fn root(&self) -> LayerId {
        self.tree.root()
    }
}
