//! RenderClipPath - clips child to an arbitrary path

use flui_types::{Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Clip behavior for RenderClipPath
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipBehavior {
    /// No clipping
    None,
    /// Clip with anti-aliasing
    AntiAlias,
    /// Clip with anti-aliasing and save layer
    AntiAliasWithSaveLayer,
}

/// Path clipper trait
///
/// Implement this trait to define custom clip paths.
/// The path should be relative to the widget's bounds.
pub trait PathClipper: std::fmt::Debug + Send + Sync {
    /// Get the clip path for the given size
    fn get_clip(&self, size: Size) -> Vec<Offset>;
}

/// Data for RenderClipPath
#[derive(Debug)]
pub struct ClipPathData {
    /// Clip behavior
    pub clip_behavior: ClipBehavior,
    /// Custom clipper (optional)
    pub clipper: Option<Box<dyn PathClipper>>,
}

impl ClipPathData {
    /// Create new clip path data
    pub fn new(clip_behavior: ClipBehavior) -> Self {
        Self {
            clip_behavior,
            clipper: None,
        }
    }

    /// Create with custom clipper
    pub fn with_clipper(clipper: Box<dyn PathClipper>) -> Self {
        Self {
            clip_behavior: ClipBehavior::AntiAlias,
            clipper: Some(clipper),
        }
    }

    /// Create with anti-aliasing
    pub fn anti_alias() -> Self {
        Self::new(ClipBehavior::AntiAlias)
    }
}

impl Default for ClipPathData {
    fn default() -> Self {
        Self::new(ClipBehavior::AntiAlias)
    }
}

/// RenderObject that clips its child to an arbitrary path
///
/// Unlike RenderClipRect/RenderClipOval which clip to simple shapes,
/// RenderClipPath can clip to any arbitrary path defined by a PathClipper.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::effects::ClipPathData};
///
/// // Clip child to custom path
/// let mut clip_path = SingleRenderBox::new(ClipPathData::anti_alias());
/// ```
pub type RenderClipPath = SingleRenderBox<ClipPathData>;

// ===== Public API =====

impl RenderClipPath {
    /// Get clip behavior
    pub fn clip_behavior(&self) -> ClipBehavior {
        self.data().clip_behavior
    }

    /// Set clip behavior
    pub fn set_clip_behavior(&mut self, clip_behavior: ClipBehavior) {
        if self.data().clip_behavior != clip_behavior {
            self.data_mut().clip_behavior = clip_behavior;
            RenderBoxMixin::mark_needs_paint(self);
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderClipPath {
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

            // Get clip path from clipper
            let size = self.state().size.unwrap_or(Size::ZERO);
            let _clip_path = if let Some(clipper) = &self.data().clipper {
                clipper.get_clip(size)
            } else {
                // No clipper - just paint normally
                child.paint(painter, offset);
                return;
            };

            // TODO: egui doesn't directly support arbitrary path clipping
            // For now, we just paint the child normally
            // In a real implementation, we would:
            // 1. Convert clip_path to egui::Shape::Path
            // 2. Apply the path as a clip region
            // 3. Paint the child
            // 4. Restore the clip region
            //
            // Alternative approaches:
            // - Use painter.with_clip_rect() for bounding box
            // - Draw to an off-screen buffer and mask it
            // - Use custom egui::Shape implementation

            // Paint child (without path clipping for now - TODO)
            child.paint(painter, offset);

            // Note: Full path clipping requires either:
            // - Custom Shape::Path implementation in egui
            // - Off-screen rendering with masking
            // - Integration with graphics backend (wgpu/glow)
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
    fn test_clip_path_data_new() {
        let data = ClipPathData::new(ClipBehavior::AntiAlias);
        assert_eq!(data.clip_behavior, ClipBehavior::AntiAlias);
        assert!(data.clipper.is_none());
    }

    #[test]
    fn test_clip_path_data_anti_alias() {
        let data = ClipPathData::anti_alias();
        assert_eq!(data.clip_behavior, ClipBehavior::AntiAlias);
    }

    #[test]
    fn test_clip_path_data_default() {
        let data = ClipPathData::default();
        assert_eq!(data.clip_behavior, ClipBehavior::AntiAlias);
    }

    #[test]
    fn test_render_clip_path_new() {
        let clip_path = SingleRenderBox::new(ClipPathData::anti_alias());
        assert_eq!(clip_path.clip_behavior(), ClipBehavior::AntiAlias);
    }

    #[test]
    fn test_render_clip_path_set_clip_behavior() {
        let mut clip_path = SingleRenderBox::new(ClipPathData::default());

        clip_path.set_clip_behavior(ClipBehavior::AntiAliasWithSaveLayer);
        assert_eq!(clip_path.clip_behavior(), ClipBehavior::AntiAliasWithSaveLayer);
        assert!(RenderBoxMixin::needs_paint(&clip_path));
    }

    #[test]
    fn test_render_clip_path_layout() {
        let mut clip_path = SingleRenderBox::new(ClipPathData::default());
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let size = clip_path.layout(constraints);

        // No child, should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
    }
}
