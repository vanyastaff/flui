# flui-view Architecture Discussion

This document outlines the design decisions for the `flui-view` crate - the low-level View system.

## Scope Clarification

**flui-view** is a LOW-LEVEL crate providing:
- Core View traits (`View`, `StatelessView`, `StatefulView`, etc.)
- ViewObject trait (type-erased behavior)
- Key system (`ViewKey`)
- Build context abstraction
- Element lifecycle contracts

**NOT in scope** (belongs to `flui-widgets`):
- Concrete widgets (Text, Container, Column, Row, etc.)
- Layout widgets (Padding, Center, Align, etc.)
- Material/Cupertino design widgets

## Key Design Questions

### 1. View Trait Hierarchy

**Option A: Separate traits per category**
```rust
pub trait View: 'static {
    fn key(&self) -> ViewKey;
}

pub trait StatelessView: View {
    fn build(&self, ctx: &impl BuildContext) -> impl IntoElement;
}

pub trait StatefulView: View {
    type State: ViewState;
    fn create_state(&self) -> Self::State;
}

pub trait RenderView<P: Protocol, A: Arity>: View {
    fn create_render(&self, ctx: &impl BuildContext) -> Box<dyn RenderObject<Protocol=P, Arity=A>>;
    fn update_render(&self, ctx: &impl BuildContext, render: &mut dyn RenderObject);
}

pub trait ProxyView: View {
    fn child(&self) -> &dyn View;
}

pub trait ProviderView<T>: View {
    fn value(&self) -> &T;
    fn update_should_notify(&self, old: &T) -> bool;
}
```

**Option B: Single trait with associated type**
```rust
pub trait View: 'static {
    type Config: ViewConfig;
    fn key(&self) -> ViewKey;
    fn config(&self) -> Self::Config;
}

pub enum ViewConfig {
    Stateless(Box<dyn Fn(&impl BuildContext) -> impl IntoElement>),
    Stateful { create_state: Box<dyn Fn() -> Box<dyn ViewState>> },
    Render { create_render: ... },
    Proxy { child: Box<dyn View> },
    Provider { value: Box<dyn Any>, notify: ... },
}
```

**Recommendation:** Option A (separate traits)
- Clearer type signatures
- Better compile-time checking
- Easier to extend
- Follows Flutter's pattern

### 2. ViewObject (Type-Erased View)

The `ViewObject` trait enables storing different View types in Element tree:

```rust
pub trait ViewObject: Any + Send + Sync {
    /// Type identifier for reconciliation
    fn type_id(&self) -> TypeId;
    
    /// Key for reconciliation
    fn key(&self) -> ViewKey;
    
    /// Check if can update with new view
    fn can_update(&self, other: &dyn ViewObject) -> bool {
        self.type_id() == other.type_id() && self.key() == other.key()
    }
    
    /// Create element for this view
    fn create_element(&self) -> Box<dyn ElementBehavior>;
    
    /// Downcast to concrete type
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
```

**Question:** Should `ViewObject` implementations be auto-generated?

**Option A: Manual implementation**
- Each view type implements manually
- More control but repetitive

**Option B: Derive macro**
```rust
#[derive(ViewObject)]
pub struct MyView { ... }
```

**Option C: Blanket implementation**
```rust
impl<T: View + Any + Send + Sync> ViewObject for T {
    fn type_id(&self) -> TypeId { TypeId::of::<T>() }
    fn key(&self) -> ViewKey { View::key(self) }
    fn create_element(&self) -> Box<dyn ElementBehavior> {
        T::create_element(self)
    }
    ...
}
```

**Recommendation:** Option C (blanket impl) with optional derive for customization

### 3. BuildContext Design

**Current State:** BuildContext defined in `flui-foundation`

**Question:** What methods should BuildContext have?

**Minimal API:**
```rust
pub trait BuildContext {
    fn element_id(&self) -> ElementId;
    fn mounted(&self) -> bool;
}
```

