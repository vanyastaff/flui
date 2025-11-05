# Hooks Guide

## Introduction

Hooks provide state management for FLUI views, inspired by React Hooks. They enable reactive state that automatically triggers rebuilds when values change.

## Core Concept

Hooks allow you to use state in functional-style views without explicit classes or setState calls:

```rust
fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
    // Create reactive state
    let count = use_signal(ctx, 0);

    // Derived state
    let doubled = use_memo(ctx, |_| count.get() * 2);

    // Side effects
    use_effect_simple(ctx, || {
        println!("Count: {}", count.get());
    });

    // Build UI that reacts to state changes
    //...
}
```

## The Rules of Hooks

**These rules are CRITICAL and must be followed:**

### Rule 1: Only Call Hooks at the Top Level

**✅ DO**: Call hooks at component top level
```rust
fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
    let state1 = use_signal(ctx, 0);
    let state2 = use_signal(ctx, "");
    // Build UI...
}
```

**❌ DON'T**: Call hooks inside loops, conditionals, or nested functions
```rust
fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
    if self.condition {
        let state = use_signal(ctx, 0);  // ❌ WRONG!
    }

    for i in 0..10 {
        let state = use_signal(ctx, i);  // ❌ WRONG!
    }
}
```

### Rule 2: Always Call Hooks in the Same Order

Hooks rely on call order to maintain state between rebuilds.

**✅ DO**: Call same hooks every build
```rust
fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
    let name = use_signal(ctx, String::new());  // Always first
    let age = use_signal(ctx, 0);                // Always second
    // ...
}
```

**❌ DON'T**: Conditionally skip hooks
```rust
fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
    let name = use_signal(ctx, String::new());

    // ❌ WRONG - hook order changes based on condition!
    if self.show_age {
        let age = use_signal(ctx, 0);
    }
}
```

### Rule 3: Make Values Conditional, Not Hook Calls

**✅ DO**: Call hook always, make VALUE conditional
```rust
fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
    let is_visible = use_signal(ctx, true);  // Always called

    // Make the VALUE conditional
    let content = if is_visible.get() {
        "Visible content"
    } else {
        "Hidden"
    };

    Text::new(content)
}
```

**❌ DON'T**: Conditionally call hooks
```rust
fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
    // ❌ WRONG!
    if self.show_content {
        let content = use_signal(ctx, String::new());
    }
}
```

## Available Hooks

### use_signal - Reactive State

Creates a reactive value that triggers rebuilds on change.

**Signature:**
```rust
pub fn use_signal<T: Clone + 'static>(
    ctx: &BuildContext,
    initial: T
) -> Signal<T>
```

**Usage:**
```rust
let count = use_signal(ctx, 0);

// Get value
let value = count.get();

// Set value
count.set(42);

// Update with function
count.update(|n| n + 1);

// Update with fallible function
count.try_update(|n| {
    if *n < 100 {
        *n += 1;
        Ok(())
    } else {
        Err("Max reached")
    }
});
```

**Signal Methods:**

```rust
impl<T: Clone> Signal<T> {
    /// Get current value
    pub fn get(&self) -> T;

    /// Set new value
    pub fn set(&self, value: T);

    /// Update value with function
    pub fn update(&self, f: impl FnOnce(&mut T));

    /// Try to update value
    pub fn try_update(&self, f: impl FnOnce(&mut T) -> Result<(), E>)
        -> Result<(), E>;

    /// Clone the signal (cheap - just Rc increment)
    pub fn clone(&self) -> Self;
}
```

**Example:**
```rust
#[derive(Clone)]
struct Counter;

impl View for Counter {
    type Element = Element;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        let count = use_signal(ctx, 0);

        // Clone for closures
        let count_inc = count.clone();
        let count_dec = count.clone();

        let element = Column::new()
            .child(Text::new(format!("Count: {}", count.get())))
            .child(Button::new("Increment", move || {
                count_inc.update(|n| *n += 1);
            }))
            .child(Button::new("Decrement", move || {
                count_dec.update(|n| *n -= 1);
            }))
            .into_element();

        (element, ())
    }
}
```

