//! Layer types - Compositor layers for advanced effects
//!
//! This module provides layer types for the compositor:
//!
//! ## Leaf Layers
//! - **CanvasLayer**: Standard canvas drawing commands (mutable)
//! - **PictureLayer**: Recorded drawing commands (immutable, used by repaint boundaries)
//! - **TextureLayer**: External GPU texture rendering
//! - **PlatformViewLayer**: Native view embedding
//! - **PerformanceOverlayLayer**: Performance metrics display
//!
//! ## Clip Layers
//! - **ClipRectLayer**: Rectangular clipping
//! - **ClipRRectLayer**: Rounded rectangle clipping
//! - **ClipPathLayer**: Arbitrary path clipping
//!
//! ## Transform Layers
//! - **OffsetLayer**: Simple translation (optimized for repaint boundaries)
//! - **TransformLayer**: Full matrix transformation
//!
//! ## Effect Layers
//! - **OpacityLayer**: Alpha blending
//! - **ColorFilterLayer**: Color matrix transformation
//! - **ImageFilterLayer**: Image filters (blur, dilate, erode)
//! - **ShaderMaskLayer**: GPU shader masking effects
//! - **BackdropFilterLayer**: Backdrop filtering effects (frosted glass)
//!
//! ## Linking Layers
//! - **LeaderLayer**: Anchor point for linked positioning
//! - **FollowerLayer**: Positions content relative to a leader
//!
//! ## Annotation Layers
//! - **AnnotatedRegionLayer**: Metadata regions for system UI integration
//!
//! ## Annotation Search
//! - **AnnotationEntry**: Single annotation with local position
//! - **AnnotationResult**: Collection of found annotations
//!
//! ## Composition Callbacks
//! - **CompositionCallbackRegistry**: Registry for compositing event callbacks
//!
//! ## Architecture
//!
//! ```text
//! RenderObject (flui_rendering)
//!     │
//!     │ paint() generates Canvas OR pushes Layer
//!     ▼
//! Layer (this module)
//!     │
//!     │ render() → CommandRenderer (in flui_engine)
//!     ▼
//! GPU Rendering (wgpu)
//! ```

// Leaf layers
mod canvas;
mod performance_overlay;
mod picture;
mod platform_view;
mod texture;

// Clip layers
mod clip_path;
mod clip_rect;
mod clip_rrect;

// Transform layers
mod offset;
mod transform;

// Linking layers
mod follower;
mod leader;

// Annotation layers
mod annotated_region;

// Effect layers
mod backdrop_filter;
mod color_filter;
mod image_filter;
mod opacity;
mod shader_mask;

// Annotation search system
pub mod annotation;

// Composition callbacks
pub mod composition_callback;

// Re-exports
pub use annotated_region::{
    AnnotatedRegionLayer, AnnotationValue, SemanticLabel, SystemUiOverlayStyle,
};
pub use backdrop_filter::BackdropFilterLayer;
pub use canvas::CanvasLayer;
pub use clip_path::ClipPathLayer;
pub use clip_rect::ClipRectLayer;
pub use clip_rrect::ClipRRectLayer;
pub use color_filter::ColorFilterLayer;
pub use follower::FollowerLayer;
pub use image_filter::ImageFilterLayer;
pub use leader::{LayerLink, LeaderLayer};
pub use offset::OffsetLayer;
pub use opacity::OpacityLayer;
pub use performance_overlay::{PerformanceOverlayLayer, PerformanceOverlayOption};
pub use picture::PictureLayer;
pub use platform_view::{PlatformViewHitTestBehavior, PlatformViewId, PlatformViewLayer};
pub use shader_mask::ShaderMaskLayer;
pub use texture::TextureLayer;
pub use transform::TransformLayer;

use flui_types::geometry::Rect;

