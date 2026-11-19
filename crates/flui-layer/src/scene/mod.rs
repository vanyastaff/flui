//! The scene: the [`Scene`] the raster side draws, a persistent
//! [`SceneVersion`] its [`SceneOwner`] changes by typed patches (ADR-0304).

mod content;
mod footprint;
mod from_tree;
mod geometry;
mod id;
mod inspect;
mod owner;
mod pending;
mod table;
mod version;

pub use content::{BoundaryContent, Bounds, Chunk, ContentBuilder, Entry, EntryIndex};
pub use footprint::TextureDamageScratch;
pub use id::{ContentVersion, Lineage, SceneNodeId, SceneNodeMarker, SceneVersionId};
pub use owner::{
    Applied, Placement, Reclaimed, ReleasedContents, SceneError, SceneOp, SceneOwner,
    ScenePublication, SceneWriter, SurfaceKey,
};
pub use version::{EntryAddress, SceneNode, SceneVersion, SubtreeCull};

use crate::tree::LayerTree;

/// What the raster side draws: a retained [`SceneVersion`], walked in place.
///
/// A frame's paint publishes its version; a scene authored by hand is built
/// as a [`LayerTree`] and converted when the scene is made
/// ([`Scene::new`]), so the raster side walks one form. Nothing is mutable
/// from here: there is no `&mut` path back to the version. It is moved by
/// value across the raster boundary (inside a
/// [`SceneSnapshot`](crate::SceneSnapshot)) and through `flui-hot-reload`'s
/// plugin ABI, whose compatibility token folds in this type's size and
/// alignment.
///
/// ```rust
/// use flui_layer::{Layer, LayerTree, OffsetLayer, Scene};
///
/// let scene = Scene::new(LayerTree::new(Layer::from(OffsetLayer::zero())));
/// let root = scene.version().root();
/// assert!(scene.version().node(root).is_some());
/// ```
#[derive(Debug)]
pub struct Scene {
    version: SceneVersion,
}

impl Default for Scene {
    /// A scene that draws nothing: a fresh owner's empty version.
    fn default() -> Self {
        Self::of_version(SceneOwner::new().current().clone())
    }
}

impl Scene {
    /// The scene `tree` paints, converted into a version
    /// ([`SceneVersion::from_tree`]).
    #[must_use]
    pub fn new(tree: LayerTree) -> Self {
        Self::of_version(SceneVersion::from_tree(&tree))
    }

    /// Holds `version`.
    #[must_use]
    pub fn of_version(version: SceneVersion) -> Self {
        Self { version }
    }

    /// The version it holds.
    #[inline]
    #[must_use]
    pub fn version(&self) -> &SceneVersion {
        &self.version
    }
}
