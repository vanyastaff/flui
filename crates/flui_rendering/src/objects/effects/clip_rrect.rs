//! RenderClipRRect - clips child to rounded rectangle

use flui_types::{Offset, Size, Rect, constraints::BoxConstraints, styling::BorderRadius};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};
use super::clip_rect::Clip;

/// Data for RenderClipRRect
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipRRectData {
    /// Border radius for rounded corners
    pub border_radius: BorderRadius,
    /// Clipping behavior
    pub clip_behavior: Clip,
}

impl ClipRRectData {
    /// Create new clip rounded rect data
    pub fn new(border_radius: BorderRadius, clip_behavior: Clip) -> Self {
        Self {
            border_radius,
            clip_behavior,
        }
    }

    /// Create with circular radius
    pub fn circular(radius: f32) -> Self {
        Self::new(BorderRadius::circular(radius), Clip::AntiAlias)
    }
}

/// RenderObject that clips its child to a rounded rectangle
///
/// The child is clipped to the bounds of this RenderObject with rounded corners.
/// Changing clip behavior or border radius only affects painting, not layout.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::effects::ClipRRectData};
/// use flui_types::styling::BorderRadius;
///
/// let mut clip = SingleRenderBox::new(ClipRRectData::circular(10.0));
/// ```
pub type RenderClipRRect = SingleRenderBox<ClipRRectData>;

// ===== Public API =====

impl RenderClipRRect {
    /// Get the border radius
    pub fn border_radius(&self) -> BorderRadius {
        self.data().border_radius
    }

    /// Get the clip behavior
    pub fn clip_behavior(&self) -> Clip {
        self.data().clip_behavior
    }

    /// Set new border radius
    pub fn set_border_radius(&mut self, border_radius: BorderRadius) {
        if self.data().border_radius != border_radius {
            self.data_mut().border_radius = border_radius;
            self.mark_needs_paint();
        }
    }

    /// Set new clip behavior
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) {
        if self.data().clip_behavior != clip_behavior {
            self.data_mut().clip_behavior = clip_behavior;
            self.mark_needs_paint();
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderClipRRect {
    fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
        // Store constraints
        *state.constraints.lock() = Some(constraints);

        // Get children from ElementTree via RenderContext
        let children_ids = ctx.children();

        // Layout child with same constraints
        let size = if let Some(&child_id) = children_ids.first() {
            ctx.layout_child_cached(child_id, constraints, None)
        } else {
            // No child - use smallest size
            constraints.smallest()
        };

        // Store size and clear needs_layout flag
        *state.size.lock() = Some(size);
        state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);

        size
    }

    fn paint(&self, state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) {
        // Get children from ElementTree via RenderContext
        let children_ids = ctx.children();

        // Paint child with clipping
        if let Some(&child_id) = children_ids.first() {
            let clip_behavior = self.data().clip_behavior;

            // If no clipping, paint normally
            if clip_behavior == Clip::None {
                ctx.paint_child(child_id, painter, offset);
                return;
            }

            // Get clip rect
            let size = state.size.lock().unwrap_or(Size::ZERO);
            let clip_rect = Rect::from_xywh(offset.dx, offset.dy, size.width, size.height);

            // Get border radius
            let _border_radius = self.data().border_radius;

            // TODO: When egui supports rounded rect clipping, apply it here
            // For now, we use rectangular clipping
            // In a real implementation, we would:
            // 1. Save painter state
            // 2. Set rounded rect clip path with border_radius
            // 3. Paint child
            // 4. Restore painter state

            // Convert to egui rect and apply simple rectangular clipping
            let egui_rect = egui::Rect::from_min_max(
                egui::pos2(clip_rect.left(), clip_rect.top()),
                egui::pos2(clip_rect.right(), clip_rect.bottom()),
            );

            // TODO: Apply corner radius from border_radius when egui supports it
            // For now, just use rectangular clipping

            // Create a new painter with clipping
            let clip_painter = painter.with_clip_rect(egui_rect);
            ctx.paint_child(child_id, &clip_painter, offset);
        }
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clip_rrect_data_new() {
        let radius = BorderRadius::circular(10.0);
        let data = ClipRRectData::new(radius, Clip::AntiAlias);
        assert_eq!(data.border_radius, radius);
        assert_eq!(data.clip_behavior, Clip::AntiAlias);
    }

    #[test]
    fn test_clip_rrect_data_circular() {
        let data = ClipRRectData::circular(15.0);
        assert_eq!(data.border_radius, BorderRadius::circular(15.0));
        assert_eq!(data.clip_behavior, Clip::AntiAlias);
    }

    #[test]
    fn test_render_clip_rrect_new() {
        let clip = SingleRenderBox::new(ClipRRectData::circular(10.0));
        assert_eq!(clip.border_radius(), BorderRadius::circular(10.0));
        assert_eq!(clip.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn test_render_clip_rrect_set_border_radius() {
        use flui_core::testing::mock_render_context;

        let mut clip = SingleRenderBox::new(ClipRRectData::circular(10.0));

        // Clear initial needs_layout flag
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
        let (_tree, ctx) = mock_render_context();
        let _ = clip.layout(constraints, &ctx);

        clip.set_border_radius(BorderRadius::circular(20.0));
        assert_eq!(clip.border_radius(), BorderRadius::circular(20.0));
        assert!(clip.needs_paint());
        assert!(!clip.needs_layout());
    }

    #[test]
    fn test_render_clip_rrect_set_clip_behavior() {
        use flui_core::testing::mock_render_context;

        let mut clip = SingleRenderBox::new(ClipRRectData::circular(10.0));

        // Clear initial needs_layout flag
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
        let (_tree, ctx) = mock_render_context();
        let _ = clip.layout(constraints, &ctx);

        clip.set_clip_behavior(Clip::HardEdge);
        assert_eq!(clip.clip_behavior(), Clip::HardEdge);
        assert!(clip.needs_paint());
        assert!(!clip.needs_layout());
    }

    #[test]
    fn test_render_clip_rrect_layout() {
        use flui_core::testing::mock_render_context;

        let clip = SingleRenderBox::new(ClipRRectData::circular(10.0));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let (_tree, ctx) = mock_render_context();
        let size = clip.layout(constraints, &ctx);

        // Should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
    }
}
