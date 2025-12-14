//! RenderClipRRect - clips child to a rounded rectangle.
//!
//! This render object clips its child to a rounded rectangular region.

use flui_types::{geometry::Radius, BoxConstraints, Offset, Point, RRect, Rect, Size};

use crate::containers::ProxyBox;
use crate::delegates::CustomClipper;
use crate::objects::r#box::effects::clip_rect::Clip;
use crate::pipeline::PaintingContext;
use crate::traits::TextBaseline;

/// A render object that clips its child to a rounded rectangle.
///
/// By default, clips to the bounds with the specified border radius.
/// A custom clipper can be provided for different rounded shapes.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::r#box::effects::RenderClipRRect;
/// use flui_types::Radius;
///
/// // Clip with uniform radius
/// let clip = RenderClipRRect::with_radius(Radius::circular(10.0));
///
/// // Clip with different corner radii
/// let clip = RenderClipRRect::new();
/// ```
#[derive(Debug)]
pub struct RenderClipRRect {
    /// Container holding the child and geometry.
    proxy: ProxyBox,

    /// The border radius.
    border_radius: BorderRadius,

    /// Optional custom clipper.
    clipper: Option<Box<dyn CustomClipper<RRect>>>,

    /// The clip behavior.
    clip_behavior: Clip,
}

/// Border radius for all four corners.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BorderRadius {
    /// Top-left corner radius.
    pub top_left: Radius,
    /// Top-right corner radius.
    pub top_right: Radius,
    /// Bottom-left corner radius.
    pub bottom_left: Radius,
    /// Bottom-right corner radius.
    pub bottom_right: Radius,
}

impl BorderRadius {
    /// Zero radius (sharp corners).
    pub const ZERO: Self = Self {
        top_left: Radius::ZERO,
        top_right: Radius::ZERO,
        bottom_left: Radius::ZERO,
        bottom_right: Radius::ZERO,
    };

    /// Creates a uniform border radius.
    pub fn all(radius: Radius) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_left: radius,
            bottom_right: radius,
        }
    }

    /// Creates a circular border radius (same x and y).
    pub fn circular(radius: f32) -> Self {
        Self::all(Radius::circular(radius))
    }

    /// Creates with different radii for each corner.
    pub fn only(
        top_left: Radius,
        top_right: Radius,
        bottom_left: Radius,
        bottom_right: Radius,
    ) -> Self {
        Self {
            top_left,
            top_right,
            bottom_left,
            bottom_right,
        }
    }

    /// Creates with horizontal corners only.
    pub fn horizontal(left: Radius, right: Radius) -> Self {
        Self {
            top_left: left,
            top_right: right,
            bottom_left: left,
            bottom_right: right,
        }
    }

    /// Creates with vertical corners only.
    pub fn vertical(top: Radius, bottom: Radius) -> Self {
        Self {
            top_left: top,
            top_right: top,
            bottom_left: bottom,
            bottom_right: bottom,
        }
    }

    /// Converts to an RRect with the given rect.
    pub fn to_rrect(&self, rect: Rect) -> RRect {
        RRect::from_rect_and_corners(
            rect,
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        )
    }
}

impl Default for BorderRadius {
    fn default() -> Self {
        Self::ZERO
    }
}

impl RenderClipRRect {
    /// Creates a new clip rrect with zero border radius.
    pub fn new() -> Self {
        Self {
            proxy: ProxyBox::new(),
            border_radius: BorderRadius::ZERO,
            clipper: None,
            clip_behavior: Clip::AntiAlias,
        }
    }

    /// Creates with a uniform radius.
    pub fn with_radius(radius: Radius) -> Self {
        Self {
            proxy: ProxyBox::new(),
            border_radius: BorderRadius::all(radius),
            clipper: None,
            clip_behavior: Clip::AntiAlias,
        }
    }

    /// Creates with a circular radius.
    pub fn circular(radius: f32) -> Self {
        Self::with_radius(Radius::circular(radius))
    }

