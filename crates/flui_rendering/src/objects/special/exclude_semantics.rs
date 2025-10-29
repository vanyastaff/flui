//! RenderExcludeSemantics - excludes child from semantics tree

use flui_core::render::{
    LayoutCx, PaintCx, RenderObject, SingleArity, SingleChild, SingleChildPaint,
};
use flui_engine::BoxedLayer;
use flui_types::Size;

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

impl RenderObject for RenderExcludeSemantics {
    type Arity = SingleArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        // Layout child with same constraints (pass-through)
        let child = cx.child();
        cx.layout_child(child, cx.constraints())
    }

    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        // Paint child directly (pass-through)
        let child = cx.child();
        cx.capture_child_layer(child)
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
