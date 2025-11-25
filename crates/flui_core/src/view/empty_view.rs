//! EmptyView - A view that renders nothing
//!
//! This module provides EmptyView, a special view that represents "no content"
//! and renders nothing. It's useful for conditional rendering and placeholder cases.

use crate::element::IntoElement;
use crate::view::{BuildContext, StatelessView};

/// A view that renders absolutely nothing.
///
/// EmptyView is useful for:
/// - Conditional rendering when no content should be shown
/// - Placeholder cases in Option<View>::None
/// - Default cases in match expressions
/// - Testing scenarios that need a no-op view
///
/// # Examples
///
/// ```rust,ignore
/// use flui_core::view::EmptyView;
///
/// // Conditional rendering
/// let view = if show_content {
///     Box::new(Text::new("Hello")) as Box<dyn IntoElement>
/// } else {
///     Box::new(EmptyView) as Box<dyn IntoElement>
/// };
///
/// // In Option cases
/// let optional_view: Option<Text> = None;
/// let view = optional_view.unwrap_or(EmptyView);
/// ```
///
/// # Performance
///
/// EmptyView is extremely lightweight:
/// - Zero allocations during build
/// - No render object creation
/// - Minimal tree impact (proxy element only)
/// - Fast build and teardown
#[derive(Debug, Clone, Copy, Default)]
pub struct EmptyView;

impl StatelessView for EmptyView {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Return unit type, which converts to an empty element
        // This creates the minimal possible element tree representation
        ()
    }
}

// EmptyView only needs StatelessView implementation
// ProxyView is for different use cases

// ============================================================================
// CONVENIENCE CONSTRUCTORS
// ============================================================================

impl EmptyView {
    /// Creates a new EmptyView.
    ///
    /// This is equivalent to `EmptyView` or `EmptyView::default()`.
    #[inline]
    pub const fn new() -> Self {
        EmptyView
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_view_creation() {
        let _empty1 = EmptyView;
        let _empty2 = EmptyView::new();
        let _empty3 = EmptyView::default();
    }

    #[test]
    fn test_empty_view_clone() {
        let empty = EmptyView::new();
        let _cloned = empty; // Copy, not clone
        let _also_cloned = empty.clone();
    }

    #[test]
    fn test_empty_view_debug() {
        let empty = EmptyView::new();
        let debug_str = format!("{:?}", empty);
        assert_eq!(debug_str, "EmptyView");
    }
}