/// Compositor layer - polymorphic layer types for advanced rendering
///
/// This enum provides a type-safe wrapper for different layer types,
/// enabling clean dispatch without requiring trait objects.
///
/// # Layer Categories
///
/// ## Leaf Layers (no children, direct rendering)
/// - `Canvas`: Standard drawing commands (mutable)
/// - `Picture`: Recorded drawing commands (immutable, for repaint boundaries)
/// - `Texture`: External GPU texture
/// - `PlatformView`: Native platform view
///
/// ## Container Layers (have children, apply effects)
///
/// ### Clip Layers
/// - `ClipRect`: Rectangular clipping
/// - `ClipRRect`: Rounded rectangle clipping
/// - `ClipPath`: Arbitrary path clipping
///
/// ### Transform Layers
/// - `Offset`: Simple translation
/// - `Transform`: Full matrix transformation
///
/// ### Effect Layers
/// - `Opacity`: Alpha blending
/// - `ColorFilter`: Color matrix transformation
/// - `ImageFilter`: Blur, dilate, erode effects
/// - `ShaderMask`: GPU shader masking
/// - `BackdropFilter`: Backdrop filtering (frosted glass)
///
/// ### Linking Layers
/// - `Leader`: Anchor point for linked positioning
/// - `Follower`: Positions content relative to a leader
///
/// ### Annotation Layers
/// - `AnnotatedRegion`: Metadata regions
///
/// # Example
///
/// ```rust
/// use flui_layer::{Layer, CanvasLayer, ClipRectLayer, OpacityLayer};
/// use flui_types::geometry::Rect;
/// use flui_types::painting::Clip;
///
/// // Leaf layer
/// let canvas_layer = Layer::Canvas(CanvasLayer::new());
///
/// // Clip layer
/// let clip_layer = Layer::ClipRect(ClipRectLayer::new(
///     Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
///     Clip::HardEdge,
/// ));
///
/// // Effect layer
/// let opacity_layer = Layer::Opacity(OpacityLayer::new(0.5));
/// ```
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Layer {
    // ========== Leaf Layers ==========
    /// Canvas layer - standard drawing commands (mutable)
    Canvas(CanvasLayer),

    /// Picture layer - recorded drawing commands (immutable, for repaint boundaries)
    Picture(PictureLayer),

    /// Texture layer - external GPU texture
    Texture(TextureLayer),

    /// Platform view layer - native platform view embedding
    PlatformView(PlatformViewLayer),

    /// Performance overlay layer - displays performance statistics
    PerformanceOverlay(PerformanceOverlayLayer),

    // ========== Clip Layers ==========
    /// Clip to rectangle
    ClipRect(ClipRectLayer),

    /// Clip to rounded rectangle
    ClipRRect(ClipRRectLayer),

    /// Clip to arbitrary path
    ClipPath(ClipPathLayer),

    // ========== Transform Layers ==========
    /// Simple offset/translation
    Offset(OffsetLayer),

    /// Full matrix transformation
    Transform(TransformLayer),

    // ========== Effect Layers ==========
    /// Opacity/alpha blending
    Opacity(OpacityLayer),

    /// Color matrix filter
    ColorFilter(ColorFilterLayer),

    /// Image filter (blur, etc.)
    ImageFilter(ImageFilterLayer),

    /// Shader mask layer - GPU shader masking effects
    ShaderMask(ShaderMaskLayer),

    /// Backdrop filter layer - backdrop filtering effects (frosted glass, blur)
    BackdropFilter(BackdropFilterLayer),

    // ========== Linking Layers ==========
    /// Leader layer - anchor point for linked positioning
    Leader(LeaderLayer),

    /// Follower layer - positions content relative to a leader
    Follower(FollowerLayer),

    // ========== Annotation Layers ==========
    /// Annotated region layer - metadata regions for system UI
    AnnotatedRegion(AnnotatedRegionLayer),
}

