# Flutter Key System

This document analyzes the Key system from Flutter's `framework.dart`.

## Key Hierarchy

```
Key (abstract, foundation)
├── LocalKey (abstract)
│   ├── ValueKey<T>
│   ├── ObjectKey
│   └── UniqueKey
└── GlobalKey<T extends State> (abstract)
    ├── LabeledGlobalKey<T>
    └── GlobalObjectKey<T>
```

## Key (Base)

**Source:** `package:flutter/foundation.dart`

The base `Key` class is defined in foundation, not framework.

### Purpose

- Controls widget identity during reconciliation
- Determines whether an Element can be updated vs recreated
- Enables widget reparenting (GlobalKey)

### Reconciliation Rule

```dart
static bool canUpdate(Widget oldWidget, Widget newWidget) {
  return oldWidget.runtimeType == newWidget.runtimeType 
      && oldWidget.key == newWidget.key;
}
```

## LocalKey

**Source:** `package:flutter/foundation.dart`

Keys that are unique within their parent's children list.

### ValueKey<T>

```dart
class ValueKey<T> extends LocalKey {
  const ValueKey(this.value);
  final T value;
  
  @override
  bool operator ==(Object other) {
    if (other.runtimeType != runtimeType) return false;
    return other is ValueKey<T> && other.value == value;
  }
  
  @override
  int get hashCode => Object.hash(runtimeType, value);
}
```

**Use Case:** Key by domain value (ID, name, etc.)

```dart
ListView.builder(
  itemBuilder: (context, index) => ListTile(
    key: ValueKey(items[index].id),
    title: Text(items[index].name),
  ),
)
```

### ObjectKey

**Source:** `framework.dart:86-110`

```dart
class ObjectKey extends LocalKey {
  const ObjectKey(this.value);
  final Object? value;
  
  @override
  bool operator ==(Object other) {
    if (other.runtimeType != runtimeType) return false;
    return other is ObjectKey && identical(other.value, value);
  }
  
  @override
  int get hashCode => Object.hash(runtimeType, identityHashCode(value));
}
```

**Use Case:** Key by object identity (not value equality).

```dart
// Different from ValueKey - uses identical() not ==
final key1 = ObjectKey(myObject);
final key2 = ObjectKey(myObject);
// key1 == key2 only if identical(myObject, myObject)
```

### UniqueKey

```dart
class UniqueKey extends LocalKey {
  UniqueKey();
  
  @override
  String toString() => '[#${shortHash(this)}]';
}
```

**Use Case:** Force element recreation on every build.

```dart
// Always creates new element
Container(key: UniqueKey(), ...)
```

## GlobalKey<T>

**Source:** `framework.dart:112-220`

### Structure

```dart
abstract class GlobalKey<T extends State<StatefulWidget>> extends Key {
  factory GlobalKey({String? debugLabel}) => LabeledGlobalKey<T>(debugLabel);
  
  const GlobalKey.constructor() : super.empty();

  Element? get _currentElement => 
    WidgetsBinding.instance.buildOwner!._globalKeyRegistry[this];

  BuildContext? get currentContext => _currentElement;
  
  Widget? get currentWidget => _currentElement?.widget;
  
  T? get currentState => switch (_currentElement) {
    StatefulElement(:final T state) => state,
    _ => null,
  };
}
```

### Key Features

1. **Unique across entire app** (not just siblings)
2. **Access to Element, Widget, and State** from anywhere
3. **Enables widget reparenting** without losing state
4. **Registered in BuildOwner._globalKeyRegistry**

### Reparenting Behavior

```dart
// Widget can move between parents without losing state
Column(
  children: [
    if (showFirst)
      Container(key: _globalKey, child: MyStatefulWidget())
    else
      Text('Hidden'),
  ],
)

// Later, same widget in different parent:
Row(
  children: [
    if (!showFirst)
      Container(key: _globalKey, child: MyStatefulWidget())
    // State is preserved!
  ],
)
```

### Implementation Details

```dart
class LabeledGlobalKey<T extends State<StatefulWidget>> extends GlobalKey<T> {
  LabeledGlobalKey(this._debugLabel) : super.constructor();
  
  final String? _debugLabel;
  
  @override
  String toString() {
    final label = _debugLabel != null ? ' $_debugLabel' : '';
    if (runtimeType == LabeledGlobalKey) {
      return '[GlobalKey#${shortHash(this)}$label]';
    }
    return '[${describeIdentity(this)}$label]';
  }
}
```

### GlobalObjectKey<T>

**Source:** `framework.dart:222-268`

