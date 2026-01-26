//! Scene compositor for building layer hierarchies
//!
//! This module provides a stack-based compositor API similar to Flutter's SceneBuilder.
//! It allows building complex layer trees using push/pop operations.
//!
//! # Architecture
//!
//! The compositor maintains a stack of layer IDs. Each `push_*` operation creates
//! a new layer and pushes it onto the stack. Content is added to the current
//! top-of-stack layer. `pop()` removes the top layer from the stack.
//!
//! # Example
//!
//! ```rust
//! use flui_layer::{LayerTree, SceneBuilder, OpacityLayer, CanvasLayer, OffsetLayer};
//! use flui_types::Offset;
//!
//! let mut tree = LayerTree::new();
//! let mut builder = SceneBuilder::new(&mut tree);
//!
//! // Push an offset layer
//! builder.push_offset(Offset::new(10.0, 20.0));
//!
//! // Push opacity layer as child
//! builder.push_opacity(0.5);
//!
//! // Add canvas content
//! let canvas = CanvasLayer::new();
//! builder.add_canvas(canvas);
//!
//! // Pop back up
//! builder.pop();
//! builder.pop();
//!
//! // Build returns the root layer ID
//! let root_id = builder.build();
//! ```

use flui_foundation::LayerId;
use flui_types::geometry::{Pixels, RRect, Rect};
use flui_types::painting::{
    effects::ColorMatrix, BlendMode, Clip, FilterQuality, ImageFilter, Path, ShaderSpec, TextureId,
};
use flui_types::Matrix4;

use crate::layer::{
    BackdropFilterLayer, CanvasLayer, ClipPathLayer, ClipRRectLayer, ClipRectLayer,
    ColorFilterLayer, ImageFilterLayer, Layer, OffsetLayer, OpacityLayer, ShaderMaskLayer,
    TextureLayer, TransformLayer,
};
use crate::tree::LayerTree;

// ============================================================================
// SCENE BUILDER
// ============================================================================

/// A stack-based scene builder for constructing layer hierarchies.
///
/// SceneBuilder provides a Flutter-style API for building complex layer trees
/// using push/pop operations. Each push operation creates a new layer and
/// makes it the current parent for subsequent operations.
///
/// # Design
///
/// - **Stack-based**: Maintains a stack of LayerIds for nested operations
/// - **Automatic parenting**: Children are automatically added to current parent
/// - **Type-safe**: Each push method creates the appropriate layer type
///
/// # Example
///
/// ```rust
/// use flui_layer::{LayerTree, SceneBuilder, CanvasLayer};
/// use flui_types::Offset;
///
/// let mut tree = LayerTree::new();
/// let mut builder = SceneBuilder::new(&mut tree);
///
/// // Build a simple scene
/// builder.push_offset(Offset::new(100.0, 50.0));
/// builder.push_opacity(0.8);
/// builder.add_canvas(CanvasLayer::new());
/// builder.pop();
/// builder.pop();
///
/// let root = builder.build();
/// assert!(root.is_some());
/// ```
pub struct SceneBuilder<'a> {
    /// Reference to the layer tree being built
    tree: &'a mut LayerTree,

    /// Stack of current layer IDs (parent chain)
    stack: Vec<LayerId>,

    /// The root layer ID (first pushed layer)
    root: Option<LayerId>,
}

impl<'a> SceneBuilder<'a> {
    /// Creates a new SceneBuilder for the given LayerTree.
    pub fn new(tree: &'a mut LayerTree) -> Self {
        Self {
            tree,
            stack: Vec::with_capacity(16),
            root: None,
        }
    }

    /// Returns the current depth of the layer stack.
    #[inline]
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Returns the current parent layer ID (top of stack).
    #[inline]
    pub fn current(&self) -> Option<LayerId> {
        self.stack.last().copied()
    }

    /// Returns the root layer ID if any layers have been pushed.
    #[inline]
    pub fn root(&self) -> Option<LayerId> {
        self.root
    }

    // ========================================================================
    // INTERNAL HELPERS
    // ========================================================================

