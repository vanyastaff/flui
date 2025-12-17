# Flutter Foundation - Architecture Map

This document maps Flutter's foundation types to their usage across the framework,
helping understand what FLUI types should inherit/implement.

## 1. Observables Hierarchy (Listenable)

```
Listenable (interface)
├── ChangeNotifier (mixin class)
│   ├── ValueNotifier<T>
│   │   ├── TextEditingController
│   │   ├── TransformationController
│   │   ├── ClipboardStatusNotifier
│   │   ├── UndoHistoryController
│   │   └── WidgetStatesController
│   ├── ScrollController
│   ├── TabController
│   ├── AnimationController
│   ├── FocusNode
│   ├── MouseTracker
│   ├── ViewportOffset
│   ├── SemanticsOwner
│   ├── RestorationManager
│   ├── DraggableScrollableController
│   ├── ScrollbarPainter
│   ├── DataTableSource
│   └── RestorableProperty<T>
├── ValueListenable<T> (interface)
│   ├── Animation<T>
│   ├── ValueNotifier<T>
│   └── RouteInformationProvider
├── CustomPainter (abstract)
│   ├── _CupertinoActivityIndicatorPainter
│   ├── _LinearProgressIndicatorPainter
│   ├── _CircularProgressIndicatorPainter
│   ├── _DialPainter
│   ├── BannerPainter
│   └── ScrollbarPainter (also ChangeNotifier)
├── CustomClipper<T> (abstract)
│   ├── ShapeBorderClipper
│   ├── _BottomAppBarClipper
│   └── _DecorationClipper
├── Animation<T>
├── OverlayEntry
└── RouterDelegate<T>
```

### Key Patterns

- **Listenable**: Base interface for anything that can notify listeners
- **ChangeNotifier**: Mutable observable with add/remove/notify pattern
- **ValueNotifier<T>**: Single-value ChangeNotifier with `.value` getter/setter
- **ValueListenable<T>**: Read-only interface for value observables

## 2. Diagnostics Hierarchy

```
Diagnosticable (mixin)
├── DiagnosticableTree (abstract)
│   ├── Widget (abstract)
│   │   ├── StatelessWidget
│   │   └── StatefulWidget
│   ├── Element (abstract)
│   │   ├── ComponentElement
│   │   └── RenderObjectElement
│   ├── RenderObject (abstract)
│   │   ├── RenderBox
│   │   │   ├── RenderProxyBox
│   │   │   ├── RenderFlex
│   │   │   ├── RenderFlow
│   │   │   ├── RenderParagraph
│   │   │   └── RenderImage
│   │   └── RenderSliver
│   ├── Layer (abstract)
│   ├── SemanticsNode
│   ├── SemanticsProperties
│   ├── InlineSpan (TextSpan)
│   ├── PipelineOwner
│   └── FocusNode/FocusManager
├── FlutterErrorDetails
├── FlutterError
├── GestureRecognizer
├── PointerEvent (all gesture details)
│   ├── DragDownDetails
│   ├── DragStartDetails
│   ├── DragUpdateDetails
│   ├── TapDownDetails
│   ├── ScaleStartDetails
│   └── LongPressDetails
└── ThemeData classes
    ├── AnimationStyle
    ├── CupertinoThemeData
    ├── AppBarThemeData
    ├── BadgeThemeData
    └── ... (all *ThemeData)
```

### Key Patterns

- **Diagnosticable**: Mixin for debug output (toString, debugFillProperties)
- **DiagnosticableTree**: For hierarchical structures with children
- **DiagnosticableTreeMixin**: Convenience mixin implementing DiagnosticableTree

## 3. Keys Hierarchy

```
Key (abstract)
├── LocalKey (abstract)
│   ├── ValueKey<T>      - key by value equality
│   ├── UniqueKey        - unique identity (each instance different)
│   ├── ObjectKey        - key by object identity (pointer)
│   └── _SaltedKey<S,V>  - internal composite key
└── GlobalKey<T> (abstract)
    ├── LabeledGlobalKey<T>   - GlobalKey with debug label
    └── GlobalObjectKey<T>    - GlobalKey by object identity
```

### Key Patterns

- **LocalKey**: For children of same parent (list items, tabs)
- **GlobalKey**: For cross-tree element access (measuring, state access)
- **ValueKey**: When you have a unique value (database ID, enum)
- **UniqueKey**: When you need guaranteed uniqueness
- **ObjectKey**: When keying by specific object instance

## 4. Callbacks (Type Aliases)

