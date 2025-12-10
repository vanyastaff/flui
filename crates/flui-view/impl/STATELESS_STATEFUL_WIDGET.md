# StatelessWidget and StatefulWidget

This document analyzes Flutter's StatelessWidget and StatefulWidget - the two primary ways to define composable UI in Flutter.

## Source Files
- `packages/flutter/lib/src/widgets/framework.dart`

## Widget Hierarchy

```
Widget (abstract)
├── StatelessWidget    - Immutable, no state
├── StatefulWidget     - Creates State object
├── ProxyWidget        - Passes child through (InheritedWidget, ParentDataWidget)
└── RenderObjectWidget - Creates RenderObject
```

**Key Distinction:**
- `StatelessWidget` / `StatefulWidget` - **compose** other widgets (no RenderObject)
- `RenderObjectWidget` - **create** RenderObjects for actual rendering

---

## StatelessWidget

### Definition

```dart
abstract class StatelessWidget extends Widget {
  const StatelessWidget({super.key});

  @override
  StatelessElement createElement() => StatelessElement(this);

  /// Build method - called when widget is inserted or dependencies change
  @protected
  Widget build(BuildContext context);
}
```

### When build() is called

1. First time widget is inserted into tree
2. Parent rebuilds with new configuration
3. InheritedWidget dependency changes

### Usage Example

```dart
class GreenFrog extends StatelessWidget {
  const GreenFrog({super.key});

  @override
  Widget build(BuildContext context) {
    return Container(color: const Color(0xFF2DBD3A));
  }
}

// With parameters
class Frog extends StatelessWidget {
  const Frog({
    super.key,
    this.color = const Color(0xFF2DBD3A),
    this.child,
  });

  final Color color;
  final Widget? child;

  @override
  Widget build(BuildContext context) {
    return ColoredBox(color: color, child: child);
  }
}
```

### StatelessElement

```dart
class StatelessElement extends ComponentElement {
  StatelessElement(StatelessWidget super.widget);

  @override
  Widget build() => (widget as StatelessWidget).build(this);

  @override
  void update(StatelessWidget newWidget) {
    super.update(newWidget);
    assert(widget == newWidget);
    rebuild(force: true);
  }
}
```

**Key Points:**
- `build()` delegates to widget's `build()`
- `update()` is called when parent provides new widget with same runtimeType+key
- Always triggers `rebuild(force: true)` on update

### Performance Tips

1. **Use `const` constructors** - enables widget reuse
2. **Minimize nodes** - fewer widgets = faster rebuilds
3. **Push state to leaves** - only rebuild what changes
4. **Avoid helper methods** - prefer separate widgets (better rebuild granularity)
5. **Split by InheritedWidget usage** - isolate dependency-triggered rebuilds

---

## StatefulWidget

### Definition

```dart
abstract class StatefulWidget extends Widget {
  const StatefulWidget({super.key});

  @override
  StatefulElement createElement() => StatefulElement(this);

  /// Creates the mutable State for this widget
  @protected
  @factory
  State createState();
}
```

**Important:** StatefulWidget itself is **immutable**. Mutable state lives in the `State` object.

### State Class

```dart
@optionalTypeArgs
abstract class State<T extends StatefulWidget> with Diagnosticable {
  /// The current widget configuration
  T get widget => _widget!;
  T? _widget;

  /// The BuildContext for this State
  BuildContext get context => _element!;
  StatefulElement? _element;

  /// Whether this State is currently in the tree
  bool get mounted => _element != null;

  // === Lifecycle Methods ===

  /// Called once when State is created
  @protected
  @mustCallSuper
  void initState() {}

  /// Called when widget configuration changes
  @mustCallSuper
  @protected
  void didUpdateWidget(covariant T oldWidget) {}

  /// Called when InheritedWidget dependency changes
  @protected
  @mustCallSuper
  void didChangeDependencies() {}

  /// Trigger a rebuild
  @protected
  void setState(VoidCallback fn) {
    // fn is called synchronously
    fn();
    _element!.markNeedsBuild();
  }

  /// Build the widget tree
  @protected
  Widget build(BuildContext context);

  /// Called when removed from tree (may be reinserted)
  @protected
  @mustCallSuper
  void deactivate() {}

  /// Called if reinserted after deactivate
  @protected
  @mustCallSuper
  void activate() {}

  /// Called when permanently removed - release resources
  @protected
  @mustCallSuper
  void dispose() {}

  /// Called during hot reload
  @protected
  @mustCallSuper
  void reassemble() {}
}
```

