//! Counter Example using Signal<T>
//!
//! Demonstrates fine-grained reactive state management using Signal primitives.
//! This example shows how to use Signal<T> for efficient, granular updates
//! without rebuilding entire widget trees.

use flui_core::widget::{State, StatefulWidget};
use flui_core::{BuildContext, Widget, Signal};

/// Counter widget using Signal for reactive state
#[derive(Debug, Clone)]
pub struct Counter {
    pub initial_count: i32,
}

impl Counter {
    pub fn new(initial_count: i32) -> Self {
        Self { initial_count }
    }
}

impl StatefulWidget for Counter {
    fn create_state(&self) -> Box<dyn State> {
        Box::new(CounterState {
            count: Signal::new(self.initial_count),
        })
    }
}

/// State for the Counter widget using Signal
#[derive(Debug)]
pub struct CounterState {
    count: Signal<i32>,
}

impl State for CounterState {
    fn build(&mut self, _ctx: &BuildContext) -> Widget {
        // In a real app, you would use the Signal like this:
        //
        // column![
        //     // Only this text widget rebuilds when count changes
        //     text(format!("Count: {}", self.count.get())),
        //
        //     row![
        //         button("-").on_press({
        //             let count = self.count;  // Signal is Copy!
        //             move |_| count.decrement()
        //         }),
        //
        //         button("+").on_press({
        //             let count = self.count;  // No .clone() needed
        //             move |_| count.increment()
        //         }),
        //
        //         button("Reset").on_press({
        //             let count = self.count;
        //             move |_| count.set(0)
        //         })
        //     ]
        // ]

        // For now, return a placeholder widget
        Widget::stateless(DummyWidget)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Dummy widget for demonstration
#[derive(Debug, Clone)]
struct DummyWidget;

impl flui_core::widget::StatelessWidget for DummyWidget {
    fn build(&self, _context: &BuildContext) -> Widget {
        Widget::stateless(DummyWidget)
    }
}

fn main() {
    println!("Counter with Signal<T> API");
    println!("=====================================");
    println!();

    // Demonstrate Signal usage
    println!("Creating a counter with Signal...");
    let count = Signal::new(0);
    println!("Initial count: {}", count.get());
    println!();

    println!("Demonstrating Signal operations:");
    println!("  count.increment() -> {}", { count.increment(); count.get() });
    println!("  count.increment() -> {}", { count.increment(); count.get() });
    println!("  count.set(10)     -> {}", { count.set(10); count.get() });
    println!("  count.update(|v| *v *= 2) -> {}", { count.update(|v| *v *= 2); count.get() });
    println!("  count.decrement() -> {}", { count.decrement(); count.get() });
    println!();

    println!("Signal is Copy (8 bytes):");
    println!("  let count_copy = count;  // No .clone() needed!");
    let count_copy = count;
    println!("  count_copy.get() = {}", count_copy.get());
    println!();

    // Demonstrate subscriptions
    println!("Demonstrating subscriptions:");
    use std::sync::Arc;
    let subscription = count.subscribe(Arc::new(|| {
        println!("  [Subscription] Count changed!");
    }));

    println!("  count.set(100)");
    count.set(100);
    println!();

    // Demonstrate reactive scopes
    println!("Demonstrating reactive scopes:");
    use flui_core::create_scope;

    let a = Signal::new(5);
    let b = Signal::new(10);

    let (_scope_id, result, deps) = create_scope(|| {
        a.get() + b.get()
    });

    println!("  Scope tracked {} signals automatically", deps.len());
    println!("  Result: a + b = {}", result);
    println!();

    println!("=== Comparison with set_state() ===");
    println!();
    println!("set_state() approach:");
    println!("```rust");
    println!("button(\"+\").on_press({{");
    println!("    let ctx = ctx.clone();");
    println!("    move |_| {{");
    println!("        ctx.set_state(|state: &mut CounterState| {{");
    println!("            state.count += 1;");
    println!("        }});");
    println!("    }}");
    println!("}})");
    println!("```");
    println!("  - Rebuilds entire widget");
    println!("  - Need to clone BuildContext");
    println!("  - Verbose closure syntax");
    println!();
    println!("Signal<T> approach:");
    println!("```rust");
    println!("button(\"+\").on_press({{");
    println!("    let count = self.count;  // Copy!");
    println!("    move |_| count.increment()");
    println!("}})");
    println!("```");
    println!("  ✓ Fine-grained updates (only text rebuilds)");
    println!("  ✓ No cloning (Signal is Copy)");
    println!("  ✓ Cleaner syntax");
    println!("  ✓ Better performance");
    println!();
    println!("=== When to use each ===");
    println!();
    println!("Use set_state() when:");
    println!("  • Simple widgets with few state changes");
    println!("  • State changes affect most of the widget");
    println!("  • You want Flutter-familiar API");
    println!();
    println!("Use Signal<T> when:");
    println!("  • High-frequency updates (animations, timers)");
    println!("  • Multiple independent reactive values");
    println!("  • Fine-grained control over what rebuilds");
    println!("  • Maximum performance needed");
}
