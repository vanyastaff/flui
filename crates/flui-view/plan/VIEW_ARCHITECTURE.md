# View Architecture (flui-view)

Документация по архитектуре View слоя FLUI, основанная на Flutter Widget system.

---

## Flutter Widget Hierarchy

### Core Widget Class

```dart
@immutable
abstract class Widget extends DiagnosticableTree implements HitTestTarget {
  const Widget({ this.key });
  
  final Key? key;
  
  @protected
  Element createElement();
  
  static bool canUpdate(Widget oldWidget, Widget newWidget) {
    return oldWidget.runtimeType == newWidget.runtimeType
        && oldWidget.key == newWidget.key;
  }
}
```

**Ключевые принципы:**
- `@immutable` — все поля должны быть `final`
- `key` — контролирует замену widget в дереве
- `createElement()` — создаёт Element для этого Widget
- `canUpdate()` — определяет можно ли обновить Element или нужно создать новый

### Widget Types in Flutter

| Widget Type | Description | Element Type |
|-------------|-------------|--------------|
| `StatelessWidget` | Immutable, no state | `StatelessElement` |
| `StatefulWidget` | Has mutable State | `StatefulElement` |
| `RenderObjectWidget` | Creates RenderObject | `RenderObjectElement` |
| `ProxyWidget` | Wraps single child | `ProxyElement` |
| `InheritedWidget` | Provides data to descendants | `InheritedElement` |
| `ParentDataWidget` | Configures child's parent data | `ParentDataElement` |

---

## Flutter → FLUI Mapping

| Flutter | FLUI | Notes |
|---------|------|-------|
| `Widget` | `View` (concept) | Immutable UI configuration |
| `StatelessWidget` | `StatelessView` | Views without state |
| `StatefulWidget` | `StatefulView` | Views with mutable state |
| `State<T>` | `ViewState` | State associated with StatefulView |
| `RenderObjectWidget` | `RenderView<P, A>` | Views that create render objects |
| `SingleChildRenderObjectWidget` | `RenderView<P, Single>` | Single child render view |
| `MultiChildRenderObjectWidget` | `RenderView<P, Variable>` | Multi-child render view |
| `LeafRenderObjectWidget` | `RenderView<P, Leaf>` | No children render view |
| `ProxyWidget` | `ProxyView` | Wraps single child |
| `InheritedWidget` | `ProviderView<T>` | Provides data to descendants |
| `Element` | `ViewElement` | Mutable instance in tree |
| `BuildContext` | `BuildContext` | Context during build |
| `Key` | `Key`, `ViewKey` | Identity for reconciliation |

---

## StatelessWidget → StatelessView

### Flutter StatelessWidget

```dart
abstract class StatelessWidget extends Widget {
  const StatelessWidget({ super.key });
  
  @override
  StatelessElement createElement() => StatelessElement(this);
  
  @protected
  Widget build(BuildContext context);
}
```

**When to use:**
- UI depends only on configuration (constructor arguments)
- No need to persist state between rebuilds
- Lightweight, frequently rebuilt components

**Build called when:**
1. First time widget is inserted into tree
2. Parent reconfigures widget
3. Dependent InheritedWidget changes

### FLUI StatelessView

```rust
pub trait StatelessView: Send + Sync + 'static {
    fn build(self, ctx: &dyn BuildContext) -> impl IntoView;
}
```

**Отличия от Flutter:**
- `self` consumed (moved) — Rust ownership
- Returns `impl IntoView` — type erasure через trait
- `Send + Sync + 'static` — thread safety requirements

---

## StatefulWidget → StatefulView

### Flutter StatefulWidget + State

