//! Core Widget system - the foundation of Flui's UI
//!
//! This module defines the Widget trait and related traits that all widgets must implement.
//! Widgets are immutable descriptions of part of the user interface.
//!
//! # Module Structure
//!
//! - `dyn_widget` - DynWidget trait for heterogeneous collections
//! - `traits` - Widget trait with associated types, StatelessWidget, StatefulWidget, State
//! - `lifecycle` - State lifecycle tracking (StateLifecycle enum)
//! - `into_widget` - Helper trait for converting types to Widget trait objects
//! - `provider` - InheritedWidget for dependency injection
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_core::{DynWidget, Widget, StatelessWidget, StatefulWidget, State};
//!
//! // StatelessWidget example
//! #[derive(Debug, Clone)]
//! struct MyWidget {
//!     title: String,
//! }
//!
//! impl StatelessWidget for MyWidget {
//!     fn build(&self, _context: &Context) -> Box<dyn DynWidget> {
//!         Box::new(Text::new(self.title.clone()))
//!     }
//! }
//! ```

// Module declarations
pub mod dyn_widget;
pub mod element_macros;
pub mod equality;
pub mod error_widget;
pub mod inherited_model;
mod into_widget;
mod lifecycle;
pub mod parent_data_widget;
pub mod provider;
pub mod proxy;
mod traits;








