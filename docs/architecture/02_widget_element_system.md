# Chapter 2: Widget/Element System

## 📋 Overview

Widget/Element система - это сердце FLUI. Widgets описывают **что** показывать (immutable configuration), а Elements управляют **как** это живет в дереве (mutable state holders).

## 🎨 Widget System

### Widget Hierarchy

```
Widget (sealed trait)
  ├── StatelessWidget      - pure component, no state
  ├── StatefulWidget       - creates State object
  ├── InheritedWidget      - data propagation down tree
  ├── ParentDataWidget     - attaches layout metadata
  └── RenderObjectWidget   - direct rendering control
```

### Base Widget Trait

```rust
/// Base Widget trait (sealed - cannot be implemented directly)
pub trait Widget: sealed::Sealed + DynWidget + Clone + Sized {
    /// Optional key for widget identity
    fn key(&self) -> Option<&str> {
        None
    }
    
    /// Create element from this widget
    fn into_element(self) -> Self::ElementType {
        <Self as sealed::Sealed>::ElementType::from_widget(self)
    }
}

/// Sealed trait pattern - prevents external implementation
mod sealed {
    pub trait Sealed {
        /// The concrete Element type for this Widget
        type ElementType: DynElement + Send + Sync + 'static;
    }
}

/// Object-safe trait for heterogeneous storage
pub trait DynWidget: Debug + Send + Sync + 'static {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
```

**Design Choices:**
- ✅ **Sealed** - prevents external trait implementation (stability)
- ✅ **Clone** - widgets are immutable, cheap to clone
- ✅ **Send + Sync** - thread-safe by construction
- ✅ **Associated ElementType** - compile-time widget→element link

---

## 1️⃣ StatelessWidget

### Definition

```rust
/// StatelessWidget - pure function from configuration to UI
pub trait StatelessWidget: Debug + Clone + Send + Sync + 'static {
    /// Build child widget tree
    fn build(&self) -> BoxedWidget;
}

// Automatic implementations
impl<T: StatelessWidget> sealed::Sealed for T {
    type ElementType = ComponentElement<T>;
}

impl<T: StatelessWidget> Widget for T { /* ... */ }
impl<T: StatelessWidget> DynWidget for T { /* ... */ }
```

### Example: Greeting Widget

```rust
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct Greeting {
    name: Arc<String>,
    style: TextStyle,
}

impl Greeting {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: Arc::new(name.into()),
            style: TextStyle::default(),
        }
    }

    pub fn with_style(mut self, style: TextStyle) -> Self {
        self.style = style;
        self
    }
}

impl StatelessWidget for Greeting {
    fn build(&self) -> BoxedWidget {
        Box::new(
            Container::new()
                .padding(EdgeInsets::all(16.0))
                .child(Box::new(
                    Text::new(format!("Hello, {}!", self.name))
                        .style(self.style.clone())
                ))
        )
    }
}

// Usage:
fn main() {
    let widget = Greeting::new("World")
        .with_style(TextStyle::new()
            .size(24.0)
            .weight(FontWeight::Bold)
            .color(Color::BLUE));

    run_app(widget);
}
```

### ComponentElement (for StatelessWidget)

```rust
pub struct ComponentElement<W: StatelessWidget> {
    /// The widget configuration
    widget: W,
    
    /// Parent element ID
    parent: Option<ElementId>,
    
    /// Child element (result of build())
    child: Option<ElementId>,
    
    /// Lifecycle state
    lifecycle: ElementLifecycle,
    
    /// Needs rebuild flag
    dirty: bool,
}

impl<W: StatelessWidget> ComponentElement<W> {
    pub fn new(widget: W) -> Self {
        Self {
            widget,
            parent: None,
            child: None,
            lifecycle: ElementLifecycle::Initial,
            dirty: true,
        }
    }
    
    /// Rebuild widget (calls build())
    fn rebuild(&mut self) -> Vec<(ElementId, BoxedWidget, usize)> {
        if !self.dirty {
            return Vec::new();
        }
        
        self.dirty = false;
        
        // Call build() to get new child
        let child_widget = self.widget.build();
        
        // Return child for mounting
        vec![(self.id(), child_widget, 0)]
    }
}
```

**Use Cases:**
- Pure presentation widgets
- Composition of other widgets
- Layout helpers
- Decorators

---

## 2️⃣ StatefulWidget + State

### Definition

