//! RenderClipRRect - clips child to rounded rectangle

use flui_types::{Rect, Size, styling::BorderRadius, painting::Clip};
use flui_engine::{ClipRRectLayer, BoxedLayer, painter::RRect};

use super::clip_base::{ClipShape, RenderClip};

/// Shape implementation for rounded rectangle clipping
#[derive(Debug, Clone, Copy)]
pub struct RRectShape {
    /// Border radius for rounded corners
    pub border_radius: BorderRadius,
}

impl RRectShape {
    /// Create new RRectShape with border radius
    pub fn new(border_radius: BorderRadius) -> Self {
        Self { border_radius }
    }

    /// Create with circular radius (all corners same)
    pub fn circular(radius: f32) -> Self {
        Self::new(BorderRadius::circular(radius))
    }
}

impl ClipShape for RRectShape {
    fn create_clip_layer(&self, child_layer: BoxedLayer, size: Size) -> BoxedLayer {
        let rect = Rect::from_xywh(0.0, 0.0, size.width, size.height);

        // Calculate average radius from all corners
        // TODO: Support per-corner radii when RRect supports it
        let avg_radius = (self.border_radius.top_left.x + self.border_radius.top_right.x +
                         self.border_radius.bottom_right.x + self.border_radius.bottom_left.x) / 4.0;

        let rrect = RRect {
            rect,
            corner_radius: avg_radius,
        };

        let mut clip_layer = ClipRRectLayer::new(rrect);
        clip_layer.add_child(child_layer);
        Box::new(clip_layer)
    }
}

/// RenderObject that clips its child to a rounded rectangle
///
/// The clipping is applied during painting with rounded corners.
/// It doesn't affect layout, so the child is laid out normally
/// and then clipped to its bounds with rounded corners.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderClipRRect;
/// use flui_types::styling::BorderRadius;
/// use flui_types::painting::Clip;
///
/// let clip = RenderClipRRect::circular(10.0);
/// ```
pub type RenderClipRRect = RenderClip<RRectShape>;

impl RenderClipRRect {
    /// Create with border radius and clip behavior
    pub fn with_border_radius(border_radius: BorderRadius, clip_behavior: Clip) -> Self {
        RenderClip::new(RRectShape::new(border_radius), clip_behavior)
    }

    /// Create with circular radius (all corners same)
    pub fn circular(radius: f32) -> Self {
        Self::with_border_radius(BorderRadius::circular(radius), Clip::AntiAlias)
    }

    /// Set new border radius
    pub fn set_border_radius(&mut self, border_radius: BorderRadius) {
        self.shape_mut().border_radius = border_radius;
    }

    /// Get border radius
    pub fn border_radius(&self) -> BorderRadius {
        self.shape().border_radius
    }
}

impl Default for RenderClipRRect {
    fn default() -> Self {
        Self::circular(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_clip_rrect_with_border_radius() {
        let radius = BorderRadius::circular(10.0);
        let clip = RenderClipRRect::with_border_radius(radius, Clip::AntiAlias);
        assert_eq!(clip.border_radius(), radius);
        assert_eq!(clip.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn test_render_clip_rrect_circular() {
        let clip = RenderClipRRect::circular(15.0);
        assert_eq!(clip.border_radius(), BorderRadius::circular(15.0));
        assert_eq!(clip.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn test_render_clip_rrect_default() {
        let clip = RenderClipRRect::default();
        assert_eq!(clip.border_radius(), BorderRadius::circular(0.0));
        assert_eq!(clip.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn test_render_clip_rrect_set_border_radius() {
        let mut clip = RenderClipRRect::circular(10.0);
        clip.set_border_radius(BorderRadius::circular(20.0));
        assert_eq!(clip.border_radius(), BorderRadius::circular(20.0));
    }

    #[test]
    fn test_render_clip_rrect_set_clip_behavior() {
        let mut clip = RenderClipRRect::circular(10.0);
        clip.set_clip_behavior(Clip::HardEdge);
        assert_eq!(clip.clip_behavior(), Clip::HardEdge);
    }
}
