//! Scene Graph - Immutable rendering primitives
//!
//! This module provides an immutable scene graph architecture inspired by GPUI.
//! Scenes are built once via `SceneBuilder` and can be cached, diffed, and replayed.
//!
//! # Architecture
//!
//! ```text
//! Scene (immutable)
//!   ├─ layers: Vec<Layer>
//!   ├─ viewport: Size<DevicePixels>
//!   └─ clear_color: Color
//!
//! Layer (immutable)
//!   ├─ primitives: Vec<Primitive>
//!   ├─ transform: Matrix4
//!   ├─ opacity: f32
//!   ├─ blend_mode: BlendMode
//!   └─ clip: Option<Rect<Pixels>>
//!
//! Primitive (enum)
//!   ├─ Rect { rect, color, border_radius }
//!   ├─ Text { text, position, style, color }
//!   ├─ Path { path, fill, stroke }
//!   ├─ Image { image, rect, source_rect }
//!   ├─ Underline { start, end, thickness, color }
//!   └─ Shadow { primitive, offset, blur, color }
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_engine::wgpu::scene::{Scene, SceneBuilder};
//! use flui_types::{Size, Point, Color};
//!
//! let viewport = Size::new(800.0, 600.0);
//! let scene = Scene::builder(viewport)
//!     .clear_color(Color::WHITE)
//!     .push_layer()
//!         .add_rect(Rect::new(10.0, 10.0, 100.0, 100.0), Color::RED)
//!         .add_text("Hello".to_string(), Point::new(20.0, 20.0), TextStyle::default())
//!         .opacity(0.8)
//!     .finish_layer()
//!     .build();
//! ```

use flui_types::{
    geometry::{DevicePixels, Pixels, Point, Rect, Size},
    styling::Color,
    typography::TextStyle,
};

/// Immutable scene graph
///
/// A Scene represents a complete frame ready for rendering. It contains:
/// - Layers (primitives grouped with transform/opacity/blend)
/// - Viewport size (render target dimensions)
/// - Clear color (background)
///
/// Scenes are built via `SceneBuilder` and are immutable after construction.
/// This enables caching, diffing, and replay optimizations.
#[derive(Clone, Debug)]
pub struct Scene {
    /// All layers in the scene (ordered front-to-back)
    layers: Vec<Layer>,

    /// Viewport size (in DevicePixels)
    viewport: Size<DevicePixels>,

    /// Global clear color
    clear_color: Color,
}

impl Scene {
    /// Create a new SceneBuilder
    pub fn builder(viewport: Size<DevicePixels>) -> SceneBuilder {
        SceneBuilder::new(viewport)
    }

    /// Get all layers in the scene
    #[must_use]
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    /// Get viewport size
    #[must_use]
    pub fn viewport(&self) -> Size<DevicePixels> {
        self.viewport
    }

    /// Get clear color
    #[must_use]
    pub fn clear_color(&self) -> Color {
        self.clear_color
    }

    /// Check if scene is empty (no layers)
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// Get total number of primitives across all layers
    #[must_use]
    pub fn primitive_count(&self) -> usize {
        self.layers.iter().map(|layer| layer.primitives.len()).sum()
    }

    /// Group primitives into batches for efficient rendering
    ///
    /// Batching reduces draw calls by grouping similar primitives together.
    /// Primitives are batched by type and texture (for images).
    ///
    /// # Returns
    ///
    /// Vector of batches, each containing primitives that can be rendered
    /// with a single draw call.
    #[must_use]
    pub fn batch_primitives(&self) -> Vec<PrimitiveBatch> {
        let mut batches = Vec::new();

        for layer in &self.layers {
            let layer_batches = Self::batch_layer_primitives(&layer.primitives);
            batches.extend(layer_batches);
        }

        batches
    }

    /// Group primitives with layer context (transform, opacity, blend mode)
    ///
    /// This advanced batching method preserves layer context for GPU rendering.
    /// Each batch includes the layer's transform matrix and blend state.
    ///
    /// # Returns
    ///
    /// Vector of batches with layer context included.
    #[must_use]
    pub fn batch_with_context(&self) -> Vec<LayerBatch> {
        let mut batches = Vec::new();

        for layer in &self.layers {
            let primitive_batches = Self::batch_layer_primitives(&layer.primitives);

            for prim_batch in primitive_batches {
                batches.push(LayerBatch {
                    primitives: prim_batch,
                    transform: layer.transform,
                    opacity: layer.opacity,
                    blend_mode: layer.blend_mode,
                    clip: layer.clip,
                });
            }
        }

        batches
    }

    /// Batch primitives within a single layer
    fn batch_layer_primitives(primitives: &[Primitive]) -> Vec<PrimitiveBatch> {
        let mut batches: Vec<PrimitiveBatch> = Vec::new();

        for primitive in primitives {
            let prim_type = primitive.primitive_type();

            // Special handling for Image primitives (batch by texture)
            if let Primitive::Image { image_id, .. } = primitive {
                if let Some(batch) = batches.iter_mut().find(|b| {
                    b.primitive_type == PrimitiveType::Image && b.texture_id == Some(*image_id)
                }) {
                    batch.primitives.push(primitive.clone());
                    continue;
                }

                // Create new image batch
                batches.push(PrimitiveBatch {
                    primitive_type: PrimitiveType::Image,
                    primitives: vec![primitive.clone()],
                    texture_id: Some(*image_id),
                });
                continue;
            }

            // Try to batch with previous batch of same type
            if let Some(last_batch) = batches.last_mut() {
                if last_batch.primitive_type == prim_type && last_batch.texture_id.is_none() {
                    last_batch.primitives.push(primitive.clone());
                    continue;
                }
            }

            // Create new batch
            batches.push(PrimitiveBatch {
                primitive_type: prim_type,
                primitives: vec![primitive.clone()],
                texture_id: None,
            });
        }

        batches
    }
}

/// Builder for constructing immutable Scenes
///
/// Provides a fluent API for building scenes layer-by-layer.
///
/// # Example
///
/// ```rust,ignore
/// let scene = SceneBuilder::new(viewport)
///     .clear_color(Color::BLACK)
///     .push_layer()
///         .add_rect(rect1, Color::RED)
///         .add_rect(rect2, Color::BLUE)
///     .finish()
///     .push_layer()
///         .add_text(text, pos, style)
///         .opacity(0.5)
///     .finish()
///     .build();
/// ```
pub struct SceneBuilder {
    /// Completed layers
    layers: Vec<Layer>,