    /// Creates with a border radius.
    pub fn with_border_radius(border_radius: BorderRadius) -> Self {
        Self {
            proxy: ProxyBox::new(),
            border_radius,
            clipper: None,
            clip_behavior: Clip::AntiAlias,
        }
    }

    /// Creates with a custom clipper.
    pub fn with_clipper(clipper: Box<dyn CustomClipper<RRect>>) -> Self {
        Self {
            proxy: ProxyBox::new(),
            border_radius: BorderRadius::ZERO,
            clipper: Some(clipper),
            clip_behavior: Clip::AntiAlias,
        }
    }

    /// Returns the border radius.
    pub fn border_radius(&self) -> BorderRadius {
        self.border_radius
    }

    /// Sets the border radius.
    pub fn set_border_radius(&mut self, radius: BorderRadius) {
        if self.border_radius != radius {
            self.border_radius = radius;
            // In real implementation: self.mark_needs_paint();
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
            // In real implementation: self.mark_needs_paint();
        }
    }

    /// Sets the custom clipper.
    pub fn set_clipper(&mut self, clipper: Option<Box<dyn CustomClipper<RRect>>>) {
        self.clipper = clipper;
        // In real implementation: self.mark_needs_paint();
    }

    /// Computes the clip rrect.
    pub fn get_clip(&self) -> RRect {
        let size = self.size();
        if let Some(clipper) = &self.clipper {
            clipper.get_clip(size)
        } else {
            let rect = Rect::from_origin_size(Point::ZERO, size);
            self.border_radius.to_rrect(rect)
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
            // No clipping
            let _ = (context, offset);
            return;
        }

        let clip = self.get_clip();
        // In real implementation:
        // context.push_clip_rrect(offset, clip, self.clip_behavior, |ctx| {
        //     ctx.paint_child(child, offset)
        // });
        let _ = (context, offset, clip);
    }

    /// Hit test - only hits within the clip region.
    pub fn hit_test(&self, position: Offset) -> bool {
        let clip = self.get_clip();
        clip.contains(Point::new(position.dx, position.dy))
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

impl Default for RenderClipRRect {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clip_rrect_new() {
        let clip = RenderClipRRect::new();
        assert_eq!(clip.border_radius(), BorderRadius::ZERO);
        assert_eq!(clip.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn test_clip_rrect_circular() {
        let clip = RenderClipRRect::circular(10.0);
        assert_eq!(clip.border_radius().top_left, Radius::circular(10.0));
    }

    #[test]
    fn test_border_radius_all() {
        let radius = BorderRadius::all(Radius::circular(5.0));
        assert_eq!(radius.top_left, radius.top_right);
        assert_eq!(radius.top_left, radius.bottom_left);
        assert_eq!(radius.top_left, radius.bottom_right);
    }

    #[test]
    fn test_border_radius_to_rrect() {
        let radius = BorderRadius::circular(10.0);
        let rect = Rect::from_origin_size(Point::ZERO, Size::new(100.0, 80.0));
        let rrect = radius.to_rrect(rect);

        assert!((rrect.rect.width() - 100.0).abs() < f32::EPSILON);
        assert!((rrect.rect.height() - 80.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_get_clip() {
        let mut clip = RenderClipRRect::circular(10.0);
        clip.perform_layout_with_child(
            BoxConstraints::tight(Size::new(100.0, 80.0)),
            Size::new(100.0, 80.0),
        );

        let rrect = clip.get_clip();
        assert!((rrect.rect.width() - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_hit_test_center() {
        let mut clip = RenderClipRRect::circular(10.0);
        clip.perform_layout_with_child(
            BoxConstraints::tight(Size::new(100.0, 100.0)),
            Size::new(100.0, 100.0),
        );

        // Center should always be inside
        assert!(clip.hit_test(Offset::new(50.0, 50.0)));
    }

    #[test]
    fn test_hit_test_outside() {
        let mut clip = RenderClipRRect::circular(10.0);
        clip.perform_layout_with_child(
            BoxConstraints::tight(Size::new(100.0, 100.0)),
            Size::new(100.0, 100.0),
        );

        // Outside should not hit
        assert!(!clip.hit_test(Offset::new(150.0, 50.0)));
    }
}
