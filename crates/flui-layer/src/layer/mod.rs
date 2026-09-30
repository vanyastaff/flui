//! The closed [`Layer`] vocabulary: every compositor primitive the paint walk
//! can emit and the GPU backend must lower.
//!
//! Leaf layers (`Canvas`, `Picture`, `Texture`, `PlatformView`,
//! `PerformanceOverlay`) draw; every other variant is a container that
//! applies a clip, a transform, an effect, or a link to the subtree beneath
//! it. The per-variant types live in the sibling modules and are re-exported
//! from the crate root.

mod annotated_region;
mod backdrop_filter;
mod canvas;
mod clip_path;
mod clip_rect;
mod clip_rrect;
mod clip_superellipse;
mod color_filter;
mod follower;
mod image_filter;
mod leader;
mod offset;
mod opacity;
mod performance_overlay;
mod picture;
mod platform_view;
mod shader_mask;
mod texture;
mod transform;

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
use flui_foundation::geometry::{Offset, Rect};
use flui_foundation::{Diagnosticable, DiagnosticsBuilder, DiagnosticsNode};
pub use follower::FollowerLayer;
pub use image_filter::ImageFilterLayer;
pub use leader::LeaderLayer;
pub use offset::OffsetLayer;
pub use opacity::OpacityLayer;
pub use performance_overlay::{PerformanceOverlayLayer, PerformanceOverlayOption};
pub use picture::PictureLayer;
pub use platform_view::{PlatformViewHitTestBehavior, PlatformViewId, PlatformViewLayer};
pub use shader_mask::ShaderMaskLayer;
pub use texture::TextureLayer;
pub use transform::TransformLayer;

/// One compositor primitive.
///
/// The enum is deliberately **not** `#[non_exhaustive]`: compile-time
/// exhaustiveness is the contract of this closed set. `flui-engine`'s wgpu
/// backend matches every variant, so a new primitive must break that match
/// and land its GPU lowering in the same change (this crate's
/// `ARCHITECTURE.md`, mapping decision 1).
///
/// Payloads are inline unless the variant would dominate the enum's footprint
/// (`Canvas` carries a live recorder); the `From` impls hide the box.
///
/// ```rust
/// use flui_layer::{ClipRectLayer, Layer, OpacityLayer};
/// use flui_foundation::geometry::Rect;
/// use flui_painting::paint::Clip;
///
/// let clip = Layer::from(ClipRectLayer::new(
///     Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
///     Clip::HardEdge,
/// ));
/// let opacity = Layer::from(OpacityLayer::new(0.5));
/// assert_eq!(clip.kind_name(), "ClipRect");
/// assert!(opacity.bounds().is_none());
/// ```
#[derive(Debug, Clone)]
pub enum Layer {
    /// A live recorder whose commands draw when the layer is lowered.
    Canvas(Box<CanvasLayer>),
    /// Sealed drawing commands (the leaf the paint walk emits).
    Picture(PictureLayer),
    /// An external GPU texture.
    Texture(TextureLayer),
    /// A native platform view the embedder composites.
    PlatformView(PlatformViewLayer),
    /// Frame statistics drawn by the backend.
    PerformanceOverlay(PerformanceOverlayLayer),
    /// Clips the subtree to a rectangle.
    ClipRect(ClipRectLayer),
    /// Clips the subtree to a rounded rectangle.
    ClipRRect(ClipRRectLayer),
    /// Clips the subtree to a path.
    ClipPath(ClipPathLayer),
    /// Clips the subtree to a superellipse (iOS-style squircle).
    ClipSuperellipse(ClipSuperellipseLayer),
    /// Translates the subtree.
    Offset(OffsetLayer),
    /// Transforms the subtree by a matrix.
    Transform(TransformLayer),
    /// Blends the subtree at an alpha.
    Opacity(OpacityLayer),
    /// Recolours the subtree.
    ColorFilter(ColorFilterLayer),
    /// Filters the subtree's pixels (blur, dilate, erode).
    ImageFilter(ImageFilterLayer),
    /// Masks the subtree with a shader.
    ShaderMask(ShaderMaskLayer),
    /// Filters what is already behind the subtree.
    BackdropFilter(BackdropFilterLayer),
    /// Publishes an anchor followers position against.
    Leader(LeaderLayer),
    /// Positions its subtree relative to a leader.
    Follower(FollowerLayer),
    /// Attaches a value to a region for a reader above the tree.
    AnnotatedRegion(AnnotatedRegionLayer),
}