**Extended API (Flutter-like):**
```rust
pub trait BuildContext {
    fn element_id(&self) -> ElementId;
    fn mounted(&self) -> bool;
    
    // Inherited data
    fn depend_on<T: InheritedView + 'static>(&self) -> Option<&T>;
    fn get_inherited<T: InheritedView + 'static>(&self) -> Option<&T>;
    
    // Ancestor access
    fn find_ancestor_view<T: View + 'static>(&self) -> Option<&T>;
    fn find_ancestor_state<T: ViewState + 'static>(&self) -> Option<&T>;
    
    // Tree walking
    fn visit_ancestors(&self, visitor: impl FnMut(ElementId) -> bool);
    fn visit_children(&self, visitor: impl FnMut(ElementId));
    
    // Notifications
    fn dispatch_notification<N: Notification>(&self, notification: N);
    
    // Render access
    fn find_render_object(&self) -> Option<RenderId>;
}
```

**Recommendation:** Start with extended API - it's needed for real widgets

### 4. Key System

**Question:** How to implement ViewKey?

```rust
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ViewKey {
    /// No key - position-based identity
    None,
    
    /// Value-based key (hash)
    Value {
        type_id: TypeId,
        hash: u64,
    },
    
    /// Object identity key
    Object(usize),
    
    /// Unique key (always different)
    Unique(UniqueKeyId),
    
    /// Global key (with registry)
    Global(GlobalKeyId),
}
```

**Helper functions:**
```rust
pub fn value_key<T: Hash + 'static>(value: &T) -> ViewKey;
pub fn unique_key() -> ViewKey;
pub fn global_key() -> GlobalKey;
```

### 5. Mixin Strategy: Ambassador (как в flui-rendering)

Используем **ambassador crate** для автоматического делегирования трейтов — тот же подход что в `flui-rendering`.

#### Delegatable Traits

```rust
use ambassador::delegatable_trait;

#[delegatable_trait]
pub trait HasChild {
    fn child(&self) -> &Box<dyn ViewObject>;
    fn child_mut(&mut self) -> &mut Box<dyn ViewObject>;
}

#[delegatable_trait]
pub trait HasChildren {
    fn children(&self) -> &[Box<dyn ViewObject>];
    fn children_mut(&mut self) -> &mut Vec<Box<dyn ViewObject>>;
}

#[delegatable_trait]
pub trait HasState<S: ViewState> {
    fn state(&self) -> &S;
    fn state_mut(&mut self) -> &mut S;
}
```

#### Base Structs

```rust
/// Base for stateless views with single child
#[derive(Debug, Default)]
pub struct StatelessBase {
    child: Option<Box<dyn ViewObject>>,
}

impl HasChild for StatelessBase {
    fn child(&self) -> &Option<Box<dyn ViewObject>> { &self.child }
    fn child_mut(&mut self) -> &mut Option<Box<dyn ViewObject>> { &mut self.child }
}

/// Base for stateful views
#[derive(Debug)]
pub struct StatefulBase<S: ViewState> {
    state: S,
    child: Option<Box<dyn ViewObject>>,
}

impl<S: ViewState> HasState<S> for StatefulBase<S> {
    fn state(&self) -> &S { &self.state }
    fn state_mut(&mut self) -> &mut S { &mut self.state }
}

impl<S: ViewState> HasChild for StatefulBase<S> {
    fn child(&self) -> &Option<Box<dyn ViewObject>> { &self.child }
    fn child_mut(&mut self) -> &mut Option<Box<dyn ViewObject>> { &mut self.child }
}
```

#### Wrapper with Ambassador

