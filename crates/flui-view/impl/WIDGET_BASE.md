# Flutter Widget Base Classes

This document analyzes the low-level Widget base classes from Flutter's `framework.dart`.

## Core Hierarchy

```
Widget (abstract)
├── StatelessWidget
├── StatefulWidget  
├── ProxyWidget (abstract)
│   ├── ParentDataWidget<T>
│   └── InheritedWidget
│       ├── InheritedModel<T>
│       └── InheritedNotifier<T>
└── RenderObjectWidget (abstract)
    ├── LeafRenderObjectWidget
    ├── SingleChildRenderObjectWidget
    └── MultiChildRenderObjectWidget
```

## Widget (Base Class)

**Source:** `framework.dart:269-340`

### Key Properties

```dart
@immutable
abstract class Widget extends DiagnosticableTree {
  const Widget({this.key});
  
  final Key? key;
  
  @protected
  @factory
  Element createElement();
}
```

### Key Methods

| Method | Description |
|--------|-------------|
| `createElement()` | Factory method - creates Element for this Widget |
| `canUpdate(oldWidget, newWidget)` | Static - checks if element can be updated |

### canUpdate Logic

```dart
static bool canUpdate(Widget oldWidget, Widget newWidget) {
  return oldWidget.runtimeType == newWidget.runtimeType 
      && oldWidget.key == newWidget.key;
}
```

**FLUI Equivalent:** This maps to `View.can_update()` or element reconciliation logic.

### Immutability

- Widget is `@immutable` - all fields must be `final`
- Widgets are descriptions, not instances
- Same widget can be inflated multiple times into different Elements

## StatelessWidget

**Source:** `framework.dart:507-609`

### Structure

```dart
abstract class StatelessWidget extends Widget {
  const StatelessWidget({super.key});

  @override
  StatelessElement createElement() => StatelessElement(this);

  @protected
  Widget build(BuildContext context);
}
```

### Key Points

- Single `build()` method returns child Widget
- No mutable state
- Rebuilds when parent rebuilds or InheritedWidget changes
- `createElement()` returns `StatelessElement`

**FLUI Equivalent:** `StatelessView` trait with `fn build(&self, ctx: &BuildContext) -> impl IntoElement`

## StatefulWidget

**Source:** `framework.dart:793-961`

### Structure

```dart
abstract class StatefulWidget extends Widget {
  const StatefulWidget({super.key});

  @override
  StatefulElement createElement() => StatefulElement(this);

  @protected
  @factory
  State createState();
}
```

### State Class

```dart
abstract class State<T extends StatefulWidget> with Diagnosticable {
  T get widget => _widget!;
  T? _widget;
  
  BuildContext get context => _element!;
  StatefulElement? _element;
  
  bool get mounted => _element != null;
  
  @protected
  @mustCallSuper
  void initState() {}
  
  @mustCallSuper
  @protected
  void didUpdateWidget(covariant T oldWidget) {}
  
  @protected
  @mustCallSuper
  void reassemble() {}
  
  @protected
  void setState(VoidCallback fn) {
    fn();
    _element!.markNeedsBuild();
  }
  
  @protected
  @mustCallSuper
  void deactivate() {}
  
  @protected
  @mustCallSuper
  void activate() {}
  
  @protected
  @mustCallSuper
  void dispose() {}
  
  @protected
  Widget build(BuildContext context);
  
  @protected
  @mustCallSuper
  void didChangeDependencies() {}
}
```

### State Lifecycle

```
created → initState() → initialized → ready → [build loops] → deactivate → dispose → defunct
```

**Lifecycle Enum:**
```dart
enum _StateLifecycle {
  created,
  initialized, 
  ready,
  defunct,
}
```

### Key Points

- Widget is immutable, State is mutable
- `createState()` called each time widget is inflated
- State persists across widget rebuilds (if `canUpdate` returns true)
- `setState()` schedules rebuild via `markNeedsBuild()`

**FLUI Equivalent:** `StatefulView` with separate `ViewState` trait.

## ProxyWidget

**Source:** `framework.dart:1405-1420`

### Structure

