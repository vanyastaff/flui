//! Example demonstrating thread-safe hooks system.

use flui_core::hooks::hook_context::{ComponentId, HookContext};
use flui_core::hooks::signal::SignalHook;
use std::thread;

fn main() {
    println!("Testing thread-safe hooks system...");

    // Create a signal in one thread
    let mut ctx = HookContext::new();
    ctx.begin_component(ComponentId(1));
    let signal = ctx.use_hook::<SignalHook<i32>>(0);

    // Clone the signal to send to another thread
    let signal_clone = signal.clone();

    // Spawn a thread that modifies the signal
    let handle = thread::spawn(move || {
        println!("Thread: Setting signal to 42");
        signal_clone.set(42);
        println!("Thread: Signal set successfully");
    });

    // Wait for the thread to complete
    handle.join().unwrap();

    // Read the value from the main thread
    let value = signal.get(&mut ctx);
    println!("Main thread: Signal value is {}", value);

    assert_eq!(value, 42);
    println!("Success! Hooks system is thread-safe.");
}