impl Layer {
    /// The rectangle this layer defines, for the variants that define one.
    ///
    /// Transform-like and effect-only containers (`Offset`, `Transform`,
    /// `Opacity`, `ColorFilter`, `ImageFilter`) have no bounds of their own,
    /// and a `Follower`'s depend on composite-time resolution.
    pub fn bounds(&self) -> Option<Rect<f64>> {
        match self {
            Layer::Canvas(layer) => layer.bounds(),
            Layer::Picture(layer) => layer.bounds(),
            Layer::Texture(layer) => Some(layer.bounds()),
            Layer::PlatformView(layer) => Some(layer.bounds()),
            Layer::PerformanceOverlay(layer) => Some(layer.bounds()),
            Layer::ClipRect(layer) => Some(layer.bounds()),
            Layer::ClipRRect(layer) => Some(layer.bounds()),
            Layer::ClipPath(layer) => Some(layer.bounds()),
            Layer::ClipSuperellipse(layer) => Some(layer.bounds()),
            Layer::ShaderMask(layer) => Some(layer.bounds()),
            Layer::BackdropFilter(layer) => Some(layer.bounds()),
            Layer::Leader(layer) => Some(layer.bounds()),
            Layer::AnnotatedRegion(layer) => Some(layer.bounds()),
            Layer::Offset(_)
            | Layer::Transform(_)
            | Layer::Opacity(_)
            | Layer::ColorFilter(_)
            | Layer::ImageFilter(_)
            | Layer::Follower(_) => None,
        }
    }

    /// The translation this layer applies to its children's coordinate
    /// system.
    ///
    /// This is the one place the set of translating variants is written down:
    /// each variant that carries an offset answers here, and both consumers —
    /// the GPU walk's pushes and the follower resolver's chain sums — read the
    /// same answer. A `Transform`
    /// contributes only its translation (the follower system is offset-only);
    /// a `Follower` contributes zero because its own translation is resolved
    /// by the walk rather than stored on the layer.
    pub fn local_translation(&self) -> Offset<f64> {
        match self {
            Layer::Offset(layer) => layer.offset(),
            Layer::Transform(layer) => {
                let (dx, dy, _dz) = layer.transform().translation_component();
                Offset::new(dx, dy)
            }
            Layer::Opacity(layer) => layer.offset(),
            Layer::ImageFilter(layer) => layer.offset(),
            Layer::Leader(layer) => layer.offset(),
            Layer::Canvas(_)
            | Layer::Picture(_)
            | Layer::Texture(_)
            | Layer::PlatformView(_)
            | Layer::PerformanceOverlay(_)
            | Layer::ClipRect(_)
            | Layer::ClipRRect(_)
            | Layer::ClipPath(_)
            | Layer::ClipSuperellipse(_)
            | Layer::ColorFilter(_)
            | Layer::ShaderMask(_)
            | Layer::BackdropFilter(_)
            | Layer::Follower(_)
            | Layer::AnnotatedRegion(_) => Offset::ZERO,
        }
    }

    /// The variant's name (`"Picture"`, `"ClipRect"`, …) for diagnostics and
    /// structural snapshots.
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

    /// Whether `self` and `other` apply the same effect to the subtree under
    /// them: the same variant with equal parameters.
    ///
    /// The question a damage differ asks of the layers above a repaint
    /// boundary: if an ancestor's opacity, clip or filter changed, the pixels
    /// the boundary's content lands on changed even though the content did
    /// not. Leaves (`Picture`, `Canvas`, `Texture`, `PlatformView`,
    /// `PerformanceOverlay`) apply no effect to a subtree and answer `false`;
    /// an `AnnotatedRegion` changes no pixel and answers `true` for any two.
    #[must_use]
    pub(crate) fn same_effect(&self, other: &Layer) -> bool {
        match (self, other) {
            (Layer::ClipRect(a), Layer::ClipRect(b)) => a == b,
            (Layer::ClipRRect(a), Layer::ClipRRect(b)) => a == b,
            (Layer::ClipPath(a), Layer::ClipPath(b)) => a == b,
            (Layer::ClipSuperellipse(a), Layer::ClipSuperellipse(b)) => a == b,
            (Layer::Offset(a), Layer::Offset(b)) => a == b,
            (Layer::Transform(a), Layer::Transform(b)) => a == b,
            (Layer::Opacity(a), Layer::Opacity(b)) => a == b,
            (Layer::ColorFilter(a), Layer::ColorFilter(b)) => a == b,
            (Layer::ImageFilter(a), Layer::ImageFilter(b)) => a == b,
            (Layer::ShaderMask(a), Layer::ShaderMask(b)) => a == b,
            (Layer::BackdropFilter(a), Layer::BackdropFilter(b)) => a == b,
            (Layer::Leader(a), Layer::Leader(b)) => a == b,
            (Layer::Follower(a), Layer::Follower(b)) => a == b,
            (Layer::AnnotatedRegion(_), Layer::AnnotatedRegion(_)) => true,
            _ => false,
        }
    }

