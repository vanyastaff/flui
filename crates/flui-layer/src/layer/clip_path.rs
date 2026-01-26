//! ClipPathLayer - Arbitrary path clipping layer
//!
//! This layer clips its children to an arbitrary path shape.
//! Corresponds to Flutter's `ClipPathLayer`.

use flui_types::geometry::{Pixels, Rect};
use flui_types::painting::{Clip, Path};

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
/// use flui_layer::ClipPathLayer;
/// use flui_types::painting::{Path, Clip};
/// use flui_types::geometry::Point;
///
/// // Create a triangular clip path
/// let path = Path::polygon(&[
///     Point::new(50.0, 0.0),
///     Point::new(100.0, 100.0),
///     Point::new(0.0, 100.0),
/// ]);
/// let layer = ClipPathLayer::new(path, Clip::AntiAlias);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ClipPathLayer {
    /// The path to clip to
    clip_path: Path,

    /// Clip behavior (HardEdge, AntiAlias, etc.)
    clip_behavior: Clip,
}

impl ClipPathLayer {
    /// Creates a new clip path layer.
    ///
    /// # Arguments
    ///
    /// * `clip_path` - The path to clip to
    /// * `clip_behavior` - How to apply the clip
    #[inline]
    pub fn new(clip_path: Path, clip_behavior: Clip) -> Self {
        Self {
            clip_path,
            clip_behavior,
        }
    }

    /// Creates a clip path layer with anti-aliased clipping.
    ///
    /// Anti-aliasing is recommended for path clipping to avoid
    /// jagged edges on curves and diagonals.
    #[inline]
    pub fn anti_alias(clip_path: Path) -> Self {
        Self::new(clip_path, Clip::AntiAlias)
    }

    /// Creates a clip path layer with hard edge clipping.
    #[inline]
    pub fn hard_edge(clip_path: Path) -> Self {
        Self::new(clip_path, Clip::HardEdge)
    }

    /// Creates a circular clip path.
    ///
    /// # Arguments
    ///
    /// * `center` - Center point of the circle
    /// * `radius` - Radius of the circle
    /// * `clip_behavior` - How to apply the clip
    #[inline]
    pub fn circle(
        center: flui_types::geometry::Point<flui_types::Pixels>,
        radius: f32,
        clip_behavior: Clip,
    ) -> Self {
        Self::new(Path::circle(center, radius), clip_behavior)
    }

    /// Creates an oval (ellipse) clip path.
    ///
    /// # Arguments
    ///
    /// * `rect` - Bounding rectangle of the oval
    /// * `clip_behavior` - How to apply the clip
    #[inline]
    pub fn oval(rect: Rect<Pixels>, clip_behavior: Clip) -> Self {
        Self::new(Path::oval(rect), clip_behavior)
    }

    /// Returns a reference to the clipping path.
    #[inline]
    pub fn clip_path(&self) -> &Path {
        &self.clip_path
    }

    /// Sets the clipping path.
    #[inline]
    pub fn set_clip_path(&mut self, clip_path: Path) {
        self.clip_path = clip_path;
    }

    /// Returns the clip behavior.
    #[inline]
    pub fn clip_behavior(&self) -> Clip {
        self.clip_behavior
    }

    /// Sets the clip behavior.
    #[inline]
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) {
        self.clip_behavior = clip_behavior;
    }

    /// Returns the bounding rectangle of this layer.
    ///
    /// This is the bounding box of the path, which may be larger
    /// than the actual clipped area for complex paths.
    #[inline]
    pub fn bounds(&self) -> Rect<Pixels> {
        self.clip_path.compute_bounds()
    }

    /// Returns true if this layer performs actual clipping.
    #[inline]
    pub fn clips(&self) -> bool {
        self.clip_behavior.clips()
    }

    /// Returns true if this layer uses anti-aliased clipping.
    #[inline]
    pub fn is_anti_aliased(&self) -> bool {
        self.clip_behavior.is_anti_aliased()
    }

    /// Returns true if the clipping path is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.clip_path.is_empty()
    }
}

// Thread safety
unsafe impl Send for ClipPathLayer {}
unsafe impl Sync for ClipPathLayer {}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::{px, Point};

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

        assert!(layer.is_anti_aliased());
        assert!(layer.clips());
    }

    #[test]
    fn test_clip_path_layer_hard_edge() {
        let path = Path::circle(Point::new(px(50.0), px(50.0)), 25.0);
        let layer = ClipPathLayer::hard_edge(path);

        assert!(!layer.is_anti_aliased());
        assert!(layer.clips());
    }

    #[test]
    fn test_clip_path_layer_circle() {
        let center = Point::new(px(50.0), px(50.0));
        let radius = 25.0;
        let layer = ClipPathLayer::circle(center, radius, Clip::AntiAlias);

        let bounds = layer.bounds();
        assert!((bounds.left() - px(25.0)).abs() < px(0.01));
        assert!((bounds.top() - px(25.0)).abs() < px(0.01));
        assert!((bounds.width() - px(50.0)).abs() < px(0.01));
        assert!((bounds.height() - px(50.0)).abs() < px(0.01));
    }

    #[test]
    fn test_clip_path_layer_oval() {
        let rect = Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(50.0));
        let layer = ClipPathLayer::oval(rect, Clip::AntiAlias);

        assert_eq!(layer.bounds(), rect);
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

    #[test]
    fn test_clip_path_layer_setters() {
        let path1 = Path::circle(Point::new(px(25.0), px(25.0)), 10.0);
        let path2 = Path::circle(Point::new(px(50.0), px(50.0)), 20.0);
        let mut layer = ClipPathLayer::new(path1, Clip::HardEdge);

        layer.set_clip_path(path2.clone());
        assert_eq!(layer.clip_path(), &path2);

        layer.set_clip_behavior(Clip::AntiAlias);
        assert_eq!(layer.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn test_clip_path_layer_clone() {
        let path = Path::rectangle(Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0)));
        let layer = ClipPathLayer::new(path, Clip::AntiAlias);
        let cloned = layer.clone();

        assert_eq!(layer, cloned);
    }

    #[test]
    fn test_clip_path_layer_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<ClipPathLayer>();
        assert_sync::<ClipPathLayer>();
    }
}
