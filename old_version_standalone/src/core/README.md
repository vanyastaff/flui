# Core Foundation

The core foundation module provides fundamental building blocks for the Nebula UI framework, similar to Flutter's foundation library.

## Architecture

```
nebula-ui/
└── src/
    └── core/                    ← Foundation layer (like Flutter foundation)
        ├── callbacks.rs         - Callback type aliases
        ├── diagnostics.rs       - Debugging and introspection
        ├── key.rs               - Widget identification
        └── listenable.rs        - Observable values
```

## Modules

### `callbacks` - Callback Types

Common callback type aliases for consistent APIs.

```rust
use nebula_ui::core::{VoidCallback, ValueChanged, ValueGetter, ValueSetter};

// No arguments, no return
let on_pressed: VoidCallback = Box::new(|| {
    println!("Button pressed!");
});

// Value changed callback
let on_text_changed: ValueChanged<String> = Box::new(|text| {
    println!("Text changed to: {}", text);
});

// Value getter
let get_count: ValueGetter<i32> = Box::new(|| 42);

// Value setter
let set_count: ValueSetter<i32> = Box::new(|value| {
    println!("Count set to: {}", value);
});

// Helper functions
use nebula_ui::core::{void_callback, value_changed};

let callback = void_callback(|| println!("Hello!"));
let on_change = value_changed(|val: String| println!("{}", val));
```

**Available types:**
- `VoidCallback` - `() -> ()`
- `AsyncCallback` - `() -> Future<()>`
- `ValueChanged<T>` - `T -> ()`
- `ValueGetter<T>` - `() -> T`
- `ValueSetter<T>` - `T -> ()`
- `AsyncValueGetter<T>` - `() -> Future<T>`
- `AsyncValueSetter<T>` - `T -> Future<()>`

### `key` - Widget Keys

Keys uniquely identify widgets and optimize updates.

```rust
use nebula_ui::core::{UniqueKey, ValueKey, StringKey, IntKey, KeyFactory};

// Unique key (each instance is different)
let key1 = UniqueKey::new();
let key2 = UniqueKey::new();
assert_ne!(key1.id(), key2.id());

// Value key (same value = same key)
let str_key1 = ValueKey::new("button_1".to_string());
let str_key2 = ValueKey::new("button_1".to_string());
assert_eq!(str_key1.id(), str_key2.id());

// Type aliases for convenience
let string_key: StringKey = ValueKey::new("my_widget".to_string());
let int_key: IntKey = ValueKey::new(42);

// Key factory
let unique = KeyFactory::unique();
let string = KeyFactory::string("test");
let int = KeyFactory::int(100);

// Optional key wrapper
use nebula_ui::core::WidgetKey;

let key = WidgetKey::string("my_button");
if key.is_some() {
    println!("Key ID: {:?}", key.id());
}
```

**Key types:**
- `UniqueKey` - Identity-based (each instance unique)
- `ValueKey<T>` - Value-based (same value = same key)
- `StringKey` - Alias for `ValueKey<String>`
- `IntKey` - Alias for `ValueKey<i32>`

**Use cases:**
- Preserving widget state across rebuilds
- Optimizing widget tree updates
- Identifying specific widgets in collections

### `listenable` - Observable Values

Notification system for value changes.

```rust
use nebula_ui::core::{ChangeNotifier, ValueNotifier, Listenable};
use std::sync::Arc;

// ChangeNotifier - notify listeners of changes
let mut notifier = ChangeNotifier::new();

let listener = Arc::new(|| {
    println!("Something changed!");
});

let id = notifier.add_listener(listener);
notifier.notify_listeners(); // Prints: "Something changed!"
notifier.remove_listener(id);

// ValueNotifier - ChangeNotifier with a value
let mut count = ValueNotifier::new(0);

count.add_listener(Arc::new(|| {
    println!("Count changed!");
}));

count.set_value(5); // Prints: "Count changed!"
println!("Current value: {}", count.value()); // 5

// Update value with a function
count.update(|val| *val += 10);
// Prints: "Count changed!"
// Now value is 15
```

**Types:**
- `Listenable` - Trait for objects that can be listened to
- `ChangeNotifier` - Notifies listeners when changed
- `ValueNotifier<T>` - ChangeNotifier that holds a value