    /// The leader payload, if this is a `Leader`.
    #[inline]
    pub fn as_leader(&self) -> Option<&LeaderLayer> {
        match self {
            Layer::Leader(layer) => Some(layer),
            _ => None,
        }
    }

    /// The follower payload, if this is a `Follower`.
    #[inline]
    pub fn as_follower(&self) -> Option<&FollowerLayer> {
        match self {
            Layer::Follower(layer) => Some(layer),
            _ => None,
        }
    }

    /// The overlay payload, if this is a `PerformanceOverlay`.
    #[inline]
    pub fn as_performance_overlay(&self) -> Option<&PerformanceOverlayLayer> {
        match self {
            Layer::PerformanceOverlay(layer) => Some(layer),
            _ => None,
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
    }
}

/// Normalises a caller-supplied alpha into `0.0..=1.0`.
///
/// `f64::clamp` lets `NaN` through, which would leave an `OpacityLayer` or
/// `TextureLayer` outside the range its type promises. `NaN` becomes 1.0 —
/// the effect disappears rather than the content — and a debug build trips,
/// since a `NaN` alpha is a caller bug.
pub(crate) fn unit_alpha(alpha: f64) -> f64 {
    debug_assert!(!alpha.is_nan(), "BUG: alpha must be a number in 0.0..=1.0");
    if alpha.is_nan() {
        1.0
    } else {
        alpha.clamp(0.0, 1.0)
    }
}

macro_rules! layer_from_impls {
    () => {};
    ( $variant:ident => boxed $ty:ty; $($rest:tt)* ) => {
        impl From<$ty> for Layer {
            fn from(layer: $ty) -> Self {
                Layer::$variant(Box::new(layer))
            }
        }
        layer_from_impls! { $($rest)* }
    };
    ( $variant:ident => $ty:ty; $($rest:tt)* ) => {
        impl From<$ty> for Layer {
            fn from(layer: $ty) -> Self {
                Layer::$variant(layer)
            }
        }
        layer_from_impls! { $($rest)* }
    };
}

layer_from_impls! {
    Canvas => boxed CanvasLayer;
    Picture => PictureLayer;
    Texture => TextureLayer;
    PlatformView => PlatformViewLayer;
    PerformanceOverlay => PerformanceOverlayLayer;
    ClipRect => ClipRectLayer;
    ClipRRect => ClipRRectLayer;
    ClipPath => ClipPathLayer;
    ClipSuperellipse => ClipSuperellipseLayer;
    Offset => OffsetLayer;
    Transform => TransformLayer;
    Opacity => OpacityLayer;
    ColorFilter => ColorFilterLayer;
    ImageFilter => ImageFilterLayer;
    ShaderMask => ShaderMaskLayer;
    BackdropFilter => BackdropFilterLayer;
    Leader => LeaderLayer;
    Follower => FollowerLayer;
    AnnotatedRegion => AnnotatedRegionLayer;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_effect_compares_variant_and_parameters() {
        let half = Layer::from(OpacityLayer::new(0.5));
        assert!(half.same_effect(&Layer::from(OpacityLayer::new(0.5))));
        assert!(!half.same_effect(&Layer::from(OpacityLayer::new(0.25))));
        assert!(!half.same_effect(&Layer::from(ColorFilterLayer::identity())));
        let clip = |w| {
            Layer::from(ClipRectLayer::new(
                Rect::from_xywh(0.0, 0.0, w, 10.0),
                flui_painting::paint::Clip::HardEdge,
            ))
        };
        assert!(clip(10.0).same_effect(&clip(10.0)));
        assert!(!clip(10.0).same_effect(&clip(20.0)));
        let backdrop = |sigma| {
            Layer::from(BackdropFilterLayer::new(
                flui_painting::paint::ImageFilter::blur(sigma),
                flui_painting::paint::BlendMode::SrcOver,
                Rect::from_xywh(0.0, 0.0, 10.0, 10.0),
            ))
        };
        assert!(backdrop(2.0).same_effect(&backdrop(2.0)));
        assert!(!backdrop(2.0).same_effect(&backdrop(3.0)));
        let picture = Layer::from(PictureLayer::default());
        assert!(
            !picture.same_effect(&picture.clone()),
            "a leaf is no effect"
        );
    }

    #[test]
    #[cfg_attr(debug_assertions, should_panic(expected = "alpha must be a number"))]
    fn nan_alpha_never_escapes_the_unit_range() {
        assert_eq!(unit_alpha(f64::NAN), 1.0);
    }
}