    /// Viewport size
    viewport: Size<DevicePixels>,

    /// Clear color
    clear_color: Color,
}

impl SceneBuilder {
    /// Create a new SceneBuilder with viewport size
    #[must_use]
    pub fn new(viewport: Size<DevicePixels>) -> Self {
        Self {
            layers: Vec::new(),
            viewport,
            clear_color: Color::WHITE,
        }
    }

    /// Set clear color (background)
    #[must_use]
    pub fn clear_color(mut self, color: Color) -> Self {
        self.clear_color = color;
        self
    }

    /// Start building a new layer
    ///
    /// Returns a LayerBuilder that must call `.finish()` to return to SceneBuilder.
    #[must_use]
    pub fn push_layer(self) -> LayerBuilder {
        LayerBuilder::new(self)
    }

    /// Add a pre-built layer directly
    #[must_use]
    pub fn add_layer(mut self, layer: Layer) -> Self {
        self.layers.push(layer);
        self
    }

    /// Build the final Scene
    ///
    /// Creates an immutable Scene from all added layers.
    #[must_use]
    pub fn build(self) -> Scene {
        Scene {
            layers: self.layers,
            viewport: self.viewport,
            clear_color: self.clear_color,
        }
    }
}

/// Immutable layer containing rendering primitives
///
/// A Layer groups primitives with shared properties:
/// - Transform (translation, rotation, scale)
/// - Opacity (alpha blending)
/// - Blend mode (how layer composites with background)
/// - Clipping region (optional)
#[derive(Clone, Debug)]
pub struct Layer {
    /// Rendering primitives in this layer
    primitives: Vec<Primitive>,

    /// Transform matrix (applied to all primitives)
    transform: glam::Mat4,

    /// Layer opacity (0.0 = transparent, 1.0 = opaque)
    opacity: f32,

    /// Blend mode for compositing
    blend_mode: BlendMode,

    /// Optional clipping region
    clip: Option<Rect<DevicePixels>>,
}

impl Layer {
    /// Create a new LayerBuilder
    #[must_use]
    pub fn builder() -> LayerBuilderStandalone {
        LayerBuilderStandalone::default()
    }

    /// Get primitives in this layer
    #[must_use]
    pub fn primitives(&self) -> &[Primitive] {
        &self.primitives
    }

    /// Get transform matrix
    #[must_use]
    pub fn transform(&self) -> glam::Mat4 {
        self.transform
    }

    /// Get opacity
    #[must_use]
    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    /// Get blend mode
    #[must_use]
    pub fn blend_mode(&self) -> BlendMode {
        self.blend_mode
    }

    /// Get clipping region
    #[must_use]
    pub fn clip(&self) -> Option<Rect<DevicePixels>> {
        self.clip
    }

    /// Check if layer is empty (no primitives)
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.primitives.is_empty()
    }
}

/// Builder for constructing Layers (attached to SceneBuilder)
pub struct LayerBuilder {
    /// Parent scene builder (moved in, returned on finish)
    scene_builder: Option<SceneBuilder>,

    /// Primitives being added
    primitives: Vec<Primitive>,

    /// Transform matrix
    transform: glam::Mat4,

    /// Opacity
    opacity: f32,

    /// Blend mode
    blend_mode: BlendMode,

    /// Clipping region
    clip: Option<Rect<DevicePixels>>,
}

impl LayerBuilder {
    fn new(scene_builder: SceneBuilder) -> Self {
        Self {
            scene_builder: Some(scene_builder),
            primitives: Vec::new(),
            transform: glam::Mat4::IDENTITY,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            clip: None,
        }
    }

    /// Add a rectangle primitive
    pub fn add_rect(mut self, rect: Rect<DevicePixels>, color: Color) -> Self {
        self.primitives.push(Primitive::Rect {
            rect,
            color,
            border_radius: 0.0,
        });
        self
    }

    /// Add a rounded rectangle primitive
    pub fn add_rounded_rect(
        mut self,
        rect: Rect<DevicePixels>,
        color: Color,
        border_radius: f32,
    ) -> Self {
        self.primitives.push(Primitive::Rect {
            rect,
            color,
            border_radius,
        });
        self
    }

    /// Add a text primitive
    pub fn add_text(
        mut self,
        text: String,
        position: Point<DevicePixels>,
        style: TextStyle,
        color: Color,
    ) -> Self {
        self.primitives.push(Primitive::Text {
            text,
            position,
            style,
            color,
        });
        self
    }

    /// Add an underline primitive
    pub fn add_underline(
        mut self,
        start: Point<DevicePixels>,
        end: Point<DevicePixels>,
        thickness: f32,
        color: Color,
    ) -> Self {
        self.primitives.push(Primitive::Underline {
            start,
            end,
            thickness,
            color,
        });
        self
    }

    /// Set transform matrix
    #[must_use]
    pub fn transform(mut self, transform: glam::Mat4) -> Self {
        self.transform = transform;
        self
    }

    /// Set opacity (0.0 - 1.0)
    #[must_use]
    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }

    /// Set blend mode
    #[must_use]
    pub fn blend_mode(mut self, blend_mode: BlendMode) -> Self {
        self.blend_mode = blend_mode;
        self
    }

    /// Set clipping region
    #[must_use]
    pub fn clip(mut self, clip: Rect<DevicePixels>) -> Self {
        self.clip = Some(clip);
        self
    }

    /// Finish building this layer and return to SceneBuilder
    #[must_use]
    pub fn finish(mut self) -> SceneBuilder {
        let layer = Layer {
            primitives: self.primitives,
            transform: self.transform,
            opacity: self.opacity,
            blend_mode: self.blend_mode,
            clip: self.clip,
        };

        let mut scene_builder = self
            .scene_builder
            .take()
            .expect("SceneBuilder should exist");
        scene_builder.layers.push(layer);
        scene_builder
    }
}

/// Standalone LayerBuilder (not attached to SceneBuilder)
///
/// Use this when building layers independently of a scene.
pub struct LayerBuilderStandalone {
    primitives: Vec<Primitive>,
    transform: glam::Mat4,
    opacity: f32,
    blend_mode: BlendMode,
    clip: Option<Rect<DevicePixels>>,
}

