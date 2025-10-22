//! RenderClipRect - clips child to a rectangle

use flui_types::{Offset, Size, constraints::BoxConstraints, Rect};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Clip behavior for RenderClipRect
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clip {
    /// No clipping
    None,
    /// Clip to bounds with hard edges
    HardEdge,
    /// Clip to bounds with anti-aliasing
    AntiAlias,
    /// Clip to bounds with anti-aliasing and save layer
    AntiAliasWithSaveLayer,
}

/// Data for RenderClipRect
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipRectData {
    /// Clipping behavior
    pub clip_behavior: Clip,
}

impl ClipRectData {
    /// Create new clip rect data
    pub fn new(clip_behavior: Clip) -> Self {
        Self { clip_behavior }
    }
}

/// RenderObject that clips its child to a rectangle
///
/// The child is clipped to the bounds of this RenderObject.
/// Changing clip behavior only affects painting, not layout.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::effects::{ClipRectData, Clip}};
///
/// let mut clip = SingleRenderBox::new(ClipRectData::new(Clip::AntiAlias));
/// ```
pub type RenderClipRect = SingleRenderBox<ClipRectData>;

// ===== Public API =====

impl RenderClipRect {
    /// Get the clip behavior
    pub fn clip_behavior(&self) -> Clip {
        self.data().clip_behavior
    }

    /// Set new clip behavior
    ///
    /// If clip behavior changes, marks as needing paint (not layout).
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) {
        if self.data().clip_behavior != clip_behavior {
            self.data_mut().clip_behavior = clip_behavior;
            RenderBoxMixin::mark_needs_paint(self);
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderClipRect {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Store constraints
        self.state_mut().constraints = Some(constraints);

        // Layout child with same constraints
        let size = if let Some(child) = self.child_mut() {
            child.layout(constraints)
        } else {
            // No child - use smallest size
            constraints.smallest()
        };

        // Store size and clear needs_layout flag
        self.state_mut().size = Some(size);
        self.clear_needs_layout();

        size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        // Paint child with clipping
        if let Some(child) = self.child() {
            let clip_behavior = self.data().clip_behavior;

            // If no clipping, paint normally
            if clip_behavior == Clip::None {
                child.paint(painter, offset);
                return;
            }

            // Get clip rect
            let size = self.state().size.unwrap_or(Size::ZERO);
            let clip_rect = Rect::from_xywh(offset.dx, offset.dy, size.width, size.height);

            // TODO: When egui supports clip layers, apply clipping here
            // For now, we set the clip rect on the painter
            // In a real implementation, we would:
            // 1. Save painter state
            // 2. Set clip rect based on clip_behavior
            // 3. Paint child
            // 4. Restore painter state

            // egui uses egui::Rect for clipping
            let egui_rect = egui::Rect::from_min_max(
                egui::pos2(clip_rect.left(), clip_rect.top()),
                egui::pos2(clip_rect.right(), clip_rect.bottom()),
            );

            // Create a new painter with clipping
            let clip_painter = painter.with_clip_rect(egui_rect);
            child.paint(&clip_painter, offset);
        }
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_clip_rect_new() {
        let clip = SingleRenderBox::new(ClipRectData::new(Clip::AntiAlias));
        assert_eq!(clip.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn test_render_clip_rect_set_clip_behavior() {
        let mut clip = SingleRenderBox::new(ClipRectData::new(Clip::HardEdge));

        // Clear initial needs_layout flag by doing a layout
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
        let _ = clip.layout(constraints);

        // Now set clip behavior - should only mark needs_paint, not needs_layout
        clip.set_clip_behavior(Clip::AntiAlias);
        assert_eq!(clip.clip_behavior(), Clip::AntiAlias);
        assert!(RenderBoxMixin::needs_paint(&clip));
        assert!(!RenderBoxMixin::needs_layout(&clip));
    }

    #[test]
    fn test_render_clip_rect_layout_no_child() {
        let mut clip = SingleRenderBox::new(ClipRectData::new(Clip::AntiAlias));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let size = clip.layout(constraints);

        // Should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
    }

    #[test]
    fn test_clip_rect_data_debug() {
        let data = ClipRectData::new(Clip::HardEdge);
        let debug_str = format!("{:?}", data);
        assert!(debug_str.contains("ClipRectData"));
    }

    #[test]
    fn test_clip_behavior_variants() {
        // Test all clip behavior variants
        assert_eq!(Clip::None, Clip::None);
        assert_eq!(Clip::HardEdge, Clip::HardEdge);
        assert_eq!(Clip::AntiAlias, Clip::AntiAlias);
        assert_eq!(Clip::AntiAliasWithSaveLayer, Clip::AntiAliasWithSaveLayer);

        assert_ne!(Clip::None, Clip::HardEdge);
        assert_ne!(Clip::HardEdge, Clip::AntiAlias);
    }
}