```dart
class GlobalObjectKey<T extends State<StatefulWidget>> extends GlobalKey<T> {
  const GlobalObjectKey(this.value) : super.constructor();
  
  final Object value;
  
  @override
  bool operator ==(Object other) {
    if (other.runtimeType != runtimeType) return false;
    return other is GlobalObjectKey<T> && identical(other.value, value);
  }
  
  @override
  int get hashCode => identityHashCode(value);
}
```

**Use Case:** GlobalKey keyed by object identity.

**Warning:** Risk of collisions if same object used in different parts of tree.

```dart
// Create private subclass to avoid collisions
class _MyKey extends GlobalObjectKey {
  const _MyKey(super.value);
}
```

## Key Usage in Reconciliation

### Element.updateChild Logic

```dart
Element? updateChild(Element? child, Widget? newWidget, Object? newSlot) {
  if (newWidget == null) {
    if (child != null) deactivateChild(child);
    return null;
  }
  
  if (child != null) {
    if (child.widget == newWidget) {
      // Same widget instance - just update slot
      if (child.slot != newSlot) updateSlotForChild(child, newSlot);
      return child;
    }
    
    if (Widget.canUpdate(child.widget, newWidget)) {
      // Same type and key - update in place
      if (child.slot != newSlot) updateSlotForChild(child, newSlot);
      child.update(newWidget);
      return child;
    }
    
    // Different type or key - deactivate old, create new
    deactivateChild(child);
  }
  
  return inflateWidget(newWidget, newSlot);
}
```

### MultiChildRenderObjectElement

For lists of children, keys enable efficient diffing:

```dart
// Without keys: O(n) comparison, may recreate unnecessarily
// With keys: O(n) HashMap lookup, preserves elements correctly

// Example: reordering items
[A, B, C] → [C, A, B]

// Without keys: Updates A→C, B→A, C→B (potentially wrong state)
// With keys: Moves elements C, A, B to new positions (preserves state)
```

## FLUI Key System Design

### Proposed Structure

```rust
/// Key for widget reconciliation
pub enum ViewKey {
    /// No key - identified by position only
    None,
    /// Value-based key
    Value(TypeId, u64),  // (type discriminant, hash)
    /// Object identity key  
    Object(usize),       // pointer address
    /// Unique key (always different)
    Unique(UniqueKeyId),
    /// Global key with registry
    Global(GlobalKeyId),
}

/// Unique key generator
pub struct UniqueKeyId(NonZeroU64);

/// Global key with access to element
pub struct GlobalKeyId(NonZeroU64);
```

### Value Key

```rust
/// Creates a value-based key
pub fn value_key<T: Hash + 'static>(value: &T) -> ViewKey {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    ViewKey::Value(TypeId::of::<T>(), hasher.finish())
}

// Usage
fn build(&self, ctx: &BuildContext) -> impl IntoElement {
    Column::new()
        .children(self.items.iter().map(|item| {
            ListTile::new()
                .key(value_key(&item.id))
                .title(item.name.clone())
        }))
}
```

### Global Key Registry

```rust
pub struct GlobalKeyRegistry {
    keys: DashMap<GlobalKeyId, ElementId>,
    next_id: AtomicU64,
}

impl GlobalKeyRegistry {
    pub fn register(&self, key: GlobalKeyId, element: ElementId) {
        self.keys.insert(key, element);
    }
    
    pub fn lookup(&self, key: GlobalKeyId) -> Option<ElementId> {
        self.keys.get(&key).map(|r| *r)
    }
    
    pub fn unregister(&self, key: GlobalKeyId) {
        self.keys.remove(&key);
    }
}
```

### Reconciliation Integration

```rust
impl ElementTree {
    pub fn can_update(old_view: &dyn ViewObject, new_view: &dyn ViewObject) -> bool {
        // Same type ID
        old_view.type_id() == new_view.type_id() 
        // Same key (or both None)
        && old_view.key() == new_view.key()
    }
}
```

## Summary: Flutter → FLUI Key Mapping

| Flutter | FLUI | Notes |
|---------|------|-------|
| `Key` | `ViewKey` enum | Base key type |
| `LocalKey` | `ViewKey::Value`, `ViewKey::Object` | Position-local keys |
| `ValueKey<T>` | `value_key<T>()` | By value equality |
| `ObjectKey` | `ViewKey::Object(ptr)` | By identity |
| `UniqueKey` | `ViewKey::Unique(id)` | Always unique |
| `GlobalKey<T>` | `GlobalKey<T>` struct | With registry |
| `LabeledGlobalKey` | Debug label field | Optional |
| `GlobalObjectKey` | `GlobalKey::from_ptr()` | By identity |
| `_globalKeyRegistry` | `GlobalKeyRegistry` | DashMap-based |