```dart
abstract class StatefulWidget extends Widget {
  const StatefulWidget({ super.key });
  
  @override
  StatefulElement createElement() => StatefulElement(this);
  
  @protected
  State createState();
}

abstract class State<T extends StatefulWidget> {
  T get widget => _widget!;
  BuildContext get context => _element!;
  bool get mounted => _element != null;
  
  // Lifecycle
  void initState() {}
  void didChangeDependencies() {}
  Widget build(BuildContext context);
  void didUpdateWidget(T oldWidget) {}
  void deactivate() {}
  void activate() {}
  void dispose() {}
  
  // Trigger rebuild
  void setState(VoidCallback fn) {
    fn();
    _element!.markNeedsBuild();
  }
}
```

**Lifecycle order:**
1. `createState()` — create State object
2. `initState()` — initialize (subscriptions, etc.)
3. `didChangeDependencies()` — inherited widget changed
4. `build()` — build UI
5. `didUpdateWidget()` — widget config changed
6. `deactivate()` — temporarily removed
7. `activate()` — reinserted after deactivate
8. `dispose()` — permanently removed

### FLUI StatefulView

```rust
pub trait StatefulView: Send + Sync + 'static {
    type State: ViewState;
    
    fn create_state(&self) -> Self::State;
    
    fn init_state(&self, state: &mut Self::State, ctx: &dyn BuildContext) {}
    fn did_change_dependencies(&self, state: &mut Self::State, ctx: &dyn BuildContext) {}
    fn build(&self, state: &mut Self::State, ctx: &dyn BuildContext) -> impl IntoView;
    fn did_update_view(&self, state: &mut Self::State, old_view: &Self) {}
    fn deactivate(&self, state: &mut Self::State, ctx: &dyn BuildContext) {}
    fn activate(&self, state: &mut Self::State, ctx: &dyn BuildContext) {}
    fn dispose(&self, state: &mut Self::State, ctx: &dyn BuildContext) {}
}
```

**Отличия от Flutter:**
- `State` как associated type
- View не consumed в `build()` — `&self` reference
- State passed explicitly — `&mut Self::State`
- No `setState()` — use Signals or `ctx.mark_dirty()`

---

## RenderObjectWidget → RenderView

### Flutter RenderObjectWidget

```dart
abstract class RenderObjectWidget extends Widget {
  const RenderObjectWidget({ super.key });
  
  @override
  RenderObjectElement createElement();
  
  @protected
  RenderObject createRenderObject(BuildContext context);
  
  @protected
  void updateRenderObject(BuildContext context, RenderObject renderObject) {}
  
  @protected
  void didUnmountRenderObject(RenderObject renderObject) {}
}
```

**Subclasses:**
- `LeafRenderObjectWidget` — no children
- `SingleChildRenderObjectWidget` — single child
- `MultiChildRenderObjectWidget` — multiple children

### FLUI RenderView

```rust
pub trait RenderView<P: Protocol, A: Arity>: Send + Sync + 'static {
    type RenderObject: RenderObjectFor<P, A>;
    
    fn create(&self) -> Self::RenderObject;
    
    fn update(&self, render: &mut Self::RenderObject) -> UpdateResult {
        UpdateResult::Unchanged
    }
    
    fn dispose(&self, render: &mut Self::RenderObject) {}
}

pub enum UpdateResult {
    Unchanged,
    NeedsLayout,
    NeedsPaint,
}
```

**Arity variants (compile-time child count):**
- `Leaf` — no children (like `LeafRenderObjectWidget`)
- `Single` — exactly one child (like `SingleChildRenderObjectWidget`)
- `Optional` — zero or one child
- `Variable` — any number of children (like `MultiChildRenderObjectWidget`)

**Protocol variants:**
- `BoxProtocol` — 2D rectangular layout (BoxConstraints → Size)
- `SliverProtocol` — scrollable layout (SliverConstraints → SliverGeometry)

---

## ProxyWidget → ProxyView

### Flutter ProxyWidget

```dart
abstract class ProxyWidget extends Widget {
  const ProxyWidget({ super.key, required this.child });
  
  final Widget child;
}
```

**Purpose:**
- Wrap single child without changing layout
- Add behavior, metadata, or event handling
- Base class for `InheritedWidget`, `ParentDataWidget`