// Re-exports - Public API
pub use dyn_widget::DynWidget;
pub use traits::{State, StatefulWidget, StatelessWidget, Widget};
pub use lifecycle::StateLifecycle;
pub use into_widget::IntoWidget;
pub use parent_data_widget::{ParentDataElement, ParentDataWidget};
pub use provider::{InheritedElement, InheritedWidget};
pub use proxy::{ProxyElement, ProxyWidget};
pub use error_widget::{ErrorWidget, ErrorDetails, ErrorWidgetBuilder}; // Phase 3.3
pub use inherited_model::InheritedModel; // Phase 6 (InheritedModel)
pub use equality::{WidgetEq, widgets_equal}; // Phase 12

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Context, impl_widget_for_stateful};

    // Test widget for testing
    #[derive(Debug, Clone)]
    struct TestWidget {
        value: i32,
    }

    impl StatelessWidget for TestWidget {
        fn build(&self, _context: &Context) -> Box<dyn DynWidget> {
            // Return self for testing purposes
            Box::new(TestWidget { value: self.value })
        }
    }

    #[test]
    fn test_widget_type_name() {
        let widget = TestWidget { value: 42 };
        assert!(widget.type_name().contains("TestWidget"));
    }

    #[test]
    fn test_widget_can_update_same_type() {
        let widget1 = TestWidget { value: 1 };
        let widget2 = TestWidget { value: 2 };

        assert!(widget1.can_update(&widget2));
    }

    #[test]
    fn test_widget_downcast() {
        let widget = TestWidget { value: 42 };
        let boxed: Box<dyn DynWidget> = Box::new(widget);

        // Test downcast_ref
        assert!(boxed.is::<TestWidget>());
        let downcasted = boxed.downcast_ref::<TestWidget>().unwrap();
        assert_eq!(downcasted.value, 42);
    }

    #[test]
    fn test_into_widget() {
        let widget = TestWidget { value: 42 };
        let boxed: Box<dyn DynWidget> = widget.into_widget();

        assert!(boxed.type_name().contains("TestWidget"));
    }

    #[test]
    fn test_create_element() {
        let widget = TestWidget { value: 42 };
        let element = widget.create_element();

        // Element should be created successfully
        assert!(element.is_dirty());
    }

    // Test StatefulWidget
    #[derive(Debug, Clone)]
    struct CounterWidget {
        initial: i32,
    }

    #[derive(Debug)]
    struct CounterState {
        count: i32,
    }

    impl StatefulWidget for CounterWidget {
        type State = CounterState;

        fn create_state(&self) -> Self::State {
            CounterState {
                count: self.initial,
            }
        }
    }

    // Use macro to implement Widget for StatefulWidget
    impl_widget_for_stateful!(CounterWidget);

    impl State for CounterState {
        fn build(&mut self, _context: &Context) -> Box<dyn DynWidget> {
            Box::new(TestWidget { value: self.count })
        }
    }

    #[test]
    fn test_stateful_widget_create_state() {
        let widget = CounterWidget { initial: 10 };
        let state = widget.create_state();

        assert_eq!(state.count, 10);
    }

    #[test]
    fn test_state_build() {
        let mut state = CounterState { count: 5 };
        let context = Context::empty();

        let child = state.build(&context);
        // Should create a widget
        assert!(child.is::<TestWidget>());
    }

    #[test]
    fn test_widget_clone_box() {
        let widget = TestWidget { value: 42 };
        let boxed: Box<dyn DynWidget> = Box::new(widget);

        // Clone the boxed trait object using dyn-clone
        let cloned = dyn_clone::clone_box(&*boxed);

        // Both should be TestWidget with same value
        assert!(cloned.is::<TestWidget>());
        let cloned_test = cloned.downcast_ref::<TestWidget>().unwrap();
        assert_eq!(cloned_test.value, 42);
    }

    #[test]
    fn test_widget_vec_clone() {
        let widgets: Vec<Box<dyn DynWidget>> = vec![
            Box::new(TestWidget { value: 1 }),
            Box::new(TestWidget { value: 2 }),
            Box::new(TestWidget { value: 3 }),
        ];

        // Clone the entire vector of trait objects
        let cloned: Vec<Box<dyn DynWidget>> = widgets.iter().map(|w| dyn_clone::clone_box(&**w)).collect();

        assert_eq!(cloned.len(), 3);
        for (i, widget) in cloned.iter().enumerate() {
            let test_widget = widget.downcast_ref::<TestWidget>().unwrap();
            assert_eq!(test_widget.value, (i + 1) as i32);
        }
    }

    #[test]
    fn test_widget_downcast_mut() {
        let widget = TestWidget { value: 10 };
        let mut boxed: Box<dyn DynWidget> = Box::new(widget);

        // Test downcast_mut
        if let Some(downcasted) = boxed.downcast_mut::<TestWidget>() {
            downcasted.value = 20;
        }

        let result = boxed.downcast_ref::<TestWidget>().unwrap();
        assert_eq!(result.value, 20);
    }

    #[test]
    fn test_widget_downcast_owned() {
        let widget = TestWidget { value: 100 };
        let boxed: Box<dyn DynWidget> = Box::new(widget);

        // Test downcast (owned)
        let downcasted: Box<TestWidget> = boxed.downcast::<TestWidget>().ok().unwrap();
        assert_eq!(downcasted.value, 100);
    }

    #[test]
    fn test_state_downcast() {
        let state = CounterState { count: 42 };
        let boxed: Box<dyn State> = Box::new(state);

        // Test is() check
        assert!(boxed.is::<CounterState>());

        // Test downcast_ref
        let downcasted = boxed.downcast_ref::<CounterState>().unwrap();
        assert_eq!(downcasted.count, 42);
    }

    #[test]
    fn test_state_downcast_mut() {
        let state = CounterState { count: 10 };
        let mut boxed: Box<dyn State> = Box::new(state);

        // Test downcast_mut
        let downcasted = boxed.downcast_mut::<CounterState>().unwrap();
        downcasted.count = 20;

        let result = boxed.downcast_ref::<CounterState>().unwrap();
        assert_eq!(result.count, 20);
    }

    #[test]
    fn test_state_downcast_owned() {
        let state = CounterState { count: 100 };
        let boxed: Box<dyn State> = Box::new(state);

        // Consume and downcast
        let owned: Box<CounterState> = boxed.downcast().ok().unwrap();
        assert_eq!(owned.count, 100);
    }

    // Phase 2: State Lifecycle Tests

    #[test]
    fn test_state_lifecycle_enum() {
        assert_eq!(StateLifecycle::Created.is_mounted(), false);
        assert_eq!(StateLifecycle::Initialized.is_mounted(), true);
        assert_eq!(StateLifecycle::Ready.is_mounted(), true);
        assert_eq!(StateLifecycle::Defunct.is_mounted(), false);
    }

    #[test]
    fn test_state_lifecycle_can_build() {
        assert_eq!(StateLifecycle::Created.can_build(), false);
        assert_eq!(StateLifecycle::Initialized.can_build(), false);
        assert_eq!(StateLifecycle::Ready.can_build(), true);
        assert_eq!(StateLifecycle::Defunct.can_build(), false);
    }

    /// Test state that tracks lifecycle callbacks
    #[derive(Debug)]
    struct LifecycleTrackingState {
        pub init_state_called: bool,
        pub did_change_dependencies_called: bool,
        pub did_update_widget_called: bool,
        pub reassemble_called: bool,
        pub deactivate_called: bool,
        pub activate_called: bool,
        pub dispose_called: bool,
        pub build_count: usize,
    }

    impl State for LifecycleTrackingState {
        fn build(&mut self, _context: &Context) -> Box<dyn DynWidget> {
            self.build_count += 1;
            Box::new(TestWidget { value: self.build_count as i32 })
        }

        fn init_state(&mut self) {
            self.init_state_called = true;
        }

        fn did_change_dependencies(&mut self) {
            self.did_change_dependencies_called = true;
        }

        fn did_update_widget(&mut self, _old_widget: &dyn std::any::Any) {
            self.did_update_widget_called = true;
        }

        fn reassemble(&mut self) {
            self.reassemble_called = true;
        }

        fn deactivate(&mut self) {
            self.deactivate_called = true;
        }

        fn activate(&mut self) {
            self.activate_called = true;
        }

        fn dispose(&mut self) {
            self.dispose_called = true;
        }
    }

    #[test]
    fn test_state_lifecycle_callbacks_exist() {
        let mut state = LifecycleTrackingState {
            init_state_called: false,
            did_change_dependencies_called: false,
            did_update_widget_called: false,
            reassemble_called: false,
            deactivate_called: false,
            activate_called: false,
            dispose_called: false,
            build_count: 0,
        };

        // Call each lifecycle method
        state.init_state();
        assert!(state.init_state_called);

        state.did_change_dependencies();
        assert!(state.did_change_dependencies_called);

        state.did_update_widget(&());
        assert!(state.did_update_widget_called);

        state.reassemble();
        assert!(state.reassemble_called);

        state.deactivate();
        assert!(state.deactivate_called);

        state.activate();
        assert!(state.activate_called);

        state.dispose();
        assert!(state.dispose_called);
    }

    #[test]
    fn test_state_mounted_default() {
        let state = CounterState { count: 0 };
        // Default implementation returns true for backward compatibility
        assert!(state.is_mounted());
    }

    #[test]
    fn test_state_lifecycle_default() {
        let state = CounterState { count: 0 };
        // Default implementation returns Ready for backward compatibility
        assert_eq!(state.lifecycle(), StateLifecycle::Ready);
    }

    #[test]
    fn test_state_build_increments() {
        let mut state = LifecycleTrackingState {
            init_state_called: false,
            did_change_dependencies_called: false,
            did_update_widget_called: false,
            reassemble_called: false,
            deactivate_called: false,
            activate_called: false,
            dispose_called: false,
            build_count: 0,
        };

        let context = Context::empty();

        assert_eq!(state.build_count, 0);
        state.build(&context);
        assert_eq!(state.build_count, 1);
        state.build(&context);
        assert_eq!(state.build_count, 2);
    }

    #[test]
    fn test_state_lifecycle_ordering() {
        let mut state = LifecycleTrackingState {
            init_state_called: false,
            did_change_dependencies_called: false,
            did_update_widget_called: false,
            reassemble_called: false,
            deactivate_called: false,
            activate_called: false,
            dispose_called: false,
            build_count: 0,
        };

        let context = Context::empty();

        // Typical lifecycle order:
        // 1. init_state
        state.init_state();
        assert!(state.init_state_called);
        assert!(!state.did_change_dependencies_called);

        // 2. did_change_dependencies
        state.did_change_dependencies();
        assert!(state.did_change_dependencies_called);

        // 3. build
        state.build(&context);
        assert_eq!(state.build_count, 1);

        // 4. did_update_widget (when widget changes)
        state.did_update_widget(&());
        assert!(state.did_update_widget_called);

        // 5. deactivate (when removed from tree)
        state.deactivate();
        assert!(state.deactivate_called);

        // 6. dispose (when permanently removed)
        state.dispose();
        assert!(state.dispose_called);
    }

    #[test]
    fn test_state_reassemble_hot_reload() {
        let mut state = LifecycleTrackingState {
            init_state_called: false,
            did_change_dependencies_called: false,
            did_update_widget_called: false,
            reassemble_called: false,
            deactivate_called: false,
            activate_called: false,
            dispose_called: false,
            build_count: 0,
        };

        // Simulate hot reload
        assert!(!state.reassemble_called);
        state.reassemble();
        assert!(state.reassemble_called);
    }

    #[test]
    fn test_state_activate_after_deactivate() {
        let mut state = LifecycleTrackingState {
            init_state_called: false,
            did_change_dependencies_called: false,
            did_update_widget_called: false,
            reassemble_called: false,
            deactivate_called: false,
            activate_called: false,
            dispose_called: false,
            build_count: 0,
        };

        // Simulate reparenting scenario (GlobalKey)
        state.deactivate();
        assert!(state.deactivate_called);

        state.activate();
        assert!(state.activate_called);
    }
}