impl Layer {
    /// Returns the bounds of this layer.
    #[allow(clippy::match_same_arms)] // Each arm is documented separately for clarity
    pub fn bounds(&self) -> Option<Rect> {
        match self {
            Layer::Canvas(layer) => Some(layer.bounds()),
            Layer::Picture(layer) => Some(layer.bounds()),
            Layer::Texture(layer) => Some(layer.bounds()),
            Layer::PlatformView(layer) => Some(layer.bounds()),
            Layer::PerformanceOverlay(layer) => Some(layer.bounds()),
            Layer::ClipRect(layer) => Some(layer.bounds()),
            Layer::ClipRRect(layer) => Some(layer.bounds()),
            Layer::ClipPath(layer) => Some(layer.bounds()),
            Layer::Offset(_) => None,      // Offset doesn't define bounds
            Layer::Transform(_) => None,   // Transform doesn't define bounds
            Layer::Opacity(_) => None,     // Opacity doesn't define bounds
            Layer::ColorFilter(_) => None, // ColorFilter doesn't define bounds
            Layer::ImageFilter(_) => None, // ImageFilter doesn't define bounds
            Layer::ShaderMask(layer) => Some(layer.bounds()),
            Layer::BackdropFilter(layer) => Some(layer.bounds()),
            Layer::Leader(layer) => Some(layer.bounds()),
            Layer::Follower(_) => None, // Follower bounds depend on runtime positioning
            Layer::AnnotatedRegion(layer) => Some(layer.bounds()),
        }
    }

    /// Returns true if this layer needs compositing.
    ///
    /// Compositing requires offscreen rendering and is more expensive.
    #[allow(clippy::match_same_arms)] // Each arm is documented separately for clarity
    pub fn needs_compositing(&self) -> bool {
        match self {
            Layer::Canvas(_) => false,
            Layer::Picture(_) => false, // Picture is immutable, doesn't need compositing
            Layer::Texture(layer) => !layer.is_opaque(), // Needs compositing if transparent
            Layer::PlatformView(_) => true, // Platform views always need compositing
            Layer::PerformanceOverlay(_) => true, // Performance overlay always needs compositing
            Layer::ClipRect(layer) => layer.is_anti_aliased(),
            Layer::ClipRRect(layer) => layer.is_anti_aliased(),
            Layer::ClipPath(layer) => layer.is_anti_aliased(),
            Layer::Offset(_) => false, // Simple translation, no compositing
            Layer::Transform(layer) => !layer.is_translation_only(),
            Layer::Opacity(layer) => layer.needs_compositing(),
            Layer::ColorFilter(_) => true,
            Layer::ImageFilter(_) => true,
            Layer::ShaderMask(_) => true,
            Layer::BackdropFilter(_) => true,
            Layer::Leader(_) => false, // Leader is just a coordinate anchor
            Layer::Follower(_) => false, // Follower is just positioning
            Layer::AnnotatedRegion(_) => false, // Annotation is metadata only
        }
    }

    // ========== Type Checking ==========

    /// Returns true if this is a canvas layer.
    #[inline]
    pub fn is_canvas(&self) -> bool {
        matches!(self, Layer::Canvas(_))
    }

    /// Returns true if this is a picture layer.
    #[inline]
    pub fn is_picture(&self) -> bool {
        matches!(self, Layer::Picture(_))
    }

    /// Returns true if this is a clip rect layer.
    #[inline]
    pub fn is_clip_rect(&self) -> bool {
        matches!(self, Layer::ClipRect(_))
    }

    /// Returns true if this is a clip rrect layer.
    #[inline]
    pub fn is_clip_rrect(&self) -> bool {
        matches!(self, Layer::ClipRRect(_))
    }

    /// Returns true if this is a clip path layer.
    #[inline]
    pub fn is_clip_path(&self) -> bool {
        matches!(self, Layer::ClipPath(_))
    }

    /// Returns true if this is any clip layer.
    #[inline]
    pub fn is_clip(&self) -> bool {
        matches!(
            self,
            Layer::ClipRect(_) | Layer::ClipRRect(_) | Layer::ClipPath(_)
        )
    }

    /// Returns true if this is an offset layer.
    #[inline]
    pub fn is_offset(&self) -> bool {
        matches!(self, Layer::Offset(_))
    }

