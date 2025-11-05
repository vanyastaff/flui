# View Trait Guide

## Introduction

The `View` trait is the primary abstraction for building UI in FLUI Core. It follows Xilem's approach: immutable view trees that efficiently diff and update a mutable element tree.

## The View Trait

```rust
pub trait View: Clone + 'static {
    type State: 'static;
    type Element: ViewElement;

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State);

    fn rebuild(
        self,
        prev: &Self,
        state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        element.mark_dirty();
        ChangeFlags::NEEDS_BUILD
    }

    fn teardown(&self, state: &mut Self::State, element: &mut Self::Element) {}
}
```

### Type Parameters

**`State`** - Persistent state that survives across rebuilds
- Use `()` if no state needed
- Use tuple or struct for complex state
- State is created in `build()` and passed to `rebuild()`

**`Element`** - The element type this view creates
- Typically `Element` (the enum)
- Could be specific element type for optimization

## Creating Views

### Pattern 1: Simple Stateless View

The simplest view has no state and creates a render element directly.

```rust
use flui_core::{View, Element, BuildContext, ChangeFlags};

#[derive(Debug, Clone, PartialEq)]
pub struct SimpleText {
    pub text: String,
}

impl SimpleText {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

impl View for SimpleText {
    type Element = Element;
    type State = ();

    fn build(self, _ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Create a render element
        let element = create_text_render_element(&self.text);
        (element, ())
    }

    fn rebuild(
        self,
        prev: &Self,
        _state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        // Only rebuild if text changed
        if self.text != prev.text {
            element.mark_dirty();
            ChangeFlags::NEEDS_BUILD
        } else {
            ChangeFlags::NONE  // Skip rebuild!
        }
    }
}
```

**Key points:**
- Implement `PartialEq` for efficient comparison
- Override `rebuild()` to avoid unnecessary work
- Return `ChangeFlags::NONE` when nothing changed

### Pattern 2: View with Hooks

Use hooks for reactive state management:

```rust
use flui_core::hooks::use_signal;

#[derive(Debug, Clone)]
pub struct Counter {
    initial_value: i32,
}

impl View for Counter {
    type Element = Element;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Create signal for counter
        let count = use_signal(ctx, self.initial_value);

        // Clone for closures
        let count_inc = count.clone();
        let count_dec = count.clone();

        // Build UI (pseudo-code)
        let element = Column::new()
            .child(Text::new(format!("Count: {}", count.get())))
            .child(Button::new("Increment", move || {
                count_inc.update(|n| n + 1);
            }))
            .child(Button::new("Decrement", move || {
                count_dec.update(|n| n - 1);
            }))
            .into_element();

        (element, ())
    }
}
```

**Key points:**
- Hooks manage state and rebuild automatically
- Clone signals before moving into closures
- No need to override `rebuild()` - hooks handle it

### Pattern 3: Composition View

Views can compose other views:

```rust
#[derive(Debug, Clone)]
pub struct UserCard {
    name: String,
    email: String,
    avatar_url: Option<String>,
}

impl View for UserCard {
    type Element = Element;
    type State = ();

    fn build(self, _ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Compose multiple views
        let element = Column::new()
            .padding(EdgeInsets::all(16.0))
            .child(
                // Avatar row
                Row::new()
                    .spacing(12.0)
                    .child(Avatar::new(self.avatar_url))
                    .child(Text::new(&self.name).size(18.0).bold())
            )
            .child(
                // Email
                Text::new(&self.email)
                    .size(14.0)
                    .color(Color::GRAY)
            )
            .into_element();

        (element, ())
    }

    fn rebuild(
        self,
        prev: &Self,
        _state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        // Rebuild if any property changed
        if self.name != prev.name ||
           self.email != prev.email ||
           self.avatar_url != prev.avatar_url {
            element.mark_dirty();
            ChangeFlags::NEEDS_BUILD
        } else {
            ChangeFlags::NONE
        }
    }
}
```

### Pattern 4: Container View with Children

Views can accept child views:

```rust
#[derive(Debug, Clone)]
pub struct Card {
    children: Vec<Box<dyn AnyView>>,
    padding: f32,
}

impl Card {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            padding: 16.0,
        }
    }

    pub fn child(mut self, child: impl View<Element = Element, State = ()> + 'static) -> Self {
        self.children.push(Box::new(child));
        self
    }

    pub fn padding(mut self, padding: f32) -> Self {
        self.padding = padding;
        self
    }
}

impl View for Card {
    type Element = Element;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Build all children
        let child_elements: Vec<Element> = self.children
            .into_iter()
            .map(|child| {
                let (element, _) = child.build(ctx);
                element
            })
            .collect();

        // Create container
        let element = Padding::new(EdgeInsets::all(self.padding))
            .child(Column::new().children(child_elements))
            .into_element();

        (element, ())
    }
}
```

### Pattern 5: Conditional Rendering

Always call hooks at the same level, make the VALUES conditional:

```rust
#[derive(Debug, Clone)]
pub struct ConditionalView {
    show_details: bool,
}

impl View for ConditionalView {
    type Element = Element;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // ✅ Correct: Hook called at top level
        let is_expanded = use_signal(ctx, self.show_details);

        // Make the VALUE conditional, not the hook call
        let content = if is_expanded.get() {
            "Detailed information here..."
        } else {
            "Summary"
        };

        let element = Column::new()
            .child(Text::new(content))
            .child(Button::new("Toggle", move || {
                is_expanded.update(|v| !v);
            }))
            .into_element();

        (element, ())
    }
}

// ❌ WRONG: Don't do this!
fn build_wrong(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
    if self.some_condition {
        let state = use_signal(ctx, 0);  // ❌ Conditional hook!
    }
    // ...
}
```

## The build() Method

### Signature

```rust
fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State)
```

### Parameters

**`self`** - Takes ownership of the view
- View is consumed during build
- This enforces immutability

**`ctx`** - Build context
- Read-only access to element tree
- Access to hooks via `ctx.with_hook_context_mut()`
- Tree queries for inherited widgets

### Return Value

Returns a tuple of `(Element, State)`:
- **Element** - The created element
- **State** - Initial state (use `()` if none)

### What to do in build()

1. **Call hooks** (if needed)
2. **Create child views**
3. **Build element from children**
4. **Return element and state**

```rust
fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
    // 1. Call hooks
    let count = use_signal(ctx, 0);

    // 2. Create child views
    let text = Text::new(format!("Count: {}", count.get()));
    let button = Button::new("Increment", move || count.update(|n| n + 1));

    // 3. Build element
    let element = Column::new()
        .child(text)
        .child(button)
        .into_element();

    // 4. Return
    (element, ())
}
```

## The rebuild() Method

### Signature

```rust
fn rebuild(
    self,
    prev: &Self,
    state: &mut Self::State,
    element: &mut Self::Element,
) -> ChangeFlags
```

### Parameters

**`self`** - New view (owns)
- The new view configuration

**`prev`** - Previous view (borrows)
- Compare with `self` to detect changes

**`state`** - Mutable state (borrows)
- Persistent state from previous build
- Can be updated if needed

**`element`** - Mutable element (borrows)
- The existing element to update
- Call `element.mark_dirty()` if changed

### Return Value

Returns `ChangeFlags` indicating what changed:

```rust
pub struct ChangeFlags(u8);

impl ChangeFlags {
    pub const NONE: Self;          // Nothing changed
    pub const NEEDS_BUILD: Self;   // Children need rebuild
    pub const NEEDS_LAYOUT: Self;  // Layout needs recalc
    pub const NEEDS_PAINT: Self;   // Paint needs refresh
}
```

### Default Implementation

**⚠️ The default implementation always marks dirty!**

```rust
fn rebuild(
    self,
    _prev: &Self,
    _state: &mut Self::State,
    element: &mut Self::Element,
) -> ChangeFlags {
    element.mark_dirty();
    ChangeFlags::NEEDS_BUILD  // Always rebuilds!
}
```

### When to Override

**Override when:**
- Your view has expensive rendering
- Your view is frequently rebuilt
- You can cheaply compare props

**Don't override when:**
- Your view is simple and fast
- Comparison is as expensive as rebuilding
- Your view rarely changes

### Optimization Patterns

**Pattern A: Simple equality check**

```rust
impl View for SimpleText {
    fn rebuild(
        self,
        prev: &Self,
        _state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        if self.text != prev.text {
            element.mark_dirty();
            ChangeFlags::NEEDS_BUILD
        } else {
            ChangeFlags::NONE  // Skip rebuild - huge optimization!
        }
    }
}
```

