//! Integration tests for View → Element conversion.
//!
//! Tests the IntoView, IntoElement, BoxedView, BoxedElement traits
//! and downcast-rs / dyn-clone integration.

use flui_view::{
    BoxedElement, BoxedView, BuildContext, ElementBase, IntoElement, IntoView, Lifecycle,
    StatefulElement, StatefulView, StatelessElement, StatelessView, View, ViewExt, ViewState,
};
use std::any::TypeId;

// ============================================================================
// Test Views
// ============================================================================

#[derive(Clone, Debug)]
struct SimpleView {
    text: String,
}

impl StatelessView for SimpleView {
    fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
        Box::new(self.clone())
    }
}

impl View for SimpleView {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(StatelessElement::new(self))
    }
}

#[derive(Clone, Debug)]
struct CounterView {
    initial: i32,
}

struct CounterState {
    count: i32,
}

impl StatefulView for CounterView {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        CounterState {
            count: self.initial,
        }
    }
}

impl ViewState<CounterView> for CounterState {
    fn build(&self, _view: &CounterView, _ctx: &dyn BuildContext) -> Box<dyn View> {
        Box::new(SimpleView {
            text: format!("Count: {}", self.count),
        })
    }
}

impl View for CounterView {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(StatefulElement::new(self))
    }
}

// ============================================================================
// IntoView Tests
// ============================================================================

#[test]
fn test_into_view_identity() {
    // Views should convert to themselves
    let view = SimpleView {
        text: "Hello".to_string(),
    };
    let converted = view.clone().into_view();
    assert_eq!(converted.text, view.text);
}

#[test]
fn test_into_view_type_preservation() {
    let view = SimpleView {
        text: "Test".to_string(),
    };
    let converted: SimpleView = view.clone().into_view();
    assert_eq!(TypeId::of::<SimpleView>(), converted.view_type_id());
}

// ============================================================================
// IntoElement Tests
// ============================================================================

#[test]
fn test_into_element_from_view() {
    let view = SimpleView {
        text: "Hello".to_string(),
    };

    let element = view.into_element();
    assert_eq!(element.view_type_id(), TypeId::of::<SimpleView>());
    assert_eq!(element.lifecycle(), Lifecycle::Initial);
}

#[test]
fn test_into_element_from_stateful_view() {
    let view = CounterView { initial: 10 };

    let element = view.into_element();
    assert_eq!(element.view_type_id(), TypeId::of::<CounterView>());
    assert_eq!(element.lifecycle(), Lifecycle::Initial);
}

#[test]
fn test_into_element_from_boxed_view() {
    let view: Box<dyn View> = Box::new(SimpleView {
        text: "Boxed".to_string(),
    });

    let element = view.into_element();
    assert_eq!(element.view_type_id(), TypeId::of::<SimpleView>());
}

// ============================================================================
// BoxedView Tests
// ============================================================================

#[test]
fn test_boxed_view_creation() {
    let view = SimpleView {
        text: "Test".to_string(),
    };
    let boxed = view.boxed();

    assert_eq!(boxed.view_type_id(), TypeId::of::<SimpleView>());
}

#[test]
fn test_boxed_view_clone() {
    let view = SimpleView {
        text: "Cloneable".to_string(),
    };
    let boxed = view.boxed();
    let cloned = boxed.clone();

    assert_eq!(boxed.view_type_id(), cloned.view_type_id());
}

#[test]
fn test_boxed_view_create_element() {
    let view = SimpleView {
        text: "Test".to_string(),
    };
    let boxed = view.boxed();
    let element = boxed.create_element();

    assert_eq!(element.view_type_id(), TypeId::of::<SimpleView>());
}