    /// Inserts a layer and optionally adds it as child of current parent.
    fn push_layer(&mut self, layer: Layer) -> LayerId {
        let id = self.tree.insert(layer);

        // Add as child of current parent
        if let Some(parent_id) = self.current() {
            self.tree.add_child(parent_id, id);
        }

        // Set as root if first layer
        if self.root.is_none() {
            self.root = Some(id);
            self.tree.set_root(Some(id));
        }

        // Push onto stack
        self.stack.push(id);

        id
    }

    /// Adds a leaf layer (doesn't push onto stack).
    fn add_leaf(&mut self, layer: Layer) -> LayerId {
        let id = self.tree.insert(layer);

        // Add as child of current parent
        if let Some(parent_id) = self.current() {
            self.tree.add_child(parent_id, id);
        }

        // Set as root if first layer (unusual but valid)
        if self.root.is_none() {
            self.root = Some(id);
            self.tree.set_root(Some(id));
        }

        id
    }

    // ========================================================================
    // PUSH OPERATIONS (Container Layers)
    // ========================================================================

    /// Pushes an offset layer that translates all children.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_layer::{LayerTree, SceneBuilder};
    /// use flui_types::Offset;
    ///
    /// let mut tree = LayerTree::new();
    /// let mut builder = SceneBuilder::new(&mut tree);
    ///
    /// builder.push_offset(Offset::new(100.0, 50.0));
    /// // ... add children ...
    /// builder.pop();
    /// ```
    pub fn push_offset(&mut self, offset: flui_types::Offset<flui_types::Pixels>) -> LayerId {
        self.push_layer(Layer::Offset(OffsetLayer::new(offset)))
    }

    /// Pushes a transform layer that applies a 4x4 matrix transformation.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_layer::{LayerTree, SceneBuilder};
    /// use flui_types::Matrix4;
    ///
    /// let mut tree = LayerTree::new();
    /// let mut builder = SceneBuilder::new(&mut tree);
    ///
    /// let transform = Matrix4::scaling(2.0, 2.0, 1.0);
    /// builder.push_transform(transform);
    /// // ... add children ...
    /// builder.pop();
    /// ```
    pub fn push_transform(&mut self, transform: Matrix4) -> LayerId {
        self.push_layer(Layer::Transform(TransformLayer::new(transform)))
    }

    /// Pushes an opacity layer that applies alpha blending to children.
    ///
    /// # Arguments
    ///
    /// * `alpha` - Opacity value from 0.0 (transparent) to 1.0 (opaque)
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_layer::{LayerTree, SceneBuilder};
    ///
    /// let mut tree = LayerTree::new();
    /// let mut builder = SceneBuilder::new(&mut tree);
    ///
    /// builder.push_opacity(0.5);
    /// // ... add children at 50% opacity ...
    /// builder.pop();
    /// ```
    pub fn push_opacity(&mut self, alpha: f32) -> LayerId {
        self.push_layer(Layer::Opacity(OpacityLayer::new(alpha)))
    }

    /// Pushes an opacity layer with offset.
    pub fn push_opacity_with_offset(
        &mut self,
        alpha: f32,
        offset: flui_types::Offset<flui_types::Pixels>,
    ) -> LayerId {
        self.push_layer(Layer::Opacity(OpacityLayer::with_offset(alpha, offset)))
    }

    /// Pushes a clip rect layer that clips children to a rectangle.
    ///
    /// # Arguments
    ///
    /// * `rect` - The clipping rectangle
    /// * `clip` - Clip behavior (HardEdge, AntiAlias, AntiAliasWithSaveLayer)
    pub fn push_clip_rect(&mut self, rect: Rect<Pixels>, clip: Clip) -> LayerId {
        self.push_layer(Layer::ClipRect(ClipRectLayer::new(rect, clip)))
    }

    /// Pushes a clip rect layer with hard edge clipping.
    pub fn push_clip_rect_hard(&mut self, rect: Rect<Pixels>) -> LayerId {
        self.push_layer(Layer::ClipRect(ClipRectLayer::hard_edge(rect)))
    }

    /// Pushes a clip rect layer with anti-aliased clipping.
    pub fn push_clip_rect_aa(&mut self, rect: Rect<Pixels>) -> LayerId {
        self.push_layer(Layer::ClipRect(ClipRectLayer::anti_alias(rect)))
    }

