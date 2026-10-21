//! Finding the annotations a composited scene places at a point.

use std::{any::Any, sync::Arc};

use flui_foundation::{LayerId, geometry::Point};
use flui_painting::paint::Clip;

use crate::{Layer, LayerTree, link::resolve_follower_offset};

impl LayerTree {
    /// Every `T` an [`AnnotatedRegionLayer`](crate::AnnotatedRegionLayer)
    /// places at `point` (scene coordinates), topmost first.
    ///
    /// The point maps through each layer's offset or transform, follower
    /// placement included; a clip that excludes it hides that subtree, and so
    /// does a transform that is not invertible or that puts the point's
    /// intersection with the subtree's plane behind the camera. Later siblings paint above
    /// earlier ones and a region's own children above the region, so they are
    /// reported first. Annotations of other types are skipped.
    #[must_use]
    pub fn find_annotations<T: Any + Send + Sync>(&self, point: Point<f64>) -> Vec<Arc<T>> {
        let mut found = Vec::new();
        self.collect_annotations(self.root(), point, &mut found);
        found
    }

    /// The topmost `T` at `point`, as [`Self::find_annotations`] orders them.
    #[must_use]
    pub fn find_annotation<T: Any + Send + Sync>(&self, point: Point<f64>) -> Option<Arc<T>> {
        self.find_annotations(point).into_iter().next()
    }

    fn collect_annotations<T: Any + Send + Sync>(
        &self,
        id: LayerId,
        point: Point<f64>,
        found: &mut Vec<Arc<T>>,
    ) {
        let Some(node) = self.get(id) else {
            return;
        };
        let layer = node.layer();
        let Some(local) = self.point_in_layer(id, layer, point) else {
            return;
        };
        if !clip_admits(layer, local) {
            return;
        }
        for &child in node.children().iter().rev() {
            self.collect_annotations(child, local, found);
        }
        if let Layer::AnnotatedRegion(region) = layer
            && (region.is_sized_by_parent() || region.bounds().contains(local))
            && let Some(value) = region.value().downcast::<T>()
        {
            found.push(value);
        }
    }

    /// `point` in this layer's own coordinates, or `None` when the layer's
    /// placement cannot be inverted or resolved.
    fn point_in_layer(&self, id: LayerId, layer: &Layer, point: Point<f64>) -> Option<Point<f64>> {
        let offset = match layer {
            Layer::Transform(transform) => {
                let inverse = transform.transform().try_inverse()?;
                let (x, y) = inverse.unproject_to_plane(point.x, point.y)?;
                return Some(Point::new(x, y));
            }
            Layer::Follower(_) => resolve_follower_offset(self, id)?,
            _ => layer.local_translation(),
        };
        Some(Point::new(point.x - offset.dx, point.y - offset.dy))
    }
}

/// Whether a clipping layer keeps `point`; non-clipping layers keep it.
fn clip_admits(layer: &Layer, point: Point<f64>) -> bool {
    let clips = |behavior: Clip| behavior != Clip::None;
    match layer {
        Layer::ClipRect(clip) => !clips(clip.clip_behavior()) || clip.clip_rect().contains(point),
        Layer::ClipRRect(clip) => !clips(clip.clip_behavior()) || clip.clip_rrect().contains(point),
        Layer::ClipSuperellipse(clip) => {
            !clips(clip.clip_behavior()) || clip.clip_superellipse().contains(point)
        }
        Layer::ClipPath(clip) => !clips(clip.clip_behavior()) || clip.clip_path().contains(point),
        _ => true,
    }
}