```rust
/// StatefulWidget - creates mutable State object
pub trait StatefulWidget: Debug + Clone + Send + Sync + DynWidget + 'static {
    type State: State<Widget = Self>;
    
    /// Create state object (called once on mount)
    fn create_state(&self) -> Self::State;
}

/// State - mutable state that persists across rebuilds
pub trait State: Debug + Send + Sync + 'static {
    type Widget: StatefulWidget;
    
    /// Build widget tree (can be called multiple times)
    fn build(&mut self) -> BoxedWidget;
    
    /// Lifecycle hooks
    fn init_state(&mut self) {}
    fn did_update_widget(&mut self, new_widget: &Self::Widget) {}
    fn dispose(&mut self) {}
}
```

### Example: Counter Widget

```rust
#[derive(Debug, Clone)]
pub struct Counter {
    pub initial: i32,
    pub step: i32,
}

impl Counter {
    pub fn new(initial: i32) -> Self {
        Self { initial, step: 1 }
    }
}

impl StatefulWidget for Counter {
    type State = CounterState;
    
    fn create_state(&self) -> Self::State {
        CounterState {
            count: Signal::new(self.initial),
            step: Signal::new(self.step),
            timer: None,
        }
    }
}

// Implement Widget + DynWidget via macro
impl_widget_for_stateful!(Counter);

#[derive(Debug)]
pub struct CounterState {
    count: Signal<i32>,
    step: Signal<i32>,
    timer: Option<Timer>,
}

impl State for CounterState {
    type Widget = Counter;
    
    fn build(&mut self) -> BoxedWidget {
        Box::new(
            column![
                text(format!("Count: {}", self.count.get()))
                    .size(24.0),
                    
                row![
                    button("−")
                        .on_press_signal_update(&self.count, |c| *c -= 1),
                    button("+")
                        .on_press_signal_inc(&self.count),
                ],
                
                text(format!("Step: {}", self.step.get()))
                    .size(14.0),
            ]
        )
    }
    
    fn init_state(&mut self) {
        println!("Counter initialized");
        // Start auto-increment timer
        self.timer = Some(Timer::periodic(Duration::from_secs(1)));
    }
    
    fn did_update_widget(&mut self, new_widget: &Counter) {
        // React to widget configuration changes
        if new_widget.step != self.step.get() {
            self.step.set(new_widget.step);
        }
    }
    
    fn dispose(&mut self) {
        // Cleanup
        if let Some(timer) = self.timer.take() {
            timer.cancel();
        }
        println!("Counter disposed");
    }
}
```

### StatefulElement

```rust
pub struct StatefulElement<W: StatefulWidget> {
    /// Widget configuration (recreated on update)
    widget: W,
    
    /// State object (persists across rebuilds!)
    state: W::State,
    
    /// Parent element
    parent: Option<ElementId>,
    
    /// Child element
    child: Option<ElementId>,
    
    /// Lifecycle
    lifecycle: ElementLifecycle,
    
    /// Dirty flag
    dirty: bool,
    
    /// Has init_state been called?
    initialized: bool,
}

impl<W: StatefulWidget> StatefulElement<W> {
    pub fn new(widget: W) -> Self {
        let state = widget.create_state();
        
        Self {
            widget,
            state,
            parent: None,
            child: None,
            lifecycle: ElementLifecycle::Initial,
            dirty: true,
            initialized: false,
        }
    }
    
    /// Get mutable reference to state
    pub fn state_mut(&mut self) -> &mut W::State {
        &mut self.state
    }
    
    /// Update with new widget configuration
    pub fn update(&mut self, new_widget: W) {
        let _old_widget = std::mem::replace(&mut self.widget, new_widget);
        
        // Notify state of widget change
        self.state.did_update_widget(&self.widget);
        
        // Mark dirty for rebuild
        self.dirty = true;
    }
}
```

### State Lifecycle

```
1. create_state()      → State object created
         ↓
2. mount()             → Element mounted to tree
         ↓
3. init_state()        → State initialization (once!)
         ↓
4. build()             → Build UI (multiple times)
         ↓
   ┌──────────┐
   │ ACTIVE   │ ←──┐
   └──────────┘    │
         ↓         │
5. did_update_widget() → Widget config changed
         │         │
         └─────────┘
         ↓
6. dispose()           → State cleanup
         ↓
   DEFUNCT
```

**Use Cases:**
- Widgets with mutable state
- User interactions (forms, buttons)
- Animations
- Timers and subscriptions
- Data fetching

---

## 3️⃣ InheritedWidget

### Definition