    /// Pushes a clip rounded rect layer.
    ///
    /// # Arguments
    ///
    /// * `rrect` - The rounded rectangle for clipping
    /// * `clip` - Clip behavior
    pub fn push_clip_rrect(&mut self, rrect: RRect, clip: Clip) -> LayerId {
        self.push_layer(Layer::ClipRRect(ClipRRectLayer::new(rrect, clip)))
    }

    /// Pushes a clip path layer.
    ///
    /// # Arguments
    ///
    /// * `path` - The path for clipping
    /// * `clip` - Clip behavior
    pub fn push_clip_path(&mut self, path: Path, clip: Clip) -> LayerId {
        self.push_layer(Layer::ClipPath(ClipPathLayer::new(path, clip)))
    }

    /// Pushes a color filter layer.
    ///
    /// # Arguments
    ///
    /// * `color_matrix` - The color matrix to apply
    pub fn push_color_filter(&mut self, color_matrix: ColorMatrix) -> LayerId {
        self.push_layer(Layer::ColorFilter(ColorFilterLayer::new(color_matrix)))
    }

    /// Pushes an image filter layer (blur, etc.).
    ///
    /// # Arguments
    ///
    /// * `filter` - The image filter to apply
    pub fn push_image_filter(&mut self, filter: ImageFilter) -> LayerId {
        self.push_layer(Layer::ImageFilter(ImageFilterLayer::new(filter)))
    }

    /// Pushes a blur image filter layer.
    ///
    /// # Arguments
    ///
    /// * `sigma` - Blur radius (applies equally to x and y)
    pub fn push_blur(&mut self, sigma: f32) -> LayerId {
        self.push_layer(Layer::ImageFilter(ImageFilterLayer::blur(sigma)))
    }

    /// Pushes a shader mask layer.
    ///
    /// # Arguments
    ///
    /// * `shader` - The shader specification for the mask
    /// * `blend_mode` - How to blend the shader with content
    /// * `bounds` - The bounds for the shader
    pub fn push_shader_mask(
        &mut self,
        shader: ShaderSpec,
        blend_mode: BlendMode,
        bounds: Rect<Pixels>,
    ) -> LayerId {
        self.push_layer(Layer::ShaderMask(ShaderMaskLayer::new(
            shader, blend_mode, bounds,
        )))
    }

    /// Pushes a backdrop filter layer.
    ///
    /// # Arguments
    ///
    /// * `filter` - The filter to apply to content behind this layer
    /// * `blend_mode` - How to blend the filtered backdrop
    /// * `bounds` - The bounds of the effect
    pub fn push_backdrop_filter(
        &mut self,
        filter: ImageFilter,
        blend_mode: BlendMode,
        bounds: Rect<Pixels>,
    ) -> LayerId {
        self.push_layer(Layer::BackdropFilter(BackdropFilterLayer::new(
            filter, blend_mode, bounds,
        )))
    }

    /// Pushes a backdrop blur layer (convenience method).
    ///
    /// # Arguments
    ///
    /// * `sigma` - Blur radius
    /// * `bounds` - The bounds of the effect
    pub fn push_backdrop_blur(&mut self, sigma: f32, bounds: Rect<Pixels>) -> LayerId {
        self.push_layer(Layer::BackdropFilter(BackdropFilterLayer::new(
            ImageFilter::blur(sigma),
            BlendMode::SrcOver,
            bounds,
        )))
    }

    // ========================================================================
    // ADD OPERATIONS (Leaf Layers)
    // ========================================================================

    /// Adds a canvas layer containing drawing commands.
    ///
    /// Canvas layers are leaf nodes that don't have children.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_layer::{LayerTree, SceneBuilder, CanvasLayer};
    ///
    /// let mut tree = LayerTree::new();
    /// let mut builder = SceneBuilder::new(&mut tree);
    ///
    /// let canvas = CanvasLayer::new();
    /// builder.add_canvas(canvas);
    ///
    /// let root = builder.build();
    /// ```
    pub fn add_canvas(&mut self, canvas: CanvasLayer) -> LayerId {
        self.add_leaf(Layer::Canvas(canvas))
    }