### FLUI ProxyView

```rust
pub trait ProxyView: Send + Sync + 'static {
    fn build_child(&mut self, ctx: &dyn BuildContext) -> impl IntoView;
    
    // Lifecycle
    fn init(&mut self, ctx: &dyn BuildContext) {}
    fn did_change_dependencies(&mut self, ctx: &dyn BuildContext) {}
    fn before_child_build(&mut self, ctx: &dyn BuildContext) {}
    fn after_child_build(&mut self, ctx: &dyn BuildContext) {}
    fn deactivate(&mut self, ctx: &dyn BuildContext) {}
    fn activate(&mut self, ctx: &dyn BuildContext) {}
    fn dispose(&mut self, ctx: &dyn BuildContext) {}
    
    // Event handling
    fn handle_event(&mut self, event: &Event, ctx: &dyn BuildContext) -> bool {
        false // Don't block events
    }
}
```

**Use cases:**
- Event interception (`IgnorePointer`, `GestureDetector`)
- Accessibility (`Semantics`)
- Focus management (`Focus`, `FocusScope`)
- Optimization hints (`RepaintBoundary`)

---

## InheritedWidget → ProviderView

### Flutter InheritedWidget

```dart
abstract class InheritedWidget extends ProxyWidget {
  const InheritedWidget({ super.key, required super.child });
  
  @override
  InheritedElement createElement() => InheritedElement(this);
  
  @protected
  bool updateShouldNotify(covariant InheritedWidget oldWidget);
}

// Usage in descendants:
Theme.of(context);  // calls context.dependOnInheritedWidgetOfExactType<Theme>()
```

**Dependency tracking:**
- `context.dependOnInheritedWidgetOfExactType<T>()` registers dependency
- When inherited widget updates, dependents rebuild
- `updateShouldNotify()` controls whether to notify

### FLUI ProviderView

```rust
pub trait ProviderView<T: Send + Sync + 'static>: Send + Sync + 'static {
    fn build(&mut self, ctx: &dyn BuildContext) -> impl IntoView;
    
    fn value(&self) -> Arc<T>;
    
    fn should_notify(&self, old_value: &T) -> bool {
        true // Default: always notify
    }
    
    // Lifecycle
    fn init(&mut self, ctx: &dyn BuildContext) {}
    fn did_change_dependencies(&mut self, ctx: &dyn BuildContext) {}
    fn deactivate(&mut self, ctx: &dyn BuildContext) {}
    fn activate(&mut self, ctx: &dyn BuildContext) {}
    fn dispose(&mut self, ctx: &dyn BuildContext) {}
}

// Usage in descendants:
let theme = ctx.depend_on::<Theme>().expect("ThemeProvider not found");
```

---

## ViewObject — Dynamic Dispatch

### Purpose

`ViewObject` is the type-erased interface stored in `Element`:

```rust
pub struct Element {
    view_object: Box<dyn ViewObject>,
    // ...
}
```

### Trait Definition

```rust
pub trait ViewObject: Send + Sync + 'static {
    // Core
    fn mode(&self) -> ViewMode;
    fn build(&mut self, ctx: &dyn BuildContext) -> Option<Box<dyn ViewObject>>;
    
    // Lifecycle
    fn init(&mut self, ctx: &dyn BuildContext) {}
    fn did_change_dependencies(&mut self, ctx: &dyn BuildContext) {}
    fn did_update(&mut self, old_view: &dyn Any, ctx: &dyn BuildContext) {}
    fn deactivate(&mut self, ctx: &dyn BuildContext) {}
    fn activate(&mut self, ctx: &dyn BuildContext) {}
    fn dispose(&mut self, ctx: &dyn BuildContext) {}
    
    // Render state (for render views)
    fn render_state(&self) -> Option<&dyn Any> { None }
    fn render_state_mut(&mut self) -> Option<&mut dyn Any> { None }
    
    // Provider methods
    fn provided_value(&self) -> Option<Arc<dyn Any + Send + Sync>> { None }
    fn dependents(&self) -> &[ElementId] { &[] }
    fn dependents_mut(&mut self) -> Option<&mut Vec<ElementId>> { None }
    fn should_notify_dependents(&self, old_value: &dyn Any) -> bool { false }
    
    // Downcasting
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    
    // Debug
    fn debug_name(&self) -> &'static str { "ViewObject" }
}
```