    /// Returns true if this is a transform layer.
    #[inline]
    pub fn is_transform(&self) -> bool {
        matches!(self, Layer::Transform(_))
    }

    /// Returns true if this is an opacity layer.
    #[inline]
    pub fn is_opacity(&self) -> bool {
        matches!(self, Layer::Opacity(_))
    }

    /// Returns true if this is a color filter layer.
    #[inline]
    pub fn is_color_filter(&self) -> bool {
        matches!(self, Layer::ColorFilter(_))
    }

    /// Returns true if this is an image filter layer.
    #[inline]
    pub fn is_image_filter(&self) -> bool {
        matches!(self, Layer::ImageFilter(_))
    }

    /// Returns true if this is a shader mask layer.
    #[inline]
    pub fn is_shader_mask(&self) -> bool {
        matches!(self, Layer::ShaderMask(_))
    }

    /// Returns true if this is a backdrop filter layer.
    #[inline]
    pub fn is_backdrop_filter(&self) -> bool {
        matches!(self, Layer::BackdropFilter(_))
    }

    /// Returns true if this is a texture layer.
    #[inline]
    pub fn is_texture(&self) -> bool {
        matches!(self, Layer::Texture(_))
    }

    /// Returns true if this is a platform view layer.
    #[inline]
    pub fn is_platform_view(&self) -> bool {
        matches!(self, Layer::PlatformView(_))
    }

    /// Returns true if this is a performance overlay layer.
    #[inline]
    pub fn is_performance_overlay(&self) -> bool {
        matches!(self, Layer::PerformanceOverlay(_))
    }

    /// Returns true if this is a leader layer.
    #[inline]
    pub fn is_leader(&self) -> bool {
        matches!(self, Layer::Leader(_))
    }

    /// Returns true if this is a follower layer.
    #[inline]
    pub fn is_follower(&self) -> bool {
        matches!(self, Layer::Follower(_))
    }

    /// Returns true if this is an annotated region layer.
    #[inline]
    pub fn is_annotated_region(&self) -> bool {
        matches!(self, Layer::AnnotatedRegion(_))
    }

    /// Returns true if this is any linking layer (leader or follower).
    #[inline]
    pub fn is_linking(&self) -> bool {
        matches!(self, Layer::Leader(_) | Layer::Follower(_))
    }

    // ========== Downcasting ==========

