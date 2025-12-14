//! RenderClipPath - clips child to an arbitrary path.
//!
//! This render object clips its child to a path defined by a custom clipper.

use flui_types::{BoxConstraints, Offset, Point, Rect, Size};

use crate::containers::ProxyBox;
use crate::delegates::CustomClipper;
use crate::objects::r#box::effects::clip_rect::Clip;
use crate::pipeline::PaintingContext;
use crate::traits::TextBaseline;
use flui_types::painting::Path;

/// A render object that clips its child to an arbitrary path.
///
/// Unlike other clip render objects, this one requires a custom clipper
/// because there's no sensible default path.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::r#box::effects::RenderClipPath;
///
/// // Create with a custom path clipper
/// let clip = RenderClipPath::with_clipper(my_clipper);
/// ```
#[derive(Debug)]
pub struct RenderClipPath {
    /// Container holding the child and geometry.
    proxy: ProxyBox,

    /// Custom clipper that provides the path.
    clipper: Option<Box<dyn CustomClipper<Path>>>,

    /// The clip behavior.
    clip_behavior: Clip,
}

impl RenderClipPath {
    /// Creates a new clip path render object without a clipper.
    ///
    /// Note: Without a clipper, this will clip to a rectangle (the bounds).
    pub fn new() -> Self {
        Self {
            proxy: ProxyBox::new(),
            clipper: None,
            clip_behavior: Clip::AntiAlias,
        }
    }

    /// Creates with a custom clipper.
    pub fn with_clipper(clipper: Box<dyn CustomClipper<Path>>) -> Self {
        Self {
            proxy: ProxyBox::new(),
            clipper: Some(clipper),
            clip_behavior: Clip::AntiAlias,
        }
    }

    /// Returns the clip behavior.
    pub fn clip_behavior(&self) -> Clip {
        self.clip_behavior
    }

    /// Sets the clip behavior.
    pub fn set_clip_behavior(&mut self, behavior: Clip) {
        if self.clip_behavior != behavior {
            self.clip_behavior = behavior;
        }
    }

    /// Returns whether a clipper is set.
    pub fn has_clipper(&self) -> bool {
        self.clipper.is_some()
    }

    /// Sets the custom clipper.
    pub fn set_clipper(&mut self, clipper: Option<Box<dyn CustomClipper<Path>>>) {
        self.clipper = clipper;
    }

    /// Computes the clip path.
    pub fn get_clip(&self) -> Path {
        let size = self.size();
        if let Some(clipper) = &self.clipper {
            clipper.get_clip(size)
        } else {
            // Default to rectangle path
            let mut path = Path::new();
            path.add_rect(Rect::from_origin_size(Point::ZERO, size));
            path
        }
    }

    /// Returns the current size.
    pub fn size(&self) -> Size {
        *self.proxy.geometry()
    }

    /// Performs layout without a child.
    pub fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        let size = constraints.smallest();
        self.proxy.set_geometry(size);
        size
    }

    /// Performs layout with a child size.
    pub fn perform_layout_with_child(
        &mut self,
        _constraints: BoxConstraints,
        child_size: Size,
    ) -> Size {
        self.proxy.set_geometry(child_size);
        child_size
    }

    /// Returns constraints for the child.
    pub fn constraints_for_child(&self, constraints: BoxConstraints) -> BoxConstraints {
        constraints
    }

    /// Paints this render object.
    pub fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        if self.clip_behavior == Clip::None {
            let _ = (context, offset);
            return;
        }

        let clip = self.get_clip();
        // In real implementation:
        // context.push_clip_path(offset, clip, self.clip_behavior, |ctx| {
        //     ctx.paint_child(child, offset)
        // });
        let _ = (context, offset, clip);
    }

    /// Hit test - only hits within the path region.
    ///
    /// Note: Path hit testing requires point-in-path calculation which
    /// is complex. For now, we fall back to bounding box.
    pub fn hit_test(&self, position: Offset) -> bool {
        let mut clip = self.get_clip();
        let bounds = clip.bounds();
        bounds.contains(Point::new(position.dx, position.dy))
    }

    /// Computes minimum intrinsic width.
    pub fn compute_min_intrinsic_width(&self, _height: f32, child_width: Option<f32>) -> f32 {
        child_width.unwrap_or(0.0)
    }

    /// Computes maximum intrinsic width.
    pub fn compute_max_intrinsic_width(&self, _height: f32, child_width: Option<f32>) -> f32 {
        child_width.unwrap_or(0.0)
    }

    /// Computes minimum intrinsic height.
    pub fn compute_min_intrinsic_height(&self, _width: f32, child_height: Option<f32>) -> f32 {
        child_height.unwrap_or(0.0)
    }

    /// Computes maximum intrinsic height.
    pub fn compute_max_intrinsic_height(&self, _width: f32, child_height: Option<f32>) -> f32 {
        child_height.unwrap_or(0.0)
    }

    /// Computes distance to baseline.
    pub fn compute_distance_to_baseline(
        &self,
        _baseline: TextBaseline,
        child_baseline: Option<f32>,
    ) -> Option<f32> {
        child_baseline
    }
}

impl Default for RenderClipPath {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clip_path_new() {
        let clip = RenderClipPath::new();
        assert_eq!(clip.clip_behavior(), Clip::AntiAlias);
        assert!(!clip.has_clipper());
    }

    #[test]
    fn test_default_path() {
        let mut clip = RenderClipPath::new();
        clip.perform_layout_with_child(
            BoxConstraints::tight(Size::new(100.0, 80.0)),
            Size::new(100.0, 80.0),
        );

        let mut path = clip.get_clip();
        let bounds = path.bounds();
        assert!((bounds.width() - 100.0).abs() < f32::EPSILON);
        assert!((bounds.height() - 80.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_hit_test_inside() {
        let mut clip = RenderClipPath::new();
        clip.perform_layout_with_child(
            BoxConstraints::tight(Size::new(100.0, 100.0)),
            Size::new(100.0, 100.0),
        );

        assert!(clip.hit_test(Offset::new(50.0, 50.0)));
    }

    #[test]
    fn test_hit_test_outside() {
        let mut clip = RenderClipPath::new();
        clip.perform_layout_with_child(
            BoxConstraints::tight(Size::new(100.0, 100.0)),
            Size::new(100.0, 100.0),
        );

        assert!(!clip.hit_test(Offset::new(150.0, 50.0)));
    }

    #[test]
    fn test_layout() {
        let mut clip = RenderClipPath::new();
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 150.0);
        let child_size = Size::new(100.0, 75.0);

        let size = clip.perform_layout_with_child(constraints, child_size);

        assert_eq!(size, child_size);
    }
}
