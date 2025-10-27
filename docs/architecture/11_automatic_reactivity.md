# Chapter 11: Automatic Reactivity

## 📋 Overview

FLUI's reactive system provides **automatic, transparent reactivity** without explicit wrappers. The goal: write natural code and have it "just work" - signals track dependencies automatically, updates propagate efficiently, and rebuilds are fine-grained.

## 🎯 Design Goals

### What We Want

```rust
// ✅ Natural code - no explicit reactive_*() wrappers
fn build(&mut self) -> Widget {
    column![
        text(format!("Count: {}", self.count.get())),  // ← Automatically reactive!
        button("+").on_press_increment(&self.count),   // ← Clean API
    ]
}
```

### What We Avoid

```rust
// ❌ Verbose wrappers (like some frameworks)
reactive_text(|| format!("Count: {}", count.get()))

// ❌ Manual subscription management
let subscription = count.subscribe(|value| { /* ... */ });
// Don't forget to unsubscribe!

// ❌ setState-style manual updates
setState(() {
    count++;
});  // Re-renders everything
```

---

## 🔧 How It Works: Reactive Scopes

### Thread-Local Scope Stack

```rust
use std::cell::RefCell;
use std::collections::HashSet;

thread_local! {
    /// Global reactive scope stack
    static SCOPE_STACK: RefCell<Vec<ReactiveScope>> = RefCell::new(vec![]);
}

/// Reactive scope that tracks signal accesses
#[derive(Clone)]
pub struct ReactiveScope {
    id: ScopeId,
    dependencies: HashSet<SignalId>,
}

type ScopeId = usize;
type SignalId = usize;
```

### Automatic Dependency Tracking

```rust
/// Run code in a reactive scope
pub fn create_scope<R>(f: impl FnOnce() -> R) -> (R, ReactiveScope) {
    let scope_id = RUNTIME.with(|rt| rt.borrow_mut().create_scope());
    let scope = ReactiveScope::new(scope_id);

    // Push scope onto stack
    SCOPE_STACK.with(|stack| {
        stack.borrow_mut().push(scope.clone());
    });

    // Run user code (will track dependencies automatically)
    let result = f();

    // Pop scope
    let final_scope = SCOPE_STACK.with(|stack| {
        stack.borrow_mut().pop().unwrap()
    });

    (result, final_scope)
}

/// Get current reactive scope (if any)
pub fn current_scope() -> Option<ReactiveScope> {
    SCOPE_STACK.with(|stack| {
        stack.borrow().last().cloned()
    })
}
```

---

## 📡 Signal Implementation

### Signal with Automatic Tracking

```rust
use std::rc::Rc;
use std::cell::RefCell;

/// Signal - cheap to clone (Rc inside)
#[derive(Clone)]
pub struct Signal<T> {
    inner: Rc<SignalInner<T>>,
}

struct SignalInner<T> {
    id: SignalId,
    value: RefCell<T>,
    subscribers: RefCell<HashSet<ScopeId>>,
}

impl<T: Clone> Signal<T> {
    pub fn new(initial: T) -> Self {
        let id = RUNTIME.with(|rt| rt.borrow_mut().create_signal());

        Self {
            inner: Rc::new(SignalInner {
                id,
                value: RefCell::new(initial),
                subscribers: RefCell::new(HashSet::new()),
            }),
        }
    }

    /// Get value - AUTOMATICALLY tracks dependency!
    pub fn get(&self) -> T {
        // Track in current scope (if any)
        if let Some(scope) = current_scope() {
            // Register this scope as dependent on this signal
            self.inner.subscribers.borrow_mut().insert(scope.id);

            // Register this signal as dependency of the scope
            RUNTIME.with(|rt| {
                rt.borrow_mut().track_dependency(scope.id, self.inner.id);
            });
        }

        self.inner.value.borrow().clone()
    }

    /// Update value - notifies all subscribers
    pub fn set(&self, new_value: T) {
        *self.inner.value.borrow_mut() = new_value;
        self.notify_subscribers();
    }

    pub fn update(&self, f: impl FnOnce(&mut T)) {
        f(&mut self.inner.value.borrow_mut());
        self.notify_subscribers();
    }

    fn notify_subscribers(&self) {
        // Notify all scopes that depend on this signal
        for &scope_id in self.inner.subscribers.borrow().iter() {
            RUNTIME.with(|rt| {
                rt.borrow_mut().mark_scope_dirty(scope_id);
            });
        }
    }
}
```

### Key Insight: `.get()` does TWO things

