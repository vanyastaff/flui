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
use flui_foundation::{Diagnosticable, DiagnosticsBuilder, DiagnosticsNode};
use flui_types::geometry::{Offset, Pixels, Rect};
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
/// use flui_types::{geometry::{Rect, px}, painting::Clip};
///
/// let clip = Layer::from(ClipRectLayer::new(
///     Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0)),
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
    pub fn bounds(&self) -> Option<Rect<Pixels>> {
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
    pub fn local_translation(&self) -> Offset<Pixels> {
        match self {
            Layer::Offset(layer) => layer.offset(),
            Layer::Transform(layer) => {
                let (dx, dy, _dz) = layer.transform().translation_component();
                Offset::new(Pixels::new(dx), Pixels::new(dy))
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
/// `f32::clamp` lets `NaN` through, which would leave an `OpacityLayer` or
/// `TextureLayer` outside the range its type promises. `NaN` becomes 1.0 —
/// the effect disappears rather than the content — and a debug build trips,
/// since a `NaN` alpha is a caller bug.
pub(crate) fn unit_alpha(alpha: f32) -> f32 {
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
    use flui_types::{
        geometry::{Size, px},
        painting::Clip,
    };

    use super::*;
    use crate::LayerLink;

    #[test]
    fn bounds_come_from_the_payload() {
        let layer = Layer::from(ClipRectLayer::new(
            Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(50.0)),
            Clip::HardEdge,
        ));
        assert_eq!(
            layer.bounds(),
            Some(Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(50.0)))
        );
        assert_eq!(Layer::from(OpacityLayer::new(0.5)).bounds(), None);
    }

    #[test]
    fn local_translation_covers_every_offset_carrying_variant() {
        let offset = Offset::new(px(3.0), px(4.0));
        let translating: [Layer; 5] = [
            OffsetLayer::new(offset).into(),
            TransformLayer::translation(3.0, 4.0).into(),
            OpacityLayer::with_offset(0.5, offset).into(),
            ImageFilterLayer::with_offset(flui_types::painting::ImageFilter::blur(1.0), offset)
                .into(),
            LeaderLayer::with_offset(LayerLink::new(), Size::ZERO, offset).into(),
        ];
        for layer in &translating {
            assert_eq!(layer.local_translation(), offset, "{}", layer.kind_name());
        }
        assert_eq!(
            Layer::from(FollowerLayer::new(LayerLink::new()).with_target_offset(offset))
                .local_translation(),
            Offset::ZERO,
            "a follower's translation is resolved by the walk, not stored on the layer"
        );
        assert_eq!(
            Layer::from(ClipRectLayer::new(Rect::ZERO, Clip::HardEdge)).local_translation(),
            Offset::ZERO
        );
    }

    #[test]
    fn accessors_match_their_variant_only() {
        let link = LayerLink::new();
        let leader = Layer::from(LeaderLayer::new(link, Size::ZERO));
        assert!(leader.as_leader().is_some());
        assert!(leader.as_follower().is_none());
        assert!(leader.as_performance_overlay().is_none());
        assert!(
            Layer::from(FollowerLayer::new(link))
                .as_follower()
                .is_some()
        );
    }

    #[test]
    #[cfg_attr(debug_assertions, should_panic(expected = "alpha must be a number"))]
    fn nan_alpha_never_escapes_the_unit_range() {
        assert_eq!(unit_alpha(f32::NAN), 1.0);
    }

    #[test]
    fn alpha_is_clamped_to_the_unit_range() {
        assert_eq!(unit_alpha(-2.0), 0.0);
        assert_eq!(unit_alpha(0.25), 0.25);
        assert_eq!(unit_alpha(7.0), 1.0);
        assert_eq!(OpacityLayer::new(f32::INFINITY).alpha(), 1.0);
    }

    /// The enum's footprint is a hot-path number: every node holds one inline.
    /// `Canvas` stays boxed (a live recorder is ~184 B); everything else fits
    /// the budget unboxed. Re-measure before boxing more — a box is one heap
    /// allocation per layer per frame.
    #[test]
    fn layer_fits_the_inline_budget() {
        const BUDGET: usize = 128;
        let size = std::mem::size_of::<Layer>();
        assert!(size <= BUDGET, "size_of::<Layer>() = {size} > {BUDGET}");
    }
}
