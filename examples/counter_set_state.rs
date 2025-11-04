//! Counter Example using ctx.set_state()
//!
//! Demonstrates the new Flutter-style setState API for reactive state management.
//! This example shows how to use BuildContext::set_state() to update state and
//! trigger rebuilds without manual cloning or complex state management.

use flui_core::widget::{State, StatefulWidget};
use flui_core::{BuildContext, Widget};

/// Counter widget with stateful behavior
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
            count: self.initial_count,
        })
    }
}

/// State for the Counter widget
#[derive(Debug)]
pub struct CounterState {
    count: i32,
}

impl State for CounterState {
    fn build(&mut self, _ctx: &BuildContext) -> Widget {
        // Create a simple UI showing the count
        // In a real app, you'd use column!, button(), etc.

        // Example of how to use set_state in an event handler:
        //
        // button("+").on_press({
        //     let ctx = ctx.clone();  // Clone is cheap (Arc internally)
        //     move |_| {
        //         ctx.set_state(|state: &mut CounterState| {
        //             state.count += 1;  // Direct access to state!
        //         });
        //     }
        // })

        // For now, return a placeholder widget
        // In a real implementation, this would be a proper widget tree
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
    println!("Counter with set_state() API");
    println!("=====================================");
    println!();
    println!("This example demonstrates the new setState API:");
    println!();
    println!("```rust");
    println!("button(\"+\").on_press({{");
    println!("    let ctx = ctx.clone();");
    println!("    move |_| {{");
    println!("        ctx.set_state(|state: &mut CounterState| {{");
    println!("            state.count += 1;  // Direct access!");
    println!("        }});");
    println!("    }}");
    println!("}})");
    println!("```");
    println!();
    println!("Benefits:");
    println!("  ✓ No manual signal cloning");
    println!("  ✓ Direct access to state fields");
    println!("  ✓ Automatic rebuild on change");
    println!("  ✓ Familiar Flutter-style API");
}
