//! `SceneBuilder` — stack-based construction of a [`LayerTree`].
//!
//! The production frame is built by `flui-rendering`'s fragment composer
//! straight through [`LayerTree::new`] / [`LayerTree::push_child`];
//! this builder is the hand-authored path — `flui_app::run_direct`, examples,
//! and the GPU readback tests: `push` a container, `add` leaves under it,
//! `pop` back out.

use flui_foundation::LayerId;
use flui_types::{
    Matrix4,
    geometry::{Offset, Pixels, RRect, RSuperellipse, Rect},
    painting::{Clip, ColorFilter, ImageFilter, Path},
};

use crate::{
    layer::{
        CanvasLayer, ClipPathLayer, ClipRRectLayer, ClipRectLayer, ClipSuperellipseLayer,
        ColorFilterLayer, ImageFilterLayer, Layer, OffsetLayer, OpacityLayer, PictureLayer,
        TransformLayer,
    },
    tree::LayerTree,
};

/// Builds a [`LayerTree`] with a push/pop stack of open containers.
///
/// The builder owns the tree it grows, so one build is single-writer by
/// construction. The tree starts as a zero [`OffsetLayer`] root (or the root
/// given to [`Self::with_root`]); every layer pushed or added is appended
/// under the innermost open container, or under the root when none is open.
///
/// ```rust
/// use flui_layer::{PictureLayer, SceneBuilder};
/// use flui_types::Offset;
///
/// let mut builder = SceneBuilder::new();
/// let offset = builder.push_offset(Offset::ZERO);
/// builder.push_opacity(0.5);
/// builder.add(PictureLayer::default());
/// builder.pop();
/// let tree = builder.build();
/// assert_eq!(tree.parent(offset), Some(tree.root()));
/// ```
#[derive(Debug, Default)]
pub struct SceneBuilder {
    tree: LayerTree,
    stack: Vec<LayerId>,
}

impl SceneBuilder {
    /// A builder over a tree whose root is a zero [`OffsetLayer`].
    pub fn new() -> Self {
        Self::with_root(OffsetLayer::zero())
    }

    /// A builder over a tree rooted at `root`.
    pub fn with_root(root: impl Into<Layer>) -> Self {
        Self {
            tree: LayerTree::new(root.into()),
            stack: Vec::with_capacity(16),
        }
    }

    /// How many containers are open.
    #[inline]
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// The innermost open container, or the root when none is open.
    #[inline]
    pub fn current(&self) -> LayerId {
        self.stack
            .last()
            .copied()
            .unwrap_or_else(|| self.tree.root())
    }

    /// The tree's root.
    #[inline]
    pub fn root(&self) -> LayerId {
        self.tree.root()
    }

    /// The tree as built so far.
    #[inline]
    pub fn tree(&self) -> &LayerTree {
        &self.tree
    }

    /// Appends `layer` under the current container and leaves it closed.
    pub fn add(&mut self, layer: impl Into<Layer>) -> LayerId {
        self.tree.push_child(self.current(), layer.into())
    }

    /// Appends `layer` like [`Self::add`] and opens it as the current
    /// container until the matching [`Self::pop`].
    pub fn push(&mut self, layer: impl Into<Layer>) -> LayerId {
        let id = self.add(layer);
        self.stack.push(id);
        id
    }

    /// Closes the innermost container, returning its id; `None` when nothing
    /// is open (a `pop` without a matching `push` is a caller bug, but an
    /// empty stack is plain absence — the `Vec::pop` contract).
    pub fn pop(&mut self) -> Option<LayerId> {
        self.stack.pop()
    }

    /// Finishes the build and returns the tree.
    #[tracing::instrument(skip_all, name = "scene_build", fields(depth = self.depth()))]
    pub fn build(self) -> LayerTree {
        self.tree
    }

    // Typed conveniences over `push`/`add` for the layers hand-authored
    // scenes reach for. Each is one line; a variant without one is reached
    // through `push(SomeLayer::new(..))`.

    /// Pushes an [`OffsetLayer`].
    pub fn push_offset(&mut self, offset: Offset<Pixels>) -> LayerId {
        self.push(OffsetLayer::new(offset))
    }

