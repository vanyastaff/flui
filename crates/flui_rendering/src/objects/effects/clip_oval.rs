//! RenderClipOval - clips child to an oval shape

use flui_types::{Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Clip behavior for RenderClipOval
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipBehavior {
    /// No clipping
    None,
    /// Clip to oval shape
    AntiAlias,
    /// Clip with anti-aliasing (slower but smoother)
    AntiAliasWithSaveLayer,
}

/// Data for RenderClipOval
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipOvalData {
    /// Clip behavior
    pub clip_behavior: ClipBehavior,
}

impl ClipOvalData {
    /// Create new clip oval data
    pub fn new(clip_behavior: ClipBehavior) -> Self {
        Self { clip_behavior }
    }

    /// Create with anti-alias clipping
    pub fn anti_alias() -> Self {
        Self::new(ClipBehavior::AntiAlias)
    }
}

impl Default for ClipOvalData {
    fn default() -> Self {
        Self::new(ClipBehavior::AntiAlias)
    }
}

/// RenderObject that clips its child to an oval shape
///
/// The oval fills the bounds of this RenderObject.
/// If the bounds are square, this creates a circle.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::effects::{ClipOvalData, ClipBehavior}};
///
/// // Clip child to oval with anti-aliasing
/// let mut clip_oval = SingleRenderBox::new(ClipOvalData::anti_alias());
/// ```
pub type RenderClipOval = SingleRenderBox<ClipOvalData>;

// ===== Public API =====

impl RenderClipOval {
    /// Get clip behavior
    pub fn clip_behavior(&self) -> ClipBehavior {
        self.data().clip_behavior
    }

    /// Set clip behavior
    pub fn set_clip_behavior(&mut self, clip_behavior: ClipBehavior) {
        if self.data().clip_behavior != clip_behavior {
            self.data_mut().clip_behavior = clip_behavior;
            self.mark_needs_paint();
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderClipOval {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Store constraints
        self.state_mut().constraints = Some(constraints);

        // Layout child with same constraints
        let size = if let Some(child) = self.child_mut() {
            child.layout(constraints)
        } else {
            constraints.smallest()
        };

        // Store size and clear needs_layout flag
        self.state_mut().size = Some(size);
        self.clear_needs_layout();

        size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        if let Some(child) = self.child() {
            let clip_behavior = self.data().clip_behavior;

            // Skip clipping if behavior is None
            if matches!(clip_behavior, ClipBehavior::None) {
                child.paint(painter, offset);
                return;
            }

            // Get clip bounds
            let size = self.state().size.unwrap_or(Size::ZERO);
            let _clip_rect = egui::Rect::from_min_size(
                egui::pos2(offset.dx, offset.dy),
                egui::vec2(size.width, size.height),
            );

            // TODO: egui doesn't directly support oval clipping with immutable painter
            // For now, we just paint the child normally
            // In a real implementation, we would:
            // 1. Create an oval path
            // 2. Apply the path as a clip region
            // 3. Paint the child
            // 4. Restore the clip region
            //
            // Alternative approaches:
            // - Use painter.with_clip_rect() if available
            // - Draw to an off-screen buffer and mask it
            // - Use egui::Shape::circle() for circular clipping

            // Paint child (without clipping for now - TODO)
            child.paint(painter, offset);

            // Note: Full oval clipping requires either:
            // - A mutable context to set clip regions
            // - Custom Shape implementation
            // - Off-screen rendering with masking
        }
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clip_behavior_variants() {
        assert_ne!(ClipBehavior::None, ClipBehavior::AntiAlias);
        assert_ne!(ClipBehavior::AntiAlias, ClipBehavior::AntiAliasWithSaveLayer);
    }

    #[test]
    fn test_clip_oval_data_new() {
        let data = ClipOvalData::new(ClipBehavior::AntiAlias);
        assert_eq!(data.clip_behavior, ClipBehavior::AntiAlias);
    }

    #[test]
    fn test_clip_oval_data_anti_alias() {
        let data = ClipOvalData::anti_alias();
        assert_eq!(data.clip_behavior, ClipBehavior::AntiAlias);
    }

    #[test]
    fn test_clip_oval_data_default() {
        let data = ClipOvalData::default();
        assert_eq!(data.clip_behavior, ClipBehavior::AntiAlias);
    }

    #[test]
    fn test_render_clip_oval_new() {
        let clip_oval = SingleRenderBox::new(ClipOvalData::anti_alias());
        assert_eq!(clip_oval.clip_behavior(), ClipBehavior::AntiAlias);
    }

    #[test]
    fn test_render_clip_oval_set_clip_behavior() {
        let mut clip_oval = SingleRenderBox::new(ClipOvalData::default());

        clip_oval.set_clip_behavior(ClipBehavior::AntiAliasWithSaveLayer);
        assert_eq!(clip_oval.clip_behavior(), ClipBehavior::AntiAliasWithSaveLayer);
        assert!(clip_oval.needs_paint());
    }

    #[test]
    fn test_render_clip_oval_layout() {
        let mut clip_oval = SingleRenderBox::new(ClipOvalData::default());
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let size = clip_oval.layout(constraints);

        // No child, should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
    }

    #[test]
    fn test_render_clip_oval_layout_with_constraints() {
        let mut clip_oval = SingleRenderBox::new(ClipOvalData::default());
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));

        let size = clip_oval.layout(constraints);

        // No child, should use smallest (which is 100x100 for tight constraints)
        assert_eq!(size, Size::new(100.0, 100.0));
    }
}
