# Flutter Foundation Types - Complete Reference

This document maps ALL types from Flutter's `foundation` library to their usage patterns across the Flutter codebase, providing guidance for FLUI implementation.

## Table of Contents

1. [Annotations](#1-annotations)
2. [Basic Types & Callbacks](#2-basic-types--callbacks)
3. [Assertions & Errors](#3-assertions--errors)
4. [Binding](#4-binding)
5. [Change Notifier & Listenable](#5-change-notifier--listenable)
6. [Collections](#6-collections)
7. [Diagnostics](#7-diagnostics)
8. [Keys](#8-keys)
9. [Licenses](#9-licenses)
10. [Memory Allocations](#10-memory-allocations)
11. [Node](#11-node)
12. [Observer List](#12-observer-list)
13. [Persistent Hash Map](#13-persistent-hash-map)
14. [Platform](#14-platform)
15. [Serialization](#15-serialization)
16. [Stack Frame](#16-stack-frame)
17. [Synchronous Future](#17-synchronous-future)
18. [FLUI Implementation Status](#18-flui-implementation-status)

---

## 1. Annotations

**File:** `annotations.dart`

| Type | Description | Used By |
|------|-------------|---------|
| `Category` | Documentation category annotation | Documentation tooling |
| `DocumentationIcon` | Icon for documentation | Documentation tooling |
| `Summary` | Short summary annotation | Documentation tooling |

**FLUI:** Not needed - Rust uses `#[doc]` attributes.

---

## 2. Basic Types & Callbacks

**File:** `basic_types.dart`

| Type | Signature | Used By |
|------|-----------|---------|
| `ValueChanged<T>` | `void Function(T value)` | All widgets with callbacks |
| `ValueSetter<T>` | `void Function(T value)` | Property setters |
| `ValueGetter<T>` | `T Function()` | Property getters |
| `IterableFilter<T>` | `Iterable<T> Function(Iterable<T>)` | Collection filtering |
| `AsyncCallback` | `Future<void> Function()` | Async operations |
| `AsyncValueSetter<T>` | `Future<void> Function(T)` | Async setters |
| `AsyncValueGetter<T>` | `Future<T> Function()` | Async getters |
| `Factory<T>` | Factory pattern wrapper | Object creation |
| `CachingIterable<E>` | Lazy caching iterable | Performance optimization |

**FLUI:** Use Rust closures directly:
```rust
type ValueChanged<T> = Box<dyn Fn(T)>;
type ValueGetter<T> = Box<dyn Fn() -> T>;
type AsyncCallback = Box<dyn Fn() -> Pin<Box<dyn Future<Output = ()>>>>;
```

---

## 3. Assertions & Errors

**File:** `assertions.dart`

| Type | Description | Used By |
|------|-------------|---------|
| `FlutterExceptionHandler` | `void Function(FlutterErrorDetails)` | Global error handling |
| `DiagnosticPropertiesTransformer` | Transform diagnostics | Error formatting |
| `InformationCollector` | `Iterable<DiagnosticsNode> Function()` | Context collection |
| `StackTraceDemangler` | `StackTrace Function(StackTrace)` | Stack trace cleanup |
| `PartialStackFrame` | Partial frame matcher | Stack filtering |
| `StackFilter` | Abstract stack filter | Stack filtering |
| `RepetitiveStackFrameFilter` | Filter repetitive frames | Stack cleanup |
| `ErrorDescription` | Error description node | Error messages |
| `ErrorSummary` | Error summary node | Error headers |
| `ErrorHint` | Error hint node | Help messages |
| `ErrorSpacer` | Visual spacer | Error formatting |
| `FlutterErrorDetails` | Complete error details | **All error reporting** |
| `FlutterError` | Main error class | **Thrown everywhere** |
| `DiagnosticsStackTrace` | Stack trace diagnostic | Error display |

### Usage Examples

```dart
// animation/animation_controller.dart
throw FlutterError('AnimationController.forward() called after dispose()');

// animation/listener_helpers.dart
FlutterError.reportError(FlutterErrorDetails(
  exception: exception,
  stack: stack,
  library: 'animation library',
  context: ErrorDescription('while notifying listeners for $runtimeType'),
));
```

**FLUI:** Use `thiserror` and `anyhow`:
```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FluiError {
    #[error("Layout error: {0}")]
    Layout(String),
    #[error("Paint error: {0}")]
    Paint(String),
}
```

---

## 4. Binding

**File:** `binding.dart`

| Type | Description | Used By |
|------|-------------|---------|
| `ServiceExtensionCallback` | Service extension handler | DevTools |
| `BindingBase` | Abstract base for all bindings | **Core architecture** |

### Binding Hierarchy

```
BindingBase (abstract)
├── GesturesBinding (mixin)
├── SchedulerBinding (mixin)
├── ServicesBinding (mixin)
├── PaintingBinding (mixin)
├── SemanticsBinding (mixin)
├── RendererBinding (mixin)
└── WidgetsBinding (mixin)

Concrete implementations:
├── RenderingFlutterBinding = BindingBase + GesturesBinding + SchedulerBinding + 
│                             ServicesBinding + PaintingBinding + SemanticsBinding +
│                             RendererBinding
└── WidgetsFlutterBinding = RenderingFlutterBinding + WidgetsBinding
```

**FLUI:** See `flui_app/src/bindings/` for implementation.

---

## 5. Change Notifier & Listenable

**File:** `change_notifier.dart`

| Type | Description | Used By |
|------|-------------|---------|
| `Listenable` | Abstract listener interface | **Base for all notifiers** |
| `ValueListenable<T>` | Listenable with value | Value observation |
| `ChangeNotifier` | Mixin for notification | **Most stateful objects** |
| `ValueNotifier<T>` | Simple value holder | Simple state |

### Inheritance Hierarchy

```
Listenable (abstract)
├── ValueListenable<T> (abstract, extends Listenable)
│   ├── Animation<T> (implements ValueListenable)
│   ├── ValueNotifier<T> (extends ChangeNotifier, implements ValueListenable)
│   └── RouteInformationProvider (extends ValueListenable)
│
└── ChangeNotifier (mixin, implements Listenable)
    ├── ValueNotifier<T>
    ├── ScrollController
    ├── TabController
    ├── TextEditingController (extends ValueNotifier<TextEditingValue>)
    ├── AnimationController
    ├── FocusNode (with DiagnosticableTreeMixin)
    ├── FocusManager (with DiagnosticableTreeMixin)
    ├── SemanticsOwner
    ├── MouseTracker
    ├── ViewportOffset
    ├── RestorationManager
    ├── DataTableSource
    └── 30+ more classes
```

### Usage Examples

```dart
// widgets/scroll_controller.dart
class ScrollController extends ChangeNotifier {
  void jumpTo(double value) {
    // ...
    notifyListeners();
  }
}

// widgets/editable_text.dart
class TextEditingController extends ValueNotifier<TextEditingValue> {
  String get text => value.text;
  set text(String newText) {
    value = value.copyWith(text: newText);
  }
}
```

**FLUI:** See `flui-reactivity` for signal-based alternative.

---

## 6. Collections

**File:** `collections.dart`

No public types exported - internal utilities only.

---

## 7. Diagnostics

**File:** `diagnostics.dart`

| Type | Description | Used By |
|------|-------------|---------|
| `DiagnosticLevel` | Severity enum | All diagnostics |
| `DiagnosticsTreeStyle` | Tree display style | Debug output |
| `TextTreeConfiguration` | Tree rendering config | Debug output |
| `TextTreeRenderer` | Renders diagnostic trees | Debug output |
| `DiagnosticsNode` | Abstract node | **All diagnostics** |
| `MessageProperty` | Simple message | Error messages |
| `StringProperty` | String value | Object inspection |
| `DoubleProperty` | Double value | Layout debugging |
| `IntProperty` | Int value | Counting |
| `PercentProperty` | Percentage value | Progress |
| `FlagProperty` | Boolean flag | Feature flags |
| `IterableProperty<T>` | Collection property | Lists |
| `EnumProperty<T>` | Enum value | State |
| `ObjectFlagProperty<T>` | Object flag | Null checks |
| `FlagsSummary<T>` | Multiple flags | Feature summary |
| `DiagnosticsProperty<T>` | Generic property | **Base class** |
| `DiagnosticableNode<T>` | Node for Diagnosticable | Tree nodes |
| `DiagnosticableTreeNode` | Node for DiagnosticableTree | Tree nodes |
| `DiagnosticPropertiesBuilder` | Property collector | Debug info |
| `Diagnosticable` | Mixin for debug info | **Many classes** |
| `DiagnosticableTree` | Tree with children | **Widget/Element/RenderObject** |
| `DiagnosticableTreeMixin` | Impl helper | Tree classes |
| `DiagnosticsBlock` | Block of diagnostics | Grouping |
| `DiagnosticsSerializationDelegate` | Serialization | DevTools |

### Inheritance Hierarchy

```
Diagnosticable (mixin)
├── with Diagnosticable:
│   ├── FlutterErrorDetails
│   ├── AnimationStyle
│   ├── PointerEvent
│   ├── DragDownDetails, DragStartDetails, DragUpdateDetails, DragEndDetails
│   ├── TapDownDetails, TapUpDetails
│   ├── ScaleStartDetails, ScaleUpdateDetails, ScaleEndDetails
│   ├── LongPressDownDetails, LongPressStartDetails, etc.
│   ├── CupertinoThemeData, CupertinoTextThemeData
│   └── 40+ gesture/theme classes
│
└── DiagnosticableTree (abstract, with Diagnosticable)
    ├── Widget (abstract)
    │   ├── StatelessWidget
    │   ├── StatefulWidget
    │   ├── InheritedWidget
    │   └── RenderObjectWidget
    │
    ├── Element (abstract)
    │   ├── ComponentElement
    │   ├── RenderObjectElement
    │   └── ProxyElement
    │
    ├── RenderObject (with DiagnosticableTreeMixin)
    │   ├── RenderBox
    │   ├── RenderSliver
    │   └── RenderView
    │
    ├── Layer (with DiagnosticableTreeMixin)
    │
    ├── SemanticsNode (with DiagnosticableTreeMixin)
    │
    ├── InlineSpan (abstract)
    │   └── TextSpan
    │
    ├── SemanticsHintOverrides
    ├── SemanticsProperties
    │
    └── GestureRecognizer (with DiagnosticableTreeMixin)
```

### Usage Pattern

```dart
// widgets/framework.dart
abstract class Widget extends DiagnosticableTree {
  @override
  void debugFillProperties(DiagnosticPropertiesBuilder properties) {
    super.debugFillProperties(properties);
    properties.defaultDiagnosticsTreeStyle = DiagnosticsTreeStyle.dense;
  }
}

// rendering/object.dart
abstract class RenderObject with DiagnosticableTreeMixin implements HitTestTarget {
  @override
  void debugFillProperties(DiagnosticPropertiesBuilder properties) {
    super.debugFillProperties(properties);
    properties.add(DiagnosticsProperty<Size>('size', size));
    properties.add(FlagProperty('needsLayout', value: _needsLayout));
  }
}
```

**FLUI:** Use `Debug` trait with custom formatters.

---

## 8. Keys

**File:** `key.dart`

| Type | Description | Used By |
|------|-------------|---------|
| `Key` | Abstract key base | Widget identity |
| `LocalKey` | Key within parent | Widget reconciliation |
| `UniqueKey` | Unique instance key | Force rebuild |
| `ValueKey<T>` | Value-based key | Data-driven lists |

### Extended in widgets layer

| Type | File | Description |
|------|------|-------------|
| `GlobalKey<T>` | `widgets/framework.dart` | Cross-tree access |
| `LabeledGlobalKey<T>` | `widgets/framework.dart` | Debuggable global key |
| `GlobalObjectKey<T>` | `widgets/framework.dart` | Object identity key |
| `ObjectKey` | `widgets/framework.dart` | Object identity local key |

### Inheritance Hierarchy

```
Key (abstract)
├── LocalKey (abstract)
│   ├── UniqueKey
│   ├── ValueKey<T>
│   ├── ObjectKey (in widgets layer)
│   └── _SaltedKey<S,V> (material, internal)
│
└── GlobalKey<T> (abstract, in widgets layer)
    ├── LabeledGlobalKey<T>
    └── GlobalObjectKey<T>
```

### Usage Examples

```dart
// foundation/key.dart
class ValueKey<T> extends LocalKey {
  const ValueKey(this.value);
  final T value;
  
  @override
  bool operator ==(Object other) {
    if (other.runtimeType != runtimeType) return false;
    return other is ValueKey<T> && other.value == value;
  }
}

// widgets/framework.dart
class ObjectKey extends LocalKey {
  const ObjectKey(this.value);
  final Object? value;
  
  @override
  bool operator ==(Object other) {
    if (other.runtimeType != runtimeType) return false;
    return other is ObjectKey && identical(other.value, value);
  }
}
```

**FLUI:** See `flui-view/src/key/` for implementation.

---

## 9. Licenses

**File:** `licenses.dart`

| Type | Description | Used By |
|------|-------------|---------|
| `LicenseEntryCollector` | `Stream<LicenseEntry> Function()` | License registration |
| `LicenseParagraph` | Text paragraph with indent | License display |
| `LicenseEntry` | Abstract license entry | License data |
| `LicenseEntryWithLineBreaks` | Concrete implementation | Package licenses |

**FLUI:** Lower priority - implement when needed for license compliance UI.

---

## 10. Memory Allocations

**File:** `memory_allocations.dart`

| Type | Description | Used By |
|------|-------------|---------|
| `ObjectEvent` | Abstract memory event | Memory tracking |
| `ObjectCreated` | Object creation event | Leak detection |
| `ObjectDisposed` | Object disposal event | Leak detection |
| `ObjectEventListener` | `void Function(ObjectEvent)` | Listeners |
| `FlutterMemoryAllocations` | Singleton tracker | DevTools |

### Usage

```dart
// foundation/change_notifier.dart
if (kFlutterMemoryAllocationsEnabled) {
  FlutterMemoryAllocations.instance.dispatchObjectCreated(
    library: 'package:flutter/foundation.dart',
    className: '$ChangeNotifier',
    object: this,
  );
}
```

**FLUI:** Lower priority - Rust has different memory model.

---

## 11. Node

**File:** `node.dart`

| Type | Description | Used By |
|------|-------------|---------|
| `AbstractNode` | Base tree node | **Not used directly** |

**Note:** `AbstractNode` is defined but NOT used by `RenderObject`. Flutter's `RenderObject` implements its own parent-child management.

**FLUI:** Use `flui-tree` for tree operations.

---

## 12. Observer List

**File:** `observer_list.dart`

| Type | Description | Used By |
|------|-------------|---------|
| `ObserverList<T>` | Ordered observer list | Notification systems |
| `HashedObserverList<T>` | Fast lookup observers | High-performance listeners |

### Usage Examples

```dart
// animation/listener_helpers.dart
final HashedObserverList<VoidCallback> _listeners = HashedObserverList<VoidCallback>();
final ObserverList<AnimationStatusListener> _statusListeners = ObserverList<AnimationStatusListener>();

// widgets/focus_manager.dart
final HashedObserverList<OnKeyEventCallback> _earlyKeyEventHandlers = HashedObserverList<OnKeyEventCallback>();
```

**FLUI:** Use `Vec<T>` with dedup or `IndexSet<T>`.

---

## 13. Persistent Hash Map

**File:** `persistent_hash_map.dart`

| Type | Description | Used By |
|------|-------------|---------|
| `PersistentHashMap<K,V>` | Immutable hash map | Internal optimization |

**FLUI:** Use `im` crate for persistent data structures if needed.

---

## 14. Platform

**File:** `platform.dart`

| Type | Description | Used By |
|------|-------------|---------|
| `TargetPlatform` | Platform enum | Platform-specific behavior |

### Values

```dart
enum TargetPlatform {
  android,
  fuchsia,
  iOS,
  linux,
  macOS,
  windows,
}
```

### Usage

```dart
// Everywhere for platform-specific behavior
switch (defaultTargetPlatform) {
  case TargetPlatform.iOS:
  case TargetPlatform.macOS:
    return CupertinoStyle();
  case TargetPlatform.android:
  case TargetPlatform.fuchsia:
  case TargetPlatform.linux:
  case TargetPlatform.windows:
    return MaterialStyle();
}
```

**FLUI:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetPlatform {
    Android,
    Fuchsia,
    IOS,
    Linux,
    MacOS,
    Windows,
    Web,
}
```

---

## 15. Serialization

**File:** `serialization.dart`

| Type | Description | Used By |
|------|-------------|---------|
| `WriteBuffer` | Binary write buffer | Platform channels |
| `ReadBuffer` | Binary read buffer | Platform channels |

### Usage

```dart
// services/message_codecs.dart
final buffer = WriteBuffer(startCapacity: _writeBufferStartCapacity);
void writeValue(WriteBuffer buffer, Object? value) { ... }
Object? readValue(ReadBuffer buffer) { ... }
```

**FLUI:** Use `bytes` crate or `std::io::{Read, Write}`.

---

## 16. Stack Frame

**File:** `stack_frame.dart`

| Type | Description | Used By |
|------|-------------|---------|
| `StackFrame` | Parsed stack frame | Error reporting |

### Fields

```dart
class StackFrame {
  final int number;
  final String? column;
  final String? line;
  final String packageScheme;
  final String package;
  final String packagePath;
  final String? className;
  final String method;
  final bool isConstructor;
  final String source;
}
```

**FLUI:** Use `backtrace` crate.

---

## 17. Synchronous Future

**File:** `synchronous_future.dart`

| Type | Description | Used By |
|------|-------------|---------|
| `SynchronousFuture<T>` | Immediately resolved future | Caching, optimization |

### Usage Examples

```dart
// painting/image_provider.dart
return SynchronousFuture<FileImage>(this);

// widgets/localizations.dart
return SynchronousFuture<Map<Type, dynamic>>(output);

// services/restoration.dart
return SynchronousFuture<RestorationBucket?>(_rootBucket);
```

**FLUI:** Use `std::future::ready()` or just return values directly (Rust async is different).

---

## 18. FLUI Implementation Status

### Implemented

| Flutter Type | FLUI Equivalent | Location |
|--------------|-----------------|----------|
| `Key` | `Key` trait | `flui-view/src/key/mod.rs` |
| `LocalKey` | `LocalKey` | `flui-view/src/key/local.rs` |
| `UniqueKey` | `UniqueKey` | `flui-view/src/key/unique.rs` |
| `ValueKey<T>` | `ValueKey<T>` | `flui-view/src/key/value.rs` |
| `GlobalKey<T>` | `GlobalKey<T>` | `flui-view/src/key/global_key.rs` |
| `ObjectKey` | `ObjectKey` | `flui-view/src/key/object_key.rs` |
| ID types | `Id<T: Marker>` | `flui-foundation/src/id.rs` |
| `TargetPlatform` | `Platform` enum | `flui_types` |

### To Implement

| Flutter Type | Priority | Notes |
|--------------|----------|-------|
| `ChangeNotifier` | Low | Use signals instead |
| `Diagnosticable` | Medium | Use `Debug` + custom traits |
| `FlutterError` | High | Use `thiserror` |
| `BindingBase` | High | Already in `flui_app` |
| `ObserverList` | Low | Use `Vec` or `IndexSet` |
| `WriteBuffer/ReadBuffer` | Medium | Use `bytes` crate |

### Not Needed

| Flutter Type | Reason |
|--------------|--------|
| `Category`, `Summary`, etc. | Dart doc annotations |
| `AbstractNode` | Use `flui-tree` instead |
| `FlutterMemoryAllocations` | Rust has different memory model |
| `SynchronousFuture` | Rust async works differently |
| `PersistentHashMap` | Use `im` crate if needed |
| `LicenseEntry` | Implement when needed |

---

## Architecture Decision Summary

| Decision | Flutter | FLUI |
|----------|---------|------|
| Tree hierarchy | `DiagnosticableTree` mixin | `Debug` trait + custom traits |
| State notification | `ChangeNotifier` | Signals (`flui-reactivity`) |
| Error handling | `FlutterError` | `thiserror` + `anyhow` |
| Identity | `Key` class hierarchy | `Key` trait + impls |
| IDs | Plain integers | `Id<T: Marker>` (wgpu-style) |
| Tree storage | Class references | `Slab` with typed IDs |
| Platform channels | `WriteBuffer/ReadBuffer` | `bytes` crate |
| Binding system | Mixin composition | Trait composition |