```rust
/// InheritedWidget - efficient data propagation down tree
pub trait InheritedWidget: Debug + Clone + Send + Sync + 'static {
    /// Child widget
    fn child(&self) -> BoxedWidget;
    
    /// Should notify dependents when updated?
    fn update_should_notify(&self, old: &Self) -> bool;
}
```

### Example: Theme Widget

```rust
#[derive(Debug, Clone)]
pub struct Theme {
    pub primary_color: Color,
    pub background_color: Color,
    pub text_style: TextStyle,
    pub child: BoxedWidget,
}

impl Theme {
    pub fn new(child: BoxedWidget) -> Self {
        Self {
            primary_color: Color::BLUE,
            background_color: Color::WHITE,
            text_style: TextStyle::default(),
            child,
        }
    }
    
    pub fn dark(child: BoxedWidget) -> Self {
        Self {
            primary_color: Color::from_hex("#90CAF9"),
            background_color: Color::from_hex("#121212"),
            text_style: TextStyle::default().color(Color::WHITE),
            child,
        }
    }
    
    /// Access Theme from BuildContext
    pub fn of(cx: &BuildContext) -> &Theme {
        cx.depend_on_inherited::<Theme>()
            .expect("No Theme found in context")
    }
}

impl InheritedWidget for Theme {
    fn child(&self) -> BoxedWidget {
        self.child.clone()
    }
    
    fn update_should_notify(&self, old: &Self) -> bool {
        self.primary_color != old.primary_color
            || self.background_color != old.background_color
            || self.text_style != old.text_style
    }
}

// Usage:
fn main() {
    let app = Theme::dark(
        Box::new(MyApp)
    );
    run_app(app);
}

// In any descendant widget - access via BuildContext
#[derive(Debug, Clone)]
struct ThemedButton {
    label: String,
}

impl StatelessWidget for ThemedButton {
    fn build(&self) -> BoxedWidget {
        // Note: In actual implementation, BuildContext would be passed to build()
        // This is a conceptual example showing how theme access would work
        Box::new(
            container()
                .child(text(&self.label))
        )
    }
}
```

### InheritedElement

```rust
pub struct InheritedElement<W: InheritedWidget> {
    widget: W,
    child: Option<ElementId>,
    
    /// Elements that depend on this InheritedWidget
    dependents: HashSet<ElementId>,
}

impl<W: InheritedWidget> InheritedElement<W> {
    /// Register dependent element
    pub fn add_dependent(&mut self, dependent_id: ElementId) {
        self.dependents.insert(dependent_id);
    }
    
    /// Notify dependents when widget updates
    pub fn notify_dependents(&mut self, tree: &mut ElementTree) {
        for &dependent_id in &self.dependents {
            tree.mark_dirty(dependent_id);
        }
    }
    
    /// Update widget and notify if needed
    pub fn update(&mut self, new_widget: W, tree: &mut ElementTree) {
        let should_notify = new_widget.update_should_notify(&self.widget);
        self.widget = new_widget;
        
        if should_notify {
            self.notify_dependents(tree);
        }
    }
}
```

**Use Cases:**
- Theme propagation
- Locale/Internationalization
- MediaQuery (screen size, orientation)
- Navigation state
- Authentication state

---

## 4️⃣ ParentDataWidget

### Definition

```rust
/// ParentDataWidget - attaches metadata to descendant RenderObjects
pub trait ParentDataWidget: Debug + Clone + Send + Sync + 'static {
    type ParentData: ParentData;
    
    fn child(&self) -> BoxedWidget;
    fn create_parent_data(&self) -> Self::ParentData;
    fn update_parent_data(&self, parent_data: &mut Self::ParentData);
}

/// ParentData trait - marker for metadata types
pub trait ParentData: Debug + Send + Sync + 'static {}
```

### Example: Flexible (Flex Layout)

```rust
#[derive(Debug, Clone)]
pub struct FlexParentData {
    pub flex: i32,
    pub fit: FlexFit,
}

impl ParentData for FlexParentData {}

#[derive(Debug, Clone)]
pub struct Flexible {
    pub flex: i32,
    pub fit: FlexFit,
    pub child: BoxedWidget,
}

impl Flexible {
    pub fn new(child: BoxedWidget) -> Self {
        Self {
            flex: 1,
            fit: FlexFit::Loose,
            child,
        }
    }
    
    pub fn with_flex(mut self, flex: i32) -> Self {
        self.flex = flex;
        self
    }
}

impl ParentDataWidget for Flexible {
    type ParentData = FlexParentData;
    
    fn child(&self) -> BoxedWidget {
        self.child.clone()
    }
    
    fn create_parent_data(&self) -> Self::ParentData {
        FlexParentData {
            flex: self.flex,
            fit: self.fit,
        }
    }
    
    fn update_parent_data(&self, parent_data: &mut Self::ParentData) {
        parent_data.flex = self.flex;
        parent_data.fit = self.fit;
    }
}

// Usage:
let layout = row![
    flexible(text("Left")).with_flex(1),
    flexible(text("Right")).with_flex(2),  // Takes 2x space
];
```

