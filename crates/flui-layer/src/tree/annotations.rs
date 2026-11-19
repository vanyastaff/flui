//! Finding the annotations a composited scene places at a point.

use std::{any::Any, sync::Arc};

use flui_foundation::{LayerId, geometry::Point};

use crate::{Layer, LayerTree, tree::follower::resolve_follower_transform};

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
        if !layer.clip_admits(local) {
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
        let transform = match layer {
            Layer::Transform(transform) => *transform.transform(),
            Layer::Follower(_) => resolve_follower_transform(self, id)?,
            _ => {
                let offset = layer.local_translation();
                return Some(Point::new(point.x - offset.dx, point.y - offset.dy));
            }
        };
        let inverse = transform.try_inverse()?;
        let (x, y) = inverse.unproject_to_plane(point.x, point.y)?;
        Some(Point::new(x, y))
    }
}
