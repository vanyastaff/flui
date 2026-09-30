# FLUI Foundation

**Foundation types and utilities for the FLUI framework ecosystem.**

FLUI Foundation provides fundamental building blocks used throughout the FLUI UI framework. It contains minimal-dependency types for element identification, change notification, diagnostics, and other core abstractions.

## Features

- **Tree IDs**: Type-safe `Id<T>` with wgpu-style marker traits, and generational keys (`ElementId`, `RenderId`, `RealmId`) for slots that are reused
- **Keys**: `Key`, `ValueKey`, `UniqueKey` for widget identity (GlobalKey/ObjectKey in flui-view)
- **Change Notification**: `ChangeNotifier`, `ValueNotifier`, the generic `Notifier<Arg>` and `ListenerRegistry`
- **Diagnostics**: `DiagnosticsNode` trees and the `Diagnosticable` trait
- **Callbacks**: Type-safe callback aliases (`VoidCallback`, `ValueChanged`, etc.)
- **Geometry**: `geometry` module — `Point`, `Offset`, `Size`, `Rect`, `RRect`, `EdgeInsets`, `Matrix4`, device-grid types and snapping (ADR-0098)
- **Clocks and epochs**: `MonotonicClock` (`SystemClock`, `ManualClock`), `FrameEpoch`, `SurfaceGeneration`, `FrameStamp`
- **WASM Support**: `WasmNotSendSync` trait for web compatibility

## Quick Start

Add FLUI Foundation to your `Cargo.toml`:

```toml
[dependencies]
flui-foundation = { git = "https://github.com/vanyastaff/flui" }
```

