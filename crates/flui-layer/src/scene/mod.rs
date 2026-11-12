//! The scene: the frozen per-frame [`Scene`], and the retained scene that
//! replaces it (ADR-0304): a persistent [`SceneVersion`] its [`SceneOwner`]
//! changes by typed patches.

mod content;
mod footprint;
mod geometry;
mod id;
mod owner;
mod pending;
mod table;
mod version;

pub use content::{BoundaryContent, Bounds, Chunk, ContentBuilder, Entry, EntryIndex};
pub use id::{ContentVersion, Lineage, SceneNodeId, SceneNodeMarker, SceneVersionId};
pub use owner::{
    Applied, Placement, ReleasedContents, SceneError, SceneOp, SceneOwner, ScenePublication,
    SceneWriter, SurfaceKey,
};
pub use version::{EntryAddress, SceneNode, SceneVersion, SubtreeCull};

use crate::tree::LayerTree;

/// What the raster side draws: a retained [`SceneVersion`] it walks in
/// place, or a composited layer tree, which a plugin image, a hand-built
/// test scene and a headless capture still hand it.
///
/// Neither is mutable from here: there is no `&mut` path back to the tree
/// or the version. It is moved by value across the raster boundary (inside a
/// [`SceneSnapshot`](crate::SceneSnapshot)) and through `flui-hot-reload`'s
/// plugin ABI, whose compatibility token folds in this type's size and
/// alignment.
///
/// ```rust
/// use flui_layer::{Layer, LayerTree, OffsetLayer, Scene, SceneContent};
///
/// let tree = LayerTree::new(Layer::from(OffsetLayer::zero()));
/// let scene = Scene::new(tree);
///
/// let SceneContent::Tree(tree) = scene.content() else {
///     unreachable!("a scene built from a tree holds it");
/// };
/// assert_eq!(tree.len(), 1);
/// ```
#[derive(Debug)]
pub struct Scene {
    content: Held,
}

#[derive(Debug)]
enum Held {
    Tree(LayerTree),
    Version(SceneVersion),
}

/// What a [`Scene`] holds, borrowed.
#[derive(Debug, Clone, Copy)]
pub enum SceneContent<'a> {
    /// A composited layer tree, walked from its root.
    Tree(&'a LayerTree),
    /// A retained version, walked from its root node (ADR-0304 §9).
    Version(&'a SceneVersion),
}

impl Default for Scene {
    fn default() -> Self {
        Self::new(LayerTree::default())
    }
}

impl Scene {
    /// Freezes `tree`.
    pub fn new(tree: LayerTree) -> Self {
        Self {
            content: Held::Tree(tree),
        }
    }

    /// Holds `version`, which the raster side walks without building a tree.
    #[must_use]
    pub fn of_version(version: SceneVersion) -> Self {
        Self {
            content: Held::Version(version),
        }
    }

    /// What it holds.
    #[inline]
    #[must_use]
    pub fn content(&self) -> SceneContent<'_> {
        match &self.content {
            Held::Tree(tree) => SceneContent::Tree(tree),
            Held::Version(version) => SceneContent::Version(version),
        }
    }

    /// The tree it holds, `None` for a version.
    #[inline]
    #[must_use]
    pub fn tree(&self) -> Option<&LayerTree> {
        match &self.content {
            Held::Tree(tree) => Some(tree),
            Held::Version(_) => None,
        }
    }

    /// The version it holds, `None` for a tree.
    #[inline]
    #[must_use]
    pub fn version(&self) -> Option<&SceneVersion> {
        match &self.content {
            Held::Tree(_) => None,
            Held::Version(version) => Some(version),
        }
    }
}
