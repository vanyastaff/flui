//! [`Effect`]: a layer a retained scene's content may open, which is every
//! layer but a picture.

use super::{
    AnnotatedRegionLayer, BackdropFilterLayer, ClipPathLayer, ClipRRectLayer, ClipRectLayer,
    ClipSuperellipseLayer, ColorFilterLayer, FollowerLayer, ImageFilterLayer, Layer, LeaderLayer,
    OffsetLayer, OpacityLayer, PictureLayer, ShaderMaskLayer, TransformLayer,
};

/// A container layer: the clip, transform, filter, link or annotation a
/// boundary's content applies to the entries under it
/// ([`ContentBuilder::push_effect`](crate::ContentBuilder::push_effect)).
///
/// It holds any [`Layer`] but a [`Picture`](Layer::Picture): what a content
/// draws is a [`Chunk`](crate::Chunk) of recorded commands, so an effect that
/// drew would be a second way to draw that every reader of the stream would
/// have to answer.
///
/// ```rust
/// use flui_layer::{Effect, Layer, OpacityLayer, PictureLayer};
///
/// let opacity = Effect::from(OpacityLayer::new(0.5));
/// assert_eq!(opacity.as_layer().kind_name(), "Opacity");
///
/// let picture = Layer::from(PictureLayer::default());
/// assert!(Effect::try_from(picture).is_err());
/// ```
#[derive(Debug, Clone)]
pub struct Effect(Layer);

impl Effect {
    /// The layer it opens.
    #[must_use]
    pub fn as_layer(&self) -> &Layer {
        &self.0
    }

    /// The layer it opens, by value.
    #[must_use]
    pub fn into_layer(self) -> Layer {
        self.0
    }
}

impl From<Effect> for Layer {
    fn from(effect: Effect) -> Self {
        effect.0
    }
}

/// A picture is no effect: the error hands it back.
impl TryFrom<Layer> for Effect {
    type Error = PictureLayer;

    fn try_from(layer: Layer) -> Result<Self, Self::Error> {
        match layer {
            Layer::Picture(picture) => Err(picture),
            Layer::ClipRect(_)
            | Layer::ClipRRect(_)
            | Layer::ClipPath(_)
            | Layer::ClipSuperellipse(_)
            | Layer::Offset(_)
            | Layer::Transform(_)
            | Layer::Opacity(_)
            | Layer::ColorFilter(_)
            | Layer::ImageFilter(_)
            | Layer::ShaderMask(_)
            | Layer::BackdropFilter(_)
            | Layer::Leader(_)
            | Layer::Follower(_)
            | Layer::AnnotatedRegion(_) => Ok(Self(layer)),
        }
    }
}

macro_rules! effect_from_impls {
    ($($ty:ty),* $(,)?) => {
        $(
            impl From<$ty> for Effect {
                fn from(layer: $ty) -> Self {
                    Self(Layer::from(layer))
                }
            }
        )*
    };
}

effect_from_impls!(
    ClipRectLayer,
    ClipRRectLayer,
    ClipPathLayer,
    ClipSuperellipseLayer,
    OffsetLayer,
    TransformLayer,
    OpacityLayer,
    ColorFilterLayer,
    ImageFilterLayer,
    ShaderMaskLayer,
    BackdropFilterLayer,
    LeaderLayer,
    FollowerLayer,
    AnnotatedRegionLayer,
);
