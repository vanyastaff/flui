//! Update result for render view updates

/// Result of updating a render object
///
/// Returned by `RenderView::update()` to indicate what changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[derive(Default)]
pub enum UpdateResult {
    /// Nothing changed - skip work
    #[default]
    Unchanged,

    /// Layout-affecting properties changed - needs relayout
    NeedsLayout,

    /// Only visual properties changed - needs repaint only
    NeedsPaint,
}

impl UpdateResult {
    /// Check if any update is needed
    #[inline]
    pub fn needs_update(self) -> bool {
        !matches!(self, Self::Unchanged)
    }

    /// Check if layout is needed
    #[inline]
    pub fn needs_layout(self) -> bool {
        matches!(self, Self::NeedsLayout)
    }

    /// Check if paint is needed (either paint-only or layout which implies paint)
    #[inline]
    pub fn needs_paint(self) -> bool {
        matches!(self, Self::NeedsLayout | Self::NeedsPaint)
    }
}

