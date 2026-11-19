//! A hand-built [`LayerTree`] as a retained version: how a scene a plugin,
//! an example or a GPU test authors by hand reaches the raster side, which
//! walks versions only (ADR-0304 §9).

use std::sync::Arc;

use flui_foundation::{LayerId, RenderId, SurfaceGeneration};

use super::content::{BoundaryContent, Chunk};
use super::owner::{SceneOp, SceneOwner, SurfaceKey};
use super::version::SceneVersion;
use crate::layer::Layer;
use crate::tree::LayerTree;

impl SceneVersion {
    /// The version that paints `tree`: one node holding the whole tree, each
    /// picture without children a chunk of its recording and every other
    /// layer an effect over its children, in the tree's paint order.
    ///
    /// No render tree owns a hand-built scene: a layer
    /// [`with_render`](crate::LayerNode::with_render) is filed under that
    /// render object, any other under the id numbered as its layer, which
    /// keeps the entries' pairing keys apart. The version comes from an owner
    /// of its own, so its damage pairs with no other scene's.
    ///
    /// ```rust
    /// use flui_layer::{Layer, LayerTree, OffsetLayer, PictureLayer, SceneVersion};
    ///
    /// let mut tree = LayerTree::new(Layer::from(OffsetLayer::zero()));
    /// tree.push_child(tree.root(), Layer::from(PictureLayer::default()));
    ///
    /// let version = SceneVersion::from_tree(&tree);
    /// let root = version.node(version.root()).expect("a version holds its root");
    /// assert_eq!(root.content().entries().len(), 2);
    /// ```
    #[must_use]
    pub fn from_tree(tree: &LayerTree) -> Self {
        let mut owner = SceneOwner::new();
        let mut writer = owner.writer();
        let mut content = BoundaryContent::builder();
        let mut stack = vec![Visit::Enter(tree.root())];
        while let Some(visit) = stack.pop() {
            let id = match visit {
                Visit::Enter(id) => id,
                Visit::Exit => {
                    content.pop_effect();
                    continue;
                }
            };
            let node = tree
                .get(id)
                .expect("BUG: a tree's child ids name its nodes");
            let render = node.render_id().unwrap_or_else(|| filed_under(id));
            match node.layer() {
                Layer::Picture(picture) if node.children().is_empty() => {
                    let version = writer
                        .next_content_version()
                        .expect("BUG: a fresh owner mints fewer than 2^64 - 1 recordings");
                    content.chunk(Arc::new(Chunk::of_shared(
                        render,
                        0,
                        version,
                        Arc::clone(picture.shared()),
                    )));
                }
                layer => {
                    content.push_effect(layer.clone(), render, 0);
                    stack.push(Visit::Exit);
                    stack.extend(
                        node.children()
                            .iter()
                            .rev()
                            .map(|&child| Visit::Enter(child)),
                    );
                }
            }
        }
        let root = writer.current().root();
        writer.push(SceneOp::Publish {
            node: root,
            content: Arc::new(content.finish()),
        });
        // A hand-built scene has no surface of its own; the key only pairs
        // damage, and this owner publishes once.
        let surface = SurfaceKey {
            size: (0, 0),
            generation: SurfaceGeneration::ZERO,
        };
        owner
            .apply(surface)
            .expect("BUG: a patch publishing the root of a fresh owner applies");
        owner.current().clone()
    }
}

/// One step of the conversion's walk: a layer to enter, or the close of the
/// innermost effect it opened.
enum Visit {
    Enter(LayerId),
    Exit,
}

/// The id a layer no render object painted is filed under.
fn filed_under(id: LayerId) -> RenderId {
    RenderId::new(id.get())
}