**Pattern B: PartialEq check**

If your view implements `PartialEq`:

```rust
fn rebuild(
    self,
    prev: &Self,
    _state: &mut Self::State,
    element: &mut Self::Element,
) -> ChangeFlags {
    if self == *prev {
        ChangeFlags::NONE  // Nothing changed
    } else {
        element.mark_dirty();
        ChangeFlags::NEEDS_BUILD
    }
}
```

**Pattern C: Selective checks**

Check only expensive properties:

```rust
fn rebuild(
    self,
    prev: &Self,
    _state: &mut Self::State,
    element: &mut Self::Element,
) -> ChangeFlags {
    // Only rebuild if expensive property changed
    if self.data != prev.data {
        element.mark_dirty();
        ChangeFlags::NEEDS_BUILD
    } else if self.style != prev.style {
        // Style changed but not data - only repaint needed
        ChangeFlags::NEEDS_PAINT
    } else {
        ChangeFlags::NONE
    }
}
```

## The teardown() Method

### Signature

```rust
fn teardown(&self, state: &mut Self::State, element: &mut Self::Element) {}
```

### When Called

Called when the view is being removed from the tree.

### Use Cases

- Clean up resources
- Cancel subscriptions
- Close connections
- Free allocations

### Example

```rust
struct NetworkView {
    url: String,
}

// State holds connection
struct NetworkState {
    connection: Option<Connection>,
}

impl View for NetworkView {
    type Element = Element;
    type State = NetworkState;

    fn build(self, _ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Establish connection
        let connection = Connection::new(&self.url);
        let state = NetworkState {
            connection: Some(connection),
        };

        // Build UI...
        (element, state)
    }

    fn teardown(&self, state: &mut Self::State, _element: &mut Self::Element) {
        // Close connection when removed
        if let Some(conn) = state.connection.take() {
            conn.close();
        }
    }
}
```

## BuildContext

### What is BuildContext?

BuildContext provides read-only access during build phase:

```rust
pub struct BuildContext {
    tree: Arc<RwLock<ElementTree>>,
    element_id: ElementId,
    hook_context: Arc<RefCell<HookContext>>,
}
```

### Available Methods

**Hook access:**
```rust
ctx.with_hook_context_mut(|hook_ctx| {
    // Access hooks
})
```

**Tree queries:**
```rust
let parent_id = ctx.parent();
let depth = ctx.depth();
ctx.visit_ancestors(&mut |id| { /* ... */ true });
```

**Render object:**
```rust
let render_id = ctx.find_render_object();
let size = ctx.size();
```

### Why Read-Only?

BuildContext is intentionally read-only to:
- Enable parallel builds
- Prevent lock contention
- Make build phase predictable
- Match Flutter semantics

**State changes happen via hooks, not BuildContext!**

```rust
// ✅ Correct
let signal = use_signal(ctx, 0);
signal.set(42);  // Signal handles rebuild scheduling

// ❌ Wrong
// ctx.schedule_rebuild();  // This method doesn't exist!
```

## ChangeFlags

ChangeFlags provide granular control over updates:

```rust
pub struct ChangeFlags(u8);

impl ChangeFlags {
    pub const NONE: Self = Self(0);
    pub const NEEDS_BUILD: Self = Self(1 << 0);
    pub const NEEDS_LAYOUT: Self = Self(1 << 1);
    pub const NEEDS_PAINT: Self = Self(1 << 2);
    pub const ALL: Self = Self(0xFF);
}
```

### Combining Flags

```rust
// Multiple flags
ChangeFlags::NEEDS_LAYOUT | ChangeFlags::NEEDS_PAINT

// Check if flag is set
if flags.contains(ChangeFlags::NEEDS_BUILD) {
    // Rebuild needed
}
```

### Common Patterns

```rust
// Nothing changed
return ChangeFlags::NONE;

// Full rebuild
return ChangeFlags::NEEDS_BUILD;

// Only layout changed (e.g., size)
return ChangeFlags::NEEDS_LAYOUT;

// Only visual changed (e.g., color)
return ChangeFlags::NEEDS_PAINT;

// Layout and paint but not structure
return ChangeFlags::NEEDS_LAYOUT | ChangeFlags::NEEDS_PAINT;
```