This crate isn't published to crates.io; depend on it via git or path — see
the [flui facade's README](../../README.md).

Basic usage:

```rust
use flui_foundation::{ElementId, Key, ChangeNotifier, Listenable};
use std::sync::Arc;

// Create a generational element-tree key (1-based ctor → 0-based slab index)
let element_id = ElementId::new(1); // .index() == 0, .generation() == 1

// Observable values for reactive UI
let notifier = ChangeNotifier::new();
let _listener_id = notifier.add_listener(Arc::new(|| {
    println!("Value changed!");
}));

// Notify listeners of changes
notifier.notify_listeners();
```

## Core Types

### Type-Safe ID System (wgpu-style)

IDs use marker traits for type safety, similar to wgpu's resource ID system:

```rust
use flui_foundation::{ElementId, Identifier, LayerId, RenderId, TreeId, ViewId};

// The plain `Id<T: Marker>` family (ViewId, LayerId, SemanticsId, ListenerId,
// ObserverId, FrameCallbackId, FrameId, TaskId, TickerId) are
// `NonZeroUsize`-backed indices into a Slab. They implement `Identifier`.
// `zip` takes the public (1-based) value; the slab offset is the caller's.
let slab_index = 2;
let layer_id = LayerId::zip(slab_index + 1);  // slot 2 → ID 3
assert_eq!(layer_id.unzip() - 1, slab_index); // ID 3 → slot 2
let first_view = ViewId::zip(1);              // slot 0 → ID 1

// Generational keys (`ElementId`, and `GenId<T>` aliases `RenderId`,
// `RealmId`, `DataTransferId`) pack a 32-bit slab index and a non-zero 32-bit
// generation into a `NonZeroU64`. They do NOT implement `Identifier` (no bare
// `.get()` that would strip the generation); use `.index()` / `.generation()`.
let element_id = ElementId::new(1);           // 1-based ctor → index 0, generation 1
assert_eq!(element_id.index(), 0);
let render_id = RenderId::new_gen(5, std::num::NonZeroU32::new(2).expect("non-zero"));
assert_eq!((render_id.index(), render_id.generation().get()), (5, 2));

// All IDs keep their niche: Option<Id> is the same size as Id.
assert_eq!(std::mem::size_of::<Option<ElementId>>(), std::mem::size_of::<ElementId>());

// Generic operations over the index family use the `Identifier` trait;
// generic operations over any tree id (generational ones included) use `TreeId`.
fn process<I: Identifier>(id: I) {
    let index = id.get();
    println!("Processing index: {}", index);
}
fn any_tree_id<I: TreeId>(id: I) {
    println!("Tree id: {id}"); // Display/Eq/Hash, but no bare index accessor
}
process(first_view);
any_tree_id(render_id);
```

#### Available ID Types

| Kind | Types |
|------|-------|
| **Plain `Id<T>`** (1-based slab index) | `ViewId`, `LayerId`, `SemanticsId`, `ListenerId`, `ObserverId`, `FrameCallbackId`, `FrameId`, `TaskId`, `TickerId` |
| **Generational** (slab index + generation) | `ElementId`, `RenderId`, `RealmId`, `DataTransferId`, `PresentationId` |
| **Composite** | `PresentationAddress` (a `RealmId` plus a `PresentationId`) |

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

### WASM Compatibility

```rust
use flui_foundation::WasmNotSendSync;

// WasmNotSendSync: Send + Sync on native, empty on WASM
// This allows IDs and markers to work on both platforms

fn use_in_thread<T: WasmNotSendSync>(value: T) {
    // Works on native (requires Send + Sync)
    // Works on WASM (no thread requirements)
}
```

### Geometry

`flui_foundation::geometry` holds the geometry values every layer shares. A
logical length is a plain `f64` (`10.0` is ten logical pixels); `Point<i32>`,
`Size<i32>` and `Rect<i32>` are the device-pixel grid (`DevicePoint`,
`DeviceSize`, `DeviceRect`), and `snap`, `snap_edges`, `cover` and
`device_rect_covering` round logical values to that grid. See
[ADR-0098](../../docs/adr/ADR-0098-owned-f64-geometry-values.md).

```rust
use flui_foundation::geometry::{Point, Rect, Size};

let rect = Rect::from_ltwh(10.0, 20.0, 100.0, 50.0);
assert!(rect.contains(Point::new(15.0, 25.0)));
assert_eq!(rect.size(), Size::new(100.0, 50.0));
```

### Platform Detection

`TargetPlatform` lives in `flui-platform-api`:

```rust
use flui_platform_api::TargetPlatform;

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

## ID System Design

The ID system follows wgpu's pattern for type-safe resource identification:

```rust
// RawId: The underlying NonZeroUsize value
pub struct RawId(NonZeroUsize);

// Marker trait: Discriminates ID types (zero-sized)
pub trait Marker: 'static + WasmNotSendSync + Debug {}

// Id<T>: Generic typed ID
pub struct Id<T: Marker>(RawId, PhantomData<T>);

// Type aliases for each index-keyed tree
pub type ViewId = Id<markers::View>;
pub type LayerId = Id<markers::Layer>;
// ... etc

// Identifier trait for generic operations
pub trait Identifier {
    fn get(self) -> Index;
    fn zip(index: Index) -> Self;
    fn try_zip(index: Index) -> Option<Self>;
}
```

### Index Offset Convention

**CRITICAL**: for the `Id<T>` family (`ViewId`/`LayerId`/`SemanticsId`, …)
the Slab uses 0-based indices while IDs use 1-based `NonZeroUsize` values:

```rust
// Inserting into Slab:
let slab_index = slab.insert(node);       // 0, 1, 2, ...
let id = LayerId::zip(slab_index + 1);     // 1, 2, 3, ...

// Accessing from Slab:
let index = id.unzip() - 1;                // ID → slab index
let node = slab.get(index);
```

The generational keys (`ElementId`, `RenderId`, `RealmId`, …) do **not**
follow this 1-based `zip`/`unzip` convention. Mint one with
`new_gen(slab_index, generation)` and read the slab slot with `.index()`; the
generation guards against stale ids addressing a reused slot.

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
│ flui-foundation │  ← Foundation types and geometry (this crate)
└─────────────────┘
```

See [ARCHITECTURE.md](./ARCHITECTURE.md) for complete Flutter foundation types reference.

## Performance

Foundation types are optimized for common UI patterns:

- **IDs**: `NonZeroUsize` for niche optimization (`Option<Id>` same size as `Id`)
- **Keys**: Atomic counter for O(1) generation, FNV-1a hash for string keys
- **Change Notifiers**: listener callbacks snapshotted into a `SmallVec` before firing

## Thread Safety

All foundation types are designed for multi-threaded use:

- **IDs**: `Send + Sync` (Copy types via `WasmNotSendSync`)
- **Keys**: `Send + Sync` (Copy types with atomic generation)
- **ChangeNotifier**: `Send + Sync`; clones share one listener list

## Feature Flags

- `serde`: Enables serialization support for foundation types

```toml
[dependencies]
flui-foundation = { git = "https://github.com/vanyastaff/flui", features = ["serde"] }
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
- MIT License ([LICENSE](../../LICENSE))

at your option.

## Related Crates

- [`flui-painting`](../flui-painting): Paint, colour, border and text-style values
- [`flui-platform-api`](../flui-platform-api): Platform contracts, `TargetPlatform`
- [`flui-view`](../flui-view): View/Element trees, GlobalKey, ObjectKey
- [`flui_rendering`](../flui-rendering): Render tree and layout
- [`flui_app`](../flui-app): Application framework
