//! Integration tests for flui-foundation
//!
//! These tests verify that all components work together correctly
//! in realistic usage scenarios.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use flui_foundation::{
    ChangeNotifier, DiagnosticLevel, Diagnosticable, DiagnosticsBuilder, DiagnosticsNode,
    ElementId, Key, LayerId, Listenable, ListenerId, RenderId, SemanticsId, ValueNotifier, ViewId,
};
use flui_types::platform::TargetPlatform;

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

/// Test ElementId ordering and index extraction for tree navigation.
///
/// `ElementId` is a generational arena key — `Add`/`Sub` are not meaningful
/// (packing a generation into the high 32 bits makes raw arithmetic wrong).
/// Callers navigate by slot index; ordering by packed value is stable and
/// suitable for sorted collections / BTreeMap keys.
#[test]
fn test_id_ordering_for_tree_navigation() {
    use std::num::NonZeroU32;

    let gen1 = NonZeroU32::MIN;

    // Two ids at consecutive slots, same generation.
    let id_99 = ElementId::new_gen(98, gen1); // slot 98 (0-based)
    let id_100 = ElementId::new_gen(99, gen1); // slot 99 (0-based)
    let id_101 = ElementId::new_gen(100, gen1); // slot 100 (0-based)

    assert_eq!(id_100.index(), 99);
    assert_eq!(id_99.index(), 98);
    assert_eq!(id_101.index(), 100);

    // Ordering is by packed value (generation << 32 | index) — stable
    // for same-generation ids sorted by slot position.
    assert!(id_99 < id_100);
    assert!(id_100 < id_101);
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

// ============================================================================
// Observer System Integration Tests — removed
// ============================================================================
//
// `ObserverList` was deleted (zero in-workspace consumers; `ChangeNotifier`
// from `notifier.rs` is the canonical Listenable-pattern primitive). The
// pre-cycle `test_observer_event_handling` exercised the deleted type and
// was removed alongside the source.

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
// Error Handling Integration Tests — removed
// ============================================================================
//
// `FoundationError` + `ErrorContext` were deleted (zero in-workspace
// consumers; `anyhow::Context` covers the chaining pattern and the rest of
// the workspace already uses `anyhow` / `thiserror` directly). The
// pre-cycle `test_error_context_chaining` and `test_error_recovery` tests
// exercised the deleted types and were removed alongside the source.

// ============================================================================
// Platform Integration Tests
// ============================================================================

/// Test platform detection via the canonical type in `flui-types`.
#[test]
fn test_platform_detection() {
    let platform = TargetPlatform::current();

    // Platform should have a non-empty static string identifier.
    let platform_str = platform.as_str();
    assert!(!platform_str.is_empty());

    // Default matches current.
    assert_eq!(TargetPlatform::default(), platform);
}

// ============================================================================
// Combined Feature Integration Tests
// ============================================================================

/// Test a realistic widget state management scenario
#[test]
fn test_widget_state_management() {
    // Simulate widget with state + a `ListenerId` tracked alongside (audit
    // I-1: `ObserverList` was deleted as a zero-consumer parallel API;
    // `ValueNotifier` + `Vec<ListenerId>` covers the same shape).
    struct Widget {
        id: ElementId,
        state: ValueNotifier<i32>,
        listener_ids: Vec<ListenerId>,
    }

    impl Widget {
        fn new(id: ElementId) -> Self {
            Self {
                id,
                state: ValueNotifier::new(0),
                listener_ids: Vec::new(),
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

    widget.listener_ids.push(listener_id);

    // Trigger state changes
    widget.increment();
    widget.increment();
    widget.increment();

    assert_eq!(*widget.state.value(), 3);
    assert_eq!(rebuild_count.load(Ordering::SeqCst), 3);
    // ElementId::new(1) is 1-based: index() == 0 (0-based slab slot).
    assert_eq!(widget.id.index(), 0);
    assert_eq!(widget.listener_ids.len(), 1);
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

        fn depth_first_indices(&self) -> Vec<u32> {
            let mut ids = vec![self.id.index()];
            for child in &self.children {
                ids.extend(child.depth_first_indices());
            }
            ids
        }
    }

    // Build tree: ElementId::new(n) stores slot index n-1.
    let mut root = TreeNode::new(1); // index 0

    let mut child1 = TreeNode::new(2); // index 1
    child1.add_child(TreeNode::new(4)); // index 3
    child1.add_child(TreeNode::new(5)); // index 4

    let child2 = TreeNode::new(3); // index 2

    root.add_child(child1);
    root.add_child(child2);

    // Verify depth-first slot-index order (0-based).
    let ids = root.depth_first_indices();
    assert_eq!(ids, vec![0, 1, 3, 4, 2]);
}

/// Test thread safety of foundation types
#[test]
fn test_thread_safety() {
    use std::thread;

    // Test that ElementId can be sent between threads.
    // ElementId::new(42) is 1-based: index() == 41.
    let id = ElementId::new(42);
    let handle = thread::spawn(move || {
        assert_eq!(id.index(), 41);
        id
    });
    let returned_id = handle.join().unwrap();
    assert_eq!(returned_id.index(), 41);

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