    /// Adds a picture layer with recorded drawing commands.
    ///
    /// This is the primary method for adding cached/recorded content to the scene.
    /// The picture is an immutable DisplayList that was previously recorded via Canvas.
    ///
    /// # Architecture
    ///
    /// ```text
    /// Canvas → finish() → Picture → SceneBuilder::add_picture() → PictureLayer
    /// ```
    ///
    /// # Arguments
    ///
    /// * `picture` - The recorded Picture (DisplayList) from Canvas::finish()
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_layer::{LayerTree, SceneBuilder};
    /// use flui_painting::Canvas;
    /// use flui_types::{Rect, Color};
    /// use flui_types::painting::Paint;
    /// use flui_types::geometry::px;
    ///
    /// // Record drawing commands
    /// let mut canvas = Canvas::new();
    /// canvas.draw_rect(
    ///     Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0)),
    ///     &Paint::fill(Color::RED)
    /// );
    /// let picture = canvas.finish();
    ///
    /// // Add to scene
    /// let mut tree = LayerTree::new();
    /// let mut builder = SceneBuilder::new(&mut tree);
    /// let picture_id = builder.add_picture(picture);
    ///
    /// let root = builder.build();
    /// assert!(root.is_some());
    /// ```
    ///
    /// # Performance
    ///
    /// PictureLayer enables Flutter's repaint boundary optimization:
    /// - Cached pictures can be replayed without re-recording
    /// - Reduces CPU overhead for unchanged content
    /// - Enables partial screen updates
    ///
    /// # See Also
    ///
    /// - [`add_canvas`](Self::add_canvas) - For mutable canvas recording
    /// - [`PictureLayer`](crate::PictureLayer) - The layer type created
    pub fn add_picture(&mut self, picture: flui_painting::Picture) -> LayerId {
        use crate::layer::PictureLayer;
        self.add_leaf(Layer::Picture(PictureLayer::new(picture)))
    }

    /// Adds a texture layer for GPU texture rendering.
    ///
    /// # Arguments
    ///
    /// * `texture_id` - The GPU texture identifier
    /// * `rect` - The destination rectangle
    pub fn add_texture(&mut self, texture_id: TextureId, rect: Rect<Pixels>) -> LayerId {
        self.add_leaf(Layer::Texture(TextureLayer::new(texture_id, rect)))
    }

    /// Adds a texture layer with additional options.
    ///
    /// # Arguments
    ///
    /// * `texture_id` - The GPU texture identifier
    /// * `rect` - The destination rectangle
    /// * `filter_quality` - Texture filtering quality
    /// * `opacity` - Texture opacity (0.0 to 1.0)
    pub fn add_texture_with_options(
        &mut self,
        texture_id: TextureId,
        rect: Rect<Pixels>,
        filter_quality: FilterQuality,
        opacity: f32,
    ) -> LayerId {
        self.add_leaf(Layer::Texture(
            TextureLayer::new(texture_id, rect)
                .with_filter_quality(filter_quality)
                .with_opacity(opacity),
        ))
    }

    /// Adds an arbitrary layer as a leaf.
    ///
    /// This is useful for custom layer types or layers that don't fit
    /// the push/pop pattern.
    pub fn add_layer(&mut self, layer: Layer) -> LayerId {
        self.add_leaf(layer)
    }

    // ========================================================================
    // RETAINED LAYERS
    // ========================================================================

    /// Adds a retained layer subtree.
    ///
    /// This reuses an existing layer and its children, adding them as
    /// children of the current parent.
    ///
    /// # Arguments
    ///
    /// * `layer_id` - The root of the retained subtree
    pub fn add_retained(&mut self, layer_id: LayerId) {
        if let Some(parent_id) = self.current() {
            self.tree.add_child(parent_id, layer_id);
        }
    }

    // ========================================================================
    // POP OPERATION
    // ========================================================================

