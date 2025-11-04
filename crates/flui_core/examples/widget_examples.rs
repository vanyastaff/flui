//! Widget Examples - Comprehensive Guide to Creating Views
//!
//! This file demonstrates how to create different types of widgets using the View API.
//! Each example shows a different pattern and use case.

use flui_core::{
    BuildContext, View, Element, ChangeFlags,
    hooks::{use_signal, use_memo, use_effect_simple},
};

// ============================================================================
// Example 1: Simple Stateless Widget
// ============================================================================

/// A simple text widget that displays a message.
///
/// This is the simplest type of widget - it has no state, no children,
/// and just renders a text element.
///
/// # Usage
///
/// ```rust,ignore
/// let greeting = SimpleText::new("Hello, World!");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct SimpleText {
    text: String,
}

impl SimpleText {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
        }
    }
}

impl View for SimpleText {
    // Element type - typically you'd use a real render element
    type Element = Element;

    // State type - no state needed for simple text
    type State = ();

    fn build(self, _ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // In a real implementation, you'd create a TextRenderElement here
        // For this example, we'll use a placeholder

        // Example:
        // let element = TextRenderElement::new(self.text);
        // (Element::from(element), ())

        todo!("Create actual render element for text: {}", self.text)
    }

    fn rebuild(
        self,
        prev: &Self,
        _state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        // Only rebuild if text actually changed
        if self.text != prev.text {
            element.mark_dirty();
            ChangeFlags::NEEDS_BUILD
        } else {
            ChangeFlags::NONE
        }
    }
}

// ============================================================================
// Example 2: Stateful Widget with Hooks
// ============================================================================

/// A counter widget that maintains its own state using signals.
///
/// This demonstrates:
/// - Using hooks (use_signal)
/// - Managing local state
/// - Responding to user interactions
///
/// # Usage
///
/// ```rust,ignore
/// let counter = Counter::new(0);
/// ```
#[derive(Debug, Clone)]
pub struct Counter {
    initial_value: i32,
}

impl Counter {
    pub fn new(initial_value: i32) -> Self {
        Self { initial_value }
    }
}

impl View for Counter {
    type Element = Element;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Create a signal for the counter value
        let count = use_signal(ctx, self.initial_value);

        // In a real implementation, you'd create a column with:
        // - Text showing the count
        // - Button to increment
        // - Button to decrement

        // Example structure:
        // Column {
        //     children: [
        //         Text(format!("Count: {}", count.get())),
        //         Button("Increment", move || count.update(|n| n + 1)),
        //         Button("Decrement", move || count.update(|n| n - 1)),
        //     ]
        // }

        todo!("Build counter UI with count: {}", count.get())
    }
}

// ============================================================================
// Example 3: Widget with Computed Values (Memo)
// ============================================================================

/// A widget that displays a value and its computed derivative.
///
/// This demonstrates:
/// - Using use_memo for derived state
/// - Avoiding expensive recomputation
/// - Reactive dependencies
///
/// # Usage
///
/// ```rust,ignore
/// let display = ComputedDisplay::new();
/// ```
#[derive(Debug, Clone)]
pub struct ComputedDisplay;

impl ComputedDisplay {
    pub fn new() -> Self {
        Self
    }
}

impl View for ComputedDisplay {
    type Element = Element;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Original value
        let value = use_signal(ctx, 10);

        // Computed value - only recalculated when `value` changes
        let doubled = use_memo(ctx, |_hook_ctx| {
            let val = value.get();
            println!("Computing doubled value..."); // Only prints when recomputed
            val * 2
        });

        let tripled = use_memo(ctx, |_hook_ctx| {
            let val = value.get();
            val * 3
        });

        // Build UI showing value, doubled, and tripled
        // In real code:
        // Column {
        //     children: [
        //         Text(format!("Value: {}", value.get())),
        //         Text(format!("Doubled: {}", doubled.get())),
        //         Text(format!("Tripled: {}", tripled.get())),
        //         Button("Increment", move || value.update(|n| n + 1)),
        //     ]
        // }

        todo!("Build computed display UI")
    }
}

// ============================================================================
// Example 4: Widget with Side Effects
// ============================================================================

/// A widget that logs changes to console.
///
/// This demonstrates:
/// - Using use_effect for side effects
/// - Cleanup on unmount
/// - Responding to state changes
///
/// # Usage
///
/// ```rust,ignore
/// let logger = LoggingWidget::new("My Widget");
/// ```
#[derive(Debug, Clone)]
pub struct LoggingWidget {
    name: String,
}

impl LoggingWidget {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
        }
    }
}

impl View for LoggingWidget {
    type Element = Element;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        let count = use_signal(ctx, 0);

        // Log whenever count changes
        let name = self.name.clone();
        use_effect_simple(ctx, move || {
            println!("[{}] Count changed to: {}", name, count.get());
        });

        // Build UI
        // Button("Increment", move || count.update(|n| n + 1))

        todo!("Build logging widget UI")
    }
}

// ============================================================================
// Example 5: Container Widget with Children
// ============================================================================

/// A container widget that can hold multiple children.
///
/// This demonstrates:
/// - Working with child views
/// - Layout management
/// - Passing data to children
///
/// # Usage
///
/// ```rust,ignore
/// let container = Container::new()
///     .child(SimpleText::new("First"))
///     .child(SimpleText::new("Second"));
/// ```
#[derive(Debug, Clone)]
pub struct Container {
    children: Vec<Box<dyn View<Element = Element, State = ()>>>,
    padding: f32,
}

impl Container {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            padding: 10.0,
        }
    }

    pub fn padding(mut self, padding: f32) -> Self {
        self.padding = padding;
        self
    }

    pub fn child(mut self, child: impl View<Element = Element, State = ()> + 'static) -> Self {
        self.children.push(Box::new(child));
        self
    }
}