### How RenderObject Uses ParentData

```rust
impl RenderObject for RenderFlex {
    type Arity = MultiArity;
    
    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        let children = cx.children();
        
        // Access parent data attached by Flexible widget
        for &child_id in children {
            let parent_data = cx.parent_data::<FlexParentData>(child_id);
            
            if let Some(pd) = parent_data {
                // Use flex factor for layout
                let flex_factor = pd.flex as f32;
                let constraints = self.compute_child_constraints(flex_factor);
                cx.layout_child(child_id, constraints);
            }
        }
        
        // ...
    }
}
```

**Use Cases:**
- Flex layout (flex factor, fit)
- Stack layout (alignment, positioning)
- Table layout (row/column span)
- Grid layout (area placement)
- Custom layouts

---

## 5️⃣ RenderObjectWidget

### Definition

```rust
/// RenderObjectWidget - creates RenderObject directly
pub trait RenderObjectWidget: Debug + Clone + Send + Sync + 'static {
    type Arity: Arity;
    type Render: RenderObject<Arity = Self::Arity>;
    
    fn create_render_object(&self) -> Self::Render;
    fn update_render_object(&self, render: &mut Self::Render);
}
```

### Example: Opacity (SingleArity)

```rust
#[derive(Debug, Clone)]
pub struct Opacity {
    pub opacity: f32,
    pub child: BoxedWidget,
}

impl Opacity {
    pub fn new(opacity: f32, child: BoxedWidget) -> Self {
        Self { opacity, child }
    }
}

impl RenderObjectWidget for Opacity {
    type Arity = SingleArity;
    type Render = RenderOpacity;
    
    fn create_render_object(&self) -> Self::Render {
        RenderOpacity {
            opacity: self.opacity,
        }
    }
    
    fn update_render_object(&self, render: &mut Self::Render) {
        render.opacity = self.opacity;
    }
}

#[derive(Debug)]
pub struct RenderOpacity {
    opacity: f32,
}

impl RenderObject for RenderOpacity {
    type Arity = SingleArity;
    
    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        // Get the single child (guaranteed by SingleArity)
        let child = cx.child();
        cx.layout_child(child, cx.constraints())
    }

    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        // Get the single child (guaranteed by SingleArity)
        let child = cx.child();
        let child_layer = cx.capture_child_layer(child);

        let mut opacity_layer = OpacityLayer::new(self.opacity);
        opacity_layer.add_child(child_layer);
        Box::new(opacity_layer)
    }
}
```

**Use Cases:**
- Direct layout/paint control
- Performance-critical widgets
- Custom rendering
- Platform integration

**See:** [Chapter 3: RenderObject System](03_render_objects.md) for details

---

## 🔄 Element Lifecycle

### Lifecycle States

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementLifecycle {
    Initial,    // Created but not mounted
    Active,     // Mounted and in tree
    Inactive,   // Deactivated (can be reactivated)
    Defunct,    // Permanently removed
}
```

### Lifecycle Flow

```
┌─────────┐
│ Initial │  ← Element created from Widget
└────┬────┘
     │ mount()
     ↓
┌─────────┐
│ Active  │  ← In tree, participating in layout/paint
└────┬────┘
     │
     │ deactivate()
     ↓
┌──────────┐
│ Inactive │  ← Temporarily removed (e.g., in Navigator)
└────┬─────┘
     │
     │ activate() ──┐
     ↓              │
┌─────────┐         │
│ Active  │ ←───────┘
└────┬────┘
     │ unmount()
     ↓
┌─────────┐
│ Defunct │  ← Permanently removed, cleanup done
└─────────┘
```

### Lifecycle Methods

```rust
pub trait DynElement: Debug + Send + Sync {
    /// Mount element to tree
    fn mount(&mut self, parent: Option<ElementId>, slot: usize);
    
    /// Unmount element (permanent removal)
    fn unmount(&mut self);
    
    /// Deactivate (temporary removal)
    fn deactivate(&mut self);
    
    /// Activate (restore from inactive)
    fn activate(&mut self);
    