```rust
use ambassador::Delegate;

/// Stateless view wrapper — delegates child access
#[derive(Debug, Delegate)]
#[delegate(HasChild, target = "base")]
pub struct StatelessWrapper<T: StatelessViewData> {
    base: StatelessBase,
    pub data: T,
}

impl<T: StatelessViewData> Deref for StatelessWrapper<T> {
    type Target = T;
    fn deref(&self) -> &T { &self.data }
}

/// Stateful view wrapper — delegates state + child
#[derive(Debug, Delegate)]
#[delegate(HasState<T::State>, target = "base")]
#[delegate(HasChild, target = "base")]
pub struct StatefulWrapper<T: StatefulViewData> {
    base: StatefulBase<T::State>,
    pub data: T,
}

impl<T: StatefulViewData> Deref for StatefulWrapper<T> {
    type Target = T;
    fn deref(&self) -> &T { &self.data }
}
```

#### State Mixins via Ambassador

```rust
/// Ticker storage trait
#[delegatable_trait]
pub trait HasTicker {
    fn ticker(&self) -> &Option<TickerHandle>;
    fn ticker_mut(&mut self) -> &mut Option<TickerHandle>;
}

/// Base for state with ticker
pub struct TickerStateBase<S: ViewState> {
    inner: S,
    ticker: Option<TickerHandle>,
}

impl<S: ViewState> HasTicker for TickerStateBase<S> {
    fn ticker(&self) -> &Option<TickerHandle> { &self.ticker }
    fn ticker_mut(&mut self) -> &mut Option<TickerHandle> { &mut self.ticker }
}

/// Mixin trait with default behavior
pub trait TickerProviderMixin: HasTicker {
    fn create_ticker(&mut self, on_tick: impl Fn(Duration) + 'static) -> TickerHandle {
        assert!(self.ticker().is_none(), "SingleTickerProvider can only create one ticker");
        let handle = Ticker::new(on_tick).handle();
        *self.ticker_mut() = Some(handle.clone());
        handle
    }
    
    fn dispose_ticker(&mut self) {
        if let Some(handle) = self.ticker_mut().take() {
            handle.dispose();
        }
    }
}

// Blanket impl
impl<T: HasTicker> TickerProviderMixin for T {}
```

#### Usage Example

```rust
// Define state data
#[derive(Default)]
pub struct AnimatedBoxData {
    animation_value: f32,
}

// State with ticker mixin
#[derive(Delegate)]
#[delegate(HasTicker, target = "base")]
pub struct AnimatedBoxState {
    base: TickerStateBase<()>,
    pub data: AnimatedBoxData,
}

impl Deref for AnimatedBoxState {
    type Target = AnimatedBoxData;
    fn deref(&self) -> &Self::Target { &self.data }
}

impl ViewState for AnimatedBoxState {
    fn init_state(&mut self, ctx: &impl BuildContext) {
        // Use mixin method!
        let ticker = self.create_ticker(|dt| {
            self.data.animation_value += dt.as_secs_f32();
        });
    }
    
    fn dispose(&mut self) {
        self.dispose_ticker();  // Mixin method
    }
}
```

#### Flutter Mixin → FLUI Ambassador Mapping

| Flutter Mixin | FLUI Implementation |
|---------------|---------------------|
| `SingleTickerProviderStateMixin` | `TickerStateBase<S>` + `HasTicker` + `TickerProviderMixin` |
| `TickerProviderStateMixin` | `MultiTickerStateBase<S>` + `HasTickers` + `MultiTickerProviderMixin` |
| `AutomaticKeepAliveClientMixin` | `KeepAliveStateBase<S>` + `HasKeepAlive` + `KeepAliveMixin` |
| `RestorationMixin` | `RestorationStateBase<S>` + `HasRestoration` + `RestorationMixin` |

**Преимущества ambassador подхода:**
- Автоматическое делегирование (нет boilerplate)
- Deref для прямого доступа к полям (`self.animation_value`)
- Composable mixins (можно комбинировать)
- Compile-time проверки
- Единый подход с `flui-rendering`

### 6. IntoElement Trait

**Question:** What can be converted to Element?

```rust
pub trait IntoElement {
    fn into_element(self, parent: ElementId, tree: &mut ElementTree) -> ElementId;
}

// Implementations for:
impl IntoElement for Box<dyn ViewObject> { ... }
impl<V: View> IntoElement for V { ... }
impl<V: View> IntoElement for Option<V> { ... }
impl<V: View, I: Iterator<Item=V>> IntoElement for I { ... }
impl IntoElement for () { ... }  // Empty/null element
```

