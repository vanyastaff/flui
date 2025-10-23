//! RenderExcludeSemantics - excludes child from semantics tree

use flui_types::{Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Data for RenderExcludeSemantics
#[derive(Debug, Clone, Copy)]
pub struct ExcludeSemanticsData {
    /// Whether to exclude semantics
    pub excluding: bool,
}

impl ExcludeSemanticsData {
    /// Create new exclude semantics data
    pub fn new(excluding: bool) -> Self {
        Self { excluding }
    }
}

impl Default for ExcludeSemanticsData {
    fn default() -> Self {
        Self::new(true)
    }
}

/// RenderObject that excludes its child from the semantics tree
///
/// When `excluding` is true, this and all descendants are invisible to
/// accessibility systems (screen readers, etc.).
///
/// Useful for decorative elements that don't need to be announced.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::special::ExcludeSemanticsData};
///
/// // Exclude decorative icon from screen readers
/// let mut exclude = SingleRenderBox::new(ExcludeSemanticsData::new(true));
/// ```
pub type RenderExcludeSemantics = SingleRenderBox<ExcludeSemanticsData>;

// ===== Public API =====

impl RenderExcludeSemantics {
    /// Check if excluding semantics
    pub fn excluding(&self) -> bool {
        self.data().excluding
    }

    /// Set whether to exclude semantics
    pub fn set_excluding(&mut self, excluding: bool) {
        if self.data().excluding != excluding {
            self.data_mut().excluding = excluding;
            // In a full implementation, would notify semantics system
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderExcludeSemantics {
    fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
        // Store constraints
        *state.constraints.lock() = Some(constraints);

        // Layout child with same constraints (pass-through)
        let children_ids = ctx.children();
        let size =
        if let Some(&child_id) = children_ids.first() {
            ctx.layout_child(child_id, constraints)
        } else {
            constraints.smallest()
        };

        // Store size and clear needs_layout flag
        *state.size.lock() = Some(size);
        state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);

        size
    }

    fn paint(&self, state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) {
        // Paint child directly (pass-through)
        // Get children from ElementTree via RenderContext
        let children_ids = ctx.children();

        if let Some(&child_id) = children_ids.first() {
            ctx.paint_child(child_id, painter, offset);
        }
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exclude_semantics_data_new() {
        let data = ExcludeSemanticsData::new(true);
        assert!(data.excluding);

        let data = ExcludeSemanticsData::new(false);
        assert!(!data.excluding);
    }

    #[test]
    fn test_exclude_semantics_data_default() {
        let data = ExcludeSemanticsData::default();
        assert!(data.excluding);
    }

    #[test]
    fn test_render_exclude_semantics_new() {
        let exclude = SingleRenderBox::new(ExcludeSemanticsData::new(true));
        assert!(exclude.excluding());
    }

    #[test]
    fn test_render_exclude_semantics_set_excluding() {
        let mut exclude = SingleRenderBox::new(ExcludeSemanticsData::new(true));

        exclude.set_excluding(false);
        assert!(!exclude.excluding());

        exclude.set_excluding(true);
        assert!(exclude.excluding());
    }

    #[test]
    fn test_render_exclude_semantics_layout() {
        use flui_core::testing::mock_render_context;

        let exclude = SingleRenderBox::new(ExcludeSemanticsData::new(true));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let (_tree, ctx) = mock_render_context();
        let size = exclude.layout(constraints, &ctx);

        // No child, should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
    }
}
