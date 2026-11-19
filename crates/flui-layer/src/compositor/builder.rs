//! `SceneBuilder`: a [`Scene`] authored by hand with a push/pop stack.
//!
//! A frame is not built here: `flui-rendering`'s paint writes the retained
//! scene through a [`SceneWriter`](crate::SceneWriter). This builder is the
//! hand-authored path, for hot-reload plugins, examples and the GPU readback
//! tests: `push` a container, `add` leaves under it, `pop` back out, and
//! `build` the scene, as `dart:ui`'s `SceneBuilder` does.

use flui_foundation::LayerId;
use flui_foundation::geometry::{Matrix4, Offset, RRect, RSuperellipse, Rect};
use flui_painting::paint::{Clip, ColorFilter, ImageFilter, Path};

use crate::{
    Scene,
    layer::{
        ClipPathLayer, ClipRRectLayer, ClipRectLayer, ClipSuperellipseLayer, ColorFilterLayer,
        ImageFilterLayer, Layer, OffsetLayer, OpacityLayer, PictureLayer, TransformLayer,
    },
    tree::LayerTree,
};

/// Builds a [`Scene`] with a push/pop stack of open containers.
///
/// The scene's root is a zero [`OffsetLayer`]; every layer pushed or added
/// goes under the innermost open container, or under the root when none is
/// open. [`Self::build`] makes the layers a version ([`Scene::new`]).
///
/// ```rust
/// use flui_layer::{PictureLayer, SceneBuilder};
/// use flui_foundation::geometry::Offset;
///
/// let mut builder = SceneBuilder::new();
/// builder
///     .push_offset(Offset::new(10.0, 0.0))
///     .push_opacity(0.5)
///     .add(PictureLayer::default())
///     .pop()
///     .pop();
/// let scene = builder.build();
/// assert_eq!(scene.version().len(), 1);
/// ```
#[derive(Debug, Default)]
pub struct SceneBuilder {
    tree: LayerTree,
    stack: Vec<LayerId>,
}

impl SceneBuilder {
    /// A builder with nothing under its root.
    pub fn new() -> Self {
        Self {
            tree: LayerTree::new(Layer::from(OffsetLayer::zero())),
            stack: Vec::with_capacity(16),
        }
    }

    /// The innermost open container, or the root when none is open.
    fn current(&self) -> LayerId {
        self.stack
            .last()
            .copied()
            .unwrap_or_else(|| self.tree.root())
    }

    /// Appends `layer` under the current container and leaves it closed.
    pub fn add(&mut self, layer: impl Into<Layer>) -> &mut Self {
        self.tree.push_child(self.current(), layer.into());
        self
    }

    /// Appends `layer` like [`Self::add`] and opens it as the current
    /// container until the matching [`Self::pop`].
    pub fn push(&mut self, layer: impl Into<Layer>) -> &mut Self {
        let id = self.tree.push_child(self.current(), layer.into());
        self.stack.push(id);
        self
    }

    /// Closes the innermost container. A `pop` with nothing open is a
    /// caller's bug, which a debug build asserts and a release build
    /// ignores.
    pub fn pop(&mut self) -> &mut Self {
        let closed = self.stack.pop();
        debug_assert!(
            closed.is_some(),
            "SceneBuilder::pop without an open container"
        );
        self
    }

    /// The scene the layers make, whatever containers are still open.
    #[must_use]
    #[tracing::instrument(skip_all, name = "scene_build", fields(open = self.stack.len()))]
    pub fn build(self) -> Scene {
        Scene::new(self.tree)
    }

    // Typed conveniences over `push`/`add` for the layers hand-authored
    // scenes reach for. Each is one line; a variant without one is reached
    // through `push(SomeLayer::new(..))`.

    /// Pushes an [`OffsetLayer`].
    pub fn push_offset(&mut self, offset: Offset<f64>) -> &mut Self {
        self.push(OffsetLayer::new(offset))
    }

    /// Pushes a [`TransformLayer`].
    pub fn push_transform(&mut self, transform: Matrix4) -> &mut Self {
        self.push(TransformLayer::new(transform))
    }

    /// Pushes an [`OpacityLayer`] (`alpha` in `0.0..=1.0`).
    pub fn push_opacity(&mut self, alpha: f64) -> &mut Self {
        self.push(OpacityLayer::new(alpha))
    }

    /// Pushes a [`ClipRectLayer`].
    pub fn push_clip_rect(&mut self, rect: Rect<f64>, clip: Clip) -> &mut Self {
        self.push(ClipRectLayer::new(rect, clip))
    }

    /// Pushes a [`ClipRRectLayer`].
    pub fn push_clip_rrect(&mut self, rrect: RRect, clip: Clip) -> &mut Self {
        self.push(ClipRRectLayer::new(rrect, clip))
    }

    /// Pushes a [`ClipSuperellipseLayer`].
    pub fn push_clip_superellipse(&mut self, shape: RSuperellipse, clip: Clip) -> &mut Self {
        self.push(ClipSuperellipseLayer::new(shape, clip))
    }

    /// Pushes a [`ClipPathLayer`].
    pub fn push_clip_path(&mut self, path: Path, clip: Clip) -> &mut Self {
        self.push(ClipPathLayer::new(path, clip))
    }

    /// Pushes a [`ColorFilterLayer`].
    pub fn push_color_filter(&mut self, filter: ColorFilter) -> &mut Self {
        self.push(ColorFilterLayer::new(filter))
    }

    /// Pushes an [`ImageFilterLayer`].
    pub fn push_image_filter(&mut self, filter: ImageFilter) -> &mut Self {
        self.push(ImageFilterLayer::new(filter))
    }

    /// Adds a [`PictureLayer`] leaf.
    pub fn add_picture(&mut self, picture: flui_painting::DisplayList) -> &mut Self {
        self.add(PictureLayer::new(picture))
    }
}

#[cfg(test)]
mod tests {
    use flui_painting::paint::Clip;

    use super::*;
    use crate::testing::inspect::structure_with_depth;

    /// A push nests what follows under it until its pop, and the scene is
    /// the one `Scene::new` makes of the same tree built by hand.
    #[test]
    fn pushed_layers_nest_and_pop_unwinds_them() {
        let clip = Rect::from_xywh(0.0, 0.0, 10.0, 10.0);
        let mut builder = SceneBuilder::new();
        builder
            .push_offset(Offset::new(10.0, 0.0))
            .push_clip_rect(clip, Clip::HardEdge)
            .add_picture(flui_painting::DisplayList::new())
            .pop()
            .pop()
            .add_picture(flui_painting::DisplayList::new());

        let mut tree = LayerTree::new(Layer::from(OffsetLayer::zero()));
        let root = tree.root();
        let offset = tree.push_child(root, Layer::from(OffsetLayer::new(Offset::new(10.0, 0.0))));
        let clipped = tree.push_child(
            offset,
            Layer::from(ClipRectLayer::new(clip, Clip::HardEdge)),
        );
        tree.push_child(clipped, Layer::from(PictureLayer::default()));
        tree.push_child(root, Layer::from(PictureLayer::default()));

        assert_eq!(
            structure_with_depth(&builder.build().version().flatten()),
            structure_with_depth(&Scene::new(tree).version().flatten()),
        );
    }
}
