//! RenderClipRect - clips child to a rectangle.
//!
//! This render object clips its child to a rectangular region.

use flui_types::{Offset, Point, Rect, Size};

use crate::constraints::BoxConstraints;

use crate::containers::ProxyBox;
use crate::delegates::CustomClipper;
use crate::pipeline::PaintingContext;
use crate::traits::TextBaseline;

/// Clip behavior determines how clipping is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Clip {
    /// No clipping (for debugging or when clip is known to be unnecessary).
    None,
    /// Clip without anti-aliasing (faster but jagged edges).
    HardEdge,
    /// Clip with anti-aliasing (default, smooth edges).
    #[default]
    AntiAlias,
    /// Clip with anti-aliasing and save layer (highest quality, slowest).
    AntiAliasWithSaveLayer,
}

/// A render object that clips its child to a rectangle.
///
/// By default, clips to the bounds of the render object. A custom clipper
/// can be provided for different clip shapes.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::r#box::effects::RenderClipRect;
///
/// // Clip to bounds
/// let clip = RenderClipRect::new();
///
/// // Clip with anti-aliasing disabled
/// let mut clip = RenderClipRect::new();
/// clip.set_clip_behavior(Clip::HardEdge);
/// ```
#[derive(Debug)]
pub struct RenderClipRect {
    /// Container holding the child and geometry.
    proxy: ProxyBox,

    /// Optional custom clipper.
    clipper: Option<Box<dyn CustomClipper<Rect>>>,

    /// The clip behavior.
    clip_behavior: Clip,
}

impl RenderClipRect {
    /// Creates a new clip rect render object.
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
            // In real implementation: self.mark_needs_paint();
        }
    }

    /// Sets the custom clipper.
    pub fn set_clipper(&mut self, clipper: Option<Box<dyn CustomClipper<Rect>>>) {
        self.clipper = clipper;
        // In real implementation: self.mark_needs_paint();
    }

    /// Computes the clip rect.
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
            // No clipping
            let _ = (context, offset);
            // In real implementation: context.paint_child(child, offset);
            return;
        }

        let clip = self.get_clip();
        // In real implementation:
        // context.push_clip_rect(
        //     offset,
        //     clip,
        //     self.clip_behavior,
        //     |ctx| ctx.paint_child(child, offset)
        // );
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

impl Default for RenderClipRect {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clip_rect_new() {
        let clip = RenderClipRect::new();
        assert_eq!(clip.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn test_clip_rect_behavior() {
        let mut clip = RenderClipRect::new();
        clip.set_clip_behavior(Clip::HardEdge);
        assert_eq!(clip.clip_behavior(), Clip::HardEdge);
    }

    #[test]
    fn test_clip_rect_default() {
        let mut clip = RenderClipRect::new();
        clip.perform_layout_with_child(
            BoxConstraints::tight(Size::new(100.0, 80.0)),
            Size::new(100.0, 80.0),
        );

        let rect = clip.get_clip();
        assert!((rect.left() - 0.0).abs() < f32::EPSILON);
        assert!((rect.top() - 0.0).abs() < f32::EPSILON);
        assert!((rect.width() - 100.0).abs() < f32::EPSILON);
        assert!((rect.height() - 80.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_hit_test_inside() {
        let mut clip = RenderClipRect::new();
        clip.perform_layout_with_child(
            BoxConstraints::tight(Size::new(100.0, 100.0)),
            Size::new(100.0, 100.0),
        );

        assert!(clip.hit_test(Offset::new(50.0, 50.0)));
    }

    #[test]
    fn test_hit_test_outside() {
        let mut clip = RenderClipRect::new();
        clip.perform_layout_with_child(
            BoxConstraints::tight(Size::new(100.0, 100.0)),
            Size::new(100.0, 100.0),
        );

        assert!(!clip.hit_test(Offset::new(150.0, 50.0)));
    }

    #[test]
    fn test_layout() {
        let mut clip = RenderClipRect::new();
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 150.0);
        let child_size = Size::new(100.0, 75.0);

        let size = clip.perform_layout_with_child(constraints, child_size);

        assert_eq!(size, child_size);
    }
}