### State Lifecycle

```
┌─────────────────────────────────────────────────────────────────┐
│                        LIFECYCLE                                 │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  createState()                                                   │
│       │                                                          │
│       ▼                                                          │
│  ┌─────────┐                                                     │
│  │ created │  ─── initState() ───►  ┌─────────────┐              │
│  └─────────┘                        │ initialized │              │
│                                     └─────────────┘              │
│                                           │                      │
│                        didChangeDependencies()                   │
│                                           │                      │
│                                           ▼                      │
│                                     ┌─────────┐                  │
│                              ┌─────►│  ready  │◄─────┐           │
│                              │      └─────────┘      │           │
│                              │           │           │           │
│                         activate()       │      setState()       │
│                              │           │    didUpdateWidget()  │
│                              │           │    didChangeDeps()    │
│                              │           │           │           │
│                              │      build()          │           │
│                              │           │           │           │
│                              │           ▼           │           │
│                              │      [subtree]────────┘           │
│                              │                                   │
│                              │                                   │
│                        ┌──────────┐                              │
│                        │ inactive │ ◄── deactivate()             │
│                        └──────────┘                              │
│                              │                                   │
│                   (reinserted?)                                  │
│                      │       │                                   │
│                     YES      NO                                  │
│                      │       │                                   │
│                      │       ▼                                   │
│                      │  ┌─────────┐                              │
│                      │  │ defunct │ ◄── dispose()                │
│                      │  └─────────┘                              │
│                      │                                           │
│                      └──────────────────────────────────────────►│
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### _StateLifecycle Enum (Internal)

```dart
enum _StateLifecycle {
  created,      // After createState(), before initState()
  initialized,  // After initState(), before didChangeDependencies()
  ready,        // After didChangeDependencies(), can build
  defunct,      // After dispose(), cannot use
}
```

### Usage Example

```dart
class Bird extends StatefulWidget {
  const Bird({
    super.key,
    this.color = const Color(0xFFFFE306),
    this.child,
  });

  final Color color;
  final Widget? child;

  @override
  State<Bird> createState() => _BirdState();
}

class _BirdState extends State<Bird> {
  double _size = 1.0;

  void grow() {
    setState(() { _size += 0.1; });
  }

  @override
  Widget build(BuildContext context) {
    return Container(
      color: widget.color,  // Access widget config
      transform: Matrix4.diagonal3Values(_size, _size, 1.0),
      child: widget.child,
    );
  }
}
```

### StatefulElement

```dart
class StatefulElement extends ComponentElement {
  StatefulElement(StatefulWidget widget) 
      : _state = widget.createState(), 
        super(widget) {
    // Verify State type matches Widget
    assert(state._debugTypesAreRight(widget));
    // Associate State with this Element
    state._element = this;
    state._widget = widget;
  }

  State<StatefulWidget> get state => _state!;
  State<StatefulWidget>? _state;

  @override
  Widget build() => state.build(this);

  @override
  void _firstBuild() {
    // 1. Call initState
    state.initState();
    // 2. Call didChangeDependencies
    state.didChangeDependencies();
    // 3. First build
    super._firstBuild();
  }

  @override
  void performRebuild() {
    // Call didChangeDependencies if needed
    if (_didChangeDependencies) {
      state.didChangeDependencies();
      _didChangeDependencies = false;
    }
    super.performRebuild();
  }

  @override
  void update(StatefulWidget newWidget) {
    super.update(newWidget);
    final oldWidget = state._widget!;
    state._widget = widget as StatefulWidget;
    // Call didUpdateWidget with old widget
    state.didUpdateWidget(oldWidget);
    rebuild(force: true);
  }

  @override
  void activate() {
    super.activate();
    state.activate();
    markNeedsBuild();  // Rebuild after reactivation
  }

