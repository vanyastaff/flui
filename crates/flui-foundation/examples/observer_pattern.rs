//! Observer pattern example
//!
//! This example demonstrates the efficient observer list implementations
//! for managing event listeners and callbacks.

#![allow(
    clippy::items_after_statements,
    reason = "examples define types where they read best, alongside the prose explaining them"
)]

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use flui_foundation::ObserverList;

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
    // Real-world example: Event dispatcher
    // -------------------------------------------------------------------------
    println!("3. Event Dispatcher Pattern");
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