## Best Practices

### 1. Keep Views Cheap

Views are created every frame - keep them lightweight:

```rust
// ✅ Good - simple fields
struct GoodView {
    text: String,
    count: i32,
}

// ❌ Bad - expensive fields
struct BadView {
    heavy_data: Vec<ComplexStruct>,  // Cloned every frame!
    connection: Arc<Mutex<Connection>>,
}
```

### 2. Implement Clone Efficiently

```rust
// ✅ Good - cheap clone
#[derive(Clone)]
struct GoodView {
    text: String,  // String uses Arc internally
    data: Arc<Vec<i32>>,  // Explicit Arc for shared data
}

// ❌ Bad - expensive clone
#[derive(Clone)]
struct BadView {
    data: Vec<ComplexStruct>,  // Deep clone every frame!
}
```

### 3. Optimize rebuild()

Always override `rebuild()` for frequently changing views:

```rust
impl View for MyView {
    fn rebuild(
        self,
        prev: &Self,
        _state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        // Compare cheaply
        if self.id == prev.id && self.version == prev.version {
            return ChangeFlags::NONE;  // Massive optimization!
        }

        // Only rebuild if actually changed
        element.mark_dirty();
        ChangeFlags::NEEDS_BUILD
    }
}
```

### 4. Use Hooks Correctly

```rust
// ✅ Correct - hooks at top level
fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
    let state1 = use_signal(ctx, 0);
    let state2 = use_signal(ctx, "");
    let memo = use_memo(ctx, |_| state1.get() * 2);
    // Build UI...
}

// ❌ Wrong - conditional hooks
fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
    if self.condition {
        let state = use_signal(ctx, 0);  // ❌ DON'T!
    }
    // ...
}
```

### 5. Clone Signals for Closures

```rust
fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
    let count = use_signal(ctx, 0);

    // ✅ Clone before moving
    let count_clone = count.clone();
    let button = Button::new("Click", move || {
        count_clone.update(|n| n + 1);
    });

    // count is still available here
    Text::new(format!("Count: {}", count.get()))
}
```

## Common Patterns

### Form with Validation

```rust
#[derive(Clone)]
struct LoginForm;

impl View for LoginForm {
    type Element = Element;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        let email = use_signal(ctx, String::new());
        let password = use_signal(ctx, String::new());

        // Computed validation
        let is_valid = use_memo(ctx, |_| {
            let email_val = email.get();
            let pass_val = password.get();
            email_val.contains('@') && pass_val.len() >= 8
        });

        // Submit handler
        let email_clone = email.clone();
        let password_clone = password.clone();
        let on_submit = move || {
            if is_valid.get() {
                submit_login(&email_clone.get(), &password_clone.get());
            }
        };

        // Build form
        let element = Column::new()
            .child(TextField::new("Email", email))
            .child(TextField::new("Password", password).password())
            .child(Button::new("Login", on_submit).enabled(is_valid.get()))
            .into_element();

        (element, ())
    }
}
```

### List with Dynamic Items

```rust
#[derive(Clone)]
struct TodoList {
    items: Arc<Vec<TodoItem>>,  // Arc for cheap clone
}

impl View for TodoList {
    type Element = Element;
    type State = ();

    fn build(self, _ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        let children: Vec<_> = self.items
            .iter()
            .map(|item| TodoItemView::new(item.clone()))
            .collect();

        let element = Column::new()
            .children(children)
            .into_element();

        (element, ())
    }

    fn rebuild(
        self,
        prev: &Self,
        _state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        // Use Arc::ptr_eq for cheap comparison
        if Arc::ptr_eq(&self.items, &prev.items) {
            return ChangeFlags::NONE;
        }

        element.mark_dirty();
        ChangeFlags::NEEDS_BUILD
    }
}
```

## See Also

- [ARCHITECTURE.md](./ARCHITECTURE.md) - Overall architecture
- [HOOKS_GUIDE.md](./HOOKS_GUIDE.md) - State management with hooks
- [RENDER_INTEGRATION.md](./RENDER_INTEGRATION.md) - Creating RenderObjects
- [examples/](../examples/) - Runnable examples
