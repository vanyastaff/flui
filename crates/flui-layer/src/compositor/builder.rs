//! Stack-based [] for constructing layer hierarchies.
//!
//! Extracted from `compositor.rs` in Mythos Step 10. The builder is the
//! canonical scene-construction API; `LayerTree::push_*` helpers were
//! deleted in Step 5.

use flui_foundation::LayerId;
use flui_types::{
    geometry::{Pixels, RRect, Rect},
    painting::{
        effects::ColorMatrix, BlendMode, Clip, FilterQuality, ImageFilter, Path, Shader, TextureId,
    },
    Matrix4,
};

use crate::{
    layer::{
        BackdropFilterLayer, CanvasLayer, ClipPathLayer, ClipRRectLayer, ClipRectLayer,
        ColorFilterLayer, ImageFilterLayer, Layer, OffsetLayer, OpacityLayer, ShaderMaskLayer,
        TextureLayer, TransformLayer,
    },
    tree::LayerTree,
};

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
/// - **Automatic parenting**: Children are automatically added to current
///   parent
/// - **Type-safe**: Each push method creates the appropriate layer type
///
/// # Example
///
/// ```rust
/// use flui_types::geometry::px;
/// use flui_layer::{CanvasLayer, LayerTree, SceneBuilder};
/// use flui_types::Offset;
///
/// let mut tree = LayerTree::new();
/// let mut builder = SceneBuilder::new(&mut tree);
///
/// // Build a simple scene
/// builder.push_offset(Offset::new(px(100.0), px(50.0)));
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
    /// use flui_types::geometry::px;
    /// use flui_layer::{LayerTree, SceneBuilder};
    /// use flui_types::Offset;
    ///
    /// let mut tree = LayerTree::new();
    /// let mut builder = SceneBuilder::new(&mut tree);
    ///
    /// builder.push_offset(Offset::new(px(100.0), px(50.0)));
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

    /// Pushes an opacity layer with an explicit blend mode.
    ///
    /// For plain opacity (`SrcOver`) use [`push_opacity`](Self::push_opacity).
    /// For advanced blend modes (Multiply, Screen, …) use this method; the
    /// engine will route the layer through the dst-read compositor path.
    pub fn push_opacity_blend(&mut self, alpha: f32, blend: BlendMode) -> LayerId {
        self.push_layer(Layer::Opacity(OpacityLayer::with_blend(
            alpha,
            flui_types::Offset::ZERO,
            blend,
        )))
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
        self.push_layer(Layer::from(ClipPathLayer::new(path, clip)))
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
        shader: Shader,
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
    /// use flui_layer::{CanvasLayer, LayerTree, SceneBuilder};
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
        self.add_leaf(Layer::from(canvas))
    }

    /// Adds a picture layer with recorded drawing commands.
    ///
    /// This is the primary method for adding cached/recorded content to the
    /// scene. The picture is an immutable DisplayList that was previously
    /// recorded via Canvas.
    ///
    /// # Architecture
    ///
    /// ```text
    /// Canvas → finish() → DisplayList → SceneBuilder::add_picture() → PictureLayer
    /// ```
    ///
    /// # Arguments
    ///
    /// * `picture` - The recorded `DisplayList` from `Canvas::finish()`
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_layer::{LayerTree, SceneBuilder};
    /// use flui_painting::Canvas;
    /// use flui_types::{geometry::px, painting::Paint, Color, Rect};
    ///
    /// // Record drawing commands
    /// let mut canvas = Canvas::new();
    /// canvas.draw_rect(
    ///     Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0)),
    ///     &Paint::fill(Color::RED),
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
    pub fn add_picture(&mut self, picture: flui_painting::DisplayList) -> LayerId {
        use crate::layer::PictureLayer;
        self.add_leaf(Layer::from(PictureLayer::new(picture)))
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
    /// Returns `Err(LayerError::BuilderStackUnderflow)` if the stack is
    /// empty -- programmer error in the paint phase. For the panic-free
    /// probe form, use [`try_pop`].
    ///
    /// [`try_pop`]: Self::try_pop
    ///
    /// # Mythos
    ///
    /// Replaces a previous `expect("SceneBuilder::pop called on empty stack")`
    /// panic with a structured error in Step 9.
    pub fn pop(&mut self) -> crate::LayerResult<()> {
        self.stack
            .pop()
            .map(|_| ())
            .ok_or(crate::LayerError::BuilderStackUnderflow)
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
    /// use flui_layer::{CanvasLayer, LayerTree, SceneBuilder};
    ///
    /// let mut tree = LayerTree::new();
    /// let mut builder = SceneBuilder::new(&mut tree);
    ///
    /// builder.add_canvas(CanvasLayer::new());
    /// let root = builder.build();
    ///
    /// assert!(root.is_some());
    /// ```
    #[tracing::instrument(skip_all, name = "scene_build", fields(depth = self.depth()))]
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