```dart
abstract class ProxyWidget extends Widget {
  const ProxyWidget({super.key, required this.child});
  
  final Widget child;
}
```

### Purpose

- Base for widgets that wrap a child without building new content
- Used for `InheritedWidget` and `ParentDataWidget`

**FLUI Equivalent:** `ProxyView` - passes through child with modifications.

## ParentDataWidget<T>

**Source:** `framework.dart:1460-1580`

### Structure

```dart
abstract class ParentDataWidget<T extends ParentData> extends ProxyWidget {
  const ParentDataWidget({super.key, required super.child});

  @override
  ParentDataElement<T> createElement() => ParentDataElement<T>(this);

  bool debugIsValidRenderObject(RenderObject renderObject) {
    return renderObject.parentData is T;
  }

  Type get debugTypicalAncestorWidgetClass;

  @protected
  void applyParentData(RenderObject renderObject);

  @protected
  bool debugCanApplyOutOfTurn() => false;
}
```

### Purpose

- Configures `RenderObject.parentData` on child
- Used by layout widgets like `Stack` (via `Positioned`)
- Type parameter `T` specifies ParentData type

### Key Methods

| Method | Description |
|--------|-------------|
| `applyParentData()` | Writes data to child's parentData |
| `debugIsValidRenderObject()` | Validates parentData type compatibility |
| `debugTypicalAncestorWidgetClass` | Error messages - typical parent widget type |

**FLUI Equivalent:** Could use a trait like `ParentDataProvider<T>`.

## InheritedWidget

**Source:** `framework.dart:1582-1680`

### Structure

```dart
abstract class InheritedWidget extends ProxyWidget {
  const InheritedWidget({super.key, required super.child});

  @override
  InheritedElement createElement() => InheritedElement(this);

  @protected
  bool updateShouldNotify(covariant InheritedWidget oldWidget);
}
```

### Purpose

- Propagates data down the tree efficiently
- Descendants can depend on it via `BuildContext.dependOnInheritedWidgetOfExactType<T>()`
- Only rebuilds dependents when `updateShouldNotify()` returns true

### Usage Pattern

```dart
class FrogColor extends InheritedWidget {
  const FrogColor({super.key, required this.color, required super.child});
  
  final Color color;

  static FrogColor? maybeOf(BuildContext context) {
    return context.dependOnInheritedWidgetOfExactType<FrogColor>();
  }

  static FrogColor of(BuildContext context) {
    final result = maybeOf(context);
    assert(result != null, 'No FrogColor found');
    return result!;
  }

  @override
  bool updateShouldNotify(FrogColor oldWidget) => color != oldWidget.color;
}
```

**FLUI Equivalent:** `ProviderView<T>` - provides context data to descendants.

## InheritedModel<T>

**Source:** `inherited_model.dart`

### Structure

```dart
abstract class InheritedModel<T> extends InheritedWidget {
  const InheritedModel({super.key, required super.child});

  @override
  InheritedModelElement<T> createElement() => InheritedModelElement<T>(this);

  @protected
  bool updateShouldNotifyDependent(covariant InheritedModel<T> oldWidget, Set<T> dependencies);

  @protected
  bool isSupportedAspect(Object aspect) => true;

  static T? inheritFrom<T extends InheritedModel<Object>>(
    BuildContext context, 
    {Object? aspect}
  );
}
```

### Purpose

- Fine-grained dependency tracking
- Dependents specify which "aspect" they care about
- Only rebuilds if relevant aspect changed

### Key Methods

| Method | Description |
|--------|-------------|
| `updateShouldNotifyDependent()` | Check if specific aspects changed |
| `isSupportedAspect()` | Whether model supports given aspect |
| `inheritFrom()` | Static - create aspect-specific dependency |

**FLUI Equivalent:** Could extend `ProviderView` with aspect-based subscriptions using signals.

## InheritedNotifier<T>

**Source:** `inherited_notifier.dart`

### Structure

```dart
abstract class InheritedNotifier<T extends Listenable> extends InheritedWidget {
  const InheritedNotifier({super.key, this.notifier, required super.child});

  final T? notifier;

  @override
  bool updateShouldNotify(InheritedNotifier<T> oldWidget) {
    return oldWidget.notifier != notifier;
  }

  @override
  InheritedElement createElement() => _InheritedNotifierElement<T>(this);
}
```