### use_memo - Derived State

Creates a value that's only recomputed when dependencies change.

**Signature:**
```rust
pub fn use_memo<T: Clone + 'static, F>(
    ctx: &BuildContext,
    compute: F
) -> Memo<T>
where
    F: Fn(&HookContext) -> T + 'static
```

**Usage:**
```rust
let count = use_signal(ctx, 10);

// Computed value - only recalculated when count changes
let doubled = use_memo(ctx, |_hook_ctx| {
    println!("Computing...");  // Only prints when count changes
    count.get() * 2
});

// Get memoized value
let value = doubled.get();
```

**Why Use Memo?**

- Avoid expensive recomputation
- Ensure referential equality for derived data
- Performance optimization for complex calculations

**Example:**
```rust
#[derive(Clone)]
struct ExpensiveView;

impl View for ExpensiveView {
    type Element = Element;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        let numbers = use_signal(ctx, vec![1, 2, 3, 4, 5]);

        // Expensive computation - only runs when numbers change
        let stats = use_memo(ctx, |_| {
            let nums = numbers.get();
            let sum: i32 = nums.iter().sum();
            let avg = sum as f32 / nums.len() as f32;
            format!("Sum: {}, Avg: {:.2}", sum, avg)
        });

        let element = Column::new()
            .child(Text::new(stats.get()))
            .into_element();

        (element, ())
    }
}
```

### use_effect - Side Effects

Runs side effects in response to changes.

**Signature:**
```rust
pub fn use_effect<F, C>(
    ctx: &BuildContext,
    effect: F,
    cleanup: C,
    deps: Vec<Box<dyn Any>>
)
where
    F: FnOnce() + 'static,
    C: FnOnce() + 'static
```

**Simplified version:**
```rust
pub fn use_effect_simple<F>(ctx: &BuildContext, effect: F)
where
    F: Fn() + 'static
```

**Usage:**
```rust
let count = use_signal(ctx, 0);

// Runs whenever count changes
use_effect_simple(ctx, move || {
    println!("Count changed to: {}", count.get());
});
```

**Common Use Cases:**

1. **Logging**
```rust
use_effect_simple(ctx, || {
    println!("Component rendered at: {:?}", SystemTime::now());
});
```

2. **API Calls**
```rust
let user_id = use_signal(ctx, 123);

use_effect_simple(ctx, move || {
    let id = user_id.get();
    fetch_user_data(id);
});
```

3. **Subscriptions**
```rust
use_effect(
    ctx,
    || {
        // Subscribe
        let subscription = subscribe_to_events();
        subscription
    },
    |subscription| {
        // Cleanup
        subscription.unsubscribe();
    },
    vec![]
);
```

### use_resource - Async Data

Load async data with automatic loading states.

**Signature:**
```rust
pub fn use_resource<T, F, Fut>(
    ctx: &BuildContext,
    fetcher: F
) -> Resource<T>
where
    T: Clone + 'static,
    F: Fn() -> Fut + 'static,
    Fut: Future<Output = T> + 'static
```

**Usage:**
```rust
let user_id = use_signal(ctx, 123);

let user_data = use_resource(ctx, move || async move {
    fetch_user(user_id.get()).await
});

match user_data.get() {
    ResourceState::Loading => {
        // Show loading spinner
    }
    ResourceState::Ready(data) => {
        // Show data
    }
    ResourceState::Error(err) => {
        // Show error
    }
}
```

**Example:**
```rust
#[derive(Clone)]
struct UserProfile {
    user_id: u64,
}

impl View for UserProfile {
    type Element = Element;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        let user_id = self.user_id;

        let user = use_resource(ctx, move || async move {
            // Fetch user data
            api::fetch_user(user_id).await
        });

        let content = match user.get() {
            ResourceState::Loading => {
                Text::new("Loading...").into_element()
            }
            ResourceState::Ready(data) => {
                Column::new()
                    .child(Text::new(&data.name))
                    .child(Text::new(&data.email))
                    .into_element()
            }
            ResourceState::Error(err) => {
                Text::new(format!("Error: {}", err)).into_element()
            }
        };

        (content, ())
    }
}
```

