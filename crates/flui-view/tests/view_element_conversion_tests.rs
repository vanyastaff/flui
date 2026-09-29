//! Integration tests for View → Element conversion.
//!
//! Tests the IntoView, IntoElement, BoxedView, BoxedElement traits
//! and downcast-rs / dyn-clone integration.

use flui_view::{
    BoxedView, BuildContext, IntoView, StatefulView, StatelessView, View, ViewExt, ViewState,
};
use static_assertions::{assert_impl_all, assert_not_impl_any};

// ============================================================================
// Test Views
// ============================================================================

#[derive(Clone, Debug)]
struct SimpleView;

impl StatelessView for SimpleView {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.clone().boxed()
    }
}

impl View for SimpleView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

#[derive(Clone, Debug)]
struct CounterView {
    initial: i32,
}

struct CounterState;

impl StatefulView for CounterView {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        let _ = self.initial;
        CounterState
    }
}

impl ViewState<CounterView> for CounterState {
    fn build(&self, _view: &CounterView, _ctx: &dyn BuildContext) -> impl IntoView {
        SimpleView
    }
}

impl View for CounterView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

// ============================================================================
// BoxedView Tests
// ============================================================================

#[test]
fn test_boxed_view_can_update() {
    let view1 = SimpleView;
    let view2 = SimpleView;
    let view3 = CounterView { initial: 0 };

    let boxed1 = view1.boxed();
    let boxed2 = view2.boxed();
    let boxed3 = view3.boxed();

    // Same type can update
    assert!(boxed1.can_update(&boxed2));
    assert!(boxed2.can_update(&boxed1));

    // Different types cannot update
    assert!(!boxed1.can_update(&boxed3));
    assert!(!boxed3.can_update(&boxed1));
}

// ============================================================================
// Ownership Tests
// ============================================================================

#[test]
fn concrete_view_configs_remain_send_sync_but_erased_views_are_owner_local() {
    assert_impl_all!(SimpleView: Send, Sync);
    assert_impl_all!(CounterView: Send, Sync);
    assert_not_impl_any!(BoxedView: Send, Sync);
    assert_not_impl_any!(Box<dyn View>: Send, Sync);
}

// ============================================================================
// View Type ID Tests
// ============================================================================