    /// Pops the current layer from the stack.
    ///
    /// After popping, subsequent operations will add children to the
    /// previous parent layer.
    ///
    /// # Panics
    ///
    /// Panics if the stack is empty.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_layer::{LayerTree, SceneBuilder};
    /// use flui_types::Offset;
    ///
    /// let mut tree = LayerTree::new();
    /// let mut builder = SceneBuilder::new(&mut tree);
    ///
    /// builder.push_offset(Offset::new(10.0, 20.0));
    /// builder.push_opacity(0.5);
    /// builder.pop(); // Back to offset layer
    /// builder.pop(); // Stack is now empty
    /// ```
    pub fn pop(&mut self) {
        let _ = self
            .stack
            .pop()
            .expect("SceneBuilder::pop called on empty stack");
    }

    /// Pops the current layer, returning its ID.
    ///
    /// Returns `None` if the stack is empty.
    pub fn try_pop(&mut self) -> Option<LayerId> {
        self.stack.pop()
    }

    /// Pops all layers from the stack.
    pub fn pop_all(&mut self) {
        self.stack.clear();
    }

    /// Pops layers until reaching the specified depth.
    ///
    /// # Arguments
    ///
    /// * `depth` - Target depth (0 = empty stack)
    pub fn pop_to_depth(&mut self, depth: usize) {
        while self.stack.len() > depth {
            self.stack.pop();
        }
    }

    // ========================================================================
    // BUILD
    // ========================================================================

    /// Finishes building and returns the root layer ID.
    ///
    /// This consumes the builder. The returned ID is the root of the
    /// constructed layer tree.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_layer::{LayerTree, SceneBuilder, CanvasLayer};
    ///
    /// let mut tree = LayerTree::new();
    /// let mut builder = SceneBuilder::new(&mut tree);
    ///
    /// builder.add_canvas(CanvasLayer::new());
    /// let root = builder.build();
    ///
    /// assert!(root.is_some());
    /// ```
    pub fn build(self) -> Option<LayerId> {
        self.root
    }

    /// Builds and returns the root ID, clearing the internal state.
    ///
    /// Unlike `build()`, this allows reusing the builder for another scene.
    pub fn build_and_reset(&mut self) -> Option<LayerId> {
        let root = self.root.take();
        self.stack.clear();
        root
    }
}

// ============================================================================
// SCENE COMPOSITOR
// ============================================================================

/// High-level compositor for managing multiple scenes.
///
/// SceneCompositor provides utilities for compositing multiple layer trees,
/// managing retained layers, and optimizing layer reuse.
#[derive(Debug, Default)]
pub struct SceneCompositor {
    /// Retained layer roots from previous frames
    retained: Vec<LayerId>,

    /// Statistics for debugging
    stats: CompositorStats,
}

/// Statistics about compositor operations.
#[derive(Debug, Default, Clone, Copy)]
pub struct CompositorStats {
    /// Number of layers created this frame
    pub layers_created: usize,

    /// Number of retained layers reused
    pub layers_reused: usize,

    /// Number of layers removed
    pub layers_removed: usize,

    /// Current total layer count
    pub total_layers: usize,
}

impl SceneCompositor {
    /// Creates a new SceneCompositor.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns current compositor statistics.
    pub fn stats(&self) -> CompositorStats {
        self.stats
    }

    /// Resets statistics for a new frame.
    pub fn reset_stats(&mut self) {
        self.stats = CompositorStats::default();
    }

    /// Marks a layer subtree for retention.
    ///
    /// Retained layers can be reused across frames without rebuilding.
    pub fn retain(&mut self, layer_id: LayerId) {
        if !self.retained.contains(&layer_id) {
            self.retained.push(layer_id);
        }
    }

    /// Returns all retained layer IDs.
    pub fn retained_layers(&self) -> &[LayerId] {
        &self.retained
    }

    /// Checks if a layer is retained.
    pub fn is_retained(&self, layer_id: LayerId) -> bool {
        self.retained.contains(&layer_id)
    }

    /// Clears all retained layers.
    pub fn clear_retained(&mut self) {
        self.retained.clear();
    }

    /// Removes a layer from retention.
    pub fn release(&mut self, layer_id: LayerId) {
        self.retained.retain(|&id| id != layer_id);
    }