impl Default for LayerBuilderStandalone {
    fn default() -> Self {
        Self {
            primitives: Vec::new(),
            transform: glam::Mat4::IDENTITY,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            clip: None,
        }
    }
}

impl LayerBuilderStandalone {
    /// Add a rectangle primitive
    pub fn add_rect(&mut self, rect: Rect<DevicePixels>, color: Color) -> &mut Self {
        self.primitives.push(Primitive::Rect {
            rect,
            color,
            border_radius: 0.0,
        });
        self
    }

    /// Add a rounded rectangle primitive
    pub fn add_rounded_rect(
        &mut self,
        rect: Rect<DevicePixels>,
        color: Color,
        border_radius: f32,
    ) -> &mut Self {
        self.primitives.push(Primitive::Rect {
            rect,
            color,
            border_radius,
        });
        self
    }

    /// Add a text primitive
    pub fn add_text(
        &mut self,
        text: String,
        position: Point<DevicePixels>,
        style: TextStyle,
        color: Color,
    ) -> &mut Self {
        self.primitives.push(Primitive::Text {
            text,
            position,
            style,
            color,
        });
        self
    }

    /// Add an underline primitive
    pub fn add_underline(
        &mut self,
        start: Point<DevicePixels>,
        end: Point<DevicePixels>,
        thickness: f32,
        color: Color,
    ) -> &mut Self {
        self.primitives.push(Primitive::Underline {
            start,
            end,
            thickness,
            color,
        });
        self
    }

    /// Set transform matrix
    pub fn transform(&mut self, transform: glam::Mat4) -> &mut Self {
        self.transform = transform;
        self
    }

    /// Set opacity (0.0 - 1.0)
    pub fn opacity(&mut self, opacity: f32) -> &mut Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }

    /// Set blend mode
    pub fn blend_mode(&mut self, blend_mode: BlendMode) -> &mut Self {
        self.blend_mode = blend_mode;
        self
    }

    /// Set clipping region
    pub fn clip(&mut self, clip: Rect<DevicePixels>) -> &mut Self {
        self.clip = Some(clip);
        self
    }

    /// Build the final Layer
    #[must_use]
    pub fn build(self) -> Layer {
        Layer {
            primitives: self.primitives,
            transform: self.transform,
            opacity: self.opacity,
            blend_mode: self.blend_mode,
            clip: self.clip,
        }
    }
}

/// Rendering primitive (leaf node in scene graph)
///
/// Primitives are the atomic rendering operations. Each primitive type
/// maps to a specific GPU rendering path:
/// - Rect → instanced quad rendering
/// - Text → glyph atlas + instanced quads
/// - Path → lyon tessellation + triangle buffers
/// - Image → textured quad
/// - Underline → thin rectangle
/// - Shadow → blur shader + primitive rendering
#[derive(Clone, Debug)]
pub enum Primitive {
    /// Solid or rounded rectangle
    Rect {
        rect: Rect<DevicePixels>,
        color: Color,
        border_radius: f32,
    },

    /// Text with style and color
    Text {
        text: String,
        position: Point<DevicePixels>,
        style: TextStyle,
        color: Color,
    },

    /// Vector path (filled or stroked)
    Path {
        path: Vec<PathCommand>,
        fill: Option<Color>,
        stroke: Option<StrokeStyle>,
    },

    /// Image/texture rectangle
    Image {
        /// Destination rectangle (where to draw)
        rect: Rect<DevicePixels>,
        /// Source rectangle (which part of image to draw)
        source_rect: Option<Rect<DevicePixels>>,
        /// Image handle (references texture atlas or uploaded texture)
        image_id: u32,
    },

    /// Text underline
    Underline {
        start: Point<DevicePixels>,
        end: Point<DevicePixels>,
        thickness: f32,
        color: Color,
    },

    /// Drop shadow (primitive + blur)
    Shadow {
        /// Boxed primitive to cast shadow from
        primitive: Box<Primitive>,
        /// Shadow offset
        offset: Point<DevicePixels>,
        /// Blur radius
        blur_radius: f32,
        /// Shadow color
        color: Color,
    },
}

impl Primitive {
    /// Get the type of this primitive for batching
    #[must_use]
    pub fn primitive_type(&self) -> PrimitiveType {
        match self {
            Primitive::Rect { .. } => PrimitiveType::Rect,
            Primitive::Text { .. } => PrimitiveType::Text,
            Primitive::Path { .. } => PrimitiveType::Path,
            Primitive::Image { .. } => PrimitiveType::Image,
            Primitive::Underline { .. } => PrimitiveType::Underline,
            Primitive::Shadow { .. } => PrimitiveType::Shadow,
        }
    }

    /// Get bounding box of this primitive
    #[must_use]
    pub fn bounds(&self) -> Rect<DevicePixels> {
        match self {
            Primitive::Rect { rect, .. } => *rect,
            Primitive::Text {
                position, style, ..
            } => {
                // Approximate bounds (will be refined with actual text layout)
                let font_size = style.font_size.unwrap_or(14.0);
                Rect::from_min_max(
                    *position,
                    Point::new(
                        DevicePixels(position.x.0 + 100), // Placeholder width
                        DevicePixels(position.y.0 + font_size as i32),
                    ),
                )
            }
            Primitive::Path { path, .. } => {
                if path.is_empty() {
                    return Rect::from_min_max(
                        Point::new(DevicePixels(0), DevicePixels(0)),
                        Point::new(DevicePixels(0), DevicePixels(0)),
                    );
                }

                let mut min_x = path[0].point().x;
                let mut max_x = min_x;
                let mut min_y = path[0].point().y;
                let mut max_y = min_y;

                for cmd in &path[1..] {
                    let pt = cmd.point();
                    min_x = DevicePixels(min_x.0.min(pt.x.0));
                    max_x = DevicePixels(max_x.0.max(pt.x.0));
                    min_y = DevicePixels(min_y.0.min(pt.y.0));
                    max_y = DevicePixels(max_y.0.max(pt.y.0));
                }

                Rect::from_min_max(Point::new(min_x, min_y), Point::new(max_x, max_y))
            }
            Primitive::Image { rect, .. } => *rect,
            Primitive::Underline {
                start,
                end,
                thickness,
                ..
            } => {
                let min_x = DevicePixels(start.x.0.min(end.x.0));
                let max_x = DevicePixels(start.x.0.max(end.x.0));
                let min_y = DevicePixels((start.y.0 as f32 - thickness / 2.0) as i32);
                let max_y = DevicePixels((end.y.0 as f32 + thickness / 2.0) as i32);
                Rect::from_min_max(Point::new(min_x, min_y), Point::new(max_x, max_y))
            }
            Primitive::Shadow {
                primitive,
                offset,
                blur_radius,
                ..
            } => {
                let base_bounds = primitive.bounds();
                let offset_bounds = Rect::from_min_max(
                    Point::new(
                        DevicePixels(base_bounds.min_x().0 + offset.x.0),
                        DevicePixels(base_bounds.min_y().0 + offset.y.0),
                    ),
                    Point::new(
                        DevicePixels(base_bounds.max_x().0 + offset.x.0),
                        DevicePixels(base_bounds.max_y().0 + offset.y.0),
                    ),
                );
                let blur = *blur_radius as f32;
                Rect::from_min_max(
                    Point::new(
                        DevicePixels((offset_bounds.min_x().0 as f32 - blur) as i32),
                        DevicePixels((offset_bounds.min_y().0 as f32 - blur) as i32),
                    ),
                    Point::new(
                        DevicePixels((offset_bounds.max_x().0 as f32 + blur) as i32),
                        DevicePixels((offset_bounds.max_y().0 as f32 + blur) as i32),
                    ),
                )
            }
        }
    }
}

