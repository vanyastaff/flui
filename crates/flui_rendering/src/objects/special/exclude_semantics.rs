//! RenderExcludeSemantics - excludes child from semantics tree

use flui_core::element::{ElementId, ElementTree};
use flui_core::render::SingleRender;
use flui_engine::BoxedLayer;
use flui_types::{Offset, Size, constraints::BoxConstraints};

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
/// use flui_rendering::RenderExcludeSemantics;
///
/// // Exclude decorative icon from screen readers
/// let mut exclude = RenderExcludeSemantics::new(true);
/// ```
#[derive(Debug)]
pub struct RenderExcludeSemantics {
    /// Whether to exclude semantics
    pub excluding: bool,
}

// ===== Public API =====

impl RenderExcludeSemantics {
    /// Create new RenderExcludeSemantics
    pub fn new(excluding: bool) -> Self {
        Self { excluding }
    }

    /// Check if excluding semantics
    pub fn excluding(&self) -> bool {
        self.excluding
    }

    /// Set whether to exclude semantics
    pub fn set_excluding(&mut self, excluding: bool) {
        if self.excluding != excluding {
            self.excluding = excluding;
            // In a full implementation, would notify semantics system
        }
    }
}

// ===== RenderObject Implementation =====

impl SingleRender for RenderExcludeSemantics {
    /// No metadata needed
    type Metadata = ();

    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        // Layout child with same constraints (pass-through)
        tree.layout_child(child_id, constraints)
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
        // Paint child directly (pass-through)
        tree.paint_child(child_id, offset)
    }
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
        let exclude = RenderExcludeSemantics::new(true);
        assert!(exclude.excluding());
    }

    #[test]
    fn test_render_exclude_semantics_set_excluding() {
        let mut exclude = RenderExcludeSemantics::new(true);

        exclude.set_excluding(false);
        assert!(!exclude.excluding());

        exclude.set_excluding(true);
        assert!(exclude.excluding());
    }
}
