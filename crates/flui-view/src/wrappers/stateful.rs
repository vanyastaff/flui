//! StatefulViewWrapper - Wrapper that holds a StatefulView and its state
//!
//! Implements ViewObject for StatefulView types.

use std::any::Any;

use flui_element::{Element, IntoElement};

use crate::context::BuildContext;
use crate::object::ViewObject;
use crate::protocol::ViewMode;
use crate::traits::StatefulView;

/// Wrapper for StatefulView that implements ViewObject
///
/// Stores both the view configuration and the mutable state.
pub struct StatefulViewWrapper<V: StatefulView> {
    /// The view configuration
    view: V,

    /// The mutable state (created lazily)
    state: Option<V::State>,

    /// Cached child element from last build
    child: Option<Element>,
}

impl<V: StatefulView> StatefulViewWrapper<V> {
    /// Create a new wrapper
    pub fn new(view: V) -> Self {
        Self {
            view,
            state: None,
            child: None,
        }
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
            .field("has_child", &self.child.is_some())
            .finish()
    }
}

impl<V: StatefulView> ViewObject for StatefulViewWrapper<V> {
    fn mode(&self) -> ViewMode {
        ViewMode::Stateful
    }

    fn build(&mut self, ctx: &dyn BuildContext) -> Element {
        // Initialize state on first build
        if self.state.is_none() {
            self.state = Some(self.view.create_state());
        }

        // Build with state
        if let Some(ref mut state) = self.state {
            let child = self.view.build(state, ctx).into_element();
            self.child = Some(child);
        }

        // Return the cached child or empty
        self.child.take().unwrap_or_else(Element::empty)
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
        self.child = None;
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
// IntoElement IMPLEMENTATION
// ============================================================================

/// Helper struct to disambiguate StatefulView from StatelessView
///
/// Use `Stateful(my_view)` to create a stateful element.
pub struct Stateful<V: StatefulView>(pub V);

impl<V: StatefulView> IntoElement for Stateful<V> {
    fn into_element(self) -> Element {
        let wrapper = StatefulViewWrapper::new(self.0);
        Element::with_mode(wrapper, ViewMode::Stateful)
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ViewState;

    struct TestStatefulView {
        initial: i32,
    }

    struct TestState {
        count: i32,
    }

    impl StatefulView for TestStatefulView {
        type State = TestState;

        fn create_state(&self) -> Self::State {
            TestState {
                count: self.initial,
            }
        }

        fn build(&self, state: &mut Self::State, _ctx: &BuildContext) -> impl IntoElement {
            state.count += 1;
            ()
        }
    }

    #[test]
    fn test_wrapper_creation() {
        let wrapper = StatefulViewWrapper::new(TestStatefulView { initial: 10 });
        assert!(wrapper.state.is_none()); // State not created until build
        assert_eq!(wrapper.mode(), ViewMode::Stateful);
    }

    #[test]
    fn test_into_element() {
        let view = TestStatefulView { initial: 10 };
        let element = Stateful(view).into_element();
        assert!(element.has_view_object());
    }
}