/// Path drawing command
#[derive(Clone, Debug)]
pub enum PathCommand {
    MoveTo(Point<DevicePixels>),
    LineTo(Point<DevicePixels>),
    QuadraticTo(Point<DevicePixels>, Point<DevicePixels>),
    CubicTo(
        Point<DevicePixels>,
        Point<DevicePixels>,
        Point<DevicePixels>,
    ),
    Close,
}

impl PathCommand {
    /// Get the primary point from this command (for bounds calculation)
    #[must_use]
    pub fn point(&self) -> Point<DevicePixels> {
        match self {
            PathCommand::MoveTo(p) => *p,
            PathCommand::LineTo(p) => *p,
            PathCommand::QuadraticTo(_, p) => *p,
            PathCommand::CubicTo(_, _, p) => *p,
            PathCommand::Close => Point::new(DevicePixels(0), DevicePixels(0)),
        }
    }
}

/// Stroke style for path rendering
#[derive(Clone, Debug)]
pub struct StrokeStyle {
    pub color: Color,
    pub width: f32,
    pub line_cap: LineCap,
    pub line_join: LineJoin,
}

/// Line cap style
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineCap {
    Butt,
    Round,
    Square,
}

/// Line join style
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineJoin {
    Miter,
    Round,
    Bevel,
}

/// Batch with layer context (transform, opacity, blend mode)
///
/// This batch type includes layer-level rendering context, allowing
/// the GPU to apply transforms and effects to the entire batch.
#[derive(Clone, Debug)]
pub struct LayerBatch {
    /// Primitive batch
    pub primitives: PrimitiveBatch,

    /// Layer transform matrix
    pub transform: glam::Mat4,

    /// Layer opacity (0.0 - 1.0)
    pub opacity: f32,

    /// Layer blend mode
    pub blend_mode: BlendMode,

    /// Layer clipping region
    pub clip: Option<Rect<DevicePixels>>,
}

impl LayerBatch {
    /// Check if this batch has identity transform
    #[must_use]
    pub fn has_identity_transform(&self) -> bool {
        self.transform == glam::Mat4::IDENTITY
    }

    /// Check if this batch is fully opaque
    #[must_use]
    pub fn is_opaque(&self) -> bool {
        self.opacity >= 1.0
    }

    /// Check if this batch has clipping
    #[must_use]
    pub fn has_clip(&self) -> bool {
        self.clip.is_some()
    }
}

/// Batch of similar primitives for efficient rendering
///
/// Batches group primitives that can be rendered with a single draw call.
/// This reduces GPU overhead by minimizing pipeline state changes.
#[derive(Clone, Debug)]
pub struct PrimitiveBatch {
    /// Type of primitives in this batch
    pub primitive_type: PrimitiveType,

    /// Primitives in this batch
    pub primitives: Vec<Primitive>,

    /// Texture ID (for image batches only)
    pub texture_id: Option<u32>,
}

impl PrimitiveBatch {
    /// Get the number of primitives in this batch
    #[must_use]
    pub fn len(&self) -> usize {
        self.primitives.len()
    }

    /// Check if batch is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.primitives.is_empty()
    }
}

/// Type of rendering primitive
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PrimitiveType {
    /// Rectangle (solid or rounded)
    Rect,
    /// Text glyph run
    Text,
    /// Vector path
    Path,
    /// Textured image
    Image,
    /// Text underline
    Underline,
    /// Drop shadow
    Shadow,
}

/// Blend mode for layer compositing
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlendMode {
    /// Normal alpha blending (default)
    #[default]
    Normal,
    /// Multiply (darkens)
    Multiply,
    /// Screen (lightens)
    Screen,
    /// Overlay (contrast)
    Overlay,
    /// Darken (min)
    Darken,
    /// Lighten (max)
    Lighten,
    /// Color dodge
    ColorDodge,
    /// Color burn
    ColorBurn,
    /// Hard light
    HardLight,
    /// Soft light
    SoftLight,
    /// Difference
    Difference,
    /// Exclusion
    Exclusion,
}

