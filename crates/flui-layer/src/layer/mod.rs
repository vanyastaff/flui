//! Layer types - Compositor layers for advanced effects
//!
//! This module provides layer types for the compositor:
//!
//! ## Leaf Layers
//! - **CanvasLayer**: Standard canvas drawing commands (mutable)
//! - **PictureLayer**: Recorded drawing commands (immutable, used by repaint
//!   boundaries)
//! - **TextureLayer**: External GPU texture rendering
//! - **PlatformViewLayer**: Native view embedding
//! - **PerformanceOverlayLayer**: Performance metrics display
//!
//! ## Clip Layers
//! - **ClipRectLayer**: Rectangular clipping
//! - **ClipRRectLayer**: Rounded rectangle clipping
//! - **ClipSuperellipseLayer**: iOS-style squircle clipping
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
mod clip_superellipse;

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

// LayerBounds trait + impls (Mythos Step 6).
mod bounds;

// Generated is_*/as_*/as_*_mut accessors on the Layer enum (Mythos Step 4).
mod dispatch;

pub use bounds::LayerBounds;

// Re-exports
pub use annotated_region::{
    AnnotatedRegionLayer, AnnotationValue, SemanticLabel, SystemUiOverlayStyle,
};
pub use backdrop_filter::BackdropFilterLayer;
pub use canvas::CanvasLayer;
pub use clip_path::ClipPathLayer;
pub use clip_rect::ClipRectLayer;
pub use clip_rrect::ClipRRectLayer;
pub use clip_superellipse::ClipSuperellipseLayer;
pub use color_filter::ColorFilterLayer;
use flui_foundation::{Diagnosticable, DiagnosticsBuilder, DiagnosticsNode};
// `DisplayListCore` brings `len()` on the picture's display list into scope.
use flui_painting::DisplayListCore;
use flui_types::geometry::{Pixels, Rect};
pub use follower::FollowerLayer;
pub use image_filter::ImageFilterLayer;
pub use leader::{LayerLink, LeaderLayer};
pub use offset::OffsetLayer;
pub use opacity::OpacityLayer;
pub use performance_overlay::{
    PerformanceOverlayLayer, PerformanceOverlayOption, PerformanceStats,
};
pub use picture::PictureLayer;
pub use platform_view::{PlatformViewHitTestBehavior, PlatformViewId, PlatformViewLayer};
pub use shader_mask::ShaderMaskLayer;
pub use texture::TextureLayer;
pub use transform::TransformLayer;

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
/// use flui_types::geometry::px;
/// use flui_layer::{CanvasLayer, ClipRectLayer, Layer, OpacityLayer};
/// use flui_types::{geometry::Rect, painting::Clip};
///
/// // Leaf layer
/// let canvas_layer = Layer::Canvas(Box::new(CanvasLayer::new()));
///
/// // Clip layer
/// let clip_layer = Layer::ClipRect(ClipRectLayer::new(
///     Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0)),
///     Clip::HardEdge,
/// ));
///
/// // Effect layer
/// let opacity_layer = Layer::Opacity(OpacityLayer::new(0.5));
/// ```
#[derive(Debug, Clone)]
pub enum Layer {
    // ========== Leaf Layers ==========
    /// Canvas layer — standard drawing commands (mutable).
    ///
    /// Boxed because the inner `CanvasLayer` carries an unbounded
    /// `Vec<CanvasCommand>` that dominates the variant footprint.
    Canvas(Box<CanvasLayer>),

    /// Picture layer — recorded drawing commands (immutable, for repaint
    /// boundaries).
    ///
    /// Boxed because the inner `PictureLayer` carries an unbounded
    /// display-list `Vec` that dominates the variant footprint.
    Picture(Box<PictureLayer>),

    /// Texture layer - external GPU texture
    Texture(TextureLayer),

    /// Platform view layer - native platform view embedding
    PlatformView(PlatformViewLayer),

    /// Performance overlay layer — displays performance statistics.
    ///
    /// Boxed because the inner type stores frame-history rings and
    /// renderer state that exceed the per-variant inline budget.
    PerformanceOverlay(Box<PerformanceOverlayLayer>),