  @override
  void deactivate() {
    state.deactivate();
    super.deactivate();
  }

  @override
  void unmount() {
    super.unmount();
    state.dispose();
    state._element = null;
    state._widget = null;
  }
}
```

### setState() Details

```dart
void setState(VoidCallback fn) {
  // Error checks:
  // - Can't call after dispose (defunct)
  // - Can't call in constructor (before mount)
  // - Can't be async (no Future return)
  
  final result = fn();  // Synchronous!
  assert(result is! Future);
  
  _element!.markNeedsBuild();
}
```

**Why callback instead of just `markNeedsBuild()`?**
- Forces developers to think about what state is changing
- Prevents "good luck charm" usage (calling it "just in case")
- Results in better performance understanding

---

## ComponentElement (Base Class)

```dart
abstract class ComponentElement extends Element {
  ComponentElement(super.widget);

  Element? _child;  // Single child (the build() result)

  @override
  void mount(Element? parent, Object? newSlot) {
    super.mount(parent, newSlot);
    _firstBuild();
  }

  void _firstBuild() {
    rebuild();
  }

  @override
  void performRebuild() {
    Widget built;
    try {
      built = build();  // Call subclass build()
    } catch (e, stack) {
      built = ErrorWidget.builder(...);
    } finally {
      super.performRebuild();  // Clears dirty flag
    }
    
    _child = updateChild(_child, built, slot);
  }

  /// Subclasses implement to call widget.build() or state.build()
  @protected
  Widget build();

  @override
  void visitChildren(ElementVisitor visitor) {
    if (_child != null) visitor(_child!);
  }
}
```

**Key Points:**
- ComponentElement has exactly **one child** (the build result)
- `performRebuild()` calls `build()` then `updateChild()`
- Error handling wraps build in try-catch, shows ErrorWidget on failure

---

## FLUI Design

### StatelessView

```rust
/// Immutable view that builds from configuration
pub trait StatelessView: View {
    fn build(&self, ctx: &mut BuildContext) -> impl IntoElement;
}

// Most views are stateless - just implement View
pub struct GreenFrog;

impl View for GreenFrog {
    fn build(&self, ctx: &mut BuildContext) -> impl IntoElement {
        Container::new().color(Color::GREEN)
    }
}

// With parameters
pub struct Frog {
    pub color: Color,
    pub child: Option<Box<dyn View>>,
}

impl View for Frog {
    fn build(&self, ctx: &mut BuildContext) -> impl IntoElement {
        ColoredBox::new(self.color)
            .child(self.child.clone())
    }
}
```

### StatefulView (Hooks-based approach)

FLUI uses hooks instead of separate State class:

```rust
/// View that uses hooks for state
fn counter(ctx: &mut BuildContext) -> impl View {
    let count = use_signal(ctx, 0);
    
    Column::new()
        .children([
            Text::new(format!("Count: {}", count.get())),
            Button::new("Increment")
                .on_click(move || count.set(count.get() + 1)),
        ])
}
```

### Traditional StatefulView (Flutter-like)

For complex cases or Flutter familiarity:

```rust
/// Marker trait for stateful views
pub trait StatefulView: View {
    type State: ViewState;
    
    fn create_state(&self) -> Self::State;
}

/// Mutable state for a view
pub trait ViewState: 'static {
    type View: StatefulView<State = Self>;
    
    fn init_state(&mut self) {}
    fn did_update_view(&mut self, old_view: &Self::View) {}
    fn did_change_dependencies(&mut self) {}
    fn build(&mut self, ctx: &mut BuildContext, view: &Self::View) -> impl IntoElement;
    fn deactivate(&mut self) {}
    fn dispose(&mut self) {}
}

// Usage
pub struct Bird {
    pub color: Color,
    pub child: Option<Box<dyn View>>,
}

impl StatefulView for Bird {
    type State = BirdState;
    
    fn create_state(&self) -> BirdState {
        BirdState { size: 1.0 }
    }
}

pub struct BirdState {
    size: f64,
}

impl ViewState for BirdState {
    type View = Bird;
    