1. **Returns the value** (obvious)
2. **Registers dependency** (automatic!)

---

## 🏗️ Integration with Element System

### StatefulElement with Reactive Scope

```rust
pub struct StatefulElement<W: StatefulWidget> {
    widget: W,
    state: W::State,
    reactive_scope: Option<ScopeId>,  // ← Track scope ID
    // ... other fields
}

impl<W: StatefulWidget> StatefulElement<W> {
    /// Build widget in reactive scope
    pub fn build_with_tracking(&mut self) -> BoxedWidget {
        // Create reactive scope for this build
        let (widget, scope) = create_scope(|| {
            // Call user's build() - any Signal.get() calls
            // will automatically register as dependencies
            self.state.build()
        });

        // Store scope ID for this element
        self.reactive_scope = Some(scope.id);

        // Register rebuild callback
        let element_id = self.id;
        RUNTIME.with(|rt| {
            rt.borrow_mut().register_rebuild(scope.id, Box::new(move || {
                // Mark element dirty when any dependency changes
                mark_needs_rebuild(element_id);
            }));
        });

        widget
    }
}
```

### Reactive Runtime

```rust
thread_local! {
    static RUNTIME: RefCell<ReactiveRuntime> = RefCell::new(ReactiveRuntime::new());
}

struct ReactiveRuntime {
    next_signal_id: SignalId,
    next_scope_id: ScopeId,
    dirty_scopes: HashSet<ScopeId>,
    scope_dependencies: HashMap<ScopeId, HashSet<SignalId>>,
    scope_rebuild_callbacks: HashMap<ScopeId, Box<dyn Fn()>>,
}

impl ReactiveRuntime {
    fn new() -> Self {
        Self {
            next_signal_id: 0,
            next_scope_id: 0,
            dirty_scopes: HashSet::new(),
            scope_dependencies: HashMap::new(),
            scope_rebuild_callbacks: HashMap::new(),
        }
    }

    fn create_signal(&mut self) -> SignalId {
        let id = self.next_signal_id;
        self.next_signal_id += 1;
        id
    }

    fn create_scope(&mut self) -> ScopeId {
        let id = self.next_scope_id;
        self.next_scope_id += 1;
        id
    }

    fn track_dependency(&mut self, scope_id: ScopeId, signal_id: SignalId) {
        self.scope_dependencies
            .entry(scope_id)
            .or_insert_with(HashSet::new)
            .insert(signal_id);
    }

    fn mark_scope_dirty(&mut self, scope_id: ScopeId) {
        self.dirty_scopes.insert(scope_id);

        // Trigger rebuild callback (if registered)
        if let Some(callback) = self.scope_rebuild_callbacks.get(&scope_id) {
            callback();
        }
    }

    fn register_rebuild(&mut self, scope_id: ScopeId, callback: Box<dyn Fn()>) {
        self.scope_rebuild_callbacks.insert(scope_id, callback);
    }

    fn flush_updates(&mut self) {
        // Execute all pending rebuilds
        let dirty = std::mem::take(&mut self.dirty_scopes);
        for scope_id in dirty {
            if let Some(callback) = self.scope_rebuild_callbacks.get(&scope_id) {
                callback();
            }
        }
    }
}
```

---

## 💡 Signal Ergonomics: No Manual Clone

### Problem: Closures Need Clone

```rust
// ❌ Annoying: need to clone for closures
button("+").on_press({
    let count = self.count.clone();  // ← Manual clone
    move |_| count.update(|c| *c += 1)
})
```

### Solution 1: Signal is Cheap to Clone (Rc inside)

```rust
// Signal.clone() just increments Rc - very cheap!
impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),  // ← Just Rc::clone (cheap!)
        }
    }
}

// So manual clone is not expensive:
let count = self.count.clone();  // Just Rc increment
```

### Solution 2: Helper Methods on Signal

```rust
impl<T> Signal<T> {
    /// Increment (for numeric types)
    pub fn increment(&self)
    where
        T: std::ops::AddAssign + From<i32>,
    {
        self.update(|v| *v += T::from(1));
    }

    pub fn decrement(&self)
    where
        T: std::ops::SubAssign + From<i32>,
    {
        self.update(|v| *v -= T::from(1));
    }

    pub fn toggle(&self)
    where
        T: std::ops::Not<Output = T> + Copy,
    {
        self.update(|v| *v = !*v);
    }
}

// Usage:
button("+").on_press({
    let count = self.count.clone();
    move |_| count.increment()  // ← Clean!
})
```

