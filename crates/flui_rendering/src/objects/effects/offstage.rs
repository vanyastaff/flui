//! RenderOffstage - hides widget from display

use flui_types::{Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Data for RenderOffstage
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OffstageData {
    /// Whether the child is offstage (hidden)
    pub offstage: bool,
}

impl OffstageData {
    /// Create new offstage data
    pub fn new(offstage: bool) -> Self {
        Self { offstage }
    }
}

impl Default for OffstageData {
    fn default() -> Self {
        Self { offstage: true }
    }
}

/// RenderObject that hides its child from display
///
/// When offstage is true:
/// - The child is not painted
/// - The child is laid out (to maintain its state)
/// - The size is reported as zero
///
/// This is different from Opacity(0) - the child doesn't take up space.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::effects::OffstageData};
///
/// let mut offstage = SingleRenderBox::new(OffstageData::new(true));
/// ```
pub type RenderOffstage = SingleRenderBox<OffstageData>;

// ===== Public API =====

impl RenderOffstage {
    /// Check if child is offstage
    pub fn offstage(&self) -> bool {
        self.data().offstage
    }

    /// Set whether child is offstage
    pub fn set_offstage(&mut self, offstage: bool) {
        if self.data().offstage != offstage {
            self.data_mut().offstage = offstage;
            RenderBoxMixin::mark_needs_layout(self);
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderOffstage {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Store constraints
        self.state_mut().constraints = Some(constraints);

        let offstage = self.data().offstage;

        // Always layout child to maintain state
        if let Some(child) = self.child_mut() {
            child.layout(constraints);
        }

        // Report size as zero if offstage, otherwise use child size
        let size = if offstage {
            Size::ZERO
        } else if let Some(child) = self.child() {
            child.size()
        } else {
            constraints.smallest()
        };

        // Store size and clear needs_layout flag
        self.state_mut().size = Some(size);
        self.clear_needs_layout();

        size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        // Don't paint if offstage
        if !self.data().offstage {
            if let Some(child) = self.child() {
                child.paint(painter, offset);
            }
        }
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_offstage_data_new() {
        let data = OffstageData::new(true);
        assert!(data.offstage);

        let data = OffstageData::new(false);
        assert!(!data.offstage);
    }

    #[test]
    fn test_offstage_data_default() {
        let data = OffstageData::default();
        assert!(data.offstage);
    }

    #[test]
    fn test_render_offstage_new() {
        let offstage = SingleRenderBox::new(OffstageData::new(true));
        assert!(offstage.offstage());
    }

    #[test]
    fn test_render_offstage_set_offstage() {
        let mut offstage = SingleRenderBox::new(OffstageData::new(true));

        offstage.set_offstage(false);
        assert!(!offstage.offstage());
        assert!(RenderBoxMixin::needs_layout(&offstage));
    }

    #[test]
    fn test_render_offstage_layout_offstage() {
        let mut offstage = SingleRenderBox::new(OffstageData::new(true));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let size = offstage.layout(constraints);

        // Should report zero size when offstage
        assert_eq!(size, Size::ZERO);
    }

    #[test]
    fn test_render_offstage_layout_onstage() {
        let mut offstage = SingleRenderBox::new(OffstageData::new(false));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let size = offstage.layout(constraints);

        // Should use smallest size when onstage (no child)
        assert_eq!(size, Size::new(0.0, 0.0));
    }
}