    // ========== Clip Layers ==========
    /// Clip to rectangle
    ClipRect(ClipRectLayer),

    /// Clip to rounded rectangle
    ClipRRect(ClipRRectLayer),

    /// Clip to arbitrary path.
    ///
    /// Boxed because the inner `Path` owns a command `Vec` whose
    /// worst-case footprint dwarfs the lightweight variants.
    ClipPath(Box<ClipPathLayer>),

    /// Clip to superellipse (iOS-style squircle)
    ClipSuperellipse(ClipSuperellipseLayer),

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
    pub fn bounds(&self) -> Option<Rect<Pixels>> {
        match self {
            Layer::Canvas(layer) => Some(layer.bounds()),
            Layer::Picture(layer) => Some(layer.bounds()),
            Layer::Texture(layer) => Some(layer.bounds()),
            Layer::PlatformView(layer) => Some(layer.bounds()),
            Layer::PerformanceOverlay(layer) => Some(layer.bounds()),
            Layer::ClipRect(layer) => Some(layer.bounds()),
            Layer::ClipRRect(layer) => Some(layer.bounds()),
            Layer::ClipPath(layer) => Some(layer.bounds()),
            Layer::ClipSuperellipse(layer) => Some(layer.bounds()),
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
            Layer::ClipSuperellipse(layer) => layer.is_anti_aliased(),
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

    // ========== Composite Type Checks ==========
    //
    // Individual `is_<variant>`, `as_<variant>`, `as_<variant>_mut` accessors
    // are generated by `gen_layer_accessors!` at the bottom of this file
    // (see `layer/dispatch.rs`). The composite checks below pattern-match
    // across multiple variants and stay hand-written.

    /// Returns true if this is any clip layer (rect / rrect / path /
    /// superellipse).
    #[inline]
    pub fn is_clip(&self) -> bool {
        matches!(
            self,
            Layer::ClipRect(_)
                | Layer::ClipRRect(_)
                | Layer::ClipPath(_)
                | Layer::ClipSuperellipse(_)
        )
    }

    /// Returns true if this is any linking layer (leader or follower).
    #[inline]
    pub fn is_linking(&self) -> bool {
        matches!(self, Layer::Leader(_) | Layer::Follower(_))
    }

    /// Returns true if this layer is known to be fully opaque.
    ///
    /// Conservative check used by `OcclusionTracker` to register opaque regions.
    /// Only returns `true` for leaf layers that are guaranteed to draw solid
    /// content with no transparency: `Canvas` and `Picture` layers (which always
    /// fill their bounds), and `Texture` layers with opacity >= 1.0.
    ///
    /// Container/effect layers (clips, transforms, opacity, filters) return
    /// `false` because their visual output depends on their children.
    #[allow(clippy::match_same_arms)]
    pub fn is_opaque(&self) -> bool {
        match self {
            // Canvas and Picture layers draw solid content into their bounds
            Layer::Canvas(_) => true,
            Layer::Picture(_) => true,
            // Texture is opaque only if its opacity is >= 1.0
            Layer::Texture(layer) => layer.is_opaque(),
            // All other layers are conservatively treated as non-opaque
            _ => false,
        }
    }

    /// Returns the short variant name of this layer (e.g. `"Picture"`,
    /// `"ClipRect"`). Used for diagnostics and structural test snapshots.
    #[must_use]
    pub const fn kind_name(&self) -> &'static str {
        match self {
            Layer::Canvas(_) => "Canvas",
            Layer::Picture(_) => "Picture",
            Layer::Texture(_) => "Texture",
            Layer::PlatformView(_) => "PlatformView",
            Layer::PerformanceOverlay(_) => "PerformanceOverlay",
            Layer::ClipRect(_) => "ClipRect",
            Layer::ClipRRect(_) => "ClipRRect",
            Layer::ClipPath(_) => "ClipPath",
            Layer::ClipSuperellipse(_) => "ClipSuperellipse",
            Layer::Offset(_) => "Offset",
            Layer::Transform(_) => "Transform",
            Layer::Opacity(_) => "Opacity",
            Layer::ColorFilter(_) => "ColorFilter",
            Layer::ImageFilter(_) => "ImageFilter",
            Layer::ShaderMask(_) => "ShaderMask",
            Layer::BackdropFilter(_) => "BackdropFilter",
            Layer::Leader(_) => "Leader",
            Layer::Follower(_) => "Follower",
            Layer::AnnotatedRegion(_) => "AnnotatedRegion",
        }
    }
}

impl Diagnosticable for Layer {
    fn to_diagnostics_node(&self) -> DiagnosticsNode {
        let mut node = DiagnosticsNode::new(self.kind_name());
        let mut builder = DiagnosticsBuilder::new();
        self.debug_fill_properties(&mut builder);
        *node.properties_mut() = builder.build();
        node
    }

