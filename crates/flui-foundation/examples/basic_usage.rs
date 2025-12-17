//! Basic usage example for FLUI Foundation
//!
//! This example demonstrates the core functionality of FLUI Foundation types:
//! - Element identification (ElementId, Key)
//! - Change notification (ChangeNotifier, ValueNotifier)
//! - Atomic flags for state management
//! - Diagnostics for debugging
//!
//! Run with: cargo run --example basic_usage

use flui_foundation::prelude::*;
use flui_foundation::{DiagnosticsNode, FoundationError};
use std::sync::Arc;

fn main() {
    println!("🏗️  FLUI Foundation - Basic Usage Example\n");

    // ========================================================================
    // ELEMENT IDENTIFICATION
    // ========================================================================

    println!("📋 Element Identification:");

    // ElementId - unique identifiers for elements in the tree
    let element1 = ElementId::new(1);
    let element2 = ElementId::new(42);

    println!("  Element 1 ID: {}", element1);
    println!("  Element 2 ID: {}", element2);
    println!("  Are they equal? {}", element1 == element2);

    // Key - for widget identity across rebuilds
    let key1 = Key::new();
    let key2 = Key::new();

    println!("  Key 1: {}", key1);
    println!("  Key 2: {}", key2);
    println!("  Keys are unique: {}", key1 != key2);
    println!();

    // ========================================================================
    // CHANGE NOTIFICATION
    // ========================================================================

    println!("📢 Change Notification:");

    // Basic change notifier
    let notifier = ChangeNotifier::new();

    // Add listeners
    let listener1 = notifier.add_listener(Arc::new(|| {
        println!("  📨 Listener 1: Something changed!");
    }));

    let _listener2 = notifier.add_listener(Arc::new(|| {
        println!("  📨 Listener 2: I also noticed the change!");
    }));

    println!("  Added {} listeners", 2);

    // Notify all listeners
    println!("  Triggering notification...");
    notifier.notify_listeners();

    // Remove a listener
    notifier.remove_listener(listener1);
    println!("  Removed listener 1, notifying again...");
    notifier.notify_listeners();

    println!();

    // ========================================================================
    // VALUE NOTIFICATION
    // ========================================================================

    println!("💎 Value Notification:");

    let mut value_notifier = ValueNotifier::new(0);

    // Add value listener
    let _value_listener = value_notifier.add_listener(Arc::new(|| {
        println!("  📊 Value changed!");
    }));

    // Update values
    println!("  Setting value to 42...");
    *value_notifier.value_mut() = 42;
    value_notifier.notify();

    println!("  Current value: {}", value_notifier.value());

    println!("  Updating value with closure...");
    *value_notifier.value_mut() *= 2;
    value_notifier.notify();

    println!("  Final value: {}", value_notifier.value());
    println!();

    // ========================================================================
    // DIAGNOSTICS
    // ========================================================================

    println!("🐛 Diagnostics:");

    // Create a diagnostic tree
    let root_node = DiagnosticsNode::new("MyApp")
        .property("version", "1.0.0")
        .property("debug_mode", true)
        .child(
            DiagnosticsNode::new("MainView")
                .property("width", 800.0)
                .property("height", 600.0)
                .child(
                    DiagnosticsNode::new("Button")
                        .property("text", "Click me!")
                        .property("enabled", true),
                )
                .child(
                    DiagnosticsNode::new("TextField")
                        .property("placeholder", "Enter text...")
                        .property("text", "Hello, World!"),
                ),
        );

    println!("  Diagnostic tree (sparse format):");
    println!("{}", root_node.format_deep(0));

    println!("  Diagnostic tree (dense format):");
    println!("{}", root_node.format_deep(0));

    // ========================================================================
    // ERROR HANDLING
    // ========================================================================

    println!("❌ Error Handling:");

    // Create and categorize errors
    let id_error = FoundationError::invalid_id(0, "ElementId cannot be zero");
    let listener_error = FoundationError::listener_error("add", "listener callback failed");

    println!("  ID Error: {}", id_error);
    println!("  Category: {}", id_error.category());
    println!("  Recoverable: {}", id_error.is_recoverable());
    println!();

    println!("  Listener Error: {}", listener_error);
    println!("  Category: {}", listener_error.category());
    println!("  Recoverable: {}", listener_error.is_recoverable());
    println!();

    println!("✅ All examples completed successfully!");
}

// Example of a custom diagnostic provider
#[derive(Debug)]
#[allow(dead_code)]
struct MyWidget {
    name: String,
    count: i32,
    enabled: bool,
}

#[allow(dead_code)]
impl MyWidget {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            count: 0,
            enabled: true,
        }
    }
}

impl Diagnosticable for MyWidget {
    fn to_diagnostics_node(&self) -> DiagnosticsNode {
        DiagnosticsNode::new("MyWidget")
            .property("name", &self.name)
            .property("count", self.count)
            .property("enabled", self.enabled)
    }
}

// Example showing thread safety with ChangeNotifier
#[allow(dead_code)]
fn demonstrate_thread_safety() {
    use std::thread;

    let notifier = Arc::new(ChangeNotifier::new());

    // Spawn threads to trigger notifications
    let handles: Vec<_> = (0..4)
        .map(|_i| {
            let notifier = notifier.clone();

            thread::spawn(move || {
                // Notify from each thread
                notifier.notify_listeners();
            })
        })
        .collect();

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    println!("All threads completed notifications successfully");
}