## Cloning Signals

Signals are designed to be cloned cheaply - they use `Rc` internally:

```rust
let count = use_signal(ctx, 0);

// ✅ Clone for closures - this is cheap!
let count1 = count.clone();
let count2 = count.clone();
let count3 = count.clone();

Button::new("A", move || count1.update(|n| *n += 1));
Button::new("B", move || count2.update(|n| *n += 1));
Button::new("C", move || count3.update(|n| *n += 1));

// All three buttons increment the SAME counter
```

**Why cloning is cheap:**
- Signal contains `Rc<RefCell<T>>`
- Cloning just increments reference count
- All clones share the same underlying value

## Pattern: Multiple Related Signals

```rust
#[derive(Clone)]
struct LoginForm;

impl View for LoginForm {
    type Element = Element;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Multiple signals for form fields
        let email = use_signal(ctx, String::new());
        let password = use_signal(ctx, String::new());
        let remember_me = use_signal(ctx, false);

        // Derived validation state
        let is_valid = use_memo(ctx, |_| {
            let email_val = email.get();
            let pass_val = password.get();
            !email_val.is_empty() &&
            email_val.contains('@') &&
            pass_val.len() >= 8
        });

        // Submit handler
        let email_clone = email.clone();
        let password_clone = password.clone();
        let remember_clone = remember_me.clone();

        let on_submit = move || {
            if is_valid.get() {
                login(
                    &email_clone.get(),
                    &password_clone.get(),
                    remember_clone.get()
                );
            }
        };

        // Build form
        let element = Column::new()
            .child(TextField::new("Email", email))
            .child(TextField::new("Password", password).password())
            .child(Checkbox::new("Remember me", remember_me))
            .child(Button::new("Login", on_submit)
                .enabled(is_valid.get()))
            .into_element();

        (element, ())
    }
}
```

## Pattern: Toggling State

```rust
let is_visible = use_signal(ctx, false);

// Clone for button
let is_visible_toggle = is_visible.clone();

Button::new("Toggle", move || {
    is_visible_toggle.update(|v| *v = !*v);
})
```

## Pattern: List Management

```rust
#[derive(Clone)]
struct TodoApp;

impl View for TodoApp {
    type Element = Element;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        let todos = use_signal(ctx, Vec::<String>::new());
        let input = use_signal(ctx, String::new());

        // Add todo
        let todos_add = todos.clone();
        let input_add = input.clone();
        let on_add = move || {
            let text = input_add.get();
            if !text.is_empty() {
                todos_add.update(|list| list.push(text.clone()));
                input_add.set(String::new());
            }
        };

        // Remove todo
        let todos_remove = todos.clone();
        let make_remove_handler = move |index: usize| {
            let todos_remove = todos_remove.clone();
            move || {
                todos_remove.update(|list| {
                    list.remove(index);
                });
            }
        };

        // Build list
        let todo_items: Vec<_> = todos.get()
            .iter()
            .enumerate()
            .map(|(i, text)| {
                Row::new()
                    .child(Text::new(text))
                    .child(Button::new("Remove", make_remove_handler(i)))
            })
            .collect();

        let element = Column::new()
            .child(TextField::new("New todo", input))
            .child(Button::new("Add", on_add))
            .children(todo_items)
            .into_element();

        (element, ())
    }
}
```

## Pattern: Computed Validation

```rust
let password = use_signal(ctx, String::new());
let confirm = use_signal(ctx, String::new());

// Computed validation
let passwords_match = use_memo(ctx, |_| {
    password.get() == confirm.get()
});

let is_strong = use_memo(ctx, |_| {
    let pass = password.get();
    pass.len() >= 8 &&
    pass.chars().any(|c| c.is_numeric()) &&
    pass.chars().any(|c| c.is_uppercase())
});

let can_submit = use_memo(ctx, |_| {
    passwords_match.get() && is_strong.get()
});
```

## Pattern: Effect with Cleanup

