//! Change notification example
//!
//! This example demonstrates the reactive change notification system,
//! similar to Flutter's ChangeNotifier pattern.

use flui_foundation::{ChangeNotifier, Listenable, MergedListenable, ValueNotifier};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn main() {
    println!("=== FLUI Foundation: Change Notification Example ===\n");

    // -------------------------------------------------------------------------
    // Basic ChangeNotifier
    // -------------------------------------------------------------------------
    println!("1. Basic ChangeNotifier");
    println!("   ---------------------");

    let notifier = ChangeNotifier::new();

    // Add a listener
    let listener_id = notifier.add_listener(Arc::new(|| {
        println!("   [Listener 1] Notified!");
    }));
    println!("   Added listener with ID: {listener_id}");

    // Add another listener
    let _listener_id2 = notifier.add_listener(Arc::new(|| {
        println!("   [Listener 2] Also notified!");
    }));

    println!("   Has listeners: {}", notifier.has_listeners());
    println!("   Listener count: {}", notifier.len());

    // Trigger notification
    println!("   Calling notify_listeners()...");
    notifier.notify_listeners();
    println!();

    // -------------------------------------------------------------------------
    // ValueNotifier - Holds a value and notifies on change
    // -------------------------------------------------------------------------
    println!("2. ValueNotifier");
    println!("   ---------------");

    let mut counter = ValueNotifier::new(0);

    let change_count = Arc::new(AtomicUsize::new(0));
    let change_count_clone = Arc::clone(&change_count);

    let _listener = counter.add_listener(Arc::new(move || {
        change_count_clone.fetch_add(1, Ordering::SeqCst);
    }));

    println!("   Initial value: {}", counter.value());

    // set_value only notifies if value changed
    counter.set_value(5);
    println!("   After set_value(5): {}", counter.value());
    println!(
        "   Notifications so far: {}",
        change_count.load(Ordering::SeqCst)
    );

    counter.set_value(5); // Same value - no notification
    println!("   After set_value(5) again: {}", counter.value());
    println!(
        "   Notifications so far: {}",
        change_count.load(Ordering::SeqCst)
    );

    // set_value_force always notifies
    counter.set_value_force(5);
    println!("   After set_value_force(5): {}", counter.value());
    println!(
        "   Notifications so far: {}",
        change_count.load(Ordering::SeqCst)
    );

    // Update with closure
    counter.update(|v| *v += 10);
    println!("   After update(|v| *v += 10): {}", counter.value());
    println!(
        "   Notifications so far: {}",
        change_count.load(Ordering::SeqCst)
    );
    println!();

    // -------------------------------------------------------------------------
    // Complex State with ValueNotifier
    // -------------------------------------------------------------------------
    println!("3. Complex State");
    println!("   ---------------");

    #[derive(Clone, PartialEq, Debug)]
    struct AppState {
        count: i32,
        name: String,
    }

    let mut state = ValueNotifier::new(AppState {
        count: 0,
        name: "App".to_string(),
    });

    let _listener = state.add_listener(Arc::new(|| {
        println!("   [State] Changed!");
    }));

    println!("   Initial state: {:?}", state.value());

    state.update(|s| {
        s.count += 1;
        s.name = "Updated App".to_string();
    });
    println!("   Updated state: {:?}", state.value());
    println!();

    // -------------------------------------------------------------------------
    // MergedListenable - Combine multiple notifiers
    // -------------------------------------------------------------------------
    println!("4. MergedListenable");
    println!("   ------------------");

    let notifier_a = ChangeNotifier::new();
    let notifier_b = ChangeNotifier::new();

    let merged = MergedListenable::new(vec![Box::new(notifier_a), Box::new(notifier_b)]);

    let _listener = merged.add_listener(Arc::new(|| {
        println!("   [Merged] Received notification!");
    }));

    println!("   Source count: {}", merged.source_count());
    println!("   Calling merged.notify()...");
    merged.notify();
    println!();

    // -------------------------------------------------------------------------
    // Removing Listeners
    // -------------------------------------------------------------------------
    println!("5. Removing Listeners");
    println!("   --------------------");

    let notifier = ChangeNotifier::new();

    let id1 = notifier.add_listener(Arc::new(|| println!("   [A] Called")));
    let id2 = notifier.add_listener(Arc::new(|| println!("   [B] Called")));
    let _id3 = notifier.add_listener(Arc::new(|| println!("   [C] Called")));

    println!("   Listener count: {}", notifier.len());
    println!("   Notifying all...");
    notifier.notify_listeners();

    println!("   Removing listener A and B...");
    notifier.remove_listener(id1);
    notifier.remove_listener(id2);

    println!("   Listener count: {}", notifier.len());
    println!("   Notifying remaining...");
    notifier.notify_listeners();
    println!();

    println!("=== Example Complete ===");
}