### Solution 3: Widget Extension Traits (Best!)

```rust
/// Extension trait for button to work with signals directly
pub trait ButtonSignalExt {
    fn on_press_signal_inc<T>(self, signal: &Signal<T>) -> Self
    where
        T: std::ops::AddAssign + From<i32> + 'static;

    fn on_press_signal_update<T>(
        self,
        signal: &Signal<T>,
        f: impl Fn(&mut T) + 'static,
    ) -> Self
    where
        T: 'static;

    fn on_press_signal_set<T>(
        self,
        signal: &Signal<T>,
        value: T,
    ) -> Self
    where
        T: Clone + 'static;
}

impl ButtonSignalExt for Button {
    fn on_press_signal_inc<T>(self, signal: &Signal<T>) -> Self
    where
        T: std::ops::AddAssign + From<i32> + 'static,
    {
        let signal = signal.clone();
        self.on_press(move |_| signal.increment())
    }

    fn on_press_signal_update<T>(
        self,
        signal: &Signal<T>,
        f: impl Fn(&mut T) + 'static,
    ) -> Self
    where
        T: 'static,
    {
        let signal = signal.clone();
        self.on_press(move |_| signal.update(&f))
    }

    fn on_press_signal_set<T>(
        self,
        signal: &Signal<T>,
        value: T,
    ) -> Self
    where
        T: Clone + 'static,
    {
        let signal = signal.clone();
        self.on_press(move |_| signal.set(value.clone()))
    }
}

// Usage - NO manual clone!
button("+").on_press_signal_inc(&self.count)  // ✅ Clean!
button("Reset").on_press_signal_set(&self.count, 0)  // ✅ Clean!
```

### Solution 4: clone! Macro for Complex Cases

```rust
/// Clone variables into closure automatically
#[macro_export]
macro_rules! clone {
    ($($var:ident),+ => $closure:expr) => {
        {
            $(let $var = $var.clone();)+
            $closure
        }
    };
}

// Usage:
button("Add Step").on_press(clone!(self.count, self.step => move |_| {
    self.count.update(|c| *c += self.step.get())
}))
```

---

## 📚 Complete Example

### Counter with Automatic Reactivity

```rust
use flui_core::prelude::*;

#[derive(Debug)]
pub struct Counter {
    initial: i32,
}

#[derive(Debug)]
pub struct CounterState {
    count: Signal<i32>,
    step: Signal<i32>,
}

impl StatefulWidget for Counter {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        CounterState {
            count: Signal::new(self.initial),
            step: Signal::new(1),
        }
    }
}

impl State for CounterState {
    type Widget = Counter;

    fn build(&mut self) -> BoxedWidget {
        // ✅ NO reactive_*() wrappers needed!
        // ✅ All Signal.get() calls automatically tracked!

        Box::new(
            column![
                // Automatically reactive - rebuilds when count changes
                text(format!("Count: {}", self.count.get())),

                // Automatically reactive - rebuilds when step changes
                text(format!("Step: {}", self.step.get())),

                row![
                    // Clean API - no manual clone in sight!
                    button("−")
                        .on_press_signal_update(&self.count, |c| *c -= 1),

                    button("+")
                        .on_press_signal_inc(&self.count),

                    button("×2")
                        .on_press_signal_update(&self.count, |c| *c *= 2),

                    button("Reset")
                        .on_press_signal_set(&self.count, 0),
                ],

                // Complex update with multiple signals
                button("Add Step").on_press(clone!(self.count, self.step => move |_| {
                    self.count.update(|c| *c += self.step.get())
                })),

                // Conditional rendering - automatically reactive
                if self.count.get() > 10 {
                    container()
                        .color(Color::GREEN)
                        .child(text("High count!"))
                } else {
                    container()
                        .color(Color::GRAY)
                        .child(text("Low count"))
                },
            ]
        )
    }
}

// Usage:
fn main() {
    let counter = Counter { initial: 0 };
    run_app(counter);
}
```

### Todo App Example

