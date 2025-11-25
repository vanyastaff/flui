# flui-view

View traits and abstractions for the FLUI UI framework.

This crate provides the view layer of FLUI's three-tree architecture, defining how declarative UI components are structured and built.

## Architecture

```
View (immutable config) --> Element (mutable state) --> RenderObject (layout/paint)
^^^^^^^^^^^^^^^^^^^^
This crate!
```

In FLUI's three-tree architecture:

- **View Layer** - Immutable configuration objects describing UI *(this crate)*
- **Element Layer** - Mutable instances managing lifecycle and state
- **Render Layer** - Layout computation and painting

## Key Features

- **Multiple view types** - Stateless, Stateful, Animated, Provider, Proxy
- **Type-safe wrappers** - ViewObject trait with downcasting support
- **Abstract BuildContext** - Decoupled from concrete pipeline implementation
- **Ergonomic child APIs** - `Child` and `Children` helper types
- **Flutter-inspired** - Familiar patterns for Flutter developers

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
flui-view = { path = "../flui-view" }
```

## Module Structure

```
flui-view/
├── Cargo.toml
└── src/
    ├── lib.rs              # Crate root, re-exports
    ├── empty.rs            # EmptyView - renders nothing
    ├── state.rs            # ViewState marker trait
    │
    ├── traits/             # View trait definitions
    │   ├── mod.rs
    │   ├── stateless.rs    # StatelessView trait
    │   ├── stateful.rs     # StatefulView trait
    │   ├── animated.rs     # AnimatedView + Listenable traits
    │   ├── provider.rs     # ProviderView trait
    │   └── proxy.rs        # ProxyView trait
    │
    ├── wrappers/           # ViewObject implementations
    │   ├── mod.rs
    │   ├── stateless.rs    # StatelessViewWrapper + Stateless helper
    │   ├── stateful.rs     # StatefulViewWrapper + Stateful helper
    │   ├── animated.rs     # AnimatedViewWrapper + Animated helper
    │   ├── provider.rs     # ProviderViewWrapper + Provider helper
    │   └── proxy.rs        # ProxyViewWrapper + Proxy helper
    │
    ├── object/             # ViewObject trait
    │   ├── mod.rs
    │   └── view_object.rs  # ViewObject trait + ElementViewObjectExt
    │
    ├── context/            # BuildContext abstraction
    │   ├── mod.rs
    │   └── build_context.rs # BuildContext trait
    │
    ├── protocol/           # View categorization
    │   └── mod.rs          # ViewMode enum
    │
    └── children/           # Child helper types
        ├── mod.rs
        ├── child.rs        # Child - optional single child
        └── children.rs     # Children - multiple children
```

## View Types

### StatelessView

Views without internal state. Rebuild completely when parent rebuilds.

```rust
use flui_view::{StatelessView, BuildContext, IntoElement};

struct Greeting {
    name: String,
}

impl StatelessView for Greeting {
    fn build(self, ctx: &dyn BuildContext) -> impl IntoElement {
        Text::new(format!("Hello, {}!", self.name))
    }
}

// Convert to element
let element = Stateless(Greeting { name: "World".into() }).into_element();
```

**When to use:**
- Simple views depending only on configuration
- No state persistence needed
- Can be recreated at any time

### StatefulView

Views with persistent mutable state across rebuilds.

```rust
use flui_view::{StatefulView, BuildContext, IntoElement, ViewState};

struct Counter {
    initial: i32,
}

struct CounterState {
    count: i32,
}

impl StatefulView for Counter {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        CounterState { count: self.initial }
    }

    fn build(&self, state: &mut Self::State, ctx: &dyn BuildContext) -> impl IntoElement {
        Column::new()
            .child(Text::new(format!("Count: {}", state.count)))
            .child(Button::new("+").on_press(|| {
                state.count += 1;
                ctx.mark_dirty();
            }))
    }
}

// Convert to element
let element = Stateful(Counter { initial: 0 }).into_element();
```

**Lifecycle:**
1. `create_state()` - Called once when element is first mounted
2. `build()` - Called on each rebuild with current state
3. `did_update_view()` - Called when view configuration changes
4. State persists until element is unmounted

### AnimatedView

Views that subscribe to animation changes and rebuild automatically.

```rust
use flui_view::{AnimatedView, Listenable, BuildContext, IntoElement};

struct FadeTransition {
    opacity: Animation<f32>,
    child: Element,
}

impl AnimatedView<Animation<f32>> for FadeTransition {
    fn listenable(&self) -> &Animation<f32> {
        &self.opacity
    }

    fn build(&mut self, ctx: &dyn BuildContext) -> impl IntoElement {
        Opacity::new(self.opacity.value())
            .child(self.child.clone())
    }
}

