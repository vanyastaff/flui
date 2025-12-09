//! `EmptyView` - A view that renders nothing
//!
//! Useful for conditional rendering and placeholder cases.

use std::any::Any;

use crate::handle::ViewConfig;
use crate::{BuildContext, IntoView, IntoViewConfig, ViewMode, ViewObject};

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

impl IntoViewConfig for EmptyView {
    fn into_view_config(self) -> ViewConfig {
        // EmptyView is zero-sized, so we can use () as the data
        ViewConfig::new_with_factory((), |_: &()| Box::new(EmptyViewObject))
    }
}

/// Internal `ViewObject` for `EmptyView`
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

impl IntoViewConfig for () {
    fn into_view_config(self) -> ViewConfig {
        ViewConfig::new_with_factory((), |_: &()| Box::new(EmptyViewObject))
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

    #[test]
    fn test_empty_view_into_view_config() {
        let view = EmptyView::new();
        let config = view.into_view_config();

        // Create ViewObject from config
        let view_obj = config.create_view_object();
        assert_eq!(view_obj.mode(), ViewMode::Empty);
    }

    #[test]
    fn test_unit_into_view_config() {
        let config = ().into_view_config();

        // Create ViewObject from config
        let view_obj = config.create_view_object();
        assert_eq!(view_obj.mode(), ViewMode::Empty);
    }

    #[test]
    fn test_empty_config_can_update() {
        let config1 = EmptyView.into_view_config();
        let config2 = EmptyView.into_view_config();
        let config3 = ().into_view_config();

        // Same type, should be able to update
        assert!(config1.can_update(&config2));

        // EmptyView and () both create EmptyViewObject, should be compatible
        assert!(config1.can_update(&config3));
        assert!(config3.can_update(&config1));
    }
}