    /// Returns the canvas layer if this is one.
    #[inline]
    pub fn as_canvas(&self) -> Option<&CanvasLayer> {
        match self {
            Layer::Canvas(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the canvas layer mutably if this is one.
    #[inline]
    pub fn as_canvas_mut(&mut self) -> Option<&mut CanvasLayer> {
        match self {
            Layer::Canvas(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the picture layer if this is one.
    #[inline]
    pub fn as_picture(&self) -> Option<&PictureLayer> {
        match self {
            Layer::Picture(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the picture layer mutably if this is one.
    #[inline]
    pub fn as_picture_mut(&mut self) -> Option<&mut PictureLayer> {
        match self {
            Layer::Picture(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the clip rect layer if this is one.
    #[inline]
    pub fn as_clip_rect(&self) -> Option<&ClipRectLayer> {
        match self {
            Layer::ClipRect(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the clip rect layer mutably if this is one.
    #[inline]
    pub fn as_clip_rect_mut(&mut self) -> Option<&mut ClipRectLayer> {
        match self {
            Layer::ClipRect(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the clip rrect layer if this is one.
    #[inline]
    pub fn as_clip_rrect(&self) -> Option<&ClipRRectLayer> {
        match self {
            Layer::ClipRRect(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the clip rrect layer mutably if this is one.
    #[inline]
    pub fn as_clip_rrect_mut(&mut self) -> Option<&mut ClipRRectLayer> {
        match self {
            Layer::ClipRRect(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the clip path layer if this is one.
    #[inline]
    pub fn as_clip_path(&self) -> Option<&ClipPathLayer> {
        match self {
            Layer::ClipPath(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the clip path layer mutably if this is one.
    #[inline]
    pub fn as_clip_path_mut(&mut self) -> Option<&mut ClipPathLayer> {
        match self {
            Layer::ClipPath(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the offset layer if this is one.
    #[inline]
    pub fn as_offset(&self) -> Option<&OffsetLayer> {
        match self {
            Layer::Offset(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the offset layer mutably if this is one.
    #[inline]
    pub fn as_offset_mut(&mut self) -> Option<&mut OffsetLayer> {
        match self {
            Layer::Offset(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the transform layer if this is one.
    #[inline]
    pub fn as_transform(&self) -> Option<&TransformLayer> {
        match self {
            Layer::Transform(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the transform layer mutably if this is one.
    #[inline]
    pub fn as_transform_mut(&mut self) -> Option<&mut TransformLayer> {
        match self {
            Layer::Transform(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the opacity layer if this is one.
    #[inline]
    pub fn as_opacity(&self) -> Option<&OpacityLayer> {
        match self {
            Layer::Opacity(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the opacity layer mutably if this is one.
    #[inline]
    pub fn as_opacity_mut(&mut self) -> Option<&mut OpacityLayer> {
        match self {
            Layer::Opacity(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the color filter layer if this is one.
    #[inline]
    pub fn as_color_filter(&self) -> Option<&ColorFilterLayer> {
        match self {
            Layer::ColorFilter(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the color filter layer mutably if this is one.
    #[inline]
    pub fn as_color_filter_mut(&mut self) -> Option<&mut ColorFilterLayer> {
        match self {
            Layer::ColorFilter(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the image filter layer if this is one.
    #[inline]
    pub fn as_image_filter(&self) -> Option<&ImageFilterLayer> {
        match self {
            Layer::ImageFilter(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the image filter layer mutably if this is one.
    #[inline]
    pub fn as_image_filter_mut(&mut self) -> Option<&mut ImageFilterLayer> {
        match self {
            Layer::ImageFilter(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the shader mask layer if this is one.
    #[inline]
    pub fn as_shader_mask(&self) -> Option<&ShaderMaskLayer> {
        match self {
            Layer::ShaderMask(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the shader mask layer mutably if this is one.
    #[inline]
    pub fn as_shader_mask_mut(&mut self) -> Option<&mut ShaderMaskLayer> {
        match self {
            Layer::ShaderMask(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the backdrop filter layer if this is one.
    #[inline]
    pub fn as_backdrop_filter(&self) -> Option<&BackdropFilterLayer> {
        match self {
            Layer::BackdropFilter(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the backdrop filter layer mutably if this is one.
    #[inline]
    pub fn as_backdrop_filter_mut(&mut self) -> Option<&mut BackdropFilterLayer> {
        match self {
            Layer::BackdropFilter(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the texture layer if this is one.
    #[inline]
    pub fn as_texture(&self) -> Option<&TextureLayer> {
        match self {
            Layer::Texture(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the texture layer mutably if this is one.
    #[inline]
    pub fn as_texture_mut(&mut self) -> Option<&mut TextureLayer> {
        match self {
            Layer::Texture(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the platform view layer if this is one.
    #[inline]
    pub fn as_platform_view(&self) -> Option<&PlatformViewLayer> {
        match self {
            Layer::PlatformView(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the platform view layer mutably if this is one.
    #[inline]
    pub fn as_platform_view_mut(&mut self) -> Option<&mut PlatformViewLayer> {
        match self {
            Layer::PlatformView(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the performance overlay layer if this is one.
    #[inline]
    pub fn as_performance_overlay(&self) -> Option<&PerformanceOverlayLayer> {
        match self {
            Layer::PerformanceOverlay(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the performance overlay layer mutably if this is one.
    #[inline]
    pub fn as_performance_overlay_mut(&mut self) -> Option<&mut PerformanceOverlayLayer> {
        match self {
            Layer::PerformanceOverlay(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the leader layer if this is one.
    #[inline]
    pub fn as_leader(&self) -> Option<&LeaderLayer> {
        match self {
            Layer::Leader(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the leader layer mutably if this is one.
    #[inline]
    pub fn as_leader_mut(&mut self) -> Option<&mut LeaderLayer> {
        match self {
            Layer::Leader(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the follower layer if this is one.
    #[inline]
    pub fn as_follower(&self) -> Option<&FollowerLayer> {
        match self {
            Layer::Follower(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the follower layer mutably if this is one.
    #[inline]
    pub fn as_follower_mut(&mut self) -> Option<&mut FollowerLayer> {
        match self {
            Layer::Follower(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the annotated region layer if this is one.
    #[inline]
    pub fn as_annotated_region(&self) -> Option<&AnnotatedRegionLayer> {
        match self {
            Layer::AnnotatedRegion(layer) => Some(layer),
            _ => None,
        }
    }

    /// Returns the annotated region layer mutably if this is one.
    #[inline]
    pub fn as_annotated_region_mut(&mut self) -> Option<&mut AnnotatedRegionLayer> {
        match self {
            Layer::AnnotatedRegion(layer) => Some(layer),
            _ => None,
        }
    }
}

// ========== From implementations ==========

impl From<CanvasLayer> for Layer {
    fn from(layer: CanvasLayer) -> Self {
        Layer::Canvas(layer)
    }
}

impl From<PictureLayer> for Layer {
    fn from(layer: PictureLayer) -> Self {
        Layer::Picture(layer)
    }
}

impl From<ClipRectLayer> for Layer {
    fn from(layer: ClipRectLayer) -> Self {
        Layer::ClipRect(layer)
    }
}

impl From<ClipRRectLayer> for Layer {
    fn from(layer: ClipRRectLayer) -> Self {
        Layer::ClipRRect(layer)
    }
}

impl From<ClipPathLayer> for Layer {
    fn from(layer: ClipPathLayer) -> Self {
        Layer::ClipPath(layer)
    }
}

impl From<OffsetLayer> for Layer {
    fn from(layer: OffsetLayer) -> Self {
        Layer::Offset(layer)
    }
}

impl From<TransformLayer> for Layer {
    fn from(layer: TransformLayer) -> Self {
        Layer::Transform(layer)
    }
}

impl From<OpacityLayer> for Layer {
    fn from(layer: OpacityLayer) -> Self {
        Layer::Opacity(layer)
    }
}

impl From<ColorFilterLayer> for Layer {
    fn from(layer: ColorFilterLayer) -> Self {
        Layer::ColorFilter(layer)
    }
}

impl From<ImageFilterLayer> for Layer {
    fn from(layer: ImageFilterLayer) -> Self {
        Layer::ImageFilter(layer)
    }
}

impl From<ShaderMaskLayer> for Layer {
    fn from(layer: ShaderMaskLayer) -> Self {
        Layer::ShaderMask(layer)
    }
}

impl From<BackdropFilterLayer> for Layer {
    fn from(layer: BackdropFilterLayer) -> Self {
        Layer::BackdropFilter(layer)
    }
}

impl From<TextureLayer> for Layer {
    fn from(layer: TextureLayer) -> Self {
        Layer::Texture(layer)
    }
}

impl From<PlatformViewLayer> for Layer {
    fn from(layer: PlatformViewLayer) -> Self {
        Layer::PlatformView(layer)
    }
}

impl From<LeaderLayer> for Layer {
    fn from(layer: LeaderLayer) -> Self {
        Layer::Leader(layer)
    }
}

impl From<FollowerLayer> for Layer {
    fn from(layer: FollowerLayer) -> Self {
        Layer::Follower(layer)
    }
}

impl From<AnnotatedRegionLayer> for Layer {
    fn from(layer: AnnotatedRegionLayer) -> Self {
        Layer::AnnotatedRegion(layer)
    }
}

// ============================================================================
// LAYER BOUNDS TRAIT
// ============================================================================

/// Trait for layers that have bounds.
pub trait LayerBounds {
    /// Returns the bounding rectangle of this layer.
    fn bounds(&self) -> Rect;
}

impl LayerBounds for CanvasLayer {
    fn bounds(&self) -> Rect {
        self.bounds()
    }
}

impl LayerBounds for ClipRectLayer {
    fn bounds(&self) -> Rect {
        self.bounds()
    }
}

impl LayerBounds for ClipRRectLayer {
    fn bounds(&self) -> Rect {
        self.bounds()
    }
}

impl LayerBounds for ClipPathLayer {
    fn bounds(&self) -> Rect {
        self.bounds()
    }
}

impl LayerBounds for ShaderMaskLayer {
    fn bounds(&self) -> Rect {
        self.bounds()
    }
}

impl LayerBounds for BackdropFilterLayer {
    fn bounds(&self) -> Rect {
        self.bounds()
    }
}

impl LayerBounds for TextureLayer {
    fn bounds(&self) -> Rect {
        self.bounds()
    }
}

impl LayerBounds for PlatformViewLayer {
    fn bounds(&self) -> Rect {
        self.bounds()
    }
}

impl LayerBounds for LeaderLayer {
    fn bounds(&self) -> Rect {
        self.bounds()
    }
}

impl LayerBounds for AnnotatedRegionLayer {
    fn bounds(&self) -> Rect {
        self.bounds()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::painting::Clip;

    #[test]
    fn test_layer_from_canvas() {
        let canvas = CanvasLayer::new();
        let layer = Layer::from(canvas);
        assert!(layer.is_canvas());
    }

    #[test]
    fn test_layer_from_clip_rect() {
        let clip = ClipRectLayer::new(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), Clip::HardEdge);
        let layer = Layer::from(clip);
        assert!(layer.is_clip_rect());
        assert!(layer.is_clip());
    }

    #[test]
    fn test_layer_from_opacity() {
        let opacity = OpacityLayer::new(0.5);
        let layer = Layer::from(opacity);
        assert!(layer.is_opacity());
    }

    #[test]
    fn test_layer_needs_compositing() {
        // Canvas doesn't need compositing
        let canvas = Layer::Canvas(CanvasLayer::new());
        assert!(!canvas.needs_compositing());

        // Hard edge clip doesn't need compositing
        let clip = Layer::ClipRect(ClipRectLayer::hard_edge(Rect::from_xywh(
            0.0, 0.0, 100.0, 100.0,
        )));
        assert!(!clip.needs_compositing());

        // Anti-aliased clip needs compositing
        let aa_clip = Layer::ClipRect(ClipRectLayer::anti_alias(Rect::from_xywh(
            0.0, 0.0, 100.0, 100.0,
        )));
        assert!(aa_clip.needs_compositing());

        // Opacity needs compositing (unless fully opaque/transparent)
        let opacity = Layer::Opacity(OpacityLayer::new(0.5));
        assert!(opacity.needs_compositing());

        let opaque = Layer::Opacity(OpacityLayer::opaque());
        assert!(!opaque.needs_compositing());
    }

    #[test]
    fn test_layer_bounds() {
        let clip = Layer::ClipRect(ClipRectLayer::new(
            Rect::from_xywh(10.0, 20.0, 100.0, 50.0),
            Clip::HardEdge,
        ));
        let bounds = clip.bounds().unwrap();
        assert_eq!(bounds.left(), 10.0);
        assert_eq!(bounds.top(), 20.0);
        assert_eq!(bounds.width(), 100.0);
        assert_eq!(bounds.height(), 50.0);
    }

    #[test]
    fn test_layer_as_methods() {
        let layer = Layer::Opacity(OpacityLayer::new(0.5));

        assert!(layer.as_opacity().is_some());
        assert!(layer.as_canvas().is_none());
        assert!(layer.as_clip_rect().is_none());
    }
}
