//! Framework Demo - demonstrates StatelessWidget, StatefulWidget, and Element system
//!
//! This example shows the Flutter-like three-tree architecture:
//! - Widget tree (immutable configuration)
//! - Element tree (mutable state holder)
//! - Render tree (layout and paint)

use nebula_ui::widgets::{
    StatelessWidgetTrait as StatelessWidget,
    StatefulWidgetTrait as StatefulWidget,
    State,
    BuildContext,
    ComponentElement,
    StatefulElement,
    ElementTree,
    ElementId,
    Element, // Trait needed to call Element methods
};

// ============================================================================
// Example 1: StatelessWidget
// ============================================================================

/// MyGreeting - simple stateless widget
#[derive(Debug, Clone)]
struct MyGreeting {
    name: String,
}

impl MyGreeting {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
        }
    }
}

impl StatelessWidget for MyGreeting {
    fn build(&self, _context: &BuildContext) -> Box<dyn std::any::Any> {
        // In real implementation, this would return a Text widget
        // For now, just return the name as Any
        Box::new(format!("Hello, {}!", self.name))
    }
}

// ============================================================================
// Example 2: StatefulWidget (simplified)
// ============================================================================

/// Counter - stateful widget with increment functionality
#[derive(Debug, Clone)]
struct Counter {
    initial_count: i32,
}

impl Counter {
    fn new(initial_count: i32) -> Self {
        Self { initial_count }
    }
}

impl StatefulWidget for Counter {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        CounterState {
            count: self.initial_count,
            context: None,
        }
    }
}

/// CounterState - mutable state for Counter widget
#[derive(Debug)]
struct CounterState {
    count: i32,
    context: Option<BuildContext>,
}

impl State for CounterState {
    fn build(&mut self, _context: &BuildContext) -> Box<dyn std::any::Any> {
        // In real implementation, this would build a Column with Text and Button
        // For now, just return the count
        Box::new(format!("Count: {}", self.count))
    }

    fn init_state(&mut self) {
        println!("Counter state initialized with count: {}", self.count);
    }

    fn dispose(&mut self) {
        println!("Counter state disposed");
    }

    fn get_context(&self) -> Option<BuildContext> {
        self.context.clone()
    }

    fn set_context(&mut self, context: BuildContext) {
        self.context = Some(context);
    }
}

impl CounterState {
    /// Increment the counter (like setState in Flutter)
    pub fn increment(&mut self) {
        self.count += 1;
        self.mark_needs_build();
        println!("Counter incremented to: {}", self.count);
    }
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    println!("🚀 Framework Demo - Element System\n");

    // Example 1: ComponentElement for StatelessWidget
    println!("📦 Example 1: StatelessWidget");
    println!("─────────────────────────────");

    let greeting = MyGreeting::new("World");
    println!("Created widget: {:?}", greeting);

    let mut element = ComponentElement::new(Box::new(greeting));
    println!("Created element with ID: {:?}", element.id());

    element.mount(None, 0);
    println!("Element mounted");

    println!("Element is dirty: {}", element.is_dirty());

    element.rebuild();
    println!("Element rebuilt");

    println!();

    // Example 2: StatefulElement for StatefulWidget
    println!("📊 Example 2: StatefulWidget");
    println!("─────────────────────────────");

    let counter = Counter::new(0);
    println!("Created widget: {:?}", counter);

    let mut stateful_element = StatefulElement::new(Box::new(counter));
    println!("Created stateful element with ID: {:?}", stateful_element.id());

    stateful_element.mount(None, 0);
    println!("Stateful element mounted");

    // In real implementation, we would call increment on the state through UI interaction
    println!("State lifecycle demonstrated");

    stateful_element.rebuild();
    println!("Stateful element rebuilt");

    stateful_element.unmount();
    println!("Stateful element unmounted");

    println!();

    // Example 3: Element Tree
    println!("🌳 Example 3: Element Tree");
    println!("─────────────────────────────");

    let mut tree = ElementTree::new();
    println!("Created element tree");

    let id = ElementId::new();
    println!("Generated element ID: {:?}", id);

    tree.mark_dirty(id);
    println!("Marked element as dirty");

    println!("Tree has dirty elements: {}", tree.has_dirty_elements());

    tree.rebuild_dirty();
    println!("Rebuilt dirty elements");

    println!("Tree has dirty elements after rebuild: {}", tree.has_dirty_elements());

    println!();

    println!("✅ Framework demo completed successfully!");
    println!();
    println!("Key Concepts Demonstrated:");
    println!("  • StatelessWidget - immutable widget that builds once");
    println!("  • StatefulWidget - widget with mutable state that persists");
    println!("  • State lifecycle - init_state, build, dispose");
    println!("  • Element - mutable state holder in element tree");
    println!("  • ElementTree - manages dirty tracking and rebuilds");
    println!();
    println!("This is the foundation for Flutter-like declarative UI in Rust!");
}