### 7. Notification System

**Question:** How to implement notification dispatching?

```rust
pub trait Notification: 'static {
    fn should_continue(&self) -> bool { true }
}

pub trait NotificationListener<N: Notification> {
    fn on_notification(&self, notification: &N) -> bool;
}
```

**Usage:**
```rust
impl StatelessView for MyWidget {
    fn build(&self, ctx: &impl BuildContext) -> impl IntoElement {
        NotificationListener::new(
            |notification: &ScrollNotification| {
                // Handle scroll
                true // continue bubbling
            },
            child: MyChild::new(),
        )
    }
}
```

## Proposed Module Structure

```
flui-view/
├── lib.rs
├── view/
│   ├── mod.rs
│   ├── stateless.rs      # StatelessView trait
│   ├── stateful.rs       # StatefulView, ViewState traits
│   ├── render.rs         # RenderView trait
│   ├── proxy.rs          # ProxyView trait
│   └── provider.rs       # ProviderView (InheritedWidget equiv)
├── object.rs             # ViewObject trait
├── key.rs                # ViewKey, GlobalKey, etc.
├── context.rs            # BuildContext trait
├── element/
│   ├── mod.rs
│   ├── behavior.rs       # ElementBehavior trait
│   └── lifecycle.rs      # ElementLifecycle enum
├── notification.rs       # Notification system
└── into_element.rs       # IntoElement trait
```

## Open Questions

1. **Should View require Clone?**
   - Flutter: Widgets don't require Clone
   - FLUI: Currently not required
   - Decision: Keep NOT requiring Clone

2. **Should ViewState be Send + Sync?**
   - Required for parallel builds
   - FLUI is thread-safe by design
   - Decision: YES, require Send + Sync

3. **How to handle View children?**
   - Single child: `child: Box<dyn ViewObject>`
   - Multiple children: `children: Vec<Box<dyn ViewObject>>`
   - Or use associated type?
   - Decision: Defer to concrete implementations

4. **Should BuildContext be a trait or struct?**
   - Trait: More flexible, can mock
   - Struct: Simpler, concrete
   - Decision: Trait for flexibility

## Provider System Design

Flutter's Provider package wraps `InheritedWidget` with ergonomic API. In FLUI we can build this natively with signals.

### Flutter Provider Types → FLUI Mapping

| Flutter Provider | FLUI Equivalent | Notes |
|------------------|-----------------|-------|
| `Provider<T>` | `ProviderView<T>` | Static value |
| `ChangeNotifierProvider<T>` | `ProviderView<Signal<T>>` | Signals are reactive |
| `ValueListenableProvider<T>` | `ProviderView<Signal<T>>` | Same - signals |
| `StreamProvider<T>` | `ProviderView<StreamSignal<T>>` | Stream → Signal adapter |
| `FutureProvider<T>` | `ProviderView<Resource<T>>` | Async resource |
| `ProxyProvider<T, R>` | `ProviderView<Computed<R>>` | Derived value |

### Proposed Provider API

```rust
// 1. Basic provider (static value)
Provider::new(my_service)
    .child(MyApp::new())

// 2. Signal provider (reactive)
SignalProvider::new(use_signal(ctx, 0))
    .child(MyApp::new())

// 3. Multi-provider
MultiProvider::new()
    .provide(Counter::new())
    .provide(ThemeData::dark())
    .provide_signal(user_signal)
    .child(MyApp::new())
```

### Reading Providers

```rust
// With subscription (rebuilds on change)
let counter = ctx.watch::<Counter>();

// Without subscription (no rebuild)  
let counter = ctx.read::<Counter>();

// Selective subscription (only rebuilds when specific value changes)
let count = ctx.select::<Counter, i32>(|c| c.count);
```

### Implementation Sketch

