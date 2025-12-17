# FLUI Foundation

[![Crates.io](https://img.shields.io/crates/v/flui-foundation)](https://crates.io/crates/flui-foundation)
[![Documentation](https://docs.rs/flui-foundation/badge.svg)](https://docs.rs/flui-foundation)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](https://github.com/flui-org/flui)

**Foundation types and utilities for the FLUI framework ecosystem.**

FLUI Foundation provides fundamental building blocks used throughout the FLUI UI framework. It contains minimal-dependency types for element identification, change notification, diagnostics, and other core abstractions.

## Features

- **Tree IDs**: `ViewId`, `ElementId`, `RenderId`, `LayerId`, `SemanticsId` for the 5-tree architecture
- **Keys**: `Key`, `ValueKey`, `ObjectKey`, `UniqueKey`, `GlobalKey` for widget identity
- **Change Notification**: Observable patterns for reactive UI updates
- **Observer Lists**: Efficient `ObserverList`, `SyncObserverList`, `HashedObserverList`
- **Diagnostics**: Rich debugging and introspection utilities
- **Error Handling**: Standardized `FoundationError` with context chaining
- **Callbacks**: Type-safe callback aliases (`VoidCallback`, `ValueChanged`, etc.)
- **Platform Detection**: `TargetPlatform` for cross-platform code
- **Thread Safety**: All types designed for multi-threaded contexts
- **Minimal Dependencies**: Only essential external crates

## Quick Start

Add FLUI Foundation to your `Cargo.toml`:

```toml
[dependencies]
flui-foundation = "0.1"
```

Basic usage:

```rust
use flui_foundation::{ElementId, Key, ChangeNotifier, Listenable};
use std::sync::Arc;

// Create unique identifiers
let element_id = ElementId::new(1);
let key = Key::new();

// Observable values for reactive UI
let notifier = ChangeNotifier::new();
let listener_id = notifier.add_listener(Arc::new(|| {
    println!("Value changed!");
}));

// Notify listeners of changes
notifier.notify_listeners();
```

## Core Types

### Tree IDs (5-Tree Architecture)

```rust
use flui_foundation::{ViewId, ElementId, RenderId, LayerId, SemanticsId};

// Each tree level has its own ID type
let view_id = ViewId::new(1);
let element_id = ElementId::new(1);
let render_id = RenderId::new(1);
let layer_id = LayerId::new(1);
let semantics_id = SemanticsId::new(1);

// IDs support arithmetic and comparison
let next = element_id + 1;
assert!(element_id < next);

// Niche optimization: Option<ElementId> is 8 bytes
assert_eq!(std::mem::size_of::<Option<ElementId>>(), 8);
```

### Keys for Widget Identity

```rust
use flui_foundation::{Key, ValueKey};

// Auto-generated unique keys
let key1 = Key::new();
let key2 = Key::new();
assert_ne!(key1, key2);

// String-based keys (same string = same key)
let header = Key::from_str("header");
let header2 = Key::from_str("header");
assert_eq!(header, header2);

// Value keys for list items
let item_key = ValueKey::new(42i64);
```

### Change Notification

```rust
use flui_foundation::{ChangeNotifier, ValueNotifier, Listenable};
use std::sync::Arc;

// Basic change notification
let notifier = ChangeNotifier::new();
let id = notifier.add_listener(Arc::new(|| println!("Changed!")));
notifier.notify_listeners();
notifier.remove_listener(id);

// Value-holding notifier
let mut value = ValueNotifier::new(42);
value.add_listener(Arc::new(|| println!("Value updated!")));

value.set_value(100);        // Notifies only if value changed
value.set_value_force(100);  // Always notifies
value.update(|v| *v += 1);   // Update with closure
```

### Observer Lists

```rust
use flui_foundation::{ObserverList, SyncObserverList, HashedObserverList};

// Basic observer list (not thread-safe)
let mut observers: ObserverList<i32> = ObserverList::new();
let id = observers.add(42);
observers.remove(id);

// Thread-safe observer list
let sync_observers: SyncObserverList<i32> = SyncObserverList::new();
sync_observers.add(42);
sync_observers.for_each(|v| println!("{}", v));

// Hash-based for O(1) operations on large collections
let hashed: HashedObserverList<String> = HashedObserverList::new();
let id = hashed.add("observer".to_string());
hashed.remove(id);
```

### Diagnostics

```rust
use flui_foundation::{DiagnosticsNode, DiagnosticsProperty, DiagnosticLevel};

// Build diagnostic tree
let tree = DiagnosticsNode::new("MyWidget")
    .property("width", 100)
    .property("height", 50)
    .child(
        DiagnosticsNode::new("Child")
            .property("text", "Hello")
    );

println!("{}", tree);

// Custom diagnosticable
use flui_foundation::{Diagnosticable, DiagnosticsBuilder};

#[derive(Debug)]
struct MyWidget { width: f32 }

impl Diagnosticable for MyWidget {
    fn debug_fill_properties(&self, props: &mut Vec<DiagnosticsProperty>) {
        props.push(DiagnosticsProperty::new("width", self.width));
    }
}
```

### Error Handling

```rust
use flui_foundation::{FoundationError, Result, error::ErrorContext};

fn example() -> Result<i32> {
    // Create specific errors
    Err(FoundationError::invalid_id(0, "ID cannot be zero"))
}

fn with_context() -> Result<i32> {
    example().with_context("in with_context function")
}

// Check error properties
let err = FoundationError::listener_error("add", "limit reached");
assert!(err.is_recoverable());
assert_eq!(err.category(), "listener");
```

### Platform Detection

```rust
use flui_foundation::TargetPlatform;

let platform = TargetPlatform::current();

if platform.is_desktop() {
    println!("Running on desktop: {}", platform.as_str());
} else if platform.is_mobile() {
    println!("Running on mobile");
} else if platform.is_web() {
    println!("Running in browser");
}
```

## Feature Flags

- `serde`: Enables serialization support for foundation types
- `full`: Enables all optional features

```toml
[dependencies]
flui-foundation = { version = "0.1", features = ["serde"] }
```

## Examples

Run the examples to see the types in action:

```bash
# Basic ID usage
cargo run -p flui-foundation --example basic_ids

# Change notification patterns
cargo run -p flui-foundation --example change_notification

# Diagnostics and debugging
cargo run -p flui-foundation --example diagnostics

# Observer pattern implementations
cargo run -p flui-foundation --example observer_pattern
```

## Architecture

Foundation sits at the base of the FLUI architecture:

```
┌─────────────────┐
│   flui_app      │  ← Application framework
├─────────────────┤
│  flui_widgets   │  ← Widget library  
├─────────────────┤
│   flui_core     │  ← Core framework
├─────────────────┤
│ flui-foundation │  ← Foundation types (this crate)
├─────────────────┤
│  flui_types     │  ← Basic geometry and math
└─────────────────┘
```

## Performance

Foundation types are optimized for common UI patterns:

- **IDs**: Use `NonZeroUsize` for niche optimization (`Option<ElementId>` = 8 bytes)
- **Keys**: Atomic counter for O(1) generation, FNV-1a hash for string keys
- **Observer Lists**: O(1) add/remove with slot reuse, optional compaction
- **Change Notifiers**: Efficient listener storage with `parking_lot` locks

## Thread Safety

All foundation types are designed for multi-threaded use:

- **IDs**: `Send + Sync` (Copy types)
- **Keys**: `Send + Sync` (Copy types with atomic generation)
- **ChangeNotifier**: `Send + Sync` with `parking_lot::Mutex`
- **SyncObserverList**: Thread-safe with `RwLock`
- **HashedObserverList**: Lock-free with `DashMap`

## Development

```bash
# Run tests
cargo test -p flui-foundation

# Run tests with all features
cargo test -p flui-foundation --all-features

# Run clippy with pedantic lints
cargo clippy -p flui-foundation -- -W clippy::pedantic

# Check documentation
cargo doc -p flui-foundation --open
```

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))
- MIT License ([LICENSE-MIT](../../LICENSE-MIT))

at your option.

## Related Crates

- [`flui-types`](../flui_types): Basic geometry and mathematical types
- [`flui-tree`](../flui-tree): Tree abstractions and visitor patterns
- [`flui-view`](../flui-view): View traits and abstractions
- [`flui-core`](../flui_core): Core FLUI framework
- [`flui-widgets`](../flui_widgets): Widget library
- [`flui-app`](../flui_app): Application framework