```rust
let is_connected = use_signal(ctx, false);

use_effect(
    ctx,
    move || {
        // Setup: connect to server
        let connection = connect_to_server();
        is_connected.set(true);

        connection
    },
    move |connection| {
        // Cleanup: disconnect
        connection.disconnect();
        is_connected.set(false);
    },
    vec![]  // Empty deps = run once on mount
);
```

## Common Mistakes

### Mistake 1: Not Cloning Signals

**❌ Wrong:**
```rust
let count = use_signal(ctx, 0);

Button::new("Click", move || {
    count.update(|n| *n += 1);  // ERROR: count moved into closure
})

// count is no longer available here!
```

**✅ Correct:**
```rust
let count = use_signal(ctx, 0);
let count_clone = count.clone();  // Clone first!

Button::new("Click", move || {
    count_clone.update(|n| *n += 1);
})

// count still available here
Text::new(format!("Count: {}", count.get()))
```

### Mistake 2: Conditional Hooks

**❌ Wrong:**
```rust
fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
    if self.show_advanced {
        let advanced_state = use_signal(ctx, 0);  // ❌ Hook order changes!
    }
}
```

**✅ Correct:**
```rust
fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
    // Always call hook
    let advanced_state = use_signal(ctx, 0);

    // Make rendering conditional
    if self.show_advanced {
        // Use advanced_state here
    }
}
```

### Mistake 3: Forgetting Memo Dependencies

When using closures in memo, they capture values:

**❌ May not update:**
```rust
let multiplier = use_signal(ctx, 2);
let count = use_signal(ctx, 10);

let result = use_memo(ctx, |_| {
    count.get() * multiplier.get()  // Will update when either changes
});
```

This actually works because signals are accessed inside memo. But be careful with captured values outside:

**❌ Won't update:**
```rust
let multiplier_value = multiplier.get();  // Captured once
let result = use_memo(ctx, |_| {
    count.get() * multiplier_value  // Won't update when multiplier changes!
});
```

**✅ Correct:**
```rust
let result = use_memo(ctx, |_| {
    count.get() * multiplier.get()  // Both accessed inside memo
});
```

## Performance Tips

### 1. Use Memo for Expensive Computations

```rust
// ❌ Recomputes every render
let result = expensive_calculation(data.get());

// ✅ Only recomputes when data changes
let result = use_memo(ctx, |_| {
    expensive_calculation(data.get())
});
```

### 2. Clone Signals, Not Values

```rust
// ❌ Clones value every time
let value = count.get();
Button::new("Click", move || {
    // Can't update count here!
});

// ✅ Clones signal handle (cheap)
let count_clone = count.clone();
Button::new("Click", move || {
    count_clone.update(|n| *n += 1);
});
```

### 3. Batch Signal Updates

```rust
// ❌ Multiple rebuilds
count.set(1);
name.set("Alice");
age.set(30);

// ✅ Better: update together if possible
update_user_data(|user| {
    user.count = 1;
    user.name = "Alice".to_string();
    user.age = 30;
});
```

## Advanced: Custom Hooks

You can create custom hooks by combining built-in hooks:

```rust
// Custom hook for debounced value
pub fn use_debounced<T: Clone + 'static>(
    ctx: &BuildContext,
    value: Signal<T>,
    delay_ms: u64
) -> Signal<T> {
    let debounced = use_signal(ctx, value.get());

    use_effect_simple(ctx, move || {
        let current = value.get();
        let debounced_clone = debounced.clone();

        // Spawn delay task
        spawn_delayed(delay_ms, move || {
            debounced_clone.set(current);
        });
    });

    debounced
}

// Usage
let search = use_signal(ctx, String::new());
let debounced_search = use_debounced(ctx, search.clone(), 300);

// debounced_search only updates 300ms after typing stops
```

## See Also

- [VIEW_GUIDE.md](./VIEW_GUIDE.md) - How to use the View trait
- [ARCHITECTURE.md](./ARCHITECTURE.md) - Overall architecture
- [examples/](../examples/) - Runnable examples
