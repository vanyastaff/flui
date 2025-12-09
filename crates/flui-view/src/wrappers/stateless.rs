//! `StatelessViewWrapper` - Wrapper that holds a `StatelessView`
//!
//! Implements `ViewObject` for `StatelessView` types.

use std::any::Any;

use crate::traits::StatelessView;
use crate::{BuildContext, IntoView, ViewMode, ViewObject};

/// Wrapper for `StatelessView` that implements `ViewObject`
///
/// Stored inside Element as the `view_object`.
pub struct StatelessViewWrapper<V: StatelessView> {
    /// The view (consumed on first build)
    view: Option<V>,
}

impl<V: StatelessView> StatelessViewWrapper<V> {
    /// Create a new wrapper
    pub fn new(view: V) -> Self {
        Self { view: Some(view) }
    }

    /// Extract the inner view, consuming the wrapper.
    ///
    /// Returns `None` if the view has already been consumed by `build()`.
    pub fn into_inner(self) -> V {
        self.view.expect("View has been consumed by build()")
    }

    /// Get a reference to the inner view, if present.
    pub fn inner(&self) -> Option<&V> {
        self.view.as_ref()
    }
}

impl<V: StatelessView> std::fmt::Debug for StatelessViewWrapper<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatelessViewWrapper")
            .field("has_view", &self.view.is_some())
            .finish()
    }
}

impl<V: StatelessView> ViewObject for StatelessViewWrapper<V> {
    #[inline]
    fn mode(&self) -> ViewMode {
        ViewMode::Stateless
    }

    fn build(&mut self, ctx: &dyn BuildContext) -> Option<Box<dyn ViewObject>> {
        // Take the view (it's consumed by build)
        if let Some(view) = self.view.take() {
            Some(view.build(ctx).into_view())
        } else {
            None
        }
    }

    #[inline]
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<V>()
    }

    #[inline]
    fn as_any(&self) -> &dyn Any {
        self
    }

    #[inline]
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ============================================================================
// IntoView IMPLEMENTATION
// ============================================================================

/// Helper struct to convert `StatelessView` into `ViewObject`
///
/// Use `Stateless(my_view)` to create a view object from a stateless view.
#[derive(Debug)]
pub struct Stateless<V: StatelessView>(pub V);

impl<V: StatelessView> IntoView for Stateless<V> {
    fn into_view(self) -> Box<dyn ViewObject> {
        Box::new(StatelessViewWrapper::new(self.0))
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::MockBuildContext;
    use flui_foundation::ElementId;

    struct TestView {
        _value: i32,
    }

    impl StatelessView for TestView {
        fn build(self, _ctx: &dyn BuildContext) -> impl IntoView {
            // Returns None - empty view
            EmptyIntoView
        }
    }

    // Helper for tests - represents an empty view
    struct EmptyIntoView;

    impl IntoView for EmptyIntoView {
        fn into_view(self) -> Box<dyn ViewObject> {
            Box::new(EmptyViewObject)
        }
    }

    struct EmptyViewObject;

    impl ViewObject for EmptyViewObject {
        fn mode(&self) -> ViewMode {
            ViewMode::Stateless
        }

        fn build(&mut self, _ctx: &dyn BuildContext) -> Option<Box<dyn ViewObject>> {
            None
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    #[test]
    fn test_wrapper_creation() {
        let wrapper = StatelessViewWrapper::new(TestView { _value: 42 });
        assert!(wrapper.view.is_some());
        assert_eq!(wrapper.mode(), ViewMode::Stateless);
    }

    #[test]
    fn test_into_view() {
        let view = TestView { _value: 42 };
        let view_obj = Stateless(view).into_view();
        assert_eq!(view_obj.mode(), ViewMode::Stateless);
    }

    #[test]
    fn test_build_consumes_view() {
        let mut wrapper = StatelessViewWrapper::new(TestView { _value: 42 });
        let ctx = MockBuildContext::new(ElementId::new(1));

        // First build should succeed
        let result = wrapper.build(&ctx);
        assert!(result.is_some());

        // Second build should return None (view consumed)
        let result2 = wrapper.build(&ctx);
        assert!(result2.is_none());
    }
}
