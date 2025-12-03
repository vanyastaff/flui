//! `StatefulViewWrapper` - Wrapper that holds a `StatefulView` and its state
//!
//! Implements `ViewObject` for `StatefulView` types.

use std::any::Any;

use crate::traits::StatefulView;
use crate::{BuildContext, IntoView, ViewMode, ViewObject};

/// Wrapper for `StatefulView` that implements `ViewObject`
///
/// Stores both the view configuration and the mutable state.
pub struct StatefulViewWrapper<V: StatefulView> {
    /// The view configuration
    view: V,

    /// The mutable state (created lazily)
    state: Option<V::State>,
}

impl<V: StatefulView> StatefulViewWrapper<V> {
    /// Create a new wrapper
    pub fn new(view: V) -> Self {
        Self { view, state: None }
    }

    /// Get reference to view
    pub fn view(&self) -> &V {
        &self.view
    }

    /// Get reference to state (if initialized)
    pub fn state(&self) -> Option<&V::State> {
        self.state.as_ref()
    }

    /// Get mutable reference to state (if initialized)
    pub fn state_mut(&mut self) -> Option<&mut V::State> {
        self.state.as_mut()
    }
}

impl<V: StatefulView> std::fmt::Debug for StatefulViewWrapper<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatefulViewWrapper")
            .field("has_state", &self.state.is_some())
            .finish()
    }
}

impl<V: StatefulView> ViewObject for StatefulViewWrapper<V> {
    fn mode(&self) -> ViewMode {
        ViewMode::Stateful
    }

    fn build(&mut self, ctx: &dyn BuildContext) -> Option<Box<dyn ViewObject>> {
        // Initialize state on first build
        if self.state.is_none() {
            self.state = Some(self.view.create_state());
        }

        // Build with state
        if let Some(ref mut state) = self.state {
            Some(self.view.build(state, ctx).into_view())
        } else {
            None
        }
    }

    fn init(&mut self, _ctx: &dyn BuildContext) {
        // State is already created in build(), but could add hooks here
    }

    fn did_update(&mut self, old_view: &dyn Any, _ctx: &dyn BuildContext) {
        // Notify view of configuration change
        if let Some(old) = old_view.downcast_ref::<V>() {
            if let Some(ref mut state) = self.state {
                self.view.did_update_view(state, old);
            }
        }
    }

    fn dispose(&mut self, _ctx: &dyn BuildContext) {
        // Clean up state
        self.state = None;
    }

    fn debug_name(&self) -> &'static str {
        std::any::type_name::<V>()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ============================================================================
// IntoView IMPLEMENTATION
// ============================================================================

/// Helper struct to disambiguate `StatefulView` from `StatelessView`
///
/// Use `Stateful(my_view)` to create a stateful view object.
#[derive(Debug)]
pub struct Stateful<V: StatefulView>(pub V);

impl<V: StatefulView> IntoView for Stateful<V> {
    fn into_view(self) -> Box<dyn ViewObject> {
        Box::new(StatefulViewWrapper::new(self.0))
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::MockBuildContext;
    use crate::state::ViewState;
    use flui_foundation::ElementId;

    struct TestStatefulView {
        initial: i32,
    }

    struct TestState {
        count: i32,
    }

    // Note: ViewState is automatically implemented via blanket impl

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

    impl StatefulView for TestStatefulView {
        type State = TestState;

        fn create_state(&self) -> Self::State {
            TestState {
                count: self.initial,
            }
        }

        fn build(&self, state: &mut Self::State, _ctx: &dyn BuildContext) -> impl IntoView {
            state.count += 1;
            EmptyIntoView
        }
    }

    #[test]
    fn test_wrapper_creation() {
        let wrapper = StatefulViewWrapper::new(TestStatefulView { initial: 10 });
        assert!(wrapper.state.is_none()); // State not created until build
        assert_eq!(wrapper.mode(), ViewMode::Stateful);
    }

    #[test]
    fn test_into_view() {
        let view = TestStatefulView { initial: 10 };
        let view_obj = Stateful(view).into_view();
        assert_eq!(view_obj.mode(), ViewMode::Stateful);
    }

    #[test]
    fn test_state_persistence() {
        let mut wrapper = StatefulViewWrapper::new(TestStatefulView { initial: 10 });
        let ctx = MockBuildContext::new(ElementId::new(1));

        // First build creates state
        wrapper.build(&ctx);
        assert!(wrapper.state.is_some());
        assert_eq!(wrapper.state().unwrap().count, 11);

        // Second build reuses state
        wrapper.build(&ctx);
        assert_eq!(wrapper.state().unwrap().count, 12);
    }
}