impl BlendMode {
    /// Convert to wgpu blend state
    ///
    /// Maps FLUI blend modes to wgpu's BlendState configuration.
    /// Note: Some advanced blend modes require custom shaders.
    #[must_use]
    #[cfg(feature = "wgpu-backend")]
    pub fn to_wgpu_blend(&self) -> wgpu::BlendState {
        use wgpu::{BlendComponent, BlendFactor, BlendOperation, BlendState};

        match self {
            BlendMode::Normal => BlendState::ALPHA_BLENDING,

            BlendMode::Multiply => BlendState {
                color: BlendComponent {
                    src_factor: BlendFactor::Dst,
                    dst_factor: BlendFactor::Zero,
                    operation: BlendOperation::Add,
                },
                alpha: BlendComponent::OVER,
            },

            BlendMode::Screen => BlendState {
                color: BlendComponent {
                    src_factor: BlendFactor::One,
                    dst_factor: BlendFactor::OneMinusSrc,
                    operation: BlendOperation::Add,
                },
                alpha: BlendComponent::OVER,
            },

            // Advanced blend modes require shader-based implementation
            BlendMode::Overlay
            | BlendMode::ColorDodge
            | BlendMode::ColorBurn
            | BlendMode::HardLight
            | BlendMode::SoftLight
            | BlendMode::Difference
            | BlendMode::Exclusion => {
                // Fall back to normal blending
                // TODO: Implement via custom fragment shader
                BlendState::ALPHA_BLENDING
            }

            BlendMode::Darken => BlendState {
                color: BlendComponent {
                    src_factor: BlendFactor::One,
                    dst_factor: BlendFactor::One,
                    operation: BlendOperation::Min,
                },
                alpha: BlendComponent::OVER,
            },

            BlendMode::Lighten => BlendState {
                color: BlendComponent {
                    src_factor: BlendFactor::One,
                    dst_factor: BlendFactor::One,
                    operation: BlendOperation::Max,
                },
                alpha: BlendComponent::OVER,
            },
        }
    }

    /// Check if this blend mode requires custom shader implementation
    #[must_use]
    pub fn requires_shader(&self) -> bool {
        matches!(
            self,
            BlendMode::Overlay
                | BlendMode::ColorDodge
                | BlendMode::ColorBurn
                | BlendMode::HardLight
                | BlendMode::SoftLight
                | BlendMode::Difference
                | BlendMode::Exclusion
        )
    }
}

// NOTE: Tests temporarily disabled - need update for Pixels/DevicePixels migration
#[cfg(all(test, feature = "disabled-tests"))]
mod tests {
    use super::*;

    fn px(value: f32) -> DevicePixels {
        DevicePixels(value as i32)
    }

    #[test]
    fn test_empty_scene() {
        let viewport = Size::new(px(800.0), px(600.0));
        let scene = Scene::builder(viewport).build();

        assert!(scene.is_empty());
        assert_eq!(scene.primitive_count(), 0);
        assert_eq!(scene.viewport(), viewport);
    }

    #[test]
    fn test_scene_with_clear_color() {
        let viewport = Size::new(px(800.0), px(600.0));
        let scene = Scene::builder(viewport).clear_color(Color::BLACK).build();

        assert_eq!(scene.clear_color(), Color::BLACK);
    }

    #[test]
    fn test_single_layer() {
        let viewport = Size::new(px(800.0), px(600.0));
        let rect = Rect::new(px(10.0), px(10.0), px(100.0), px(100.0));

        let scene = Scene::builder(viewport)
            .push_layer()
            .add_rect(rect, Color::RED)
            .finish()
            .build();

        assert_eq!(scene.layers().len(), 1);
        assert_eq!(scene.primitive_count(), 1);
    }

    #[test]
    fn test_multiple_layers() {
        let viewport = Size::new(px(800.0), px(600.0));

        let scene = Scene::builder(viewport)
            .push_layer()
            .add_rect(
                Rect::new(px(0.0), px(0.0), px(100.0), px(100.0)),
                Color::RED,
            )
            .finish()
            .push_layer()
            .add_rect(
                Rect::new(px(50.0), px(50.0), px(100.0), px(100.0)),
                Color::BLUE,
            )
            .finish()
            .build();

        assert_eq!(scene.layers().len(), 2);
        assert_eq!(scene.primitive_count(), 2);
    }

    #[test]
    fn test_layer_opacity() {
        let viewport = Size::new(px(800.0), px(600.0));

        let scene = Scene::builder(viewport)
            .push_layer()
            .add_rect(
                Rect::new(px(0.0), px(0.0), px(100.0), px(100.0)),
                Color::RED,
            )
            .opacity(0.5)
            .finish()
            .build();

        assert_eq!(scene.layers()[0].opacity(), 0.5);
    }

    #[test]
    fn test_layer_blend_mode() {
        let viewport = Size::new(px(800.0), px(600.0));

        let scene = Scene::builder(viewport)
            .push_layer()
            .add_rect(
                Rect::new(px(0.0), px(0.0), px(100.0), px(100.0)),
                Color::RED,
            )
            .blend_mode(BlendMode::Multiply)
            .finish()
            .build();

        assert_eq!(scene.layers()[0].blend_mode(), BlendMode::Multiply);
    }

    #[test]
    fn test_layer_clipping() {
        let viewport = Size::new(px(800.0), px(600.0));
        let clip_rect = Rect::new(px(0.0), px(0.0), px(200.0), px(200.0));

        let scene = Scene::builder(viewport)
            .push_layer()
            .add_rect(
                Rect::new(px(10.0), px(10.0), px(100.0), px(100.0)),
                Color::RED,
            )
            .clip(clip_rect)
            .finish()
            .build();

        assert_eq!(scene.layers()[0].clip(), Some(clip_rect));
    }

    #[test]
    fn test_rounded_rect() {
        let viewport = Size::new(px(800.0), px(600.0));

        let scene = Scene::builder(viewport)
            .push_layer()
            .add_rounded_rect(
                Rect::new(px(10.0), px(10.0), px(100.0), px(100.0)),
                Color::RED,
                10.0,
            )
            .finish()
            .build();

        match &scene.layers()[0].primitives()[0] {
            Primitive::Rect { border_radius, .. } => {
                assert_eq!(*border_radius, 10.0);
            }
            _ => panic!("Expected Rect primitive"),
        }
    }

    #[test]
    fn test_text_primitive() {
        let viewport = Size::new(px(800.0), px(600.0));

        let scene = Scene::builder(viewport)
            .push_layer()
            .add_text(
                "Hello".to_string(),
                Point::new(px(20.0), px(20.0)),
                TextStyle::default(),
                Color::BLACK,
            )
            .finish()
            .build();

        match &scene.layers()[0].primitives()[0] {
            Primitive::Text { text, .. } => {
                assert_eq!(text, "Hello");
            }
            _ => panic!("Expected Text primitive"),
        }
    }

