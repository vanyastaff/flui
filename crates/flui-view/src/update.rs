//! Update result types for render views.
//!
//! Defines the result of updating a render object when view configuration changes.

// ============================================================================
// UPDATE RESULT
// ============================================================================

/// Result of updating a render object.
///
/// Returned by `RenderView::update()` to indicate what changed.
/// The framework uses this to schedule the appropriate pipeline phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum UpdateResult {
    /// Nothing changed - skip all work.
    #[default]
    Unchanged,

    /// Layout-affecting properties changed.
    ///
    /// Triggers layout phase, which also triggers paint.
    NeedsLayout,

    /// Only visual properties changed.
    ///
    /// Triggers paint phase only, skips layout.
    NeedsPaint,
}

impl UpdateResult {
    /// Returns `true` if layout is needed.
    #[inline]
    pub const fn needs_layout(self) -> bool {
        matches!(self, Self::NeedsLayout)
    }

    /// Returns `true` if paint is needed.
    #[inline]
    pub const fn needs_paint(self) -> bool {
        matches!(self, Self::NeedsLayout | Self::NeedsPaint)
    }

    /// Returns `true` if any work is needed.
    #[inline]
    pub const fn needs_work(self) -> bool {
        !matches!(self, Self::Unchanged)
    }

    /// Combines two results, taking the more expensive one.
    ///
    /// Priority: `NeedsLayout` > `NeedsPaint` > `Unchanged`
    #[inline]
    pub const fn combine(self, other: Self) -> Self {
        match (self, other) {
            (Self::NeedsLayout, _) | (_, Self::NeedsLayout) => Self::NeedsLayout,
            (Self::NeedsPaint, _) | (_, Self::NeedsPaint) => Self::NeedsPaint,
            _ => Self::Unchanged,
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_needs_layout() {
        assert!(!UpdateResult::Unchanged.needs_layout());
        assert!(UpdateResult::NeedsLayout.needs_layout());
        assert!(!UpdateResult::NeedsPaint.needs_layout());
    }

    #[test]
    fn test_needs_paint() {
        assert!(!UpdateResult::Unchanged.needs_paint());
        assert!(UpdateResult::NeedsLayout.needs_paint());
        assert!(UpdateResult::NeedsPaint.needs_paint());
    }

    #[test]
    fn test_needs_work() {
        assert!(!UpdateResult::Unchanged.needs_work());
        assert!(UpdateResult::NeedsLayout.needs_work());
        assert!(UpdateResult::NeedsPaint.needs_work());
    }

    #[test]
    fn test_combine() {
        use UpdateResult::*;

        // Layout takes priority
        assert_eq!(NeedsLayout.combine(Unchanged), NeedsLayout);
        assert_eq!(NeedsLayout.combine(NeedsPaint), NeedsLayout);
        assert_eq!(Unchanged.combine(NeedsLayout), NeedsLayout);

        // Paint is middle priority
        assert_eq!(NeedsPaint.combine(Unchanged), NeedsPaint);
        assert_eq!(Unchanged.combine(NeedsPaint), NeedsPaint);

        // Unchanged combines to unchanged
        assert_eq!(Unchanged.combine(Unchanged), Unchanged);
    }
}
