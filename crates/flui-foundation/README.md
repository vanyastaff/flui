# FLUI Foundation

[![Crates.io](https://img.shields.io/crates/v/flui-foundation)](https://crates.io/crates/flui-foundation)
[![Documentation](https://docs.rs/flui-foundation/badge.svg)](https://docs.rs/flui-foundation)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](https://github.com/flui-org/flui)

**Foundation types and utilities for the FLUI framework ecosystem.**

FLUI Foundation provides fundamental building blocks used throughout the FLUI UI framework. It contains minimal-dependency types for element identification, change notification, diagnostics, and other core abstractions.

## Features

- 🔑 **Core Types**: ElementId, Key, Slot for element identification and positioning
- 📢 **Change Notification**: Observable patterns for reactive UI updates
- 🐛 **Diagnostics**: Rich debugging and introspection utilities
- ⚡ **Atomic Utilities**: Lock-free operations for performance-critical code
- 🔒 **Thread Safety**: All types designed for multi-threaded contexts
- 📦 **Minimal Dependencies**: Only essential external crates
- 🚀 **Zero-Cost Abstractions**: Performance-critical paths have no overhead

## Quick Start

Add FLUI Foundation to your `Cargo.toml`:

```toml
[dependencies]
flui-foundation = "0.1"
```

Basic usage:

```rust
use flui_foundation::prelude::*;

// Create unique identifiers
let element_id = ElementId::new(1);
let key = Key::new();

// Observable values for reactive UI
let mut notifier = ChangeNotifier::new();
let listener_id = notifier.add_listener(std::sync::Arc::new(|| {
    println!("Value changed!");
}));

// Notify listeners of changes
notifier.notify();
```

## Core Types

### Element Identification

```rust
use flui_foundation::{ElementId, Key, Slot};

// Unique element identifier with O(1) operations
let element_id = ElementId::new(42);
assert_eq!(element_id.get(), 42);

// Keys for element matching during rebuilds
let key1 = Key::new();
let key2 = Key::new();
assert_ne!(key1, key2);

// Slots for positioned elements
let slot = Slot::new(0);
```

### Change Notification

```rust
use flui_foundation::{ChangeNotifier, ValueNotifier};
use std::sync::Arc;

// Basic change notification
let mut notifier = ChangeNotifier::new();
let listener = notifier.add_listener(Arc::new(|| println!("Changed!")));

// Value-holding notifier
let mut value_notifier = ValueNotifier::new(42);
let value_listener = value_notifier.add_listener(Arc::new(|value| {
    println!("New value: {}", value);
}));

// Update value and notify listeners
value_notifier.set(100);
```

### Atomic Flags

```rust
use flui_foundation::{AtomicElementFlags, ElementFlags};

// Thread-safe element state flags
let flags = AtomicElementFlags::new();

// Set flags atomically
flags.insert(ElementFlags::DIRTY);
flags.insert(ElementFlags::NEEDS_LAYOUT);

// Check flags
assert!(flags.contains(ElementFlags::DIRTY));

// Clear flags
flags.remove(ElementFlags::DIRTY);
```

### Diagnostics

```rust
use flui_foundation::{DiagnosticsNode, DiagnosticsTreeStyle};

// Create diagnostic information
let node = DiagnosticsNode::new("MyWidget")
    .with_property("width", 100.0)
    .with_property("height", 200.0)
    .with_child(
        DiagnosticsNode::new("Child")
            .with_property("text", "Hello World")
    );

// Print diagnostic tree
println!("{}", node.to_string_deep(DiagnosticsTreeStyle::Sparse));
```

## Feature Flags

- `serde`: Enables serialization support for foundation types
- `async`: Enables async utilities and notification patterns  
- `full`: Enables all optional features

### Serialization Support

Enable the `serde` feature for serialization:

```toml
[dependencies]
flui-foundation = { version = "0.1", features = ["serde"] }
```

```rust
use flui_foundation::{ElementId, serde_support::*};

// Serialize to JSON
let element_id = ElementId::new(42);
let json = to_json_string(&element_id).unwrap();
assert_eq!(json, "42");

// Deserialize from JSON
let recovered: ElementId = from_json_string(&json).unwrap();
assert_eq!(recovered.get(), 42);

// Binary serialization
let binary = to_binary(&element_id).unwrap();
let recovered: ElementId = from_binary(&binary).unwrap();
```

### Async Support

Enable the `async` feature for async utilities:

```toml
[dependencies]
flui-foundation = { version = "0.1", features = ["async"] }
```

```rust
use flui_foundation::{AsyncChangeNotifier, AsyncValueNotifier};
use tokio::time::Duration;

#[tokio::main]
async fn main() {
    // Async change notification
    let notifier = AsyncChangeNotifier::new();
    let mut receiver = notifier.subscribe();
    
    // Notify asynchronously
    notifier.notify().await;
    
    // Wait for changes with timeout
    let result = notifier.wait_for_change(Duration::from_millis(100)).await;
    
    // Async value notifier
    let value_notifier = AsyncValueNotifier::new(0);
    value_notifier.set(42).await;
    
    let mut value_receiver = value_notifier.subscribe();
    value_receiver.changed().await.unwrap();
    assert_eq!(*value_receiver.borrow(), 42);
}
```

## Design Principles

1. **Minimal Dependencies**: Only essential external crates to reduce dependency tree
2. **Zero-Cost Abstractions**: Performance-critical paths have no runtime overhead  
3. **Thread Safety**: All types work correctly in multi-threaded environments
4. **Composability**: Types work well together and with external code
5. **Stability**: Strong backwards compatibility guarantees

## Performance

Foundation types are optimized for common UI patterns:

- **ElementId**: Uses `NonZeroUsize` for niche optimization - `Option<ElementId>` is 8 bytes
- **Key**: Atomic counter generation for O(1) creation with collision resistance
- **AtomicElementFlags**: Lock-free atomic operations for high-performance state tracking
- **ChangeNotifier**: Efficient listener storage with minimal allocation overhead

## Thread Safety

All foundation types are designed for multi-threaded use:

- **ElementId**: `Send + Sync` (copy type, no shared state)
- **Key**: `Send + Sync` (copy type, atomic generation)  
- **ChangeNotifier**: `Send + Sync` with internal synchronization via `Arc`
- **AtomicElementFlags**: Lock-free atomic operations safe across threads
- **Diagnostics**: Immutable data structures safe to share

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
│ flui-foundation │  ← **Foundation types** (this crate)
├─────────────────┤
│  flui_types     │  ← Basic geometry and math
└─────────────────┘
```

## Examples

See the `examples/` directory for complete examples:

- **Basic Usage**: Core types and operations
- **Reactive Programming**: Change notification patterns
- **Diagnostics**: Debug tree construction and formatting
- **Async Integration**: Using async features with tokio

## Contributing

We welcome contributions! Please see [CONTRIBUTING.md](../../CONTRIBUTING.md) for guidelines.

### Development

```bash
# Run tests
cargo test -p flui-foundation

# Run tests with all features
cargo test -p flui-foundation --all-features

# Check documentation
cargo doc -p flui-foundation --open

# Run benchmarks
cargo bench -p flui-foundation
```

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))
- MIT License ([LICENSE-MIT](../../LICENSE-MIT))

at your option.

## Related Crates

- [`flui-types`](../flui_types): Basic geometry and mathematical types
- [`flui-core`](../flui_core): Core FLUI framework with element tree and rendering
- [`flui-widgets`](../flui_widgets): Widget library built on FLUI
- [`flui-app`](../flui_app): Application framework for building FLUI apps

---

**FLUI Foundation** - Building the foundation for fast, reliable UI frameworks in Rust.