    /// Update with new widget configuration
    fn update_any(&mut self, new_widget: Box<dyn DynWidget>);
    
    /// Check if needs rebuild
    fn is_dirty(&self) -> bool;
    
    /// Mark as needing rebuild
    fn mark_dirty(&mut self);
    
    /// Rebuild (calls build() on widget/state)
    fn rebuild(&mut self, element_id: ElementId) -> Vec<(ElementId, BoxedWidget, usize)>;
}
```

---

## 🏗️ BuildContext

### Definition

```rust
/// BuildContext - provides access to tree and services during build
pub struct BuildContext {
    /// Current element ID
    element_id: ElementId,
    
    /// Reference to element tree
    tree: &ElementTree,
    
    /// Inherited widgets cache
    inherited_cache: HashMap<TypeId, *const dyn Any>,
}

impl BuildContext {
    /// Get inherited widget (registers dependency)
    pub fn depend_on_inherited<T: InheritedWidget + 'static>(&self) -> Option<&T> {
        // Find T in ancestor chain by TypeId
        let type_id = TypeId::of::<T>();

        // Walk up tree to find InheritedElement with matching type
        let mut current = Some(self.element_id);
        while let Some(id) = current {
            if let Some(element) = self.tree.get(id) {
                if element.type_id() == type_id {
                    // Found matching InheritedElement
                    // Register this element as dependent
                    element.add_dependent(self.element_id);

                    // Return widget (with proper lifetime and type casting)
                    return element.widget_as::<T>();
                }
                current = self.tree.parent(id);
            } else {
                break;
            }
        }

        None
    }
    
    /// Get service from dependency injection
    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.tree.get_service::<T>()
    }
    
    /// Create callback that marks element dirty
    pub fn callback<F>(&self, f: F) -> impl Fn() + 'static
    where
        F: Fn() + 'static,
    {
        let element_id = self.element_id;
        move || {
            PIPELINE.with(|p| {
                p.borrow_mut().mark_dirty(element_id);
            });
            f();
        }
    }
}
```

**Use Cases:**
- Access inherited widgets
- Dependency injection
- Create callbacks that trigger rebuilds
- Navigate tree

---

## 📊 Widget Comparison Table

| Type | Mutable State | Children | Lifecycle | Use Case |
|------|---------------|----------|-----------|----------|
| **StatelessWidget** | ❌ No | Single (build result) | Simple | Pure composition |
| **StatefulWidget** | ✅ Yes (State) | Single (build result) | Complex | Interactive widgets |
| **InheritedWidget** | ❌ No | Single | Medium | Data propagation |
| **ParentDataWidget** | ❌ No | Single | Simple | Layout metadata |
| **RenderObjectWidget** | ❌ No | 0/1/N (by Arity) | Medium | Direct rendering |

## 🎯 Best Practices

### 1. Prefer StatelessWidget

```rust
// ✅ Good - stateless when possible
#[derive(Clone)]
struct UserCard {
    user: User,
}

impl StatelessWidget for UserCard {
    fn build(&self) -> BoxedWidget {
        Box::new(card().child(text(&self.user.name)))
    }
}
```

### 2. Keep Widgets Small

```rust
// ✅ Good - small, focused widgets
impl StatelessWidget for UserProfile {
    fn build(&self) -> BoxedWidget {
        Box::new(column![
            UserAvatar::new(&self.user),
            UserInfo::new(&self.user),
            UserActions::new(&self.user),
        ])
    }
}
```

### 3. Use Keys for Dynamic Lists

```rust
// ✅ Good - keys preserve state
impl StatelessWidget for TodoList {
    fn build(&self) -> BoxedWidget {
        Box::new(
            column(
                self.todos.iter().map(|todo| {
                    TodoItem::new(todo)
                        .with_key(&todo.id.to_string())
                }).collect()
            )
        )
    }
}
```

### 4. Implement Clone Efficiently

```rust
// ✅ Good - cheap clone with Rc/Arc
#[derive(Clone)]
pub struct ExpensiveWidget {
    data: Arc<ExpensiveData>,  // Shared, not copied!
}
```

## 🔗 Cross-References

- **Previous:** [Chapter 1: Architecture](01_architecture.md)
- **Next:** [Chapter 3: RenderObject System](03_render_objects.md)
- **Related:** [Appendix A: Reactive System](appendix_a_reactive_system.md)

---

**Key Takeaway:** FLUI's widget system provides five widget types for different use cases, with automatic Element management and lifecycle handling. Choose the right widget type for your needs!