    fn debug_fill_properties(&self, properties: &mut DiagnosticsBuilder) {
        if let Some(bounds) = self.bounds() {
            properties.add("bounds", format!("{bounds:?}"));
        }
        match self {
            Layer::Offset(layer) => {
                properties.add("offset", format!("{:?}", layer.offset()));
            }
            Layer::Transform(layer) => {
                properties.add("transform", format!("{:?}", layer.transform()));
            }
            Layer::Opacity(layer) => {
                properties.add("alpha", layer.alpha());
                properties.add("offset", format!("{:?}", layer.offset()));
            }
            Layer::ClipRect(layer) => {
                properties.add("clip_rect", format!("{:?}", layer.clip_rect()));
            }
            Layer::Picture(layer) => {
                properties.add("commands", layer.picture().len());
            }
            _ => {}
        }
        properties.add_flag("needs_compositing", self.needs_compositing(), "true");
    }
}

// ============================================================================
// GENERATED is_*/as_*/as_*_mut ACCESSORS
// ============================================================================
//
// Mythos Step 4 collapsed ~600 LOC of hand-written boilerplate (19 variants ×
// 3 methods) into this single macro invocation. The macro definition lives in
// `layer/dispatch.rs`. Composite predicates (`is_clip`, `is_linking`,
// `is_opaque`) and the semantic methods (`bounds`, `needs_compositing`) stay
// hand-written above.

dispatch::gen_layer_accessors! {
    Canvas => CanvasLayer, is_canvas, as_canvas, as_canvas_mut;
    Picture => PictureLayer, is_picture, as_picture, as_picture_mut;
    Texture => TextureLayer, is_texture, as_texture, as_texture_mut;
    PlatformView => PlatformViewLayer, is_platform_view, as_platform_view, as_platform_view_mut;
    PerformanceOverlay => PerformanceOverlayLayer, is_performance_overlay, as_performance_overlay, as_performance_overlay_mut;
    ClipRect => ClipRectLayer, is_clip_rect, as_clip_rect, as_clip_rect_mut;
    ClipRRect => ClipRRectLayer, is_clip_rrect, as_clip_rrect, as_clip_rrect_mut;
    ClipPath => ClipPathLayer, is_clip_path, as_clip_path, as_clip_path_mut;
    ClipSuperellipse => ClipSuperellipseLayer, is_clip_superellipse, as_clip_superellipse, as_clip_superellipse_mut;
    Offset => OffsetLayer, is_offset, as_offset, as_offset_mut;
    Transform => TransformLayer, is_transform, as_transform, as_transform_mut;
    Opacity => OpacityLayer, is_opacity, as_opacity, as_opacity_mut;
    ColorFilter => ColorFilterLayer, is_color_filter, as_color_filter, as_color_filter_mut;
    ImageFilter => ImageFilterLayer, is_image_filter, as_image_filter, as_image_filter_mut;
    ShaderMask => ShaderMaskLayer, is_shader_mask, as_shader_mask, as_shader_mask_mut;
    BackdropFilter => BackdropFilterLayer, is_backdrop_filter, as_backdrop_filter, as_backdrop_filter_mut;
    Leader => LeaderLayer, is_leader, as_leader, as_leader_mut;
    Follower => FollowerLayer, is_follower, as_follower, as_follower_mut;
    AnnotatedRegion => AnnotatedRegionLayer, is_annotated_region, as_annotated_region, as_annotated_region_mut;
}

// ========== From implementations ==========

impl From<CanvasLayer> for Layer {
    fn from(layer: CanvasLayer) -> Self {
        Layer::Canvas(Box::new(layer))
    }
}

impl From<PictureLayer> for Layer {
    fn from(layer: PictureLayer) -> Self {
        Layer::Picture(Box::new(layer))
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
        Layer::ClipPath(Box::new(layer))
    }
}

impl From<ClipSuperellipseLayer> for Layer {
    fn from(layer: ClipSuperellipseLayer) -> Self {
        Layer::ClipSuperellipse(layer)
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
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use flui_types::{geometry::px, painting::Clip};

    use super::*;

    #[test]
    fn test_layer_from_canvas() {
        let canvas = CanvasLayer::new();
        let layer = Layer::from(canvas);
        assert!(layer.is_canvas());
    }

    #[test]
    fn test_layer_from_clip_rect() {
        let clip = ClipRectLayer::new(
            Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0)),
            Clip::HardEdge,
        );
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
        let canvas = Layer::from(CanvasLayer::new());
        assert!(!canvas.needs_compositing());