    /// Pushes a [`TransformLayer`].
    pub fn push_transform(&mut self, transform: Matrix4) -> LayerId {
        self.push(TransformLayer::new(transform))
    }

    /// Pushes an [`OpacityLayer`] (`alpha` in `0.0..=1.0`).
    pub fn push_opacity(&mut self, alpha: f32) -> LayerId {
        self.push(OpacityLayer::new(alpha))
    }

    /// Pushes a [`ClipRectLayer`].
    pub fn push_clip_rect(&mut self, rect: Rect<Pixels>, clip: Clip) -> LayerId {
        self.push(ClipRectLayer::new(rect, clip))
    }

    /// Pushes a [`ClipRRectLayer`].
    pub fn push_clip_rrect(&mut self, rrect: RRect, clip: Clip) -> LayerId {
        self.push(ClipRRectLayer::new(rrect, clip))
    }

    /// Pushes a [`ClipSuperellipseLayer`].
    pub fn push_clip_superellipse(&mut self, shape: RSuperellipse, clip: Clip) -> LayerId {
        self.push(ClipSuperellipseLayer::new(shape, clip))
    }

    /// Pushes a [`ClipPathLayer`].
    pub fn push_clip_path(&mut self, path: Path, clip: Clip) -> LayerId {
        self.push(ClipPathLayer::new(path, clip))
    }

    /// Pushes a [`ColorFilterLayer`].
    pub fn push_color_filter(&mut self, filter: ColorFilter) -> LayerId {
        self.push(ColorFilterLayer::new(filter))
    }

    /// Pushes an [`ImageFilterLayer`].
    pub fn push_image_filter(&mut self, filter: ImageFilter) -> LayerId {
        self.push(ImageFilterLayer::new(filter))
    }

    /// Adds a [`CanvasLayer`] leaf.
    pub fn add_canvas(&mut self, canvas: CanvasLayer) -> LayerId {
        self.add(canvas)
    }

    /// Adds a [`PictureLayer`] leaf.
    pub fn add_picture(&mut self, picture: flui_painting::DisplayList) -> LayerId {
        self.add(PictureLayer::new(picture))
    }
}

#[cfg(test)]
mod tests {
    use flui_types::{
        geometry::{Size, px},
        painting::Clip,
    };

    use super::*;

    #[test]
    fn pushed_layers_nest_and_pop_unwinds_them() {
        let mut builder = SceneBuilder::new();
        let offset = builder.push_offset(Offset::ZERO);
        let clip = builder.push_clip_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(10.0), px(10.0)),
            Clip::HardEdge,
        );
        let leaf = builder.add_canvas(CanvasLayer::new());
        assert_eq!(builder.depth(), 2);
        assert_eq!(builder.current(), clip);
        assert_eq!(builder.pop(), Some(clip));
        assert_eq!(builder.pop(), Some(offset));
        assert_eq!(builder.pop(), None);
        assert_eq!(builder.current(), builder.root());

        let tree = builder.build();
        assert_eq!(tree.parent(offset), Some(tree.root()));
        assert_eq!(tree.parent(clip), Some(offset));
        assert_eq!(tree.parent(leaf), Some(clip));
    }

    #[test]
    fn layers_added_with_nothing_open_hang_off_the_root() {
        let mut builder = SceneBuilder::new();
        builder.push_opacity(0.5);
        builder.pop();
        let late = builder.add(PictureLayer::default());
        let tree = builder.build();
        assert_eq!(tree.parent(late), Some(tree.root()));
    }

    #[test]
    fn with_root_replaces_the_default_offset_root() {
        let builder = SceneBuilder::with_root(TransformLayer::identity());
        let tree = builder.build();
        assert_eq!(tree.len(), 1);
        assert!(matches!(
            tree.get_layer(tree.root()),
            Some(Layer::Transform(_))
        ));
    }

    #[test]
    fn push_accepts_any_layer_payload() {
        let mut builder = SceneBuilder::new();
        let _ = builder.push(TransformLayer::identity());
        let _ = builder.push(crate::LeaderLayer::new(crate::LayerLink::new(), Size::ZERO));
        assert_eq!(builder.depth(), 2);
    }
}