impl View for Container {
    type Element = Element;
    type State = ();

    fn build(self, _ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // In real implementation:
        // - Create a Column or Row element
        // - Build all children
        // - Apply padding
        // - Return the container element

        todo!("Build container with {} children", self.children.len())
    }

    fn rebuild(
        self,
        prev: &Self,
        _state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        // Check if padding changed or number of children changed
        if self.padding != prev.padding || self.children.len() != prev.children.len() {
            element.mark_dirty();
            ChangeFlags::NEEDS_BUILD
        } else {
            // Children will handle their own rebuild
            ChangeFlags::NONE
        }
    }
}

// ============================================================================
// Example 6: Conditional Widget
// ============================================================================

/// A widget that conditionally shows content based on a condition.
///
/// This demonstrates:
/// - Conditional rendering
/// - State-driven UI updates
/// - Toggle interactions
///
/// # Usage
///
/// ```rust,ignore
/// let conditional = ConditionalWidget::new();
/// ```
#[derive(Debug, Clone)]
pub struct ConditionalWidget;

impl View for ConditionalWidget {
    type Element = Element;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        let is_visible = use_signal(ctx, true);

        // Conditional rendering - CORRECT approach
        // Always call hooks at the same level, make the VALUE conditional

        let content = if is_visible.get() {
            // Show content
            "Content is visible!"
        } else {
            // Hide content
            "Content is hidden"
        };

        // Build UI:
        // Column {
        //     children: [
        //         Text(content),
        //         Button("Toggle", move || is_visible.update(|v| !v)),
        //     ]
        // }

        todo!("Build conditional widget")
    }
}

// ============================================================================
// Example 7: Form Widget with Multiple Fields
// ============================================================================

/// A form widget with multiple input fields.
///
/// This demonstrates:
/// - Managing multiple signals
/// - Form validation
/// - Computed validation state
///
/// # Usage
///
/// ```rust,ignore
/// let form = FormWidget::new();
/// ```
#[derive(Debug, Clone)]
pub struct FormWidget;

impl View for FormWidget {
    type Element = Element;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Multiple form fields
        let name = use_signal(ctx, String::new());
        let email = use_signal(ctx, String::new());
        let age = use_signal(ctx, 0);

        // Computed validation
        let is_valid = use_memo(ctx, |_hook_ctx| {
            let name_val = name.get();
            let email_val = email.get();
            let age_val = age.get();

            !name_val.is_empty()
                && email_val.contains('@')
                && age_val >= 18
        });

        // Log when form becomes valid
        use_effect_simple(ctx, move || {
            if is_valid.get() {
                println!("Form is now valid!");
            }
        });

        // Build form UI:
        // Column {
        //     children: [
        //         TextField("Name", name),
        //         TextField("Email", email),
        //         NumberField("Age", age),
        //         Button("Submit", enabled: is_valid.get()),
        //     ]
        // }

        todo!("Build form widget")
    }
}

// ============================================================================
// Example 8: List Widget with Dynamic Children
// ============================================================================

/// A list widget that displays dynamic items.
///
/// This demonstrates:
/// - Working with collections
/// - Mapping over data
/// - Keys for list items
///
/// # Usage
///
/// ```rust,ignore
/// let list = ListWidget::new(vec!["Item 1", "Item 2", "Item 3"]);
/// ```
#[derive(Debug, Clone)]
pub struct ListWidget {
    items: Vec<String>,
}

impl ListWidget {
    pub fn new(items: Vec<impl Into<String>>) -> Self {
        Self {
            items: items.into_iter().map(|s| s.into()).collect(),
        }
    }
}

impl View for ListWidget {
    type Element = Element;
    type State = ();

    fn build(self, _ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Map items to child views
        // Important: Use keys for list items!

        // In real code:
        // Column {
        //     children: self.items.iter().enumerate().map(|(i, item)| {
        //         SimpleText::new(item.clone())
        //             .key(Key::from_u64(i as u64))  // Key for efficient updates
        //     })
        // }

        todo!("Build list with {} items", self.items.len())
    }

    fn rebuild(
        self,
        prev: &Self,
        _state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        // Rebuild if items changed
        if self.items != prev.items {
            element.mark_dirty();
            ChangeFlags::NEEDS_BUILD
        } else {
            ChangeFlags::NONE
        }
    }
}

// ============================================================================
// Pattern Summary
// ============================================================================

/*

## Widget Creation Patterns

### 1. Stateless Widget
- No hooks, just props
- Example: SimpleText

### 2. Stateful Widget
- use_signal for local state
- Example: Counter

### 3. Computed State
- use_memo for derived values
- Example: ComputedDisplay

### 4. Side Effects
- use_effect for logging, API calls, etc.
- Example: LoggingWidget

### 5. Container Widget
- Children management
- Example: Container

### 6. Conditional Rendering
- Always call hooks, make VALUES conditional
- Example: ConditionalWidget

### 7. Form Widget
- Multiple signals + validation
- Example: FormWidget

### 8. List Widget
- Map over data, use keys
- Example: ListWidget

## Key Rules

1. **Always call hooks in the same order**
   - ✅ DO: Call all hooks at top level
   - ❌ DON'T: Call hooks conditionally

2. **Optimize rebuild()**
   - Compare props to avoid unnecessary rebuilds
   - Return ChangeFlags::NONE if nothing changed

3. **Use keys for lists**
   - Helps framework track items efficiently
   - Use stable IDs, not indices

4. **Clone signals for closures**
   - Signals are cheap to clone (just Rc increment)
   - Example: `let count = count.clone();`

5. **Use memo for expensive computations**
   - Only recomputes when dependencies change
   - Better than computing every render

*/
