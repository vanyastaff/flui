//! EmptyView - A view that renders nothing
//!
//! Useful for conditional rendering and placeholder cases.

use flui_element::IntoElement;

use crate::context::BuildContext;
use crate::traits::StatelessView;

/// A view that renders nothing
///
/// Useful for:
/// - Conditional rendering when no content should be shown
/// - Placeholder cases in `Option<View>::None`
/// - Default cases in match expressions
///
/// # Example
///
/// ```rust,ignore
/// let view = if show_content {
///     Text::new("Hello").into_element()
/// } else {
///     EmptyView.into_element()
/// };
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct EmptyView;

impl EmptyView {
    /// Create a new EmptyView
    #[inline]
    pub const fn new() -> Self {
        EmptyView
    }
}

impl StatelessView for EmptyView {
    fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
        () // Unit type converts to empty element
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_view() {
        let _view = EmptyView::new();
        let _default = EmptyView::default();
    }
}
