//! Integration tests for flui-foundation
//!
//! These tests verify that all components work together correctly
//! in realistic usage scenarios.

use flui_foundation::{
    error::ErrorContext, ChangeNotifier, DiagnosticLevel, Diagnosticable, DiagnosticsBuilder,
    DiagnosticsNode, ElementId, FoundationError, HashedObserverList, Key, LayerId, Listenable,
    ListenerId, MergedListenable, ObserverId, ObserverList, RenderId, Result, SemanticsId,
    SyncObserverList, TargetPlatform, ValueNotifier, ViewId,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

// ============================================================================
// ID System Integration Tests
// ============================================================================

/// Test that IDs can be used as HashMap keys
#[test]
fn test_ids_as_hash_keys() {
    let mut element_map: HashMap<ElementId, String> = HashMap::new();
    let mut view_map: HashMap<ViewId, String> = HashMap::new();
    let mut render_map: HashMap<RenderId, String> = HashMap::new();

    // Insert IDs
    let elem_id = ElementId::new(1);
    let view_id = ViewId::new(2);
    let render_id = RenderId::new(3);

    element_map.insert(elem_id, "Element 1".to_string());
    view_map.insert(view_id, "View 2".to_string());
    render_map.insert(render_id, "Render 3".to_string());

    // Retrieve and verify
    assert_eq!(element_map.get(&elem_id), Some(&"Element 1".to_string()));
    assert_eq!(view_map.get(&view_id), Some(&"View 2".to_string()));
    assert_eq!(render_map.get(&render_id), Some(&"Render 3".to_string()));
}

/// Test ID arithmetic for tree navigation
#[test]
fn test_id_arithmetic_for_tree_navigation() {
    let base_id = ElementId::new(100);

    // Navigate to siblings using Add (returns ElementId)
    let next_sibling = base_id + 1;
    assert_eq!(next_sibling.get(), 101);

    // Sub returns usize (distance between IDs)
    let distance = base_id - 1;
    assert_eq!(distance, 99);

    // Verify ordering for sorted collections
    let prev_sibling = ElementId::new(99);
    assert!(prev_sibling < base_id);
    assert!(base_id < next_sibling);
}

/// Test that Optional IDs are properly optimized
#[test]
fn test_optional_id_niche_optimization() {
    // All these should be 8 bytes due to NonZeroUsize niche optimization
    assert_eq!(std::mem::size_of::<Option<ElementId>>(), 8);
    assert_eq!(std::mem::size_of::<Option<ViewId>>(), 8);
    assert_eq!(std::mem::size_of::<Option<RenderId>>(), 8);
    assert_eq!(std::mem::size_of::<Option<LayerId>>(), 8);
    assert_eq!(std::mem::size_of::<Option<SemanticsId>>(), 8);
}

// ============================================================================
// Key System Integration Tests
// ============================================================================

/// Test key-based widget identity lookup
#[test]
fn test_key_based_widget_lookup() {
    let mut widget_registry: HashMap<Key, String> = HashMap::new();

    // Register widgets with string keys
    let key1 = Key::from_str("header");
    let key2 = Key::from_str("footer");
    let key3 = Key::from_str("sidebar");

    widget_registry.insert(key1, "HeaderWidget".to_string());
    widget_registry.insert(key2, "FooterWidget".to_string());
    widget_registry.insert(key3, "SidebarWidget".to_string());

    // Lookup by key
    assert_eq!(
        widget_registry.get(&Key::from_str("header")),
        Some(&"HeaderWidget".to_string())
    );

    // Different string keys with same content should match
    let lookup_key = Key::from_str("footer");
    assert_eq!(
        widget_registry.get(&lookup_key),
        Some(&"FooterWidget".to_string())
    );
}

/// Test key uniqueness
#[test]
fn test_key_uniqueness() {
    // Each call to Key::new() should produce a unique key
    let key1 = Key::new();
    let key2 = Key::new();
    let key3 = Key::new();

    assert_ne!(key1, key2);
    assert_ne!(key2, key3);
    assert_ne!(key1, key3);
}

// ============================================================================
// Change Notification Integration Tests
// ============================================================================

/// Test notification chain with multiple listeners
#[test]
fn test_notification_chain() {
    let notifier = ChangeNotifier::new();
    let call_order = Arc::new(std::sync::Mutex::new(Vec::new()));

    // Add multiple listeners
    let order1 = Arc::clone(&call_order);
    let _id1 = notifier.add_listener(Arc::new(move || {
        order1.lock().unwrap().push(1);
    }));

    let order2 = Arc::clone(&call_order);
    let _id2 = notifier.add_listener(Arc::new(move || {
        order2.lock().unwrap().push(2);
    }));

    let order3 = Arc::clone(&call_order);
    let _id3 = notifier.add_listener(Arc::new(move || {
        order3.lock().unwrap().push(3);
    }));

    // Notify all
    notifier.notify_listeners();

    // All listeners should have been called
    let calls = call_order.lock().unwrap();
    assert_eq!(calls.len(), 3);
    assert!(calls.contains(&1));
    assert!(calls.contains(&2));
    assert!(calls.contains(&3));
}

/// Test value notifier with complex state
#[test]
fn test_value_notifier_complex_state() {
    #[derive(Clone, PartialEq, Debug)]
    struct AppState {
        counter: i32,
        name: String,
        items: Vec<String>,
    }

    let mut state = ValueNotifier::new(AppState {
        counter: 0,
        name: "App".to_string(),
        items: vec![],
    });

    let change_count = Arc::new(AtomicUsize::new(0));
    let change_count_clone = Arc::clone(&change_count);

    let _ = state.add_listener(Arc::new(move || {
        change_count_clone.fetch_add(1, Ordering::SeqCst);
    }));

    // Update using closure
    state.update(|s| {
        s.counter += 1;
        s.items.push("Item 1".to_string());
    });

    assert_eq!(state.value().counter, 1);
    assert_eq!(state.value().items.len(), 1);
    assert_eq!(change_count.load(Ordering::SeqCst), 1);

    // Set value (different value)
    state.set_value(AppState {
        counter: 10,
        name: "NewApp".to_string(),
        items: vec!["A".to_string(), "B".to_string()],
    });

    assert_eq!(change_count.load(Ordering::SeqCst), 2);
}

/// Test merged listenable for combining state sources
#[test]
fn test_merged_listenable() {
    let notifier1 = ChangeNotifier::new();
    let notifier2 = ChangeNotifier::new();

    let merged = MergedListenable::new(vec![Box::new(notifier1.clone()), Box::new(notifier2)]);

    let notification_count = Arc::new(AtomicUsize::new(0));
    let count_clone = Arc::clone(&notification_count);

    let _ = merged.add_listener(Arc::new(move || {
        count_clone.fetch_add(1, Ordering::SeqCst);
    }));

    // Notify through merged
    merged.notify();
    assert_eq!(notification_count.load(Ordering::SeqCst), 1);
}

// ============================================================================
// Observer System Integration Tests
// ============================================================================

/// Test observer pattern for event handling
#[test]
fn test_observer_event_handling() {
    let mut observers: ObserverList<Box<dyn Fn(i32) + Send + Sync>> = ObserverList::new();

    let sum = Arc::new(AtomicUsize::new(0));

    // Add observers
    let sum1 = Arc::clone(&sum);
    let id1 = observers.add(Box::new(move |value| {
        sum1.fetch_add(value as usize, Ordering::SeqCst);
    }));

    let sum2 = Arc::clone(&sum);
    let _id2 = observers.add(Box::new(move |value| {
        sum2.fetch_add((value * 2) as usize, Ordering::SeqCst);
    }));

    // Dispatch event
    for observer in observers.iter() {
        observer(10);
    }

    // 10 + 20 = 30
    assert_eq!(sum.load(Ordering::SeqCst), 30);

    // Remove first observer
    observers.remove(id1);

    // Reset and dispatch again
    sum.store(0, Ordering::SeqCst);
    for observer in observers.iter() {
        observer(10);
    }

    // Only second observer: 20
    assert_eq!(sum.load(Ordering::SeqCst), 20);
}

/// Test thread-safe observer list
#[test]
fn test_concurrent_observers() {
    let observers: Arc<SyncObserverList<i32>> = Arc::new(SyncObserverList::new());

    // Add observers from multiple threads
    let handles: Vec<_> = (0..10)
        .map(|i| {
            let obs = Arc::clone(&observers);
            std::thread::spawn(move || {
                let _ = obs.add(i);
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(observers.len(), 10);
}

/// Test hashed observer list for large collections
#[test]
fn test_hashed_observer_list_performance() {
    let observers: HashedObserverList<String> = HashedObserverList::new();

    // Add many observers
    let ids: Vec<ObserverId> = (0..1000)
        .map(|i| observers.add(format!("Observer {i}")))
        .collect();

    assert_eq!(observers.len(), 1000);

    // Remove every other observer
    for id in ids.iter().step_by(2) {
        observers.remove(*id);
    }

    assert_eq!(observers.len(), 500);
}

// ============================================================================
// Diagnostics Integration Tests
// ============================================================================

/// Test diagnostics for widget tree debugging
#[test]
fn test_widget_tree_diagnostics() {
    // Simulate a widget tree
    let tree = DiagnosticsNode::new("MaterialApp")
        .property("theme", "light")
        .with_level(DiagnosticLevel::Info)
        .child(
            DiagnosticsNode::new("Scaffold")
                .property("hasAppBar", true)
                .child(
                    DiagnosticsNode::new("Column")
                        .property("mainAxisAlignment", "center")
                        .child(DiagnosticsNode::new("Text").property("data", "Hello World"))
                        .child(
                            DiagnosticsNode::new("ElevatedButton")
                                .property("onPressed", "<closure>")
                                .child(DiagnosticsNode::new("Text").property("data", "Click Me")),
                        ),
                ),
        );

    let output = tree.format_deep(0);

    assert!(output.contains("MaterialApp"));
    assert!(output.contains("Scaffold"));
    assert!(output.contains("Column"));
    assert!(output.contains("Text"));
    assert!(output.contains("Hello World"));
}

/// Test custom diagnosticable implementation
#[test]
fn test_custom_diagnosticable() {
    #[derive(Debug)]
    struct CustomWidget {
        width: f32,
        height: f32,
        visible: bool,
        child_count: usize,
    }

    impl Diagnosticable for CustomWidget {
        fn debug_fill_properties(&self, properties: &mut DiagnosticsBuilder) {
            properties.add("width", self.width);
            properties.add("height", self.height);
            if !self.visible {
                properties.add_with_level("visible", self.visible, DiagnosticLevel::Warning);
            }
            properties.add("childCount", self.child_count);
        }
    }

    let widget = CustomWidget {
        width: 100.0,
        height: 50.0,
        visible: false,
        child_count: 3,
    };

    let node = widget.to_diagnostics_node();
    let props = node.properties();

    assert_eq!(props.len(), 4);
    assert_eq!(props[0].name(), "width");
    assert_eq!(props[0].value(), "100");
}

/// Test diagnostics builder
#[test]
fn test_diagnostics_builder_usage() {
    let mut builder = DiagnosticsBuilder::new();

    builder
        .add("id", 42)
        .add("name", "TestWidget")
        .add_flag("isVisible", true, "VISIBLE")
        .add_flag("isDisabled", false, "DISABLED")
        .add_optional("tooltip", Some("Help text"))
        .add_optional::<String>("icon", None)
        .add_with_level("debug_info", "internal", DiagnosticLevel::Debug);

    let props = builder.build();

    // 5 properties (flag=false skipped, optional=None skipped)
    assert_eq!(props.len(), 5);
}

// ============================================================================
// Error Handling Integration Tests
// ============================================================================

/// Test error context chaining
#[test]
fn test_error_context_chaining() {
    fn inner_operation() -> Result<i32> {
        Err(FoundationError::invalid_id(0, "ID cannot be zero"))
    }

    fn middle_operation() -> Result<i32> {
        inner_operation().with_context("in middle_operation")
    }

    fn outer_operation() -> Result<i32> {
        middle_operation().with_context("in outer_operation")
    }

    let result = outer_operation();
    assert!(result.is_err());

    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("in outer_operation"));
}

/// Test error recovery patterns
#[test]
fn test_error_recovery() {
    fn fallible_operation(succeed: bool) -> Result<i32> {
        if succeed {
            Ok(42)
        } else {
            Err(FoundationError::listener_error(
                "add",
                "listener limit reached",
            ))
        }
    }

    // Test recoverable error
    let err = fallible_operation(false).unwrap_err();
    assert!(err.is_recoverable());

    // Retry pattern
    let result = fallible_operation(false)
        .or_else(|_| fallible_operation(true))
        .unwrap();
    assert_eq!(result, 42);
}

// ============================================================================
// Platform Integration Tests
// ============================================================================

/// Test platform detection
#[test]
fn test_platform_detection() {
    let platform = TargetPlatform::current();

    // At least one category should be true
    let is_any_platform = platform.is_desktop() || platform.is_mobile() || platform.is_web();
    assert!(is_any_platform);

    // Platform should have a string representation
    let platform_str = platform.as_str();
    assert!(!platform_str.is_empty());
}

// ============================================================================
// Combined Feature Integration Tests
// ============================================================================

/// Test a realistic widget state management scenario
#[test]
fn test_widget_state_management() {
    // Simulate widget with state and observers
    struct Widget {
        id: ElementId,
        state: ValueNotifier<i32>,
        observers: ObserverList<ListenerId>,
    }

    impl Widget {
        fn new(id: ElementId) -> Self {
            Self {
                id,
                state: ValueNotifier::new(0),
                observers: ObserverList::new(),
            }
        }

        fn increment(&mut self) {
            self.state.update(|v| *v += 1);
        }
    }

    let mut widget = Widget::new(ElementId::new(1));

    let rebuild_count = Arc::new(AtomicUsize::new(0));
    let rebuild_clone = Arc::clone(&rebuild_count);

    let listener_id = widget.state.add_listener(Arc::new(move || {
        rebuild_clone.fetch_add(1, Ordering::SeqCst);
    }));

    let _observer_id = widget.observers.add(listener_id);

    // Trigger state changes
    widget.increment();
    widget.increment();
    widget.increment();

    assert_eq!(*widget.state.value(), 3);
    assert_eq!(rebuild_count.load(Ordering::SeqCst), 3);
    assert_eq!(widget.id.get(), 1);
}

/// Test tree structure with IDs
#[test]
fn test_tree_structure() {
    struct TreeNode {
        id: ElementId,
        children: Vec<TreeNode>,
    }

    impl TreeNode {
        fn new(id: usize) -> Self {
            Self {
                id: ElementId::new(id),
                children: Vec::new(),
            }
        }

        fn add_child(&mut self, child: TreeNode) {
            self.children.push(child);
        }

        fn depth_first_ids(&self) -> Vec<usize> {
            let mut ids = vec![self.id.get()];
            for child in &self.children {
                ids.extend(child.depth_first_ids());
            }
            ids
        }
    }

    // Build tree
    let mut root = TreeNode::new(1);

    let mut child1 = TreeNode::new(2);
    child1.add_child(TreeNode::new(4));
    child1.add_child(TreeNode::new(5));

    let child2 = TreeNode::new(3);

    root.add_child(child1);
    root.add_child(child2);

    // Verify structure
    let ids = root.depth_first_ids();
    assert_eq!(ids, vec![1, 2, 4, 5, 3]);
}

/// Test thread safety of foundation types
#[test]
fn test_thread_safety() {
    use std::thread;

    // Test that ElementId can be sent between threads
    let id = ElementId::new(42);
    let handle = thread::spawn(move || {
        assert_eq!(id.get(), 42);
        id
    });
    let returned_id = handle.join().unwrap();
    assert_eq!(returned_id.get(), 42);

    // Test that Key can be sent between threads
    let key = Key::from_str("test");
    let handle = thread::spawn(move || {
        assert_eq!(key.as_u64(), Key::from_str("test").as_u64());
        key
    });
    let returned_key = handle.join().unwrap();
    assert_eq!(returned_key.as_u64(), Key::from_str("test").as_u64());

    // Test that ChangeNotifier works across threads
    let notifier = Arc::new(ChangeNotifier::new());
    let counter = Arc::new(AtomicUsize::new(0));

    let counter_clone = Arc::clone(&counter);
    let _ = notifier.add_listener(Arc::new(move || {
        counter_clone.fetch_add(1, Ordering::SeqCst);
    }));

    let notifier_clone = Arc::clone(&notifier);
    let handle = thread::spawn(move || {
        notifier_clone.notify_listeners();
    });
    handle.join().unwrap();

    assert_eq!(counter.load(Ordering::SeqCst), 1);
}