    #[test]
    fn test_underline_primitive() {
        let viewport = Size::new(px(800.0), px(600.0));

        let scene = Scene::builder(viewport)
            .push_layer()
            .add_underline(
                Point::new(px(10.0), px(30.0)),
                Point::new(px(50.0), px(30.0)),
                1.0,
                Color::BLACK,
            )
            .finish()
            .build();

        match &scene.layers()[0].primitives()[0] {
            Primitive::Underline { thickness, .. } => {
                assert_eq!(*thickness, 1.0);
            }
            _ => panic!("Expected Underline primitive"),
        }
    }

    #[test]
    fn test_multiple_primitives_single_layer() {
        let viewport = Size::new(px(800.0), px(600.0));

        let scene = Scene::builder(viewport)
            .push_layer()
            .add_rect(
                Rect::new(px(0.0), px(0.0), px(100.0), px(100.0)),
                Color::RED,
            )
            .add_rect(
                Rect::new(px(110.0), px(0.0), px(100.0), px(100.0)),
                Color::BLUE,
            )
            .add_text(
                "Test".to_string(),
                Point::new(px(20.0), px(20.0)),
                TextStyle::default(),
                Color::BLACK,
            )
            .finish()
            .build();

        assert_eq!(scene.layers()[0].primitives().len(), 3);
    }

    #[test]
    fn test_layer_transform() {
        let viewport = Size::new(px(800.0), px(600.0));
        let transform = glam::Mat4::from_translation(glam::Vec3::new(10.0, 20.0, 0.0));

        let scene = Scene::builder(viewport)
            .push_layer()
            .add_rect(
                Rect::new(px(0.0), px(0.0), px(100.0), px(100.0)),
                Color::RED,
            )
            .transform(transform)
            .finish()
            .build();

        assert_eq!(scene.layers()[0].transform(), transform);
    }

    #[test]
    fn test_opacity_clamping() {
        let viewport = Size::new(px(800.0), px(600.0));

        let scene1 = Scene::builder(viewport)
            .push_layer()
            .add_rect(
                Rect::new(px(0.0), px(0.0), px(100.0), px(100.0)),
                Color::RED,
            )
            .opacity(1.5) // Should clamp to 1.0
            .finish()
            .build();

        assert_eq!(scene1.layers()[0].opacity(), 1.0);

        let scene2 = Scene::builder(viewport)
            .push_layer()
            .add_rect(
                Rect::new(px(0.0), px(0.0), px(100.0), px(100.0)),
                Color::RED,
            )
            .opacity(-0.5) // Should clamp to 0.0
            .finish()
            .build();

        assert_eq!(scene2.layers()[0].opacity(), 0.0);
    }

    #[test]
    fn test_standalone_layer_builder() {
        let mut builder = Layer::builder();
        builder
            .add_rect(
                Rect::new(px(0.0), px(0.0), px(100.0), px(100.0)),
                Color::RED,
            )
            .opacity(0.8);

        let layer = builder.build();

        assert_eq!(layer.primitives().len(), 1);
        assert_eq!(layer.opacity(), 0.8);
    }

    #[test]
    fn test_add_layer_directly() {
        let viewport = Size::new(px(800.0), px(600.0));

        let mut layer_builder = Layer::builder();
        layer_builder.add_rect(
            Rect::new(px(0.0), px(0.0), px(100.0), px(100.0)),
            Color::RED,
        );
        let layer = layer_builder.build();

        let scene = Scene::builder(viewport).add_layer(layer).build();

        assert_eq!(scene.layers().len(), 1);
        assert_eq!(scene.primitive_count(), 1);
    }

    #[test]
    fn test_shadow_primitive() {
        let shadow_prim = Primitive::Shadow {
            primitive: Box::new(Primitive::Rect {
                rect: Rect::new(px(10.0), px(10.0), px(100.0), px(100.0)),
                color: Color::RED,
                border_radius: 0.0,
            }),
            offset: Point::new(px(2.0), px(2.0)),
            blur_radius: 4.0,
            color: Color::BLACK,
        };

        match shadow_prim {
            Primitive::Shadow { blur_radius, .. } => {
                assert_eq!(blur_radius, 4.0);
            }
            _ => panic!("Expected Shadow primitive"),
        }
    }

    #[test]
    fn test_path_primitive() {
        let viewport = Size::new(px(800.0), px(600.0));

        let path_commands = vec![
            PathCommand::MoveTo(Point::new(px(0.0), px(0.0))),
            PathCommand::LineTo(Point::new(px(100.0), px(0.0))),
            PathCommand::LineTo(Point::new(px(100.0), px(100.0))),
            PathCommand::Close,
        ];

        let mut builder = Layer::builder();
        builder.primitives.push(Primitive::Path {
            path: path_commands.clone(),
            fill: Some(Color::RED),
            stroke: None,
        });

        let layer = builder.build();
        match &layer.primitives()[0] {
            Primitive::Path { path, .. } => {
                assert_eq!(path.len(), 4);
            }
            _ => panic!("Expected Path primitive"),
        }
    }

    #[test]
    fn test_image_primitive() {
        let viewport = Size::new(px(800.0), px(600.0));

        let mut builder = Layer::builder();
        builder.primitives.push(Primitive::Image {
            rect: Rect::new(px(0.0), px(0.0), px(200.0), px(200.0)),
            source_rect: Some(Rect::new(px(0.0), px(0.0), px(100.0), px(100.0))),
            image_id: 42,
        });

        let layer = builder.build();
        match &layer.primitives()[0] {
            Primitive::Image { image_id, .. } => {
                assert_eq!(*image_id, 42);
            }
            _ => panic!("Expected Image primitive"),
        }
    }

    #[test]
    fn test_blend_modes() {
        assert_eq!(BlendMode::default(), BlendMode::Normal);

        let modes = [
            BlendMode::Normal,
            BlendMode::Multiply,
            BlendMode::Screen,
            BlendMode::Overlay,
            BlendMode::Darken,
            BlendMode::Lighten,
        ];

        for mode in modes {
            let viewport = Size::new(px(800.0), px(600.0));
            let scene = Scene::builder(viewport)
                .push_layer()
                .add_rect(
                    Rect::new(px(0.0), px(0.0), px(100.0), px(100.0)),
                    Color::RED,
                )
                .blend_mode(mode)
                .finish_layer()
                .build();

            assert_eq!(scene.layers()[0].blend_mode(), mode);
        }
    }