```rust
/// Provider view - wraps InheritedWidget pattern
pub struct Provider<T: 'static> {
    value: T,
    child: Box<dyn ViewObject>,
}

impl<T: 'static + Send + Sync> View for Provider<T> {
    fn key(&self) -> ViewKey { ViewKey::None }
}

impl<T: 'static + Send + Sync> ProviderView<T> for Provider<T> {
    fn value(&self) -> &T { &self.value }
    
    fn update_should_notify(&self, old: &T) -> bool {
        // For static values, always false (immutable)
        // For Signal<T>, check if signal changed
        false
    }
}

/// BuildContext extensions for provider access
pub trait ProviderContext {
    /// Get value WITH dependency (rebuilds when provider updates)
    fn watch<T: 'static>(&self) -> Option<&T>;
    
    /// Get value WITHOUT dependency (no rebuild)
    fn read<T: 'static>(&self) -> Option<&T>;
    
    /// Selective dependency on derived value
    fn select<T: 'static, R: PartialEq + 'static>(
        &self, 
        selector: impl Fn(&T) -> R
    ) -> Option<R>;
}
```

### Signal Integration

Since FLUI has first-class signals, providers work naturally:

```rust
// Provider for Signal<Counter>
SignalProvider::new(use_signal(ctx, Counter::new()))
    .child(
        // Consumer automatically rebuilds when signal changes
        Consumer::<Signal<Counter>>::new(|counter| {
            Text::new(format!("Count: {}", counter.get().value))
        })
    )
```

### Consumer Widget

```rust
/// Widget that rebuilds when provider changes
pub struct Consumer<T: 'static> {
    builder: Box<dyn Fn(&T) -> Box<dyn ViewObject>>,
}

impl<T: 'static> Consumer<T> {
    pub fn new(builder: impl Fn(&T) -> impl IntoElement + 'static) -> Self {
        Self { builder: Box::new(move |v| Box::new(builder(v))) }
    }
}

impl<T: 'static> StatelessView for Consumer<T> {
    fn build(&self, ctx: &impl BuildContext) -> impl IntoElement {
        let value = ctx.watch::<T>().expect("Provider<T> not found");
        (self.builder)(value)
    }
}
```

### Scoped Providers (Override)

```rust
// Override provider for subtree
Provider::new(ThemeData::light())
    .child(
        Column::new()
            .child(LightThemedWidget::new())  // Uses light theme
            .child(
                Provider::new(ThemeData::dark())
                    .child(DarkThemedWidget::new())  // Uses dark theme (override)
            )
    )
```

### Aspect-Based Providers (like InheritedModel)

```rust
/// Provider with aspect-based subscriptions
pub struct AspectProvider<T, A> {
    value: T,
    _aspect: PhantomData<A>,
    child: Box<dyn ViewObject>,
}

impl<T, A> ProviderView<T> for AspectProvider<T, A> {
    fn update_should_notify_dependent(&self, old: &T, aspects: &HashSet<A>) -> bool {
        // Check if specific aspects changed
    }
}

// Usage
let user = ctx.watch_aspect::<User, &str>("name");  // Only rebuilds on name change
```

## Implementation Priority

1. **Phase 1: Core Traits**
   - [ ] `View` base trait
   - [ ] `StatelessView` trait
   - [ ] `StatefulView` and `ViewState` traits
   - [ ] `ViewObject` with blanket impl

2. **Phase 2: Key System**
   - [ ] `ViewKey` enum
   - [ ] `GlobalKeyRegistry`
   - [ ] Helper functions

3. **Phase 3: Context**
   - [ ] `BuildContext` trait
   - [ ] Inherited widget lookup
   - [ ] Ancestor/descendant access

4. **Phase 4: Render Integration**
   - [ ] `RenderView` trait
   - [ ] Protocol/Arity integration

5. **Phase 5: Advanced**
   - [ ] `ProxyView` and `ProviderView`
   - [ ] Notification system
   - [ ] State mixins/hooks