    /// Updates statistics after frame composition.
    pub fn update_stats(&mut self, tree: &LayerTree) {
        self.stats.total_layers = tree.len();
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::px;
    use flui_types::Offset;

    #[test]
    fn test_scene_builder_new() {
        let mut tree = LayerTree::new();
        let builder = SceneBuilder::new(&mut tree);

        assert_eq!(builder.depth(), 0);
        assert!(builder.current().is_none());
        assert!(builder.root().is_none());
    }

    #[test]
    fn test_scene_builder_push_offset() {
        let mut tree = LayerTree::new();
        let mut builder = SceneBuilder::new(&mut tree);

        let id = builder.push_offset(Offset::new(px(10.0), px(20.0)));

        assert_eq!(builder.depth(), 1);
        assert_eq!(builder.current(), Some(id));
        assert_eq!(builder.root(), Some(id));
    }

    #[test]
    fn test_scene_builder_push_pop() {
        let mut tree = LayerTree::new();
        let mut builder = SceneBuilder::new(&mut tree);

        let offset_id = builder.push_offset(Offset::new(px(10.0), px(20.0)));
        assert_eq!(builder.depth(), 1);

        let opacity_id = builder.push_opacity(0.5);
        assert_eq!(builder.depth(), 2);

        builder.pop();
        assert_eq!(builder.depth(), 1);
        assert_eq!(builder.current(), Some(offset_id));

        builder.pop();
        assert_eq!(builder.depth(), 0);

        // Root should still be offset
        assert_eq!(builder.root(), Some(offset_id));

        // Verify tree structure
        let children = tree.children(offset_id).unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0], opacity_id);
    }

    #[test]
    fn test_scene_builder_add_canvas() {
        let mut tree = LayerTree::new();
        let mut builder = SceneBuilder::new(&mut tree);

        let offset_id = builder.push_offset(Offset::ZERO);
        let canvas_id = builder.add_canvas(CanvasLayer::new());

        // Canvas should not be pushed onto stack
        assert_eq!(builder.depth(), 1);
        assert_eq!(builder.current(), Some(offset_id));

        // Finish building to release borrow
        let _root = builder.build();

        // Now we can check tree structure
        let children = tree.children(offset_id).unwrap();
        assert!(children.contains(&canvas_id));
    }

    #[test]
    fn test_scene_builder_add_picture() {
        use flui_painting::Canvas;
        use flui_types::painting::Paint;
        use flui_types::{Color, Rect};

        let mut tree = LayerTree::new();
        let mut builder = SceneBuilder::new(&mut tree);

        // Record a picture
        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0)),
            &Paint::fill(Color::RED),
        );
        let picture = canvas.finish();

        // Add picture to scene
        let offset_id = builder.push_offset(Offset::ZERO);
        let picture_id = builder.add_picture(picture);

        // Picture should not be pushed onto stack
        assert_eq!(builder.depth(), 1);
        assert_eq!(builder.current(), Some(offset_id));

        // Finish building to release borrow
        let _root = builder.build();

        // Verify tree structure
        let children = tree.children(offset_id).unwrap();
        assert!(children.contains(&picture_id));

        // Verify layer type
        let layer = tree.get_layer(picture_id).unwrap();
        assert!(layer.is_picture());
    }

    #[test]
    fn test_scene_builder_add_texture() {
        let mut tree = LayerTree::new();
        let mut builder = SceneBuilder::new(&mut tree);

        let _ = builder.push_offset(Offset::ZERO);
        let texture_id = builder.add_texture(
            TextureId::new(42),
            Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(100.0)),
        );

        let _ = builder.build();

        let layer = tree.get_layer(texture_id).unwrap();
        assert!(layer.is_texture());
    }

    #[test]
    fn test_scene_builder_nested_transforms() {
        let mut tree = LayerTree::new();
        let mut builder = SceneBuilder::new(&mut tree);

        // Build: offset -> opacity -> transform -> canvas
        let _ = builder.push_offset(Offset::new(px(100.0), px(50.0)));
        let _ = builder.push_opacity(0.8);
        let _ = builder.push_transform(Matrix4::scaling(2.0, 2.0, 1.0));
        let _ = builder.add_canvas(CanvasLayer::new());
        builder.pop();
        builder.pop();
        builder.pop();

        let root = builder.build().unwrap();
        assert!(tree.get_layer(root).unwrap().is_offset());
    }

    #[test]
    fn test_scene_builder_clip_rect() {
        let mut tree = LayerTree::new();
        let mut builder = SceneBuilder::new(&mut tree);

        let clip_id = builder.push_clip_rect(
            Rect::from_ltwh(px(0.0), px(0.0), px(200.0), px(200.0)),
            Clip::HardEdge,
        );
        builder.pop();

        let layer = tree.get_layer(clip_id).unwrap();
        assert!(layer.is_clip_rect());
    }

    #[test]
    fn test_scene_builder_build() {
        let mut tree = LayerTree::new();
        let mut builder = SceneBuilder::new(&mut tree);

        let _ = builder.push_offset(Offset::ZERO);
        let _ = builder.add_canvas(CanvasLayer::new());
        builder.pop();

        let root = builder.build();
        assert!(root.is_some());

        // Tree should have root set
        assert_eq!(tree.root(), root);
    }

    #[test]
    fn test_scene_builder_build_and_reset() {
        let mut tree = LayerTree::new();
        let mut builder = SceneBuilder::new(&mut tree);

        let _ = builder.push_offset(Offset::ZERO);
        builder.pop();

        let root1 = builder.build_and_reset();
        assert!(root1.is_some());
        assert!(builder.root().is_none());
        assert_eq!(builder.depth(), 0);

        // Can build another scene
        let _ = builder.push_opacity(1.0);
        builder.pop();

        let root2 = builder.build_and_reset();
        assert!(root2.is_some());
        assert_ne!(root1, root2);
    }

    #[test]
    fn test_scene_builder_pop_to_depth() {
        let mut tree = LayerTree::new();
        let mut builder = SceneBuilder::new(&mut tree);

        let _ = builder.push_offset(Offset::ZERO);
        let _ = builder.push_opacity(0.5);
        let _ = builder.push_transform(Matrix4::IDENTITY);
        assert_eq!(builder.depth(), 3);

        builder.pop_to_depth(1);
        assert_eq!(builder.depth(), 1);

        builder.pop_to_depth(0);
        assert_eq!(builder.depth(), 0);
    }

    #[test]
    fn test_scene_builder_try_pop() {
        let mut tree = LayerTree::new();
        let mut builder = SceneBuilder::new(&mut tree);

        assert!(builder.try_pop().is_none());

        let id = builder.push_offset(Offset::ZERO);
        assert_eq!(builder.try_pop(), Some(id));
        assert!(builder.try_pop().is_none());
    }

    #[test]
    fn test_scene_compositor_new() {
        let compositor = SceneCompositor::new();
        assert!(compositor.retained_layers().is_empty());
    }

    #[test]
    fn test_scene_compositor_retain() {
        let mut compositor = SceneCompositor::new();
        let id = LayerId::new(1);

        compositor.retain(id);
        assert!(compositor.is_retained(id));
        assert_eq!(compositor.retained_layers().len(), 1);

        // Retaining same ID again should not duplicate
        compositor.retain(id);
        assert_eq!(compositor.retained_layers().len(), 1);
    }

    #[test]
    fn test_scene_compositor_release() {
        let mut compositor = SceneCompositor::new();
        let id = LayerId::new(1);

        compositor.retain(id);
        assert!(compositor.is_retained(id));

        compositor.release(id);
        assert!(!compositor.is_retained(id));
    }

    #[test]
    fn test_scene_compositor_clear_retained() {
        let mut compositor = SceneCompositor::new();

        compositor.retain(LayerId::new(1));
        compositor.retain(LayerId::new(2));
        compositor.retain(LayerId::new(3));

        assert_eq!(compositor.retained_layers().len(), 3);

        compositor.clear_retained();
        assert!(compositor.retained_layers().is_empty());
    }

    #[test]
    fn test_scene_compositor_stats() {
        let mut compositor = SceneCompositor::new();
        let mut tree = LayerTree::new();

        let _ = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let _ = tree.insert(Layer::Canvas(CanvasLayer::new()));

        compositor.update_stats(&tree);
        assert_eq!(compositor.stats().total_layers, 2);

        compositor.reset_stats();
        assert_eq!(compositor.stats().total_layers, 0);
    }
}