// Convert to element
let element = Animated::new(fade_transition).into_element();
```

**When to use:**
- Widget driven by animation
- Rebuild on every animation frame
- Multiple widgets sharing same animation

### ProviderView

Views that provide data to descendants via dependency injection.

```rust
use flui_view::{ProviderView, BuildContext, IntoElement};
use std::sync::Arc;

struct ThemeProvider {
    theme: Arc<Theme>,
    child: Element,
}

impl ProviderView<Theme> for ThemeProvider {
    fn value(&self) -> &Theme {
        &self.theme
    }

    fn build(&mut self, ctx: &dyn BuildContext) -> impl IntoElement {
        self.child.clone()
    }

    fn should_notify(&self, old: &Theme) -> bool {
        !Arc::ptr_eq(&self.theme, &old)
    }
}

// Usage in descendant via ctx.depend_on::<Theme>()
```

**When to use:**
- Shared state/config (theme, locale, user)
- Dependency injection
- Configuration cascading down tree

### ProxyView

Views that wrap a single child without affecting layout.

```rust
use flui_view::{ProxyView, BuildContext, IntoElement};
use flui_types::Event;

struct IgnorePointer {
    ignoring: bool,
    child: Element,
}

impl ProxyView for IgnorePointer {
    fn build_child(&mut self, ctx: &dyn BuildContext) -> impl IntoElement {
        self.child.clone()
    }

    fn handle_event(&mut self, event: &Event, ctx: &dyn BuildContext) -> bool {
        self.ignoring  // Block events if ignoring
    }
}

// Convert to element
let element = Proxy(ignore_pointer).into_element();
```

**When to use:**
- Event interception (IgnorePointer, GestureDetector)
- Accessibility (Semantics, ExcludeSemantics)
- Focus management (Focus, FocusScope)
- Optimization hints (RepaintBoundary)

## ViewObject Trait

The `ViewObject` trait provides dynamic dispatch for view lifecycle operations:

```rust
pub trait ViewObject: Send + 'static {
    // Core
    fn mode(&self) -> ViewMode;
    fn build(&mut self, ctx: &dyn BuildContext) -> Element;
    
    // Lifecycle (with defaults)
    fn init(&mut self, ctx: &dyn BuildContext) {}
    fn did_change_dependencies(&mut self, ctx: &dyn BuildContext) {}
    fn did_update(&mut self, old_view: &dyn Any, ctx: &dyn BuildContext) {}
    fn deactivate(&mut self, ctx: &dyn BuildContext) {}
    fn dispose(&mut self, ctx: &dyn BuildContext) {}
    
    // Downcasting
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    
    // Provider-specific (default: None)
    fn provided_value(&self) -> Option<&(dyn Any + Send + Sync)> { None }
    fn dependents(&self) -> Option<&[ElementId]> { None }
}
```

### Downcasting ViewObjects

```rust
use flui_view::{ViewObject, StatelessViewWrapper};

// Check view mode
if view_object.is_component() {
    // Handle component view
}

if view_object.is_render() {
    // Handle render view
}

// Downcast to concrete type
if let Some(wrapper) = view_object.downcast_ref::<StatelessViewWrapper<MyView>>() {
    // Access wrapper-specific methods
}
```

## BuildContext

Abstract context trait for view building:

```rust
pub trait BuildContext: Send + Sync {
    /// Get the current element's ID being built
    fn element_id(&self) -> ElementId;
    
    /// Get the parent element's ID
    fn parent_id(&self) -> Option<ElementId>;
    
    /// Get depth of current element in tree
    fn depth(&self) -> usize;
    
    /// Mark current element as needing rebuild
    fn mark_dirty(&self);
    
    /// Schedule a rebuild for a specific element
    fn schedule_rebuild(&self, element_id: ElementId);
}
```

The concrete implementation `PipelineBuildContext` lives in `flui-pipeline`, avoiding circular dependencies.

## ViewMode

Categorizes view behavior for framework processing:

| Mode | Description | Builds Children | Layout/Paint |
|------|-------------|-----------------|--------------|
| `Stateless` | No internal state | Yes | No |
| `Stateful` | Has mutable state | Yes | No |
| `Animated` | Driven by animation | Yes | No |
| `Provider` | Provides data to descendants | Yes | No |
| `Proxy` | Wraps single child | Yes | No |
| `RenderBox` | Box layout protocol | No | Yes |
| `RenderSliver` | Sliver layout protocol | No | Yes |

```rust
use flui_view::ViewMode;

let mode = ViewMode::Stateful;
assert!(mode.is_component());
assert!(!mode.is_render());
```

## Helper Types

### Child - Optional Single Child

```rust
use flui_view::Child;

