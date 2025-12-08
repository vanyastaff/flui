//! Observer pattern example
//!
//! This example demonstrates the efficient observer list implementations
//! for managing event listeners and callbacks.

use flui_foundation::{HashedObserverList, ObserverList, SyncObserverList};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

fn main() {
    println!("=== FLUI Foundation: Observer Pattern Example ===\n");

    // -------------------------------------------------------------------------
    // Basic ObserverList
    // -------------------------------------------------------------------------
    println!("1. Basic ObserverList");
    println!("   -------------------");

    let mut observers: ObserverList<Box<dyn Fn(i32)>> = ObserverList::new();

    // Add observers
    let id1 = observers.add(Box::new(|value| {
        println!("   Observer A received: {value}");
    }));

    let id2 = observers.add(Box::new(|value| {
        println!("   Observer B received: {}", value * 2);
    }));

    let _id3 = observers.add(Box::new(|value| {
        println!("   Observer C received: {}", value + 100);
    }));

    println!("   Observer count: {}", observers.len());
    println!("   Dispatching event with value 10:");

    for observer in observers.iter() {
        observer(10);
    }

    // Remove an observer
    println!("\n   Removing Observer A and B...");
    observers.remove(id1);
    observers.remove(id2);

    println!("   Observer count: {}", observers.len());
    println!("   Dispatching event with value 5:");

    for observer in observers.iter() {
        observer(5);
    }
    println!();

    // -------------------------------------------------------------------------
    // ObserverList with slot reuse
    // -------------------------------------------------------------------------
    println!("2. Slot Reuse");
    println!("   -----------");

    let mut observers: ObserverList<i32> = ObserverList::new();

    // Add and remove to demonstrate slot reuse
    let id1 = observers.add(1);
    let id2 = observers.add(2);
    let _id3 = observers.add(3);

    println!("   After adding 3 observers:");
    println!("   - Internal slots: {:?}", observers.len());

    observers.remove(id1);
    observers.remove(id2);

    println!("   After removing 2 observers:");
    println!("   - Active observers: {}", observers.len());

    // Adding new observers reuses slots
    let _id4 = observers.add(4);
    let _id5 = observers.add(5);

    println!("   After adding 2 more (reusing slots):");
    println!("   - Active observers: {}", observers.len());

    // Compact to remove empty slots
    observers.compact();
    println!("   After compact():");
    println!("   - Values: {:?}", observers.iter().collect::<Vec<_>>());
    println!();

    // -------------------------------------------------------------------------
    // Thread-safe SyncObserverList
    // -------------------------------------------------------------------------
    println!("3. SyncObserverList (Thread-safe)");
    println!("   --------------------------------");

    let observers: Arc<SyncObserverList<i32>> = Arc::new(SyncObserverList::new());

    // Spawn threads to add observers concurrently
    let handles: Vec<_> = (0..5)
        .map(|i| {
            let obs = Arc::clone(&observers);
            thread::spawn(move || {
                let _id = obs.add(i * 10);
                println!("   Thread {i} added observer with value {}", i * 10);
            })
        })
        .collect();

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    println!("   Total observers: {}", observers.len());

    // Iterate safely
    print!("   Values: ");
    observers.for_each(|v| print!("{v} "));
    println!("\n");

    // -------------------------------------------------------------------------
    // HashedObserverList for O(1) operations
    // -------------------------------------------------------------------------
    println!("4. HashedObserverList (O(1) Operations)");
    println!("   -------------------------------------");

    let observers: HashedObserverList<String> = HashedObserverList::new();

    // Add many observers
    let ids: Vec<_> = (0..10)
        .map(|i| observers.add(format!("Observer_{i}")))
        .collect();

    println!("   Added {} observers", observers.len());

    // Remove specific observers by ID
    observers.remove(ids[0]);
    observers.remove(ids[5]);
    observers.remove(ids[9]);

    println!("   After removing 3 by ID: {} remain", observers.len());

    // Iterate
    print!("   Remaining: ");
    observers.for_each(|s| print!("{s} "));
    println!("\n");

    // -------------------------------------------------------------------------
    // Real-world example: Event dispatcher
    // -------------------------------------------------------------------------
    println!("5. Event Dispatcher Pattern");
    println!("   --------------------------");

    type EventCallback = Box<dyn Fn(&str) + Send + Sync>;

    struct EventDispatcher {
        listeners: ObserverList<EventCallback>,
    }

    impl EventDispatcher {
        fn new() -> Self {
            Self {
                listeners: ObserverList::new(),
            }
        }

        fn on_event(
            &mut self,
            callback: impl Fn(&str) + Send + Sync + 'static,
        ) -> flui_foundation::ObserverId {
            self.listeners.add(Box::new(callback))
        }

        fn emit(&self, event: &str) {
            for listener in self.listeners.iter() {
                listener(event);
            }
        }
    }

    let mut dispatcher = EventDispatcher::new();

    let event_count = Arc::new(AtomicUsize::new(0));

    let count1 = Arc::clone(&event_count);
    let _ = dispatcher.on_event(move |event| {
        count1.fetch_add(1, Ordering::SeqCst);
        println!("   Handler 1: {event}");
    });

    let count2 = Arc::clone(&event_count);
    let _ = dispatcher.on_event(move |event| {
        count2.fetch_add(1, Ordering::SeqCst);
        println!("   Handler 2: {event}");
    });

    println!("   Emitting 'button_click'...");
    dispatcher.emit("button_click");

    println!("   Emitting 'form_submit'...");
    dispatcher.emit("form_submit");

    println!(
        "   Total events handled: {}",
        event_count.load(Ordering::SeqCst)
    );
    println!();

    println!("=== Example Complete ===");
}