**Use cases:**
- State management
- Reactive UI updates
- Observer pattern implementation

### `diagnostics` - Debugging Support

Introspection and debugging information.

```rust
use nebula_ui::core::{
    Diagnosticable, DiagnosticsNode, DiagnosticsProperty,
    DiagnosticsBuilder, DiagnosticLevel,
};

// Create diagnostic properties
let prop1 = DiagnosticsProperty::new("width", 100);
let prop2 = DiagnosticsProperty::new("height", 50)
    .with_level(DiagnosticLevel::Debug);

// Build diagnostics tree
let mut root = DiagnosticsNode::new("MyWidget");
root.add_property(DiagnosticsProperty::new("id", 1));
root.add_property(DiagnosticsProperty::new("visible", true));

let mut child = DiagnosticsNode::new("ChildWidget");
child.add_property(DiagnosticsProperty::new("text", "Hello"));
root.add_child(child);

// Print diagnostics
println!("{}", root);
// Output:
// MyWidget
//   id: 1
//   visible: true
//   ChildWidget
//     text: Hello

// Diagnostics builder
let mut builder = DiagnosticsBuilder::new();
builder.add("width", 100);
builder.add("height", 50);
builder.add_flag("enabled", true, "ENABLED");
builder.add_optional("title", Some("Test"));

let properties = builder.build();

// Implement Diagnosticable trait
struct MyWidget {
    width: f32,
    height: f32,
}

impl Diagnosticable for MyWidget {
    fn to_diagnostics_node(&self) -> DiagnosticsNode {
        let mut node = DiagnosticsNode::new("MyWidget");
        node.add_property(DiagnosticsProperty::new("width", self.width));
        node.add_property(DiagnosticsProperty::new("height", self.height));
        node
    }
}
```

**Types:**
- `Diagnosticable` - Trait for objects that provide diagnostics
- `DiagnosticsNode` - Node in the diagnostics tree
- `DiagnosticsProperty` - Individual property
- `DiagnosticsBuilder` - Helper for building properties
- `DiagnosticLevel` - Hidden, Fine, Debug, Info, Warning, Hint, Error
- `DiagnosticsTreeStyle` - Sparse, Shallow, Dense, SingleLine, ErrorProperty

**Use cases:**
- Widget introspection
- Debug printing
- Performance profiling
- Error reporting

## Integration Examples

### Using Keys in Widgets

```rust
use nebula_ui::prelude::*;
use nebula_ui::core::{KeyFactory, WidgetKey};

struct MyStatefulWidget {
    key: WidgetKey,
    count: i32,
}

impl MyStatefulWidget {
    fn new(key: impl Into<String>) -> Self {
        Self {
            key: WidgetKey::string(key),
            count: 0,
        }
    }
}

// In a list of widgets
let widgets = vec![
    MyStatefulWidget::new("item_1"),
    MyStatefulWidget::new("item_2"),
    MyStatefulWidget::new("item_3"),
];
// Keys help preserve state when list is reordered
```

### Reactive State with ValueNotifier

```rust
use nebula_ui::core::{ValueNotifier, Listenable};
use std::sync::{Arc, Mutex};

// Shared state
let counter = Arc::new(Mutex::new(ValueNotifier::new(0)));

// Widget can listen to changes
{
    let counter_clone = Arc::clone(&counter);
    counter.lock().unwrap().add_listener(Arc::new(move || {
        // Trigger UI update
        println!("UI should update!");
    }));
}

// Somewhere else in code
counter.lock().unwrap().update(|val| *val += 1);
// This will trigger the listener
```

### Debugging with Diagnostics

```rust
use nebula_ui::core::{Diagnosticable, DiagnosticsNode, DiagnosticsProperty};

struct Button {
    label: String,
    enabled: bool,
    width: f32,
}

impl Diagnosticable for Button {
    fn to_diagnostics_node(&self) -> DiagnosticsNode {
        let mut node = DiagnosticsNode::new("Button");
        node.add_property(DiagnosticsProperty::new("label", &self.label));
        node.add_property(DiagnosticsProperty::new("enabled", self.enabled));
        node.add_property(DiagnosticsProperty::new("width", self.width));
        node
    }
}

// Debug print
let button = Button {
    label: "Click me".to_string(),
    enabled: true,
    width: 100.0,
};

let diagnostics = button.to_diagnostics_node();
println!("{}", diagnostics);
```