pub struct Padding {
    padding: EdgeInsets,
    child: Child,
}

impl Padding {
    pub fn new(padding: EdgeInsets) -> Self {
        Self { padding, child: Child::none() }
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.child = Child::new(child);
        self
    }
}
```

### Children - Multiple Children

```rust
use flui_view::Children;

pub struct Column {
    children: Children,
}

impl Column {
    pub fn new() -> Self {
        Self { children: Children::new() }
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child);
        self
    }
    
    pub fn children<V: IntoElement>(mut self, items: impl IntoIterator<Item = V>) -> Self {
        self.children.extend(items);
        self
    }
}
```

## Wrapper Types and IntoElement Helpers

Each view trait has a corresponding wrapper and helper:

| View Trait | Wrapper | IntoElement Helper |
|------------|---------|-------------------|
| `StatelessView` | `StatelessViewWrapper<V>` | `Stateless(view)` |
| `StatefulView` | `StatefulViewWrapper<V>` | `Stateful(view)` |
| `AnimatedView<L>` | `AnimatedViewWrapper<V, L>` | `Animated::new(view)` |
| `ProviderView<T>` | `ProviderViewWrapper<V, T>` | `Provider::new(view)` |
| `ProxyView` | `ProxyViewWrapper<V>` | `Proxy(view)` |

```rust
use flui_view::{Stateless, Stateful, Proxy, Provider, Animated, IntoElement};

// Convert views to elements
let stateless_element = Stateless(my_stateless_view).into_element();
let stateful_element = Stateful(my_stateful_view).into_element();
let proxy_element = Proxy(my_proxy_view).into_element();
let provider_element = Provider::new(my_provider_view).into_element();
let animated_element = Animated::new(my_animated_view).into_element();
```

## EmptyView

A view that renders nothing, useful for conditional rendering:

```rust
use flui_view::EmptyView;

let view = if show_content {
    Text::new("Hello").into_element()
} else {
    EmptyView.into_element()
};
```

## Thread Safety

All view traits require `Send + Sync + 'static`:

- Views can be transferred between threads
- State types must be `Send + Sync`
- BuildContext is `Send + Sync`

## API Reference

### Core Types

| Type | Description |
|------|-------------|
| `StatelessView` | Trait for stateless views |
| `StatefulView` | Trait for stateful views |
| `AnimatedView<L>` | Trait for animation-driven views |
| `ProviderView<T>` | Trait for dependency injection |
| `ProxyView` | Trait for single-child wrappers |
| `ViewObject` | Dynamic dispatch interface |
| `BuildContext` | Abstract build context trait |
| `ViewMode` | View categorization enum |
| `ViewState` | Marker trait for state types |

### Helper Types

| Type | Description |
|------|-------------|
| `Child` | Optional single child wrapper |
| `Children` | Multiple children wrapper |
| `EmptyView` | View that renders nothing |

### Wrappers

| Type | Description |
|------|-------------|
| `StatelessViewWrapper<V>` | Wraps StatelessView |
| `StatefulViewWrapper<V>` | Wraps StatefulView with state |
| `AnimatedViewWrapper<V, L>` | Wraps AnimatedView with subscription |
| `ProviderViewWrapper<V, T>` | Wraps ProviderView with dependents |
| `ProxyViewWrapper<V>` | Wraps ProxyView |

## Crate Dependencies

```
flui-foundation (ElementId, Slot)
       |
       v
flui-tree (TreeRead, TreeNav, TreeWrite)
       |
       v
flui-element (Element, IntoElement)
       |
       v
flui-view (View traits, ViewObject, BuildContext)  <-- This crate
       |
       v
flui-pipeline (PipelineBuildContext - concrete impl)
```

## Design Decisions

### Abstract BuildContext

`BuildContext` is a trait, not a concrete type. This allows:
- `flui-view` to define the interface
- `flui-pipeline` to implement it
- No circular dependencies

### ViewObject vs Trait Objects

`ViewObject` is stored in Element as `Box<dyn Any + Send + Sync>`, not `Box<dyn ViewObject>`. This:
- Allows downcasting to concrete wrapper types
- Breaks dependency on ViewObject in flui-element
- Enables extension without modifying Element

### Wrapper Pattern

Each view trait has a wrapper that implements ViewObject:
- Wrappers manage state lifecycle (StatefulViewWrapper)
- Wrappers manage subscriptions (AnimatedViewWrapper)
- Wrappers track dependents (ProviderViewWrapper)

## Testing

```bash
# Run tests
cargo test -p flui-view

# Run with logging
RUST_LOG=debug cargo test -p flui-view
```

## License

Same as the FLUI framework - see root LICENSE file.