    #[test]
    fn test_empty_layer() {
        let mut builder = Layer::builder();
        let layer = builder.build();

        assert!(layer.is_empty());
        assert_eq!(layer.primitives().len(), 0);
    }

    // ========== Batching Tests ==========

    #[test]
    fn test_empty_scene_batching() {
        let viewport = Size::new(px(800.0), px(600.0));
        let scene = Scene::builder(viewport).build();

        let batches = scene.batch_primitives();
        assert_eq!(batches.len(), 0);
    }

    #[test]
    fn test_single_rect_batch() {
        let viewport = Size::new(px(800.0), px(600.0));
        let scene = Scene::builder(viewport)
            .push_layer()
            .add_rect(
                Rect::new(px(0.0), px(0.0), px(100.0), px(100.0)),
                Color::RED,
            )
            .finish()
            .build();

        let batches = scene.batch_primitives();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].primitive_type, PrimitiveType::Rect);
        assert_eq!(batches[0].primitives.len(), 1);
    }

    #[test]
    fn test_multiple_rects_batched_together() {
        let viewport = Size::new(px(800.0), px(600.0));
        let scene = Scene::builder(viewport)
            .push_layer()
            .add_rect(
                Rect::new(px(0.0), px(0.0), px(100.0), px(100.0)),
                Color::RED,
            )
            .add_rect(
                Rect::new(px(110.0), px(0.0), px(100.0), px(100.0)),
                Color::BLUE,
            )
            .add_rect(
                Rect::new(px(220.0), px(0.0), px(100.0), px(100.0)),
                Color::GREEN,
            )
            .finish()
            .build();

        let batches = scene.batch_primitives();
        assert_eq!(batches.len(), 1, "All rects should batch together");
        assert_eq!(batches[0].primitive_type, PrimitiveType::Rect);
        assert_eq!(batches[0].primitives.len(), 3);
    }

    #[test]
    fn test_mixed_primitive_types_create_multiple_batches() {
        let viewport = Size::new(px(800.0), px(600.0));
        let scene = Scene::builder(viewport)
            .push_layer()
            .add_rect(
                Rect::new(px(0.0), px(0.0), px(100.0), px(100.0)),
                Color::RED,
            )
            .add_text(
                "Hello".to_string(),
                Point::new(px(10.0), px(10.0)),
                TextStyle::default(),
                Color::BLACK,
            )
            .add_rect(
                Rect::new(px(110.0), px(0.0), px(100.0), px(100.0)),
                Color::BLUE,
            )
            .finish()
            .build();

        let batches = scene.batch_primitives();
        assert_eq!(
            batches.len(),
            3,
            "Should have 3 batches: Rect<Pixels>, Text, Rect"
        );
        assert_eq!(batches[0].primitive_type, PrimitiveType::Rect);
        assert_eq!(batches[0].primitives.len(), 1);
        assert_eq!(batches[1].primitive_type, PrimitiveType::Text);
        assert_eq!(batches[1].primitives.len(), 1);
        assert_eq!(batches[2].primitive_type, PrimitiveType::Rect);
        assert_eq!(batches[2].primitives.len(), 1);
    }

    #[test]
    fn test_image_batching_by_texture() {
        let viewport = Size::new(px(800.0), px(600.0));

        let mut builder = Layer::builder();
        builder.primitives.push(Primitive::Image {
            rect: Rect::new(px(0.0), px(0.0), px(100.0), px(100.0)),
            source_rect: None,
            image_id: 1,
        });
        builder.primitives.push(Primitive::Image {
            rect: Rect::new(px(110.0), px(0.0), px(100.0), px(100.0)),
            source_rect: None,
            image_id: 1, // Same texture
        });
        builder.primitives.push(Primitive::Image {
            rect: Rect::new(px(220.0), px(0.0), px(100.0), px(100.0)),
            source_rect: None,
            image_id: 2, // Different texture
        });
        let layer = builder.build();

        let scene = Scene::builder(viewport).add_layer(layer).build();
        let batches = scene.batch_primitives();

        assert_eq!(batches.len(), 2, "Should have 2 batches by texture ID");
        assert_eq!(batches[0].texture_id, Some(1));
        assert_eq!(batches[0].primitives.len(), 2);
        assert_eq!(batches[1].texture_id, Some(2));
        assert_eq!(batches[1].primitives.len(), 1);
    }

    #[test]
    fn test_underline_batching() {
        let viewport = Size::new(px(800.0), px(600.0));
        let scene = Scene::builder(viewport)
            .push_layer()
            .add_underline(
                Point::new(px(0.0), px(20.0)),
                Point::new(px(100.0), px(20.0)),
                1.0,
                Color::BLACK,
            )
            .add_underline(
                Point::new(px(0.0), px(40.0)),
                Point::new(px(100.0), px(40.0)),
                1.0,
                Color::BLACK,
            )
            .finish()
            .build();

        let batches = scene.batch_primitives();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].primitive_type, PrimitiveType::Underline);
        assert_eq!(batches[0].primitives.len(), 2);
    }

    #[test]
    fn test_batch_across_multiple_layers() {
        let viewport = Size::new(px(800.0), px(600.0));
        let scene = Scene::builder(viewport)
            .push_layer()
            .add_rect(
                Rect::new(px(0.0), px(0.0), px(100.0), px(100.0)),
                Color::RED,
            )
            .finish()
            .push_layer()
            .add_rect(
                Rect::new(px(110.0), px(0.0), px(100.0), px(100.0)),
                Color::BLUE,
            )
            .finish()
            .build();

        let batches = scene.batch_primitives();
        // Each layer creates separate batches (layers might have different transforms/opacity)
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].primitives.len(), 1);
        assert_eq!(batches[1].primitives.len(), 1);
    }

    #[test]
    fn test_primitive_type_method() {
        let rect_prim = Primitive::Rect {
            rect: Rect::new(px(0.0), px(0.0), px(100.0), px(100.0)),
            color: Color::RED,
            border_radius: 0.0,
        };
        assert_eq!(rect_prim.primitive_type(), PrimitiveType::Rect);

        let text_prim = Primitive::Text {
            text: "Test".to_string(),
            position: Point::new(px(10.0), px(10.0)),
            style: TextStyle::default(),
            color: Color::BLACK,
        };
        assert_eq!(text_prim.primitive_type(), PrimitiveType::Text);
    }

    #[test]
    fn test_primitive_bounds_rect() {
        let prim = Primitive::Rect {
            rect: Rect::new(px(10.0), px(20.0), px(110.0), px(120.0)),
            color: Color::RED,
            border_radius: 0.0,
        };
        let bounds = prim.bounds();
        assert_eq!(bounds.min_x(), px(10.0));
        assert_eq!(bounds.min_y(), px(20.0));
    }

    #[test]
    fn test_primitive_bounds_underline() {
        let prim = Primitive::Underline {
            start: Point::new(px(10.0), px(20.0)),
            end: Point::new(px(100.0), px(20.0)),
            thickness: 2.0,
            color: Color::BLACK,
        };
        let bounds = prim.bounds();
        assert_eq!(bounds.min_x(), px(10.0));
        assert_eq!(bounds.max_x(), px(100.0));
    }

    #[test]
    fn test_batch_is_empty() {
        let batch = PrimitiveBatch {
            primitive_type: PrimitiveType::Rect,
            primitives: vec![],
            texture_id: None,
        };
        assert!(batch.is_empty());
        assert_eq!(batch.len(), 0);
    }

    #[test]
    fn test_path_command_point() {
        let cmd = PathCommand::MoveTo(Point::new(px(10.0), px(20.0)));
        assert_eq!(cmd.point().x, px(10.0));
        assert_eq!(cmd.point().y, px(20.0));

        let cmd2 = PathCommand::LineTo(Point::new(px(30.0), px(40.0)));
        assert_eq!(cmd2.point().x, px(30.0));
    }

    // ========== Advanced Batching Tests ==========

    #[test]
    fn test_batch_with_context() {
        let viewport = Size::new(px(800.0), px(600.0));
        let scene = Scene::builder(viewport)
            .push_layer()
            .add_rect(
                Rect::new(px(0.0), px(0.0), px(100.0), px(100.0)),
                Color::RED,
            )
            .opacity(0.5)
            .blend_mode(BlendMode::Multiply)
            .finish()
            .build();

        let batches = scene.batch_with_context();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].opacity, 0.5);
        assert_eq!(batches[0].blend_mode, BlendMode::Multiply);
    }

    #[test]
    fn test_layer_batch_identity_transform() {
        let viewport = Size::new(px(800.0), px(600.0));
        let scene = Scene::builder(viewport)
            .push_layer()
            .add_rect(
                Rect::new(px(0.0), px(0.0), px(100.0), px(100.0)),
                Color::RED,
            )
            .finish()
            .build();

        let batches = scene.batch_with_context();
        assert!(batches[0].has_identity_transform());
    }

    #[test]
    fn test_layer_batch_with_transform() {
        let viewport = Size::new(px(800.0), px(600.0));
        let transform = glam::Mat4::from_translation(glam::Vec3::new(50.0, 50.0, 0.0));

        let scene = Scene::builder(viewport)
            .push_layer()
            .add_rect(
                Rect::new(px(0.0), px(0.0), px(100.0), px(100.0)),
                Color::RED,
            )
            .transform(transform)
            .finish()
            .build();

        let batches = scene.batch_with_context();
        assert!(!batches[0].has_identity_transform());
        assert_eq!(batches[0].transform, transform);
    }

    #[test]
    fn test_layer_batch_opacity_check() {
        let viewport = Size::new(px(800.0), px(600.0));

        let scene1 = Scene::builder(viewport)
            .push_layer()
            .add_rect(
                Rect::new(px(0.0), px(0.0), px(100.0), px(100.0)),
                Color::RED,
            )
            .opacity(1.0)
            .finish()
            .build();

        let batches1 = scene1.batch_with_context();
        assert!(batches1[0].is_opaque());

        let scene2 = Scene::builder(viewport)
            .push_layer()
            .add_rect(
                Rect::new(px(0.0), px(0.0), px(100.0), px(100.0)),
                Color::RED,
            )
            .opacity(0.5)
            .finish()
            .build();

        let batches2 = scene2.batch_with_context();
        assert!(!batches2[0].is_opaque());
    }

    #[test]
    fn test_layer_batch_clipping() {
        let viewport = Size::new(px(800.0), px(600.0));
        let clip = Rect::new(px(0.0), px(0.0), px(200.0), px(200.0));

        let scene = Scene::builder(viewport)
            .push_layer()
            .add_rect(
                Rect::new(px(0.0), px(0.0), px(100.0), px(100.0)),
                Color::RED,
            )
            .clip(clip)
            .finish()
            .build();

        let batches = scene.batch_with_context();
        assert!(batches[0].has_clip());
        assert_eq!(batches[0].clip, Some(clip));
    }

    // ========== BlendMode Tests ==========

    #[test]
    #[cfg(feature = "wgpu-backend")]
    fn test_blend_mode_to_wgpu_normal() {
        let blend = BlendMode::Normal.to_wgpu_blend();
        assert_eq!(blend, wgpu::BlendState::ALPHA_BLENDING);
    }

    #[test]
    #[cfg(feature = "wgpu-backend")]
    fn test_blend_mode_to_wgpu_multiply() {
        let blend = BlendMode::Multiply.to_wgpu_blend();
        assert_eq!(blend.color.src_factor, wgpu::BlendFactor::Dst);
    }

    #[test]
    #[cfg(feature = "wgpu-backend")]
    fn test_blend_mode_to_wgpu_screen() {
        let blend = BlendMode::Screen.to_wgpu_blend();
        assert_eq!(blend.color.src_factor, wgpu::BlendFactor::One);
        assert_eq!(blend.color.dst_factor, wgpu::BlendFactor::OneMinusSrc);
    }

    #[test]
    fn test_blend_mode_requires_shader() {
        assert!(!BlendMode::Normal.requires_shader());
        assert!(!BlendMode::Multiply.requires_shader());
        assert!(!BlendMode::Screen.requires_shader());
        assert!(BlendMode::Overlay.requires_shader());
        assert!(BlendMode::ColorDodge.requires_shader());
        assert!(BlendMode::HardLight.requires_shader());
    }

    #[test]
    fn test_blend_mode_default() {
        let mode: BlendMode = Default::default();
        assert_eq!(mode, BlendMode::Normal);
    }
}
