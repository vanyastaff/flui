//! RenderClipOval - clips child to an oval/ellipse.
//!
//! This render object clips its child to an oval (ellipse) region.

use flui_types::{BoxConstraints, Offset, Point, Rect, Size};

use crate::containers::ProxyBox;
use crate::delegates::CustomClipper;
use crate::objects::r#box::effects::clip_rect::Clip;
use crate::pipeline::PaintingContext;
use crate::traits::TextBaseline;

/// A render object that clips its child to an oval.
///
/// The oval is inscribed in the rectangle defined by the render object's size
/// (or by a custom clipper).
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::r#box::effects::RenderClipOval;
///
/// // Clip to oval inscribed in bounds
/// let clip = RenderClipOval::new();
/// ```
#[derive(Debug)]
pub struct RenderClipOval {
    /// Container holding the child and geometry.
    proxy: ProxyBox,

    /// Optional custom clipper (provides the bounding rect for the oval).
    clipper: Option<Box<dyn CustomClipper<Rect>>>,

    /// The clip behavior.
    clip_behavior: Clip,
}

impl RenderClipOval {
    /// Creates a new clip oval render object.
    pub fn new() -> Self {
        Self {
            proxy: ProxyBox::new(),
            clipper: None,
            clip_behavior: Clip::AntiAlias,
        }
    }

    /// Creates with a custom clipper.
    pub fn with_clipper(clipper: Box<dyn CustomClipper<Rect>>) -> Self {
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

    /// Sets the custom clipper.
    pub fn set_clipper(&mut self, clipper: Option<Box<dyn CustomClipper<Rect>>>) {
        self.clipper = clipper;
    }

    /// Computes the bounding rect for the oval.
    pub fn get_clip(&self) -> Rect {
        let size = self.size();
        if let Some(clipper) = &self.clipper {
            clipper.get_clip(size)
        } else {
            Rect::from_origin_size(Point::ZERO, size)
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
        // context.push_clip_oval(offset, clip, self.clip_behavior, |ctx| {
        //     ctx.paint_child(child, offset)
        // });
        let _ = (context, offset, clip);
    }

    /// Hit test - only hits within the oval region.
    pub fn hit_test(&self, position: Offset) -> bool {
        let clip = self.get_clip();
        let center_x = clip.left() + clip.width() / 2.0;
        let center_y = clip.top() + clip.height() / 2.0;
        let rx = clip.width() / 2.0;
        let ry = clip.height() / 2.0;

        if rx <= 0.0 || ry <= 0.0 {
            return false;
        }

        // Check if point is inside ellipse: (x-cx)²/rx² + (y-cy)²/ry² <= 1
        let dx = position.dx - center_x;
        let dy = position.dy - center_y;
        (dx * dx) / (rx * rx) + (dy * dy) / (ry * ry) <= 1.0
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

impl Default for RenderClipOval {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clip_oval_new() {
        let clip = RenderClipOval::new();
        assert_eq!(clip.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn test_hit_test_center() {
        let mut clip = RenderClipOval::new();
        clip.perform_layout_with_child(
            BoxConstraints::tight(Size::new(100.0, 100.0)),
            Size::new(100.0, 100.0),
        );

        // Center of a circle should be inside
        assert!(clip.hit_test(Offset::new(50.0, 50.0)));
    }

    #[test]
    fn test_hit_test_on_edge() {
        let mut clip = RenderClipOval::new();
        clip.perform_layout_with_child(
            BoxConstraints::tight(Size::new(100.0, 100.0)),
            Size::new(100.0, 100.0),
        );

        // Right edge of circle (50, 50) + radius 50 = (100, 50)
        // Should be on the boundary
        assert!(clip.hit_test(Offset::new(100.0, 50.0)));
    }

    #[test]
    fn test_hit_test_corner() {
        let mut clip = RenderClipOval::new();
        clip.perform_layout_with_child(
            BoxConstraints::tight(Size::new(100.0, 100.0)),
            Size::new(100.0, 100.0),
        );

        // Corners are outside an inscribed circle/oval
        assert!(!clip.hit_test(Offset::new(0.0, 0.0)));
        assert!(!clip.hit_test(Offset::new(100.0, 0.0)));
        assert!(!clip.hit_test(Offset::new(0.0, 100.0)));
        assert!(!clip.hit_test(Offset::new(100.0, 100.0)));
    }

    #[test]
    fn test_hit_test_ellipse() {
        let mut clip = RenderClipOval::new();
        clip.perform_layout_with_child(
            BoxConstraints::tight(Size::new(200.0, 100.0)),
            Size::new(200.0, 100.0),
        );

        // Center
        assert!(clip.hit_test(Offset::new(100.0, 50.0)));
        // Along major axis
        assert!(clip.hit_test(Offset::new(199.0, 50.0)));
        // Corner (outside)
        assert!(!clip.hit_test(Offset::new(0.0, 0.0)));
    }

    #[test]
    fn test_layout() {
        let mut clip = RenderClipOval::new();
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 150.0);
        let child_size = Size::new(100.0, 75.0);

        let size = clip.perform_layout_with_child(constraints, child_size);

        assert_eq!(size, child_size);
    }
}