    fn build(&mut self, ctx: &mut BuildContext, view: &Bird) -> impl IntoElement {
        Container::new()
            .color(view.color)
            .transform(Matrix4::scale(self.size, self.size, 1.0))
            .child(view.child.clone())
    }
}

impl BirdState {
    pub fn grow(&mut self, ctx: &mut BuildContext) {
        self.size += 0.1;
        ctx.mark_needs_build();
    }
}
```

### Element Implementation

```rust
/// Element for stateless views
pub struct StatelessElement {
    view: Box<dyn View>,
    child: Option<ElementId>,
}

impl Element for StatelessElement {
    fn build(&self, ctx: &mut BuildContext) -> Box<dyn IntoElement> {
        self.view.build(ctx)
    }
    
    fn update(&mut self, new_view: Box<dyn View>, ctx: &mut UpdateContext) {
        self.view = new_view;
        ctx.rebuild(force: true);
    }
}

/// Element for stateful views
pub struct StatefulElement<V: StatefulView> {
    view: V,
    state: V::State,
    child: Option<ElementId>,
}

impl<V: StatefulView> StatefulElement<V> {
    fn new(view: V) -> Self {
        let state = view.create_state();
        Self { view, state, child: None }
    }
}

impl<V: StatefulView> Element for StatefulElement<V> {
    fn mount(&mut self, ctx: &mut MountContext) {
        self.state.init_state();
        self.state.did_change_dependencies();
        // First build
    }
    
    fn build(&mut self, ctx: &mut BuildContext) -> Box<dyn IntoElement> {
        self.state.build(ctx, &self.view)
    }
    
    fn update(&mut self, new_view: Box<dyn View>, ctx: &mut UpdateContext) {
        let old_view = std::mem::replace(&mut self.view, *new_view.downcast().unwrap());
        self.state.did_update_view(&old_view);
        ctx.rebuild(force: true);
    }
    
    fn deactivate(&mut self) {
        self.state.deactivate();
    }
    
    fn unmount(&mut self) {
        self.state.dispose();
    }
}
```

### Recommended: Hooks Approach

```rust
// Hooks are simpler for most cases
fn bird(ctx: &mut BuildContext, color: Color, child: Option<Box<dyn View>>) -> impl View {
    let size = use_signal(ctx, 1.0);
    
    Container::new()
        .color(color)
        .transform(Matrix4::scale(size.get(), size.get(), 1.0))
        .child(child)
}

// With lifecycle hooks
fn animated_bird(ctx: &mut BuildContext) -> impl View {
    let controller = use_animation_controller(ctx, Duration::from_millis(500));
    
    // initState equivalent
    use_effect(ctx, || {
        controller.forward();
        // dispose equivalent
        move || controller.dispose()
    }, []);
    
    // didUpdateWidget equivalent via dependency tracking
    use_effect(ctx, || {
        // React to prop changes
    }, [some_prop]);
    
    ScaleTransition::new(controller, FlutterLogo::new())
}
```

---

## Comparison

| Aspect | Flutter | FLUI (Hooks) | FLUI (Traditional) |
|--------|---------|--------------|-------------------|
| Stateless | `StatelessWidget` | `impl View` | `impl View` |
| Stateful | `StatefulWidget` + `State` | Hooks | `StatefulView` + `ViewState` |
| State storage | `State` class | Signals | `ViewState` struct |
| Lifecycle | Methods on State | `use_effect` | Methods on ViewState |
| Rebuild | `setState()` | Signal auto-tracks | `mark_needs_build()` |
| Config access | `widget.prop` | Closure capture | `view.prop` |

## Summary

**StatelessWidget:**
- Immutable configuration
- `build()` returns widget tree
- Rebuilds on parent rebuild or InheritedWidget change

**StatefulWidget:**
- Immutable configuration + mutable `State`
- State lifecycle: init → ready → deactivate → dispose
- `setState()` triggers rebuild
- State persists across widget updates (same runtimeType + key)

**FLUI Approach:**
- Prefer hooks (`use_signal`, `use_effect`) for simplicity
- Traditional `StatefulView` available for complex cases
- Signals provide automatic dependency tracking
- Lifecycle hooks via `use_effect` cleanup