## Comparison with Flutter Foundation

| Flutter | Nebula-UI Core | Status |
|---------|----------------|--------|
| `VoidCallback` | `VoidCallback` | ✅ |
| `ValueChanged<T>` | `ValueChanged<T>` | ✅ |
| `ValueGetter<T>` | `ValueGetter<T>` | ✅ |
| `ValueSetter<T>` | `ValueSetter<T>` | ✅ |
| `Key` | `Key` trait | ✅ |
| `LocalKey` | `LocalKey` trait | ✅ |
| `UniqueKey` | `UniqueKey` | ✅ |
| `ValueKey<T>` | `ValueKey<T>` | ✅ |
| `Listenable` | `Listenable` trait | ✅ |
| `ChangeNotifier` | `ChangeNotifier` | ✅ |
| `ValueNotifier<T>` | `ValueNotifier<T>` | ✅ |
| `Diagnosticable` | `Diagnosticable` trait | ✅ |
| `DiagnosticsNode` | `DiagnosticsNode` | ✅ |
| `DiagnosticsProperty` | `DiagnosticsProperty` | ✅ |
| `DiagnosticLevel` | `DiagnosticLevel` | ✅ |
| `DiagnosticsTreeStyle` | `DiagnosticsTreeStyle` | ✅ |

## Best Practices

### 1. Use Type Aliases for Callbacks

```rust
// Good - using type aliases
use nebula_ui::core::VoidCallback;

struct Button {
    on_pressed: Option<VoidCallback>,
}

// Instead of verbose Box<dyn Fn()>
```

### 2. Keys for Dynamic Lists

```rust
use nebula_ui::core::ValueKey;

// When rendering dynamic lists, use keys
for item in items {
    let key = ValueKey::new(item.id);
    render_item_with_key(item, key);
}
```

### 3. ValueNotifier for Simple State

```rust
use nebula_ui::core::ValueNotifier;

// Simple reactive state
let is_loading = ValueNotifier::new(false);

// Update from anywhere
is_loading.set_value(true);
```

### 4. Diagnostics for Complex Widgets

```rust
use nebula_ui::core::{Diagnosticable, DiagnosticsBuilder};

impl Diagnosticable for ComplexWidget {
    fn debug_fill_properties(&self, properties: &mut Vec<DiagnosticsProperty>) {
        let mut builder = DiagnosticsBuilder::new();
        builder.add("field1", &self.field1);
        builder.add("field2", &self.field2);
        builder.add_optional("optional_field", self.optional_field.as_ref());
        properties.extend(builder.build());
    }
}
```

## Testing

Run core foundation tests:

```bash
cargo test --lib core
```

Current test coverage:
- `callbacks`: 5 tests
- `key`: 8 tests
- `listenable`: 5 tests
- `diagnostics`: 7 tests

Total: **576 tests passing** in nebula-ui (including core foundation)

## Future Enhancements

Planned additions:

1. **Error Handling**
   - `NebulaError` type
   - `ErrorDetails` with stack traces
   - Error descriptions and hints

2. **Utilities**
   - `BitField` for flag management
   - `compute()` for background tasks
   - Utility functions (listEquals, mapEquals, etc.)

3. **Platform Integration**
   - `BindingBase` for platform bindings
   - Service extensions
   - Memory allocations tracking

4. **Advanced Observability**
   - `ValueListenable<T>` trait
   - `MergedListenable` for combining streams
   - Animation integration with listenables

## References

- [Flutter Foundation Library](https://api.flutter.dev/flutter/foundation/foundation-library.html)
- [Flutter Key Class](https://api.flutter.dev/flutter/foundation/Key-class.html)
- [Flutter ChangeNotifier](https://api.flutter.dev/flutter/foundation/ChangeNotifier-class.html)
- [Flutter Diagnosticable](https://api.flutter.dev/flutter/foundation/Diagnosticable-class.html)
