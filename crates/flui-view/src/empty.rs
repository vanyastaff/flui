//! `EmptyView` - A view that renders nothing
//!
//! Useful for conditional rendering and placeholder cases.

use std::any::Any;

use crate::{BuildContext, IntoView, ViewMode, ViewObject};

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
///     Stateless(Text::new("Hello")).into_view()
/// } else {
///     EmptyView.into_view()
/// };
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct EmptyView;

impl EmptyView {
    /// Create a new `EmptyView`
    #[inline]
    pub const fn new() -> Self {
        EmptyView
    }
}

impl IntoView for EmptyView {
    fn into_view(self) -> Box<dyn ViewObject> {
        Box::new(EmptyViewObject)
    }
}

/// Internal ViewObject for EmptyView
#[derive(Debug)]
struct EmptyViewObject;

impl ViewObject for EmptyViewObject {
    fn mode(&self) -> ViewMode {
        ViewMode::Empty
    }

    fn build(&mut self, _ctx: &dyn BuildContext) -> Option<Box<dyn ViewObject>> {
        None // Empty view has no children
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn debug_name(&self) -> &'static str {
        "EmptyView"
    }
}

// ============================================================================
// IMPLEMENTATION FOR UNIT TYPE
// ============================================================================

/// Unit type `()` can be used as an empty view.
impl IntoView for () {
    fn into_view(self) -> Box<dyn ViewObject> {
        Box::new(EmptyViewObject)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_view() {
        let view = EmptyView::new();
        let view_obj = view.into_view();
        assert_eq!(view_obj.mode(), ViewMode::Empty);
    }

    #[test]
    fn test_empty_view_default() {
        let view = EmptyView::default();
        let view_obj = view.into_view();
        assert_eq!(view_obj.mode(), ViewMode::Empty);
    }

    #[test]
    fn test_unit_into_view() {
        let view_obj = ().into_view();
        assert_eq!(view_obj.mode(), ViewMode::Empty);
    }

    #[test]
    fn test_empty_build_returns_none() {
        let mut view_obj = EmptyView.into_view();
        use crate::context::MockBuildContext;
        use flui_foundation::ElementId;

        let ctx = MockBuildContext::new(ElementId::new(1));
        let result = view_obj.build(&ctx);
        assert!(result.is_none());
    }
}
