//! Integration tests for flui-foundation
//!
//! These tests verify that all components work together correctly
//! in realistic usage scenarios.

use std::sync::Arc;

use flui_foundation::{ChangeNotifier, DiagnosticLevel, DiagnosticsNode, Listenable};

// ============================================================================
// ID System Integration Tests
// ============================================================================

// ============================================================================
// Key System Integration Tests
// ============================================================================

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
// Combined Feature Integration Tests
// ============================================================================