### ViewMode Enum

```rust
pub enum ViewMode {
    Stateless,
    Stateful,
    Proxy,
    Animated,
    Provider,
    RenderBox,
    RenderSliver,
}
```

---

## Key System

### Flutter Key

```dart
@immutable
abstract class Key {
  const factory Key(String value) = ValueKey<String>;
  const Key.empty();
}

class ValueKey<T> extends LocalKey { ... }
class ObjectKey extends LocalKey { ... }
class UniqueKey extends LocalKey { ... }
class GlobalKey<T extends State> extends Key { ... }
```

**Purpose:**
- Control when framework reuses vs recreates Elements
- `canUpdate()` checks `runtimeType` AND `key`
- GlobalKey enables cross-subtree state preservation

### FLUI Key

```rust
pub enum Key {
    Value(ValueKey),
    Object(ObjectKey),
    Unique(UniqueKey),
    Global(GlobalKey),
}

pub struct ValueKey(pub u64);  // Hash of value
pub struct ObjectKey(pub u64); // Pointer-based identity
pub struct UniqueKey(pub u64); // Auto-generated unique
pub struct GlobalKey(pub u64); // Cross-subtree identity

pub trait WithKey: Sized {
    fn with_key(self, key: impl Into<Key>) -> Keyed<Self>;
}
```

---

## IntoView Trait

### Purpose

Convert any view type to `Box<dyn ViewObject>`:

```rust
pub trait IntoView {
    fn into_view(self) -> Box<dyn ViewObject>;
}
```

### Implementations

```rust
// For StatelessView
impl<V: StatelessView> IntoView for V {
    fn into_view(self) -> Box<dyn ViewObject> {
        Box::new(StatelessViewWrapper::new(self))
    }
}

// For StatefulView
impl<V: StatefulView> IntoView for V {
    fn into_view(self) -> Box<dyn ViewObject> {
        Box::new(StatefulViewWrapper::new(self))
    }
}

// For RenderView
impl<P: Protocol, A: Arity, V: RenderView<P, A>> IntoView for V {
    fn into_view(self) -> Box<dyn ViewObject> {
        Box::new(RenderViewWrapper::new(self))
    }
}
```

---

## Wrappers

Each View trait has a corresponding Wrapper that implements `ViewObject`:

| View Trait | Wrapper | ViewMode |
|------------|---------|----------|
| `StatelessView` | `StatelessViewWrapper` | `Stateless` |
| `StatefulView` | `StatefulViewWrapper` | `Stateful` |
| `ProxyView` | `ProxyViewWrapper` | `Proxy` |
| `AnimatedView` | `AnimatedViewWrapper` | `Animated` |
| `ProviderView<T>` | `ProviderViewWrapper<T>` | `Provider` |
| `RenderView<BoxProtocol, A>` | `RenderViewWrapper` | `RenderBox` |
| `RenderView<SliverProtocol, A>` | `RenderViewWrapper` | `RenderSliver` |

---

## BuildContext

### Flutter BuildContext

```dart
abstract class BuildContext {
  Widget get widget;
  bool get mounted;
  
  // Inherited widgets
  T? dependOnInheritedWidgetOfExactType<T extends InheritedWidget>();
  T? findAncestorWidgetOfExactType<T extends Widget>();
  T? findAncestorStateOfType<T extends State>();
  T? findAncestorRenderObjectOfType<T extends RenderObject>();
  
  // Size (after layout)
  Size? get size;
}
```

### FLUI BuildContext

