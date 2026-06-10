# FLUI Foundation

[![Crates.io](https://img.shields.io/crates/v/flui-foundation)](https://crates.io/crates/flui-foundation)
[![Documentation](https://docs.rs/flui-foundation/badge.svg)](https://docs.rs/flui-foundation)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](https://github.com/flui-org/flui)

**Foundation types and utilities for the FLUI framework ecosystem.**

FLUI Foundation provides fundamental building blocks used throughout the FLUI UI framework. It contains minimal-dependency types for element identification, change notification, diagnostics, and other core abstractions.

## Features

- **Tree IDs**: Type-safe `Id<T>` with wgpu-style marker traits for all tree levels
- **Keys**: `Key`, `ValueKey`, `UniqueKey` for widget identity (GlobalKey/ObjectKey in flui-view)
- **Change Notification**: Observable patterns for reactive UI updates
- **Observer Lists**: Efficient `ObserverList` with O(1) add/remove by ID
- **Diagnostics**: Rich debugging and introspection utilities
- **Error Handling**: Standardized `FoundationError` with context chaining
- **Callbacks**: Type-safe callback aliases (`VoidCallback`, `ValueChanged`, etc.)
- **Platform Detection**: re-exported from `flui-types` — see `flui_types::platform::TargetPlatform`
- **WASM Support**: `WasmNotSendSync` trait for web compatibility
- **Thread Safety**: All types designed for multi-threaded contexts

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

// Create a generational element-tree key (1-based ctor → 0-based slab index)
let element_id = ElementId::new(1); // .index() == 0, .generation() == 1

// Observable values for reactive UI
let notifier = ChangeNotifier::new();
let listener_id = notifier.add_listener(Arc::new(|| {
    println!("Value changed!");
}));

// Notify listeners of changes
notifier.notify_listeners();
```

## Core Types

### Type-Safe ID System (wgpu-style)

IDs use marker traits for type safety, similar to wgpu's resource ID system:

```rust
use flui_foundation::{
    Id, RawId, Marker, Identifier, TreeId,
    ViewId, ElementId, RenderId, LayerId, SemanticsId
};

// The `Id<T: Marker>` family (ViewId/RenderId/LayerId/SemanticsId) are
// `NonZeroUsize`-backed indices into a Slab. They implement `Identifier`.
let view_id = ViewId::zip(0);      // index 0 → ID 1
let render_id = RenderId::zip(2);   // index 2 → ID 3

// Extract index from an Id<T> (for Slab access)
let index = render_id.unzip(); // ID 3 → index 2

// `ElementId` is a DISTINCT generational arena key: it packs a 32-bit slab
// index + a 32-bit non-zero generation into a `NonZeroU64`. It does NOT
// implement `Identifier` (no bare `.get()` that would strip the generation);
// use `.index()` / `.generation()` instead.
let element_id = ElementId::new(1);           // 1-based ctor → index 0, generation 1
assert_eq!(element_id.index(), 0);
let explicit = ElementId::new_gen(5, std::num::NonZeroU32::new(2).unwrap());
assert_eq!((explicit.index(), explicit.generation().get()), (5, 2));

// All IDs keep their niche: Option<Id> is the same size as Id.
assert_eq!(std::mem::size_of::<Option<ElementId>>(), std::mem::size_of::<ElementId>());

// Generic operations over the index family use the `Identifier` trait;
// generic operations over any tree id (including `ElementId`) use `TreeId`.
fn process<I: Identifier>(id: I) {
    let index = id.get();
    println!("Processing index: {}", index);
}
fn any_tree_id<I: TreeId>(id: I) {
    println!("Tree id: {id}"); // Display/Eq/Hash, but no bare index accessor
}
```

#### Available ID Types

| Category | Types |
|----------|-------|
| **Core Tree** | `ViewId`, `ElementId`, `RenderId`, `LayerId`, `SemanticsId` |
| **Animation** | `AnimationId`, `FrameCallbackId` |
| **Input** | `PointerId`, `GestureId`, `KeyId`, `MotionEventId` |
| **Platform** | `PlatformViewId`, `TextureId`, `EmbedderId`, `DeviceId` |
| **Focus/Groups** | `FocusId`, `GroupId`, `LocationId` |
| **Navigation** | `RouteId`, `RestorationScopeId` |
| **Observers** | `ListenerId`, `ObserverId` |
| **Debug** | `DiagnosticsId`, `ProductId`, `VendorId` |

### Keys for Widget Identity

```rust
use flui_foundation::{Key, ValueKey, UniqueKey};

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

// Unique keys (each instance is unique)
let unique1 = UniqueKey::new();
let unique2 = UniqueKey::new();
assert_ne!(unique1, unique2);
```

> **Note**: `GlobalKey` and `ObjectKey` are in `flui-view` (widgets layer), matching Flutter's architecture.

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
use flui_foundation::ObserverList;

// O(1) add/remove by ID, slot reuse, NOT thread-safe — wrap in your
// own synchronization primitive (e.g. `parking_lot::RwLock`) for
// concurrent access.
let mut observers: ObserverList<i32> = ObserverList::new();
let id = observers.add(42);
observers.remove(id);
```

### WASM Compatibility

```rust
use flui_foundation::{WasmNotSendSync, WasmNotSend};

// WasmNotSendSync: Send + Sync on native, empty on WASM
// This allows IDs and markers to work on both platforms

fn use_in_thread<T: WasmNotSendSync>(value: T) {
    // Works on native (requires Send + Sync)
    // Works on WASM (no thread requirements)
}
```

### Platform Detection

Platform detection lives in the lower-layer `flui-types` crate. Import it
from there:

```rust
use flui_types::platform::TargetPlatform;

let platform = TargetPlatform::current();

if platform.is_desktop() {
    // desktop branch
} else if platform.is_mobile() {
    // mobile branch
}
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
```

### Error Handling

```rust
use flui_foundation::{FoundationError, Result};

fn example() -> Result<i32> {
    // Create specific errors
    Err(FoundationError::invalid_id(0, "ID cannot be zero"))
}

// Check error properties
let err = FoundationError::listener_error("add", "limit reached");
assert!(err.is_recoverable());
assert_eq!(err.category(), "listener");
```

## ID System Design

The ID system follows wgpu's pattern for type-safe resource identification:

```rust
// RawId: The underlying NonZeroUsize value
pub struct RawId(NonZeroUsize);

// Marker trait: Discriminates ID types (zero-sized)
pub trait Marker: 'static + WasmNotSendSync + Debug {}

// Id<T>: Generic typed ID
pub struct Id<T: Marker>(RawId, PhantomData<T>);

// Type aliases for each tree level
pub type ElementId = Id<markers::Element>;
pub type RenderId = Id<markers::Render>;
// ... etc

// Identifier trait for generic operations
pub trait Identifier {
    fn get(self) -> Index;
    fn zip(index: Index) -> Self;
    fn try_zip(index: Index) -> Option<Self>;
}
```

### Index Offset Convention

**CRITICAL**: for the `Id<T>` family (`ViewId`/`RenderId`/`LayerId`/`SemanticsId`)
the Slab uses 0-based indices while IDs use 1-based `NonZeroUsize` values:

```rust
// Inserting into Slab:
let slab_index = slab.insert(node);      // 0, 1, 2, ...
let id = RenderId::zip(slab_index);       // 1, 2, 3, ... (index + 1)

// Accessing from Slab:
let index = id.unzip();                   // ID → index (value - 1)
let node = slab.get(index);
```

`ElementId` does **not** follow this 1-based `zip`/`unzip` convention — it is a
generational key. Mint it with `ElementId::new_gen(slab_index, generation)` and
read the slab slot with `.index()`; the generation guards against stale ids
addressing a reused slot.

## Architecture

Foundation sits at the base of the FLUI architecture:

```
┌─────────────────┐
│   flui_app      │  ← Application framework
├─────────────────┤
│  flui_widgets   │  ← Widget library  
├─────────────────┤
│   flui-view     │  ← View/Element trees (GlobalKey, ObjectKey here)
├─────────────────┤
│ flui-foundation │  ← Foundation types (this crate)
├─────────────────┤
│  flui_types     │  ← Basic geometry and math
└─────────────────┘
```

See [ARCHITECTURE.md](./ARCHITECTURE.md) for complete Flutter foundation types reference.

## Performance

Foundation types are optimized for common UI patterns:

- **IDs**: `NonZeroUsize` for niche optimization (`Option<Id>` same size as `Id`)
- **Keys**: Atomic counter for O(1) generation, FNV-1a hash for string keys
- **Observer Lists**: O(1) add/remove with slot reuse
- **Change Notifiers**: Efficient listener storage with `parking_lot` locks

## Thread Safety

All foundation types are designed for multi-threaded use:

- **IDs**: `Send + Sync` (Copy types via `WasmNotSendSync`)
- **Keys**: `Send + Sync` (Copy types with atomic generation)
- **ChangeNotifier**: `Send + Sync` with `parking_lot::Mutex`
- **ObserverList**: Not thread-safe by itself; wrap in your own
  synchronization primitive for concurrent access

## Feature Flags

- `serde`: Enables serialization support for foundation types
- `full`: Enables all optional features

```toml
[dependencies]
flui-foundation = { version = "0.1", features = ["serde"] }
```

## Development

```bash
# Run tests
cargo test -p flui-foundation

# Run tests with all features
cargo test -p flui-foundation --all-features

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
- [`flui-view`](../flui-view): View/Element trees, GlobalKey, ObjectKey
- [`flui_rendering`](../flui_rendering): Render tree and layout
- [`flui_app`](../flui_app): Application framework