```rust
#[derive(Debug, Clone)]
pub struct Todo {
    id: usize,
    text: String,
    completed: bool,
}

#[derive(Debug)]
pub struct TodoApp;

#[derive(Debug)]
pub struct TodoAppState {
    todos: Signal<Vec<Todo>>,
    filter: Signal<TodoFilter>,
    next_id: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TodoFilter {
    All,
    Active,
    Completed,
}

impl TodoFilter {
    fn matches(&self, todo: &Todo) -> bool {
        match self {
            TodoFilter::All => true,
            TodoFilter::Active => !todo.completed,
            TodoFilter::Completed => todo.completed,
        }
    }
}

impl State for TodoAppState {
    type Widget = TodoApp;

    fn build(&mut self) -> BoxedWidget {
        // ✅ Automatically reactive - rebuilds only affected parts!

        Box::new(
            column![
                // Input field
                text_field()
                    .placeholder("What needs to be done?")
                    .on_submit({
                        let todos = self.todos.clone();
                        let mut next_id = self.next_id;
                        move |text| {
                            todos.update(|t| {
                                t.push(Todo {
                                    id: next_id,
                                    text,
                                    completed: false,
                                });
                            });
                            next_id += 1;
                        }
                    }),

                // Todo list - automatically rebuilds when todos or filter changes
                column(
                    self.todos.get()
                        .into_iter()
                        .filter(|t| self.filter.get().matches(t))
                        .map(|todo| todo_item(todo, &self.todos))
                        .collect()
                ),

                // Filter buttons
                row![
                    filter_button("All", TodoFilter::All, &self.filter),
                    filter_button("Active", TodoFilter::Active, &self.filter),
                    filter_button("Completed", TodoFilter::Completed, &self.filter),
                ],

                // Stats - automatically reactive
                text(format!(
                    "{} items left",
                    self.todos.get().iter().filter(|t| !t.completed).count()
                )),
            ]
        )
    }
}

fn filter_button(
    label: &str,
    filter: TodoFilter,
    current_filter: &Signal<TodoFilter>,
) -> impl Widget {
    let is_active = current_filter.get() == filter;

    button(label)
        .style(if is_active {
            ButtonStyle::Primary
        } else {
            ButtonStyle::Secondary
        })
        .on_press_signal_set(current_filter, filter)
}
```

---

## 🎯 Benefits

### 1. Automatic Dependency Tracking

```rust
// ✅ Just use .get() - tracking is automatic!
text(format!("Count: {}", self.count.get()))
//                        ↑ Automatically registered as dependency

// No manual subscribe/unsubscribe needed
// No manual dependency arrays
// No useEffect() complexity
```

### 2. Fine-Grained Updates

```rust
column![
    expensive_header(),              // ✅ NOT rebuilt
    text(format!("{}", count.get())), // ✅ Rebuilt only when count changes
    expensive_footer(),              // ✅ NOT rebuilt
]

// Only the text widget rebuilds when count changes!
// Not the entire column or app
```

### 3. Clean API

```rust
// ✅ Multiple styles - choose your preference:

// 1. Extension methods (cleanest for simple cases)
button("+").on_press_signal_inc(&self.count)

// 2. Signal helper methods
button("+").on_press({
    let count = self.count.clone();
    move |_| count.increment()
})

// 3. Manual clone + update (most flexible)
button("Add").on_press(clone!(self.count, self.step => move |_| {
    self.count.update(|c| *c += self.step.get())
}))
```

### 4. No Memory Leaks

```rust
// ✅ Automatic cleanup!
// When Signal is dropped, all subscriptions are automatically removed
// When Scope is dropped, all dependencies are cleared
// RAII guarantees no leaks
```

---

## 📊 Performance Characteristics

| Operation | Time Complexity | Notes |
|-----------|----------------|-------|
| Signal.get() | O(1) | + scope tracking overhead |
| Signal.set() | O(n) | n = number of subscribers |
| Signal.clone() | O(1) | Just Rc increment |
| Scope creation | O(1) | Stack push |
| Dependency tracking | O(1) | HashSet insert |
| Rebuild notification | O(n) | n = number of dependent scopes |

### Memory Overhead

```rust
// Signal<i32> size:
// - Rc<SignalInner> = 8 bytes (pointer)
// Total: 8 bytes (same as a pointer!)

// SignalInner<i32> size:
// - id: 8 bytes
// - value: RefCell<i32> = 16 bytes
// - subscribers: RefCell<HashSet> = 32+ bytes
// Total: ~56+ bytes (shared across all clones!)

// Cloning is cheap - just Rc increment
```

---

## 🔗 Cross-References

- **Previous:** [Chapter 10: Future Extensions](10_future_extensions.md)
- **Related:** [Chapter 2: Widget & Element System](02_widget_element_system.md)
- **See Also:** [Chapter 1: Architecture](01_architecture.md)

---

**Key Takeaway:** FLUI's automatic reactivity provides the best of both worlds - the simplicity of "just use .get()" with the performance of fine-grained updates. No manual subscription management, no memory leaks, and clean ergonomic API! 🚀