```dart
// No-argument callback
typedef VoidCallback = void Function();

// Value change callbacks
typedef ValueChanged<T> = void Function(T value);
typedef ValueSetter<T> = void Function(T value);  // same as ValueChanged
typedef ValueGetter<T> = T Function();

// Predicates
typedef Predicate<T> = bool Function(T value);

// Async
typedef AsyncCallback = Future<void> Function();
typedef AsyncValueGetter<T> = Future<T> Function();
typedef AsyncValueSetter<T> = Future<void> Function(T value);
```

## 5. Observer Lists

```
ObserverList<T>           - List that can be iterated during modification
HashedObserverList<T>     - O(1) add/remove using hash set
```

### Usage

| List Type | Use Case |
|-----------|----------|
| `ObserverList` | AnimationStatusListener, semantics actions |
| `HashedObserverList` | VoidCallback listeners (ChangeNotifier), focus handlers |

## 6. Architecture Decision Table

| Your Class Type | Should Inherit/Implement | Flutter Examples |
|-----------------|--------------------------|------------------|
| **Any observable** | `Listenable` | Animation, OverlayEntry, RouterDelegate |
| **Mutable observable** | `ChangeNotifier` | ScrollController, TabController, FocusNode, MouseTracker |
| **Single value observable** | `ValueNotifier<T>` | TextEditingController, TransformationController |
| **Read-only value** | `impl ValueListenable<T>` | Animation<T>, RouteInformationProvider |
| **Widget** | `extends DiagnosticableTree` | Widget, Element, RenderObject |
| **RenderObject** | `extends RenderObject` + `DiagnosticableTreeMixin` | RenderBox, RenderSliver, RenderFlex |
| **Layer** | `with DiagnosticableTreeMixin` | OffsetLayer, ClipRectLayer, OpacityLayer |
| **Semantics node** | `with DiagnosticableTreeMixin` | SemanticsNode |
| **Gesture details** | `with Diagnosticable` | DragStartDetails, TapDownDetails, ScaleUpdateDetails |
| **Theme data** | `with Diagnosticable` | AppBarThemeData, BadgeThemeData, ButtonThemeData |
| **Error info** | `with Diagnosticable` | FlutterErrorDetails |
| **Custom painting** | `extends CustomPainter` | All painters (Listenable for repaint) |
| **Custom clipping** | `extends CustomClipper<T>` | ShapeBorderClipper (Listenable for reclip) |
| **Child identity** | `extends LocalKey` | ValueKey, UniqueKey, ObjectKey |
| **Cross-tree access** | `extends GlobalKey<T>` | GlobalObjectKey, LabeledGlobalKey |
| **Focus management** | `extends ChangeNotifier` + `DiagnosticableTreeMixin` | FocusNode, FocusManager |

## 7. FLUI Mapping

| Flutter Type | FLUI Type | Location |
|--------------|-----------|----------|
| `Listenable` | `Listenable` | `flui-foundation/notifier.rs` |
| `ChangeNotifier` | `ChangeNotifier` | `flui-foundation/notifier.rs` |
| `ValueNotifier<T>` | `ValueNotifier<T>` | `flui-foundation/notifier.rs` |
| `ValueListenable<T>` | `ValueListenable<T>` | `flui-foundation/notifier.rs` |
| `ObserverList<T>` | `ObserverList<T>` | `flui-foundation/observer.rs` |
| `HashedObserverList<T>` | `HashedObserverList<T>` | `flui-foundation/observer.rs` |
| `Key` | `Key` | `flui-foundation/key.rs` |
| `LocalKey` | `ViewKey` (trait) | `flui-foundation/key.rs` |
| `ValueKey<T>` | `ValueKey<T>` | `flui-foundation/key.rs` |
| `UniqueKey` | `UniqueKey` | `flui-foundation/key.rs` |
| `ObjectKey` | `ObjectKey` | `flui-view/key/object_key.rs` |
| `GlobalKey<T>` | `GlobalKey<T>` | `flui-view/key/global_key.rs` |
| `VoidCallback` | `VoidCallback` | `flui-foundation/callbacks.rs` |
| `ValueChanged<T>` | `ValueChanged<T>` | `flui-foundation/callbacks.rs` |
| `Diagnosticable` | `Diagnosticable` | `flui-foundation/debug.rs` |
| `TargetPlatform` | `TargetPlatform` | `flui-foundation/platform.rs` |

## 8. ID Types (FLUI-specific)

FLUI uses typed IDs for tree node identification (wgpu-style):

```rust
// Generic ID with marker trait
Id<T: Marker>

// Core tree IDs
ViewId      // View tree (immutable config)
ElementId   // Element tree (mutable state)
RenderId    // RenderObject tree (layout/paint)
LayerId     // Layer tree (compositing)
SemanticsId // Semantics tree (accessibility)

// Platform IDs (from Flutter)
TextureId, PlatformViewId, DeviceId, PointerId, etc.
```

See `flui-foundation/id.rs` for full list.
