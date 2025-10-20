//! Integration tests for State Lifecycle (Phase 2)
//!
//! Tests State lifecycle tracking, validation, and transitions.

use flui_core::{
    AnyElement, AnyWidget, Context, State, StateLifecycle, StatefulElement, StatefulWidget, Widget,
};
use std::sync::{Arc, Mutex};

// ============================================================================
// Test Widgets and State
// ============================================================================

/// Counter widget for testing stateful lifecycle
#[derive(Debug, Clone)]
struct CounterWidget {
    initial: i32,
}

impl StatefulWidget for CounterWidget {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        CounterState {
            count: self.initial,
            lifecycle_events: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

// Manual Widget impl for CounterWidget (Phase 2 pattern)
impl flui_core::Widget for CounterWidget {
    type Element = StatefulElement<CounterWidget>;

    fn into_element(self) -> Self::Element {
        StatefulElement::new(self)
    }
}

/// State that tracks lifecycle events
#[derive(Debug)]
struct CounterState {
    count: i32,
    lifecycle_events: Arc<Mutex<Vec<String>>>,
}

impl State for CounterState {
    fn build(&mut self, _context: &Context) -> Box<dyn AnyWidget> {
        self.log_event("build");
        Box::new(CounterWidget { initial: self.count })
    }

    fn init_state(&mut self) {
        self.log_event("init_state");
    }

    fn did_change_dependencies(&mut self) {
        self.log_event("did_change_dependencies");
    }

    fn did_update_widget(&mut self, _old_widget: &dyn std::any::Any) {
        self.log_event("did_update_widget");
    }

    fn reassemble(&mut self) {
        self.log_event("reassemble");
    }

    fn deactivate(&mut self) {
        self.log_event("deactivate");
    }

    fn activate(&mut self) {
        self.log_event("activate");
    }

    fn dispose(&mut self) {
        self.log_event("dispose");
    }
}

impl CounterState {
    fn log_event(&self, event: &str) {
        self.lifecycle_events.lock().unwrap().push(event.to_string());
    }

