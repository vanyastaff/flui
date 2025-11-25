//! StatelessViewWrapper - Wrapper that holds a StatelessView
//!
//! Implements ViewObject for StatelessView types.

use std::any::Any;
use std::marker::PhantomData;

use flui_element::{Element, IntoElement};
use flui_foundation::RenderStateAccessor;

use crate::context::BuildContext;
use crate::object::ViewObject;
use crate::protocol::ViewMode;
use crate::traits::StatelessView;

/// Wrapper for StatelessView that implements ViewObject
///
/// Stored inside Element as the view_object.
pub struct StatelessViewWrapper<V: StatelessView> {
    /// The view (consumed on first build)
    view: Option<V>,

    /// Type name for debugging
    _marker: PhantomData<V>,
}

impl<V: StatelessView> StatelessViewWrapper<V> {
    /// Create a new wrapper
    pub fn new(view: V) -> Self {
        Self {
            view: Some(view),
            _marker: PhantomData,
        }
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
    fn mode(&self) -> ViewMode {
        ViewMode::Stateless
    }

    fn build(&mut self, ctx: &dyn BuildContext) -> Element {
        // Take the view (it's consumed by build)
        if let Some(view) = self.view.take() {
            view.build(ctx).into_element()
        } else {
            Element::empty()
        }
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

// RenderStateAccessor - Non-render wrapper uses defaults (returns None)
impl<V: StatelessView> RenderStateAccessor for StatelessViewWrapper<V> {
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

// Note: Blanket impl `impl<V: StatelessView> IntoElement for V` is not possible
// because IntoElement is defined in flui-element (orphan rules).
//
// Instead, users can:
// 1. Implement IntoElement manually for their view type
// 2. Use StatelessViewWrapper directly: Element::new(StatelessViewWrapper::new(view))
// 3. Use the Stateless helper (below)

/// Helper struct to convert StatelessView into Element
///
/// Use `Stateless(my_view)` to create an element from a stateless view.
pub struct Stateless<V: StatelessView>(pub V);

impl<V: StatelessView> IntoElement for Stateless<V> {
    fn into_element(self) -> Element {
        let wrapper = StatelessViewWrapper::new(self.0);
        Element::with_mode(wrapper, ViewMode::Stateless)
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    struct TestView {
        value: i32,
    }

    impl StatelessView for TestView {
        fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
            () // Returns empty element
        }
    }

    #[test]
    fn test_wrapper_creation() {
        let wrapper = StatelessViewWrapper::new(TestView { value: 42 });
        assert!(wrapper.view.is_some());
        assert_eq!(wrapper.mode(), ViewMode::Stateless);
    }

    #[test]
    fn test_into_element() {
        let view = TestView { value: 42 };
        let element = Stateless(view).into_element();
        assert!(element.has_view_object());
    }
}
