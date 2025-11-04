//! Basic Signal usage example
//!
//! Demonstrates core Signal features: Copy semantics, automatic tracking,
//! and subscriptions.

use flui_reactive::{Signal, create_scope};

fn main() {
    println!("=== Basic Signal Example ===\n");

    // 1. Creating signals
    println!("1. Creating signals:");
    let count = Signal::new(0);
    let name = Signal::new("Counter".to_string());
    println!("   count = Signal::new(0)");
    println!("   name = Signal::new(\"Counter\")");
    println!();

    // 2. Reading signals
    println!("2. Reading signals:");
    println!("   count.get() = {}", count.get());
    println!("   name.get() = {}", name.get());
    println!();

    // 3. Signal is Copy!
    println!("3. Signal is Copy (just 8 bytes):");
    let count_copy = count;  // No .clone() needed!
    println!("   let count_copy = count;  // Copy, not clone!");
    println!("   count_copy.get() = {}", count_copy.get());
    println!();

    // 4. Writing to signals
    println!("4. Writing to signals:");
    count.set(10);
    println!("   count.set(10)");
    println!("   count.get() = {}", count.get());
    println!();

    count.update(|v| *v += 5);
    println!("   count.update(|v| *v += 5)");
    println!("   count.get() = {}", count.get());
    println!();

    // 5. Convenience methods
    println!("5. Convenience methods:");
    count.increment();
    println!("   count.increment()");
    println!("   count.get() = {}", count.get());
    println!();

    // 6. Subscriptions
    println!("6. Subscriptions (observing changes):");
    use std::sync::{Arc, atomic::{AtomicI32, Ordering}};

    let observed_value = Arc::new(AtomicI32::new(0));
    let observed_clone = Arc::clone(&observed_value);

    let _sub_id = count.subscribe(Arc::new(move || {
        println!("   [Subscription] Count changed!");
    }));

    count.set(100);
    println!("   count.set(100) -> triggers subscription");
    println!();

    // 7. Reactive scopes (automatic tracking)
    println!("7. Reactive scopes (automatic dependency tracking):");
    let a = Signal::new(10);
    let b = Signal::new(20);

    let (_scope_id, result, deps) = create_scope(|| {
        // Both a and b are automatically tracked
        let sum = a.get() + b.get();
        println!("   Inside scope: a.get() + b.get() = {}", sum);
        sum
    });

    println!("   Result: {}", result);
    println!("   Tracked {} signals automatically", deps.len());
    println!();

    // 8. with() for non-Copy types (avoids cloning)
    println!("8. Using with() for non-Copy types:");
    name.with(|s| {
        println!("   name.with(|s| s.len()) = {}", s.len());
    });
    println!();

    println!("=== Summary ===");
    println!("✓ Signals are Copy (8 bytes each)");
    println!("✓ No manual cloning needed");
    println!("✓ Automatic dependency tracking in scopes");
    println!("✓ Subscribe to changes");
    println!("✓ Type-safe and efficient");
}