#[test]
fn test_boxed_view_can_update() {
    let view1 = SimpleView {
        text: "First".to_string(),
    };
    let view2 = SimpleView {
        text: "Second".to_string(),
    };
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

#[test]
fn test_boxed_view_is_view() {
    // BoxedView should implement View
    fn takes_view(_: &dyn View) {}

    let boxed = SimpleView {
        text: "Test".to_string(),
    }
    .boxed();
    takes_view(&boxed);
}

// ============================================================================
// BoxedElement Tests
// ============================================================================

#[test]
fn test_boxed_element_creation() {
    let view = SimpleView {
        text: "Test".to_string(),
    };
    let boxed = BoxedElement::new(view);

    assert_eq!(boxed.inner().view_type_id(), TypeId::of::<SimpleView>());
}

#[test]
fn test_boxed_element_inner_access() {
    let view = SimpleView {
        text: "Inner".to_string(),
    };
    let boxed = BoxedElement::new(view);

    assert_eq!(boxed.inner().lifecycle(), Lifecycle::Initial);
}

#[test]
fn test_boxed_element_inner_mut() {
    let view = SimpleView {
        text: "Mutable".to_string(),
    };
    let mut boxed = BoxedElement::new(view);

    // Mount the element
    boxed.inner_mut().mount(None, 0);
    assert_eq!(boxed.inner().lifecycle(), Lifecycle::Active);
}

#[test]
fn test_boxed_element_into_inner() {
    let view = SimpleView {
        text: "Owned".to_string(),
    };
    let boxed = BoxedElement::new(view);
    let inner = boxed.into_inner();

    assert_eq!(inner.view_type_id(), TypeId::of::<SimpleView>());
}

// ============================================================================
// Downcast Tests (downcast-rs integration)
// ============================================================================

#[test]
fn test_view_downcast_ref() {
    let view: Box<dyn View> = Box::new(SimpleView {
        text: "Downcast".to_string(),
    });

    // Downcast to concrete type
    let concrete = view.as_any().downcast_ref::<SimpleView>();
    assert!(concrete.is_some());
    assert_eq!(concrete.unwrap().text, "Downcast");
}

#[test]
fn test_view_downcast_wrong_type() {
    let view: Box<dyn View> = Box::new(SimpleView {
        text: "Wrong".to_string(),
    });

    // Downcast to wrong type should fail
    let wrong = view.as_any().downcast_ref::<CounterView>();
    assert!(wrong.is_none());
}

#[test]
fn test_view_downcast_mut() {
    let mut view: Box<dyn View> = Box::new(SimpleView {
        text: "Mutable".to_string(),
    });

    // Downcast mutably
    if let Some(concrete) = view.as_any_mut().downcast_mut::<SimpleView>() {
        concrete.text = "Modified".to_string();
    }

    let concrete = view.as_any().downcast_ref::<SimpleView>().unwrap();
    assert_eq!(concrete.text, "Modified");
}

#[test]
fn test_element_downcast_ref() {
    let element: Box<dyn ElementBase> = Box::new(StatelessElement::new(&SimpleView {
        text: "Element".to_string(),
    }));

    // Downcast to concrete element type
    let concrete = element
        .as_any()
        .downcast_ref::<StatelessElement<SimpleView>>();
    assert!(concrete.is_some());
}

#[test]
fn test_element_downcast_wrong_type() {
    let element: Box<dyn ElementBase> = Box::new(StatelessElement::new(&SimpleView {
        text: "Wrong".to_string(),
    }));

    // Downcast to wrong element type should fail
    let wrong = element
        .as_any()
        .downcast_ref::<StatefulElement<CounterView>>();
    assert!(wrong.is_none());
}

// ============================================================================
// Clone Tests (dyn-clone integration)
// ============================================================================

#[test]
fn test_view_dyn_clone() {
    let view: Box<dyn View> = Box::new(SimpleView {
        text: "Clone me".to_string(),
    });

    // Clone the trait object
    let cloned: Box<dyn View> = dyn_clone::clone_box(&*view);

    assert_eq!(view.view_type_id(), cloned.view_type_id());

    // Verify content was cloned
    let original = view.as_any().downcast_ref::<SimpleView>().unwrap();
    let cloned_concrete = cloned.as_any().downcast_ref::<SimpleView>().unwrap();
    assert_eq!(original.text, cloned_concrete.text);
}

#[test]
fn test_boxed_view_dyn_clone() {
    let view = SimpleView {
        text: "Boxed clone".to_string(),
    };
    let boxed = BoxedView(Box::new(view));
    let cloned = boxed.clone();

    // Verify clone
    let original = boxed.0.as_any().downcast_ref::<SimpleView>().unwrap();
    let cloned_concrete = cloned.0.as_any().downcast_ref::<SimpleView>().unwrap();
    assert_eq!(original.text, cloned_concrete.text);
}

#[test]
fn test_boxed_view_independence_after_clone() {
    let view = SimpleView {
        text: "Original".to_string(),
    };
    let boxed = BoxedView(Box::new(view));
    let mut cloned = boxed.clone();

    // Modify the clone
    if let Some(concrete) = cloned.0.as_any_mut().downcast_mut::<SimpleView>() {
        concrete.text = "Modified".to_string();
    }

    // Original should be unchanged
    let original = boxed.0.as_any().downcast_ref::<SimpleView>().unwrap();
    assert_eq!(original.text, "Original");

    let cloned_concrete = cloned.0.as_any().downcast_ref::<SimpleView>().unwrap();
    assert_eq!(cloned_concrete.text, "Modified");
}

// ============================================================================
// Thread Safety Tests
// ============================================================================

#[test]
fn test_view_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<SimpleView>();
    assert_send_sync::<CounterView>();
    assert_send_sync::<BoxedView>();
    assert_send_sync::<Box<dyn View>>();
}

#[test]
fn test_element_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<Box<dyn ElementBase>>();
    assert_send_sync::<BoxedElement>();
    assert_send_sync::<StatelessElement<SimpleView>>();
    assert_send_sync::<StatefulElement<CounterView>>();
}

// ============================================================================
// View Type ID Tests
// ============================================================================

#[test]
fn test_view_type_id_consistency() {
    let view1 = SimpleView {
        text: "One".to_string(),
    };
    let view2 = SimpleView {
        text: "Two".to_string(),
    };

    // Same type should have same type ID
    assert_eq!(view1.view_type_id(), view2.view_type_id());
    assert_eq!(view1.view_type_id(), TypeId::of::<SimpleView>());
}

#[test]
fn test_different_view_types_different_ids() {
    let simple = SimpleView {
        text: "Simple".to_string(),
    };
    let counter = CounterView { initial: 0 };

    assert_ne!(simple.view_type_id(), counter.view_type_id());
}

#[test]
fn test_boxed_view_preserves_type_id() {
    let view = SimpleView {
        text: "Test".to_string(),
    };
    let original_id = view.view_type_id();
    let boxed = view.boxed();

    // BoxedView should return the inner type's ID
    assert_eq!(boxed.view_type_id(), original_id);
}