```rust
pub trait BuildContext: Send + Sync {
    // Identity
    fn element_id(&self) -> ElementId;
    
    // Inherited data
    fn depend_on<T: Send + Sync + 'static>(&self) -> Option<Arc<T>>;
    fn find_ancestor<T: 'static>(&self) -> Option<&T>;
    
    // Dirty marking
    fn mark_dirty(&self);
    
    // Debug
    fn debug_path(&self) -> String;
}
```

---

## Lifecycle Comparison

### Flutter Widget Lifecycle

```
Widget created (immutable)
    ↓
Element.mount()
    ↓
State.initState() [StatefulWidget only]
    ↓
State.didChangeDependencies()
    ↓
State.build() / Widget.build()
    ↓ (on update)
State.didUpdateWidget()
    ↓ (on rebuild)
State.build()
    ↓ (on deactivate)
State.deactivate()
    ↓ (on reactivate)
State.activate()
    ↓ (on unmount)
State.dispose()
```

### FLUI View Lifecycle

```
View created (immutable struct)
    ↓
ViewObject.init()
    ↓
ViewObject.did_change_dependencies()
    ↓
ViewObject.build()
    ↓ (on update)
ViewObject.did_update()
    ↓ (on rebuild)
ViewObject.build()
    ↓ (on deactivate)
ViewObject.deactivate()
    ↓ (on reactivate)
ViewObject.activate()
    ↓ (on unmount)
ViewObject.dispose()
```

---

## Current File Structure

```
crates/flui-view/src/
├── lib.rs                 # Module exports
├── context.rs             # BuildContext trait
├── state.rs               # ViewState trait
├── view_mode.rs           # ViewMode enum
├── view_object.rs         # ViewObject trait
├── into_view.rs           # IntoView trait
├── into_view_config.rs    # IntoViewConfig trait
├── empty.rs               # EmptyView
├── handle.rs              # ViewHandle, ViewConfig
│
├── traits/
│   ├── mod.rs
│   ├── stateless.rs       # StatelessView trait
│   ├── stateful.rs        # StatefulView trait
│   ├── render.rs          # RenderView trait
│   ├── proxy.rs           # ProxyView trait
│   ├── provider.rs        # ProviderView trait
│   ├── animated.rs        # AnimatedView trait
│   └── update_result.rs   # UpdateResult enum
│
├── wrappers/
│   ├── mod.rs
│   ├── stateless.rs       # StatelessViewWrapper
│   ├── stateful.rs        # StatefulViewWrapper
│   ├── proxy.rs           # ProxyViewWrapper
│   ├── provider.rs        # ProviderViewWrapper
│   ├── animated.rs        # AnimatedViewWrapper
│   └── render.rs          # RenderViewWrapper
│
├── children/
│   ├── mod.rs
│   ├── child.rs           # Child<V>
│   └── children.rs        # Children<V>
│
├── element/
│   ├── mod.rs
│   ├── view_element.rs    # ViewElement
│   ├── lifecycle.rs       # ViewLifecycle
│   └── flags.rs           # ViewFlags, AtomicViewFlags
│
└── tree/
    ├── mod.rs
    ├── view_tree.rs       # ViewTree
    └── snapshot.rs        # TreeSnapshot
```

---

## Sources

- [Flutter Widget Class](https://api.flutter.dev/flutter/widgets/Widget-class.html)
- [Flutter StatelessWidget](https://api.flutter.dev/flutter/widgets/StatelessWidget-class.html)
- [Flutter StatefulWidget](https://api.flutter.dev/flutter/widgets/StatefulWidget-class.html)
- [Flutter RenderObjectWidget](https://api.flutter.dev/flutter/widgets/RenderObjectWidget-class.html)
- [Flutter InheritedWidget](https://api.flutter.dev/flutter/widgets/InheritedWidget-class.html)
- [Flutter ProxyWidget](https://api.flutter.dev/flutter/widgets/ProxyWidget-class.html)
- [Flutter Architectural Overview](https://docs.flutter.dev/resources/architectural-overview)