    #[allow(dead_code)]
    fn get_events(&self) -> Vec<String> {
        self.lifecycle_events.lock().unwrap().clone()
    }
}

// ============================================================================
// StateLifecycle Enum Tests
// ============================================================================

#[test]
fn test_state_lifecycle_enum_is_mounted() {
    assert!(!StateLifecycle::Created.is_mounted());
    assert!(StateLifecycle::Initialized.is_mounted());
    assert!(StateLifecycle::Ready.is_mounted());
    assert!(!StateLifecycle::Defunct.is_mounted());
}

#[test]
fn test_state_lifecycle_enum_can_build() {
    assert!(!StateLifecycle::Created.can_build());
    assert!(!StateLifecycle::Initialized.can_build());
    assert!(StateLifecycle::Ready.can_build());
    assert!(!StateLifecycle::Defunct.can_build());
}

// ============================================================================
// StatefulElement Lifecycle Tracking Tests
// ============================================================================

#[test]
fn test_stateful_element_initial_state_lifecycle() {
    let widget = CounterWidget { initial: 0 };
    let element = widget.into_element();

    // State should be Created after construction
    assert_eq!(element.state_lifecycle(), StateLifecycle::Created);
}

#[test]
fn test_stateful_element_mount_transitions() {
    let widget = CounterWidget { initial: 0 };
    let mut element = widget.into_element();

    // Initially Created
    assert_eq!(element.state_lifecycle(), StateLifecycle::Created);

    // Mount transitions: Created → Initialized → Ready
    element.mount(None, 0);
    assert_eq!(element.state_lifecycle(), StateLifecycle::Ready);
}

#[test]
fn test_stateful_element_unmount_transitions() {
    let widget = CounterWidget { initial: 0 };
    let mut element = widget.into_element();

    element.mount(None, 0);
    assert_eq!(element.state_lifecycle(), StateLifecycle::Ready);

    // Unmount transitions to Defunct
    element.unmount();
    assert_eq!(element.state_lifecycle(), StateLifecycle::Defunct);
}

#[test]
fn test_stateful_element_full_lifecycle() {
    let widget = CounterWidget { initial: 0 };
    let mut element = widget.into_element();

    // Created → Ready → Defunct
    assert_eq!(element.state_lifecycle(), StateLifecycle::Created);

    element.mount(None, 0);
    assert_eq!(element.state_lifecycle(), StateLifecycle::Ready);

    element.unmount();
    assert_eq!(element.state_lifecycle(), StateLifecycle::Defunct);
}

// ============================================================================
// State Lifecycle Callback Order Tests
// ============================================================================

#[test]
fn test_state_callbacks_on_mount() {
    let widget = CounterWidget { initial: 0 };
    let mut element = widget.into_element();

    // Mount should call init_state() then did_change_dependencies()
    element.mount(None, 0);

    // Verify the lifecycle transitioned correctly
    // (We can't easily access the events from inside the state, but we can verify lifecycle)
    assert_eq!(element.state_lifecycle(), StateLifecycle::Ready);
}

#[test]
fn test_state_lifecycle_progression() {
    let widget = CounterWidget { initial: 0 };
    let mut element = widget.into_element();

    // Step 1: Created
    assert_eq!(element.state_lifecycle(), StateLifecycle::Created);
    assert!(!element.state_lifecycle().is_mounted());
    assert!(!element.state_lifecycle().can_build());

    // Step 2: Mount (Created → Initialized → Ready)
    element.mount(None, 0);
    assert_eq!(element.state_lifecycle(), StateLifecycle::Ready);
    assert!(element.state_lifecycle().is_mounted());
    assert!(element.state_lifecycle().can_build());

    // Step 3: Unmount (Ready → Defunct)
    element.unmount();
    assert_eq!(element.state_lifecycle(), StateLifecycle::Defunct);
    assert!(!element.state_lifecycle().is_mounted());
    assert!(!element.state_lifecycle().can_build());
}

// ============================================================================
// Lifecycle Validation Tests
// ============================================================================

#[test]
#[should_panic(expected = "State must be Created before mount")]
fn test_cannot_mount_twice() {
    let widget = CounterWidget { initial: 0 };
    let mut element = widget.into_element();

    element.mount(None, 0);
    // Second mount should panic
    element.mount(None, 0);
}

#[test]
#[should_panic(expected = "State must be mounted before unmount")]
fn test_cannot_unmount_before_mount() {
    let widget = CounterWidget { initial: 0 };
    let mut element = widget.into_element();

    // Unmount without mount should panic
    element.unmount();
}

#[test]
#[should_panic(expected = "State must be Ready to build")]
fn test_cannot_build_before_mount() {
    let widget = CounterWidget { initial: 0 };
    let mut element = widget.into_element();

    // Try to rebuild before mount should panic
    element.mark_dirty();
    element.rebuild();
}

// ============================================================================
// Reassemble Tests (Hot Reload)
// ============================================================================

#[test]
fn test_reassemble() {
    let widget = CounterWidget { initial: 0 };
    let mut element = widget.into_element();

    element.mount(None, 0);
    assert_eq!(element.state_lifecycle(), StateLifecycle::Ready);

    // Reassemble should keep state Ready and mark dirty
    element.reassemble();
    assert_eq!(element.state_lifecycle(), StateLifecycle::Ready);
    assert!(element.is_dirty());
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_multiple_reassemble_calls() {
    let widget = CounterWidget { initial: 0 };
    let mut element = widget.into_element();

    element.mount(None, 0);

    // Multiple reassemble calls should be safe
    element.reassemble();
    element.reassemble();
    element.reassemble();

    assert_eq!(element.state_lifecycle(), StateLifecycle::Ready);
}

#[test]
fn test_state_lifecycle_after_update() {
    let widget = CounterWidget { initial: 0 };
    let mut element = widget.into_element();

    element.mount(None, 0);
    assert_eq!(element.state_lifecycle(), StateLifecycle::Ready);

    // Update widget
    let new_widget = Box::new(CounterWidget { initial: 5 });
    element.update_any(new_widget);

    // State lifecycle should remain Ready after update
    assert_eq!(element.state_lifecycle(), StateLifecycle::Ready);
}