        // Hard edge clip doesn't need compositing
        let clip = Layer::ClipRect(ClipRectLayer::hard_edge(Rect::from_xywh(
            px(0.0),
            px(0.0),
            px(100.0),
            px(100.0),
        )));
        assert!(!clip.needs_compositing());

        // Anti-aliased clip needs compositing
        let aa_clip = Layer::ClipRect(ClipRectLayer::anti_alias(Rect::from_xywh(
            px(0.0),
            px(0.0),
            px(100.0),
            px(100.0),
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
            Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(50.0)),
            Clip::HardEdge,
        ));
        let bounds = clip.bounds().unwrap();
        assert_eq!(bounds.left(), px(10.0));
        assert_eq!(bounds.top(), px(20.0));
        assert_eq!(bounds.width(), px(100.0));
        assert_eq!(bounds.height(), px(50.0));
    }

    #[test]
    fn test_layer_as_methods() {
        let layer = Layer::Opacity(OpacityLayer::new(0.5));

        assert!(layer.as_opacity().is_some());
        assert!(layer.as_canvas().is_none());
        assert!(layer.as_clip_rect().is_none());
    }
}

#[cfg(test)]
mod size_tests {
    use super::Layer;

    /// Locks the `Layer` enum footprint after U2 boxing of the four heavy
    /// variants (`Canvas`, `Picture`, `ClipPath`, `PerformanceOverlay`).
    ///
    /// Pre-boxing, the enum sat at ~496 bytes because `ClipPathLayer` (488 B)
    /// pulled its command `Vec` into every variant slot. After boxing the
    /// four documented heavies, the discriminant plus the largest *inline*
    /// variant (`ShaderMaskLayer` / `BackdropFilterLayer` at 112 B) drives
    /// the footprint to ~120 B — a 4× compression.
    ///
    /// The 128-byte ceiling here is the *post-U2* receipt — it locks the
    /// outcome of this commit and surfaces unintended footprint growth (e.g.
    /// a future variant addition that exceeds the current widest inline
    /// type) in CI rather than silently.
    ///
    /// A follow-up unit (audit ref: U2-extension) can drop this budget to
    /// ≤40 B by boxing the medium-heavy variants (`Transform`,
    /// `ColorFilter`, `ImageFilter`, `ShaderMask`, `BackdropFilter`,
    /// `ClipRRect`, `ClipSuperellipse`, `Follower`, `AnnotatedRegion`) and
    /// to ≤32 B by also evicting `TextureLayer` / `PlatformViewLayer`.
    /// That work is intentionally scoped out of U2 — boxing 13 variants in
    /// one commit obscures review of the macro/`From`-impl rewiring U3/U4
    /// just landed.
    #[test]
    fn layer_enum_size_compressed_via_boxing() {
        let size = std::mem::size_of::<Layer>();
        assert!(
            size <= 128,
            "Layer enum exceeds 128-byte budget; got {size} bytes. \
             Heavy variants should be `Box<T>` — see U2 of \
             docs/plans/2026-05-22-004-feat-layer-semantics-repair-plan.md",
        );
    }
}
