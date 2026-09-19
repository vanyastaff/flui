//! `ClipPathLayer` — clips its subtree to a path.

use std::sync::Arc;

use flui_types::{
    geometry::{Pixels, Rect},
    painting::{Clip, Path},
};

/// Layer that clips children to an arbitrary path.
///
/// Path clipping enables complex shapes like circles, polygons,
/// bezier curves, and custom shapes defined by a `Path`.
///
/// # Performance
///
/// Path clipping is more expensive than rect or rounded rect clipping.
/// Use `ClipRectLayer` or `ClipRRectLayer` when possible.
///
/// # Architecture
///
/// ```text
/// ClipPathLayer
///   │
///   │ Apply path clip (GPU stencil buffer)
///   ▼
/// Children rendered within clipped path
/// ```
///
/// # Example
///
/// ```rust
/// use flui_types::geometry::px;
/// use flui_layer::ClipPathLayer;
/// use flui_types::{
///     geometry::Point,
///     painting::{Clip, Path},
/// };
///
/// // Create a triangular clip path
/// let path = Path::polygon(&[
///     Point::new(px(50.0), px(0.0)),
///     Point::new(px(100.0), px(100.0)),
///     Point::new(px(0.0), px(100.0)),
/// ]);
/// let layer = ClipPathLayer::new(path, Clip::AntiAlias);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ClipPathLayer {
    /// The path to clip to, shared with the render object that produced it
    /// so a coordinate query never copies a command buffer.
    clip_path: Arc<Path>,

    /// Clip behavior (HardEdge, AntiAlias, etc.)
    clip_behavior: Clip,
}

impl ClipPathLayer {
    /// Clips the subtree to a path; `Clip::None` is accepted and lowered as no clip.
    #[inline]
    pub fn new(clip_path: impl Into<Arc<Path>>, clip_behavior: Clip) -> Self {
        Self {
            clip_path: clip_path.into(),
            clip_behavior,
        }
    }

    /// Anti-aliased clip edge.
    #[inline]
    pub fn anti_alias(clip_path: Path) -> Self {
        Self::new(clip_path, Clip::AntiAlias)
    }

    /// Aliased clip: cheapest, jagged on curves and diagonals.
    #[inline]
    pub fn hard_edge(clip_path: Path) -> Self {
        Self::new(clip_path, Clip::HardEdge)
    }

    /// The clip shape.
    #[inline]
    pub fn clip_path(&self) -> &Path {
        &self.clip_path
    }

    /// How the edge is rasterised; `Clip::None` means no clip.
    #[inline]
    pub fn clip_behavior(&self) -> Clip {
        self.clip_behavior
    }

    /// The clip shape's bounding box.
    #[inline]
    pub fn bounds(&self) -> Rect<Pixels> {
        self.clip_path.compute_bounds()
    }

    /// Whether the layer clips at all (`clip_behavior != Clip::None`).
    #[inline]
    pub fn clips(&self) -> bool {
        self.clip_behavior.clips()
    }

    /// Whether the path has no segments (everything is clipped away).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.clip_path.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use flui_types::geometry::{Point, px};

    use super::*;

    #[test]
    fn test_clip_path_layer_new() {
        let path = Path::rectangle(Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0)));
        let layer = ClipPathLayer::new(path.clone(), Clip::AntiAlias);

        assert_eq!(layer.clip_path(), &path);
        assert_eq!(layer.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn test_clip_path_layer_anti_alias() {
        let path = Path::circle(Point::new(px(50.0), px(50.0)), 25.0);
        let layer = ClipPathLayer::anti_alias(path);

        assert_eq!(layer.clip_behavior(), Clip::AntiAlias);
        assert!(layer.clips());
    }

    #[test]
    fn test_clip_path_layer_hard_edge() {
        let path = Path::circle(Point::new(px(50.0), px(50.0)), 25.0);
        let layer = ClipPathLayer::hard_edge(path);

        assert_eq!(layer.clip_behavior(), Clip::HardEdge);
        assert!(layer.clips());
    }

    #[test]
    fn test_clip_path_layer_polygon() {
        let path = Path::polygon(&[
            Point::new(px(50.0), px(0.0)),
            Point::new(px(100.0), px(100.0)),
            Point::new(px(0.0), px(100.0)),
        ]);
        let layer = ClipPathLayer::new(path, Clip::AntiAlias);

        assert!(!layer.is_empty());
        assert!(layer.clips());
    }

    #[test]
    fn test_clip_path_layer_empty() {
        let path = Path::new();
        let layer = ClipPathLayer::new(path, Clip::HardEdge);

        assert!(layer.is_empty());
    }
}