### Purpose

- Wraps a `Listenable` (ChangeNotifier, ValueNotifier, Animation)
- Automatically rebuilds dependents when notifier fires
- Coalesces multiple notifications per frame

**FLUI Equivalent:** Natural fit with signals - `ProviderView<Signal<T>>`.

## RenderObjectWidget

**Source:** `framework.dart:1682-1760`

### Structure

```dart
abstract class RenderObjectWidget extends Widget {
  const RenderObjectWidget({super.key});

  @override
  @factory
  RenderObjectElement createElement();

  @protected
  @factory
  RenderObject createRenderObject(BuildContext context);

  @protected
  void updateRenderObject(BuildContext context, covariant RenderObject renderObject) {}

  @protected
  void didUnmountRenderObject(covariant RenderObject renderObject) {}
}
```

### Key Methods

| Method | Description |
|--------|-------------|
| `createRenderObject()` | Factory - creates RenderObject |
| `updateRenderObject()` | Updates existing RenderObject with new config |
| `didUnmountRenderObject()` | Cleanup when RenderObject removed |

**FLUI Equivalent:** `RenderView<P, A>` where P=Protocol, A=Arity.

## LeafRenderObjectWidget

**Source:** `framework.dart:1762-1775`

```dart
abstract class LeafRenderObjectWidget extends RenderObjectWidget {
  const LeafRenderObjectWidget({super.key});

  @override
  LeafRenderObjectElement createElement() => LeafRenderObjectElement(this);
}
```

**FLUI Equivalent:** `RenderView<BoxProtocol, Leaf>` - zero children.

## SingleChildRenderObjectWidget

**Source:** `framework.dart:1777-1800`

```dart
abstract class SingleChildRenderObjectWidget extends RenderObjectWidget {
  const SingleChildRenderObjectWidget({super.key, this.child});

  final Widget? child;

  @override
  SingleChildRenderObjectElement createElement() => SingleChildRenderObjectElement(this);
}
```

**FLUI Equivalent:** `RenderView<BoxProtocol, Single>` - exactly one child.

## MultiChildRenderObjectWidget

**Source:** `framework.dart:1802-1870`

```dart
abstract class MultiChildRenderObjectWidget extends RenderObjectWidget {
  const MultiChildRenderObjectWidget({super.key, this.children = const <Widget>[]});

  final List<Widget> children;

  @override
  MultiChildRenderObjectElement createElement() => MultiChildRenderObjectElement(this);
}
```

**FLUI Equivalent:** `RenderView<BoxProtocol, Variable>` - N children.

## Summary Table: Flutter → FLUI Mapping

| Flutter | FLUI | Description |
|---------|------|-------------|
| `Widget` | `View` trait | Immutable UI description |
| `StatelessWidget` | `StatelessView` | No state, single build |
| `StatefulWidget` | `StatefulView` | Mutable state |
| `State<T>` | `ViewState` trait | Holds mutable state |
| `ProxyWidget` | `ProxyView` | Wraps child |
| `ParentDataWidget<T>` | `ParentDataProvider<T>` | Configures child parentData |
| `InheritedWidget` | `ProviderView<T>` | Ambient data |
| `InheritedModel<T>` | Aspect-based signals | Fine-grained dependencies |
| `InheritedNotifier<T>` | `ProviderView<Signal<T>>` | Listenable integration |
| `RenderObjectWidget` | `RenderView<P, A>` | Creates RenderObject |
| `LeafRenderObjectWidget` | `RenderView<P, Leaf>` | No children |
| `SingleChildRenderObjectWidget` | `RenderView<P, Single>` | One child |
| `MultiChildRenderObjectWidget` | `RenderView<P, Variable>` | N children |
| `BuildContext` | `BuildContext` | Tree location handle |
| `Element` | `Element` | Widget instance in tree |
| `createElement()` | `into_element()` | Factory method |
| `key` | `ViewKey` | Reconciliation key |
