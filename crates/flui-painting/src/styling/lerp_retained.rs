//! What a [`BoxDecoration::lerp`](super::BoxDecoration::lerp)(super::BoxDecoration::lerp) keeps beside the
//! fields it publishes: the bounded gradient and the blend of image layers.

use super::{DecorationImage, Gradient};

/// The images a [`BoxDecoration::lerp`](super::BoxDecoration::lerp) blends: each image with its share of
/// the blend folded into its opacity. The shares add up to one, and paint adds
/// the layers together, so an opaque crossfade covers the box at every `t`.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct ImageBlend {
    /// The `image` the lerp published beside the blend.
    pub(crate) published: DecorationImage,
    /// The blended layers, in paint order.
    pub(crate) layers: Vec<DecorationImage>,
}

impl ImageBlend {
    /// The most layers a blend keeps: a run retargeted from a blend blends its
    /// layers again. Past it the weakest layers other than the published one
    /// go, and the kept layers take up their shares.
    const MAX_LAYERS: usize = 4;

    /// `a`'s layers at `1 - t` of their opacity and `b`'s at `t`, as the
    /// decoration's `image` and the blend beside it. One layer needs no blend.
    ///
    /// The published `image` is the layer that holds `b`'s own published image
    /// (`target`), as the blend paints it, else the strongest layer. Past
    /// [`MAX_LAYERS`](Self::MAX_LAYERS) the weakest other layers go, and the ones
    /// kept take up their shares, so the blend covers the box as fully as
    /// before.
    pub(crate) fn mix(
        a: &[DecorationImage],
        b: &[DecorationImage],
        target: Option<&DecorationImage>,
        t: f64,
    ) -> (Option<DecorationImage>, Option<Self>) {
        let mut layers: Vec<DecorationImage> = Vec::new();
        let shares = a
            .iter()
            .map(|layer| (layer, 1.0 - t))
            .chain(b.iter().map(|layer| (layer, t)));
        for (layer, share) in shares {
            let opacity = layer.opacity * share;
            // A layer with no opacity, or a non-finite one from a non-finite `t`, is
            // no layer.
            if !(opacity > 0.0 && opacity.is_finite()) {
                continue;
            }
            // The same image on both sides is one layer: its shares add up.
            match layers.iter_mut().find(|kept| Self::same_image(kept, layer)) {
                Some(kept) => kept.opacity += opacity,
                None => layers.push(DecorationImage {
                    opacity,
                    ..layer.clone()
                }),
            }
        }

        let strongest = |layers: &[DecorationImage]| {
            layers
                .iter()
                .enumerate()
                .max_by(|(_, x), (_, y)| x.opacity.total_cmp(&y.opacity))
                .map(|(index, _)| index)
        };
        let mut published = target
            .and_then(|target| {
                layers
                    .iter()
                    .position(|kept| Self::same_image(kept, target))
            })
            .or_else(|| strongest(&layers));

        let total: f64 = layers.iter().map(|layer| layer.opacity).sum();
        while layers.len() > Self::MAX_LAYERS {
            let Some(weakest) = layers
                .iter()
                .enumerate()
                .filter(|(index, _)| Some(*index) != published)
                .min_by(|(_, x), (_, y)| x.opacity.total_cmp(&y.opacity))
                .map(|(index, _)| index)
            else {
                break;
            };
            layers.remove(weakest);
            published = published.map(|index| if weakest < index { index - 1 } else { index });
        }
        let kept: f64 = layers.iter().map(|layer| layer.opacity).sum();
        if kept > 0.0 && kept < total {
            let scale = total / kept;
            for layer in &mut layers {
                layer.opacity *= scale;
            }
        }

        let image = published.and_then(|index| layers.get(index)).cloned();
        if layers.len() < 2 {
            return (image, None);
        }
        let Some(published) = image else {
            return (None, None);
        };
        (Some(published.clone()), Some(Self { published, layers }))
    }

    /// Whether two layers draw the same image, whatever their opacity.
    fn same_image(x: &DecorationImage, y: &DecorationImage) -> bool {
        let DecorationImage {
            image,
            fit,
            alignment,
            repeat,
            opacity: _,
            color_filter,
        } = x;
        *image == y.image
            && *fit == y.fit
            && *alignment == y.alignment
            && *repeat == y.repeat
            && *color_filter == y.color_filter
    }
}

/// What a [`BoxDecoration::lerp`](super::BoxDecoration::lerp) keeps beside the public fields it published.
/// Each part is honoured only while the public field still holds what the lerp
/// wrote, so a direct write to `gradient` or `image` cannot reach stale state.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct LerpRetained {
    /// The raw gradient the lerp published and the bounded endpoint painted
    /// when the raw one cannot be represented at the box's size.
    pub(crate) gradient: Option<(Gradient, Gradient)>,
    /// The image layers the lerp blends.
    pub(crate) image: Option<ImageBlend>,
}

impl LerpRetained {
    /// The retained state, boxed, or nothing when no part is kept.
    pub(crate) fn boxed(
        gradient: Option<(Gradient, Gradient)>,
        image: Option<ImageBlend>,
    ) -> Option<Box<Self>> {
        (gradient.is_some() || image.is_some()).then(|| Box::new(Self { gradient, image }))
    }

    /// Replaces the retained gradient, keeping the image blend.
    pub(crate) fn set_gradient(
        retained: &mut Option<Box<Self>>,
        gradient: Option<(Gradient, Gradient)>,
    ) {
        let image = retained.take().and_then(|kept| kept.image);
        *retained = Self::boxed(gradient, image);
    }
}
