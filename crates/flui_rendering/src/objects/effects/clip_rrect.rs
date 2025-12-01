//! RenderClipRRect - clips child to rounded rectangle

use flui_painting::Canvas;
use flui_types::{
    geometry::RRect,
    painting::Clip,
    styling::{BorderRadius, Radius},
    Offset, Rect, Size,
};

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
    fn apply_clip(&self, canvas: &mut Canvas, size: Size) {
        let rect = Rect::from_xywh(0.0, 0.0, size.width, size.height);

        // Calculate average radius from all corners
        // TODO: Support per-corner radii when RRect supports it
        let avg_radius = (self.border_radius.top_left.x
            + self.border_radius.top_right.x
            + self.border_radius.bottom_right.x
            + self.border_radius.bottom_left.x)
            / 4.0;

        let radius = Radius::circular(avg_radius);
        let rrect = RRect::from_rect_and_radius(rect, radius);

        canvas.clip_rrect(rrect);
    }

    fn contains_point(&self, position: Offset, size: Size) -> bool {
        // For rounded rectangle hit testing, we need to check:
        // 1. If point is in the main rectangular area (excluding corners)
        // 2. If point is in one of the corner circles
        //
        // Using average radius for simplicity (matching apply_clip behavior)

        let x = position.dx;
        let y = position.dy;

        // Quick bounds check
        if x < 0.0 || y < 0.0 || x > size.width || y > size.height {
            return false;
        }

        // Calculate average radius (matching apply_clip)
        let radius = (self.border_radius.top_left.x
            + self.border_radius.top_right.x
            + self.border_radius.bottom_right.x
            + self.border_radius.bottom_left.x)
            / 4.0;

        // If radius is 0, it's a regular rectangle
        if radius < f32::EPSILON {
            return true; // Already passed bounds check
        }

        // Check which region the point is in:
        // - Top-left corner region
        if x < radius && y < radius {
            let dx = x - radius;
            let dy = y - radius;
            return dx * dx + dy * dy <= radius * radius;
        }

        // - Top-right corner region
        if x > size.width - radius && y < radius {
            let dx = x - (size.width - radius);
            let dy = y - radius;
            return dx * dx + dy * dy <= radius * radius;
        }

        // - Bottom-left corner region
        if x < radius && y > size.height - radius {
            let dx = x - radius;
            let dy = y - (size.height - radius);
            return dx * dx + dy * dy <= radius * radius;
        }

        // - Bottom-right corner region
        if x > size.width - radius && y > size.height - radius {
            let dx = x - (size.width - radius);
            let dy = y - (size.height - radius);
            return dx * dx + dy * dy <= radius * radius;
        }

        // Point is in the main rectangular area (not in corners)
        true
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
